fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "workspace_snapshot", "workspace_send", "workspace_control",
            "workspace_tick", "workspace_open_docs",
            "live_probe", "live_snapshot", "live_create", "live_set_remote", "live_rename", "review_files", "review_load", "review_mark", "review_clear", "live_start", "live_read",
            "live_write", "live_resize", "live_stop", "usage_snapshot", "usage_refresh",
            "app_settings_snapshot", "app_settings_save", "app_settings_reset", "app_settings_apply", "app_about", "app_diagnostics_export",
        ]),
    )).expect("Tauri build configuration")
}
