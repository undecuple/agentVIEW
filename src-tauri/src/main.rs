//! Thin shell: one window on viewer://localhost served by the core library. No IPC, no listener.
#[cfg(all(feature = "e2e", not(debug_assertions)))]
compile_error!("the e2e feature is test-only and must never be built in a release profile");
use agent_app_core::{handle, Config};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{http::Response, webview::NewWindowResponse, WebviewUrl, WebviewWindowBuilder};

#[cfg(feature = "e2e")]
mod e2e;

#[cfg(feature = "e2e")]
const START: &str = "viewer://localhost/__e2e";
#[cfg(not(feature = "e2e"))]
const START: &str = "viewer://localhost/";

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

fn main() {
    let (cfg, note) = match Config::from_file(&config_path()) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e)),
    };
    let cfg = Arc::new(cfg);
    let note = Arc::new(note);
    tauri::Builder::default()
        .register_uri_scheme_protocol("viewer", move |_ctx, req| {
            let target = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
            let sfs = req.headers().get("sec-fetch-site").and_then(|v| v.to_str().ok()).map(String::from);
            #[cfg(feature = "e2e")]
            if let Some(resp) = e2e::intercept(&target, sfs.as_deref()) {
                return resp;
            }
            let mut r = handle(&cfg, req.method().as_str(), &target, sfs.as_deref());
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
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(START.parse().unwrap()))
                .title("agent-app")
                .inner_size(1000.0, 800.0)
                .on_new_window(|_url, _features| {
                    #[cfg(feature = "e2e")]
                    eprintln!("E2E new-window-denied {_url}");
                    NewWindowResponse::Deny
                })
                .on_navigation(|u| {
                    let ok = u.scheme() == "viewer" && u.host_str() == Some("localhost") && u.port().is_none() && u.username().is_empty() && u.password().is_none();
                    #[cfg(feature = "e2e")]
                    eprintln!("E2E navigation {} {u}", if ok { "allowed" } else { "blocked" });
                    ok
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run");
}
