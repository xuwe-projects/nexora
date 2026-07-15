use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const TEST_PERSONAL_ACCESS_TOKEN: &str = "test-personal-access-token";
const TEST_PROJECT_ID: &str = "test-project-id";
const TEST_SETUP_SECRET: &str = "test-setup-secret";

#[test]
fn server_uses_required_server_file_by_default() {
    let directory = temporary_directory("default-file");
    fs::create_dir_all(directory.join("config")).expect("应当可以创建测试配置目录");
    fs::write(
        directory.join("config/server.toml"),
        "[server]\nhost = \"127.0.0.1\"\nport = 3100\n",
    )
    .expect("应当可以写入本地配置");

    let output = clean_config_command(&directory)
        .arg("--check-config")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert_config_log(&output, "127.0.0.1:3100");
    _ = fs::remove_dir_all(directory);
}

#[test]
fn missing_default_file_is_rejected() {
    let directory = temporary_directory("missing-default-file");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");

    let output = clean_config_command(&directory)
        .arg("--check-config")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("配置加载失败"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn positional_argument_loads_config_file() {
    let directory = temporary_directory("positional-file");
    fs::create_dir_all(directory.join("config")).expect("应当可以创建测试配置目录");
    fs::write(
        directory.join("config/production.toml"),
        "[server]\nhost = \"0.0.0.0\"\nport = 8080\n",
    )
    .expect("应当可以写入生产配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            directory
                .join("config/production.toml")
                .to_str()
                .expect("测试路径应当是 UTF-8"),
        ])
        .output()
        .expect("应当可以启动测试服务端进程");

    assert_config_log(&output, "0.0.0.0:8080");
    _ = fs::remove_dir_all(directory);
}

#[test]
fn positional_config_argument_loads_toml() {
    let directory = temporary_directory("file");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nhost = \"0.0.0.0\"\nport = 3200\n").expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .output()
        .expect("应当可以启动测试服务端进程");

    assert_config_log(&output, "0.0.0.0:3200");
    _ = fs::remove_dir_all(directory);
}

#[test]
fn unprefixed_environment_overrides_file() {
    let directory = temporary_directory("environment");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nhost = \"127.0.0.1\"\nport = 3200\n")
        .expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .env("SERVER__HOST", "0.0.0.0")
        .env("SERVER__PORT", "4200")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert_config_log(&output, "0.0.0.0:4200");
    _ = fs::remove_dir_all(directory);
}

#[test]
fn personal_access_token_can_be_loaded_from_toml() {
    let directory = temporary_directory("personal-access-token-file");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(
        &path,
        "[server]\nport = 3200\n[oidc]\npersonal_access_token = \"file-secret\"\n",
    )
    .expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .env_remove("OIDC__PERSONAL_ACCESS_TOKEN")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert_config_log(&output, "127.0.0.1:3200");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("file-secret"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn personal_access_token_can_be_loaded_from_environment() {
    let directory = temporary_directory("personal-access-token-environment");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nport = 3200\n").expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .env("OIDC__PERSONAL_ACCESS_TOKEN", "environment-secret")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert_config_log(&output, "127.0.0.1:3200");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("environment-secret"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn malformed_toml_does_not_echo_the_sensitive_source_line() {
    const SECRET_MARKER: &str = "PAT_PARSE_SECRET_MUST_NOT_LEAK";

    let directory = temporary_directory("malformed-secret");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(
        &path,
        format!("[oidc]\npersonal_access_token = {SECRET_MARKER}\n"),
    )
    .expect("应当可以写入语法错误的服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("配置文件语法无效"));
    assert!(stderr.contains("server.toml"));
    assert!(!stderr.contains(SECRET_MARKER));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn personal_access_token_is_required() {
    let directory = temporary_directory("required-personal-access-token");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nport = 3200\n").expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .env_remove("OIDC__PERSONAL_ACCESS_TOKEN")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("oidc.personal_access_token 不能为空"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn project_id_is_required() {
    let directory = temporary_directory("required-project-id");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nport = 3200\n").expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .env_remove("OIDC__PROJECT_ID")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("oidc.project_id 不能为空"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn setup_secret_is_required_and_never_echoed() {
    let directory = temporary_directory("required-setup-secret");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nport = 3200\n").expect("应当可以写入服务配置");

    let output = clean_config_command(&directory)
        .args([
            "--check-config",
            path.to_str().expect("测试路径应当是 UTF-8"),
        ])
        .env_remove("SETUP__SECRET")
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("setup.secret 不能为空"));
    assert!(!stderr.contains(TEST_SETUP_SECRET));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn check_config_rejects_insecure_oidc_and_invalid_database_or_audience() {
    let cases = [
        (
            "insecure-issuer",
            "[oidc]\nissuer_url = \"http://id.example.com\"\n",
            "oidc.issuer_url 必须使用 HTTPS",
        ),
        (
            "database-scheme",
            "[database]\nurl = \"mysql://localhost/xuwe\"\n",
            "database.url 必须使用 postgres 或 postgresql scheme",
        ),
        (
            "empty-audience",
            "[oidc]\naudience = \" \"\n",
            "oidc.audience 不能为空",
        ),
    ];

    for (label, contents, expected_error) in cases {
        let directory = temporary_directory(label);
        let path = directory.join("server.toml");
        fs::create_dir_all(&directory).expect("应当可以创建测试目录");
        fs::write(&path, contents).expect("应当可以写入无效服务配置");

        let output = clean_config_command(&directory)
            .args([
                "--check-config",
                path.to_str().expect("测试路径应当是 UTF-8"),
            ])
            .output()
            .expect("应当可以启动测试服务端进程");

        assert!(!output.status.success(), "{label} 必须被配置检查拒绝");
        assert!(output.stdout.is_empty(), "配置错误不应写入 stdout");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected_error),
            "{label} 应报告明确且不含密钥的错误，实际输出: {stderr}"
        );
        _ = fs::remove_dir_all(directory);
    }
}

fn temporary_directory(label: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "server-config-{label}-{}-{timestamp}",
        std::process::id()
    ))
}

fn clean_config_command(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_server"));
    command
        .env("RUST_LOG", "info")
        .env_remove("SERVER__HOST")
        .env_remove("SERVER__PORT")
        .env_remove("DATABASE__URL")
        .env_remove("DATABASE__MAX_CONNECTIONS")
        .env_remove("OIDC__ISSUER_URL")
        .env_remove("OIDC__AUDIENCE")
        .env("OIDC__PROJECT_ID", TEST_PROJECT_ID)
        .env("OIDC__PERSONAL_ACCESS_TOKEN", TEST_PERSONAL_ACCESS_TOKEN)
        .env("SETUP__SECRET", TEST_SETUP_SECRET)
        .current_dir(directory);
    command
}

fn assert_config_log(output: &std::process::Output, expected_address: &str) {
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "配置检查不应绕过日志写入 stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("服务端配置已加载"));
    assert!(stderr.contains(format!("address={expected_address}").as_str()));
}
