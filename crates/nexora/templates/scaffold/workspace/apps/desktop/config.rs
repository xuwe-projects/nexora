use serde::Deserialize;

#[derive(Debug, Deserialize, nexora::Settings)]
pub(crate) struct Settings {
    #[nexora(account_client)]
    pub(crate) account: nexora::account::client::Settings,
}
