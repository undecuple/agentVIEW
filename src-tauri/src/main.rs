//! Separate trusted workspace terminals and read-only viewer windows. No network listener.
#[cfg(all(feature = "e2e", not(debug_assertions)))]
compile_error!("the e2e feature is test-only and must never be built in a release profile");
use agent_app_core::{handle, Config};
use agent_app_workspace::{Availability, Snapshot, Store};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{http::Response, webview::NewWindowResponse, Manager, WebviewUrl, WebviewWindowBuilder};

mod docshare;
mod review;
mod notes;
mod claude_config;
mod gitview;
mod rclone;
mod notify;
mod timelog;
mod away;
mod hooksignal;
mod filesaccess;
mod tasks;
mod subagents;
mod kanban;
mod limits;
mod mailbox;
mod projwrite;
mod queues;
mod claude_settings;
mod claude_memory;
mod bundle;
#[cfg(feature = "e2e")]
mod e2e;
#[cfg(feature="workspace-e2e")]
mod workspace_e2e;

mod live;
mod settings;
mod background;

struct WorkspaceState(std::result::Result<Mutex<Store>, String>);

fn authorized(window: &tauri::WebviewWindow) -> Result<(), String> {
    let url = window.url().map_err(|_| "Unavailable window")?;
    if agent_app_workspace::trusted_window(window.label(), url.as_str()) { Ok(()) }
    else { Err("Workspace commands are unavailable in this window".into()) }
}

fn with_store<T>(window: &tauri::WebviewWindow, state: &WorkspaceState, f: impl FnOnce(&mut Store) -> Result<T,String>) -> Result<T,String> {
    authorized(window)?;
    let mut store = state.0.as_ref().map_err(Clone::clone)?.lock().map_err(|_| "Workspace storage lock failed")?;
    f(&mut store)
}

#[tauri::command]
fn workspace_snapshot(window: tauri::WebviewWindow, state: tauri::State<WorkspaceState>, agent_id: String, before: Option<i64>) -> Result<Snapshot,String> {
    with_store(&window, &state, |s| s.snapshot(&agent_id,before))
}
#[tauri::command]
fn workspace_send(window: tauri::WebviewWindow, state: tauri::State<WorkspaceState>, agent_id: String, request_id: String, body: String) -> Result<i64,String> {
    with_store(&window, &state, |s| s.send(&agent_id,&request_id,&body))
}
#[tauri::command]
fn workspace_control(window: tauri::WebviewWindow, state: tauri::State<WorkspaceState>, agent_id: String, availability: Availability) -> Result<(),String> {
    with_store(&window, &state, |s| s.control(&agent_id,availability))
}
#[tauri::command]
fn workspace_tick(window: tauri::WebviewWindow, state: tauri::State<WorkspaceState>) -> Result<usize,String> {
    with_store(&window, &state, Store::tick)
}
#[tauri::command]
fn workspace_open_docs(window: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(),String> {
    authorized(&window)?;
    if let Some(w) = app.get_webview_window("docs") { return w.set_focus().map_err(|_| "Unable to focus documents".into()); }
    #[cfg(not(feature="workspace-e2e"))]
    let start = "viewer://localhost/";
    #[cfg(feature="workspace-e2e")]
    let start = "viewer://localhost/__workspace-deny";
    build_viewer(&app, start).map_err(|_| "Unable to open documents".into())
}

pub(crate) fn app_data_dir() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    let override_dir = std::env::var_os("AGENT_APP_DATA_DIR").map(PathBuf::from);
    #[cfg(not(debug_assertions))]
    let override_dir: Option<PathBuf> = None;
    let base = override_dir.or_else(|| std::env::var_os("XDG_DATA_HOME").map(|p| PathBuf::from(p).join("agent-app")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/agent-app")))
        .ok_or("No app data directory available")?;
    if !base.is_absolute() { return Err("App data directory must be absolute".into()); }
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&base).map_err(|_| "Cannot create app data directory")?;
    let m = std::fs::symlink_metadata(&base).map_err(|_| "Cannot inspect app data directory")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 {
        return Err("App data directory must be private and owned by you".into());
    }
    Ok(base)
}

fn workspace_store() -> Result<Mutex<Store>, String> {
    Store::open(&app_data_dir()?.join("workspace.sqlite3")).map(Mutex::new)
}

fn build_viewer(app: &tauri::AppHandle, start: &str) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "docs", WebviewUrl::External(start.parse().expect("static viewer URL")))
        .title("agentVIEW · Documents").inner_size(1000.0,800.0)
        .on_new_window(|_url,_features| {
            #[cfg(feature="e2e")]
            eprintln!("E2E new-window-denied {_url}");
            NewWindowResponse::Deny
        })
        .on_navigation(|u| {
            let ok = u.scheme()=="viewer" && u.host_str()==Some("localhost") && u.port().is_none() && u.username().is_empty() && u.password().is_none();
            #[cfg(feature="e2e")]
            eprintln!("E2E navigation {} {u}",if ok {"allowed"} else {"blocked"});
            ok
        }).build()?;
    Ok(())
}

fn config_path() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(p) = std::env::var_os("AGENT_APP_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    base.join("agent-app").join("allowlist.toml")
}

/// On some Wayland sessions GTK reports no screen resolution (-1). WebKit divides by it, so page widths and the pixel ratio
/// turn negative and the layout collapses. Use the standard 96 DPI only when the session gives none; a real value is kept.
#[cfg(target_os = "linux")]
fn fix_unset_screen_dpi() {
    if let Some(screen) = gtk::gdk::Screen::default() { if screen.resolution() <= 0.0 { screen.set_resolution(96.0); } }
}
#[cfg(not(target_os = "linux"))]
fn fix_unset_screen_dpi() {}

fn workspace_width() -> f64 {
    #[cfg(feature="workspace-e2e")]
    if let Some(width) = std::env::var("AGENT_APP_TEST_WIDTH").ok().and_then(|s| s.parse::<u32>().ok()).filter(|w| (480..=1920).contains(w)) {
        return width as f64;
    }
    1440.0
}

fn main() {
    // No allowlist.toml is normal: folders are shared from the Documents page. A file that exists but is unsafe or broken is reported.
    let (cfg, note) = match std::fs::symlink_metadata(config_path()) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (agent_app_core::policy::builtin(), None),
        _ => match Config::from_file(&config_path()) {
            Ok(c) => (c, None),
            Err(e) => (agent_app_core::policy::builtin(), Some(e)),
        },
    };
    let cfg = Arc::new(cfg);
    let note = Arc::new(note);
    // A second launch hands over to a running instance and exits here, before any window is built.
    let control = match background::startup(std::env::args().any(|a| a == "--quit")) {
        background::Startup::Exit(code) => std::process::exit(code),
        background::Startup::Run(listener) => Mutex::new(listener),
    };
    tauri::Builder::default()
        .manage(WorkspaceState(workspace_store()))
        .manage(live::LiveState::default())
        .manage(cfg.clone())
        .manage(docshare::DocsNote(note.clone()))
        .on_window_event(|window,event| {
            if window.label()=="workspace" && matches!(event,tauri::WindowEvent::CloseRequested { .. }) {
                if background::wants_background(window.app_handle()) {
                    if let tauri::WindowEvent::CloseRequested { api, .. }=event { api.prevent_close(); }
                    background::hide(window.app_handle());
                    #[cfg(feature="workspace-e2e")]
                    eprintln!("WORKSPACE_E2E close=hidden");
                    return;
                }
                let accepted=live::close_workspace(window,&window.state::<live::LiveState>());
                #[cfg(feature="workspace-e2e")]
                eprintln!("WORKSPACE_E2E close={}",if accepted{"accepted"}else{"deferred"});
                if !accepted {
                    if let tauri::WindowEvent::CloseRequested { api, .. }=event { api.prevent_close(); }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![workspace_snapshot, workspace_send, workspace_control, workspace_tick, workspace_open_docs, live::live_probe, live::live_snapshot, live::live_create, live::live_set_remote, live::live_rename, live::live_remove, docshare::docs_list, docshare::docs_check, docshare::docs_add, docshare::docs_remove, docshare::docs_read, live::live_activity, live::live_stop_all, review::review_files, review::review_load, review::review_mark, review::review_clear, notes::notes_list, claude_config::claude_config_view, gitview::git_repos, gitview::git_status, rclone::rclone_remotes, rclone::rclone_backup, notify::webhook_status, notify::webhook_save, notify::webhook_clear, notify::webhook_test, timelog::time_report, hooksignal::claude_hook_status, hooksignal::codex_hook_status, filesaccess::files_access_status, filesaccess::files_access_report, tasks::tasks_status, tasks::tasks_report, tasks::workflows_report, subagents::subagents_report, background::app_quit, limits::claude_limits_status, live::session_changes, projwrite::project_file_preview, kanban::kanban_projects, kanban::kanban_board, kanban::kanban_alert, queues::queues_view, away::away_status, mailbox::mailbox_status, mailbox::mailbox_list, projwrite::project_file_write, notify::timeline_list, notify::attention_list, notify::desktop_notify, claude_settings::claude_settings_view, claude_settings::claude_inventory, claude_memory::claude_memory_list, claude_memory::claude_memory_read, claude_memory::claude_sessions_list, notes::notes_get, notes::notes_save, notes::notes_delete, notes::notes_revision, notes::notes_render, bundle::bundle_export, bundle::bundle_list, bundle::bundle_preview, bundle::bundle_import, live::live_start, live::live_read, live::live_write, live::live_resize, live::live_stop, live::usage_snapshot, live::usage_refresh, settings::app_settings_snapshot, settings::app_settings_save, settings::app_settings_reset, settings::app_settings_apply, settings::app_about, settings::app_diagnostics_export])
        .register_uri_scheme_protocol("workspace", move |_ctx,req| {
            #[cfg(feature="workspace-e2e")]
            eprintln!("WORKSPACE_REQUEST {}",req.uri());
            #[cfg(feature="workspace-e2e")]
            if req.uri().path()=="/__workspace-report" && req.uri().query()==Some("result=pass") {
                let _=live::shutdown(&_ctx.app_handle().state::<live::LiveState>());
            }
            #[cfg(feature="workspace-e2e")]
            if req.uri().path()=="/__workspace-close" {
                if let Some(window)=_ctx.app_handle().get_webview_window("workspace"){let _=window.close();}
                return Response::builder().header("Content-Type","text/plain").body(b"closing".to_vec()).unwrap();
            }
            #[cfg(feature="workspace-e2e")]
            if let Some(r) = workspace_e2e::intercept(&req) { return r; }
            let valid = req.uri().host()==Some("localhost") && req.uri().port().is_none() && req.uri().query().is_none();
            let asset = if valid && req.method()=="GET" { agent_app_workspace::asset(req.uri().path()) } else { None };
            let (mime,body) = asset.unwrap_or(("text/plain; charset=utf-8","Not found"));
            #[cfg(feature="workspace-e2e")]
            let body = if req.uri().path()=="/" { format!("{body}<script src=\"/__workspace-test.js\"></script>") } else { body.to_string() };
            Response::builder().status(if asset.is_some(){200}else{404})
                .header("Content-Type",mime).header("Content-Security-Policy",agent_app_workspace::CSP)
                .header("Cache-Control","no-store").header("X-Content-Type-Options","nosniff")
                .header("Referrer-Policy","no-referrer").header("Cross-Origin-Resource-Policy","same-origin")
                .body(body.as_bytes().to_vec()).expect("static workspace response")
        })
        .register_uri_scheme_protocol("viewer", move |_ctx, req| {
            let target = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
            let sfs = req.headers().get("sec-fetch-site").and_then(|v| v.to_str().ok()).map(String::from);
            #[cfg(feature = "e2e")]
            if let Some(resp) = e2e::intercept(&target, sfs.as_deref()) {
                return resp;
            }
            let mut r = handle(&docshare::viewer_config(&cfg), req.method().as_str(), &target, sfs.as_deref());
            if let Some(n) = note.as_ref() {
                if r.status == 200 && r.headers.iter().any(|(_, v)| v.starts_with("text/html")) {
                    let banner = format!("<p><b>Config not loaded, nothing is readable:</b> {}</p>", n.replace('<', "&lt;"));
                    r.body.extend(banner.into_bytes());
                }
            }
            let mut b = Response::builder().status(r.status);
            for (k, v) in &r.headers {
                b = b.header(k.as_str(), v.as_str());
            }
            b.body(r.body).unwrap_or_else(|_| {
                Response::builder().status(500).body(b"internal error".to_vec()).expect("static response")
            })
        })
        .setup(move |app| {
            fix_unset_screen_dpi();
            #[cfg(all(feature="e2e",not(feature="workspace-e2e")))]
            build_viewer(app.handle(), "viewer://localhost/__e2e")?;
            #[cfg(any(not(feature="e2e"),feature="workspace-e2e"))]
            WebviewWindowBuilder::new(app,"workspace",WebviewUrl::External("workspace://localhost/".parse().unwrap()))
                .title("agentVIEW · Agent workspace").inner_size(workspace_width(),1000.0)
                .on_new_window(|_,_| NewWindowResponse::Deny)
                .on_navigation(|u| agent_app_workspace::trusted_window("workspace",u.as_str()))
                .build()?;
            notify::start_monitor(app.handle().clone());
            if let Some(listener) = control.lock().ok().and_then(|mut l| l.take()) { background::start_control(app.handle().clone(), listener); }
            background::start_watcher(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build")
        .run(|_app, event| { if matches!(event, tauri::RunEvent::Exit) { background::cleanup(); } });
}
