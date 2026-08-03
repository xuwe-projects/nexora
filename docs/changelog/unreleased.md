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

- 更新协议改为 Ed25519 签名信封和 `build_number` 字段，保留 SHA-256 作为负载完整性校验。
- macOS updater 安装阶段改为启动复制到随机临时目录的独立 sidecar，并通过一次性健康确认决定
  保留新版本或回滚旧版本。
- 账户菜单 key context 从产品命名改为通用 `nexora_account_menu`。
- `nexora build` 从所选 app 注册读取品牌和 updater 配置，将 ICNS 写入 `.app` 的 Resources 与
  Info.plist；缺失或格式错误的目标平台图标会在打包前失败。
- 手动更新流程在发现版本后等待用户确认再下载；可选启动检查仅检查版本，并以非模态通知提示。
- `examples/updater-macos` 改用 app 级品牌目录与 `nexora::desktop` updater 安装入口。

## Removed

- 删除 Jenkinsfile、旧桌面构建 env 示例、旧裸 `latest.json` 示例、macOS shell updater helper
  及其测试、macOS-only updater README。

## Upgrade Notes

1. 在根目录新增 `nexora.toml`，为每个桌面 app 声明 `app_id`、`branding`、平台图标、
   `publish_target`、`object_prefix`、updater 公钥和 required targets。
2. 运行 `nexora updater keygen --app <id>`，把公钥写入 `trusted_public_keys`，私钥放入安全文件
   或 CI Secret。
3. 下游已有项目同步 `.agents/skills/develop-nexora-updater`，或重新运行 `nexora init .` 让 CLI
   写入缺失 Skill。

## Validation

- 本变更应至少运行 `cargo fmt --all`、相关 crate 测试、`cargo check`、严格 Clippy 和
  `nexora lint --deny-warnings`。Windows/Linux 自更新替换和 macOS 签名/公证需要对应宿主验证。
