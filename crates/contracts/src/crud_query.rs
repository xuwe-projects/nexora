//! 标准 CRUD 列表共享的查询模型契约。
//!
//! 本模块只描述分页、筛选和排序元数据，不依赖 GPUI。桌面端可以据此选择官方表单控件，
//! 服务端和 HTTP client 则继续直接序列化调用方自己的请求结构体。

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::pagination::PageQuery;

/// 没有声明服务端排序字段的查询使用的零尺寸排序类型。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct NoCrudSort;

/// CRUD 筛选字段推荐使用的官方控件语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrudFilterControl {
    /// 单行文本输入框。
    Input,
    /// 数值输入框。
    NumberInput,
    /// 二元开关。
    Switch,
    /// 固定选项下拉框。
    Select,
    /// 日期选择器。
    DatePicker,
    /// 由业务代码提供的显式渲染适配器。
    Custom,
}

/// CRUD 筛选字段在页面中的展示区域。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CrudFilterPresentation {
    /// 在标准筛选表单中展示。
    #[default]
    Form,
    /// 在标题下方的快速筛选区展示，并且不在标准表单中重复出现。
    Quick,
}

/// CRUD 筛选字段提交查询的触发策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrudFilterTrigger {
    /// 输入停止三百毫秒后提交，按 Enter 时立即提交。
    Debounce {
        /// 停止输入后等待的毫秒数。
        milliseconds: u64,
    },
    /// 控件值变化后立即提交。
    Immediate,
    /// 只更新筛选草稿，由显式查询按钮提交。
    Manual,
}

/// 一个 GPUI 中立的 CRUD 筛选字段描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrudFilterMetadata {
    /// 请求结构体中的稳定字段名，同时也是 HTTP wire 字段名。
    pub name: &'static str,
    /// 用户可见标签。
    pub label: &'static str,
    /// 可选的字段说明。
    pub description: Option<&'static str>,
    /// 可选的控件占位文字。
    pub placeholder: Option<&'static str>,
    /// 推荐控件语义。
    pub control: CrudFilterControl,
    /// 字段展示区域。
    pub presentation: CrudFilterPresentation,
    /// 查询触发策略。
    pub trigger: CrudFilterTrigger,
    /// 字段是否必填。
    pub required: bool,
    /// 必填校验失败时展示的安全消息。
    pub required_message: Option<&'static str>,
    /// 可选的正则表达式约束。
    pub pattern: Option<&'static str>,
    /// 正则约束失败时展示的安全消息。
    pub pattern_message: Option<&'static str>,
    /// 类型转换失败时展示的安全消息。
    pub parse_error: Option<&'static str>,
    /// 建议控件宽度；`None` 表示由官方 Form 布局决定。
    pub width: Option<u16>,
}

/// CRUD 查询的页大小边界和设置页可选值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrudPageSizeMetadata {
    /// 新查询使用的默认页大小。
    pub default: u32,
    /// 服务端允许的最小页大小。
    pub min: u32,
    /// 服务端允许的最大页大小。
    pub max: u32,
    /// 分页控件展示的常用页大小。
    pub options: &'static [u32],
}

/// 一个 CRUD 查询类型的静态元数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrudQueryMetadata {
    /// 页大小约束。
    pub page_size: CrudPageSizeMetadata,
    /// 仅包含显式标记为筛选条件的字段。
    pub filters: &'static [CrudFilterMetadata],
    /// 可选的服务端排序字段名。
    pub sort_field: Option<&'static str>,
}

/// 标准 CRUD 列表使用的强类型请求模型。
///
/// 实现通常由 `#[derive(nexora_contracts::CrudQuery)]` 生成。请求模型仍由调用方拥有，
/// 因而可以同时派生 serde 并维持既有的扁平 HTTP wire 格式。
pub trait CrudQuery: Clone + Serialize + 'static {
    /// 查询中服务端排序枚举的类型；没有排序字段时为 [`NoCrudSort`]。
    type Sort: Clone + PartialEq + Serialize + 'static;

    /// 返回分页字段。
    fn pagination(&self) -> &PageQuery;

    /// 返回可变分页字段，供列表状态归一化、跳页和切换页大小使用。
    fn pagination_mut(&mut self) -> &mut PageQuery;

    /// 返回当前服务端排序值。
    fn sort(&self) -> Option<&Self::Sort>;

    /// 替换服务端排序值。
    fn set_sort(&mut self, sort: Option<Self::Sort>);

    /// 返回派生宏生成的分页、筛选与排序元数据。
    fn metadata() -> &'static CrudQueryMetadata;

    /// 读取一个显式筛选字段的序列化值。
    ///
    /// 未声明为筛选条件的字段返回 `None`，避免桌面端意外把内部请求字段暴露为 UI。
    fn filter_value(&self, name: &str) -> Option<Value>;

    /// 从 JSON 值更新一个显式筛选字段。
    ///
    /// # Errors
    ///
    /// 字段不存在、不是筛选字段或值无法反序列化为字段的强类型时返回安全错误消息。
    fn set_filter_value(&mut self, name: &str, value: Value) -> Result<(), String>;

    /// 将页码和页大小归一化到宏声明的合法边界。
    fn normalize(&mut self) {
        let metadata = Self::metadata().page_size;
        let page = self.pagination_mut();
        page.page = page.page.max(1);
        page.page_size = if page.page_size == 0 {
            metadata.default
        } else {
            page.page_size.clamp(metadata.min, metadata.max)
        };
    }

    /// 返回不包含当前页码的稳定缓存身份。
    ///
    /// 页大小、筛选和排序仍参与身份；因此跳页可以复用缓存，改变筛选、排序或页大小会自然
    /// 进入新的缓存分区。序列化失败只可能来自调用方自定义 serde 实现，此时返回错误。
    ///
    /// # Errors
    ///
    /// 调用方的自定义 [`Serialize`] 实现无法生成 JSON 时返回原始序列化错误。
    fn cache_identity(&self) -> Result<String, serde_json::Error> {
        let mut query = self.clone();
        query.pagination_mut().page = 1;
        serde_json::to_string(&query)
    }
}

/// 将 JSON 筛选值转换为派生字段的强类型。
///
/// 该函数主要供 `CrudQuery` 派生代码调用；公开是为了让生成代码在下游 crate 中无需依赖
/// `serde_json` 的内部实现路径。
///
/// # Errors
///
/// JSON 值与目标字段类型不匹配时返回不包含请求正文的安全错误消息。
#[doc(hidden)]
pub fn decode_filter_value<T>(field: &str, value: Value) -> Result<T, String>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value).map_err(|_| format!("筛选字段 `{field}` 的值类型无效"))
}
