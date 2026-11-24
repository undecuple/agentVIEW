//! App preferences and a bounded, deliberately projected diagnostic export.
use agent_app_workspace::settings::{AppSettings,SettingsSnapshot};
use serde::Serialize;
use serde_json::{json,Value};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt,MetadataExt,OpenOptionsExt};

#[tauri::command]
pub fn app_settings_snapshot(window:tauri::WebviewWindow,state:tauri::State<'_,super::WorkspaceState>)->Result<SettingsSnapshot,String>{
    super::with_store(&window,&state,|s|s.settings_snapshot())
}
#[tauri::command]
pub fn app_settings_save(window:tauri::WebviewWindow,state:tauri::State<'_,super::WorkspaceState>,expected_revision:u64,settings:AppSettings)->Result<SettingsSnapshot,String>{
    super::with_store(&window,&state,|s|s.settings_save(expected_revision,settings))
}
#[tauri::command]
pub fn app_settings_reset(window:tauri::WebviewWindow,state:tauri::State<'_,super::WorkspaceState>,expected_revision:u64)->Result<SettingsSnapshot,String>{
    super::with_store(&window,&state,|s|s.settings_reset(expected_revision))
}
#[tauri::command]
pub fn app_settings_apply(window:tauri::WebviewWindow,state:tauri::State<'_,super::WorkspaceState>)->Result<u16,String>{
    let settings=super::with_store(&window,&state,|s|s.settings_snapshot())?.settings;
    window.set_zoom(f64::from(settings.ui_scale_percent)/100.0).map_err(|_|"Interface scale could not be applied".to_string())?;Ok(settings.ui_scale_percent)
}

#[derive(Serialize)]
pub struct About { app_version:&'static str, build:&'static str, platform:&'static str, data_directory:String, allowlist_file:String, saved_cli_versions:Vec<(String,String)>, report:Value }

// Only conventional three-part numeric versions enter diagnostics. Arbitrary
// wrapper --version text may be displayed by Sessions, but is never exported.
fn numeric_version(value:&str)->Option<String>{
    value.split_whitespace().find(|token|{
        let parts:Vec<_>=token.split('.').collect();
        parts.len()==3&&parts.iter().all(|p|!p.is_empty()&&p.len()<=3&&p.bytes().all(|b|b.is_ascii_digit()))
    }).map(str::to_string)
}
async fn about(window:tauri::WebviewWindow,state:&super::WorkspaceState,live:&super::live::LiveState)->Result<About,String>{
    let prefs=super::with_store(&window,state,|s|s.settings_snapshot())?;
    let versions=super::live::saved_versions(window,live).await?;
    let mut saved_cli_versions:Vec<(String,String)>=versions.into_iter().filter_map(|(p,v)|numeric_version(&v).map(|v|(p,v))).collect();
    saved_cli_versions.sort();saved_cli_versions.dedup();
    let s=&prefs.settings;
    let report=json!({"report_format":1,"app_version":env!("CARGO_PKG_VERSION"),"platform":"linux",
        "preferences_revision":prefs.revision,"preferences_recovery":prefs.recovery,
        "appearance":{"theme":s.theme,"scale_percent":s.ui_scale_percent,"density":s.density},
        "terminal":{"font_size":s.terminal_font_size,"scrollback_lines":s.terminal_scrollback,"cursor_blink":s.terminal_cursor_blink},
        "usage_refresh_seconds":s.usage_refresh_seconds,"saved_cli_versions":saved_cli_versions});
    Ok(About{app_version:env!("CARGO_PKG_VERSION"),build:env!("AGENT_APP_GIT"),platform:"Linux",data_directory:super::app_data_dir()?.to_string_lossy().into_owned(),
        allowlist_file:super::config_path().to_string_lossy().into_owned(),saved_cli_versions,report})
}
#[tauri::command]
pub async fn app_about(window:tauri::WebviewWindow,state:tauri::State<'_,super::WorkspaceState>,live:tauri::State<'_,super::live::LiveState>)->Result<About,String>{about(window,&state,&live).await}

#[derive(Serialize)]
pub struct Export { path:String, report:Value }
#[tauri::command]
pub async fn app_diagnostics_export(window:tauri::WebviewWindow,state:tauri::State<'_,super::WorkspaceState>,live:tauri::State<'_,super::live::LiveState>)->Result<Export,String>{
    super::authorized(&window)?;
    let info=about(window,&state,&live).await?;
    let bytes=serde_json::to_vec_pretty(&info.report).map_err(|_|"Cannot encode diagnostic report")?;
    if bytes.len()>16*1024{return Err("Diagnostic report exceeds its size limit".into());}
    let base=super::app_data_dir()?;
    if std::fs::canonicalize(&base).map_err(|_|"Data directory unavailable")?!=base{return Err("Diagnostic storage cannot use symlinks".into());}
    let directory=base.join("exports");
    match std::fs::DirBuilder::new().mode(0o700).create(&directory){Ok(())=>(),Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>(),Err(_)=>return Err("Cannot create diagnostic directory".into())}
    let meta=std::fs::symlink_metadata(&directory).map_err(|_|"Cannot inspect diagnostic directory")?;
    if !meta.is_dir()||meta.uid()!=agent_app_core::sys::euid()||meta.mode()&0o077!=0||std::fs::canonicalize(&directory).map_err(|_|"Diagnostic directory unavailable")?!=directory{return Err("Diagnostic directory must be private and owned by you".into());}
    let nonce=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"Clock unavailable")?.as_nanos();
    let path=directory.join(format!("agentview-diagnostics-{nonce}-{}.json",std::process::id()));
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|_|"Cannot create diagnostic report")?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|_|"Cannot finish diagnostic report")?;
    Ok(Export{path:path.to_string_lossy().into_owned(),report:info.report})
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn diagnostic_versions_exclude_unstructured_text(){
        assert_eq!(numeric_version("codex-cli 0.165.0"),Some("0.165.0".into()));
        assert_eq!(numeric_version("2.1.316 (Claude Code)"),Some("2.1.316".into()));
        for v in["secret-token","file=/home/private/2.1.316", "12345.12345.12345"]{
            assert_eq!(numeric_version(v),None);
        }
    }
}
