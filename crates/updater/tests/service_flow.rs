#![cfg(any(target_os = "macos", target_os = "windows"))]

use std::{
    fs,
    io::{Read as _, Write as _},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use std::{os::unix::fs::PermissionsExt as _, process::Command};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey};
use semver::Version;
use sha2::{Digest as _, Sha256};
use updater::{
    INSTALLATION_IDENTITY_FILE_NAME, ReleaseStatus, SignedUpdateManifest, UpdateArtifact,
    UpdateChannel, UpdateConfig, UpdateEvent, UpdateManifest, UpdateManifestSignature,
    UpdateTarget, Updater,
};
#[cfg(target_os = "windows")]
use zip::{ZipWriter, write::SimpleFileOptions};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct StagedFixture {
    root: PathBuf,
    cache: PathBuf,
    config: UpdateConfig,
    staged: Option<updater::StagedUpdate>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Drop for StagedFixture {
    fn drop(&mut self) {
        self.staged.take();
        _ = fs::remove_dir_all(&self.root);
    }
}

fn test_root(name: &str) -> PathBuf {
    let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "nexora-updater-pending-{}-{id}-{name}",
        std::process::id()
    ))
}

#[cfg(target_os = "macos")]
fn create_test_app(root: &Path, name: &str) -> PathBuf {
    let app = root.join(format!("{name}.app"));
    let macos = app.join("Contents/MacOS");
    fs::create_dir_all(&macos).unwrap();
    let executable = macos.join(name);
    fs::write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.{name}</string>
<key>CFBundleExecutable</key><string>{name}</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
        ),
    )
    .unwrap();
    let resources = app.join("Contents/Resources");
    fs::create_dir_all(&resources).unwrap();
    fs::write(
        resources.join(INSTALLATION_IDENTITY_FILE_NAME),
        "com.example.pending-flow\n",
    )
    .unwrap();
    let status = Command::new("/usr/bin/codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&app)
        .status()
        .unwrap();
    assert!(status.success());
    app
}

#[cfg(target_os = "windows")]
fn create_test_app(root: &Path, name: &str) -> PathBuf {
    let app = root.join(name);
    fs::create_dir_all(&app).unwrap();
    let (main_exe, updater_exe) = current_test_exe_names();
    write_current_arch_pe(&app.join(main_exe));
    write_current_arch_pe(&app.join(updater_exe));
    fs::write(app.join("nexora-updater.json"), b"{}").unwrap();
    fs::write(
        app.join(INSTALLATION_IDENTITY_FILE_NAME),
        "com.example.pending-flow\n",
    )
    .unwrap();
    app
}

#[cfg(target_os = "windows")]
fn current_test_exe_names() -> (String, String) {
    let main = std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let stem = main.strip_suffix(".exe").unwrap();
    let updater = format!("{stem}-updater.exe");
    (main, updater)
}

#[cfg(target_os = "windows")]
fn write_current_arch_pe(path: &Path) {
    let machine = if cfg!(target_arch = "x86_64") {
        0x8664_u16
    } else if cfg!(target_arch = "aarch64") {
        0xaa64_u16
    } else {
        panic!("unsupported Windows test architecture");
    };
    let pe_offset = 0x80_usize;
    let mut bytes = vec![0_u8; 0x100];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&(pe_offset as u32).to_le_bytes());
    bytes[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
    bytes[pe_offset + 4..pe_offset + 6].copy_from_slice(&machine.to_le_bytes());
    bytes[pe_offset + 24..pe_offset + 26].copy_from_slice(&0x20b_u16.to_le_bytes());
    fs::write(path, bytes).unwrap();
}

#[cfg(target_os = "macos")]
fn package_test_app(app: &Path, archive: &Path) {
    let status = Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(app)
        .arg(archive)
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(target_os = "windows")]
fn package_test_app(app: &Path, archive: &Path) {
    let file = fs::File::create(archive).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for entry in fs::read_dir(app).unwrap() {
        let entry = entry.unwrap();
        zip.start_file(entry.file_name().to_string_lossy(), options)
            .unwrap();
        zip.write_all(&fs::read(entry.path()).unwrap()).unwrap();
    }
    zip.finish().unwrap();
}

fn artifact_kind() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows_update_zip"
    } else {
        "macos_app_zip"
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn stage_update(name: &str, version: &str, build_number: u64) -> StagedFixture {
    stage_update_in_cache(name, version, build_number, None)
}

fn stage_update_in_cache(
    name: &str,
    version: &str,
    build_number: u64,
    cache_override: Option<PathBuf>,
) -> StagedFixture {
    let root = test_root(name);
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    let staged_app = create_test_app(&source, "Fixture");
    let archive = root.join(if cfg!(target_os = "windows") {
        "update.windows.zip"
    } else {
        "update.app.zip"
    });
    package_test_app(&staged_app, &archive);
    let artifact = fs::read(&archive).unwrap();

    let signing_key = SigningKey::from_bytes(&[29_u8; 32]);
    let target = UpdateTarget::current().unwrap();
    let payload = UpdateManifest {
        manifest_sequence: build_number,
        app_id: "com.example.pending-flow".to_owned(),
        channel: UpdateChannel::Stable,
        version: Version::parse(version).unwrap(),
        build_number,
        minimum_supported_version: Version::parse("1.0.0").unwrap(),
        published_at: 1_784_304_000,
        status: ReleaseStatus::Available,
        notes_url: None,
        notes_sha256: None,
        notes_size: None,
        artifacts: vec![UpdateArtifact {
            target: target.as_str().to_owned(),
            url: archive.file_name().unwrap().to_string_lossy().into_owned(),
            sha256: sha256(&artifact),
            size: artifact.len() as u64,
            kind: artifact_kind().to_owned(),
        }],
    };
    let signature = signing_key.sign(&serde_json::to_vec(&payload).unwrap());
    let manifest = serde_json::to_vec(&SignedUpdateManifest {
        schema_version: 1,
        payload,
        signatures: vec![UpdateManifestSignature {
            key_id: "pending-test".to_owned(),
            algorithm: "ed25519".to_owned(),
            signature: STANDARD.encode(signature.to_bytes()),
        }],
    })
    .unwrap();
    let trusted_key = format!(
        "pending-test:ed25519:{}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server_requests = requests.clone();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap()
                .to_owned();
            server_requests.lock().unwrap().push(path.clone());
            let body = if path == "/latest.json" {
                &manifest
            } else {
                &artifact
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        }
    });

    let cache = cache_override.unwrap_or_else(|| root.join("cache"));
    let current_app = create_test_app(&root, "Current");
    let config = UpdateConfig::new(
        format!("http://{address}/latest.json"),
        "com.example.pending-flow",
        "1.0.0",
        1,
        UpdateChannel::Stable,
    )
    .unwrap()
    .with_trusted_public_keys([trusted_key])
    .unwrap()
    .with_app_bundle_path(current_app)
    .with_cache_dir(&cache);
    #[cfg(target_os = "macos")]
    let config = {
        let sidecar = root.join("sidecar");
        fs::write(&sidecar, b"#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o755)).unwrap();
        config.with_sidecar_path(sidecar)
    };
    let session = Updater::new(config.clone()).start().unwrap();
    let events = session.events();
    let staged = loop {
        match events.recv_blocking().unwrap() {
            UpdateEvent::ReadyToRestart(staged) => break staged,
            UpdateEvent::Failed(message) => panic!("暂存更新失败: {message}"),
            _ => {}
        }
    };
    server.join().unwrap();

    StagedFixture {
        root,
        cache,
        config,
        staged: Some(staged),
        requests,
    }
}

fn pending_record(cache: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(cache.join("pending.json")).unwrap()).unwrap()
}

fn write_pending_record(cache: &Path, record: &serde_json::Value) {
    fs::write(
        cache.join("pending.json"),
        serde_json::to_vec(record).unwrap(),
    )
    .unwrap();
}

fn config_for_current(fixture: &StagedFixture, version: &str, build_number: u64) -> UpdateConfig {
    let signing_key = SigningKey::from_bytes(&[29_u8; 32]);
    let trusted_key = format!(
        "pending-test:ed25519:{}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    );
    let config = UpdateConfig::new(
        fixture.config.manifest_url().as_str(),
        "com.example.pending-flow",
        version,
        build_number,
        UpdateChannel::Stable,
    )
    .unwrap()
    .with_trusted_public_keys([trusted_key])
    .unwrap()
    .with_app_bundle_path(fixture.root.join(if cfg!(target_os = "windows") {
        "Current"
    } else {
        "Current.app"
    }))
    .with_cache_dir(&fixture.cache);
    #[cfg(target_os = "macos")]
    let config = config.with_sidecar_path(fixture.root.join("sidecar"));
    config
}

fn staging_is_empty(cache: &Path) -> bool {
    fs::read_dir(cache.join("staging"))
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

#[test]
fn ordinary_staged_update_is_cleaned_after_drop() {
    let mut fixture = stage_update("ordinary-drop", "1.1.0", 2);
    assert!(!staging_is_empty(&fixture.cache));

    drop(fixture.staged.take());
    let deadline = Instant::now() + Duration::from_secs(3);
    while !staging_is_empty(&fixture.cache) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }

    assert!(staging_is_empty(&fixture.cache));
    assert!(!fixture.cache.join("pending.json").exists());
}

#[test]
fn preserved_update_survives_drop_and_restores_without_artifact_request() {
    let mut fixture = stage_update("preserve-restore", "1.1.0", 2);
    fixture
        .staged
        .as_mut()
        .unwrap()
        .preserve_for_next_launch()
        .unwrap();
    drop(fixture.staged.take());

    assert!(fixture.cache.join("pending.json").is_file());
    assert!(staging_is_empty(&fixture.cache));
    let restored = Updater::new(fixture.config.clone())
        .restore_pending()
        .unwrap()
        .expect("有效待安装更新应被恢复");
    assert_eq!(restored.release().version, Version::parse("1.1.0").unwrap());
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.ends_with(".zip") && path.as_str() != "/latest.json")
            .count(),
        1,
        "恢复待安装更新不得再次请求 artifact"
    );
}

#[test]
fn invalid_pending_records_are_cleaned_without_blocking_startup() {
    let mut fixture = stage_update("invalid-record", "1.1.0", 2);
    fixture
        .staged
        .as_mut()
        .unwrap()
        .preserve_for_next_launch()
        .unwrap();
    drop(fixture.staged.take());
    fs::write(fixture.cache.join("pending.json"), b"{not-json").unwrap();

    assert!(
        Updater::new(fixture.config.clone())
            .restore_pending()
            .unwrap()
            .is_none()
    );
    assert!(!fixture.cache.join("pending.json").exists());
    assert!(!fixture.cache.join("pending").exists());
}

#[test]
fn missing_or_tampered_pending_files_are_rejected() {
    for failure in ["missing", "signature"] {
        let mut fixture = stage_update(failure, "1.1.0", 2);
        fixture
            .staged
            .as_mut()
            .unwrap()
            .preserve_for_next_launch()
            .unwrap();
        drop(fixture.staged.take());
        let mut record = pending_record(&fixture.cache);
        if failure == "missing" {
            let archive = record["archive_path"].as_str().unwrap();
            fs::remove_file(fixture.cache.join(archive)).unwrap();
        } else {
            record["manifest"]["signatures"][0]["signature"] =
                serde_json::Value::String(STANDARD.encode([0_u8; 64]));
            write_pending_record(&fixture.cache, &record);
        }

        assert!(
            Updater::new(fixture.config.clone())
                .restore_pending()
                .unwrap()
                .is_none(),
            "{failure} 待安装缓存必须被拒绝"
        );
        assert!(!fixture.cache.join("pending").exists());
    }
}

#[test]
fn escaped_paths_and_corrupted_archives_are_rejected() {
    for failure in ["path", "archive"] {
        let mut fixture = stage_update(failure, "1.1.0", 2);
        fixture
            .staged
            .as_mut()
            .unwrap()
            .preserve_for_next_launch()
            .unwrap();
        drop(fixture.staged.take());
        let mut record = pending_record(&fixture.cache);
        if failure == "path" {
            record["staged_app"] = serde_json::Value::String("../Outside.app".to_owned());
            write_pending_record(&fixture.cache, &record);
        } else {
            let archive = record["archive_path"].as_str().unwrap();
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(fixture.cache.join(archive))
                .unwrap();
            file.write_all(b"tampered").unwrap();
        }

        assert!(
            Updater::new(fixture.config.clone())
                .restore_pending()
                .unwrap()
                .is_none(),
            "{failure} 待安装缓存必须被拒绝"
        );
        assert!(!fixture.cache.join("pending").exists());
    }
}

#[test]
fn pending_release_not_newer_than_current_is_cleaned() {
    let mut fixture = stage_update("already-current", "1.1.0", 2);
    fixture
        .staged
        .as_mut()
        .unwrap()
        .preserve_for_next_launch()
        .unwrap();
    drop(fixture.staged.take());
    let updated_config = config_for_current(&fixture, "1.1.0", 2);

    assert!(
        Updater::new(updated_config)
            .restore_pending()
            .unwrap()
            .is_none()
    );
    assert!(!fixture.cache.join("pending").exists());
}

#[test]
fn newer_preserved_update_atomically_replaces_previous_pending_version() {
    let mut first = stage_update("replace-first", "1.1.0", 2);
    first
        .staged
        .as_mut()
        .unwrap()
        .preserve_for_next_launch()
        .unwrap();
    drop(first.staged.take());

    let mut second = stage_update_in_cache("replace-second", "1.2.0", 3, Some(first.cache.clone()));
    second
        .staged
        .as_mut()
        .unwrap()
        .preserve_for_next_launch()
        .unwrap();
    drop(second.staged.take());

    let pending_roots = fs::read_dir(first.cache.join("pending"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .count();
    assert_eq!(pending_roots, 1);
    let restored = Updater::new(config_for_current(&first, "1.0.0", 1))
        .restore_pending()
        .unwrap()
        .unwrap();
    assert_eq!(restored.release().version, Version::parse("1.2.0").unwrap());
    assert_eq!(restored.release().build_number, 3);
}

#[test]
fn check_reports_available_release_without_requesting_artifact() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let signing_key = SigningKey::from_bytes(&[17_u8; 32]);
    let target = UpdateTarget::current().unwrap();
    let payload = UpdateManifest {
        manifest_sequence: 1,
        app_id: "com.example.updater-flow".to_owned(),
        channel: UpdateChannel::Stable,
        version: Version::parse("1.1.0").unwrap(),
        build_number: 2,
        minimum_supported_version: Version::parse("1.0.0").unwrap(),
        published_at: 1_784_304_000,
        status: ReleaseStatus::Available,
        notes_url: None,
        notes_sha256: None,
        notes_size: None,
        artifacts: vec![UpdateArtifact {
            target: target.as_str().to_owned(),
            url: "update.app.zip".to_owned(),
            sha256: "00".repeat(32),
            size: 1024,
            kind: "macos_app_zip".to_owned(),
        }],
    };
    let signature = signing_key.sign(&serde_json::to_vec(&payload).unwrap());
    let manifest = serde_json::to_vec(&SignedUpdateManifest {
        schema_version: 1,
        payload,
        signatures: vec![UpdateManifestSignature {
            key_id: "test".to_owned(),
            algorithm: "ed25519".to_owned(),
            signature: STANDARD.encode(signature.to_bytes()),
        }],
    })
    .unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            manifest.len()
        )
        .unwrap();
        stream.write_all(&manifest).unwrap();
    });
    let trusted_key = format!(
        "test:ed25519:{}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    );
    let config = UpdateConfig::new(
        format!("http://{address}/latest.json"),
        "com.example.updater-flow",
        "1.0.0",
        1,
        UpdateChannel::Stable,
    )
    .unwrap()
    .with_trusted_public_keys([trusted_key])
    .unwrap();

    let session = Updater::new(config).check().unwrap();
    let events = session.events();

    assert!(matches!(
        events.recv_blocking().unwrap(),
        UpdateEvent::Checking
    ));
    let UpdateEvent::UpdateAvailable(release) = events.recv_blocking().unwrap() else {
        panic!("检查应当返回可安装更新");
    };
    assert_eq!(release.version, Version::parse("1.1.0").unwrap());
    assert!(
        events.recv_blocking().is_err(),
        "仅检查会话在报告版本后必须结束，不能继续请求安装包"
    );
    server.join().unwrap();
}
