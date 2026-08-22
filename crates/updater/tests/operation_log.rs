#[path = "../src/operation_log.rs"]
mod operation_log;

use std::{
    env, fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use operation_log::{LOG_DIRECTORY_NAME, MAX_LOG_SESSION_BYTES, MAX_LOG_SESSIONS, OperationLog};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = env::temp_dir().join(format!(
            "nexora-updater-log-{name}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn operation_logs_keep_only_ten_logical_sessions() {
    let fixture = Fixture::new("retention");
    for index in 0..(MAX_LOG_SESSIONS + 3) {
        let log = OperationLog::start_best_effort(&fixture.root).unwrap();
        log.write(&format!("session {index}"));
    }

    let sessions = fs::read_dir(fixture.root.join(LOG_DIRECTORY_NAME))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .count();
    assert_eq!(sessions, MAX_LOG_SESSIONS);
}

#[test]
fn main_and_sidecar_share_one_mib_aggregate_limit() {
    let fixture = Fixture::new("size");
    let main = OperationLog::start_best_effort(&fixture.root).unwrap();
    main.write(&"甲".repeat(600_000));
    let restored_main =
        OperationLog::open_main_best_effort(&fixture.root, main.session_id()).unwrap();
    restored_main.write("恢复主进程日志");
    let sidecar = OperationLog::open_sidecar_best_effort(&fixture.root, main.session_id()).unwrap();
    sidecar.write(&"乙".repeat(600_000));

    let session = fixture
        .root
        .join(LOG_DIRECTORY_NAME)
        .join(main.session_id());
    let size = fs::read_dir(session)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_file())
                && !entry.file_name().to_string_lossy().starts_with('.')
        })
        .map(|entry| entry.metadata().unwrap().len())
        .sum::<u64>();
    assert!(size <= MAX_LOG_SESSION_BYTES, "日志大小为 {size}");
}

#[test]
fn operation_log_redacts_secrets_and_url_queries() {
    let fixture = Fixture::new("redaction");
    let log = OperationLog::start_best_effort(&fixture.root).unwrap();
    log.write(
        "GET https://updates.example.test/latest.json?token=url-secret Authorization: header-secret password=form-secret",
    );
    let contents = fs::read_to_string(
        fixture
            .root
            .join(LOG_DIRECTORY_NAME)
            .join(log.session_id())
            .join("main.log"),
    )
    .unwrap();

    assert!(contents.contains("GET"));
    assert!(contents.contains("[REDACTED]"));
    for secret in ["url-secret", "header-secret", "form-secret"] {
        assert!(!contents.contains(secret));
    }
}

#[test]
fn log_initialization_failure_can_be_ignored() {
    let fixture = Fixture::new("failure");
    let file = fixture.root.join("not-a-directory");
    fs::write(&file, b"occupied").unwrap();

    assert!(OperationLog::start_best_effort(&file).is_none());
}
