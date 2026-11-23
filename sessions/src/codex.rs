//! Trusted stdio metadata bootstrap only. Never starts a model turn, reads a
//! terminal ID, approves a server request, lists latest threads or edits rollout
//! files. Codex 0.165 requires setting a name to persist an otherwise empty thread.
use super::{sys, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_BUFFER: usize = 2 * 1024 * 1024;
const MAX_TOTAL: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn validate_uuid(value: &str) -> Result<()> {
    if Uuid::parse_str(value).map_err(|_| "Native provider identity is not a UUID")?.to_string() != value {
        return Err("Native provider identity is not a canonical UUID".into());
    }
    Ok(())
}

struct Protocol {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    buffer: Vec<u8>,
    total: usize,
    cancel: Arc<AtomicBool>,
}
impl Protocol {
    fn open(executable: &Path, cwd: &str, cancel: Arc<AtomicBool>) -> Result<Self> {
        if cancel.load(Ordering::Acquire) { return Err("Codex identity bootstrap cancelled".into()); }
        let mut command = Command::new(executable);
        command.args(["app-server", "--listen", "stdio://"]).current_dir(cwd)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        sys::isolate(&mut command);
        let mut child = command.spawn().map_err(|_| "Cannot start the dedicated Codex identity helper")?;
        let input = child.stdin.take().ok_or("Codex identity helper has no input")?;
        let output = child.stdout.take().ok_or("Codex identity helper has no output")?;
        let mut protocol = Self { child, input: Some(input), output, buffer: Vec::new(), total: 0, cancel };
        // All pipe I/O is nonblocking so cancellation does not depend on the
        // provider replying, consuming input or closing its stdout.
        sys::nonblocking(protocol.input.as_ref().unwrap()).map_err(|_| "Cannot configure Codex protocol input")?;
        sys::nonblocking(&protocol.output).map_err(|_| "Cannot configure Codex protocol output")?;
        protocol.request(1,"initialize",json!({"clientInfo":{"name":"agentview_sessions","version":"0.1.0"}}))?;
        protocol.send(json!({"method":"initialized","params":{}}),Instant::now()+TIMEOUT)?;
        Ok(protocol)
    }

    fn check(&self, deadline: Instant) -> Result<()> {
        if self.cancel.load(Ordering::Acquire) { return Err("Codex identity bootstrap cancelled; inspect native history before creating another session".into()); }
        if Instant::now() >= deadline { return Err("Codex identity bootstrap timed out; outcome unknown, no fallback thread will be created".into()); }
        Ok(())
    }
    fn send(&mut self, value: Value, deadline: Instant) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| "Cannot encode Codex protocol request")?;
        bytes.push(b'\n');
        let mut sent = 0;
        while sent < bytes.len() {
            self.check(deadline)?;
            match self.input.as_mut().ok_or("Codex identity input is closed")?.write(&bytes[sent..]) {
                Ok(0) => return Err("Codex identity input closed".into()),
                Ok(n) => sent += n,
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted => (),
                Err(_) => return Err("Codex identity request failed; outcome unknown".into()),
            }
        }
        Ok(())
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        let deadline = Instant::now()+TIMEOUT;
        self.send(json!({"id":id,"method":method,"params":params}),deadline)?;
        loop {
            self.check(deadline)?;
            if let Some(end) = self.buffer.iter().position(|b|*b==b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                let message: Value = serde_json::from_slice(&line).map_err(|_| "Codex sent invalid protocol data")?;
                if message.get("method").is_some() {
                    if message.get("id").is_some() { return Err("Codex requested an unexpected action during identity bootstrap; nothing was approved".into()); }
                    continue; // Notifications are neither identities nor approvals.
                }
                if message.get("id").and_then(Value::as_u64) != Some(id) { return Err("Codex returned a mismatched protocol response ID".into()); }
                if message.get("error").is_some() { return Err("Codex rejected the identity metadata operation; no fallback thread will be created".into()); }
                return message.get("result").cloned().ok_or_else(|| "Codex protocol result is missing".into());
            }
            let mut bytes = [0u8;8192];
            match self.output.read(&mut bytes) {
                Ok(0) => return Err("Codex identity helper exited before acknowledging the operation".into()),
                Ok(n) => {
                    self.total += n;
                    if self.total > MAX_TOTAL || self.buffer.len()+n > MAX_BUFFER { return Err("Codex identity protocol exceeded bounded output limits".into()); }
                    self.buffer.extend_from_slice(&bytes[..n]);
                },
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted => (),
                Err(_) => return Err("Codex identity protocol read failed".into()),
            }
        }
    }
}
impl Drop for Protocol {
    fn drop(&mut self) {
        self.input.take();
        let pid=self.child.id();
        let deadline=Instant::now()+Duration::from_millis(250);
        while Instant::now()<deadline && !sys::exited(pid).unwrap_or(true) {
            thread::sleep(Duration::from_millis(10));
        }
        // Retain unreaped PID through group termination to prevent PGID reuse.
        sys::signal_group(pid,libc::SIGKILL);
        let _=self.child.wait();
    }
}

fn identity(value: &Value, expected: Option<&str>, cwd: &str) -> Result<String> {
    let thread=value.get("thread").ok_or("Codex thread metadata is missing")?;
    let id=thread.get("id").and_then(Value::as_str).ok_or("Codex native thread ID is missing")?;
    validate_uuid(id)?;
    if expected.is_some_and(|e|e!=id) { return Err("Codex returned a different persisted thread ID".into()); }
    if thread.get("ephemeral").and_then(Value::as_bool)!=Some(false)
        || thread.get("cwd").and_then(Value::as_str)!=Some(cwd) {
        return Err("Codex thread persistence/project metadata does not match the reviewed project".into());
    }
    let path=thread.get("path").and_then(Value::as_str).ok_or("Codex did not provide a persistent rollout identity")?;
    if !PathBuf::from(path).is_absolute() { return Err("Codex rollout identity path is not absolute".into()); }
    Ok(id.into())
}

pub(super) struct Identity { pub id:String, pub path:String }
pub(super) fn bootstrap(executable: &Path, cwd: &str, label: &str, cancel: Arc<AtomicBool>) -> Result<Identity> {
    let mut first=Protocol::open(executable,cwd,Arc::clone(&cancel))?;
    let started=first.request(2,"thread/start",json!({"cwd":cwd,"ephemeral":false}))?;
    let id=identity(&started,None,cwd)?;
    // A name is provider-owned metadata. In installed 0.165 this causes an empty
    // thread's rollout to persist; thread/start alone does not. Never send a turn.
    first.request(3,"thread/name/set",json!({"threadId":id,"name":label}))?;
    drop(first);
    let mut verify=Protocol::open(executable,cwd,cancel)?;
    let saved=verify.request(2,"thread/read",json!({"threadId":id,"includeTurns":false}))?;
    let id=identity(&saved,Some(&id),cwd)?;
    let path=saved.pointer("/thread/path").and_then(Value::as_str).ok_or("Codex persisted rollout identity is missing")?.to_string();
    Ok(Identity{id,path})
}
