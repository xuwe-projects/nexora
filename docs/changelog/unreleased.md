# Nexora Unreleased

## 修复

- `nexora::config::initialize(None)` 现在通过当前可执行文件与合法 `nexora-release.json` 读取
  macOS/Windows 正式安装包中冻结的 channel `runtime_config`。正式包不再依赖 cwd 或源码仓库，
  配置缺失、不可读或无效时明确失败且禁止开发回退；显式路径、用户位置参数和环境变量覆盖
  的既有优先级保持不变。updater health 参数及其值不会被误判为配置路径。

  — 处理人：[@openai](https://github.com/openai)
  — 关联 Issue/PR：无

- 增加 macOS `Contents/Resources/config`、Windows EXE 同级 `config`、正式/开发优先级、失败
  边界，以及 nightly/beta/stable 冻结配置的回归测试。

  — 处理人：[@openai](https://github.com/openai)
  — 关联 Issue/PR：无

## 兼容性与升级

- 下游应用无需复制路径判断或修改 `initialize(None)` 调用；升级 Nexora 并使用新版 CLI 重新构建
  正式安装包即可。旧安装包没有 `nexora-release.json`，不会被识别为新的正式配置边界。
- 本次同步修改 `develop-nexora-apps`、`develop-nexora-updater` 及 updater 协议 Skill；下游项目
  升级时应同步 `.agents/skills`。
