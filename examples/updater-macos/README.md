# macOS 更新程序示例

这个独立 workspace 验证 Nexora 的首次 DMG 安装、签名 `latest.json`、`.app.zip` 自更新、公共
更新界面、独立 sidecar、健康确认和失败回滚。可见名称是“macOS 更新程序示例”；Cargo package
与远端技术产物名仍为 `nexora-updater-macos-example`。

示例使用 `assets/logos/updater-macos/` 的 app 级品牌资源，只通过 `nexora::desktop` 安装 updater
并运行 sidecar；没有直接依赖内部 `updater` crate，也没有私有弹窗或状态机。

完整配置字段、默认值、密钥轮换、验证链路与生产签名说明见
[桌面自动更新文档](../../docs/desktop/updater.md)。

## 首次安装开发工具

```bash
xcode-select --install
rustup target add aarch64-apple-darwin
cargo install cargo-bundle
brew install create-dmg
cargo install --git https://github.com/xuwe-projects/nexora \
  --tag v0.21.2 nexora --locked --force \
  --no-default-features --features cli --bin nexora

nexora doctor
# 缺 cargo-bundle/create-dmg 时可让 CLI 安装：
nexora doctor --fix
```

`nexora doctor` 不代替 Rust、Xcode Command Line Tools 或 Homebrew 的首次安装。

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

## 第一次构建和安装 DMG

```bash
nexora doctor
nexora icons generate --app updater-macos
nexora build --app updater-macos
```

`nexora build` 只生成 `.app`、DMG、`.app.zip`、sidecar、bundle updater 配置、SHA-256 和
`artifact.json`，不访问 RustFS。打开生成的 DMG，把“macOS 更新程序示例.app”拖入
`/Applications`，再从 Finder 启动。DMG 是首次安装介质；`.app.zip` 是 sidecar 使用的更新负载；
`latest.json` 是 publish 生成的签名清单。

确认 bundle 中存在：

```text
Contents/Resources/logo-icon.icns
Contents/Resources/nexora-updater.json
Contents/Helpers/updater-macos-updater
```

## 第一次发布

保持基础版本为 `1.0.0` / build `1`，先完成 dry-run 再发布：

```bash
nexora publish --app updater-macos --dry-run
nexora publish --app updater-macos
```

`nexora publish` 只校验并上传既有 build 产物，不会隐式 build。它读取并验签远端
`latest.json`，计算新的 manifest sequence，最后才上传 mutable `latest.json`，并匿名回读所有
更新 URL。

## 从旧版本升级

把 `nexora.toml` 改为：

```toml
[apps.updater-macos.release]
version = "1.0.1"
build_number = 2
```

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
