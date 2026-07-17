# 应用内嵌更新日志

更新日志采用以下目录结构：

```text
docs/changelog/components/<version>/<component>/<locale>.md
```

- `version` 必须是合法的 SemVer，例如 `1.0.2`。
- `component` 是稳定的产品或服务标识，例如 `api`、`console`、`merchant-desktop`。
- `locale` 是语言区域标识，例如 `zh-CN`、`en-US`。

新增版本时创建对应 Markdown 文件。应用构建时会自动嵌入这些面向最终用户的组件日志，并
按语义化版本从新到旧排列。完整版本说明与升级文档放在 `docs/changelog/<version>.md`。
