---
title: CLI
order: 2
---

# CLI

## 安装

从 GitHub tag 安装正式发布的独立 `cli` package：

```bash
cargo install --git https://github.com/xuwe-projects/nexora --tag v0.33.2 cli --locked --force --bin nexora
```

从 Nexora 仓库根目录安装当前本地源码：

```bash
cargo install --path crates/cli --locked --force --bin nexora
```

以上命令不使用 Shell 专属的续行或环境变量语法，可直接作为单行命令用于 Unix Shell、
PowerShell 与 CMD。

## 命令

```text
nexora create <name> --layout single
nexora create <name> --layout workspace
nexora create <name> --layout workspace --features account
nexora init [path] --layout workspace
nexora icons generate --app <id>
nexora updater keygen --app <id>
nexora build
nexora build --app <id>
nexora build --app <id> --channel beta
nexora build --app <id> --all-channels
nexora publish
nexora publish --app <id> --dry-run
nexora publish --all --yes
nexora publish --app <id> yank
nexora doctor
nexora lint --workspace . --deny-warnings
nexora update
nexora version
```

## 只读依赖诊断

`nexora doctor` 只读取当前宿主的工具路径、版本与能力，不下载或安装软件，不执行 `winget
install`、`rustup target add`、`cargo install`、`brew install` 或 `xcode-select --install`，也不修改
PATH、打开浏览器或启动系统安装器。交互终端与 CI 使用相同语义；只有颜色和排版可以不同。

诊断会逐项显示用途、必需/条件必需状态、检测路径与版本、支持范围、官方下载地址、可复制的
人工安装/验证命令，以及安装后应重跑的 Nexora 命令。缺少必需工具或版本不兼容时返回非零
状态；只缺少当前配置未启用的签名/公证工具时显示 warning。

`nexora build` 在写入 `dist/<app>/<channel>/release.json`、创建 staging 或开始 Cargo 编译前运行
相同的只读预检。失败时不会写入新的 release receipt 或构建状态；用户安装依赖后重新执行原始
build 命令。

`nexora doctor --fix` 已删除。迁移方式是把旧命令改为 `nexora doctor`，然后根据输出自行执行
安装命令。完整 Windows/macOS 工具清单、版本范围、证书和密钥边界见
[桌面自动更新](../desktop/updater.md#人工安装构建依赖)。

Account 同时需要桌面端与服务端，只支持 workspace 布局。生成项目会固定当前 Nexora Git
tag；测试本地改动时可先用 `cargo install --path crates/cli --locked --bin nexora` 安装 CLI。

本地安装只替换 CLI 本身。要让新生成的应用也使用未发布代码，请把生成项目根清单中的
`nexora` workspace 依赖临时改成当前仓库 `crates/nexora` 的绝对 `path`。

在发布给其他仓库通过 Git tag 使用前，需要推送包含这些改动的新 tag；只测试当前仓库和
本地 CLI 时不需要发布 tag。

`nexora create` 与 `nexora init` 会同时生成根 `AGENTS.md` 和 `.agents/skills`。前者提供
始终生效的架构硬约束，后者提供按任务加载的详细工作流；`init` 不会覆盖项目已有的规则或
Skill 文件。生成的 `publish-nexora-release` Skill 负责版本升级、完整 Release Notes、处理人
与 Issue/PR 关联、相邻版本升级指南以及 tag/Release 发布门禁。

桌面自动更新使用仓库根目录 `nexora.toml` 注册 app、updater 策略和 S3 兼容发布目标。
同一 app 记录也声明 `assets/logos/<app_key>/` 下的应用内 Logo、图标源文件以及各平台图标；
`nexora icons generate --app <id>` 只根据所选 app 的源 PNG 重新生成标准 PNG、ICNS 和 ICO。
手工管理的品牌资源需要显式 `--force` 才允许覆盖。
同一 app 声明多个 `release.channels` 时，交互终端中的 `nexora build` / `publish` 会显示 channel
多选菜单并预选 `default_channel`；CI 应显式使用可重复的 `--channel <name>` 或
`--all-channels`。单通道 `release.channel` 继续兼容。
`nexora build` 只构建当前宿主的现有产物并写入 `artifact.json`；`nexora publish` 只发布已有
artifact，不会隐式触发 build。发布命令先上传并验证 versioned 不可变产物，再更新 channel 根
目录的 branded 文件和 sequence manifest，最后上传并匿名验证签名 `latest.json`。更新清单
使用 Ed25519 签名信封，公钥写入 app 的 `trusted_public_keys`，私钥只从安全文件或
`signing_key_env` 指定的环境变量读取。强制更新通过 `--minimum-supported-version` 写入清单。

`nexora update` 只更新 CLI 自身。它读取官方 GitHub Release 的 HTTPS `nexora-update.json`，选择
当前 target 的预编译资产并校验 size/SHA-256；不支持当前平台时会给出 package 为 `cli`、binary
为 `nexora` 的手工安装命令，不会回退本地 Cargo 编译，也不会请求 sudo 或 UAC。
