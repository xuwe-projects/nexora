use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer as _, SigningKey};
use semver::Version;
use updater::{
    ReleaseStatus, SignedUpdateManifest, TrustedPublicKey, UpdateArtifact, UpdateChannel,
    UpdateConfig, UpdateError, UpdateManifest, UpdateManifestSignature, UpdateTarget,
};

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7_u8; 32])
}

fn trusted_key(key_id: &str, signing_key: &SigningKey) -> String {
    format!(
        "{key_id}:ed25519:{}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    )
}

fn payload() -> UpdateManifest {
    UpdateManifest {
        manifest_sequence: 42,
        app_id: "com.example.console".to_owned(),
        channel: UpdateChannel::Beta,
        version: Version::parse("1.2.0-beta.2").unwrap(),
        build_number: 1043,
        minimum_supported_version: Version::parse("1.1.0").unwrap(),
        published_at: 1_784_304_000,
        status: ReleaseStatus::Available,
        notes_url: Some("https://updates.example.com/notes.md".to_owned()),
        artifacts: vec![UpdateArtifact {
            target: "aarch64-apple-darwin".to_owned(),
            url: "console.app.zip".to_owned(),
            sha256: "abc123".to_owned(),
            size: 1024,
            kind: "macos_app_zip".to_owned(),
        }],
    }
}

fn signed_manifest(key_id: &str, signing_key: &SigningKey) -> String {
    let payload = payload();
    let payload_bytes = serde_json::to_vec(&payload).unwrap();
    let signature = signing_key.sign(&payload_bytes);
    let envelope = SignedUpdateManifest {
        schema_version: 1,
        payload,
        signatures: vec![UpdateManifestSignature {
            key_id: key_id.to_owned(),
            algorithm: "ed25519".to_owned(),
            signature: STANDARD.encode(signature.to_bytes()),
        }],
    };
    serde_json::to_string(&envelope).unwrap()
}

#[test]
fn update_config_rejects_unsafe_application_identifiers() {
    for app_id in ["", ".", "..", ".hidden", "hidden.", "../console", "铉微"] {
        let error = UpdateConfig::new(
            "https://updates.example.com/stable/latest.json",
            app_id,
            "1.0.0",
            1,
            UpdateChannel::Stable,
        )
        .expect_err("不安全的应用标识必须被拒绝");

        assert!(matches!(error, UpdateError::InvalidAppId));
    }
}

#[test]
fn http_requires_explicit_allowance_outside_loopback() {
    let error = UpdateConfig::new(
        "http://updates.example.com/stable/latest.json",
        "com.example.app",
        "1.0.0",
        1,
        UpdateChannel::Stable,
    )
    .expect_err("非 loopback HTTP 默认必须拒绝");

    assert!(matches!(error, UpdateError::InsecureHttpDenied));
    assert!(
        UpdateConfig::with_transport_policy(
            "http://updates.example.com/stable/latest.json",
            "com.example.app",
            "1.0.0",
            1,
            UpdateChannel::Stable,
            true,
        )
        .is_ok()
    );
}

#[test]
fn signed_manifest_accepts_trusted_key_and_rotation() {
    let main = signing_key();
    let backup = SigningKey::from_bytes(&[9_u8; 32]);
    let trusted = vec![
        TrustedPublicKey::parse(&trusted_key("backup", &backup)).unwrap(),
        TrustedPublicKey::parse(&trusted_key("main", &main)).unwrap(),
    ];
    let manifest = signed_manifest("main", &main);

    let verified =
        SignedUpdateManifest::parse_and_verify(&manifest, &trusted).expect("签名应通过验证");

    assert_eq!(verified.manifest_sequence, 42);
    assert_eq!(verified.build_number, 1043);
}

#[test]
fn signed_manifest_rejects_unknown_key() {
    let main = signing_key();
    let other = SigningKey::from_bytes(&[11_u8; 32]);
    let trusted = vec![TrustedPublicKey::parse(&trusted_key("other", &other)).unwrap()];
    let manifest = signed_manifest("main", &main);

    let error =
        SignedUpdateManifest::parse_and_verify(&manifest, &trusted).expect_err("未知 key 必须拒绝");

    assert!(matches!(error, UpdateError::ManifestSignatureRejected));
}

#[test]
fn higher_build_number_updates_same_semver() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.example.console",
        "1.2.0-beta.2",
        1042,
        UpdateChannel::Beta,
    )
    .unwrap();
    let manifest = payload();

    let release = manifest
        .select_update(&config, UpdateTarget::MacOsAarch64)
        .expect("版本选择不应失败")
        .expect("更高构建号应当产生更新");

    assert_eq!(release.build_number, 1043);
    assert_eq!(release.version.to_string(), "1.2.0-beta.2");
}

#[test]
fn equal_release_is_up_to_date() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.example.console",
        "1.2.0-beta.2",
        1043,
        UpdateChannel::Beta,
    )
    .unwrap();

    assert!(
        payload()
            .select_update(&config, UpdateTarget::MacOsAarch64)
            .expect("版本选择不应失败")
            .is_none()
    );
}

#[test]
fn manifest_sequence_replay_is_rejected() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.example.console",
        "1.0.0",
        1,
        UpdateChannel::Beta,
    )
    .unwrap()
    .with_highest_manifest_sequence(100);

    let error = payload()
        .select_update(&config, UpdateTarget::MacOsAarch64)
        .expect_err("更低 sequence 必须拒绝");

    assert!(matches!(error, UpdateError::ManifestReplay { .. }));
}

#[test]
fn forced_update_returns_release_even_without_newer_version() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.example.console",
        "1.0.0",
        2000,
        UpdateChannel::Beta,
    )
    .unwrap();

    let release = payload()
        .select_update(&config, UpdateTarget::MacOsAarch64)
        .unwrap()
        .expect("低于 minimum_supported_version 必须强制更新");

    assert!(release.mandatory);
}
