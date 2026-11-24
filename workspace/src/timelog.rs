//! Time per project, observed while the app is open: how long a project's sessions were running and how long they
//! showed terminal output recently ("active"). Daily totals in the app's own database; days are UTC.
use super::{db, Result, Store};
use rusqlite::{params, Transaction};
use serde::Serialize;

const KEEP_DAYS: i64 = 400;

pub(super) fn initialize(tx: &Transaction<'_>) -> Result<()> {
    db(tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_time (
           day TEXT NOT NULL, project TEXT NOT NULL, running_ms INTEGER NOT NULL DEFAULT 0, active_ms INTEGER NOT NULL DEFAULT 0,
           PRIMARY KEY(day, project));"))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectTime {
    pub project: String,
    pub today_running_ms: i64, pub today_active_ms: i64,
    pub week_running_ms: i64, pub week_active_ms: i64,
    pub all_running_ms: i64, pub all_active_ms: i64,
}

impl Store {
    /// Adds observed time to today's row (UTC) for each project. Old days are pruned.
    pub fn time_add(&mut self, samples: &[(String, i64, i64)]) -> Result<()> {
        let tx = db(self.connection.transaction())?;
        for (project, run, act) in samples {
            let p: String = project.chars().take(300).collect();
            if p.is_empty() || *run < 0 || *act < 0 || *act > *run { continue; }
            db(tx.execute(
                "INSERT INTO project_time(day,project,running_ms,active_ms) VALUES(date('now'),?1,?2,?3)
                 ON CONFLICT(day,project) DO UPDATE SET running_ms=running_ms+?2, active_ms=active_ms+?3", params![p, run, act]))?;
        }
        db(tx.execute("DELETE FROM project_time WHERE day < date('now', ?1)", params![format!("-{KEEP_DAYS} days")]))?;
        db(tx.commit())
    }
    pub fn time_report(&self) -> Result<Vec<ProjectTime>> {
        let mut s = db(self.connection.prepare(
            "SELECT project,
               COALESCE(SUM(CASE WHEN day=date('now') THEN running_ms END),0), COALESCE(SUM(CASE WHEN day=date('now') THEN active_ms END),0),
               COALESCE(SUM(CASE WHEN day>=date('now','-6 days') THEN running_ms END),0), COALESCE(SUM(CASE WHEN day>=date('now','-6 days') THEN active_ms END),0),
               SUM(running_ms), SUM(active_ms)
             FROM project_time GROUP BY project ORDER BY 7 DESC, project LIMIT 200"))?;
        let rows = db(s.query_map([], |r| Ok(ProjectTime { project: r.get(0)?, today_running_ms: r.get(1)?, today_active_ms: r.get(2)?, week_running_ms: r.get(3)?, week_active_ms: r.get(4)?, all_running_ms: r.get(5)?, all_active_ms: r.get(6)? })))?;
        rows.map(|r| db(r)).collect()
    }
}
