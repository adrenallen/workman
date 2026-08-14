//! A desktop binary built for real use must embed the frontend: without
//! `tauri/custom-protocol` the release opens a blank development WebView, the
//! exact failure scripts/tauri-dist-runner.sh refuses to package.

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
