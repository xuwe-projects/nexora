# macOS 更新程序示例

这个独立 workspace 验证 Nexora 的首次 DMG 安装、签名 `latest.json`、`.app.zip` 自更新、公共
更新界面、独立 sidecar、健康确认和失败回滚。可见名称是“macOS 更新程序示例”；Cargo package
内部 executable 与 updater sidecar 名仍使用 `nexora-updater-macos-example`；外部分发文件使用
展示名称与架构，例如 `macOS 更新程序示例-aarch64.dmg`。

示例使用 `assets/logos/updater-macos/` 的 app 级品牌资源，只通过 `nexora::desktop` 安装 updater
并运行 sidecar；没有直接依赖内部 `updater` crate，也没有私有弹窗或状态机。

完整配置字段、默认值、密钥轮换、验证链路与生产签名说明见
[桌面自动更新文档](../../docs/desktop/updater.md)。

## 首次安装开发工具

```bash
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.28.0 cli --locked --force --bin nexora

nexora doctor
nexora doctor --fix
```

交互式 `nexora build` 与 `nexora doctor --fix` 会自动准备 Rust target、`cargo-bundle`、Homebrew
和 `create-dmg`。缺少 Xcode Command Line Tools 时会启动系统安装器；完成后重新运行原命令。
`nexora doctor` 只检查，CI/非交互环境只返回完整安装命令。

## 第一次配置

```bash
cp nexora.toml.example nexora.toml
nexora updater keygen --app updater-macos \
  --private-key-file .secrets/updater-macos.key
```

把 keygen 输出的公钥写入 `trusted_public_keys`。`.secrets/` 已被 Git 忽略；私钥只由 publish
读取，不会进入应用 bundle。若 `release.signing_key_file` 非空，publish 只读取该文件；文件
不存在或无效立即失败，不回退环境变量。只有该字段未配置或为空时才读取 `signing_key_env`。

RustFS 管理员需要提前创建 bucket、配置公开匿名下载，并向发布者提供 S3 兼容 endpoint、bucket、
region、path-style 选项和 AK/SK。凭据只通过发布 shell/CI 环境加载，例如：

```bash
set -a
source ./rustfs.env
set +a
```

不要把 `rustfs.env` 或其内容提交、复制到 `nexora.toml` 或输出到日志。HTTP 与 ad-hoc 只适用于
本地/LAN 测试；正式环境必须使用 HTTPS、Developer ID、notarization 和 `expected_team_id`。

示例的 release identity 参数支持以下两组值：

| 参数 | 动态值（默认） | 显式兼容值 |
| --- | --- | --- |
| `release.version` | `"${CARGO_PKG_VERSION}"` | SemVer，例如 `"1.2.3"` |
| `release.build_number` | `"${BUILD_DATETIME}"` | 正整数，例如 `42` |

## 第一次构建和安装 DMG

```bash
nexora doctor
nexora icons generate --app updater-macos
nexora build --app updater-macos
```

示例默认从所选 `nexora-updater-macos-example` package 读取 Cargo version（包括
`version.workspace = true`），并用构建机器本机时区的 24 小时制 `yyMMddHHmmss` 生成 build
number。`nexora build` 会先把
最终身份冻结到 `dist/updater-macos/stable/release.json`，再生成 `.app`、DMG、`.app.zip`、
sidecar、bundle updater 配置、每个 ZIP/DMG 的 `.sha256` 和 `artifact.json`，不访问 RustFS。
失败重试在 target 未完整时复用该构建号；完整构建后再次执行会生成严格更高的动态构建号。

打开生成的 DMG，把“macOS 更新程序示例.app”拖入
`/Applications`，再从 Finder 启动。DMG 是首次安装介质；`.app.zip` 是 sidecar 使用的更新负载；
`latest.json` 是 publish 生成的签名清单。`platforms.macos.icon` 配置的 ICNS 只写入 `.app` 的
`CFBundleIconFile`；DMG 文件及其挂载卷保留系统默认外观，不设置软件品牌图标。

确认 bundle 中存在：

```text
Contents/Resources/logo-icon.icns
Contents/Resources/nexora-updater.json
Contents/Helpers/updater-macos-updater
```

## 第一次发布

完成一次 build 后，先完成 dry-run 再发布：

```bash
nexora publish --app updater-macos --dry-run
nexora publish --app updater-macos
```

`nexora publish` 只从 release receipt 读取冻结的 version/build number，校验所有 required target
的 artifact、大小和 SHA-256，不重新计算时间，也不会隐式 build。它读取并验签远端
`latest.json`，要求待发布 identity 严格更高，上传版本化 ZIP/DMG 及其 `.sha256`，计算新的
manifest sequence，先更新 channel 根 branded 文件，最后才上传 mutable `latest.json`，并匿名回读
校验旁车和所有 versioned 更新 URL。不会再生成安装包 `latest.*` alias。
发布端从重新校验后的摘要生成旁车，因此旧构建没有本地 `.sha256` 时仍可发布。dry-run 走相同
预检但不写本地或远端。

## 从旧版本升级

默认动态配置下，把 `Cargo.toml` 中所选 package 的版本升级（若使用
`version.workspace = true`，则升级 workspace version），然后重新 build。仍可改回显式配置：

```toml
[apps.updater-macos.release]
version = "1.0.1"
build_number = 2
```

显式 SemVer 和正整数继续兼容；`${CARGO_PKG_VERSION}` 与 `${BUILD_DATETIME}` 必须是完整字段值，
不支持通用环境变量插值。

然后执行：

```bash
nexora build --app updater-macos
nexora publish --app updater-macos --dry-run
nexora publish --app updater-macos
```

启动已安装的 `1.0.0`，验证：

1. 启动检查不阻塞登录或主窗口，网络失败也不影响应用。
2. 发现 `1.0.1` 后只显示非模态通知，不自动下载。
3. 默认登录页、账户菜单、Settings、示例页面按钮和 macOS 原生“检查更新…”打开同一会话。
4. 用户在公共弹窗确认后才下载，并显示下载、验签、SHA-256、暂存和重启进度。
5. sidecar 安装前再次验证 app/channel/version/build/target、清单、artifact 与 macOS 签名。
6. 替换后显示名称、安装路径和图标不变，新版本报告健康；多次触发不并发下载。
7. 取消、健康超时或启动失败不损坏旧应用，健康失败会回滚。

第一次安装的旧版本如果没有 bundle updater 配置或 sidecar，必须重新从新 DMG 安装，不能靠
旧应用自举更新能力。

## 密钥与签名边界

- RustFS AK/SK：只授权发布端上传对象，不进入客户端。
- Ed25519 私钥/公钥：私钥签署 `latest.json`，内置公钥验证发布者；SHA-256 只验证 artifact
  与已签清单一致。
- Apple Developer ID：签署 `.app` 的 macOS 代码身份；Team ID 与 notarization 由 Apple
  信任链验证。

轮换 Ed25519 密钥时，先发布同时信任新旧公钥的客户端，再切换签名私钥，最后在覆盖足够版本
后移除旧公钥。不能直接替换客户端唯一公钥。
