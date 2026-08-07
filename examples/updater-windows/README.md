# Windows 更新程序示例

这个独立 workspace 用于真实验收 Windows x64/ARM64 的首次安装、签名 manifest 更新、sidecar
事务替换、健康确认和失败回滚。首次安装介质是简体中文 Inno Setup EXE；应用内更新只使用
`windows.zip`，绝不会重新运行 Inno Setup。默认安装范围是当前用户，不要求 UAC，目录位于
`%LOCALAPPDATA%\Programs\Nexora Examples\Windows 更新程序示例`，同时保留目录选择、开始菜单、
桌面快捷方式和安装完成后启动选项。

完整依赖版本、官方地址、人工安装命令和证书边界见
[桌面自动更新文档](../../docs/desktop/updater.md#人工安装构建依赖)。Nexora CLI 只检测这些工具；
不会执行 `winget install`、`rustup target add` 或任何下载/安装命令。

## 一次性准备

1. 人工安装 Rustup、目标 Rust target、Visual Studio Build Tools、Windows SDK 和兼容的 Inno
   Setup。Inno 支持 `>= 6.7.3, < 8.0.0`，新安装推荐 7.x。
2. 关闭并重新打开终端，运行 `nexora doctor`；必需项全部通过后再继续。
3. 准备一个 S3 兼容测试 bucket。客户端必须能匿名读取，发布凭据只存在于当前 shell 或秘密
   系统，不得写入仓库。
4. 复制配置并生成 Ed25519 更新签名密钥：

```powershell
Copy-Item nexora.toml.example nexora.toml
nexora updater keygen --app updater-windows `
  --private-key-file .secrets/updater-windows.key
```

把输出公钥写入 `trusted_public_keys`。`.secrets/` 已被 Git 忽略；私钥只由 publish 读取，不进入
应用、安装包或日志。两个验收版本必须使用相同 `app_id`、channel、feed URL 和受信公钥。

## 默认签名策略

`nexora.toml.example` 默认使用 `signing = "none"`，因此本地功能验收不需要 Authenticode 证书。
该模式只跳过 Windows 代码身份验证，仍强制执行：

- Ed25519 `latest.json` 签名与 manifest sequence 防重放；
- app ID、channel、target、version 和 build number 校验；
- artifact 文件大小和 SHA-256；
- ZIP 绝对路径、`..`、symlink/逃逸与条目分隔符检查；
- 主程序与 updater sidecar 的 PE 架构检查。

`signing = "none"` 不得残留 `signing_thumbprint`、`expected_publisher` 或 `timestamp_url`。环境中
存在 `WINDOWS_SIGN_CERTIFICATE_SHA1` 也不会自动开启签名。

## 两版本验收身份

示例配置的基线 A 是 `version = "1.0.1"`、`build_number = 5`。正常更新 B 可临时使用
`version = "1.0.2"`、`build_number = 6`；回滚注入版本 C 可使用 `version = "1.0.3"`、
`build_number = 7`。开始前先确认测试 channel 尚未使用这些身份；如已使用，选择更高且严格
递增的 `(version, build_number)`。不要复用远端已发布身份。

每次 build 后保留并核对：

```text
dist/updater-windows/<channel>/release.json
dist/updater-windows/<channel>/<version>/<build>/<target>/artifact.json
<setup>.exe
<update>.windows.zip
<artifact>.sha256
```

## 构建并真实安装版本 A

```powershell
nexora doctor
nexora icons generate --app updater-windows
nexora build --app updater-windows --channel stable
```

预检失败时立即停止：不要安装缺失工具，也不要把未执行的步骤算作通过。确认失败前没有新增
release receipt、staging 或 dist 构建状态；按诊断人工安装后重跑原始 build。

构建成功后，从 `artifact.json` 定位 Setup EXE 和更新 ZIP，核对 EXE、ZIP、`.sha256` 与 metadata
均存在。双击真实 Setup EXE，并逐项验收：

1. 不出现 UAC，安装范围为当前用户。
2. 默认目录位于 LocalAppData Programs，安装目录选择页可用。
3. 开始菜单和桌面快捷方式选项可切换。
4. 完成页“安装完成后运行应用”可切换且行为一致。
5. 从安装目录或安装器创建的快捷方式启动，不从 Cargo target/staging 启动。
6. 安装目录同时存在主 EXE、`nexora-updater.json`、`nexora-release.json` 和 updater sidecar。
7. 应用完成 updater 初始化并在首个主窗口创建后报告健康。
8. Apps & Features 的名称、图标和卸载项正确；卸载不删除用户业务数据、配置或日志。

## 发布版本 B 并验证正常自动更新

把 `nexora.toml` 临时改为 B 的严格更高 identity，保持 app ID、stable channel、公钥和 feed
不变，然后执行：

```powershell
nexora build --app updater-windows --channel stable
nexora publish --app updater-windows --channel stable --dry-run
nexora publish --app updater-windows --channel stable
```

从已安装的 A 检查更新并记录客户端/sidecar 日志。必须逐项确认：

1. 客户端匿名读取并验签 `latest.json`，验证 app ID、channel、target、version、build number、
   manifest sequence 和签名。
2. 用户确认后才开始下载；sidecar 独立再次下载/验签，而不是信任主进程传入的 URL 或摘要。
3. 大小、SHA-256、ZIP 路径和 PE 架构全部通过。
4. 主程序退出后 sidecar 在同卷隐藏事务目录中替换，B 从原安装目录重启并完成健康确认。
5. sidecar 精确启动 `nexora-desktop.exe`；同目录的 `unins000.exe` 不会被当作候选主程序。
6. 更新后 `unins000.exe` 与 `unins000.dat` 仍存在，Apps & Features 的原卸载入口可以正常卸载。
7. 最终运行 identity 是 B；安装目录中没有活动 staging、错误的并行安装目录或悬挂 pending。
8. 更新过程中没有启动 Inno Setup，也没有 UAC。

## 注入健康失败并验证回滚

将 identity 提升为 C，在构建 C 的终端为示例设置编译期失败注入：

```powershell
$env:NEXORA_EXAMPLE_HEALTH_FAILURE = 'before-health'
nexora build --app updater-windows --channel stable
Remove-Item Env:NEXORA_EXAMPLE_HEALTH_FAILURE
nexora publish --app updater-windows --channel stable --dry-run
nexora publish --app updater-windows --channel stable
```

从健康的 B 发起更新，确认 C 在健康报告前失败。必须验证 sidecar 识别启动失败、崩溃或健康
超时，恢复 B 的备份并从原目录重启 B；用户看到的失败原因不包含私钥、凭据或不必要的内部
路径。下一次启动应消费并显示持久化失败结果，且不留下阻止后续正常更新的锁、pending 或
staging。完成后再发布一个严格高于 C 的健康版本，确认后续更新仍可进行。

## Authenticode 单独验收

只有具备有效证书时才执行本节；没有证书必须报告“未验证”，不能用 unsigned 流程替代。
临时把配置改为：

```toml
[apps.updater-windows.platforms.windows]
publisher = "证书中的发布者名称"
signing = "authenticode"
signing_thumbprint = "40_HEX_SHA1_THUMBPRINT"
expected_publisher = "证书中的发布者名称"
timestamp_url = "https://可信 RFC3161 服务"
```

证书私钥/PFX 密码只进入当前用户证书存储或秘密系统。重新 build 后运行：

```powershell
signtool verify /pa <主程序.exe>
signtool verify /pa <updater-sidecar.exe>
signtool verify /pa <Setup.exe>
```

再重复 A → B 更新，确认 updater 对 ZIP 内两个 EXE 执行 `WinVerifyTrust` 并严格匹配 thumbprint
与 publisher。Ed25519 与 Authenticode 是两条独立信任链，任一失败都必须拒绝更新。

## target 与验收记录

省略 `--target` 时使用 `rustc -vV` 的 host：Intel/AMD Windows 为
`x86_64-pc-windows-msvc`，Windows on ARM 为 `aarch64-pc-windows-msvc`。需要显式覆盖时可传：

```powershell
nexora build --app updater-windows --channel stable --target aarch64-pc-windows-msvc
```

不要把“代码测试通过”“安装器成功编译”或“文档已补充”记录成“真实自动更新通过”。验收记录
必须分别注明：真实 Setup 安装、A → B 更新、C → B 回滚、Authenticode 是否实际完成；还要列出
因缺少人工依赖、证书或对象存储凭据而未执行的步骤。
