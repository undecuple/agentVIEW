use agent_app_workspace::Store;
fn store() -> (tempfile::TempDir, Store) { let d = tempfile::tempdir().unwrap(); let s = Store::open(&d.path().join("w.sqlite3")).unwrap(); (d, s) }
#[test]
fn notes_save_list_conflict_and_history() {
    let (_d, mut s) = store();
    let n = s.note_save(None, "Runbooks", "Start", "# One", None).unwrap();
    assert_eq!(s.notes_list("one").unwrap().len(), 1);
    let n2 = s.note_save(Some(n.id), "Runbooks", "Start", "# Two", Some(&n.updated)).unwrap();
    assert_eq!(n2.revisions.len(), 1);
    assert_eq!(s.note_revision(n.id, n2.revisions[0].id).unwrap(), "# One");
    // A save based on the old timestamp is refused
    assert!(s.note_save(Some(n.id), "Runbooks", "Start", "# Three", Some("1999-01-01T00:00:00.000Z")).is_err());
    assert!(s.note_save(None, " ", "x", "", None).is_err());
    assert!(s.note_save(None, "a", "b", &"x".repeat(256 * 1024 + 1), None).is_err());
    s.note_delete(n.id).unwrap();
    assert!(s.note_get(n.id).is_err());
}
