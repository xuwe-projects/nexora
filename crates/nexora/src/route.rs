//! 统一路径请求、参数和路由目标类型。

use std::{any::type_name, collections::BTreeMap, ops::Deref};

use percent_encoding::percent_decode_str;
use serde::de::DeserializeOwned;
use thiserror::Error;
use url::{Url, form_urlencoded};

use crate::{FeatureMetadata, WindowMetadata};

/// 路由最终要打开的界面类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteTargetKind {
    /// 在主窗口中打开并参与标签与历史的 Feature。
    Feature,
    /// 在独立原生窗口中打开的 Window。
    Window,
}

/// 一条已注册路径对应的具体目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteTarget {
    /// 指向业务 Feature，并携带生成导航所需的静态描述。
    Feature(
        /// 与该路由绑定的 Feature 静态元数据。
        FeatureMetadata,
    ),
    /// 指向独立窗口，并携带窗口标题等静态描述。
    Window(
        /// 与该路由绑定的独立窗口静态元数据。
        WindowMetadata,
    ),
}

impl RouteTarget {
    /// 返回目标属于 Feature 还是独立 Window。
    pub const fn kind(self) -> RouteTargetKind {
        match self {
            Self::Feature(_) => RouteTargetKind::Feature,
            Self::Window(_) => RouteTargetKind::Window,
        }
    }

    /// 返回目标的稳定标识。
    pub const fn id(self) -> &'static str {
        match self {
            Self::Feature(metadata) => metadata.id(),
            Self::Window(metadata) => metadata.id(),
        }
    }

    /// 返回目标的展示标题。
    pub const fn title(self) -> &'static str {
        match self {
            Self::Feature(metadata) => metadata.title(),
            Self::Window(metadata) => metadata.title(),
        }
    }

    /// 返回目标注册时使用的路径模式。
    pub const fn path(self) -> &'static str {
        match self {
            Self::Feature(metadata) => metadata.path(),
            Self::Window(metadata) => metadata.path(),
        }
    }

    /// 返回由具体 UI 层解释的可选图标标识。
    pub const fn icon(self) -> Option<&'static str> {
        match self {
            Self::Feature(metadata) => metadata.icon(),
            Self::Window(metadata) => metadata.icon(),
        }
    }
}

/// 从动态路径参数中反序列化得到的强类型值。
///
/// 该类型采用公开元组字段并实现 [`Deref`]：既可以使用 `Path(value)` 解构，也可以在
/// 包装值上直接访问 `path.id` 等业务字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<T>(
    /// 由当前路由的动态路径参数反序列化得到的业务值。
    pub T,
);

impl<T> Deref for Path<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 从查询字符串中反序列化得到的强类型值。
///
/// 同名查询键会按出现顺序反序列化到 `Vec<T>` 字段，例如 `?tag=a&tag=b` 对应
/// `tag: Vec<String>`。该类型同样实现 [`Deref`]，因此可以直接访问 `query.page`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query<T>(
    /// 由当前路由的查询参数反序列化得到的业务值。
    pub T,
);

impl<T> Deref for Query<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 强类型路由参数提取失败时返回的结构化错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RouteExtractError {
    /// 动态路径参数无法反序列化为调用方指定的类型。
    #[error("无法将路径参数提取为 `{target}` 的字段 `{field}`：{message}")]
    Path {
        /// 调用方请求反序列化到的 Rust 类型名称。
        target: &'static str,
        /// 反序列化失败的字段路径；无法定位到具体字段时为 `.`。
        field: String,
        /// 底层反序列化器给出的具体失败原因。
        message: String,
    },
    /// 查询参数无法反序列化为调用方指定的类型。
    #[error("无法将查询参数提取为 `{target}` 的字段 `{field}`：{message}")]
    Query {
        /// 调用方请求反序列化到的 Rust 类型名称。
        target: &'static str,
        /// 反序列化失败的字段路径；无法定位到具体字段时为 `.`。
        field: String,
        /// 底层反序列化器给出的具体失败原因。
        message: String,
    },
}

struct ExtractFailure {
    field: String,
    message: String,
}

fn deserialize_pairs<'a, T>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<T, ExtractFailure>
where
    T: DeserializeOwned,
{
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    pairs.into_iter().for_each(|(name, value)| {
        serializer.append_pair(name, value);
    });
    let encoded = serializer.finish();
    let deserializer = serde_html_form::Deserializer::from_bytes(encoded.as_bytes());

    serde_path_to_error::deserialize(deserializer).map_err(|error| ExtractFailure {
        field: error.path().to_string(),
        message: error.inner().to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RouteParameters {
    values: BTreeMap<String, String>,
}

impl RouteParameters {
    pub(crate) fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> RouteParameters {
        Self {
            values: pairs.into_iter().collect(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RouteQuery {
    values: BTreeMap<String, Vec<String>>,
}

impl RouteQuery {
    fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut values = BTreeMap::<String, Vec<String>>::new();
        pairs.into_iter().for_each(|(name, value)| {
            values.entry(name).or_default().push(value);
        });
        Self { values }
    }
}

/// 一次成功路径匹配的完整结果。
///
/// `concrete_path` 是标签和窗口实例的默认身份。同一路径会激活已有实例，而
/// `/users/details/1` 与 `/users/details/2` 会自然形成两个不同实例。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatch {
    target: RouteTarget,
    concrete_path: String,
    parameters: RouteParameters,
    query: RouteQuery,
}

impl RouteMatch {
    pub(crate) fn new(
        target: RouteTarget,
        concrete_path: String,
        parameters: RouteParameters,
        query: RouteQuery,
    ) -> Self {
        Self {
            target,
            concrete_path,
            parameters,
            query,
        }
    }

    /// 返回此次匹配要打开的 Feature 或 Window。
    pub const fn target(&self) -> RouteTarget {
        self.target
    }

    /// 返回规范化后的具体路径，可直接作为标签或窗口实例键。
    pub fn concrete_path(&self) -> &str {
        &self.concrete_path
    }

    /// 把动态路径参数反序列化为业务类型并包装为 [`Path<T>`]。
    ///
    /// `T` 使用 [`serde::Deserialize`] 声明路径中的字段名及字段类型。参数值会先执行
    /// UTF-8 percent decoding，再交给业务类型完成字段与类型校验。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nexora::{Path, RouteExtractError, RouteMatch};
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct UserPath {
    ///     id: u64,
    /// }
    ///
    /// fn read_route(route: &RouteMatch) -> Result<u64, RouteExtractError> {
    ///     let Path(path) = route.path::<UserPath>()?;
    ///     Ok(path.id)
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// 当目标类型要求的字段缺失，或参数值无法转换为字段类型时返回错误。
    pub fn path<T>(&self) -> Result<Path<T>, RouteExtractError>
    where
        T: DeserializeOwned,
    {
        deserialize_pairs(self.parameters.iter())
            .map(Path)
            .map_err(|failure| RouteExtractError::Path {
                target: type_name::<T>(),
                field: failure.field,
                message: failure.message,
            })
    }

    /// 把查询参数反序列化为业务类型并包装为 [`Query<T>`]。
    ///
    /// `T` 使用 [`serde::Deserialize`] 声明查询键与类型。同名查询键会按出现顺序
    /// 反序列化到 `Vec<T>` 字段；可选键可以使用 `Option<T>` 或 `#[serde(default)]` 表达。
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use nexora::{Query, RouteExtractError, RouteMatch};
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct UserQuery {
    ///     page: u32,
    ///     #[serde(default)]
    ///     tag: Vec<String>,
    /// }
    ///
    /// fn read_route(route: &RouteMatch) -> Result<(u32, Vec<String>), RouteExtractError> {
    ///     let Query(query) = route.query::<UserQuery>()?;
    ///     Ok((query.page, query.tag))
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// 当目标类型要求的字段缺失，或参数值无法转换为字段类型时返回错误。
    pub fn query<T>(&self) -> Result<Query<T>, RouteExtractError>
    where
        T: DeserializeOwned,
    {
        let pairs = self.query.values.iter().flat_map(|(name, values)| {
            values
                .iter()
                .map(move |value| (name.as_str(), value.as_str()))
        });
        deserialize_pairs(pairs)
            .map(Query)
            .map_err(|failure| RouteExtractError::Query {
                target: type_name::<T>(),
                field: failure.field,
                message: failure.message,
            })
    }
}

/// 解析或匹配路径请求时可能发生的错误。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// 输入既不是以 `/` 开头的内部路径，也不是合法的绝对 URI。
    #[error("无法解析路由位置 `{location}`：{message}")]
    InvalidLocation {
        /// 调用方传入的原始位置。
        location: String,
        /// 解析器返回的具体原因。
        message: String,
    },
    /// 动态路径参数在 percent decoding 后不是有效的 UTF-8 文本。
    #[error("路径参数 `{parameter}` 解码后不是有效的 UTF-8 文本")]
    InvalidParameterEncoding {
        /// 解码失败的参数名。
        parameter: String,
    },
    /// 注册表中没有任何 Feature 或 Window 匹配规范化路径。
    #[error("没有找到与路径 `{path}` 匹配的 Feature 或 Window")]
    NotFound {
        /// 已完成 URI 规范化的内部路径。
        path: String,
    },
}

pub(crate) struct ParsedLocation {
    pub(crate) path: String,
    pub(crate) query: RouteQuery,
}

pub(crate) fn parse_location(location: &str) -> Result<ParsedLocation, ResolveError> {
    if location.starts_with('/') {
        return parse_internal_location(location);
    }

    let url = Url::parse(location).map_err(|error| ResolveError::InvalidLocation {
        location: location.to_owned(),
        message: error.to_string(),
    })?;
    let mut path = String::new();
    if let Some(host) = url.host_str() {
        path.push('/');
        path.push_str(host);
    }
    let url_path = url.path();
    if !url_path.is_empty() && url_path != "/" {
        if !url_path.starts_with('/') {
            path.push('/');
        }
        path.push_str(url_path);
    }
    if path.is_empty() {
        path.push('/');
    }

    Ok(ParsedLocation {
        path: canonical_path(&path).map_err(|message| ResolveError::InvalidLocation {
            location: location.to_owned(),
            message,
        })?,
        query: RouteQuery::from_pairs(
            url.query_pairs()
                .map(|(name, value)| (name.into_owned(), value.into_owned())),
        ),
    })
}

fn parse_internal_location(location: &str) -> Result<ParsedLocation, ResolveError> {
    let without_fragment = location.split_once('#').map_or(location, |(path, _)| path);
    let (path, query) = without_fragment
        .split_once('?')
        .map_or((without_fragment, ""), |(path, query)| (path, query));
    if path.is_empty() {
        return Err(ResolveError::InvalidLocation {
            location: location.to_owned(),
            message: "内部路径不能为空".to_owned(),
        });
    }

    Ok(ParsedLocation {
        path: canonical_path(path).map_err(|message| ResolveError::InvalidLocation {
            location: location.to_owned(),
            message,
        })?,
        query: RouteQuery::from_pairs(
            form_urlencoded::parse(query.as_bytes())
                .map(|(name, value)| (name.into_owned(), value.into_owned())),
        ),
    })
}

fn canonical_path(path: &str) -> Result<String, String> {
    let path = if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    };
    let mut canonical = String::with_capacity(path.len());
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            canonical.push('/');
        }
        canonical.push_str(&canonical_segment(segment)?);
    }
    Ok(canonical)
}

pub(crate) fn canonical_segment(segment: &str) -> Result<String, String> {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(format!("路径段 `{segment}` 包含不完整的 percent encoding"));
        }
        let Some(high) = hex_value(bytes[index + 1]) else {
            return Err(format!("路径段 `{segment}` 包含非法的 percent encoding"));
        };
        let Some(low) = hex_value(bytes[index + 2]) else {
            return Err(format!("路径段 `{segment}` 包含非法的 percent encoding"));
        };
        decoded.push((high << 4) | low);
        index += 3;
    }

    let mut canonical = String::with_capacity(segment.len());
    for byte in decoded {
        if is_unreserved(byte) {
            canonical.push(char::from(byte));
        } else {
            canonical.push('%');
            canonical.push(HEX[usize::from(byte >> 4)] as char);
            canonical.push(HEX[usize::from(byte & 0x0f)] as char);
        }
    }
    Ok(canonical)
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn decode_parameter(name: &str, value: &str) -> Result<String, ResolveError> {
    percent_decode_str(value)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| ResolveError::InvalidParameterEncoding {
            parameter: name.to_owned(),
        })
}
