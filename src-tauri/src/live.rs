//! Trusted workspace only. Provider identity comes from saved native metadata.
use agent_app_sessions::{Provider, ReadResult, SessionInfo, SessionManager};
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
pub struct ToolInfo { provider: Provider, executable: String, version: String }

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
    manager(window,&state,move|m|{
        let path=executable(provider)?;let version=capture_cli(&path,&["--version"],4096)?.trim().to_string();
        if version.is_empty()||version.len()>200||version.chars().any(char::is_control){return Err("Unrecognized CLI version output".into());}
        let help=capture_cli(&path,&["--help"],65536)?;
        match provider{
            Provider::Claude if !help.contains("--session-id")||!help.contains("--resume")=>return Err("Claude CLI lacks required session flags".into()),
            Provider::Codex=>{if !help.contains("app-server")||!help.contains("resume")||!capture_cli(&path,&["resume","--help"],65536)?.contains("--no-daemon"){return Err("Codex CLI lacks required native resume/app-server capabilities".into());}},
            _=>(),
        }
        m.configure_provider(provider,&path,&version)?;m.set_capabilities(provider,help.contains("--name"),help.contains("--remote-control"));Ok(ToolInfo{provider,executable:path.to_string_lossy().into_owned(),version})
    }).await
}
#[tauri::command]
pub async fn live_set_remote(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,enabled:bool)->Result<SessionInfo,String>{manager(window,&state,move|m|m.set_remote_control(&id,enabled)).await}
#[tauri::command]
pub async fn live_rename(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String,label:String)->Result<SessionInfo,String>{manager(window,&state,move|m|m.rename(&id,&label)).await}
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
#[tauri::command]
pub async fn live_start(window:tauri::WebviewWindow,state:tauri::State<'_,LiveState>,id:String)->Result<SessionInfo,String>{manager(window,&state,move|m|m.start(&id)).await}
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
