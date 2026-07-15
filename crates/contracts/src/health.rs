//! 服务健康检查的公开响应契约。

use serde::{Deserialize, Serialize};

/// 服务健康检查返回的稳定状态值。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// 服务及其必要依赖当前可以正常响应。
    Ok,
}

/// 无需认证即可读取的服务健康状态。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub struct HealthResponse {
    /// 当前服务健康状态。
    pub status: HealthStatus,
}
