{%- if account_enabled -%}
mod config;
mod features;

use gpui::App;
use nexora::{
    Application as _, ApplicationLogo, ApplicationOptions, desktop::AccountAuthenticator,
};

struct DesktopApplication {
    authenticator: AccountAuthenticator,
}

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("{{ project_name }}")
            .application_version(env!("CARGO_PKG_VERSION"))
            .application_logo(ApplicationLogo::png(include_bytes!(
                "../assets/logos/logo-icon-128.png"
            )))
            .initial_path("/")
            .window_size(900.0, 640.0)
    }

    fn initialize(&mut self, cx: &mut App) {
        nexora::desktop::install_authenticator(self.authenticator.clone(), cx);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings: config::Settings = nexora::config::initialize(None)?;
    let client_config = nexora::desktop::client_config(&settings, &settings.api)?;
    let authenticator = nexora::desktop::AccountAuthenticator::new(&client_config)?;

    DesktopApplication { authenticator }.run()?;
    Ok(())
}
{%- else -%}
mod features;

use nexora::{Application as _, ApplicationLogo, ApplicationOptions};

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("{{ project_name }}")
            .application_version(env!("CARGO_PKG_VERSION"))
            .application_logo(ApplicationLogo::png(include_bytes!(
                "../assets/logos/logo-icon-128.png"
            )))
            .initial_path("/")
            .window_size(900.0, 640.0)
    }
}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopApplication.run()
}
{%- endif -%}
