use std::{
    env, fs,
    fs::File,
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use updater::{
    UpdateError, WindowsSignatureConfig, extract_windows_update_zip,
    validate_windows_zip_entry_path,
};
use zip::{ZipWriter, write::SimpleFileOptions};

#[allow(dead_code)]
#[path = "../src/windows.rs"]
mod windows_implementation;

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
fn unsigned_windows_staging_skips_authenticode_but_keeps_pe_validation() {
    let fixture = Fixture::new("unsigned-staging");
    let main = fixture.root.join("main.exe");
    let updater = fixture.root.join("main-updater.exe");
    if !write_current_arch_pe(&main) || !write_current_arch_pe(&updater) {
        return;
    }

    windows_implementation::verify_staged_update_signatures(
        &fixture.root,
        "main.exe",
        "main-updater.exe",
        None,
    )
    .unwrap();

    fs::write(&updater, b"not a PE file").unwrap();
    let error = windows_implementation::verify_staged_update_signatures(
        &fixture.root,
        "main.exe",
        "main-updater.exe",
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("不是有效 PE 文件"),
        "unexpected error: {error}"
    );
}

#[test]
#[cfg(target_os = "windows")]
fn configured_windows_staging_rejects_unsigned_executable() {
    let fixture = Fixture::new("authenticode-staging");
    let main = fixture.root.join("main.exe");
    let updater = fixture.root.join("main-updater.exe");
    if !write_current_arch_pe(&main) || !write_current_arch_pe(&updater) {
        return;
    }
    let signature = WindowsSignatureConfig {
        signer_thumbprint: "00112233445566778899AABBCCDDEEFF00112233".to_owned(),
        publisher: "Nexora Test Publisher".to_owned(),
    };

    let error = windows_implementation::verify_staged_update_signatures(
        &fixture.root,
        "main.exe",
        "main-updater.exe",
        Some(&signature),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Authenticode"), "unexpected error: {error}");
}

#[test]
#[cfg(target_os = "windows")]
fn windows_record_replace_overwrites_existing_destination_atomically() {
    let fixture = Fixture::new("atomic-record-replace");
    let source = fixture.root.join("pending.new");
    let destination = fixture.root.join("pending.json");
    fs::write(&source, b"new-record").unwrap();
    fs::write(&destination, b"old-record").unwrap();

    windows_implementation::replace_file(&source, &destination).unwrap();

    assert_eq!(fs::read(&destination).unwrap(), b"new-record");
    assert!(!source.exists());
}

#[test]
#[cfg(target_os = "windows")]
fn windows_default_update_cache_is_a_sibling_of_the_install_directory() {
    let fixture = Fixture::new("same-volume-cache");
    let install = fixture.root.join("custom-location").join("installed-app");

    let cache =
        windows_implementation::cache_dir_for_install(&install, "com.example.same-volume-cache")
            .unwrap();

    assert_eq!(
        cache,
        fixture
            .root
            .join("custom-location")
            .join(".nexora-updater")
            .join("com.example.same-volume-cache")
    );
    assert_eq!(install.components().next(), cache.components().next());
    assert!(!cache.starts_with(&install));
}

#[test]
#[cfg(target_os = "windows")]
fn windows_install_preflight_accepts_writable_sibling_staging() {
    let fixture = Fixture::new("install-preflight");
    let current = fixture.root.join("installed-app");
    let staging_root = fixture.root.join(".nexora-updater").join("staging");
    let staged = staging_root.join("extracted");
    fs::create_dir_all(&current).unwrap();
    fs::create_dir_all(&staged).unwrap();
    fs::write(current.join("main.exe"), b"current").unwrap();
    fs::write(current.join("main-updater.exe"), b"updater").unwrap();
    fs::write(current.join("unins000.exe"), b"uninstaller").unwrap();
    fs::write(current.join("unins000.dat"), b"uninstaller data").unwrap();
    fs::write(staged.join("main.exe"), b"staged").unwrap();

    windows_implementation::preflight_install_layout(&current, &staged, &staging_root, "main.exe")
        .unwrap();
}

#[test]
#[cfg(target_os = "windows")]
fn windows_install_preflight_rejects_a_different_executable_as_the_main_app() {
    let fixture = Fixture::new("exact-main-executable-preflight");
    let current = fixture.root.join("installed-app");
    let staging_root = fixture.root.join(".nexora-updater").join("staging");
    let staged = staging_root.join("extracted");
    fs::create_dir_all(&current).unwrap();
    fs::create_dir_all(&staged).unwrap();
    fs::write(current.join("different.exe"), b"current").unwrap();
    fs::write(current.join("unins000.exe"), b"uninstaller").unwrap();
    fs::write(staged.join("main.exe"), b"staged").unwrap();

    let error = windows_implementation::preflight_install_layout(
        &current,
        &staged,
        &staging_root,
        "main.exe",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("main.exe"), "unexpected error: {error}");
}

#[test]
#[cfg(target_os = "windows")]
fn windows_install_preflight_rejects_staging_inside_install_directory() {
    let fixture = Fixture::new("nested-install-preflight");
    let current = fixture.root.join("installed-app");
    let staging_root = current.join(".nexora-updater").join("staging");
    let staged = staging_root.join("extracted");
    fs::create_dir_all(&staged).unwrap();
    fs::write(current.join("main.exe"), b"current").unwrap();
    fs::write(staged.join("main.exe"), b"staged").unwrap();

    let error = windows_implementation::preflight_install_layout(
        &current,
        &staged,
        &staging_root,
        "main.exe",
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("边界无效"), "unexpected error: {error}");
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

#[test]
fn windows_install_helper_runs_outside_the_install_directory() {
    let fixture = Fixture::new("sidecar-working-directory");
    let install_dir = fixture.root.join("installed-app");
    let staging_root = fixture.root.join("transactions/staging/session");
    let staged_app = staging_root.join("extracted");
    let runtime_dir = fixture.root.join("temporary-sidecar");
    let sidecar_runtime = runtime_dir.join("main-updater.exe");
    let bundled_sidecar = staged_app.join("main-updater.exe");
    let request = windows_implementation::InstallHelperRequest {
        process_id: 42,
        app_id: "com.example.desktop",
        main_exe_name: "main.exe",
        current_app: &install_dir,
        staged_app: &staged_app,
        staging_root: &staging_root,
        sidecar_path: &bundled_sidecar,
        health_timeout: std::time::Duration::from_secs(120),
        pending_records: None,
        operation_log_session: Some("1700000000000-abcdefghijklmnop"),
    };

    let command =
        windows_implementation::install_helper_command(&sidecar_runtime, request).unwrap();

    assert_eq!(command.get_current_dir(), Some(runtime_dir.as_path()));
    assert_ne!(command.get_current_dir(), Some(install_dir.as_path()));
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--main-exe-name", "main.exe"])
    );
    assert!(
        args.windows(2)
            .any(|pair| { pair == ["--operation-log-session", "1700000000000-abcdefghijklmnop",] })
    );
}

#[test]
fn windows_update_preserves_inno_setup_uninstaller_files() {
    let fixture = Fixture::new("preserve-inno-uninstaller");
    let backup = fixture.root.join("backup");
    let current = fixture.root.join("current");
    fs::create_dir_all(&backup).unwrap();
    fs::create_dir_all(&current).unwrap();
    fs::write(backup.join("unins000.exe"), b"uninstaller").unwrap();
    fs::write(backup.join("unins000.dat"), b"uninstaller data").unwrap();
    fs::write(backup.join("UNINS001.MSG"), b"localized messages").unwrap();
    fs::write(backup.join("uninsABC.exe"), b"not an Inno uninstaller").unwrap();
    fs::write(backup.join("application.exe"), b"old application").unwrap();

    windows_implementation::preserve_inno_uninstaller_files(&backup, &current).unwrap();

    assert_eq!(
        fs::read(current.join("unins000.exe")).unwrap(),
        b"uninstaller"
    );
    assert_eq!(
        fs::read(current.join("unins000.dat")).unwrap(),
        b"uninstaller data"
    );
    assert_eq!(
        fs::read(current.join("UNINS001.MSG")).unwrap(),
        b"localized messages"
    );
    assert!(!current.join("uninsABC.exe").exists());
    assert!(!current.join("application.exe").exists());
}

#[test]
fn windows_update_refuses_to_overwrite_staged_inno_uninstaller_files() {
    let fixture = Fixture::new("reject-staged-inno-uninstaller");
    let backup = fixture.root.join("backup");
    let current = fixture.root.join("current");
    fs::create_dir_all(&backup).unwrap();
    fs::create_dir_all(&current).unwrap();
    fs::write(backup.join("unins000.exe"), b"trusted uninstaller").unwrap();
    fs::write(current.join("unins000.exe"), b"staged collision").unwrap();

    let error = windows_implementation::preserve_inno_uninstaller_files(&backup, &current)
        .unwrap_err()
        .to_string();

    assert!(error.contains("unins000.exe"), "unexpected error: {error}");
    assert_eq!(
        fs::read(current.join("unins000.exe")).unwrap(),
        b"staged collision"
    );
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

fn write_current_arch_pe(path: &Path) -> bool {
    let machine = if cfg!(target_arch = "x86_64") {
        0x8664_u16
    } else if cfg!(target_arch = "aarch64") {
        0xaa64_u16
    } else {
        return false;
    };
    let pe_offset = 0x80_usize;
    let mut bytes = vec![0_u8; 0x100];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&(pe_offset as u32).to_le_bytes());
    bytes[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
    bytes[pe_offset + 4..pe_offset + 6].copy_from_slice(&machine.to_le_bytes());
    bytes[pe_offset + 24..pe_offset + 26].copy_from_slice(&0x20b_u16.to_le_bytes());
    fs::write(path, bytes).unwrap();
    true
}
