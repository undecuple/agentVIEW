//! Add-time checks for a shared path (the only test in this binary, because it sets HOME).
use agent_app_core::policy::validate_allow;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

#[test]
fn share_validation_refuses_unsafe_paths() {
    let root = std::env::temp_dir().join(format!("agentview-share-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let root = { fs::create_dir_all(&root).unwrap(); fs::canonicalize(&root).unwrap() };
    let home = root.join("home");
    for d in ["notes", ".ssh", ".config", "work-docs"] { fs::create_dir_all(home.join(d)).unwrap(); }
    fs::write(home.join("notes/a.md"), "hello").unwrap();
    fs::write(home.join("notes/.env"), "SECRET=1").unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    std::env::set_var("HOME", &home);
    let s = |p: &Path| validate_allow(p.to_str().unwrap(), true);

    // Good: a folder and a file inside the home folder come back resolved.
    assert_eq!(s(&home.join("notes")).unwrap(), home.join("notes"));
    assert_eq!(s(&home.join("notes/a.md")).unwrap(), home.join("notes/a.md"));
    // The tilde form resolves the same way.
    assert_eq!(validate_allow("~/notes", true).unwrap(), home.join("notes"));

    // Refused: home, a parent of home, folders that hold secrets, a parent of one, denied names.
    assert!(s(&home).is_err(), "home itself");
    assert!(s(&root).is_err(), "a parent of home");
    assert!(s(Path::new("/")).is_err() && s(Path::new("/etc")).is_err() && s(Path::new("/proc/self")).is_err());
    assert!(s(&home.join(".ssh")).is_err() && s(&home.join(".config")).is_err(), "secret folders");
    assert!(s(&home.join("notes/.env")).is_err(), "denied file name");
    assert!(s(&home.join("work-docs")).is_ok(), "deny names match whole path parts, so work-docs is not work");

    // A symlink is judged by where it leads, so a link to a secret folder or out of the home tree is judged on its target.
    symlink(home.join(".ssh"), home.join("notes/link-ssh")).unwrap();
    assert!(s(&home.join("notes/link-ssh")).is_err(), "symlink to a secret folder");
    symlink(root.join("outside"), home.join("notes/link-out")).unwrap();
    assert_eq!(s(&home.join("notes/link-out")).unwrap(), root.join("outside"), "the stored form is the resolved target");

    // Shape: missing, relative, empty, very long, control characters, FIFO.
    assert!(s(&home.join("nope")).is_err());
    assert!(validate_allow("notes", true).is_err() && validate_allow("", true).is_err());
    assert!(validate_allow(&format!("/{}", "a".repeat(2000)), true).is_err());
    assert!(validate_allow("/tmp/a\nb", true).is_err() && validate_allow("/tmp/a\0b", true).is_err());
    let fifo = home.join("notes/pipe");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    assert!(s(&fifo).is_err(), "a FIFO is neither a folder nor a regular file");

    let _ = fs::remove_dir_all(&root);
}
