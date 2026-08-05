# API 契约约定

## 目标

同一套公开类型同时服务于 Axum handler、客户端 SDK 和接口测试，避免每个调用方重复定义请求与响应结构，同时保持数据库模型私有。

## 存放位置

所有跨 HTTP 边界的公共数据类型统一放在 `crates/contracts`：

```text
crates/contracts/src/
├── contracts.rs
├── account.rs
├── warehouse.rs
└── error.rs
```

按业务域拆分文件。crate 根模块公开业务模块；SDK 通过 `contracts::<module>::...` 直接使用相同类型。

## 类型职责

- 请求 DTO：例如 `CreateAccountRequest`，同时派生 `Deserialize` 和 `Serialize`，服务端负责反序列化，SDK 负责序列化。
- 响应 DTO：例如 `AccountResponse`，同时派生 `Serialize` 和 `Deserialize`，服务端负责序列化，SDK 负责反序列化。
- 错误 DTO：使用公开的统一结构，例如 `ErrorResponse { code, message }`；业务模块内部错误枚举仍留在各模块。
- 数据库 Entity：只存在于 `modules/<module>/src/entities/`，保持 `pub(crate)`，不属于公共契约。

## 依赖方向

```text
SDK ───────────────> crates/contracts
modules/<module> ──> crates/contracts
apps/<app> ────────> modules/<module>
```

`crates/contracts` 只能依赖序列化等协议层基础库。禁止依赖 Axum、SQLx、业务模块或后端应用。

## 映射规则

store 返回私有 Entity，handler 显式转换为公开响应 DTO：

```rust
fn account_response(entity: Account) -> AccountResponse {
    AccountResponse {
        id: entity.id,
        username: entity.username,
        nickname: entity.nickname,
    }
}
```

禁止以下做法：

- 在 handler 文件中定义未来 SDK 也需要的请求或响应结构；
- 为了直接 `Json(entity)` 而给数据库 Entity 派生 `Serialize`；
- 在公共 DTO 上派生 `sqlx::FromRow`；
- 让 SDK 依赖业务模块或 SQLx；
- 因数据库新增内部字段而无意改变接口 JSON。

## HTTP wire 格式硬性约束

- JSON 字段、query 参数、path 参数和 form 字段统一使用 `snake_case`，例如 `user_id`、`created_at`；禁止输出 `userId`、`UserID` 或 `user-id` 字段。
- URI 静态路径段继续使用小写连字符；路径占位符使用 `snake_case`，例如 `/user-grants/{user_id}`。标准 HTTP header 名称遵循协议约定，不改写为下划线格式。
- 请求和响应中的枚举值统一使用 `snake_case`，Rust 契约枚举显式添加 `#[serde(rename_all = "snake_case")]`。
- 所有 HTTP 请求和响应时间字段统一使用有符号 Unix 秒时间戳，以 JSON integer 传输；禁止使用 RFC 3339 字符串或毫秒、微秒、纳秒时间戳。
- 数据库 Entity 和内部领域模型可以使用 `TIMESTAMPTZ`、`DateTime<Utc>`；在 HTTP 边界使用 `DateTime::timestamp()` 转为 `i64` 秒，并在接收请求时校验时间戳范围。
- 时间字段仍按业务语义命名为 `created_at`、`updated_at`、`expires_at`，不要使用含糊的 `time`，也不要通过字段名暗示另一种精度。

公共契约示例：

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 会员关系在公开 API 中使用的稳定状态。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    /// 已提交并等待审核。
    PendingReview,
    /// 已启用并允许正常使用。
    Active,
    /// 已暂停且暂时不可使用。
    Suspended,
}

/// 创建会员关系的请求。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CreateMembershipRequest {
    /// 要建立会员关系的用户 ID。
    pub user_id: Uuid,
    /// 新会员关系的初始状态。
    pub status: MembershipStatus,
    /// 会员关系过期时间的 Unix 秒时间戳。
    pub expires_at: i64,
}

/// API 返回的会员关系。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct MembershipResponse {
    /// 会员关系所属的用户 ID。
    pub user_id: Uuid,
    /// 当前会员状态。
    pub status: MembershipStatus,
    /// 创建时间的 Unix 秒时间戳。
    pub created_at: i64,
    /// 最后更新时间的 Unix 秒时间戳。
    pub updated_at: i64,
}
```

## 演进规则

- API 字段变更以兼容客户端为前提，数据库字段可以通过映射独立变化。
- 只把接口真正承诺的字段放进响应 DTO，密码摘要、内部状态、审计字段等不得暴露。
- 输入校验可以在 handler 或独立业务服务中完成，但校验失败的响应体仍使用公共错误 DTO。
- 若接口版本的结构不兼容，创建明确的版本化 DTO，禁止偷偷复用语义已经变化的旧类型。
