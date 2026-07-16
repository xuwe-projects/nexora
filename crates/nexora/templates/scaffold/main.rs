{%- if account_enabled -%}
mod account;
mod config;
mod features;

use crate::account::AccountRuntime;
use nexora::{Application as _, ApplicationOptions, gpui::App};

struct DesktopApplication {
    account: AccountRuntime,
}

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .initial_path("/")
            .window_size(900.0, 640.0)
    }

    fn initialize(&mut self, cx: &mut App) {
        cx.set_global(self.account.clone());
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings: config::Settings = nexora::config::initialize(None)?;
    let client_config = nexora::account::client::client_config(&settings)?;
    let authenticator = nexora::account::client::AccountAuthenticator::new(&client_config)?;

    DesktopApplication {
        account: AccountRuntime::new(client_config, authenticator),
    }
    .run()?;
    Ok(())
}
{%- else -%}
mod features;

use nexora::{Application as _, ApplicationOptions};

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .initial_path("/")
            .window_size(900.0, 640.0)
    }
}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopApplication.run()
}
{%- endif -%}
