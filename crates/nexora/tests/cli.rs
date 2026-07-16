#![cfg(feature = "cli")]

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
        .replace("{{ nexora_source }}", &expected_nexora_source())
}

fn expected_workspace_manifest(project_name: &str, account_enabled: bool) -> String {
    render_account_condition(WORKSPACE_MANIFEST_TEMPLATE, account_enabled)
        .replace("{{ project_name }}", project_name)
        .replace("{{ nexora_source }}", &expected_nexora_source())
}

fn expected_nexora_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .to_string_lossy()
        .replace('\\', "/");
    format!("path = \"{path}\"")
}

fn expected_desktop_manifest(project_name: &str, account_enabled: bool) -> String {
    render_account_condition(DESKTOP_MANIFEST_TEMPLATE, account_enabled)
        .replace("{{ project_name }}", project_name)
}

fn expected_main(project_name: &str, account_enabled: bool) -> String {
    const START: &str = "{%- if account_enabled -%}\n";
    const ELSE: &str = "\n{%- else -%}\n";
    const END: &str = "\n{%- endif -%}";

    let source = askama_source(MAIN_TEMPLATE);
    let template = source
        .strip_prefix(START)
        .expect("main.rs 条件模板必须以 account_enabled 分支开始")
        .strip_suffix(END)
        .expect("main.rs 条件模板必须闭合");
    let (enabled, disabled) = template
        .split_once(ELSE)
        .expect("main.rs 条件模板必须包含无 Account 分支");
    let rendered = if account_enabled {
        enabled.to_owned()
    } else {
        disabled.to_owned()
    };
    rendered.replace("{{ project_name }}", project_name)
}

fn render_account_condition(template: &str, account_enabled: bool) -> String {
    const START: &str = "{% if account_enabled %}";
    const ELSE: &str = "{% else %}";
    const END: &str = "{% endif %}";

    let mut rendered = askama_source(template);
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

fn askama_source(template: &str) -> String {
    let normalized = template.replace("\r\n", "\n").replace('\r', "");
    normalized
        .strip_suffix('\n')
        .unwrap_or(&normalized)
        .to_owned()
}

fn assert_valid_manifest(path: &Path) {
    let contents = fs::read_to_string(path).expect("应能读取生成的 Cargo manifest");
    assert!(
        !contents.contains('\r'),
        "生成的 Cargo manifest 必须使用 LF 行尾：{}",
        path.display()
    );
    contents
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_else(|error| {
            panic!(
                "生成的 Cargo manifest 语法无效：{}：{error}",
                path.display()
            )
        });
}

fn collect_relative_files(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("应能读取 Skill 目录") {
            let path = entry.expect("应能读取 Skill 目录项").path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.push(
                    path.strip_prefix(root)
                        .expect("Skill 文件应位于根目录内")
                        .to_path_buf(),
                );
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

fn assert_generated_skills(project: &Path) {
    let template_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/skills");
    let generated_root = project.join(".agents/skills");
    let template_files = collect_relative_files(&template_root);
    let generated_files = collect_relative_files(&generated_root);

    assert_eq!(generated_files, template_files);
    assert!(
        generated_files.contains(&PathBuf::from("develop-nexora-apps/SKILL.md")),
        "生成项目应包含 Nexora 框架 Skill"
    );
    for relative_path in template_files {
        assert_eq!(
            fs::read(generated_root.join(&relative_path)).unwrap(),
            fs::read(template_root.join(&relative_path)).unwrap(),
            "Skill 模板应原样写入：{}",
            relative_path.display()
        );
    }
}

#[test]
fn packaged_skill_templates_match_the_workspace_agent_skills() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest.join("../../.agents/skills");
    let template_root = manifest.join("templates/skills");
    if !source_root.is_dir() {
        return;
    }
    let source_files = collect_relative_files(&source_root);
    let template_files = collect_relative_files(&template_root);

    assert_eq!(template_files, source_files);
    for relative_path in source_files {
        assert_eq!(
            fs::read(source_root.join(&relative_path)).unwrap(),
            fs::read(template_root.join(&relative_path)).unwrap(),
            "发布模板必须与仓库 Skill 保持一致：{}",
            relative_path.display()
        );
    }
}

#[test]
fn help_and_version_are_available() {
    let directory = TestDirectory::new("help-version");

    let help = directory.run(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: nexora"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("create"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("init"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("build"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("doctor"));
    assert!(String::from_utf8_lossy(&help.stdout).contains("lint"));

    let build_help = directory.run(&["help", "build"]);
    assert!(build_help.status.success());
    let build_help = String::from_utf8_lossy(&build_help.stdout);
    assert!(build_help.contains("build [OPTIONS]"));
    assert!(build_help.contains("--mode <MODE>"));
    assert!(build_help.contains("--targets <TARGETS>"));
    assert!(build_help.contains("--app-id <APP_ID>"));

    let create_help = directory.run(&["help", "create"]);
    assert!(create_help.status.success());
    let create_help = String::from_utf8_lossy(&create_help.stdout);
    assert!(create_help.contains("create [OPTIONS] [name]"));
    assert!(create_help.contains("--layout <LAYOUT>"));
    assert!(create_help.contains("single, workspace"));
    assert!(create_help.contains("--features <FEATURES>"));
    assert!(create_help.contains("account"));

    let init_help = directory.run(&["help", "init"]);
    assert!(init_help.status.success());
    let init_help = String::from_utf8_lossy(&init_help.stdout);
    assert!(init_help.contains("init [OPTIONS] [path]"));
    assert!(init_help.contains("--layout <LAYOUT>"));
    assert!(init_help.contains("--features <FEATURES>"));

    for flag in ["--version", "-v"] {
        let version = directory.run(&[flag]);
        assert!(version.status.success());
        assert_eq!(
            String::from_utf8_lossy(&version.stdout),
            format!("nexora {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

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
    assert_generated_skills(&project);
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
    assert_generated_skills(directory.path());
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
    assert_valid_manifest(&project.join("Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_single_manifest("demo-app")
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        askama_source(GITIGNORE_TEMPLATE)
    );
    let main = fs::read_to_string(project.join("src/main.rs")).unwrap();
    assert_eq!(main, expected_main("demo-app", false));
    assert_eq!(
        fs::read_to_string(project.join("src/features.rs")).unwrap(),
        askama_source(FEATURES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("src/features/home.rs")).unwrap(),
        askama_source(HOME_FEATURE_TEMPLATE)
    );
    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(!readme.contains('\r'));
    assert!(readme.contains("# demo-app"));
    assert!(readme.contains("cargo run"));
    assert!(!readme.contains("cargo run -p desktop"));
    assert!(!project.join("apps").exists());
    assert_generated_skills(&project);

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
    let desktop = project.join("apps/workspace-app");
    assert_valid_manifest(&project.join("Cargo.toml"));
    assert_valid_manifest(&desktop.join("Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest("workspace-app", false)
    );
    assert_eq!(
        fs::read_to_string(project.join(".gitignore")).unwrap(),
        askama_source(GITIGNORE_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        expected_desktop_manifest("workspace-app", false)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/main.rs")).unwrap(),
        expected_main("workspace-app", false)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/features.rs")).unwrap(),
        askama_source(FEATURES_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/features/home.rs")).unwrap(),
        askama_source(HOME_FEATURE_TEMPLATE)
    );
    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(!readme.contains('\r'));
    assert!(readme.contains("# workspace-app"));
    assert!(readme.contains("cargo run"));
    assert!(!readme.contains("cargo run -p desktop"));
    assert!(!project.join("src").exists());
    assert!(!project.join("apps/server").exists());
    assert!(!desktop.join("src/account.rs").exists());
    assert!(!desktop.join("src/config.rs").exists());
    assert!(!project.join("config").exists());
    assert_generated_skills(&project);
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
        expected_main("existing-app", false)
    );
    assert_generated_skills(&project);
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
        expected_workspace_manifest("existing-workspace", false)
    );
    let desktop = project.join("apps/existing-workspace");
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        expected_desktop_manifest("existing-workspace", false)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/main.rs")).unwrap(),
        expected_main("existing-workspace", false)
    );
    assert_generated_skills(&project);
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
    let desktop = project.join("apps/fullstack-app");
    assert_valid_manifest(&project.join("Cargo.toml"));
    assert_valid_manifest(&desktop.join("Cargo.toml"));
    assert_valid_manifest(&project.join("apps/server/Cargo.toml"));
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).unwrap(),
        expected_workspace_manifest("fullstack-app", true)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        expected_desktop_manifest("fullstack-app", true)
    );
    assert_eq!(
        fs::read_to_string(desktop.join("src/main.rs")).unwrap(),
        expected_main("fullstack-app", true)
    );
    assert!(!desktop.join("src/account.rs").exists());
    assert_eq!(
        fs::read_to_string(desktop.join("src/config.rs")).unwrap(),
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
        fs::read_to_string(project.join("config/server.toml")).unwrap(),
        askama_source(EXAMPLE_SERVER_CONFIG_TEMPLATE)
    );
    assert_eq!(
        fs::read_to_string(project.join("config/fullstack-app.toml")).unwrap(),
        askama_source(EXAMPLE_DESKTOP_CONFIG_TEMPLATE)
    );
    let readme = fs::read_to_string(project.join("README.md")).unwrap();
    assert!(!readme.contains('\r'));
    assert!(readme.contains("cargo run -p server -- config/server.toml"));
    assert!(readme.contains("cargo run -- config/fullstack-app.toml"));
    assert_generated_skills(&project);

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
        fs::read_to_string(project.join("config/server.toml"))
            .unwrap()
            .contains("首次连接确认为空的新数据库时改为 true")
    );

    let desktop_main = fs::read_to_string(desktop.join("src/main.rs")).unwrap();
    assert!(desktop_main.contains("nexora::config::initialize(None)"));
    assert!(desktop_main.contains("account::client::client_config"));
    assert!(desktop_main.contains("AccountAuthenticator::new"));
    assert!(desktop_main.contains("authenticator: AccountAuthenticator"));
    assert!(desktop_main.contains(
        "nexora::account::client::install_authenticator(self.authenticator.clone(), cx)"
    ));
    assert!(!desktop_main.contains("AccountRuntime"));
    assert!(!desktop_main.contains("begin_login"));
    assert!(
        fs::read_to_string(desktop.join("src/config.rs"))
            .unwrap()
            .contains("#[nexora(account_client)]")
    );
}

#[test]
fn init_workspace_account_generates_all_agent_skills() {
    let directory = TestDirectory::new("init-workspace-account-skills");
    let project = directory.path().join("existing-account-workspace");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("README.md"), "keep me").unwrap();

    let output = directory.run(&[
        "init",
        "existing-account-workspace",
        "--layout",
        "workspace",
        "--features",
        "account",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.join("README.md")).unwrap(),
        "keep me"
    );
    assert!(project.join("apps/server/src/main.rs").is_file());
    assert!(project.join("config/server.toml").is_file());
    assert!(
        project
            .join("config/existing-account-workspace.toml")
            .is_file()
    );
    assert_generated_skills(&project);
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
fn account_workspace_rejects_the_reserved_server_project_name() {
    let directory = TestDirectory::new("account-reserved-server-name");

    for name in ["server", "Server"] {
        let output = directory.run(&[
            "create",
            name,
            "--layout",
            "workspace",
            "--features",
            "account",
        ]);

        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("保留包名"));
        assert!(!directory.path().join(name).exists());
    }
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
    assert!(!project.join("apps/account-app/src/account.rs").exists());
    assert!(project.join("apps/account-app/src/config.rs").is_file());
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
    assert!(!project.join("apps/server-route-exists").exists());
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
fn init_never_overwrites_an_existing_agent_skill() {
    let directory = TestDirectory::new("skill-no-overwrite");
    let project = directory.path().join("existing-skill");
    let skill = project.join(".agents/skills/develop-nexora-apps/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "skill sentinel").unwrap();

    let output = directory.run(&["init", "existing-skill"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(".agents/skills/develop-nexora-apps/SKILL.md")
    );
    assert_eq!(fs::read_to_string(skill).unwrap(), "skill sentinel");
    assert!(!project.join("Cargo.toml").exists());
}

#[test]
fn workspace_failures_do_not_leave_partial_scaffolds() {
    let directory = TestDirectory::new("workspace-no-partial");

    let manifest_project = directory.path().join("desktop-manifest-exists");
    let desktop = manifest_project.join("apps/desktop-manifest-exists");
    fs::create_dir_all(&desktop).unwrap();
    fs::write(desktop.join("Cargo.toml"), "manifest sentinel").unwrap();
    let manifest_output =
        directory.run(&["init", "desktop-manifest-exists", "--layout", "workspace"]);
    assert!(!manifest_output.status.success());
    assert_eq!(
        fs::read_to_string(desktop.join("Cargo.toml")).unwrap(),
        "manifest sentinel"
    );
    assert!(!manifest_project.join("Cargo.toml").exists());
    assert!(!desktop.join("src/main.rs").exists());

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
