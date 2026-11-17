use agent_app_core::{gate, images, policy, router::handle, Config};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::Instant;
use tempfile::TempDir;

struct Env {
    _t: TempDir,
    d: PathBuf,
    cfg: Config,
}

fn env() -> Env {
    let t = tempfile::Builder::new().prefix("aa-").tempdir().unwrap();
    let d = fs::canonicalize(t.path()).unwrap();
    for (n, c) in [
        ("docs/a.md", "line1\nline2\n"),
        ("docs/b.md", "line1\nline3\n"),
        ("docs/work/secret.md", "nope"),
        ("docs/key.PEM", "nope"),
        ("outside.md", "outside"),
    ] {
        let p = d.join(n);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, c).unwrap();
    }
    symlink(d.join("outside.md"), d.join("docs/link.md")).unwrap();
    symlink(d.join("docs/work/secret.md"), d.join("docs/ok-name.md")).unwrap();
    let cfg = Config::from_toml(&format!("allow = [\"{}\"]\n", d.join("docs").display())).unwrap();
    Env { _t: t, d, cfg }
}

fn get(e: &Env, target: &str) -> (u16, String) {
    let r = handle(&e.cfg, "GET", target, None);
    (r.status, String::from_utf8_lossy(&r.body).into_owned())
}
fn view(e: &Env, rel: &str) -> (u16, String) {
    get(e, &format!("/view?p={}", e.d.join(rel).display()))
}
fn img(e: &Env, rel: &str) -> u16 {
    get(e, &format!("/img?p={}", e.d.join(rel).display())).0
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend(w.to_be_bytes());
    v.extend(h.to_be_bytes());
    v.extend([8, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    v
}
fn gif(w: u16, h: u16) -> Vec<u8> {
    let mut v = b"GIF89a".to_vec();
    v.extend(w.to_le_bytes());
    v.extend(h.to_le_bytes());
    v.extend(b"\0\0\0;");
    v
}
fn jpeg(w: u16, h: u16) -> Vec<u8> {
    let mut v = b"\xff\xd8\xff\xe0\0\x04\0\0\xff\xc0\0\x0b\x08".to_vec();
    v.extend(h.to_be_bytes());
    v.extend(w.to_be_bytes());
    v.extend(b"\x01\x01\x11\0\xff\xd9");
    v
}
fn webp(w: u32, h: u32) -> Vec<u8> {
    let mut body = b"VP8X".to_vec();
    body.extend(10u32.to_le_bytes());
    body.extend([0, 0, 0, 0]);
    body.extend(&(w - 1).to_le_bytes()[..3]);
    body.extend(&(h - 1).to_le_bytes()[..3]);
    let mut v = b"RIFF".to_vec();
    v.extend((4 + body.len() as u32).to_le_bytes());
    v.extend(b"WEBP");
    v.extend(body);
    v
}

// ---- path policy
#[test]
fn policy_cases() {
    let e = env();
    let c = |rel: &str| policy::check(&e.d.join(rel).to_string_lossy(), &e.cfg);
    assert!(c("docs/a.md").is_ok());
    assert!(c("docs/work/secret.md").is_err());
    assert!(c("docs/key.PEM").is_err());
    assert!(c("docs/link.md").is_err(), "symlink out of the list");
    assert!(c("docs/ok-name.md").is_err(), "symlink into a denied folder");
    assert!(c("docs/../outside.md").is_err());
    assert!(c("docs/missing.md").is_err());
    assert!(policy::check("docs/a.md\0.pem", &e.cfg).is_err());
    let names: Vec<_> = policy::listing(&e.cfg).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["a.md", "b.md"]);
}

#[test]
fn hardlink_fifo_and_system_paths_refused() {
    let e = env();
    fs::hard_link(e.d.join("docs/work/secret.md"), e.d.join("docs/hl.md")).unwrap();
    assert_eq!(view(&e, "docs/hl.md").0, 403);
    assert!(std::process::Command::new("mkfifo").arg(e.d.join("docs/f")).status().unwrap().success());
    assert_eq!(view(&e, "docs/f").0, 403);
    for sys in ["/proc/self/environ", "/sys/kernel/version", "/dev/null"] {
        assert!(Config::from_toml(&format!("allow = [\"{sys}\"]\n")).is_err(), "{sys} refused at load");
        let cfg = Config::from_toml("allow = [\"/tmp\"]\n").unwrap();
        assert!(policy::check(sys, &cfg).is_err(), "{sys} refused at read");
    }
}

#[test]
fn non_utf8_name_refused() {
    use std::os::unix::ffi::OsStrExt;
    let e = env();
    let name = std::ffi::OsStr::from_bytes(b"bad\xff.md");
    let p = e.d.join("docs").join(name);
    fs::write(&p, "x").unwrap();
    assert!(p.exists());
    assert!(policy::check_path(&p, &e.cfg).is_err(), "check on the exact bytes");
    assert!(policy::listing(&e.cfg).iter().all(|p| p.to_str().is_some()));
}

// ---- config
#[test]
fn config_cases() {
    let c = Config::from_toml("allow = [\"/tmp\"]\n").unwrap();
    assert!(c.deny.iter().any(|d| d == "work") && c.deny.iter().any(|d| d == "*.pem"));
    let c = Config::from_toml("allow = [\"/tmp\"]\ndeny_names = [\"Extra\"]\n").unwrap();
    assert!(c.deny.iter().any(|d| d == "extra") && c.deny.iter().any(|d| d == "work"));
    assert!(Config::from_toml("allow = [\"docs\"]\n").is_err());
    assert!(Config::from_toml("allow = [\"/tmp\"]\ngate_file = \"rel.md\"\n").is_err());
    assert!(Config::from_toml("not toml [").is_err());
}

#[test]
fn config_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::Builder::new().prefix("aa-").tempdir().unwrap();
    let dir = fs::canonicalize(t.path()).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let f = dir.join("allowlist.toml");
    fs::write(&f, "allow = [\"/tmp\"]\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Config::from_file(&f).is_ok());
    fs::set_permissions(&f, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(Config::from_file(&f).is_err(), "world-writable file");
    fs::set_permissions(&f, fs::Permissions::from_mode(0o620)).unwrap();
    assert!(Config::from_file(&f).is_err(), "group-writable file");
    fs::set_permissions(&f, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(Config::from_file(&f).is_err(), "world-writable directory");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let l = dir.join("link.toml");
    symlink(&f, &l).unwrap();
    assert!(Config::from_file(&l).is_err(), "symlinked config");
    assert!(Config::from_file(&dir.join("missing.toml")).is_err());
}

// ---- gate
#[test]
fn gate_cases() {
    assert_eq!(gate::pending("| d | x | **t: PASS on abc1234, approved later** |\n", 50).len(), 1);
    assert_eq!(gate::pending("| d | x | **t: not approved yet, abc1234** |\n| d | x | **t: PASS on abc1234** |\n", 50).len(), 1);
    assert_eq!(gate::pending("| d | x | **t: approved abc9999** |\n| d | x | **t: PASS on abc1234** |\n", 50).len(), 1);
    assert!(gate::pending("| d | x | **t: approved abc1234** |\n| d | x | **t: PASS on abc1234** |\n", 50).is_empty());
    assert_eq!(gate::pending("| d | x | **t: PASS on deadbeef, decaded** |\n", 50).len(), 1);
    let t = "| d | x | **t: posted on abc1234** |\n| d | x | **t: PASS on abc1234** |\n| d | x | **u: PASS on def5678** awaiting approval |\n";
    let r = gate::pending(t, 50);
    assert_eq!(r.len(), 1);
    assert!(r[0].contains("def5678"));
}

#[test]
fn gate_is_linear_on_big_input() {
    let mut t = String::new();
    for i in 0..20000 {
        t.push_str(&format!("| d | x | **t{i}: PASS on {:07x}a** |\n", i));
    }
    let s = Instant::now();
    let _ = gate::pending(&t, usize::MAX);
    assert!(s.elapsed().as_secs() < 3);
}

// ---- images
#[test]
fn image_units() {
    assert_eq!(images::validate(&png(2, 2), "a.png"), Ok("image/png"));
    assert_eq!(images::validate(&gif(3, 4), "a.GIF"), Ok("image/gif"));
    assert_eq!(images::validate(&jpeg(5, 6), "a.jpeg"), Ok("image/jpeg"));
    assert_eq!(images::validate(&webp(7, 8), "a.webp"), Ok("image/webp"));
    assert_eq!(images::size(&png(9, 10), "png"), Ok((9, 10)));
    assert_eq!(images::size(&jpeg(5, 6), "jpeg"), Ok((5, 6)));
    assert_eq!(images::size(&webp(7, 8), "webp"), Ok((7, 8)));
    for (d, n) in [
        (png(2, 2), "a.jpg"),
        (b"<svg xmlns='x'/>".to_vec(), "a.svg"),
        (b"<svg/>".to_vec(), "a.png"),
        (b"nothing".to_vec(), "a.png"),
        (png(20000, 2), "a.png"),
        (png(10000, 10000), "a.png"),
        (png(2, 2)[..20].to_vec(), "a.png"),
        (b"\xff\xd8\xff\xe0".to_vec(), "a.jpg"),
        (b"GIF89a".to_vec(), "a.gif"),
        (b"RIFF\0\0\0\0WEBP".to_vec(), "a.webp"),
    ] {
        assert!(images::validate(&d, n).is_err(), "{n}");
    }
    assert_eq!(images::validate(&png(16384, 1), "a.png"), Ok("image/png"));
    assert_eq!(images::validate(&png(4000, 4000), "a.png"), Ok("image/png"));
    assert!(images::validate(&png(4001, 4000), "a.png").is_err());
    assert!(images::validate(&png(16385, 1), "a.png").is_err());
    let mut j = b"\xff\xd8".to_vec();
    j.extend(b"\xff\xd0".repeat(100000));
    j.extend([0u8; 20]);
    assert!(images::validate(&j, "a.jpg").is_err(), "marker loop capped");
}

// ---- router
#[test]
fn router_basics() {
    let e = env();
    let (s, b) = get(&e, "/");
    assert_eq!(s, 200);
    assert!(b.contains("a.md") && !b.contains("secret.md"));
    assert_eq!(view(&e, "docs/work/secret.md").0, 403);
    let (s, b) = view(&e, "docs/work/secret.md");
    assert_eq!(s, 403);
    assert!(!b.contains(e.d.to_str().unwrap()), "no full path in errors");
    let (s, b) = get(&e, &format!("/diff?a={}&b={}", e.d.join("docs/a.md").display(), e.d.join("docs/b.md").display()));
    assert_eq!(s, 200);
    assert!(b.contains("-line2") && b.contains("+line3"));
    assert_eq!(get(&e, &format!("/diff?a={}&b={}", e.d.join("docs/a.md").display(), e.d.join("docs/work/secret.md").display())).0, 403);
    for m in ["POST", "PUT", "DELETE", "HEAD", "PATCH"] {
        assert_eq!(handle(&e.cfg, m, "/", None).status, 405, "{m}");
    }
    fs::write(e.d.join("docs/x.md"), "<script>alert(1)</script>").unwrap();
    assert!(!view(&e, "docs/x.md").1.contains("<script>alert"));
    let r = handle(&e.cfg, "GET", "/", None);
    let h = |k: &str| r.headers.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    assert_eq!(h("Cache-Control").as_deref(), Some("no-store"));
    assert_eq!(h("Cross-Origin-Resource-Policy").as_deref(), Some("same-origin"));
    assert_eq!(h("X-Content-Type-Options").as_deref(), Some("nosniff"));
    assert_eq!(h("Referrer-Policy").as_deref(), Some("no-referrer"));
}

#[test]
fn uri_handling() {
    let e = env();
    let p = e.d.join("docs/a.md");
    let p = p.display();
    for t in ["//etc/passwd", "/view/../view", "/view/", "/ view", "/gate/", "/img/x", "/VIEW", "/view%2f"] {
        assert_eq!(get(&e, t).0, 404, "{t}");
    }
    assert_eq!(get(&e, "/view?p=%00").0, 403);
    assert_eq!(get(&e, &format!("/view?p={p}%00.pem")).0, 403);
    fs::create_dir_all(e.d.join("docs/%2e%2e")).unwrap();
    fs::write(e.d.join("docs/%2e%2e/x.md"), "literal").unwrap();
    fs::write(e.d.join("outside.png"), png(2, 2)).unwrap();
    let dd = e.d.display();
    // refused by rule although the target exists
    assert_eq!(get(&e, &format!("/view?p={dd}/docs/%2e%2e/outside.md")).0, 403);
    assert_eq!(get(&e, &format!("/view?p={dd}/docs/%2E%2E%2Foutside.md")).0, 403);
    assert_eq!(get(&e, &format!("/view?p={dd}/docs/..%2foutside.png")).0, 403);
    assert_eq!(get(&e, &format!("/view?p={dd}/docs/../outside.md")).0, 403);
    // decoded exactly once: %252e%252e becomes the literal directory name "%2e%2e", which exists and is allowed
    let (s, b) = get(&e, &format!("/view?p={dd}/docs/%252e%252e/x.md"));
    assert_eq!((s, b.contains("literal")), (200, true));
    assert_eq!(get(&e, "/view?p=%ff%fe").0, 403, "invalid UTF-8");
    assert_eq!(get(&e, &format!("/view?p={p}&p={p}")).0, 403, "repeated p");
    assert_eq!(get(&e, &format!("/view?p={p}")).0, 200);
}

#[test]
fn truncation_and_caps() {
    let e = env();
    fs::write(e.d.join("docs/big.md"), "x".repeat(1_000_010)).unwrap();
    assert!(view(&e, "docs/big.md").1.contains("Truncated"));
    let many = "![](http://x/a.png)".repeat(60000);
    fs::write(e.d.join("docs/many.md"), many).unwrap();
    let s = Instant::now();
    let (_, b) = view(&e, "docs/many.md");
    assert!(s.elapsed().as_secs() < 3);
    assert!(b.matches("<li>").count() <= 22 && b.contains("limit 20"));
    fs::write(e.d.join("docs/long.md"), format!("![{}](x.png", "a".repeat(900_000))).unwrap();
    let s = Instant::now();
    assert_eq!(view(&e, "docs/long.md").0, 200);
    assert!(s.elapsed().as_secs() < 3);
}

#[test]
fn images_over_http() {
    let e = env();
    for (n, d) in [
        ("docs/ok.png", png(2, 2)),
        ("docs/work/hidden.png", png(2, 2)),
        ("docs/bad.jpg", png(2, 2)),
        ("docs/fake.png", b"<svg onload=alert(1)/>".to_vec()),
        ("docs/huge.png", png(20000, 2)),
    ] {
        fs::write(e.d.join(n), d).unwrap();
    }
    let mut big = png(2, 2);
    big.resize(images::MAX_IMG_BYTES + 1, 0);
    fs::write(e.d.join("docs/big.png"), &big).unwrap();
    big.truncate(images::MAX_IMG_BYTES);
    fs::write(e.d.join("docs/edge.png"), &big).unwrap();
    assert_eq!(img(&e, "docs/ok.png"), 200);
    assert_eq!(img(&e, "docs/edge.png"), 200, "exactly the cap");
    for n in ["docs/work/hidden.png", "docs/bad.jpg", "docs/fake.png", "docs/huge.png", "docs/big.png", "docs/a.md", "docs/missing.png"] {
        assert_eq!(img(&e, n), 403, "{n}");
    }
    assert_eq!(img(&e, "outside.md"), 403);
    let r = handle(&e.cfg, "GET", &format!("/img?p={}", e.d.join("docs/ok.png").display()), None);
    let h = |k: &str| r.headers.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    assert_eq!(h("Content-Type").as_deref(), Some("image/png"));
    assert!(h("Content-Security-Policy").unwrap().contains("sandbox"));
    assert_eq!(r.body, png(2, 2));
    // symlinked images
    symlink(e.d.join("docs/ok.png"), e.d.join("docs/ln.png")).unwrap();
    symlink(e.d.join("docs/work/hidden.png"), e.d.join("docs/ln2.png")).unwrap();
    assert_eq!(img(&e, "docs/ln.png"), 200);
    assert_eq!(img(&e, "docs/ln2.png"), 403);
    // Sec-Fetch-Site
    for (site, code) in [("cross-site", 403), ("same-site", 403), ("same-origin", 200), ("none", 200)] {
        let r = handle(&e.cfg, "GET", &format!("/img?p={}", e.d.join("docs/ok.png").display()), Some(site));
        assert_eq!(r.status, code, "{site}");
    }
    // view of an image path, and an alias decided by resolved name
    assert!(view(&e, "docs/ok.png").1.contains("<img"));
    symlink(e.d.join("docs/ok.png"), e.d.join("docs/alias.md")).unwrap();
    assert!(view(&e, "docs/alias.md").1.contains("<img"));
}

#[test]
fn doc_image_refs() {
    let e = env();
    fs::write(e.d.join("docs/ok.png"), png(2, 2)).unwrap();
    fs::write(e.d.join("docs/work/hidden.png"), png(2, 2)).unwrap();
    fs::write(
        e.d.join("docs/doc.md"),
        "![a](ok.png) ![b](http://example.com/x.png) ![c](work/hidden.png) ![d](/etc/x.png) ![e](../outside.png)",
    )
    .unwrap();
    let (_, b) = view(&e, "docs/doc.md");
    assert_eq!(b.matches("<img ").count(), 1);
    assert!(b.contains("not shown"));
    fs::write(e.d.join("docs/t.md"), "![a](ok.png \"a title\")").unwrap();
    assert_eq!(view(&e, "docs/t.md").1.matches("<img ").count(), 1);
    fs::write(e.d.join("docs/some.md"), "![a](ok.png)\n".repeat(30)).unwrap();
    assert_eq!(view(&e, "docs/some.md").1.matches("<img ").count(), 20);
    let fake = format!("gh{}{}", "p_", "abcdefghijklmnopqrstuvwxyz01");
    fs::write(e.d.join("docs/r.md"), format!("![a](http://x/{fake})")).unwrap();
    assert!(!view(&e, "docs/r.md").1.contains(&fake));
}

#[test]
fn redaction_is_case_insensitive_and_global() {
    let fake = format!("gh{}{}", "p_", "abcdefghijklmnopqrstuvwxyz01");
    assert!(!policy::redact(&format!("x {fake} y")).contains(&fake));
    let line = format!("TOK{}{} = \"abcdefghijklmnop\"", "EN", "");
    assert!(policy::redact(&line).contains("[redacted]"));
    let up = format!("GH{}{}", "P_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ01");
    assert!(policy::redact(&up).contains("[redacted]"));
}

#[test]
fn gate_page_over_router() {
    let e = env();
    fs::write(e.d.join("docs/up.md"), "| d | x | **t: PASS on abc1234** |\n").unwrap();
    let mut cfg = e.cfg.clone();
    cfg.gate_file = Some(e.d.join("docs/up.md"));
    let r = handle(&cfg, "GET", "/gate", None);
    let b = String::from_utf8_lossy(&r.body).into_owned();
    assert!(b.contains("abc1234") && b.contains("does NOT mean"));
    let r = handle(&e.cfg, "GET", "/gate", None);
    assert!(String::from_utf8_lossy(&r.body).contains("No gate file"));
}

#[test]
fn gate_big_single_row_and_16mb() {
    let row = format!("| d | x | **t: PASS on abc1234 {}** |\n", "z".repeat(5_000_000));
    let s = Instant::now();
    assert_eq!(gate::pending(&row, 50).len(), 1);
    assert!(s.elapsed().as_secs() < 5);
    let mut big = String::new();
    while big.len() < 16_000_000 {
        big.push_str("| d | x | **t: PASS on abc1234** |\n| d | x | **t: approved abc1234** |\n");
    }
    let s = Instant::now();
    let _ = gate::pending(&big, usize::MAX);
    // 0.16 s in a release build; the debug build is far slower, so the bound here only catches blow-ups.
    assert!(s.elapsed().as_secs() < 60);
}

#[test]
fn gate_multibyte_negation_and_splitlines() {
    let t = "| d | x | **t: ééééééééééééééééééééééééééééééééééé not approved abc1234** |\n| d | x | **t: PASS on abc1234** |\n";
    assert_eq!(gate::pending(t, 50).len(), 1);
    let t2 = "| d | x | **t: approved abc1234** |\u{2028}| d | x | **t: PASS on abc1234** |\n";
    assert!(gate::pending(t2, 50).is_empty(), "U+2028 splits rows like Python splitlines");
}

#[test]
fn allow_entries_rejected_at_load() {
    let home = std::env::var("HOME").unwrap();
    for bad in ["/", "/etc", "/usr", "/proc", "/proc/self", &home, "/home", "~/"] {
        let toml = format!("allow = [\"{bad}\"]\n");
        assert!(Config::from_toml(&toml).is_err(), "{bad}");
    }
    assert!(Config::from_toml("allow = [\"/nonexistent-dir-xyz\"]\n").is_err(), "missing entry refused");
    assert!(Config::from_toml("allow = [1]\n").is_err());
    assert!(Config::from_toml("allow = [\"/tmp\"]\ndeny_names = [1]\n").is_err());
    assert!(Config::from_toml("allow = [\"/tmp\"]\ndeny_names = \"work\"\n").is_err());
    assert!(Config::from_toml("allow = [\"/tmp\"]\ngate_file = 5\n").is_err());
}

#[test]
fn file_allow_entry_and_default_config() {
    let e = env();
    let one = e.d.join("docs/a.md");
    let cfg = Config::from_toml(&format!("allow = [\"{}\"]\n", one.display())).unwrap();
    assert_eq!(handle(&cfg, "GET", &format!("/view?p={}", one.display()), None).status, 200);
    assert_eq!(handle(&cfg, "GET", &format!("/view?p={}", e.d.join("docs/b.md").display()), None).status, 403);
    // a failed config load leaves the default (empty) config: nothing is readable
    let none = Config::default();
    assert_eq!(handle(&none, "GET", &format!("/view?p={}", one.display()), None).status, 403);
    assert!(policy::listing(&none).is_empty());
    assert!(Config::from_file(&e.d.join("missing.toml")).is_err());
}

#[test]
fn glob_cases() {
    use policy::glob_match as g;
    assert!(g("*.pem", "KEY.PEM") && g("id_rsa*", "id_rsa.pub") && g("?x", "ax") && !g("?x", "x"));
    assert!(g("[abc]x", "bx") && !g("[abc]x", "dx") && g("[a-c]x", "cx") && g("[!a]x", "bx") && !g("[!a]x", "ax"));
    assert!(g("*é*", "caféx") && g(".git-credentials", ".GIT-credentials") && !g("work", "network"));
    assert!(g("[", "[") && !g("*.key", "key"));
}

#[test]
fn default_deny_matches_python() {
    // Keep the Rust default list equal to the Python one (viewer/paths.py DEFAULT_DENY plus *.iso).
    let py = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../viewer/paths.py")).unwrap();
    let a = py.find("DEFAULT_DENY = [").unwrap();
    let b = py[a..].find(']').unwrap() + a;
    let mut py_list: Vec<String> =
        py[a..b].split('"').enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s.to_lowercase()).collect();
    let cfg = Config::from_toml("allow = [\"/tmp\"]\n").unwrap();
    py_list.sort();
    py_list.dedup();
    assert_eq!(cfg.deny, py_list);
}

#[test]
fn diff_of_different_big_files_is_bounded() {
    let e = env();
    let a: String = (0..20000).map(|i| format!("a{i}\n")).collect();
    let b: String = (0..20000).map(|i| format!("b{i}\n")).collect();
    fs::write(e.d.join("docs/big1.md"), &a[..200_000.min(a.len())]).unwrap();
    fs::write(e.d.join("docs/big2.md"), &b[..200_000.min(b.len())]).unwrap();
    let s = Instant::now();
    let (st, body) = get(&e, &format!("/diff?a={}&b={}", e.d.join("docs/big1.md").display(), e.d.join("docs/big2.md").display()));
    assert_eq!(st, 200);
    assert!(s.elapsed().as_secs() < 8);
    assert!(body.contains("output truncated"));
}

#[test]
fn more_image_kinds() {
    // VP8 (lossy) and VP8L (lossless) webp, and progressive JPEG (SOF2)
    let mut vp8 = b"RIFF\0\0\0\0WEBPVP8 ".to_vec();
    vp8.extend([0u8; 4]);
    vp8.extend([0u8; 6]);
    vp8.extend(30u16.to_le_bytes());
    vp8.extend(40u16.to_le_bytes());
    assert_eq!(images::size(&vp8, "webp"), Ok((30, 40)));
    let mut vp8l = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
    vp8l.extend([0u8; 4]);
    vp8l.push(0x2f);
    let bits: u32 = (9 - 1) | ((7 - 1) << 14);
    vp8l.extend(bits.to_le_bytes());
    assert_eq!(images::size(&vp8l, "webp"), Ok((9, 7)));
    let mut sof2 = b"\xff\xd8\xff\xe0\0\x04\0\0\xff\xc2\0\x0b\x08".to_vec();
    sof2.extend(8u16.to_be_bytes());
    sof2.extend(6u16.to_be_bytes());
    sof2.extend(b"\x01\x01\x11\0\xff\xd9");
    assert_eq!(images::size(&sof2, "jpeg"), Ok((6, 8)));
}
