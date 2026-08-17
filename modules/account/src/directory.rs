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
    CreateHumanIdentity, CreateServiceAccountIdentity, ExternalIdentity, IdentityDirectory,
    IdentityDirectoryError, ProjectRoleEnsureOutcome, ProviderUsernameMatch,
    ServiceAccountDirectory, ServiceAccountDirectoryError, ServiceAccountIdentity, SystemRole,
    generated::zitadel::{
        authorization::v2::{
            AuthorizationView, AuthorizationsSearchFilter, CreateAuthorizationRequest,
            DeleteAuthorizationRequest, IDFilter as AuthorizationIDFilter,
            InIDsFilter as AuthorizationInIDsFilter, ListAuthorizationsRequest,
            PaginationRequest as AuthorizationPaginationRequest, UpdateAuthorizationRequest,
            authorization_service_client::AuthorizationServiceClient,
        },
        project::v2::{
            AddProjectRoleRequest, RemoveProjectRoleRequest, UpdateProjectRoleRequest,
            project_service_client::ProjectServiceClient,
        },
        user::v2::{
            AccessTokenType, CreateUserRequest, DeleteUserRequest, HumanUserView, InUserIDQuery,
            ListQuery, ListUsersRequest, SearchQuery, StateQuery, TextQueryMethod, Type, TypeQuery,
            UpdateUserRequest, UserFieldName, UserNameQuery, UserState, UserView,
            create_user_request::Machine as CreateMachine,
            update_user_request::Machine as UpdateMachine, user_service_client::UserServiceClient,
        },
    },
    zitadel::{self, REQUEST_TIMEOUT},
    zitadel_user,
};

const PAGE_SIZE: u32 = 100;
const MAX_DIRECTORY_USERS: u64 = 10_000;
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
}

impl DirectoryUser {
    /// 把目录用户转换为账号领域可绑定的外部身份。
    pub fn into_external_identity(self) -> ExternalIdentity {
        ExternalIdentity {
            identity_id: self.identity_id,
            username: Some(self.username),
            email: self.email,
            display_name: self.display_name,
        }
    }
}

/// 通过 Personal Access Token 调用 ZITADEL UserService 与 ProjectService v2 gRPC API 的客户端。
#[derive(Clone)]
pub struct ZitadelUserDirectory {
    user_client: UserServiceClient<Channel>,
    project_client: ProjectServiceClient<Channel>,
    authorization_client: AuthorizationServiceClient<Channel>,
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
            project_client: ProjectServiceClient::new(channel.clone()),
            authorization_client: AuthorizationServiceClient::new(channel),
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
        Ok(DirectoryUser {
            identity_id,
            username: request.username.clone(),
            display_name: request
                .display_name
                .clone()
                .unwrap_or_else(|| format!("{} {}", request.given_name, request.family_name)),
            email: Some(request.email.clone()),
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

    async fn replace_authorization_roles(
        &self,
        identity_id: &str,
        role_keys: &[String],
    ) -> Result<Vec<String>, DirectoryError> {
        let mut desired = role_keys
            .iter()
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty())
            .collect::<Vec<_>>();
        desired.sort();
        desired.dedup();

        let existing = self.authorization_for_user(identity_id).await?;
        let previous = existing
            .as_ref()
            .map(|(_, roles)| roles.clone())
            .unwrap_or_default();
        match existing {
            Some((authorization_id, current)) => {
                if desired.is_empty() {
                    let mut request = DeleteAuthorizationRequest::new();
                    request.set_id(authorization_id.as_str());
                    self.authorization_client
                        .delete_authorization(request.as_view())
                        .with_timeout(REQUEST_TIMEOUT)
                        .await?;
                } else if current != desired {
                    let mut request = UpdateAuthorizationRequest::new();
                    request.set_id(authorization_id.as_str());
                    request.role_keys_mut().extend(desired);
                    self.authorization_client
                        .update_authorization(request.as_view())
                        .with_timeout(REQUEST_TIMEOUT)
                        .await?;
                }
            }
            None if desired.is_empty() => {}
            None => {
                let mut request = CreateAuthorizationRequest::new();
                request.set_user_id(identity_id);
                request.set_project_id(self.project_id.as_str());
                request.set_organization_id(self.organization_id.as_str());
                request.role_keys_mut().extend(desired.clone());
                match self
                    .authorization_client
                    .create_authorization(request.as_view())
                    .with_timeout(REQUEST_TIMEOUT)
                    .await
                {
                    Ok(_) => {}
                    Err(error) if error.code() == StatusCodeError::AlreadyExists => {
                        let Some((authorization_id, _)) =
                            self.authorization_for_user(identity_id).await?
                        else {
                            return Err(DirectoryError::Request {
                                code: StatusCodeError::Unknown,
                                message: "角色关联已存在但无法重新读取".to_owned(),
                            });
                        };
                        let mut update = UpdateAuthorizationRequest::new();
                        update.set_id(authorization_id.as_str());
                        update.role_keys_mut().extend(desired);
                        self.authorization_client
                            .update_authorization(update.as_view())
                            .with_timeout(REQUEST_TIMEOUT)
                            .await?;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        Ok(previous)
    }

    async fn authorization_for_user(
        &self,
        identity_id: &str,
    ) -> Result<Option<(String, Vec<String>)>, DirectoryError> {
        let mut pagination = AuthorizationPaginationRequest::new();
        pagination.set_limit(2);
        pagination.set_asc(true);
        let mut request = ListAuthorizationsRequest::new();
        request.set_pagination(pagination);

        let mut users = AuthorizationInIDsFilter::new();
        users.ids_mut().push(identity_id);
        let mut user_filter = AuthorizationsSearchFilter::new();
        user_filter.set_in_user_ids(users);
        request.filters_mut().push(user_filter);

        let mut project = AuthorizationIDFilter::new();
        project.set_id(self.project_id.as_str());
        let mut project_filter = AuthorizationsSearchFilter::new();
        project_filter.set_project_id(project);
        request.filters_mut().push(project_filter);

        let mut organization = AuthorizationIDFilter::new();
        organization.set_id(self.organization_id.as_str());
        let mut organization_filter = AuthorizationsSearchFilter::new();
        organization_filter.set_organization_id(organization);
        request.filters_mut().push(organization_filter);

        let response = self
            .authorization_client
            .list_authorizations(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await?;
        let mut matches = response.authorizations().iter();
        let first = matches.next().map(authorization_snapshot).transpose()?;
        if matches.next().is_some() {
            return Err(DirectoryError::Request {
                code: StatusCodeError::Unknown,
                message: "同一用户和 Project 存在多个角色关联".to_owned(),
            });
        }
        Ok(first)
    }
}

fn authorization_snapshot(
    authorization: AuthorizationView<'_>,
) -> Result<(String, Vec<String>), DirectoryError> {
    let id = required_string(authorization.id(), "authorization.id")?;
    let mut role_keys = authorization
        .roles()
        .iter()
        .map(|role| required_string(role.key(), "authorization.role.key"))
        .collect::<Result<Vec<_>, _>>()?;
    role_keys.sort();
    role_keys.dedup();
    Ok((id, role_keys))
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
        Ok(Some(user.into_external_identity()))
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

    async fn ensure_project_role(
        &self,
        key: &str,
        display_name: &str,
    ) -> Result<ProjectRoleEnsureOutcome, IdentityDirectoryError> {
        let mut request = AddProjectRoleRequest::new();
        request.set_project_id(self.project_id.as_str());
        request.set_role_key(key);
        request.set_display_name(display_name);
        match self
            .project_client
            .add_project_role(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
        {
            Ok(_) => Ok(ProjectRoleEnsureOutcome::Created),
            Err(error) if error.code() == StatusCodeError::AlreadyExists => {
                Ok(ProjectRoleEnsureOutcome::Existing)
            }
            Err(error) => {
                tracing::warn!(
                    code = ?error.code(),
                    project_id = %self.project_id,
                    role_key = key,
                    "ZITADEL Project 角色创建失败"
                );
                Err(IdentityDirectoryError::Unavailable)
            }
        }
    }

    async fn update_project_role(
        &self,
        key: &str,
        display_name: &str,
    ) -> Result<(), IdentityDirectoryError> {
        let mut request = UpdateProjectRoleRequest::new();
        request.set_project_id(self.project_id.as_str());
        request.set_role_key(key);
        request.set_display_name(display_name);
        self.project_client
            .update_project_role(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| {
                tracing::warn!(
                    code = ?error.code(),
                    project_id = %self.project_id,
                    role_key = key,
                    "ZITADEL Project 角色更新失败"
                );
                IdentityDirectoryError::Unavailable
            })?;
        Ok(())
    }

    async fn remove_project_role(&self, key: &str) -> Result<(), IdentityDirectoryError> {
        let mut request = RemoveProjectRoleRequest::new();
        request.set_project_id(self.project_id.as_str());
        request.set_role_key(key);
        let result = self
            .project_client
            .remove_project_role(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error) if error.code() == StatusCodeError::NotFound => Ok(()),
            Err(error) => {
                tracing::warn!(
                    code = ?error.code(),
                    project_id = %self.project_id,
                    role_key = key,
                    "ZITADEL Project 角色删除失败"
                );
                Err(IdentityDirectoryError::Unavailable)
            }
        }
    }

    async fn replace_project_roles(
        &self,
        identity_id: &str,
        role_keys: &[String],
    ) -> Result<Vec<String>, IdentityDirectoryError> {
        self.replace_authorization_roles(identity_id, role_keys)
            .await
            .map_err(identity_directory_error)
    }
}

#[async_trait]
impl ServiceAccountDirectory for ZitadelUserDirectory {
    async fn account_by_username(
        &self,
        username: &str,
    ) -> Result<Option<ProviderUsernameMatch>, ServiceAccountDirectoryError> {
        let request = user_by_username_request(username);
        let response = self
            .user_client
            .list_users(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(service_account_directory_status)?;
        let mut matches = response.result().iter().filter(|user| {
            user.username()
                .to_str()
                .is_ok_and(|value| value.eq_ignore_ascii_case(username))
        });
        let result = matches.next().map(provider_username_match).transpose()?;
        if matches.next().is_some() {
            tracing::warn!("ZITADEL username 查询返回多个精确匹配账号");
            return Err(ServiceAccountDirectoryError::Unavailable);
        }
        Ok(result)
    }

    async fn create_service_account(
        &self,
        request: &CreateServiceAccountIdentity,
    ) -> Result<ServiceAccountIdentity, ServiceAccountDirectoryError> {
        let create = create_service_account_request(self.organization_id.as_str(), request);
        let response = self
            .user_client
            .create_user(create.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(service_account_directory_status)?;
        let identity_id = required_string(response.id(), "create_service_account.id")
            .map_err(service_account_directory_error)?;
        if identity_id.trim().is_empty() {
            return Err(ServiceAccountDirectoryError::Unavailable);
        }
        Ok(ServiceAccountIdentity {
            identity_id,
            username: request.username.clone(),
            display_name: request.display_name.clone(),
            description: request.description.clone(),
        })
    }

    async fn update_service_account(
        &self,
        identity_id: &str,
        display_name: &str,
        description: Option<&str>,
    ) -> Result<(), ServiceAccountDirectoryError> {
        let mut machine = UpdateMachine::new();
        machine.set_name(display_name);
        machine.set_description(description.unwrap_or_default());
        machine.set_access_token_type(AccessTokenType::Jwt);
        let mut request = UpdateUserRequest::new();
        request.set_user_id(identity_id);
        request.set_machine(machine);
        self.user_client
            .update_user(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(service_account_directory_status)?;
        Ok(())
    }

    async fn delete_uncommitted_service_account(
        &self,
        identity_id: &str,
    ) -> Result<(), ServiceAccountDirectoryError> {
        self.delete_user(identity_id)
            .await
            .map_err(service_account_directory_error)
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
        _ => {
            tracing::warn!(error = ?error, "ZITADEL 身份目录请求失败");
            IdentityDirectoryError::Unavailable
        }
    }
}

/// 测试可见的 ZITADEL machine user 创建请求摘要，不包含任何敏感字段。
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelCreateServiceAccountRequestInspection {
    /// 目标 Organization ID。
    pub organization_id: String,
    /// 稳定 username / Client ID。
    pub username: String,
    /// machine user 展示名称。
    pub display_name: String,
    /// 可选用途说明。
    pub description: Option<String>,
    /// 是否明确要求 ZITADEL 签发 JWT access token。
    pub access_token_is_jwt: bool,
}

/// 构造并检查实际使用的 ZITADEL machine user 创建请求。
#[doc(hidden)]
pub fn inspect_create_service_account_request(
    organization_id: &str,
    request: &CreateServiceAccountIdentity,
) -> ZitadelCreateServiceAccountRequestInspection {
    let create = create_service_account_request(organization_id, request);
    let view = create.as_view();
    let machine = view.machine();
    ZitadelCreateServiceAccountRequestInspection {
        organization_id: view
            .organization_id()
            .to_str()
            .expect("测试 Organization ID 必须是 UTF-8")
            .to_owned(),
        username: view
            .username()
            .to_str()
            .expect("测试 username 必须是 UTF-8")
            .to_owned(),
        display_name: machine
            .name()
            .to_str()
            .expect("测试展示名称必须是 UTF-8")
            .to_owned(),
        description: machine
            .description_opt()
            .into_option()
            .map(|value| value.to_str().expect("测试说明必须是 UTF-8").to_owned()),
        access_token_is_jwt: machine.access_token_type() == AccessTokenType::Jwt,
    }
}

/// 检查 ZITADEL gRPC 状态到稳定服务账号 Provider 错误的映射。
#[doc(hidden)]
pub fn inspect_service_account_status_mapping(
    code: StatusCodeError,
) -> ServiceAccountDirectoryError {
    service_account_directory_status(StatusError::new(code, "test status"))
}

fn service_account_directory_status(error: StatusError) -> ServiceAccountDirectoryError {
    match error.code() {
        StatusCodeError::AlreadyExists => ServiceAccountDirectoryError::Conflict,
        StatusCodeError::NotFound => ServiceAccountDirectoryError::NotFound,
        _ => {
            tracing::warn!(code = ?error.code(), "ZITADEL 服务账号目录请求失败");
            ServiceAccountDirectoryError::Unavailable
        }
    }
}

fn service_account_directory_error(error: DirectoryError) -> ServiceAccountDirectoryError {
    match error {
        DirectoryError::Request {
            code: StatusCodeError::AlreadyExists,
            ..
        } => ServiceAccountDirectoryError::Conflict,
        DirectoryError::Request {
            code: StatusCodeError::NotFound,
            ..
        } => ServiceAccountDirectoryError::NotFound,
        error => {
            tracing::warn!(error = ?error, "ZITADEL 服务账号目录响应无效");
            ServiceAccountDirectoryError::Unavailable
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

fn create_service_account_request(
    organization_id: &str,
    request: &CreateServiceAccountIdentity,
) -> CreateUserRequest {
    let mut machine = CreateMachine::new();
    machine.set_name(request.display_name.as_str());
    if let Some(description) = request.description.as_deref() {
        machine.set_description(description);
    }
    machine.set_access_token_type(AccessTokenType::Jwt);

    let mut create = CreateUserRequest::new();
    create.set_organization_id(organization_id);
    create.set_username(request.username.as_str());
    create.set_machine(machine);
    create
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

fn user_by_username_request(username: &str) -> ListUsersRequest {
    let mut request = ListUsersRequest::new();
    let mut query = ListQuery::new();
    query.set_limit(2);
    query.set_asc(true);
    request.set_query(query);
    request.set_sorting_column(UserFieldName::UserName);

    let mut username_query = UserNameQuery::new();
    username_query.set_user_name(username);
    username_query.set_method(TextQueryMethod::EqualsIgnoreCase);
    let mut search = SearchQuery::new();
    search.set_user_name_query(username_query);
    request.queries_mut().push(search);
    request
}

fn provider_username_match(
    user: UserView<'_>,
) -> Result<ProviderUsernameMatch, ServiceAccountDirectoryError> {
    if user.human_opt().into_option().is_some() {
        return Ok(ProviderUsernameMatch::Human);
    }
    let Some(machine) = user.machine_opt().into_option() else {
        return Err(ServiceAccountDirectoryError::Unavailable);
    };
    let identity_id = required_string(user.user_id(), "service_account.user_id")
        .map_err(service_account_directory_error)?;
    let username = required_string(user.username(), "service_account.username")
        .map_err(service_account_directory_error)?;
    let display_name = required_string(machine.name(), "service_account.name")
        .map_err(service_account_directory_error)?;
    let description = required_string(machine.description(), "service_account.description")
        .map_err(service_account_directory_error)
        .map(non_empty_owned)?;
    Ok(ProviderUsernameMatch::ServiceAccount(
        ServiceAccountIdentity {
            identity_id,
            username,
            display_name,
            description,
        },
    ))
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
