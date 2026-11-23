//! Manual review labels for drafts. A label is the user's own note that they read a file, bound to the file's
//! content hash: if the file changes, the label is stale and shows as such. It is not an approval of anything.
use super::{db, Result, Store};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Review { pub path: String, pub sha256: String, pub checks: Vec<String>, pub reviewed_at: String }

/// Checks the reviewer must tick before a PASS (manual review) can be saved.
pub const REQUIRED_CHECKS: [&str; 5] = ["tests_run", "wording_true", "overlap_listed", "evidence_stated", "no_private_names"];

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    db(tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS manual_reviews (
           path TEXT PRIMARY KEY, sha256 TEXT NOT NULL, checks TEXT NOT NULL,
           reviewed_at TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now')));"))
}

impl Store {
    pub fn review_get(&self, path: &str) -> Result<Option<Review>> {
        db(self.connection.query_row("SELECT path,sha256,checks,reviewed_at FROM manual_reviews WHERE path=?1", params![path], |r| {
            let checks: String = r.get(2)?;
            Ok(Review { path: r.get(0)?, sha256: r.get(1)?, checks: checks.split(',').filter(|s| !s.is_empty()).map(String::from).collect(), reviewed_at: r.get(3)? })
        }).optional())
    }
    /// Saves PASS (manual review) for exactly this content hash. Every required check must be ticked.
    pub fn review_mark(&mut self, path: &str, sha256: &str, checks: &[String]) -> Result<Review> {
        if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) { return Err("Invalid content hash".into()); }
        for required in REQUIRED_CHECKS { if !checks.iter().any(|c| c == required) { return Err("Tick every review check before saving PASS".into()); } }
        db(self.connection.execute(
            "INSERT INTO manual_reviews(path,sha256,checks) VALUES (?1,?2,?3)
             ON CONFLICT(path) DO UPDATE SET sha256=excluded.sha256,checks=excluded.checks,reviewed_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            params![path, sha256.to_ascii_lowercase(), REQUIRED_CHECKS.join(",")]))?;
        self.review_get(path)?.ok_or_else(|| "Review was not saved".into())
    }
    pub fn review_clear(&mut self, path: &str) -> Result<()> {
        db(self.connection.execute("DELETE FROM manual_reviews WHERE path=?1", params![path])).map(|_| ())
    }
}
