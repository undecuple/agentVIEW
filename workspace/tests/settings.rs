use agent_app_workspace::{AppSettings,QuickPrompt,SettingsRecovery,Store};
use rusqlite::{params,Connection};
use serde_json::json;
use std::path::Path;
use std::sync::{Arc,Barrier};

fn fixture()->(tempfile::TempDir,std::path::PathBuf,Store){
    let temp=tempfile::tempdir().unwrap();let path=temp.path().join("workspace.sqlite3");let store=Store::open(&path).unwrap();(temp,path,store)
}
fn seed(path:&Path,revision:u64,version:i64,raw:&str){
    Connection::open(path).unwrap().execute("INSERT OR REPLACE INTO app_settings VALUES(1,?1,?2,?3)",params![revision,version,raw]).unwrap();
}
fn raw(path:&Path)->(u64,i64,String){
    Connection::open(path).unwrap().query_row("SELECT revision,format_version,settings_json FROM app_settings WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}

#[test]
fn absent_settings_are_defaults_without_autowrite_and_unicode_roundtrips_after_reopen(){
    let (_temp,path,mut store)=fixture();let default=store.settings_snapshot().unwrap();
    assert_eq!(default.settings,AppSettings::default());assert_eq!(default.defaults,AppSettings::default());
    assert_eq!(default.revision,0);assert!(default.writable);assert_eq!(default.recovery,None);
    assert_eq!(Connection::open(&path).unwrap().query_row("SELECT count(*) FROM app_settings",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    let mut changed=default.settings;changed.theme="light".into();changed.ui_scale_percent=125;changed.terminal_font_size=16;
    changed.quick_prompts=vec![QuickPrompt{id:"review_2".into(),label:"Réviser 漢字 👩‍💻".into(),prompt:"Review café and 日本語, then ask for approval 👩‍💻.".into()}];
    let saved=store.settings_save(0,changed.clone()).unwrap();assert_eq!(saved.settings,changed);assert_eq!(saved.revision,1);
    drop(store);let store=Store::open(&path).unwrap();assert_eq!(store.settings_snapshot().unwrap().settings,changed);
}

#[test]
fn every_closed_choice_and_numeric_limit_is_validated_before_mutation(){
    let (_temp,_path,mut store)=fixture();let defaults=AppSettings::default();
    let mut invalid=Vec::new();
    for value in [79,151]{let mut s=defaults.clone();s.ui_scale_percent=value;invalid.push(s);}
    for value in [9,25]{let mut s=defaults.clone();s.terminal_font_size=value;invalid.push(s);}
    let mut s=defaults.clone();s.terminal_scrollback=10001;invalid.push(s);
    for value in [1,59,301,901]{let mut s=defaults.clone();s.usage_refresh_seconds=value;invalid.push(s);}
    for field in ["theme","density","default_page","preferred_provider","hierarchy_provider","hierarchy_project","hierarchy_status","hierarchy_view","live_provider_filter","usage_provider","usage_period","close_behavior"]{
        let mut value=serde_json::to_value(&defaults).unwrap();value[field]=json!("unrecognized-private-value");invalid.push(serde_json::from_value(value).unwrap());
    }
    for setting in invalid{let error=store.settings_save(0,setting).unwrap_err();assert!(!error.contains("unrecognized-private-value"));assert_eq!(store.settings_snapshot().unwrap().revision,0);}
    for (scale,font,scroll,refresh)in[(80,10,0,0),(150,24,10000,900)]{
        let mut settings=defaults.clone();settings.ui_scale_percent=scale;settings.terminal_font_size=font;settings.terminal_scrollback=scroll;settings.usage_refresh_seconds=refresh;
        let rev=store.settings_snapshot().unwrap().revision;store.settings_save(rev,settings).unwrap();
    }
}

#[test]
fn quick_prompts_reject_control_injection_and_allow_empty_lists_and_bounded_unicode(){
    let (_temp,_path,mut store)=fixture();
    for character in ['\0','\t','\n','\r','\u{1b}','\u{7f}','\u{85}','\u{9b}','\u{2028}','\u{2029}','\u{202e}','\u{feff}','\u{200b}']{
        for label in[false,true]{
            let mut settings=AppSettings::default();let bad=format!("before{character}after");
            if label{settings.quick_prompts[0].label=bad;}else{settings.quick_prompts[0].prompt=bad;}
            assert!(store.settings_save(0,settings).is_err());
        }
    }
    for id in["","Upper","1first","a space","非ascii"]{
        let mut settings=AppSettings::default();settings.quick_prompts[0].id=id.into();assert!(store.settings_save(0,settings).is_err());
    }
    let mut settings=AppSettings::default();settings.quick_prompts.push(settings.quick_prompts[0].clone());assert!(store.settings_save(0,settings).is_err());
    let mut settings=AppSettings::default();settings.quick_prompts[0].prompt=" ".into();assert!(store.settings_save(0,settings).is_err());
    let mut settings=AppSettings::default();settings.quick_prompts[0].label="x".repeat(49);assert!(store.settings_save(0,settings).is_err());
    let mut settings=AppSettings::default();settings.quick_prompts[0].prompt="🚀".repeat(4001);assert!(store.settings_save(0,settings).is_err());
    let mut settings=AppSettings::default();settings.quick_prompts.clear();let saved=store.settings_save(0,settings).unwrap();assert!(saved.settings.quick_prompts.is_empty());
    let mut maximum=saved.settings;maximum.quick_prompts=(0..8).map(|i|QuickPrompt{id:format!("prompt_{i}"),label:"🚀".repeat(48),prompt:"🚀".repeat(4000)}).collect();
    let saved=store.settings_save(saved.revision,maximum).unwrap();assert_eq!(saved.settings.quick_prompts.len(),8);
    let many=|n:usize|(0..n).map(|i|QuickPrompt{id:format!("p_{i}"),label:format!("Prompt {i}"),prompt:"Summarize the current project status.".into()}).collect::<Vec<_>>();
    let mut s=saved.settings.clone();s.quick_prompts=many(24);let saved=store.settings_save(saved.revision,s).unwrap();assert_eq!(saved.settings.quick_prompts.len(),24);
    let mut s=saved.settings.clone();s.quick_prompts=many(25);assert!(store.settings_save(saved.revision,s).is_err(),"more than 24 presets are refused");
}

#[test]
fn strict_json_rejects_unknown_fields_wrong_types_and_provider_settings(){
    let mut value=serde_json::to_value(AppSettings::default()).unwrap();value["provider_api_key"]=json!("PRIVATE_SYNTHETIC_VALUE");
    assert!(serde_json::from_value::<AppSettings>(value).is_err());
    let mut value=serde_json::to_value(AppSettings::default()).unwrap();value["terminal_font_size"]=json!(-1);assert!(serde_json::from_value::<AppSettings>(value).is_err());
    let mut value=serde_json::to_value(AppSettings::default()).unwrap();value["remember_filters"]=json!("true");assert!(serde_json::from_value::<AppSettings>(value).is_err());
    let mut value=serde_json::to_value(AppSettings::default()).unwrap();value["quick_prompts"][0]["command"]=json!("unrecognized");assert!(serde_json::from_value::<AppSettings>(value).is_err());
}

#[test]
fn stale_form_and_filter_saves_cannot_overwrite_newer_preferences_or_reset(){
    let (_temp,path,mut form)=fixture();let mut filter=Store::open(&path).unwrap();
    let stale=filter.settings_snapshot().unwrap();let mut newest=AppSettings::default();newest.theme="light".into();newest.terminal_font_size=19;
    let saved=form.settings_save(0,newest.clone()).unwrap();let mut old_filter=stale.settings;old_filter.hierarchy_provider="codex".into();
    assert!(filter.settings_save(stale.revision,old_filter).unwrap_err().contains("reload"));assert!(filter.settings_reset(stale.revision).is_err());
    assert_eq!(form.settings_snapshot().unwrap().settings,newest);
    let fresh=filter.settings_snapshot().unwrap();let mut merged=fresh.settings;merged.hierarchy_provider="codex".into();
    let updated=filter.settings_save(fresh.revision,merged).unwrap();assert_eq!(updated.revision,saved.revision+1);assert_eq!(updated.settings.terminal_font_size,19);
    assert_eq!(updated.settings.theme,"light");assert_eq!(updated.settings.hierarchy_provider,"codex");
}

#[test]
fn concurrent_first_saves_have_one_winner(){
    let (_temp,path,_store)=fixture();let gate=Arc::new(Barrier::new(2));
    let handles:Vec<_>=(0..2).map(|i|{let path=path.clone();let gate=Arc::clone(&gate);std::thread::spawn(move||{
        let mut store=Store::open(&path).unwrap();let snapshot=store.settings_snapshot().unwrap();let mut settings=snapshot.settings;settings.terminal_font_size=14+i;
        gate.wait();store.settings_save(snapshot.revision,settings)
    })}).collect();
    let results:Vec<_>=handles.into_iter().map(|h|h.join().unwrap()).collect();assert_eq!(results.iter().filter(|r|r.is_ok()).count(),1);
    assert_eq!(Store::open(&path).unwrap().settings_snapshot().unwrap().revision,1);
}

#[test]
fn corrupt_supported_preferences_need_explicit_revision_bound_reset_and_keep_original_bytes(){
    let (_temp,path,mut store)=fixture();let malformed="{PRIVATE_SYNTHETIC_VALUE: broken";seed(&path,7,1,malformed);
    let snapshot=store.settings_snapshot().unwrap();assert_eq!(snapshot.settings,AppSettings::default());assert_eq!(snapshot.recovery,Some(SettingsRecovery::CorruptJson));assert!(snapshot.writable);
    let error=store.settings_save(7,AppSettings::default()).unwrap_err();assert!(!error.contains("PRIVATE_SYNTHETIC_VALUE"));assert_eq!(raw(&path).2,malformed);
    assert!(store.settings_reset(6).is_err());assert_eq!(raw(&path).2,malformed);
    let repaired=store.settings_reset(7).unwrap();assert_eq!(repaired.revision,8);assert_eq!(repaired.recovery,None);assert_eq!(repaired.settings,AppSettings::default());
    let archived:Vec<u8>=Connection::open(&path).unwrap().query_row("SELECT settings_json FROM app_settings_recovery WHERE revision=7",[],|r|r.get(0)).unwrap();assert_eq!(archived,malformed.as_bytes());
}

#[test]
fn newer_formats_and_oversized_preferences_are_never_replaced(){
    let (_temp,path,mut store)=fixture();
    for (format,value,reason)in[(2,"future PRIVATE_SYNTHETIC_VALUE".to_string(),SettingsRecovery::NewerVersion),
        (1,json!({"version":2,"settings":{"future":"PRIVATE_SYNTHETIC_VALUE"}}).to_string(),SettingsRecovery::NewerVersion),
        (1,"x".repeat(256*1024+1),SettingsRecovery::Oversized)]{
        seed(&path,9,format,&value);let before=raw(&path);let snapshot=store.settings_snapshot().unwrap();
        assert!(!snapshot.writable);assert_eq!(snapshot.recovery,Some(reason));assert_eq!(snapshot.settings,AppSettings::default());
        assert!(store.settings_save(9,AppSettings::default()).is_err());assert!(store.settings_reset(9).is_err());assert_eq!(raw(&path),before);
    }
}

#[test]
fn duplicate_keys_require_recovery_and_any_future_version_stays_read_only(){
    let defaults=serde_json::to_string(&AppSettings::default()).unwrap();
    let (_temp,path,mut store)=fixture();
    let future=format!("{{\"version\":2,\"version\":1,\"settings\":{defaults}}}");
    seed(&path,3,1,&future);let snapshot=store.settings_snapshot().unwrap();
    assert_eq!(snapshot.recovery,Some(SettingsRecovery::NewerVersion));assert!(!snapshot.writable);
    assert!(store.settings_save(3,AppSettings::default()).is_err());assert!(store.settings_reset(3).is_err());assert_eq!(raw(&path).2,future);
    for raw_json in[
        format!("{{\"version\":1,\"version\":1,\"settings\":{defaults}}}"),
        format!("{{\"version\":1,\"settings\":{}}}",defaults.replacen("\"theme\":\"dark\"","\"theme\":\"dark\",\"theme\":\"light\"",1)),
    ]{
        let (_temp,path,mut store)=fixture();seed(&path,3,1,&raw_json);
        let snapshot=store.settings_snapshot().unwrap();assert_eq!(snapshot.recovery,Some(SettingsRecovery::InvalidSettings));
        assert!(store.settings_save(3,AppSettings::default()).is_err());assert_eq!(raw(&path).2,raw_json);
        store.settings_reset(3).unwrap();
        let archived:Vec<u8>=Connection::open(&path).unwrap().query_row("SELECT settings_json FROM app_settings_recovery WHERE revision=3",[],|r|r.get(0)).unwrap();
        assert_eq!(archived,raw_json.as_bytes());
    }
}

#[test]
fn invalid_current_settings_recover_without_exposing_raw_values_and_workspace_data_survives(){
    let (_temp,path,mut store)=fixture();store.send("codex-app-coord","settings-request-1","Synthetic retained message").unwrap();
    let mut settings=serde_json::to_value(AppSettings::default()).unwrap();settings["preferred_provider"]=json!("PRIVATE_SYNTHETIC_VALUE");
    seed(&path,1,1,&json!({"version":1,"settings":settings}).to_string());let snapshot=store.settings_snapshot().unwrap();
    assert_eq!(snapshot.recovery,Some(SettingsRecovery::InvalidSettings));assert!(!serde_json::to_string(&snapshot).unwrap().contains("PRIVATE_SYNTHETIC_VALUE"));
    store.settings_reset(1).unwrap();assert_eq!(store.snapshot("codex-app-coord",None).unwrap().messages.len(),1);
    assert_eq!(store.agents().unwrap().len(),14);
}

#[test]
fn old_workspace_migrates_settings_tables_without_creating_a_preference_row(){
    let (_temp,path,store)=fixture();drop(store);
    Connection::open(&path).unwrap().execute_batch("DROP TABLE app_settings; DROP TABLE app_settings_recovery; PRAGMA user_version=1;").unwrap();
    let store=Store::open(&path).unwrap();assert_eq!(store.settings_snapshot().unwrap().revision,0);assert_eq!(store.agents().unwrap().len(),14);
    assert_eq!(Connection::open(&path).unwrap().query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),2);
}

#[test]
fn close_behavior_defaults_to_stop_validates_and_old_records_without_it_still_load(){
    let (_temp,path,mut store)=fixture();
    assert_eq!(AppSettings::default().close_behavior,"stop");
    assert_eq!(store.settings_snapshot().unwrap().settings.close_behavior,"stop");
    // A record saved before the preference existed has no such key: it loads with the safe default, not as damaged.
    let mut old=serde_json::to_value(AppSettings::default()).unwrap();old.as_object_mut().unwrap().remove("close_behavior");
    seed(&path,1,1,&json!({"version":1,"settings":old}).to_string());
    let loaded=store.settings_snapshot().unwrap();assert_eq!(loaded.recovery,None);assert_eq!(loaded.settings.close_behavior,"stop");
    let mut background=loaded.settings.clone();background.close_behavior="background".into();
    assert_eq!(store.settings_save(1,background).unwrap().settings.close_behavior,"background");
    let mut bad=AppSettings::default();bad.close_behavior="hide".into();
    assert!(store.settings_save(2,bad).is_err());
    assert_eq!(raw(&path).0,2);
}
