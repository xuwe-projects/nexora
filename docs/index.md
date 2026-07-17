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
    details: Server 负责迁移、Account、Setup 与 Router 组合；连接池、监听器和服务生命周期由应用掌控。
---

## 一张图了解 Nexora

工作区、外观设置与登录体验使用同一套应用品牌，并同时支持浅色和深色模式。

<ProductShowcase locale="zh" />

::: warning Early alpha
Nexora 目前仍在快速演进，公开 API 可能发生破坏性调整。
:::
