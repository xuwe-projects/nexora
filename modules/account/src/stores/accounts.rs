//! 账号持久化端口与 PostgreSQL 实现入口。

mod postgres;

use async_trait::async_trait;
use kernel::{Page, PageRequest};
use uuid::Uuid;

use crate::{
    AccessProfile, CreateRole, ExternalIdentity, Permission, Role, StoreError, UpdateRole, User,
    UserStatus,
};

pub use postgres::PostgresAccountsStore;

/// 账号领域依赖的持久化端口。
///
/// application 只通过该 trait 访问用户、角色和权限；SQL、事务与连接池实现不得进入
/// application、handler 或领域模型。
#[async_trait]
pub trait AccountsStore: Send + Sync {
    /// 创建或更新外部身份对应的本地用户，并确保默认角色存在。
    ///
    /// # Errors
    ///
    /// 数据库不可用、系统角色缺失或身份数据违反数据库约束时返回错误。
    async fn sync_identity(&self, identity: &ExternalIdentity) -> Result<User, StoreError>;

    /// 返回当前唯一内置超级管理员；尚未完成首次绑定时返回 `None`。
    ///
    /// # Errors
    ///
    /// 数据库不可用或用户数据无法映射为领域模型时返回错误。
    async fn super_admin(&self) -> Result<Option<User>, StoreError>;

    /// 原子绑定唯一内置超级管理员身份，并把直接角色收敛为成员与超级管理员角色。
    ///
    /// 对同一 issuer/subject 重复调用是幂等的；已经绑定其他身份时拒绝替换。
    ///
    /// # Errors
    ///
    /// 已经绑定其他超级管理员、系统角色缺失、身份违反数据库约束或事务失败时返回错误。
    async fn bind_super_admin(&self, identity: &ExternalIdentity) -> Result<User, StoreError>;

    /// 加载用户、直接角色以及合并后的权限。
    ///
    /// # Errors
    ///
    /// 用户不存在、数据库不可用或持久化数据无法映射为领域模型时返回错误。
    async fn access_profile(&self, user_id: Uuid) -> Result<AccessProfile, StoreError>;

    /// 按页码返回用户集合。
    ///
    /// # Errors
    ///
    /// 数据库不可用或用户数据无法映射为领域模型时返回错误。
    async fn list_users(&self, request: PageRequest) -> Result<Page<User>, StoreError>;

    /// 返回指定用户。
    ///
    /// # Errors
    ///
    /// 用户不存在、数据库不可用或用户状态值无效时返回错误。
    async fn user(&self, user_id: Uuid) -> Result<User, StoreError>;

    /// 修改用户状态，并原子保护最后一个启用管理员。
    ///
    /// # Errors
    ///
    /// 用户不存在、操作会停用最后一个管理员、系统角色缺失或数据库不可用时返回错误。
    async fn set_user_status(&self, user_id: Uuid, status: UserStatus) -> Result<User, StoreError>;

    /// 返回所有角色及其权限。
    ///
    /// # Errors
    ///
    /// 数据库不可用或角色、权限数据无法读取时返回错误。
    async fn list_roles(&self) -> Result<Vec<Role>, StoreError>;

    /// 返回指定角色及其权限。
    ///
    /// # Errors
    ///
    /// 角色不存在或数据库不可用时返回错误。
    async fn role(&self, role_id: Uuid) -> Result<Role, StoreError>;

    /// 创建自定义角色及其初始权限。
    ///
    /// # Errors
    ///
    /// 角色键冲突、权限不存在、字段违反数据库约束或数据库不可用时返回错误。
    async fn create_role(&self, input: &CreateRole) -> Result<Role, StoreError>;

    /// 修改自定义角色元数据。
    ///
    /// # Errors
    ///
    /// 角色不存在、角色是系统角色、字段违反数据库约束或数据库不可用时返回错误。
    async fn update_role(&self, role_id: Uuid, input: &UpdateRole) -> Result<Role, StoreError>;

    /// 删除未被用户引用的自定义角色。
    ///
    /// # Errors
    ///
    /// 角色不存在、角色是系统角色、仍被用户引用或数据库不可用时返回错误。
    async fn delete_role(&self, role_id: Uuid) -> Result<(), StoreError>;

    /// 原子替换自定义角色包含的权限。
    ///
    /// # Errors
    ///
    /// 角色或权限不存在、角色是系统角色或数据库事务失败时返回错误。
    async fn replace_role_permissions(
        &self,
        role_id: Uuid,
        permission_ids: &[Uuid],
    ) -> Result<Role, StoreError>;

    /// 返回系统支持的完整权限目录。
    ///
    /// # Errors
    ///
    /// 数据库不可用或权限数据无法读取时返回错误。
    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError>;

    /// 原子替换用户角色，并确保保留默认成员角色及至少一个启用管理员。
    ///
    /// # Errors
    ///
    /// 用户或角色不存在、操作会移除最后一个管理员、系统角色缺失或事务失败时返回错误。
    async fn replace_user_roles(
        &self,
        user_id: Uuid,
        role_ids: &[Uuid],
        granted_by: Uuid,
    ) -> Result<AccessProfile, StoreError>;
}
