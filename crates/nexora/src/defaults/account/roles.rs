//! Account 默认角色与权限管理 Feature。

mod components;

use gpui::{
    AnyView, AppContext as _, Context, Entity, IntoElement, Render, Window, div, prelude::*,
};

use crate::{
    Feature, FeatureElement, FeatureInstance, FeatureMetadata, FeatureRuntimeError, NoPath,
    NoQuery, RouteMatch,
};

use self::components::{RoleCreateDialog, RoleEditor, RolesPage};

pub(super) const ROLES_METADATA: FeatureMetadata = FeatureMetadata::new(
    "roles",
    "角色与权限",
    "/roles",
    Some("访问控制"),
    Some("asterisk"),
    None,
    910,
    true,
)
.with_visible_permissions_any(&["roles:read"]);

#[derive(Default)]
struct DefaultRolesFeature {
    page: Option<Entity<RolesPage>>,
    dialog_layer: Option<Entity<RolesDialogLayer>>,
}

struct RolesDialogLayer {
    create_dialog: Entity<RoleCreateDialog>,
    editor: Entity<RoleEditor>,
}

impl Render for RolesDialogLayer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children([self.create_dialog.clone().into_any_element()])
            .child(self.editor.clone())
    }
}

impl Feature for DefaultRolesFeature {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: FeatureMetadata = ROLES_METADATA;
}

impl Render for DefaultRolesFeature {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        <Self as FeatureElement>::render(self, window, cx)
    }
}

impl FeatureElement for DefaultRolesFeature {
    fn initialize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.new(|cx| RolesPage::new(window, cx));
        let editor = cx.new(|cx| RoleEditor::new(page.downgrade(), window, cx));
        let create_dialog = cx.new(|cx| RoleCreateDialog::new(page.downgrade(), window, cx));
        page.update(cx, |page, cx| {
            page.set_components(editor.clone(), create_dialog.downgrade(), cx);
        });
        let dialog_layer = cx.new(|_| RolesDialogLayer {
            create_dialog,
            editor,
        });
        self.page = Some(page);
        self.dialog_layer = Some(dialog_layer);
    }

    fn activated(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = &self.page {
            page.update(cx, RolesPage::load_if_needed);
        }
    }

    fn panel_overlay(&self) -> Option<AnyView> {
        self.dialog_layer.clone().map(Into::into)
    }

    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.page
            .clone()
            .expect("默认角色 Feature 必须先完成 initialize")
    }
}

pub(super) fn create_roles_feature(
    route: RouteMatch,
    window: &mut Window,
    cx: &mut gpui::App,
) -> Result<FeatureInstance, FeatureRuntimeError> {
    crate::__private::create_feature::<DefaultRolesFeature>(route, window, cx, |_, _| {
        DefaultRolesFeature::default()
    })
}
