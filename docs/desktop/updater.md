# 桌面自动更新

Nexora 使用 Ed25519 签名的 `latest.json`、匿名下载的 `.app.zip` 和独立 sidecar 完成应用内
更新；DMG 只用于首次安装。S3/RustFS 凭据与更新签名私钥只存在于开发者发布环境，不能写入
客户端、bundle、日志或仓库。

当前生产链路只实现 macOS 打包和自动安装。Windows/Linux 图标字段属于统一 app 注册，但当前
不会生成对应安装包，也不会执行自动安装。

## 开发环境第一次准备

先安装 Rust、Xcode Command Line Tools、Homebrew，以及 Nexora CLI：

```bash
xcode-select --install
rustup target add aarch64-apple-darwin
cargo install cargo-bundle
brew install create-dmg
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.24.1 nexora --locked --force \
  --no-default-features --features cli --bin nexora
```

`nexora doctor` 只检查当前 macOS 工具链；缺少工具时返回失败。`nexora doctor --fix` 会尝试用
Cargo 安装 `cargo-bundle`、用 Homebrew 安装 `create-dmg`。Xcode Command Line Tools、Rust 与
Homebrew 本身应由开发者先安装：

```bash
nexora doctor
nexora doctor --fix
```

## 第一次创建项目、构建与安装

新项目可使用 `nexora create <name> --layout single`；需要 Account 桌面端和服务端时使用
`--layout workspace --features account`。已有 workspace 使用 `nexora init .`，然后在根目录维护
唯一的 `nexora.toml`，不要再用 Cargo bundle metadata 建立第二套 app 注册。

```bash
nexora create desktop --layout single
cd desktop
nexora doctor
nexora icons generate --app desktop
nexora build --app desktop
```

第一次构建会为所选 target 安装 Rust target，并生成：

- `.app`：本地 bundle，包含主程序、ICNS、sidecar 和 `nexora-updater.json`。
- `.app.zip`：应用内更新负载；sidecar 下载、校验并替换当前 `.app`。
- DMG：首次分发与安装介质；用户把显示名称对应的 `.app` 拖入 Applications。
- `release.json`：`dist/<app>/<channel>/release.json` 中冻结的本次 release identity 与目标列表。
- `artifact.json`：本地 build 产物索引与 SHA-256；publish 只消费它描述的既有产物。
- `latest.json`：publish 最后上传的 Ed25519 签名清单；客户端只信任内置公钥验证通过的内容。
- sidecar：独立进程，重新验签、下载、校验、暂存、等待主进程退出、事务替换、重启、健康
  确认并在失败时回滚。

应用使用 `nexora::config::initialize(None)` 时，Nexora 会忽略 sidecar 注入的
`--nexora-updater-health-*` 内部参数并继续选择默认 TOML；显式配置路径和普通首个位置参数
仍保持原有优先级。这样新版本可以完成配置初始化、创建主窗口并回报健康，而不会把健康会话
参数误判为配置文件路径。

应用成功调用 `nexora::desktop::install_updater` 后，Sidebar Footer 的“检查更新”菜单项、
macOS 原生菜单和默认快捷键会共同分发 `CheckForUpdates` Action。快捷键在 macOS 是
`Cmd+Shift+U`，在 Windows/Linux 是 `Ctrl+Shift+U`。

打开 DMG、拖入 `/Applications` 后，从 Finder 启动一次基础版本。只有从包含 updater 配置和
sidecar 的新 DMG 安装过的版本，后续才能应用内自更新。

## 完整 `nexora.toml` 示例

```toml
schema_version = 1

[publish.targets.rustfs]
provider = "s3"
endpoint = "https://updates-internal.example.com"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "https://downloads.example.com/desktop-releases"
allow_insecure_http = false

[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "示例桌面应用"
publish_target = "rustfs"
object_prefix = "products"

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
channel = "stable"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.0.0"
signing_key_file = ".secrets/desktop-update.key"

[apps.desktop.updater]
enabled = true
check_on_launch = true
feed_url = "https://downloads.example.com/desktop-releases/products/desktop/stable/latest.json"
channels = ["stable"]
trusted_public_keys = ["desktop-main:ed25519:BASE64_PUBLIC_KEY"]
signing_key_env = "DESKTOP_UPDATE_SIGNING_KEY"
check_interval = "15m"
check_jitter = "1m"
offline_grace_period = "24h"
mandatory_restart_delay = "15m"
health_timeout = "2m"

[apps.desktop.targets]
required = ["aarch64-apple-darwin"]

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "developer_id"
notarize = true
expected_team_id = "ABCDE12345"

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"

[apps.desktop.platforms.linux]
icons = [
  "assets/logos/desktop/logo-icon-16.png",
  "assets/logos/desktop/logo-icon-128.png",
  "assets/logos/desktop/logo-icon-512.png",
]
```

## 字段参考

“必填”表示当前 schema 或后续校验要求必须提供；配置解析使用 `deny_unknown_fields`，拼错字段
会立即失败。路径均相对 workspace 根目录，且必须留在 workspace 内。

### 根与发布目标

| 字段 | 必填 | 作用、来源与示例 | 默认值 | 秘密 | 配置错误行为 |
| --- | --- | --- | --- | --- | --- |
| `schema_version` | 是 | 配置 schema；固定写 `1` | 无 | 否 | 非 1 立即失败 |
| `publish.targets.<name>` | 是 | app 引用的稳定发布目标名，如 `rustfs` | 无 | 否 | 缺失或名称不安全时失败 |
| `provider` | 是 | 对象存储协议；当前只支持 `s3` | 无 | 否 | 非 `s3` 失败 |
| `endpoint` | 是 | S3/RustFS API 地址，由存储管理员提供 | 无 | 否 | URL 无效失败；HTTP 需显式允许 |
| `bucket` | 是 | 已创建的 bucket 名，如 `desktop-releases` | 无 | 否 | 空值或不安全名称失败 |
| `region` | 否 | S3 签名 region，由服务端提供 | `us-east-1` | 否 | 与服务端不符会导致请求签名/上传失败 |
| `force_path_style` | 否 | 使用 `endpoint/bucket/key`，RustFS/MinIO 常设 `true` | `false` | 否 | 错误模式会导致对象地址或签名失败 |
| `public_base_url` | 是 | 匿名下载 bucket 根 URL，由公开下载配置取得 | 无 | 否 | URL 无效失败；必须能匿名 GET |
| `allow_insecure_http` | 否 | 仅本地/LAN 测试允许 HTTP | `false` | 否 | HTTP 且未设 `true` 时立即失败 |

发布凭据不写入 TOML。publish 从 `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` 读取，或从
`RUSTFS_ACCESS_KEY_ID`/`RUSTFS_SECRET_ACCESS_KEY` 读取；可选 `AWS_SESSION_TOKEN`。bucket 必须
提前创建，并允许 `public_base_url` 下的 release 对象匿名下载。生产 endpoint 与下载地址都必须
使用 HTTPS。

### app、品牌与 release

其中 release identity 支持的配置参数是 `release.version` 与 `release.build_number`；下表同时列出
动态表达式和继续兼容的显式值形式。

| 字段 | 必填 | 作用、来源与示例 | 默认值 | 秘密 | 配置错误行为 |
| --- | --- | --- | --- | --- | --- |
| `apps.<app_key>` | 是 | CLI 稳定 app key，如 `desktop`；进入对象路径 | 无 | 否 | 缺 app 或 key 不安全时失败 |
| `package` | 是 | Cargo package 名，从 `cargo metadata` 取得 | 无 | 否 | package 不存在时 build 失败 |
| `app_id` | 是 | 永久 bundle identifier，如 `com.example.desktop` | 无 | 否 | 格式无效或冲突时失败 |
| `display_name` | 是 | Finder、DMG、安装路径显示名称 | 无 | 否 | 空值、路径字符或控制字符失败 |
| `publish_target` | 是 | 引用 `publish.targets` 的名称 | 无 | 否 | 目标不存在时失败 |
| `object_prefix` | 是 | bucket 内产品前缀，如 `products` | 无 | 否 | 不安全路径段失败 |
| `branding.application_logo` | 是 | 应用内 PNG，通常为 128px | 无 | 否 | 文件不存在或非 PNG 时失败 |
| `branding.icon_source` | 是 | 图标生成源 PNG | 无 | 否 | 文件不存在、格式/尺寸/透明通道无效时失败 |
| `branding.managed` | 否 | 允许 `icons generate` 重建已有输出 | `false` | 否 | `false` 且输出已存在时拒绝覆盖；可显式 `--force` |
| `release.channel` | 是 | 发布通道，如 `stable`；必须包含在 updater channels | 无 | 否 | 不一致时失败 |
| `release.version` | 是 | 完整字段 `${CARGO_PKG_VERSION}`，或显式 SemVer 如 `"1.2.3"` | 无 | 否 | 未知/片段表达式或非 SemVer 失败 |
| `release.build_number` | 是 | 完整字段 `${BUILD_DATETIME}`，或显式正整数如 `42` | 无 | 否 | 未知字符串、0 或溢出失败 |
| `release.minimum_supported_version` | 否 | 低于该版本时进入强制更新门禁 | `"0.0.0"` | 否 | 非 SemVer 失败 |
| `release.signing_key_file` | 否 | 更新签名私钥文件，相对根目录或绝对路径 | 未配置 | **是（文件内容）** | 见下方严格优先级 |

`${CARGO_PKG_VERSION}` 通过 `cargo metadata --no-deps --format-version 1` 读取所选 app 的
`package`，因此同时支持 package 自有 `version` 和 `version.workspace = true`；它不是 workspace
根名称或 Nexora CLI 自身版本。`${BUILD_DATETIME}` 使用 UTC `yyMMddHHmmss`，并在同秒重建或
时钟回拨时取 `max(当前 UTC 值, 上次本地构建号 + 1)`。这两个表达式都必须占满字段，不提供
任意环境变量插值或通用模板引擎。显式 SemVer 与正整数继续兼容。

`nexora icons generate --app <key>` 只消费所选 app 的品牌路径。构建把 ICNS 复制到
`.app/Contents/Resources` 并写入 `CFBundleIconFile`，不会修改 Cargo manifest。DMG 文件及其
挂载卷不设置软件品牌图标，保留系统默认外观，也不增加 `dmg_icon` 一类重复配置。

### updater

`[apps.<key>.updater]` 表本身必填。`enabled = false` 时不会嵌入配置、构建 sidecar、安装公共入口
或启动网络任务。

| 字段 | 必填 | 作用、来源与示例 | 默认值 | 秘密 | 配置错误行为 |
| --- | --- | --- | --- | --- | --- |
| `enabled` | 是 | 是否为该 app 构建 updater | 无 | 否 | 缺失无法解析 |
| `app_id` | 否 | 清单身份覆盖；通常不要配置 | 继承 app `app_id` | 否 | 格式无效失败；远端身份不匹配失败 |
| `check_on_launch` | 否 | 首个主窗口创建后非阻塞后台检查 | `false` | 否 | 非布尔值无法解析 |
| `feed_url` | 启用时是 | 必须等于 `<public_base_url>/<prefix>/<app>/<channel>/latest.json` | `""` | 否 | 启用后不完全匹配预期地址即失败 |
| `channels` | 启用时是 | 客户端内置信任通道，如 `["stable"]` | `[]` | 否 | 不含 release channel 时失败 |
| `trusted_public_keys` | 启用时是 | 客户端内置 Ed25519 公钥，来自 keygen 输出 | `[]` | 否 | 空、格式/Base64/长度无效时失败 |
| `signing_key_env` | 条件必填 | 未配置私钥文件时读取的环境变量名 | `""` | 否（变量值是秘密） | 文件未配置且环境变量缺失时 publish 失败 |
| `check_interval` | 否 | 周期检查间隔 | `"15m"` | 否 | 空值或运行时无法解析时失败 |
| `check_jitter` | 否 | 分散客户端请求的随机抖动 | `"1m"` | 否 | 空值或运行时无法解析时失败 |
| `offline_grace_period` | 否 | 已验证缓存允许离线运行的最长时间 | `"24h"` | 否 | 空值或运行时无法解析时失败 |
| `mandatory_restart_delay` | 否 | 强制更新暂存完成后的重启倒计时 | `"15m"` | 否 | 空值或运行时无法解析时失败 |
| `health_timeout` | 否 | 新版本启动后健康确认超时 | `"2m"` | 否 | 空值或 bundle 加载时无法解析则失败 |

### target 与平台

| 字段 | 必填 | 作用、来源与示例 | 默认值 | 秘密 | 配置错误行为 |
| --- | --- | --- | --- | --- | --- |
| `targets.required` | 是 | 完整发布必须包含的 macOS Rust targets | 无 | 否 | 空、重复或不支持 target 时失败 |
| `platforms.macos.icon` | 是 | 已生成的 `.icns` | 无 | 否 | 文件缺失或 ICNS 无效时失败 |
| `platforms.macos.signing` | 是 | `none`、`ad_hoc` 或 `developer_id` | 无 | 否 | 未知值失败；生产应为 `developer_id` |
| `platforms.macos.notarize` | 是 | 是否提交 Apple notarization | 无 | 否 | `true` 且非 Developer ID 时失败 |
| `platforms.macos.expected_team_id` | 否 | sidecar 安装前要求的新 bundle Team ID | 未配置 | 否 | 不匹配时拒绝安装；ad-hoc 本地验证应省略 |
| `platforms.windows.icon` | 是 | 统一注册的 ICO | 无 | 否 | 文件缺失/无效时图标生成或校验失败；当前不打包 Windows |
| `platforms.linux.icons` | 是 | 统一注册的 PNG 列表 | 无 | 否 | 空列表或文件无效时失败；当前不打包 Linux |

## 更新签名密钥

生成 Ed25519 密钥：

```bash
nexora updater keygen --app desktop \
  --private-key-file .secrets/desktop-update.key
```

把命令输出的公钥写入 `trusted_public_keys`；私钥文件加入 `.gitignore`，权限限制给发布账号，并
通过受控备份或 CI secret 保存。不要在命令行、日志、Issue 或聊天中输出私钥。

私钥读取优先级是安全边界：

1. `signing_key_file` 非空时只读取该文件。
2. 已配置文件不存在、不可读或内容无效时立即失败，绝不回退环境变量。
3. 字段未配置或去除空白后为空时，才读取 `signing_key_env` 指定的环境变量。

轮换时必须先发布一个同时信任新旧公钥的客户端并完成覆盖，再切换发布端签名私钥，确认活跃
客户端能验证新签名后，最后在后续客户端移除旧公钥。直接替换唯一公钥会让旧客户端永久无法
验证新清单。

三类凭据不可混淆：RustFS/S3 AK/SK 只授权上传对象；Ed25519 私钥签署更新清单，公钥内置于
客户端验证来源；Apple Developer ID 证书签署 macOS 代码身份并配合 notarization/Gatekeeper。
它们互不替代，也不应存放在同一配置文件。

## 签名与验证链路

publish 使用私钥签署 canonical JSON manifest payload，并把签名信封作为 `latest.json` 上传；
客户端只接受内置公钥验证通过的清单。随后客户端与 sidecar 都验证 app ID、channel、version、
build number、target 和 release 状态，并对下载的 `.app.zip` 做长度与 SHA-256 校验。macOS
Developer ID 发布还验证新 bundle 的代码签名与 `expected_team_id`。sidecar 在真正替换前会再次
独立完成清单、artifact、应用身份和代码签名验证，不能信任主进程传入的 URL 或 hash。

SHA-256 只证明下载内容与已签清单一致，不证明发布者身份；发布者身份来自 Ed25519 清单签名，
macOS 可执行身份来自 Developer ID 与 Apple 信任链。

## 接入公共 updater 与 sidecar

应用从 bundle 读取构建时嵌入的安全配置，并在 `Application::initialize` 安装一次：

```rust
use nexora::desktop;

let updater = desktop::UpdateConfig::from_current_bundle()?;

fn initialize(
    updater: desktop::UpdateConfig,
    cx: &mut gpui::App,
) -> Result<(), desktop::UpdaterInstallError> {
    desktop::install_updater(updater, cx)
}
```

独立 sidecar binary 只依赖 `nexora` 的 `desktop` feature：

```rust
fn main() -> std::process::ExitCode {
    match nexora::desktop::run_sidecar_from_env_args() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) | Err(_) => std::process::ExitCode::FAILURE,
    }
}
```

成功安装后，框架注册公共 `CheckForUpdates` Action，并在默认登录页、登录后账户菜单、Settings
和 macOS 原生应用菜单显示入口。多入口复用同一 app 级协调器。未安装或配置加载失败时，应用
不应继续创建 updater 入口；更新不读取 Account token，也不需要业务权限。

`check_on_launch = true` 时，首个主窗口创建后非阻塞检查。没有更新或网络失败不影响登录；发现
可选更新只显示非模态通知，不会自动下载。用户打开公共 Dialog 并确认后才下载，并使用公共
Dialog、Progress、Button、Icon、Alert/Notification 展示进度。不要在应用中复制 updater UI、
状态机或 sidecar。

## build、publish 与第一次升级验证

`nexora build` 在任何 target 构建前原子写入
`dist/<app>/<channel>/release.json`。收据记录 schema、app key、package、channel、最终
version/build number、两项来源、创建 Unix 秒和本次 targets；同一次 build 的全部 target 共用
该身份。构建中途失败时，只要收据仍与当前配置匹配且 target 未全部完成，重试会复用原构建号；
全部 target 的 artifact 已完整后再次显式 build，动态构建号会严格增大，旧版本化产物不会删除。
损坏或不支持的收据会在构建前失败，不从目录名猜测身份。

`nexora build` 只构建本地 `.app`、DMG、`.app.zip`、sidecar、bundle 配置、hash 和
`artifact.json`，不访问对象存储。版本、构建号、图标、updater 配置和 sidecar 全部写入后才
签名；ZIP 与 DMG 都来自同一个已完成资源写入并签名的 `.app`。

`nexora publish`（包括 yank）只从当前 release receipt 读取 version/build number，不重新计算
UTC 时间，也不会隐式 build。它校验收据与 app、package、channel、Cargo version、当前配置和
required targets 一致，再逐个验证 `artifact.json` 身份、文件存在性、大小与 SHA-256。dry-run
执行相同的本地与远端预检，但不写本地或远端；available identity 必须严格高于远端
`(version, build_number)`。

第一次发布：

```bash
nexora build --app desktop
nexora publish --app desktop --dry-run
nexora publish --app desktop
```

publish 会读取并验签远端 `latest.json`；404 代表 sequence 1，否则使用远端 sequence 加一。
它按“版本化 ZIP/DMG → release notes → immutable sequence manifest → latest DMG aliases →
`latest.json`”顺序上传，最后匿名回读 mutable 对象和 updater 下载 URL。对象布局为：

```text
<prefix>/<app>/<channel>/latest.json
<prefix>/<app>/<channel>/manifests/<sequence>.json
<prefix>/<app>/<channel>/releases/<version>/<build>/<target>/...
```

从旧版本升级验证时，先通过 DMG 安装基础版本，再更新 Cargo package version（或显式
`release.version` / `release.build_number`），重新 build、dry-run、publish。启动已安装旧版本，确认启动检查不阻塞
登录、通知不自动下载、公共弹窗确认后才下载、sidecar 替换并重启、新版本健康确认成功、多次
触发不并发下载、取消或网络失败不损坏旧应用。

## 本地与生产 macOS 签名

本地 LAN 验证可以使用 HTTP、`allow_insecure_http = true`、`signing = "ad_hoc"`、
`notarize = false`，并省略 `expected_team_id`。ad-hoc 没有 Apple Team ID，只证明 bundle 在本机
具有一致代码封装，不能称为生产签名。

生产必须切换到 HTTPS，并使用：

```toml
[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "developer_id"
notarize = true
expected_team_id = "ABCDE12345"
```

把 Developer ID Application 证书安装到构建 Keychain；只有多个证书时需要设置
`MACOS_SIGN_IDENTITY`。先用 `xcrun notarytool store-credentials` 创建 Keychain profile，默认名为
`nexora`，或用 `NOTARY_PROFILE` 选择其它 profile。构建会启用 hardened runtime、timestamp，
验证代码签名，提交 DMG notarization 并 staple。HTTP 永远只能用于受控本地测试，不能进入正式
发布配置。
