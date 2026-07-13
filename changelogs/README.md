# 更新日志目录

更新日志采用以下目录结构：

```text
changelogs/<version>/<component>/<locale>.md
```

- `version` 必须是合法的 SemVer，例如 `1.0.2`。
- `component` 是稳定的产品或服务标识，例如 `api`、`console`、`merchant-desktop`。
- `locale` 是语言区域标识，例如 `zh-CN`、`en-US`。

新增版本时只需要创建对应 Markdown 文件。应用构建时会自动嵌入所有日志，并按语义化版本从新到旧排列。
