#[path = "../src/commands/mod.rs"]
pub mod commands;

use commands::{
    BuildMode, SigningMode, build_plan_from_args, macos_target_for_arch, write_sha256_sidecar,
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
    let plan = build_plan_from_args(["xuwecli", "build", "--mode", "local"], "arm64").unwrap();

    assert_eq!(plan.target(), "aarch64-apple-darwin");
    assert_eq!(plan.signing(), SigningMode::AdHoc);
    assert!(!plan.notarize());
}

#[test]
fn dist_build_defaults_to_developer_id_signing_with_notarization() {
    let plan = build_plan_from_args(["xuwecli", "build"], "x86_64").unwrap();

    assert_eq!(plan.target(), "x86_64-apple-darwin");
    assert_eq!(plan.signing(), SigningMode::DeveloperId);
    assert!(plan.notarize());
}

#[test]
fn build_outputs_versioned_dmg_to_dist_by_default() {
    let plan = build_plan_from_args(["xuwecli", "build"], "arm64").unwrap();
    let expected_dmg_path = PathBuf::from(format!(
        "dist/console-{}-aarch64.dmg",
        env!("CARGO_PKG_VERSION")
    ));

    assert_eq!(plan.output_dir(), PathBuf::from("dist").as_path());
    assert_eq!(plan.dmg_path(), expected_dmg_path.as_path());
}

#[test]
fn checksum_writes_sha256_sidecar_file() {
    let root = env::temp_dir().join(format!("xuwecli-checksum-{}", std::process::id()));
    let dmg_path = root.join("console-0.1.0-aarch64.dmg");
    let checksum_path = root.join("console-0.1.0-aarch64.dmg.sha256");

    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }

    fs::create_dir_all(&root).unwrap();
    fs::write(&dmg_path, b"xuwe").unwrap();

    write_sha256_sidecar(&dmg_path).unwrap();

    assert_eq!(
        fs::read_to_string(&checksum_path).unwrap(),
        "a7b3c35ced49e279435986e7be3de315a45d12b01c4d51c4acfbf2fa0fdce691  console-0.1.0-aarch64.dmg\n"
    );

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn build_accepts_app_version_override() {
    let plan = build_plan_from_args(
        [
            "xuwecli",
            "build",
            "--app-version",
            "1.2.3",
            "--app-name",
            "Xuwe",
        ],
        "arm64",
    )
    .unwrap();

    assert_eq!(
        plan.dmg_path(),
        PathBuf::from("dist/Xuwe-1.2.3-aarch64.dmg").as_path()
    );
}

#[test]
fn skip_flags_disable_dmg_and_notarization() {
    let plan = build_plan_from_args(
        ["xuwecli", "build", "--skip-dmg", "--skip-notarize"],
        "arm64",
    )
    .unwrap();

    assert!(!plan.create_dmg());
    assert!(!plan.notarize());
}

#[test]
fn skip_dmg_also_disables_notarization() {
    let plan = build_plan_from_args(["xuwecli", "build", "--skip-dmg"], "arm64").unwrap();

    assert!(!plan.create_dmg());
    assert!(!plan.notarize());
}

#[test]
fn build_args_support_equals_syntax() {
    let plan =
        build_plan_from_args(["xuwecli", "build", "--mode=local", "--sign=none"], "arm64").unwrap();

    assert_eq!(plan.mode(), BuildMode::Local);
    assert_eq!(plan.signing(), SigningMode::None);
    assert!(!plan.notarize());
}
