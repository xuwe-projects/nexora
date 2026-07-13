use updater::{UpdateChannel, UpdateConfig, UpdateManifest, UpdateTarget};

const MANIFEST: &str = r#"
{
  "schema_version": 1,
  "app_id": "com.xuwe.console",
  "channel": "beta",
  "version": "1.2.0-beta.2",
  "bundle_version": 1043,
  "notes_url": "https://updates.example.com/notes.md",
  "artifacts": [
    {
      "target": "aarch64-apple-darwin",
      "url": "console.app.zip",
      "sha256": "abc123",
      "size": 1024
    }
  ]
}
"#;

#[test]
fn higher_bundle_version_updates_same_semver() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.xuwe.console",
        "1.2.0-beta.2",
        1042,
        UpdateChannel::Beta,
    )
    .expect("测试配置应当有效");
    let manifest = UpdateManifest::parse(MANIFEST).expect("测试清单应当有效");

    let release = manifest
        .select_update(&config, UpdateTarget::MacOsAarch64)
        .expect("版本选择不应失败")
        .expect("更高构建号应当产生更新");

    assert_eq!(release.bundle_version, 1043);
    assert_eq!(release.version.to_string(), "1.2.0-beta.2");
}

#[test]
fn equal_release_is_up_to_date() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.xuwe.console",
        "1.2.0-beta.2",
        1043,
        UpdateChannel::Beta,
    )
    .expect("测试配置应当有效");
    let manifest = UpdateManifest::parse(MANIFEST).expect("测试清单应当有效");

    assert!(
        manifest
            .select_update(&config, UpdateTarget::MacOsAarch64)
            .expect("版本选择不应失败")
            .is_none()
    );
}

#[test]
fn channel_mismatch_is_rejected() {
    let config = UpdateConfig::new(
        "https://updates.example.com/stable/latest.json",
        "com.xuwe.console",
        "1.1.0",
        1000,
        UpdateChannel::Stable,
    )
    .expect("测试配置应当有效");
    let manifest = UpdateManifest::parse(MANIFEST).expect("测试清单应当有效");

    let error = manifest
        .select_update(&config, UpdateTarget::MacOsAarch64)
        .expect_err("不同更新通道必须被拒绝");

    assert!(error.to_string().contains("更新通道不匹配"));
}

#[test]
fn higher_semver_updates_even_with_lower_bundle_version() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.xuwe.console",
        "1.1.0",
        2000,
        UpdateChannel::Beta,
    )
    .expect("测试配置应当有效");
    let manifest = UpdateManifest::parse(MANIFEST).expect("测试清单应当有效");

    assert!(
        manifest
            .select_update(&config, UpdateTarget::MacOsAarch64)
            .expect("版本选择不应失败")
            .is_some()
    );
}

#[test]
fn lower_semver_does_not_update_even_with_higher_bundle_version() {
    let config = UpdateConfig::new(
        "https://updates.example.com/beta/latest.json",
        "com.xuwe.console",
        "1.3.0-beta.1",
        1000,
        UpdateChannel::Beta,
    )
    .expect("测试配置应当有效");
    let manifest = UpdateManifest::parse(MANIFEST).expect("测试清单应当有效");

    assert!(
        manifest
            .select_update(&config, UpdateTarget::MacOsAarch64)
            .expect("版本选择不应失败")
            .is_none()
    );
}
