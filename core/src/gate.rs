//! Best-effort read of a gate log file (rows newest first). Rows are newest first. A PASS row is pending unless a
//! row ABOVE it (later in time) names the same hash with an un-negated closing word.
//! A PASS row with no hash cannot be closed, so it is listed.
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

fn re(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).unwrap())
}

fn hashes(row: &str) -> Vec<String> {
    static H: OnceLock<Regex> = OnceLock::new();
    re(&H, r"\b[0-9a-f]{7,40}\b")
        .find_iter(row)
        .map(|m| m.as_str())
        .filter(|h| h.bytes().any(|b| b.is_ascii_digit()) && h.bytes().any(|b| (b'a'..=b'f').contains(&b)))
        .map(String::from)
        .collect()
}

fn closes(row: &str) -> bool {
    static C: OnceLock<Regex> = OnceLock::new();
    static N: OnceLock<Regex> = OnceLock::new();
    let close = re(&C, r"(?i)\b(approved|posted|merged|read back)\b");
    let neg = re(
        &N,
        r"(?i)(\bnot\b|n't\b|\bno\b|\bnever\b|\bawaiting\b|\buntil\b|\bbefore\b|\bwithout\b|\bunless\b|\bif\b|\bpending\b)[^|.;]{0,30}$",
    );
    close.find_iter(row).any(|m| {
        // 40 characters before the match (not bytes), as in the Python version.
        let start = row[..m.start()].char_indices().rev().nth(39).map(|(i, _)| i).unwrap_or(0);
        !neg.is_match(&row[start..m.start()])
    })
}

/// Split like Python's str.splitlines().
pub fn splitlines(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c| matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

pub fn pending(text: &str, limit: usize) -> Vec<String> {
    static P: OnceLock<Regex> = OnceLock::new();
    let pass = re(&P, r"\bPASS\b");
    let rows: Vec<&str> = splitlines(text).filter(|l| l.starts_with('|')).collect();
    let mut first_close: HashMap<String, usize> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        if closes(row) {
            for h in hashes(row) {
                first_close.entry(h[..7].to_string()).or_insert(i);
            }
        }
    }
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if !pass.is_match(row) {
            continue;
        }
        let closed = hashes(row).iter().any(|h| first_close.get(&h[..7]).map_or(false, |&c| c < i));
        if !closed {
            out.push(row.chars().take(400).collect());
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}
