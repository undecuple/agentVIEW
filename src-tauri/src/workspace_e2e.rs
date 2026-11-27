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
                let bg=std::env::var("AGENT_APP_TEST_BG").ok().filter(|m|m=="default"||m=="background");
                let script=if let Some(mode)=&bg{format!("const BG_MODE='{mode}';\n{}",include_str!("../../tests/background-e2e.js"))}else if std::env::var_os("AGENT_APP_TEST_CANCEL").is_some(){include_str!("../../tests/live-close-e2e.js").to_string()}else{include_str!("../../tests/live-e2e.js").to_string()};
                let theme=if std::env::var("AGENT_APP_TEST_THEME").as_deref()==Ok("light"){"light"}else{"dark"};
                format!("const TEST_PHASE='{phase}'; const PROJECT_ROOT='{root}'; const TEST_LAYOUT={}; const TEST_THEME='{theme}';\n{}", std::env::var_os("AGENT_APP_TEST_LAYOUT").is_some(), script)
            } else if std::env::var_os("AGENT_APP_TEST_DIAG").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/diag-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_HOOKSIG").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/hooksig-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_SHOTS").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/shots-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_CURL").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/webhook-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_RCLONE").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/rclone-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_GIT").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/git-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_CCFG").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/claudecfg-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_NOTES").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/notes-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_REVIEW").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/review-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_OVERLAP").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/overlap-e2e.js"))} else if std::env::var_os("AGENT_APP_TEST_SETTINGS").is_some(){format!("const TEST_PHASE='{phase}';\n{}",include_str!("../../tests/settings-e2e.js"))} else {script};
            Some(response("text/javascript; charset=utf-8",&format!("try{{if(!{}){{localStorage.setItem('agentview.welcomeSeen','1');}}}}catch(e){{}}\n{script}",std::env::var_os("AGENT_APP_TEST_WELCOME").is_some())))
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
        "/__workspace-bg-status.js" => Some(response("text/javascript",&format!("window.__bgShown={};",crate::background::SHOWN.load(Ordering::SeqCst)))),
        "/__workspace-docs-status.js" => Some(response("text/javascript",if DOCS_DENIED.load(Ordering::SeqCst){"window.__docsDenied=true;"}else{"window.__docsDenied=false;"})),
        _ => None,
    }
}

pub fn viewer_intercept(target: &str) -> Option<Response<Vec<u8>>> {
    if target == "/__workspace-deny" {
        return Some(response("text/html",r#"<!doctype html><title>Workspace isolation test</title><p>Testing document viewer isolation.</p><script>
        (async()=>{
          const commands=[['workspace_snapshot',{agentId:'codex-app-coord'}],['workspace_send',{agentId:'codex-app-coord',requestId:'unauthorized-0001',body:'must not persist'}],['workspace_control',{agentId:'codex-app-coord',availability:'idle'}],['workspace_tick',{}],['workspace_open_docs',{}],['live_probe',{provider:'codex'}],['live_tools',{}],['live_forget_tool',{provider:'codex'}],['live_snapshot',{}],['live_create',{provider:'codex',label:'forbidden',projectDir:'/tmp'}],['live_set_remote',{id:'forbidden',enabled:true}],['live_remove',{id:'forbidden'}],['docs_list',{}],['docs_check',{path:'/tmp'}],['docs_add',{path:'/tmp',expect:'/tmp'}],['docs_remove',{path:'/tmp'}],['docs_read',{path:'/tmp/x'}],['live_activity',{}],['live_stop_all',{}],['review_files',{}],['review_load',{path:'/x'}],['review_mark',{path:'/x',sha256:'0',checks:[]}],['review_clear',{path:'/x'}],['notes_list',{query:''}],['claude_config_view',{}],['git_repos',{}],['git_status',{path:'/x'}],['rclone_remotes',{}],['rclone_backup',{remote:'x',apply:false}],['webhook_status',{}],['webhook_save',{url:'https://x.example/y',enabled:true}],['webhook_clear',{}],['webhook_test',{}],['time_report',{}],['claude_hook_status',{}],['codex_hook_status',{}],['files_access_status',{}],['tasks_status',{}],['tasks_report',{}],['workflows_report',{}],['subagents_report',{sessions:[]}],['app_quit',{}],['files_access_report',{}],['claude_limits_status',{}],['session_changes',{id:'x'}],['kanban_projects',{}],['kanban_board',{project:'x'}],['kanban_alert',{enabled:true}],['queues_view',{}],['away_status',{away:true}],['mailbox_status',{name:'x'}],['mailbox_list',{}],['project_file_preview',{id:'x',name:'CLAUDE.md',content:'x'}],['project_file_write',{id:'x',name:'CLAUDE.md',content:'x',expectedHash:'',backup:true,confirm:true}],['timeline_list',{}],['attention_list',{}],['desktop_notify',{}],['claude_settings_view',{}],['claude_inventory',{}],['claude_memory_list',{}],['claude_memory_read',{key:'g|CLAUDE.md'}],['claude_sessions_list',{}],['notes_get',{id:1}],['notes_save',{id:null,shelf:'x',title:'x',body:'x',baseUpdated:null}],['notes_delete',{id:1}],['notes_revision',{id:1,revision:1}],['notes_render',{text:'x'}],['bundle_export',{}],['bundle_list',{}],['bundle_preview',{name:'x'}],['bundle_import',{name:'x',includeSettings:false}],['live_rename',{id:'forbidden',label:'forbidden'}],['live_start',{id:'forbidden'}],['live_read',{id:'forbidden',generation:'forbidden',offset:0}],['live_write',{id:'forbidden',generation:'forbidden',data:'forbidden'}],['live_resize',{id:'forbidden',generation:'forbidden',cols:80,rows:24}],['live_stop',{id:'forbidden',generation:'forbidden'}],['usage_snapshot',{}],['usage_refresh',{}],['app_settings_snapshot',{}],['app_settings_save',{expectedRevision:0,settings:{}}],['app_settings_reset',{expectedRevision:0}],['app_settings_apply',{}],['app_about',{}],['app_diagnostics_export',{}]];
          let denied=0;
          for(const [command,args] of commands){try{await window.__TAURI_INTERNALS__.invoke(command,args);}catch(e){denied++;}}
          new Image().src='/__workspace-deny-report?denied='+denied;
        })();</script>"#));
    }
    if let Some(q) = target.strip_prefix("/__workspace-deny-report?") {
        eprintln!("WORKSPACE_E2E docs-{q}");
        if q=="denied=85" { DOCS_DENIED.store(true,Ordering::SeqCst); }
        return Some(response("text/plain","ok"));
    }
    None
}
