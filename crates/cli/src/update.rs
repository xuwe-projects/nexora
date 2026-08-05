//! Nexora CLI 的预编译二进制自更新流程。

use std::{
    collections::BTreeMap,
    env, fs,
    fs::OpenOptions,
    io::{Read, Write as _},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use std::process::Command;

use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

const UPDATE_MANIFEST_URL: &str =
    "https://github.com/xuwe-projects/nexora/releases/latest/download/nexora-update.json";
const UPDATE_SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_SIZE: u64 = 1024 * 1024;
const DOWNLOAD_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateManifest {
    schema_version: u32,
    #[serde(alias = "version")]
    release: Version,
    #[serde(default)]
    draft: bool,
    assets: BTreeMap<String, UpdateAsset>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateAsset {
    name: String,
    url: String,
    size: u64,
    sha256: String,
}

/// 使用官方 GitHub Release 的预编译资产更新当前 `nexora` CLI。
///
/// # Errors
///
/// manifest 或资产不是 HTTPS、结构或目标不匹配、版本降级、下载大小/摘要不一致、当前
/// executable 不可写，或平台替换流程无法安全完成时返回错误。该流程不会调用 Cargo、sudo
/// 或系统提权。
pub(super) fn run() -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("nexora/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("无法创建 CLI 更新客户端：{error}"))?;
    let manifest = fetch_manifest(&client, UPDATE_MANIFEST_URL)?;
    validate_manifest(&manifest)?;

    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("当前 CLI 版本无效：{error}"))?;
    if manifest.release < current {
        return Err(format!(
            "拒绝把 Nexora CLI 从 {current} 降级到 {}",
            manifest.release
        ));
    }
    if manifest.release == current {
        println!("Nexora CLI 已是最新版本 {current}");
        return Ok(());
    }

    let target = current_target()?;
    let asset = manifest.assets.get(target).ok_or_else(|| {
        format!(
            "Nexora CLI {} 没有提供当前目标 `{target}` 的预编译资产。请手工运行：{}",
            manifest.release,
            manual_install_command(&manifest.release)
        )
    })?;
    validate_asset(target, asset)?;

    let executable =
        env::current_exe().map_err(|error| format!("无法定位当前 nexora executable：{error}"))?;
    let replacement = create_replacement_path(&executable)?;
    if let Err(error) = download_asset(&client, asset, &replacement) {
        let _ = fs::remove_file(&replacement);
        return Err(error);
    }

    #[cfg(unix)]
    {
        install_unix(&executable, &replacement)?;
        println!("Nexora CLI 已更新到 {}（{}）", manifest.release, target);
        return Ok(());
    }

    #[cfg(windows)]
    {
        launch_windows_helper(&executable, &replacement, asset)?;
        println!(
            "Nexora CLI {} 已下载并校验；当前进程退出后将由更新 helper 完成替换",
            manifest.release
        );
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("当前平台不支持 Nexora CLI 自更新".to_owned())
}

fn fetch_manifest(
    client: &reqwest::blocking::Client,
    manifest_url: &str,
) -> Result<UpdateManifest, String> {
    validate_https_url(manifest_url, "CLI update manifest URL")?;
    let response = client
        .get(manifest_url)
        .send()
        .map_err(|error| format!("下载 CLI update manifest 失败：{error}"))?;
    if response.url().scheme() != "https" {
        return Err("CLI update manifest 重定向到了非 HTTPS URL".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "下载 CLI update manifest 返回 HTTP {}",
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MANIFEST_SIZE)
    {
        return Err("CLI update manifest 超过 1 MiB 限制".to_owned());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_MANIFEST_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取 CLI update manifest 失败：{error}"))?;
    if bytes.len() as u64 > MAX_MANIFEST_SIZE {
        return Err("CLI update manifest 超过 1 MiB 限制".to_owned());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("CLI update manifest 无效：{error}"))
}

fn validate_manifest(manifest: &UpdateManifest) -> Result<(), String> {
    if manifest.schema_version != UPDATE_SCHEMA_VERSION {
        return Err(format!(
            "CLI update manifest schema_version {} 不受支持",
            manifest.schema_version
        ));
    }
    if manifest.draft {
        return Err("拒绝安装 draft GitHub Release".to_owned());
    }
    if manifest.assets.is_empty() {
        return Err("CLI update manifest 没有任何预编译资产".to_owned());
    }
    Ok(())
}

fn validate_asset(target: &str, asset: &UpdateAsset) -> Result<(), String> {
    let expected_name = expected_asset_name(target);
    if asset.name != expected_name {
        return Err(format!(
            "目标 `{target}` 的资产名不匹配；期望 `{expected_name}`，实际 `{}`",
            asset.name
        ));
    }
    if asset.size == 0 {
        return Err(format!(
            "CLI update 资产 `{}` 的 size 必须大于 0",
            asset.name
        ));
    }
    if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("CLI update 资产 `{}` 的 SHA-256 无效", asset.name));
    }
    let url = validate_https_url(&asset.url, "CLI update asset URL")?;
    let url_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default();
    if url_name != asset.name {
        return Err(format!(
            "CLI update 资产 URL 文件名 `{url_name}` 与 manifest 的 `{}` 不一致",
            asset.name
        ));
    }
    Ok(())
}

fn validate_https_url(value: &str, label: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(value).map_err(|error| format!("{label} 无效：{error}"))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(format!("{label} 必须是包含 host 的 HTTPS URL"));
    }
    Ok(url)
}

fn download_asset(
    client: &reqwest::blocking::Client,
    asset: &UpdateAsset,
    destination: &Path,
) -> Result<(), String> {
    let mut response = client
        .get(&asset.url)
        .send()
        .map_err(|error| format!("下载 CLI update 资产 `{}` 失败：{error}", asset.name))?;
    if response.url().scheme() != "https" {
        return Err(format!(
            "CLI update 资产 `{}` 重定向到了非 HTTPS URL",
            asset.name
        ));
    }
    if !response.status().is_success() {
        return Err(format!(
            "下载 CLI update 资产 `{}` 返回 HTTP {}",
            asset.name,
            response.status()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length != asset.size)
    {
        return Err(format!(
            "CLI update 资产 `{}` 的 Content-Length 与 manifest 不一致",
            asset.name
        ));
    }

    write_verified_asset(&mut response, asset, destination)
}

fn write_verified_asset(
    mut source: impl Read,
    asset: &UpdateAsset,
    destination: &Path,
) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| {
            format!(
                "无法在当前 executable 所在文件系统创建更新文件 `{}`：{error}",
                destination.display()
            )
        })?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|error| format!("读取 CLI update 资产 `{}` 失败：{error}", asset.name))?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| "CLI update 资产大小溢出".to_owned())?;
        if size > asset.size {
            return Err(format!(
                "CLI update 资产 `{}` 的实际大小超过 manifest",
                asset.name
            ));
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| format!("写入 CLI update 临时文件失败：{error}"))?;
    }
    if size != asset.size {
        return Err(format!(
            "CLI update 资产 `{}` 大小不匹配；期望 {}，实际 {size}",
            asset.name, asset.size
        ));
    }
    let actual_sha256 = hex_lower(&hasher.finalize());
    if !actual_sha256.eq_ignore_ascii_case(&asset.sha256) {
        return Err(format!("CLI update 资产 `{}` SHA-256 不匹配", asset.name));
    }
    file.sync_all()
        .map_err(|error| format!("同步 CLI update 临时文件失败：{error}"))?;
    Ok(())
}

fn create_replacement_path(executable: &Path) -> Result<PathBuf, String> {
    let parent = executable
        .parent()
        .ok_or_else(|| "当前 nexora executable 缺少父目录".to_owned())?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于 Unix 元年：{error}"))?
        .as_nanos();
    let extension = if cfg!(windows) { ".tmp.exe" } else { ".tmp" };
    Ok(parent.join(format!(
        ".nexora-update-{}-{timestamp}{extension}",
        std::process::id()
    )))
}

#[cfg(unix)]
/// 在 Unix 上保留 executable 权限并通过同文件系统 rename 原子安装已校验的替换文件。
///
/// # Errors
///
/// 无法读取或设置权限、原子替换失败时返回错误；替换失败不会修改现有 executable。
pub fn install_unix(executable: &Path, replacement: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;

    let current_mode = fs::metadata(executable)
        .map_err(|error| format!("无法读取当前 nexora executable 权限：{error}"))?
        .permissions()
        .mode();
    fs::set_permissions(
        replacement,
        fs::Permissions::from_mode(current_mode | 0o100),
    )
    .map_err(|error| format!("无法设置新 nexora executable 执行权限：{error}"))?;
    if let Err(error) = fs::rename(replacement, executable) {
        let _ = fs::remove_file(replacement);
        return Err(format!(
            "无法原子替换 `{}`：{error}；当前 CLI 保持不变",
            executable.display()
        ));
    }
    if let Some(parent) = executable.parent()
        && let Ok(directory) = fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(windows)]
fn launch_windows_helper(
    executable: &Path,
    replacement: &Path,
    asset: &UpdateAsset,
) -> Result<(), String> {
    let helper = env::temp_dir().join(format!(
        "nexora-update-helper-{}-{}.exe",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("系统时间早于 Unix 元年：{error}"))?
            .as_nanos()
    ));
    fs::copy(executable, &helper)
        .map_err(|error| format!("无法创建 Windows CLI update helper：{error}"))?;
    Command::new(&helper)
        .arg("__update-helper")
        .arg("--parent-pid")
        .arg(std::process::id().to_string())
        .arg("--target")
        .arg(executable)
        .arg("--replacement")
        .arg(replacement)
        .arg("--expected-size")
        .arg(asset.size.to_string())
        .arg("--expected-sha256")
        .arg(&asset.sha256)
        .spawn()
        .map_err(|error| format!("无法启动 Windows CLI update helper：{error}"))?;
    Ok(())
}

/// Windows 隐藏 helper 的参数与替换状态机入口。
///
/// # Errors
///
/// 父 CLI 在限定时间内没有退出、替换文件校验失败、同卷原子替换失败或权限不足时返回错误。
/// 所有失败路径都会保留原始 CLI executable。
#[cfg(windows)]
pub(super) fn run_windows_helper(
    parent_pid: u32,
    target: &Path,
    replacement: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), String> {
    wait_for_process(parent_pid)?;
    verify_file(replacement, expected_size, expected_sha256)?;
    replace_windows_file(target, replacement, parent_pid)?;
    println!("Nexora CLI 更新完成：{}", target.display());
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn run_windows_helper(
    _parent_pid: u32,
    _target: &Path,
    _replacement: &Path,
    _expected_size: u64,
    _expected_sha256: &str,
) -> Result<(), String> {
    Err("CLI update helper 仅用于 Windows".to_owned())
}

#[cfg(windows)]
fn wait_for_process(parent_pid: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };

    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, parent_pid) };
    if handle.is_null() {
        return Ok(());
    }
    let wait = unsafe { WaitForSingleObject(handle, 120_000) };
    unsafe {
        CloseHandle(handle);
    }
    if wait != WAIT_OBJECT_0 {
        return Err("Windows CLI update helper 等待原进程退出超时".to_owned());
    }
    Ok(())
}

#[cfg(windows)]
fn verify_file(path: &Path, expected_size: u64, expected_sha256: &str) -> Result<(), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Windows CLI update helper 无法读取替换文件：{error}"))?;
    let actual_size = file
        .metadata()
        .map_err(|error| format!("Windows CLI update helper 无法读取替换文件元数据：{error}"))?
        .len();
    if actual_size != expected_size {
        return Err("Windows CLI update helper 检测到替换文件大小变化".to_owned());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; DOWNLOAD_BUFFER_SIZE];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Windows CLI update helper 读取替换文件失败：{error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if !hex_lower(&hasher.finalize()).eq_ignore_ascii_case(expected_sha256) {
        return Err("Windows CLI update helper 检测到替换文件 SHA-256 变化".to_owned());
    }
    Ok(())
}

#[cfg(windows)]
fn replace_windows_file(target: &Path, replacement: &Path, parent_pid: u32) -> Result<(), String> {
    use std::{thread, time::Duration};
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let backup = target.with_file_name(format!(".nexora-update-backup-{parent_pid}.exe"));
    if backup.exists() {
        return Err(format!(
            "Windows CLI update backup 已存在：{}；原 CLI 未修改",
            backup.display()
        ));
    }
    let target_wide = wide_path(target);
    let replacement_wide = wide_path(replacement);
    let backup_wide = wide_path(&backup);
    let mut last_error = None;
    for _ in 0..50 {
        let replaced = unsafe {
            ReplaceFileW(
                target_wide.as_ptr(),
                replacement_wide.as_ptr(),
                backup_wide.as_ptr(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if replaced != 0 {
            let _ = fs::remove_file(&backup);
            return Ok(());
        }
        last_error = Some(std::io::Error::last_os_error());
        thread::sleep(Duration::from_millis(100));
    }
    if !target.exists() && backup.is_file() {
        fs::rename(&backup, target).map_err(|restore_error| {
            format!(
                "Windows CLI update 替换失败且无法从 `{}` 恢复原 CLI：{restore_error}；请保留该备份并手工恢复",
                backup.display()
            )
        })?;
    }
    Err(format!(
        "Windows CLI update 原子替换失败：{}；原 CLI 保持可用，新文件保留在 `{}`{}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "未知错误".to_owned()),
        replacement.display(),
        if backup.is_file() {
            format!("，可恢复备份位于 `{}`", backup.display())
        } else {
            String::new()
        }
    ))
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn current_target() -> Result<&'static str, String> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("linux", "x86_64") if cfg!(target_env = "gnu") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") if cfg!(target_env = "gnu") => Ok("aarch64-unknown-linux-gnu"),
        (os, arch) => Err(format!(
            "当前 CLI 平台 `{arch}-{os}` 没有受支持的预编译 target"
        )),
    }
}

fn expected_asset_name(target: &str) -> String {
    let extension = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    format!("nexora-{target}{extension}")
}

fn manual_install_command(version: &Version) -> String {
    format!(
        "cargo install --git https://github.com/xuwe-projects/nexora --tag v{version} cli --locked --bin nexora"
    )
}

/// 为集成测试校验 manifest、版本决策和目标资产，不访问网络或修改 executable。
///
/// # Errors
///
/// manifest 结构、schema、版本或目标资产无效，以及请求会导致降级或缺少当前目标时返回错误。
#[allow(dead_code)]
pub fn inspect_update_decision(
    manifest_bytes: &[u8],
    current_version: &str,
    target: &str,
) -> Result<String, String> {
    let manifest: UpdateManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|error| format!("CLI update manifest 无效：{error}"))?;
    validate_manifest(&manifest)?;
    let current =
        Version::parse(current_version).map_err(|error| format!("当前 CLI 版本无效：{error}"))?;
    if manifest.release < current {
        return Err(format!(
            "拒绝把 Nexora CLI 从 {current} 降级到 {}",
            manifest.release
        ));
    }
    if manifest.release == current {
        return Ok(format!("current:{current}"));
    }
    let asset = manifest.assets.get(target).ok_or_else(|| {
        format!(
            "Nexora CLI {} 没有提供当前目标 `{target}` 的预编译资产。请手工运行：{}",
            manifest.release,
            manual_install_command(&manifest.release)
        )
    })?;
    validate_asset(target, asset)?;
    Ok(format!("download:{}", asset.name))
}

/// 为集成测试把内存中的预编译资产按 size 与 SHA-256 约束写入指定临时文件。
///
/// # Errors
///
/// size、SHA-256 不匹配，目标已存在或写入、同步失败时返回错误。
#[allow(dead_code)]
pub fn inspect_verified_download(
    bytes: &[u8],
    expected_size: u64,
    expected_sha256: &str,
    destination: &Path,
) -> Result<(), String> {
    let asset = UpdateAsset {
        name: "nexora-x86_64-unknown-linux-gnu".to_owned(),
        url: "https://example.invalid/nexora-x86_64-unknown-linux-gnu".to_owned(),
        size: expected_size,
        sha256: expected_sha256.to_owned(),
    };
    write_verified_asset(bytes, &asset, destination)
}

/// 返回 Windows helper 的隐藏命令参数，用于跨平台验证参数契约不会污染普通帮助输出。
#[allow(dead_code)]
pub fn inspect_windows_helper_arguments(
    parent_pid: u32,
    target: &Path,
    replacement: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Vec<String> {
    vec![
        "__update-helper".to_owned(),
        "--parent-pid".to_owned(),
        parent_pid.to_string(),
        "--target".to_owned(),
        target.display().to_string(),
        "--replacement".to_owned(),
        replacement.display().to_string(),
        "--expected-size".to_owned(),
        expected_size.to_string(),
        "--expected-sha256".to_owned(),
        expected_sha256.to_owned(),
    ]
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("写入 String 不会失败");
        output
    })
}
