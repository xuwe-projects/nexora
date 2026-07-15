# Kernel

`kernel` 只保存多个业务模块真实共享、并且不依赖具体传输协议或基础设施的应用原语。

当前包含：

- `RequestId`：安全生成或校验调用链关联 ID。
- `ExecutionContext`：传播关联 ID 与调用开始时间。
- `Clock`：为需要确定性时间的代码提供可替换时间来源。
- `ValidationError`：供业务 application 包装的字段校验详情。
- `PageRequest`、`Page<T>`：领域/application 层分页值对象。

HTTP 请求、响应和 JSON 序列化结构属于 `contracts`；数据库连接、事务和 SQL 属于
`database` 或业务模块 store。不要为了“以后可能用到”把业务模型、权限或万能错误枚举加入
kernel。
