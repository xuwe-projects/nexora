use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn database_startup_failure_prints_context_and_hides_credentials() {
    let directory = temporary_directory();
    fs::create_dir_all(&directory).expect("应当可以创建临时配置目录");
    let config_path = directory.join("server.toml");
    fs::write(
        &config_path,
        r#"
[database]
url = "postgres://secret-user:secret-password@127.0.0.1:1/unavailable"
max_connections = 1

[oidc]
organization_id = "startup-test-organization-id"
project_id = "startup-test-project-id"
personal_access_token = "startup-test-personal-access-token"

[setup]
secret = "startup-test-setup-secret"
"#,
    )
    .expect("应当可以写入数据库失败测试配置");

    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .arg(config_path.to_str().expect("测试配置路径应当是 UTF-8"))
        .env("RUST_LOG", "error")
        .env_remove("DATABASE__URL")
        .env_remove("DATABASE__MAX_CONNECTIONS")
        .env_remove("OIDC__PERSONAL_ACCESS_TOKEN")
        .output()
        .expect("应当可以启动服务端进程");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("服务端启动失败"));
    assert!(stderr.contains("无法连接 PostgreSQL"));
    assert!(stderr.contains("请检查数据库服务、网络地址、端口和凭据"));
    assert!(!stderr.contains("Database("));
    assert!(!stderr.contains("secret-user"));
    assert!(!stderr.contains("secret-password"));

    _ = fs::remove_dir_all(directory);
}

fn temporary_directory() -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "server-startup-error-{}-{timestamp}",
        std::process::id()
    ))
}
