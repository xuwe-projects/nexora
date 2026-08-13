//! Shell 全局搜索的 Provider 契约与弹层运行时。
//!
//! Provider 只负责按请求返回分组结果，Shell 负责输入、并发、revision、键盘选择、
//! 局部错误和弹层生命周期。该边界不建立第二套路由表；内置页面 Provider 由应用 Shell
//! 根据现有 [`crate::AppRegistry`] 动态构造。

use std::{collections::HashMap, rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    alert::Alert,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    scroll::ScrollableElement as _,
    spinner::Spinner,
    v_flex,
};
use serde::{Deserialize, Serialize};

/// 全局搜索的运行模式。
///
/// `Global` 允许页面、命令和业务资源等全部 Provider；`OpenPage` 用于 Tabs 加号，应用
/// Provider 只有显式声明支持该模式才会收到请求。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// 全局搜索模式。
    Global,
    /// 仅打开可导航页面的模式。
    OpenPage,
    /// 应用定义的稳定扩展模式。
    Custom(
        /// 应用定义并负责保持稳定的模式 ID。
        String,
    ),
}

impl SearchMode {
    /// 返回适合持久化和组合搜索历史键的稳定标识。
    pub fn id(&self) -> &str {
        match self {
            Self::Global => "global",
            Self::OpenPage => "open_page",
            Self::Custom(id) => id.as_str(),
        }
    }

    fn owns_id(&self) -> String {
        match self {
            Self::Global => "global".to_owned(),
            Self::OpenPage => "open_page".to_owned(),
            Self::Custom(id) => id.clone(),
        }
    }

    fn from_id(id: String) -> Self {
        match id.as_str() {
            "global" => Self::Global,
            "open_page" => Self::OpenPage,
            _ => Self::Custom(id),
        }
    }
}

/// 一次 Provider 搜索请求的不可变上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    /// 当前输入文本；允许为空。
    pub query: String,
    /// 当前搜索模式。
    pub mode: SearchMode,
    /// 当前弹层内单调递增的请求修订号。
    pub revision: u64,
    /// Account 稳定用户 ID；没有安装 Account 时为 `anonymous`。
    pub account_id: String,
}

/// 搜索项执行成功后交给 Shell 处理的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchAction {
    /// 关闭搜索弹层。
    Close,
    /// 保持弹层与当前结果不变。
    KeepOpen,
    /// 替换输入并触发一次 `on_change`，不触发 `on_search`。
    ReplaceQuery(
        /// 写回搜索输入框的新查询文本。
        String,
    ),
    /// 保留输入与模式，只刷新当前项所属 Provider。
    RefreshProvider,
    /// 保留输入与模式，刷新全部 Provider。
    RefreshAll,
}

/// 搜索项异步动作失败时展示的安全错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SearchActionError {
    message: String,
    retryable: bool,
}

impl SearchActionError {
    /// 创建一个可安全展示给用户的动作错误。
    pub fn new(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            message: message.into(),
            retryable,
        }
    }

    /// 返回安全用户消息。
    pub fn message(&self) -> &str {
        self.message.as_str()
    }

    /// 返回当前项是否允许用户重试。
    pub const fn retryable(&self) -> bool {
        self.retryable
    }
}

/// 单个 Provider 请求失败时展示的安全错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SearchProviderError {
    message: String,
    retryable: bool,
}

impl SearchProviderError {
    /// 创建一个可安全展示给用户的 Provider 错误。
    pub fn new(message: impl Into<String>, retryable: bool) -> Self {
        Self {
            message: message.into(),
            retryable,
        }
    }

    /// 返回安全用户消息。
    pub fn message(&self) -> &str {
        self.message.as_str()
    }

    /// 返回 Provider 是否允许用户重试。
    pub const fn retryable(&self) -> bool {
        self.retryable
    }
}

type SearchItemHandler = Rc<
    dyn Fn(&SearchRequest, &mut Window, &mut App) -> Task<Result<SearchAction, SearchActionError>>,
>;

/// Provider 返回的一个可执行搜索项。
#[derive(Clone)]
pub struct SearchItem {
    provider_id: String,
    item_id: String,
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<Icon>,
    disabled: bool,
    loading: bool,
    on_activate: SearchItemHandler,
}

impl SearchItem {
    /// 创建搜索项。
    ///
    /// `provider_id + item_id` 必须在当前模式中稳定，成功动作会使用它写入搜索历史。
    pub fn new(
        provider_id: impl Into<String>,
        item_id: impl Into<String>,
        title: impl Into<SharedString>,
        on_activate: impl Fn(
            &SearchRequest,
            &mut Window,
            &mut App,
        ) -> Task<Result<SearchAction, SearchActionError>>
        + 'static,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            item_id: item_id.into(),
            title: title.into(),
            description: None,
            icon: None,
            disabled: false,
            loading: false,
            on_activate: Rc::new(on_activate),
        }
    }

    /// 设置次要说明文本。
    #[must_use]
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 设置搜索项图标。
    #[must_use]
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// 设置搜索项禁用状态。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置 Provider 自身声明的加载状态。
    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// 返回所属 Provider 的稳定 ID。
    pub fn provider_id(&self) -> &str {
        self.provider_id.as_str()
    }

    /// 返回搜索项稳定 ID。
    pub fn item_id(&self) -> &str {
        self.item_id.as_str()
    }

    /// 返回搜索项标题。
    pub fn title(&self) -> &SharedString {
        &self.title
    }
}

/// Provider 返回的一个结果分区。
#[derive(Clone)]
pub struct SearchSection {
    section_id: String,
    title: SharedString,
    items: Vec<SearchItem>,
}

impl SearchSection {
    /// 创建具有稳定 ID 和展示标题的结果分区。
    pub fn new(section_id: impl Into<String>, title: impl Into<SharedString>) -> Self {
        Self {
            section_id: section_id.into(),
            title: title.into(),
            items: Vec::new(),
        }
    }

    /// 追加一个搜索项。
    #[must_use]
    pub fn item(mut self, item: SearchItem) -> Self {
        self.items.push(item);
        self
    }

    /// 追加多个搜索项。
    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = SearchItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// 返回分区稳定 ID。
    pub fn section_id(&self) -> &str {
        self.section_id.as_str()
    }

    /// 返回分区标题。
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// 返回当前分区中的搜索项。
    pub fn search_items(&self) -> &[SearchItem] {
        &self.items
    }
}

type SearchProviderHandler = Rc<
    dyn Fn(
        SearchRequest,
        &mut Window,
        &mut App,
    ) -> Task<Result<Vec<SearchSection>, SearchProviderError>>,
>;
type ResolveHistoryHandler = Rc<
    dyn Fn(
        SearchMode,
        String,
        &mut Window,
        &mut App,
    ) -> Task<Result<Option<SearchItem>, SearchProviderError>>,
>;

/// 可安装到 Shell 的全局搜索 Provider。
#[derive(Clone)]
pub struct SearchProvider {
    provider_id: String,
    order: i32,
    modes: Vec<SearchMode>,
    debounce: Duration,
    on_change: Option<SearchProviderHandler>,
    on_search: Option<SearchProviderHandler>,
    on_resolve_history: Option<ResolveHistoryHandler>,
}

impl SearchProvider {
    /// 创建 Provider；默认支持全局模式、无 debounce，且没有请求回调。
    pub fn new(provider_id: impl Into<String>, order: i32) -> Self {
        Self {
            provider_id: provider_id.into(),
            order,
            modes: vec![SearchMode::Global],
            debounce: Duration::ZERO,
            on_change: None,
            on_search: None,
            on_resolve_history: None,
        }
    }

    /// 设置 Provider 支持的模式。
    #[must_use]
    pub fn modes(mut self, modes: impl IntoIterator<Item = SearchMode>) -> Self {
        self.modes = modes.into_iter().collect();
        self
    }

    /// 设置输入变化请求的 debounce；默认不等待。
    #[must_use]
    pub fn debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    /// 设置每次输入变化时执行的异步查询，包括空字符串。
    #[must_use]
    pub fn on_change(
        mut self,
        handler: impl Fn(
            SearchRequest,
            &mut Window,
            &mut App,
        ) -> Task<Result<Vec<SearchSection>, SearchProviderError>>
        + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// 设置没有激活项时按 Enter 执行的提交式异步查询。
    #[must_use]
    pub fn on_search(
        mut self,
        handler: impl Fn(
            SearchRequest,
            &mut Window,
            &mut App,
        ) -> Task<Result<Vec<SearchSection>, SearchProviderError>>
        + 'static,
    ) -> Self {
        self.on_search = Some(Rc::new(handler));
        self
    }

    /// 设置跨重启搜索历史解析回调。
    #[must_use]
    pub fn on_resolve_history(
        mut self,
        handler: impl Fn(
            SearchMode,
            String,
            &mut Window,
            &mut App,
        ) -> Task<Result<Option<SearchItem>, SearchProviderError>>
        + 'static,
    ) -> Self {
        self.on_resolve_history = Some(Rc::new(handler));
        self
    }

    /// 返回 Provider 稳定 ID。
    pub fn provider_id(&self) -> &str {
        self.provider_id.as_str()
    }

    /// 返回 Provider 排序值；数值越小越靠前。
    pub const fn order(&self) -> i32 {
        self.order
    }

    /// 返回 Provider 是否支持指定模式。
    pub fn supports(&self, mode: &SearchMode) -> bool {
        self.modes.iter().any(|candidate| candidate == mode)
    }

    /// 返回 Provider 是否可以解析跨重启历史。
    pub fn resolves_history(&self) -> bool {
        self.on_resolve_history.is_some()
    }
}

#[derive(Clone, Default)]
pub(crate) struct SearchProviderRegistry {
    providers: Vec<SearchProvider>,
}

impl gpui::Global for SearchProviderRegistry {}

/// 安装应用级全局搜索 Provider。
///
/// 后一次安装完整替换前一次列表；内置页面 Provider 不受影响。重复 Provider ID 会
/// panic，避免结果与历史身份不确定。
///
/// # Panics
///
/// `providers` 中存在重复稳定 ID 时 panic，避免搜索结果和历史项无法确定归属。
pub fn install_search_providers(mut providers: Vec<SearchProvider>, cx: &mut App) {
    providers.sort_by(|left, right| {
        left.order()
            .cmp(&right.order())
            .then_with(|| left.provider_id().cmp(right.provider_id()))
    });
    for pair in providers.windows(2) {
        assert_ne!(
            pair[0].provider_id(),
            pair[1].provider_id(),
            "SearchProvider ID 不能重复"
        );
    }
    let registry = SearchProviderRegistry { providers };
    if cx.has_global::<SearchProviderRegistry>() {
        *cx.global_mut::<SearchProviderRegistry>() = registry;
    } else {
        cx.set_global(registry);
    }
}

pub(crate) fn installed_search_providers(cx: &App) -> Vec<SearchProvider> {
    cx.try_global::<SearchProviderRegistry>()
        .map(|registry| registry.providers.clone())
        .unwrap_or_default()
}

/// 搜索历史持久化所需的最小身份记录。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SearchHistoryEntry {
    /// 执行成功时使用的搜索模式。
    pub mode: SearchMode,
    /// 执行成功的 Provider ID。
    pub provider_id: String,
    /// 执行成功的搜索项 ID。
    pub item_id: String,
}

impl SearchHistoryEntry {
    pub(crate) fn stable_key(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}",
            self.mode.owns_id(),
            self.provider_id,
            self.item_id
        )
    }

    pub(crate) fn from_stable_key(value: &str) -> Option<Self> {
        let mut parts = value.split('\u{1f}');
        let mode = parts.next()?.to_owned();
        let provider_id = parts.next()?.to_owned();
        let item_id = parts.next()?.to_owned();
        if provider_id.is_empty() || item_id.is_empty() || parts.next().is_some() {
            return None;
        }
        Some(Self {
            mode: SearchMode::from_id(mode),
            provider_id,
            item_id,
        })
    }
}

#[derive(Default)]
struct ProviderViewState {
    sections: Vec<SearchSection>,
    loading: bool,
    error: Option<SearchProviderError>,
}

enum ProviderRequestKind {
    Change,
    Search,
}

/// 官方 Dialog 内渲染的搜索状态宿主。
pub(crate) struct SearchDialog {
    mode: SearchMode,
    account_id: String,
    input: Entity<InputState>,
    providers: Vec<SearchProvider>,
    provider_states: HashMap<String, ProviderViewState>,
    query: String,
    revision: u64,
    active_index: Option<usize>,
    provider_tasks: HashMap<String, Task<()>>,
    history_tasks: Vec<Task<()>>,
    history_items: Vec<(usize, SearchItem)>,
    history_entries: Vec<SearchHistoryEntry>,
    item_task: Option<Task<()>>,
    loading_item: Option<(String, String)>,
    item_errors: HashMap<(String, String), SearchActionError>,
    _input_subscription: Subscription,
}

impl SearchDialog {
    pub(crate) fn new(
        mode: SearchMode,
        account_id: String,
        mut providers: Vec<SearchProvider>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        providers.retain(|provider| provider.supports(&mode));
        providers.sort_by(|left, right| {
            left.order()
                .cmp(&right.order())
                .then_with(|| left.provider_id().cmp(right.provider_id()))
        });
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match &mode {
                SearchMode::OpenPage => "搜索可打开的页面",
                _ => "搜索页面、命令或业务资源",
            })
        });
        let _input_subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::Change => {
                    this.query = input.read(cx).value().to_string();
                    this.request_all(ProviderRequestKind::Change, window, cx);
                }
                InputEvent::PressEnter { .. } => {
                    if let Some(index) = this.active_index {
                        this.activate_item(index, window, cx);
                    } else {
                        this.request_all(ProviderRequestKind::Search, window, cx);
                    }
                }
                InputEvent::Focus | InputEvent::Blur => {}
            },
        );
        let provider_states = providers
            .iter()
            .map(|provider| {
                (
                    provider.provider_id().to_owned(),
                    ProviderViewState::default(),
                )
            })
            .collect();
        let history_entries =
            crate::application::search_history_for_account(account_id.as_str(), cx)
                .into_iter()
                .filter(|entry| entry.mode == mode)
                .collect();

        Self {
            mode,
            account_id,
            input,
            providers,
            provider_states,
            query: String::new(),
            revision: 0,
            active_index: None,
            provider_tasks: HashMap::new(),
            history_tasks: Vec::new(),
            history_items: Vec::new(),
            history_entries,
            item_task: None,
            loading_item: None,
            item_errors: HashMap::new(),
            _input_subscription,
        }
    }

    pub(crate) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_all(ProviderRequestKind::Change, window, cx);
        self.resolve_history(window, cx);
        self.input.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn resolve_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_tasks.clear();
        self.history_items.clear();
        for (index, entry) in self.history_entries.iter().cloned().enumerate() {
            let Some(resolver) = self
                .providers
                .iter()
                .find(|provider| provider.provider_id() == entry.provider_id)
                .and_then(|provider| provider.on_resolve_history.clone())
            else {
                continue;
            };
            let account_id = self.account_id.clone();
            self.history_tasks.push(cx.spawn_in(
                window,
                async move |this: WeakEntity<Self>, cx| {
                    let task = this.update_in(cx, |_, window, cx| {
                        resolver(entry.mode.clone(), entry.item_id.clone(), window, cx)
                    });
                    let Ok(task) = task else {
                        return;
                    };
                    let result = task.await;
                    _ = this.update_in(cx, |this, _, cx| {
                        match result {
                            Ok(Some(item)) if item.provider_id() == entry.provider_id => {
                                this.history_items.push((index, item));
                                this.history_items.sort_by_key(|(index, _)| *index);
                            }
                            Ok(_) => {
                                crate::application::remove_search_history(
                                    account_id.as_str(),
                                    &entry,
                                    cx,
                                );
                                this.history_entries.retain(|candidate| candidate != &entry);
                            }
                            Err(error) => {
                                if let Some(state) =
                                    this.provider_states.get_mut(entry.provider_id.as_str())
                                {
                                    state.error = Some(error);
                                }
                            }
                        }
                        cx.notify();
                    });
                },
            ));
        }
    }

    fn request(&self) -> SearchRequest {
        SearchRequest {
            query: self.query.clone(),
            mode: self.mode.clone(),
            revision: self.revision,
            account_id: self.account_id.clone(),
        }
    }

    fn request_all(
        &mut self,
        kind: ProviderRequestKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.revision = self.revision.saturating_add(1);
        self.active_index = None;
        self.item_errors.clear();
        let provider_ids = self
            .providers
            .iter()
            .map(|provider| provider.provider_id().to_owned())
            .collect::<Vec<_>>();
        for provider_id in provider_ids {
            self.request_provider(provider_id.as_str(), &kind, window, cx);
        }
        cx.notify();
    }

    fn request_provider(
        &mut self,
        provider_id: &str,
        kind: &ProviderRequestKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self
            .providers
            .iter()
            .find(|provider| provider.provider_id() == provider_id)
        else {
            return;
        };
        let handler = match kind {
            ProviderRequestKind::Change => provider.on_change.clone(),
            ProviderRequestKind::Search => provider.on_search.clone(),
        };
        let Some(handler) = handler else {
            return;
        };
        let debounce = matches!(kind, ProviderRequestKind::Change)
            .then_some(provider.debounce)
            .unwrap_or_default();
        let request = self.request();
        let provider_id = provider_id.to_owned();
        if let Some(state) = self.provider_states.get_mut(provider_id.as_str()) {
            state.loading = true;
            state.error = None;
        }
        self.provider_tasks.remove(provider_id.as_str());
        let timer = cx.background_executor().timer(debounce);
        let task_provider_id = provider_id.clone();
        let task = cx.spawn_in(window, async move |this: WeakEntity<Self>, cx| {
            timer.await;
            let result_task =
                this.update_in(cx, |_, window, cx| handler(request.clone(), window, cx));
            let Ok(result_task) = result_task else {
                return;
            };
            let result = result_task.await;
            _ = this.update_in(cx, |this, _, cx| {
                if request.revision != this.revision {
                    return;
                }
                if let Some(state) = this.provider_states.get_mut(task_provider_id.as_str()) {
                    state.loading = false;
                    match result {
                        Ok(sections) => {
                            state.sections = sections;
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error),
                    }
                }
                this.provider_tasks.remove(task_provider_id.as_str());
                cx.notify();
            });
        });
        self.provider_tasks.insert(provider_id, task);
    }

    fn flattened_items(&self) -> Vec<SearchItem> {
        let mut items = if self.query.trim().is_empty() {
            self.history_items
                .iter()
                .map(|(_, item)| item.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        items.extend(
            self.providers
                .iter()
                .filter_map(|provider| self.provider_states.get(provider.provider_id()))
                .flat_map(|state| state.sections.iter())
                .flat_map(|section| section.items.iter().cloned()),
        );
        items
    }

    fn move_selection(&mut self, offset: isize, cx: &mut Context<Self>) {
        let item_count = self.flattened_items().len();
        if item_count == 0 {
            self.active_index = None;
            return;
        }
        let current =
            self.active_index
                .map_or(if offset > 0 { 0 } else { item_count - 1 }, |index| {
                    if offset > 0 {
                        (index + 1) % item_count
                    } else if index == 0 {
                        item_count - 1
                    } else {
                        index - 1
                    }
                });
        self.active_index = Some(current);
        cx.notify();
    }

    fn activate_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.item_task.is_some() {
            return;
        }
        let Some(item) = self.flattened_items().get(index).cloned() else {
            return;
        };
        if item.disabled || item.loading {
            return;
        }
        let key = (item.provider_id.clone(), item.item_id.clone());
        self.loading_item = Some(key.clone());
        self.item_errors.remove(&key);
        let request = self.request();
        let action_task = (item.on_activate)(&request, window, cx);
        let account_id = self.account_id.clone();
        self.item_task = Some(
            cx.spawn_in(window, async move |this: WeakEntity<Self>, cx| {
                let result = action_task.await;
                _ = this.update_in(cx, |this, window, cx| {
                    this.item_task = None;
                    this.loading_item = None;
                    match result {
                        Ok(action) => {
                            crate::application::record_search_history(
                                account_id.as_str(),
                                SearchHistoryEntry {
                                    mode: request.mode.clone(),
                                    provider_id: item.provider_id.clone(),
                                    item_id: item.item_id.clone(),
                                },
                                cx,
                            );
                            match action {
                                SearchAction::Close => window.close_dialog(cx),
                                SearchAction::KeepOpen => {}
                                SearchAction::ReplaceQuery(query) => {
                                    this.query = query.clone();
                                    this.input.update(cx, |input, cx| {
                                        input.set_value(query, window, cx);
                                    });
                                    this.request_all(ProviderRequestKind::Change, window, cx);
                                }
                                SearchAction::RefreshProvider => {
                                    this.request_provider(
                                        item.provider_id.as_str(),
                                        &ProviderRequestKind::Change,
                                        window,
                                        cx,
                                    );
                                }
                                SearchAction::RefreshAll => {
                                    this.request_all(ProviderRequestKind::Change, window, cx);
                                }
                            }
                        }
                        Err(error) => {
                            this.item_errors.insert(key, error);
                        }
                    }
                    cx.notify();
                });
            }),
        );
        cx.notify();
    }

    fn render_item(&self, item: SearchItem, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let key = (item.provider_id.clone(), item.item_id.clone());
        let loading = self.loading_item.as_ref() == Some(&key) || item.loading;
        let error = self.item_errors.get(&key).cloned();
        let selected = self.active_index == Some(index);
        let title = item.title.clone();
        let description = item.description.clone();
        let icon = item.icon.clone();
        let item_disabled = item.disabled;
        v_flex()
            .gap_1()
            .child(
                ListItem::new(format!("search-{}-{}", item.provider_id, item.item_id))
                    .selected(selected)
                    .disabled(item_disabled || loading)
                    .on_mouse_enter(cx.listener(move |this, _, _, cx| {
                        this.active_index = Some(index);
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.activate_item(index, window, cx);
                    }))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .when_some(icon, |this, icon| this.child(icon.small()))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .child(div().truncate().child(title))
                                    .when_some(description, |this, description| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .truncate()
                                                .child(description),
                                        )
                                    }),
                            )
                            .when(loading, |this| this.child(Spinner::new().small())),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(Alert::error(
                            format!("search-item-error-{}-{}", key.0, key.1),
                            error.message().to_owned(),
                        ))
                        .when(error.retryable(), |this| {
                            this.child(
                                Button::new(format!("retry-search-item-{}-{}", key.0, key.1))
                                    .outline()
                                    .xsmall()
                                    .label("重试")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.activate_item(index, window, cx);
                                    })),
                            )
                        }),
                )
            })
            .into_any_element()
    }
}

impl Render for SearchDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut global_index = 0_usize;
        let mut sections = Vec::new();
        if self.query.trim().is_empty() && !self.history_items.is_empty() {
            let history_items = self
                .history_items
                .iter()
                .map(|(_, item)| {
                    let index = global_index;
                    global_index += 1;
                    self.render_item(item.clone(), index, cx)
                })
                .collect::<Vec<_>>();
            let account_id = self.account_id.clone();
            sections.push(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .px_2()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("最近使用"),
                            )
                            .child(
                                Button::new("clear-search-history")
                                    .ghost()
                                    .xsmall()
                                    .label("清空")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        crate::application::clear_search_history(
                                            account_id.as_str(),
                                            cx,
                                        );
                                        this.history_entries.clear();
                                        this.history_items.clear();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(history_items)
                    .into_any_element(),
            );
        }
        for provider in &self.providers {
            let Some(state) = self.provider_states.get(provider.provider_id()) else {
                continue;
            };
            for section in &state.sections {
                if section.items.is_empty() {
                    continue;
                }
                let items = section
                    .items
                    .iter()
                    .cloned()
                    .map(|item| {
                        let index = global_index;
                        global_index += 1;
                        self.render_item(item, index, cx)
                    })
                    .collect::<Vec<_>>();
                sections.push(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .px_2()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .child(section.title.clone()),
                        )
                        .children(items)
                        .into_any_element(),
                );
            }
            if let Some(error) = state.error.clone() {
                let provider_id = provider.provider_id().to_owned();
                sections.push(
                    v_flex()
                        .gap_1()
                        .child(Alert::error(
                            format!("search-provider-error-{provider_id}"),
                            error.message().to_owned(),
                        ))
                        .when(error.retryable(), |this| {
                            this.child(
                                Button::new(format!("retry-search-provider-{provider_id}"))
                                    .outline()
                                    .xsmall()
                                    .label("重试")
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.request_provider(
                                            provider_id.as_str(),
                                            &ProviderRequestKind::Change,
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                        })
                        .into_any_element(),
                );
            }
        }
        let loading = self.provider_states.values().any(|state| state.loading);
        let empty = global_index == 0 && !loading;

        v_flex()
            .key_context("NexoraGlobalSearch")
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "up" => {
                        cx.stop_propagation();
                        this.move_selection(-1, cx);
                    }
                    "down" => {
                        cx.stop_propagation();
                        this.move_selection(1, cx);
                    }
                    _ => {}
                }
            }))
            .gap_3()
            .min_h(px(280.0))
            .max_h(px(560.0))
            .child(
                Input::new(&self.input)
                    .prefix(Icon::new(IconName::Search).small())
                    .suffix(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Esc"),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .gap_3()
                    .children(sections)
                    .when(loading, |this| {
                        this.child(
                            h_flex()
                                .justify_center()
                                .gap_2()
                                .py_4()
                                .text_color(cx.theme().muted_foreground)
                                .child(Spinner::new().small())
                                .child("正在搜索"),
                        )
                    })
                    .when(empty, |this| {
                        this.child(
                            v_flex()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .py_8()
                                .text_color(cx.theme().muted_foreground)
                                .child(Icon::new(IconName::Search).size_8())
                                .child("没有匹配结果"),
                        )
                    }),
            )
    }
}
