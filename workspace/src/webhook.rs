//! One webhook address for "an agent may be waiting for you" messages. The address often holds a secret token, so it is
//! kept only in the app's private database, never returned to the page (only its host name), and left out of bundles.
use super::{db, Result, Store};
use rusqlite::{params, Transaction};
use std::net::Ipv4Addr;

pub const MAX_HOOKS: i64 = 5;
pub const EVENTS: [&str; 4] = ["quiet", "error", "finished", "waiting"];

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    db(tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS webhooks (
           id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, url TEXT NOT NULL, enabled INTEGER NOT NULL, events TEXT NOT NULL,
           updated TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now')));
         CREATE TABLE IF NOT EXISTS webhook (id INTEGER PRIMARY KEY CHECK(id=1), url TEXT NOT NULL, enabled INTEGER NOT NULL, updated TEXT NOT NULL DEFAULT 'x');
         INSERT INTO webhooks(name,url,enabled,events)
           SELECT 'Default',url,enabled,'quiet,error' FROM webhook WHERE NOT EXISTS (SELECT 1 FROM webhooks);
         DROP TABLE IF EXISTS webhook;"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook { pub id: i64, pub name: String, pub url: String, pub enabled: bool, pub events: Vec<String> }

fn clean_events(events: &[String]) -> Result<String> {
    let mut v: Vec<&str> = EVENTS.iter().copied().filter(|e| events.iter().any(|x| x == e)).collect();
    v.dedup();
    if events.iter().any(|x| !EVENTS.contains(&x.as_str())) { return Err("Unknown event".into()); }
    if v.is_empty() { return Err("Choose at least one event".into()); }
    Ok(v.join(","))
}
fn clean_name(n: &str) -> Result<String> {
    let t = n.trim();
    if t.is_empty() || t.chars().count() > 40 || t.chars().any(char::is_control) { return Err("Give the destination a short name (up to 40 characters)".into()); }
    Ok(t.to_string())
}

/// Accepts only `https://host[:port]/path` made of URL-safe characters, with a public-looking host name.
/// Returns the host (no port, path or token). It does not resolve names, so a name that points at a private address is not caught here.
pub fn validate_url(url: &str) -> Result<String> {
    let u = url.trim();
    if u.len() > 500 { return Err("That address is too long".into()); }
    let rest = u.strip_prefix("https://").ok_or("Only https:// addresses are accepted")?;
    if !u.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&b)) { return Err("The address contains characters that are not allowed".into()); }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.starts_with('[') { return Err("The address needs a plain host name".into()); }
    let (host, port) = match authority.split_once(':') { Some((h, p)) => (h, Some(p)), None => (authority, None) };
    if port.is_some_and(|p| p.parse::<u16>().map_or(true, |n| n == 0)) { return Err("The port is not valid".into()); }
    let host = host.to_ascii_lowercase();
    if host.is_empty() || host.len() > 253 || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') || host.starts_with(['.', '-']) || host.ends_with(['.', '-']) || host.contains("..") {
        return Err("The host name is not valid".into());
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        if ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]) { return Err("Local and private network addresses are refused".into()); }
    } else if !host.contains('.') || host == "localhost" || [".local", ".localhost", ".internal", ".lan", ".home", ".corp"].iter().any(|s| host.ends_with(s)) {
        return Err("Local host names are refused".into());
    } else if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err("Use a host name or a full IPv4 address".into());
    }
    Ok(host)
}

impl Store {
    /// Includes the address, so the caller must not forward `url` to a page.
    pub fn webhooks(&self) -> Result<Vec<Hook>> {
        let mut s = db(self.connection.prepare("SELECT id,name,url,enabled,events FROM webhooks ORDER BY id"))?;
        let rows = db(s.query_map([], |r| Ok(Hook { id: r.get(0)?, name: r.get(1)?, url: r.get(2)?, enabled: r.get::<_, i64>(3)? != 0, events: r.get::<_, String>(4)?.split(',').map(String::from).collect() })))?;
        rows.map(|r| db(r)).collect()
    }
    pub fn webhook_save(&mut self, id: Option<i64>, name: &str, url: Option<&str>, enabled: bool, events: &[String]) -> Result<()> {
        let name = clean_name(name)?;
        let ev = clean_events(events)?;
        match id {
            None => {
                let url = url.ok_or("Paste an address")?;
                validate_url(url)?;
                let n: i64 = db(self.connection.query_row("SELECT count(*) FROM webhooks", [], |r| r.get(0)))?;
                if n >= MAX_HOOKS { return Err(format!("At most {MAX_HOOKS} destinations")); }
                db(self.connection.execute("INSERT INTO webhooks(name,url,enabled,events) VALUES(?1,?2,?3,?4)", params![name, url.trim(), enabled as i64, ev]))?;
            }
            Some(id) => {
                if let Some(u) = url { validate_url(u)?; db(self.connection.execute("UPDATE webhooks SET url=?1 WHERE id=?2", params![u.trim(), id]))?; }
                let n = db(self.connection.execute("UPDATE webhooks SET name=?1,enabled=?2,events=?3,updated=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?4", params![name, enabled as i64, ev, id]))?;
                if n == 0 { return Err("That destination no longer exists".into()); }
            }
        }
        Ok(())
    }
    pub fn webhook_remove(&mut self, id: i64) -> Result<()> { db(self.connection.execute("DELETE FROM webhooks WHERE id=?1", params![id]))?; Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn accepts_public_https() {
        assert_eq!(validate_url("https://hooks.example.test/api/1/abc_DEF-1").unwrap(), "hooks.example.test");
        assert_eq!(validate_url(" https://Hooks.Slack.com:443/services/T/B/x ").unwrap(), "hooks.slack.com");
    }
    #[test] fn refuses_unsafe() {
        for bad in ["http://a.example/x", "https://localhost/x", "https://127.0.0.1/x", "https://10.0.0.5/x", "https://192.168.1.1/x", "https://169.254.169.254/x", "https://100.64.0.1/x",
            "https://[::1]/x", "https://user\u{40}a.example/x", "https://a.example/x y", "https://a.example/\"x", "https://a.example/x\\y", "https://intranet/x", "https://printer.local/x",
            "https://a.example:99999/x", "https://a.example:/x", "https://-a.example/x", "https://a..example/x", "https://123.456/x", "ftp://a.example", ""] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
    }
}
