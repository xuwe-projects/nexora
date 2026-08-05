# Windows 更新程序示例

这个独立 workspace 用来验证 Nexora 的 Windows x86_64/ARM64 首次安装和自动更新链路。首次安装同时生成 branded `.exe` 和 WiX MSI；EXE 使用 Burn 包装 MSI，并直接显示同一套简体中文 MSI 向导。应用内自动更新仍只下载并应用 `windows.zip`，不会重新运行首次安装程序。

安装向导包含安装目录、安装选项、确认、进度和完成页面。安装选项可勾选桌面快捷方式与开始菜单快捷方式；完成页可勾选立即运行应用。当前只支持不提权的当前用户安装，默认目录为 `%LOCALAPPDATA%\Programs\<app_id>`。

## 一次性准备

```powershell
Copy-Item nexora.toml.example nexora.toml
nexora doctor --fix
nexora updater keygen --app updater-windows `
  --private-key-file .secrets/updater-windows.key
```

把 keygen 输出的公钥写入 `trusted_public_keys`。`.secrets/` 已被 Git 忽略；私钥只由 `nexora publish` 本地读取，不会进入应用安装包。

交互式 `nexora build` / `nexora doctor --fix` 会自动安装 Rust target、固定 revision 的
`cargo-wix`、WiX 5.0.2 和对应扩展。没有 .NET 时会通过微软官方脚本安装用户级 .NET 10 SDK
到 `%LOCALAPPDATA%\Nexora\tools\dotnet`。缺少 Windows SDK 时会启动官方安装流程；如果安装器
要求确认或重启，完成后重新执行相同命令。Nexora 从 Windows Kits 标准目录定位 `rc.exe`、
`fxc.exe` 和 `signtool.exe`，不永久修改 PATH。非交互环境只输出完整安装命令。

如启用 `signing = "authenticode"`，当前用户证书存储中还需要可用于签名的证书，并配置 `signing_thumbprint` 或环境变量 `WINDOWS_SIGN_CERTIFICATE_SHA1`，同时配置匹配的 `publisher`、`expected_publisher` 和 RFC 3161 `timestamp_url`。

## Windows 签名策略

当前实际示例使用 `signing = "none"`，无需购买 Authenticode 证书。该模式仍会验证 Ed25519
签名的 `latest.json`、manifest sequence、artifact size/SHA-256、ZIP 路径安全和两个 EXE 的 PE
架构，只跳过 Windows 证书身份验证。不要在该模式残留 `signing_thumbprint`、
`expected_publisher` 或 `timestamp_url`，否则构建会立即指出冲突字段；全局存在的
`WINDOWS_SIGN_CERTIFICATE_SHA1` 不会让该模式自动开启签名。

面向公众发布时建议切换到 `signing = "authenticode"`。构建会签署主程序、updater、MSI 和
Setup EXE；应用内更新还会对 ZIP 中两个 EXE 执行 `WinVerifyTrust`，并严格匹配证书 thumbprint
与 publisher。只配置其中一项、EXE 未签名或证书身份不匹配都会拒绝更新。Nexora 的 Ed25519
公私钥只签署更新 manifest，不能替代 Windows 代码签名证书。

## target 选择

普通构建不需要 `[apps.updater-windows.targets]`。省略后，Nexora 使用 `rustc -vV` 返回的 host target：Intel/AMD Windows 为 `x86_64-pc-windows-msvc`，Windows on ARM 为 `aarch64-pc-windows-msvc`。

只有需要明确覆盖时才传：

```powershell
nexora build --app updater-windows --target aarch64-pc-windows-msvc
```

旧配置中的 `targets.required` 仍可读取，但不再是普通项目的必填项。交互式 build 会自动执行
缺失的 `rustup target add`；CI/非交互环境会返回准确命令并停止。

## 构建与发布

```powershell
nexora icons generate --app updater-windows
nexora doctor
nexora build --app updater-windows
nexora build --app updater-windows --channel beta
nexora publish --app updater-windows --dry-run
nexora publish --app updater-windows
```

示例同时声明 `stable` 与 `beta`。在真实终端里省略 `--channel` 时，`nexora build` 会显示
channel 多选菜单并默认勾选 `stable`；CI 或脚本应显式传 `--channel stable`、`--channel beta`
或 `--all-channels`，避免依赖非交互默认值。

`build` 只在本地生成 `<display_name>-<arch>.msi`、`<display_name>-<arch>.exe`、
`<display_name>-<arch>.windows.zip`、release notes、SHA-256 旁车文件和 `artifact.json`，不访问
S3。Windows 更新 ZIP 由 Rust 直接写入，归档条目统一使用 `/`，不会把本机的 `\\` 路径分隔符
带入更新协议。`publish` 只校验并发布已有产物，不隐式构建。MSI/EXE 用于首次安装，更新清单
只引用 versioned `windows.zip`。

## 真实验收

- 分别启动 `.msi` 与 branded `.exe`，两者都显示简体中文安装步骤。
- 安装选项页可以切换“创建桌面快捷方式”和“创建开始菜单快捷方式”。
- 完成页可以切换“安装完成后运行应用”，点击“完成”后行为与 checkbox 一致。
- 安装目录默认位于 `%LOCALAPPDATA%\Programs\com.nexora.examples.updater-windows`，也可以在安装目录页改到用户选择的位置。
- 主应用和 updater sidecar 都是 Windows GUI subsystem，正常启动和更新时不创建命令行窗口。
- Apps & Features、开始菜单和可选桌面快捷方式使用示例名称与 ICO。
- `artifact.json` 同时包含 `windows_msi`、`windows_setup_exe` 与 `windows_update_zip`；更新清单只包含 ZIP。
- 启用签名时，`signtool verify /pa` 通过主 EXE、updater EXE、MSI 和 Setup EXE。
- 不启用签名时，完整更新流程仍能通过，且不会显示必须配置 Authenticode 身份的错误。
- 从 Apps & Features 卸载时删除安装文件、安装器创建的快捷方式和注册表项，但不删除用户业务数据、配置和日志；自定义到其他磁盘时也不得因卷根目录 `Config.Msi` ACL 弹出 1926。
