use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("xuwecli-lint-{name}-{}-{id}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn write(&self, path: &str, content: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn run(&self, arguments: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xuwecli"));
        command.arg("lint").arg("--workspace").arg(&self.root);
        command.args(arguments).output().unwrap()
    }

    fn git(&self, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(arguments)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.exists() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}

#[test]
fn valid_workspace_passes_with_warnings_denied() {
    let fixture = valid_workspace("valid");

    let output = fixture.run(&["--deny-warnings"]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "lint result: 0 error(s), 0 warning(s)\n"
    );
}

#[test]
fn cargo_rules_report_workspace_and_technology_violations() {
    let fixture = Fixture::new("cargo-rules");
    fixture.write(
        "Cargo.toml",
        r#"[workspace]
resolver = "3"
members = ["crates/xuwe-core"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
serde = "1"
"actix-web" = "4"
"#,
    );
    fixture.write(
        "crates/xuwe-core/Cargo.toml",
        r#"[package]
name = "xuwe-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { version = "1", features = ["full"] }
actix-web = "4"
"#,
    );
    fixture.write(
        "crates/xuwe-core/src/main.rs",
        "//! 二进制入口。\nfn main() {}\n",
    );
    fixture.write("crates/xuwe-core/src/lib.rs", "//! 库入口。\n");
    fixture.write(
        "crates/xuwe-core/migrations/0001_create_users.sql",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\n",
    );

    let output = fixture.run(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    for rule in [
        "xuwe::dependency_not_in_workspace",
        "xuwe::broad_dependency_feature",
        "xuwe::forbidden_technology",
        "xuwe::invalid_crate_name",
        "xuwe::invalid_migration_location",
        "xuwe::mixed_binary_library",
    ] {
        assert!(stdout.contains(rule), "missing {rule} in:\n{stdout}");
    }
}

#[test]
fn rust_and_gpui_rules_report_source_violations() {
    let fixture = Fixture::new("source-rules");
    fixture.write(
        "Cargo.toml",
        r#"[workspace]
resolver = "3"
members = ["crates/dashboard"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
axum = "0.8"
gpui = "0.2"
sqlx = "0.8"
"#,
    );
    fixture.write(
        "crates/dashboard/Cargo.toml",
        r#"[package]
name = "dashboard"
version.workspace = true
edition.workspace = true

[dependencies]
axum = { workspace = true }
gpui = { workspace = true }
sqlx = { workspace = true }
"#,
    );
    fixture.write(
        "crates/dashboard/src/lib.rs",
        r#"//! 故意包含违规写法的测试源码。

use std::sync::Mutex;

static APP_STATE: Mutex<u8> = Mutex::new(0);

pub fn undocumented() -> Result<(), ()> {
    Ok(())
}

/// 用于验证 panic 文档检查的公开函数。
pub fn panic_api() {
    panic!("fixture");
}

#[cfg(test)]
mod tests {}

gpui::actions!(fixture, [Run]);

struct Panel {
    theme: Theme,
}

struct View;

impl Render for View {
    fn render(&mut self, cx: &mut Context<Self>) {
        cx.new(|_| ());
        div().id(("row", row_ix));
        Button::new("icon")
            .icon(IconName::Search)
            .on_click(|_, _, _| {});
        rgb(0xffffff);
        cx.refresh_windows();
    }
}

fn start(cx: &mut Context<View>) {
    cx.observe().detach();
    cx.subscribe();
}

async fn raw_handler(request: Request) {}

fn routes() {
    Router::new().route("/getProjects", get(raw_handler));
}

fn find_user(name: &str) {
    sqlx::query(&format!("SELECT * FROM users WHERE name = '{name}'"));
}
"#,
    );
    fixture.write(
        "crates/dashboard/src/nested/mod.rs",
        "//! 故意使用禁止的模块入口文件。\n",
    );

    let output = fixture.run(&["--deny-warnings"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    for rule in [
        "xuwe::action_outside_actions",
        "xuwe::copied_global_state",
        "xuwe::detached_lifecycle",
        "xuwe::dynamic_sql_concatenation",
        "xuwe::empty_event_handler",
        "xuwe::forbidden_mod_rs",
        "xuwe::global_refresh_scope",
        "xuwe::hardcoded_visual_color",
        "xuwe::icon_button_without_tooltip",
        "xuwe::inline_test_module",
        "xuwe::missing_errors_section",
        "xuwe::missing_panics_section",
        "xuwe::non_chinese_public_docs",
        "xuwe::non_gpui_global_state",
        "xuwe::non_rest_route",
        "xuwe::raw_axum_request",
        "xuwe::render_side_effect",
        "xuwe::unbounded_request_body",
        "xuwe::unstable_element_id",
        "xuwe::untracked_task",
    ] {
        assert!(stdout.contains(rule), "missing {rule} in:\n{stdout}");
    }
}

#[test]
fn warnings_can_be_emitted_as_json_and_promoted_to_failures() {
    let fixture = Fixture::new("json-warning");
    fixture.write(
        "Cargo.toml",
        r#"[workspace]
resolver = "3"
members = ["crates/example"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
syn = { version = "2", features = ["full"] }
"#,
    );
    fixture.write(
        "crates/example/Cargo.toml",
        r#"[package]
name = "example"
version.workspace = true
edition.workspace = true

[dependencies]
syn = { workspace = true }
"#,
    );
    fixture.write("crates/example/src/lib.rs", "//! 合规示例。\n");

    let output = fixture.run(&["--format", "json"]);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["summary"]["errors"], 0);
    assert_eq!(value["summary"]["warnings"], 1);
    assert_eq!(
        value["diagnostics"][0]["rule"],
        "xuwe::broad_dependency_feature"
    );

    let denied = fixture.run(&["--deny-warnings"]);
    assert!(!denied.status.success());
}

#[test]
fn contract_models_cannot_leak_database_types() {
    let fixture = Fixture::new("contract-model");
    fixture.write(
        "Cargo.toml",
        r#"[workspace]
resolver = "3"
members = ["crates/models"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
sqlx = "0.8"
"#,
    );
    fixture.write(
        "crates/models/Cargo.toml",
        r#"[package]
name = "models"
version.workspace = true
edition.workspace = true

[dependencies]
sqlx = { workspace = true }
"#,
    );
    fixture.write(
        "crates/models/src/lib.rs",
        r#"//! 跨端共享模型。

/// 用户响应模型。
#[derive(sqlx::FromRow)]
pub struct User {
    /// 用户唯一编号。
    pub id: i64,
}
"#,
    );

    let output = fixture.run(&["--deny-warnings"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    assert!(stdout.contains("xuwe::database_entity_in_contract"));
    assert!(stdout.contains("xuwe::forbidden_dependency_edge"));
}

#[test]
fn committed_migrations_must_not_be_edited() {
    let fixture = Fixture::new("modified-migration");
    fixture.write(
        "Cargo.toml",
        r#"[workspace]
resolver = "3"
members = ["crates/migrate"]

[workspace.package]
version = "0.1.0"
edition = "2024"
"#,
    );
    fixture.write(
        "crates/migrate/Cargo.toml",
        r#"[package]
name = "migrate"
version.workspace = true
edition.workspace = true
"#,
    );
    fixture.write("crates/migrate/src/lib.rs", "//! 数据库迁移入口。\n");
    fixture.write(
        "crates/migrate/migrations/0001_create_users.sql",
        "CREATE TABLE users (id INTEGER PRIMARY KEY);\n",
    );
    fixture.git(&["init", "--quiet"]);
    fixture.git(&["add", "."]);
    fixture.git(&[
        "-c",
        "user.name=Lint Test",
        "-c",
        "user.email=lint@example.com",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "--quiet",
        "-m",
        "initial",
    ]);
    fixture.write(
        "crates/migrate/migrations/0001_create_users.sql",
        "CREATE TABLE users (id BIGINT PRIMARY KEY);\n",
    );

    let output = fixture.run(&[]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    assert!(stdout.contains("xuwe::modified_migration"));
}

fn valid_workspace(name: &str) -> Fixture {
    let fixture = Fixture::new(name);
    fixture.write(
        "Cargo.toml",
        r#"[workspace]
resolver = "3"
members = ["apps/console", "crates/actions"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
actions = { path = "crates/actions" }
gpui = "0.2"
"#,
    );
    fixture.write(
        "apps/console/Cargo.toml",
        r#"[package]
name = "console"
version.workspace = true
edition.workspace = true

[dependencies]
actions = { workspace = true }
gpui = { workspace = true }
"#,
    );
    fixture.write(
        "apps/console/src/main.rs",
        "//! 控制台入口。\nfn main() {}\n",
    );
    fixture.write(
        "crates/actions/Cargo.toml",
        r#"[package]
name = "actions"
version.workspace = true
edition.workspace = true
"#,
    );
    fixture.write(
        "crates/actions/src/lib.rs",
        "//! Action 定义。\n\n/// 初始化全局 Action。\npub fn init() {}\n",
    );
    fixture
}
