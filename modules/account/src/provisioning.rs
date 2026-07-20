//! ZITADEL 客户门户与多租户开通管理客户端。
//!
//! 本模块封装 ZITADEL v2 Organization、Project、User 与 Authorization gRPC API，
//! 用于业务系统按客户 Organization 动态开通 portal project grant、用户与角色授权。

use std::{collections::BTreeSet, fmt};

use grpc::{StatusCodeError, StatusError, client::Channel};
use grpc_protobuf::CallBuilder as _;
use protobuf::{AsView, ProtoString, View};
use thiserror::Error;

use crate::{
    CreateHumanIdentity, SystemRole,
    directory::DirectoryUser,
    generated::zitadel::{
        authorization::v2::{
            AuthorizationView, AuthorizationsSearchFilter, CreateAuthorizationRequest,
            DeleteAuthorizationRequest, IDFilter as AuthorizationIDFilter,
            InIDsFilter as AuthorizationInIDsFilter, ListAuthorizationsRequest,
            PaginationRequest as AuthorizationPaginationRequest, UpdateAuthorizationRequest,
            authorization_service_client::AuthorizationServiceClient,
        },
        org::v2::{
            AddOrganizationRequest, ListOrganizationsRequest, ListQuery as OrganizationListQuery,
            OrganizationFieldName, OrganizationIDQuery, OrganizationNameQuery, OrganizationView,
            SearchQuery as OrganizationSearchQuery, TextQueryMethod,
            add_organization_request::Admin as AddOrganizationAdmin,
            organization_service_client::OrganizationServiceClient,
        },
        project::v2::{
            AddProjectRoleRequest, CreateProjectGrantRequest, DeleteProjectGrantRequest,
            IDFilter as ProjectIDFilter, InIDsFilter as ProjectInIDsFilter,
            ListProjectGrantsRequest, PaginationRequest as ProjectPaginationRequest,
            ProjectGrantFieldName, ProjectGrantSearchFilter,
            ProjectGrantState as ProtoProjectGrantState, ProjectGrantView,
            UpdateProjectGrantRequest, project_service_client::ProjectServiceClient,
        },
        user::v2::{
            CreateUserRequest, DeactivateUserRequest, DeleteUserRequest, Password,
            SendEmailVerificationCode, SetHumanEmail, SetHumanProfile,
            create_user_request::Human as CreateHumanUser, user_service_client::UserServiceClient,
        },
    },
    zitadel::{self, REQUEST_TIMEOUT},
};

const PAGE_SIZE: u32 = 100;

/// 面向业务客户绑定的 ZITADEL Organization 摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelOrganization {
    /// ZITADEL Organization 的稳定唯一 ID，业务库应保存该值用于后续绑定客户租户。
    pub id: String,
    /// Organization 的展示名称。
    pub name: String,
    /// ZITADEL 为该 Organization 生成或配置的主域名。
    pub primary_domain: Option<String>,
    /// Organization 当前是否处于可用状态。
    pub state: ZitadelOrganizationState,
}

/// ZITADEL Organization 的公开状态分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZitadelOrganizationState {
    /// Organization 已启用，用户可以在该组织上下文登录。
    Active,
    /// Organization 已停用，用户无法继续使用该组织。
    Inactive,
    /// Organization 已删除或被移除。
    Removed,
    /// 当前 ZITADEL 版本返回了框架尚未细分的状态。
    Unspecified,
}

/// 创建 ZITADEL Organization 时需要的管理员绑定输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateZitadelOrganizationRequest {
    /// 可选指定 Organization ID；通常由 ZITADEL 自动生成，迁移或幂等绑定已有 ID 时才填写。
    pub organization_id: Option<String>,
    /// Organization 名称。ZITADEL 要求实例范围内唯一，建议使用业务客户的稳定展示名称。
    pub name: String,
    /// 创建后立即授予组织管理员能力的 ZITADEL 用户 ID。
    ///
    /// 这里通常传当前平台服务账号或一个已知平台管理员用户的 ZITADEL user ID。为空会被拒绝，
    /// 避免创建出业务后端无法继续管理的客户 Organization。
    pub administrator_user_ids: Vec<String>,
    /// 授予管理员用户的 ZITADEL 组织成员角色。为空时 ZITADEL 默认授予 `ORG_OWNER`。
    pub administrator_roles: Vec<String>,
}

/// Project Grant 请求，用于把 portal Project 授权给某个客户 Organization。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelProjectGrantRequest {
    /// portal Project 的 ZITADEL Project ID。
    pub project_id: String,
    /// 获得 portal Project 访问与授权自管理能力的客户 Organization ID。
    pub granted_organization_id: String,
    /// 客户 Organization 可以管理或授予的 portal Project role keys。
    pub role_keys: Vec<String>,
}

/// ZITADEL Project Grant 摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelProjectGrant {
    /// 被 grant 的 portal Project ID。
    pub project_id: String,
    /// 拥有该 Project 的 Organization ID。
    pub owner_organization_id: Option<String>,
    /// 被授权访问 portal Project 的客户 Organization ID。
    pub granted_organization_id: String,
    /// 客户 Organization 名称；ZITADEL 列表响应没有返回时为 `None`。
    pub granted_organization_name: Option<String>,
    /// 当前 grant 允许客户 Organization 管理的 role keys。
    pub role_keys: Vec<String>,
    /// Project Grant 当前状态。
    pub state: ZitadelProjectGrantState,
}

/// Project Grant 的公开状态分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZitadelProjectGrantState {
    /// Project Grant 已启用。
    Active,
    /// Project Grant 已停用。
    Inactive,
    /// 当前 ZITADEL 版本返回了框架尚未细分的状态。
    Unspecified,
}

/// 幂等确保 Project Grant 后的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZitadelProjectGrantOutcome {
    /// 本次调用创建了新的 Project Grant。
    Created(
        /// 创建完成后可保存或审计的 Project Grant 摘要。
        ZitadelProjectGrant,
    ),
    /// Project Grant 已存在，本次调用把 role keys 更新为目标集合。
    Updated(
        /// 更新后的目标 Project Grant 摘要。
        ZitadelProjectGrant,
    ),
    /// Project Grant 已存在且 role keys 与目标集合一致。
    Unchanged(
        /// 已存在且无需修改的 Project Grant 摘要。
        ZitadelProjectGrant,
    ),
}

/// ZITADEL 用户授权请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelAuthorizationRequest {
    /// 被授予 portal Project role 的 ZITADEL 用户 ID。
    pub user_id: String,
    /// portal Project 的 ZITADEL Project ID。
    pub project_id: String,
    /// 授权所属的客户 Organization ID。
    pub organization_id: String,
    /// 要授予该用户的 portal Project role keys。
    pub role_keys: Vec<String>,
}

/// ZITADEL Authorization 摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZitadelAuthorization {
    /// ZITADEL Authorization 的稳定 ID，后续更新或删除授权时使用。
    pub id: String,
    /// 被授权用户的 ZITADEL 用户 ID。
    pub user_id: String,
    /// 授权所属的 Project ID。
    pub project_id: String,
    /// 授权所属的 Organization ID。
    pub organization_id: String,
    /// 当前授予的 Project role keys。
    pub role_keys: Vec<String>,
}

/// 幂等确保用户授权后的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZitadelAuthorizationOutcome {
    /// 本次调用创建了新的用户授权。
    Created(
        /// 创建完成后可保存或审计的用户授权摘要。
        ZitadelAuthorization,
    ),
    /// 授权已存在，本次调用把 role keys 更新为目标集合。
    Updated(
        /// 更新后的目标用户授权摘要。
        ZitadelAuthorization,
    ),
    /// 授权已存在且 role keys 与目标集合一致。
    Unchanged(
        /// 已存在且无需修改的用户授权摘要。
        ZitadelAuthorization,
    ),
}

/// 删除用户的幂等结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZitadelDeleteUserOutcome {
    /// 本次调用删除了目标用户。
    Deleted,
    /// 目标用户已经不存在，调用方可以按补偿成功继续。
    AlreadyAbsent,
}

/// 通过 Personal Access Token 调用 ZITADEL v2 管理 API 的 provisioning client。
#[derive(Clone)]
pub struct ZitadelProvisioningClient {
    org_client: OrganizationServiceClient<Channel>,
    project_client: ProjectServiceClient<Channel>,
    user_client: UserServiceClient<Channel>,
    authorization_client: AuthorizationServiceClient<Channel>,
}

impl fmt::Debug for ZitadelProvisioningClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZitadelProvisioningClient")
            .finish_non_exhaustive()
    }
}

impl ZitadelProvisioningClient {
    /// 使用 OIDC issuer 与服务账号 Personal Access Token 创建 ZITADEL provisioning client。
    ///
    /// 生产 issuer 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP。PAT 会作为敏感 gRPC
    /// metadata 发送，`Debug` 输出不会包含 PAT 或 Authorization header。
    ///
    /// # Errors
    ///
    /// issuer 不是安全绝对 URL、PAT 为空或含非法 metadata 字符，或 TLS 凭据无法创建时返回
    /// [`ZitadelProvisioningError`]。
    pub fn new(
        issuer: &str,
        personal_access_token: &str,
    ) -> Result<Self, ZitadelProvisioningError> {
        let channel = zitadel::authenticated_channel(issuer, personal_access_token)?;
        Ok(Self {
            org_client: OrganizationServiceClient::new(channel.clone()),
            project_client: ProjectServiceClient::new(channel.clone()),
            user_client: UserServiceClient::new(channel.clone()),
            authorization_client: AuthorizationServiceClient::new(channel),
        })
    }

    /// 创建客户 ZITADEL Organization，并立即绑定指定管理员用户。
    ///
    /// `administrator_user_ids` 不能为空；调用方应传平台服务账号或平台管理员在 ZITADEL 中的
    /// user ID，以保证 Organization 创建后业务后端仍具备管理路径。
    ///
    /// # Errors
    ///
    /// 请求字段无效、Organization 名称或 ID 冲突、ZITADEL 拒绝请求或响应缺少 ID 时返回错误。
    pub async fn create_organization(
        &self,
        request: &CreateZitadelOrganizationRequest,
    ) -> Result<ZitadelOrganization, ZitadelProvisioningError> {
        let name = required_input(request.name.as_str(), "organization.name")?;
        let organization_id = optional_input(
            request.organization_id.as_deref(),
            "organization.organization_id",
        )?;
        let administrator_user_ids = normalize_ids(
            request.administrator_user_ids.as_slice(),
            "organization.administrator_user_ids",
        )?;
        let administrator_roles = normalize_optional_list(
            request.administrator_roles.as_slice(),
            "organization.administrator_roles",
        )?;

        let mut create = AddOrganizationRequest::new();
        create.set_name(name.as_str());
        if let Some(organization_id) = organization_id.as_deref() {
            create.set_organization_id(organization_id);
        }
        for user_id in administrator_user_ids {
            let mut admin = AddOrganizationAdmin::new();
            admin.set_user_id(user_id.as_str());
            admin
                .roles_mut()
                .extend(administrator_roles.iter().cloned());
            create.admins_mut().push(admin);
        }
        let response = self
            .org_client
            .add_organization(create.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("OrganizationService.AddOrganization", error))?;
        let id = required_string(
            response.organization_id(),
            "add_organization.organization_id",
        )?;
        if id.trim().is_empty() {
            return Err(ZitadelProvisioningError::InvalidResponse(
                "add_organization.organization_id",
            ));
        }
        Ok(ZitadelOrganization {
            id,
            name,
            primary_domain: None,
            state: ZitadelOrganizationState::Active,
        })
    }

    /// 按 ZITADEL Organization ID 查找客户组织。
    ///
    /// # Errors
    ///
    /// organization ID 为空、ZITADEL 查询失败或响应字符串无效时返回错误。
    pub async fn organization_by_id(
        &self,
        organization_id: &str,
    ) -> Result<Option<ZitadelOrganization>, ZitadelProvisioningError> {
        let organization_id = required_input(organization_id, "organization_id")?;
        let mut id = OrganizationIDQuery::new();
        id.set_id(organization_id.as_str());
        let mut search = OrganizationSearchQuery::new();
        search.set_id_query(id);
        self.find_organization(search).await
    }

    /// 按 Organization 名称查找客户组织，便于业务幂等绑定已存在的 ZITADEL org。
    ///
    /// # Errors
    ///
    /// Organization 名称为空、ZITADEL 查询失败或响应字符串无效时返回错误。
    pub async fn organization_by_name(
        &self,
        name: &str,
    ) -> Result<Option<ZitadelOrganization>, ZitadelProvisioningError> {
        let name = required_input(name, "organization.name")?;
        let mut name_query = OrganizationNameQuery::new();
        name_query.set_name(name.as_str());
        name_query.set_method(TextQueryMethod::Equals);
        let mut search = OrganizationSearchQuery::new();
        search.set_name_query(name_query);
        self.find_organization(search).await
    }

    /// 确保 portal Project 中存在给定 role keys。
    ///
    /// 该方法与默认 Account 系统角色同步使用同一 ProjectService v2 RPC，但 project_id 由调用方
    /// 动态传入，因此可用于 portal Project，而不会污染内部 Account 默认用户管理。
    ///
    /// # Errors
    ///
    /// project ID、role key 或 role 名称无效，或 ZITADEL 拒绝创建角色时返回错误。
    pub async fn ensure_project_roles(
        &self,
        project_id: &str,
        roles: &[SystemRole],
    ) -> Result<(), ZitadelProvisioningError> {
        let project_id = required_input(project_id, "project_id")?;
        for role in roles {
            let role_key = required_input(role.key.as_str(), "role.key")?;
            let display_name = required_input(role.name.as_str(), "role.name")?;
            let mut request = AddProjectRoleRequest::new();
            request.set_project_id(project_id.as_str());
            request.set_role_key(role_key.as_str());
            request.set_display_name(display_name.as_str());
            match self
                .project_client
                .add_project_role(request.as_view())
                .with_timeout(REQUEST_TIMEOUT)
                .await
            {
                Ok(_) => {}
                Err(error) if error.code() == StatusCodeError::AlreadyExists => {}
                Err(error) => {
                    return Err(request_error("ProjectService.AddProjectRole", error)
                        .with_resource("project_id", project_id.as_str())
                        .with_resource("role_key", role_key.as_str()));
                }
            }
        }
        Ok(())
    }

    /// 读取 portal Project 授予某个客户 Organization 的 Project Grant。
    ///
    /// # Errors
    ///
    /// project ID 或 organization ID 为空、ZITADEL 查询失败或响应字符串无效时返回错误。
    pub async fn project_grant(
        &self,
        project_id: &str,
        granted_organization_id: &str,
    ) -> Result<Option<ZitadelProjectGrant>, ZitadelProvisioningError> {
        let project_id = required_input(project_id, "project_id")?;
        let granted_organization_id =
            required_input(granted_organization_id, "granted_organization_id")?;
        let grants = self
            .list_project_grants(project_id.as_str(), granted_organization_id.as_str())
            .await?;
        Ok(grants.into_iter().next())
    }

    /// 幂等创建或更新 portal Project Grant。
    ///
    /// 已存在且 role keys 完全一致时返回 [`ZitadelProjectGrantOutcome::Unchanged`]；已存在但
    /// role keys 不一致时调用 ZITADEL update 并返回 `Updated`；不存在时调用 create 并返回
    /// `Created`。role keys 按集合语义比较，重复或空 role key 会被拒绝。
    ///
    /// # Errors
    ///
    /// 请求字段无效、查询/创建/更新 Project Grant 失败或 ZITADEL 响应无效时返回错误。
    pub async fn ensure_project_grant(
        &self,
        request: &ZitadelProjectGrantRequest,
    ) -> Result<ZitadelProjectGrantOutcome, ZitadelProvisioningError> {
        let normalized = normalized_project_grant_request(request)?;
        if let Some(existing) = self
            .project_grant(
                normalized.project_id.as_str(),
                normalized.granted_organization_id.as_str(),
            )
            .await?
        {
            if same_set(
                existing.role_keys.as_slice(),
                normalized.role_keys.as_slice(),
            ) {
                return Ok(ZitadelProjectGrantOutcome::Unchanged(existing));
            }
            let grant = self.update_project_grant(&normalized).await?;
            return Ok(ZitadelProjectGrantOutcome::Updated(grant));
        }
        match self.create_project_grant(&normalized).await {
            Ok(grant) => Ok(ZitadelProjectGrantOutcome::Created(grant)),
            Err(ZitadelProvisioningError::Request {
                code: StatusCodeError::AlreadyExists,
                ..
            }) => {
                let grant = self.update_project_grant(&normalized).await?;
                Ok(ZitadelProjectGrantOutcome::Updated(grant))
            }
            Err(error) => Err(error),
        }
    }

    /// 在指定客户 Organization 中创建 portal 人类用户。
    ///
    /// 返回的 identity ID 来自 ZITADEL `CreateUser` 响应；调用方应把它保存到业务库或本地
    /// Account 映射中。该方法不把用户写入 Nexora 默认内部 Account 用户表。
    ///
    /// # Errors
    ///
    /// organization ID、用户名、邮箱或姓名无效，ZITADEL 拒绝创建用户，或响应缺少用户 ID 时
    /// 返回错误。
    pub async fn create_human_user(
        &self,
        organization_id: &str,
        request: &CreateHumanIdentity,
    ) -> Result<DirectoryUser, ZitadelProvisioningError> {
        let organization_id = required_input(organization_id, "organization_id")?;
        let username = required_input(request.username.as_str(), "user.username")?;
        let given_name = required_input(request.given_name.as_str(), "user.given_name")?;
        let family_name = required_input(request.family_name.as_str(), "user.family_name")?;
        let email_address = required_input(request.email.as_str(), "user.email")?;
        let display_name = optional_input(request.display_name.as_deref(), "user.display_name")?;
        let initial_password =
            required_input(request.initial_password.as_str(), "user.initial_password")?;

        let mut profile = SetHumanProfile::new();
        profile.set_given_name(given_name.as_str());
        profile.set_family_name(family_name.as_str());
        if let Some(display_name) = display_name.as_deref() {
            profile.set_display_name(display_name);
        }

        let mut email = SetHumanEmail::new();
        email.set_email(email_address.as_str());
        email.set_send_code(SendEmailVerificationCode::new());

        let mut human = CreateHumanUser::new();
        human.set_profile(profile);
        human.set_email(email);
        let mut password = Password::new();
        password.set_password(initial_password.as_str());
        password.set_change_required(request.require_password_change);
        human.set_password(password);

        let mut create = CreateUserRequest::new();
        create.set_organization_id(organization_id.as_str());
        create.set_username(username.as_str());
        create.set_human(human);
        let response = self
            .user_client
            .create_user(create.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("UserService.CreateUser", error))?;
        let identity_id = required_string(response.id(), "create_user.id")?;
        if identity_id.trim().is_empty() {
            return Err(ZitadelProvisioningError::InvalidResponse("create_user.id"));
        }
        Ok(DirectoryUser {
            identity_id,
            username,
            display_name: display_name.unwrap_or_else(|| format!("{given_name} {family_name}")),
            email: Some(email_address),
            avatar_url: None,
        })
    }

    /// 停用指定 ZITADEL 用户。
    ///
    /// 停用会保留用户资料但阻止后续登录，适合客户员工离职或门户访问冻结。若 ZITADEL 返回
    /// “已停用”状态错误，调用方可按自己的幂等策略处理。
    ///
    /// # Errors
    ///
    /// user ID 为空或 ZITADEL 拒绝停用用户时返回错误。
    pub async fn deactivate_user(&self, user_id: &str) -> Result<(), ZitadelProvisioningError> {
        let user_id = required_input(user_id, "user_id")?;
        let mut request = DeactivateUserRequest::new();
        request.set_user_id(user_id.as_str());
        self.user_client
            .deactivate_user(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("UserService.DeactivateUser", error))?;
        Ok(())
    }

    /// 幂等删除指定 ZITADEL 用户，主要用于本地业务事务失败后的补偿。
    ///
    /// 目标用户不存在时返回 [`ZitadelDeleteUserOutcome::AlreadyAbsent`]，调用方可以把补偿视为
    /// 成功继续回滚本地状态。
    ///
    /// # Errors
    ///
    /// user ID 为空或 ZITADEL 以非 NotFound 状态拒绝删除时返回错误。
    pub async fn delete_user(
        &self,
        user_id: &str,
    ) -> Result<ZitadelDeleteUserOutcome, ZitadelProvisioningError> {
        let user_id = required_input(user_id, "user_id")?;
        let mut request = DeleteUserRequest::new();
        request.set_user_id(user_id.as_str());
        match self
            .user_client
            .delete_user(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
        {
            Ok(_) => Ok(ZitadelDeleteUserOutcome::Deleted),
            Err(error) if error.code() == StatusCodeError::NotFound => {
                Ok(ZitadelDeleteUserOutcome::AlreadyAbsent)
            }
            Err(error) => Err(request_error("UserService.DeleteUser", error)),
        }
    }

    /// 读取一个用户在 portal Project 与客户 Organization 上的授权。
    ///
    /// # Errors
    ///
    /// user ID、project ID 或 organization ID 为空，ZITADEL 查询失败或响应无效时返回错误。
    pub async fn authorization(
        &self,
        request: &ZitadelAuthorizationRequest,
    ) -> Result<Option<ZitadelAuthorization>, ZitadelProvisioningError> {
        let normalized = normalized_authorization_request(request)?;
        let authorizations = self
            .list_authorizations(
                normalized.user_id.as_str(),
                normalized.project_id.as_str(),
                normalized.organization_id.as_str(),
            )
            .await?;
        Ok(authorizations.into_iter().next())
    }

    /// 幂等创建或更新 portal 用户 Project role assignment。
    ///
    /// 已存在且 role keys 完全一致时返回 [`ZitadelAuthorizationOutcome::Unchanged`]；已存在但
    /// role keys 不一致时调用 update；不存在时调用 create。该方法不读取或修改 Nexora 默认
    /// Account RBAC 表，适合 iMES portal/openapi 的独立租户鉴权链路。
    ///
    /// # Errors
    ///
    /// 请求字段无效、查询/创建/更新授权失败或 ZITADEL 响应无效时返回错误。
    pub async fn ensure_authorization(
        &self,
        request: &ZitadelAuthorizationRequest,
    ) -> Result<ZitadelAuthorizationOutcome, ZitadelProvisioningError> {
        let normalized = normalized_authorization_request(request)?;
        if let Some(existing) = self.authorization(&normalized).await? {
            if same_set(
                existing.role_keys.as_slice(),
                normalized.role_keys.as_slice(),
            ) {
                return Ok(ZitadelAuthorizationOutcome::Unchanged(existing));
            }
            let authorization = self
                .update_authorization(existing.id.as_str(), normalized.role_keys.as_slice())
                .await?;
            return Ok(ZitadelAuthorizationOutcome::Updated(
                ZitadelAuthorization {
                    role_keys: normalized.role_keys,
                    ..existing
                }
                .with_updated_id(authorization.id),
            ));
        }
        match self.create_authorization(&normalized).await {
            Ok(authorization) => Ok(ZitadelAuthorizationOutcome::Created(authorization)),
            Err(ZitadelProvisioningError::Request {
                code: StatusCodeError::AlreadyExists,
                ..
            }) => {
                let existing = self.authorization(&normalized).await?.ok_or(
                    ZitadelProvisioningError::InvalidResponse(
                        "authorization.lookup_after_already_exists",
                    ),
                )?;
                let authorization = self
                    .update_authorization(existing.id.as_str(), normalized.role_keys.as_slice())
                    .await?;
                Ok(ZitadelAuthorizationOutcome::Updated(
                    ZitadelAuthorization {
                        role_keys: normalized.role_keys,
                        ..existing
                    }
                    .with_updated_id(authorization.id),
                ))
            }
            Err(error) => Err(error),
        }
    }

    /// 删除指定 ZITADEL Authorization。
    ///
    /// ZITADEL v2 删除 Authorization 本身按目标状态语义处理，不存在时也可返回成功。
    ///
    /// # Errors
    ///
    /// authorization ID 为空或 ZITADEL 拒绝删除时返回错误。
    pub async fn delete_authorization(
        &self,
        authorization_id: &str,
    ) -> Result<(), ZitadelProvisioningError> {
        let authorization_id = required_input(authorization_id, "authorization_id")?;
        let mut request = DeleteAuthorizationRequest::new();
        request.set_id(authorization_id.as_str());
        self.authorization_client
            .delete_authorization(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("AuthorizationService.DeleteAuthorization", error))?;
        Ok(())
    }

    async fn find_organization(
        &self,
        search: OrganizationSearchQuery,
    ) -> Result<Option<ZitadelOrganization>, ZitadelProvisioningError> {
        let mut request = ListOrganizationsRequest::new();
        let mut query = OrganizationListQuery::new();
        query.set_limit(2);
        query.set_asc(true);
        request.set_query(query);
        request.set_sorting_column(OrganizationFieldName::Name);
        request.queries_mut().push(search);
        let response = self
            .org_client
            .list_organizations(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("OrganizationService.ListOrganizations", error))?;
        response
            .result()
            .iter()
            .next()
            .map(organization_from_view)
            .transpose()
    }

    async fn list_project_grants(
        &self,
        project_id: &str,
        granted_organization_id: &str,
    ) -> Result<Vec<ZitadelProjectGrant>, ZitadelProvisioningError> {
        let mut pagination = ProjectPaginationRequest::new();
        pagination.set_limit(PAGE_SIZE);
        pagination.set_asc(true);
        let mut request = ListProjectGrantsRequest::new();
        request.set_pagination(pagination);
        request.set_sorting_column(ProjectGrantFieldName::ProjectId);

        let mut project_ids = ProjectInIDsFilter::new();
        project_ids.ids_mut().push(project_id);
        let mut project_filter = ProjectGrantSearchFilter::new();
        project_filter.set_in_project_ids_filter(project_ids);
        request.filters_mut().push(project_filter);

        let mut organization = ProjectIDFilter::new();
        organization.set_id(granted_organization_id);
        let mut organization_filter = ProjectGrantSearchFilter::new();
        organization_filter.set_granted_organization_id_filter(organization);
        request.filters_mut().push(organization_filter);

        let response = self
            .project_client
            .list_project_grants(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("ProjectService.ListProjectGrants", error))?;
        response
            .project_grants()
            .iter()
            .map(project_grant_from_view)
            .collect()
    }

    async fn create_project_grant(
        &self,
        request: &ZitadelProjectGrantRequest,
    ) -> Result<ZitadelProjectGrant, ZitadelProvisioningError> {
        let mut create = CreateProjectGrantRequest::new();
        create.set_project_id(request.project_id.as_str());
        create.set_granted_organization_id(request.granted_organization_id.as_str());
        create
            .role_keys_mut()
            .extend(request.role_keys.iter().cloned());
        self.project_client
            .create_project_grant(create.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("ProjectService.CreateProjectGrant", error))?;
        Ok(grant_from_request(
            request,
            ZitadelProjectGrantState::Active,
        ))
    }

    async fn update_project_grant(
        &self,
        request: &ZitadelProjectGrantRequest,
    ) -> Result<ZitadelProjectGrant, ZitadelProvisioningError> {
        let mut update = UpdateProjectGrantRequest::new();
        update.set_project_id(request.project_id.as_str());
        update.set_granted_organization_id(request.granted_organization_id.as_str());
        update
            .role_keys_mut()
            .extend(request.role_keys.iter().cloned());
        self.project_client
            .update_project_grant(update.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("ProjectService.UpdateProjectGrant", error))?;
        Ok(grant_from_request(
            request,
            ZitadelProjectGrantState::Active,
        ))
    }

    /// 删除 portal Project Grant。
    ///
    /// # Errors
    ///
    /// project ID 或客户 Organization ID 为空，或 ZITADEL 拒绝删除时返回错误。
    pub async fn delete_project_grant(
        &self,
        project_id: &str,
        granted_organization_id: &str,
    ) -> Result<(), ZitadelProvisioningError> {
        let project_id = required_input(project_id, "project_id")?;
        let granted_organization_id =
            required_input(granted_organization_id, "granted_organization_id")?;
        let mut request = DeleteProjectGrantRequest::new();
        request.set_project_id(project_id.as_str());
        request.set_granted_organization_id(granted_organization_id.as_str());
        self.project_client
            .delete_project_grant(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("ProjectService.DeleteProjectGrant", error))?;
        Ok(())
    }

    async fn list_authorizations(
        &self,
        user_id: &str,
        project_id: &str,
        organization_id: &str,
    ) -> Result<Vec<ZitadelAuthorization>, ZitadelProvisioningError> {
        let mut pagination = AuthorizationPaginationRequest::new();
        pagination.set_limit(PAGE_SIZE);
        pagination.set_asc(true);
        let mut request = ListAuthorizationsRequest::new();
        request.set_pagination(pagination);

        let mut users = AuthorizationInIDsFilter::new();
        users.ids_mut().push(user_id);
        let mut user_filter = AuthorizationsSearchFilter::new();
        user_filter.set_in_user_ids(users);
        request.filters_mut().push(user_filter);

        let mut project = AuthorizationIDFilter::new();
        project.set_id(project_id);
        let mut project_filter = AuthorizationsSearchFilter::new();
        project_filter.set_project_id(project);
        request.filters_mut().push(project_filter);

        let mut organization = AuthorizationIDFilter::new();
        organization.set_id(organization_id);
        let mut organization_filter = AuthorizationsSearchFilter::new();
        organization_filter.set_organization_id(organization);
        request.filters_mut().push(organization_filter);

        let response = self
            .authorization_client
            .list_authorizations(request.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("AuthorizationService.ListAuthorizations", error))?;
        response
            .authorizations()
            .iter()
            .map(authorization_from_view)
            .collect()
    }

    async fn create_authorization(
        &self,
        request: &ZitadelAuthorizationRequest,
    ) -> Result<ZitadelAuthorization, ZitadelProvisioningError> {
        let mut create = CreateAuthorizationRequest::new();
        create.set_user_id(request.user_id.as_str());
        create.set_project_id(request.project_id.as_str());
        create.set_organization_id(request.organization_id.as_str());
        create
            .role_keys_mut()
            .extend(request.role_keys.iter().cloned());
        let response = self
            .authorization_client
            .create_authorization(create.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("AuthorizationService.CreateAuthorization", error))?;
        let id = required_string(response.id(), "create_authorization.id")?;
        if id.trim().is_empty() {
            return Err(ZitadelProvisioningError::InvalidResponse(
                "create_authorization.id",
            ));
        }
        Ok(ZitadelAuthorization {
            id,
            user_id: request.user_id.clone(),
            project_id: request.project_id.clone(),
            organization_id: request.organization_id.clone(),
            role_keys: request.role_keys.clone(),
        })
    }

    async fn update_authorization(
        &self,
        authorization_id: &str,
        role_keys: &[String],
    ) -> Result<ZitadelAuthorization, ZitadelProvisioningError> {
        let mut update = UpdateAuthorizationRequest::new();
        update.set_id(authorization_id);
        update.role_keys_mut().extend(role_keys.iter().cloned());
        self.authorization_client
            .update_authorization(update.as_view())
            .with_timeout(REQUEST_TIMEOUT)
            .await
            .map_err(|error| request_error("AuthorizationService.UpdateAuthorization", error))?;
        Ok(ZitadelAuthorization {
            id: authorization_id.to_owned(),
            user_id: String::new(),
            project_id: String::new(),
            organization_id: String::new(),
            role_keys: role_keys.to_vec(),
        })
    }
}

impl ZitadelAuthorization {
    fn with_updated_id(mut self, id: String) -> Self {
        self.id = id;
        self
    }
}

/// ZITADEL provisioning 操作失败原因。
#[derive(Debug, Error)]
pub enum ZitadelProvisioningError {
    /// 本地 ZITADEL issuer 或 PAT 配置无效。
    #[error("ZITADEL provisioning 配置无效: {0}")]
    InvalidConfiguration(
        /// 不包含密钥或 token 的配置错误说明。
        &'static str,
    ),
    /// 请求字段不满足 ZITADEL provisioning 调用前置条件。
    #[error("ZITADEL provisioning 请求字段无效（field={field}, message={message}）")]
    InvalidInput {
        /// 出错字段的稳定名称。
        field: &'static str,
        /// 不包含敏感值的错误说明。
        message: &'static str,
    },
    /// gRPC TLS 凭据无法使用系统证书库创建。
    #[error("ZITADEL provisioning TLS 配置无效: {0}")]
    TlsConfiguration(
        /// gRPC 官方库返回的底层错误，不包含 PAT。
        String,
    ),
    /// ZITADEL v2 管理 API 请求失败。
    #[error(
        "ZITADEL provisioning gRPC 请求失败（operation={operation}, code={code:?}, message={message}, resources={resources:?}）"
    )]
    Request {
        /// 失败的稳定 RPC 操作名。
        operation: &'static str,
        /// gRPC 返回的标准状态码。
        code: StatusCodeError,
        /// gRPC 返回的状态消息；该值不包含标记为 sensitive 的 PAT metadata。
        message: String,
        /// 便于定位的非敏感资源字段。
        resources: Vec<(&'static str, String)>,
    },
    /// Protobuf 响应中的字符串不是有效 UTF-8。
    #[error("ZITADEL provisioning 响应中的 {0} 不是有效 UTF-8")]
    InvalidString(
        /// 无效字符串对应的稳定字段名。
        &'static str,
    ),
    /// ZITADEL 响应缺少调用方必须保存的关键字段。
    #[error("ZITADEL provisioning 响应缺少必要字段: {0}")]
    InvalidResponse(
        /// 缺失或为空的稳定字段名。
        &'static str,
    ),
}

impl ZitadelProvisioningError {
    fn with_resource(self, key: &'static str, value: &str) -> Self {
        match self {
            Self::Request {
                operation,
                code,
                message,
                mut resources,
            } => {
                resources.push((key, value.to_owned()));
                Self::Request {
                    operation,
                    code,
                    message,
                    resources,
                }
            }
            other => other,
        }
    }
}

impl From<zitadel::ClientError> for ZitadelProvisioningError {
    fn from(error: zitadel::ClientError) -> Self {
        match error {
            zitadel::ClientError::InvalidConfiguration(message) => {
                Self::InvalidConfiguration(message)
            }
            zitadel::ClientError::TlsConfiguration(message) => Self::TlsConfiguration(message),
        }
    }
}

fn request_error(operation: &'static str, error: StatusError) -> ZitadelProvisioningError {
    ZitadelProvisioningError::Request {
        operation,
        code: error.code(),
        message: error.message().to_owned(),
        resources: Vec::new(),
    }
}

fn required_input(value: &str, field: &'static str) -> Result<String, ZitadelProvisioningError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ZitadelProvisioningError::InvalidInput {
            field,
            message: "不能为空",
        });
    }
    Ok(value.to_owned())
}

fn optional_input(
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<String>, ZitadelProvisioningError> {
    value.map(|value| required_input(value, field)).transpose()
}

fn normalize_ids(
    values: &[String],
    field: &'static str,
) -> Result<Vec<String>, ZitadelProvisioningError> {
    let values = normalize_optional_list(values, field)?;
    if values.is_empty() {
        return Err(ZitadelProvisioningError::InvalidInput {
            field,
            message: "至少需要一个 ID",
        });
    }
    Ok(values)
}

fn normalize_role_keys(
    values: &[String],
    field: &'static str,
) -> Result<Vec<String>, ZitadelProvisioningError> {
    let values = normalize_optional_list(values, field)?;
    if values.is_empty() {
        return Err(ZitadelProvisioningError::InvalidInput {
            field,
            message: "至少需要一个 role key",
        });
    }
    Ok(values)
}

fn normalize_optional_list(
    values: &[String],
    field: &'static str,
) -> Result<Vec<String>, ZitadelProvisioningError> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = required_input(value, field)?;
        if !seen.insert(value.clone()) {
            return Err(ZitadelProvisioningError::InvalidInput {
                field,
                message: "不能包含重复值",
            });
        }
        normalized.push(value);
    }
    Ok(normalized)
}

fn normalized_project_grant_request(
    request: &ZitadelProjectGrantRequest,
) -> Result<ZitadelProjectGrantRequest, ZitadelProvisioningError> {
    Ok(ZitadelProjectGrantRequest {
        project_id: required_input(request.project_id.as_str(), "project_id")?,
        granted_organization_id: required_input(
            request.granted_organization_id.as_str(),
            "granted_organization_id",
        )?,
        role_keys: normalize_role_keys(request.role_keys.as_slice(), "role_keys")?,
    })
}

fn normalized_authorization_request(
    request: &ZitadelAuthorizationRequest,
) -> Result<ZitadelAuthorizationRequest, ZitadelProvisioningError> {
    Ok(ZitadelAuthorizationRequest {
        user_id: required_input(request.user_id.as_str(), "user_id")?,
        project_id: required_input(request.project_id.as_str(), "project_id")?,
        organization_id: required_input(request.organization_id.as_str(), "organization_id")?,
        role_keys: normalize_role_keys(request.role_keys.as_slice(), "role_keys")?,
    })
}

fn same_set(left: &[String], right: &[String]) -> bool {
    left.iter().collect::<BTreeSet<_>>() == right.iter().collect::<BTreeSet<_>>()
}

fn organization_from_view(
    organization: OrganizationView<'_>,
) -> Result<ZitadelOrganization, ZitadelProvisioningError> {
    let id = required_string(organization.id(), "organization.id")?;
    let name = required_string(organization.name(), "organization.name")?;
    let primary_domain =
        required_string(organization.primary_domain(), "organization.primary_domain")?;
    Ok(ZitadelOrganization {
        id,
        name,
        primary_domain: non_empty_owned(primary_domain),
        state: match organization.state() {
            crate::generated::zitadel::org::v2::OrganizationState::Active => {
                ZitadelOrganizationState::Active
            }
            crate::generated::zitadel::org::v2::OrganizationState::Inactive => {
                ZitadelOrganizationState::Inactive
            }
            crate::generated::zitadel::org::v2::OrganizationState::Removed => {
                ZitadelOrganizationState::Removed
            }
            _ => ZitadelOrganizationState::Unspecified,
        },
    })
}

fn project_grant_from_view(
    grant: ProjectGrantView<'_>,
) -> Result<ZitadelProjectGrant, ZitadelProvisioningError> {
    let owner_organization_id =
        required_string(grant.organization_id(), "project_grant.organization_id")?;
    let granted_organization_id = required_string(
        grant.granted_organization_id(),
        "project_grant.granted_organization_id",
    )?;
    let granted_organization_name = required_string(
        grant.granted_organization_name(),
        "project_grant.granted_organization_name",
    )?;
    let project_id = required_string(grant.project_id(), "project_grant.project_id")?;
    Ok(ZitadelProjectGrant {
        project_id,
        owner_organization_id: non_empty_owned(owner_organization_id),
        granted_organization_id,
        granted_organization_name: non_empty_owned(granted_organization_name),
        role_keys: grant
            .granted_role_keys()
            .iter()
            .map(|role| required_string(role.as_view(), "project_grant.granted_role_keys"))
            .collect::<Result<Vec<_>, _>>()?,
        state: match grant.state() {
            ProtoProjectGrantState::Active => ZitadelProjectGrantState::Active,
            ProtoProjectGrantState::Inactive => ZitadelProjectGrantState::Inactive,
            _ => ZitadelProjectGrantState::Unspecified,
        },
    })
}

fn grant_from_request(
    request: &ZitadelProjectGrantRequest,
    state: ZitadelProjectGrantState,
) -> ZitadelProjectGrant {
    ZitadelProjectGrant {
        project_id: request.project_id.clone(),
        owner_organization_id: None,
        granted_organization_id: request.granted_organization_id.clone(),
        granted_organization_name: None,
        role_keys: request.role_keys.clone(),
        state,
    }
}

fn authorization_from_view(
    authorization: AuthorizationView<'_>,
) -> Result<ZitadelAuthorization, ZitadelProvisioningError> {
    let id = required_string(authorization.id(), "authorization.id")?;
    let user =
        authorization
            .user_opt()
            .into_option()
            .ok_or(ZitadelProvisioningError::InvalidResponse(
                "authorization.user",
            ))?;
    let project = authorization.project_opt().into_option().ok_or(
        ZitadelProvisioningError::InvalidResponse("authorization.project"),
    )?;
    let organization = authorization.organization_opt().into_option().ok_or(
        ZitadelProvisioningError::InvalidResponse("authorization.organization"),
    )?;
    Ok(ZitadelAuthorization {
        id,
        user_id: required_string(user.id(), "authorization.user.id")?,
        project_id: required_string(project.id(), "authorization.project.id")?,
        organization_id: required_string(organization.id(), "authorization.organization.id")?,
        role_keys: authorization
            .roles()
            .iter()
            .map(|role| required_string(role.key(), "authorization.roles.key"))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn required_string(
    value: View<'_, ProtoString>,
    field: &'static str,
) -> Result<String, ZitadelProvisioningError> {
    value
        .to_str()
        .map(str::to_owned)
        .map_err(|_| ZitadelProvisioningError::InvalidString(field))
}

fn non_empty_owned(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}
