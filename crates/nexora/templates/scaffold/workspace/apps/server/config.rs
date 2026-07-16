use std::net::SocketAddr;

use serde::Deserialize;

#[derive(Debug, Deserialize, nexora::Settings)]
pub(crate) struct Settings {
    pub(crate) server: ServerSettings,
    pub(crate) database: DatabaseSettings,
    #[nexora(account_server)]
    pub(crate) account: nexora::account::server::Settings,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ServerSettings {
    pub(crate) bind: SocketAddr,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DatabaseSettings {
    pub(crate) url: String,
    pub(crate) max_connections: u32,
    #[serde(default)]
    pub(crate) initialize_empty_database: bool,
}
