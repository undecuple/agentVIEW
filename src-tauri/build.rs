fn main() {
    // Build id shown in Settings > About: the short commit when built from a git checkout, or AGENT_APP_BUILD if set.
    let build = std::env::var("AGENT_APP_BUILD").ok().filter(|s| !s.is_empty() && s.len() <= 40 && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.'))
        .or_else(|| std::process::Command::new("git").args(["rev-parse", "--short=12", "HEAD"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()))
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=AGENT_APP_GIT={build}");
    println!("cargo:rerun-if-env-changed=AGENT_APP_BUILD");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "workspace_snapshot", "workspace_send", "workspace_control",
            "workspace_tick", "workspace_open_docs",
            "live_probe", "live_tools", "live_forget_tool", "live_snapshot", "live_follow_check", "live_use_conversation", "live_create", "live_set_remote", "live_rename", "live_remove", "docs_list", "docs_check", "docs_add", "docs_remove", "docs_read", "live_activity", "live_stop_all", "review_files", "review_load", "review_mark", "review_clear", "notes_list", "claude_config_view", "git_repos", "git_status", "rclone_remotes", "rclone_backup", "webhook_status", "webhook_save", "webhook_clear", "webhook_test", "time_report", "claude_hook_status", "codex_hook_status", "files_access_status", "files_access_report", "tasks_status", "tasks_report", "workflows_report", "subagents_report", "claude_limits_status", "session_changes", "project_file_preview", "kanban_projects", "kanban_board", "kanban_alert", "queues_view", "away_status", "mailbox_status", "mailbox_list", "project_file_write", "timeline_list", "attention_list", "desktop_notify", "claude_settings_view", "claude_inventory", "claude_memory_list", "claude_memory_read", "claude_sessions_list", "notes_get", "notes_save", "notes_delete", "notes_revision", "notes_render", "bundle_export", "bundle_list", "bundle_preview", "bundle_import", "live_start", "live_read",
            "live_write", "live_resize", "live_stop", "usage_snapshot", "usage_refresh",
            "app_settings_snapshot", "app_settings_save", "app_settings_reset", "app_settings_apply", "app_about", "app_diagnostics_export", "app_quit",
        ]),
    )).expect("Tauri build configuration")
}
