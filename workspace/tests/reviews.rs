use agent_app_workspace::{reviews::REQUIRED_CHECKS, Store};

fn all() -> Vec<String> { REQUIRED_CHECKS.iter().map(|s| s.to_string()).collect() }

#[test]
fn review_is_bound_to_a_hash_and_needs_every_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(&dir.path().join("w.sqlite3")).unwrap();
    let h = "a".repeat(64);
    assert!(s.review_get("/d/pr.md").unwrap().is_none());
    assert!(s.review_mark("/d/pr.md", &h, &all()[..4]).is_err(), "missing a check");
    assert!(s.review_mark("/d/pr.md", "xyz", &all()).is_err(), "bad hash");
    let r = s.review_mark("/d/pr.md", &h, &all()).unwrap();
    assert_eq!(r.sha256, h);
    let h2 = "b".repeat(64);
    assert_eq!(s.review_mark("/d/pr.md", &h2, &all()).unwrap().sha256, h2, "re-review replaces the hash");
    s.review_clear("/d/pr.md").unwrap();
    assert!(s.review_get("/d/pr.md").unwrap().is_none());
}

#[test]
fn reviews_survive_reopening() {
    let dir = tempfile::tempdir().unwrap(); let p = dir.path().join("w.sqlite3");
    { let mut s = Store::open(&p).unwrap(); s.review_mark("/d/a.md", &"c".repeat(64), &all()).unwrap(); }
    assert!(Store::open(&p).unwrap().review_get("/d/a.md").unwrap().is_some());
}
