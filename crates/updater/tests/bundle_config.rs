use std::{env, fs, path::PathBuf};

use updater::{UpdateChannel, UpdateConfig, UpdateError};

fn bundle_root(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "nexora-updater-bundle-{}-{name}",
        std::process::id()
    ))
}

fn write_bundle_config(name: &str, value: serde_json::Value) -> PathBuf {
    let bundle = bundle_root(name);
    if bundle.exists() {
        fs::remove_dir_all(&bundle).unwrap();
    }
    let resources = bundle.join("Contents/Resources");
    fs::create_dir_all(&resources).unwrap();
    fs::write(
        resources.join("nexora-updater.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    bundle
}

#[test]
fn loads_safe_updater_configuration_from_app_bundle() {
    let bundle = write_bundle_config(
        "valid",
        serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.desktop",
            "channel": "stable",
            "feed_url": "http://192.168.0.250/releases/desktop/stable/latest.json",
            "trusted_public_keys": [
                "desktop-main:ed25519:uOr57PW5BEf4f77Hhzqw/4qMiURStMouY1q7HrP3iEs="
            ],
            "current_version": "1.0.0",
            "current_build_number": 2,
            "allow_insecure_http": true,
            "health_timeout": "20s",
            "expected_team_id": "TEAM123456",
            "expected_windows_signer_thumbprint": "00112233445566778899aabbccddeeff00112233",
            "expected_windows_publisher": "Nexora Test Publisher",
            "check_on_launch": true
        }),
    );

    let config = UpdateConfig::from_app_bundle(&bundle).unwrap();

    assert_eq!(config.app_id(), "com.example.desktop");
    assert_eq!(config.current_version().to_string(), "1.0.0");
    assert_eq!(config.current_build_number(), 2);
    assert_eq!(config.channel(), UpdateChannel::Stable);
    assert_eq!(config.trusted_public_keys().len(), 1);
    assert_eq!(config.expected_team_id(), Some("TEAM123456"));
    assert_eq!(
        config.windows_signature().unwrap().signer_thumbprint,
        "00112233445566778899AABBCCDDEEFF00112233"
    );
    assert_eq!(
        config.windows_signature().unwrap().publisher,
        "Nexora Test Publisher"
    );
    assert!(config.check_on_launch());

    fs::remove_dir_all(bundle).unwrap();
}

#[test]
fn loads_safe_updater_configuration_from_windows_install_dir() {
    let install_dir = bundle_root("valid-windows");
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).unwrap();
    }
    fs::create_dir_all(&install_dir).unwrap();
    fs::write(
        install_dir.join("nexora-updater.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "app_id": "com.example.windows",
            "channel": "stable",
            "feed_url": "http://127.0.0.1:9000/releases/windows/stable/latest.json",
            "trusted_public_keys": [
                "windows-main:ed25519:uOr57PW5BEf4f77Hhzqw/4qMiURStMouY1q7HrP3iEs="
            ],
            "current_version": "2.0.0",
            "current_build_number": 9,
            "allow_insecure_http": true,
            "health_timeout": "20s",
            "check_on_launch": false
        }))
        .unwrap(),
    )
    .unwrap();

    let config = UpdateConfig::from_windows_install_dir(&install_dir).unwrap();

    assert_eq!(config.app_id(), "com.example.windows");
    assert_eq!(config.current_version().to_string(), "2.0.0");
    assert_eq!(config.current_build_number(), 9);
    assert_eq!(config.channel(), UpdateChannel::Stable);
    assert!(config.windows_signature().is_none());
    assert!(!config.check_on_launch());

    fs::remove_dir_all(install_dir).unwrap();
}

#[test]
fn rejects_partial_windows_signature_configuration() {
    let base = serde_json::json!({
        "schema_version": 1,
        "app_id": "com.example.desktop",
        "channel": "stable",
        "feed_url": "https://updates.example.com/latest.json",
        "trusted_public_keys": [
            "desktop-main:ed25519:uOr57PW5BEf4f77Hhzqw/4qMiURStMouY1q7HrP3iEs="
        ],
        "current_version": "1.0.0",
        "current_build_number": 2,
        "allow_insecure_http": false,
        "health_timeout": "20s"
    });

    for (name, field, value) in [
        (
            "thumbprint-only",
            "expected_windows_signer_thumbprint",
            "00112233445566778899AABBCCDDEEFF00112233",
        ),
        (
            "publisher-only",
            "expected_windows_publisher",
            "Nexora Test Publisher",
        ),
    ] {
        let mut config = base.clone();
        config[field] = serde_json::Value::String(value.to_owned());
        let bundle = write_bundle_config(name, config);

        let error = UpdateConfig::from_app_bundle(&bundle).unwrap_err();

        assert!(matches!(error, UpdateError::InvalidBundleConfig(_)));
        fs::remove_dir_all(bundle).unwrap();
    }
}

#[test]
fn rejects_invalid_complete_windows_signature_configuration() {
    for (name, thumbprint, publisher) in [
        ("invalid-thumbprint", "not-a-thumbprint", "Nexora Publisher"),
        (
            "empty-publisher",
            "00112233445566778899AABBCCDDEEFF00112233",
            "   ",
        ),
    ] {
        let bundle = write_bundle_config(
            name,
            serde_json::json!({
                "schema_version": 1,
                "app_id": "com.example.desktop",
                "channel": "stable",
                "feed_url": "https://updates.example.com/latest.json",
                "trusted_public_keys": [
                    "desktop-main:ed25519:uOr57PW5BEf4f77Hhzqw/4qMiURStMouY1q7HrP3iEs="
                ],
                "current_version": "1.0.0",
                "current_build_number": 2,
                "allow_insecure_http": false,
                "health_timeout": "20s",
                "expected_windows_signer_thumbprint": thumbprint,
                "expected_windows_publisher": publisher
            }),
        );

        let error = UpdateConfig::from_app_bundle(&bundle).unwrap_err();

        assert!(matches!(error, UpdateError::InvalidBundleConfig(_)));
        fs::remove_dir_all(bundle).unwrap();
    }
}

#[test]
fn rejects_unknown_bundle_configuration_schema() {
    let bundle = write_bundle_config(
        "schema",
        serde_json::json!({
            "schema_version": 2,
            "app_id": "com.example.desktop",
            "channel": "stable",
            "feed_url": "https://updates.example.com/latest.json",
            "trusted_public_keys": [
                "desktop-main:ed25519:uOr57PW5BEf4f77Hhzqw/4qMiURStMouY1q7HrP3iEs="
            ],
            "current_version": "1.0.0",
            "current_build_number": 2,
            "allow_insecure_http": false,
            "health_timeout": "20s"
        }),
    );

    let error = UpdateConfig::from_app_bundle(&bundle).unwrap_err();

    assert!(matches!(error, UpdateError::InvalidBundleConfig(_)));
    fs::remove_dir_all(bundle).unwrap();
}
