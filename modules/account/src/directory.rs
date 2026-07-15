//! ZITADEL 用户目录访问适配器。
//!
//! 本模块只负责通过 ZITADEL User v2 API 读取可选的人类用户，不持有数据库连接，也不执行
//! 超级管理员绑定。绑定规则仍由 `accounts` 服务与 store 负责。

use std::time::Duration;

use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Host;

use crate::ExternalIdentity;

const PAGE_SIZE: u32 = 100;
const MAX_DIRECTORY_USERS: u64 = 10_000;
const DIRECTORY_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// 可用于首次启动选择的 ZITADEL 人类用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryUser {
    /// ZITADEL 用户的稳定 `userId`，同时作为 OIDC subject。
    pub subject: String,
    /// ZITADEL 用户名。
    pub username: String,
    /// 适合在交互式选择器中展示的名称。
    pub display_name: String,
    /// 主邮箱；目录没有返回邮箱时为 `None`。
    pub email: Option<String>,
    /// 头像 URL；目录没有返回头像时为 `None`。
    pub avatar_url: Option<String>,
}

impl DirectoryUser {
    /// 把目录用户转换为账号领域可绑定的外部身份。
    pub fn into_external_identity(self, issuer: impl Into<String>) -> ExternalIdentity {
        ExternalIdentity {
            issuer: issuer.into(),
            subject: self.subject,
            email: self.email,
            display_name: self.display_name,
            avatar_url: self.avatar_url,
        }
    }
}

/// 通过 Personal Access Token 调用 ZITADEL User v2 API 的目录客户端。
#[derive(Debug, Clone)]
pub struct ZitadelUserDirectory {
    client: Client,
    users_url: Url,
}

impl ZitadelUserDirectory {
    /// 使用 OIDC issuer 所在域名和 ZITADEL Personal Access Token 创建目录客户端。
    ///
    /// Token 只会写入标记为敏感的默认请求头，不会保存在可调试输出的字段中。
    ///
    /// # Errors
    ///
    /// issuer 不是安全的绝对 URL、PAT 为空、PAT 不能编码为 HTTP 请求头，或 HTTP client
    /// 无法创建时返回错误。生产 issuer 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP。
    pub fn new(issuer: &str, personal_access_token: &str) -> Result<Self, DirectoryError> {
        let mut base_url = Url::parse(issuer.trim())
            .map_err(|_| DirectoryError::InvalidConfiguration("OIDC issuer URL 无效"))?;
        if base_url.host().is_none() {
            return Err(DirectoryError::InvalidConfiguration(
                "OIDC issuer 必须是包含主机的绝对 URL",
            ));
        }
        if !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(DirectoryError::InvalidConfiguration(
                "OIDC issuer 不能包含凭据、query 或 fragment",
            ));
        }
        if base_url.scheme() != "https" && !(base_url.scheme() == "http" && is_loopback(&base_url))
        {
            return Err(DirectoryError::InvalidConfiguration(
                "OIDC issuer 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP",
            ));
        }
        base_url.set_path("/");
        let users_url = base_url
            .join("v2/users")
            .map_err(|_| DirectoryError::InvalidConfiguration("无法构造 ZITADEL API URL"))?;

        let token = personal_access_token.trim();
        if token.is_empty() {
            return Err(DirectoryError::InvalidConfiguration(
                "ZITADEL Personal Access Token 不能为空",
            ));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| DirectoryError::InvalidConfiguration("ZITADEL PAT 包含非法字符"))?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(Policy::none())
            .timeout(DIRECTORY_REQUEST_TIMEOUT)
            .build()?;

        Ok(Self { client, users_url })
    }

    /// 分页读取当前调用方可见的启用状态人类用户。
    ///
    /// 机器用户与非启用用户不会出现在返回值中。结果会按展示名、用户名和 subject 排序，
    /// 从而让交互式选择器保持稳定。
    ///
    /// # Errors
    ///
    /// ZITADEL 请求失败、响应结构无效、分页总数无效，或目录用户数超过安全上限时返回错误。
    pub async fn list_active_human_users(&self) -> Result<Vec<DirectoryUser>, DirectoryError> {
        self.list_users(active_human_queries()).await
    }

    /// 按稳定 `userId` 查找一个启用状态人类用户。
    ///
    /// 该方法供非交互首次部署使用，避免为了匹配一个显式 subject 而读取完整目录。
    ///
    /// # Errors
    ///
    /// subject 为空、ZITADEL 请求失败、响应结构无效或返回规模异常时返回错误。
    pub async fn active_human_user(
        &self,
        subject: &str,
    ) -> Result<Option<DirectoryUser>, DirectoryError> {
        let subject = subject.trim();
        if subject.is_empty() {
            return Err(DirectoryError::InvalidConfiguration(
                "超级管理员 subject 不能为空",
            ));
        }
        let mut queries = active_human_queries();
        queries.push(SearchQuery {
            state_query: None,
            type_query: None,
            in_user_ids_query: Some(InUserIdsQuery {
                user_ids: vec![subject.to_owned()],
            }),
        });
        Ok(self
            .list_users(queries)
            .await?
            .into_iter()
            .find(|user| user.subject == subject))
    }

    async fn list_users(
        &self,
        queries: Vec<SearchQuery>,
    ) -> Result<Vec<DirectoryUser>, DirectoryError> {
        let mut offset = 0_u64;
        let mut users = Vec::new();

        loop {
            if offset >= MAX_DIRECTORY_USERS {
                return Err(DirectoryError::UserLimitExceeded(MAX_DIRECTORY_USERS));
            }
            let response = self
                .client
                .post(self.users_url.clone())
                .json(&ListUsersRequest {
                    query: ListQuery {
                        offset: offset.to_string(),
                        limit: PAGE_SIZE,
                        ascending: true,
                    },
                    sorting_column: "USER_FIELD_NAME_DISPLAY_NAME",
                    queries: &queries,
                })
                .send()
                .await?
                .error_for_status()?
                .json::<ListUsersResponse>()
                .await?;

            let result_count = response.result.len() as u64;
            users.extend(response.result.into_iter().filter_map(directory_user));
            offset = offset.saturating_add(result_count);

            let total = response.details.total_result.as_u64()?;
            if result_count == 0 || offset >= total {
                break;
            }
        }

        users.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.username.cmp(&right.username))
                .then_with(|| left.subject.cmp(&right.subject))
        });
        Ok(users)
    }
}

/// ZITADEL 目录读取错误。
#[derive(Debug, Error)]
pub enum DirectoryError {
    /// 本地 ZITADEL 目录配置无效。
    #[error("ZITADEL 目录配置无效: {0}")]
    InvalidConfiguration(
        /// 不包含密钥的配置错误说明。
        &'static str,
    ),
    /// HTTP client 创建、请求、状态检查或响应解析失败。
    #[error("ZITADEL 用户目录请求失败")]
    Request(
        /// Reqwest 返回的底层错误；不会包含敏感 PAT 请求头。
        #[from]
        reqwest::Error,
    ),
    /// API 返回的 protobuf JSON 整数字段无效。
    #[error("ZITADEL 用户目录响应中的 {0} 无效")]
    InvalidInteger(
        /// 无法解析的字段名称。
        &'static str,
    ),
    /// 目录规模超过首次启动选择器的安全上限。
    #[error("ZITADEL 可见用户数超过首次启动上限 {0}")]
    UserLimitExceeded(
        /// 客户端允许读取的最大目录用户数。
        u64,
    ),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListUsersRequest<'a> {
    query: ListQuery,
    sorting_column: &'static str,
    queries: &'a [SearchQuery],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    offset: String,
    limit: u32,
    #[serde(rename = "asc")]
    ascending: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    state_query: Option<StateQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    type_query: Option<TypeQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_user_ids_query: Option<InUserIdsQuery>,
}

#[derive(Debug, Serialize)]
struct StateQuery {
    state: &'static str,
}

#[derive(Debug, Serialize)]
struct TypeQuery {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InUserIdsQuery {
    user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListUsersResponse {
    details: ListDetails,
    #[serde(default)]
    result: Vec<ZitadelUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListDetails {
    total_result: ProtobufInteger,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProtobufInteger {
    Number(u64),
    String(String),
}

impl ProtobufInteger {
    fn as_u64(&self) -> Result<u64, DirectoryError> {
        match self {
            Self::Number(value) => Ok(*value),
            Self::String(value) => value
                .parse()
                .map_err(|_| DirectoryError::InvalidInteger("totalResult")),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ZitadelUser {
    user_id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    preferred_login_name: String,
    human: Option<HumanUser>,
}

#[derive(Debug, Deserialize)]
struct HumanUser {
    profile: Option<HumanProfile>,
    email: Option<HumanEmail>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HumanProfile {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    avatar_url: String,
}

#[derive(Debug, Deserialize)]
struct HumanEmail {
    #[serde(default)]
    email: String,
}

fn directory_user(user: ZitadelUser) -> Option<DirectoryUser> {
    if user.state != "USER_STATE_ACTIVE" || user.user_id.trim().is_empty() {
        return None;
    }
    let human = user.human?;
    let profile = human.profile;
    let display_name = profile
        .as_ref()
        .map(|profile| profile.display_name.trim())
        .filter(|value| !value.is_empty())
        .or_else(|| non_empty(user.preferred_login_name.as_str()))
        .or_else(|| non_empty(user.username.as_str()))
        .unwrap_or(user.user_id.as_str())
        .to_owned();
    let avatar_url =
        profile.and_then(|profile| non_empty(profile.avatar_url.as_str()).map(str::to_owned));
    let email = human
        .email
        .and_then(|email| non_empty(email.email.as_str()).map(str::to_owned));

    Some(DirectoryUser {
        subject: user.user_id,
        username: user.username,
        display_name,
        email,
        avatar_url,
    })
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn active_human_queries() -> Vec<SearchQuery> {
    vec![
        SearchQuery {
            state_query: Some(StateQuery {
                state: "USER_STATE_ACTIVE",
            }),
            type_query: None,
            in_user_ids_query: None,
        },
        SearchQuery {
            state_query: None,
            type_query: Some(TypeQuery { kind: "TYPE_HUMAN" }),
            in_user_ids_query: None,
        },
    ]
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}
