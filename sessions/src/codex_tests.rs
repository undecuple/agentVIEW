use super::*;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

// Fixed test executable, not a production override. It models provider-owned
// IDs and the 0.165 empty-thread persistence gap, never a model turn.
const FIXTURE: &str = r#"
import os,sys,json,uuid,time,tty
from pathlib import Path
root=Path.cwd()
scenario=(root/'scenario').read_text() if (root/'scenario').exists() else ''
if len(sys.argv)>1 and sys.argv[1]=='app-server':
 with (root/'helper-pids').open('a') as f:f.write(str(os.getpid())+'\n')
 thread=None
 for line in sys.stdin:
  req=json.loads(line);method=req['method']
  with (root/'methods').open('a') as f:f.write(method+'\n')
  if method=='initialized':continue
  result={}
  if method=='thread/start':
   if scenario=='hang':time.sleep(60)
   if scenario=='overflow':sys.stdout.write('x'*(3*1024*1024));sys.stdout.flush();continue
   if scenario=='approval':
    print(json.dumps({'id':900,'method':'item/commandExecution/requestApproval','params':{}}),flush=True);continue
   if scenario=='malformed':print('not-json',flush=True);continue
   assert set(req['params'])=={'cwd','ephemeral'}
   assert req['params']['cwd']==str(root) and req['params']['ephemeral']==False
   thread={'id':str(uuid.uuid4()),'cwd':str(root),'ephemeral':False,'path':str(root/'provider-record')}
   if scenario=='bad_uuid':thread['id']='untrusted terminal text'
   if scenario=='bad_cwd':thread['cwd']='/unreviewed-project'
   if scenario=='ephemeral':thread['ephemeral']=True
   if scenario=='bad_path':thread['path']='relative-rollout'
   result={'thread':thread}
  elif method=='thread/name/set':
   assert thread and req['params']['threadId']==thread['id']
   if scenario=='name_error':print(json.dumps({'id':req['id'],'error':{'code':-1,'message':'synthetic'}}),flush=True);continue
   (root/'provider-record').write_text(json.dumps(thread))
  elif method=='thread/read':
   assert req['params']['includeTurns']==False
   thread=json.loads((root/'provider-record').read_text())
   assert req['params']['threadId']==thread['id']
   if scenario=='read_mismatch':thread['id']=str(uuid.uuid4())
   if scenario=='read_ephemeral':thread['ephemeral']=True
   if scenario=='read_cwd':thread['cwd']='/wrong-project'
   result={'thread':thread}
  elif method!='initialize':raise Exception('Unexpected protocol request')
  id=req['id']+1 if scenario=='wrong_response' and method=='thread/start' else req['id']
  print(json.dumps({'id':id,'result':result}),flush=True)
 sys.exit(0)
tty.setraw(0)
if sys.argv[1]=='resume':
 expected=json.loads((root/'provider-record').read_text())['id']
 assert sys.argv[2:]==[expected,'--no-daemon']
print('READY:'+json.dumps({'argv':sys.argv[1:],'cwd':str(root)}),flush=True)
buf=b''
while True:
 data=os.read(0,4096)
 if not data:break
 buf+=data
 while b'\n' in buf:
  line,buf=buf.split(b'\n',1)
  print('ECHO:'+line.decode(),flush=True)
"#;

struct Fixture { _temp: TempDir, root: PathBuf, project: PathBuf, cli: PathBuf }
impl Fixture {
    fn new(scenario: &str) -> Self {
        let temp=tempfile::Builder::new().prefix("agentview-codex-test-").tempdir().unwrap();
        let root=fs::canonicalize(temp.path()).unwrap();
        let project=root.join("project");fs::create_dir(&project).unwrap();
        fs::write(project.join("scenario"),scenario).unwrap();
        let python=Command::new("python3").args(["-c","import sys;print(sys.executable)"]).output().unwrap();
        let cli=root.join("fixture-codex");
        fs::write(&cli,format!("#!{}\n{}",String::from_utf8(python.stdout).unwrap().trim(),FIXTURE)).unwrap();
        fs::set_permissions(&cli,fs::Permissions::from_mode(0o700)).unwrap();
        Self { _temp:temp, root, project, cli }
    }
    fn manager(&self) -> SessionManager {
        let mut manager=SessionManager::open_unconfigured(&self.root.join("sessions")).unwrap();
        manager.configure_provider(Provider::Codex,&self.cli,"fixture-codex-1").unwrap();
        manager
    }
    fn methods(&self) -> String { fs::read_to_string(self.project.join("methods")).unwrap_or_default() }
    fn assert_helpers_stopped(&self) {
        for pid in fs::read_to_string(self.project.join("helper-pids")).unwrap_or_default().lines() {
            let stat=fs::read_to_string(format!("/proc/{pid}/stat"));
            assert!(stat.is_err() || stat.unwrap().split_whitespace().nth(2)==Some("Z"),"Bootstrap helper survived cleanup");
        }
    }
}
fn terminal(manager: &mut SessionManager, info: &SessionInfo, marker: &str) -> String {
    let deadline=Instant::now()+Duration::from_secs(3);
    loop {
        let read=manager.read(&info.id,&info.generation,0).unwrap();
        let text=String::from_utf8_lossy(&read.data).into_owned();
        if text.contains(marker) { return text; }
        assert!(Instant::now()<deadline,"Fixture terminal marker missing");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn codex_identity_is_provider_owned_durable_and_resume_is_exact() {
    let fixture=Fixture::new("");let mut manager=fixture.manager();
    let record=manager.create_for(Provider::Codex,"Codex project",&fixture.project).unwrap();
    assert!(record.provider_session_id.is_none());assert!(!record.bootstrap_attempted);
    let live=manager.start(&record.id).unwrap();
    let native=live.provider_session_id.as_ref().unwrap();assert_ne!(native,&live.id);
    let text=terminal(&mut manager,&live,"READY:");
    let ready:serde_json::Value=serde_json::from_str(text.lines().find_map(|s|s.strip_prefix("READY:")).unwrap()).unwrap();
    assert_eq!(ready["argv"],serde_json::json!(["resume",native,"--no-daemon"]));
    let methods=fixture.methods();
    assert_eq!(methods.matches("thread/start\n").count(),1);
    assert_eq!(methods.matches("thread/read\n").count(),1);
    assert!(!methods.contains("turn/start"));assert!(!methods.contains("thread/list"));
    manager.write(&live.id,&live.generation,b"accepted-once\n").unwrap();
    terminal(&mut manager,&live,"ECHO:accepted-once");
    manager.stop(&live.id,&live.generation).unwrap();drop(manager);
    let mut manager=SessionManager::open_unconfigured(&fixture.root.join("sessions")).unwrap();
    let saved=manager.list().unwrap().remove(0);
    assert_eq!(saved.provider,Provider::Codex);assert_eq!(saved.provider_session_id,live.provider_session_id);
    assert!(!manager.read(&saved.id,&saved.generation,0).unwrap().running);
    assert!(manager.start(&saved.id).unwrap_err().contains("Inspect this provider"));
    manager.configure_provider(Provider::Codex,&fixture.cli,"fixture-codex-1").unwrap();
    let resumed=manager.start(&saved.id).unwrap();
    assert_ne!(resumed.generation,live.generation);
    let output=terminal(&mut manager,&resumed,"READY:");assert!(!output.contains("accepted-once"));
    assert_eq!(fixture.methods(),methods,"Resume must not create another provider thread");
    manager.stop(&resumed.id,&resumed.generation).unwrap();
    // An updated Codex CLI: strict without the setting, adopted with it, same native thread resumed, no new thread.
    manager.configure_provider(Provider::Codex,&fixture.cli,"fixture-codex-2").unwrap();
    assert!(manager.start(&saved.id).unwrap_err().contains("changed"));
    let followed=manager.start_with(&saved.id,true).unwrap();
    assert_eq!(followed.version,"fixture-codex-2");assert_eq!(followed.provider_session_id,saved.provider_session_id);
    let text=terminal(&mut manager,&followed,"READY:");
    let ready:serde_json::Value=serde_json::from_str(text.lines().find_map(|s|s.strip_prefix("READY:")).unwrap()).unwrap();
    assert_eq!(ready["argv"],serde_json::json!(["resume",saved.provider_session_id.as_deref().unwrap(),"--no-daemon"]));
    assert_eq!(fixture.methods(),methods,"Following an update must not create another provider thread");
    manager.shutdown().unwrap();fixture.assert_helpers_stopped();
}

#[test]
fn bootstrap_adverse_protocol_inputs_fail_closed_without_retry_or_approval() {
    for scenario in ["wrong_response","bad_uuid","bad_cwd","ephemeral","bad_path","name_error",
                     "read_mismatch","read_ephemeral","read_cwd","approval","malformed","overflow"] {
        let fixture=Fixture::new(scenario);let mut manager=fixture.manager();
        let record=manager.create_for(Provider::Codex,scenario,&fixture.project).unwrap();
        assert!(manager.start(&record.id).is_err(),"Accepted {scenario}");
        let failed=manager.list().unwrap().remove(0);
        assert!(failed.bootstrap_attempted);assert!(failed.provider_session_id.is_none());
        assert_eq!(failed.status,"bootstrap_unknown");
        let methods=fixture.methods();
        assert!(manager.start(&record.id).unwrap_err().contains("outcome is unknown"));
        assert_eq!(fixture.methods(),methods,"Uncertain bootstrap silently retried: {scenario}");
        assert!(!methods.contains("turn/start"));
        fixture.assert_helpers_stopped();
    }
}

#[test]
fn bootstrap_cancels_without_locking_the_ui_or_leaking_helper() {
    let fixture=Fixture::new("hang");let mut manager=fixture.manager();
    let cancel=Arc::new(AtomicBool::new(false));manager.set_cancel_flag(Arc::clone(&cancel));
    let record=manager.create_for(Provider::Codex,"Cancel",&fixture.project).unwrap();
    let trigger=thread::spawn(move || {thread::sleep(Duration::from_millis(100));cancel.store(true,Ordering::Release);});
    let began=Instant::now();
    assert!(manager.start(&record.id).unwrap_err().contains("cancelled"));
    trigger.join().unwrap();assert!(began.elapsed()<Duration::from_secs(2));
    assert_eq!(manager.list().unwrap()[0].status,"bootstrap_unknown");fixture.assert_helpers_stopped();
}

#[test]
fn interrupted_bootstrap_restores_unknown_without_creating_another_thread() {
    let fixture=Fixture::new("");let mut manager=fixture.manager();
    let record=manager.create_for(Provider::Codex,"Interrupted identity",&fixture.project).unwrap();
    manager.connection.execute("UPDATE sessions SET bootstrap_attempted=1,status='bootstrapping',generation='interrupted-generation' WHERE id=?1",[&record.id]).unwrap();
    drop(manager);
    let mut manager=fixture.manager();
    let saved=manager.list().unwrap().remove(0);
    assert_eq!(saved.status,"bootstrap_unknown");assert!(saved.provider_session_id.is_none());
    assert_eq!(saved.generation,"interrupted-generation");
    assert!(manager.start(&saved.id).unwrap_err().contains("outcome is unknown"));
    assert!(fixture.methods().is_empty(),"Restoring uncertain metadata must not bootstrap or replay");
}

#[test]
fn native_identity_save_failure_never_launches_or_retries_a_new_thread() {
    let fixture=Fixture::new("");let mut manager=fixture.manager();
    let record=manager.create_for(Provider::Codex,"Storage uncertainty",&fixture.project).unwrap();
    manager.connection.execute_batch("CREATE TRIGGER fail_native_identity BEFORE UPDATE OF provider_session_id ON sessions
        WHEN NEW.provider_session_id IS NOT NULL BEGIN SELECT RAISE(FAIL,'synthetic identity write failure'); END;").unwrap();
    assert!(manager.start(&record.id).unwrap_err().contains("outcome unknown"));
    let failed=manager.list().unwrap().remove(0);
    assert!(failed.bootstrap_attempted);assert!(failed.provider_session_id.is_none());
    assert_eq!(failed.status,"bootstrap_unknown");assert!(manager.running.is_empty());
    let methods=fixture.methods();assert_eq!(methods.matches("thread/start\n").count(),1);
    assert!(manager.start(&record.id).is_err());assert_eq!(fixture.methods(),methods);
    fixture.assert_helpers_stopped();
}

#[test]
fn v1_claude_migration_preserves_identity_generation_and_recipe() {
    let fixture=Fixture::new("");let directory=fixture.root.join("sessions");
    fs::DirBuilder::new().mode(0o700).create(&directory).unwrap();
    let path=directory.join("sessions.sqlite3");let c=Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,label TEXT NOT NULL,cwd TEXT NOT NULL,
        executable TEXT NOT NULL,version TEXT NOT NULL,recipe TEXT NOT NULL,attempted INTEGER NOT NULL,
        generation TEXT NOT NULL,status TEXT NOT NULL,executable_identity TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    let id=Uuid::new_v4().to_string();
    c.execute("INSERT INTO sessions VALUES (?1,'Original',?2,?3,'old-version',?4,1,'old-generation','stopped',?5)",
        params![id,fixture.project.to_str().unwrap(),fixture.cli.to_str().unwrap(),RECIPE,executable_identity(&fixture.cli).unwrap()]).unwrap();
    drop(c);fs::set_permissions(&path,fs::Permissions::from_mode(0o600)).unwrap();
    let mut manager=SessionManager::open_unconfigured(&directory).unwrap();
    let saved=manager.list().unwrap().remove(0);
    assert_eq!(saved.id,id);assert_eq!(saved.provider,Provider::Claude);
    assert_eq!(saved.provider_session_id.as_deref(),Some(id.as_str()));
    assert!(saved.attempted);assert!(!saved.bootstrap_attempted);
    assert_eq!(saved.generation,"old-generation");assert_eq!(saved.recipe,RECIPE);
    assert_eq!(saved.version,"old-version");assert_eq!(saved.status,"stopped");
    assert!(fixture.methods().is_empty());
    assert_eq!(manager.connection.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),3);
}

#[test]
fn two_providers_use_stored_tags_and_share_only_the_total_slot_limit() {
    let fixture=Fixture::new("");let mut manager=fixture.manager();
    manager.configure_provider(Provider::Claude,&fixture.cli,"fixture-claude-1").unwrap();
    let claude=manager.create("Claude",&fixture.project).unwrap();
    let codex=manager.create_for(Provider::Codex,"Codex",&fixture.project).unwrap();
    let claude=manager.start(&claude.id).unwrap();terminal(&mut manager,&claude,"READY:");
    let codex=manager.start(&codex.id).unwrap();terminal(&mut manager,&codex,"READY:");
    assert_eq!(claude.provider_session_id.as_deref(),Some(claude.id.as_str()));
    assert_ne!(codex.provider_session_id.as_deref(),Some(codex.id.as_str()));
    manager.set_max_active(2); // the shared total limit, without starting 30 processes
    let third=manager.create("Third",&fixture.project).unwrap();assert!(manager.start(&third.id).is_err());
    assert!(Provider::parse("model-output-provider").is_err());
    manager.shutdown().unwrap();
}
