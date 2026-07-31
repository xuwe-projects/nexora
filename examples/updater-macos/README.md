# macOS 更新程序示例

这个独立 workspace 验证 Nexora 的首次 DMG 安装、签名 `latest.json`、`.app.zip` 自更新、sidecar 替换、健康确认和失败回滚。应用安装后的可见名称是“macOS 更新程序示例”；Cargo package 与远端技术产物名仍为 `nexora-updater-macos-example`。

## 一次性准备

```bash
cp nexora.toml.example nexora.toml
nexora updater keygen --app updater-macos \
  --private-key-file .secrets/updater-macos.key
```

把命令输出的公钥写入 `trusted_public_keys`。`.secrets/` 已被 Git 忽略；私钥只由 `nexora publish` 读取，不会进入应用 bundle。RustFS/S3 凭据只属于开发者发布环境，例如：

```bash
set -a
source ./rustfs.env
set +a
```

最终用户不需要设置任何环境变量。

## 构建与发布

每次发布只修改 `nexora.toml` 的：

```toml
[apps.updater-macos.release]
version = "1.0.1"
build_number = 2
```

然后运行：

```bash
nexora build
nexora publish --dry-run
nexora publish
```

单 app 会自动选择。`build` 只在本地生成 DMG、`.app.zip` 和多产物 `artifact.json`；`publish` 只校验并发布既有产物，不会隐式构建。`manifest_sequence` 不写入配置，由 publish 验签远端 `latest.json` 后自动取远端 sequence 加一；首次发布为 1。

首次安装永久链接单 target 的 `latest.dmg`。应用内更新读取签名 `latest.json`，并且只下载 `.app.zip`，不会下载或运行 DMG。

旧流程生成的基础版本如果没有 bundle updater 配置或 sidecar，需要先从新 DMG 重新安装一次。此后应用显示的当前 version/build 来自自身 bundle，不来自 Cargo package 固定版本，也不来自服务器 latest 版本。

## 真实验收

先以 `1.0.0` / build `1` 执行 `nexora build` 并安装 DMG，再改为 `1.0.1` / build `2`，依次执行 build、publish dry-run 和 publish。启动已安装的旧版本，确认应用会静默检查更新，并在检测到新版本后自动弹出确认框；可选更新应提供“立即更新”“后台下载”和“稍后”，强制更新不得提供跳过入口。继续确认 sidecar 替换后仍保留“macOS 更新程序示例”的安装路径与显示名称，并在健康失败注入时回滚。
