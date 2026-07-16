# 参与 Nexora

感谢你愿意参与 Nexora。项目目前处于 early alpha：设计仍在快速收敛，维护者可能会调整
公开 API，也会优先接受边界清楚、能够验证的改动。参与前请同时阅读
[社区行为准则](CODE_OF_CONDUCT.md)。

## 开始之前

- 修复明确缺陷、补充测试或完善文档可以直接提交 Pull Request；
- 新增公开 API、依赖、平台能力或大范围重构，请先创建 Issue 说明动机、使用场景和备选方案；
- 安全漏洞不要提交公开 Issue，请遵循 [安全策略](SECURITY.md)；
- 不要提交访问令牌、真实账号、生产配置、内部域名或用户数据。

## 本地开发

项目使用 Rust 2024 edition。请安装当前稳定版 Rust、`rustfmt` 与 Clippy。完整 workspace 的
GPUI 依赖可能还需要对应平台的系统构建工具；服务端和数据库集成测试需要 PostgreSQL。

先运行与改动范围最接近的检查：

```bash
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p nexora -- lint --workspace . --deny-warnings
```

仅修改 Nexora 框架时，可以先使用更快的核心测试：

```bash
cargo test -p nexora -p nexora-macros
```

数据库测试需要显式配置测试数据库；不要让测试指向生产或共享环境。具体配置和迁移约定见
[`config/README.md`](config/README.md) 与
[`crates/migrate/README.md`](crates/migrate/README.md)。

## 提交改动

1. 从最新主分支创建短生命周期分支；
2. 保持一次提交或一组提交只解决一个问题；
3. 为行为变更补充测试，并同步 rustdoc/README 中受影响的契约；
4. 运行与改动范围相称的检查，在 Pull Request 中写明未运行的检查及原因；
5. 清楚描述问题、方案、兼容性影响和验证结果。

提交信息使用 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/v1.0.0/)，例如：

```text
feat(nexora): 增加 Feature 生命周期
fix(route): 修复动态参数解码
docs: 补充贡献指南
```

## 设计原则

- 框架提供通用能力，业务 Feature 只实现自己的状态与界面；
- 不把尚未实现或未经测试的能力写成已支持；
- 优先复用 workspace 中已有抽象，避免为单一调用方引入全局机制；
- 公开 API 应有 rustdoc、失败语义和覆盖正常/异常路径的测试；
- 保持错误可诊断，避免静默回退和隐藏副作用。

## 许可证

除非你明确另行声明，所有有意提交到本项目的贡献都将按照项目的
[Apache-2.0](LICENSE-APACHE) 或 [MIT](LICENSE-MIT) 双许可证发布，不附加其他条款。
