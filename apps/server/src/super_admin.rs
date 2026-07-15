//! 首次启动超级管理员引导。
//!
//! 本模块只负责启动时目录读取与终端交互；唯一性、不可替换性和授权规则由账号 application、
//! store 与 PostgreSQL 约束共同保证。

use std::io::{self, IsTerminal, Write};

use account::{
    Account,
    directory::{DirectoryError, DirectoryUser, ZitadelUserDirectory},
};
use thiserror::Error;

use crate::config::ServerConfig;

const MAX_DISPLAYED_USERS: usize = 100;

/// 确保数据库已经绑定唯一内置超级管理员。
///
/// 已绑定时立即返回且不会访问 ZITADEL。首次启动时会优先使用配置的 subject；没有配置且
/// 标准输入为终端时，显示交互式选择器。非交互部署必须显式配置 subject。
///
/// # Errors
///
/// 数据库读取或绑定失败、首次启动配置不完整、ZITADEL 目录不可用、找不到候选用户，或用户
/// 取消交互确认时返回错误。
pub async fn ensure_super_admin(
    account: &Account,
    config: &ServerConfig,
) -> Result<(), SuperAdminSetupError> {
    if let Some(user) = account.super_admin().await? {
        tracing::info!(
            subject = %user.subject,
            display_name = %user.display_name,
            "内置超级管理员已绑定"
        );
        return Ok(());
    }

    let directory =
        ZitadelUserDirectory::new(&config.oidc.issuer_url, config.oidc.personal_access_token())?;
    let configured_subject = config
        .oidc
        .super_admin_subject
        .as_deref()
        .and_then(non_empty);
    let selected = if let Some(subject) = configured_subject {
        directory
            .active_human_user(subject)
            .await?
            .ok_or_else(|| SuperAdminSetupError::ConfiguredSubjectNotFound(subject.to_owned()))?
    } else {
        if !io::stdin().is_terminal() {
            return Err(SuperAdminSetupError::NonInteractiveSelectionRequired);
        }
        let users = directory.list_active_human_users().await?;
        if users.is_empty() {
            return Err(SuperAdminSetupError::NoEligibleUsers);
        }
        tokio::task::spawn_blocking(move || select_interactively(users)).await??
    };

    let identity = selected.into_external_identity(config.oidc.issuer_url.clone());
    let user = account.bind_super_admin(&identity).await?;
    tracing::info!(
        subject = %user.subject,
        display_name = %user.display_name,
        "内置超级管理员绑定完成"
    );
    Ok(())
}

/// 首次启动超级管理员引导错误。
#[derive(Debug, Error)]
pub enum SuperAdminSetupError {
    /// 账号 application 无法读取或绑定超级管理员。
    #[error("超级管理员持久化失败")]
    Account(
        /// 账号领域返回的底层错误。
        #[from]
        account::AccountError,
    ),
    /// ZITADEL 用户目录配置或请求失败。
    #[error(transparent)]
    Directory(
        /// 用户目录客户端返回的底层错误。
        #[from]
        DirectoryError,
    ),
    /// 阻塞式终端选择任务异常结束。
    #[error("超级管理员交互式选择任务异常结束")]
    Join(
        /// Tokio 返回的阻塞任务错误。
        #[from]
        tokio::task::JoinError,
    ),
    /// 可见目录中没有启用的人类用户。
    #[error("ZITADEL 用户目录中没有可绑定的启用状态人类用户")]
    NoEligibleUsers,
    /// 显式配置的 subject 不存在或不可用。
    #[error("配置的超级管理员 subject 不在 ZITADEL 可选用户中: {0}")]
    ConfiguredSubjectNotFound(
        /// 无法在启用状态人类用户中找到的配置值。
        String,
    ),
    /// 当前启动环境无法安全执行交互式选择。
    #[error(
        "首次非交互启动必须配置 oidc.super_admin_subject；也可使用环境变量 OIDC__SUPER_ADMIN_SUBJECT"
    )]
    NonInteractiveSelectionRequired,
    /// 终端输入输出失败。
    #[error("无法读取超级管理员选择")]
    TerminalIo(
        /// 标准输入输出返回的底层错误。
        #[from]
        io::Error,
    ),
    /// 用户取消了不可逆绑定确认。
    #[error("已取消超级管理员绑定")]
    Cancelled,
}

fn select_interactively(users: Vec<DirectoryUser>) -> Result<DirectoryUser, SuperAdminSetupError> {
    println!("\n首次启动：请选择唯一内置超级管理员。");
    println!("该身份绑定后不能替换、停用、删除或修改角色，并自动拥有全部权限。\n");
    for (index, user) in users.iter().take(MAX_DISPLAYED_USERS).enumerate() {
        println!(
            "{:>3}. {} | {} | {} | subject={}",
            index + 1,
            user.display_name,
            optional_label(user.email.as_deref()),
            optional_label(non_empty(user.username.as_str())),
            user.subject
        );
    }
    if users.len() > MAX_DISPLAYED_USERS {
        println!(
            "\n目录共有 {} 个候选用户，仅展示前 {} 个；可直接输入完整 subject。",
            users.len(),
            MAX_DISPLAYED_USERS
        );
    }

    let selected = loop {
        let input = read_line("\n请输入序号或完整 subject：")?;
        if let Ok(index) = input.parse::<usize>()
            && (1..=users.len().min(MAX_DISPLAYED_USERS)).contains(&index)
        {
            break users[index - 1].clone();
        }
        if let Some(user) = users.iter().find(|user| user.subject == input) {
            break user.clone();
        }
        println!("输入无效，请重新选择。");
    };

    println!(
        "\n将绑定：{} <{}>，subject={}。",
        selected.display_name,
        optional_label(selected.email.as_deref()),
        selected.subject
    );
    let confirmation = read_line("这是不可替换的安全身份，输入 BIND 确认：")?;
    if confirmation != "BIND" {
        return Err(SuperAdminSetupError::Cancelled);
    }
    Ok(selected)
}

fn read_line(prompt: &str) -> Result<String, io::Error> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_owned())
}

fn optional_label(value: Option<&str>) -> &str {
    value.unwrap_or("-")
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}
