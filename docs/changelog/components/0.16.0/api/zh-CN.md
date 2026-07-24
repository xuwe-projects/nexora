## Account 角色 owner 作用域

- Account 角色新增 `owner` 字段，默认后台系统范围为 `IMES`，客户门户可使用客户 ID 字符串隔离角色。
- 新增 owner 作用域可信宿主 API，包括 `create_role_for_owner`、`create_generated_role_for_owner`、`replace_user_roles_for_owner` 与 `grant_user_role`。
- 新增全局门户管理员系统角色 `portal_admin`，由宿主通过 `ensure_system_role_with_permissions` 同步权限，普通角色编辑和删除入口不可修改。
