//! Time per project, from what the app observes while it is open. A project counts as running while any of its sessions
//! runs and as active while any shows terminal output from the last two minutes. It is terminal activity, not time at the keyboard.
use agent_app_sessions::Activity;
use agent_app_workspace::timelog::ProjectTime;
use std::collections::HashMap;

pub const ACTIVE_WITHIN_MS: u64 = 2 * 60 * 1000;

/// (project, running_ms, active_ms) for one sampling step of `dt_ms`. A project is counted once even with several sessions.
pub fn sample(acts: &[Activity], dt_ms: i64) -> Vec<(String, i64, i64)> {
    let mut m: HashMap<&str, bool> = HashMap::new(); // project -> any active
    for a in acts.iter().filter(|a| a.running && !a.project.is_empty()) {
        let active = a.idle_ms.is_some_and(|i| i <= ACTIVE_WITHIN_MS);
        let e = m.entry(a.project.as_str()).or_insert(false);
        *e = *e || active;
    }
    m.into_iter().map(|(p, active)| (p.to_string(), dt_ms, if active { dt_ms } else { 0 })).collect()
}

#[tauri::command]
pub fn time_report(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>) -> Result<Vec<ProjectTime>, String> {
    super::with_store(&window, &state, |s| s.time_report())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_app_sessions::Provider;
    fn act(project: &str, running: bool, idle: Option<u64>) -> Activity {
        Activity { id: format!("{project}{idle:?}"), display_name: "x".into(), label: String::new(), project: project.into(), provider: Provider::Claude, status: "running".into(),
            running, output_total: 0, input_total: 0, idle_ms: idle, action: None, action_at: None, exit_info: None, exit_at: None }
    }
    #[test] fn counts_each_project_once_and_activity_by_recent_output() {
        let mut s = sample(&[act("/a", true, Some(1000)), act("/a", true, Some(600_000)), act("/b", true, Some(600_000)), act("/c", false, Some(0)), act("", true, Some(0))], 30_000);
        s.sort();
        assert_eq!(s, vec![("/a".to_string(), 30_000, 30_000), ("/b".to_string(), 30_000, 0)]);
    }
}
