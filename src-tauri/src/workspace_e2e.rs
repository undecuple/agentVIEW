//! Test-only GUI probes, excluded from ordinary and release builds.
use std::sync::atomic::{AtomicBool,Ordering};
use tauri::http::{Request,Response};
static DOCS_DENIED: AtomicBool = AtomicBool::new(false);

fn response(mime: &str, body: &str) -> Response<Vec<u8>> {
    Response::builder().header("Content-Type",mime).header("Cache-Control","no-store")
        .header("Content-Security-Policy","default-src 'none'; script-src 'unsafe-inline'; img-src 'self'; connect-src ipc: http://ipc.localhost")
        .body(body.as_bytes().to_vec()).unwrap()
}

pub fn intercept(req: &Request<Vec<u8>>) -> Option<Response<Vec<u8>>> {
    match req.uri().path() {
        "/__workspace-test.js" => {
            let phase = match std::env::var("AGENT_APP_TEST_PHASE").as_deref(){Ok("restart")=>"restart",Ok("corrupt")=>"corrupt",Ok("future")=>"future",_=>"first"};
            let narrow = std::env::var("AGENT_APP_TEST_WIDTH").ok().and_then(|s| s.parse::<u32>().ok()).is_some_and(|w| (480..=800).contains(&w));
            let script = format!("const TEST_PHASE='{phase}'; const TEST_NARROW={narrow};\n{}",include_str!("../../tests/workspace-e2e.js"));
            let script = if std::env::var_os("AGENT_APP_TEST_LIVE").is_some() {
                let root=std::env::var("AGENT_APP_TEST_PROJECT_ROOT").expect("fixture root");
                assert!(root.chars().all(|c| c.is_ascii_alphanumeric() || "/_- .".contains(c)));
                let script=if std::env::var_os("AGENT_APP_TEST_CANCEL").is_some(){include_str!("../../tests/live-close-e2e.js")}else{include_str!("../../tests/live-e2e.js")};
                let theme=if std::env::var("AGENT_APP_TEST_THEME").as_deref()==Ok("light"){"light"}else{"dark"};
                format!("const TEST_PHASE='{phase}'; const PROJECT_ROOT='{root}'; const TEST_LAYOUT={}; const TEST_THEME='{theme}';\n{}", std::env::var_os("AGENT_APP_TEST_LAYOUT").is_some(), script)
            } else if std::env::var_os("AGENT_APP_TEST_REVIEW").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/review-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_OVERLAP").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/overlap-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_SETTINGS").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/settings-e2e.js"))} else {script};
            Some(response("text/javascript; charset=utf-8",&script))
        },
        "/__workspace-report" => {
            let q = req.uri().query().unwrap_or(""); eprintln!("WORKSPACE_E2E {q}");
            if q=="result=pass" { std::process::exit(0); }
            if q.starts_with("result=fail") { std::process::exit(1); }
            Some(response("text/plain","ok"))
        },
        "/__workspace-cancel-status.js" => {
            let ready=std::env::var("AGENT_APP_TEST_PROJECT_ROOT").ok().is_some_and(|root|std::path::Path::new(&root).join("beta/.helper-started").is_file());
            Some(response("text/javascript",if ready{"window.__cancelReady=true;"}else{"window.__cancelReady=false;"}))
        },
        "/__workspace-docs-status.js" => Some(response("text/javascript",if DOCS_DENIED.load(Ordering::SeqCst){"window.__docsDenied=true;"}else{"window.__docsDenied=false;"})),
        _ => None,
    }
}

pub fn viewer_intercept(target: &str) -> Option<Response<Vec<u8>>> {
    if target == "/__workspace-deny" {
        return Some(response("text/html",r#"<!doctype html><title>Workspace isolation test</title><p>Testing document viewer isolation.</p><script>
        (async()=>{
          const commands=[['workspace_snapshot',{agentId:'codex-app-coord'}],['workspace_send',{agentId:'codex-app-coord',requestId:'unauthorized-0001',body:'must not persist'}],['workspace_control',{agentId:'codex-app-coord',availability:'idle'}],['workspace_tick',{}],['workspace_open_docs',{}],['live_probe',{provider:'codex'}],['live_snapshot',{}],['live_create',{provider:'codex',label:'forbidden',projectDir:'/tmp'}],['live_set_remote',{id:'forbidden',enabled:true}],['review_files',{}],['review_load',{path:'/x'}],['review_mark',{path:'/x',sha256:'0',checks:[]}],['review_clear',{path:'/x'}],['live_rename',{id:'forbidden',label:'forbidden'}],['live_start',{id:'forbidden'}],['live_read',{id:'forbidden',generation:'forbidden',offset:0}],['live_write',{id:'forbidden',generation:'forbidden',data:'forbidden'}],['live_resize',{id:'forbidden',generation:'forbidden',cols:80,rows:24}],['live_stop',{id:'forbidden',generation:'forbidden'}],['usage_snapshot',{}],['usage_refresh',{}],['app_settings_snapshot',{}],['app_settings_save',{expectedRevision:0,settings:{}}],['app_settings_reset',{expectedRevision:0}],['app_settings_apply',{}],['app_about',{}],['app_diagnostics_export',{}]];
          let denied=0;
          for(const [command,args] of commands){try{await window.__TAURI_INTERNALS__.invoke(command,args);}catch(e){denied++;}}
          new Image().src='/__workspace-deny-report?denied='+denied;
        })();</script>"#));
    }
    if let Some(q) = target.strip_prefix("/__workspace-deny-report?") {
        eprintln!("WORKSPACE_E2E docs-{q}");
        if q=="denied=27" { DOCS_DENIED.store(true,Ordering::SeqCst); }
        return Some(response("text/plain","ok"));
    }
    None
}
