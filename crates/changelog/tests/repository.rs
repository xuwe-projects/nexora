use changelog::EmbeddedChangelogRepository;
use semver::Version;

#[test]
fn embedded_repository_finds_current_console_changelog() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let version = Version::parse("0.1.0").unwrap();
    let entry = repository.find("console", &version, "zh-CN").unwrap();

    assert_eq!(entry.version(), &version);
    assert_eq!(entry.component(), "console");
    assert_eq!(entry.locale(), "zh-CN");
    assert_eq!(entry.source_path(), "0.1.0/console/zh-CN.md");
    assert!(entry.markdown().contains("桌面工作台"));
}

#[test]
fn embedded_repository_supports_multiple_release_components() {
    let repository = EmbeddedChangelogRepository::load().unwrap();
    let version = Version::parse("0.1.0").unwrap();

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

    assert_eq!(versions, ["0.1.0", "0.0.1"]);
}
