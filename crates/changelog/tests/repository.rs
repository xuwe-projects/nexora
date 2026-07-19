use changelog::EmbeddedChangelogRepository;
use semver::Version;

#[test]
fn current_release_is_newer_than_previous_release() {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let previous = Version::parse("0.8.0").unwrap();

    assert!(current > previous);
}

#[test]
fn embedded_repository_finds_current_console_changelog() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    let entry = repository.find("console", &version, "zh-CN").unwrap();

    assert_eq!(entry.version(), &version);
    assert_eq!(entry.component(), "console");
    assert_eq!(entry.locale(), "zh-CN");
    assert_eq!(entry.source_path(), format!("{version}/console/zh-CN.md"));
    assert!(entry.markdown().contains("CRUD 表格"));
    assert!(entry.markdown().contains("CrudTableRow"));
}

#[test]
fn embedded_repository_supports_multiple_release_components() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();

    assert!(repository.find("api", &version, "zh-CN").is_some());
    assert!(repository.find("console", &version, "zh-CN").is_some());
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
        .releases("console", "zh-CN")
        .map(|entry| entry.version().to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        versions,
        [
            env!("CARGO_PKG_VERSION"),
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
            "0.0.1"
        ]
    );
}
