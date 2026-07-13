---
name: rust-code-style
description: 用于编写、修改、审查或生成本仓库 Rust 代码与 Cargo 配置，规范 rustdoc、命名与导入、控制流、迭代器、类型转换、错误处理、异步 I/O、测试与质量检查，以及 Cargo workspace 的依赖和 crate 组织。
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
- 公开 API 的用法不直观时，使用 `# Examples` 提供可运行的示例，并尽量让示例可作为 doctest 执行。
- 公开函数可能返回错误时，使用 `# Errors` 说明错误条件；可能发生 panic 时，使用 `# Panics` 明确触发条件。

## 编写要求

- 新增公开 API 时必须同时新增中文 rustdoc。
- 修改公开 API 行为时必须同步更新 rustdoc。
- 删除或重命名公开 API 时，要检查引用处的文档是否仍然准确。
- 内部实现注释可以使用普通 `//`，但公开 API 的文档必须使用 rustdoc 注释。

## 命名与导入

- 遵循 Rust 标准命名约定：模块、函数和变量使用 `snake_case`，类型、trait 和枚举变体使用 `UpperCamelCase`，常量和静态变量使用 `SCREAMING_SNAKE_CASE`。
- 使用花括号合并同一个 crate 或模块下的导入，避免为每个类型编写重复的 `use` 声明。
- 函数签名和类型声明中优先使用已导入的简短类型名，不要反复书写冗长的完全限定路径；需要消除同名歧义或局部使用一次时，可以保留限定路径。
- 运行 `rustfmt` 统一导入布局，不要手工维护与格式化工具冲突的排序。

推荐写法：

```rust
use crate::{
    error::AppError,
    repository::{FileRepository, Repository},
};
```

避免写法：

```rust
use crate::error::AppError;
use crate::repository::FileRepository;
use crate::repository::Repository;
```

## 实现范围

- 严格实现任务说明、验收条件和指定边界情况，不要擅自增加功能、配置或抽象层。
- 保持改动最小且聚焦，只修改完成任务所必需的代码和配置。
- 仅在完成需求确实需要、能够消除当前问题，或用户明确要求时进行重构；不要把无关清理混入功能改动。
- 覆盖与改动直接相关的回归风险，但不要为假想需求引入额外复杂度。

## 控制流

- 只处理一个模式、其余情况无需操作时，使用 `if let`，不要使用带空兜底分支的 `match`。
- 循环只在某个模式持续匹配时执行时，使用 `while let`，不要使用 `loop`、`match` 和 `break` 手动表达同一控制流。

推荐写法：

```rust
if let Some(value) = option {
    do_something(value);
}

while let Some(item) = iterator.next() {
    process(item);
}
```

避免写法：

```rust
match option {
    Some(value) => do_something(value),
    None => {}
}

loop {
    match iterator.next() {
        Some(item) => process(item),
        None => break,
    }
}
```

## 迭代器模式

- 对集合执行连续的筛选、转换和收集时，优先组合 `iter`、`into_iter`、`filter`、`map` 和 `collect` 等迭代器方法，不要使用可变集合与手写循环表达同一过程。
- 能在一次迭代中完成处理时，使用 `filter_map`、`flatten`、`flat_map`、`fold` 或 `try_fold` 等组合器，不要创建只供下一步处理使用的中间集合。
- 保持迭代器链易于理解；闭包包含复杂分支或明显副作用时，提取具名函数或改用更清晰的控制流。

推荐写法：

```rust
let results: Vec<_> = items
    .iter()
    .filter(|item| item.is_valid())
    .map(|item| item.process())
    .collect();
```

避免写法：

```rust
let mut results = Vec::new();
for item in &items {
    if item.is_valid() {
        results.push(item.process());
    }
}
```

## 解构、更新与类型转换

- 能直接表达所需字段或变体时，使用解构绑定或解构赋值，不要逐字段重复访问同一个值。
- 基于已有值构造同类型结构体且只修改部分字段时，使用结构体更新语法 `..base`，并确认字段移动、复制或借用语义符合预期。
- 无损且不会失败的转换实现 `From`，让调用方按上下文使用 `From` 或 `Into`；可能失败的转换使用 `TryFrom` 或 `TryInto`，不要用 panic 表达转换失败。
- 参数确实需要接受多种可借用表示时，使用 `AsRef` 或 `AsMut` 约束；只接受单一类型时保留具体参数类型，避免无意义的泛型化。

示例：

```rust
let Config { host, port, .. } = config;
let updated = Config {
    port: new_port,
    ..previous
};
```

## 错误处理

- 仅需向上传播 `Result` 错误，且错误可通过 `From` 或 `thiserror` 的 `#[from]` 自动转换时，使用 `?` 运算符，不要用 `match` 手动展开 `Ok` 和 `Err`，也不要调用 `map_err(ErrorType::from)` 显式执行等价转换。
- 只有需要添加无法由 `From` 表达的上下文，或执行自定义错误转换时，才使用 `map_err`。使用 `anyhow` 时，优先使用 `context` 或 `with_context` 添加上下文。
- 应用程序边界需要汇总异构错误并附带调用上下文时，优先使用 `anyhow`；需要稳定、可匹配的结构化错误类型时，优先使用 `thiserror`。
- 组合多步 `Option` 或 `Result` 转换时，在保持可读性的前提下使用 `ok_or`、`ok_or_else`、`and_then`、`or_else`、`map` 或 `transpose` 等方法，避免不必要的嵌套 `match` 和中间值。

推荐写法：

```rust
let file = File::open("data.txt")?;

let file = File::open(path).map_err(|source| NoteError::IoWithContext {
    path: path.to_owned(),
    source,
})?;

let value = option.ok_or_else(|| NoteError::MissingValue)?;
```

避免写法：

```rust
let file = File::open("data.txt").map_err(NoteError::from)?;

let file = match File::open("data.txt") {
    Ok(file) => file,
    Err(error) => return Err(NoteError::from(error)),
};
```

## 异步 I/O

- 异步调用链中的文件、网络、进程和定时 I/O 使用 `async`/`await` 与异步 API，不要阻塞异步执行器线程。
- 必须调用阻塞 API 或执行长时间 CPU 密集任务时，使用运行时提供的阻塞任务机制，例如 `tokio::task::spawn_blocking`，并明确任务取消和错误传播语义。
- 同步程序或明确的同步边界可以使用同步 I/O；不要仅为形式统一而把不需要并发的接口改成异步。

## 依赖管理

- 所有第三方依赖和 workspace 内部 crate 依赖都必须由根 `Cargo.toml` 的 `[workspace.dependencies]` 统一管理版本、Git 来源或本地路径。
- 成员 crate 的普通依赖、开发依赖和构建依赖必须使用 `{ workspace = true }` 引入，不要在成员 `Cargo.toml` 中单独填写 `version`、`git`、`branch`、`rev` 或 `path`。
- 平台条件依赖可以继续放在成员 crate 的 `[target.'cfg(...)'.dependencies]` 下，但依赖本身仍必须使用 `{ workspace = true }`，不允许绕过 workspace 单独声明版本或来源。
- 公共 feature 优先在 `[workspace.dependencies]` 中统一配置；只有单个 crate 确实需要的附加 feature，才在继承 workspace 依赖时单独启用。
- 新增、升级或替换依赖时，先修改根 `Cargo.toml`，再检查所有成员 crate 是否继续通过 workspace 继承，避免同一个依赖出现多个版本声明。

推荐写法：

```toml
# 根 Cargo.toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
domain = { path = "crates/domain" }

# 成员 crate/Cargo.toml
[dependencies]
serde = { workspace = true }
domain = { workspace = true }
```

禁止写法：

```toml
# 成员 crate/Cargo.toml
[dependencies]
serde = { version = "1", features = ["derive"] }
domain = { path = "../../crates/domain" }
```

### 最小 Feature

- 引入依赖前先检查它提供的 feature 和默认 feature，只启用当前 crate 实际使用的能力。
- 不需要的 feature 不要声明；不要为了省事启用 `full`、`all` 等聚合 feature，除非当前 crate 确实使用其中全部或绝大多数能力，并在改动说明中写明原因。
- 默认 feature 包含当前项目不需要的运行时、协议、平台或实现时，在确认兼容后使用 `default-features = false`，再显式启用必要 feature。
- 根 `[workspace.dependencies]` 只配置所有使用方都需要的最小公共 feature。单个成员 crate 的额外能力在 `{ workspace = true }` 基础上按需追加，避免把专用 feature 扩散到整个 workspace。
- 修改 feature 后使用 `cargo tree -e features` 检查实际启用结果，确认没有因为 feature 统一机制意外引入大范围依赖。

推荐写法：

```toml
# 根 Cargo.toml：只保留公共最小能力
[workspace.dependencies]
tokio = { version = "1", default-features = false }

# 仅 HTTP 服务需要宏和多线程运行时
[dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

不推荐写法：

```toml
[dependencies]
tokio = { workspace = true, features = ["full"] }
```

### 内部依赖边界

- workspace 内部依赖也必须最小化。一个 crate 只依赖它实际使用的契约、模型或能力，不要为了少量类型依赖完整应用、HTTP 服务或基础设施 crate。
- 当多个 crate 只需要共享请求参数、响应结果、事件载荷或稳定数据模型时，将这些类型抽到独立的轻量 crate，例如 `contracts`、`models` 或更具体的业务 crate。
- 共享模型 crate 只保留跨边界需要的类型、序列化定义和必要校验，不要引入 Axum 路由、SQLx 数据访问、Tokio runtime 或服务端启动逻辑。
- 依赖方向保持单向：例如 `api -> contracts`、`console -> contracts`，禁止让 `contracts` 反向依赖 `api` 或 `console`。
- `console` 只需要 API 请求和响应模型时，应依赖共享模型 crate，不要直接依赖完整 `api` crate，避免把服务端依赖树和实现细节传递到桌面程序。
- 只有一个 crate 使用的类型继续放在该 crate 的私有模块中；至少两个 crate 确实共享稳定边界时，再拆分公共 crate，避免为假想复用制造碎片化模块。
- 共享类型仍然属于公开 API，必须提供详细的中文 rustdoc，并避免直接暴露数据库实体或框架专用类型作为跨 crate 契约。

依赖结构示例：

```text
contracts
  ↑       ↑
 api    console
```

禁止让 `console` 仅为了复用请求和响应类型而依赖 `api`：

```text
console -> api -> axum + sqlx + tokio + ...
```

## Crate 命名

- crate 名称直接表达职责或业务边界，不要重复项目名、产品名或 workspace 名作为前缀。
- 禁止使用 `xxx-core`、`xxx-handle`、`xxx-handler` 这类“项目前缀 + 通用技术角色”的名称，例如 `xuwe-core`、`console-core`、`xuwe-handler`。
- 当职责明确且不存在歧义时，直接使用 `core`、`handler` 等简洁名称。
- 如果 `core` 与 Rust 核心库冲突、名称不可用，或者 `handler` 无法准确表达职责，不要重新添加 `xxx-` 前缀；改用更符合业务场景或架构职责的名称，例如 `domain`、`application`、`accounts`、`projects`、`builds`、`migration` 或 `desktop`。
- 当一个 workspace 存在多个同类 handler 时，优先按业务边界拆分并使用业务名称，例如 `accounts`、`billing`，不要创建 `account-handler`、`billing-handler`。
- crate 目录名、`Cargo.toml` 中的 package 名以及代码中的 crate 标识应保持一致；连字符仅用于 Cargo package 名，Rust 代码引用时按规则转换为下划线。

命名示例：

| 不推荐 | 推荐 | 原因 |
| --- | --- | --- |
| `xuwe-core` | `core` 或 `domain` | 去掉冗余项目名前缀，并在冲突时表达领域职责。 |
| `console-handler` | `handler` 或具体业务名 | 避免使用应用名前缀包装通用角色。 |
| `account-handler` | `accounts` | 直接表达账号业务边界，而不是实现层角色。 |
| `desktop-handle` | `desktop` | 使用实际业务或平台职责，避免含义模糊的 `handle`。 |

## Crate 目标组织

- bin 类型 crate 已经使用 `src/main.rs` 作为入口时，不要在同一个 package 中再创建 `src/lib.rs`。
- `src/main.rs` 只负责参数解析、配置加载、依赖装配和启动流程，避免直接承载大量可复用业务逻辑。
- bin crate 需要复用或测试业务逻辑时，将逻辑拆到独立的 workspace library crate，再由 bin crate 通过 `{ workspace = true }` 依赖该 crate。
- 不要为了让集成测试能够导入 bin crate 而补建薄 `src/lib.rs`，也不要在 `main.rs` 和 `lib.rs` 之间重复声明同一组模块。
- 同一个产品同时需要命令入口和可复用能力时，使用两个职责明确的 crate，例如 `apps/server` 只保留 `src/main.rs`，可复用能力放在 `crates/application`、`crates/domain` 或具体业务 crate 中。
- library crate 使用 `src/lib.rs`，bin crate 使用 `src/main.rs`；除非用户明确要求特殊 Cargo 多目标 package，否则两种目标不要混放在同一个 crate。

推荐结构：

```text
apps/server/
├── Cargo.toml
└── src/main.rs

crates/application/
├── Cargo.toml
└── src/lib.rs
```

## 模块文件组织

- 禁止使用 `mod.rs` 作为模块入口文件；模块入口统一使用与模块同名的 `.rs` 文件。
- 模块同时包含自身定义和多个子模块时，父模块放在 `src/<module>.rs`，子模块放在 `src/<module>/` 目录中。
- crate 入口通过 `mod features;` 或 `pub mod features;` 引入 `src/features.rs`；`features.rs` 再声明 `home`、`settings` 等子模块。
- 不要同时创建 `src/features.rs` 与 `src/features/mod.rs`，也不要把父模块自己的类型和函数散落到子模块目录中。
- 移动或新增模块时检查整个 workspace，确保生产源码和测试辅助模块中都没有遗留 `mod.rs`。

推荐结构：

```text
src/
├── main.rs
├── features.rs
└── features/
    ├── home.rs
    ├── settings.rs
    └── tasks.rs
```

```rust
// src/main.rs
mod features;

// src/features.rs
mod home;
mod settings;
mod tasks;
```

禁止结构：

```text
src/
└── features/
    ├── mod.rs
    ├── home.rs
    └── settings.rs
```

## 测试组织

- 所有 Rust 测试用例必须放在对应 crate 的 `tests/` 集成测试目录中，例如 `apps/console/tests/` 或 `crates/desktop/tests/`。
- 生产源码中禁止出现 `#[cfg(test)]`、`mod tests`、测试专用导入、测试专用类型或仅为测试条件编译的逻辑。
- 新增行为或修复缺陷时，优先先编写或调整能够复现预期与失败场景的测试，再实现生产代码。
- 测试 bin crate 时，优先验证命令入口的外部行为，或直接测试它依赖的独立 library crate；不要通过新增同 package 的 `src/lib.rs` 暴露实现细节。
- 需要测试内部行为时，优先通过有真实业务含义的公共 API 或只读查询方法建立边界；不要为了测试把无意义的实现细节公开。
- 公开给集成测试使用的 API 仍然必须遵守中文 rustdoc 规范。
- 测试辅助代码可以放在 `tests/common/` 或测试文件的私有模块中，但不能放回生产源码。
- 文件系统测试使用 `tempfile` 创建自动清理且彼此隔离的临时目录或文件，不要依赖固定路径或共享测试状态。
- 命令行程序的集成测试优先使用 `assert_cmd` 启动真实二进制，并断言退出状态、标准输出和标准错误。

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

## 质量检查

- 完成 Rust 源码或 Cargo 配置改动后，按改动范围运行格式化、静态检查和测试；在标记任务完成、提交或创建 PR 前再次确认结果。
- 默认在 workspace 根目录运行以下命令；若仓库环境暂时无法执行完整 workspace，运行能够覆盖改动的最小 crate 范围，并在交付说明中明确未执行项和原因。

```bash
cargo fmt --all
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p cli --bin xuwecli -- lint --workspace . --deny-warnings
```

- `xuwecli lint` 是本仓库的团队规则门禁，负责补充 rustc 和 Clippy 无法覆盖的 workspace 依赖、crate 组织、中文 rustdoc、测试目录、GPUI 生命周期、技术选型、API 与数据库边界检查。
- 自定义 lint 的完整规则、级别和带原因豁免格式见 `apps/cli/LINTS.md`；确定性 `error` 不允许豁免，启发式 `warning` 必须经过审查后才能局部放行。
- 需要生成或验证开发构建产物时运行 `cargo build --workspace`；修改发布配置、条件编译或仅在优化构建中出现的代码时，额外运行 `cargo build --workspace --release`。
- 修改可执行程序行为时，除自动化测试外，使用 `cargo run -p <package> -- <args>` 和代表性参数验证关键路径。
- 修复 `rustfmt`、编译器、Clippy 或测试报告的全部相关错误和警告，然后重新运行受影响的检查，直到结果通过；不要通过无依据的 `allow` 属性掩盖问题。

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
