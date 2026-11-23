use agent_app_workspace::{asset, trusted_window, validate_hierarchy, Availability, Store};
use rusqlite::Connection;
use std::sync::{Arc, Barrier};
const AGENT: &str = "codex-app-coord";

#[test]
fn restart_idempotency_and_busy_queue() {
    let t = tempfile::tempdir().unwrap(); let path = t.path().join("state.db");
    let mut s = Store::open(&path).unwrap();
    s.control(AGENT,Availability::Busy).unwrap();
    let id = s.send(AGENT,"request-0001","hello").unwrap();
    assert_eq!(s.tick().unwrap(),0);
    assert_eq!(s.send(AGENT,"request-0001","hello").unwrap(),id);
    assert!(s.send(AGENT,"request-0001","other content").is_err());
    assert!(s.send("claude-app-coord","request-0001","hello").is_err());
    drop(s);
    let mut s = Store::open(&path).unwrap();
    let before = s.snapshot(AGENT,None).unwrap();
    assert_eq!(before.messages.len(),1); assert_eq!(before.messages[0].id,id);
    assert_eq!(before.agents.iter().find(|a| a.id==AGENT).unwrap().availability,"busy");
    s.control(AGENT,Availability::Paused).unwrap(); assert_eq!(s.tick().unwrap(),0);
    s.control(AGENT,Availability::Idle).unwrap(); assert_eq!(s.tick().unwrap(),1);
    assert_eq!(s.tick().unwrap(),0); drop(s);
    let mut s = Store::open(&path).unwrap(); let after = s.snapshot(AGENT,None).unwrap();
    assert_eq!(after.messages.len(),2); assert_eq!(after.messages[0].state,"delivered");
    assert_eq!(after.messages[1].reply_to,Some(id)); assert_eq!(after.messages[1].sender,"simulator");
    assert_eq!(s.send(AGENT,"request-0001","hello").unwrap(),id);
    assert_eq!(s.tick().unwrap(),0);
}

#[test]
fn two_providers_have_valid_explicit_parentage() {
    let t=tempfile::tempdir().unwrap(); let s=Store::open(&t.path().join("state.db")).unwrap();
    let roster=s.agents().unwrap(); validate_hierarchy(&roster).unwrap();
    assert_eq!(roster.iter().filter(|a|a.role=="intake").count(),2);
    assert_eq!(roster.iter().filter(|a|a.role=="coordinator").count(),4);
    assert_eq!(roster.iter().filter(|a|a.role=="subagent").count(),8);
    for mutation in 0..6 {
        let mut bad=roster.clone(); let worker=bad.iter().position(|a|a.id=="codex-app-coord-worker").unwrap();
        match mutation {
            0=>bad[worker].parent_id=Some("claude-app-coord".into()),
            1=>bad[worker].parent_id=Some("codex-intake".into()),
            2=>bad[worker].project=Some("other project".into()),
            3=>bad[worker].parent_id=Some(bad[worker].id.clone()),
            4=>bad[worker].id=bad[0].id.clone(),
            _=>bad[worker].parent_id=None,
        }
        assert!(validate_hierarchy(&bad).is_err(),"case {mutation}");
    }
}

#[test]
fn concurrent_retries_and_ticks_do_not_duplicate_replies() {
    let t=tempfile::tempdir().unwrap(); let path=t.path().join("state.db");
    drop(Store::open(&path).unwrap());
    let barrier=Arc::new(Barrier::new(4));
    let handles:Vec<_>=(0..4).map(|_| {let path=path.clone();let barrier=barrier.clone();std::thread::spawn(move|| {
        let mut s=Store::open(&path).unwrap(); barrier.wait();
        for i in 0..20 { s.send(AGENT,&format!("request-{i:04}"),&format!("message{i}")).unwrap(); s.tick().unwrap(); }
    })}).collect();
    for h in handles { h.join().unwrap(); }
    let mut s=Store::open(&path).unwrap(); while s.tick().unwrap()>0 {}
    let snapshot=s.snapshot(AGENT,None).unwrap(); assert_eq!(snapshot.messages.len(),40);
    let user:Vec<_>=snapshot.messages.iter().filter(|m|m.sender=="user").collect();
    let reply:Vec<_>=snapshot.messages.iter().filter(|m|m.sender=="simulator").collect();
    assert_eq!(user.len(),20); assert_eq!(reply.len(),20);
    for m in user { assert_eq!(reply.iter().filter(|r|r.reply_to==Some(m.id)).count(),1); }
}

#[test]
fn simultaneous_first_open_seeds_once() {
    let t=tempfile::tempdir().unwrap();let path=t.path().join("state.db"); let barrier=Arc::new(Barrier::new(4));
    let handles:Vec<_>=(0..4).map(|_| {let path=path.clone();let barrier=barrier.clone();std::thread::spawn(move||{barrier.wait();Store::open(&path).unwrap().agents().unwrap().len()})}).collect();
    for h in handles {assert_eq!(h.join().unwrap(),14);}
}

#[test]
fn history_pages_use_stable_ids_even_with_identical_timestamps() {
    let t=tempfile::tempdir().unwrap();let path=t.path().join("state.db");let mut s=Store::open(&path).unwrap();
    for i in 0..205 { s.send(AGENT,&format!("request-{i:04}"),&format!("message{i}")).unwrap(); }
    let c=Connection::open(&path).unwrap();c.execute("UPDATE messages SET created_at='2026-11-21T00:00:00Z'",[]).unwrap();
    let mut cursor=None;let mut ids=Vec::new();
    loop { let p=s.snapshot(AGENT,cursor).unwrap();assert!(p.messages.len()<=100);
        assert!(p.messages.windows(2).all(|p|p[0].id<p[1].id));
        cursor=p.messages.first().map(|m|m.id);ids.extend(p.messages.iter().map(|m|m.id)); if !p.has_more {break;}
    }
    ids.sort();ids.dedup();assert_eq!(ids,(1..=205).collect::<Vec<_>>());
}

#[test]
fn invalid_payloads_do_not_create_messages() {
    let t=tempfile::tempdir().unwrap();let mut s=Store::open(&t.path().join("state.db")).unwrap();
    for (recipient,id,body) in [(AGENT,"request-0001","  "),(AGENT,"bad","hello"),("missing","request-0001","hello"),(AGENT,"request-0001;","hello")] {assert!(s.send(recipient,id,body).is_err());}
    assert!(s.send(AGENT,"request-0001",&"x".repeat(8001)).is_err());
    assert!(s.snapshot("missing",None).is_err());assert!(s.control("missing",Availability::Idle).is_err());
    assert!(s.snapshot(AGENT,None).unwrap().messages.is_empty());
    s.send(AGENT,"request-0001","<script>alert(1)</script>").unwrap();
    assert_eq!(s.snapshot(AGENT,None).unwrap().messages[0].body,"<script>alert(1)</script>");
}

#[test]
fn future_schema_is_preserved_and_links_refused() {
    let t=tempfile::tempdir().unwrap();let path=t.path().join("future.db");
    let c=Connection::open(&path).unwrap();c.execute_batch("PRAGMA user_version=99; CREATE TABLE future(content TEXT);INSERT INTO future VALUES('preserve');").unwrap();drop(c);
    let before=std::fs::read(&path).unwrap(); assert!(Store::open(&path).is_err());assert_eq!(std::fs::read(&path).unwrap(),before);
    let link=t.path().join("alias.db"); std::os::unix::fs::symlink(&path,&link).unwrap();assert!(Store::open(&link).is_err());
}

#[test]
fn viewer_origin_and_assets_are_not_workspace_authority() {
    assert!(trusted_window("workspace","workspace://localhost/"));
    for (label,url) in [("docs","workspace://localhost/"),("workspace","viewer://localhost/"),("docs","viewer://localhost/"),("workspace","workspace://localhost/?x=1"),("workspace","workspace://evil/"),("workspace","workspace://localhost:99/"),("workspace","workspace://user@localhost/")] {assert!(!trusted_window(label,url));}
    assert!(asset("/").is_some()); assert!(asset("/workspace.js").is_some());
    for p in ["/../Cargo.toml","/etc/passwd","/workspace.js?x=1","//"] {assert!(asset(p).is_none());}
}
