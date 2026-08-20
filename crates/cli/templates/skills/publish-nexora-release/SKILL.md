---
name: publish-nexora-release
description: 用于准备和发布 Nexora 或 Nexora 应用的版本，并为启用 Updater 的应用生成面向最终用户的 release.notes。适用于升级 SemVer、整理 GitHub Release/开发者 Changelog、标注处理人与 Issue/PR、生成应用内更新说明、记录破坏性变更与升级指南，以及执行验证、提交、推送 tag 和创建 GitHub Release。
---

# 发布 Nexora 版本

## 确认发布边界

1. 读取仓库规则、当前版本、默认分支、远端和最近 tag。
2. 运行 `git status -sb`、`gh auth status`，确认全部待提交文件属于本次发布。
3. 确定目标 SemVer 和唯一 tag；创建前同时确认本地、远端和 GitHub Release 中不存在该版本。
4. 以“上一版本 tag 到目标提交”为唯一改动范围，不把更早版本的历史重复写进本次升级指南。
5. 从提交、已合并 PR 和显式关联关系收集变更；不要仅凭标题相似度猜测 Issue 或处理人。

版本号必须严格遵守 [Semantic Versioning 2.0.0](https://semver.org/) 的递增规则，并作为发布
前置门禁执行：

- 使用 SemVer 解析器比较版本优先级，目标版本必须严格大于最新已发布版本；禁止使用字符串
  比较，也禁止复用、降低或移动已发布版本。
- 存在不兼容公开 API、配置、数据或迁移行为时递增 `MAJOR`；在 `0.y.z` 初始开发阶段，
  不兼容变更至少递增 `MINOR`，不得只递增 `PATCH`。
- 新增向后兼容功能时递增 `MINOR` 并把 `PATCH` 归零；只有向后兼容缺陷修复、文档或发布
  流程调整且不新增功能时才递增 `PATCH`。
- 递增 `MAJOR` 时把 `MINOR` 和 `PATCH` 归零。预发布标识按 SemVer 优先级推进，正式版
  必须高于同核心版本的预发布版。
- 同一发布同时包含多类变更时采用影响最大的级别。若实际改动与目标版本级别不匹配，停止
  发布并先修正版本，不得为了沿用预设版本而淡化破坏性变更或把新增功能伪装成补丁。

处理人优先使用合并 PR 作者；没有 PR 时，通过 GitHub commit API 把提交映射到账号。无法可靠
确认时先询问，不要编造。Release 中统一写成：

```markdown
— 处理人：[@login](https://github.com/login)
```

Issue 或 PR 仅在确实关联时写成可点击链接，例如
`[#42](https://github.com/owner/repo/issues/42)`；没有关联项时明确写“无”。

## 生成 GitHub Release 与开发者 Changelog

这一节的输出面向开发者，不得用后文的应用内简化格式替换。

- 更新根 workspace 版本以及内部依赖的版本约束，让 `Cargo.lock` 记录全部 workspace package
  的新版本。
- 更新 README 安装 tag、OpenAPI 版本和其他面向用户展示的当前版本，但保留脚手架新项目自身
  的 `0.1.0` 初始版本。
- 在 `docs/changelog/<version>.md` 编写可直接作为 GitHub Release body 的完整中文说明，并
  在 `docs/en/changelog/<version>.md` 提供英文版本。
- 在组件需要内嵌更新日志时，同步维护
  `docs/changelog/components/<version>/<component>/<locale>.md`。
- 只写上一版本到当前版本的升级说明。存在破坏性变更时，必须列出旧写法、新写法、配置或
  数据迁移顺序和回滚注意事项；没有破坏性变更时明确说明无需手工迁移。
- 如果本版本新增或修改了脚手架 Skill、宏编写规则或生成项目约束，Release Notes 必须在
  升级指南中提醒应用项目同步 `.agents/skills`；涉及自定义宏规则时，明确说明需要接入
  `cargo expand`、手写等价对比、`cargo bench`、`cargo bloat`，以及未达标时的 `cargo asm`
  分析闭环。

每个 Release 至少包含：

1. 版本摘要与发布日期；
2. 按 Added、Changed、Fixed 等类别组织的完整改动；
3. 每条改动的处理人 GitHub 链接；
4. 确实关联的 Issue/PR；
5. 兼容性与破坏性变更；
6. 从上一版本升级到本版本的操作；
7. 实际执行过的验证。
8. 需要下游项目同步的 Skill 或脚手架规则。

不要只使用 GitHub 自动生成说明代替人工 Release Notes；它可以辅助收集提交，但不能省略
用户影响、处理人和升级信息。

## 生成应用内 Updater 更新说明

发布启用 Updater 的应用时，为当前 app/channel 配置的 `release.notes` 单独生成
面向最终用户的 Markdown。它与 GitHub Release/开发者 Changelog 是两种输出，
不得相互代替：

1. 从 `nexora.toml` 读取所选 app 的 `package`，通过
   `cargo metadata --no-deps --format-version 1` 解析该 Cargo package 的实际版本，
   包括 `version.workspace = true` 的情况。标题禁止手写、复制 tag 或使用 Nexora CLI
   自身版本；无法唯一解析所发布 package 时停止生成。
2. 使用准备本次发布时的实际本地日期，固定写为 `yyyy-MM-dd`；不复制旧文档
   日期，也不手写其他日期格式。
3. 检查上一应用版本到当前发布的实际改动，按用户能看到的结果合并条目。
   无法从实际改动确认用户可见内容时，停止生成并要求补充信息；不得为了
   填满分类而编造影响。
4. 把结果写入当前 app/channel 实际配置的 `release.notes` 路径，再由现有
   `nexora build` / `publish` 流程校验、冻结、签名和发布。不增加标题解析器，
   也不让 Updater 运行时理解下列分类。

使用以下格式，并删除没有内容的整个分类：

```markdown
## v{Cargo package version}（{yyyy-MM-dd}）

### 重要提醒
- 用户升级前后必须执行或特别注意的事项。

### 新功能
- 新增的用户能力。

### 问题修复
- 修复的用户可感知问题。

### 其他调整
- 体验、性能、兼容性或行为方面的其他调整。
```

标题必须严格为 `## v{Cargo package version}（{yyyy-MM-dd}）`。正常分类只允许
`新功能`、`问题修复`和 `其他调整`，统一使用“其他”，不使用“其它”。没有内容的
分类必须连同标题完全省略，不写“暂无”、“无”或占位条目；只有一个正常分类时，
输出中不得出现其他空标题。

`重要提醒` 不是固定分类。只有用户必须重新登录、备份、迁移、重新配置、停机、
重启或人工处理，或必须提前知道环境、工作流程、兼容性或安全影响时才生成。
它必须位于版本标题之后、所有正常分类之前；没有用户必须操作或注意的事项时，
完全省略该标题。不得把普通优化、内部重构、测试调整或开发者注意事项写成重要提醒。

每条只描述用户能看到什么变化、可以完成什么事情，或什么问题不再发生，并优先
说明用户收益。合并同一用户能力下的内部改动，不按 commit 逐条复制。除非用户必须
操作或理解，不写 crate、模块、类型、函数、源码路径、GPUI/Axum/SQLx、API/DTO/
Router/handler、数据库对象与迁移、CI/Clippy/测试/构建脚本、commit/PR/Issue/处理人，
也不解释 Updater manifest、sidecar 或签名实现。把确实影响用户的技术变化改写为
用户结果，例如把查询参数的内部修复改写为“修复用户列表在使用分页或筛选条件时无法
正常加载的问题”。

## 执行发布验证

按改动范围执行以下门禁，并在 Release Notes 中如实记录结果：

```bash
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo run -p cli -- lint --workspace . --deny-warnings
bash scripts/check-scaffold-consumer.sh
cd docs && bun install --frozen-lockfile && bun run build
```

`check-scaffold-consumer.sh` 必须从无 `Cargo.lock` 的实际生成项目执行 `cargo check`，用于
发现 gpui-component 与 GPUI revision 在下游重新解析时产生的不兼容。修改 Account 脚手架时
还要实际生成 workspace 项目并编译桌面端与服务端。依赖外部 PostgreSQL、OIDC 或
签名环境的验证没有执行时，必须在提交与 Release Notes 中说明，不得写成已经通过。

任一必需门禁失败时停止发布，修复后从失败项开始重跑；不要先打 tag 再补测试。

## 提交、打 Tag 与创建 Release

1. 按 `git-commit` Skill 审查 staged diff，并使用中文 Conventional Commit 与完整正文。
2. 推送目标提交，确认远端分支 SHA 与本地发布提交一致。
3. 创建 annotated tag 并单独推送；已公开 tag 不得移动或强推，除非用户明确授权。
4. 使用文档中的目标版本页面作为 Release Notes 创建 GitHub Release。Early alpha 版本默认
   标记为 pre-release，除非项目已经明确进入稳定发布通道。
5. 验证 GitHub Release、tag peeled commit、CI/Pages workflow 和文档 URL；失败时读取 Actions
   日志并修复，不把失败发布报告为完成。

典型命令：

```bash
git tag -a vX.Y.Z -m "Nexora X.Y.Z" <commit>
git push origin refs/tags/vX.Y.Z
gh release create vX.Y.Z \
  --verify-tag \
  --prerelease \
  --title "Nexora X.Y.Z" \
  --notes-file docs/changelog/X.Y.Z.md
```

发布完成后报告分支、提交 SHA、tag、Release URL、文档 URL、验证结果和仍未执行的外部环境
测试。不得在 Release、日志或配置样例中暴露 PAT、setup secret、数据库密码或 token。
