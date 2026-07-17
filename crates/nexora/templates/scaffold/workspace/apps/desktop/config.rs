use serde::Deserialize;

#[derive(Debug, Deserialize, nexora::Settings)]
pub(crate) struct Settings {
    pub(crate) api: nexora::desktop::ApiSettings,
    #[nexora(account_client)]
    pub(crate) account: nexora::desktop::AccountSettings,
}
