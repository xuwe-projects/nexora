//! 数据库迁移前的 fail-closed 状态检查。

use std::io;

pub(crate) struct DatabaseState {
    pub(crate) applied_migrations: Vec<(i64, bool)>,
    pub(crate) account_schema_exists: bool,
    pub(crate) users_exists: bool,
    pub(crate) roles_exists: bool,
    pub(crate) permissions_exists: bool,
    pub(crate) role_permissions_exists: bool,
    pub(crate) user_roles_exists: bool,
    pub(crate) system_initialization_exists: bool,
}

pub(crate) fn validate_migration_safety(
    state: &DatabaseState,
    initialize_empty_database: bool,
) -> Result<(), io::Error> {
    if state.applied_migrations.iter().any(|(_, success)| !success) {
        return Err(io::Error::other(
            "检测到失败的迁移记录，拒绝继续；请先完成数据库事故排查",
        ));
    }

    let Some(latest_version) = state
        .applied_migrations
        .iter()
        .map(|(version, _)| *version)
        .max()
    else {
        if state.account_schema_exists {
            return Err(io::Error::other(
                "检测到 account schema 但没有迁移历史，拒绝接管或重建现有数据库",
            ));
        }
        if !initialize_empty_database {
            return Err(io::Error::other(
                "目标数据库没有迁移历史；首次安装必须显式传入 --initialize-empty-database",
            ));
        }
        return Ok(());
    };

    let base_schema_complete = state.users_exists
        && state.roles_exists
        && state.permissions_exists
        && state.role_permissions_exists
        && state.user_roles_exists;
    if latest_version >= 1 && !base_schema_complete {
        return Err(io::Error::other(
            "迁移历史存在但 account 核心表缺失，拒绝把疑似损坏的生产库当作正常升级目标",
        ));
    }
    if latest_version >= 3 && !state.system_initialization_exists {
        return Err(io::Error::other(
            "迁移历史已包含版本 3，但系统初始化表缺失，拒绝继续迁移",
        ));
    }
    Ok(())
}
