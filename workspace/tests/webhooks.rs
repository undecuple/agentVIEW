use agent_app_workspace::Store;
fn ev(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }
#[test]
fn several_destinations_with_events_and_limits() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("w.sqlite3")).unwrap();
    s.webhook_save(None, "Phone", Some("https://hooks.example.test/a"), true, &ev(&["error", "quiet"])).unwrap();
    s.webhook_save(None, "Team", Some("https://hooks.example.test/b"), false, &ev(&["finished"])).unwrap();
    let all = s.webhooks().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].events, ev(&["quiet", "error"]));
    // Editing without a new address keeps the old one
    s.webhook_save(Some(all[0].id), "Phone 2", None, false, &ev(&["quiet"])).unwrap();
    let a = &s.webhooks().unwrap()[0];
    assert_eq!((a.name.as_str(), a.url.as_str(), a.enabled), ("Phone 2", "https://hooks.example.test/a", false));
    // Refusals
    assert!(s.webhook_save(None, "Bad", Some("http://hooks.example.test/x"), true, &ev(&["quiet"])).is_err());
    assert!(s.webhook_save(None, "Bad", Some("https://hooks.example.test/x"), true, &ev(&["nope"])).is_err());
    assert!(s.webhook_save(None, "Bad", Some("https://hooks.example.test/x"), true, &[]).is_err());
    assert!(s.webhook_save(None, " ", Some("https://hooks.example.test/x"), true, &ev(&["quiet"])).is_err());
    assert!(s.webhook_save(Some(999), "x", None, true, &ev(&["quiet"])).is_err());
    for i in 0..3 { s.webhook_save(None, &format!("n{i}"), Some("https://hooks.example.test/x"), true, &ev(&["quiet"])).unwrap(); }
    assert!(s.webhook_save(None, "sixth", Some("https://hooks.example.test/x"), true, &ev(&["quiet"])).is_err());
    s.webhook_remove(all[1].id).unwrap();
    assert_eq!(s.webhooks().unwrap().len(), 4);
}
#[test]
fn old_single_webhook_is_migrated() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("w.sqlite3");
    { let c = rusqlite::Connection::open(&path).unwrap();
      c.execute_batch("CREATE TABLE webhook (id INTEGER PRIMARY KEY CHECK(id=1), url TEXT NOT NULL, enabled INTEGER NOT NULL, updated TEXT NOT NULL DEFAULT 'x'); INSERT INTO webhook(id,url,enabled) VALUES(1,'https://hooks.example.test/old',1);").unwrap(); }
    let s = Store::open(&path).unwrap();
    let h = s.webhooks().unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!((h[0].name.as_str(), h[0].url.as_str(), h[0].enabled, h[0].events.clone()), ("Default", "https://hooks.example.test/old", true, ev(&["quiet", "error"])));
}
