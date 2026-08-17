use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
};

use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use workman_core::{
    ApplicationInstallTarget, ReleaseTarget, UpdateChannel, UpdateClient, UpdateError,
    UpdateInstallTarget, UpdateStage,
};

const TEST_UPDATE_KEY: &str = "fixture-update-key";

fn archive() -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (name, body) in [
        ("wrk", b"new wrk".as_slice()),
        ("workmand", b"new workmand".as_slice()),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_path(name).unwrap();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append(&header, body).unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

fn unified_zip_archive() -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    for (name, body) in [
        ("bin/wrk", b"new wrk".as_slice()),
        ("bin/workmand", b"new workmand".as_slice()),
    ] {
        archive.start_file(name, options).unwrap();
        archive.write_all(body).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[cfg(unix)]
fn executable_unified_zip_archive() -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    for (name, body) in [
        (
            "bin/wrk",
            b"#!/bin/sh\nprintf 'workman 9.0.0\\n'\n".as_slice(),
        ),
        (
            "bin/workmand",
            b"#!/bin/sh\nprintf 'workmand 9.0.0\\n'\n".as_slice(),
        ),
    ] {
        archive.start_file(name, options).unwrap();
        archive.write_all(body).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn app_surface_zip_archive(bundle_identifier: &str) -> Vec<u8> {
    app_surface_zip_archive_with_binaries(bundle_identifier, b"new wrk", b"new workmand")
}

#[cfg(unix)]
fn executable_app_surface_zip_archive(bundle_identifier: &str) -> Vec<u8> {
    app_surface_zip_archive_with_binaries(
        bundle_identifier,
        b"#!/bin/sh\nprintf 'workman 9.0.0\\n'\n",
        b"#!/bin/sh\nprintf 'workmand 9.0.0\\n'\n",
    )
}

fn app_surface_zip_archive_with_binaries(
    bundle_identifier: &str,
    wrk: &[u8],
    workmand: &[u8],
) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let executable = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755);
    let regular = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, body, options) in [
        ("bin/wrk", wrk, executable),
        ("bin/workmand", workmand, executable),
        (
            "Workman.app/Contents/MacOS/workman-desktop",
            b"new desktop".as_slice(),
            executable,
        ),
        (
            "Workman.app/Contents/Resources/build-marker",
            b"new app".as_slice(),
            regular,
        ),
    ] {
        archive.start_file(name, options).unwrap();
        archive.write_all(body).unwrap();
    }
    archive
        .start_file("Workman.app/Contents/Info.plist", regular)
        .unwrap();
    archive
        .write_all(
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_identifier}</string>
<key>CFBundleExecutable</key><string>workman-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
            )
            .as_bytes(),
        )
        .unwrap();
    archive.finish().unwrap().into_inner()
}

fn seed_app_bundle(root: &Path, bundle_identifier: &str) -> PathBuf {
    let app = root.join("Applications/Workman QA.app");
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    fs::create_dir_all(contents.join("Resources")).unwrap();
    fs::write(
        contents.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_identifier}</string>
<key>CFBundleExecutable</key><string>workman-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
        ),
    )
    .unwrap();
    fs::write(contents.join("MacOS/workman-desktop"), "old desktop").unwrap();
    fs::write(contents.join("Resources/build-marker"), "old app").unwrap();
    app
}

fn app_install_target(root: &Path, app: PathBuf) -> ApplicationInstallTarget {
    let home = root.join("home");
    let launchers = home.join(".local/bin");
    ApplicationInstallTarget::new(
        app,
        &home,
        vec![launchers.clone()],
        vec![launchers],
        home.join(".local/share/workman/dist"),
    )
}

struct Fixture {
    base: String,
    _thread: thread::JoinHandle<()>,
}

impl Fixture {
    fn start(archive: Vec<u8>, checksum: String) -> Self {
        Self::start_named(
            archive,
            checksum,
            "workman-fixture.tar.gz",
            "workman-desktop-fixture.zip",
            2,
        )
    }

    fn start_named(
        archive: Vec<u8>,
        checksum: String,
        binary_asset: &str,
        desktop_asset: &str,
        request_count: usize,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mut assets = vec![serde_json::json!({
            "name": binary_asset,
            "target": "test-binary",
            "sha256": checksum,
            "size": archive.len(),
            "url": format!("{base}/archive")
        })];
        if desktop_asset != binary_asset {
            assets.push(serde_json::json!({
                "name": desktop_asset,
                "target": "test-desktop",
                "sha256": checksum,
                "size": archive.len(),
                "url": format!("{base}/desktop")
            }));
        }
        assets.push(serde_json::json!({
            "name": "SHA256SUMS",
            "target": "checksums",
            "sha256": "a".repeat(64),
            "size": 1,
            "url": format!("{base}/sums")
        }));
        let release = serde_json::json!({
            "version": "9.0.0",
            "published_at": "2026-08-06T12:00:00Z",
            "notes_url": format!("{base}/release"),
            "assets": assets
        });
        let manifest = serde_json::json!({
            "channels": {
                "stable": release,
                "latest": release,
            }
        })
        .to_string()
        .into_bytes();
        let responses = Arc::new(HashMap::from([
            ("/latest".to_owned(), manifest),
            ("/archive".to_owned(), archive),
        ]));
        let thread = thread::spawn(move || {
            for stream in listener.incoming().flatten().take(request_count) {
                respond(stream, &responses, Some(TEST_UPDATE_KEY));
            }
        });
        Self {
            base,
            _thread: thread,
        }
    }
}

fn respond(
    mut stream: TcpStream,
    responses: &HashMap<String, Vec<u8>>,
    expected_key: Option<&str>,
) {
    let mut request = [0_u8; 4096];
    let length = stream.read(&mut request).unwrap();
    let request = String::from_utf8_lossy(&request[..length]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    if let Some(expected_key) = expected_key {
        let expected = format!("Authorization: Bearer {expected_key}");
        if !request
            .lines()
            .any(|line| line.eq_ignore_ascii_case(&expected))
        {
            let body = b"unauthorized";
            write!(
                stream,
                "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
            return;
        }
    }
    let (status, body) = responses
        .get(path)
        .map(|body| ("200 OK", body.as_slice()))
        .unwrap_or(("404 Not Found", b"missing"));
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    stream.write_all(body).unwrap();
}

fn fixture_client(base: &str) -> UpdateClient {
    UpdateClient::with_target(
        format!("{base}/latest"),
        ReleaseTarget {
            binary_asset_name: "workman-fixture.tar.gz".to_owned(),
            desktop_asset_name: "workman-desktop-fixture.zip".to_owned(),
            platform_label: "test".to_owned(),
        },
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap()
}

fn fixture_target() -> ReleaseTarget {
    ReleaseTarget {
        binary_asset_name: "workman-fixture.tar.gz".to_owned(),
        desktop_asset_name: "workman-desktop-fixture.zip".to_owned(),
        platform_label: "test".to_owned(),
    }
}

fn seed_install() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("wrk"), "old wrk").unwrap();
    fs::write(directory.path().join("workmand"), "old workmand").unwrap();
    directory
}

fn seed_v0_1_0_install() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("awm"), "old awm").unwrap();
    fs::write(directory.path().join("awmd"), "old awmd").unwrap();
    directory
}

#[tokio::test]
async fn bearer_authenticated_manifest_and_artifact_are_verified_and_installed() {
    let archive = archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start(archive, checksum);
    let install = seed_install();
    let client = fixture_client(&fixture.base);

    let check = client.check("0.1.0").await.unwrap();
    assert_eq!(check.latest, "9.0.0");
    assert!(check.available);
    let report = client.install(&check, install.path()).await.unwrap();

    assert_eq!(
        fs::read_to_string(install.path().join("wrk")).unwrap(),
        "new wrk"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("workmand")).unwrap(),
        "new workmand"
    );
    assert_eq!(report.updated_files.len(), 2);
    assert!(
        report
            .desktop_instruction
            .unwrap()
            .contains("workman-desktop-fixture.zip")
    );
    assert!(report.restart_plan.daemon);
    assert!(!report.restart_plan.app);
    assert_eq!(report.installed_app_bundle, None);
}

#[tokio::test]
async fn install_progress_reports_download_verify_install_in_order() {
    let archive = archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start(archive, checksum);
    let install = seed_install();
    let client = fixture_client(&fixture.base);
    let check = client.check("0.1.0").await.unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let captured = progress.clone();

    client
        .install_target_with_progress(
            &check,
            &UpdateInstallTarget::binary_directory(install.path()),
            &move |event| captured.lock().unwrap().push(event),
        )
        .await
        .unwrap();

    let progress = progress.lock().unwrap();
    let stages = progress.iter().map(|event| event.stage).fold(
        Vec::<UpdateStage>::new(),
        |mut stages, stage| {
            if stages.last() != Some(&stage) {
                stages.push(stage);
            }
            stages
        },
    );
    assert_eq!(
        stages,
        [
            UpdateStage::Downloading,
            UpdateStage::Verifying,
            UpdateStage::Installing
        ]
    );
    assert_eq!(progress.first().unwrap().percent, Some(0));
    let download_events = progress
        .iter()
        .filter(|event| event.stage == UpdateStage::Downloading)
        .collect::<Vec<_>>();
    assert!(download_events.len() <= 101);
    assert!(
        download_events
            .windows(2)
            .all(|pair| pair[0].percent != pair[1].percent)
    );
    assert_eq!(
        progress
            .iter()
            .rev()
            .find(|event| event.stage == UpdateStage::Downloading)
            .unwrap()
            .percent,
        Some(100)
    );
}

#[tokio::test]
async fn transitional_install_keeps_legacy_named_pair_updateable() {
    let archive = archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start(archive, checksum);
    let install = seed_v0_1_0_install();
    let client = fixture_client(&fixture.base);

    let check = client.check("0.1.0").await.unwrap();
    let report = client.install(&check, install.path()).await.unwrap();

    assert_eq!(
        fs::read_to_string(install.path().join("awm")).unwrap(),
        "new wrk"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("awmd")).unwrap(),
        "new workmand"
    );
    assert_eq!(report.updated_files.len(), 2);
}

#[tokio::test]
async fn unified_macos_zip_updates_binaries_from_bin_directory() {
    let archive = unified_zip_archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let target = ReleaseTarget::for_platform("macos", "aarch64").unwrap();
    let fixture = Fixture::start_named(
        archive,
        checksum,
        &target.binary_asset_name,
        &target.desktop_asset_name,
        2,
    );
    let install = seed_install();
    let client = UpdateClient::with_target(format!("{}/latest", fixture.base), target)
        .unwrap()
        .with_key(TEST_UPDATE_KEY)
        .unwrap();

    let check = client.check("0.1.0").await.unwrap();
    let report = client.install(&check, install.path()).await.unwrap();

    assert_eq!(
        fs::read_to_string(install.path().join("wrk")).unwrap(),
        "new wrk"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("workmand")).unwrap(),
        "new workmand"
    );
    assert!(
        report
            .desktop_instruction
            .unwrap()
            .contains("platform bundle workman-macos-arm64.zip")
    );
}

#[cfg(unix)]
#[test]
fn versioned_cli_update_honors_injected_layout_and_survives_old_source_removal() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let launcher_dir = root.path().join("injected-path");
    let versioned_root = root.path().join("injected-dist");
    let old_version = versioned_root.join("0.1.5");
    let old_extraction = root.path().join("old-extraction");
    let old_bin = old_extraction.join("bin");
    fs::create_dir_all(&old_bin).unwrap();
    fs::create_dir_all(old_version.join("bin")).unwrap();
    fs::create_dir_all(&launcher_dir).unwrap();
    fs::write(old_bin.join("wrk"), "old wrk").unwrap();
    fs::write(old_bin.join("workmand"), "old workmand").unwrap();
    fs::write(old_version.join("bin/wrk"), "historical wrk").unwrap();
    fs::write(old_version.join("bin/workmand"), "historical workmand").unwrap();
    symlink(old_bin.join("wrk"), launcher_dir.join("wrk")).unwrap();
    symlink(old_bin.join("workmand"), launcher_dir.join("workmand")).unwrap();

    let archive = executable_unified_zip_archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start_named(
        archive,
        checksum,
        "workman-update-fixture.zip",
        "workman-update-fixture.zip",
        2,
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "versioned_cli_update_injected_environment_child",
            "--nocapture",
        ])
        .env("WORKMAN_UPDATE_LAYOUT_CHILD", "1")
        .env(
            "WORKMAN_UPDATE_TEST_API",
            format!("{}/latest", fixture.base),
        )
        .env("WORKMAN_UPDATE_TEST_EXECUTABLE", old_bin.join("wrk"))
        .env("WORKMAN_UPDATE_TEST_OLD_EXTRACTION", &old_extraction)
        .env("WORKMAN_UPDATE_TEST_OLD_VERSION", &old_version)
        .env("WORKMAN_UPDATE_HOME", &home)
        .env("WORKMAN_UPDATE_PATH", &launcher_dir)
        .env("WORKMAN_UPDATE_VERSION_ROOT", &versioned_root)
        .env("WORKMAN_INSTALL_TEST_ROOT", root.path().join("system-root"))
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "child updater regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!old_extraction.exists());
    assert!(!old_version.exists());
    assert_eq!(
        fs::canonicalize(launcher_dir.join("wrk")).unwrap(),
        fs::canonicalize(versioned_root.join("9.0.0/bin/wrk")).unwrap()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn versioned_cli_update_injected_environment_child() {
    if std::env::var_os("WORKMAN_UPDATE_LAYOUT_CHILD").is_none() {
        return;
    }

    let home = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_HOME").unwrap());
    let launcher_dir = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_PATH").unwrap());
    let versioned_root = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_VERSION_ROOT").unwrap());
    let executable = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_TEST_EXECUTABLE").unwrap());
    let old_extraction =
        PathBuf::from(std::env::var_os("WORKMAN_UPDATE_TEST_OLD_EXTRACTION").unwrap());
    let old_version = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_TEST_OLD_VERSION").unwrap());
    let target = UpdateInstallTarget::discover(&executable).unwrap();
    let UpdateInstallTarget::VersionedBinary(discovered) = &target else {
        panic!("CLI discovery did not choose the durable versioned target");
    };
    assert_eq!(
        discovered.current_binary_dir,
        fs::canonicalize(executable.parent().unwrap()).unwrap()
    );
    assert_eq!(discovered.home_dir, home);
    assert_eq!(discovered.search_path, vec![launcher_dir.clone()]);
    assert_eq!(discovered.versioned_root, versioned_root);

    let client = UpdateClient::with_target(
        std::env::var("WORKMAN_UPDATE_TEST_API").unwrap(),
        ReleaseTarget {
            binary_asset_name: "workman-update-fixture.zip".to_owned(),
            desktop_asset_name: "workman-update-fixture.zip".to_owned(),
            platform_label: "injected updater layout".to_owned(),
        },
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap();
    let check = client.check("0.1.5").await.unwrap();
    let report = client.install_target(&check, &target).await.unwrap();
    let new_bin = versioned_root.join("9.0.0/bin");
    assert_eq!(
        report.install_dir,
        versioned_root.join("9.0.0").display().to_string()
    );
    assert_eq!(
        fs::canonicalize(launcher_dir.join("wrk")).unwrap(),
        fs::canonicalize(new_bin.join("wrk")).unwrap()
    );
    assert_eq!(
        fs::canonicalize(home.join(".local/bin/workmand")).unwrap(),
        fs::canonicalize(new_bin.join("workmand")).unwrap()
    );

    fs::remove_dir_all(old_extraction).unwrap();
    fs::remove_dir_all(old_version).unwrap();
    let version_output = Command::new(launcher_dir.join("wrk"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version_output.status.success());
    assert_eq!(
        String::from_utf8(version_output.stdout).unwrap(),
        "workman 9.0.0\n"
    );
}

#[cfg(unix)]
#[test]
fn app_surface_recovery_honors_todo66_identity_and_injected_durable_layout() {
    use std::os::unix::fs::symlink;

    let root = tempfile::Builder::new()
        .prefix("workman-todo66-recovery-")
        .tempdir_in("/tmp")
        .unwrap();
    let home = root.path().join("home");
    let data_dir = root.path().join("com.workman.todo66-data");
    let config = root.path().join("com.workman.todo66-config/config.yml");
    let launcher_dir = home.join(".local/bin");
    let versioned_root = home.join(".local/share/workman/dist");
    let missing_bin = versioned_root.join("0.1.2/bin");
    fs::create_dir_all(&launcher_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    symlink(missing_bin.join("wrk"), launcher_dir.join("wrk")).unwrap();
    symlink(missing_bin.join("workmand"), launcher_dir.join("workmand")).unwrap();
    assert!(
        !versioned_root.exists(),
        "the simulated install directory must be missing"
    );

    let app = seed_app_bundle(root.path(), "com.workman.todo66");
    let app_executable = app.join("Contents/MacOS/workman-desktop");
    let archive = executable_app_surface_zip_archive("com.workman.todo66");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start_named(
        archive,
        checksum,
        "workman-app-fixture.zip",
        "workman-app-fixture.zip",
        2,
    );
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "app_surface_recovery_injected_environment_child",
            "--nocapture",
        ])
        .env("WORKMAN_APP_RECOVERY_CHILD", "1")
        .env(
            "WORKMAN_UPDATE_TEST_API",
            format!("{}/latest", fixture.base),
        )
        .env("WORKMAN_UPDATE_TEST_EXECUTABLE", &app_executable)
        .env("WORKMAN_DATA_DIR", &data_dir)
        .env("WORKMAN_CONFIG", &config)
        .env("WORKMAN_UPDATE_HOME", &home)
        .env("WORKMAN_UPDATE_PATH", &launcher_dir)
        .env("WORKMAN_UPDATE_VERSION_ROOT", &versioned_root)
        .env("WORKMAN_INSTALL_TEST_ROOT", root.path().join("system-root"))
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "isolated app recovery failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let durable_bin = versioned_root.join("9.0.0/bin");
    assert_eq!(
        fs::canonicalize(launcher_dir.join("wrk")).unwrap(),
        fs::canonicalize(durable_bin.join("wrk")).unwrap()
    );
    assert_eq!(
        fs::canonicalize(launcher_dir.join("workmand")).unwrap(),
        fs::canonicalize(durable_bin.join("workmand")).unwrap()
    );
    let version = Command::new(launcher_dir.join("wrk"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        "workman 9.0.0\n"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn app_surface_recovery_injected_environment_child() {
    if std::env::var_os("WORKMAN_APP_RECOVERY_CHILD").is_none() {
        return;
    }

    let executable = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_TEST_EXECUTABLE").unwrap());
    let home = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_HOME").unwrap());
    let launcher_dir = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_PATH").unwrap());
    let versioned_root = PathBuf::from(std::env::var_os("WORKMAN_UPDATE_VERSION_ROOT").unwrap());
    let target = UpdateInstallTarget::discover(&executable).unwrap();
    let UpdateInstallTarget::Application(application) = &target else {
        panic!("the isolated desktop executable did not select the app update surface");
    };
    assert_eq!(
        application.app_bundle,
        fs::canonicalize(
            executable
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(application.home_dir, home);
    assert_eq!(application.search_path, vec![launcher_dir.clone()]);
    assert_eq!(application.versioned_root, versioned_root);
    assert!(
        target.cli_recovery_required(),
        "dangling launchers must offer recovery"
    );

    let client = UpdateClient::with_target(
        std::env::var("WORKMAN_UPDATE_TEST_API").unwrap(),
        ReleaseTarget {
            binary_asset_name: "workman-app-fixture.zip".to_owned(),
            desktop_asset_name: "workman-app-fixture.zip".to_owned(),
            platform_label: "todo 66 isolated app recovery".to_owned(),
        },
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap();
    let check = client.check("0.1.2").await.unwrap();
    assert!(check.available);
    let report = client.install_target(&check, &target).await.unwrap();

    assert_eq!(
        report.install_dir,
        versioned_root.join("9.0.0").display().to_string()
    );
    assert!(
        report
            .desktop_instruction
            .as_deref()
            .unwrap()
            .contains("Reinstalled the command-line tools")
    );
    assert!(
        !target.cli_recovery_required(),
        "a repaired install must not keep prompting"
    );
    assert_eq!(
        fs::read_to_string(
            application
                .app_bundle
                .join("Contents/Resources/build-marker")
        )
        .unwrap(),
        "new app"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn app_surface_hop_updates_versioned_layout_launchers_and_matching_app() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let app = seed_app_bundle(root.path(), "com.workman.todo467");
    let target = app_install_target(root.path(), app.clone());
    let old_bin = target.versioned_root.join("0.1.2/bin");
    let launcher_dir = target.home_dir.join(".local/bin");
    fs::create_dir_all(&old_bin).unwrap();
    fs::create_dir_all(&launcher_dir).unwrap();
    fs::write(old_bin.join("wrk"), "old wrk").unwrap();
    fs::write(old_bin.join("workmand"), "old workmand").unwrap();
    symlink(old_bin.join("wrk"), launcher_dir.join("wrk")).unwrap();
    symlink(old_bin.join("workmand"), launcher_dir.join("workmand")).unwrap();
    let install_target = UpdateInstallTarget::Application(target.clone());
    assert!(
        !install_target.cli_recovery_required(),
        "a complete durable CLI install must not trigger recovery"
    );

    let archive = app_surface_zip_archive("com.workman.todo467");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start_named(
        archive,
        checksum,
        "workman-app-fixture.zip",
        "workman-app-fixture.zip",
        2,
    );
    let client = UpdateClient::with_target(
        format!("{}/latest", fixture.base),
        ReleaseTarget {
            binary_asset_name: "workman-app-fixture.zip".to_owned(),
            desktop_asset_name: "workman-app-fixture.zip".to_owned(),
            platform_label: "test app surface".to_owned(),
        },
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap();

    let check = client.check("0.1.2").await.unwrap();
    let report = client
        .install_target(&check, &install_target)
        .await
        .unwrap();

    let new_bin = target.versioned_root.join("9.0.0/bin");
    assert_eq!(fs::read_to_string(new_bin.join("wrk")).unwrap(), "new wrk");
    assert_eq!(
        fs::read_to_string(new_bin.join("workmand")).unwrap(),
        "new workmand"
    );
    assert_eq!(
        fs::canonicalize(launcher_dir.join("wrk")).unwrap(),
        fs::canonicalize(new_bin.join("wrk")).unwrap()
    );
    assert_eq!(
        fs::canonicalize(launcher_dir.join("workmand")).unwrap(),
        fs::canonicalize(new_bin.join("workmand")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(app.join("Contents/Resources/build-marker")).unwrap(),
        "new app"
    );
    assert!(report.restart_plan.daemon);
    assert!(report.restart_plan.app);
    assert_eq!(
        report.installed_app_bundle.as_deref(),
        Some(app.to_string_lossy().as_ref())
    );
    assert_eq!(
        serde_json::to_value(&report.restart_plan).unwrap(),
        serde_json::json!({ "daemon": true, "app": true })
    );
    let message = report.desktop_instruction.unwrap();
    assert!(message.contains("running app must restart"));
    assert!(message.contains("Updated 2 command-line launchers"));
    assert!(!install_target.cli_recovery_required());
}

#[test]
fn legacy_install_reports_default_to_a_daemon_only_restart_plan() {
    let report: workman_core::UpdateInstallReport = serde_json::from_value(serde_json::json!({
        "current": "0.1.9",
        "latest": "0.1.10",
        "install_dir": "/tmp/workman",
        "updated_files": ["/tmp/workman/wrk", "/tmp/workman/workmand"],
        "desktop_instruction": null,
        "quarantine_cleared": false
    }))
    .unwrap();
    assert_eq!(report.installed_app_bundle, None);
    assert!(report.restart_plan.daemon);
    assert!(!report.restart_plan.app);
}

#[tokio::test]
async fn current_app_surface_reinstalls_a_missing_cli_in_the_durable_layout() {
    let root = tempfile::tempdir().unwrap();
    let app = seed_app_bundle(root.path(), "com.workman.todo467");
    let target = app_install_target(root.path(), app.clone());
    let install_target = UpdateInstallTarget::Application(target.clone());
    assert!(install_target.cli_recovery_required());

    let archive = app_surface_zip_archive("com.workman.todo467");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start_named(
        archive,
        checksum,
        "workman-app-fixture.zip",
        "workman-app-fixture.zip",
        2,
    );
    let client = UpdateClient::with_target(
        format!("{}/latest", fixture.base),
        ReleaseTarget {
            binary_asset_name: "workman-app-fixture.zip".to_owned(),
            desktop_asset_name: "workman-app-fixture.zip".to_owned(),
            platform_label: "test app surface".to_owned(),
        },
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap();

    let check = client.check("9.0.0").await.unwrap();
    assert!(!check.available);
    let report = client
        .install_target(&check, &install_target)
        .await
        .unwrap();

    let durable_bin = target.versioned_root.join("9.0.0/bin");
    let launcher_dir = target.home_dir.join(".local/bin");
    assert_eq!(
        fs::canonicalize(launcher_dir.join("wrk")).unwrap(),
        fs::canonicalize(durable_bin.join("wrk")).unwrap()
    );
    assert_eq!(
        fs::canonicalize(launcher_dir.join("workmand")).unwrap(),
        fs::canonicalize(durable_bin.join("workmand")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(app.join("Contents/Resources/build-marker")).unwrap(),
        "new app"
    );
    let message = report.desktop_instruction.unwrap();
    assert!(message.contains("Reinstalled the command-line tools"));
    assert!(message.contains(".local/bin"));
    assert!(!install_target.cli_recovery_required());
}

#[tokio::test]
async fn app_surface_rejects_mismatched_bundle_identity_before_replacement() {
    let root = tempfile::tempdir().unwrap();
    let app = seed_app_bundle(root.path(), "com.workman.todo467");
    let target = app_install_target(root.path(), app.clone());
    let archive = app_surface_zip_archive("com.workman.desktop");
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start_named(
        archive,
        checksum,
        "workman-app-fixture.zip",
        "workman-app-fixture.zip",
        2,
    );
    let client = UpdateClient::with_target(
        format!("{}/latest", fixture.base),
        ReleaseTarget {
            binary_asset_name: "workman-app-fixture.zip".to_owned(),
            desktop_asset_name: "workman-app-fixture.zip".to_owned(),
            platform_label: "test app surface".to_owned(),
        },
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap();

    let check = client.check("0.1.2").await.unwrap();
    let error = client
        .install_target(&check, &UpdateInstallTarget::Application(target.clone()))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("bundle identifier"));
    assert!(error.to_string().contains("com.workman.desktop"));
    assert!(error.to_string().contains("com.workman.todo467"));
    assert_eq!(
        fs::read_to_string(app.join("Contents/Resources/build-marker")).unwrap(),
        "old app"
    );
    assert!(!target.versioned_root.join("9.0.0").exists());
}

#[tokio::test]
async fn legacy_target_still_finds_transitional_macos_tarball() {
    let archive = archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let target = ReleaseTarget {
        binary_asset_name: "awm-macos-arm64.tar.gz".to_owned(),
        desktop_asset_name: "awm-desktop-macos-arm64.zip".to_owned(),
        platform_label: "legacy macOS updater".to_owned(),
    };
    let fixture = Fixture::start_named(
        archive,
        checksum,
        &target.binary_asset_name,
        &target.desktop_asset_name,
        1,
    );
    let client = UpdateClient::with_target(format!("{}/latest", fixture.base), target)
        .unwrap()
        .with_key(TEST_UPDATE_KEY)
        .unwrap();

    let check = client.check("0.1.0").await.unwrap();
    assert_eq!(check.binary_asset.unwrap().name, "awm-macos-arm64.tar.gz");
}

#[tokio::test]
async fn checksum_mismatch_leaves_both_installed_binaries_untouched() {
    let fixture = Fixture::start(archive(), "0".repeat(64));
    let install = seed_install();
    let client = fixture_client(&fixture.base);
    let check = client.check("0.1.0").await.unwrap();

    let error = client.install(&check, install.path()).await.unwrap_err();
    assert!(matches!(error, UpdateError::ChecksumMismatch { .. }));
    assert_eq!(
        fs::read_to_string(install.path().join("wrk")).unwrap(),
        "old wrk"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("workmand")).unwrap(),
        "old workmand"
    );
}

#[tokio::test]
async fn checksum_failure_is_attributed_to_verifying_before_install_starts() {
    let fixture = Fixture::start(archive(), "0".repeat(64));
    let install = seed_install();
    let client = fixture_client(&fixture.base);
    let check = client.check("0.1.0").await.unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let captured = progress.clone();

    let error = client
        .install_target_with_progress(
            &check,
            &UpdateInstallTarget::binary_directory(install.path()),
            &move |event| captured.lock().unwrap().push(event),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, UpdateError::ChecksumMismatch { .. }));
    let progress = progress.lock().unwrap();
    assert_eq!(progress.last().unwrap().stage, UpdateStage::Verifying);
    assert!(
        progress
            .iter()
            .all(|event| event.stage != UpdateStage::Installing)
    );
}

#[tokio::test]
async fn stable_ignores_prereleases_while_latest_selects_them() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let stable = serde_json::json!({
        "version": "1.0.0",
        "published_at": "2026-08-01T12:00:00Z",
        "notes_url": format!("{base}/release/v1.0.0"),
        "assets": []
    });
    let latest = serde_json::json!({
        "version": "1.1.0",
        "published_at": "2026-08-06T12:00:00Z",
        "notes_url": format!("{base}/release/v1.1.0"),
        "assets": []
    });
    let manifest = serde_json::json!({
        "channels": { "stable": stable, "latest": latest }
    })
    .to_string()
    .into_bytes();
    let responses = Arc::new(HashMap::from([("/manifest".to_owned(), manifest)]));
    let thread = thread::spawn(move || {
        for stream in listener.incoming().flatten().take(2) {
            respond(stream, &responses, Some(TEST_UPDATE_KEY));
        }
    });

    let stable = UpdateClient::with_target_for_channel(
        format!("{base}/manifest"),
        fixture_target(),
        UpdateChannel::Stable,
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap()
    .check("0.9.0")
    .await
    .unwrap();
    let latest = UpdateClient::with_target_for_channel(
        format!("{base}/manifest"),
        fixture_target(),
        UpdateChannel::Latest,
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap()
    .check("0.9.0")
    .await
    .unwrap();
    thread.join().unwrap();

    assert_eq!(stable.latest, "1.0.0");
    assert_eq!(stable.channel, UpdateChannel::Stable);
    assert!(!stable.prerelease);
    assert_eq!(latest.latest, "1.1.0");
    assert_eq!(latest.channel, UpdateChannel::Latest);
    assert!(latest.prerelease);
}

#[tokio::test]
async fn http_failure_is_never_reported_as_up_to_date() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            respond(stream, &HashMap::new(), Some(TEST_UPDATE_KEY));
        }
    });

    let error = UpdateClient::with_target_for_channel(
        format!("{base}/no-stable-release"),
        fixture_target(),
        UpdateChannel::Stable,
    )
    .unwrap()
    .with_key(TEST_UPDATE_KEY)
    .unwrap()
    .check("0.0.9")
    .await
    .unwrap_err();
    thread.join().unwrap();

    assert!(matches!(error, UpdateError::CheckFailed(_)));
    assert!(error.to_string().starts_with("couldn't check for updates:"));
}

#[tokio::test]
async fn malformed_manifest_surfaces_an_honest_check_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let responses = Arc::new(HashMap::from([(
        "/manifest".to_owned(),
        br#"{"unexpected":true}"#.to_vec(),
    )]));
    let thread = thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            respond(stream, &responses, Some(TEST_UPDATE_KEY));
        }
    });

    let error = UpdateClient::with_target(format!("{base}/manifest"), fixture_target())
        .unwrap()
        .with_key(TEST_UPDATE_KEY)
        .unwrap()
        .check("0.0.9")
        .await
        .unwrap_err();
    thread.join().unwrap();

    assert!(matches!(error, UpdateError::CheckFailed(_)));
    assert!(error.to_string().starts_with("couldn't check for updates:"));
    assert!(error.to_string().contains("missing field `channels`"));
}

#[tokio::test]
async fn rejected_manifest_key_is_an_honest_check_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            respond(stream, &HashMap::new(), Some(TEST_UPDATE_KEY));
        }
    });

    let error = UpdateClient::with_target(format!("{base}/manifest"), fixture_target())
        .unwrap()
        .with_key("wrong-key")
        .unwrap()
        .check("0.0.9")
        .await
        .unwrap_err();
    thread.join().unwrap();

    assert!(matches!(error, UpdateError::CheckFailed(_)));
    assert_eq!(
        error.to_string(),
        "couldn't check for updates: update server rejected our key"
    );
}

#[tokio::test]
async fn rejected_artifact_key_is_an_honest_install_failure() {
    let archive = archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let fixture = Fixture::start(archive, checksum);
    let install = seed_install();
    let client = fixture_client(&fixture.base);
    let check = client.check("0.1.0").await.unwrap();

    let error = client
        .with_key("wrong-key")
        .unwrap()
        .install(&check, install.path())
        .await
        .unwrap_err();

    assert!(matches!(error, UpdateError::RejectedKey));
    assert_eq!(error.to_string(), "update server rejected our key");
    assert_eq!(
        fs::read_to_string(install.path().join("wrk")).unwrap(),
        "old wrk"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("workmand")).unwrap(),
        "old workmand"
    );
}
