use std::{env, fs, path::PathBuf};

use semver::Version;
use sha2::{Digest as _, Sha256};
use updater::{
    ApplicationReleaseMetadata, RELEASE_METADATA_FILE_NAME, RELEASE_NOTES_FILE_NAME,
    ReleaseNotesMetadata, UpdateChannel, load_release_metadata_from_directory,
    read_verified_local_release_notes,
};

fn fixture(name: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "nexora-release-metadata-{}-{name}",
        std::process::id()
    ));
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir_all(&path).unwrap();
    path
}

fn metadata(notes: Option<ReleaseNotesMetadata>) -> ApplicationReleaseMetadata {
    ApplicationReleaseMetadata {
        schema_version: 1,
        app_key: "desktop".to_owned(),
        app_id: "com.example.desktop".to_owned(),
        display_name: "Desktop".to_owned(),
        package: "desktop".to_owned(),
        version: Version::parse("1.2.3").unwrap(),
        build_number: 260804153012,
        channel: UpdateChannel::Stable,
        target: "aarch64-apple-darwin".to_owned(),
        notes,
    }
}

#[test]
fn missing_metadata_is_development_mode() {
    let directory = fixture("missing");
    assert!(
        load_release_metadata_from_directory(&directory)
            .unwrap()
            .is_none()
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn loads_release_identity_and_verifies_local_notes() {
    let directory = fixture("valid");
    let markdown = b"# Changes\n\n<a>raw html</a> [site](https://example.com)\n";
    let notes = ReleaseNotesMetadata {
        file_name: RELEASE_NOTES_FILE_NAME.to_owned(),
        size: markdown.len() as u64,
        sha256: format!("{:x}", Sha256::digest(markdown)),
    };
    fs::write(directory.join(RELEASE_NOTES_FILE_NAME), markdown).unwrap();
    fs::write(
        directory.join(RELEASE_METADATA_FILE_NAME),
        serde_json::to_vec_pretty(&metadata(Some(notes.clone()))).unwrap(),
    )
    .unwrap();

    let loaded = load_release_metadata_from_directory(&directory)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.metadata().build_number, 260804153012);
    assert_eq!(loaded.metadata().channel, UpdateChannel::Stable);
    let verified = read_verified_local_release_notes(loaded.resource_directory(), &notes).unwrap();
    assert!(verified.contains("&lt;a&gt;raw html&lt;/a&gt;"));
    assert!(verified.contains("https://example.com"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn existing_invalid_metadata_and_corrupt_notes_are_rejected() {
    let directory = fixture("invalid");
    let mut invalid = metadata(None);
    invalid.schema_version = 99;
    fs::write(
        directory.join(RELEASE_METADATA_FILE_NAME),
        serde_json::to_vec_pretty(&invalid).unwrap(),
    )
    .unwrap();
    assert!(load_release_metadata_from_directory(&directory).is_err());

    let markdown = b"trusted";
    let notes = ReleaseNotesMetadata {
        file_name: RELEASE_NOTES_FILE_NAME.to_owned(),
        size: markdown.len() as u64,
        sha256: format!("{:x}", Sha256::digest(markdown)),
    };
    fs::write(
        directory.join(RELEASE_METADATA_FILE_NAME),
        serde_json::to_vec_pretty(&metadata(Some(notes.clone()))).unwrap(),
    )
    .unwrap();
    fs::write(directory.join(RELEASE_NOTES_FILE_NAME), b"tampered").unwrap();
    assert!(read_verified_local_release_notes(&directory, &notes).is_err());
    fs::remove_dir_all(directory).unwrap();
}
