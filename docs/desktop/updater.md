# 桌面自动更新

Nexora 使用签名 `latest.json`、匿名 `.app.zip` 下载和独立 sidecar 完成应用内更新；首次安装使用 DMG。私钥与 S3/RustFS 凭据仅存在于开发者发布环境，最终用户不配置更新环境变量。

## 配置

仓库根目录 `nexora.toml` 是 build/publish 的唯一项目配置：

```toml
schema_version = 1

[publish.targets.rustfs]
provider = "s3"
endpoint = "http://127.0.0.1:9000"
bucket = "desktop-releases"
region = "us-east-1"
force_path_style = true
public_base_url = "http://127.0.0.1:9000/desktop-releases"
allow_insecure_http = true

[apps.desktop]
package = "desktop"
app_id = "com.example.desktop"
display_name = "示例桌面应用"
publish_target = "rustfs"
object_prefix = "products"

[apps.desktop.release]
channel = "stable"
version = "1.2.0"
build_number = 12
minimum_supported_version = "0.0.0"
signing_key_file = ".secrets/desktop-update.key"

[apps.desktop.updater]
enabled = true
feed_url = "http://127.0.0.1:9000/desktop-releases/products/desktop/stable/latest.json"
channels = ["stable"]
trusted_public_keys = ["desktop-main:ed25519:BASE64_PUBLIC_KEY"]
signing_key_env = "DESKTOP_UPDATE_SIGNING_KEY"
check_interval = "15m"
check_jitter = "1m"
offline_grace_period = "24h"
mandatory_restart_delay = "60s"
health_timeout = "20s"

[apps.desktop.targets]
required = ["aarch64-apple-darwin"]

[apps.desktop.platforms.macos]
signing = "ad_hoc"
notarize = false
```

`package` 是 Cargo、cargo-bundle 原始路径和技术产物名；`display_name` 是 Info.plist、DMG 卷与安装后 `.app` 的用户可见名称。release channel 必须属于 updater channels，version 必须是 SemVer，build number 必须大于零。

私钥优先读取相对于 `nexora.toml` 的 `release.signing_key_file`。未配置或为空时才读取 `signing_key_env` 指向的环境变量；明确配置但文件不存在时直接失败，不回退。

## 命令

```bash
nexora build
nexora publish --dry-run
nexora publish
```

单 app 自动选择；多 app 交互终端显示 `display_name（app key / package）` 菜单，非交互环境必须提供 `--app`，发布全部 app 必须显式 `--all`。非交互真实发布还必须提供 `--yes`。

build 不访问对象存储，按 required targets 构建主程序和 `<executable>-updater` sidecar，写入 bundle 配置与 Info.plist，完成签名后同时生成技术名 `.app.zip`、DMG 和 `artifact.json`。publish 不执行 build，并要求每个 required macOS target 同时具备 ZIP 与 DMG。

## Manifest sequence 与远端对象

开发者不维护 `manifest_sequence`。远端 `latest.json` 为 404 时本次 sequence 为 1；否则 publish 必须先验签并使用远端 sequence 加一。dry-run 同样读取远端但不写入。写 mutable 对象前会再次检查 sequence，变化则拒绝并发覆盖。

版本化 ZIP、DMG 与 sequence manifest 不可覆盖并使用 immutable 缓存：

```text
<prefix>/<app>/<channel>/releases/<version>/<build>/<target>/<technical-name>.app.zip
<prefix>/<app>/<channel>/releases/<version>/<build>/<target>/<technical-name>.dmg
<prefix>/<app>/<channel>/manifests/<sequence>.json
```

每个 target 还有 no-cache 的 `latest-<arch>.dmg`；只有单 target 时额外发布 `latest.dmg`。`latest.json` 始终最后上传。Updater manifest 只包含 `.app.zip`，不包含 DMG。

客户端从 bundle 的 updater 配置读取当前 `(version, build_number)`，服务器 `latest.json` 只代表最新版本。版本比较使用 `(version, build_number)`；sequence 只用于清单防重放。

## 启动检查与用户确认

主窗口创建完成后，应用静默读取并验证 `latest.json`。没有新版本或启动检查失败时不打断用户；发现可选更新后才打开窗口级确认框，并提供“立即更新”“后台下载”和“稍后”。只有用户选择前两项后才会下载更新包；后台下载完成后会再次弹出重启确认。低于 `minimum_supported_version` 的强制更新不提供“稍后”或关闭入口。

用户主动点击“检查更新”时会立即显示检查进度；后续确认、下载、校验、暂存和重启流程与启动检查一致。同一进程只保留一个更新协调器，避免启动检查和手动检查并发下载同一版本。
