#![cfg(feature = "account-client")]

use gpui::TestAppContext;
use nexora::account::client::{
    AccountAuthenticator, AccountLoginRuntimeError, ApiSettings, OidcSettings, Settings,
    client_config, install_authenticator, login_profile, login_session, login_snapshot, sign_out,
    start_login,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, nexora::Settings)]
struct DesktopSettings {
    #[nexora(account_client)]
    account: Settings,
}

#[gpui::test]
fn account_login_runtime_exposes_unconfigured_installed_and_signed_out_states(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        let snapshot = login_snapshot(cx);
        assert!(!snapshot.configured);
        assert!(!snapshot.authenticated);
        assert_eq!(start_login(cx), Err(AccountLoginRuntimeError::NotInstalled));

        let settings = DesktopSettings {
            account: Settings {
                api: ApiSettings {
                    endpoint: "http://127.0.0.1:3000".to_owned(),
                },
                oidc: OidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    client_id: "desktop-client".to_owned(),
                    scopes: vec!["openid".to_owned(), "profile".to_owned()],
                    redirect_uri: "http://127.0.0.1:0/auth/callback".to_owned(),
                },
            },
        };
        let config = client_config(&settings).expect("测试 Account 客户端配置应有效");
        let authenticator = AccountAuthenticator::new(&config).expect("测试认证协调器应能离线构造");
        install_authenticator(authenticator, cx);

        let snapshot = login_snapshot(cx);
        assert!(snapshot.configured);
        assert!(!snapshot.authenticated);
        assert!(!snapshot.busy);
        assert!(login_profile(cx).is_none());
        assert!(login_session(cx).is_none());

        sign_out(cx);
        let snapshot = login_snapshot(cx);
        assert!(snapshot.configured);
        assert!(!snapshot.authenticated);
        assert_eq!(snapshot.status.as_ref(), "已退出登录");
    });
}
