use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use configuration::{LayeredConfigLoader, UserConfigStore, VersionedConfiguration};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct ServiceConfig {
    server: ServerConfig,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct ServerConfig {
    host: String,
    port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 3000,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
struct Preferences {
    schema_version: u32,
    theme: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
struct ConcurrentPreferences {
    theme: String,
    density: String,
}

impl VersionedConfiguration for Preferences {
    const CURRENT_SCHEMA_VERSION: u32 = 1;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[test]
fn local_user_store_uses_project_local_config_directory() {
    let project_dirs = ProjectDirs::from("com", "Nexora", "ConfigurationTest")
        .expect("当前平台应当可以确定本机配置目录");
    let store = UserConfigStore::<Preferences>::for_local_application(
        "com",
        "Nexora",
        "ConfigurationTest",
        "settings.toml",
    )
    .expect("应当可以创建本机用户配置存储");

    assert_eq!(
        store.path(),
        project_dirs.config_local_dir().join("settings.toml")
    );
}

#[test]
fn layered_loader_reads_toml_file() {
    let directory = temporary_directory("loader");
    let path = directory.join("server.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "[server]\nhost = \"0.0.0.0\"\nport = 8080\n").expect("应当可以写入测试配置");

    let config = LayeredConfigLoader::<ServiceConfig>::new()
        .with_required_file(&path)
        .without_environment()
        .load()
        .expect("有效 TOML 应当可以加载");

    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 8080);
    _ = fs::remove_dir_all(directory);
}

#[test]
fn later_configuration_file_overrides_earlier_file() {
    let directory = temporary_directory("layered-files");
    let base_path = directory.join("base.toml");
    let override_path = directory.join("override.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&base_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .expect("应当可以写入基础配置");
    fs::write(&override_path, "[server]\nport = 8080\n").expect("应当可以写入覆盖配置");

    let config = LayeredConfigLoader::<ServiceConfig>::new()
        .with_required_file(&base_path)
        .with_required_file(&override_path)
        .without_environment()
        .load()
        .expect("后加载的配置文件应当可以覆盖基础配置");

    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 8080);
    _ = fs::remove_dir_all(directory);
}

#[test]
fn user_store_round_trips_toml_atomically() {
    let directory = temporary_directory("store");
    let path = directory.join("settings.toml");
    let store = UserConfigStore::<Preferences>::at_path(&path);
    let preferences = Preferences {
        schema_version: 1,
        theme: "dark".to_owned(),
    };

    store.save(&preferences).expect("用户配置应当可以保存");
    let loaded = store
        .load_versioned_or_default()
        .expect("用户配置应当可以重新加载");

    assert_eq!(loaded, preferences);
    assert!(!path.with_extension("toml.tmp").exists());
    _ = fs::remove_dir_all(directory);
}

#[test]
fn user_store_update_reloads_latest_value_under_cross_process_lock() {
    let directory = temporary_directory("concurrent-update");
    let path = directory.join("settings.toml");
    let store = UserConfigStore::<ConcurrentPreferences>::at_path(&path);
    store
        .save(&ConcurrentPreferences::default())
        .expect("应当可以写入初始配置");
    let barrier = Arc::new(Barrier::new(3));

    let theme_worker = {
        let path = path.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            let store = UserConfigStore::<ConcurrentPreferences>::at_path(path);
            barrier.wait();
            store
                .update(|preferences| preferences.theme = "dark".to_owned())
                .expect("主题 patch 应当成功");
        })
    };
    let density_worker = {
        let path = path.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            let store = UserConfigStore::<ConcurrentPreferences>::at_path(path);
            barrier.wait();
            store
                .update(|preferences| preferences.density = "compact".to_owned())
                .expect("密度 patch 应当成功");
        })
    };

    barrier.wait();
    theme_worker.join().expect("主题线程不应 panic");
    density_worker.join().expect("密度线程不应 panic");
    let loaded = store.load_or_default().expect("应当可以读取合并结果");

    assert_eq!(loaded.theme, "dark");
    assert_eq!(loaded.density, "compact");
    let temporary_files = fs::read_dir(&directory)
        .expect("应当可以读取配置目录")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(temporary_files, 0);
    _ = fs::remove_dir_all(directory);
}

#[test]
fn newer_user_schema_is_rejected() {
    let directory = temporary_directory("schema");
    let path = directory.join("settings.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    fs::write(&path, "schema_version = 2\ntheme = \"dark\"\n").expect("应当可以写入测试配置");
    let store = UserConfigStore::<Preferences>::at_path(&path);

    let error = store
        .load_versioned_or_default()
        .expect_err("更高 schema 版本必须被拒绝");

    assert!(error.to_string().contains("不支持配置 schema 版本 2"));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn versioned_update_does_not_overwrite_newer_schema() {
    let directory = temporary_directory("schema-update");
    let path = directory.join("settings.toml");
    fs::create_dir_all(&directory).expect("应当可以创建测试目录");
    let original = "schema_version = 2\ntheme = \"future\"\nfuture_option = true\n";
    fs::write(&path, original).expect("应当可以写入未来版本配置");
    let store = UserConfigStore::<Preferences>::at_path(&path);

    let error = store
        .update_versioned(|preferences| preferences.theme = "dark".to_owned())
        .expect_err("旧程序不得更新更高版本 schema");

    assert!(error.to_string().contains("不支持配置 schema 版本 2"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    _ = fs::remove_dir_all(directory);
}

fn temporary_directory(label: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "configuration-{label}-{}-{timestamp}",
        std::process::id()
    ))
}
