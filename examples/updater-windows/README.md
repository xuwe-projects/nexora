# Windows 更新程序示例

这个独立 workspace 用来验证 Nexora 的 Windows x86_64 首次安装和自动更新链路。首次安装产物是 NSIS 3 Unicode 生成的版本化 `setup.exe`，应用内自动更新只下载并应用 `windows.zip`，不会下载或运行 setup，也不会生成 MSI。

示例使用 `assets/logos/updater-windows/` 下的品牌资源，并通过 `nexora::desktop::install_updater` 安装公共 updater。应用页面不复制更新配置、弹窗或状态机；检查更新入口来自 `nexora::desktop`。

## 一次性准备

```powershell
Copy-Item nexora.toml.example nexora.toml
nexora updater keygen --app updater-windows `
  --private-key-file .secrets/updater-windows.key
```

把命令输出的公钥写入 `trusted_public_keys`。`.secrets/` 已被 Git 忽略；私钥只由 `nexora publish` 本地读取，不会进入应用 bundle。

Windows 实机构建还需要：

- NSIS 3 Unicode，并确保 `makensis.exe` 在 `PATH` 中。
- Windows SDK，并确保 `rc.exe`、`signtool.exe` 在 `PATH` 中。
- 如启用 `signing = "authenticode"`，当前用户证书存储中需要可用于签名的证书，并配置 `signing_thumbprint` 或环境变量 `WINDOWS_SIGN_CERTIFICATE_SHA1`。
- 与证书匹配的 `publisher`、`expected_publisher` 和 RFC 3161 `timestamp_url`。

## 构建与发布

每次发布只修改 `nexora.toml` 的版本与 build：

```toml
[apps.updater-windows.release]
version = "1.0.1"
build_number = 2
```

然后运行：

```powershell
nexora icons generate --app updater-windows
nexora build --app updater-windows
nexora publish --app updater-windows --dry-run
nexora publish --app updater-windows
```

`build` 只在本地生成版本化 setup EXE、`windows.zip`、release notes 和 `artifact.json`，不访问 S3。`publish` 只校验并发布已有产物，不隐式构建。`manifest_sequence` 不写入配置，由 publish 验签远端 `latest.json` 后自动取远端 sequence 加一，首次发布为 1。

## 真实验收

- `signtool verify /pa` 通过主 EXE、updater EXE 和 setup EXE。
- 产物目录没有 `.msi`，`artifact.json` 只包含 `windows_setup_exe` 与 `windows_update_zip`。
- 首次安装入口是版本化 setup EXE；应用内更新只下载 `windows.zip`。
- 安装目录、Apps & Features、开始菜单快捷方式和可选桌面快捷方式使用示例 ICO。
- 自更新后保留 `%LOCALAPPDATA%\Programs\com.nexora.examples.updater-windows\` 安装目录、显示名称和品牌图标。
- 设置 `NEXORA_EXAMPLE_HEALTH_FAILURE=before-health` 构建新版本后，安装应在健康超时前回滚到旧版本。
- 从 Apps & Features 卸载时删除安装文件、快捷方式和注册表项，但不删除用户业务数据、配置和日志。
