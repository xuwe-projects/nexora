---
layout: home

hero:
  name: Nexora
  text: Rust 桌面全栈框架
  tagline: 用 GPUI 构建桌面体验，用 Axum、SQLx 与 PostgreSQL 承载服务端能力。
  actions:
    - theme: brand
      text: 快速开始
      link: /guide/getting-started
    - theme: alt
      text: 查看 GitHub
      link: https://github.com/xuwe-projects/nexora

features:
  - title: 声明式桌面应用
    details: Feature、Window、强类型路由、Sidebar 分组、标签页与生命周期由框架统一协调。
  - title: 可选的 Account 体验
    details: 默认登录门禁、用户管理、角色权限管理与可覆盖的页面实现。
  - title: 可组合服务端
    details: Server 负责连接池、迁移、Account、Setup、日志和优雅关闭，业务 Router 继续由应用定义。
---

::: warning Early alpha
Nexora 目前仍在快速演进，公开 API 可能发生破坏性调整。
:::
