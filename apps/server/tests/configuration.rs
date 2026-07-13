use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn server_uses_local_profile_by_default() {
    let directory = temporary_directory("local-profile");
    fs::create_dir_all(directory.join("config")).expect("应当可以创建测试配置目录");
    fs::write(
        directory.join("config/local.toml"),
        "[server]\nhost = \"127.0.0.1\"\nport = 3100\n",
    )
    .expect("应当可以写入本地配置");

    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .env_remove("SERVER__HOST")
        .env_remove("SERVER__PORT")
        .current_dir(&directory)
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "服务端配置已加载：127.0.0.1:3100"
    );
    _ = fs::remove_dir_all(directory);
}

#[test]
fn missing_default_profile_uses_code_defaults() {
    let directory = temporary_directory("missing-profile");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");

    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .env_remove("SERVER__HOST")
        .env_remove("SERVER__PORT")
        .current_dir(&directory)
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "服务端配置已加载：127.0.0.1:3000"
    );
    _ = fs::remove_dir_all(directory);
}

#[test]
fn profile_argument_loads_matching_config_file() {
    let directory = temporary_directory("profile");
    fs::create_dir_all(directory.join("config")).expect("应当可以创建测试配置目录");
    fs::write(
        directory.join("config/production.toml"),
        "[server]\nhost = \"0.0.0.0\"\nport = 8080\n",
    )
    .expect("应当可以写入生产配置");

    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .args(["--profile", "production"])
        .env_remove("SERVER__HOST")
        .env_remove("SERVER__PORT")
        .current_dir(&directory)
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "服务端配置已加载：0.0.0.0:8080"
    );
    _ = fs::remove_dir_all(directory);
}

#[test]
fn short_config_argument_loads_toml() {
    let directory = temporary_directory("file");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nhost = \"0.0.0.0\"\nport = 3200\n").expect("应当可以写入服务配置");

    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .args(["-c", path.to_str().expect("测试路径应当是 UTF-8")])
        .env_remove("SERVER__HOST")
        .env_remove("SERVER__PORT")
        .current_dir(&directory)
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "服务端配置已加载：0.0.0.0:3200"
    );
    _ = fs::remove_dir_all(directory);
}

#[test]
fn unprefixed_environment_overrides_file() {
    let directory = temporary_directory("environment");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nhost = \"127.0.0.1\"\nport = 3200\n")
        .expect("应当可以写入服务配置");

    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .args(["--config", path.to_str().expect("测试路径应当是 UTF-8")])
        .env("SERVER__HOST", "0.0.0.0")
        .env("SERVER__PORT", "4200")
        .current_dir(&directory)
        .output()
        .expect("应当可以启动测试服务端进程");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "服务端配置已加载：0.0.0.0:4200"
    );
    _ = fs::remove_dir_all(directory);
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
