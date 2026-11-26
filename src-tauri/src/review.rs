//! Manual review of draft documents: rendered Markdown plus a PASS (manual review) label bound to the file hash.
//! The label is the user's own note, stored in the app's database. It is not an approval and grants nothing.
use agent_app_core::{markdown, policy, Config};
use agent_app_workspace::reviews::{Review, REQUIRED_CHECKS};
use serde::Serialize;
use std::sync::Arc;

const CAP: usize = 256 * 1024;
const REFUSED: &str = "That file is not available for review";

#[derive(Serialize)]
pub struct ReviewFile { path: String, name: String, status: &'static str, reviewed_at: Option<String> }
#[derive(Serialize)]
pub struct ReviewView {
    path: String, name: String, html: String, sha256: String,
    review: Option<Review>, stale: bool, required_checks: Vec<&'static str>,
}

fn load(window: &tauri::WebviewWindow, cfg: &Config, state: &super::WorkspaceState, path: &str) -> Result<ReviewView, String> {
    let (real, bytes, more) = policy::read_bytes(path, cfg, CAP).map_err(|_| REFUSED.to_string())?;
    if more { return Err("This file is larger than 256 KB; it cannot be reviewed here".into()); }
    let key = real.to_string_lossy().into_owned();
    let sha = markdown::sha256_hex(&bytes);
    let text = policy::redact(&String::from_utf8_lossy(&bytes));
    let review = super::with_store(window, state, |s| s.review_get(&key))?;
    let stale = review.as_ref().is_some_and(|r| r.sha256 != sha);
    Ok(ReviewView { name: policy::short(&key), path: key, html: markdown::render(&text), sha256: sha, review, stale, required_checks: REQUIRED_CHECKS.to_vec() })
}

/// The review queue: every allow-listed draft with its state. Waiting items (pending, then stale) come first.
#[tauri::command]
pub fn review_files(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>, state: tauri::State<super::WorkspaceState>) -> Result<Vec<ReviewFile>, String> {
    super::authorized(&window)?;
    let cfg = super::docshare::viewer_config(&cfg); // allowlist.toml plus the folders shared on the Documents page
    let mut queue = Vec::new();
    for p in policy::listing(&cfg).into_iter().take(500) {
        if !p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt")) { continue; }
        let path = p.to_string_lossy().into_owned();
        let Ok((real, bytes, more)) = policy::read_bytes(&path, &cfg, CAP) else { continue };
        if more { continue; }
        let key = real.to_string_lossy().into_owned();
        let sha = markdown::sha256_hex(&bytes);
        let review = super::with_store(&window, &state, |s| s.review_get(&key))?;
        let (status, reviewed_at) = match review {
            None => ("pending", None),
            Some(r) if r.sha256 != sha => ("stale", Some(r.reviewed_at)),
            Some(r) => ("pass", Some(r.reviewed_at)),
        };
        queue.push(ReviewFile { name: policy::short(&key), path: key, status, reviewed_at });
    }
    let rank = |s: &str| match s { "pending" => 0, "stale" => 1, _ => 2 };
    queue.sort_by(|a, b| rank(a.status).cmp(&rank(b.status)).then_with(|| a.name.cmp(&b.name)));
    Ok(queue)
}

#[tauri::command]
pub fn review_load(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>, state: tauri::State<super::WorkspaceState>, path: String) -> Result<ReviewView, String> {
    super::authorized(&window)?;
    let cfg = super::docshare::viewer_config(&cfg); // allowlist.toml plus the folders shared on the Documents page
    load(&window, &cfg, &state, &path)
}

/// Saves PASS (manual review) for the exact bytes the reader saw. A changed file must be reloaded and read again.
#[tauri::command]
pub fn review_mark(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>, state: tauri::State<super::WorkspaceState>, path: String, sha256: String, checks: Vec<String>) -> Result<ReviewView, String> {
    super::authorized(&window)?;
    let cfg = super::docshare::viewer_config(&cfg); // allowlist.toml plus the folders shared on the Documents page
    let (real, bytes, more) = policy::read_bytes(&path, &cfg, CAP).map_err(|_| REFUSED.to_string())?;
    if more { return Err("This file is larger than 256 KB; it cannot be reviewed here".into()); }
    if markdown::sha256_hex(&bytes) != sha256.to_ascii_lowercase() { return Err("The file changed after it was shown. Reload it and review it again.".into()); }
    let key = real.to_string_lossy().into_owned();
    super::with_store(&window, &state, |s| s.review_mark(&key, &sha256, &checks))?;
    load(&window, &cfg, &state, &key)
}

#[tauri::command]
pub fn review_clear(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>, state: tauri::State<super::WorkspaceState>, path: String) -> Result<ReviewView, String> {
    super::authorized(&window)?;
    let cfg = super::docshare::viewer_config(&cfg); // allowlist.toml plus the folders shared on the Documents page
    let (real, _, _) = policy::read_bytes(&path, &cfg, 1).map_err(|_| REFUSED.to_string())?;
    let key = real.to_string_lossy().into_owned();
    super::with_store(&window, &state, |s| s.review_clear(&key))?;
    load(&window, &cfg, &state, &key)
}
