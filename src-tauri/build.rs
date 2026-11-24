fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "workspace_snapshot", "workspace_send", "workspace_control",
            "workspace_tick", "workspace_open_docs",
            "live_probe", "live_snapshot", "live_create", "live_set_remote", "live_rename", "live_activity", "live_stop_all", "review_files", "review_load", "review_mark", "review_clear", "notes_list", "claude_config_view", "git_repos", "git_status", "rclone_remotes", "rclone_backup", "notes_get", "notes_save", "notes_delete", "notes_revision", "notes_render", "bundle_export", "bundle_list", "bundle_preview", "bundle_import", "live_start", "live_read",
            "live_write", "live_resize", "live_stop", "usage_snapshot", "usage_refresh",
            "app_settings_snapshot", "app_settings_save", "app_settings_reset", "app_settings_apply", "app_about", "app_diagnostics_export",
        ]),
    )).expect("Tauri build configuration")
}
