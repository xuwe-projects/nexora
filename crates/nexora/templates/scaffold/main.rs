{%- if account_enabled -%}
mod config;
mod features;

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};
use nexora::{
    Application as _, ApplicationLogo, ApplicationOptions, desktop::AccountAuthenticator,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/assets"]
#[include = "icons/**/*.svg"]
#[allow_missing = true]
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        Ok(Self::get(path).map(|file| file.data))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|asset| asset.starts_with(path))
            .map(Into::into)
            .collect())
    }
}

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
            .application_assets(AppAssets)
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

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};
use nexora::{Application as _, ApplicationLogo, ApplicationOptions};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/assets"]
#[include = "icons/**/*.svg"]
#[allow_missing = true]
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }

        Ok(Self::get(path).map(|file| file.data))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|asset| asset.starts_with(path))
            .map(Into::into)
            .collect())
    }
}

struct DesktopApplication;

impl nexora::Application for DesktopApplication {
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::new()
            .application_name("{{ project_name }}")
            .application_version(env!("CARGO_PKG_VERSION"))
            .application_logo(ApplicationLogo::png(include_bytes!(
                "../assets/logos/logo-icon-128.png"
            )))
            .application_assets(AppAssets)
            .initial_path("/")
            .window_size(900.0, 640.0)
    }
}

fn main() -> Result<(), nexora::ApplicationError> {
    DesktopApplication.run()
}
{%- endif -%}
