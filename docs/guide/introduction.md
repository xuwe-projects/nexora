---
title: 介绍
order: 1
---

# 介绍

Nexora 是一个 Rust 桌面全栈框架。它不替代 GPUI、gpui-component、Axum 或 SQLx，
而是把桌面应用注册、导航、Account 认证授权和服务端启动流程组合成稳定边界。

## 依赖边界

- 应用直接依赖并导入 `gpui` 与 `gpui_component`，可以自行选择和使用组件能力。
- `nexora` 提供 Feature、Window、Application、Server、配置和 Account 契约。
- 服务端只创建一个 `PgPool`，Nexora 模块与应用 Router 共享其廉价克隆句柄。
- 数据库迁移集中在 Nexora 的 migrate crate，并由 `Server` 在接收流量前执行。

## 公开 feature

| Feature | 用途 |
| --- | --- |
| `desktop` | GPUI 桌面运行时与 Account 客户端能力 |
| `server` | Axum 服务端、Account、ZITADEL 与默认 Setup |
| `derive` | Feature、Window、Settings 等派生宏 |
| `cli` | `nexora` 命令行 |

应用通常只写 `desktop, derive`；生成的服务端只写 `server, derive`。
