//! Nexora 项目创建与初始化命令。

#[path = "skills.rs"]
mod skills;
#[path = "tooling.rs"]
mod tooling;

use std::{
    env,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{IsTerminal as _, Write},
    path::{Path, PathBuf},
};

use askama::Template as _;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use dialoguer::{Confirm, Input, Select};

const DEFAULT_PROJECT_NAME: &str = "nexora-app";

// Cargo 发布包会强制排除任何包含子 Cargo.toml 的目录，因此清单模板使用
// `.askama` 语义后缀；脚手架写出时仍命名为 Cargo.toml。
#[derive(askama::Template)]
#[template(path = "scaffold/single/Cargo.toml.askama", escape = "none")]
struct SingleManifestTemplate<'a> {
    project_name: &'a str,
    nexora_source: &'a str,
}

#[derive(askama::Template)]
#[template(path = "scaffold/workspace/Cargo.toml.askama", escape = "none")]
struct WorkspaceManifestTemplate<'a> {
    project_name: &'a str,
    nexora_source: &'a str,
    account_enabled: bool,
}

#[derive(askama::Template)]
#[template(
    path = "scaffold/workspace/apps/desktop/Cargo.toml.askama",
    escape = "none"
)]
struct DesktopManifestTemplate<'a> {
    project_name: &'a str,
    account_enabled: bool,
}

#[derive(askama::Template)]
#[template(path = "scaffold/main.rs", escape = "none")]
struct MainTemplate<'a> {
    project_name: &'a str,
    account_enabled: bool,
}

#[derive(askama::Template)]
#[template(path = "scaffold/features.rs", escape = "none")]
struct FeaturesTemplate;

#[derive(askama::Template)]
#[template(path = "scaffold/features/home.rs", escape = "none")]
struct HomeFeatureTemplate;

#[derive(askama::Template)]
#[template(path = "scaffold/README.md", escape = "none")]
struct ReadmeTemplate<'a> {
    project_name: &'a str,
    account_enabled: bool,
}

#[derive(askama::Template)]
#[template(path = "scaffold/gitignore.askama", escape = "none")]
struct GitignoreTemplate;

#[derive(askama::Template)]
#[template(path = "scaffold/workspace/apps/desktop/config.rs", escape = "none")]
struct DesktopConfigTemplate;

#[derive(askama::Template)]
#[template(
    path = "scaffold/workspace/apps/server/Cargo.toml.askama",
    escape = "none"
)]
struct ServerManifestTemplate;

#[derive(askama::Template)]
#[template(path = "scaffold/workspace/apps/server/main.rs", escape = "none")]
struct ServerMainTemplate;

#[derive(askama::Template)]
#[template(path = "scaffold/workspace/apps/server/config.rs", escape = "none")]
struct ServerConfigTemplate;

#[derive(askama::Template)]
#[template(path = "scaffold/workspace/apps/server/routes.rs", escape = "none")]
struct ServerRoutesTemplate;

#[derive(askama::Template)]
#[template(
    path = "scaffold/workspace/config/example.server.toml",
    escape = "none"
)]
struct ExampleServerConfigTemplate;

#[derive(askama::Template)]
#[template(
    path = "scaffold/workspace/config/example.desktop.toml",
    escape = "none"
)]
struct ExampleDesktopConfigTemplate;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Layout {
    Single,
    Workspace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ProjectFeature {
    Account,
}

#[derive(Debug, Parser)]
#[command(
    name = "nexora",
    version,
    about = "Nexora 全栈桌面应用框架",
    disable_version_flag = true,
    propagate_version = true
)]
struct Cli {
    /// 输出当前 Nexora 版本并退出。
    #[arg(short = 'v', long = "version", global = true)]
    version: bool,

    #[command(subcommand)]
    command: Option<NexoraCommand>,
}

#[derive(Debug, Subcommand)]
enum NexoraCommand {
    /// 构建、签名并打包 macOS 桌面应用。
    Build(Box<tooling::BuildConfig>),

    /// 创建一个新的 Nexora 项目。
    Create {
        /// 要创建的项目名称；非交互模式省略时使用 `nexora-app`。
        #[arg(value_name = "name")]
        name: Option<String>,

        /// 要生成的项目目录布局；非交互模式省略时使用 `single`。
        #[arg(long, value_enum)]
        layout: Option<Layout>,

        /// 要启用的框架业务能力；`account` 同时生成桌面认证与服务端账号模块。
        #[arg(long, value_enum, value_delimiter = ',')]
        features: Vec<ProjectFeature>,
    },

    /// 在指定目录中初始化 Nexora 项目。
    Init {
        /// 目标目录，省略时使用当前目录。
        #[arg(value_name = "path")]
        path: Option<PathBuf>,

        /// 要生成的项目目录布局；非交互模式省略时使用 `single`。
        #[arg(long, value_enum)]
        layout: Option<Layout>,

        /// 要启用的框架业务能力；`account` 同时生成桌面认证与服务端账号模块。
        #[arg(long, value_enum, value_delimiter = ',')]
        features: Vec<ProjectFeature>,
    },

    /// 检查本地 macOS 打包依赖。
    Doctor(tooling::DoctorConfig),

    /// 检查 workspace 是否符合 Nexora 工程规范。
    Lint(tooling::LintConfig),

    /// 显示当前 Nexora 版本。
    Version,
}

/// 解析当前进程的命令行参数并执行 Nexora 子命令。
///
/// 该函数只向命令行入口开放；成功时将帮助、版本或项目路径输出到标准输出。
///
/// # Errors
///
/// 项目名称或目标路径不合法、目标文件已存在、目录与文件创建失败，macOS 构建或环境
/// 检查无法完成，或者 workspace lint 未通过时，返回面向用户的错误信息。
pub(super) fn run() -> Result<(), String> {
    let interactive = is_interactive_terminal();
    let cli = Cli::parse();
    if cli.version {
        print_version();
        return Ok(());
    }

    match cli.command {
        Some(NexoraCommand::Build(config)) => {
            tooling::run_build_command(config).map_err(|error| error.to_string())
        }
        Some(NexoraCommand::Create {
            name,
            layout,
            features,
        }) => {
            let name = resolve_project_name(name, interactive)?;
            let options = resolve_scaffold_options(layout, features, interactive)?;
            run_create(name, options)
        }
        Some(NexoraCommand::Init {
            path,
            layout,
            features,
        }) => {
            let options = resolve_scaffold_options(layout, features, interactive)?;
            run_init(path, options)
        }
        Some(NexoraCommand::Doctor(config)) => {
            tooling::run_doctor_command(config).map_err(|error| error.to_string())
        }
        Some(NexoraCommand::Lint(config)) => {
            tooling::run_lint_command(config).map_err(|error| error.to_string())
        }
        Some(NexoraCommand::Version) => {
            print_version();
            Ok(())
        }
        None => print_help(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScaffoldOptions {
    layout: Layout,
    account_enabled: bool,
}

fn run_create(name: String, options: ScaffoldOptions) -> Result<(), String> {
    validate_package_name(&name)?;
    validate_scaffold_options(options)?;
    let target = env::current_dir()
        .map_err(|error| format!("无法读取当前目录：{error}"))?
        .join(&name);

    if path_exists(&target)? {
        return Err(format!(
            "目标目录 `{}` 已存在，请改用 `nexora init {name}`",
            target.display()
        ));
    }

    fs::create_dir(&target)
        .map_err(|error| format!("无法创建目录 `{}`：{error}", target.display()))?;
    if let Err(error) = scaffold(&target, &name, options) {
        let _ = fs::remove_dir(&target);
        return Err(error);
    }

    println!("已创建 Nexora 项目 `{}`", target.display());
    Ok(())
}

fn run_init(path: Option<PathBuf>, options: ScaffoldOptions) -> Result<(), String> {
    validate_scaffold_options(options)?;
    let path = path.unwrap_or_else(|| PathBuf::from("."));

    let target_existed = path_exists(&path)?;
    if target_existed && !path.is_dir() {
        return Err(format!("目标路径 `{}` 不是目录", path.display()));
    }
    if !target_existed {
        fs::create_dir_all(&path)
            .map_err(|error| format!("无法创建目录 `{}`：{error}", path.display()))?;
    }

    let result = project_name(&path).and_then(|name| scaffold(&path, &name, options));
    if let Err(error) = result {
        if !target_existed {
            let _ = fs::remove_dir(&path);
        }
        return Err(error);
    }

    println!("已在 `{}` 初始化 Nexora 项目", path.display());
    Ok(())
}

fn scaffold(target: &Path, project_name: &str, options: ScaffoldOptions) -> Result<(), String> {
    let ScaffoldOptions {
        layout,
        account_enabled,
    } = options;
    if account_enabled && project_name.eq_ignore_ascii_case("server") {
        return Err("`server` 是 Account workspace 的保留包名，请使用其他项目名称".to_owned());
    }
    let main = normalize_template_output(
        MainTemplate {
            project_name,
            account_enabled,
        }
        .render()
        .map_err(|error| format!("无法渲染 main.rs 模板：{error}"))?,
    );
    let features_module = normalize_template_output(
        FeaturesTemplate
            .render()
            .map_err(|error| format!("无法渲染 features.rs 模板：{error}"))?,
    );
    let home_feature = normalize_template_output(
        HomeFeatureTemplate
            .render()
            .map_err(|error| format!("无法渲染 features/home.rs 模板：{error}"))?,
    );
    let readme = normalize_template_output(
        ReadmeTemplate {
            project_name,
            account_enabled,
        }
        .render()
        .map_err(|error| format!("无法渲染 README.md 模板：{error}"))?,
    );
    let gitignore = normalize_template_output(
        GitignoreTemplate
            .render()
            .map_err(|error| format!("无法渲染 .gitignore 模板：{error}"))?,
    );
    let nexora_source = nexora_dependency_source();

    match layout {
        Layout::Single => {
            let manifest = normalize_template_output(
                SingleManifestTemplate {
                    project_name,
                    nexora_source: &nexora_source,
                }
                .render()
                .map_err(|error| format!("无法渲染 Cargo.toml 模板：{error}"))?,
            );
            let mut directories = vec!["src".to_owned(), "src/features".to_owned()];
            let mut files = vec![
                (".gitignore".to_owned(), gitignore),
                ("Cargo.toml".to_owned(), manifest),
                ("src/main.rs".to_owned(), main),
                ("src/features.rs".to_owned(), features_module),
                ("src/features/home.rs".to_owned(), home_feature),
            ];
            append_readme_if_missing(target, &mut files, readme)?;
            append_agent_skills(&mut directories, &mut files)?;
            write_scaffold(target, &directories, &files)
        }
        Layout::Workspace => {
            let manifest = normalize_template_output(
                WorkspaceManifestTemplate {
                    project_name,
                    nexora_source: &nexora_source,
                    account_enabled,
                }
                .render()
                .map_err(|error| format!("无法渲染工作区 Cargo.toml 模板：{error}"))?,
            );
            let desktop_manifest = normalize_template_output(
                DesktopManifestTemplate {
                    project_name,
                    account_enabled,
                }
                .render()
                .map_err(|error| format!("无法渲染桌面应用 Cargo.toml 模板：{error}"))?,
            );
            let desktop_directory = format!("apps/{project_name}");
            let mut directories = vec![
                "apps".to_owned(),
                desktop_directory.clone(),
                format!("{desktop_directory}/src"),
                format!("{desktop_directory}/src/features"),
            ];
            let mut files = vec![
                (".gitignore".to_owned(), gitignore),
                ("Cargo.toml".to_owned(), manifest),
                (format!("{desktop_directory}/Cargo.toml"), desktop_manifest),
                (format!("{desktop_directory}/src/main.rs"), main),
                (
                    format!("{desktop_directory}/src/features.rs"),
                    features_module,
                ),
                (
                    format!("{desktop_directory}/src/features/home.rs"),
                    home_feature,
                ),
            ];

            if account_enabled {
                directories.extend(["apps/server", "apps/server/src", "config"].map(str::to_owned));
                files.extend(render_account_workspace_templates(project_name)?);
            }

            append_readme_if_missing(target, &mut files, readme)?;
            append_agent_skills(&mut directories, &mut files)?;
            write_scaffold(target, &directories, &files)
        }
    }
}

fn append_agent_skills(
    directories: &mut Vec<String>,
    files: &mut Vec<(String, String)>,
) -> Result<(), String> {
    directories.extend(
        skills::DIRECTORIES
            .iter()
            .map(|directory| (*directory).to_owned()),
    );
    files.extend(skills::render()?);
    Ok(())
}

fn append_readme_if_missing(
    target: &Path,
    files: &mut Vec<(String, String)>,
    readme: String,
) -> Result<(), String> {
    if !path_exists(&target.join("README.md"))? {
        files.push(("README.md".to_owned(), readme));
    }
    Ok(())
}

fn normalize_template_output(contents: String) -> String {
    contents.replace("\r\n", "\n").replace('\r', "")
}

fn nexora_dependency_source() -> String {
    format!(
        "git = \"{}\", tag = \"v{}\"",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    )
}

fn render_account_workspace_templates(project_name: &str) -> Result<Vec<(String, String)>, String> {
    Ok(vec![
        (
            format!("apps/{project_name}/src/config.rs"),
            normalize_template_output(
                DesktopConfigTemplate
                    .render()
                    .map_err(|error| format!("无法渲染桌面端 config.rs 模板：{error}"))?,
            ),
        ),
        (
            "apps/server/Cargo.toml".to_owned(),
            normalize_template_output(
                ServerManifestTemplate
                    .render()
                    .map_err(|error| format!("无法渲染服务端 Cargo.toml 模板：{error}"))?,
            ),
        ),
        (
            "apps/server/src/main.rs".to_owned(),
            normalize_template_output(
                ServerMainTemplate
                    .render()
                    .map_err(|error| format!("无法渲染服务端 main.rs 模板：{error}"))?,
            ),
        ),
        (
            "apps/server/src/config.rs".to_owned(),
            normalize_template_output(
                ServerConfigTemplate
                    .render()
                    .map_err(|error| format!("无法渲染服务端 config.rs 模板：{error}"))?,
            ),
        ),
        (
            "apps/server/src/routes.rs".to_owned(),
            normalize_template_output(
                ServerRoutesTemplate
                    .render()
                    .map_err(|error| format!("无法渲染服务端 routes.rs 模板：{error}"))?,
            ),
        ),
        (
            "config/server.toml".to_owned(),
            normalize_template_output(
                ExampleServerConfigTemplate
                    .render()
                    .map_err(|error| format!("无法渲染服务端示例配置模板：{error}"))?,
            ),
        ),
        (
            format!("config/{project_name}.toml"),
            normalize_template_output(
                ExampleDesktopConfigTemplate
                    .render()
                    .map_err(|error| format!("无法渲染桌面端示例配置模板：{error}"))?,
            ),
        ),
    ])
}

fn resolve_project_name(name: Option<String>, interactive: bool) -> Result<String, String> {
    let name = if let Some(name) = name {
        name
    } else if interactive {
        Input::<String>::new()
            .with_prompt("项目名称")
            .default(DEFAULT_PROJECT_NAME.to_owned())
            .validate_with(|name: &String| validate_package_name(name))
            .interact_text()
            .map_err(|error| format!("无法读取项目名称：{error}"))?
    } else {
        DEFAULT_PROJECT_NAME.to_owned()
    };
    validate_package_name(&name)?;
    Ok(name)
}

fn resolve_scaffold_options(
    layout: Option<Layout>,
    features: Vec<ProjectFeature>,
    interactive: bool,
) -> Result<ScaffoldOptions, String> {
    let layout_explicit = layout.is_some();
    let features_explicit = !features.is_empty();
    let mut layout = if let Some(layout) = layout {
        layout
    } else if interactive {
        prompt_layout()?
    } else {
        Layout::Single
    };
    let account_enabled = if features_explicit {
        features.contains(&ProjectFeature::Account)
    } else if interactive {
        Confirm::new()
            .with_prompt("是否启用 Account（桌面认证 + 服务端用户、角色与权限）")
            .default(false)
            .interact()
            .map_err(|error| format!("无法读取 Account 选择：{error}"))?
    } else {
        false
    };

    if account_enabled && layout == Layout::Single {
        if layout_explicit && features_explicit {
            return Err(single_account_error());
        }
        println!("Account 需要桌面端与服务端，项目结构已自动调整为 workspace");
        layout = Layout::Workspace;
    }

    let options = ScaffoldOptions {
        layout,
        account_enabled,
    };
    validate_scaffold_options(options)?;
    Ok(options)
}

fn prompt_layout() -> Result<Layout, String> {
    let layouts = ["single（单包桌面应用）", "workspace（桌面 + 可选服务端）"];
    let selected = Select::new()
        .with_prompt("项目结构")
        .items(layouts)
        .default(0)
        .interact()
        .map_err(|error| format!("无法读取项目结构：{error}"))?;
    Ok(if selected == 0 {
        Layout::Single
    } else {
        Layout::Workspace
    })
}

fn validate_scaffold_options(options: ScaffoldOptions) -> Result<(), String> {
    if options.layout == Layout::Single && options.account_enabled {
        return Err(single_account_error());
    }
    Ok(())
}

fn single_account_error() -> String {
    "`account` 同时包含桌面端与服务端能力，不能用于 `--layout single`；请改用 `--layout workspace`"
        .to_owned()
}

fn is_interactive_terminal() -> bool {
    std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
}

fn write_scaffold(
    target: &Path,
    directories: &[String],
    files: &[(String, String)],
) -> Result<(), String> {
    let mut existing = Vec::new();
    for (relative_path, _) in files {
        if path_exists(&target.join(relative_path))? {
            existing.push(relative_path.as_str());
        }
    }
    if !existing.is_empty() {
        return Err(format!("拒绝覆盖已有文件：{}", existing.join("、")));
    }

    for relative_path in directories {
        let path = target.join(relative_path);
        if path_exists(&path)? && !path.is_dir() {
            return Err(format!(
                "无法创建目录 `{}`：同名路径不是目录",
                path.display()
            ));
        }
    }

    let mut created_directories = Vec::new();
    for relative_path in directories {
        let path = target.join(relative_path);
        if path.is_dir() {
            continue;
        }
        if let Err(error) = fs::create_dir(&path) {
            remove_created_scaffold(&[], &created_directories);
            return Err(format!("无法创建目录 `{}`：{error}", path.display()));
        }
        created_directories.push(path);
    }

    let mut created_files = Vec::new();
    for (relative_path, contents) in files {
        let path = target.join(relative_path);
        if let Err(error) = write_new_file(&path, contents.as_bytes()) {
            remove_created_scaffold(&created_files, &created_directories);
            return Err(error);
        }
        created_files.push(path);
    }

    Ok(())
}

fn remove_created_scaffold(files: &[PathBuf], directories: &[PathBuf]) {
    for path in files.iter().rev() {
        let _ = fs::remove_file(path);
    }
    for path in directories.iter().rev() {
        let _ = fs::remove_dir(path);
    }
}

fn project_name(target: &Path) -> Result<String, String> {
    let canonical = fs::canonicalize(target)
        .map_err(|error| format!("无法解析目录 `{}`：{error}", target.display()))?;
    let name = canonical
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("无法从目录 `{}` 推导项目名称", target.display()))?;
    validate_package_name(name)?;
    Ok(name.to_owned())
}

fn validate_package_name(name: &str) -> Result<(), String> {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return Err("项目名称不能为空".to_owned());
    };
    if !first.is_ascii_alphabetic()
        || !characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(format!(
            "项目名称 `{name}` 不合法：必须以 ASCII 字母开头，且只能包含字母、数字、`-` 或 `_`"
        ));
    }
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("拒绝覆盖 `{}`：{error}", path.display()))?;

    if let Err(error) = file.write_all(contents) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("无法写入文件 `{}`：{error}", path.display()));
    }
    Ok(())
}

fn path_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("无法检查路径 `{}`：{error}", path.display())),
    }
}

fn print_help() -> Result<(), String> {
    Cli::command()
        .print_help()
        .map_err(|error| format!("无法输出帮助信息：{error}"))?;
    println!();
    Ok(())
}

fn print_version() {
    println!("nexora {}", env!("CARGO_PKG_VERSION"));
}
