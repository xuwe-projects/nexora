---
name: rust-code-style
description: 用于编写、修改、审查或生成本仓库 Rust 代码，尤其涉及公开 API、rustdoc 注释、模块结构和可维护性约定。
---

# Rust 代码风格

## 核心规则

在本仓库编写、修改或生成 Rust 代码时，所有公开 API 都必须提供详细的中文 rustdoc 注释。

这包括但不限于：

- `pub struct`、`pub enum`、`pub trait`、`pub type`、`pub const`、`pub static`
- `pub fn`、公开关联函数、公开方法、trait 中的接口方法
- `pub mod`、公开枚举变体、公开字段
- 对外可见的关联类型、泛型约束含义、返回值语义和副作用

## 注释格式

- 文件或模块职责使用 `//!`。
- 类型、函数、方法、字段、枚举变体使用 `///`。
- 注释必须使用中文，且符合 rustdoc 规范。
- 注释要解释“这个 API 表达什么职责、什么时候使用、重要参数或返回值是什么意思”。
- 不要只写名称复述，例如“获取配置”“设置值”这种过短注释不合格。

## 编写要求

- 新增公开 API 时必须同时新增中文 rustdoc。
- 修改公开 API 行为时必须同步更新 rustdoc。
- 删除或重命名公开 API 时，要检查引用处的文档是否仍然准确。
- 内部实现注释可以使用普通 `//`，但公开 API 的文档必须使用 rustdoc 注释。

## 测试组织

- 所有 Rust 测试用例必须放在对应 crate 的 `tests/` 集成测试目录中，例如 `apps/console/tests/` 或 `crates/desktop/tests/`。
- 生产源码中禁止出现 `#[cfg(test)]`、`mod tests`、测试专用导入、测试专用类型或仅为测试条件编译的逻辑。
- 如果二进制 crate 需要被集成测试覆盖，应提供薄的 `src/lib.rs` 暴露可测试的正式 API，并让 `src/main.rs` 只负责启动入口。
- 需要测试内部行为时，优先通过有真实业务含义的公共 API 或只读查询方法建立边界；不要为了测试把无意义的实现细节公开。
- 公开给集成测试使用的 API 仍然必须遵守中文 rustdoc 规范。
- 测试辅助代码可以放在 `tests/common/` 或测试文件的私有模块中，但不能放回生产源码。

## 宏与样板代码

- 能用 Rust 标准派生宏、属性宏或项目已有宏清晰表达的行为，优先使用宏，不要无端增加手写样板代码。
- `Debug`、`Clone`、`Copy`、`PartialEq`、`Eq`、`Hash`、`Default` 等标准能力，在语义完全等价时优先放进 `#[derive(...)]`。
- 枚举默认变体可以用 `#[derive(Default)]` 和 `#[default]` 表达时，不要手写 `impl Default`。
- 只有在派生宏无法表达业务逻辑、需要非平凡初始化、需要条件编译分支，或手写实现能明显改善可读性时，才手动实现 trait。
- 使用宏不能牺牲公开 API 的中文 rustdoc：公开类型、公开字段、公开枚举变体和公开方法仍然需要完整中文文档。

错误示例：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureId {
    /// 控制台首页，用于展示应用概览和常用入口。
    Home,
}

impl Default for FeatureId {
    fn default() -> Self {
        Self::Home
    }
}
```

推荐写法：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FeatureId {
    /// 控制台首页，用于展示应用概览和常用入口。
    #[default]
    Home,
}
```

## 示例

```rust
/// 桌面应用的启动模式。
///
/// 运行器会根据该模式决定启动后是否立即创建主窗口。
pub enum StartupMode {
    /// 后台启动应用，不主动打开主窗口。
    Background,

    /// 前台启动应用，并按窗口配置打开主窗口。
    Foreground,
}
```
