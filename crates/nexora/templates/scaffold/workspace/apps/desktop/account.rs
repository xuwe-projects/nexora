#![allow(
    dead_code,
    reason = "Account runtime 公开给后续业务 Feature 主动触发登录和 API 请求"
)]

use nexora::{
    account::client::{AccountAuthenticator, AccountClientConfig},
    gpui::Global,
};

#[derive(Clone)]
pub(crate) struct AccountRuntime {
    config: AccountClientConfig,
    authenticator: AccountAuthenticator,
}

impl AccountRuntime {
    pub(crate) fn new(config: AccountClientConfig, authenticator: AccountAuthenticator) -> Self {
        Self {
            config,
            authenticator,
        }
    }

    /// 返回已校验的客户端配置，供业务 Feature 创建额外的 Account API 会话能力。
    pub(crate) fn config(&self) -> &AccountClientConfig {
        &self.config
    }

    /// 返回认证协调器；业务 Feature 可以主动发起登录或刷新，不会在应用启动时自动登录。
    pub(crate) fn authenticator(&self) -> &AccountAuthenticator {
        &self.authenticator
    }
}

impl Global for AccountRuntime {}
