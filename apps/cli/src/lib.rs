//! 本地开发与发布辅助命令。

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

/// CLI 内部和库 API 共用的结果类型。
///
/// 错误会统一封装为 `CliError`，方便二进制入口和集成测试使用同一套错误表达。
pub type CliResult<T> = Result<T, CliError>;

/// 运行 `xuwecli` 命令。
///
/// 该函数接收完整命令行参数，解析子命令并执行对应流程。
/// 当前主要用于 `xuwecli build` 和 `xuwecli doctor`，二进制入口会把
/// `std::env::args_os()` 直接传入这里。
pub fn run<I, S>(args: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    let result: CliResult<()> = match cli_config(cli) {
        CliConfig::Help => {
            print_help()?;
            Ok(())
        }
        CliConfig::Version => {
            print_version();
            Ok(())
        }
        config @ CliConfig::Build(_) => {
            ensure_macos()?;
            let host_arch = detect_host_arch()?;
            let plan = build_plan(config, host_arch.trim())?;
            execute_build(plan)?;
            Ok(())
        }
        CliConfig::Doctor(config) => {
            ensure_macos()?;
            run_doctor(config.fix)?;
            Ok(())
        }
    };

    result.map_err(Into::into)
}

/// `xuwecli` 执行过程中的错误。
///
/// 该错误保存面向终端用户的中文信息，并实现标准 `Error` trait，便于向上层调用方透传。
#[derive(Debug)]
pub struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CliError {}

#[derive(Debug, Parser)]
#[command(
    name = "xuwecli",
    about = "本地开发与发布辅助命令",
    disable_version_flag = true
)]
struct Cli {
    /// 输出当前 CLI 版本并退出。
    #[arg(short = 'v', long = "version", global = true)]
    version: bool,
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// 构建、签名并打包 macOS 桌面应用。
    Build(BuildConfig),
    /// 检查本地 macOS 打包依赖。
    Doctor(DoctorConfig),
}

#[derive(Debug, Clone)]
enum CliConfig {
    Build(BuildConfig),
    Doctor(DoctorConfig),
    Help,
    Version,
}

#[derive(Args, Debug, Clone)]
struct DoctorConfig {
    /// 缺少可自动安装的依赖时尝试安装。
    #[arg(long)]
    fix: bool,
}

/// macOS 桌面应用构建模式。
///
/// 构建模式决定默认签名方式、公证行为以及产物面向本地调试还是分发。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildMode {
    /// 分发构建，默认使用 Developer ID 签名并启用公证。
    Dist,
    /// 本地构建，默认使用 ad-hoc 签名并跳过公证。
    Local,
}

/// macOS 应用签名模式。
///
/// 该枚举描述构建计划最终选择的签名策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SigningMode {
    /// 使用 Apple Developer ID Application 证书签名。
    DeveloperId,
    /// 使用本机 ad-hoc 签名，适合本地调试包。
    AdHoc,
    /// 完全跳过签名流程。
    None,
}

#[derive(Args, Debug, Clone)]
struct BuildConfig {
    /// 打包模式，`dist` 会默认走 Developer ID 签名和公证，`local` 只做本地包。
    #[arg(long, value_enum, default_value = "dist")]
    mode: BuildMode,
    /// 要打包的 Cargo package 名称。
    #[arg(short = 'p', long, default_value = "console")]
    package: String,
    /// `.app` 的名称，默认等于 package 名称。
    #[arg(long)]
    app_name: Option<String>,
    /// 产物文件名使用的应用版本号，默认使用当前 workspace package 版本。
    #[arg(long)]
    app_version: Option<String>,
    /// Rust target，默认按本机 CPU 架构识别。
    #[arg(long)]
    target: Option<String>,
    /// 签名模式。
    #[arg(long = "sign", value_enum)]
    signing: Option<SigningMode>,
    /// 跳过签名，等价于 `--sign none`。
    #[arg(long)]
    no_sign: bool,
    /// Developer ID 签名身份。
    #[arg(long)]
    sign_identity: Option<String>,
    /// `notarytool` 使用的 keychain profile 名称。
    #[arg(long)]
    notary_profile: Option<String>,
    /// 跳过 Apple 公证。
    #[arg(long)]
    skip_notarize: bool,
    /// 只生成 `.app`，不生成 DMG，也会自动跳过公证。
    #[arg(long)]
    skip_dmg: bool,
    /// 缺少打包依赖时自动安装。
    #[arg(long, default_value_t = true)]
    install_deps: bool,
    /// 禁止自动安装缺失的打包依赖。
    #[arg(long)]
    no_install_deps: bool,
    /// DMG 输出目录。
    #[arg(long, default_value = "dist")]
    output_dir: PathBuf,
    /// 传给 `codesign --entitlements` 的权限文件。
    #[arg(long)]
    entitlements: Option<PathBuf>,
    /// 传给 `cargo bundle --features` 的 features 字符串。
    #[arg(long)]
    features: Option<String>,
}

/// `xuwecli build` 解析后的构建计划。
///
/// 该类型把命令行参数、环境变量和本机架构归并成可执行的只读计划，
/// 便于执行阶段和集成测试使用同一份决策结果。
#[derive(Debug, Clone)]
pub struct BuildPlan {
    mode: BuildMode,
    package: String,
    app_name: String,
    app_version: String,
    target: String,
    signing: SigningMode,
    sign_identity: Option<String>,
    notary_profile: String,
    notarize: bool,
    create_dmg: bool,
    install_deps: bool,
    app_path: PathBuf,
    dmg_path: PathBuf,
    output_dir: PathBuf,
    entitlements: Option<PathBuf>,
    features: Option<String>,
}

impl BuildPlan {
    /// 返回构建计划使用的模式。
    ///
    /// 模式决定默认签名、公证和产物用途。
    pub fn mode(&self) -> BuildMode {
        self.mode
    }

    /// 返回要打包的 Cargo package 名称。
    ///
    /// 该名称会传给 `cargo bundle -p`。
    pub fn package(&self) -> &str {
        &self.package
    }

    /// 返回 `.app` 的展示名称。
    ///
    /// 该名称用于定位 cargo-bundle 输出目录中的应用包，并参与 DMG 文件名生成。
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// 返回产物文件名中使用的应用版本号。
    ///
    /// 默认值来自当前 package 版本，也可以通过 `--app-version` 覆盖。
    pub fn app_version(&self) -> &str {
        &self.app_version
    }

    /// 返回 Rust 编译目标三元组。
    ///
    /// 未显式传入 `--target` 时，该值会根据本机架构推导。
    pub fn target(&self) -> &str {
        &self.target
    }

    /// 返回构建计划最终使用的签名模式。
    ///
    /// 该值已经合并了 `--mode`、`--sign` 和 `--no-sign` 的影响。
    pub fn signing(&self) -> SigningMode {
        self.signing
    }

    /// 返回 Developer ID 签名身份。
    ///
    /// 当签名模式不是 Developer ID，或者需要执行阶段自动发现证书时，该值可能为空。
    pub fn sign_identity(&self) -> Option<&str> {
        self.sign_identity.as_deref()
    }

    /// 返回 Apple 公证使用的 keychain profile 名称。
    ///
    /// 该值来自命令行参数、环境变量或默认 profile。
    pub fn notary_profile(&self) -> &str {
        &self.notary_profile
    }

    /// 返回当前计划是否会执行 Apple 公证。
    ///
    /// 只有分发模式、Developer ID 签名并且生成 DMG 时，该值才会为 `true`。
    pub fn notarize(&self) -> bool {
        self.notarize
    }

    /// 返回当前计划是否会生成 DMG。
    ///
    /// 传入 `--skip-dmg` 时，该值会为 `false`。
    pub fn create_dmg(&self) -> bool {
        self.create_dmg
    }

    /// 返回缺少依赖时是否允许自动安装。
    ///
    /// 该值会同时考虑默认配置和 `--no-install-deps`。
    pub fn install_deps(&self) -> bool {
        self.install_deps
    }

    /// 返回 cargo-bundle 预期生成的 `.app` 路径。
    ///
    /// 执行阶段会用该路径确认应用包是否存在。
    pub fn app_path(&self) -> &Path {
        &self.app_path
    }

    /// 返回最终 DMG 产物路径。
    ///
    /// 路径包含输出目录、应用名称、版本号和架构后缀。
    pub fn dmg_path(&self) -> &Path {
        &self.dmg_path
    }

    /// 返回构建产物输出目录。
    ///
    /// 默认目录是 workspace 根目录下的 `dist`。
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// 返回传给 codesign 的 entitlements 文件路径。
    ///
    /// 未显式配置且默认文件不存在时，该值为空。
    pub fn entitlements(&self) -> Option<&Path> {
        self.entitlements.as_deref()
    }

    /// 返回传给 `cargo bundle --features` 的 features 字符串。
    ///
    /// 未传入 `--features` 时，该值为空。
    pub fn features(&self) -> Option<&str> {
        self.features.as_deref()
    }
}

/// 根据命令行参数和主机架构生成构建计划。
///
/// 该函数只执行参数解析和计划推导，不会检查系统依赖、运行构建命令或访问 Apple 工具链。
/// `host_arch` 应传入 `uname -m` 风格的架构名称，例如 `arm64` 或 `x86_64`。
pub fn build_plan_from_args<I, S>(args: I, host_arch: &str) -> CliResult<BuildPlan>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    let config = parse_args(args)?;

    build_plan(config, host_arch)
}

fn parse_args<I, S>(args: I) -> CliResult<CliConfig>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => Ok(cli_config(cli)),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            Ok(CliConfig::Help)
        }
        Err(error) => Err(CliError::new(error.to_string())),
    }
}

fn cli_config(cli: Cli) -> CliConfig {
    if cli.version {
        return CliConfig::Version;
    }

    match cli.command {
        Some(CliCommand::Build(config)) => CliConfig::Build(config),
        Some(CliCommand::Doctor(config)) => CliConfig::Doctor(config),
        None => CliConfig::Help,
    }
}

fn build_plan(config: CliConfig, host_arch: &str) -> CliResult<BuildPlan> {
    let CliConfig::Build(config) = config else {
        return Err(CliError::new("当前配置不是 build 命令"));
    };

    let target = match config.target {
        Some(target) => target,
        None => macos_target_for_arch(host_arch)?.to_string(),
    };
    let suffix = dmg_suffix_for_target(&target)?;
    let app_name = config.app_name.unwrap_or_else(|| config.package.clone());
    let app_version = config
        .app_version
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let signing = if config.no_sign {
        SigningMode::None
    } else {
        config.signing.unwrap_or(match config.mode {
            BuildMode::Dist => SigningMode::DeveloperId,
            BuildMode::Local => SigningMode::AdHoc,
        })
    };
    let install_deps = config.install_deps && !config.no_install_deps;
    let create_dmg = !config.skip_dmg;
    let notarize = matches!(config.mode, BuildMode::Dist)
        && !config.skip_notarize
        && create_dmg
        && matches!(signing, SigningMode::DeveloperId);

    if matches!(config.mode, BuildMode::Dist)
        && !config.skip_notarize
        && create_dmg
        && !matches!(signing, SigningMode::DeveloperId)
    {
        return Err(CliError::new(
            "`dist` 模式公证需要 `--sign developer-id`，或者显式添加 `--skip-notarize`",
        ));
    }

    let app_path = PathBuf::from("target")
        .join(&target)
        .join("release")
        .join("bundle")
        .join("osx")
        .join(format!("{app_name}.app"));
    let dmg_path = config
        .output_dir
        .join(format!("{app_name}-{app_version}-{suffix}.dmg"));
    let sign_identity = config
        .sign_identity
        .or_else(|| env::var("XUWE_MACOS_SIGN_IDENTITY").ok());
    let notary_profile = config
        .notary_profile
        .or_else(|| env::var("XUWE_NOTARY_PROFILE").ok())
        .unwrap_or_else(|| "xuwe".to_string());
    let entitlements = config.entitlements.or_else(|| {
        let path = PathBuf::from("Entitlements.plist");
        path.exists().then_some(path)
    });

    Ok(BuildPlan {
        mode: config.mode,
        package: config.package,
        app_name,
        app_version,
        target,
        signing,
        sign_identity,
        notary_profile,
        notarize,
        create_dmg,
        install_deps,
        app_path,
        dmg_path,
        output_dir: config.output_dir,
        entitlements,
        features: config.features,
    })
}

/// 根据 macOS 主机架构返回对应的 Rust target。
///
/// 支持 `arm64`、`aarch64` 和 `x86_64`，其他架构会返回面向用户的错误信息。
pub fn macos_target_for_arch(arch: &str) -> CliResult<&'static str> {
    match arch {
        "arm64" | "aarch64" => Ok("aarch64-apple-darwin"),
        "x86_64" => Ok("x86_64-apple-darwin"),
        other => Err(CliError::new(format!("不支持的 macOS 架构 `{other}`"))),
    }
}

fn dmg_suffix_for_target(target: &str) -> CliResult<&'static str> {
    match target {
        "aarch64-apple-darwin" => Ok("aarch64"),
        "x86_64-apple-darwin" => Ok("x86_64"),
        other => Err(CliError::new(format!(
            "不支持的 macOS Rust target `{other}`"
        ))),
    }
}

fn execute_build(plan: BuildPlan) -> CliResult<()> {
    println!("xuwecli build");
    println!("  mode: {:?}", plan.mode);
    println!("  package: {}", plan.package);
    println!("  app: {}", plan.app_name);
    println!("  version: {}", plan.app_version);
    println!("  target: {}", plan.target);

    ensure_build_dependencies(&plan)?;
    run_cargo_bundle(&plan)?;
    ensure_app_exists(&plan.app_path)?;
    sign_app(&plan)?;

    if plan.create_dmg {
        create_dmg(&plan)?;
    }

    if plan.notarize {
        notarize_and_staple(&plan)?;
    }

    if plan.create_dmg {
        let checksum_path = write_sha256_sidecar(&plan.dmg_path)?;
        println!("DMG: {}", plan.dmg_path.display());
        println!("SHA256: {}", checksum_path.display());
    } else {
        println!("APP: {}", plan.app_path.display());
    }

    Ok(())
}

fn ensure_macos() -> CliResult<()> {
    if env::consts::OS != "macos" {
        return Err(CliError::new("macOS 打包命令只能在 macOS 上运行"));
    }

    Ok(())
}

fn detect_host_arch() -> CliResult<String> {
    let output = Command::new("uname")
        .arg("-m")
        .output()
        .map_err(|error| CliError::new(format!("无法执行 `uname -m`: {error}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Ok(env::consts::ARCH.to_string())
    }
}

fn ensure_build_dependencies(plan: &BuildPlan) -> CliResult<()> {
    require_command("cargo")?;
    require_command("rustup")?;
    run_status(
        "rustup target add",
        Command::new("rustup")
            .arg("target")
            .arg("add")
            .arg(&plan.target),
    )?;

    if !command_exists("cargo-bundle") {
        if plan.install_deps {
            run_status(
                "cargo install cargo-bundle",
                Command::new("cargo").arg("install").arg("cargo-bundle"),
            )?;
        } else {
            return Err(CliError::new(
                "缺少 `cargo-bundle`，请安装或去掉 `--no-install-deps`",
            ));
        }
    }

    if plan.create_dmg && !command_exists("create-dmg") {
        if plan.install_deps {
            require_command("brew")?;
            run_status(
                "brew install create-dmg",
                Command::new("brew").arg("install").arg("create-dmg"),
            )?;
        } else {
            return Err(CliError::new(
                "缺少 `create-dmg`，请安装或去掉 `--no-install-deps`",
            ));
        }
    }

    if !matches!(plan.signing, SigningMode::None) {
        require_command("codesign")?;
    }

    if plan.notarize {
        require_command("xcrun")?;
    }

    Ok(())
}

fn run_cargo_bundle(plan: &BuildPlan) -> CliResult<()> {
    let mut command = Command::new("cargo");
    command
        .arg("bundle")
        .arg("--release")
        .arg("-p")
        .arg(&plan.package)
        .arg("--target")
        .arg(&plan.target);

    if let Some(features) = &plan.features {
        command.arg("--features").arg(features);
    }

    run_status("cargo bundle", &mut command)
}

fn ensure_app_exists(app_path: &Path) -> CliResult<()> {
    if app_path.exists() {
        return Ok(());
    }

    Err(CliError::new(format!(
        "没有找到生成的 .app：{}。如果 cargo-bundle 生成了不同名称，请使用 `--app-name` 指定",
        app_path.display()
    )))
}

fn sign_app(plan: &BuildPlan) -> CliResult<()> {
    match plan.signing {
        SigningMode::None => {
            println!("跳过签名");
            Ok(())
        }
        SigningMode::AdHoc => run_status(
            "codesign ad-hoc",
            Command::new("codesign")
                .arg("--force")
                .arg("--deep")
                .arg("--sign")
                .arg("-")
                .arg(&plan.app_path),
        ),
        SigningMode::DeveloperId => {
            let identity = match &plan.sign_identity {
                Some(identity) => identity.clone(),
                None => discover_developer_id_identity()?,
            };

            for attempt in 1..=5 {
                let mut command = Command::new("codesign");
                command
                    .arg("--force")
                    .arg("--deep")
                    .arg("--verbose")
                    .arg("--options")
                    .arg("runtime")
                    .arg("--timestamp");

                if let Some(entitlements) = &plan.entitlements {
                    command.arg("--entitlements").arg(entitlements);
                }

                command.arg("--sign").arg(&identity).arg(&plan.app_path);

                if command_status(&mut command)? {
                    println!("codesign succeeded on attempt {attempt}");
                    return Ok(());
                }

                if attempt == 5 {
                    return Err(CliError::new("Developer ID 签名失败，已重试 5 次"));
                }

                println!("codesign attempt {attempt} failed; retrying in 15s...");
                thread::sleep(Duration::from_secs(15));
            }

            Ok(())
        }
    }
}

fn discover_developer_id_identity() -> CliResult<String> {
    require_command("security")?;
    let output = Command::new("security")
        .arg("find-identity")
        .arg("-v")
        .arg("-p")
        .arg("codesigning")
        .output()
        .map_err(|error| CliError::new(format!("无法读取签名身份: {error}")))?;

    if !output.status.success() {
        return Err(CliError::new("读取 Keychain 签名身份失败"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let identities = stdout
        .lines()
        .filter(|line| line.contains("Developer ID Application:"))
        .filter_map(extract_quoted_identity)
        .collect::<Vec<_>>();

    match identities.as_slice() {
        [identity] => Ok(identity.clone()),
        [] => Err(CliError::new(
            "没有找到 Developer ID Application 证书，请传 `--sign-identity` 或设置 `XUWE_MACOS_SIGN_IDENTITY`",
        )),
        many => Err(CliError::new(format!(
            "找到多个 Developer ID Application 证书，请用 `--sign-identity` 指定其中一个：{}",
            many.join(" | ")
        ))),
    }
}

fn extract_quoted_identity(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let end = line.rfind('"')?;

    (end > start).then(|| line[start + 1..end].to_string())
}

fn create_dmg(plan: &BuildPlan) -> CliResult<()> {
    fs::create_dir_all(&plan.output_dir).map_err(|error| {
        CliError::new(format!(
            "无法创建输出目录 `{}`: {error}",
            plan.output_dir.display()
        ))
    })?;

    if plan.dmg_path.exists() {
        fs::remove_file(&plan.dmg_path).map_err(|error| {
            CliError::new(format!(
                "无法删除旧 DMG `{}`: {error}",
                plan.dmg_path.display()
            ))
        })?;
    }

    run_status(
        "create-dmg",
        Command::new("create-dmg")
            .arg("--volname")
            .arg(format!("{} Installer", plan.app_name))
            .arg("--window-pos")
            .arg("200")
            .arg("120")
            .arg("--window-size")
            .arg("800")
            .arg("400")
            .arg("--icon-size")
            .arg("100")
            .arg("--icon")
            .arg(format!("{}.app", plan.app_name))
            .arg("200")
            .arg("190")
            .arg("--hide-extension")
            .arg(format!("{}.app", plan.app_name))
            .arg("--app-drop-link")
            .arg("600")
            .arg("185")
            .arg(&plan.dmg_path)
            .arg(&plan.app_path),
    )
}

fn notarize_and_staple(plan: &BuildPlan) -> CliResult<()> {
    run_status(
        "notarytool submit",
        Command::new("xcrun")
            .arg("notarytool")
            .arg("submit")
            .arg(&plan.dmg_path)
            .arg("--keychain-profile")
            .arg(&plan.notary_profile)
            .arg("--wait"),
    )?;
    run_status(
        "stapler staple",
        Command::new("xcrun")
            .arg("stapler")
            .arg("staple")
            .arg(&plan.dmg_path),
    )?;
    run_status(
        "stapler validate",
        Command::new("xcrun")
            .arg("stapler")
            .arg("validate")
            .arg(&plan.dmg_path),
    )
}

/// 为指定产物写入同名 `.sha256` 校验文件。
///
/// 该函数会调用系统 `shasum -a 256` 计算哈希，并在同目录写入 `<文件名>.sha256`。
pub fn write_sha256_sidecar(path: &Path) -> CliResult<PathBuf> {
    let output = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()
        .map_err(|error| CliError::new(format!("无法执行 `shasum`: {error}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::new(format!("命令 `shasum` 执行失败：{stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let hash = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| CliError::new("`shasum` 没有输出 SHA-256 值"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::new("DMG 路径缺少可写入校验文件的文件名"))?;
    let checksum_path = path.with_file_name(format!("{file_name}.sha256"));
    let checksum = format!("{hash}  {file_name}\n");

    fs::write(&checksum_path, &checksum).map_err(|error| {
        CliError::new(format!(
            "无法写入 SHA-256 文件 `{}`: {error}",
            checksum_path.display()
        ))
    })?;
    print!("{checksum}");

    Ok(checksum_path)
}

fn run_doctor(fix: bool) -> CliResult<()> {
    let host_arch = detect_host_arch()?;
    let target = macos_target_for_arch(host_arch.trim())?;
    println!("host arch: {}", host_arch.trim());
    println!("rust target: {target}");

    check_or_install("cargo", None)?;
    check_or_install("rustup", None)?;
    run_status(
        "rustup target add",
        Command::new("rustup").arg("target").arg("add").arg(target),
    )?;
    check_or_install(
        "cargo-bundle",
        fix.then_some(("cargo", vec!["install", "cargo-bundle"])),
    )?;
    check_or_install(
        "create-dmg",
        fix.then_some(("brew", vec!["install", "create-dmg"])),
    )?;
    check_or_install("codesign", None)?;
    check_or_install("xcrun", None)?;

    Ok(())
}

fn check_or_install(command: &str, installer: Option<(&str, Vec<&str>)>) -> CliResult<()> {
    if command_exists(command) {
        println!("ok: {command}");
        return Ok(());
    }

    let Some((program, args)) = installer else {
        return Err(CliError::new(format!("缺少 `{command}`")));
    };

    let mut install = Command::new(program);
    install.args(args);
    run_status(&format!("install {command}"), &mut install)
}

fn require_command(command: &str) -> CliResult<()> {
    if command_exists(command) {
        Ok(())
    } else {
        Err(CliError::new(format!("缺少命令 `{command}`")))
    }
}

fn command_exists(command: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| path.join(command).is_file())
}

fn run_status(name: &str, command: &mut Command) -> CliResult<()> {
    if command_status(command)? {
        Ok(())
    } else {
        Err(CliError::new(format!("命令 `{name}` 执行失败")))
    }
}

fn command_status(command: &mut Command) -> CliResult<bool> {
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| CliError::new(format!("无法执行命令: {error}")))?;
    Ok(status.success())
}

fn print_help() -> CliResult<()> {
    let mut command = Cli::command();
    command
        .print_help()
        .map_err(|error| CliError::new(format!("无法输出帮助信息: {error}")))?;
    println!();
    Ok(())
}

fn print_version() {
    println!("xuwecli {}", env!("CARGO_PKG_VERSION"));
}
