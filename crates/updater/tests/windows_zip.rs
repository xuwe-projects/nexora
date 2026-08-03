use std::{
    env, fs,
    fs::File,
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use updater::{extract_windows_update_zip, validate_windows_zip_entry_path};
use zip::{ZipWriter, write::SimpleFileOptions};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = env::temp_dir().join(format!(
            "nexora-windows-zip-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn windows_zip_entry_accepts_normal_relative_paths() {
    let path = validate_windows_zip_entry_path("resources/images/logo.png").unwrap();

    assert_eq!(path, PathBuf::from("resources/images/logo.png"));
}

#[test]
fn windows_zip_entry_rejects_escape_and_special_paths() {
    for entry in [
        "",
        "/absolute/app.exe",
        r"\\server\share\app.exe",
        r"C:\Users\test\app.exe",
        "C:/Users/test/app.exe",
        "../app.exe",
        "resources/../app.exe",
        "resources//app.exe",
        "main.exe:evil",
        "resources/trailing.",
        "resources/trailing ",
        "resources/\u{0}bad",
        "CON",
        "NUL.txt",
        "LPT9.log",
        "resources/COM1",
    ] {
        assert!(
            validate_windows_zip_entry_path(entry).is_err(),
            "{entry:?} should be rejected"
        );
    }
}

#[test]
fn windows_update_zip_extracts_required_payload() {
    let fixture = Fixture::new("valid");
    let archive = fixture.root.join("update.zip");
    write_zip(
        &archive,
        &[
            ("main.exe", b"main".as_slice()),
            ("main-updater.exe", b"updater".as_slice()),
            ("nexora-updater.json", b"{}".as_slice()),
            ("resources/logo.png", b"png".as_slice()),
        ],
    );
    let destination = fixture.root.join("staging");

    extract_windows_update_zip(&archive, &destination, "main.exe", "main-updater.exe").unwrap();

    assert_eq!(fs::read(destination.join("main.exe")).unwrap(), b"main");
    assert_eq!(
        fs::read(destination.join("main-updater.exe")).unwrap(),
        b"updater"
    );
    assert_eq!(
        fs::read(destination.join("resources/logo.png")).unwrap(),
        b"png"
    );
}

#[test]
fn windows_update_zip_rejects_duplicate_and_missing_required_files() {
    let fixture = Fixture::new("invalid");
    let duplicate = fixture.root.join("duplicate.zip");
    write_zip(
        &duplicate,
        &[
            ("MAIN.exe", b"main".as_slice()),
            ("main.exe", b"evil".as_slice()),
            ("main-updater.exe", b"updater".as_slice()),
            ("nexora-updater.json", b"{}".as_slice()),
        ],
    );
    assert!(
        extract_windows_update_zip(
            &duplicate,
            &fixture.root.join("duplicate-staging"),
            "main.exe",
            "main-updater.exe",
        )
        .unwrap_err()
        .to_string()
        .contains("重复")
    );

    let missing = fixture.root.join("missing.zip");
    write_zip(
        &missing,
        &[
            ("main.exe", b"main".as_slice()),
            ("nexora-updater.json", b"{}".as_slice()),
        ],
    );
    assert!(
        extract_windows_update_zip(
            &missing,
            &fixture.root.join("missing-staging"),
            "main.exe",
            "main-updater.exe",
        )
        .unwrap_err()
        .to_string()
        .contains("main-updater.exe")
    );
}

#[test]
#[cfg(target_os = "windows")]
fn windows_update_zip_rejects_existing_reparse_parent() {
    use std::os::windows::fs::symlink_dir;

    let fixture = Fixture::new("reparse-parent");
    let archive = fixture.root.join("update.zip");
    write_zip(
        &archive,
        &[
            ("main.exe", b"main".as_slice()),
            ("main-updater.exe", b"updater".as_slice()),
            ("nexora-updater.json", b"{}".as_slice()),
            ("resources/file.txt", b"escape".as_slice()),
        ],
    );
    let destination = fixture.root.join("staging");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&destination).unwrap();
    fs::create_dir_all(&outside).unwrap();
    if symlink_dir(&outside, destination.join("resources")).is_err() {
        return;
    }

    let error = extract_windows_update_zip(&archive, &destination, "main.exe", "main-updater.exe")
        .unwrap_err()
        .to_string();

    assert!(error.contains("reparse point"));
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, contents) in entries {
        zip.start_file(name, options).unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap();
}
