//! App-owned notes library commands. Pages live in the app's database; no repository or shared file is read or written.
use agent_app_core::markdown;
use agent_app_workspace::notes::{Note, NoteMeta};

#[tauri::command]
pub fn notes_list(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, query: String) -> Result<Vec<NoteMeta>, String> {
    if query.len() > 200 { return Err("Search text is too long".into()); }
    super::with_store(&window, &state, |s| s.notes_list(&query))
}
#[tauri::command]
pub fn notes_get(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, id: i64) -> Result<Note, String> {
    super::with_store(&window, &state, |s| s.note_get(id))
}
#[tauri::command]
pub fn notes_save(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, id: Option<i64>, shelf: String, title: String, body: String, base_updated: Option<String>) -> Result<Note, String> {
    super::with_store(&window, &state, |s| s.note_save(id, &shelf, &title, &body, base_updated.as_deref()))
}
#[tauri::command]
pub fn notes_delete(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, id: i64) -> Result<(), String> {
    super::with_store(&window, &state, |s| s.note_delete(id))
}
#[tauri::command]
pub fn notes_revision(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, id: i64, revision: i64) -> Result<String, String> {
    super::with_store(&window, &state, |s| s.note_revision(id, revision))
}
/// Preview: the same safe renderer as the viewer (escape first, links as text, no HTML, no images, no scripts).
#[tauri::command]
pub fn notes_render(window: tauri::WebviewWindow, text: String) -> Result<String, String> {
    super::authorized(&window)?;
    if text.len() > agent_app_workspace::notes::MAX_BODY { return Err("Page is larger than 256 KB".into()); }
    Ok(markdown::render(&agent_app_core::policy::redact(&text)))
}
