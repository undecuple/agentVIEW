use agent_app_core::policy::redact;

// Fixture text is assembled at run time so this file holds no key-shaped literal.
fn body() -> String { format!("{}\n{}\n", "QUJD".repeat(16), "REVG".repeat(16)) }
fn begin(kind: &str) -> String { format!("{}BEGIN {kind}{}", "-".repeat(5), "-".repeat(5)) }
fn end(kind: &str) -> String { format!("{}END {kind}{}", "-".repeat(5), "-".repeat(5)) }

#[test]
fn a_whole_private_key_block_is_hidden() {
    for kind in ["PRIVATE KEY", "RSA PRIVATE KEY", "OPENSSH PRIVATE KEY", "EC PRIVATE KEY", "PGP PRIVATE KEY BLOCK"] {
        let text = format!("before\n{}\n{}{}\nafter\n", begin(kind), body(), end(kind));
        let out = redact(&text);
        assert!(out.contains("before") && out.contains("after"), "{kind}: text around the key stays");
        assert!(!out.contains("QUJD") && !out.contains("BEGIN") && !out.contains("END "), "{kind}: nothing of the block is shown: {out}");
    }
}

#[test]
fn a_cut_off_key_is_hidden_to_the_end_and_a_huge_one_loses_at_least_its_header() {
    let cut = format!("x\n{}\n{}", begin("RSA PRIVATE KEY"), body());
    let out = redact(&cut);
    assert!(out.starts_with("x\n") && !out.contains("QUJD"), "a truncated block is hidden to the end: {out}");
    let huge = format!("{}\n{}\n{}", begin("PRIVATE KEY"), "A".repeat(20000), end("PRIVATE KEY"));
    assert!(!redact(&huge).contains(&begin("PRIVATE KEY")), "the header of an oversized block is still hidden");
}

#[test]
fn ordinary_text_and_other_token_shapes_behave_as_before() {
    assert_eq!(redact("no secrets here"), "no secrets here");
    let kv = format!("{}{} = '{}'", "tok", "en", "abcdefghijklmnop1234");
    assert!(!redact(&kv).contains("abcdefghijklmnop"));
    let gh = format!("see {}{} ok", "gh", "p_abcdefghijklmnopqrstuvwxyz0123456789");
    assert!(!redact(&gh).contains("abcdefghijklmnopqrstuvwxyz"));
    assert!(redact(&format!("a public key {} is not secret", begin("PUBLIC KEY"))).contains("BEGIN PUBLIC KEY"));
}
