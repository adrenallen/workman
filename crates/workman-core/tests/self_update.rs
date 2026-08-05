use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    thread,
};

use flate2::{Compression, write::GzEncoder};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use workman_core::{ReleaseTarget, UpdateChannel, UpdateClient, UpdateError};

fn archive() -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (name, body) in [
        ("awm", b"new awm".as_slice()),
        ("awmd", b"new awmd".as_slice()),
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
        ("bin/awm", b"new awm".as_slice()),
        ("bin/awmd", b"new awmd".as_slice()),
    ] {
        archive.start_file(name, options).unwrap();
        archive.write_all(body).unwrap();
    }
    archive.finish().unwrap().into_inner()
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
            "awm-fixture.tar.gz",
            "awm-desktop-fixture.zip",
            3,
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
            "browser_download_url": format!("{base}/archive")
        })];
        if desktop_asset != binary_asset {
            assets.push(serde_json::json!({
                "name": desktop_asset,
                "browser_download_url": format!("{base}/desktop")
            }));
        }
        assets.push(serde_json::json!({
            "name": "SHA256SUMS",
            "browser_download_url": format!("{base}/sums")
        }));
        let release = serde_json::json!({
            "tag_name": "v9.0.0",
            "html_url": format!("{base}/release"),
            "body": "Fixture release notes",
            "assets": assets
        })
        .to_string()
        .into_bytes();
        let responses = Arc::new(HashMap::from([
            ("/latest".to_owned(), release),
            ("/archive".to_owned(), archive),
            (
                "/sums".to_owned(),
                format!("{checksum}  {binary_asset}\n").into_bytes(),
            ),
        ]));
        let thread = thread::spawn(move || {
            for stream in listener.incoming().flatten().take(request_count) {
                respond(stream, &responses);
            }
        });
        Self {
            base,
            _thread: thread,
        }
    }
}

fn respond(mut stream: TcpStream, responses: &HashMap<String, Vec<u8>>) {
    let mut request = [0_u8; 4096];
    let length = stream.read(&mut request).unwrap();
    let request = String::from_utf8_lossy(&request[..length]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
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
            binary_asset_name: "awm-fixture.tar.gz".to_owned(),
            desktop_asset_name: "awm-desktop-fixture.zip".to_owned(),
            platform_label: "test".to_owned(),
        },
    )
    .unwrap()
}

fn fixture_target() -> ReleaseTarget {
    ReleaseTarget {
        binary_asset_name: "awm-fixture.tar.gz".to_owned(),
        desktop_asset_name: "awm-desktop-fixture.zip".to_owned(),
        platform_label: "test".to_owned(),
    }
}

fn seed_install() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("awm"), "old awm").unwrap();
    fs::write(directory.path().join("awmd"), "old awmd").unwrap();
    directory
}

#[tokio::test]
async fn local_release_is_checked_verified_and_swapped_in_temp_install_dir() {
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
        fs::read_to_string(install.path().join("awm")).unwrap(),
        "new awm"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("awmd")).unwrap(),
        "new awmd"
    );
    assert_eq!(report.updated_files.len(), 2);
    assert!(
        report
            .desktop_instruction
            .unwrap()
            .contains("awm-desktop-fixture.zip")
    );
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
        3,
    );
    let install = seed_install();
    let client = UpdateClient::with_target(format!("{}/latest", fixture.base), target).unwrap();

    let check = client.check("0.1.0").await.unwrap();
    let report = client.install(&check, install.path()).await.unwrap();

    assert_eq!(
        fs::read_to_string(install.path().join("awm")).unwrap(),
        "new awm"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("awmd")).unwrap(),
        "new awmd"
    );
    assert!(
        report
            .desktop_instruction
            .unwrap()
            .contains("platform bundle awm-macos-arm64.zip")
    );
}

#[tokio::test]
async fn legacy_target_still_finds_transitional_macos_tarball() {
    let archive = archive();
    let checksum = format!("{:x}", Sha256::digest(&archive));
    let target = ReleaseTarget {
        binary_asset_name: "awm-macos-arm64.tar.gz".to_owned(),
        desktop_asset_name: "awm-macos-arm64.zip".to_owned(),
        platform_label: "legacy macOS updater".to_owned(),
    };
    let fixture = Fixture::start_named(
        archive,
        checksum,
        &target.binary_asset_name,
        &target.desktop_asset_name,
        1,
    );
    let client = UpdateClient::with_target(format!("{}/latest", fixture.base), target).unwrap();

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
        fs::read_to_string(install.path().join("awm")).unwrap(),
        "old awm"
    );
    assert_eq!(
        fs::read_to_string(install.path().join("awmd")).unwrap(),
        "old awmd"
    );
}

#[tokio::test]
async fn stable_ignores_prereleases_while_latest_selects_them() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let stable = serde_json::json!({
        "tag_name": "v1.0.0",
        "html_url": format!("{base}/release/v1.0.0"),
        "body": "Stable",
        "prerelease": false,
        "assets": []
    })
    .to_string()
    .into_bytes();
    let latest = serde_json::json!([
        {
            "tag_name": "v1.1.0",
            "html_url": format!("{base}/release/v1.1.0"),
            "body": "Prerelease",
            "draft": false,
            "prerelease": true,
            "assets": []
        },
        {
            "tag_name": "v1.0.0",
            "html_url": format!("{base}/release/v1.0.0"),
            "body": "Stable",
            "draft": false,
            "prerelease": false,
            "assets": []
        }
    ])
    .to_string()
    .into_bytes();
    let responses = Arc::new(HashMap::from([
        ("/stable".to_owned(), stable),
        ("/latest-channel".to_owned(), latest),
    ]));
    let thread = thread::spawn(move || {
        for stream in listener.incoming().flatten().take(2) {
            respond(stream, &responses);
        }
    });

    let stable = UpdateClient::with_target_for_channel(
        format!("{base}/stable"),
        fixture_target(),
        UpdateChannel::Stable,
    )
    .unwrap()
    .check("0.9.0")
    .await
    .unwrap();
    let latest = UpdateClient::with_target_for_channel(
        format!("{base}/latest-channel"),
        fixture_target(),
        UpdateChannel::Latest,
    )
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
async fn stable_without_a_published_release_is_current() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            respond(stream, &HashMap::new());
        }
    });

    let check = UpdateClient::with_target_for_channel(
        format!("{base}/no-stable-release"),
        fixture_target(),
        UpdateChannel::Stable,
    )
    .unwrap()
    .check("0.0.9")
    .await
    .unwrap();
    thread.join().unwrap();

    assert_eq!(check.current, "0.0.9");
    assert_eq!(check.latest, "0.0.9");
    assert!(!check.available);
    assert_eq!(check.channel, UpdateChannel::Stable);
    assert!(check.checked_at > 0);
}
