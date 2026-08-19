---
name: develop-nexora-updater
description: 使用 Nexora 设计、实现或审查跨平台桌面自动更新系统。适用于 nexora.toml 的 app/updater/publish 配置、nexora build/publish/updater keygen、Ed25519 签名清单、sidecar 更新器、S3 兼容或阿里云 OSS 发布、强制更新、离线宽限、撤回、回滚、多窗口 GPUI 更新门禁，以及清理旧 Jenkins 或旧 updater 实现。
---

# 开发 Nexora Updater

## 工作流程

1. 先读取仓库 `AGENTS.md`，再同时使用 `rust-code-style`、`develop-nexora-apps`；涉及可见 UI 时同时使用 `desktop-ui-component-selection`、`gpui-component` 与 `gpui-desktop-development`。
2. 读取 [updater-protocol.md](references/updater-protocol.md)，当任务涉及自动更新协议、构建发布、客户端检查下载、sidecar 安装、回滚、撤回、强制更新、离线宽限或遗留 updater 清理时必须遵守其中协议。
3. 从现有模块边界开始：配置解析、CLI build/publish、updater protocol/core、sidecar runtime、platform adapter、desktop integration 分层实现；不要创建平行 updater 或保留旧协议兼容分支。
4. 让 `nexora.toml` 成为 build/publish 的唯一项目配置来源。多 app 必须显式 `--app` 或 `--all`；`publish` 只发布已有产物，不隐式 build。
5. 使用 Ed25519 签名信封保护 `latest.json`，客户端和 sidecar 都必须验签；SHA-256 只验证负载完整性。S3 AK/SK、签名私钥和发布凭据不得进入客户端。
6. 桌面 UI 只使用窗口级 `gpui-component` Dialog/AlertDialog/Progress/Button/Notification layer；强制门禁必须覆盖登录页、Shell、Sidebar、标签和业务窗口。
7. 删除被新架构替代的 Jenkins、旧 macOS shell helper、旧 latest 协议、Console 项目专用默认和现行文档；保留真实历史 changelog、第三方名称和用户本地秘密配置。
8. 为配置选择、签名验签、sequence 重放、build/publish 分离、S3 上传顺序、安全解压、平台 adapter、强制门禁和遗留引用清理添加与风险相称的测试。
9. 完成后运行 `cargo fmt --all`、相关 `cargo test`、`cargo check`、严格 Clippy 和 `cargo run -p cli -- lint --workspace . --deny-warnings`；无法在当前宿主验证的 Windows/Linux/macOS 签名安装行为必须明确报告。
