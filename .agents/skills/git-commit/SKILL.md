---
name: git-commit
description: 在 Xuwe workspace 中准备、审查或创建 Git 提交时使用。强制使用 Conventional Commits v1.0.0 格式，提交内容使用中文，并要求正文详细说明改动内容、目的、影响范围和验证情况。
---

# Git 提交

## 目的

使用这个 skill 为 Xuwe workspace 创建清晰、可审阅的 Git 提交。提交信息必须遵循 Conventional Commits v1.0.0，同时人类可读的摘要和正文使用中文。

参考：https://www.conventionalcommits.org/en/v1.0.0/#summary

## 提交头

使用这个格式：

```text
<type>[optional scope][optional !]: <中文摘要>
```

规则：

- `type` 和 `scope` 保持小写英文，方便工具识别。
- `:` 后面的摘要使用中文。
- 第一行尽量简洁，条件允许时控制在 72 个字符以内。
- 如果包含破坏性变更，在 `:` 前使用 `!`。

常用类型：

- `feat`: 新功能。
- `fix`: 修复缺陷。
- `docs`: 文档、skill、说明文字。
- `test`: 测试相关改动。
- `refactor`: 不改变行为的代码重构。
- `style`: 代码格式或不影响行为的样式调整。
- `perf`: 性能优化。
- `build`: 构建系统、依赖、工具链。
- `ci`: CI 配置。
- `chore`: 维护性杂项。
- `revert`: 回滚提交。

当 scope 有助于定位影响范围时使用 scope，例如 `server`、`console`、`api`、`accounts`、`database`、`migrate`、`desktop`、`logging`、`configuration` 或 `skills`。scope 应来自本次改动的真实 crate、应用或业务模块，不要沿用其他项目的名称。

## 正文必填

每次提交都必须包含详细的中文正文。不要创建只有一行摘要的提交。

使用多个 `-m` 参数提交，例如：

```sh
git commit \
  -m "feat(accounts): 增加角色权限管理" \
  -m "改动内容：
- 增加角色创建、更新和权限替换用例。
- 补充账号管理接口的授权校验和错误映射。

目的：
- 统一账号模块的 RBAC 管理入口并避免 handler 分散实现权限规则。

影响范围：
- accounts 业务模块、API 路由和角色权限契约。

验证：
- cargo test -p accounts
- cargo test -p api"
```

## 正文内容

正文要稍微详细，但不要变成流水账。按目的和影响组织内容，不要逐行罗列文件变更。

优先使用这些段落：

- `改动内容`：说明具体改了哪些能力、页面、模块、配置或文档。
- `目的`：说明为什么需要这些改动，解决了什么问题。
- `影响范围`：说明用户可见行为、接口、数据结构、路由、配置、部署或开发流程的影响。
- `验证`：列出实际运行过的命令、人工检查、未运行的原因。

可选段落：

- `注意事项`：记录后续需要接上的真实数据、兼容保留、迁移顺序等。
- `破坏性变更`：如果有 breaking change，正文可说明原因，footer 仍按规范写 `BREAKING CHANGE:`。

## Footer

需要时使用 footer：

```text
Refs: #123
Reviewed-by: name
BREAKING CHANGE: 中文说明破坏性变更
```

规则：

- 破坏性变更必须在提交头使用 `!`，或在 footer 使用 `BREAKING CHANGE:`。
- footer token 格式保持兼容 Git trailer 风格。
- footer 的值如果是给人看的说明，也使用中文。

## 工作流

1. 先运行 `git status --short`，并检查相关 diff。
2. 只 stage 属于本次提交目标的文件。
3. 选择最准确、最小化的 `type` 和可选 `scope`。
4. 写中文提交头和必填中文正文。
5. 如实说明验证情况；如果某个命令没有运行，说明原因。
6. 不要把无关改动混进提交。

## 检查清单

提交前确认：

1. 提交头符合 `<type>[scope][!]: <中文摘要>`。
2. 正文存在，并且使用中文。
3. 正文说明了改动内容、目的、影响范围和验证情况。
4. 破坏性变更已用 `!` 或 `BREAKING CHANGE:` 标记。
5. staged diff 和提交信息一致。
