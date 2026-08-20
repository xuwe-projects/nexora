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

## 调整

- `publish-nexora-release` Skill 现在把应用内 Updater `release.notes` 与 GitHub Release/
  开发者 Changelog 分开生成。应用内说明使用所选 app 实际解析的 Cargo package
  版本和发布准备日期，只保留有内容的“新功能”、“问题修复”、“其他调整”以及
  确有用户操作要求时的“重要提醒”，并要求把内部实现改写为用户可感知结果。
  GitHub Release/开发者 Changelog 仍保留完整技术改动、处理人、Issue/PR、升级步骤与验证结果。

  — 处理人：[@openai](https://github.com/openai)
  — 关联 Issue/PR：无

- CLI 脚手架中的 Skill 镜像、中英文 CLI/updater 文档与生成项目回归断言已同步；
  Updater 仍只校验、冻结、签名、发布和展示安全 Markdown，不解析新标题或分类。

  — 处理人：[@openai](https://github.com/openai)
  — 关联 Issue/PR：无

## 兼容性与升级

- 下游应用无需复制路径判断或修改 `initialize(None)` 调用；升级 Nexora 并使用新版 CLI 重新构建
  正式安装包即可。旧安装包没有 `nexora-release.json`，不会被识别为新的正式配置边界。
- 本次同步修改 `develop-nexora-apps`、`develop-nexora-updater`、updater 协议与
  `publish-nexora-release` Skill；下游项目升级时应同步 `.agents/skills`。
