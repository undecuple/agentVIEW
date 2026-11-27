//! Trusted workspace only. Provider identity comes from saved native metadata.
use agent_app_sessions::{follow, Provider, ReadResult, SessionInfo, SessionManager};
use serde::Serialize;
use std::{io::Read, path::PathBuf, process::{Command, Stdio}, sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}}, time::{Duration,Instant}};

#[derive(Default)]
pub struct LiveState {
    manager: Arc<Mutex<Option<SessionManager>>>,
    usage: Arc<Mutex<Option<agent_app_sessions::UsageView>>>,
    cancel: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
}
#[derive(Serialize)]
pub struct ToolInfo { provider: Provider, executable: String, version: String, remembered: bool }

async fn manager<T:Send+'static>(window:tauri::WebviewWindow,state:&LiveState,f:impl FnOnce(&mut SessionManager)->Result<T,String>+Send+'static)->Result<T,String>{
    super::authorized(&window)?;
    let shared=Arc::clone(&state.manager);let cancel=Arc::clone(&state.cancel);let closing=Arc::clone(&state.closing);
    tauri::async_runtime::spawn_blocking(move||{
        let mut guard=shared.lock().map_err(|_| "Session manager lock failed")?;
        if closing.load(Ordering::Acquire){return Err("Workspace is closing; no session action was performed".into());}
        if guard.is_none(){let mut m=SessionManager::open_unconfigured(&super::app_data_dir()?.join("sessions"))?;m.set_cancel_flag(Arc::clone(&cancel));*guard=Some(m);}
        let m=guard.as_mut().ok_or("Session manager unavailable")?;
        let result=f(m);
        if closing.load(Ordering::Acquire){return Err("Workspace close requested; no terminal input was replayed".into());}
        result
    }).await.map_err(|_| "Session operation failed")?
}
/// Usage reads run on their own SQLite connection and lock, so a long refresh never delays terminal reads.
/// The main manager is opened first (it owns migrations and the instance lease); the view only attaches to its store.
async fn usage_view<T:Send+'static>(window:tauri::WebviewWindow,state:&LiveState,f:impl FnOnce(&mut agent_app_sessions::UsageView)->Result<T,String>+Send+'static)->Result<T,String>{
    super::authorized(&window)?;
    manager(window.clone(),state,|_|Ok(())).await?;
    let shared=Arc::clone(&state.usage);let cancel=Arc::clone(&state.cancel);let closing=Arc::clone(&state.closing);
    tauri::async_runtime::spawn_blocking(move||{
        let mut guard=shared.lock().map_err(|_| "Usage reader lock failed")?;
        if closing.load(Ordering::Acquire){return Err("Workspace is closing; no usage action was performed".into());}
        if guard.is_none(){let mut u=SessionManager::open_usage_view(&super::app_data_dir()?.join("sessions"))?;u.set_cancel_flag(Arc::clone(&cancel));*guard=Some(u);}
        f(guard.as_mut().ok_or("Usage reader unavailable")?)
    }).await.map_err(|_| "Usage operation failed")?
}
fn executable(provider:Provider)->Result<PathBuf,String>{
    #[cfg(feature="workspace-e2e")]
    if let Some(path)=std::env::var_os(match provider{Provider::Claude=>"AGENT_APP_TEST_CLAUDE",Provider::Codex=>"AGENT_APP_TEST_CODEX"}){return std::fs::canonicalize(path).map_err(|_| "Test CLI missing".into());}
    use std::os::unix::fs::PermissionsExt;
    let name=match provider{Provider::Claude=>"claude",Provider::Codex=>"codex"};
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()){
        if !dir.is_absolute(){continue;}
        let path=dir.join(name);
        if let Ok(meta)=std::fs::metadata(&path){if meta.is_file()&&meta.permissions().mode()&0o111!=0{return std::fs::canonicalize(path).map_err(|_| "Cannot resolve provider CLI".into());}}
    }
    Err(format!("{name} was not found on PATH. Install it and use its native login flow."))
}
fn capture_cli(path:&std::path::Path,args:&[&str],limit:u64)->Result<String,String>{
    let mut child=Command::new(path).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "Cannot inspect provider CLI")?;
    let pipe=child.stdout.take().ok_or("Cannot inspect CLI output")?;
    let(sender,receiver)=std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move||{let mut out=String::new();let result=pipe.take(limit).read_to_string(&mut out);let _=sender.send((result,out));});
    let end=Instant::now()+Duration::from_secs(3);
    loop{match child.try_wait(){
        Ok(Some(status))if status.success()=>break,
        Ok(Some(_))|Err(_)=>{let _=child.kill();let _=child.wait();return Err("Provider CLI inspection failed".into());},
        _ if Instant::now()>=end=>{let _=child.kill();let _=child.wait();return Err("Provider CLI inspection timed out".into());},
        _=>std::thread::sleep(Duration::from_millis(20)),
    }}
    let(result,out)=receiver.recv_timeout(Duration::from_millis(500)).map_err(|_| "CLI output did not close")?;
    result.map_err(|_| "Invalid CLI inspection output")?;Ok(out)
}
#[tauri::command]
pub async fn live_probe(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,provider:String)->Result<ToolInfo,String>{
    super::authorized(&window)?;let provider=Provider::parse(&provider)?;
    let probed=manager(window.clone(),&state,move|m|{
        let path=executable(provider)?;let version=capture_cli(&path,&["--version"],4096)?.trim().to_string();
        if version.is_empty()||version.len()>200||version.chars().any(char::is_control){return Err("Unrecognized CLI version output".into());}
        let help=capture_cli(&path,&["--help"],65536)?;
        match provider{
            Provider::Claude if !help.contains("--session-id")||!help.contains("--resume")=>return Err("Claude CLI lacks required session flags".into()),
            Provider::Codex=>{if !help.contains("app-server")||!help.contains("resume")||!capture_cli(&path,&["resume","--help"],65536)?.contains("--no-daemon"){return Err("Codex CLI lacks required native resume/app-server capabilities".into());}},
            _=>(),
        }
        m.configure_provider(provider,&path,&version)?;m.set_capabilities(provider,help.contains("--name"),help.contains("--remote-control"));
        let identity=agent_app_sessions::tool_identity(&path)?;
        Ok((ToolInfo{provider,executable:path.to_string_lossy().into_owned(),version,remembered:false},path,identity))
    }).await?;
    let (info,path,identity)=probed;
    // Remember what the person reviewed. The file is hashed outside the manager lock and kept only if its identity did
    // not change while hashing; failing to remember never fails the inspection (the next start just asks again).
    let id2=identity.clone();
    let sha=tauri::async_runtime::spawn_blocking(move||{
        let sha=agent_app_sessions::file_sha256(&path)?;
        if agent_app_sessions::tool_identity(&path)?!=id2{return Err("The CLI changed while it was being verified".to_string());}
        Ok::<_,String>(sha)
    }).await.map_err(|_| "Verification failed".to_string()).and_then(|r|r);
    if let Ok(sha)=sha{let _=manager(window,&state,move|m|m.remember_tool(provider,&identity,&sha)).await;}
    Ok(info)
}
/// Restores remembered inspections whose file is unchanged (cheap checks first, then SHA-256 off the manager lock,
/// then every check again under the lock). Returns the providers whose file changed, with the reason.
/// Hash results for this app run, keyed by (path, identity), at most 16. A failure is never rehashed while the file keeps
/// the same identity; a success is reused when restoring (navigation), but the check right before a spawn always rehashes.
static VERIFIED:std::sync::Mutex<Vec<(PathBuf,String,Result<String,String>)>>=std::sync::Mutex::new(Vec::new());
fn verify_cached(path:&std::path::Path,reuse_success:bool)->Result<String,String>{
    let id_now=agent_app_sessions::tool_identity(path)?;
    if let Some((_,_,r))=VERIFIED.lock().ok().and_then(|v|v.iter().find(|(p,i,_)|p==path&&*i==id_now).cloned()){
        if r.is_err()||reuse_success{return r;}
    }
    let r=agent_app_sessions::file_sha256(path);
    if agent_app_sessions::tool_identity(path).ok().as_deref()!=Some(id_now.as_str()){return Err("the file changed while it was being verified".into());}
    if let Ok(mut v)=VERIFIED.lock(){v.retain(|(p,i,_)|!(p==path&&*i==id_now));v.push((path.to_path_buf(),id_now,r.clone()));if v.len()>16{drop(v.remove(0));}}
    r
}
async fn restore_tools(window:&tauri::WebviewWindow,state:&LiveState)->Result<Vec<agent_app_sessions::ToolStatus>,String>{
    let pending=manager(window.clone(),state,|m|m.remembered_pending()).await?;
    let mut changed=Vec::new();
    for r in pending{
        let rc=r.clone();
        let sha=tauri::async_runtime::spawn_blocking(move||{agent_app_sessions::quick_check(&rc)?;verify_cached(&rc.executable,true)})
            .await.map_err(|_| "Verification failed".to_string()).and_then(|x|x);
        let outcome=match sha{Ok(sha)=>{let r2=r.clone();manager(window.clone(),state,move|m|Ok(m.adopt_remembered(&r2,&sha))).await?},Err(e)=>Err(e)};
        if let Err(reason)=outcome{changed.push(agent_app_sessions::ToolStatus{provider:r.provider,executable:r.executable.to_string_lossy().into_owned(),version:r.version.clone(),remembered:false,changed:Some(reason)});}
    }
    Ok(changed)
}
/// Forget the remembered inspection for one provider (the person's choice on the Live page). This run stops using a
/// restored copy too, so the next start asks for Inspect.
#[tauri::command(async)]
pub async fn live_forget_tool(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,provider:String)->Result<bool,String>{
    super::authorized(&window)?;let provider=Provider::parse(&provider)?;
    manager(window,&state,move|m|m.forget_tool(provider)).await
}
/// The CLIs ready for this run: inspected now, or restored from the person's earlier inspection when the same file is
/// unchanged. Never runs a CLI. Changed ones are listed with the reason so the page can ask for a new inspection.
#[tauri::command(async)]
pub async fn live_tools(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<Vec<agent_app_sessions::ToolStatus>,String>{
    super::authorized(&window)?;
    let changed=restore_tools(&window,&state).await?;
    let mut list=manager(window,&state,|m|Ok(m.tool_statuses())).await?;
    list.extend(changed.into_iter().filter(|c|!list.iter().any(|t|t.provider==c.provider)).collect::<Vec<_>>());
    Ok(list)
}
#[tauri::command]
pub async fn live_set_remote(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,enabled:bool)->Result<SessionInfo,String>{manager(window,&state,move|m|m.set_remote_control(&id,enabled)).await}
#[tauri::command]
pub async fn live_rename(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,label:String)->Result<SessionInfo,String>{manager(window,&state,move|m|m.rename(&id,&label)).await}
#[tauri::command]
pub async fn live_remove(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String)->Result<(),String>{manager(window,&state,move|m|m.remove(&id)).await}
#[tauri::command]
pub async fn live_activity(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<Vec<agent_app_sessions::Activity>,String>{manager(window,&state,|m|m.activity(&usage_roots())).await}
#[tauri::command]
pub async fn live_stop_all(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<usize,String>{manager(window,&state,|m|m.stop_all()).await}
#[tauri::command]
pub async fn live_snapshot(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<Vec<SessionInfo>,String>{manager(window,&state,|m|m.list()).await}
// Roots are native configuration paths, never arbitrary paths supplied by web content.
// Resolving a root reads no credentials, session content or provider configuration.
fn usage_roots()->agent_app_sessions::UsageRoots {
    #[cfg(feature="workspace-e2e")]
    if let Some(root)=std::env::var_os("AGENT_APP_TEST_PROJECT_ROOT") {
        let root=PathBuf::from(root);
        return agent_app_sessions::UsageRoots {claude:Some(root.join("claude-home")),codex:Some(root.join("codex-home"))};
    }
    let home=std::env::var_os("HOME").map(PathBuf::from);
    let root=|key:&str,default:&str| std::env::var_os(key).map(PathBuf::from)
        .or_else(||home.as_ref().map(|h|h.join(default))).filter(|p|p.is_absolute());
    agent_app_sessions::UsageRoots {claude:root("CLAUDE_CONFIG_DIR",".claude"),codex:root("CODEX_HOME",".codex")}
}
/// Which conversation each registered Claude session really continues. Reads only the project's transcript folder
/// (names, times, and the cwd/sessionId fields of at most 64 KiB per candidate) at most every 30 s per session, outside
/// the session manager lock. A running session whose folder is not shared and that wrote exactly one newer conversation
/// since it started is switched to it, after the folder is read again right then; anything else is only reported.
/// When a cap is hit (5000 entries, 16 opened files) nothing is switched automatically. Codex sessions are not listed.
#[derive(Serialize)]
pub struct FollowStatus { app_session_id:String, running:bool, saved:String, decision:follow::Decision, newer:Option<follow::Conversation>, candidates:usize, capped:bool, reason:String, followed:Option<follow::FollowEvent> }
type FollowCache=std::collections::HashMap<String,(Instant,String,bool,follow::Verdict)>;
static FOLLOW_CACHE:Mutex<Option<FollowCache>>=Mutex::new(None);
fn follow_every()->Duration{
    #[cfg(feature="workspace-e2e")]
    if std::env::var_os("AGENT_APP_TEST_LIVE").is_some(){return Duration::from_millis(200);}
    Duration::from_secs(30)
}
#[tauri::command]
pub async fn live_follow_check(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<Vec<FollowStatus>,String>{
    let targets=manager(window.clone(),&state,|m|m.follow_targets()).await?;
    let root=usage_roots().claude;
    let checked=tauri::async_runtime::spawn_blocking(move||{
        let mut cache=FOLLOW_CACHE.lock().map_err(|_| "Follow cache unavailable".to_string())?;let cache=cache.get_or_insert_with(Default::default);
        cache.retain(|id,_|targets.iter().any(|t|&t.app_session_id==id));
        Ok::<_,String>(targets.into_iter().map(|t|{
            let running=t.started.is_some();
            let fresh=cache.get(&t.app_session_id).filter(|(at,saved,run,_)|at.elapsed()<follow_every()&&*saved==t.saved&&*run==running).map(|c|c.3.clone());
            let v=fresh.unwrap_or_else(||{let v=follow::check(root.as_deref(),&t);cache.insert(t.app_session_id.clone(),(Instant::now(),t.saved.clone(),running,v.clone()));v});
            (t,v)
        }).collect::<Vec<_>>())
    }).await.map_err(|_| "Follow check failed")??;
    let mut out=Vec::new();
    for (t,mut v) in checked{
        let mut saved=t.saved.clone();
        if v.decision==follow::Decision::Follow{
            if let Some(new)=v.newer.clone(){
                // Never act on a cached verdict: copy the session again, re-read the folder now (off the lock), then switch.
                let fresh=manager(window.clone(),&state,|m|m.follow_targets()).await?.into_iter().find(|x|x.app_session_id==t.app_session_id&&x.saved==t.saved);
                let root=usage_roots().claude;let nid0=new.id.clone();
                let confirmed=match fresh{Some(f)=>tauri::async_runtime::spawn_blocking(move||follow::confirm(root.as_deref(),&f,&nid0)).await.map_err(|_| "Follow check failed".to_string()).and_then(|r|r),None=>Err("The session changed; nothing was switched".into())};
                if let Ok(mut c)=FOLLOW_CACHE.lock(){if let Some(c)=c.as_mut(){c.remove(&t.app_session_id);}}
                let (id,old,nid)=(t.app_session_id.clone(),t.saved.clone(),new.id.clone());
                let result=match confirmed{Ok(())=>manager(window.clone(),&state,move|m|m.adopt_conversation(&id,&old,&nid,"auto")).await,Err(e)=>Err(e)};
                match result{
                    Ok(info)=>{saved=info.provider_session_id.unwrap_or(saved);v.decision=follow::Decision::Current;v.reason="switched to the conversation this session continues".into();v.candidates.clear();v.newer=None;
                        if let Ok(mut c)=FOLLOW_CACHE.lock(){if let Some(c)=c.as_mut(){c.remove(&t.app_session_id);}}},
                    Err(e)=>{v.decision=follow::Decision::Ambiguous;v.reason=e;},
                }
            }
        }
        out.push(FollowStatus{app_session_id:t.app_session_id,running:t.started.is_some(),saved,decision:v.decision,newer:v.newer,candidates:v.candidates.len(),capped:v.capped,reason:v.reason,followed:None});
    }
    let history=manager(window,&state,|m|m.follow_history()).await?;
    for s in &mut out{s.followed=history.iter().find(|(id,e)|*id==s.app_session_id&&e.new_id==s.saved).map(|(_,e)|e.clone());}
    Ok(out)
}
/// The person picked a newer conversation for a session ("Use it" or "Resume latest"). It must still be a valid newer
/// conversation for that folder right now; the saved ID is switched and recorded. Nothing is started or resent.
/// With `undo`, the session goes back to the conversation saved before its latest follow (`conversation` must name it).
#[tauri::command]
pub async fn live_use_conversation(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,expected:String,conversation:String,undo:Option<bool>)->Result<SessionInfo,String>{
    if undo==Some(true){
        let sid=id.clone();
        let info=manager(window,&state,move|m|m.undo_follow(&id,&expected,&conversation)).await?;
        if let Ok(mut c)=FOLLOW_CACHE.lock(){if let Some(c)=c.as_mut(){c.remove(&sid);}}
        return Ok(info);
    }
    let targets=manager(window.clone(),&state,|m|m.follow_targets()).await?;
    let t=targets.into_iter().find(|t|t.app_session_id==id).ok_or("That session has no saved Claude conversation")?;
    if t.saved!=expected{return Err("The saved conversation changed meanwhile; check again".into());}
    let root=usage_roots().claude;
    let v=tauri::async_runtime::spawn_blocking(move||follow::check(root.as_deref(),&t)).await.map_err(|_| "Follow check failed")?;
    if !v.candidates.iter().any(|c|c.id==conversation){return Err("That conversation is not a newer conversation of this project folder".into());}
    let sid=id.clone();
    let info=manager(window,&state,move|m|m.adopt_conversation(&id,&expected,&conversation,"chosen")).await?;
    if let Ok(mut c)=FOLLOW_CACHE.lock(){if let Some(c)=c.as_mut(){c.remove(&sid);}}
    Ok(info)
}
#[tauri::command]
pub async fn usage_snapshot(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<agent_app_sessions::UsageSnapshot,String>{
    usage_view(window,&state,|u|u.usage_snapshot()).await
}
#[tauri::command]
pub async fn usage_refresh(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>)->Result<agent_app_sessions::UsageSnapshot,String>{
    usage_view(window,&state,|u|u.refresh_usage(&usage_roots())).await
}
#[tauri::command]
pub async fn live_create(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,provider:String,label:String,project_dir:String)->Result<SessionInfo,String>{
    super::authorized(&window)?;let provider=Provider::parse(&provider)?;let path=PathBuf::from(project_dir);
    if !path.is_absolute(){return Err("Choose an absolute project directory".into());}
    manager(window,&state,move|m|m.create_for(provider,&label,&path)).await
}
static STARTED:std::sync::Mutex<Vec<(String,i64)>>=std::sync::Mutex::new(Vec::new());
fn now_s()->i64{std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs() as i64).unwrap_or(0)}
#[tauri::command]
pub async fn live_start(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String)->Result<SessionInfo,String>{
    // Fails closed: waits for the workspace lock; a poisoned lock or unreadable settings mean the strict check (setting off).
    let follow={use tauri::Manager;let ws=window.state::<super::WorkspaceState>();ws.0.as_ref().ok().and_then(|s|s.lock().ok().and_then(|s|s.settings_snapshot().ok())).is_some_and(|s|s.settings.follow_cli_updates)};
    // A remembered inspection is restored first when the file is unchanged (read-only; no CLI runs here).
    let _=restore_tools(&window,&state).await;
    // A tool restored from a remembered inspection is re-hashed right before it is used (off the manager lock); the
    // start below then re-checks its identity under the lock. A mismatch drops it and asks for a new inspection.
    let sid0=id.clone();
    if let Some(t)=manager(window.clone(),&state,move|m|m.restored_target(&sid0)).await?{
        let path=t.executable.clone();
        let sha=tauri::async_runtime::spawn_blocking(move||verify_cached(&path,false)).await.map_err(|_| "Verification failed".to_string()).and_then(|r|r);
        if sha.as_deref()!=Ok(t.sha256.as_str()){
            let p=t.provider;let _=manager(window.clone(),&state,move|m|{m.drop_restored(p);Ok(())}).await;
            return Err(format!("The remembered CLI changed since your Inspect ({}); inspect it again before starting",sha.err().unwrap_or_else(||"its contents differ".into())));
        }
    }
    let sid=id.clone();let info=manager(window,&state,move|m|m.start_with(&id,follow)).await?;
    if let Ok(mut s)=STARTED.lock(){s.retain(|(i,_)|*i!=sid);s.push((sid,now_s()));}
    Ok(info)
}
/// The project folder of one registered session (for the consent-gated document writes).
pub(crate) async fn session_dir(window:tauri::WebviewWindow,state:&LiveState,id:&str)->Result<String,String>{
    let list=manager(window,state,|m|m.list()).await?;
    list.into_iter().find(|s|s.id==id).map(|s|s.project_dir).ok_or_else(||"That session no longer exists".to_string())
}
/// Changes in the folder of one registered session: files changed now and commits since the app started or resumed it (in this app run; unknown after a restart).
#[tauri::command]
pub async fn session_changes(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,cfg:tauri::State<'_,Arc<agent_app_core::Config>>,id:String)->Result<super::gitview::SessionChanges,String>{
    let list=manager(window,&state,|m|m.list()).await?;
    let dir=list.into_iter().find(|s|s.id==id).ok_or("That session no longer exists")?.project_dir;
    let since=STARTED.lock().ok().and_then(|s|s.iter().find(|(i,_)|*i==id).map(|(_,t)|*t)).map(|t|now_s()-t);
    let cfg=Arc::clone(&cfg);
    tauri::async_runtime::spawn_blocking(move||super::gitview::changes_in(std::path::Path::new(&dir),&cfg,since)).await.map_err(|_|"Could not read the changes".to_string())?
}
#[tauri::command]
pub async fn live_read(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,generation:String,offset:u64)->Result<ReadResult,String>{manager(window,&state,move|m|m.read(&id,&generation,offset)).await}
#[tauri::command]
pub async fn live_write(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,generation:String,data:String)->Result<usize,String>{manager(window,&state,move|m|m.write(&id,&generation,data.as_bytes())).await}
#[tauri::command]
pub async fn live_resize(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,generation:String,cols:u16,rows:u16)->Result<(),String>{manager(window,&state,move|m|m.resize(&id,&generation,cols,rows)).await}
#[tauri::command]
pub async fn live_stop(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,generation:String)->Result<SessionInfo,String>{manager(window,&state,move|m|m.stop(&id,&generation)).await}
#[cfg(feature="workspace-e2e")]
pub fn shutdown(state:&LiveState)->Result<(),String>{
    state.cancel.store(true,Ordering::Release);
    let mut guard=state.manager.lock().map_err(|_| "Session manager lock failed during shutdown")?;
    if let Some(m)=guard.as_mut(){m.shutdown()?;}Ok(())
}
/// Close never blocks the UI waiting for a bootstrap/inspection operation.
pub fn close_workspace(window:&tauri::Window,state:&LiveState)->bool{
    state.closing.store(true,Ordering::Release);state.cancel.store(true,Ordering::Release);
    match state.manager.try_lock(){
        Ok(mut guard)=>{let result=guard.as_mut().map_or(Ok(()),SessionManager::shutdown);if result.is_ok(){true}else{state.closing.store(false,Ordering::Release);state.cancel.store(false,Ordering::Release);let _=window.set_title("agentVIEW · Close failed; inspect session storage");false}},
        Err(std::sync::TryLockError::WouldBlock)=>{
            let _=window.set_title("agentVIEW · Cancelling session startup before closing");
            let shared=Arc::clone(&state.manager);let closing=Arc::clone(&state.closing);let cancel=Arc::clone(&state.cancel);let window=window.clone();
            tauri::async_runtime::spawn_blocking(move||{
                let result=shared.lock().map_err(|_| "Session lock failed".to_string()).and_then(|mut guard|guard.as_mut().map_or(Ok(()),SessionManager::shutdown));
                if result.is_ok(){let _=window.close();}else{closing.store(false,Ordering::Release);cancel.store(false,Ordering::Release);let _=window.set_title("agentVIEW · Unable to close; inspect session storage");}
            });false
        },
        Err(_)=>{let _=window.set_title("agentVIEW · Session lock failed; close prevented");false},
    }
}

/// Sanitized projection happens in Settings; no CLI is probed for this metadata.
pub async fn saved_versions(window:tauri::WebviewWindow,state:&LiveState)->Result<Vec<(String,String)>,String>{
    manager(window,state,|m|Ok(m.list()?.into_iter().map(|s|(match s.provider{Provider::Claude=>"claude",Provider::Codex=>"codex"}.to_string(),s.version)).collect())).await
}

impl LiveState {
    /// Running session count without waiting: None when the manager is busy or the workspace is closing; Some(0) when no session was ever used.
    pub fn peek_running(&self)->Option<usize>{
        if self.closing.load(Ordering::Acquire){return None;}
        let mut guard=self.manager.try_lock().ok()?;
        match guard.as_mut(){Some(m)=>m.running_count().ok(),None=>Some(0)}
    }
    /// Stops every session for good before the process exits (quit control, idle exit). Waits for a busy manager; the cancel flag ends a stalled start.
    pub fn stop_for_exit(&self)->Result<(),String>{
        self.closing.store(true,Ordering::Release);self.cancel.store(true,Ordering::Release);
        let mut guard=self.manager.lock().map_err(|_| "Session manager lock failed")?;
        guard.as_mut().map_or(Ok(()),SessionManager::shutdown)
    }
    /// Activity for the background notifier. Never blocks a terminal action: it returns None when the manager is busy,
    /// not opened yet (no session was ever used in this run) or the workspace is closing.
    pub fn peek_activity(&self)->Option<Vec<agent_app_sessions::Activity>>{
        if self.closing.load(Ordering::Acquire){return None;}
        let mut guard=self.manager.try_lock().ok()?;
        guard.as_mut()?.activity(&usage_roots()).ok()
    }
}
