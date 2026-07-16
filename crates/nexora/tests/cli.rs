use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const SINGLE_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/single/Cargo.toml.askama");
const WORKSPACE_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/Cargo.toml.askama");
const DESKTOP_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/desktop/Cargo.toml.askama");
const MAIN_TEMPLATE: &str = include_str!("../templates/scaffold/main.rs");
const FEATURES_TEMPLATE: &str = include_str!("../templates/scaffold/features.rs");
const HOME_FEATURE_TEMPLATE: &str = include_str!("../templates/scaffold/features/home.rs");
const GITIGNORE_TEMPLATE: &str = include_str!("../templates/scaffold/gitignore.askama");
const DESKTOP_ACCOUNT_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/desktop/account.rs");
const DESKTOP_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/desktop/config.rs");
const SERVER_MANIFEST_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/Cargo.toml.askama");
const SERVER_MAIN_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/main.rs");
const SERVER_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/config.rs");
const SERVER_ROUTES_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/apps/server/routes.rs");
const EXAMPLE_SERVER_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/config/example.server.toml");
const EXAMPLE_DESKTOP_CONFIG_TEMPLATE: &str =
    include_str!("../templates/scaffold/workspace/config/example.desktop.toml");

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时钟应晚于 Unix 元年")
            .as_nanos();
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "nexora-cli-{name}-{}-{timestamp}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("应能创建隔离的测试目录");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nexora"))
            .args(arguments)
            .current_dir(&self.path)
            .output()
            .expect("应能启动 nexora 命令")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn expected_single_manifest(project_name: &str) -> String {
    askama_source(SINGLE_MANIFEST_TEMPLATE)
        .replace("{{ project_name }}", project_name)
        .replace("{{ nexora_version }}", env!("CARGO_PKG_VERSION"))
}

fn expected_workspace_manifest(account_enabled: bool) -> String {
    render_account_condition(WORKSPACE_MANIFEST_TEMPLATE, account_enabled)
        .replace("{{ nexora_version }}", env!("CARGO_PKG_VERSION"))
}

fn expected_desktop_manifest(project_name: &str, account_enabled: bool) -> String {
    render_account_condition(DESKTOP_MANIFEST_TEMPLATE, account_enabled)
        .replace("{{ project_name }}", project_name)
}

fn expected_main(account_enabled: bool) -> String {
    const START: &str = "{%- if account_enabled -%}\n";
    const ELSE: &str = "\n{%- else -%}\n";
    const END: &str = "\n{%- endif -%}";

    let template = askama_source(MAIN_TEMPLATE)
        .strip_prefix(START)
        .expect("main.rs 条件模板必须以 account_enabled 分支开始")
        .strip_suffix(END)
        .expect("main.rs 条件模板必须闭合");
    let (enabled, disabled) = template
        .split_once(ELSE)
        .expect("main.rs 条件模板必须包含无 Account 分支");
    if account_enabled {
        enabled.to_owned()
    } else {
        disabled.to_owned()
    }
}

fn render_account_condition(template: &str, account_enabled: bool) -> String {
    const START: &str = "{% if account_enabled %}";
    const ELSE: &str = "{% else %}";
    const END: &str = "{% endif %}";

    let mut rendered = askama_source(template).to_owned();
    while let Some(start) = rendered.find(START) {
        let content_start = start + START.len();
        let end = rendered[content_start..]
            .find(END)
            .map(|end| content_start + end)
            .expect("account_enabled 条件模板必须闭合");
        let block = &rendered[content_start..end];
        let (enabled, disabled) = block.split_once(ELSE).unwrap_or((block, ""));
        let replacement = if account_enabled {
            enabled.to_owned()
        } else {
            disabled.to_owned()
        };
        rendered.replace_range(start..end + END.len(), replacement.as_str());
    }
    rendered
}

fn askama_source(template: &str) -> &str {
    template.strip_suffix('\n').unwrap_or(template)
}

#[test]
fn help_and_version_are_available() {
    let directory = TestDirectory::new("help-version");

    let help = directory.run(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: nexora"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("create"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("init"));

    let create_help = directory.run(&["help", "create"]);
    assert!(create_help.status.success());
    let create_help = String::from_utf8_lossy(&create_help.stdout);
    assert!(create_help.contains("nexora create [OPTIONS] [name]"));
    assert!(create_help.contains("--layout <LAYOUT>"));
    assert!(create_help.contains("single, workspace"));
    assert!(create_help.contains("--features <FEATURES>"));
    assert!(create_help.contains("account"));

    let init_help = directory.run(&["help", "init"]);
    assert!(init_help.status.success());
    let init_help = String::from_utf8_lossy(&init_help.stdout);
    assert!(init_help.contains("nexora init [OPTIONS] [path]"));
    assert!(init_help.contains("--layout <LAYOUT>"));
    assert!(init_help.contains("--features <FEATURES>"));

    let version = directory.run(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout),
        format!("nexora {}\n", env!("CARGO_PKG_VERSION"))
    );

    let version_command = directory.run(&["version"]);
    assert!(version_command.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version_command.stdout),
        format!("nexora {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn create_without_arguments_uses_non_tty_defaults() {
    let directory = TestDirectory::new("create-defaults");

    let output = directory.run(&["create"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let project = directory.path().join("nexora-app");
    assert!(project.join("Cargo.toml").is_file());
    assert!(project.join("src/main.rs").is_file());
    assert!(!project.join("apps").exists());
}

#[test]
fn init_without_arguments_uses_current_directory_in_non_tty_mode() {
    let directory = TestDirectory::new("init-defaults");

    let output = directory.run(&["init"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.path().join("Cargo.toml").is_file());
    assert!(directory.path().join("src/main.rs").is_file());
    assert!(!directory.path().join("apps").exists());
}

#[test]
fn create_defaults_to_a_single_package_project() {
    let directory = TestDirectory::new("create-single");

    let output = directory.run(&["create", "demo-app"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = directory.path().join("demo-app");
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_single_manifest("demo-app")
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        askama_source(GITIGNORE_TEMPLATE)
    );
    let main = fs::read_to_string(project.join("src/main.rs")).unwrap();
    assert_eq!(main, expected_main(false));
    assert_eq!(
        fs::read_to_string(project.join("src/features.rs")).unwrap(),
        askama_source(FEATURES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("src/features/home.rs")).unwrap(),
        askama_source(HOME_FEATURE_TEMPLATE)
    );
    assert!(!project.join("apps").exists());

    assert!(main.contains("impl nexora::Application for DesktopApplication"));
    assert!(main.contains("DesktopApplication.run()"));
    assert!(HOME_FEATURE_TEMPLATE.contains("impl FeatureElement for HomeFeature"));
}

#[test]
fn create_can_generate_a_workspace_project() {
    let directory = TestDirectory::new("create-workspace");

    let output = directory.run(&["create", "workspace-app", "--layout", "workspace"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = directory.path().join("workspace-app");
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest(false)
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        askama_source(GITIGNORE_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/Cargo.toml")).unwrap(),
        expected_desktop_manifest("workspace-app", false)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/main.rs")).unwrap(),
        expected_main(false)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/features.rs")).unwrap(),
        askama_source(FEATURES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/features/home.rs")).unwrap(),
        askama_source(HOME_FEATURE_TEMPLATE)
    );
    assert!(!project.join("src").exists());
    assert!(!project.join("apps/server").exists());
    assert!(!project.join("apps/desktop/src/account.rs").exists());
    assert!(!project.join("apps/desktop/src/config.rs").exists());
    assert!(!project.join("config").exists());
}

#[test]
fn init_single_preserves_existing_content() {
    let directory = TestDirectory::new("init-single");
    let project = directory.path().join("existing-app");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("README.md"), "keep me").unwrap();

    let output = directory.run(&["init", "existing-app"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_single_manifest("existing-app")
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).unwrap(),
        expected_main(false)
    );
}

#[test]
fn init_can_generate_a_workspace_and_preserve_existing_content() {
    let directory = TestDirectory::new("init-workspace");
    let project = directory.path().join("existing-workspace");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("README.md"), "keep me").unwrap();

    let output = directory.run(&["init", "existing-workspace", "--layout", "workspace"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest(false)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/Cargo.toml")).unwrap(),
        expected_desktop_manifest("existing-workspace", false)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/main.rs")).unwrap(),
        expected_main(false)
    );
}

#[test]
fn workspace_account_feature_generates_a_composable_server() {
    let directory = TestDirectory::new("workspace-account");

    let output = directory.run(&[
        "create",
        "fullstack-app",
        "--layout",
        "workspace",
        "--features",
        "account",
        "--features",
        "account",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = directory.path().join("fullstack-app");
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest(true)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/Cargo.toml")).unwrap(),
        expected_desktop_manifest("fullstack-app", true)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/main.rs")).unwrap(),
        expected_main(true)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/account.rs")).unwrap(),
        askama_source(DESKTOP_ACCOUNT_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/desktop/src/config.rs")).unwrap(),
        askama_source(DESKTOP_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/Cargo.toml")).unwrap(),
        askama_source(SERVER_MANIFEST_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/main.rs")).unwrap(),
        askama_source(SERVER_MAIN_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/config.rs")).unwrap(),
        askama_source(SERVER_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/routes.rs")).unwrap(),
        askama_source(SERVER_ROUTES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("config/example.server.toml")).unwrap(),
        askama_source(EXAMPLE_SERVER_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("config/example.desktop.toml")).unwrap(),
        askama_source(EXAMPLE_DESKTOP_CONFIG_TEMPLATE)
    );

    let server_main = fs::read_to_string(project.join("apps/server/src/main.rs")).unwrap();
    let migrate = server_main
        .find("account::server::migrate")
        .expect("服务端应先执行集中迁移");
    let dependencies = server_main
        .find("account::server::dependencies")
        .expect("服务端应装配 Account 依赖");
    assert!(migrate < dependencies);
    assert!(server_main.contains("MigrationOptions::new()"));
    assert!(server_main.contains("initialize_empty_database"));
    assert!(server_main.contains("Account::new(dependencies)"));
    assert!(server_main.contains("merge(account.routers::<AppState>())"));
    assert!(server_main.contains("merge(routes::routers())"));
    assert!(server_main.contains("with_graceful_shutdown(shutdown_signal())"));
    assert!(!server_main.contains("Account::run"));
    assert!(
        fs::read_to_string(project.join("apps/server/src/config.rs"))
            .unwrap()
            .contains("#[nexora(account_server)]")
    );
    assert!(
        fs::read_to_string(project.join("apps/server/src/config.rs"))
            .unwrap()
            .contains("#[serde(default)]")
    );
    assert!(
        fs::read_to_string(project.join("config/example.server.toml"))
            .unwrap()
            .contains("首次连接确认为空的新数据库时改为 true")
    );

    let desktop_main = fs::read_to_string(project.join("apps/desktop/src/main.rs")).unwrap();
    assert!(desktop_main.contains("nexora::config::initialize(None)"));
    assert!(desktop_main.contains("account::client::client_config"));
    assert!(desktop_main.contains("AccountAuthenticator::new"));
    assert!(desktop_main.contains("cx.set_global(self.account.clone())"));
    assert!(!desktop_main.contains("begin_login"));
    assert!(
        fs::read_to_string(project.join("apps/desktop/src/config.rs"))
            .unwrap()
            .contains("#[nexora(account_client)]")
    );
}

#[test]
fn account_feature_accepts_comma_separated_values() {
    let directory = TestDirectory::new("account-comma-separated");

    let output = directory.run(&[
        "create",
        "comma-app",
        "--layout",
        "workspace",
        "--features",
        "account,account",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        directory
            .path()
            .join("comma-app/apps/server/src/main.rs")
            .is_file()
    );
}

#[test]
fn account_feature_requires_workspace_without_leaving_files() {
    let directory = TestDirectory::new("account-requires-workspace");

    let create = directory.run(&[
        "create",
        "single-app",
        "--layout",
        "single",
        "--features",
        "account",
    ]);
    assert!(!create.status.success());
    assert!(String::from_utf8_lossy(&create.stderr).contains("请改用 `--layout workspace`"));
    assert!(!directory.path().join("single-app").exists());

    let init = directory.run(&[
        "init",
        "single-init",
        "--layout",
        "single",
        "--features",
        "account",
    ]);
    assert!(!init.status.success());
    assert!(String::from_utf8_lossy(&init.stderr).contains("请改用 `--layout workspace`"));
    assert!(!directory.path().join("single-init").exists());
}

#[test]
fn account_without_layout_uses_workspace_in_non_tty_mode() {
    let directory = TestDirectory::new("account-auto-workspace");

    let output = directory.run(&["create", "account-app", "--features", "account"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("项目结构已自动调整为 workspace"));
    let project = directory.path().join("account-app");
    assert!(project.join("apps/desktop/src/account.rs").is_file());
    assert!(project.join("apps/server/src/main.rs").is_file());
}

#[test]
fn account_workspace_failure_does_not_leave_partial_scaffold() {
    let directory = TestDirectory::new("account-no-partial");
    let project = directory.path().join("server-route-exists");
    fs::create_dir_all(project.join("apps/server/src")).unwrap();
    fs::write(
        project.join("apps/server/src/routes.rs"),
        "server route sentinel",
    )
    .unwrap();

    let output = directory.run(&[
        "init",
        "server-route-exists",
        "--layout",
        "workspace",
        "--features",
        "account",
    ]);

    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(project.join("apps/server/src/routes.rs")).unwrap(),
        "server route sentinel"
    );
    assert!(!project.join("Cargo.toml").exists());
    assert!(!project.join("apps/desktop").exists());
    assert!(!project.join("config").exists());
}

#[test]
fn init_never_overwrites_single_package_files() {
    let directory = TestDirectory::new("single-no-overwrite");

    let manifest_project = directory.path().join("manifest-exists");
    fs::create_dir(&manifest_project).unwrap();
    fs::write(manifest_project.join("Cargo.toml"), "manifest sentinel").unwrap();
    let manifest_output = directory.run(&["init", "manifest-exists"]);
    assert!(!manifest_output.status.success());
    assert_eq!(
        fs::read_to_string(manifest_project.join("Cargo.toml")).unwrap(),
        "manifest sentinel"
    );
    assert!(!manifest_project.join("src/main.rs").exists());

    let main_project = directory.path().join("main-exists");
    fs::create_dir_all(main_project.join("src")).unwrap();
    fs::write(main_project.join("src/main.rs"), "main sentinel").unwrap();
    let main_output = directory.run(&["init", "main-exists"]);
    assert!(!main_output.status.success());
    assert_eq!(
        fs::read_to_string(main_project.join("src/main.rs")).unwrap(),
        "main sentinel"
    );
    assert!(!main_project.join("Cargo.toml").exists());
}

#[test]
fn workspace_failures_do_not_leave_partial_scaffolds() {
    let directory = TestDirectory::new("workspace-no-partial");

    let manifest_project = directory.path().join("desktop-manifest-exists");
    fs::create_dir_all(manifest_project.join("apps/desktop")).unwrap();
    fs::write(
        manifest_project.join("apps/desktop/Cargo.toml"),
        "manifest sentinel",
    )
    .unwrap();
    let manifest_output =
        directory.run(&["init", "desktop-manifest-exists", "--layout", "workspace"]);
    assert!(!manifest_output.status.success());
    assert_eq!(
        fs::read_to_string(manifest_project.join("apps/desktop/Cargo.toml")).unwrap(),
        "manifest sentinel"
    );
    assert!(!manifest_project.join("Cargo.toml").exists());
    assert!(!manifest_project.join("apps/desktop/src/main.rs").exists());

    let blocked_project = directory.path().join("apps-is-a-file");
    fs::create_dir(&blocked_project).unwrap();
    fs::write(blocked_project.join("apps"), "apps sentinel").unwrap();
    let blocked_output = directory.run(&["init", "apps-is-a-file", "--layout", "workspace"]);
    assert!(!blocked_output.status.success());
    assert_eq!(
        fs::read_to_string(blocked_project.join("apps")).unwrap(),
        "apps sentinel"
    );
    assert!(!blocked_project.join("Cargo.toml").exists());
}

#[test]
fn failed_init_removes_a_new_target_directory() {
    let directory = TestDirectory::new("cleanup-new-target");

    let output = directory.run(&["init", "123-invalid", "--layout", "workspace"]);

    assert!(!output.status.success());
    assert!(!directory.path().join("123-invalid").exists());
}

#[test]
fn invalid_layout_is_rejected_before_creating_a_project() {
    let directory = TestDirectory::new("invalid-layout");

    let output = directory.run(&["create", "demo", "--layout", "nested"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value 'nested'"));
    assert!(!directory.path().join("demo").exists());
}

#[test]
fn invalid_feature_is_rejected_before_creating_a_project() {
    let directory = TestDirectory::new("invalid-feature");

    let output = directory.run(&[
        "create",
        "demo",
        "--layout",
        "workspace",
        "--features",
        "unknown",
    ]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid value 'unknown'"));
    assert!(!directory.path().join("demo").exists());
}

#[test]
fn create_refuses_an_existing_target_directory() {
    let directory = TestDirectory::new("existing-target");
    let project = directory.path().join("demo");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("marker.txt"), "keep me").unwrap();

    let output = directory.run(&["create", "demo", "--layout", "workspace"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(project.join("marker.txt")).unwrap(),
        "keep me"
    );
    assert!(!project.join("Cargo.toml").exists());
    assert!(!project.join("apps").exists());
}
