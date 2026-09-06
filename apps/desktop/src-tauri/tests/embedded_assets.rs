//! A desktop binary built for real use must embed the frontend: without
//! `tauri/custom-protocol` the release opens a blank development WebView, the
//! exact failure scripts/tauri-dist-runner.sh refuses to package.

#[test]
fn every_window_configuration_disables_background_throttling() {
    // Tauri replaces arrays when merging configs. The dev windows must carry this setting too.
    for source in [
        include_str!("../tauri.conf.json"),
        include_str!("../tauri.dev.conf.json"),
    ] {
        let config: tauri::utils::config::Config = serde_json::from_str(source).unwrap();
        assert!(!config.app.windows.is_empty());
        for window in config.app.windows {
            assert_eq!(
                window.background_throttling,
                Some(tauri::utils::config::BackgroundThrottlingPolicy::Disabled),
                "{} must keep notification delivery active when unfocused",
                window.label
            );
        }
    }
}

#[test]
fn embedded_builds_carry_the_frontend_index() {
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    if tauri::is_dev() {
        // Dev builds load devUrl and embed nothing by design.
        return;
    }
    let keys: Vec<String> = context
        .assets()
        .iter()
        .map(|(key, _)| key.as_ref().to_string())
        .collect();
    assert!(
        keys.iter().any(|key| key == "/index.html"),
        "embedded frontend assets are missing or misnamed: {keys:?}"
    );
}
