extern crate self as nexora;

mod config {
    #[derive(Debug, PartialEq, Eq)]
    pub struct ConfigError;

    pub trait Settings {
        const APP_NAME: &'static str;

        fn validate(&self) -> Result<(), ConfigError>;
    }

    pub trait AccountClientSection {
        fn validate_account_client(&self) -> Result<(), ConfigError>;
    }

    pub trait AccountServerSection {
        fn validate_account_server(&self) -> Result<(), ConfigError>;
    }
}

mod __private {
    use crate::config::{AccountClientSection, AccountServerSection, Settings};

    pub trait ProvidesAccountClientSettings: Settings {
        type AccountClientSettings: AccountClientSection;

        fn account_client_settings(&self) -> &Self::AccountClientSettings;
    }

    pub trait ProvidesAccountServerSettings: Settings {
        type AccountServerSettings: AccountServerSection;

        fn account_server_settings(&self) -> &Self::AccountServerSettings;
    }
}

struct ClientSettings {
    valid: bool,
}

impl config::AccountClientSection for ClientSettings {
    fn validate_account_client(&self) -> Result<(), config::ConfigError> {
        self.valid.then_some(()).ok_or(config::ConfigError)
    }
}

struct ServerSettings {
    valid: bool,
}

impl config::AccountServerSection for ServerSettings {
    fn validate_account_server(&self) -> Result<(), config::ConfigError> {
        self.valid.then_some(()).ok_or(config::ConfigError)
    }
}

#[derive(nexora_macros::Settings)]
struct ApplicationSettings {
    #[nexora(account_client)]
    client: ClientSettings,
    #[nexora(account_server)]
    server: ServerSettings,
}

#[test]
fn settings_derive_uses_calling_package_and_generates_module_providers() {
    use config::Settings as _;

    let settings = ApplicationSettings {
        client: ClientSettings { valid: true },
        server: ServerSettings { valid: true },
    };

    assert_eq!(ApplicationSettings::APP_NAME, "nexora-macros");
    assert!(__private::ProvidesAccountClientSettings::account_client_settings(&settings).valid);
    assert!(__private::ProvidesAccountServerSettings::account_server_settings(&settings).valid);
    assert!(settings.validate().is_ok());
}

#[test]
fn settings_derive_runs_each_marked_section_validation() {
    use config::Settings as _;

    let invalid_client = ApplicationSettings {
        client: ClientSettings { valid: false },
        server: ServerSettings { valid: true },
    };
    let invalid_server = ApplicationSettings {
        client: ClientSettings { valid: true },
        server: ServerSettings { valid: false },
    };

    assert_eq!(invalid_client.validate(), Err(config::ConfigError));
    assert_eq!(invalid_server.validate(), Err(config::ConfigError));
}
