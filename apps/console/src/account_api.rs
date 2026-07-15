//! Console 用户、角色与权限管理使用的业务 API 客户端。

use std::time::Duration;

use contracts::{
    account::{
        AccessProfileResponse, CreateRoleRequest, PermissionResponse,
        ReplaceRolePermissionsRequest, ReplaceUserRolesRequest, RoleResponse, UpdateRoleRequest,
        UpdateUserStatusRequest, UserPageResponse, UserResponse,
    },
    collection::ItemsResponse,
    error::ErrorEnvelope,
};
use reqwest::{
    Method, StatusCode,
    blocking::{Client, RequestBuilder, Response},
};
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::auth::ApiSession;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Console 调用账号管理 API 时返回的结构化错误。
#[derive(Debug, Error)]
pub(crate) enum AccountApiError {
    /// HTTP 连接、超时或响应 JSON 解析失败。
    #[error("账号服务请求失败: {0}")]
    Request(
        /// Reqwest 返回且不包含 Bearer token 的底层错误。
        #[from]
        reqwest::Error,
    ),
    /// 服务端使用统一错误契约拒绝请求。
    #[error("账号服务拒绝请求: {message}（code={code}, request_id={request_id}）")]
    Rejected {
        /// HTTP 状态码。
        status: u16,
        /// 服务端稳定错误码。
        code: String,
        /// 适合展示给当前用户的错误说明。
        message: String,
        /// 用于服务端日志检索的请求 ID。
        request_id: String,
    },
}

impl AccountApiError {
    /// 返回适合在管理页面 Alert 中展示的错误信息。
    pub(crate) fn user_message(&self) -> String {
        match self {
            Self::Rejected {
                message,
                request_id,
                ..
            } if request_id != "unknown" => format!("{message}（请求 ID：{request_id}）"),
            Self::Rejected { message, .. } => message.clone(),
            Self::Request(_) => "无法连接账号服务，请检查网络或稍后重试".to_owned(),
        }
    }
}

/// 使用当前短期 access token 调用账号管理资源的同步客户端。
///
/// 调用方必须在 GPUI 后台执行器中使用该类型，避免阻塞前台事件循环。
pub(crate) struct AccountApi {
    client: Client,
    session: ApiSession,
}

impl AccountApi {
    /// 创建带统一超时配置的账号 API 客户端。
    ///
    /// # Errors
    ///
    /// 当前平台无法构造 Reqwest 客户端时返回 [`AccountApiError`]。
    pub(crate) fn new(session: ApiSession) -> Result<Self, AccountApiError> {
        Ok(Self {
            client: Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            session,
        })
    }

    /// 分页读取本地用户目录。
    pub(crate) fn list_users(
        &self,
        page: u32,
        page_size: u32,
    ) -> Result<UserPageResponse, AccountApiError> {
        let request = self
            .request(Method::GET, "users")
            .query(&[("page", page), ("page_size", page_size)]);
        self.send_json(request)
    }

    /// 读取指定用户及其直接角色和合并权限。
    pub(crate) fn get_user(&self, user_id: &str) -> Result<AccessProfileResponse, AccountApiError> {
        self.send_json(self.request(Method::GET, format!("users/{user_id}")))
    }

    /// 修改指定用户的访问状态。
    pub(crate) fn update_user_status(
        &self,
        user_id: &str,
        request: &UpdateUserStatusRequest,
    ) -> Result<UserResponse, AccountApiError> {
        self.send_json(
            self.request(Method::PATCH, format!("users/{user_id}"))
                .json(request),
        )
    }

    /// 原子替换指定用户的直接角色集合。
    pub(crate) fn replace_user_roles(
        &self,
        user_id: &str,
        request: &ReplaceUserRolesRequest,
    ) -> Result<AccessProfileResponse, AccountApiError> {
        self.send_json(
            self.request(Method::PUT, format!("users/{user_id}/roles"))
                .json(request),
        )
    }

    /// 读取全部角色及其直接权限。
    pub(crate) fn list_roles(&self) -> Result<Vec<RoleResponse>, AccountApiError> {
        let response: ItemsResponse<RoleResponse> =
            self.send_json(self.request(Method::GET, "roles"))?;
        Ok(response.items)
    }

    /// 创建自定义角色并返回服务端资源表示。
    pub(crate) fn create_role(
        &self,
        request: &CreateRoleRequest,
    ) -> Result<RoleResponse, AccountApiError> {
        self.send_json(self.request(Method::POST, "roles").json(request))
    }

    /// 修改指定自定义角色的名称或说明。
    pub(crate) fn update_role(
        &self,
        role_id: i64,
        request: &UpdateRoleRequest,
    ) -> Result<RoleResponse, AccountApiError> {
        self.send_json(
            self.request(Method::PATCH, format!("roles/{role_id}"))
                .json(request),
        )
    }

    /// 删除指定自定义角色。
    pub(crate) fn delete_role(&self, role_id: i64) -> Result<(), AccountApiError> {
        self.send_empty(self.request(Method::DELETE, format!("roles/{role_id}")))
    }

    /// 原子替换指定角色的直接权限集合。
    pub(crate) fn replace_role_permissions(
        &self,
        role_id: i64,
        request: &ReplaceRolePermissionsRequest,
    ) -> Result<RoleResponse, AccountApiError> {
        self.send_json(
            self.request(Method::PUT, format!("roles/{role_id}/permissions"))
                .json(request),
        )
    }

    /// 读取系统支持的完整权限目录。
    pub(crate) fn list_permissions(&self) -> Result<Vec<PermissionResponse>, AccountApiError> {
        let response: ItemsResponse<PermissionResponse> =
            self.send_json(self.request(Method::GET, "permissions"))?;
        Ok(response.items)
    }

    fn request(&self, method: Method, path: impl AsRef<str>) -> RequestBuilder {
        self.client
            .request(method, self.session.endpoint(path.as_ref()))
            .bearer_auth(self.session.access_token())
    }

    fn send_json<T>(&self, request: RequestBuilder) -> Result<T, AccountApiError>
    where
        T: DeserializeOwned,
    {
        let response = request.send()?;
        if response.status().is_success() {
            return Ok(response.json()?);
        }
        Err(rejected(response))
    }

    fn send_empty(&self, request: RequestBuilder) -> Result<(), AccountApiError> {
        let response = request.send()?;
        if response.status().is_success() {
            return Ok(());
        }
        Err(rejected(response))
    }
}

fn rejected(response: Response) -> AccountApiError {
    let status = response.status();
    let header_request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| "unknown".to_owned());
    let envelope = response.json::<ErrorEnvelope>().ok();
    AccountApiError::Rejected {
        status: status.as_u16(),
        code: envelope
            .as_ref()
            .map(|value| value.error.code.clone())
            .unwrap_or_else(|| fallback_error_code(status)),
        message: envelope
            .as_ref()
            .map(|value| value.error.message.clone())
            .unwrap_or_else(|| "账号服务返回了无法识别的错误响应".to_owned()),
        request_id: envelope
            .map(|value| value.error.request_id)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(header_request_id),
    }
}

fn fallback_error_code(status: StatusCode) -> String {
    format!("http_{}", status.as_u16())
}
