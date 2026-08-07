# 桌面自动更新

Nexora 使用 Ed25519 签名的 `latest.json`、平台更新 ZIP 和独立 sidecar 完成应用内更新；macOS
DMG 与 Windows Inno Setup EXE 只用于首次安装。S3/RustFS 凭据与更新签名私钥只存在于开发者
发布环境，不能写入客户端、bundle、日志或仓库。

当前生产链路实现 macOS 与 Windows x86_64/ARM64。Windows 首次构建生成简体中文 Inno Setup
EXE；应用内更新只使用 `windows.zip`。Windows 最低版本默认跟随当前锁定 GPUI，
即 Windows 10 1703（build 15063）。Linux 发布资源沿用同一元数据契约，但本页不承诺 Linux
自动安装。

## 开发环境第一次准备

先安装 Nexora CLI。`nexora build` 会在交互式终端中检测、自动安装并复检当前构建真正需要的
依赖；`nexora doctor` 只检查，`nexora doctor --fix` 使用相同修复流程：

```bash
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.30.1 cli --locked --force --bin nexora
nexora doctor
nexora doctor --fix
```

macOS 会准备 Rust target、`cargo-bundle`、Homebrew 与 `create-dmg`；缺少 Xcode Command Line
Tools 时启动 `xcode-select --install`，完成系统安装后重新运行同一条 build。Windows 会准备
Rust target、固定版本的 Inno Setup 6.7.3 与 Windows SDK。交互式修复通过 winget 安装官方
`JRSoftware.InnoSetup` 包；非交互环境不会启动安装器，只输出精确命令。Nexora 读取
`ISCC.exe` PE 资源中的固定文件版本 `6.7.3.0`，不会依赖只显示主版本的 `/?` 帮助横幅。

需要手动排查时，对应官方命令是：

```powershell
winget install --source winget --exact --id JRSoftware.InnoSetup --version 6.7.3 `
  --scope user --silent --force --accept-package-agreements --accept-source-agreements
winget install --source winget --exact --id Microsoft.WindowsSDK.10.0.26100
```

Windows SDK 或 Xcode/CLT 安装需要系统确认、提权或重启时，Nexora 会明确暂停并要求重新运行
原 build；不会静默提权。非交互/CI 环境不启动安装器，而是返回完整安装命令。Windows SDK
工具位于标准 Windows Kits 目录即可，无需手工把 `rc.exe`、`fxc.exe`、`signtool.exe` 加入 PATH。

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

构建默认使用 `rustc -vV` 的 host target，也可重复传入 `--target` 显式覆盖。交互式 build 会
自动执行缺失的 `rustup target add <target>`；非交互环境返回准确命令并停止。构建会生成：

- `.app`：本地 bundle，包含主程序、ICNS、sidecar 和 `nexora-updater.json`。
- `.app.zip`：应用内更新负载；sidecar 下载、校验并替换当前 `.app`。
- DMG：首次分发与安装介质；用户把显示名称对应的 `.app` 拖入 Applications。
- `release.json`：`dist/<app>/<channel>/release.json` 中冻结的本次 release identity 与目标列表。
- `artifact.json`：本地 build 产物索引与 SHA-256；publish 只消费它描述的既有产物。
- `<产物>.sha256`：ZIP 与 DMG 的标准 SHA-256 旁车文件，内容为摘要、两个空格和文件名。
- `latest.json`：publish 最后上传的 Ed25519 签名清单；客户端只信任内置公钥验证通过的内容。
- sidecar：独立进程，重新验签、下载、校验、暂存、等待主进程退出、事务替换、重启、健康
  确认并在失败时回滚。

外部分发文件统一命名为 `<display_name>-<arch><suffix>`，例如 `iMES-aarch64.dmg`、
`iMES-x86_64.windows.zip`；version、build number、Cargo package 和完整 target triple 不进入文件名。
内部主 executable 与 sidecar 仍使用技术 `package`，不会因展示名称变化而破坏定位契约。

每个正式安装包还会在签名和归档前写入 `nexora-release.json` 与可选 `notes.md`。macOS 位于
`.app/Contents/Resources`，Windows 位于主 EXE 同级目录，因此初始 Setup 和 update ZIP 携带
同一份发布身份。元数据 schema 1 包含 `app_key`、`app_id`、`display_name`、`package`、
`version`、`build_number`、`channel`、`target`，以及日志的文件名、字节数和 SHA-256；不包含
任何私钥、对象存储凭据或 token。

业务代码无需保留 updater 配置即可读取当前发布身份：

```rust
let info = nexora::desktop::application_info(cx);
let build_number: Option<u64> = info.build_number();
```

正式产物以通过校验的 `nexora-release.json` 为准。普通 `cargo run` 或测试找不到该文件时，名称和
版本回退 `ApplicationOptions`，app ID、build number 和 channel 返回 `None`；文件存在但非法时
启动失败。安装 updater 时，其 app ID、version、build number 和 channel 必须与通用元数据一致。

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

[publish.targets.rustfs.channels.beta]
endpoint = "http://192.168.0.250:9000"
public_base_url = "http://192.168.0.250:9000/desktop-releases"
allow_insecure_http = true

[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "示例桌面应用"
publish_target = "rustfs"
object_prefix = ""

[apps.desktop.branding]
application_logo = "assets/logos/desktop/logo-icon-128.png"
icon_source = "assets/logos/desktop/logo-icon-source.png"
managed = true

[apps.desktop.release]
default_channel = "stable"
version = "${CARGO_PKG_VERSION}"
build_number = "${BUILD_DATETIME}"
minimum_supported_version = "0.0.0"
notes = "docs/releases/1.2.3/zh-CN.md"
signing_key_file = ".secrets/desktop-update.key"

[apps.desktop.release.channels.beta]

[apps.desktop.release.channels.stable]

[apps.desktop.updater]
enabled = true
check_on_launch = true
channels = ["stable", "beta"]
trusted_public_keys = ["desktop-main:ed25519:BASE64_PUBLIC_KEY"]
signing_key_env = "DESKTOP_UPDATE_SIGNING_KEY"
check_interval = "15m"
check_jitter = "1m"
offline_grace_period = "24h"
mandatory_restart_delay = "15m"
health_timeout = "2m"

[apps.desktop.platforms.macos]
icon = "assets/logos/desktop/logo-icon.icns"
signing = "developer_id"
notarize = true
expected_team_id = "ABCDE12345"

[apps.desktop.platforms.windows]
icon = "assets/logos/desktop/logo-icon.ico"
publisher = "Example Publisher"
signing = "none"
desktop_shortcut_default = false
start_menu_shortcut_default = true
launch_after_install_default = true
minimum_windows_build = 15063

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
| `channels.<channel>` | 否 | 按字段覆盖该 channel 的 target；省略字段继承基础 target | 无 | 否 | 合并后的完整 target 统一校验 |

发布凭据不写入 TOML。每个字段按当前 channel 独立查找
`NEXORA_PUBLISH_<CHANNEL>_<FIELD>`、`NEXORA_PUBLISH_<FIELD>`、`AWS_<FIELD>`。例如 beta 可用
`NEXORA_PUBLISH_BETA_ACCESS_KEY_ID` 搭配 `NEXORA_PUBLISH_SECRET_ACCESS_KEY`；空值继续回退。
Access Key 与 Secret Key 必须最终找到，Session Token 可选。`RUSTFS_*` 已移除。bucket 必须提前
创建，并允许 `public_base_url` 下的对象匿名下载；生产 endpoint 与下载地址都必须使用 HTTPS。

### app、品牌与 release

其中 release identity 支持的配置参数是 `release.version` 与 `release.build_number`；下表同时列出
动态表达式和继续兼容的显式值形式。

| 字段 | 必填 | 作用、来源与示例 | 默认值 | 秘密 | 配置错误行为 |
| --- | --- | --- | --- | --- | --- |
| `apps.<app_key>` | 是 | CLI 稳定 app key，如 `desktop`；进入对象路径 | 无 | 否 | 缺 app 或 key 不安全时失败 |
| `package` | 是 | Cargo package 名，从 `cargo metadata` 取得 | 无 | 否 | package 不存在时 build 失败 |
| `app_id` | 是 | 永久 bundle identifier，如 `com.example.desktop` | 无 | 否 | 格式无效或冲突时失败 |
| `display_name` | 是 | 用户可见名称和外部分发文件 stem；支持合法 Unicode | 无 | 否 | 分隔符、Windows 禁止字符/设备名、NUL、尾随点或空格失败 |
| `publish_target` | 是 | 引用 `publish.targets` 的名称 | 无 | 否 | 目标不存在时失败 |
| `object_prefix` | 是 | 可选额外对象前缀；`""` 表示不增加前缀 | 无 | 否 | 非空值包含不安全路径段时失败 |
| `branding.application_logo` | 是 | 应用内 PNG，通常为 128px | 无 | 否 | 文件不存在或非 PNG 时失败 |
| `branding.icon_source` | 是 | 图标生成源 PNG | 无 | 否 | 文件不存在、格式/尺寸/透明通道无效时失败 |
| `branding.managed` | 否 | 允许 `icons generate` 重建已有输出 | `false` | 否 | `false` 且输出已存在时拒绝覆盖；可显式 `--force` |
| `release.channel` | 单通道时是 | 兼容的单发布通道，如 `stable`；不能与 `release.channels` 同时使用 | 无 | 否 | 冲突或不属于 updater channels 时失败 |
| `release.default_channel` | 多通道时是 | 交互菜单默认勾选的 channel，如 `stable` | 无 | 否 | 不存在于 `release.channels` 时失败 |
| `release.channels.<name>` | 多通道时是 | 声明 `stable`、`beta` 等 channel，可覆盖 version、build number、minimum version 与 runtime config | 无 | 否 | 名称无效或静态配置冲突时失败 |
| `release.version` | 是 | 完整字段 `${CARGO_PKG_VERSION}`，或显式 SemVer 如 `"1.2.3"` | 无 | 否 | 未知/片段表达式或非 SemVer 失败 |
| `release.build_number` | 是 | 完整字段 `${BUILD_DATETIME}`，或显式正整数如 `42` | 无 | 否 | 未知字符串、0 或溢出失败 |
| `release.minimum_supported_version` | 否 | 低于该版本时进入强制更新门禁 | `"0.0.0"` | 否 | 非 SemVer 失败 |
| `release.notes` | updater 启用时是 | 本次发布的 UTF-8 Markdown，路径相对仓库根目录；channel 可覆盖 | 无 | 否 | 缺失、越界、非普通文件、超过 1 MiB 或非 UTF-8 时 build 失败 |
| `release.signing_key_file` | 否 | 更新签名私钥文件，相对根目录或绝对路径 | 未配置 | **是（文件内容）** | 见下方严格优先级 |

`${CARGO_PKG_VERSION}` 通过 `cargo metadata --no-deps --format-version 1` 读取所选 app 的
`package`，因此同时支持 package 自有 `version` 和 `version.workspace = true`；它不是 workspace
根名称或 Nexora CLI 自身版本。`${BUILD_DATETIME}` 使用构建机器本机时区的 24 小时制
`yyMMddHHmmss`，并在同秒重建、时钟回拨、夏令时回拨或时区变化时取
`max(当前本机时间值, 上次本地构建号 + 1)`。这两个表达式都必须占满字段，不提供
任意环境变量插值或通用模板引擎。显式 SemVer 与正整数继续兼容。

`nexora icons generate --app <key>` 只消费所选 app 的品牌路径。构建把 ICNS 复制到
`.app/Contents/Resources` 并写入 `CFBundleIconFile`，不会修改 Cargo manifest。DMG 文件及其
挂载卷不设置软件品牌图标，保留系统默认外观，也不增加 `dmg_icon` 一类重复配置。

### 更新日志冻结、发布与展示

`release.notes` 相对仓库根目录解析，channel 的同名字段覆盖根 release 设置。启用 updater 时，
缺失、非普通文件、越出仓库、不可读、空文件、无效 UTF-8 或超过 1 MiB 都会在打包前失败；
未启用 updater 的应用可以省略。build 只把所选内容冻结到
`dist/<app>/<channel>/<version>/<build_number>/notes.md`，全部 target 复用同一份字节，publish
只读取该冻结文件，不回读源文档。

available manifest 同时签名 `notes_url`、`notes_sha256` 和 `notes_size`。旧 manifest 缺少摘要或
大小时仍可检查并安装，但新客户端不会下载或渲染其中的 URL。远程日志仅在 manifest 已验签、
URL 符合传输策略、大小和 SHA-256 一致且正文为安全 UTF-8 Markdown 后交给
`TextView::markdown`；失败只影响日志展示，不阻止更新。

更新确认 Dialog 首次点击“查看更新日志”才异步获取正文，并保留“稍后/后台下载/立即更新”原有
行为；强制更新仍不可关闭或绕过。sidecar 替换成功后的健康启动会在首个主窗口上展示一次安装包
内日志，普通启动、首次安装和后续重启不会重复弹出。本地日志损坏不影响健康确认或回滚判断。

### 旧项目迁移

已有 updater 项目必须在每个实际发布配置增加 Markdown 路径，然后重新执行 build：

```toml
[apps.desktop.release]
notes = "docs/releases/current/zh-CN.md"
```

若各通道内容不同，在 `[apps.desktop.release.channels.beta]` 等表中覆盖 `notes`。旧的
`docs/changelog/components/<version>/<package>/zh-CN.md` 不再被隐式读取；先把文件移动或直接把
新字段指向现有文件。receipt schema 已升级，旧 `dist/<app>/<channel>/release.json` 不会被猜测
修复，应为该 release 重新 build，再 publish。不要手工编辑安装包内的发布元数据或冻结日志。

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
| `targets.required` | 否 | 兼容旧配置的 target 列表；新项目通常省略 | `rustc -vV` 的 host | 否 | 重复或不支持 target 时失败 |
| `platforms.macos.icon` | 是 | 已生成的 `.icns` | 无 | 否 | 文件缺失或 ICNS 无效时失败 |
| `platforms.macos.signing` | 是 | `none`、`ad_hoc` 或 `developer_id` | 无 | 否 | 未知值失败；生产应为 `developer_id` |
| `platforms.macos.notarize` | 是 | 是否提交 Apple notarization | 无 | 否 | `true` 且非 Developer ID 时失败 |
| `platforms.macos.expected_team_id` | 否 | sidecar 安装前要求的新 bundle Team ID | 未配置 | 否 | 不匹配时拒绝安装；ad-hoc 本地验证应省略 |
| `platforms.windows.icon` | 是 | 主 EXE 与 Inno Setup EXE 使用的 ICO | 无 | 否 | 文件缺失或 ICO 无效时失败 |
| `platforms.windows.publisher` | Windows 是 | 安装器发布者和版本资源公司名 | 无 | 否 | Windows 构建缺失时失败 |
| `platforms.windows.signing` | 否 | `none` 跳过 Authenticode；`authenticode` 签署并验证 Windows 文件身份 | `none` | 否 | 未知值失败；公开生产发布建议使用 `authenticode` |
| `platforms.windows.signing_thumbprint` | Authenticode 时是 | 当前用户 `My` 证书存储中的 40 位 SHA-1 证书指纹；也可由 `WINDOWS_SIGN_CERTIFICATE_SHA1` 注入 | 未配置 | 否 | `none` 模式配置该字段，或格式无效时构建失败 |
| `platforms.windows.expected_publisher` | 否 | updater 期望的 signer 证书 SimpleName；省略时使用 `publisher` | `publisher` | 否 | `none` 模式配置、显式空值或运行时不匹配时失败 |
| `platforms.windows.timestamp_url` | Authenticode 时是 | RFC 3161 时间戳服务 URL | 未配置 | 否 | `none` 模式配置、缺失或空值时构建失败 |
| `platforms.windows.desktop_shortcut_default` | 否 | 桌面快捷方式 checkbox 默认值 | `false` | 否 | 非布尔值无法解析 |
| `platforms.windows.start_menu_shortcut_default` | 否 | 开始菜单快捷方式 checkbox 默认值 | `true` | 否 | 非布尔值无法解析 |
| `platforms.windows.launch_after_install_default` | 否 | 完成页立即运行 checkbox 默认值 | `true` | 否 | 非布尔值无法解析 |
| `platforms.windows.minimum_windows_build` | 否 | 安装时检查的最低 Windows build | `15063` | 否 | 低于 GPUI 基线时失败 |
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

四类凭据不可混淆：RustFS/S3 AK/SK 只授权上传对象；Ed25519 私钥签署更新清单，公钥内置于
客户端验证来源；Apple Developer ID 证书签署 macOS 代码身份并配合 notarization/Gatekeeper；
Windows Authenticode 证书签署主 EXE、updater EXE 与 Setup EXE。它们互不替代；发布私钥和证书私钥都
不能进入客户端、仓库或日志。

## 签名与验证链路

publish 使用私钥签署 canonical JSON manifest payload，并把签名信封作为 `latest.json` 上传；
客户端只接受内置公钥验证通过的清单。随后客户端与 sidecar 都验证 app ID、channel、version、
build number、target 和 release 状态，并对下载的更新归档做长度与 SHA-256 校验。macOS
Developer ID 发布还验证新 bundle 的代码签名与 `expected_team_id`。Windows 始终验证 ZIP
路径安全以及主程序和 updater 的 PE 架构；仅在 `signing = "authenticode"` 时进一步通过
`WinVerifyTrust`、证书 thumbprint 和 publisher 验证两个 EXE。sidecar 在真正替换前会再次独立
完成清单、artifact、应用身份和平台代码签名验证，不能信任主进程传入的 URL 或 hash。

SHA-256 只证明下载内容与已签清单一致，不证明发布者身份；更新发布授权来自 Ed25519 清单
签名，macOS 可执行身份来自 Developer ID 与 Apple 信任链，Windows 可执行身份在启用时来自
Authenticode 与 Windows 信任链。

### Windows Authenticode 策略

本地开发、示例和受控内网测试可以显式使用：

```toml
[apps.desktop.platforms.windows]
publisher = "Example Publisher"
signing = "none"
```

该模式仍强制执行 Ed25519 manifest 验签、sequence 防重放、artifact size/SHA-256、ZIP 安全和
PE 架构验证，只跳过 Authenticode。`signing_thumbprint`、`expected_publisher` 或 `timestamp_url`
不能残留在该模式中，否则构建立即失败；全局存在的 `WINDOWS_SIGN_CERTIFICATE_SHA1` 会被忽略。

公开生产发布建议使用：

```toml
[apps.desktop.platforms.windows]
publisher = "Example Publisher"
signing = "authenticode"
signing_thumbprint = "00112233445566778899AABBCCDDEEFF00112233"
expected_publisher = "Example Publisher"
timestamp_url = "https://timestamp.example.com"
```

构建会签署并验证主程序、updater 和 Setup EXE；应用内更新会拒绝未签名、证书链无效、
thumbprint 不匹配或 publisher 不匹配的主程序和 updater。自签名证书可用于受控测试，但外部
Windows 设备默认不会信任它。

Windows 构建分别为主程序和 updater 编译 UTF-8/Unicode PE VERSIONINFO。主程序的
`FileDescription`/`ProductName` 是 `display_name`，`InternalName` 和 `OriginalFilename` 保持
技术 package；updater 的描述追加“更新程序”，内部名和原文件名使用 `<package>-updater`。
资源写入后才执行 Authenticode 签名，避免签名后修改 EXE。

主窗口原生 title 的优先级依次是应用显式 `WindowOptions.titlebar.title`、安装元数据中的
`display_name`、开发模式的 `ApplicationOptions::application_name`。默认登录页与自定义
`LoginFeature` 都由 Shell 提供一组 `gpui-component::TitleBar` 窗口控制；独立使用
`LoginGate` 时保留其默认 TitleBar，不需要应用自制关闭、最小化或最大化按钮。

## 接入公共 updater 与 sidecar

应用从当前安装目录读取构建时嵌入的安全配置：macOS 位于
`.app/Contents/Resources/nexora-updater.json`，Windows 位于主 EXE 同级。应用在
`Application::initialize` 安装一次：

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

同一个应用二进制需要同时支持 `updater.enabled = false` 的本地或私有构建时，使用
`from_current_bundle_if_present()`。它只在配置文件缺失时返回 `None`；文件存在但无效时
仍会报错，不会绕过公钥、传输或签名约束。

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

Windows 应用内更新不要求管理员权限。框架会在用户所选安装目录的父目录创建隐藏事务根
`<install-parent>/.nexora-updater/<app_id>`，让 staging、pending、backup、健康状态和安装结果与
安装目录始终位于同一卷；事务目录不会放进待替换的应用目录。用户点击立即重启前，框架会先
检查当前目录、暂存目录、PE 入口、同卷关系以及父目录的创建和重命名权限，预检失败时保持主
程序运行并显示错误，不会先退出再静默失败。

“稍后重启”使用同步后的临时文件和 Windows 原子替换提交 `pending.json`，已有待安装版本也可
安全覆盖。记录一旦提交，后续目录同步只能作为尽力而为的耐久性增强，不能再把已提交 payload
移回 staging。sidecar 在替换或健康确认失败时会停止失败的新进程、恢复旧版本、先写入受限的
用户可见失败结果，再重新打开旧版本；下次启动通过现有 Notification 显示该结果。安装到当前
用户不可写的位置会在预检阶段失败，此时应重新选择可写安装路径，而不是把 updater 整体提权。

## build、publish 与第一次升级验证

`nexora build` 在任何 target 构建前原子写入
`dist/<app>/<channel>/release.json`。收据记录 schema、app key、package、channel、最终
version/build number、两项来源、创建 Unix 秒和本次 targets；同一次 build 的全部 target 共用
该身份。构建中途失败时，只要收据仍与当前配置匹配且 target 未全部完成，重试会复用原构建号；
全部 target 的 artifact 已完整后再次显式 build，动态构建号会严格增大，旧版本化产物不会删除。
损坏或不支持的收据会在构建前失败，不从目录名猜测身份。

`nexora build` 只构建本地平台产物、sidecar、bundle 配置、每个发布产物的 `.sha256` 和
`artifact.json`，不访问对象存储。Windows 产物为 branded Inno Setup EXE 与更新 ZIP；两者来自
同一个 staging，旁车内容使用小写 SHA-256、两个空格、完整 branded 文件名和 LF 换行。版本、
构建号、图标、updater 配置和 sidecar 全部写入后才签名。

`nexora publish`（包括 yank）只从当前 release receipt 读取 version/build number，不重新计算
本机时间，也不会隐式 build。它校验收据与 app、package、channel、Cargo version 和当前配置
一致，并按收据冻结的 targets 逐个验证 `artifact.json` 身份、文件存在性、大小与 SHA-256。dry-run
执行相同的本地与远端预检，但不写本地或远端；available identity 必须严格高于远端
`(version, build_number)`。

第一次发布：

```bash
nexora build --app desktop --channel stable
nexora publish --app desktop --dry-run
nexora publish --app desktop
```

配置多个 `release.channels` 后，真实终端中省略 `--channel` 会出现多选菜单并默认勾选
`default_channel`；非交互 CI 应显式传可重复的 `--channel` 或 `--all-channels`。

publish 会读取并验签远端 `latest.json`；404 代表 sequence 1，否则使用远端 sequence 加一。
它先上传并匿名校验所有 versioned 不可变产物、checksum 与 release notes；并发 sequence 再检查
通过后，更新 channel 根 branded 产物和 checksum，再上传 sequence manifest，最后上传并回读
`latest.json`。updater manifest 的 URL 始终指向 versioned update payload，不会指向下次发布会
覆盖的 channel 根文件。

`latest.json` 是签名更新清单，继续保留且最后更新。新的发布不再生成 `latest.dmg`、
`latest-<arch>.dmg`、`latest.exe`、`latest.zip` 等安装包/负载 alias。对象布局为：

```text
[<object_prefix>/]<app_key>/<channel>/latest.json
[<object_prefix>/]<app_key>/<channel>/manifests/<sequence>.json
[<object_prefix>/]<app_key>/<channel>/<display_name>-<arch><suffix>
[<object_prefix>/]<app_key>/<channel>/<version>/<build>/<arch>/<display_name>-<arch><suffix>
```

例如 `object_prefix = ""`、app key 为 `imes` 时，当前下载地址可以是
`.../imes/nightly/iMES-aarch64.dmg`，不可变地址是
`.../imes/nightly/1.2.3/260805120000/aarch64/iMES-aarch64.dmg`。公开架构目录只使用
`x86_64`/`aarch64`，不使用完整 Rust target triple，也没有 `releases` 中间目录。
`app_key` 决定 feed 与远端目录身份；修改 `display_name` 只改变用户可见名称和新 artifact 文件名。

升级发布端后，管理员可以手工清理旧 channel 根的 installer `latest.*` alias，但 Nexora 不会
自动删除任何远端对象。不要删除旧 versioned immutable 对象，否则仍引用它们的旧客户端 manifest
会失效。

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
