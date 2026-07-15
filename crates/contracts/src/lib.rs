//! 服务端、Rust SDK 与桌面应用共享的 HTTP API 契约。
//!
//! 本 crate 只描述跨进程传输的请求、响应、分页和错误数据，不包含领域服务、数据库实体、
//! Axum 路由或 HTTP client。服务端负责在内部领域模型与这些公开契约之间显式映射。

/// 账号、角色、权限和授权快照的 HTTP 契约。
pub mod account;
/// 不分页集合资源共享的响应包装。
pub mod collection;
/// 所有 API 失败响应共享的稳定错误结构。
pub mod error;
/// 服务健康检查的公开响应契约。
pub mod health;
/// 页码分页资源共享的查询参数与响应包装。
pub mod pagination;
/// HTTP PATCH 请求共享的字段更新语义。
pub mod patch;
