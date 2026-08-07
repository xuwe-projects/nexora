//! Windows 更新包的路径安全校验与平台边界工具。

use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{UpdateError, WindowsSignatureConfig};

/// 校验 Windows ZIP 条目路径是否可以解压到受控 staging 目录。
///
/// 该函数只检查归档条目的相对路径语义，不访问文件系统。真正解压时仍必须在写入前后检查
/// staging 目录内没有符号链接、junction 或 reparse point 逃逸，并验证最终路径仍位于
/// staging 根目录下。
///
/// # Errors
///
/// 当条目为空、使用绝对路径、UNC、drive prefix、路径穿越、NTFS alternate data stream、
/// 空路径分段、控制字符或 Windows 不安全尾随字符时返回 [`UpdateError::InvalidWindowsZipEntry`]。
pub fn validate_windows_zip_entry_path(entry: &str) -> Result<PathBuf, UpdateError> {
    if entry.trim().is_empty()
        || entry.starts_with('/')
        || entry.starts_with("\\\\")
        || entry.contains('\\')
        || entry.contains('\0')
        || entry.chars().any(char::is_control)
        || has_drive_prefix(entry)
    {
        return Err(UpdateError::InvalidWindowsZipEntry(entry.to_owned()));
    }

    let mut relative = PathBuf::new();
    for part in entry.split('/') {
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.contains(':')
            || part.ends_with('.')
            || part.ends_with(' ')
            || is_reserved_windows_device_name(part)
        {
            return Err(UpdateError::InvalidWindowsZipEntry(entry.to_owned()));
        }
        relative.push(part);
    }

    Ok(relative)
}

/// 安全解压 Windows 自动更新 ZIP 到指定 staging 目录。
///
/// # Errors
///
/// 当 ZIP 损坏、条目路径不安全、包含符号链接、重复覆盖、解压体积过大，或缺少主 EXE、
/// updater EXE、`nexora-updater.json` 时返回错误。
pub fn extract_windows_update_zip(
    archive_path: &Path,
    destination: &Path,
    main_exe_name: &str,
    updater_exe_name: &str,
) -> Result<(), UpdateError> {
    validate_expected_exe_name(main_exe_name)?;
    validate_expected_exe_name(updater_exe_name)?;
    fs::create_dir_all(destination)?;
    let archive_file = fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(archive_file)
        .map_err(|error| UpdateError::InvalidWindowsZipArchive(error.to_string()))?;
    let mut seen = BTreeSet::new();
    let mut total_uncompressed = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| UpdateError::InvalidWindowsZipArchive(error.to_string()))?;
        let name = entry.name().to_owned();
        let entry_name = if entry.is_dir() {
            name.trim_end_matches('/')
        } else {
            name.as_str()
        };
        let relative = validate_windows_zip_entry_path(entry_name)?;
        if entry_is_symlink(&entry) {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "ZIP 条目 `{name}` 是符号链接"
            )));
        }
        let normalized = relative.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(normalized) {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "ZIP 条目 `{name}` 重复覆盖"
            )));
        }
        total_uncompressed = total_uncompressed
            .checked_add(entry.size())
            .ok_or_else(|| UpdateError::InvalidWindowsZipArchive("解压大小溢出".to_owned()))?;
        if total_uncompressed > MAX_WINDOWS_ZIP_UNCOMPRESSED_SIZE {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "解压大小超过 {} 字节",
                MAX_WINDOWS_ZIP_UNCOMPRESSED_SIZE
            )));
        }

        let output_path = destination.join(&relative);
        if entry.is_dir() {
            let parent = output_path.parent().ok_or_else(|| {
                UpdateError::InvalidWindowsZipArchive("ZIP 目录条目缺少父目录".to_owned())
            })?;
            ensure_no_reparse_parent(destination, parent)?;
            fs::create_dir_all(&output_path)?;
            ensure_no_reparse_parent(destination, &output_path)?;
            continue;
        }
        let parent = output_path.parent().ok_or_else(|| {
            UpdateError::InvalidWindowsZipArchive("ZIP 条目缺少父目录".to_owned())
        })?;
        ensure_no_reparse_parent(destination, parent)?;
        fs::create_dir_all(parent)?;
        ensure_no_reparse_parent(destination, parent)?;
        let mut output = fs::File::create(&output_path)?;
        io::copy(&mut entry, &mut output)?;
    }

    for required in [main_exe_name, updater_exe_name, "nexora-updater.json"] {
        let path = destination.join(required);
        if !path.is_file() {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "缺少必需文件 `{required}`"
            )));
        }
    }
    Ok(())
}

pub(crate) fn verify_staged_update_signatures(
    root: &Path,
    main_exe_name: &str,
    updater_exe_name: &str,
    signature: Option<&WindowsSignatureConfig>,
) -> Result<(), UpdateError> {
    for name in [main_exe_name, updater_exe_name] {
        let path = root.join(name);
        verify_pe_current_arch(&path)?;
        if let Some(signature) = signature {
            verify_authenticode_signature(&path, signature)?;
        }
    }
    Ok(())
}

const MAX_WINDOWS_ZIP_UNCOMPRESSED_SIZE: u64 = 4 * 1024 * 1024 * 1024;

fn has_drive_prefix(entry: &str) -> bool {
    let bytes = entry.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn validate_expected_exe_name(name: &str) -> Result<(), UpdateError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || !name.ends_with(".exe")
        || validate_windows_zip_entry_path(name).is_err()
    {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "预期 EXE 文件名 `{name}` 不合法"
        )));
    }
    Ok(())
}

fn entry_is_symlink(entry: &zip::read::ZipFile<'_>) -> bool {
    entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
}

fn ensure_no_reparse_parent(root: &Path, parent: &Path) -> Result<(), UpdateError> {
    let mut current = root.to_path_buf();
    for component in parent
        .strip_prefix(root)
        .map_err(|_| UpdateError::InvalidWindowsZipArchive("解压路径越过 staging".to_owned()))?
        .components()
    {
        current.push(component.as_os_str());
        ensure_path_is_not_reparse_point(&current)?;
    }
    Ok(())
}

fn ensure_path_is_not_reparse_point(path: &Path) -> Result<(), UpdateError> {
    if !path.try_exists()? {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "解压父目录 `{}` 是 reparse point",
            path.display()
        )));
    }
    Ok(())
}

fn is_reserved_windows_device_name(part: &str) -> bool {
    let stem = part.split_once('.').map_or(part, |(stem, _)| stem);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "CONIN$"
            | "CONOUT$"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(target_os = "windows")]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(target_os = "windows"))]
fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

pub(crate) fn default_sidecar_path() -> Result<PathBuf, UpdateError> {
    let executable = std::env::current_exe()?;
    let stem = executable
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UpdateError::SidecarUnavailable("当前可执行文件名无效".to_owned()))?;
    Ok(executable.with_file_name(format!("{stem}-updater.exe")))
}

pub(crate) fn current_install_dir() -> Result<PathBuf, UpdateError> {
    std::env::current_exe()?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| UpdateError::SidecarFailed("当前 EXE 路径缺少安装目录".to_owned()))
}

#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "仅由 Windows 服务分支调用；非 Windows 仍编译本模块以运行归档与路径测试"
    )
)]
pub(crate) fn cache_dir_for_install(
    install_dir: &Path,
    app_id: &str,
) -> Result<PathBuf, UpdateError> {
    let install_parent = install_dir
        .parent()
        .ok_or(UpdateError::CacheDirectoryUnavailable)?;
    Ok(install_parent.join(".nexora-updater").join(app_id))
}

pub(crate) fn current_main_exe_name() -> Result<String, UpdateError> {
    current_main_exe_name_from_path(&std::env::current_exe()?)
}

pub(crate) fn updater_exe_name_for(main_exe_name: &str) -> Result<String, UpdateError> {
    validate_expected_exe_name(main_exe_name)?;
    let stem = main_exe_name.strip_suffix(".exe").ok_or_else(|| {
        UpdateError::InvalidWindowsZipArchive("主 EXE 文件名缺少 .exe".to_owned())
    })?;
    Ok(format!("{stem}-updater.exe"))
}

fn current_main_exe_name_from_path(path: &Path) -> Result<String, UpdateError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UpdateError::SidecarUnavailable("当前可执行文件名无效".to_owned()))?
        .to_owned();
    validate_expected_exe_name(&name)?;
    Ok(name)
}

fn verify_pe_current_arch(path: &Path) -> Result<(), UpdateError> {
    let bytes = fs::read(path)?;
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` 不是有效 PE 文件",
            path.display()
        )));
    }
    let pe_offset =
        u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
    let machine_offset = pe_offset
        .checked_add(4)
        .ok_or_else(|| UpdateError::InvalidWindowsZipArchive("PE header offset 溢出".to_owned()))?;
    let optional_magic_offset = pe_offset.checked_add(24).ok_or_else(|| {
        UpdateError::InvalidWindowsZipArchive("PE optional header offset 溢出".to_owned())
    })?;
    if bytes.len() < optional_magic_offset + 2
        || bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0")
    {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` PE header 无效",
            path.display()
        )));
    }
    let machine = u16::from_le_bytes([bytes[machine_offset], bytes[machine_offset + 1]]);
    let optional_magic = u16::from_le_bytes([
        bytes[optional_magic_offset],
        bytes[optional_magic_offset + 1],
    ]);
    let (expected_machine, expected_arch) = if cfg!(target_arch = "x86_64") {
        (0x8664, "x86_64")
    } else if cfg!(target_arch = "aarch64") {
        (0xaa64, "aarch64")
    } else {
        return Err(UpdateError::UnsupportedPlatform);
    };
    if machine != expected_machine || optional_magic != 0x20b {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` 不是当前进程所需的 {expected_arch} PE32+ 文件",
            path.display(),
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn verify_authenticode_signature(
    path: &Path,
    signature: &WindowsSignatureConfig,
) -> Result<(), UpdateError> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _};
    use windows::{
        Win32::{
            Foundation::{HANDLE, HWND},
            Security::WinTrust::{
                WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
                WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4,
                WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN,
                WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTD_UICONTEXT_EXECUTE,
                WinVerifyTrust,
            },
        },
        core::{GUID, PCWSTR},
    };

    let mut wide_path = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide_path.as_mut_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT | WTD_DISABLE_MD2_MD4,
        dwUIContext: WTD_UICONTEXT_EXECUTE,
        ..Default::default()
    };
    let mut action = GUID::from_u128(WINTRUST_ACTION_GENERIC_VERIFY_V2.to_u128());
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut WINTRUST_DATA as *mut _,
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    let _ = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut WINTRUST_DATA as *mut _,
        )
    };
    if status == 0 {
        verify_signer_identity(path, signature)
    } else {
        Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` Authenticode 验证失败，WinVerifyTrust 状态 {status:#x}",
            path.display()
        )))
    }
}

#[cfg(not(target_os = "windows"))]
fn verify_authenticode_signature(
    _path: &Path,
    _signature: &WindowsSignatureConfig,
) -> Result<(), UpdateError> {
    Err(UpdateError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
fn verify_signer_identity(
    path: &Path,
    signature: &WindowsSignatureConfig,
) -> Result<(), UpdateError> {
    use std::{
        ffi::{OsStr, c_void},
        os::windows::ffi::OsStrExt as _,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::Security::Cryptography::{
        CERT_FIND_SUBJECT_CERT, CERT_INFO, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
        CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_INFO,
        CMSG_SIGNER_INFO_PARAM, CertCloseStore, CertFindCertificateInStore,
        CertFreeCertificateContext, CryptMsgClose, CryptMsgGetParam, CryptQueryObject, HCERTSTORE,
        PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };

    let wide_path = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut store: HCERTSTORE = null_mut();
    let mut message: *mut c_void = null_mut();
    let queried = unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            wide_path.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut store,
            &mut message,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` 无法读取 Authenticode signer",
            path.display()
        )));
    }

    let result = (|| {
        let mut signer_size = 0_u32;
        let sized = unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_INFO_PARAM,
                0,
                null_mut(),
                &mut signer_size,
            )
        };
        if sized == 0 || signer_size == 0 {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "`{}` 缺少 Authenticode signer info",
                path.display()
            )));
        }

        let mut signer_buffer = vec![0_u8; signer_size as usize];
        let read = unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_INFO_PARAM,
                0,
                signer_buffer.as_mut_ptr().cast(),
                &mut signer_size,
            )
        };
        if read == 0 {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "`{}` 读取 Authenticode signer info 失败",
                path.display()
            )));
        }

        let signer = unsafe { &*(signer_buffer.as_ptr().cast::<CMSG_SIGNER_INFO>()) };
        let mut cert_info = CERT_INFO {
            Issuer: signer.Issuer,
            SerialNumber: signer.SerialNumber,
            ..Default::default()
        };
        let cert = unsafe {
            CertFindCertificateInStore(
                store,
                X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                0,
                CERT_FIND_SUBJECT_CERT,
                (&mut cert_info as *mut CERT_INFO).cast(),
                null(),
            )
        };
        if cert.is_null() {
            return Err(UpdateError::InvalidWindowsZipArchive(format!(
                "`{}` 无法定位 Authenticode signer 证书",
                path.display()
            )));
        }

        let signer_result = verify_certificate_identity(path, cert, signature);
        unsafe {
            CertFreeCertificateContext(cert);
        }
        signer_result
    })();

    unsafe {
        CryptMsgClose(message);
        CertCloseStore(store, 0);
    }
    result
}

#[cfg(target_os = "windows")]
fn verify_certificate_identity(
    path: &Path,
    cert: *const windows_sys::Win32::Security::Cryptography::CERT_CONTEXT,
    signature: &WindowsSignatureConfig,
) -> Result<(), UpdateError> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Security::Cryptography::{
        CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_SHA1_HASH_PROP_ID, CertGetCertificateContextProperty,
        CertGetNameStringW,
    };

    let mut hash_len = 0_u32;
    let sized = unsafe {
        CertGetCertificateContextProperty(cert, CERT_SHA1_HASH_PROP_ID, null_mut(), &mut hash_len)
    };
    if sized == 0 || hash_len == 0 {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` 无法读取 signer 证书 thumbprint",
            path.display()
        )));
    }
    let mut hash = vec![0_u8; hash_len as usize];
    let read = unsafe {
        CertGetCertificateContextProperty(
            cert,
            CERT_SHA1_HASH_PROP_ID,
            hash.as_mut_ptr().cast(),
            &mut hash_len,
        )
    };
    if read == 0 {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` 读取 signer 证书 thumbprint 失败",
            path.display()
        )));
    }
    let actual_thumbprint = hex_upper(&hash);
    if actual_thumbprint != signature.signer_thumbprint {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` signer thumbprint 不匹配",
            path.display()
        )));
    }

    let name_len = unsafe {
        CertGetNameStringW(
            cert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            null_mut(),
            0,
        )
    };
    if name_len <= 1 {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` signer publisher 为空",
            path.display()
        )));
    }
    let mut name = vec![0_u16; name_len as usize];
    let read = unsafe {
        CertGetNameStringW(
            cert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            name.as_mut_ptr(),
            name_len,
        )
    };
    if read <= 1 {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` 读取 signer publisher 失败",
            path.display()
        )));
    }
    let actual_publisher = String::from_utf16_lossy(&name[..read as usize - 1]);
    if actual_publisher != signature.publisher {
        return Err(UpdateError::InvalidWindowsZipArchive(format!(
            "`{}` signer publisher 不匹配",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02X}").expect("写入 String 不会失败");
        output
    })
}

pub(crate) struct InstallHelperRequest<'a> {
    pub process_id: u32,
    pub app_id: &'a str,
    pub main_exe_name: &'a str,
    pub current_app: &'a Path,
    pub staged_app: &'a Path,
    pub staging_root: &'a Path,
    pub sidecar_path: &'a Path,
    pub health_timeout: Duration,
    pub pending_records: Option<(&'a Path, &'a Path)>,
}

pub(crate) fn spawn_install_helper(request: InstallHelperRequest<'_>) -> Result<(), UpdateError> {
    ensure_windows()?;
    if !request.sidecar_path.is_file() {
        return Err(UpdateError::SidecarUnavailable(format!(
            "`{}` 不存在或不是文件",
            request.sidecar_path.display()
        )));
    }
    preflight_install_layout(
        request.current_app,
        request.staged_app,
        request.staging_root,
        request.main_exe_name,
    )?;

    let sidecar_runtime = copy_sidecar_to_temp(request.app_id, request.sidecar_path)?;
    install_helper_command(&sidecar_runtime, request)?.spawn()?;
    Ok(())
}

pub(crate) fn install_helper_command(
    sidecar_runtime: &Path,
    request: InstallHelperRequest<'_>,
) -> Result<Command, UpdateError> {
    let sidecar_working_dir = sidecar_runtime
        .parent()
        .ok_or_else(|| UpdateError::SidecarUnavailable("临时 sidecar 路径缺少父目录".to_owned()))?;
    let mut command = Command::new(sidecar_runtime);
    command
        .current_dir(sidecar_working_dir)
        .arg("--nexora-updater-sidecar")
        .arg("apply")
        .arg("--app-id")
        .arg(request.app_id)
        .arg("--parent-pid")
        .arg(request.process_id.to_string())
        .arg("--main-exe-name")
        .arg(request.main_exe_name)
        .arg("--current-app")
        .arg(request.current_app)
        .arg("--staged-app")
        .arg(request.staged_app)
        .arg("--staging-root")
        .arg(request.staging_root)
        .arg("--health-timeout-seconds")
        .arg(request.health_timeout.as_secs().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some((pending_record, installing_record)) = request.pending_records {
        command
            .arg("--pending-record")
            .arg(pending_record)
            .arg("--installing-record")
            .arg(installing_record);
    }
    Ok(command)
}

pub(crate) fn apply_staged_update(
    parent_pid: u32,
    main_exe_name: &str,
    current_app: &Path,
    staged_app: &Path,
    staging_root: &Path,
    health_session: &str,
    health_timeout: Duration,
) -> Result<(), UpdateError> {
    ensure_windows()?;
    validate_expected_exe_name(main_exe_name)?;
    wait_for_process_exit(parent_pid, Duration::from_secs(120))?;
    ensure_same_volume(current_app, staged_app)?;
    ensure_same_volume(current_app, staging_root)?;

    let app_name = current_app
        .file_name()
        .ok_or_else(|| UpdateError::SidecarFailed("当前安装目录缺少名称".to_owned()))?;
    let backup_root = staging_root.join("backup");
    let failed_root = staging_root.join("failed");
    let health_file = staging_root.join("health").join("session.json");
    let backup_app = backup_root.join(app_name);
    let failed_app = failed_root.join(app_name);

    let result = apply_staged_update_inner(
        current_app,
        staged_app,
        &backup_app,
        &health_file,
        main_exe_name,
        health_session,
        health_timeout,
    );
    if let Err(error) = result {
        let recovery =
            restore_previous_version(current_app, &backup_app, &failed_root, &failed_app);
        if recovery.is_ok() && !backup_app.exists() {
            _ = retry_remove_dir_all(staging_root);
        }
        return match recovery {
            Ok(()) => Err(error),
            Err(recovery_error) => Err(UpdateError::SidecarFailed(format!(
                "更新安装失败，且无法恢复旧版本: {recovery_error}"
            ))),
        };
    }

    _ = retry_remove_dir_all(&backup_root);
    _ = retry_remove_dir_all(staging_root);
    Ok(())
}

fn apply_staged_update_inner(
    current_app: &Path,
    staged_app: &Path,
    backup_app: &Path,
    health_file: &Path,
    main_exe_name: &str,
    health_session: &str,
    health_timeout: Duration,
) -> Result<(), UpdateError> {
    let backup_root = backup_app
        .parent()
        .ok_or_else(|| UpdateError::SidecarFailed("备份目录缺少父目录".to_owned()))?;
    fs::create_dir_all(backup_root)?;
    if let Some(parent) = health_file.parent() {
        fs::create_dir_all(parent)?;
    }
    if backup_app.exists() {
        retry_remove_dir_all(backup_app)?;
    }
    retry_rename(current_app, backup_app).map_err(|error| {
        UpdateError::SidecarFailed(format!(
            "无法备份旧版本 `{}`: {error}",
            current_app.display()
        ))
    })?;
    retry_rename(staged_app, current_app).map_err(|error| {
        UpdateError::SidecarFailed(format!(
            "无法替换新版本 `{}`: {error}",
            current_app.display()
        ))
    })?;
    preserve_inno_uninstaller_files(backup_app, current_app)?;

    let mut child = launch_app(current_app, main_exe_name, health_session, health_file)?;
    if let Err(error) = wait_for_health(health_file, health_session, health_timeout) {
        _ = child.kill();
        _ = child.wait();
        return Err(error);
    }
    Ok(())
}

fn restore_previous_version(
    current_app: &Path,
    backup_app: &Path,
    failed_root: &Path,
    failed_app: &Path,
) -> Result<(), UpdateError> {
    if backup_app.exists() {
        if current_app.exists() {
            let retained_failed_version = fs::create_dir_all(failed_root)
                .and_then(|()| {
                    if failed_app.exists() {
                        retry_remove_dir_all(failed_app)?;
                    }
                    retry_rename(current_app, failed_app)
                })
                .is_ok();
            if !retained_failed_version {
                retry_remove_dir_all(current_app)?;
            }
        }
        restore_backup(current_app, backup_app)?;
    }
    if !current_app.is_dir() {
        return Err(UpdateError::SidecarFailed(
            "旧版本安装目录不可用".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_windows() -> Result<(), UpdateError> {
    if cfg!(target_os = "windows") {
        return Ok(());
    }
    Err(UpdateError::UnsupportedPlatform)
}

#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "仅由 Windows 服务分支调用；非 Windows 仍编译本模块以运行归档与路径测试"
    )
)]
pub(crate) fn prepare_cache_dir(cache_dir: &Path) -> Result<(), UpdateError> {
    fs::create_dir_all(cache_dir)?;
    #[cfg(target_os = "windows")]
    {
        use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
        };

        let hidden_root = cache_dir.parent().unwrap_or(cache_dir);
        let path = OsStr::new(hidden_root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
        if attributes == INVALID_FILE_ATTRIBUTES {
            tracing::warn!(
                path = %hidden_root.display(),
                error = %io::Error::last_os_error(),
                "无法读取 Windows 更新事务目录属性"
            );
        } else if attributes & FILE_ATTRIBUTE_HIDDEN == 0
            && unsafe { SetFileAttributesW(path.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN) } == 0
        {
            tracing::warn!(
                path = %hidden_root.display(),
                error = %io::Error::last_os_error(),
                "无法隐藏 Windows 更新事务目录"
            );
        }
    }
    Ok(())
}

fn copy_sidecar_to_temp(app_id: &str, sidecar_path: &Path) -> Result<PathBuf, UpdateError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    let nonce = random.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("写入 String 不会失败");
        output
    });
    let temp_root = std::env::temp_dir()
        .join("nexora-updater-sidecar")
        .join(app_id)
        .join(format!("{timestamp}-{nonce}"));
    fs::create_dir_all(&temp_root)?;
    let sidecar_name = sidecar_path
        .file_name()
        .ok_or_else(|| UpdateError::SidecarUnavailable("sidecar 路径缺少文件名".to_owned()))?;
    let runtime_path = temp_root.join(sidecar_name);
    fs::copy(sidecar_path, &runtime_path).map_err(|error| {
        UpdateError::SidecarUnavailable(format!(
            "无法复制 `{}` 到 `{}`: {error}",
            sidecar_path.display(),
            runtime_path.display()
        ))
    })?;
    Ok(runtime_path)
}

pub(crate) fn preflight_install_layout(
    current_app: &Path,
    staged_app: &Path,
    staging_root: &Path,
    main_exe_name: &str,
) -> Result<(), UpdateError> {
    validate_expected_exe_name(main_exe_name)?;
    let current_app = fs::canonicalize(current_app)
        .map_err(|error| UpdateError::SidecarFailed(format!("无法读取当前安装目录: {error}")))?;
    let staged_app = fs::canonicalize(staged_app)
        .map_err(|error| UpdateError::SidecarFailed(format!("无法读取暂存更新: {error}")))?;
    let staging_root = fs::canonicalize(staging_root)
        .map_err(|error| UpdateError::SidecarFailed(format!("无法读取更新事务目录: {error}")))?;
    ensure_same_volume(&current_app, &staged_app)?;
    ensure_same_volume(&current_app, &staging_root)?;
    if !staged_app.starts_with(&staging_root)
        || staging_root.starts_with(&current_app)
        || current_app.starts_with(&staging_root)
    {
        return Err(UpdateError::SidecarFailed(
            "Windows 更新事务目录与安装目录边界无效".to_owned(),
        ));
    }
    main_executable_path(&current_app, main_exe_name)?;
    main_executable_path(&staged_app, main_exe_name)?;

    let install_parent = current_app
        .parent()
        .ok_or_else(|| UpdateError::SidecarFailed("安装目录缺少父目录".to_owned()))?;
    verify_directory_rename_permission(install_parent)
}

fn verify_directory_rename_permission(parent: &Path) -> Result<(), UpdateError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| UpdateError::Random(error.to_string()))?;
    let nonce = random.iter().fold(String::new(), |mut output, byte| {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("写入 String 不会失败");
        output
    });
    let source = parent.join(format!(".nexora-update-preflight-{nonce}"));
    let destination = parent.join(format!(".nexora-update-preflight-{nonce}-renamed"));
    let result = (|| {
        fs::create_dir(&source)?;
        fs::rename(&source, &destination)?;
        fs::remove_dir(&destination)?;
        Ok::<_, io::Error>(())
    })();
    if result.is_err() {
        _ = fs::remove_dir(&source);
        _ = fs::remove_dir(&destination);
    }
    result.map_err(|error| {
        UpdateError::SidecarFailed(format!(
            "安装目录不可写，无法安全替换应用；请重新选择当前用户可写的安装路径: {error}"
        ))
    })
}

fn wait_for_process_exit(process_id: u32, timeout: Duration) -> Result<(), UpdateError> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_is_running(process_id) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(UpdateError::SidecarFailed(format!(
        "主应用进程 {process_id} 未在限定时间内退出"
    )))
}

pub(crate) fn process_is_running(process_id: u32) -> bool {
    if !cfg!(target_os = "windows") {
        return false;
    }
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {process_id}")])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(&process_id.to_string())
        })
        .unwrap_or(false)
}

pub(crate) fn relaunch_app(install_dir: &Path, main_exe_name: &str) -> Result<(), UpdateError> {
    drop(launch_app(install_dir, main_exe_name, "", Path::new(""))?);
    Ok(())
}

fn restore_backup(current_app: &Path, backup_app: &Path) -> Result<(), UpdateError> {
    if current_app.exists() {
        retry_remove_dir_all(current_app)?;
    }
    retry_rename(backup_app, current_app).map_err(|error| {
        UpdateError::SidecarFailed(format!(
            "无法恢复旧版本 `{}`: {error}",
            current_app.display()
        ))
    })
}

fn launch_app(
    install_dir: &Path,
    main_exe_name: &str,
    health_session: &str,
    health_file: &Path,
) -> Result<std::process::Child, UpdateError> {
    let executable = main_executable_path(install_dir, main_exe_name)?;
    let mut command = Command::new(executable);
    command.current_dir(install_dir);
    if !health_session.is_empty() {
        command
            .arg("--nexora-updater-health-session")
            .arg(health_session)
            .arg("--nexora-updater-health-file")
            .arg(health_file);
    }
    Ok(command.spawn()?)
}

fn main_executable_path(install_dir: &Path, main_exe_name: &str) -> Result<PathBuf, UpdateError> {
    validate_expected_exe_name(main_exe_name)?;
    let executable = install_dir.join(main_exe_name);
    if executable.is_file() {
        return Ok(executable);
    }
    Err(UpdateError::SidecarFailed(format!(
        "安装目录缺少主 EXE `{main_exe_name}`"
    )))
}

pub(crate) fn preserve_inno_uninstaller_files(
    backup_app: &Path,
    current_app: &Path,
) -> Result<(), UpdateError> {
    for entry in fs::read_dir(backup_app)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !is_inno_uninstaller_file(name) {
            continue;
        }

        let destination = current_app.join(&file_name);
        if destination.try_exists()? {
            return Err(UpdateError::SidecarFailed(format!(
                "暂存更新不允许包含 Inno Setup 卸载器文件 `{name}`"
            )));
        }
        fs::copy(entry.path(), destination)?;
    }
    Ok(())
}

fn is_inno_uninstaller_file(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    let Some((stem, extension)) = normalized.rsplit_once('.') else {
        return false;
    };
    let Some(sequence) = stem.strip_prefix("unins") else {
        return false;
    };
    !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(extension, "exe" | "dat" | "msg")
}

fn wait_for_health(
    health_file: &Path,
    health_session: &str,
    timeout: Duration,
) -> Result<(), UpdateError> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Ok(contents) = fs::read_to_string(health_file) {
            if contents.contains(health_session) {
                return Ok(());
            }
            return Err(UpdateError::InvalidHealthSession);
        }
        thread::sleep(Duration::from_millis(250));
    }

    Err(UpdateError::HealthCheckTimedOut)
}

fn ensure_same_volume(left: &Path, right: &Path) -> Result<(), UpdateError> {
    let left_prefix = left.components().next();
    let right_prefix = right.components().next();
    if left_prefix == right_prefix {
        return Ok(());
    }
    Err(UpdateError::SidecarFailed(
        "Windows staging、backup 与安装目录必须位于同一卷".to_owned(),
    ))
}

pub(crate) fn replace_file(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    #[cfg(target_os = "windows")]
    {
        use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _};
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let source = OsStr::new(source)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let destination = OsStr::new(destination)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let replaced = unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if replaced == 0 {
            return Err(UpdateError::Io(io::Error::last_os_error()));
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(source, destination)?;
        Ok(())
    }
}

fn retry_rename(source: &Path, destination: &Path) -> std::io::Result<()> {
    retry_file_operation(|| fs::rename(source, destination))
}

fn retry_remove_dir_all(path: &Path) -> std::io::Result<()> {
    retry_file_operation(|| fs::remove_dir_all(path))
}

fn retry_file_operation(mut operation: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut delay = Duration::from_millis(100);
    let mut last_error = None;
    for _ in 0..20 {
        match operation() {
            Ok(()) => return Ok(()),
            Err(error) if is_retryable_file_error(&error) => {
                last_error = Some(error);
                thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_secs(2));
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("Windows file operation failed")))
}

fn is_retryable_file_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
    )
}
