//! HTTP PATCH 请求共享的字段更新语义。

use serde::{Deserialize, Serialize};

/// 局部更新字段的三态值。
///
/// `Missing` 表示请求没有出现该字段，`Null` 表示显式清空字段，`Value` 表示设置新值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PatchField<T> {
    /// 请求没有携带字段，服务端应保留原值。
    #[default]
    Missing,
    /// 请求显式传入 JSON `null`，服务端应清空可选值。
    Null,
    /// 请求携带一个需要写入的新值。
    Value(
        /// 服务端应当用于替换原字段的具体内容。
        T,
    ),
}

impl<T> PatchField<T> {
    /// 判断字段是否完全没有出现在请求中。
    ///
    /// 该方法主要供 Serde 的 `skip_serializing_if` 使用，以保持 PATCH 的缺省语义。
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

impl<T> Serialize for PatchField<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Missing | Self::Null => serializer.serialize_none(),
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for PatchField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<T>::deserialize(deserializer).map(|value| match value {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}
