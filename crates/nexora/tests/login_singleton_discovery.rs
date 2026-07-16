#![cfg(feature = "account-client")]

use gpui::{Context, Empty, IntoElement, Render, Window};
use nexora::{AppRegistry, RegistryError};

#[derive(Default, nexora::LoginFeature)]
struct AlphaLoginFeature;

impl Render for AlphaLoginFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Default, nexora::LoginFeature)]
struct ZuluLoginFeature;

impl Render for ZuluLoginFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[test]
fn inventory_discovery_enforces_login_feature_singleton() {
    let error = AppRegistry::discover()
        .err()
        .expect("自动发现的多个登录页面覆盖必须失败");

    assert!(matches!(
        error,
        RegistryError::DuplicateLoginFeature { first, duplicate }
            if first.ends_with("AlphaLoginFeature")
                && duplicate.ends_with("ZuluLoginFeature")
    ));
}
