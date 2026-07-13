# Config

这个目录用于放置模板项目的“运行时配置样例”和“环境变量样例”。新成员第一次打开项目时，
应该先看这里，而不是从源码里猜系统支持哪些配置。

## 文件约定

服务端默认按配置名称加载：

```bash
cargo run -p server
```

等价于读取：

```text
config/local.toml
```

也可以切换到其它配置文件名：

```bash
cargo run -p server -- --profile production
```

这会读取：

```text
config/production.toml
```

如果需要指定任意路径，可以使用：

```bash
cargo run -p server -- --config /path/to/server.toml
```

配置优先级为：

```text
代码默认值 < config/<profile>.toml 或 --config 文件 < 环境变量
```

环境变量不带项目名前缀，嵌套字段使用双下划线：

```bash
SERVER__HOST=0.0.0.0
SERVER__PORT=8080
```

## 当前样例

- `local.toml`：本地开发默认配置。
- `production.toml`：生产部署配置形状示例，不包含真实密钥。
- `server.env.example`：服务端运行时环境变量示例。
- `desktop.toml.example`：Console 桌面应用本地运行配置样例。
- `desktop-build.env.example`：桌面端构建、签名和更新相关环境变量示例。
- `updater/latest.example.json`：`xuwecli build` 生成的更新清单形状示例，不需要手写。

## 桌面端 OIDC 认证

桌面端认证使用标准 OIDC Authorization Code + PKCE 流程。ZITADEL 已部署时，只需要把
Console 配置成 Native/Public Application，并允许下面的 loopback redirect URI：

```text
http://127.0.0.1:0/auth/callback
```

`OIDC_REDIRECT_URI` 是回调 URI 模板。端口为 `0` 时，应用会先绑定本地 listener，再将
`0` 替换为实际端口写入 `authorize` 和 `token` 请求；ZITADEL 需要将这个 Native loopback
redirect URI 视为同一路径上的动态端口。若实例返回 redirect URI 不匹配错误，说明该实例的
redirect 规则没有允许动态端口，应改用该实例允许的固定端口 URI。

本地开发可以复制运行时配置样例：

```bash
cp config/desktop.toml.example config/desktop.toml
```

然后修改 `config/desktop.toml` 中的 `[oidc]` 配置。该文件已被 Git 忽略。应用也读取通用
OIDC 环境变量，不使用 Provider 名称前缀；环境变量的优先级高于 `desktop.toml`：

```bash
OIDC_ISSUER_URL=https://id.example.com
OIDC_CLIENT_ID=console-native-client-id
OIDC_SCOPES="openid profile email offline_access"
OIDC_REDIRECT_URI=http://127.0.0.1:0/auth/callback
```

`desktop-build.env.example` 是供 shell、CI 和发布机构建桌面安装包时参考的 dotenv 风格文件，
其中还包含签名、公证和更新发布变量。`config-rs` 的文件源不会把这种 `KEY=value` 文件当作
TOML 读取，因此应用运行时只通过 `config/desktop.toml` 加载 OIDC 文件配置。

以后新增 `apps/<app>` 桌面程序时，可以复用 `crates/oidc`，应用层只需要负责自己的
环境变量装配、token 存储位置和 GPUI 状态接入。

Console 只把 refresh token 保存到系统安全凭据库：macOS 使用 Keychain，Windows 使用
Credential Manager，Linux 使用 Secret Service。access token、ID Token 和用户资料只保留在
当前进程内存中。旧版本生成的明文 `auth.toml` 会在 refresh token 成功迁移并回读校验后删除。

应用启动时会使用 refresh token 换取并验证新会话；运行期间会在 access token 到期前自动续期。
Provider 返回 `invalid_grant` 时会清除失效凭据并回到登录页，临时网络错误则保留凭据并延迟重试。

## 更新发布文件

`xuwecli build` 会在 `dist/` 下生成自动更新需要的 `.app.zip`、同名 `.sha256`、
`latest.json` 和 `notes/...md` 更新日志副本。后续 `xuwecli publish` 接入 OSS 后，应先上传
安装包、校验文件和更新日志，最后上传 `latest.json`。

本机 macOS 可以一次构建多个 macOS 架构：

```bash
xuwecli build --targets macos --bundle-version 12
```

该命令会顺序构建 Apple Silicon 与 Intel macOS 产物，并把两个 `.app.zip` 写入同一个
`latest.json.artifacts`。如果只构建当前机器架构，使用默认值或显式传入：

```bash
xuwecli build --targets current
```

Windows、Linux 和 macOS 完整发布矩阵不应依赖单个 Docker 容器完成。推荐做法是使用 CI 或
远程 runner 矩阵：

- macOS runner：生成 `.dmg`、`.app.zip`、签名、公证。
- Windows runner：生成 `.exe` 或 `.msi`，完成 Windows 签名。
- Linux runner：生成 `.tar.gz`、`.deb`、`.rpm` 或 AppImage。

各 runner 上传自己的产物元数据后，由发布步骤合并生成最终 `latest.json`，并确保最后上传它。

仓库根目录提供了一个普通 Pipeline 版本的 `Jenkinsfile`，默认只启用 `macos-arm64` agent。
你可以先在 Jenkins 中创建 Pipeline Job 指向该文件试跑；Windows stage 通过 `BUILD_WINDOWS`
参数控制，等 Windows 打包能力补齐后再打开。

## 不放在这里的配置

桌面用户偏好不放在仓库 `config/` 目录中，例如主题、颜色模式、最近打开记录等。这些属于用户
本机状态，会由桌面程序保存到操作系统标准配置目录。
