use std::{io, net::IpAddr};

use serde::Deserialize;

#[derive(Deserialize, nexora::Settings)]
pub(crate) struct Settings {
    pub(crate) server: ServerSettings,
    pub(crate) database: DatabaseSettings,
    pub(crate) setup: SetupSettings,
    #[nexora(account_server)]
    pub(crate) account: nexora::server::AccountSettings,
}

#[derive(Deserialize)]
pub(crate) struct ServerSettings {
    pub(crate) ip: IpAddr,
    pub(crate) port: u16,
}

#[derive(Deserialize)]
pub(crate) struct DatabaseSettings {
    pub(crate) url: String,
    pub(crate) max_connections: u32,
}

#[derive(Deserialize)]
pub(crate) struct SetupSettings {
    secret: String,
}

impl SetupSettings {
    pub(crate) fn secret(&self) -> Result<&str, io::Error> {
        let secret = self.secret.trim();
        if secret.is_empty() {
            return Err(io::Error::other("setup.secret 不能为空"));
        }
        if secret.len() > 1_024 {
            return Err(io::Error::other("setup.secret 不能超过 1024 字节"));
        }
        Ok(secret)
    }
}
