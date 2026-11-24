use agent_app_workspace::Store;
#[test]
fn adds_and_reports_per_project_and_ignores_bad_samples() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Store::open(&d.path().join("w.sqlite3")).unwrap();
    s.time_add(&[("/p/a".into(), 30_000, 10_000), ("/p/b".into(), 30_000, 30_000), ("".into(), 5, 5), ("/p/c".into(), 10, 20), ("/p/d".into(), -1, 0)]).unwrap();
    s.time_add(&[("/p/a".into(), 30_000, 30_000)]).unwrap();
    let r = s.time_report().unwrap();
    assert_eq!(r.len(), 2);
    // Most active first: a has 40 s active, b has 30 s
    assert_eq!((r[0].project.as_str(), r[0].today_running_ms, r[0].today_active_ms, r[0].all_active_ms), ("/p/a", 60_000, 40_000, 40_000));
    assert_eq!((r[1].project.as_str(), r[1].week_active_ms), ("/p/b", 30_000));
}
