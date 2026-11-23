//! App-owned notes library (BookStack-style): shelves hold pages of Markdown, kept in the app's own database.
//! It never reads or writes repository or shared documents. Saves keep the last revisions and refuse a stale save.
use super::{db, Result, Store};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;

pub const MAX_BODY: usize = 256 * 1024;
pub const MAX_NOTES: i64 = 2000;
const KEEP_REVISIONS: i64 = 20;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NoteMeta { pub id: i64, pub shelf: String, pub title: String, pub updated: String, pub size: usize }
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Note { pub id: i64, pub shelf: String, pub title: String, pub body: String, pub updated: String, pub revisions: Vec<Revision> }
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Revision { pub id: i64, pub saved_at: String, pub size: usize }

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    db(tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS notes (
           id INTEGER PRIMARY KEY AUTOINCREMENT, shelf TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL,
           updated TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now')));
         CREATE TABLE IF NOT EXISTS note_revisions (
           id INTEGER PRIMARY KEY AUTOINCREMENT, note_id INTEGER NOT NULL, body TEXT NOT NULL,
           saved_at TEXT NOT NULL DEFAULT(strftime('%Y-%m-%dT%H:%M:%fZ','now')));"))
}

fn clean(s: &str, max: usize, what: &str) -> Result<String> {
    let t = s.trim();
    if t.is_empty() { return Err(format!("{what} is required")); }
    if t.chars().count() > max || t.chars().any(char::is_control) { return Err(format!("{what} is too long or has control characters")); }
    Ok(t.to_string())
}

impl Store {
    pub fn notes_list(&self, query: &str) -> Result<Vec<NoteMeta>> {
        let like = format!("%{}%", query.trim().to_lowercase().replace('%', "").replace('_', ""));
        let mut s = db(self.connection.prepare(
            "SELECT id,shelf,title,updated,length(body) FROM notes WHERE lower(title) LIKE ?1 OR lower(shelf) LIKE ?1 OR lower(body) LIKE ?1 ORDER BY shelf COLLATE NOCASE,title COLLATE NOCASE LIMIT 500"))?;
        let rows = db(s.query_map(params![like], |r| Ok(NoteMeta { id: r.get(0)?, shelf: r.get(1)?, title: r.get(2)?, updated: r.get(3)?, size: r.get::<_, i64>(4)? as usize })))?;
        rows.map(|r| db(r)).collect()
    }
    pub fn note_get(&self, id: i64) -> Result<Note> {
        let mut note = db(self.connection.query_row("SELECT id,shelf,title,body,updated FROM notes WHERE id=?1", params![id],
            |r| Ok(Note { id: r.get(0)?, shelf: r.get(1)?, title: r.get(2)?, body: r.get(3)?, updated: r.get(4)?, revisions: vec![] })).optional())?
            .ok_or("That page no longer exists")?;
        let mut s = db(self.connection.prepare("SELECT id,saved_at,length(body) FROM note_revisions WHERE note_id=?1 ORDER BY id DESC LIMIT 20"))?;
        let rows = db(s.query_map(params![id], |r| Ok(Revision { id: r.get(0)?, saved_at: r.get(1)?, size: r.get::<_, i64>(2)? as usize })))?;
        note.revisions = rows.map(|r| db(r)).collect::<Result<_>>()?;
        Ok(note)
    }
    pub fn note_revision(&self, note_id: i64, revision: i64) -> Result<String> {
        db(self.connection.query_row("SELECT body FROM note_revisions WHERE id=?1 AND note_id=?2", params![revision, note_id], |r| r.get(0)).optional())?
            .ok_or_else(|| "That revision no longer exists".into())
    }
    /// Creates a page (id None) or saves one. `base_updated` must match the stored time, so a save never overwrites newer text.
    pub fn note_save(&mut self, id: Option<i64>, shelf: &str, title: &str, body: &str, base_updated: Option<&str>) -> Result<Note> {
        let shelf = clean(shelf, 60, "Shelf")?; let title = clean(title, 120, "Title")?;
        if body.len() > MAX_BODY { return Err("Page is larger than 256 KB".into()); }
        let tx = db(self.connection.transaction())?;
        let id = match id {
            None => {
                let n: i64 = db(tx.query_row("SELECT count(*) FROM notes", [], |r| r.get(0)))?;
                if n >= MAX_NOTES { return Err("The notes library is full (2000 pages)".into()); }
                db(tx.execute("INSERT INTO notes(shelf,title,body) VALUES (?1,?2,?3)", params![shelf, title, body]))?;
                tx.last_insert_rowid()
            }
            Some(id) => {
                let current: Option<(String, String)> = db(tx.query_row("SELECT updated,body FROM notes WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?))).optional())?;
                let (updated, old) = current.ok_or("That page no longer exists")?;
                if base_updated != Some(updated.as_str()) { return Err("This page changed since you opened it. Reload it before saving.".into()); }
                if old != body { db(tx.execute("INSERT INTO note_revisions(note_id,body) VALUES (?1,?2)", params![id, old]))?; }
                db(tx.execute("UPDATE notes SET shelf=?2,title=?3,body=?4,updated=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1", params![id, shelf, title, body]))?;
                db(tx.execute("DELETE FROM note_revisions WHERE note_id=?1 AND id NOT IN (SELECT id FROM note_revisions WHERE note_id=?1 ORDER BY id DESC LIMIT ?2)", params![id, KEEP_REVISIONS]))?;
                id
            }
        };
        db(tx.commit())?;
        self.note_get(id)
    }
    pub fn note_delete(&mut self, id: i64) -> Result<()> {
        let tx = db(self.connection.transaction())?;
        db(tx.execute("DELETE FROM note_revisions WHERE note_id=?1", params![id]))?;
        db(tx.execute("DELETE FROM notes WHERE id=?1", params![id]))?;
        db(tx.commit())
    }
}
