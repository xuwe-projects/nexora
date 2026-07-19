//! Account 默认用户管理 Feature。

mod components;

use gpui::{
    AnyView, AppContext as _, Context, Entity, IntoElement, Render, Window, div, prelude::*,
};

use crate::{
    Feature, FeatureElement, FeatureInstance, FeatureMetadata, FeatureRuntimeError, NoPath,
    NoQuery, RouteMatch,
};

use self::components::{ProvisionUserDialog, UserRoleEditor, UsersPage};

pub(super) const USERS_METADATA: FeatureMetadata = FeatureMetadata::new(
    "users",
    "用户管理",
    "/users",
    Some("访问控制"),
    Some("user"),
    None,
    900,
    true,
)
.with_content_scrollable(false);

#[derive(Default)]
struct DefaultUsersFeature {
    page: Option<Entity<UsersPage>>,
    dialog_layer: Option<Entity<UsersDialogLayer>>,
}

struct UsersDialogLayer {
    provision_dialog: Entity<ProvisionUserDialog>,
    role_editor: Entity<UserRoleEditor>,
}

impl Render for UsersDialogLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children([self.provision_dialog.clone().into_any_element()])
            .child(self.role_editor.clone())
    }
}

impl Feature for DefaultUsersFeature {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: FeatureMetadata = USERS_METADATA;
}

impl Render for DefaultUsersFeature {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        <Self as FeatureElement>::render(self, window, cx)
    }
}

impl FeatureElement for DefaultUsersFeature {
    fn initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.new(|cx| UsersPage::new(window, cx));
        let dialog = cx.new(|cx| ProvisionUserDialog::new(page.downgrade(), window, cx));
        page.update(cx, |page, cx| {
            page.set_provision_dialog(dialog.downgrade(), cx);
        });
        let role_editor = page.read(cx).role_editor();
        let dialog_layer = cx.new(|_| UsersDialogLayer {
            provision_dialog: dialog,
            role_editor,
        });
        self.page = Some(page);
        self.dialog_layer = Some(dialog_layer);
    }

    fn activated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &self.page {
            page.update(cx, UsersPage::load_if_needed);
        }
    }

    fn panel_overlay(&self) -> Option<AnyView> {
        self.dialog_layer.clone().map(Into::into)
    }

    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.page
            .clone()
            .expect("默认用户 Feature 必须先完成 initialize")
    }
}

pub(super) fn create_users_feature(
    route: RouteMatch,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Result<FeatureInstance, FeatureRuntimeError> {
    crate::__private::create_feature::<DefaultUsersFeature>(route, window, cx, |_, _| {
        DefaultUsersFeature::default()
    })
}
