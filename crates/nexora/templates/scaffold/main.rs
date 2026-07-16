{%- if account_enabled -%}
mod config;
mod features;

use nexora::{
    Application as _, ApplicationOptions, account::client::AccountAuthenticator, gpui::App,
};

struct DesktopApplication {
    authenticator: AccountAuthenticator,
}

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("{{ project_name }}")
            .initial_path("/")
            .window_size(900.0, 640.0)
    }

    fn initialize(&mut self, cx: &mut App) {
        nexora::account::client::install_authenticator(self.authenticator.clone(), cx);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings: config::Settings = nexora::config::initialize(None)?;
    let client_config = nexora::account::client::client_config(&settings)?;
    let authenticator = nexora::account::client::AccountAuthenticator::new(&client_config)?;

    DesktopApplication { authenticator }.run()?;
    Ok(())
}
{%- else -%}
mod features;

use nexora::{Application as _, ApplicationOptions};

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("{{ project_name }}")
            .initial_path("/")
            .window_size(900.0, 640.0)
    }
}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopApplication.run()
}
{%- endif -%}
