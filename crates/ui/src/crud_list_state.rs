//! 标准 CRUD 分页列表的查询、缓存、选择与异步加载状态。

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    future::Future,
    pin::Pin,
    rc::Rc,
};

use contracts::crud_query::CrudQuery;
use gpui::{AppContext as _, Context, Entity, SharedString, Task, WeakEntity, Window};
use gpui_component::table::TableState;
use thiserror::Error;

use crate::{CrudTableDelegate, CrudTableRow, CrudTableSelection};

type LoadFuture<R> = Pin<Box<dyn Future<Output = Result<CrudPage<R>, CrudLoadError>>>>;
type LoadHandler<R, Q> = Rc<dyn Fn(Q) -> LoadFuture<R>>;

/// 一次标准 CRUD 分页请求返回的数据。
#[derive(Clone)]
pub struct CrudPage<R> {
    /// 当前页资源。
    pub items: Vec<R>,
    /// 服务端确认的当前页码。
    pub page: u32,
    /// 服务端确认的页大小。
    pub page_size: u32,
    /// 当前查询匹配的总记录数。
    pub total: usize,
    /// 可选快速筛选计数，key 由业务查询契约定义。
    pub quick_filter_counts: BTreeMap<String, u64>,
}

impl<R> CrudPage<R> {
    /// 创建不带快速筛选计数的分页响应。
    pub fn new(items: Vec<R>, page: u32, page_size: u32, total: usize) -> Self {
        Self {
            items,
            page: page.max(1),
            page_size: page_size.max(1),
            total,
            quick_filter_counts: BTreeMap::new(),
        }
    }

    /// 设置本次响应携带的快速筛选计数。
    #[must_use]
    pub fn quick_filter_counts(mut self, counts: impl IntoIterator<Item = (String, u64)>) -> Self {
        self.quick_filter_counts = counts.into_iter().collect();
        self
    }
}

/// 标准 CRUD 数据加载失败时保存的安全用户信息。
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{message}")]
pub struct CrudLoadError {
    message: SharedString,
    request_id: Option<SharedString>,
    retryable: bool,
}

impl CrudLoadError {
    /// 创建可重试的数据加载错误。
    pub fn retryable(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            request_id: None,
            retryable: true,
        }
    }

    /// 创建不可重试的数据加载错误。
    pub fn terminal(message: impl Into<SharedString>) -> Self {
        Self {
            message: message.into(),
            request_id: None,
            retryable: false,
        }
    }

    /// 附加服务端返回的安全 request ID。
    #[must_use]
    pub fn request_id(mut self, request_id: impl Into<SharedString>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// 返回可展示给用户的安全错误消息。
    pub fn message(&self) -> &SharedString {
        &self.message
    }

    /// 返回可用于支持排查的 request ID。
    pub fn request_id_value(&self) -> Option<&SharedString> {
        self.request_id.as_ref()
    }

    /// 返回当前请求是否允许重试。
    pub const fn is_retryable(&self) -> bool {
        self.retryable
    }
}

/// 创建 CRUD 列表状态时返回的契约错误。
#[derive(Debug, Error)]
pub enum CrudListStateError {
    /// 查询无法生成稳定缓存身份。
    #[error("无法生成 CRUD 查询缓存身份")]
    InvalidCacheIdentity(#[source] serde_json::Error),
}

struct FailedPage<Q> {
    query: Q,
    error: CrudLoadError,
}

/// 标准单主数据集列表的查询、页缓存、选择和请求生命周期。
///
/// 页面应在 `FeatureElement::initialize` 中通过 [`Self::create`] 创建 Entity，并显式调用
/// [`Self::load_current`] 发起首个请求。渲染阶段只读取本状态和 [`Self::table_state`]。
pub struct CrudListState<R, Q>
where
    R: CrudTableRow,
    Q: CrudQuery<Sort = R::Sort>,
{
    query: Q,
    default_sort: Option<Q::Sort>,
    cache_identity: String,
    pages: BTreeMap<u32, Vec<R>>,
    total: usize,
    quick_filter_counts: BTreeMap<String, u64>,
    selected_ids: HashSet<R::Id>,
    requested_page: Option<u32>,
    loading_pages: BTreeSet<u32>,
    failures: BTreeMap<u32, FailedPage<Q>>,
    visible_failure_page: Option<u32>,
    revision: u64,
    loaded_once: bool,
    loader: LoadHandler<R, Q>,
    tasks: BTreeMap<u32, Task<()>>,
    table_state: Entity<TableState<CrudTableDelegate<R>>>,
}

impl<R, Q> CrudListState<R, Q>
where
    R: CrudTableRow,
    Q: CrudQuery<Sort = R::Sort>,
{
    /// 创建使用默认 `CrudTableDelegate` 的列表状态 Entity。
    ///
    /// # Errors
    ///
    /// 查询的自定义 `Serialize` 实现无法生成缓存身份时返回错误，且不会创建 Entity。
    pub fn create<T, F, Fut>(
        query: Q,
        on_load: F,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Result<Entity<Self>, CrudListStateError>
    where
        T: 'static,
        F: Fn(Q) -> Fut + 'static,
        Fut: Future<Output = Result<CrudPage<R>, CrudLoadError>> + 'static,
    {
        Self::create_with_delegate(query, on_load, |delegate| delegate, false, window, cx)
    }

    /// 创建启用跨页强类型选择的列表状态 Entity。
    ///
    /// 表头全选只作用于当前已加载页；翻页不会清空选择，查询身份变化会清空选择。
    ///
    /// # Errors
    ///
    /// 查询无法生成稳定缓存身份时返回错误。
    pub fn create_selectable<T, F, Fut>(
        query: Q,
        on_load: F,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Result<Entity<Self>, CrudListStateError>
    where
        T: 'static,
        F: Fn(Q) -> Fut + 'static,
        Fut: Future<Output = Result<CrudPage<R>, CrudLoadError>> + 'static,
    {
        Self::create_with_delegate(query, on_load, |delegate| delegate, true, window, cx)
    }

    /// 使用调用方配置过操作列和空状态的 delegate 创建列表状态 Entity。
    ///
    /// `configure_delegate` 只在初始化阶段执行一次，不能改变 `Row` 和 `Query` 的强类型边界。
    ///
    /// # Errors
    ///
    /// 查询无法生成稳定缓存身份时返回错误。
    pub fn create_with_delegate<T, F, Fut, C>(
        mut query: Q,
        on_load: F,
        configure_delegate: C,
        selectable: bool,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Result<Entity<Self>, CrudListStateError>
    where
        T: 'static,
        F: Fn(Q) -> Fut + 'static,
        Fut: Future<Output = Result<CrudPage<R>, CrudLoadError>> + 'static,
        C: FnOnce(CrudTableDelegate<R>) -> CrudTableDelegate<R> + 'static,
    {
        query.normalize();
        let default_sort = query.sort().cloned();
        let cache_identity = query
            .cache_identity()
            .map_err(CrudListStateError::InvalidCacheIdentity)?;
        let loader: LoadHandler<R, Q> = Rc::new(move |query| Box::pin(on_load(query)));

        Ok(cx.new(move |state_cx| {
            let weak_state: WeakEntity<Self> = state_cx.entity().downgrade();
            let visible_state = weak_state.clone();
            let sort_state = weak_state.clone();
            let mut delegate = configure_delegate(CrudTableDelegate::new(Vec::new()));
            delegate = delegate.on_visible_rows_changed(move |range, _, cx| {
                _ = visible_state.update(cx, |state, state_cx| {
                    state.load_visible_range(range.start, range.end, state_cx);
                });
            });
            delegate = delegate.on_sort_change(move |sort, _, cx| {
                _ = sort_state.update(cx, |state, state_cx| {
                    let sort = sort.or_else(|| state.default_sort.clone());
                    if state.set_sort(sort, state_cx).is_ok() {
                        state.load_current(state_cx);
                    }
                });
            });
            if selectable {
                let row_state = weak_state.clone();
                let loaded_state = weak_state;
                delegate = delegate.selection(CrudTableSelection::new(
                    Vec::new(),
                    move |event, _, cx| {
                        _ = row_state.update(cx, |state, cx| {
                            state.set_selected(event.row_id, event.selected, cx);
                        });
                    },
                    move |event, _, cx| {
                        _ = loaded_state.update(cx, |state, cx| {
                            state.set_selected_page(event.row_ids, event.selected, cx);
                        });
                    },
                ));
            }
            let table_state = state_cx.new(|table_cx| {
                TableState::new(delegate, window, table_cx)
                    .sortable(true)
                    .col_movable(true)
                    .col_resizable(true)
                    .col_selectable(false)
                    .row_selectable(false)
            });
            Self {
                query,
                default_sort,
                cache_identity,
                pages: BTreeMap::new(),
                total: 0,
                quick_filter_counts: BTreeMap::new(),
                selected_ids: HashSet::new(),
                requested_page: None,
                loading_pages: BTreeSet::new(),
                failures: BTreeMap::new(),
                visible_failure_page: None,
                revision: 0,
                loaded_once: false,
                loader,
                tasks: BTreeMap::new(),
                table_state,
            }
        }))
    }

    /// 返回当前已应用的强类型查询。
    pub fn query(&self) -> &Q {
        &self.query
    }

    /// 返回官方 DataTable 使用的状态 Entity。
    pub fn table_state(&self) -> &Entity<TableState<CrudTableDelegate<R>>> {
        &self.table_state
    }

    /// 返回服务端确认的总记录数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 返回当前稳定页码；跳页请求失败时该值保持原页。
    pub fn current_page(&self) -> u32 {
        self.query.pagination().page.max(1)
    }

    /// 返回当前界面正在展示或请求的页码。
    pub fn visible_page(&self) -> u32 {
        self.requested_page.unwrap_or_else(|| self.current_page())
    }

    /// 返回当前服务端确认或查询声明的页大小。
    pub fn page_size(&self) -> u32 {
        self.query.pagination().page_size.max(1)
    }

    /// 返回根据总数和页大小计算出的总页数，最少为一页。
    pub fn total_pages(&self) -> u32 {
        let page_size = self.page_size() as usize;
        self.total.div_ceil(page_size).max(1) as u32
    }

    /// 返回当前页是否已有缓存。
    pub fn has_current_page(&self) -> bool {
        self.pages.contains_key(&self.current_page())
    }

    /// 返回当前页缓存的行。
    pub fn current_rows(&self) -> &[R] {
        self.pages
            .get(&self.current_page())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// 返回当前查询是否至少成功加载过一次。
    pub const fn loaded_once(&self) -> bool {
        self.loaded_once
    }

    /// 返回当前页或正在跳转页是否处于加载状态。
    pub fn is_loading(&self) -> bool {
        let page = self.requested_page.unwrap_or_else(|| self.current_page());
        self.loading_pages.contains(&page)
    }

    /// 返回是否正在保留旧数据刷新当前页。
    pub fn is_refreshing(&self) -> bool {
        self.has_current_page() && self.loading_pages.contains(&self.current_page())
    }

    /// 返回当前页面应展示的加载错误。
    pub fn visible_error(&self) -> Option<&CrudLoadError> {
        let page = self.visible_failure_page?;
        self.failures.get(&page).map(|failure| &failure.error)
    }

    /// 返回快速筛选计数快照。
    pub fn quick_filter_counts(&self) -> &BTreeMap<String, u64> {
        &self.quick_filter_counts
    }

    /// 返回当前查询身份下跨页保留的已选业务 ID。
    pub fn selected_ids(&self) -> &HashSet<R::Id> {
        &self.selected_ids
    }

    /// 发起当前页加载；已有缓存时等价于保留旧数据刷新。
    pub fn load_current(&mut self, cx: &mut Context<Self>) {
        let page = self.current_page();
        self.request_page(page, false, cx);
    }

    /// 显式刷新当前页并使旧响应失效。
    pub fn refresh_current(&mut self, cx: &mut Context<Self>) {
        self.revision = self.revision.wrapping_add(1);
        self.tasks.clear();
        self.loading_pages.clear();
        let page = self.current_page();
        self.request_page(page, false, cx);
    }

    /// 导航到指定页；命中缓存时同步切换，未命中时先显示骨架并异步请求。
    pub fn go_to_page(&mut self, page: u32, cx: &mut Context<Self>) {
        let page = page.clamp(1, self.total_pages());
        if self.pages.contains_key(&page) {
            self.query.pagination_mut().page = page;
            self.requested_page = None;
            self.visible_failure_page = None;
            self.sync_table(cx);
            cx.notify();
            return;
        }
        self.requested_page = Some(page);
        self.request_page(page, true, cx);
    }

    /// 预加载包含指定逻辑行范围的页，供滚动可见范围变化时调用。
    pub fn load_visible_range(&mut self, start: usize, end: usize, cx: &mut Context<Self>) {
        if end <= start {
            return;
        }
        let page_size = self.page_size() as usize;
        let first = (start / page_size + 1) as u32;
        let last = ((end.saturating_sub(1)) / page_size + 1) as u32;
        for page in first..=last.min(self.total_pages()) {
            if !self.pages.contains_key(&page) {
                self.request_page(page, false, cx);
            }
        }
    }

    /// 替换完整查询；筛选、排序或页大小身份变化时清缓存、选择、错误和在途任务。
    ///
    /// # Errors
    ///
    /// 新查询无法生成稳定缓存身份时返回错误，当前状态保持不变。
    pub fn set_query(
        &mut self,
        mut query: Q,
        cx: &mut Context<Self>,
    ) -> Result<(), CrudListStateError> {
        query.normalize();
        let identity = query
            .cache_identity()
            .map_err(CrudListStateError::InvalidCacheIdentity)?;
        let identity_changed = identity != self.cache_identity;
        self.query = query;
        if identity_changed {
            self.cache_identity = identity;
            self.invalidate_query(cx);
        } else {
            self.sync_table(cx);
            cx.notify();
        }
        Ok(())
    }

    /// 更新一个派生宏声明的筛选值，回到第一页并清空旧查询缓存。
    ///
    /// # Errors
    ///
    /// 字段不存在、类型不匹配或新查询无法序列化时返回安全错误。
    pub fn set_filter_value(
        &mut self,
        name: &str,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut query = self.query.clone();
        query.set_filter_value(name, value)?;
        query.pagination_mut().page = 1;
        self.set_query(query, cx).map_err(|error| error.to_string())
    }

    /// 切换页大小，回到第一页并清空缓存与选择。
    ///
    /// # Errors
    ///
    /// 新查询无法序列化时返回错误。
    pub fn set_page_size(
        &mut self,
        page_size: u32,
        cx: &mut Context<Self>,
    ) -> Result<(), CrudListStateError> {
        let mut query = self.query.clone();
        query.pagination_mut().page = 1;
        query.pagination_mut().page_size = page_size;
        query.normalize();
        self.set_query(query, cx)
    }

    /// 切换服务端排序，回到第一页并清空缓存与选择。
    ///
    /// # Errors
    ///
    /// 新查询无法序列化时返回错误。
    pub fn set_sort(
        &mut self,
        sort: Option<Q::Sort>,
        cx: &mut Context<Self>,
    ) -> Result<(), CrudListStateError> {
        let mut query = self.query.clone();
        query.set_sort(sort);
        query.pagination_mut().page = 1;
        self.set_query(query, cx)
    }

    /// 重试当前可见失败请求，使用失败发生时的完整强类型 Query 快照。
    pub fn retry_visible(&mut self, cx: &mut Context<Self>) {
        let page = self.requested_page.unwrap_or_else(|| self.current_page());
        let Some(failure) = self.failures.remove(&page) else {
            return;
        };
        self.request_query(failure.query, page, self.requested_page == Some(page), cx);
    }

    /// 从选择集合中移除业务已经确认删除的 ID。
    pub fn remove_selected(&mut self, id: &R::Id, cx: &mut Context<Self>) {
        if self.selected_ids.remove(id) {
            self.sync_selection(cx);
            cx.notify();
        }
    }

    fn set_selected(&mut self, id: R::Id, selected: bool, cx: &mut Context<Self>) {
        if selected {
            self.selected_ids.insert(id);
        } else {
            self.selected_ids.remove(&id);
        }
        self.sync_selection(cx);
        cx.notify();
    }

    fn set_selected_page(&mut self, ids: Vec<R::Id>, selected: bool, cx: &mut Context<Self>) {
        if selected {
            self.selected_ids.extend(ids);
        } else {
            for id in ids {
                self.selected_ids.remove(&id);
            }
        }
        self.sync_selection(cx);
        cx.notify();
    }

    fn invalidate_query(&mut self, cx: &mut Context<Self>) {
        self.revision = self.revision.wrapping_add(1);
        self.pages.clear();
        self.total = 0;
        self.quick_filter_counts.clear();
        self.selected_ids.clear();
        self.requested_page = None;
        self.loading_pages.clear();
        self.failures.clear();
        self.visible_failure_page = None;
        self.tasks.clear();
        self.loaded_once = false;
        self.sync_table(cx);
        cx.notify();
    }

    fn request_page(&mut self, page: u32, navigation: bool, cx: &mut Context<Self>) {
        let mut query = self.query.clone();
        query.pagination_mut().page = page;
        self.request_query(query, page, navigation, cx);
    }

    fn request_query(&mut self, query: Q, page: u32, navigation: bool, cx: &mut Context<Self>) {
        if self.loading_pages.contains(&page) {
            return;
        }
        if navigation {
            self.requested_page = Some(page);
        }
        self.failures.remove(&page);
        self.visible_failure_page = None;
        self.loading_pages.insert(page);
        self.sync_table(cx);
        cx.notify();

        let loader = self.loader.clone();
        let revision = self.revision;
        let identity = self.cache_identity.clone();
        let request_query = query.clone();
        let task = cx.spawn(async move |state, cx| {
            let result = loader(request_query.clone()).await;
            let _ = state.update(cx, |state, cx| {
                state.complete_load(revision, identity, page, request_query, result, cx);
            });
        });
        self.tasks.insert(page, task);
    }

    fn complete_load(
        &mut self,
        revision: u64,
        identity: String,
        requested_page: u32,
        request_query: Q,
        result: Result<CrudPage<R>, CrudLoadError>,
        cx: &mut Context<Self>,
    ) {
        if revision != self.revision || identity != self.cache_identity {
            return;
        }
        self.loading_pages.remove(&requested_page);
        self.tasks.remove(&requested_page);

        match result {
            Err(error) => {
                self.failures.insert(
                    requested_page,
                    FailedPage {
                        query: request_query,
                        error,
                    },
                );
                self.visible_failure_page = Some(requested_page);
                if self.requested_page == Some(requested_page) {
                    self.requested_page = None;
                }
            }
            Ok(page) => self.apply_page(requested_page, page, cx),
        }
        self.sync_table(cx);
        cx.notify();
    }

    fn apply_page(&mut self, requested_page: u32, page: CrudPage<R>, cx: &mut Context<Self>) {
        let page_number = page.page.max(1);

        if page.page_size != self.page_size() {
            self.pages.clear();
            self.selected_ids.clear();
            self.loading_pages.clear();
            self.tasks.clear();
            self.query.pagination_mut().page_size = page.page_size;
            self.cache_identity = self
                .query
                .cache_identity()
                .expect("已成功请求的 CRUD Query 必须保持可序列化");
        }
        self.pages.insert(page_number, page.items);
        self.total = page.total;
        self.quick_filter_counts = page.quick_filter_counts;
        self.loaded_once = true;
        self.failures.remove(&requested_page);
        self.visible_failure_page = None;

        if self.requested_page == Some(requested_page)
            || !self.pages.contains_key(&self.current_page())
            || requested_page == self.current_page()
        {
            self.query.pagination_mut().page = page_number;
        }
        if self.requested_page == Some(requested_page) {
            self.requested_page = None;
        }

        let total_pages = self.total_pages();
        self.pages.retain(|number, _| *number <= total_pages);
        if self.current_page() > total_pages {
            self.query.pagination_mut().page = total_pages;
        }

        self.sync_selection(cx);
    }

    fn sync_selection(&self, cx: &mut Context<Self>) {
        let selected_ids = self.selected_ids.iter().cloned().collect::<Vec<_>>();
        self.table_state.update(cx, |table, table_cx| {
            if table.delegate().selection_enabled() {
                table.delegate_mut().set_selected_ids(selected_ids);
                table_cx.notify();
            }
        });
    }

    fn sync_table(&self, cx: &mut Context<Self>) {
        let visible_page = self.requested_page.unwrap_or_else(|| self.current_page());
        let current_rows = self.pages.get(&visible_page).cloned().unwrap_or_default();
        let loading = self.loading_pages.contains(&visible_page) && current_rows.is_empty();
        let page_size = self.page_size() as usize;
        let loaded_rows = self.pages.iter().flat_map(|(page, rows)| {
            let offset = page.saturating_sub(1) as usize * page_size;
            rows.iter()
                .cloned()
                .enumerate()
                .map(move |(index, row)| (offset + index, row))
        });
        let selected_ids = self.selected_ids.iter().cloned().collect::<Vec<_>>();
        self.table_state.update(cx, |table, table_cx| {
            let delegate = table.delegate_mut();
            delegate.replace_sparse_rows(self.total, current_rows, loaded_rows);
            delegate.set_loading(loading);
            delegate.set_loading_more(false);
            if delegate.selection_enabled() {
                delegate.set_selected_ids(selected_ids);
            }
            table_cx.notify();
        });
    }
}
