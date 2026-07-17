---
name: publish-nexora-release
description: 用于准备和发布 Nexora 或 Nexora 应用的 GitHub 版本。适用于升级 SemVer、整理从上一 tag 到当前版本的完整改动、为每项标注 GitHub 处理人、关联 Issue/PR、记录破坏性变更与升级指南、执行发布验证、提交、推送 tag 和创建 GitHub Release。
---

# 发布 Nexora 版本

## 确认发布边界

1. 读取仓库规则、当前版本、默认分支、远端和最近 tag。
2. 运行 `git status -sb`、`gh auth status`，确认全部待提交文件属于本次发布。
3. 确定目标 SemVer 和唯一 tag；创建前同时确认本地、远端和 GitHub Release 中不存在该版本。
4. 以“上一版本 tag 到目标提交”为唯一改动范围，不把更早版本的历史重复写进本次升级指南。
5. 从提交、已合并 PR 和显式关联关系收集变更；不要仅凭标题相似度猜测 Issue 或处理人。

处理人优先使用合并 PR 作者；没有 PR 时，通过 GitHub commit API 把提交映射到账号。无法可靠
确认时先询问，不要编造。Release 中统一写成：

```markdown
— 处理人：[@login](https://github.com/login)
```

Issue 或 PR 仅在确实关联时写成可点击链接，例如
`[#42](https://github.com/owner/repo/issues/42)`；没有关联项时明确写“无”。

## 更新版本与文档

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

每个 Release 至少包含：

1. 版本摘要与发布日期；
2. 按 Added、Changed、Fixed 等类别组织的完整改动；
3. 每条改动的处理人 GitHub 链接；
4. 确实关联的 Issue/PR；
5. 兼容性与破坏性变更；
6. 从上一版本升级到本版本的操作；
7. 实际执行过的验证。

不要只使用 GitHub 自动生成说明代替人工 Release Notes；它可以辅助收集提交，但不能省略
用户影响、处理人和升级信息。

## 执行发布验证

按改动范围执行以下门禁，并在 Release Notes 中如实记录结果：

```bash
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo run -p nexora -- lint --workspace . --deny-warnings
cd docs && bun install --frozen-lockfile && bun run build
```

修改脚手架时还要实际生成代表性项目并编译桌面端与服务端。依赖外部 PostgreSQL、OIDC 或
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
