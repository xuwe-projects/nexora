//! Account 桌面客户端自带的用户、角色与权限管理 Feature。

mod roles;
mod users;

use crate::__private::FeatureRegistration;
use gpui::App;

use self::{
    roles::{ROLES_METADATA, create_roles_feature},
    users::{USERS_METADATA, create_users_feature},
};

/// 返回 Account 默认管理页面的回退注册记录。
///
/// 应用只要声明相同稳定 ID 或路径的普通 `Feature`，注册表就会保留应用实现并跳过对应
/// 默认页面，因此不需要再引入专用派生宏。
pub(crate) const fn default_account_feature_registrations() -> [FeatureRegistration; 2] {
    [
        FeatureRegistration::new(USERS_METADATA, create_users_feature),
        FeatureRegistration::new(ROLES_METADATA, create_roles_feature),
    ]
}

fn has_permission(cx: &App, permission: &str) -> bool {
    crate::desktop::login_profile(cx).is_some_and(|profile| {
        profile.user.is_super_admin
            || profile
                .permissions
                .iter()
                .any(|granted| granted == permission)
    })
}
