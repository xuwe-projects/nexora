---
title: CLI
order: 2
---

# CLI

```text
nexora create <name> --layout single
nexora create <name> --layout workspace
nexora create <name> --layout workspace --features account
nexora init [path] --layout workspace
nexora build
nexora doctor
nexora lint --workspace . --deny-warnings
nexora version
```

Account 同时需要桌面端与服务端，只支持 workspace 布局。生成项目会固定当前 Nexora Git
tag；测试本地改动时可先用 `cargo install --path crates/nexora ...` 安装 CLI。

本地安装只替换 CLI 本身。要让新生成的应用也使用未发布代码，请把生成项目根清单中的
`nexora` workspace 依赖临时改成当前仓库 `crates/nexora` 的绝对 `path`。

在发布给其他仓库通过 Git tag 使用前，需要推送包含这些改动的新 tag；只测试当前仓库和
本地 CLI 时不需要发布 tag。

`nexora create` 与 `nexora init` 会同时生成根 `AGENTS.md` 和 `.agents/skills`。前者提供
始终生效的架构硬约束，后者提供按任务加载的详细工作流；`init` 不会覆盖项目已有的规则或
Skill 文件。
