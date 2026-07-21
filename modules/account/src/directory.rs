//! ZITADEL gRPC 用户目录与项目角色适配器。
//!
//! 本模块使用 gRPC 官方 Rust `grpc` 库与 ZITADEL UserService v2 和 ProjectService v2
//! 交互，读取 setup 所需的人类用户并确保本地系统角色存在于目标 Project。超级管理员绑定
//! 规则仍由账号实体校验与初始化 store 负责。

use std::fmt;

use async_trait::async_trait;
use grpc::{StatusCodeError, StatusError, client::Channel};
use grpc_protobuf::CallBuilder as _;
use protobuf::{ProtoString, View};
use thiserror::Error;

use crate::{
    CreateHumanIdentity, ExternalIdentity, IdentityDirectory, IdentityDirectoryError, SystemRole,
    generated::zitadel::{
        project::v2::{AddProjectRoleRequest, project_service_client::ProjectServiceClient},
        user::v2::{
            DeleteUserRequest, HumanUserView, InUserIDQuery, ListQuery, ListUserMetadataRequest,
            ListUsersRequest, Metadata, SearchQuery, SetUserMetadataRequest, StateQuery, Type,
            TypeQuery, UserFieldName, UserState, UserView, user_service_client::UserServiceClient,
        },
    },
    zitadel::{self, REQUEST_TIMEOUT},
    zitadel_user,
};

const PAGE_SIZE: u32 = 100;
const MAX_DIRECTORY_USERS: u64 = 10_000;
const AVATAR_METADATA_KEY: &str = "urn:nexora:account:avatar_url";

/// 可用于首次初始化选择的人类用户。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryUser {
    /// 当前目录 issuer 范围内稳定唯一的用户 ID（subject）。
    pub identity_id: String,
    /// 认证授权服务中的用户名。
    pub username: String,
    /// 适合在 setup 向导中展示的名称。
    pub display_name: String,
    /// 主邮箱；目录没有返回邮箱时为 `None`。
    pub email: Option<String>,
    /// 头像 URL；目录没有返回头像时为 `None`。
    pub avatar_url: Option<String>,
}

impl DirectoryUser {
    /// 把目录用户转换为账号领域可绑定的外部身份。
    pub fn into_external_identity(self) -> ExternalIdentity {
        ExternalIdentity {
            identity_id: self.identity_id,
            username: Some(self.username),
            email: self.email,
            display_name: self.display_name,
            avatar_url: self.avatar_url,
        }
    }
}

/// 通过 Personal Access Token 调用 ZITADEL UserService 与 ProjectService v2 gRPC API 的客户端。
#[derive(Clone)]
pub struct ZitadelUserDirectory {
    user_client: UserServiceClient<Channel>,
    project_client: ProjectServiceClient<Channel>,
    organization_id: String,
    project_id: String,
}

impl fmt::Debug for ZitadelUserDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZitadelUserDirectory")
            .field("organization_id", &self.organization_id)
            .field("project_id", &self.project_id)
            .finish_non_exhaustive()
    }
}

impl ZitadelUserDirectory {
    /// 使用 OIDC issuer、服务账户 Personal Access Token 和目标 Project ID 创建 gRPC 客户端。
    ///
    /// 生产 issuer 必须使用经过系统证书库验证的 TLS；仅 loopback 开发地址允许使用
    /// 本地信道凭据连接明文 HTTP/2。PAT 通过敏感 `authorization` metadata 发送。
    ///
    /// # Errors
    ///
    /// issuer 不是安全的绝对 URL、PAT 或 Project ID 为空、PAT 包含非法 metadata 字符、
    /// TLS 配置无法创建时返回错误。
    pub fn new(
        issuer: &str,
        personal_access_token: &str,
        organization_id: &str,
        project_id: &str,
    ) -> Result<Self, DirectoryError> {
        let organization_id = organization_id.trim();
        if organization_id.is_empty() {
            return Err(DirectoryError::InvalidConfiguration(
                "Organization ID 不能为空",
            ));
        }
        let project_id = project_id.trim();
        if project_id.is_empty() {
            return Err(DirectoryError::InvalidConfiguration("Project ID 不能为空"));
        }
        let channel = zitadel::authenticated_channel(issuer, personal_access_token)?;
        Ok(Self {
            user_client: UserServiceClient::new(channel.clone()),
            project_client: ProjectServiceClient::new(channel),
            organization_id: organization_id.to_owned(),
            project_id: project_id.to_owned(),
        })
    }

    /// 确保全部本地系统角色都存在于配置的认证授权 Project。
    ///
    /// 已存在的角色键按幂等成功处理，便于部分成功后安全重试；其他 gRPC 状态会立即终止，
    /// 调用方此时不得把本地系统标记为初始化完成。
    ///
    /// # Errors
    ///
    /// ProjectService v2 拒绝创建角色或暂时不可用时返回包含 Project、角色键与 gRPC 状态的
    /// [`DirectoryError`]。
    pub async fn ensure_project_roles(&self, roles: &[SystemRole]) -> Result<(), DirectoryError> {
        for role in roles {
            let mut request = AddProjectRoleRequest::new();
            request.set_project_id(self.project_id.as_str());
            request.set_role_key(role.key.as_str());
            request.set_display_name(role.name.as_str());
            match self
                .project_client
                .add_project_role(request.as_view())
                .with_timeout(REQUEST_TIMEOUT)
                .await
            {
                Ok(_) => tracing::info!(
                    business_operation = "zitadel_project_role_sync",
                    stage = "add_project_role",
                    project_id = %self.project_id,
                    role_key = %role.key,
                    role_name = %role.name,
                    outcome = "created",
                    "认证授权 Project 角色创建成功"
                ),
                Err(error) if error.code() == StatusCodeError::AlreadyExists => tracing::info!(
                    business_operation = "zitadel_project_role_sync",
                    stage = "add_project_role",
                    project_id = %self.project_id,
                    role_key = %role.key,
                    role_name = %role.name,
                    outcome = "already_exists",
                    "认证授权 Project 角色已存在"
                ),
                Err(error) => {
                    return Err(DirectoryError::ProjectRoleRequest {
                        project_id: self.project_id.clone(),
                        role_key: role.key.clone(),
                        code: error.code(),
                        message: error.message().to_owned(),
                    });
                }
            }
        }
        Ok(())
    }

    /// 分页读取当前 PAT 可见的启用状态人类用户。
    ///
    /// 服务账户与非启用用户不会出现在返回值中。结果按展示名、用户名和 identity ID
    /// 稳定排序。
    ///
    /// # Errors
    ///
    /// gRPC 请求失败、响应字符串无效或目录用户数超过安全上限时返回错误。
    pub async fn list_active_human_users(&self) -> Result<Vec<DirectoryUser>, DirectoryError> {
        self.list_users(None).await
    }

    /// 按稳定 identity ID 查找一个启用状态人类用户。
    ///
    /// 该方法供 setup 提交时二次确认所选用户，避免仅信任页面中的字段。
    ///
    /// # Errors
    ///
    /// identity ID 为空、gRPC 请求失败或响应字符串无效时返回错误。
    pub async fn active_human_user(
        &self,
        identity_id: &str,
    ) -> Result<Option<DirectoryUser>, DirectoryError> {
        let identity_id = identity_id.trim();
        if identity_id.is_empty() {
            return Err(DirectoryError::InvalidConfiguration(
                "超级管理员 identity ID 不能为空",
            ));
        }
        Ok(self
            .list_users(Some(identity_id))
            .await?
            .into_iter()
            .find(|user| user.identity_id == identity_id))
    }

    /// 在配置的 ZITADEL Organization 中创建带初始密码的人类用户。
    ///
    /// 返回值中的 identity ID 完全来自 ZITADEL `CreateUser` 响应，调用方无需也不能提交
    /// 裸 subject。邮箱会按已验证写入；需要同步手机号时使用
    /// [`Self::create_human_user_with_contact`]。
    ///
    /// # Errors
    ///
    /// UserService v2 拒绝请求、用户名、邮箱或密码无效/冲突、响应缺少用户 ID 时返回错误。
    pub async fn create_human_user(
        &self,
        request: &CreateHumanIdentity,
    ) -> Result<DirectoryUser, DirectoryError> {
        self.create_human_user_with_contact(request, None).await
    }

    /// 在配置的 ZITADEL Organization 中创建 human user 并可写入已验证联系手机号。
    ///
    /// `contact_phone` 非空时会写入 ZITADEL human phone/mobile 联系信息，并与邮箱一样标记为
    /// 已验证，不发送验证码。为空时保持旧行为，只创建用户名、邮箱、资料和密码。
    ///
    /// # Errors
    ///
    /// UserService v2 拒绝请求、用户名、邮箱、手机号或密码无效/冲突、响应缺少用户 ID 时返回错误。
    pub async fn create_human_user_with_contact(
        &self,
        request: &CreateHumanIdentity,
        contact_phone: Option<&str>,
    ) -> Result<DirectoryUser, DirectoryError> {
        let create = zitadel_user::create_human_user_request(
            self.organization_id.as_str(),
            request,
            contact_phone,
        );
        let response = self
            .user_client
            .create_user(create.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await?;
        let identity_id = required_string(response.id(), "create_user.id")?;
        if identity_id.trim().is_empty() {
            return Err(DirectoryError::InvalidString("create_user.id"));
        }
        if let Some(avatar_url) = request.avatar_url.as_deref()
            && let Err(error) = self
                .update_user_avatar_metadata(identity_id.as_str(), Some(avatar_url))
                .await
        {
            if let Err(delete_error) = self.delete_user(identity_id.as_str()).await {
                tracing::error!(
                    error = ?delete_error,
                    business_operation = "zitadel_avatar_metadata_compensation",
                    "failed to delete ZITADEL user after avatar metadata write failure"
                );
            }
            return Err(error);
        }
        Ok(DirectoryUser {
            identity_id,
            username: request.username.clone(),
            display_name: request
                .display_name
                .clone()
                .unwrap_or_else(|| format!("{} {}", request.given_name, request.family_name)),
            email: Some(request.email.clone()),
            avatar_url: request.avatar_url.clone(),
        })
    }

    /// 删除指定 ZITADEL 用户，用于本地账号事务失败后的补偿。
    ///
    /// # Errors
    ///
    /// identity ID 为空或 UserService v2 拒绝删除时返回错误。
    pub async fn delete_user(&self, identity_id: &str) -> Result<(), DirectoryError> {
        let identity_id = identity_id.trim();
        if identity_id.is_empty() {
            return Err(DirectoryError::InvalidConfiguration(
                "删除用户 identity ID 不能为空",
            ));
        }
        let mut request = DeleteUserRequest::new();
        request.set_user_id(identity_id);
        self.user_client
            .delete_user(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await?;
        Ok(())
    }

    /// 在 ZITADEL user metadata 中写入或清空 Nexora 头像 URL。
    ///
    /// # Errors
    ///
    /// identity ID 为空、ZITADEL 拒绝 metadata 写入或 UserService v2 暂时不可用时返回错误。
    pub async fn update_user_avatar_metadata(
        &self,
        identity_id: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), DirectoryError> {
        let identity_id = identity_id.trim();
        if identity_id.is_empty() {
            return Err(DirectoryError::InvalidConfiguration(
                "avatar metadata user id must not be empty",
            ));
        }
        let mut metadata = Metadata::new();
        metadata.set_key(AVATAR_METADATA_KEY);
        metadata.set_value(avatar_url.unwrap_or_default().as_bytes().to_vec());

        let mut request = SetUserMetadataRequest::new();
        request.set_user_id(identity_id);
        request.metadata_mut().push(metadata);
        self.user_client
            .set_user_metadata(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await?;
        Ok(())
    }

    async fn avatar_metadata(&self, identity_id: &str) -> Result<Option<String>, DirectoryError> {
        let mut request = ListUserMetadataRequest::new();
        request.set_user_id(identity_id);
        let response = self
            .user_client
            .list_user_metadata(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await?;
        response
            .metadata()
            .iter()
            .find_map(|metadata| {
                let key = required_string(metadata.key(), "metadata.key").ok()?;
                (key == AVATAR_METADATA_KEY).then(|| metadata.value().to_vec())
            })
            .map(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|_| DirectoryError::InvalidString("metadata.value"))
            })
            .transpose()
            .map(|value| value.and_then(non_empty_owned))
    }

    async fn list_users(
        &self,
        identity_id: Option<&str>,
    ) -> Result<Vec<DirectoryUser>, DirectoryError> {
        let mut offset = 0_u64;
        let mut users = Vec::new();

        loop {
            if offset >= MAX_DIRECTORY_USERS {
                return Err(DirectoryError::UserLimitExceeded(MAX_DIRECTORY_USERS));
            }
            let request = list_users_request(offset, identity_id);
            let response = self
                .user_client
                .list_users(request.as_view())
                .with_timeout(REQUEST_TIMEOUT)
                .await?;
            let result_count = response.result().len() as u64;
            for user in response.result() {
                if let Some(user) = directory_user(user)? {
                    users.push(user);
                }
            }
            offset = offset.saturating_add(result_count);
            let total = response.details().total_result();
            if result_count == 0 || offset >= total {
                break;
            }
        }

        users.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.username.cmp(&right.username))
                .then_with(|| left.identity_id.cmp(&right.identity_id))
        });
        Ok(users)
    }
}

#[async_trait]
impl IdentityDirectory for ZitadelUserDirectory {
    async fn identity(
        &self,
        identity_id: &str,
    ) -> Result<Option<ExternalIdentity>, IdentityDirectoryError> {
        let Some(user) = self
            .active_human_user(identity_id)
            .await
            .map_err(identity_directory_error)?
        else {
            return Ok(None);
        };
        let mut identity = user.into_external_identity();
        if identity.avatar_url.is_none() {
            identity.avatar_url = self
                .avatar_metadata(identity.identity_id.as_str())
                .await
                .map_err(identity_directory_error)?;
        }
        Ok(Some(identity))
    }

    async fn create_human_identity(
        &self,
        request: &CreateHumanIdentity,
    ) -> Result<ExternalIdentity, IdentityDirectoryError> {
        self.create_human_user_with_contact(request, None)
            .await
            .map(DirectoryUser::into_external_identity)
            .map_err(identity_directory_error)
    }

    async fn create_human_identity_with_contact(
        &self,
        request: &CreateHumanIdentity,
        contact_phone: Option<&str>,
    ) -> Result<ExternalIdentity, IdentityDirectoryError> {
        self.create_human_user_with_contact(request, contact_phone)
            .await
            .map(DirectoryUser::into_external_identity)
            .map_err(identity_directory_error)
    }

    async fn delete_identity(&self, identity_id: &str) -> Result<(), IdentityDirectoryError> {
        self.delete_user(identity_id)
            .await
            .map_err(identity_directory_error)
    }

    async fn update_identity_avatar(
        &self,
        identity_id: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), IdentityDirectoryError> {
        self.update_user_avatar_metadata(identity_id, avatar_url)
            .await
            .map_err(identity_directory_error)
    }
}

fn identity_directory_error(error: DirectoryError) -> IdentityDirectoryError {
    match &error {
        DirectoryError::Request { code, .. } if *code == StatusCodeError::AlreadyExists => {
            IdentityDirectoryError::Conflict
        }
        DirectoryError::Request { code, .. } if *code == StatusCodeError::NotFound => {
            IdentityDirectoryError::NotFound
        }
        DirectoryError::Request { code, .. } if *code == StatusCodeError::Unimplemented => {
            IdentityDirectoryError::AvatarUnsupported
        }
        _ => {
            tracing::warn!(error = ?error, "ZITADEL 身份目录请求失败");
            IdentityDirectoryError::Unavailable
        }
    }
}

/// ZITADEL gRPC 目录读取错误。
#[derive(Debug, Error)]
pub enum DirectoryError {
    /// 本地目录配置无效。
    #[error("ZITADEL gRPC 目录配置无效: {0}")]
    InvalidConfiguration(
        /// 不包含密钥的配置错误说明。
        &'static str,
    ),
    /// gRPC TLS 凭据无法使用系统证书库创建。
    #[error("ZITADEL gRPC TLS 配置无效: {0}")]
    TlsConfiguration(
        /// gRPC 官方库返回的底层错误，不包含 PAT。
        String,
    ),
    /// UserService v2 gRPC 请求失败。
    #[error("ZITADEL UserService v2 gRPC 请求失败（code={code:?}, message={message}）")]
    Request {
        /// gRPC 返回的标准状态码。
        code: StatusCodeError,
        /// gRPC 返回的状态消息；该值不包含标记为 sensitive 的 PAT metadata。
        message: String,
    },
    /// ProjectService v2 创建系统角色失败。
    #[error(
        "ZITADEL ProjectService v2 AddProjectRole gRPC 请求失败（project_id={project_id}, role_key={role_key}, code={code:?}, message={message}）"
    )]
    ProjectRoleRequest {
        /// 本次创建目标所属的 Project ID。
        project_id: String,
        /// 本次创建失败的稳定角色键。
        role_key: String,
        /// gRPC 返回的标准状态码。
        code: StatusCodeError,
        /// gRPC 返回的状态消息；该值不包含标记为 sensitive 的 PAT metadata。
        message: String,
    },
    /// Protobuf 响应中的字符串不是有效 UTF-8。
    #[error("ZITADEL gRPC 目录响应中的 {0} 不是有效 UTF-8")]
    InvalidString(
        /// 无效字符串对应的稳定字段名。
        &'static str,
    ),
    /// 目录规模超过 setup 安全上限。
    #[error("ZITADEL 可见用户数超过 setup 上限 {0}")]
    UserLimitExceeded(
        /// 客户端允许读取的最大目录用户数。
        u64,
    ),
}

impl From<StatusError> for DirectoryError {
    fn from(error: StatusError) -> Self {
        Self::Request {
            code: error.code(),
            message: error.message().to_owned(),
        }
    }
}

impl From<zitadel::ClientError> for DirectoryError {
    fn from(error: zitadel::ClientError) -> Self {
        match error {
            zitadel::ClientError::InvalidConfiguration(message) => {
                Self::InvalidConfiguration(message)
            }
            zitadel::ClientError::TlsConfiguration(message) => Self::TlsConfiguration(message),
        }
    }
}

fn list_users_request(offset: u64, identity_id: Option<&str>) -> ListUsersRequest {
    let mut request = ListUsersRequest::new();
    let mut list_query = ListQuery::new();
    list_query.set_offset(offset);
    list_query.set_limit(PAGE_SIZE);
    list_query.set_asc(true);
    request.set_query(list_query);
    request.set_sorting_column(UserFieldName::DisplayName);

    let mut state = StateQuery::new();
    state.set_state(UserState::Active);
    let mut state_query = SearchQuery::new();
    state_query.set_state_query(state);
    request.queries_mut().push(state_query);

    let mut user_type = TypeQuery::new();
    user_type.set_type(Type::Human);
    let mut type_query = SearchQuery::new();
    type_query.set_type_query(user_type);
    request.queries_mut().push(type_query);

    if let Some(identity_id) = identity_id {
        let mut ids = InUserIDQuery::new();
        ids.user_ids_mut().push(identity_id);
        let mut id_query = SearchQuery::new();
        id_query.set_in_user_ids_query(ids);
        request.queries_mut().push(id_query);
    }
    request
}

fn directory_user(user: UserView<'_>) -> Result<Option<DirectoryUser>, DirectoryError> {
    if user.state() != UserState::Active {
        return Ok(None);
    }
    let Some(human) = user.human_opt().into_option() else {
        return Ok(None);
    };
    let identity_id = required_string(user.user_id(), "user_id")?;
    if identity_id.trim().is_empty() {
        return Ok(None);
    }
    let username = required_string(user.username(), "username")?;
    let preferred_login_name =
        required_string(user.preferred_login_name(), "preferred_login_name")?;
    let display_name = human_display_name(human, &preferred_login_name, &username, &identity_id)?;
    let profile = human.profile_opt().into_option();
    let avatar_url = profile
        .map(|profile| required_string(profile.avatar_url(), "avatar_url"))
        .transpose()?
        .and_then(non_empty_owned);
    let email = human
        .email_opt()
        .into_option()
        .map(|email| required_string(email.email(), "email"))
        .transpose()?
        .and_then(non_empty_owned);

    Ok(Some(DirectoryUser {
        identity_id,
        username,
        display_name,
        email,
        avatar_url,
    }))
}

fn human_display_name(
    human: HumanUserView<'_>,
    preferred_login_name: &str,
    username: &str,
    identity_id: &str,
) -> Result<String, DirectoryError> {
    let profile_name = human
        .profile_opt()
        .into_option()
        .map(|profile| required_string(profile.display_name(), "display_name"))
        .transpose()?
        .and_then(non_empty_owned);
    Ok(profile_name
        .or_else(|| non_empty_owned(preferred_login_name.to_owned()))
        .or_else(|| non_empty_owned(username.to_owned()))
        .unwrap_or_else(|| identity_id.to_owned()))
}

fn required_string(
    value: View<'_, ProtoString>,
    field: &'static str,
) -> Result<String, DirectoryError> {
    value
        .to_str()
        .map(str::to_owned)
        .map_err(|_| DirectoryError::InvalidString(field))
}

fn non_empty_owned(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}
