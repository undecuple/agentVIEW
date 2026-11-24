#![forbid(unsafe_code)]
//! App-owned simulated agents and durable conversations. No process or provider access.
use agent_app_core::policy::redact;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
pub mod notes;
pub mod webhook;
pub mod reviews;
pub mod settings;
pub use settings::{AppSettings,QuickPrompt,SettingsRecovery,SettingsSnapshot};

pub type Result<T> = std::result::Result<T, String>;
const PAGE: i64 = 100;
const MAX_BODY: usize = 8000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Availability { Idle, Busy, Paused }
impl Availability {
    fn value(&self) -> &'static str {
        match self { Self::Idle => "idle", Self::Busy => "busy", Self::Paused => "paused" }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub role: String,
    pub parent_id: Option<String>,
    pub project: Option<String>,
    pub purpose: String,
    pub availability: String,
    pub queued: i64,
}
#[derive(Debug, Serialize)]
pub struct Message {
    pub id: i64,
    pub agent_id: String,
    pub sender: String,
    pub body: String,
    pub state: String,
    pub created_at: String,
    pub reply_to: Option<i64>,
}
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub agents: Vec<Agent>,
    pub messages: Vec<Message>,
    pub has_more: bool,
}

pub struct Store { connection: Connection }
fn db<T>(r: rusqlite::Result<T>) -> Result<T> {
    r.map_err(|_| "Workspace storage operation failed".to_string())
}

/// Validate the whole roster before accepting any change. Parentage cannot form cycles:
/// intake -> coordinator -> subagent is the only allowed chain.
pub fn validate_hierarchy(agents: &[Agent]) -> Result<()> {
    use std::collections::HashMap;
    if agents.is_empty() || agents.len() > 512 { return Err("Invalid roster size".into()); }
    let mut ids = HashMap::new();
    for a in agents {
        if a.id.is_empty() || a.id.len() > 100 || a.name.is_empty() || a.name.len() > 120
            || !matches!(a.provider.as_str(), "claude" | "codex")
            || !matches!(a.availability.as_str(), "idle" | "busy" | "paused")
            || ids.insert(a.id.as_str(), a).is_some()
        { return Err("Invalid agent record".into()); }
    }
    for a in agents {
        if a.role == "intake" {
            if a.parent_id.is_some() || a.project.is_some() { return Err("Intake must be a root".into()); }
            continue;
        }
        let p = a.parent_id.as_deref().and_then(|id| ids.get(id)).ok_or("Missing parent")?;
        if a.provider != p.provider || a.project.as_deref().is_none_or(str::is_empty) {
            return Err("Provider/project ownership mismatch".into());
        }
        match a.role.as_str() {
            "coordinator" if p.role == "intake" => (),
            "subagent" if p.role == "coordinator" && a.project == p.project => (),
            _ => return Err("Invalid reporting relationship".into()),
        }
    }
    for provider in ["claude", "codex"] {
        if agents.iter().filter(|a| a.role == "intake" && a.provider == provider).count() != 1 {
            return Err("Exactly one intake per provider is required".into());
        }
    }
    Ok(())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = db(Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW))?;
        db(connection.busy_timeout(Duration::from_secs(5)))?;
        db(connection.execute_batch("PRAGMA foreign_keys=ON;"))?;
        // Take the write lock before checking the version, including simultaneous first opens.
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let version: i64 = db(tx.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        if version > 2 { return Err("Workspace was created by a newer app; refusing to modify it".into()); }
        if version == 0 {
            db(tx.execute_batch(
                "CREATE TABLE agents (
                 id TEXT PRIMARY KEY, name TEXT NOT NULL, provider TEXT NOT NULL,
                 role TEXT NOT NULL, parent_id TEXT REFERENCES agents(id), project TEXT,
                 purpose TEXT NOT NULL, availability TEXT NOT NULL CHECK(availability IN ('idle','busy','paused')));
                 CREATE TABLE messages (
                 id INTEGER PRIMARY KEY AUTOINCREMENT, agent_id TEXT NOT NULL REFERENCES agents(id),
                 sender TEXT NOT NULL CHECK(sender IN ('user','simulator')),
                 body TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('queued','delivered')),
                 request_id TEXT UNIQUE, reply_to INTEGER UNIQUE REFERENCES messages(id),
                 created_at TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now')));
                 CREATE INDEX conversation_history ON messages(agent_id,id);
                 CREATE INDEX delivery_queue ON messages(state,id);
                 PRAGMA user_version=1;"))?;
            let mut roster = Vec::new();
            for (provider, display) in [("claude", "Claude"), ("codex", "Codex")] {
                let intake = format!("{provider}-intake");
                roster.push(Agent { id: intake.clone(), name: format!("{display} intake-agent"), provider: provider.into(), role: "intake".into(), parent_id: None, project: None, purpose: "Intake & routing".into(), availability: "idle".into(), queued: 0 });
                for (project_id, project) in [("app", "Agent App"), ("docs", "Documentation")] {
                    let coord = format!("{provider}-{project_id}-coord");
                    roster.push(Agent { id: coord.clone(), name: format!("{project_id}-coordinator"), provider: provider.into(), role: "coordinator".into(), parent_id: Some(intake.clone()), project: Some(project.into()), purpose: "Project coordination".into(), availability: "idle".into(), queued: 0 });
                    for purpose in ["worker", "refuter"] {
                        roster.push(Agent { id: format!("{coord}-{purpose}"), name: format!("{project_id}-{purpose}"), provider: provider.into(), role: "subagent".into(), parent_id: Some(coord.clone()), project: Some(project.into()), purpose: purpose.into(), availability: "idle".into(), queued: 0 });
                    }
                }
            }
            validate_hierarchy(&roster)?;
            for a in &roster {
                db(tx.execute("INSERT INTO agents VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![a.id,a.name,a.provider,a.role,a.parent_id,a.project,a.purpose,a.availability]))?;
            }
        }
        settings::initialize(&tx)?;
        reviews::initialize(&tx)?;
        notes::initialize(&tx)?;
        webhook::initialize(&tx)?;
        db(tx.commit())?;
        let store = Self { connection };
        validate_hierarchy(&store.agents()?)?;
        Ok(store)
    }

    pub fn agents(&self) -> Result<Vec<Agent>> {
        let mut s = db(self.connection.prepare("SELECT a.*, (SELECT count(*) FROM messages m WHERE m.agent_id=a.id AND m.state='queued') FROM agents a ORDER BY a.provider,a.rowid LIMIT 513"))?;
        let rows = db(s.query_map([], |r| Ok(Agent { id:r.get(0)?, name:r.get(1)?, provider:r.get(2)?, role:r.get(3)?, parent_id:r.get(4)?, project:r.get(5)?, purpose:r.get(6)?, availability:r.get(7)?, queued:r.get(8)? })))?;
        db(rows.collect())
    }

    pub fn snapshot(&mut self, agent_id: &str, before: Option<i64>) -> Result<Snapshot> {
        let tx = db(self.connection.transaction())?;
        let mut roster = db(tx.prepare("SELECT a.*, (SELECT count(*) FROM messages m WHERE m.agent_id=a.id AND m.state='queued') FROM agents a ORDER BY a.provider,a.rowid LIMIT 513"))?;
        let agents: Vec<Agent> = db(db(roster.query_map([], |r| Ok(Agent { id:r.get(0)?, name:r.get(1)?, provider:r.get(2)?, role:r.get(3)?, parent_id:r.get(4)?, project:r.get(5)?, purpose:r.get(6)?, availability:r.get(7)?, queued:r.get(8)? })))?.collect())?;
        validate_hierarchy(&agents)?;
        if !agents.iter().any(|a| a.id == agent_id) { return Err("Unknown agent".into()); }
        let mut s = db(tx.prepare("SELECT id,agent_id,sender,body,state,created_at,reply_to FROM messages WHERE agent_id=?1 AND id<?2 ORDER BY id DESC LIMIT ?3"))?;
        let mut messages: Vec<Message> = db(db(s.query_map(params![agent_id,before.unwrap_or(i64::MAX),PAGE+1], |r| Ok(Message { id:r.get(0)?,agent_id:r.get(1)?,sender:r.get(2)?,body:r.get(3)?,state:r.get(4)?,created_at:r.get(5)?,reply_to:r.get(6)? })))?.collect())?;
        let has_more = messages.len() > PAGE as usize;
        messages.truncate(PAGE as usize);
        messages.reverse();
        for m in &mut messages { m.body = redact(&m.body); }
        Ok(Snapshot { agents, messages, has_more })
    }

    pub fn send(&mut self, agent: &str, request: &str, body: &str) -> Result<i64> {
        if request.len() < 8 || request.len() > 100 || !request.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || body.trim().is_empty() || body.len() > MAX_BODY { return Err("Message must be 1–8000 bytes with a valid request ID".into()); }
        let body = redact(body.trim());
        let tx = db(self.connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let prior: Option<(i64,String,String)> = db(tx.query_row("SELECT id,agent_id,body FROM messages WHERE request_id=?1", [request], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
        if let Some((id, a, b)) = prior {
            if a != agent || b != body { return Err("Request ID already belongs to a different message".into()); }
            return Ok(id);
        }
        db(tx.execute("INSERT INTO messages(agent_id,sender,body,state,request_id) VALUES (?1,'user',?2,'queued',?3)", params![agent,body,request]))?;
        let id = tx.last_insert_rowid();
        db(tx.commit())?;
        Ok(id)
    }

    pub fn control(&mut self, agent: &str, availability: Availability) -> Result<()> {
        let n = db(self.connection.execute("UPDATE agents SET availability=?1 WHERE id=?2", params![availability.value(),agent]))?;
        if n != 1 { return Err("Unknown agent".into()); }
        Ok(())
    }

    /// A single simulated safe turn. Delivery and its one deterministic reply are atomic.
    /// No external effects: crash before commit rolls everything back; crash after commit
    /// leaves no queued row to deliver twice. Separate connections serialize on BEGIN IMMEDIATE.
    pub fn tick(&mut self) -> Result<usize> {
        let tx = db(self.connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let candidate: Option<(i64,String)> = db(tx.query_row(
            "SELECT m.id,m.agent_id FROM messages m JOIN agents a ON a.id=m.agent_id
             WHERE m.state='queued' AND a.availability='idle' ORDER BY m.id LIMIT 1",
            [], |r| Ok((r.get(0)?,r.get(1)?))).optional())?;
        let Some((id,agent)) = candidate else { return Ok(0); };
        let reply = format!("Simulated reply: message #{id} received and saved. This agent is a local demo; no Claude or Codex session was contacted.");
        db(tx.execute("INSERT INTO messages(agent_id,sender,body,state,reply_to) VALUES (?1,'simulator',?2,'delivered',?3)",params![agent,reply,id]))?;
        db(tx.execute("UPDATE messages SET state='delivered' WHERE id=?1",[id]))?;
        db(tx.commit())?;
        Ok(1)
    }
}

/// Both values must come from the actual native webview, never IPC arguments.
pub fn trusted_window(label: &str, url: &str) -> bool {
    label == "workspace" && url == "workspace://localhost/"
}

pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; img-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'";
pub fn asset(path: &str) -> Option<(&'static str, &'static str)> {
    match path {
        "/" => Some(("text/html; charset=utf-8", include_str!("../../ui/workspace.html"))),
        "/workspace.css" => Some(("text/css; charset=utf-8", include_str!("../../ui/workspace.css"))),
        "/workspace.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/workspace.js"))),
        "/usage.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/usage.js"))),
        "/settings.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/settings.js"))),
        "/settings.css" => Some(("text/css; charset=utf-8", include_str!("../../ui/settings.css"))),
        "/overview.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/overview.js"))),
        "/flow.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/flow.js"))),
        "/webhook.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/webhook.js"))),
        "/backup.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/backup.js"))),
        "/git.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/git.js"))),
        "/claudecfg.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/claudecfg.js"))),
        "/notes.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/notes.js"))),
        "/review.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/review.js"))),
        "/identity.css" => Some(("text/css; charset=utf-8", include_str!("../../ui/identity.css"))),
        "/prefs.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/prefs.js"))),
        "/live.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/live.js"))),
        "/vendor/xterm.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/vendor/xterm.js"))),
        "/vendor/xterm.css" => Some(("text/css; charset=utf-8", include_str!("../../ui/vendor/xterm.css"))),
        "/vendor/addon-fit.js" => Some(("text/javascript; charset=utf-8", include_str!("../../ui/vendor/addon-fit.js"))),
        "/agentview.svg" => Some(("image/svg+xml", include_str!("../../ui/agentview.svg"))),
        "/agentview-light.svg" => Some(("image/svg+xml", include_str!("../../ui/agentview-light.svg"))),
        _ => None,
    }
}
