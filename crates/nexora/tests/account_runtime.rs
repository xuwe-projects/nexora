#![cfg(feature = "desktop")]

use gpui::{AppContext as _, Context, Subscription, TestAppContext};
use nexora::desktop::{
    AccountAuthenticationScope, AccountAuthenticator, AccountLoginRuntimeError,
    AccountOidcSettings, AccountSettings, ApiSettings, authentication_scope, client_config,
    install_authenticator, login_profile, login_session, login_snapshot, observe_authentication,
    sign_out, start_login,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, nexora::Settings)]
struct DesktopSettings {
    api: ApiSettings,
    #[nexora(account_client)]
    account: AccountSettings,
}

#[derive(Default)]
struct AuthenticationObserver {
    scopes: Vec<AccountAuthenticationScope>,
    _subscription: Option<Subscription>,
}

impl AuthenticationObserver {
    fn new(cx: &mut Context<Self>) -> Self {
        let subscription = observe_authentication(cx, |this, scope, _cx| {
            this.scopes.push(scope);
        });
        Self {
            scopes: Vec::new(),
            _subscription: Some(subscription),
        }
    }
}

fn test_authenticator() -> AccountAuthenticator {
    let settings = DesktopSettings {
        api: ApiSettings {
            endpoint: "http://127.0.0.1:3000".to_owned(),
        },
        account: AccountSettings {
            oidc: AccountOidcSettings {
                issuer_url: "https://identity.example.com".to_owned(),
                client_id: "desktop-client".to_owned(),
                scopes: vec!["openid".to_owned(), "profile".to_owned()],
                redirect_uri: "http://127.0.0.1:0/auth/callback".to_owned(),
            },
        },
    };
    let config = client_config(&settings, &settings.api).expect("测试 Account 客户端配置应有效");
    AccountAuthenticator::new(&config).expect("测试认证协调器应能离线构造")
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

        assert_eq!(
            authentication_scope(cx),
            AccountAuthenticationScope::default()
        );

        install_authenticator(test_authenticator(), cx);

        let snapshot = login_snapshot(cx);
        assert!(snapshot.configured);
        assert!(!snapshot.authenticated);
        assert!(!snapshot.busy);
        assert!(login_profile(cx).is_none());
        assert!(login_session(cx).is_none());
        assert_eq!(
            authentication_scope(cx),
            AccountAuthenticationScope::default()
        );

        sign_out(cx);
        let snapshot = login_snapshot(cx);
        assert!(snapshot.configured);
        assert!(!snapshot.authenticated);
        assert_eq!(snapshot.status.as_ref(), "已退出登录");
        assert_eq!(authentication_scope(cx).revision, 1);
    });
}

#[gpui::test]
fn account_authentication_observer_reports_scope_revisions(cx: &mut TestAppContext) {
    let authenticator = test_authenticator();
    cx.update(|cx| install_authenticator(authenticator.clone(), cx));
    let observer = cx.new(AuthenticationObserver::new);
    cx.run_until_parked();

    cx.update(sign_out);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&observer, |observer, _| observer.scopes.clone()),
        [AccountAuthenticationScope {
            revision: 1,
            user_id: None,
        }]
    );

    cx.update(|cx| install_authenticator(authenticator, cx));
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&observer, |observer, _| observer.scopes.clone()),
        [
            AccountAuthenticationScope {
                revision: 1,
                user_id: None,
            },
            AccountAuthenticationScope {
                revision: 2,
                user_id: None,
            },
        ]
    );
}
