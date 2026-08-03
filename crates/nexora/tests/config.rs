#![cfg(feature = "derive")]

use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use nexora::{
    __private::{ProvidesAccountClientSettings, ProvidesAccountServerSettings},
    config::{AccountClientSection, AccountServerSection, ConfigError, Settings as _},
};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ClientAccountSettings {
    client_id: String,
    valid: bool,
}

impl AccountClientSection for ClientAccountSettings {
    fn validate_account_client(&self) -> Result<(), ConfigError> {
        if self.valid {
            Ok(())
        } else {
            Err(ConfigError::invalid_section(
                "account.client",
                "client_id 未通过测试校验",
            ))
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ServerAccountSettings {
    audience: String,
    valid: bool,
}

impl AccountServerSection for ServerAccountSettings {
    fn validate_account_server(&self) -> Result<(), ConfigError> {
        if self.valid {
            Ok(())
        } else {
            Err(ConfigError::invalid_section(
                "account.server",
                "audience 未通过测试校验",
            ))
        }
    }
}

#[derive(Debug, Deserialize, nexora::Settings)]
struct ApplicationSettings {
    application_name: String,
    #[nexora(account_client)]
    account_client: ClientAccountSettings,
    #[nexora(account_server)]
    account_server: ServerAccountSettings,
}

#[derive(Debug, Deserialize, nexora::Settings)]
struct PlainSettings {
    value: String,
}

#[test]
fn settings_derive_records_calling_package_and_exposes_account_sections() {
    let settings = ApplicationSettings {
        application_name: "Nexora Test".to_owned(),
        account_client: ClientAccountSettings {
            client_id: "desktop".to_owned(),
            valid: true,
        },
        account_server: ServerAccountSettings {
            audience: "api".to_owned(),
            valid: true,
        },
    };

    assert_eq!(ApplicationSettings::APP_NAME, env!("CARGO_PKG_NAME"));
    assert_eq!(
        ApplicationSettings::MANIFEST_DIR,
        env!("CARGO_MANIFEST_DIR")
    );
    assert_eq!(
        ProvidesAccountClientSettings::account_client_settings(&settings).client_id,
        "desktop"
    );
    assert_eq!(
        ProvidesAccountServerSettings::account_server_settings(&settings).audience,
        "api"
    );
    assert!(settings.validate().is_ok());
}

#[test]
fn initialize_prefers_explicit_path_and_loads_strongly_typed_settings() {
    let directory = temporary_directory("explicit");
    let config_path = directory.join("application.toml");
    fs::create_dir_all(&directory).expect("应当可以创建配置测试目录");
    fs::write(
        &config_path,
        concat!(
            "application_name = \"Nexora\"\n",
            "[account_client]\n",
            "client_id = \"desktop-client\"\n",
            "valid = true\n",
            "[account_server]\n",
            "audience = \"nexora-api\"\n",
            "valid = true\n",
        ),
    )
    .expect("应当可以写入配置测试文件");

    let settings: ApplicationSettings =
        nexora::config::initialize(Some(config_path)).expect("显式指定的有效配置应当可以加载");

    assert_eq!(settings.application_name, "Nexora");
    assert_eq!(settings.account_client.client_id, "desktop-client");
    assert_eq!(settings.account_server.audience, "nexora-api");
    _ = fs::remove_dir_all(directory);
}

#[test]
fn initialize_runs_derived_account_section_validation() {
    let directory = temporary_directory("validation");
    let config_path = directory.join("application.toml");
    fs::create_dir_all(&directory).expect("应当可以创建配置测试目录");
    fs::write(
        &config_path,
        concat!(
            "application_name = \"Nexora\"\n",
            "[account_client]\n",
            "client_id = \"desktop-client\"\n",
            "valid = true\n",
            "[account_server]\n",
            "audience = \"nexora-api\"\n",
            "valid = false\n",
        ),
    )
    .expect("应当可以写入配置测试文件");

    let error = nexora::config::initialize::<ApplicationSettings>(Some(config_path))
        .expect_err("无效 Account 服务端配置必须被拒绝");

    assert!(matches!(
        error,
        ConfigError::InvalidSection {
            section: "account.server",
            ..
        }
    ));
    _ = fs::remove_dir_all(directory);
}

#[test]
fn settings_without_account_sections_validate_without_extra_contracts() {
    let settings = PlainSettings {
        value: "custom".to_owned(),
    };

    assert_eq!(settings.value, "custom");
    assert!(settings.validate().is_ok());
}

#[test]
fn updater_health_arguments_do_not_override_the_default_config_path() {
    let arguments = [
        OsString::from("imes"),
        OsString::from("--nexora-updater-health-session"),
        OsString::from("session"),
        OsString::from("--nexora-updater-health-file"),
        OsString::from("/tmp/session.json"),
    ];

    assert!(nexora::config::__private::config_path_from_args(arguments).is_none());
}

#[test]
fn positional_config_path_is_preserved() {
    let arguments = [
        OsString::from("imes"),
        OsString::from("config/imes.local.toml"),
    ];

    assert_eq!(
        nexora::config::__private::config_path_from_args(arguments),
        Some("config/imes.local.toml".into())
    );
}

#[test]
fn non_toml_runtime_argument_is_not_treated_as_configuration() {
    let arguments = [OsString::from("imes"), OsString::from("--verbose")];

    assert!(nexora::config::__private::config_path_from_args(arguments).is_none());
}

#[test]
fn standard_bundle_uses_resources_configuration_without_ancestor_scanning() {
    let executable = PathBuf::from("/Applications/iMES.app/Contents/MacOS/imes");
    assert_eq!(
        nexora::config::__private::bundle_config_path_for::<PlainSettings>(&executable),
        Some(PathBuf::from(format!(
            "/Applications/iMES.app/Contents/Resources/config/{}.toml",
            env!("CARGO_PKG_NAME")
        )))
    );
    assert!(
        nexora::config::__private::bundle_config_path_for::<PlainSettings>(&PathBuf::from(
            "/tmp/Contents/MacOS/imes"
        ))
        .is_none()
    );
}

fn temporary_directory(label: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "nexora-config-{label}-{}-{timestamp}",
        std::process::id()
    ))
}
