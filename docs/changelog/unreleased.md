# Nexora Unreleased

## Added

- 新增 `develop-nexora-updater` Skill，并同步到 CLI 脚手架内置 Skill 分发清单。下游项目通过
  `nexora create`、`nexora init` 或同步 `crates/nexora/templates/skills` 可获得该 Skill；它用于
  自动更新协议、build/publish、sidecar、强制门禁、撤回和遗留清理任务。
- `nexora updater keygen` 生成 Ed25519 更新签名密钥；`nexora publish` / `publish yank` 开始使用
  `nexora.toml` app 配置生成签名 `latest.json`。
- `nexora publish` 支持 RustFS/S3 兼容真实上传，按不可变产物、不可变 manifest、`latest.json`
  顺序发布，并在完成前通过匿名公开 URL 验证 `latest.json`。
- 新增 `examples/updater-macos/`，用于本地 RustFS 验证 macOS v1 → v2、强制更新和健康失败回滚。
- 新增中英文 updater 文档，覆盖安全模型、RustFS 配置、keygen/build/publish/yank、macOS 签名、
  Developer ID/notarization 和排障。
- `nexora create` / `init` 为每个 app 生成独立的 `assets/logos/<app_key>/` 完整品牌资源集；新增
  `nexora icons generate --app <id>`，从源 PNG 确定性生成多尺寸 PNG、ICNS 与 ICO。
- updater 作为显式安装的 `nexora::desktop` 公共能力，统一提供检查 Action、公共弹窗、默认登录页、
  账户菜单、设置和 macOS 原生菜单入口；未安装 updater 的应用不会显示入口或启动网络任务。

## Changed

- Windows 首次安装改用固定版本 Inno Setup 6.7.3，只生成简体中文 Setup EXE 与 sidecar
  更新 ZIP，不再生成 MSI 或 Burn bundle；构建和 doctor 通过 winget 检测/修复 ISCC，并移除
  cargo-wix、WiX 扩展与 Nexora 管理的 .NET SDK。安装固定为无需提权的当前用户范围，默认位于
  `%LOCALAPPDATA%\Programs\<app_id>`，保留目录选择、桌面/开始菜单快捷方式与完成后运行选项。
  Setup EXE 和更新 ZIP 复用同一 staging，发布仍上传两者及校验文件，但签名 `latest.json` 只
  引用 `windows_update_zip`。Windows 应用启动时从主 EXE 同级加载 `nexora-updater.json`；
  GUI 主程序与 sidecar 不创建命令行窗口。Windows 更新 ZIP 由 Rust 直接生成并固定使用 `/`
  条目分隔符。`signing = "none"` 保留 Ed25519、SHA-256、ZIP 安全和 PE 架构校验；
  `authenticode` 仍校验证书链、thumbprint 与 publisher。应用内更新事务继续使用安装目录同卷的
  隐藏兄弟目录，退出前预检替换权限；失败时恢复并重启旧版本，并在下次启动显示持久化结果。
- 默认脚手架与 `examples/updater-windows` 同时声明 `stable`、`beta` 和 `default_channel`；交互式
  `nexora build` 会显示 channel 选择，CI 可显式使用 `--channel` 或 `--all-channels`。
- `apps.<app>.targets.required` 改为可选；`nexora build` 默认使用 `rustc -vV` 的 host target，
  也可通过重复 `--target` 显式覆盖。交互式构建会自动安装缺失的 Rust target 及 macOS/Windows
  打包依赖；系统安装器完成后可重跑原命令续接，非交互环境只返回完整安装命令。
- 删除发布目标的 `credential_env_prefix`；Access Key、Secret Key 与 Session Token 分别按
  channel 专用 Nexora 变量、基础 Nexora 变量、AWS 变量逐字段回退，允许不同字段来自不同层级。
- `nexora build` 恢复为最终 ZIP 与 DMG 生成标准 `.sha256` 旁车，publish 会把旁车上传到版本化
  release 目录；`${BUILD_DATETIME}` 改用构建机器本机时区的 24 小时制 `yyMMddHHmmss`。
- 更新协议改为 Ed25519 签名信封和 `build_number` 字段，保留 SHA-256 作为负载完整性校验。
- macOS updater 安装阶段改为启动复制到随机临时目录的独立 sidecar，并通过一次性健康确认决定
  保留新版本或回滚旧版本。
- 账户菜单 key context 从产品命名改为通用 `nexora_account_menu`。

## Fixed

- Windows 构建依赖检查改为读取 `ISCC.exe` 的 PE 固定文件版本资源，不再从官方 `/?` 帮助
  横幅中解析并不存在的补丁版本；winget 安装的 Inno Setup 6.7.3 现在可被精确识别。
- `nexora build` 从所选 app 注册读取品牌和 updater 配置，将 ICNS 写入 `.app` 的 Resources 与
  Info.plist；缺失或格式错误的目标平台图标会在打包前失败。
- 手动更新流程在发现版本后等待用户确认再下载；可选启动检查仅检查版本，并以非模态通知提示。
- `examples/updater-macos` 改用 app 级品牌目录与 `nexora::desktop` updater 安装入口。

## Removed

- 删除 Jenkinsfile、旧桌面构建 env 示例、旧裸 `latest.json` 示例、macOS shell updater helper
  及其测试、macOS-only updater README。
- 删除 `nexora.toml` 的 `credential_env_prefix` 字段；旧配置必须直接移除该字段。

## Upgrade Notes

1. 在根目录新增 `nexora.toml`，为每个桌面 app 声明 `app_id`、`branding`、平台图标、
   `publish_target`、`object_prefix` 和 updater 公钥；普通项目无需再声明 required targets。
2. 运行 `nexora updater keygen --app <id>`，把公钥写入 `trusted_public_keys`，私钥放入安全文件
   或 CI Secret。
3. 下游已有项目同步 `.agents/skills/develop-nexora-updater`，或重新运行 `nexora init .` 让 CLI
   写入缺失 Skill。

## Validation

- 本变更应至少运行 `cargo fmt --all`、相关 crate 测试、`cargo check`、严格 Clippy 和
  `nexora lint --deny-warnings`。Windows/Linux 自更新替换和 macOS 签名/公证需要对应宿主验证。
