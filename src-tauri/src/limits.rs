//! Claude plan limits, as Claude Code itself reports them to a status line script (`rate_limits.five_hour` / `.seven_day`: used percentage and reset time).
//! The person adds one line to their own status line script; it writes a tiny JSON file in this app's private signals folder. The app only READS that
//! file (symlinks refused, 4 KB cap, numbers range-checked). It never reads Claude credentials or any other Claude file and never calls any server.
//! The values are the last ones Claude Code sent, so they can be stale; the page says so.
use serde::Serialize;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

const MAX_BYTES: u64 = 4096;
const FRESH_S: i64 = 2 * 3600;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Window { pub pct: f64, pub resets_at: i64 }
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Limits { pub t: i64, pub five: Option<Window>, pub seven: Option<Window> }

pub fn file() -> Result<PathBuf, String> { Ok(super::hooksignal::dir()?.join("claude-limits.json")) }
fn threshold_file() -> Result<PathBuf, String> { Ok(super::app_data_dir()?.join("limit-alert-pct")) }

fn window(v: Option<&serde_json::Value>) -> Option<Window> {
    let v = v?;
    let pct = v.get("used_percentage")?.as_f64().filter(|p| p.is_finite() && (0.0..=100.0).contains(p))?;
    let resets_at = v.get("resets_at")?.as_i64().filter(|r| *r > 0)?;
    Some(Window { pct, resets_at })
}
pub fn parse(bytes: &[u8]) -> Option<Limits> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let t = v.get("t")?.as_i64().filter(|t| *t > 0)?;
    let l = Limits { t, five: window(v.get("five")), seven: window(v.get("seven")) };
    (l.five.is_some() || l.seven.is_some()).then_some(l)
}
pub fn read() -> Option<Limits> {
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(file().ok()?).ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() { return None; }
    let mut buf = Vec::new();
    (&mut f).take(MAX_BYTES + 1).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 > MAX_BYTES { return None; }
    parse(&buf)
}
/// The alert threshold in percent (1..=100), or None when off.
pub fn threshold() -> Option<u32> {
    let s = std::fs::read_to_string(threshold_file().ok()?).ok()?;
    s.trim().parse::<u32>().ok().filter(|n| (1..=100).contains(n))
}
/// Windows at or over the threshold whose data is fresh and whose reset time has not passed; each is reported once per reset time.
pub fn crossings(l: &Limits, now: i64, th: u32, sent: &mut std::collections::HashSet<(&'static str, i64)>) -> Vec<String> {
    let mut out = Vec::new();
    if now - l.t > FRESH_S { return out; }
    for (name, label, w) in [("five", "5-hour", l.five), ("seven", "7-day", l.seven)] {
        let Some(w) = w else { continue };
        if w.resets_at <= now || w.pct < th as f64 || !sent.insert((name, w.resets_at)) { continue; }
        let mins = (w.resets_at - now) / 60;
        let when = if mins >= 2880 { format!("{} days", mins / 1440) } else if mins >= 120 { format!("{} h", mins / 60) } else { format!("{} min", mins.max(1)) };
        out.push(format!("agentVIEW: Claude plan limit, {label} window at {:.0}% (resets in {when}). This is what Claude Code last reported.", w.pct));
    }
    sent.retain(|(_, r)| *r > now);
    out
}

#[derive(Serialize)]
pub struct LimitsStatus { file: String, snippet: String, exists: bool, limits: Option<Limits>, age_s: Option<i64>, threshold: Option<u32> }

#[tauri::command]
pub fn claude_limits_status(window: tauri::WebviewWindow, threshold: Option<u32>) -> Result<LimitsStatus, String> {
    super::authorized(&window)?;
    if let Some(n) = threshold {
        let tf = threshold_file()?;
        if n == 0 { if std::fs::symlink_metadata(&tf).is_ok() { std::fs::remove_file(&tf).map_err(|_| "Could not save the setting")?; } }
        else if (1..=100).contains(&n) {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tf).map_err(|_| "Could not save the setting")?;
            f.write_all(n.to_string().as_bytes()).map_err(|_| "Could not save the setting")?;
        } else { return Err("Choose 1 to 100, or 0 to switch the alert off".into()); }
    }
    let path = file()?;
    let p = path.to_string_lossy().into_owned();
    if !super::hooksignal::snippet_safe(&p) { return Err("The signals folder path has characters the snippet cannot carry safely".into()); }
    let snippet = format!("printf '%s' \"$input\" | jq -c '{{t:(now|floor),five:.rate_limits.five_hour,seven:.rate_limits.seven_day}}' > \"{p}.tmp\" && mv \"{p}.tmp\" \"{p}\"");
    let limits = read();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    Ok(LimitsStatus { file: p, snippet, exists: path.is_file(), limits, age_s: limits.map(|l| (now - l.t).max(0)), threshold: self::threshold() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_and_range_checks() {
        let l = parse(br#"{"t":1700000000,"five":{"used_percentage":12.5,"resets_at":1700003600},"seven":null}"#).unwrap();
        assert_eq!((l.five.unwrap().pct, l.seven), (12.5, None));
        for bad in [&b""[..], b"x", br#"{"t":1,"five":{"used_percentage":101,"resets_at":5}}"#, br#"{"t":1,"five":{"used_percentage":-1,"resets_at":5}}"#, br#"{"t":1,"five":null,"seven":null}"#, br#"{"five":{"used_percentage":1,"resets_at":5}}"#, br#"{"t":1,"five":{"used_percentage":"9","resets_at":5}}"#] { assert!(parse(bad).is_none(), "{}", String::from_utf8_lossy(bad)); }
    }
    #[test] fn crossing_is_reported_once_per_reset_and_ignores_stale_or_expired() {
        let mut sent = std::collections::HashSet::new();
        let l = Limits { t: 1000, five: Some(Window { pct: 40.0, resets_at: 5000 }), seven: Some(Window { pct: 85.0, resets_at: 400_000 }) };
        let m = crossings(&l, 1100, 80, &mut sent); assert_eq!(m.len(), 1); assert!(m[0].contains("7-day") && m[0].contains("85%") && m[0].contains("4 days"), "{}", m[0]);
        assert!(crossings(&l, 1200, 80, &mut sent).is_empty());
        let next = Limits { t: 2000, seven: Some(Window { pct: 90.0, resets_at: 900_000 }), ..l }; assert_eq!(crossings(&next, 2100, 80, &mut sent).len(), 1);
        assert!(crossings(&l, 1000 + FRESH_S + 5, 10, &mut std::collections::HashSet::new()).is_empty());
        assert!(crossings(&l, 6000, 10, &mut std::collections::HashSet::new()).iter().all(|s| !s.contains("5-hour")));
    }
}
