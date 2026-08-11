use changelog::EmbeddedChangelogRepository;
use semver::Version;

#[test]
fn current_release_is_newer_than_previous_release() {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let previous = Version::parse("0.33.1").unwrap();

    assert!(current > previous);
}

#[test]
fn embedded_repository_finds_current_api_changelog() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let entry = repository.find("api", &version, "zh-CN").unwrap();

    assert_eq!(entry.version(), &version);
    assert_eq!(entry.component(), "api");
    assert_eq!(entry.locale(), "zh-CN");
    assert_eq!(entry.source_path(), format!("{version}/api/zh-CN.md"));
    assert!(entry.markdown().contains("HTTP API"));
    assert!(entry.markdown().contains("保持兼容"));
}

#[test]
fn embedded_repository_supports_sparse_release_components() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();

    assert!(repository.find("api", &version, "zh-CN").is_some());
    assert!(repository.find("console", &version, "zh-CN").is_some());
    assert!(
        repository
            .find("console", &Version::parse("0.14.0").unwrap(), "zh-CN")
            .is_some()
    );
    assert!(
        repository
            .find("customer-desktop", &version, "zh-CN")
            .is_none()
    );
}

#[test]
fn component_releases_are_sorted_from_newest_to_oldest() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let versions = repository
        .releases("api", "zh-CN")
        .map(|entry| entry.version().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        versions,
        [
            env!("CARGO_PKG_VERSION"),
            "0.33.1",
            "0.33.0",
            "0.32.0",
            "0.31.1",
            "0.31.0",
            "0.30.2",
            "0.30.1",
            "0.30.0",
            "0.29.0",
            "0.28.0",
            "0.27.1",
            "0.27.0",
            "0.26.0",
            "0.25.0",
            "0.22.0",
            "0.21.3",
            "0.21.2",
            "0.21.1",
            "0.21.0",
            "0.20.0",
            "0.19.0",
            "0.18.0",
            "0.16.0",
            "0.15.1",
            "0.14.0",
            "0.13.0",
            "0.12.0",
            "0.11.3",
            "0.11.2",
            "0.11.1",
            "0.11.0",
            "0.10.0",
            "0.9.1",
            "0.9.0",
            "0.8.0",
            "0.7.0",
            "0.6.0",
            "0.5.2",
            "0.5.1",
            "0.5.0",
            "0.4.1",
            "0.4.0",
            "0.3.1",
            "0.3.0",
            "0.2.0",
            "0.1.2",
            "0.1.1",
            "0.1.0",
        ]
    );
}
