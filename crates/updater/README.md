# Updater

`updater` 为 workspace 中的桌面应用提供更新检查、下载进度、SHA-256 校验、macOS
代码签名验证、更新暂存和重启后原位替换能力。首版自动安装只支持 macOS。

每个更新通道使用独立的 `latest.json`：

```json
{
  "schema_version": 1,
  "app_id": "com.nexora.console",
  "channel": "stable",
  "version": "0.2.0",
  "bundle_version": 12,
  "notes_url": "./notes/0.2.0/console/zh-CN.md",
  "artifacts": [
    {
      "target": "aarch64-apple-darwin",
      "url": "./console-0.2.0-12-aarch64.app.zip",
      "sha256": "填写安装包的 SHA-256",
      "size": 67108864
    },
    {
      "target": "x86_64-apple-darwin",
      "url": "./console-0.2.0-12-x86_64.app.zip",
      "sha256": "填写安装包的 SHA-256",
      "size": 68157440
    }
  ]
}
```

更新包必须使用 macOS `ditto` 生成，以保留 `.app` 的签名和资源属性：

```bash
ditto -c -k --keepParent Console.app Console.app.zip
shasum -a 256 Console.app.zip
```

版本比较使用 `(version, bundle_version)`：版本更高时更新；版本相同时，构建号更高时更新。
稳定版、Beta 和 Nightly 应分别部署自己的 `latest.json` URL。

## 桌面应用接入

桌面应用只需要依赖 workspace 的 `updater`，创建配置并在按钮回调中打开公共弹窗：

```rust,ignore
let config = updater::UpdateConfig::new(
    "https://updates.example.com/console/stable/latest.json",
    "com.nexora.console",
    env!("CARGO_PKG_VERSION"),
    12,
    updater::UpdateChannel::Stable,
)?
.with_expected_team_id("APPLE_TEAM_ID");

updater::open_update_dialog(config, window, cx);
```

`latest.json` 由 `nexora build` 根据 `.app.zip`、SHA-256、文件大小和
`changelogs/<version>/<component>/<locale>.md` 自动生成；仓库中的 example 只描述协议形状。
默认使用相对 URL，发布时把安装包、校验文件、`notes/` 和 `latest.json` 上传到同一个 OSS 前缀即可。
本机 macOS 可以用 `nexora build --targets macos` 生成 Apple Silicon 和 Intel 两个 artifact；
Windows 与 Linux 产物应由对应系统的 runner 构建后再合并到最终清单。

Console 示例通过编译时环境变量配置清单地址和可选签名团队：

```bash
UPDATE_MANIFEST_URL=https://updates.example.com/console/stable/latest.json \
BUNDLE_VERSION=12 \
MACOS_TEAM_ID=APPLE_TEAM_ID \
cargo build --release -p console
```

未设置 `UPDATE_MANIFEST_URL` 时，Console 会关闭在线更新入口，适用于本地开发和不提供
更新能力的部署环境。

仓库根目录 `config/desktop-build.env.example` 提供了构建期环境变量样例，
`config/updater/latest.example.json` 提供了更新清单样例。
