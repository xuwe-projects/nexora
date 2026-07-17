#![cfg(feature = "cli")]

#[allow(dead_code)]
#[path = "../src/bin/nexora/tooling.rs"]
pub mod commands;

use commands::{
    BuildMode, SigningMode, build_plan_from_args, build_plans_from_args, macos_target_for_arch,
    write_sha256_sidecar, write_update_metadata_for_plan, write_update_metadata_for_plans,
};
use std::{env, fs, path::PathBuf};

#[test]
fn maps_host_arch_to_macos_target() {
    assert_eq!(
        macos_target_for_arch("arm64").unwrap(),
        "aarch64-apple-darwin"
    );
    assert_eq!(
        macos_target_for_arch("x86_64").unwrap(),
        "x86_64-apple-darwin"
    );
}

#[test]
fn local_build_defaults_to_ad_hoc_signing_without_notarization() {
    let plan = build_plan_from_args(["nexora", "build", "--mode", "local"], "arm64").unwrap();

    assert_eq!(plan.target(), "aarch64-apple-darwin");
    assert_eq!(plan.signing(), SigningMode::AdHoc);
    assert!(!plan.notarize());
}

#[test]
fn dist_build_defaults_to_developer_id_signing_with_notarization() {
    let plan = build_plan_from_args(["nexora", "build"], "x86_64").unwrap();

    assert_eq!(plan.target(), "x86_64-apple-darwin");
    assert_eq!(plan.signing(), SigningMode::DeveloperId);
    assert!(plan.notarize());
}

#[test]
fn build_outputs_versioned_dmg_to_dist_by_default() {
    let plan = build_plan_from_args(["nexora", "build"], "arm64").unwrap();
    let expected_dmg_path = PathBuf::from(format!(
        "dist/console-{}-aarch64.dmg",
        env!("CARGO_PKG_VERSION")
    ));

    assert_eq!(plan.output_dir(), PathBuf::from("dist").as_path());
    assert_eq!(plan.dmg_path(), expected_dmg_path.as_path());
}

#[test]
fn build_uses_nexora_update_and_notary_defaults() {
    let plan = build_plan_from_args(["nexora", "build"], "arm64").unwrap();

    assert_eq!(plan.app_id(), "com.nexora.console");
    assert_eq!(plan.notary_profile(), "nexora");
}

#[test]
fn build_outputs_update_package_and_manifest_by_default() {
    let plan = build_plan_from_args(
        [
            "nexora",
            "build",
            "--bundle-version",
            "7",
            "--app-id",
            "com.example.console",
        ],
        "arm64",
    )
    .unwrap();

    assert!(plan.create_update_package());
    assert_eq!(plan.bundle_version(), 7);
    assert_eq!(plan.app_id(), "com.example.console");
    assert_eq!(
        plan.app_zip_path(),
        PathBuf::from(format!(
            "dist/console-{}-7-aarch64.app.zip",
            env!("CARGO_PKG_VERSION")
        ))
        .as_path()
    );
    assert_eq!(
        plan.latest_manifest_path(),
        PathBuf::from("dist/latest.json").as_path()
    );
    assert_eq!(
        plan.changelog_path(),
        PathBuf::from(format!(
            "docs/changelog/components/{}/console/zh-CN.md",
            env!("CARGO_PKG_VERSION")
        ))
        .as_path()
    );
    assert_eq!(
        plan.notes_path(),
        PathBuf::from(format!(
            "dist/notes/{}/console/zh-CN.md",
            env!("CARGO_PKG_VERSION")
        ))
        .as_path()
    );
}

#[test]
fn skip_update_package_disables_update_artifacts() {
    let plan = build_plan_from_args(["nexora", "build", "--skip-update-package"], "arm64").unwrap();

    assert!(!plan.create_update_package());
}

#[test]
fn targets_alias_builds_macos_matrix() {
    let plans = build_plans_from_args(["nexora", "build", "--targets", "macos"], "arm64").unwrap();
    let targets = plans.iter().map(|plan| plan.target()).collect::<Vec<_>>();

    assert_eq!(targets, vec!["aarch64-apple-darwin", "x86_64-apple-darwin"]);
    assert_eq!(
        plans[0].app_zip_path(),
        PathBuf::from(format!(
            "dist/console-{}-1-aarch64.app.zip",
            env!("CARGO_PKG_VERSION")
        ))
        .as_path()
    );
    assert_eq!(
        plans[1].app_zip_path(),
        PathBuf::from(format!(
            "dist/console-{}-1-x86_64.app.zip",
            env!("CARGO_PKG_VERSION")
        ))
        .as_path()
    );
}

#[test]
fn target_and_targets_cannot_be_used_together() {
    let error = build_plans_from_args(
        [
            "nexora",
            "build",
            "--target",
            "aarch64-apple-darwin",
            "--targets",
            "macos",
        ],
        "arm64",
    )
    .expect_err("两个 target 参数不能同时使用");

    assert!(error.to_string().contains("不能同时使用"));
}

#[test]
fn checksum_writes_sha256_sidecar_file() {
    let root = env::temp_dir().join(format!("nexora-checksum-{}", std::process::id()));
    let dmg_path = root.join("console-0.1.0-aarch64.dmg");
    let checksum_path = root.join("console-0.1.0-aarch64.dmg.sha256");

    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }

    fs::create_dir_all(&root).unwrap();
    fs::write(&dmg_path, b"nexora").unwrap();

    write_sha256_sidecar(&dmg_path).unwrap();

    assert_eq!(
        fs::read_to_string(&checksum_path).unwrap(),
        "6684bd7ca5b118220b0b7f9996bc71c75359fec3242a3c8ce8a53e889081bf55  console-0.1.0-aarch64.dmg\n"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn update_metadata_writes_latest_json_for_app_zip() {
    let root = env::temp_dir().join(format!("nexora-update-{}", std::process::id()));
    let output_dir = root.join("dist");

    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }

    fs::create_dir_all(&output_dir).unwrap();
    let plan = build_plan_from_args(
        [
            "nexora",
            "build",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--bundle-version",
            "8",
            "--app-id",
            "com.example.console",
            "--channel",
            "beta",
        ],
        "arm64",
    )
    .unwrap();
    fs::write(plan.app_zip_path(), b"nexora").unwrap();

    write_update_metadata_for_plan(&plan).unwrap();

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan.latest_manifest_path()).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["app_id"], "com.example.console");
    assert_eq!(manifest["channel"], "beta");
    assert_eq!(manifest["bundle_version"], 8);
    assert_eq!(manifest["notes_url"], serde_json::Value::Null);
    assert_eq!(
        manifest["artifacts"][0]["url"],
        format!("./console-{}-8-aarch64.app.zip", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        manifest["artifacts"][0]["sha256"],
        "6684bd7ca5b118220b0b7f9996bc71c75359fec3242a3c8ce8a53e889081bf55"
    );
    assert_eq!(manifest["artifacts"][0]["size"], 6);

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn update_metadata_merges_multiple_targets_into_latest_json() {
    let root = env::temp_dir().join(format!("nexora-update-matrix-{}", std::process::id()));
    let output_dir = root.join("dist");

    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }

    fs::create_dir_all(&output_dir).unwrap();
    let plans = build_plans_from_args(
        [
            "nexora",
            "build",
            "--output-dir",
            output_dir.to_str().unwrap(),
            "--targets",
            "macos",
            "--bundle-version",
            "9",
            "--app-id",
            "com.example.console",
        ],
        "arm64",
    )
    .unwrap();
    for plan in &plans {
        fs::write(plan.app_zip_path(), plan.target().as_bytes()).unwrap();
    }

    write_update_metadata_for_plans(&plans).unwrap();

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plans[0].latest_manifest_path()).unwrap())
            .unwrap();
    assert_eq!(manifest["bundle_version"], 9);
    assert_eq!(manifest["artifacts"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["artifacts"][0]["target"], "aarch64-apple-darwin");
    assert_eq!(manifest["artifacts"][1]["target"], "x86_64-apple-darwin");

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn build_accepts_app_version_override() {
    let plan = build_plan_from_args(
        [
            "nexora",
            "build",
            "--app-version",
            "1.2.3",
            "--app-name",
            "Nexora",
        ],
        "arm64",
    )
    .unwrap();

    assert_eq!(
        plan.dmg_path(),
        PathBuf::from("dist/Nexora-1.2.3-aarch64.dmg").as_path()
    );
}

#[test]
fn skip_flags_disable_dmg_and_notarization() {
    let plan = build_plan_from_args(
        ["nexora", "build", "--skip-dmg", "--skip-notarize"],
        "arm64",
    )
    .unwrap();

    assert!(!plan.create_dmg());
    assert!(!plan.notarize());
}

#[test]
fn skip_dmg_also_disables_notarization() {
    let plan = build_plan_from_args(["nexora", "build", "--skip-dmg"], "arm64").unwrap();

    assert!(!plan.create_dmg());
    assert!(!plan.notarize());
}

#[test]
fn build_args_support_equals_syntax() {
    let plan =
        build_plan_from_args(["nexora", "build", "--mode=local", "--sign=none"], "arm64").unwrap();

    assert_eq!(plan.mode(), BuildMode::Local);
    assert_eq!(plan.signing(), SigningMode::None);
    assert!(!plan.notarize());
}
