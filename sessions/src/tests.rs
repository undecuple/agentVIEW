use super::*;
use std::os::unix::fs::{symlink, PermissionsExt};
use tempfile::TempDir;

// Tests alone use a fixed synthetic executable. Production never depends on
// Python or starts a shell. The fixture opens /dev/tty to prove controlling-PTY
// semantics, reports argv/cwd and accepts bounded commands over its native TTY.
const FIXTURE: &str = r#"
import os,sys,json,termios,fcntl,struct,tty,subprocess
tty.setraw(0)
t=os.open('/dev/tty',os.O_RDWR)
if sys.argv[1]=='--resume' and os.path.exists('reject-resume'):
 print('RESUME_REJECTED',flush=True)
 sys.exit(3)
print('READY:'+json.dumps({'argv':sys.argv[1:],'cwd':os.getcwd(),'tty':os.isatty(t),'group':os.getpgrp()==os.getpid(),'term':os.environ.get('TERM')}),flush=True)
buf=b''
while True:
 data=os.read(0,4096)
 if not data:break
 buf+=data
 while b'\n' in buf:
  line,buf=buf.split(b'\n',1)
  if line==b'size':
   rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(t,termios.TIOCGWINSZ,b'\0'*8))
   print('SIZE:'+str(cols)+':'+str(rows),flush=True)
  elif line==b'flood':
   sys.stdout.write('x'*600000+'\nFLOOD_DONE\n');sys.stdout.flush()
  elif line==b'spawn':
   p=subprocess.Popen([sys.executable,'-c','import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'])
   print('DESC:'+str(p.pid),flush=True)
  else:print('ECHO:'+line.decode('utf8',errors='replace'),flush=True)
"#;

struct Fixture { _dir: TempDir, dir: PathBuf, executable: PathBuf, a: PathBuf, b: PathBuf }
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("agentview-session-test-").tempdir().unwrap();
        let path = fs::canonicalize(dir.path()).unwrap();
        let python = Command::new("python3").args(["-c", "import sys;print(sys.executable)"]).output().unwrap();
        assert!(python.status.success());
        let executable = path.join("fixture-cli");
        fs::write(&executable, format!("#!{}\n{}", String::from_utf8(python.stdout).unwrap().trim(), FIXTURE)).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let (a,b) = (path.join("project-a"),path.join("project-b"));
        fs::create_dir(&a).unwrap();fs::create_dir(&b).unwrap();
        Self { _dir:dir, dir:path, executable, a, b }
    }
    fn manager(&self) -> SessionManager { SessionManager::open(&self.dir.join("sessions"), &self.executable, "fixture-1").unwrap() }
}

fn output(manager: &mut SessionManager, info: &SessionInfo, marker: &str) -> String {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let result = manager.read(&info.id, &info.generation, 0).unwrap();
        let text = String::from_utf8_lossy(&result.data).into_owned();
        if text.contains(marker) { return text; }
        assert!(Instant::now() < until, "Missing fixture marker {marker}; bytes={}", text.len());
        thread::sleep(Duration::from_millis(10));
    }
}
fn ready(manager: &mut SessionManager, info: &SessionInfo) -> serde_json::Value {
    let text = output(manager,info,"READY:");
    let line = text.lines().find_map(|s|s.strip_prefix("READY:")).unwrap();
    serde_json::from_str(line).unwrap()
}

#[test]
fn two_projects_exact_ids_and_controlling_terminals() {
    let fixture = Fixture::new();let mut manager = fixture.manager();
    let a = manager.create("Project A",&fixture.a).unwrap();
    let b = manager.create("Project B",&fixture.b).unwrap();
    assert_ne!(a.id,b.id);
    for (record,cwd) in [(a,&fixture.a),(b,&fixture.b)] {
        assert!(!record.attempted);
        let live = manager.start(&record.id).unwrap();
        let observed = ready(&mut manager,&live);
        assert_eq!(observed["argv"],serde_json::json!(["--session-id",live.id]));
        assert_eq!(observed["cwd"],cwd.to_str().unwrap());
        assert_eq!(observed["tty"],true);assert_eq!(observed["group"],true);
        assert_eq!(observed["term"],"xterm-256color");
        manager.write(&live.id,&live.generation,b"hello\n").unwrap();
        assert!(output(&mut manager,&live,"ECHO:hello").contains("ECHO:hello"));
        manager.resize(&live.id,&live.generation,132,43).unwrap();
        manager.write(&live.id,&live.generation,b"size\n").unwrap();
        assert!(output(&mut manager,&live,"SIZE:132:43").contains("SIZE:132:43"));
    }
    let c=manager.create("Third",&fixture.a).unwrap();
    assert!(manager.start(&c.id).unwrap_err().contains("Two live"));
    manager.shutdown().unwrap();
    assert!(manager.list().unwrap().iter().all(|s|s.status=="stopped" || !s.attempted));
}

#[test]
fn restart_is_idle_and_exact_resume_never_replays_input() {
    let fixture=Fixture::new();let mut manager=fixture.manager();
    let record=manager.create("Durable",&fixture.a).unwrap();
    let first=manager.start(&record.id).unwrap();ready(&mut manager,&first);
    manager.write(&first.id,&first.generation,b"NEVER_REPLAY_ME\n").unwrap();
    output(&mut manager,&first,"ECHO:NEVER_REPLAY_ME");
    drop(manager);
    let mut manager=fixture.manager();
    let saved=manager.list().unwrap().remove(0);
    assert!(saved.attempted);assert_ne!(saved.status,"running");
    assert!(manager.read(&saved.id,&saved.generation,0).unwrap().data.is_empty());
    let second=manager.start(&saved.id).unwrap();
    assert_ne!(first.generation,second.generation);
    let observed=ready(&mut manager,&second);
    assert_eq!(observed["argv"],serde_json::json!(["--resume",first.id]));
    assert!(!output(&mut manager,&second,"READY:").contains("NEVER_REPLAY_ME"));
    assert!(manager.write(&first.id,&first.generation,b"stale\n").is_err());
    assert!(manager.read(&first.id,&first.generation,0).is_err());
    assert!(manager.resize(&first.id,&first.generation,80,24).is_err());
    assert!(manager.stop(&first.id,&first.generation).is_err());
    manager.stop(&second.id,&second.generation).unwrap();
}

#[test]
fn rejected_resume_exits_without_new_conversation_fallback() {
    let fixture=Fixture::new();let mut manager=fixture.manager();
    let record=manager.create("Reject recovery",&fixture.a).unwrap();
    let first=manager.start(&record.id).unwrap();ready(&mut manager,&first);
    manager.stop(&first.id,&first.generation).unwrap();
    fs::write(fixture.a.join("reject-resume"),"").unwrap();
    let second=manager.start(&record.id).unwrap();
    let text=output(&mut manager,&second,"RESUME_REJECTED");
    assert!(!text.contains("READY:"));
    let until=Instant::now()+Duration::from_secs(3);
    while manager.list().unwrap()[0].status=="running" {
        assert!(Instant::now()<until);thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(manager.list().unwrap().len(),1);
    assert_eq!(manager.list().unwrap()[0].status,"exited");
}

#[test]
fn bounded_ring_discards_old_output_and_metadata_has_no_transcript() {
    let fixture=Fixture::new();let mut manager=fixture.manager();
    let record=manager.create("Bounded",&fixture.a).unwrap();
    let live=manager.start(&record.id).unwrap();ready(&mut manager,&live);
    manager.write(&live.id,&live.generation,b"flood\n").unwrap();
    let until=Instant::now()+Duration::from_secs(5);
    loop {
        let r=manager.read(&live.id,&live.generation,0).unwrap();
        if r.start_offset>300000 { break; }
        assert!(Instant::now()<until);thread::sleep(Duration::from_millis(10));
    }
    let first=manager.read(&live.id,&live.generation,0).unwrap();
    assert!(first.truncated);assert!(first.data.len()<=READ_BYTES);
    let mut offset=first.next_offset;let mut bytes=first.data;
    let until=Instant::now()+Duration::from_secs(5);
    while !bytes.ends_with(b"FLOOD_DONE\n") {
        let r=manager.read(&live.id,&live.generation,offset).unwrap();
        offset=r.next_offset;
        assert!(r.data.len()<=READ_BYTES);
        bytes.extend(r.data);
        assert!(Instant::now()<until,"Flood fixture did not finish");
        thread::sleep(Duration::from_millis(10));
    }
    manager.stop(&live.id,&live.generation).unwrap();
    let stored=fs::read(fixture.dir.join("sessions/sessions.sqlite3")).unwrap();
    assert!(!stored.windows(128).any(|s|s==[b'x';128]));
}

#[test]
fn storage_lock_symlinks_permissions_and_future_schema_are_refused() {
    let fixture=Fixture::new();let manager=fixture.manager();
    assert!(SessionManager::open(&fixture.dir.join("sessions"),&fixture.executable,"fixture-1").is_err());
    let lock = fixture.dir.join("sessions/manager.lock");
    let attempted = Command::new("python3").args(["-c", "import fcntl,sys; f=open(sys.argv[1],'r+'); fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)"])
        .arg(lock).output().unwrap();
    assert!(!attempted.status.success(), "A second process acquired the active manager lease");
    drop(manager);
    let link=fixture.dir.join("linked");symlink(fixture.dir.join("sessions"),&link).unwrap();
    assert!(SessionManager::open(&link,&fixture.executable,"fixture-1").is_err());
    let path=fixture.dir.join("sessions/sessions.sqlite3");
    let c=Connection::open(&path).unwrap();c.execute_batch("PRAGMA user_version=99").unwrap();drop(c);
    assert!(SessionManager::open(&fixture.dir.join("sessions"),&fixture.executable,"fixture-1").is_err());
    let c=Connection::open(&path).unwrap();assert_eq!(c.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),99);
    let public=fixture.dir.join("public");fs::create_dir(&public).unwrap();
    fs::set_permissions(&public,fs::Permissions::from_mode(0o755)).unwrap();
    assert!(SessionManager::open(&public,&fixture.executable,"fixture-1").is_err());
}

#[test]
fn transient_dispatch_thread_does_not_own_provider_lifetime() {
    let fixture=Fixture::new();let manager=fixture.manager();
    let project=fixture.a.clone();
    let (mut manager,live)=thread::spawn(move || {
        let mut manager=manager;
        let info=manager.create("Dispatch lifetime",&project).unwrap();
        let live=manager.start(&info.id).unwrap();
        (manager,live)
    }).join().unwrap();
    thread::sleep(Duration::from_millis(100));
    ready(&mut manager,&live);
    assert!(manager.read(&live.id,&live.generation,0).unwrap().running);
    manager.write(&live.id,&live.generation,b"after-dispatch-exit\n").unwrap();
    output(&mut manager,&live,"ECHO:after-dispatch-exit");
}

#[test]
fn post_exec_storage_failure_stops_owned_process() {
    let fixture=Fixture::new();let mut manager=fixture.manager();
    let info=manager.create("Storage failure",&fixture.a).unwrap();
    manager.connection.execute_batch("CREATE TRIGGER fail_running BEFORE UPDATE OF status ON sessions
        WHEN NEW.status='running' BEGIN SELECT RAISE(FAIL,'synthetic state write failure'); END;").unwrap();
    assert!(manager.start(&info.id).unwrap_err().contains("owned process was stopped"));
    assert!(manager.running.get(&info.id).unwrap().child.is_none());
    assert_eq!(manager.list().unwrap()[0].status,"launch_failed");
    assert!(manager.list().unwrap()[0].attempted);
}

#[test]
fn manager_crash_helper() {
    let Some(directory)=std::env::var_os("AGENTVIEW_SESSION_TEST_HELPER_DIR") else{return;};
    let executable=PathBuf::from(std::env::var_os("AGENTVIEW_SESSION_TEST_HELPER_EXE").unwrap());
    let project=PathBuf::from(std::env::var_os("AGENTVIEW_SESSION_TEST_HELPER_PROJECT").unwrap());
    let mut manager=SessionManager::open(&PathBuf::from(&directory).join("sessions"),&executable,"fixture-1").unwrap();
    let record=manager.create("Parent death",&project).unwrap();
    let live=manager.start(&record.id).unwrap();ready(&mut manager,&live);
    let pid=manager.running[&live.id].child.as_ref().unwrap().id();
    let directory=PathBuf::from(directory);
    fs::write(directory.join("helper-child.pid.tmp"),pid.to_string()).unwrap();
    fs::rename(directory.join("helper-child.pid.tmp"),directory.join("helper-child.pid")).unwrap();
    thread::sleep(Duration::from_secs(60));
}

#[test]
fn abrupt_manager_process_death_kills_provider_leader() {
    let fixture=Fixture::new();
    let mut helper=Command::new(std::env::current_exe().unwrap())
        .args(["--exact","tests::manager_crash_helper","--test-threads=1"])
        .env("AGENTVIEW_SESSION_TEST_HELPER_DIR",&fixture.dir)
        .env("AGENTVIEW_SESSION_TEST_HELPER_EXE",&fixture.executable)
        .env("AGENTVIEW_SESSION_TEST_HELPER_PROJECT",&fixture.a)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let until=Instant::now()+Duration::from_secs(5);
    let path=fixture.dir.join("helper-child.pid");
    while !path.exists() {
        if Instant::now()>=until || helper.try_wait().unwrap().is_some() {
            let _=helper.kill();let _=helper.wait();panic!("Manager helper failed to launch fixture");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let pid:u32=fs::read_to_string(path).unwrap().parse().unwrap();
    helper.kill().unwrap();helper.wait().unwrap();
    let until=Instant::now()+Duration::from_secs(3);
    loop {
        let stat=fs::read_to_string(format!("/proc/{pid}/stat"));
        if stat.is_err() || stat.unwrap().split_whitespace().nth(2)==Some("Z"){break;}
        assert!(Instant::now()<until,"Provider leader survived app process death");
        thread::sleep(Duration::from_millis(10));
    }
    // Metadata remains recoverable, without launching anything automatically.
    let mut manager=fixture.manager();
    assert_eq!(manager.list().unwrap()[0].status,"interrupted");
}

#[test]
fn spawn_failure_is_durable_and_recipe_or_version_changes_refuse_resume() {
    let fixture=Fixture::new();
    fs::write(&fixture.executable,"#!/missing-agentview-test-interpreter\n").unwrap();
    let mut manager=fixture.manager();
    let record=manager.create("Exec failure",&fixture.a).unwrap();
    assert!(manager.start(&record.id).unwrap_err().contains("--resume"));
    let info=manager.list().unwrap().remove(0);
    assert!(info.attempted);assert_eq!(info.status,"launch_failed");
    drop(manager);
    let mut changed=SessionManager::open(&fixture.dir.join("sessions"),&fixture.executable,"fixture-2").unwrap();
    assert!(changed.start(&record.id).unwrap_err().contains("changed"));
}

#[test]
fn stop_cleans_descendants_and_releases_store_after_drop() {
    let fixture=Fixture::new();let mut manager=fixture.manager();
    let record=manager.create("Cleanup",&fixture.a).unwrap();
    let live=manager.start(&record.id).unwrap();ready(&mut manager,&live);
    manager.write(&live.id,&live.generation,b"spawn\n").unwrap();
    let text=output(&mut manager,&live,"DESC:");
    let pid:u32=text.lines().find_map(|s|s.strip_prefix("DESC:")).unwrap().parse().unwrap();
    manager.stop(&live.id,&live.generation).unwrap();
    let until=Instant::now()+Duration::from_secs(3);
    loop {
        let stat=fs::read_to_string(format!("/proc/{pid}/stat"));
        if stat.is_err() || stat.unwrap().split_whitespace().nth(2)==Some("Z"){break;}
        assert!(Instant::now()<until,"Descendant still running after group stop");thread::sleep(Duration::from_millis(10));
    }
    drop(manager);drop(fixture.manager());
}

#[test]
fn input_bounds_unknown_ids_and_project_changes_fail_closed() {
    let fixture=Fixture::new();let mut manager=fixture.manager();
    assert!(manager.create("bad\nlabel",&fixture.a).is_err());
    assert!(manager.start("not-a-session").is_err());
    let record=manager.create("Bounds",&fixture.a).unwrap();
    let live=manager.start(&record.id).unwrap();ready(&mut manager,&live);
    assert!(manager.write(&live.id,&live.generation,&vec![0;WRITE_BYTES+1]).is_err());
    assert!(manager.resize(&live.id,&live.generation,0,0).is_err());
    assert!(manager.read(&live.id,&live.generation,u64::MAX).is_err());
    manager.stop(&live.id,&live.generation).unwrap();
    fs::rename(&fixture.a,fixture.dir.join("old-project")).unwrap();
    symlink(&fixture.b,&fixture.a).unwrap();
    assert!(manager.start(&record.id).is_err());
}

#[test]
fn display_names_are_clear_and_bounded() {
    assert_eq!(display_name("coordinator", "/x/demo", Provider::Claude), "demo · coordinator · Claude");
    assert_eq!(display_name("Demo coordinator", "/x/demo", Provider::Codex), "Demo coordinator · Codex");
    assert_eq!(display_name("a\u{7}b", "", Provider::Claude), "ab · Claude");
    assert!(display_name(&"x".repeat(100), "/x/p", Provider::Claude).chars().count() <= 80);
}

#[test]
fn remote_control_and_name_reach_the_cli_only_when_enabled_and_supported() {
    let fixture = Fixture::new();let mut manager = fixture.manager();
    let info = manager.create("coordinator",&fixture.a).unwrap();
    assert!(!info.remote_control);
    assert_eq!(info.display_name, "project-a · coordinator · Claude");
    manager.set_capabilities(Provider::Claude, true, false);
    // Unsupported CLI: Remote Control is refused at start, nothing launches.
    manager.set_remote_control(&info.id, true).unwrap();
    assert!(manager.start(&info.id).is_err());
    manager.set_capabilities(Provider::Claude, true, true);
    let live = manager.start(&info.id).unwrap();
    let argv = ready(&mut manager,&live)["argv"].clone();
    let args: Vec<String> = argv.as_array().unwrap().iter().map(|v|v.as_str().unwrap().to_string()).collect();
    assert!(args.windows(2).any(|w| w == ["--name", "project-a · coordinator · Claude"]));
    assert!(args.windows(2).any(|w| w == ["--remote-control", "project-a · coordinator · Claude"]));
    // Running sessions cannot be renamed or have Remote Control changed.
    assert!(manager.rename(&info.id,"other").is_err());
    assert!(manager.set_remote_control(&info.id,false).is_err());
    manager.stop(&info.id,&live.generation).unwrap();
    let renamed = manager.rename(&info.id,"  lead  ").unwrap();
    assert_eq!(renamed.display_name, "project-a · lead · Claude");
    assert!(manager.rename(&info.id,"bad\nname").is_err());
    assert!(!manager.set_remote_control(&info.id,false).unwrap().remote_control);
}
