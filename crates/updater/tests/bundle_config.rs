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
            "health_timeout": "20s"
        }),
    );

    let config = UpdateConfig::from_app_bundle(&bundle).unwrap();

    assert_eq!(config.app_id(), "com.example.desktop");
    assert_eq!(config.current_version().to_string(), "1.0.0");
    assert_eq!(config.current_build_number(), 2);
    assert_eq!(config.channel(), UpdateChannel::Stable);
    assert_eq!(config.trusted_public_keys().len(), 1);

    fs::remove_dir_all(bundle).unwrap();
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
