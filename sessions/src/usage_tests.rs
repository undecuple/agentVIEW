use super::*;
use serde_json::{json,Value};
use std::os::unix::fs::symlink;
use tempfile::TempDir;

struct Fixture{_temp:TempDir,base:PathBuf,project:PathBuf,roots:UsageRoots,manager:SessionManager}
impl Fixture{
    fn new()->Self{
        let temp=tempfile::Builder::new().prefix("agentview-usage-").tempdir().unwrap();
        let base=fs::canonicalize(temp.path()).unwrap();let project=base.join("project");fs::create_dir(&project).unwrap();
        let claude=base.join("claude-home");let codex=base.join("codex-home");fs::create_dir(&claude).unwrap();fs::create_dir(&codex).unwrap();
        let mut manager=SessionManager::open_unconfigured(&base.join("metadata")).unwrap();
        for provider in[Provider::Claude,Provider::Codex]{manager.configure_provider(provider,&std::env::current_exe().unwrap(),"synthetic-usage").unwrap();}
        Self{_temp:temp,base,project,roots:UsageRoots{claude:Some(claude),codex:Some(codex)},manager}
    }
    fn record(&mut self,provider:Provider)->SessionInfo{
        let record=self.manager.create_for(provider,"Usage fixture",&self.project).unwrap();
        self.manager.connection.execute("UPDATE sessions SET attempted=1 WHERE id=?1",[&record.id]).unwrap();
        if provider==Provider::Codex{
            let native=Uuid::new_v4().to_string();self.manager.connection.execute("UPDATE sessions SET provider_session_id=?1 WHERE id=?2",params![native,record.id]).unwrap();
            return self.manager.get(&record.id).unwrap();
        }self.manager.get(&record.id).unwrap()
    }
    fn path(&mut self,record:&SessionInfo)->PathBuf{
        let native=record.provider_session_id.as_ref().unwrap();
        let path=match record.provider{
            Provider::Claude=>{
                let slug:String=self.project.to_str().unwrap().chars().map(|c|if c.is_ascii_alphanumeric(){c}else{'-'}).collect();
                self.roots.claude.as_ref().unwrap().join("projects").join(slug).join(format!("{native}.jsonl"))
            },
            Provider::Codex=>{
                let path=self.roots.codex.as_ref().unwrap().join("sessions/2026/11/22").join(format!("rollout-{native}.jsonl"));
                let tx=self.manager.connection.transaction().unwrap();usage::bind_codex(&tx,&record.id,path.to_str().unwrap()).unwrap();tx.commit().unwrap();path
            }
        };fs::create_dir_all(path.parent().unwrap()).unwrap();path
    }
    fn refresh(&mut self)->UsageSession{self.manager.refresh_usage(&self.roots).unwrap().sessions.remove(0)}
}
fn write(path:&Path,values:&[Value]){
    let data=values.iter().map(|v|serde_json::to_string(v).unwrap()+"\n").collect::<String>();
    let mut f=OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).unwrap();f.write_all(data.as_bytes()).unwrap();
}
fn claude(record:&SessionInfo,id:&str,date:&str,input:u64,read:u64,creation:u64)->Value{
    json!({"type":"assistant","sessionId":record.provider_session_id,"cwd":record.project_dir,"timestamp":format!("{date}T12:00:00Z"),
        "message":{"id":id,"content":[{"type":"text","text":"SYNTHETIC_PRIVATE_CONTENT_NEVER_STORE"}],
        "usage":{"input_tokens":input,"cache_read_input_tokens":read,"cache_creation_input_tokens":creation,"output_tokens":1}}})
}
fn meta(record:&SessionInfo)->Value{json!({"type":"session_meta","payload":{"id":record.provider_session_id,"cwd":record.project_dir}})}
fn codex(date:&str,input:u64,cache:u64,output:u64)->Value{json!({"type":"event_msg","timestamp":format!("{date}T12:00:00Z"),"payload":{"type":"token_count","info":{
    "total_token_usage":{"input_tokens":input,"cached_input_tokens":cache,"output_tokens":output,"reasoning_output_tokens":output,"total_tokens":input+output},
    "last_token_usage":{"input_tokens":99999,"output_tokens":99999}}}})}

#[test]
fn claude_coherent_revisions_deduplicate_normalize_and_do_not_store_content(){
    let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
    let rows=vec![claude(&r,"message-1","2026-11-21",10,20,3),claude(&r,"message-1","2026-11-21",12,25,4),
        claude(&r,"message-2","2026-11-22",20,30,5)];write(&path,&rows);
    let measured=f.refresh();assert_eq!(measured.status,"partial");assert_eq!(measured.totals.input_tokens,Some(96));
    assert_eq!(measured.totals.cached_input_tokens,Some(55));assert_eq!(measured.totals.cache_creation_input_tokens,Some(9));
    assert_eq!(measured.totals.output_tokens,None);assert_eq!(measured.totals.total_tokens,None);
    assert_eq!(measured.daily[0].totals.input_tokens,Some(41));assert_eq!(measured.daily[1].totals.input_tokens,Some(55));
    assert_eq!(f.refresh().totals,measured.totals);
    let metadata=fs::read(f.base.join("metadata/sessions.sqlite3")).unwrap();
    assert!(!metadata.windows(b"SYNTHETIC_PRIVATE_CONTENT_NEVER_STORE".len()).any(|w|w==b"SYNTHETIC_PRIVATE_CONTENT_NEVER_STORE"));
    assert!(!serde_json::to_string(&f.manager.usage_snapshot().unwrap()).unwrap().contains("PRIVATE_CONTENT"));
}

#[test]
fn conflicting_claude_revisions_are_not_combined_and_midnight_replay_keeps_first_day(){
    let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
    let first=claude(&r,"same-message","2026-11-21",0,100,0);write(&path,&[first.clone()]);let prior=f.refresh();
    write(&path,&[first.clone(),claude(&r,"same-message","2026-11-22",0,0,100)]);
    let invalid=f.refresh();assert_eq!(invalid.status,"unsupported");assert_eq!(invalid.totals,prior.totals);
    write(&path,&[first,claude(&r,"same-message","2026-11-22",0,100,0)]);
    let replay=f.refresh();assert_eq!(replay.totals.input_tokens,Some(100));assert_eq!(replay.daily.len(),1);assert_eq!(replay.daily[0].date,"2026-11-21");
}

#[test]
fn claude_measurement_time_belongs_to_selected_tuple_not_dominated_or_equal_replay(){
    let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
    let chosen=claude(&r,"same","2026-11-21",100,0,0);
    write(&path,&[chosen.clone(),claude(&r,"same","2026-11-22",50,0,0)]);
    let dominated=f.refresh();assert_eq!(dominated.totals.input_tokens,Some(100));
    assert_eq!(dominated.last_measured_at.as_deref(),Some("2026-11-21T12:00:00.000Z"));
    write(&path,&[chosen,claude(&r,"same","2026-11-22",100,0,0)]);
    let equal=f.refresh();assert_eq!(equal.last_measured_at,dominated.last_measured_at);
    // An earlier-timestamped but strictly dominating snapshot brings its own
    // timestamp; it must not borrow the newer timestamp of the smaller tuple.
    write(&path,&[claude(&r,"same","2026-11-22",100,0,0),claude(&r,"same","2026-11-21",150,0,0)]);
    let greater=f.refresh();assert_eq!(greater.totals.input_tokens,Some(150));
    assert_eq!(greater.last_measured_at.as_deref(),Some("2026-11-21T12:00:00.000Z"));
}

#[test]
fn codex_cumulative_usage_daily_deltas_repeat_resume_and_restart_do_not_double_count(){
    let mut f=Fixture::new();let r=f.record(Provider::Codex);let path=f.path(&r);
    let rows=vec![meta(&r),codex("2026-11-21",100,40,20),codex("2026-11-21",100,40,20),codex("2026-11-22",160,60,35)];write(&path,&rows);
    let measured=f.refresh();assert_eq!(measured.status,"ok");assert_eq!(measured.totals.input_tokens,Some(160));assert_eq!(measured.totals.total_tokens,Some(195));
    assert_eq!(measured.totals.output_tokens,Some(35));assert_eq!(measured.totals.cached_input_tokens,Some(60));assert_eq!(measured.totals.cache_creation_input_tokens,None);
    assert_eq!(measured.daily[0].totals.total_tokens,Some(120));assert_eq!(measured.daily[1].totals.total_tokens,Some(75));
    assert_eq!(f.refresh().totals,measured.totals);
    drop(f.manager);f.manager=SessionManager::open_unconfigured(&f.base.join("metadata")).unwrap();
    assert_eq!(f.manager.usage_snapshot().unwrap().sessions[0].totals,measured.totals);
    assert_eq!(f.refresh().totals,measured.totals);
    let mut duplicate_history=rows.clone();duplicate_history.extend(rows[1..].iter().cloned());write(&path,&duplicate_history);
    assert_eq!(f.refresh().totals,measured.totals);
}

#[test]
fn missing_source_truncation_rotation_and_counter_reset_keep_historical_measurements(){
    let mut f=Fixture::new();let r=f.record(Provider::Codex);let path=f.path(&r);
    write(&path,&[meta(&r),codex("2026-11-21",100,40,20)]);let measured=f.refresh();
    write(&path,&[meta(&r),codex("2026-11-21",100,40,20),codex("2026-11-22",90,30,10)]);
    let reset=f.refresh();assert_eq!(reset.status,"counter_reset");assert_eq!(reset.totals,measured.totals);
    write(&path,&[]);let truncated=f.refresh();assert_eq!(truncated.status,"source_truncated");assert_eq!(truncated.totals,measured.totals);
    write(&path,&[meta(&r),codex("2026-11-21",100,40,20)]);assert_eq!(f.refresh().totals,measured.totals);
    fs::rename(&path,path.with_extension("old")).unwrap();let missing=f.refresh();assert_eq!(missing.status,"missing");assert_eq!(missing.last_measured_at,measured.last_measured_at);assert_eq!(missing.totals,measured.totals);
    write(&path,&[meta(&r),codex("2026-11-21",100,40,20)]);let replaced=f.refresh();assert_eq!(replaced.status,"source_replaced");assert_eq!(replaced.totals,measured.totals);
}

#[test]
fn partial_last_line_is_ignored_until_committed_and_unknowns_are_not_zero(){
    let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
    write(&path,&[]);let empty=f.refresh();assert_eq!(empty.status,"no_usage");assert_eq!(empty.totals,UsageTotals::default());
    let first=claude(&r,"one","2026-11-21",10,2,0);write(&path,&[first]);
    let second=serde_json::to_string(&claude(&r,"two","2026-11-22",5,3,0)).unwrap();
    OpenOptions::new().append(true).open(&path).unwrap().write_all(second.as_bytes()).unwrap();
    let partial=f.refresh();assert_eq!(partial.totals.input_tokens,Some(12));assert_eq!(partial.status,"partial");
    OpenOptions::new().append(true).open(&path).unwrap().write_all(b"\n").unwrap();
    assert_eq!(f.refresh().totals.input_tokens,Some(20));
}

#[test]
fn missing_supported_codex_fields_are_partial_instead_of_measured_zero(){
    let mut f=Fixture::new();let r=f.record(Provider::Codex);let path=f.path(&r);
    let mut record=codex("2026-11-22",100,20,10);
    record["payload"]["info"]["total_token_usage"].as_object_mut().unwrap().remove("cached_input_tokens");
    write(&path,&[meta(&r),record]);let observed=f.refresh();
    assert_eq!(observed.status,"partial");assert_eq!(observed.totals.input_tokens,Some(100));
    assert_eq!(observed.totals.cached_input_tokens,None);assert_eq!(observed.totals.total_tokens,Some(110));
}

#[test]
fn empty_or_output_only_claude_usage_does_not_claim_day_or_poison_numeric_totals(){
    for unsupported in [json!({}),json!({"output_tokens":1})]{
        let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
        let mut first=claude(&r,"same-message","2026-11-21",0,0,0);
        first["message"]["usage"]=unsupported.clone();
        let numeric=claude(&r,"same-message","2026-11-22",12,25,4);
        write(&path,&[first.clone(),numeric.clone()]);let observed=f.refresh();
        assert_eq!(observed.daily.len(),1);
        assert_eq!(observed.daily[0].date,"2026-11-22","Unsupported usage must not establish the numeric message day");
        assert_eq!(observed.totals.input_tokens,Some(41));
        let mut unrelated=first.clone();unrelated["message"]["id"]=json!("unrelated-empty-message");
        write(&path,&[first,numeric,unrelated]);let unrelated=f.refresh();
        assert_eq!(unrelated.daily.len(),1);assert_eq!(unrelated.daily[0].date,"2026-11-22");
        assert_eq!(unrelated.totals.input_tokens,Some(41));assert_eq!(unrelated.totals.cached_input_tokens,Some(25));
        assert_eq!(unrelated.totals.cache_creation_input_tokens,Some(4));
    }
    let mut zero=Fixture::new();let r=zero.record(Provider::Claude);let path=zero.path(&r);
    write(&path,&[claude(&r,"measured-zero","2026-11-21",0,0,0)]);let measured=zero.refresh();
    assert_eq!(measured.totals.input_tokens,Some(0));assert_eq!(measured.daily.len(),1);
    assert_eq!(measured.daily[0].date,"2026-11-21");assert_eq!(measured.daily[0].totals.input_tokens,Some(0));
}

#[test]
fn empty_codex_usage_does_not_consume_first_numeric_daily_baseline(){
    let mut f=Fixture::new();let r=f.record(Provider::Codex);let path=f.path(&r);
    let mut empty=codex("2026-11-21",0,0,0);empty["payload"]["info"]["total_token_usage"]=json!({});
    write(&path,&[meta(&r),empty,codex("2026-11-22",100,40,20)]);let observed=f.refresh();
    assert_eq!(observed.totals.total_tokens,Some(120));assert_eq!(observed.daily.len(),1);
    assert_eq!(observed.daily[0].date,"2026-11-22");assert_eq!(observed.daily[0].totals.total_tokens,Some(120));
    assert_eq!(observed.daily[0].totals.input_tokens,Some(100));
    let mut zero=Fixture::new();let r=zero.record(Provider::Codex);let path=zero.path(&r);
    write(&path,&[meta(&r),codex("2026-11-21",0,0,0)]);let measured=zero.refresh();
    assert_eq!(measured.totals.total_tokens,Some(0));assert_eq!(measured.daily.len(),1);
    assert_eq!(measured.daily[0].date,"2026-11-21");assert_eq!(measured.daily[0].totals.total_tokens,Some(0));
}

#[test]
fn native_identity_symlink_hardlink_and_unbound_sources_fail_closed(){
    let mut f=Fixture::new();let r=f.record(Provider::Codex);f.path(&r);
    f.manager.connection.execute("DELETE FROM usage_sources",[]).unwrap();
    assert_eq!(f.refresh().status,"source_not_bound");
    let path=f.path(&r);let mut wrong=meta(&r);wrong["payload"]["id"]=json!(Uuid::new_v4().to_string());
    write(&path,&[wrong,codex("2026-11-21",100,20,10)]);assert_eq!(f.refresh().status,"identity_mismatch");
    fs::remove_file(&path).unwrap();let outside=f.base.join("unrelated-private.jsonl");write(&outside,&[meta(&r),codex("2026-11-21",100,20,10)]);
    symlink(&outside,&path).unwrap();assert_eq!(f.refresh().status,"read_error");fs::remove_file(&path).unwrap();
    fs::hard_link(&outside,&path).unwrap();assert_eq!(f.refresh().status,"identity_mismatch");
}

#[test]
fn malformed_or_unsafe_numeric_data_and_cancel_are_unavailable_without_erasing_history(){
    let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
    let good=claude(&r,"one","2026-11-21",10,2,1);write(&path,&[good.clone()]);let prior=f.refresh();
    let mut negative=claude(&r,"two","2026-11-22",3,2,1);negative["message"]["usage"]["input_tokens"]=json!(-5);
    write(&path,&[good.clone(),negative]);assert_eq!(f.refresh().status,"unsupported");assert_eq!(f.refresh().totals,prior.totals);
    let mut large=claude(&r,"three","2026-11-22",3,2,1);large["message"]["usage"]["input_tokens"]=json!(u64::MAX);
    write(&path,&[good,large]);assert_eq!(f.refresh().status,"unsupported");
    let cancel=Arc::new(AtomicBool::new(true));f.manager.set_cancel_flag(cancel);let cancelled=f.refresh();assert_eq!(cancelled.status,"cancelled");assert_eq!(cancelled.totals,prior.totals);
}

#[test]
fn registered_sessions_are_not_read_and_large_sources_and_day_sets_are_bounded(){
    let mut f=Fixture::new();let r=f.record(Provider::Claude);let path=f.path(&r);
    let first=claude(&r,"one","2026-11-21",10,2,1);write(&path,&[first.clone()]);
    f.manager.connection.execute("UPDATE sessions SET attempted=0 WHERE id=?1",[&r.id]).unwrap();
    assert_eq!(f.refresh().status,"not_started");
    f.manager.connection.execute("UPDATE sessions SET attempted=1 WHERE id=?1",[&r.id]).unwrap();let measured=f.refresh();
    OpenOptions::new().write(true).open(&path).unwrap().set_len(33*1024*1024).unwrap();
    let oversized=f.refresh();assert_eq!(oversized.status,"too_large");assert_eq!(oversized.totals,measured.totals);
    let start=chrono::NaiveDate::from_ymd_opt(2000,1,1).unwrap();
    let rows:Vec<Value>=(0..4097).map(|i|claude(&r,&format!("bounded-message-{i}"),&(start+chrono::Duration::days(i)).to_string(),1,0,0)).collect();
    write(&path,&rows);let too_many_days=f.refresh();assert_eq!(too_many_days.status,"too_large");assert_eq!(too_many_days.totals,measured.totals);
}
