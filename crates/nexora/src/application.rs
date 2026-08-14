//! Nexora 桌面应用启动契约与通用 Feature Shell。
//!
//! 应用实现方只负责提供启动选项和初始化自己的全局状态；注册表发现、首路由校验、
//! 主窗口创建以及 Feature Entity 的生命周期由框架统一管理。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
    sync::mpsc::{self, Receiver, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

pub use ::desktop::ApplicationAssets;
use ::desktop::{
    Application as DesktopApplication, ApplicationOptions as DesktopApplicationOptions,
};
#[cfg(feature = "desktop")]
use actions::account::{self as account_actions, AccountActionKind, SignInAccount, SignOutAccount};
use actions::{search::OpenGlobalSearch, settings::OpenSettings, window as window_actions};
use configuration::{ConfigurationError, UserConfigStore, VersionedConfiguration};
#[cfg(feature = "desktop")]
use gpui::{Anchor, WindowHandle};
use gpui::{
    AnyElement, AnyView, App, AssetSource, Bounds, ClickEvent, Context, ElementId, Entity,
    Focusable as _, Global, Image, ImageFormat, IntoElement as _, MouseButton, Pixels, Render,
    ScrollHandle, Size, Subscription, Task, WeakEntity, Window, WindowBounds, WindowOptions, div,
    img, point, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme as _, Collapsible, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    Size as ComponentSize, StyledExt as _, TitleBar, WindowExt as _,
    alert::Alert,
    badge::Badge,
    button::{Button, ButtonVariants as _, Toggle},
    dialog::{DialogClose, DialogFooter},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarGroup, SidebarItem, SidebarMenu, SidebarMenuItem,
    },
    tab::{Tab, TabBar},
    table::{Column, TableDelegate, TableEvent, TableState},
    v_flex,
};
#[cfg(feature = "desktop")]
use gpui_component::{avatar::Avatar, menu::DropdownMenu as _};
use percent_encoding::percent_decode_str;
use pinyin::ToPinyin as _;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ui::{
    CrudTableDelegate, CrudTableRow, DataTableLayoutError, DataTableLayoutKey, ShortcutHint,
    SidebarRegion, apply_data_table_layout, data_table_layout_from_event,
    layout::{
        WORKSPACE_SHELL_ICON_SIZE, WORKSPACE_SIDEBAR_COLLAPSED_WIDTH,
        WORKSPACE_SIDEBAR_EXPANDED_WIDTH, WorkspaceLayout,
    },
};

use crate::global_search::{
    SearchAction, SearchDialog, SearchHistoryEntry, SearchItem, SearchMode, SearchProvider,
    SearchSection, installed_search_providers,
};

/// 让手写 `TableDelegate` 暴露其稳定列集合，以接入 Nexora 列布局持久化。
///
/// 实现方必须返回与 `TableDelegate::column` 同一列集合，每列使用非本地化且唯一的
/// `Column::key`。修改可变切片只能调整顺序和用户允许的宽度。
pub trait PersistentDataTableDelegate: TableDelegate {
    /// 返回当前列定义。
    fn persistent_columns(&self) -> &[Column];

    /// 返回当前列定义的可变切片。
    fn persistent_columns_mut(&mut self) -> &mut [Column];
}

impl<R> PersistentDataTableDelegate for CrudTableDelegate<R>
where
    R: CrudTableRow,
{
    fn persistent_columns(&self) -> &[Column] {
        self.columns()
    }

    fn persistent_columns_mut(&mut self) -> &mut [Column] {
        self.columns_mut()
    }
}

/// 创建自动恢复并持久化列顺序与宽度的 `TableState`。
///
/// `owner_id` 必须是 Feature 或 Window 的稳定 ID，`table_id` 必须是页面内稳定且
/// 唯一的表格 ID；两者不得使用路径、显示名称或本地化文本。函数会在创建
/// `TableState` 前合并已保存布局，并监听原生 `MoveColumn` 与
/// `ColumnWidthsChanged` 事件写入 `workspace.toml`。订阅生命周期与调用方 Entity
/// 一致，不需要业务类型额外保存 `Subscription`。
///
/// # Errors
///
/// 复合身份非法，或当前/持久布局中存在重复列 key 时返回
/// [`DataTableLayoutError`]；不会创建部分初始化的 Entity。
pub fn persistent_data_table_state<O, D>(
    owner_id: impl Into<String>,
    table_id: impl Into<String>,
    mut delegate: D,
    configure: impl FnOnce(TableState<D>) -> TableState<D>,
    window: &mut Window,
    cx: &mut Context<O>,
) -> Result<Entity<TableState<D>>, DataTableLayoutError>
where
    O: 'static,
    D: PersistentDataTableDelegate,
{
    let key = DataTableLayoutKey::new(owner_id, table_id)?;
    let storage_key = key.storage_key();
    if let Some(layout) = shell_preferences_snapshot(cx)
        .table_layouts
        .get(&storage_key)
    {
        let mut columns = delegate.persistent_columns().to_vec();
        apply_data_table_layout(&mut columns, layout)?;
        delegate
            .persistent_columns_mut()
            .clone_from_slice(columns.as_slice());
    }

    let state = cx.new(|cx| configure(TableState::new(delegate, window, cx)));
    cx.subscribe(&state, move |_, state, event: &TableEvent, cx| {
        let layout = state.update(cx, |state, _cx| {
            data_table_layout_from_event(state.delegate_mut().persistent_columns_mut(), event)
        });
        match layout {
            Ok(Some(layout)) => {
                let storage_key = storage_key.clone();
                update_shell_preferences(cx, |preferences| {
                    preferences.table_layouts.insert(storage_key, layout);
                });
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(error = %error, "无法捕获 DataTable 列布局");
            }
        }
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="订阅以被观察的 TableState 和调用方 Entity 弱引用为边界，任一释放后回调自动失效"
    .detach();
    Ok(state)
}

/// 创建自动持久列布局的标准 `CrudTableDelegate` 表格状态。
///
/// 这是 [`persistent_data_table_state`] 面向 Nexora CRUD 表格的一等便捷入口。
/// `owner_id` 使用 Feature/Window 稳定 ID，`table_id` 使用页面内非本地化稳定 ID；
/// 列身份由 `CrudTableRow::columns` 中的 `Column::key` 提供。
///
/// # Errors
///
/// 复合表格身份非法、当前列 key 重复或已保存布局包含重复 key 时返回
/// [`DataTableLayoutError`]。
pub fn persistent_crud_table_state<O, R>(
    owner_id: impl Into<String>,
    table_id: impl Into<String>,
    delegate: CrudTableDelegate<R>,
    configure: impl FnOnce(TableState<CrudTableDelegate<R>>) -> TableState<CrudTableDelegate<R>>,
    window: &mut Window,
    cx: &mut Context<O>,
) -> Result<Entity<TableState<CrudTableDelegate<R>>>, DataTableLayoutError>
where
    O: 'static,
    R: CrudTableRow,
{
    persistent_data_table_state(owner_id, table_id, delegate, configure, window, cx)
}

/// 删除指定稳定表格的持久列布局。
///
/// 该操作只影响下次恢复；调用方如需立即恢复当前 Entity，应使用代码默认列
/// 重建它。返回 `true` 表示快照中原本存在该布局。
pub fn reset_data_table_layout(key: &DataTableLayoutKey, cx: &mut App) -> bool {
    let storage_key = key.storage_key();
    let existed = shell_preferences_snapshot(cx)
        .table_layouts
        .contains_key(&storage_key);
    if existed {
        update_shell_preferences(cx, |preferences| {
            preferences.table_layouts.remove(&storage_key);
        });
    }
    existed
}

/// 应用默认品牌区域使用的 PNG Logo。
///
/// Logo 字节应通过 `include_bytes!` 编译进最终桌面程序，避免运行时依赖当前工作目录。
/// 完全自定义登录页时仍可使用 `LoginFeature` 覆盖默认实现。
#[derive(Clone, Copy, Debug)]
pub struct ApplicationLogo {
    bytes: &'static [u8],
}

impl ApplicationLogo {
    /// 从编译期 PNG 字节创建应用 Logo。
    pub const fn png(bytes: &'static [u8]) -> Self {
        Self { bytes }
    }

    /// 返回用于原生窗口与系统托盘的编译期 PNG 字节。
    pub const fn bytes(self) -> &'static [u8] {
        self.bytes
    }

    pub(crate) fn image(self) -> std::sync::Arc<Image> {
        std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, self.bytes.to_vec()))
    }
}

type ShellToolbarClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// 主窗口全局工具栏中的稳定应用动作。
///
/// 标准构造器使用 gpui-component 官方 `Button`、`Icon`、`Tooltip` 与 `Badge`。需要
/// `Popover`、`Menu` 或受控复杂内容时可使用 [`Self::custom`] 返回官方组件组合。
#[derive(Clone)]
pub struct ShellToolbarAction {
    id: String,
    order: i32,
    render: ShellToolbarRenderer,
}

#[derive(Clone)]
enum ShellToolbarRenderer {
    Standard {
        icon: Box<Icon>,
        tooltip: gpui::SharedString,
        badge: usize,
        disabled: bool,
        loading: bool,
        on_click: ShellToolbarClickHandler,
    },
    Custom(Rc<dyn Fn(&mut App) -> AnyElement>),
}

impl ShellToolbarAction {
    /// 创建与 Shell 搜索框同一视觉尺度的官方图标按钮。
    ///
    /// 自定义工具动作应使用该工厂作为交互根元素，再组合 `Badge`、`Popover` 等官方
    /// 组件，避免每个下游应用分别决定按钮与图标尺寸。
    pub fn icon_button(
        id: impl Into<ElementId>,
        icon: impl Into<Icon>,
        tooltip: impl Into<gpui::SharedString>,
    ) -> Button {
        workspace_icon_button(id, icon, tooltip)
    }

    /// 创建一个使用官方图标按钮的全局工具动作。
    ///
    /// `id` 必须在当前应用安装的动作中唯一，`order` 越小越靠左。回调只应执行窗口级或
    /// 跨 Feature 能力；创建、导出和筛选等页面级操作应留在 Feature 内部。
    pub fn new(
        id: impl Into<String>,
        order: i32,
        icon: impl Into<Icon>,
        tooltip: impl Into<gpui::SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            order,
            render: ShellToolbarRenderer::Standard {
                icon: Box::new(icon.into()),
                tooltip: tooltip.into(),
                badge: 0,
                disabled: false,
                loading: false,
                on_click: Rc::new(on_click),
            },
        }
    }

    /// 创建一个受控自定义工具动作。
    ///
    /// 该入口用于官方 `Popover`、`Menu` 或需要读取动态应用状态的组合。自定义渲染仍必须
    /// 使用 gpui-component 的语义组件，不应自行模拟按钮、焦点或键盘行为。
    pub fn custom<E>(
        id: impl Into<String>,
        order: i32,
        render: impl Fn(&mut App) -> E + 'static,
    ) -> Self
    where
        E: gpui::IntoElement,
    {
        Self {
            id: id.into(),
            order,
            render: ShellToolbarRenderer::Custom(Rc::new(move |cx| render(cx).into_any_element())),
        }
    }

    /// 设置标准图标动作的数量徽章；零值不展示。
    #[must_use]
    pub fn badge(mut self, badge: usize) -> Self {
        if let ShellToolbarRenderer::Standard { badge: current, .. } = &mut self.render {
            *current = badge;
        }
        self
    }

    /// 设置标准图标动作的禁用状态。
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        if let ShellToolbarRenderer::Standard {
            disabled: current, ..
        } = &mut self.render
        {
            *current = disabled;
        }
        self
    }

    /// 设置标准图标动作的加载状态；加载期间自动禁止重复触发。
    #[must_use]
    pub fn loading(mut self, loading: bool) -> Self {
        if let ShellToolbarRenderer::Standard {
            loading: current, ..
        } = &mut self.render
        {
            *current = loading;
        }
        self
    }

    /// 返回动作的稳定 ID。
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    /// 返回动作排序值。
    pub const fn order(&self) -> i32 {
        self.order
    }

    fn render(&self, cx: &mut App) -> AnyElement {
        match &self.render {
            ShellToolbarRenderer::Standard {
                icon,
                tooltip,
                badge,
                disabled,
                loading,
                on_click,
            } => {
                let on_click = on_click.clone();
                let button =
                    Self::icon_button(self.id.clone(), icon.as_ref().clone(), tooltip.clone())
                        .loading(*loading)
                        .disabled(*disabled || *loading)
                        .on_click(move |event, window, cx| on_click(event, window, cx));
                if *badge == 0 {
                    button.into_any_element()
                } else {
                    Badge::new()
                        .count(*badge)
                        .small()
                        .child(button)
                        .into_any_element()
                }
            }
            ShellToolbarRenderer::Custom(render) => render(cx),
        }
    }
}

fn workspace_icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    tooltip: impl Into<gpui::SharedString>,
) -> Button {
    // gpui-component 的自定义 Button 尺寸会把图标缩放到 75%；该基准值使最终图标为 20px，
    // 再用实例样式把可点击区域固定为 32px，继续保留官方 loading/disabled 图标语义。
    const COMPONENT_SIZE_FOR_TWENTY_PIXEL_ICON: Pixels = px(20.0 / 0.75);
    Button::new(id)
        .ghost()
        .with_size(COMPONENT_SIZE_FOR_TWENTY_PIXEL_ICON)
        .size_8()
        .icon(icon)
        .tooltip(tooltip)
}

fn workspace_sidebar_footer_host(
    content: AnyElement,
    collapsed: bool,
    border_color: gpui::Hsla,
) -> AnyElement {
    let host = h_flex()
        .w_full()
        .pt_3()
        .border_t_1()
        .border_color(border_color);
    if collapsed {
        host.justify_center().child(content).into_any_element()
    } else {
        host.child(div().flex_1().min_w_0().child(content))
            .into_any_element()
    }
}

/// Shell 顶部工具区的容器级布局选项。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellToolbarOptions {
    right_padding: Pixels,
}

impl ShellToolbarOptions {
    /// 创建默认工具区布局。
    ///
    /// 默认右边距为 12px，使工具动作不会贴住窗口边框。
    pub const fn new() -> Self {
        Self {
            right_padding: px(12.0),
        }
    }

    /// 设置工具动作容器与窗口右边框之间的距离。
    #[must_use]
    pub fn right_padding(mut self, padding: Pixels) -> Self {
        self.right_padding = padding;
        self
    }

    /// 返回当前配置的工具区右边距。
    pub const fn right_padding_value(&self) -> Pixels {
        self.right_padding
    }
}

impl Default for ShellToolbarOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Default)]
struct ShellToolbarActionRegistry {
    actions: Vec<ShellToolbarAction>,
    options: ShellToolbarOptions,
}

impl Global for ShellToolbarActionRegistry {}

/// 安装应用级主窗口全局工具动作。
///
/// 后一次安装会完整替换前一次安装的列表。传入空列表会清空应用动作。
///
/// # Panics
///
/// 动作列表包含重复稳定 ID 时 panic。
pub fn install_shell_toolbar_actions(actions: Vec<ShellToolbarAction>, cx: &mut App) {
    install_shell_toolbar_actions_with_options(actions, ShellToolbarOptions::default(), cx);
}

/// 使用显式容器布局安装应用级主窗口全局工具动作。
///
/// `options` 只控制 Shell 拥有的工具区容器；每个自定义动作仍负责自身的官方组件组合。
/// 后一次安装会完整替换动作与布局配置。
///
/// # Panics
///
/// 动作列表包含重复稳定 ID 时 panic。
pub fn install_shell_toolbar_actions_with_options(
    mut actions: Vec<ShellToolbarAction>,
    options: ShellToolbarOptions,
    cx: &mut App,
) {
    actions.sort_by(|left, right| {
        left.order()
            .cmp(&right.order())
            .then_with(|| left.id().cmp(right.id()))
    });
    let mut ids = HashSet::new();
    for action in &actions {
        assert!(ids.insert(action.id()), "ShellToolbarAction ID 不能重复");
    }
    let registry = ShellToolbarActionRegistry { actions, options };
    if cx.has_global::<ShellToolbarActionRegistry>() {
        *cx.global_mut::<ShellToolbarActionRegistry>() = registry;
    } else {
        cx.set_global(registry);
    }
}

struct RenderedShellToolbar {
    actions: Vec<AnyElement>,
    right_padding: Pixels,
}

fn shell_toolbar_actions(cx: &mut App) -> RenderedShellToolbar {
    let registry = cx
        .try_global::<ShellToolbarActionRegistry>()
        .cloned()
        .unwrap_or_default();
    let actions = registry
        .actions
        .into_iter()
        .map(|action| action.render(cx))
        .collect();
    RenderedShellToolbar {
        actions,
        right_padding: registry.options.right_padding,
    }
}

#[derive(Clone)]
pub(crate) struct ApplicationBranding {
    pub(crate) application_name: String,
    pub(crate) logo: Option<ApplicationLogo>,
}

impl Global for ApplicationBranding {}

pub(crate) fn application_branding(cx: &App) -> ApplicationBranding {
    cx.try_global::<ApplicationBranding>()
        .cloned()
        .unwrap_or_else(|| ApplicationBranding {
            application_name: "Nexora".to_owned(),
            logo: None,
        })
}

/// Nexora 主窗口顶部标签栏的视觉样式。
///
/// 该枚举只选择 `gpui-component` 官方 `TabBar` 的变体，不改变标签切换、滚动、右键菜单和
/// 置顶等行为。应用可以通过 [`ApplicationOptions::tab_style`] 覆盖默认样式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ApplicationTabStyle {
    /// 使用官方默认 `Tabs` 样式。
    #[default]
    Tab,
    /// 使用官方 underline 样式，适合更轻量的内容页切换。
    Underline,
    /// 使用官方 pill 样式，适合强调独立可点击标签块的界面。
    Pill,
    /// 使用官方 outline 样式，适合边界感更强的标签栏。
    Outline,
    /// 使用官方 segmented 样式，适合需要背景容器感的标签组。
    Segmented,
}

/// 下游应用随二进制注册的一组浅色与深色主题预设。
///
/// 预设由稳定 ID、设置页显示名称和内嵌 `gpui-component` `ThemeSet` JSON 构成。JSON
/// 会在 [`Application::validate`] 与 [`Application::run`] 进入原生事件循环前严格校验。
pub type ApplicationThemePreset = theme::ThemePresetSource;

/// 当前 Linux 桌面或平台无法提供可用托盘宿主时的安全降级策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrayUnavailablePolicy {
    /// 显示明确提示并只执行普通最小化，确保用户始终能恢复窗口。
    #[default]
    NotifyAndMinimize,
    /// 托盘不可用时保持窗口可见，不执行隐藏。
    KeepVisible,
}

impl ApplicationTabStyle {
    fn apply(self, tab_bar: TabBar) -> TabBar {
        match self {
            Self::Tab => tab_bar,
            Self::Underline => tab_bar.underline(),
            Self::Pill => tab_bar.pill(),
            Self::Outline => tab_bar.outline(),
            Self::Segmented => tab_bar.segmented(),
        }
    }
}

use crate::{
    AppRegistry, FeatureInstance, FeatureMetadata, FeatureRuntimeError, NavigationContextExt as _,
    NavigationGroupMetadata, RegistryError, ResolveError, RouteMatch, RouteTargetKind,
    WindowRuntimeError,
    runtime::{install_navigation_handler, remove_navigation_handler},
};

/// Nexora 桌面应用的启动选项。
///
/// 默认值会创建一个 `900 × 640` 的主窗口、限制最小尺寸为 `640 × 480`、主动激活应用，
/// 并以中文和根路径 `/` 启动。应用只需要覆盖与自身产品有关的字段。
#[derive(Debug)]
pub struct ApplicationOptions {
    /// 应用在系统菜单中展示的名称；未注册自定义 `SidebarHeader` 时也用于默认 Header。
    pub application_name: String,
    /// 默认登录页左下角展示的应用版本号。
    pub application_version: Option<String>,
    /// 默认登录页和未被自定义 `SidebarHeader` 替换的 Sidebar Header 共享的可选 PNG Logo。
    pub application_logo: Option<ApplicationLogo>,
    /// 默认 Sidebar Header 中位于应用名称下方的说明文字。
    ///
    /// 应用注册自定义 `SidebarHeader` 时不会显示该文字。
    pub sidebar_subtitle: Option<String>,
    /// 是否在最后一个窗口关闭后继续保持应用进程运行。
    pub daemon_mode: bool,
    /// 创建主窗口后是否主动激活应用。
    pub activate: bool,
    /// 需要直接传递给 GPUI 的原生窗口选项。
    ///
    /// 为 `None` 时从 GPUI 默认值构造；没有配置 titlebar 时，框架会补上与
    /// [`WorkspaceLayout`] 匹配的 `gpui-component` TitleBar 选项。
    pub window_options: Option<WindowOptions>,
    /// 主窗口的初始逻辑像素尺寸。
    ///
    /// 为 `None` 时由 GPUI 或调用方提供的 [`Self::window_options`] 决定。
    pub window_size: Option<Size<Pixels>>,
    /// 主窗口允许缩放到的最小逻辑像素尺寸。
    pub window_min_size: Option<Size<Pixels>>,
    /// 启动时优先使用的显示器稳定 UUID。
    ///
    /// 对应显示器不存在时，底层桌面运行时会安全回退到系统主显示器。
    pub startup_display_uuid: Option<String>,
    /// 应用额外提供给 GPUI 的静态资源。
    ///
    /// 资源会在 `gpui-component` 初始化前注册，并优先于 Nexora 和组件库的默认资源查找。
    /// 应用可用它嵌入 `assets/icons/**/*.svg`，再通过 `#[nexora(icon = "...")]` 或
    /// `Icon::default().path(...)` 使用。
    pub application_assets: Option<ApplicationAssets>,
    /// `gpui-component` 使用的界面语言，例如 `zh-CN` 或 `en`。
    pub locale: String,
    /// 主窗口创建后首先打开的 Feature 路径或 deeplink。
    ///
    /// 该位置会在进入 GPUI 事件循环前完成注册表匹配，并且必须指向 Feature。
    pub initial_path: String,
    /// 主窗口顶部 Feature 标签栏使用的官方 `TabBar` 样式。
    ///
    /// 默认使用 [`ApplicationTabStyle::Tab`]，与 `gpui-component` 官方 `Tabs` story 保持同步；
    /// 应用可以切换到 `Underline`、`Pill`、`Outline` 或 `Segmented`，交互行为仍由同一个
    /// gpui-component `TabBar` 负责。
    pub tab_style: ApplicationTabStyle,
    /// 是否在主窗口 Sidebar 的 Header 与导航列表之间显示导航搜索输入框。
    ///
    /// 默认关闭。启用后 Shell 会创建一个 `gpui-component` 输入状态，并仅过滤当前用户
    /// 有权看到的 Section、NavigationGroup 与 Feature 标题；清空搜索词后恢复原导航树。
    pub sidebar_search: bool,
    /// 是否在关闭主窗口时提供最小化到托盘，默认开启。
    pub tray_enabled: bool,
    /// 显式覆盖生产 app ID 或开发可执行文件派生的应用身份。
    pub application_identity_override: Option<String>,
    /// 平台不支持托盘时的降级策略。
    pub tray_unavailable_policy: TrayUnavailablePolicy,
    /// 下游应用按注册顺序提供的主题预设。
    ///
    /// 每个预设必须拥有唯一稳定 ID，并恰好包含一个浅色与一个深色主题。内置 `nexora`
    /// 主题无需重复注册且始终作为最终兜底。
    pub theme_presets: Vec<ApplicationThemePreset>,
    /// 首次安装或用户历史主题失效时使用的应用默认主题 ID。
    ///
    /// 为 `None` 时使用内置 `nexora`；显式 ID 必须出现在 [`Self::theme_presets`] 中。
    pub default_theme_preset: Option<String>,
}

impl Default for ApplicationOptions {
    fn default() -> Self {
        Self {
            application_name: "Nexora".to_owned(),
            application_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            application_logo: None,
            sidebar_subtitle: Some("Desktop workspace".to_owned()),
            daemon_mode: false,
            activate: true,
            window_options: Some(WindowOptions {
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            }),
            window_size: Some(size(px(900.0), px(640.0))),
            window_min_size: Some(size(px(640.0), px(480.0))),
            startup_display_uuid: None,
            application_assets: None,
            locale: "zh-CN".to_owned(),
            initial_path: "/".to_owned(),
            tab_style: ApplicationTabStyle::Tab,
            sidebar_search: false,
            tray_enabled: true,
            application_identity_override: None,
            tray_unavailable_policy: TrayUnavailablePolicy::NotifyAndMinimize,
            theme_presets: Vec::new(),
            default_theme_preset: None,
        }
    }
}

impl ApplicationOptions {
    /// 创建一份可以直接启动标准 Nexora 桌面程序的默认选项。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置应用在系统菜单和默认 Sidebar Header 中展示的名称。
    pub fn application_name(mut self, application_name: impl Into<String>) -> Self {
        self.application_name = application_name.into();
        self
    }

    /// 设置默认登录页左下角展示的应用版本号。
    pub fn application_version(mut self, application_version: impl Into<String>) -> Self {
        self.application_version = Some(application_version.into());
        self
    }

    /// 设置默认登录页和 Sidebar Header 使用的 PNG Logo。
    pub const fn application_logo(mut self, application_logo: ApplicationLogo) -> Self {
        self.application_logo = Some(application_logo);
        self
    }

    /// 设置默认 Sidebar Header 中位于应用名称下方的说明文字。
    pub fn sidebar_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.sidebar_subtitle = Some(subtitle.into());
        self
    }

    /// 设置主窗口首先打开的 Feature 路径或 deeplink。
    pub fn initial_path(mut self, initial_path: impl Into<String>) -> Self {
        self.initial_path = initial_path.into();
        self
    }

    /// 设置主窗口顶部 Feature 标签栏的官方 `TabBar` 样式。
    pub const fn tab_style(mut self, tab_style: ApplicationTabStyle) -> Self {
        self.tab_style = tab_style;
        self
    }

    /// 设置是否在主窗口 Sidebar 中启用导航搜索。
    ///
    /// 该开关默认关闭；关闭时 Shell 不会创建搜索输入状态、订阅或搜索索引。启用后，
    /// 搜索框显示在默认或自定义 `SidebarHeader` 下方，并只影响导航树过滤，不改变路由、
    /// 标签页或用户手动展开状态。
    pub const fn sidebar_search(mut self, sidebar_search: bool) -> Self {
        self.sidebar_search = sidebar_search;
        self
    }

    /// 设置主窗口关闭确认中是否启用托盘行为。
    pub const fn tray_enabled(mut self, enabled: bool) -> Self {
        self.tray_enabled = enabled;
        self
    }

    /// 设置应用单例与 IPC 目录使用的显式稳定身份。
    pub fn application_identity(mut self, identity: impl Into<String>) -> Self {
        self.application_identity_override = Some(identity.into());
        self
    }

    /// 设置托盘宿主不可用时的安全降级策略。
    pub const fn tray_unavailable_policy(mut self, policy: TrayUnavailablePolicy) -> Self {
        self.tray_unavailable_policy = policy;
        self
    }

    /// 注册一组随应用发布的浅色与深色主题。
    ///
    /// 可以多次调用；默认设置窗口会把内置 Nexora 放在首项，并按调用顺序追加下游预设。
    /// 主题 ID、显示名称和 JSON 会在应用启动前统一校验。
    pub fn theme_preset(mut self, preset: ApplicationThemePreset) -> Self {
        self.theme_presets.push(preset);
        self
    }

    /// 设置首次安装及失效用户偏好的应用默认主题预设 ID。
    ///
    /// 已有且仍有效的用户选择继续优先。该 ID 未注册时 [`Application::validate`] 与
    /// [`Application::run`] 会返回 [`ApplicationError::InvalidThemeConfiguration`]。
    pub fn default_theme_preset(mut self, preset_id: impl Into<String>) -> Self {
        self.default_theme_preset = Some(preset_id.into());
        self
    }

    /// 设置 `gpui-component` 使用的界面语言。
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = locale.into();
        self
    }

    /// 设置主窗口的初始逻辑像素尺寸。
    pub fn window_size(mut self, width: f32, height: f32) -> Self {
        self.window_size = Some(size(px(width), px(height)));
        self
    }

    /// 设置主窗口允许缩放到的最小逻辑像素尺寸。
    pub fn window_min_size(mut self, width: f32, height: f32) -> Self {
        self.window_min_size = Some(size(px(width), px(height)));
        self
    }

    /// 设置主窗口创建后是否主动激活应用。
    pub const fn activate(mut self, activate: bool) -> Self {
        self.activate = activate;
        self
    }

    /// 设置应用在最后一个窗口关闭后是否继续常驻。
    pub const fn daemon_mode(mut self, daemon_mode: bool) -> Self {
        self.daemon_mode = daemon_mode;
        self
    }

    /// 设置需要直接传递给 GPUI 的原生窗口选项。
    pub fn window_options(mut self, window_options: WindowOptions) -> Self {
        self.window_options = Some(window_options);
        self
    }

    /// 仅在应用没有显式设置原生 title 时补充主窗口默认 title。
    ///
    /// Nexora 启动器在 `open_window` 前用安装元数据中的最终 `display_name` 调用该方法；
    /// 开发运行没有安装元数据时使用 [`Self::application_name`]。应用在
    /// `WindowOptions::titlebar.title` 中提供的值始终优先，不会被覆盖。
    pub fn default_native_window_title(
        mut self,
        default_title: impl Into<gpui::SharedString>,
    ) -> Self {
        let window_options = self
            .window_options
            .get_or_insert_with(WindowOptions::default);
        let titlebar = window_options
            .titlebar
            .get_or_insert_with(TitleBar::title_bar_options);
        if titlebar.title.is_none() {
            titlebar.title = Some(default_title.into());
        }
        self
    }

    /// 设置启动时优先使用的显示器稳定 UUID。
    pub fn startup_display_uuid(mut self, display_uuid: impl Into<String>) -> Self {
        self.startup_display_uuid = Some(display_uuid.into());
        self
    }

    /// 设置应用提供给 GPUI 的额外静态资源源。
    ///
    /// 该资源源会在进入 GPUI 事件循环前与 Nexora 内置资源、`gpui-component-assets`
    /// 默认图标合并。应用资源优先级最高，因此可以覆盖同名默认资源。该方法通常配合
    /// `rust_embed` 使用，把桌面 package 的 `assets/icons/**/*.svg` 编译进最终程序。
    pub fn application_assets(mut self, assets: impl AssetSource) -> Self {
        self.application_assets = Some(ApplicationAssets::new(assets));
        self
    }

    fn into_desktop_options(
        self,
        default_window_title: &str,
        theme_catalog: theme::ThemeCatalog,
    ) -> DesktopApplicationOptions {
        let mut this = self.default_native_window_title(default_window_title);
        let window_options = this.window_options.take();
        DesktopApplicationOptions {
            open_startup_window: true,
            daemon_mode: this.daemon_mode,
            activate: this.activate,
            window_options,
            window_size: this.window_size,
            window_min_size: this.window_min_size,
            startup_display_uuid: this.startup_display_uuid,
            application_assets: this.application_assets,
            theme_catalog,
        }
    }
}

/// 启动 Nexora 桌面应用时可能发生的结构化错误。
///
/// 注册表和首路由错误会在进入原生事件循环前返回，因此 CLI 生成的程序可以直接使用
/// `?` 把错误报告给调用环境，而不会先创建一个不完整的窗口。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApplicationError {
    /// 应用单例门禁或重复启动激活 IPC 失败。
    #[error("桌面进程协调失败：{message}")]
    Process {
        /// 不包含本地 IPC 载荷或账号秘密的错误摘要。
        message: String,
    },

    /// 自动发现的 Feature 或 Window 元数据无效或互相冲突。
    #[error(transparent)]
    Registry(
        /// 注册表在标识、路径或父子导航校验阶段返回的具体错误。
        #[from]
        RegistryError,
    ),

    /// 配置的首路由无法由当前应用注册表解析。
    #[error("无法解析应用首路由 `{path}`：{source}")]
    InitialRoute {
        /// 应用选项中配置的原始路径或 deeplink。
        path: String,
        /// 注册表返回的具体解析错误。
        #[source]
        source: ResolveError,
    },

    /// 配置的首路由指向独立窗口，不能作为主窗口 Feature 内容。
    #[error("应用首路由 `{path}` 指向 Window `{id}`，必须配置为 Feature 路径")]
    InitialRouteIsWindow {
        /// 应用选项中配置的原始路径或 deeplink。
        path: String,
        /// 被首路由匹配到的 Window 稳定标识。
        id: &'static str,
    },

    /// 正式安装包携带的 `nexora-release.json` 存在但无法读取或通过结构校验。
    #[error("无法加载应用发布信息：{message}")]
    InvalidReleaseMetadata {
        /// 不包含秘密或文件正文的失败原因。
        message: String,
    },

    /// 下游应用注册的主题预设或默认主题 ID 无法通过启动前校验。
    #[error("应用主题配置无效：{message}")]
    InvalidThemeConfiguration {
        /// 与错误直接相关的预设 ID；注册表级错误可能没有单一 ID。
        preset_id: Option<String>,
        /// 不包含完整主题 JSON 的安全错误说明。
        message: String,
    },
}

impl From<::desktop::process::ProcessError> for ApplicationError {
    fn from(error: ::desktop::process::ProcessError) -> Self {
        Self::Process {
            message: error.to_string(),
        }
    }
}

/// Nexora 桌面应用的最小实现契约。
///
/// 框架负责自动发现 Feature、校验首路由、创建主窗口和通用导航 Shell。应用通常只需
/// 实现 [`Self::options`]；需要注册业务 Global、Action 或服务时再覆盖
/// [`Self::initialize`]。
///
/// # Examples
///
/// ```no_run
/// use nexora::{Application as _, ApplicationOptions};
///
/// struct DesktopApplication;
///
/// impl nexora::Application for DesktopApplication {
///     fn options(&self) -> ApplicationOptions {
///         ApplicationOptions::new().initial_path("/")
///     }
/// }
///
/// DesktopApplication.run()?;
/// # Ok::<(), nexora::ApplicationError>(())
/// ```
pub trait Application: Sized + 'static {
    /// 返回本次启动使用的应用选项。
    ///
    /// 默认实现会打开一个可直接使用的标准窗口；应用可以按值构造并返回自己的配置，
    /// 不需要在类型中保存一份仅供框架修改的可变选项。
    fn options(&self) -> ApplicationOptions {
        ApplicationOptions::default()
    }

    /// 在组件库初始化完成后、主窗口创建前初始化应用自己的全局状态。
    ///
    /// 这里适合注册 Global、Action、服务和恢复本地偏好。Feature Entity 和首路由由
    /// 框架随后创建，应用不需要自行组装 RootView。
    fn initialize(&mut self, _cx: &mut App) {}

    /// 校验自动发现的注册表和配置的首路由。
    ///
    /// 该方法不会启动 GPUI 或创建窗口，可以用于测试、诊断和启动前检查。
    ///
    /// # Errors
    ///
    /// 注册表无效、首路由无法解析，或首路由指向独立 Window 时返回错误。
    fn validate(&self) -> Result<(), ApplicationError> {
        prepare_application(&self.options()).map(|_| ())
    }

    /// 启动 Nexora 桌面应用并进入 GPUI 事件循环。
    ///
    /// 框架会先完成与 [`Self::validate`] 相同的同步校验。只有校验成功后才初始化原生
    /// 应用、创建主窗口并打开首个 Feature。
    ///
    /// # Errors
    ///
    /// 注册表无效、首路由无法解析，或首路由指向独立 Window 时返回错误。
    fn run(self) -> Result<(), ApplicationError> {
        run_application(self)
    }
}

struct PreparedApplication {
    registry: AppRegistry,
    initial_route: RouteMatch,
    account_registry: AppRegistry,
    account_initial_route: RouteMatch,
    theme_catalog: theme::ThemeCatalog,
}

const MAIN_WINDOW_BOUNDS_SAVE_DELAY: Duration = Duration::from_millis(120);
const SHELL_PREFERENCES_SCHEMA_VERSION: u32 = 3;
const MAIN_WINDOW_SESSION_ID: &str = "main";

/// Shell 写入 `workspace.toml` 的用户偏好快照。
///
/// 该类型把主窗口 Shell、默认设置窗口和窗口生命周期观察器共享的偏好集中到同一个
/// TOML 文档中。所有运行时修改都应通过框架安装的偏好运行时合并，避免多个窗口分别
/// 读写同一文件时覆盖其他字段。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShellPreferences {
    /// 当前偏好文档 schema 版本；高于框架支持版本的文件不会被覆盖。
    #[serde(default = "legacy_shell_preferences_schema_version")]
    pub schema_version: u32,
    /// 最近一次由权威写入者提交的单调递增修订号。
    pub revision: u64,
    /// 以 Feature/Window 与表格稳定 ID 组合键保存的 DataTable 列布局。
    pub table_layouts: BTreeMap<String, ui::DataTableLayout>,
    /// 外观相关偏好，包括主题预设、颜色模式、字号与组件尺寸。
    pub appearance: ShellAppearancePreferences,
    /// 主窗口最后一次有效的显示器和窗口边界记录。
    pub main_window: Option<MainWindowPlacement>,
    /// Account 登录偏好与恢复许可；其中不包含任何 token 或安全存储内容。
    pub account: AccountPreferences,
    /// 用户主动选择的 Sidebar 折叠偏好；临时窄窗口折叠不得写入该值。
    pub sidebar_collapsed: bool,
    /// 是否允许框架持久化成功执行的全局搜索项。
    pub search_history_enabled: bool,
    /// 按 Account 稳定用户 ID 隔离的最近搜索历史，每个分区最多 20 条。
    pub search_history: BTreeMap<String, Vec<String>>,
    /// 当前 schema 尚未识别的字段，读取和重写时原样保留。
    #[serde(flatten)]
    unknown_fields: BTreeMap<String, toml::Value>,
    #[serde(default, rename = "windows", skip_serializing)]
    legacy_windows: Option<Vec<LegacyRuntimeWindowState>>,
    #[serde(default, rename = "pinned_tabs", skip_serializing)]
    legacy_pinned_tabs: Option<Vec<String>>,
    #[serde(skip_serializing)]
    startup_display_uuid: Option<String>,
}

const fn legacy_shell_preferences_schema_version() -> u32 {
    0
}

impl Default for ShellPreferences {
    fn default() -> Self {
        Self {
            schema_version: SHELL_PREFERENCES_SCHEMA_VERSION,
            revision: 0,
            table_layouts: BTreeMap::new(),
            appearance: ShellAppearancePreferences::default(),
            main_window: None,
            account: AccountPreferences::default(),
            sidebar_collapsed: false,
            search_history_enabled: true,
            search_history: BTreeMap::new(),
            unknown_fields: BTreeMap::new(),
            legacy_windows: None,
            legacy_pinned_tabs: None,
            startup_display_uuid: None,
        }
    }
}

impl VersionedConfiguration for ShellPreferences {
    const CURRENT_SCHEMA_VERSION: u32 = SHELL_PREFERENCES_SCHEMA_VERSION;

    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
struct LegacyRuntimeWindowState {
    #[serde(alias = "id")]
    session_id: String,
    display_uuid: Option<String>,
    bounds: Option<PersistedWindowBounds>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeWindowRole {
    MainShell,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeWindowTab {
    route_id: String,
    location: String,
    pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeWindowState {
    role: RuntimeWindowRole,
    tabs: Vec<RuntimeWindowTab>,
    active_tab: Option<String>,
    display_uuid: Option<String>,
}

impl Default for RuntimeWindowState {
    fn default() -> Self {
        Self {
            role: RuntimeWindowRole::Shell,
            tabs: Vec::new(),
            active_tab: None,
            display_uuid: None,
        }
    }
}

impl RuntimeWindowState {
    fn main() -> Self {
        Self {
            role: RuntimeWindowRole::MainShell,
            ..Self::default()
        }
    }

    fn move_tab_within_partition(&mut self, from: usize, to: usize) -> bool {
        if from >= self.tabs.len() || to >= self.tabs.len() {
            return false;
        }
        let source_route_id = self.tabs[from].route_id.clone();
        self.normalize_tab_partitions();
        let Some(from) = self
            .tabs
            .iter()
            .position(|tab| tab.route_id == source_route_id)
        else {
            return false;
        };
        let pinned_count = self.tabs.iter().take_while(|tab| tab.pinned).count();
        let source_pinned = self.tabs[from].pinned;
        let target = if source_pinned {
            to.min(pinned_count.saturating_sub(1))
        } else {
            to.max(pinned_count).min(self.tabs.len().saturating_sub(1))
        };
        if from == target {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(target, tab);
        true
    }

    fn normalize_tab_partitions(&mut self) {
        let (mut pinned, regular): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tabs)
            .into_iter()
            .partition(|tab| tab.pinned);
        pinned.extend(regular);
        self.tabs = pinned;
    }
}

/// Shell 偏好文件中的 Account 非敏感登录选项。
///
/// `remember_login` 只记录用户是否希望下次启动尝试恢复；`recovery_allowed` 是安全存储
/// 写入完成后的提交标记。两者都不包含 refresh token、access token 或完整认证响应。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct AccountPreferences {
    /// Windows/macOS 上登录成功后是否尝试保存恢复凭据，默认开启。
    pub remember_login: bool,
    /// 只有安全凭据已成功保存时才为 `true`，默认关闭。
    pub recovery_allowed: bool,
}

impl Default for AccountPreferences {
    fn default() -> Self {
        Self {
            remember_login: true,
            recovery_allowed: false,
        }
    }
}

/// Shell 持久化的外观偏好。
///
/// 字段使用稳定字符串保存，读取时再映射到当前程序支持的主题枚举。这样旧版本、
/// 未来版本或手动编辑出的未知值不会导致整个偏好文件解析失败。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ShellAppearancePreferences {
    /// 主题预设的稳定标识，例如 `nexora`。
    pub theme_preset: String,
    /// 颜色模式的稳定标识，例如 `system`、`light` 或 `dark`。
    pub color_scheme: String,
    /// 基础字号，读取时会限制在 theme crate 声明的合法范围内。
    pub font_size: i64,
    /// gpui-component 组件尺寸标识，例如 `medium`。
    pub component_size: String,
}

impl Default for ShellAppearancePreferences {
    fn default() -> Self {
        Self {
            theme_preset: theme::NEXORA_THEME_PRESET_ID.to_owned(),
            color_scheme: theme::ColorScheme::default().id().to_owned(),
            font_size: i64::from(theme::DEFAULT_FONT_SIZE),
            component_size: theme::DEFAULT_COMPONENT_SIZE.as_str().to_owned(),
        }
    }
}

impl ShellAppearancePreferences {
    fn from_theme(cx: &App) -> Self {
        Self {
            theme_preset: theme::selection(cx).preset_id().to_owned(),
            color_scheme: theme::selection(cx).color_scheme().id().to_owned(),
            font_size: i64::from(theme::font_size(cx)),
            component_size: theme::component_size(cx).as_str().to_owned(),
        }
    }

    fn theme_selection(&self, prefer_saved_theme: bool, cx: &App) -> theme::ThemeSelection {
        let persisted_id = prefer_saved_theme.then_some(self.theme_preset.as_str());
        let preset_id = theme::resolve_preset_id(persisted_id, cx);
        let color_scheme =
            theme::ColorScheme::from_id(self.color_scheme.as_str()).unwrap_or_default();
        theme::ThemeSelection::new(preset_id.to_owned(), color_scheme)
    }

    fn font_size(&self) -> u16 {
        self.font_size.clamp(
            i64::from(theme::MIN_FONT_SIZE),
            i64::from(theme::MAX_FONT_SIZE),
        ) as u16
    }

    fn component_size(&self) -> ComponentSize {
        match self.component_size.as_str() {
            "xsmall" => ComponentSize::XSmall,
            "small" => ComponentSize::Small,
            "medium" => ComponentSize::Medium,
            "large" => ComponentSize::Large,
            _ => theme::DEFAULT_COMPONENT_SIZE,
        }
    }
}

/// 主窗口最后一次有效位置和状态。
///
/// `display_uuid` 保存平台提供的稳定显示器 UUID，不保存进程内 [`gpui::DisplayId`]。
/// 当该显示器暂时不存在时，启动流程会用主显示器生成临时可见位置，但不会只因为
/// 自动回退改写这个 UUID。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct MainWindowPlacement {
    /// 主窗口所在显示器的稳定 UUID。
    pub display_uuid: String,
    /// 主窗口的窗口化、最大化或全屏状态，以及对应恢复边界。
    pub bounds: PersistedWindowBounds,
}

/// 可序列化的主窗口边界状态。
///
/// 最大化和全屏变体中的坐标与尺寸表示 GPUI `WindowBounds` 保存的恢复边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PersistedWindowBounds {
    /// 窗口化状态及其当前边界。
    Windowed {
        /// 窗口左上角 X 坐标。
        x: i32,
        /// 窗口左上角 Y 坐标。
        y: i32,
        /// 窗口宽度。
        width: i32,
        /// 窗口高度。
        height: i32,
    },
    /// 最大化状态及其恢复边界。
    Maximized {
        /// 恢复边界左上角 X 坐标。
        x: i32,
        /// 恢复边界左上角 Y 坐标。
        y: i32,
        /// 恢复边界宽度。
        width: i32,
        /// 恢复边界高度。
        height: i32,
    },
    /// 全屏状态及其恢复边界。
    Fullscreen {
        /// 恢复边界左上角 X 坐标。
        x: i32,
        /// 恢复边界左上角 Y 坐标。
        y: i32,
        /// 恢复边界宽度。
        width: i32,
        /// 恢复边界高度。
        height: i32,
    },
}

impl PersistedWindowBounds {
    /// 从 GPUI 原生窗口边界转换为可写入用户偏好的表示。
    ///
    /// 当 GPUI 返回非正尺寸的恢复边界时返回 `None`，调用方应保留上一份有效记录。
    pub fn from_window_bounds(bounds: WindowBounds) -> Option<Self> {
        let persisted = match bounds {
            WindowBounds::Windowed(bounds) => Self::windowed(bounds),
            WindowBounds::Maximized(bounds) => Self::maximized(bounds),
            WindowBounds::Fullscreen(bounds) => Self::fullscreen(bounds),
        };
        persisted.is_valid().then_some(persisted)
    }

    /// 从 GPUI 窗口化边界创建可序列化状态。
    pub fn windowed(bounds: Bounds<Pixels>) -> Self {
        let (x, y, width, height) = rounded_bounds_parts(bounds);
        Self::Windowed {
            x,
            y,
            width,
            height,
        }
    }

    /// 从 GPUI 最大化恢复边界创建可序列化状态。
    pub fn maximized(bounds: Bounds<Pixels>) -> Self {
        let (x, y, width, height) = rounded_bounds_parts(bounds);
        Self::Maximized {
            x,
            y,
            width,
            height,
        }
    }

    /// 从 GPUI 全屏恢复边界创建可序列化状态。
    pub fn fullscreen(bounds: Bounds<Pixels>) -> Self {
        let (x, y, width, height) = rounded_bounds_parts(bounds);
        Self::Fullscreen {
            x,
            y,
            width,
            height,
        }
    }

    fn is_valid(self) -> bool {
        let (_, _, width, height) = self.parts();
        width > 0 && height > 0
    }

    fn into_window_bounds(
        self,
        display_bounds: Bounds<Pixels>,
        minimum_size: Option<Size<Pixels>>,
    ) -> Option<WindowBounds> {
        let bounds = sanitize_bounds(self.parts(), display_bounds, minimum_size)?;
        Some(match self {
            Self::Windowed { .. } => WindowBounds::Windowed(bounds),
            Self::Maximized { .. } => WindowBounds::Maximized(bounds),
            Self::Fullscreen { .. } => WindowBounds::Fullscreen(bounds),
        })
    }

    fn parts(self) -> (i32, i32, i32, i32) {
        match self {
            Self::Windowed {
                x,
                y,
                width,
                height,
            }
            | Self::Maximized {
                x,
                y,
                width,
                height,
            }
            | Self::Fullscreen {
                x,
                y,
                width,
                height,
            } => (x, y, width, height),
        }
    }
}

fn rounded_bounds_parts(bounds: Bounds<Pixels>) -> (i32, i32, i32, i32) {
    (
        f32::from(bounds.origin.x).round() as i32,
        f32::from(bounds.origin.y).round() as i32,
        f32::from(bounds.size.width).round() as i32,
        f32::from(bounds.size.height).round() as i32,
    )
}

fn sanitize_bounds(
    (x, y, width, height): (i32, i32, i32, i32),
    display_bounds: Bounds<Pixels>,
    minimum_size: Option<Size<Pixels>>,
) -> Option<Bounds<Pixels>> {
    if width <= 0 || height <= 0 {
        return None;
    }

    let display_x = f32::from(display_bounds.origin.x);
    let display_y = f32::from(display_bounds.origin.y);
    let display_width = f32::from(display_bounds.size.width).max(1.0);
    let display_height = f32::from(display_bounds.size.height).max(1.0);
    let min_width = minimum_size
        .map(|size| f32::from(size.width))
        .unwrap_or(1.0)
        .max(1.0);
    let min_height = minimum_size
        .map(|size| f32::from(size.height))
        .unwrap_or(1.0)
        .max(1.0);
    let width = (width as f32).max(min_width).min(display_width);
    let height = (height as f32).max(min_height).min(display_height);
    let max_x = display_x + display_width - width;
    let max_y = display_y + display_height - height;
    let x = (x as f32).clamp(display_x, max_x.max(display_x));
    let y = (y as f32).clamp(display_y, max_y.max(display_y));

    Some(Bounds::new(
        point(px(x), px(y)),
        size(px(width), px(height)),
    ))
}

impl ShellPreferences {
    fn for_local_application(application_name: &str) -> Option<UserConfigStore<Self>> {
        UserConfigStore::for_local_application("com", "Nexora", application_name, "workspace.toml")
            .ok()
    }

    /// 把已读取的历史偏好原地迁移到当前 schema。
    ///
    /// 返回 `true` 表示内存快照发生了变化，调用方应将它安全写回。
    pub fn migrate_to_current(&mut self) -> bool {
        let retired_fields_present = self.legacy_windows.is_some()
            || self.legacy_pinned_tabs.is_some()
            || self.unknown_fields.remove("windows").is_some()
            || self.unknown_fields.remove("pinned_tabs").is_some();
        if self.schema_version >= SHELL_PREFERENCES_SCHEMA_VERSION && !retired_fields_present {
            return false;
        }

        if self.schema_version == 1
            && let Some(main) = self
                .legacy_windows
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|session| session.session_id == MAIN_WINDOW_SESSION_ID)
            && let Some(bounds) = main.bounds
        {
            self.main_window = Some(MainWindowPlacement {
                display_uuid: main.display_uuid.clone().unwrap_or_default(),
                bounds,
            });
        }
        self.legacy_windows = None;
        self.legacy_pinned_tabs = None;
        self.schema_version = SHELL_PREFERENCES_SCHEMA_VERSION;
        self.revision = self.revision.saturating_add(1);
        true
    }

    fn merge_changed_fields(&mut self, before: &Self, after: &Self) {
        if before.appearance != after.appearance {
            self.appearance = after.appearance.clone();
        }
        if before.account != after.account {
            self.account = after.account.clone();
        }
        if before.sidebar_collapsed != after.sidebar_collapsed {
            self.sidebar_collapsed = after.sidebar_collapsed;
        }
        if before.search_history_enabled != after.search_history_enabled {
            self.search_history_enabled = after.search_history_enabled;
        }
        merge_changed_map(
            &mut self.search_history,
            &before.search_history,
            &after.search_history,
        );
        merge_changed_map(
            &mut self.table_layouts,
            &before.table_layouts,
            &after.table_layouts,
        );
        if before.main_window != after.main_window {
            self.main_window = after.main_window.clone();
        }
        self.schema_version = SHELL_PREFERENCES_SCHEMA_VERSION;
        self.revision = self.revision.max(after.revision).saturating_add(1);
    }
}

fn merge_changed_map<T: Clone + PartialEq>(
    target: &mut BTreeMap<String, T>,
    before: &BTreeMap<String, T>,
    after: &BTreeMap<String, T>,
) {
    let changed_keys = before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .cloned()
        .collect::<HashSet<_>>();
    for key in changed_keys {
        if let Some(value) = after.get(&key) {
            target.insert(key, value.clone());
        } else {
            target.remove(&key);
        }
    }
}

pub(crate) fn load_shell_preferences(application_name: &str) -> ShellPreferences {
    ShellPreferences::for_local_application(application_name)
        .and_then(|store| match store.load_versioned_or_default() {
            Ok(mut preferences) => {
                if preferences.migrate_to_current()
                    && let Err(error) = store.save(&preferences)
                {
                    tracing::warn!(error = %error, "无法保存迁移后的 Shell 用户偏好");
                }
                Some(preferences)
            }
            Err(error) => {
                tracing::warn!(error = %error, "无法读取 Shell 用户偏好，已使用默认值");
                None
            }
        })
        .unwrap_or_default()
}

pub(crate) fn shell_preferences_snapshot(cx: &App) -> ShellPreferences {
    cx.try_global::<ShellPreferencesRuntime>()
        .map(|runtime| runtime.snapshot.clone())
        .unwrap_or_default()
}

pub(crate) fn shell_preferences_flush_signal(cx: &App) -> Option<Receiver<()>> {
    cx.try_global::<ShellPreferencesRuntime>()
        .and_then(|runtime| runtime.writer.as_ref())
        .and_then(PreferencesWriter::flush_signal)
}

pub(crate) fn persist_current_appearance_preferences(cx: &mut App) {
    let appearance = ShellAppearancePreferences::from_theme(cx);
    update_shell_preferences(cx, |preferences| {
        preferences.appearance = appearance;
    });
}

pub(crate) fn update_shell_preferences(cx: &mut App, update: impl FnOnce(&mut ShellPreferences)) {
    if cx.has_global::<ShellPreferencesRuntime>() {
        cx.update_global::<ShellPreferencesRuntime, _>(|runtime, _cx| {
            let before = runtime.snapshot.clone();
            update(&mut runtime.snapshot);
            runtime.snapshot.schema_version = SHELL_PREFERENCES_SCHEMA_VERSION;
            runtime.snapshot.revision = runtime.snapshot.revision.saturating_add(1);
            runtime.persist(before);
        });
    } else {
        let branding = application_branding(cx);
        let mut preferences = load_shell_preferences(branding.application_name.as_str());
        let before = preferences.clone();
        update(&mut preferences);
        preferences.revision = preferences.revision.saturating_add(1);
        if let Some(store) =
            ShellPreferences::for_local_application(branding.application_name.as_str())
            && let Err(error) = store.update_versioned(|latest| {
                latest.migrate_to_current();
                latest.merge_changed_fields(&before, &preferences);
            })
        {
            tracing::warn!(error = %error, "无法保存 Shell 用户偏好");
        }
    }
}

struct PreferencesWriter {
    sender: Sender<PreferencesWriteCommand>,
    worker: Option<JoinHandle<()>>,
}

enum PreferencesWriteCommand {
    Persist {
        before: Box<ShellPreferences>,
        after: Box<ShellPreferences>,
    },
    Flush(Sender<()>),
    Shutdown,
}

impl PreferencesWriter {
    fn start(store: UserConfigStore<ShellPreferences>) -> Result<Self, std::io::Error> {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("nexora-shell-preferences".to_owned())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        PreferencesWriteCommand::Persist { before, after } => {
                            if let Err(error) = store.update_versioned(|latest| {
                                latest.migrate_to_current();
                                latest.merge_changed_fields(&before, &after);
                            }) {
                                tracing::warn!(error = %error, "无法保存 Shell 用户偏好");
                            }
                        }
                        PreferencesWriteCommand::Flush(ack) => {
                            _ = ack.send(());
                        }
                        PreferencesWriteCommand::Shutdown => break,
                    }
                }
            })?;

        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    fn persist(&self, before: ShellPreferences, after: ShellPreferences) {
        _ = self.sender.send(PreferencesWriteCommand::Persist {
            before: Box::new(before),
            after: Box::new(after),
        });
    }

    fn flush(&self) {
        let (sender, receiver) = mpsc::channel();
        if self
            .sender
            .send(PreferencesWriteCommand::Flush(sender))
            .is_ok()
        {
            _ = receiver.recv();
        }
    }

    fn flush_signal(&self) -> Option<Receiver<()>> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(PreferencesWriteCommand::Flush(sender))
            .ok()
            .map(|_| receiver)
    }
}

impl Drop for PreferencesWriter {
    fn drop(&mut self) {
        _ = self.sender.send(PreferencesWriteCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            _ = worker.join();
        }
    }
}

struct ShellPreferencesRuntime {
    snapshot: ShellPreferences,
    writer: Option<PreferencesWriter>,
}

impl ShellPreferencesRuntime {
    fn new(snapshot: ShellPreferences, store: Option<UserConfigStore<ShellPreferences>>) -> Self {
        let writer = store.and_then(|store| match PreferencesWriter::start(store) {
            Ok(writer) => Some(writer),
            Err(error) => {
                tracing::warn!(error = %error, "无法启动 Shell 用户偏好写入器");
                None
            }
        });

        Self { snapshot, writer }
    }

    fn persist(&self, before: ShellPreferences) {
        if let Some(writer) = self.writer.as_ref() {
            writer.persist(before, self.snapshot.clone());
        }
    }

    fn flush(&self) {
        if let Some(writer) = self.writer.as_ref() {
            writer.flush();
        }
    }
}

impl Global for ShellPreferencesRuntime {}

fn install_shell_preferences_runtime(
    preferences: ShellPreferences,
    store: Option<UserConfigStore<ShellPreferences>>,
    cx: &mut App,
) {
    let runtime = ShellPreferencesRuntime::new(preferences, store);
    if cx.has_global::<ShellPreferencesRuntime>() {
        *cx.global_mut::<ShellPreferencesRuntime>() = runtime;
    } else {
        cx.set_global(runtime);
    }
}

/// 把一份已有偏好文件中的外观设置恢复到当前 GPUI 主题运行时。
///
/// 未知主题预设会回退到应用默认主题，历史 `xuwe` 会迁移到 `nexora`；未知颜色模式和
/// 组件尺寸会回退到安全默认值，字号会限制在 theme crate 声明的合法范围内。
pub fn restore_appearance_preferences(preferences: &ShellPreferences, cx: &mut App) {
    restore_initial_appearance_preferences(preferences, true, cx);
}

fn restore_initial_appearance_preferences(
    preferences: &ShellPreferences,
    prefer_saved_theme: bool,
    cx: &mut App,
) {
    theme::set_selection(
        preferences
            .appearance
            .theme_selection(prefer_saved_theme, cx),
        cx,
    )
    .expect("恢复后的主题选择必须来自已注册目录");
    theme::set_font_size(preferences.appearance.font_size(), cx);
    theme::set_component_size(preferences.appearance.component_size(), cx);
}

/// 把持久化的主窗口记录应用到本次启动的桌面窗口选项。
///
/// 返回 `true` 表示存在可用历史记录，调用方应停止再按首次启动尺寸重算窗口边界。
/// 当记录的显示器缺失时，本函数只生成主显示器上的临时安全位置，不修改传入的偏好快照。
pub fn restore_main_window_options(
    options: &mut DesktopApplicationOptions,
    preferences: &ShellPreferences,
    cx: &App,
) -> bool {
    let Some(restored) = restored_main_window_bounds(
        preferences.main_window.as_ref(),
        options.window_min_size,
        cx,
    ) else {
        return false;
    };
    let window_options = options
        .window_options
        .get_or_insert_with(WindowOptions::default);
    window_options.display_id = restored.display_id;
    window_options.window_bounds = Some(restored.bounds);
    options.window_size = None;
    options.startup_display_uuid = None;
    true
}

pub(crate) struct RestoredMainWindowBounds {
    pub(crate) display_id: Option<gpui::DisplayId>,
    pub(crate) bounds: WindowBounds,
}

fn restored_main_window_bounds(
    placement: Option<&MainWindowPlacement>,
    minimum_size: Option<Size<Pixels>>,
    cx: &App,
) -> Option<RestoredMainWindowBounds> {
    let placement = placement?;
    let target_display_id = ::desktop::find_display_id_by_uuid(placement.display_uuid.as_str(), cx);
    let display = target_display_id
        .and_then(|display_id| cx.find_display(display_id))
        .or_else(|| cx.primary_display())?;
    let bounds = placement
        .bounds
        .into_window_bounds(display.visible_bounds(), minimum_size)?;

    Some(RestoredMainWindowBounds {
        display_id: target_display_id,
        bounds,
    })
}

fn capture_main_window_placement(window: &Window, cx: &App) -> Option<MainWindowPlacement> {
    let display_uuid = window.display(cx)?.uuid().ok()?.to_string();
    let bounds = PersistedWindowBounds::from_window_bounds(window.window_bounds())?;

    Some(MainWindowPlacement {
        display_uuid,
        bounds,
    })
}

fn should_update_main_window_placement(
    current: &MainWindowPlacement,
    preferences: &ShellPreferences,
    cx: &App,
) -> bool {
    let existing_display_uuid = preferences
        .main_window
        .as_ref()
        .map(|placement| placement.display_uuid.as_str());
    existing_display_uuid.is_none_or(|existing| {
        existing == current.display_uuid
            || ::desktop::find_display_id_by_uuid(existing, cx).is_some()
    })
}

fn prepare_application(
    options: &ApplicationOptions,
) -> Result<PreparedApplication, ApplicationError> {
    let theme_catalog = theme::ThemeCatalog::new(
        &options.theme_presets,
        options.default_theme_preset.as_deref(),
    )
    .map_err(|error| ApplicationError::InvalidThemeConfiguration {
        preset_id: error.preset_id().map(str::to_owned),
        message: error.to_string(),
    })?;
    let registry = AppRegistry::discover_for_application(false)?;
    let initial_route = registry
        .resolve(options.initial_path.as_str())
        .map_err(|source| ApplicationError::InitialRoute {
            path: options.initial_path.clone(),
            source,
        })?;
    if initial_route.target().kind() == RouteTargetKind::Window {
        return Err(ApplicationError::InitialRouteIsWindow {
            path: options.initial_path.clone(),
            id: initial_route.target().id(),
        });
    }

    let account_registry = AppRegistry::discover_for_application(true)?;
    let account_initial_route = account_registry
        .resolve(options.initial_path.as_str())
        .map_err(|source| ApplicationError::InitialRoute {
            path: options.initial_path.clone(),
            source,
        })?;

    Ok(PreparedApplication {
        registry,
        initial_route,
        account_registry,
        account_initial_route,
        theme_catalog,
    })
}

fn run_application<A>(application: A) -> Result<(), ApplicationError>
where
    A: Application,
{
    let options = application.options();
    let PreparedApplication {
        registry,
        initial_route,
        account_registry,
        account_initial_route,
        theme_catalog,
    } = prepare_application(&options)?;
    let locale = options.locale.clone();
    let configured_application_name = options.application_name.clone();
    let application_version = options.application_version.clone();
    let application_info = crate::application_info::ApplicationInfo::load(
        configured_application_name,
        application_version.clone(),
    )
    .map_err(|message| ApplicationError::InvalidReleaseMetadata { message })?;
    let application_name = application_info.application_name().to_owned();
    let application_identity = ::desktop::process::ApplicationIdentity::resolve(
        application_name.as_str(),
        application_info.app_id(),
        options.application_identity_override.as_deref(),
    )?;
    let application_identity_value = application_identity.as_str().to_owned();
    let tray_enabled = options.tray_enabled;
    let tray_unavailable_policy = options.tray_unavailable_policy;
    let process = ::desktop::process::bootstrap(::desktop::process::ProcessBootstrapOptions {
        identity: application_identity,
        enabled: true,
        runtime_root: None,
    })?;
    if matches!(
        process,
        ::desktop::process::ProcessBootstrap::SecondaryActivated
    ) {
        return Ok(());
    }
    let application_logo = options.application_logo;
    let sidebar_subtitle = options.sidebar_subtitle.clone();
    let tab_style = options.tab_style;
    let sidebar_search = options.sidebar_search;
    let mut preferences_store = ShellPreferences::for_local_application(application_name.as_str());
    let preferences_file_existed = preferences_store
        .as_ref()
        .is_some_and(|store| store.path().exists());
    let mut shell_preferences_loaded = false;
    let mut shell_preferences = match preferences_store
        .as_ref()
        .map(UserConfigStore::load_versioned_or_default)
    {
        Some(Ok(preferences)) => {
            shell_preferences_loaded = preferences_file_existed;
            preferences
        }
        Some(Err(error @ ConfigurationError::UnsupportedSchema { .. })) => {
            tracing::warn!(error = %error, "偏好 schema 来自更高版本，本次运行不会覆盖该文件");
            preferences_store = None;
            ShellPreferences::default()
        }
        Some(Err(error)) => {
            tracing::warn!(error = %error, "无法读取 Shell 用户偏好，已使用默认值");
            ShellPreferences::default()
        }
        None => ShellPreferences::default(),
    };
    if shell_preferences.migrate_to_current()
        && let Some(store) = &preferences_store
        && let Err(error) = store.save(&shell_preferences)
    {
        tracing::warn!(error = %error, "无法保存迁移后的 Shell 用户偏好");
    }
    let mut desktop_options =
        options.into_desktop_options(application_name.as_str(), theme_catalog);
    if tray_enabled {
        desktop_options.daemon_mode = true;
    }
    let adapter = ApplicationAdapter {
        application,
        options: desktop_options,
        locale,
        application_name,
        application_info,
        application_logo,
        account_enabled: false,
        sidebar_subtitle,
        tab_style,
        sidebar_search,
        preferences_store,
        shell_preferences,
        shell_preferences_loaded,
        process: Some(process),
        application_identity: application_identity_value,
        tray_enabled,
        tray_unavailable_policy,
        registry: Some(registry),
        initial_route: Some(initial_route),
        account_registry: Some(account_registry),
        account_initial_route: Some(account_initial_route),
    };

    DesktopApplication::run(adapter);
    Ok(())
}

struct ApplicationAdapter<A> {
    application: A,
    options: DesktopApplicationOptions,
    locale: String,
    application_name: String,
    application_info: crate::application_info::ApplicationInfo,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    tab_style: ApplicationTabStyle,
    sidebar_search: bool,
    preferences_store: Option<UserConfigStore<ShellPreferences>>,
    shell_preferences: ShellPreferences,
    shell_preferences_loaded: bool,
    process: Option<::desktop::process::ProcessBootstrap>,
    application_identity: String,
    tray_enabled: bool,
    tray_unavailable_policy: TrayUnavailablePolicy,
    registry: Option<AppRegistry>,
    initial_route: Option<RouteMatch>,
    account_registry: Option<AppRegistry>,
    account_initial_route: Option<RouteMatch>,
}

struct ApplicationProcessRuntime {
    process: ::desktop::process::ProcessBootstrap,
    application_identity: String,
    tray: Option<::desktop::tray::TrayController>,
    tray_enabled: bool,
    tray_unavailable_policy: TrayUnavailablePolicy,
    exiting: bool,
}

impl Global for ApplicationProcessRuntime {}

struct InProcessWindowRuntime {
    registry: AppRegistry,
    shell_template: ApplicationShellTemplate,
    shell_window_options: ApplicationShellWindowOptions,
}

impl Global for InProcessWindowRuntime {}

#[derive(Clone)]
struct ApplicationShellTemplate {
    application_name: String,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    tab_style: ApplicationTabStyle,
    sidebar_search: bool,
}

impl ApplicationShellTemplate {
    fn config(&self, window_state: RuntimeWindowState) -> ApplicationShellConfig {
        ApplicationShellConfig {
            application_name: self.application_name.clone(),
            application_logo: self.application_logo,
            account_enabled: self.account_enabled,
            sidebar_subtitle: self.sidebar_subtitle.clone(),
            tab_style: self.tab_style,
            sidebar_search: self.sidebar_search,
            window_state,
        }
    }
}

struct ApplicationShellWindowOptions {
    base: WindowOptions,
    window_size: Option<Size<Pixels>>,
    window_min_size: Option<Size<Pixels>>,
}

impl ApplicationShellWindowOptions {
    fn from_desktop_options(options: &DesktopApplicationOptions) -> Self {
        Self {
            base: options
                .window_options
                .as_ref()
                .map(clone_window_options)
                .unwrap_or_default(),
            window_size: options.window_size,
            window_min_size: options.window_min_size,
        }
    }

    fn for_display(&self, display_uuid: Option<&str>, cx: &App) -> WindowOptions {
        let mut options = clone_window_options(&self.base);
        ::desktop::apply_window_display_preference(
            &mut options,
            display_uuid,
            self.window_size,
            cx,
        );
        if let Some(window_min_size) = self.window_min_size {
            options.window_min_size = Some(window_min_size);
        }
        options
    }
}

fn clone_window_options(options: &WindowOptions) -> WindowOptions {
    WindowOptions {
        window_bounds: options.window_bounds,
        titlebar: options
            .titlebar
            .as_ref()
            .map(|titlebar| gpui::TitlebarOptions {
                title: titlebar.title.clone(),
                appears_transparent: titlebar.appears_transparent,
                traffic_light_position: titlebar.traffic_light_position,
            }),
        focus: options.focus,
        show: options.show,
        kind: options.kind.clone(),
        is_movable: options.is_movable,
        app_owns_titlebar_drag: options.app_owns_titlebar_drag,
        is_resizable: options.is_resizable,
        is_minimizable: options.is_minimizable,
        display_id: options.display_id,
        window_background: options.window_background,
        app_id: options.app_id.clone(),
        window_min_size: options.window_min_size,
        window_decorations: options.window_decorations,
        icon: options.icon.clone(),
        tabbing_identifier: options.tabbing_identifier.clone(),
    }
}

fn install_application_process_runtime(runtime: ApplicationProcessRuntime, cx: &mut App) {
    cx.set_global(runtime);
    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        loop {
            executor.timer(Duration::from_millis(250)).await;
            let should_continue = cx.update(process_runtime_tick);
            if !should_continue {
                break;
            }
        }
    })
    // nexora-lint: allow(nexora::detached_lifecycle) reason="进程心跳与主进程 IPC 监听属于应用级 Global 生命周期"
    .detach();
}

pub(crate) fn application_identity(cx: &App) -> Option<&str> {
    cx.try_global::<ApplicationProcessRuntime>()
        .map(|runtime| runtime.application_identity.as_str())
}

fn process_runtime_tick(cx: &mut App) -> bool {
    if !cx.has_global::<ApplicationProcessRuntime>() {
        return false;
    }
    let tray_event = cx
        .global_mut::<ApplicationProcessRuntime>()
        .tray
        .as_mut()
        .and_then(::desktop::tray::TrayController::try_recv);
    match tray_event {
        Some(::desktop::tray::TrayEvent::ActivateWindowGroup) => {
            activate_window_group(cx);
        }
        Some(::desktop::tray::TrayEvent::ExitApplication) => {
            coordinated_application_exit(cx);
            return false;
        }
        Some(::desktop::tray::TrayEvent::Unavailable)
        | Some(::desktop::tray::TrayEvent::Available)
        | None => {}
    }
    while let Some(::desktop::process::CoordinatorEvent::ActivateGroup) =
        match &cx.global::<ApplicationProcessRuntime>().process {
            ::desktop::process::ProcessBootstrap::Main(main) => main.try_recv(),
            _ => None,
        }
    {
        activate_window_group(cx);
    }
    true
}
fn minimize_coordinated_window_group(cx: &mut App) {
    for handle in cx.windows() {
        _ = handle.update(cx, |_, window, _| window.minimize_window());
    }
}

fn hide_coordinated_window_group(cx: &mut App) {
    cx.hide();
}

fn activate_window_group(cx: &mut App) {
    for handle in cx.windows() {
        _ = handle.update(cx, |_, window, _| window.activate_window());
    }
    cx.activate(true);
}

fn coordinated_application_exit(cx: &mut App) {
    if cx.has_global::<ApplicationProcessRuntime>() {
        if cx.global::<ApplicationProcessRuntime>().exiting {
            return;
        }
        cx.global_mut::<ApplicationProcessRuntime>().exiting = true;
    }
    if let Some(runtime) = cx.try_global::<ShellPreferencesRuntime>() {
        runtime.flush();
    }
    cx.quit();
}

fn open_in_process_shell_session(
    initial_route: RouteMatch,
    window_state: RuntimeWindowState,
    cx: &mut App,
) -> Result<WindowHandle<gpui_component::Root>, NavigationError> {
    let (registry, config, options) = {
        let runtime = cx.try_global::<InProcessWindowRuntime>().ok_or_else(|| {
            NavigationError::ShellWindow {
                message: "同进程窗口运行时尚未初始化".to_owned(),
            }
        })?;
        let options = runtime
            .shell_window_options
            .for_display(window_state.display_uuid.as_deref(), cx);
        (
            runtime.registry.clone(),
            runtime.shell_template.config(window_state),
            options,
        )
    };
    let handle = cx
        .open_window(options, move |window, cx| {
            let shell =
                cx.new(|cx| ApplicationShell::new(registry, initial_route, config, window, cx));
            let root = cx.new(|cx| gpui_component::Root::new(shell, window, cx));
            theme::attach_window(window, cx);
            root
        })
        .map_err(|source| NavigationError::ShellWindow {
            message: source.to_string(),
        })?;
    cx.activate(true);
    Ok(handle)
}

impl<A> DesktopApplication for ApplicationAdapter<A>
where
    A: Application,
{
    type RootView = ApplicationShell;

    fn options(&self) -> &DesktopApplicationOptions {
        &self.options
    }

    fn options_mut(&mut self) -> &mut DesktopApplicationOptions {
        &mut self.options
    }

    fn initialize(&mut self, cx: &mut App) {
        gpui_component::set_locale(self.locale.as_str());
        let shell_window_options =
            ApplicationShellWindowOptions::from_desktop_options(&self.options);
        const DEFAULT_TRAY_ICON: &[u8] = include_bytes!("../../../assets/logos/logo-icon-128.png");
        let is_main_process = matches!(
            self.process.as_ref(),
            Some(::desktop::process::ProcessBootstrap::Main(_))
        );
        let tray = if self.tray_enabled && is_main_process {
            let icon = self
                .application_logo
                .map(ApplicationLogo::bytes)
                .unwrap_or(DEFAULT_TRAY_ICON);
            match ::desktop::tray::TrayController::new(
                self.application_info
                    .app_id()
                    .unwrap_or(self.application_info.application_name()),
                self.application_info.application_name(),
                icon,
            ) {
                Ok(tray) => Some(tray),
                Err(error) => {
                    tracing::warn!(error = %error, "系统托盘不可用，将按配置执行安全降级");
                    None
                }
            }
        } else {
            None
        };
        install_application_process_runtime(
            ApplicationProcessRuntime {
                process: self.process.take().expect("应用进程协调器只能安装一次"),
                application_identity: self.application_identity.clone(),
                tray,
                tray_enabled: self.tray_enabled,
                tray_unavailable_policy: self.tray_unavailable_policy,
                exiting: false,
            },
            cx,
        );
        install_shell_preferences_runtime(
            self.shell_preferences.clone(),
            self.preferences_store.clone(),
            cx,
        );
        restore_initial_appearance_preferences(
            &self.shell_preferences,
            self.shell_preferences_loaded,
            cx,
        );
        if ShellAppearancePreferences::from_theme(cx) != self.shell_preferences.appearance {
            persist_current_appearance_preferences(cx);
        }
        restore_main_window_options(&mut self.options, &self.shell_preferences, cx);
        let application_name = self.application_info.application_name().to_owned();
        cx.set_global(self.application_info.clone());
        cx.set_global(ApplicationBranding {
            application_name,
            logo: self.application_logo,
        });
        actions::init();
        actions::settings::bind_keys(cx);
        cx.on_action(|_: &OpenSettings, cx| {
            _ = cx.navigate("/settings");
        });
        window_actions::init(self.application_name.clone(), cx);
        self.application.initialize(cx);
        self.account_enabled = crate::account::client::login_snapshot(cx).configured;
        let registry = if self.account_enabled {
            self.account_registry.as_ref()
        } else {
            self.registry.as_ref()
        }
        .expect("同进程窗口注册表应当在主窗口创建前可用")
        .clone();
        cx.set_global(InProcessWindowRuntime {
            registry,
            shell_template: ApplicationShellTemplate {
                application_name: self.application_name.clone(),
                application_logo: self.application_logo,
                account_enabled: self.account_enabled,
                sidebar_subtitle: self.sidebar_subtitle.clone(),
                tab_style: self.tab_style,
                sidebar_search: self.sidebar_search,
            },
            shell_window_options,
        });
        if self.account_enabled {
            account_actions::bind_keys(cx);
            cx.on_action(|_: &SignInAccount, cx| {
                let snapshot = crate::account::client::login_snapshot(cx);
                if !snapshot.authenticated && !snapshot.busy {
                    _ = crate::account::client::start_login(cx);
                }
            });
            cx.on_action(|_: &SignOutAccount, cx| {
                crate::account::client::sign_out(cx);
            });
        }
    }

    fn build_root_view(
        &mut self,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Entity<Self::RootView> {
        let (registry, initial_route) = if self.account_enabled {
            (
                self.account_registry
                    .take()
                    .expect("Nexora Account 主窗口注册表只能被消费一次"),
                self.account_initial_route
                    .take()
                    .expect("Nexora Account 主窗口首路由只能被消费一次"),
            )
        } else {
            (
                self.registry
                    .take()
                    .expect("Nexora 主窗口注册表只能被消费一次"),
                self.initial_route
                    .take()
                    .expect("Nexora 主窗口首路由只能被消费一次"),
            )
        };

        let application_name = self.application_name.clone();
        let application_logo = self.application_logo;
        let account_enabled = self.account_enabled;
        let sidebar_subtitle = self.sidebar_subtitle.clone();
        let tab_style = self.tab_style;
        let sidebar_search = self.sidebar_search;
        let root = cx.new(|cx| {
            ApplicationShell::new(
                registry,
                initial_route,
                ApplicationShellConfig {
                    application_name,
                    application_logo,
                    account_enabled,
                    sidebar_subtitle,
                    tab_style,
                    sidebar_search,
                    window_state: RuntimeWindowState::main(),
                },
                window,
                cx,
            )
        });
        crate::desktop::updater::start_installed_updater(window, cx);
        root
    }
}

#[derive(Debug, Error)]
enum NavigationError {
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Feature(#[from] FeatureRuntimeError),
    #[error(transparent)]
    Window(#[from] WindowRuntimeError),
    #[error(transparent)]
    Process(#[from] ::desktop::process::ProcessError),
    #[error("无法创建同进程 Shell 窗口：{message}")]
    ShellWindow { message: String },
    #[cfg(feature = "desktop")]
    #[error("未登录时不能打开独立窗口 `{path}`")]
    AuthenticationRequired { path: String },
    #[error("当前用户无权查看 Feature `{id}`")]
    FeatureHidden { id: &'static str },
}

#[derive(Debug, Clone)]
struct ShellRoute {
    route: RouteMatch,
    identity: String,
    location: String,
}

impl ShellRoute {
    fn new(route: RouteMatch) -> Self {
        debug_assert_eq!(route.target().kind(), RouteTargetKind::Feature);
        let identity = route.stable_id();
        let location = route.location();
        Self {
            route,
            identity,
            location,
        }
    }

    fn path(&self) -> &str {
        self.route.concrete_path()
    }

    fn identity(&self) -> &str {
        &self.identity
    }

    fn location(&self) -> &str {
        &self.location
    }

    fn title(&self) -> String {
        let title = self.route.target().title();
        let parameters = self
            .route
            .target()
            .path()
            .split('/')
            .zip(self.path().split('/'))
            .filter_map(|(pattern, value)| pattern.starts_with(':').then_some(value))
            .map(|value| percent_decode_str(value).decode_utf8_lossy().into_owned())
            .collect::<Vec<_>>();
        if !parameters.is_empty() {
            return format!("{title} · {}", parameters.join(" · "));
        }

        title.to_owned()
    }

    fn icon(&self) -> Option<&'static str> {
        self.route.target().icon()
    }

    const fn route(&self) -> &RouteMatch {
        &self.route
    }
}

impl PartialEq for ShellRoute {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for ShellRoute {}

#[derive(Clone)]
struct DraggedShellTab {
    route_id: String,
    title: String,
}

impl Render for DraggedShellTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().drag_border)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .child(self.title.clone())
    }
}

struct ApplicationShellConfig {
    application_name: String,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    tab_style: ApplicationTabStyle,
    sidebar_search: bool,
    window_state: RuntimeWindowState,
}

struct ApplicationShell {
    focus_handle: gpui::FocusHandle,
    registry: AppRegistry,
    application_name: String,
    application_logo: Option<ApplicationLogo>,
    account_enabled: bool,
    sidebar_subtitle: Option<String>,
    tab_style: ApplicationTabStyle,
    sidebar_search_input: Option<Entity<InputState>>,
    sidebar_search_index: Option<NavigationSearchIndex>,
    initial_route: ShellRoute,
    active_route: ShellRoute,
    opened_tabs: Vec<ShellRoute>,
    pinned_tabs: Vec<ShellRoute>,
    tab_context_route: Option<ShellRoute>,
    tab_scroll_handle: ScrollHandle,
    reload_tasks: HashMap<String, Task<()>>,
    sidebar_collapsed: bool,
    sidebar_focus_task: Option<Task<()>>,
    navigation_history: Vec<ShellRoute>,
    navigation_history_index: usize,
    expanded_navigation_groups: HashSet<&'static str>,
    main_window_persist_task: Option<Task<()>>,
    feature_instances: HashMap<String, FeatureInstance>,
    #[cfg(feature = "desktop")]
    login_feature: AnyView,
    #[cfg(feature = "desktop")]
    authenticated: bool,
    #[cfg(feature = "desktop")]
    auth_identity: Option<String>,
    #[cfg(feature = "desktop")]
    business_windows: Vec<WindowHandle<gpui_component::Root>>,
    sidebar_header: Option<AnyView>,
    sidebar_footer: Option<AnyView>,
    navigation_error: Option<String>,
    #[cfg(feature = "desktop")]
    _authentication_subscription: Option<Subscription>,
    _window_bounds_subscription: Option<Subscription>,
    _sidebar_search_subscription: Option<Subscription>,
    _release_subscription: Option<Subscription>,
}

#[derive(Clone, Copy)]
enum NavigationEntry {
    Group(NavigationGroupMetadata),
    Feature(FeatureMetadata),
}

impl NavigationEntry {
    fn sort_key(&self) -> (i32, u8, &'static str) {
        match *self {
            Self::Group(metadata) => (metadata.order(), 0, metadata.id()),
            Self::Feature(metadata) => (metadata.order(), 1, metadata.id()),
        }
    }

    fn section(self, registry: &AppRegistry) -> &'static str {
        match self {
            Self::Group(metadata) => metadata.section(),
            Self::Feature(metadata) => registry.feature_section(metadata),
        }
    }
}

#[derive(Clone)]
enum NavigationTreeEntry {
    Group {
        metadata: NavigationGroupMetadata,
        children: Vec<NavigationTreeEntry>,
    },
    Feature(FeatureMetadata),
}

impl NavigationTreeEntry {
    fn collect_group_ids(&self, output: &mut HashSet<&'static str>) {
        if let Self::Group { metadata, children } = self {
            output.insert(metadata.id());
            for child in children {
                child.collect_group_ids(output);
            }
        }
    }
}

struct NavigationSearchResult {
    sections: Vec<(&'static str, Vec<NavigationTreeEntry>)>,
    expanded_groups: HashSet<&'static str>,
}

/// 在官方 Sidebar 使用应用自定义目标宽度时协调展开与收起导航渲染。
///
/// `SidebarCollapsible::None` 让 Sidebar 本体保持 80px/236px 真实宽度，因此容器会向子项
/// 传入 `false`。展开态继续渲染官方 Group/Menu；收起态的叶子继续使用官方
/// SidebarMenuItem，有子项的目录则组合官方 Button、Popover 与 PopupMenu，在图标右侧
/// 提供递归子菜单。
#[derive(Clone)]
struct WorkspaceSidebarGroup {
    group: SidebarGroup<SidebarMenu>,
    entries: Vec<NavigationTreeEntry>,
    active_target_id: &'static str,
    collapsed: bool,
}

impl WorkspaceSidebarGroup {
    fn new(
        group: SidebarGroup<SidebarMenu>,
        entries: Vec<NavigationTreeEntry>,
        active_target_id: &'static str,
        collapsed: bool,
    ) -> Self {
        Self {
            group,
            entries,
            active_target_id,
            collapsed,
        }
    }
}

impl Collapsible for WorkspaceSidebarGroup {
    fn collapsed(self, _collapsed: bool) -> Self {
        self
    }

    fn is_collapsed(&self) -> bool {
        self.collapsed
    }
}

impl SidebarItem for WorkspaceSidebarGroup {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl gpui::IntoElement {
        if !self.collapsed {
            return self
                .group
                .collapsed(false)
                .render(id, window, cx)
                .into_any_element();
        }

        let id = id.into();
        v_flex()
            .gap_2()
            .children(self.entries.into_iter().enumerate().map(|(index, entry)| {
                render_collapsed_navigation_entry(
                    entry,
                    format!("{id}-{index}"),
                    self.active_target_id,
                    window,
                    cx,
                )
            }))
            .into_any_element()
    }
}

fn navigation_entry_contains_target(entry: &NavigationTreeEntry, target_id: &str) -> bool {
    match entry {
        NavigationTreeEntry::Feature(metadata) => metadata.id() == target_id,
        NavigationTreeEntry::Group { children, .. } => children
            .iter()
            .any(|child| navigation_entry_contains_target(child, target_id)),
    }
}

fn render_collapsed_navigation_entry(
    entry: NavigationTreeEntry,
    id: impl Into<ElementId>,
    active_target_id: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match entry {
        NavigationTreeEntry::Feature(metadata) => {
            let path = metadata.path().to_owned();
            SidebarMenuItem::new(metadata.title())
                .icon(sidebar_feature_icon(metadata.icon()))
                .active(metadata.id() == active_target_id)
                .on_click(move |_, _, cx| {
                    _ = cx.navigate(path.clone());
                })
                .collapsed(true)
                .render(id, window, cx)
                .into_any_element()
        }
        NavigationTreeEntry::Group { metadata, children } => {
            let active = children
                .iter()
                .any(|child| navigation_entry_contains_target(child, active_target_id));
            workspace_icon_button(id, sidebar_feature_icon(metadata.icon()), metadata.title())
                .w_full()
                .h_9()
                .selected(active)
                .dropdown_menu_with_anchor(Anchor::RightCenter, move |menu, window, cx| {
                    populate_navigation_popup_menu(
                        menu,
                        children.clone(),
                        active_target_id,
                        window,
                        cx,
                    )
                })
                .into_any_element()
        }
    }
}

fn populate_navigation_popup_menu(
    mut menu: PopupMenu,
    entries: Vec<NavigationTreeEntry>,
    active_target_id: &'static str,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for entry in entries {
        menu = match entry {
            NavigationTreeEntry::Feature(metadata) => {
                let path = metadata.path().to_owned();
                menu.item(
                    PopupMenuItem::new(metadata.title())
                        .icon(feature_icon(metadata.icon()))
                        .checked(metadata.id() == active_target_id)
                        .on_click(move |_, _, cx| {
                            _ = cx.navigate(path.clone());
                        }),
                )
            }
            NavigationTreeEntry::Group { metadata, children } => menu.submenu_with_icon(
                Some(feature_icon(metadata.icon())),
                metadata.title(),
                window,
                cx,
                move |submenu, window, cx| {
                    populate_navigation_popup_menu(
                        submenu,
                        children.clone(),
                        active_target_id,
                        window,
                        cx,
                    )
                },
            ),
        };
    }
    menu
}

#[derive(Clone)]
struct NavigationSearchIndex {
    sections: HashMap<&'static str, SearchableTitle>,
    groups: HashMap<&'static str, SearchableTitle>,
    features: HashMap<&'static str, SearchableTitle>,
}

impl NavigationSearchIndex {
    fn new(registry: &AppRegistry) -> Self {
        let mut sections = HashMap::new();
        let mut groups = HashMap::new();
        let mut features = HashMap::new();

        for group in registry.navigation_groups() {
            groups.insert(group.id(), SearchableTitle::new(group.title()));
            sections
                .entry(group.section())
                .or_insert_with(|| SearchableTitle::new(group.section()));
        }
        for feature in registry.navigation_features() {
            features.insert(feature.id(), SearchableTitle::new(feature.title()));
            let section = registry.feature_section(feature);
            sections
                .entry(section)
                .or_insert_with(|| SearchableTitle::new(section));
        }

        Self {
            sections,
            groups,
            features,
        }
    }

    fn section_matches(&self, section: &'static str, query: &str) -> bool {
        self.sections
            .get(section)
            .is_some_and(|title| title.matches(query))
    }

    fn group_matches(&self, metadata: NavigationGroupMetadata, query: &str) -> bool {
        self.groups
            .get(metadata.id())
            .is_some_and(|title| title.matches(query))
    }

    fn feature_matches(&self, metadata: FeatureMetadata, query: &str) -> bool {
        self.features
            .get(metadata.id())
            .is_some_and(|title| title.matches(query))
    }
}

#[derive(Clone)]
struct SearchableTitle {
    normalized: String,
    pinyin: String,
    initials: String,
}

impl SearchableTitle {
    fn new(title: &str) -> Self {
        let normalized = normalize_search_text(title);
        let pinyin = title
            .chars()
            .filter_map(|character| character.to_pinyin().map(|pinyin| pinyin.plain()))
            .collect::<String>()
            .to_lowercase();
        let initials = title
            .chars()
            .filter_map(|character| character.to_pinyin().map(|pinyin| pinyin.first_letter()))
            .collect::<String>()
            .to_lowercase();

        Self {
            normalized,
            pinyin,
            initials,
        }
    }

    fn matches(&self, query: &str) -> bool {
        self.normalized.contains(query)
            || self.pinyin.contains(query)
            || self.initials.contains(query)
    }
}

fn normalize_search_text(value: &str) -> String {
    value.trim().to_lowercase()
}

fn feature_search_section(
    registry: &AppRegistry,
    account_enabled: bool,
    request: &crate::SearchRequest,
    cx: &App,
) -> SearchSection {
    let query = normalize_search_text(request.query.as_str());
    let index = NavigationSearchIndex::new(registry);
    let items = registry
        .navigation_features()
        .filter(|metadata| {
            ApplicationShell::feature_visible_for_account(account_enabled, *metadata, cx)
        })
        .filter(|metadata| query.is_empty() || index.feature_matches(*metadata, query.as_str()))
        .filter(|metadata| !metadata.path().contains('{'))
        .map(feature_search_item);
    SearchSection::new("nexora.pages", "页面").items(items)
}

fn feature_search_item(metadata: FeatureMetadata) -> SearchItem {
    let path = metadata.path().to_owned();
    let action_path = path.clone();
    SearchItem::new(
        "nexora.features",
        metadata.id(),
        metadata.title(),
        move |_, _, cx| {
            let path = action_path.clone();
            Task::ready(
                cx.navigate(path)
                    .map(|()| SearchAction::Close)
                    .map_err(|error| crate::SearchActionError::new(error.to_string(), true)),
            )
        },
    )
    .description(path)
    .icon(feature_icon(metadata.icon()))
}

pub(crate) fn record_search_history(account_id: &str, entry: SearchHistoryEntry, cx: &mut App) {
    update_shell_preferences(cx, |preferences| {
        if !preferences.search_history_enabled {
            return;
        }
        let history = preferences
            .search_history
            .entry(account_id.to_owned())
            .or_default();
        let key = entry.stable_key();
        history.retain(|current| current != &key);
        history.insert(0, key);
        history.truncate(20);
    });
}

pub(crate) fn search_history_for_account(account_id: &str, cx: &App) -> Vec<SearchHistoryEntry> {
    let preferences = shell_preferences_snapshot(cx);
    if !preferences.search_history_enabled {
        return Vec::new();
    }
    preferences
        .search_history
        .get(account_id)
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter_map(|entry| SearchHistoryEntry::from_stable_key(entry))
        .collect()
}

pub(crate) fn remove_search_history(account_id: &str, entry: &SearchHistoryEntry, cx: &mut App) {
    update_shell_preferences(cx, |preferences| {
        let Some(history) = preferences.search_history.get_mut(account_id) else {
            return;
        };
        let key = entry.stable_key();
        history.retain(|candidate| candidate != &key);
        if history.is_empty() {
            preferences.search_history.remove(account_id);
        }
    });
}

pub(crate) fn clear_search_history(account_id: &str, cx: &mut App) {
    update_shell_preferences(cx, |preferences| {
        preferences.search_history.remove(account_id);
    });
}

impl ApplicationShell {
    fn new(
        registry: AppRegistry,
        initial_route: RouteMatch,
        config: ApplicationShellConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let ApplicationShellConfig {
            application_name,
            application_logo,
            account_enabled,
            sidebar_subtitle,
            tab_style,
            sidebar_search,
            window_state,
        } = config;
        let is_main_window = window_state.role == RuntimeWindowRole::MainShell;
        let window_id = window.window_handle().window_id();
        let shell = cx.entity().downgrade();
        install_navigation_handler(
            window_id,
            is_main_window,
            move |location, cx| {
                _ = shell.update_in(cx, move |this, window, cx| {
                    this.handle_navigation_request(location, window, cx);
                });
            },
            cx,
        );
        if is_main_window {
            window.on_window_should_close(cx, |window, cx| {
                if cx
                    .try_global::<ApplicationProcessRuntime>()
                    .is_some_and(|runtime| runtime.exiting)
                {
                    return true;
                }
                if window.has_active_dialog(cx) {
                    return false;
                }
                let tray_enabled = cx
                    .try_global::<ApplicationProcessRuntime>()
                    .is_some_and(|runtime| runtime.tray_enabled);
                let tray_available = cx
                    .try_global::<ApplicationProcessRuntime>()
                    .and_then(|runtime| runtime.tray.as_ref())
                    .is_some_and(::desktop::tray::TrayController::is_available);
                let tray_unavailable_policy = cx
                    .try_global::<ApplicationProcessRuntime>()
                    .map(|runtime| runtime.tray_unavailable_policy)
                    .unwrap_or_default();
                window.open_alert_dialog(cx, move |alert, _, _cx| {
                    alert
                        .title("关闭 Nexora 应用？")
                        .description("可以立即退出全部窗口，或将窗口组最小化后继续运行。")
                        .footer(
                            DialogFooter::new()
                                .child(
                                    DialogClose::new().child(
                                        Button::new("cancel-application-close")
                                            .outline()
                                            .label("取消"),
                                    ),
                                )
                                .child(
                                    DialogClose::new().child(
                                        Button::new("minimize-application-to-tray")
                                            .outline()
                                            .disabled(!tray_enabled)
                                            .label("最小化到托盘")
                                            .on_click(move |_, window, cx| {
                                                if tray_available {
                                                    hide_coordinated_window_group(cx);
                                                } else {
                                                    match tray_unavailable_policy {
                                                        TrayUnavailablePolicy::NotifyAndMinimize => {
                                                            window.push_notification(
                                                                "当前平台托盘宿主不可用，已降级为普通最小化。",
                                                                cx,
                                                            );
                                                            minimize_coordinated_window_group(cx);
                                                        }
                                                        TrayUnavailablePolicy::KeepVisible => {}
                                                    }
                                                }
                                                cx.activate(false);
                                            }),
                                    ),
                                )
                                .child(
                                    DialogClose::new().child(
                                        Button::new("exit-application-now")
                                            .danger()
                                            .label("立即退出")
                                            .on_click(|_, _, cx| {
                                                coordinated_application_exit(cx);
                                            }),
                                    ),
                                ),
                        )
                });
                false
            });
        }
        let initial_route = ShellRoute::new(initial_route);
        let runtime_tabs = window_state
            .tabs
            .iter()
            .filter_map(|tab| match registry.resolve(tab.location.as_str()) {
                Ok(route) if route.target().kind() == RouteTargetKind::Feature => {
                    Some((ShellRoute::new(route), tab.pinned))
                }
                Ok(route) => {
                    tracing::warn!(
                        location = %tab.location,
                        target = ?route.target().kind(),
                        "运行期窗口标签不是 Feature，已跳过该标签"
                    );
                    None
                }
                Err(error) => {
                    tracing::warn!(
                        location = %tab.location,
                        error = %error,
                        "运行期窗口标签路由无效，已跳过该标签"
                    );
                    None
                }
            })
            .fold(Vec::new(), |mut tabs, tab| {
                if !tabs.iter().any(|(route, _)| route == &tab.0) {
                    tabs.push(tab);
                }
                tabs
            });
        let mut pinned_tabs = runtime_tabs
            .iter()
            .filter(|(_, pinned)| *pinned)
            .map(|(route, _)| route.clone())
            .fold(Vec::new(), |mut routes, route| {
                if !routes.contains(&route) {
                    routes.push(route);
                }
                routes
            });
        if let Some(index) = pinned_tabs.iter().position(|route| route == &initial_route) {
            pinned_tabs[index] = initial_route.clone();
        }
        let mut opened_tabs = pinned_tabs.clone();
        opened_tabs.extend(
            runtime_tabs
                .into_iter()
                .filter(|(_, pinned)| !pinned)
                .map(|(route, _)| route),
        );
        if !opened_tabs.contains(&initial_route) {
            opened_tabs.push(initial_route.clone());
        }
        let active_route = window_state
            .active_tab
            .as_deref()
            .and_then(|route_id| {
                opened_tabs
                    .iter()
                    .find(|route| route.route().stable_id() == route_id)
            })
            .cloned()
            .unwrap_or_else(|| initial_route.clone());
        #[cfg(feature = "desktop")]
        let login_feature = registry.create_login_feature(window, cx);
        #[cfg(feature = "desktop")]
        let authenticated = !account_enabled || crate::account::client::is_authenticated(cx);
        #[cfg(feature = "desktop")]
        let auth_identity = account_enabled
            .then(|| crate::account::client::login_profile(cx))
            .flatten()
            .map(|profile| profile.user.identity_id.clone());
        #[cfg(feature = "desktop")]
        let (sidebar_header, sidebar_footer) = if authenticated {
            (
                registry.create_sidebar_header(window, cx),
                registry.create_sidebar_footer(window, cx),
            )
        } else {
            (None, None)
        };
        #[cfg(not(feature = "desktop"))]
        let sidebar_header = registry.create_sidebar_header(window, cx);
        #[cfg(not(feature = "desktop"))]
        let sidebar_footer = registry.create_sidebar_footer(window, cx);
        #[cfg(feature = "desktop")]
        let (feature_instances, navigation_error) = if authenticated {
            if let crate::RouteTarget::Feature(metadata) = active_route.route().target()
                && !Self::feature_visible_for_account(account_enabled, metadata, cx)
            {
                (
                    HashMap::new(),
                    Some(NavigationError::FeatureHidden { id: metadata.id() }.to_string()),
                )
            } else {
                create_initial_feature(&registry, active_route.route().clone(), window, cx)
            }
        } else {
            (HashMap::new(), None)
        };
        #[cfg(not(feature = "desktop"))]
        let (feature_instances, navigation_error) =
            create_initial_feature(&registry, active_route.route().clone(), window, cx);
        #[cfg(feature = "desktop")]
        let _authentication_subscription = account_enabled.then(|| {
            crate::account::client::observe_authentication_in(window, cx, |this, window, cx| {
                this.authentication_changed(window, cx);
            })
        });
        let _window_bounds_subscription = is_main_window.then(|| {
            cx.observe_window_bounds(window, |this, window, cx| {
                this.schedule_main_window_placement_persist(window, cx);
            })
        });
        let _release_subscription = Some(cx.on_release_in(window, move |this, window, cx| {
            this.main_window_persist_task = None;
            if is_main_window {
                this.persist_current_main_window_placement(window, cx);
            }
            if let Some(runtime) = cx.try_global::<ShellPreferencesRuntime>() {
                runtime.flush();
            }
            remove_navigation_handler(window_id, cx);
            for (_, mut instance) in this.feature_instances.drain() {
                instance.close(window, cx);
            }
            #[cfg(feature = "desktop")]
            this.close_business_windows(cx);
        }));
        let sidebar_search_input = sidebar_search
            .then(|| cx.new(|cx| InputState::new(window, cx).placeholder("搜索导航")));
        let _sidebar_search_subscription = sidebar_search_input.as_ref().map(|input| {
            cx.subscribe(input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
        });
        let sidebar_search_index = sidebar_search.then(|| NavigationSearchIndex::new(&registry));
        let sidebar_collapsed = shell_preferences_snapshot(cx).sidebar_collapsed;

        let expanded_navigation_groups = registry
            .navigation_group_ancestors(initial_route.route().target().id())
            .into_iter()
            .map(NavigationGroupMetadata::id)
            .collect();

        let focus_handle = cx.focus_handle();
        if window.focused(cx).is_none() {
            window.focus(&focus_handle, cx);
        }

        Self {
            focus_handle,
            registry,
            application_name,
            application_logo,
            account_enabled,
            sidebar_subtitle,
            tab_style,
            sidebar_search_input,
            sidebar_search_index,
            initial_route: initial_route.clone(),
            active_route: active_route.clone(),
            opened_tabs,
            pinned_tabs,
            tab_context_route: None,
            tab_scroll_handle: ScrollHandle::new(),
            reload_tasks: HashMap::new(),
            sidebar_collapsed,
            sidebar_focus_task: None,
            navigation_history: vec![active_route],
            navigation_history_index: 0,
            expanded_navigation_groups,
            main_window_persist_task: None,
            feature_instances,
            #[cfg(feature = "desktop")]
            login_feature,
            #[cfg(feature = "desktop")]
            authenticated,
            #[cfg(feature = "desktop")]
            auth_identity,
            #[cfg(feature = "desktop")]
            business_windows: Vec::new(),
            sidebar_header,
            sidebar_footer,
            navigation_error,
            #[cfg(feature = "desktop")]
            _authentication_subscription,
            _window_bounds_subscription,
            _sidebar_search_subscription,
            _release_subscription,
        }
    }

    #[cfg(feature = "desktop")]
    fn authentication_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.account_enabled {
            return;
        }
        let authenticated = crate::account::client::is_authenticated(cx);
        let auth_identity = crate::account::client::login_profile(cx)
            .map(|profile| profile.user.identity_id.clone());
        if authenticated == self.authenticated && auth_identity == self.auth_identity {
            cx.notify();
            return;
        }

        let identity_changed =
            authenticated && self.authenticated && auth_identity != self.auth_identity;
        self.authenticated = authenticated;
        self.auth_identity = auth_identity;
        if authenticated {
            if identity_changed {
                self.reload_tasks.clear();
                for (_, mut instance) in self.feature_instances.drain() {
                    instance.close(window, cx);
                }
                self.close_business_windows(cx);
            }
            self.sidebar_header = self.registry.create_sidebar_header(window, cx);
            self.sidebar_footer = self.registry.create_sidebar_footer(window, cx);
            self.activate_selected_feature(window, cx);
        } else {
            self.reload_tasks.clear();
            for (_, mut instance) in self.feature_instances.drain() {
                instance.close(window, cx);
            }
            self.close_business_windows(cx);
            self.sidebar_header = None;
            self.sidebar_footer = None;
            self.navigation_error = None;
        }
        cx.notify();
    }

    #[cfg(feature = "desktop")]
    fn close_business_windows(&mut self, cx: &mut App) {
        for handle in self.business_windows.drain(..) {
            _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    #[cfg(feature = "desktop")]
    fn activate_selected_feature(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active_route = self.active_route.clone();
        if let crate::RouteTarget::Feature(metadata) = active_route.route().target()
            && !self.feature_visible(metadata, cx)
        {
            self.navigation_error =
                Some(NavigationError::FeatureHidden { id: metadata.id() }.to_string());
            return;
        }
        match self.ensure_feature_instance(&active_route, window, cx) {
            Ok(()) => {
                self.feature_instances
                    .get_mut(active_route.identity())
                    .expect("当前 Feature 应当已进入运行时缓存")
                    .activate(window, cx);
                self.navigation_error = None;
            }
            Err(error) => self.navigation_error = Some(error.to_string()),
        }
    }

    fn active_key(&self) -> &str {
        self.active_route.identity()
    }

    fn ensure_feature_instance(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if self.feature_instances.contains_key(route.identity()) {
            return Ok(());
        }

        let instance = self
            .registry
            .create_feature(route.route().clone(), window, cx)?;
        self.feature_instances
            .insert(route.identity().to_owned(), instance);
        Ok(())
    }

    fn close_feature_instance(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_tasks.remove(path);
        let Some(mut instance) = self.feature_instances.remove(path) else {
            return;
        };
        instance.close(window, cx);
    }

    fn close_orphaned_feature_instances(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let removed_paths = self
            .feature_instances
            .keys()
            .filter(|key| {
                !self
                    .opened_tabs
                    .iter()
                    .any(|route| route.identity() == *key)
            })
            .cloned()
            .collect::<Vec<_>>();

        for path in removed_paths {
            self.close_feature_instance(path.as_str(), window, cx);
        }
    }

    fn synchronize_feature_runtime(
        &mut self,
        previous_active_key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        let active_route = self.active_route.clone();
        self.ensure_feature_instance(&active_route, window, cx)?;
        self.close_orphaned_feature_instances(window, cx);

        if previous_active_key != active_route.identity()
            && let Some(previous) = self.feature_instances.get_mut(previous_active_key)
        {
            previous.deactivate(window, cx);
        }
        self.feature_instances
            .get_mut(active_route.identity())
            .expect("当前 Feature 应当已进入运行时缓存")
            .activate(window, cx);
        Ok(())
    }

    fn navigate_to_route_in(
        &mut self,
        route: ShellRoute,
        record_history: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        self.ensure_feature_instance(&route, window, cx)?;
        let previous_active_key = self.active_key().to_owned();
        self.navigate_to_route(route, record_history);
        self.synchronize_feature_runtime(previous_active_key.as_str(), window, cx)?;
        Ok(())
    }

    fn navigate_to_route(&mut self, route: ShellRoute, record_history: bool) {
        let same_instance = self.active_route == route;
        self.open_feature_tab(route.clone());

        if same_instance {
            self.active_route = route.clone();
            if let Some(current) = self
                .navigation_history
                .get_mut(self.navigation_history_index)
                && *current == route
            {
                *current = route.clone();
            }
            self.scroll_tab_into_view(&route);
            self.expand_active_navigation_groups();
            return;
        }

        self.active_route = route.clone();
        if record_history {
            self.push_navigation_history(route.clone());
        }
        self.scroll_tab_into_view(&route);
        self.expand_active_navigation_groups();
    }

    fn push_navigation_history(&mut self, route: ShellRoute) {
        if self.navigation_history.get(self.navigation_history_index) == Some(&route) {
            return;
        }

        self.navigation_history
            .truncate(self.navigation_history_index + 1);
        self.navigation_history.push(route);
        self.navigation_history_index = self.navigation_history.len().saturating_sub(1);
    }

    fn can_navigate_back(&self) -> bool {
        self.navigation_history_index > 0
    }

    fn can_navigate_forward(&self) -> bool {
        self.navigation_history_index + 1 < self.navigation_history.len()
    }

    fn navigate_back_in(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if !self.can_navigate_back() {
            return Ok(());
        }

        let target_index = self.navigation_history_index - 1;
        let route = self.navigation_history[target_index].clone();
        self.ensure_feature_instance(&route, window, cx)?;
        self.navigation_history_index = target_index;
        self.navigate_to_route_in(route, false, window, cx)
    }

    fn navigate_forward_in(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if !self.can_navigate_forward() {
            return Ok(());
        }

        let target_index = self.navigation_history_index + 1;
        let route = self.navigation_history[target_index].clone();
        self.ensure_feature_instance(&route, window, cx)?;
        self.navigation_history_index = target_index;
        self.navigate_to_route_in(route, false, window, cx)
    }

    fn handle_navigation_request(
        &mut self,
        location: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.open_path(location.as_str(), window, cx) {
            self.navigation_error = Some(error.to_string());
            cx.notify();
        }
    }

    fn open_path(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NavigationError> {
        let route = self.registry.resolve(path)?;
        self.open_route(route, window, cx)
    }

    fn open_route(
        &mut self,
        route: RouteMatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), NavigationError> {
        if route.target().kind() == RouteTargetKind::Window {
            #[cfg(feature = "desktop")]
            if self.account_enabled && !self.authenticated && route.target().id() != "settings" {
                return Err(NavigationError::AuthenticationRequired {
                    path: route.concrete_path().to_owned(),
                });
            }
            #[cfg(feature = "desktop")]
            {
                let display_uuid = window
                    .display(cx)
                    .and_then(|display| display.uuid().ok())
                    .map(|uuid| uuid.to_string());
                let is_settings = route.target().id() == "settings";
                let handle =
                    self.registry
                        .open_window_on_display(route, display_uuid.as_deref(), cx)?;
                if !is_settings {
                    self.business_windows.push(handle);
                }
            }
            #[cfg(not(feature = "desktop"))]
            self.registry.open_window(route, cx)?;
            self.navigation_error = None;
            cx.notify();
            return Ok(());
        }

        if let crate::RouteTarget::Feature(metadata) = route.target()
            && !self.feature_visible(metadata, cx)
        {
            return Err(NavigationError::FeatureHidden { id: metadata.id() });
        }
        let route = ShellRoute::new(route);
        #[cfg(feature = "desktop")]
        if self.account_enabled && !self.authenticated {
            self.navigate_to_route(route, true);
            self.navigation_error = None;
            cx.notify();
            return Ok(());
        }

        self.navigate_to_route_in(route, true, window, cx)?;
        self.navigation_error = None;
        cx.notify();
        Ok(())
    }

    fn open_feature_tab(&mut self, route: ShellRoute) {
        if let Some(index) = self.tab_index(&route) {
            self.opened_tabs[index] = route.clone();
            if let Some(index) = self.pinned_tabs.iter().position(|pinned| pinned == &route) {
                self.pinned_tabs[index] = route;
            }
            return;
        }

        self.opened_tabs.push(route);
        self.reorder_tabs_by_pin();
    }

    fn tab_index(&self, route: &ShellRoute) -> Option<usize> {
        self.opened_tabs.iter().position(|opened| opened == route)
    }

    fn scroll_tab_into_view(&self, route: &ShellRoute) {
        if let Some(index) = self.tab_index(route) {
            self.tab_scroll_handle.scroll_to_item(index);
        }
    }

    fn is_route_pinned(&self, route: &ShellRoute) -> bool {
        self.pinned_tabs.contains(route)
    }

    fn toggle_pin_route(&mut self, route: &ShellRoute) {
        if self.is_route_pinned(route) {
            self.pinned_tabs.retain(|pinned| pinned != route);
        } else {
            self.pinned_tabs.push(route.clone());
        }

        self.reorder_tabs_by_pin();
        self.scroll_tab_into_view(&self.active_route);
    }

    fn move_tab_within_partition(&mut self, source_route_id: &str, target_index: usize) {
        let Some(source_index) = self
            .opened_tabs
            .iter()
            .position(|route| route.identity() == source_route_id)
        else {
            return;
        };
        if target_index >= self.opened_tabs.len() {
            return;
        }
        let mut session = RuntimeWindowState {
            tabs: self
                .opened_tabs
                .iter()
                .map(|route| RuntimeWindowTab {
                    route_id: route.identity().to_owned(),
                    location: route.location().to_owned(),
                    pinned: self.is_route_pinned(route),
                })
                .collect(),
            ..RuntimeWindowState::default()
        };
        if !session.move_tab_within_partition(source_index, target_index) {
            return;
        }
        let mut routes = std::mem::take(&mut self.opened_tabs)
            .into_iter()
            .map(|route| (route.identity().to_owned(), route))
            .collect::<HashMap<_, _>>();
        self.opened_tabs = session
            .tabs
            .into_iter()
            .filter_map(|tab| routes.remove(&tab.route_id))
            .collect();
        self.pinned_tabs = self
            .opened_tabs
            .iter()
            .filter(|route| self.is_route_pinned(route))
            .cloned()
            .collect();
        self.scroll_tab_into_view(&self.active_route);
    }

    fn open_route_in_new_window(
        &self,
        route: &ShellRoute,
        window: &Window,
        cx: &mut App,
    ) -> Result<(), NavigationError> {
        let display_uuid = window
            .display(cx)
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string());
        let window_state = RuntimeWindowState {
            role: RuntimeWindowRole::Shell,
            tabs: vec![RuntimeWindowTab {
                route_id: route.identity().to_owned(),
                location: route.location().to_owned(),
                pinned: false,
            }],
            active_tab: Some(route.identity().to_owned()),
            display_uuid: display_uuid.clone(),
        };
        open_in_process_shell_session(route.route().clone(), window_state, cx)?;
        Ok(())
    }

    fn schedule_main_window_placement_persist(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.main_window_persist_task.is_some() {
            return;
        }

        self.main_window_persist_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(MAIN_WINDOW_BOUNDS_SAVE_DELAY)
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                this.main_window_persist_task = None;
                this.persist_current_main_window_placement(window, cx);
            });
        }));
    }

    fn persist_current_main_window_placement(&self, window: &mut Window, cx: &mut App) {
        let Some(current) = capture_main_window_placement(window, cx) else {
            return;
        };
        let preferences = shell_preferences_snapshot(cx);
        if !should_update_main_window_placement(&current, &preferences, cx) {
            return;
        }

        update_shell_preferences(cx, |preferences| {
            preferences.main_window = Some(current);
        });
    }

    fn reorder_tabs_by_pin(&mut self) {
        // 置顶列表只决定路径顺序；具体 route 始终取 opened_tabs 中的最新 query 快照。
        let mut pinned = self
            .pinned_tabs
            .iter()
            .filter_map(|pinned| {
                self.opened_tabs
                    .iter()
                    .find(|opened| *opened == pinned)
                    .cloned()
            })
            .fold(Vec::new(), |mut routes, route| {
                if !routes.contains(&route) {
                    routes.push(route);
                }
                routes
            });
        let mut regular = self
            .opened_tabs
            .iter()
            .filter(|route| !pinned.contains(route))
            .cloned()
            .collect::<Vec<_>>();

        self.pinned_tabs = pinned.clone();
        pinned.append(&mut regular);
        self.opened_tabs = pinned;
    }

    fn ensure_active_tab(&mut self) {
        if self.opened_tabs.is_empty() {
            self.opened_tabs.push(self.initial_route.clone());
        }
        self.active_route = self
            .opened_tabs
            .iter()
            .find(|opened| *opened == &self.active_route)
            .cloned()
            .unwrap_or_else(|| self.opened_tabs[0].clone());

        self.pinned_tabs
            .retain(|route| self.opened_tabs.contains(route));
        self.scroll_tab_into_view(&self.active_route);
    }

    fn ensure_active_or_select(&mut self, fallback: ShellRoute) {
        if !self.opened_tabs.contains(&self.active_route) {
            self.active_route = fallback;
        }
        self.ensure_active_tab();
    }

    fn close_tab_route(&mut self, route: &ShellRoute) {
        let Some(index) = self.tab_index(route) else {
            return;
        };
        let closing_active = &self.active_route == route;
        self.opened_tabs.remove(index);
        self.pinned_tabs.retain(|pinned| pinned != route);

        if self.opened_tabs.is_empty() {
            self.opened_tabs.push(self.initial_route.clone());
        }
        if closing_active {
            let fallback_index = index.min(self.opened_tabs.len().saturating_sub(1));
            if let Some(route) = self.opened_tabs.get(fallback_index).cloned() {
                self.active_route = route;
            }
        }
        self.ensure_active_tab();
    }

    fn close_tabs_to_left(&mut self, route: &ShellRoute) {
        let Some(index) = self.tab_index(route) else {
            return;
        };
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index >= index || opened == route || self.is_route_pinned(opened))
                    .then_some(opened.clone())
            })
            .collect();
        self.ensure_active_or_select(route.clone());
    }

    fn close_tabs_to_right(&mut self, route: &ShellRoute) {
        let Some(index) = self.tab_index(route) else {
            return;
        };
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .enumerate()
            .filter_map(|(tab_index, opened)| {
                (tab_index <= index || opened == route || self.is_route_pinned(opened))
                    .then_some(opened.clone())
            })
            .collect();
        self.ensure_active_or_select(route.clone());
    }

    fn close_other_tabs(&mut self, route: &ShellRoute) {
        self.opened_tabs = self
            .opened_tabs
            .iter()
            .filter(|opened| *opened == route || self.is_route_pinned(opened))
            .cloned()
            .collect();
        self.ensure_active_or_select(route.clone());
        self.reorder_tabs_by_pin();
    }

    fn update_runtime_after_tab_change(
        &mut self,
        previous_active_route: ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if previous_active_route.identity() != self.active_key() {
            self.push_navigation_history(self.active_route.clone());
        } else if previous_active_route.route() != self.active_route.route() {
            if let Some(current) = self
                .navigation_history
                .get_mut(self.navigation_history_index)
                && current == &self.active_route
            {
                *current = self.active_route.clone();
            } else {
                self.push_navigation_history(self.active_route.clone());
            }
        }
        match self.synchronize_feature_runtime(previous_active_route.identity(), window, cx) {
            Ok(()) => self.navigation_error = None,
            Err(error) => self.navigation_error = Some(error.to_string()),
        }
        self.expand_active_navigation_groups();
        cx.notify();
    }

    fn close_tab_route_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_tab_route(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn close_tabs_to_left_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_tabs_to_left(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn close_tabs_to_right_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_tabs_to_right(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn close_other_tabs_in(
        &mut self,
        route: &ShellRoute,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_active_route = self.active_route.clone();
        self.close_other_tabs(route);
        self.update_runtime_after_tab_change(previous_active_route, window, cx);
    }

    fn select_tab_in(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), FeatureRuntimeError> {
        if let Some(route) = self.opened_tabs.get(index).cloned() {
            self.navigate_to_route_in(route, true, window, cx)?;
        }
        Ok(())
    }

    fn render_navigation_feature(
        &self,
        metadata: FeatureMetadata,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        let path = metadata.path();
        SidebarMenuItem::new(metadata.title())
            .icon(sidebar_feature_icon(metadata.icon()))
            .active(self.active_target_id() == metadata.id())
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Err(error) = this.open_path(path, window, cx) {
                    this.navigation_error = Some(error.to_string());
                }
                cx.notify();
            }))
    }

    fn active_target_id(&self) -> &'static str {
        self.active_route.route().target().id()
    }

    fn expand_active_navigation_groups(&mut self) {
        for group in self
            .registry
            .navigation_group_ancestors(self.active_target_id())
        {
            self.expanded_navigation_groups.insert(group.id());
        }
    }

    fn toggle_navigation_group(&mut self, group_id: &'static str, cx: &mut Context<Self>) {
        if !self.expanded_navigation_groups.remove(group_id) {
            self.expanded_navigation_groups.insert(group_id);
        }
        cx.notify();
    }

    fn render_navigation_group(
        &self,
        metadata: NavigationGroupMetadata,
        children: &[NavigationTreeEntry],
        search_expanded_groups: Option<&HashSet<&'static str>>,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        let group_id = metadata.id();
        let expanded = search_expanded_groups.map_or_else(
            || self.expanded_navigation_groups.contains(group_id),
            |expanded_groups| expanded_groups.contains(group_id),
        );
        let children = children
            .iter()
            .map(|entry| self.render_navigation_entry(entry, search_expanded_groups, cx))
            .collect::<Vec<_>>();

        let item = SidebarMenuItem::new(metadata.title())
            .icon(sidebar_feature_icon(metadata.icon()))
            .default_open(expanded)
            .click_to_toggle(true)
            .children(children);

        if search_expanded_groups.is_some() {
            item
        } else {
            item.on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_navigation_group(group_id, cx);
            }))
        }
    }

    fn render_navigation_entry(
        &self,
        entry: &NavigationTreeEntry,
        search_expanded_groups: Option<&HashSet<&'static str>>,
        cx: &mut Context<Self>,
    ) -> SidebarMenuItem {
        match entry {
            NavigationTreeEntry::Group { metadata, children } => {
                self.render_navigation_group(*metadata, children, search_expanded_groups, cx)
            }
            NavigationTreeEntry::Feature(metadata) => self.render_navigation_feature(*metadata, cx),
        }
    }

    fn navigation_children(
        &self,
        parent: Option<&str>,
        section: &'static str,
        cx: &App,
    ) -> Vec<NavigationEntry> {
        let mut entries = self
            .registry
            .navigation_groups()
            .iter()
            .copied()
            .filter(|group| group.parent() == parent && group.section() == section)
            .filter(|group| self.navigation_group_visible(*group, cx))
            .map(NavigationEntry::Group)
            .chain(
                self.registry
                    .navigation_features()
                    .filter(|feature| {
                        feature.group() == parent
                            && self.registry.feature_section(*feature) == section
                            && self.feature_visible(*feature, cx)
                    })
                    .map(NavigationEntry::Feature),
            )
            .collect::<Vec<_>>();
        entries.sort_by_key(NavigationEntry::sort_key);
        entries
    }

    fn navigation_sections(&self, cx: &App) -> Vec<(&'static str, Vec<NavigationEntry>)> {
        let mut sections = Vec::<(&'static str, Vec<NavigationEntry>)>::new();
        let mut roots = self
            .registry
            .navigation_groups()
            .iter()
            .copied()
            .filter(|group| group.parent().is_none())
            .filter(|group| self.navigation_group_visible(*group, cx))
            .map(NavigationEntry::Group)
            .chain(
                self.registry
                    .navigation_features()
                    .filter(|feature| feature.group().is_none())
                    .filter(|feature| self.feature_visible(*feature, cx))
                    .map(NavigationEntry::Feature),
            )
            .collect::<Vec<_>>();
        roots.sort_by_key(NavigationEntry::sort_key);
        for entry in roots {
            let section = entry.section(&self.registry);
            if let Some((_, items)) = sections
                .iter_mut()
                .find(|(existing, _)| *existing == section)
            {
                items.push(entry);
            } else {
                sections.push((section, vec![entry]));
            }
        }
        sections
    }

    fn navigation_tree_children(
        &self,
        parent: Option<&str>,
        section: &'static str,
        cx: &App,
    ) -> Vec<NavigationTreeEntry> {
        self.navigation_children(parent, section, cx)
            .into_iter()
            .map(|entry| match entry {
                NavigationEntry::Group(metadata) => NavigationTreeEntry::Group {
                    metadata,
                    children: self.navigation_tree_children(Some(metadata.id()), section, cx),
                },
                NavigationEntry::Feature(metadata) => NavigationTreeEntry::Feature(metadata),
            })
            .collect()
    }

    fn navigation_tree_sections(&self, cx: &App) -> Vec<(&'static str, Vec<NavigationTreeEntry>)> {
        let mut sections = Vec::<(&'static str, Vec<NavigationTreeEntry>)>::new();
        for (section, roots) in self.navigation_sections(cx) {
            let entries = roots
                .into_iter()
                .map(|entry| match entry {
                    NavigationEntry::Group(metadata) => NavigationTreeEntry::Group {
                        metadata,
                        children: self.navigation_tree_children(Some(metadata.id()), section, cx),
                    },
                    NavigationEntry::Feature(metadata) => NavigationTreeEntry::Feature(metadata),
                })
                .collect::<Vec<_>>();
            sections.push((section, entries));
        }
        sections
    }

    fn filtered_navigation_sections(&self, cx: &App) -> NavigationSearchResult {
        let sections = self.navigation_tree_sections(cx);
        let Some(query) = self.sidebar_search_query(cx) else {
            return NavigationSearchResult {
                sections,
                expanded_groups: HashSet::new(),
            };
        };
        let Some(index) = self.sidebar_search_index.as_ref() else {
            return NavigationSearchResult {
                sections,
                expanded_groups: HashSet::new(),
            };
        };

        let mut expanded_groups = HashSet::new();
        let sections = sections
            .into_iter()
            .filter_map(|(section, entries)| {
                if index.section_matches(section, query.as_str()) {
                    for entry in &entries {
                        entry.collect_group_ids(&mut expanded_groups);
                    }
                    return Some((section, entries));
                }

                let filtered_entries = entries
                    .into_iter()
                    .filter_map(|entry| {
                        Self::filter_navigation_entry(
                            entry,
                            index,
                            query.as_str(),
                            &mut expanded_groups,
                        )
                    })
                    .collect::<Vec<_>>();
                (!filtered_entries.is_empty()).then_some((section, filtered_entries))
            })
            .collect();

        NavigationSearchResult {
            sections,
            expanded_groups,
        }
    }

    fn filter_navigation_entry(
        entry: NavigationTreeEntry,
        index: &NavigationSearchIndex,
        query: &str,
        expanded_groups: &mut HashSet<&'static str>,
    ) -> Option<NavigationTreeEntry> {
        match entry {
            NavigationTreeEntry::Feature(metadata) => index
                .feature_matches(metadata, query)
                .then_some(NavigationTreeEntry::Feature(metadata)),
            NavigationTreeEntry::Group { metadata, children } => {
                if index.group_matches(metadata, query) {
                    let entry = NavigationTreeEntry::Group { metadata, children };
                    entry.collect_group_ids(expanded_groups);
                    return Some(entry);
                }

                let filtered_children = children
                    .into_iter()
                    .filter_map(|child| {
                        Self::filter_navigation_entry(child, index, query, expanded_groups)
                    })
                    .collect::<Vec<_>>();
                if filtered_children.is_empty() {
                    None
                } else {
                    expanded_groups.insert(metadata.id());
                    Some(NavigationTreeEntry::Group {
                        metadata,
                        children: filtered_children,
                    })
                }
            }
        }
    }

    fn sidebar_search_query(&self, cx: &App) -> Option<String> {
        let input = self.sidebar_search_input.as_ref()?;
        let query = input.read(cx).value();
        let query = normalize_search_text(query.as_ref());
        (!query.is_empty()).then_some(query)
    }

    fn navigation_group_visible(&self, metadata: NavigationGroupMetadata, cx: &App) -> bool {
        !self
            .navigation_children(Some(metadata.id()), metadata.section(), cx)
            .is_empty()
    }

    fn feature_visible(&self, metadata: FeatureMetadata, cx: &App) -> bool {
        Self::feature_visible_for_account(self.account_enabled, metadata, cx)
    }

    fn feature_visible_for_account(
        account_enabled: bool,
        metadata: FeatureMetadata,
        cx: &App,
    ) -> bool {
        if !account_enabled {
            return true;
        }

        #[cfg(feature = "desktop")]
        {
            metadata
                .visible_permissions()
                .allows_profile(crate::account::client::login_profile(cx))
        }

        #[cfg(not(feature = "desktop"))]
        {
            false
        }
    }

    fn render_default_sidebar_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .child(
                img(self
                    .application_logo
                    .map(ApplicationLogo::image)
                    .unwrap_or_else(ui::default_application_logo))
                .size_7()
                .flex_shrink_0(),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(theme.sidebar_accent_foreground)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .truncate()
                            .child(self.application_name.clone()),
                    )
                    .when_some(self.sidebar_subtitle.clone(), |this, subtitle| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.sidebar_foreground.opacity(0.66))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .truncate()
                                .child(subtitle),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_sidebar_header_content(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(sidebar_header) = self.sidebar_header.as_ref() {
            return sidebar_header.clone().into_any_element();
        }

        SidebarRegion::new("nexora-sidebar-brand")
            .py_2()
            .child(self.render_default_sidebar_header(cx))
            .into_any_element()
    }

    fn render_sidebar_search(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.sidebar_search_input.as_ref().map(|input| {
            Input::new(input)
                .prefix(Icon::new(IconName::Search).with_size(WORKSPACE_SHELL_ICON_SIZE))
                .with_size(theme::component_size(cx))
                .into_any_element()
        })
    }

    #[cfg(feature = "desktop")]
    fn render_default_account_footer(&self, collapsed: bool, cx: &mut Context<Self>) -> AnyElement {
        let profile = crate::account::client::login_profile(cx);
        let display_name = profile
            .map(|profile| profile.user.display_name.clone())
            .unwrap_or_else(|| "当前账户".to_owned());
        let avatar = Avatar::new().name(display_name.clone()).small();
        let menu_items =
            account_actions::menu_actions_with_updates(crate::desktop::updater_available(cx));
        let action_context = cx.focus_handle();
        let content = if collapsed {
            h_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(avatar)
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(avatar)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .truncate()
                        .child(display_name),
                )
                .into_any_element()
        };

        SidebarRegion::new("nexora-default-account-footer")
            .when(collapsed, |this| this.size_8())
            .when(!collapsed, |this| this.w_full().py_2())
            .rounded(cx.theme().radius)
            .hover(|this| {
                this.bg(cx.theme().tokens.sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .child(content)
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                menu_items.iter().cloned().fold(
                    menu.action_context(action_context.clone()).min_w(220.0),
                    |menu, item| {
                        let menu_item =
                            PopupMenuItem::new(item.label()).icon(account_icon(item.kind()));
                        let menu_item = match item.kind() {
                            AccountActionKind::SignIn => menu_item.on_click(|_, _, cx| {
                                _ = crate::account::client::start_login(cx);
                            }),
                            AccountActionKind::SignOut => menu_item.on_click(|_, _, cx| {
                                crate::account::client::sign_out(cx);
                            }),
                            AccountActionKind::Settings => menu_item.action(item.to_action()),
                            AccountActionKind::Updates => menu_item.action(item.to_action()),
                        };
                        menu.item(menu_item)
                    },
                )
            })
            .into_any_element()
    }

    fn set_sidebar_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.sidebar_collapsed == collapsed {
            return;
        }
        self.sidebar_collapsed = collapsed;
        update_shell_preferences(cx, |preferences| {
            preferences.sidebar_collapsed = collapsed;
        });
        cx.notify();
    }

    fn expand_sidebar_and_focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_collapsed(false, cx);
        let Some(input) = self.sidebar_search_input.clone() else {
            return;
        };
        let timer = cx.background_executor().timer(Duration::from_millis(220));
        self.sidebar_focus_task = Some(cx.spawn_in(window, async move |this, cx| {
            timer.await;
            _ = this.update_in(cx, |this, window, cx| {
                input.read(cx).focus_handle(cx).focus(window, cx);
                this.sidebar_focus_task = None;
            });
        }));
    }

    fn render_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let sidebar_border = cx.theme().sidebar_border;
        let reserves_macos_traffic_lights = cfg!(target_os = "macos") && !window.is_fullscreen();
        let search_active = self.sidebar_search_query(cx).is_some();
        let NavigationSearchResult {
            sections,
            expanded_groups,
        } = self.filtered_navigation_sections(cx);
        let search_expanded_groups = search_active.then_some(&expanded_groups);
        let has_navigation_results = !sections.is_empty();
        let navigation_groups = sections
            .into_iter()
            .map(|(section, items)| {
                let group = SidebarGroup::new(section).child(SidebarMenu::new().children(
                    items.iter().map(|entry| {
                        self.render_navigation_entry(entry, search_expanded_groups, cx)
                    }),
                ));
                WorkspaceSidebarGroup::new(
                    group,
                    items,
                    self.active_target_id(),
                    self.sidebar_collapsed,
                )
            })
            .collect::<Vec<_>>();
        let header = if self.sidebar_collapsed {
            v_flex()
                .w_full()
                .items_center()
                .gap_2()
                .when(reserves_macos_traffic_lights, |this| this.pt_5())
                .pb_3()
                .border_b_1()
                .border_color(sidebar_border)
                .child(
                    workspace_icon_button("expand-sidebar", IconName::PanelLeftOpen, "展开侧边栏")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_sidebar_collapsed(false, cx);
                        })),
                )
                .when(self.sidebar_search_input.is_some(), |this| {
                    this.child(
                        workspace_icon_button(
                            "expand-sidebar-search",
                            IconName::Search,
                            "展开并搜索导航",
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.expand_sidebar_and_focus_search(window, cx);
                        })),
                    )
                })
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .gap_2()
                .when(reserves_macos_traffic_lights, |this| this.pt_5())
                .px_2()
                .pb_3()
                .border_b_1()
                .border_color(sidebar_border)
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.render_sidebar_header_content(cx)),
                        )
                        .child(
                            workspace_icon_button(
                                "collapse-sidebar",
                                IconName::PanelLeftClose,
                                "收起侧边栏",
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_sidebar_collapsed(true, cx);
                            })),
                        ),
                )
                .children(self.render_sidebar_search(cx))
                .into_any_element()
        };
        let empty_navigation = (search_active && !has_navigation_results).then(|| {
            WorkspaceSidebarGroup::new(
                SidebarGroup::new("").child(
                    SidebarMenu::new()
                        .child(SidebarMenuItem::new("未找到匹配的导航").disable(true)),
                ),
                Vec::new(),
                self.active_target_id(),
                self.sidebar_collapsed,
            )
        });

        let footer = if let Some(footer) = self.sidebar_footer.as_ref() {
            Some(workspace_sidebar_footer_host(
                footer.clone().into_any_element(),
                self.sidebar_collapsed,
                sidebar_border,
            ))
        } else {
            #[cfg(feature = "desktop")]
            if self.account_enabled {
                Some(workspace_sidebar_footer_host(
                    self.render_default_account_footer(self.sidebar_collapsed, cx),
                    self.sidebar_collapsed,
                    sidebar_border,
                ))
            } else {
                None
            }
            #[cfg(not(feature = "desktop"))]
            None
        };

        let sidebar_width = if self.sidebar_collapsed {
            WORKSPACE_SIDEBAR_COLLAPSED_WIDTH
        } else {
            WORKSPACE_SIDEBAR_EXPANDED_WIDTH
        };
        Sidebar::new("nexora-sidebar")
            .h_full()
            .w(sidebar_width)
            // 工作区导航框架统一绘制外侧边框，并与官方 Sidebar 使用同一真实宽度。
            .border_r_0()
            .collapsible(SidebarCollapsible::None)
            .header(header)
            .children(navigation_groups)
            .children(empty_navigation)
            .when_some(footer, |this, footer| this.footer(footer))
            .into_any_element()
    }

    fn render_tab(
        route: ShellRoute,
        index: usize,
        is_pinned: bool,
        shell: WeakEntity<Self>,
    ) -> Tab {
        let action_shell = shell.clone();
        let context_shell = shell.clone();
        let action_route = route.clone();
        let action = if is_pinned {
            Toggle::new(format!("pin-tab-{}", route.identity()))
                .xsmall()
                .checked(true)
                .icon(IconName::StarFill)
                .tooltip("取消置顶")
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    _ = action_shell.update(cx, |this, cx| {
                        this.toggle_pin_route(&action_route);
                        cx.notify();
                    });
                })
                .into_any_element()
        } else {
            Button::new(format!("close-tab-{}", route.identity()))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("关闭标签")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    _ = action_shell.update(cx, |this, cx| {
                        this.close_tab_route_in(&action_route, window, cx);
                    });
                })
                .into_any_element()
        };

        let drag_route_id = route.identity().to_owned();
        let drag_title = route.title();
        Tab::new()
            .px_1()
            .prefix(feature_icon(route.icon()))
            .label(route.title())
            .suffix(h_flex().gap_1().child(action))
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                _ = context_shell.update(cx, |this, _| {
                    this.tab_context_route = Some(route.clone());
                });
            })
            .on_drag(
                DraggedShellTab {
                    route_id: drag_route_id,
                    title: drag_title,
                },
                |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                },
            )
            .drag_over::<DraggedShellTab>(|this, _, _, cx| {
                this.border_l_2().border_color(cx.theme().drag_border)
            })
            .on_drop(move |drag: &DraggedShellTab, _, cx| {
                _ = shell.update(cx, |this, cx| {
                    this.move_tab_within_partition(drag.route_id.as_str(), index);
                    cx.notify();
                });
            })
    }

    fn build_tab_context_menu(
        menu: PopupMenu,
        route: ShellRoute,
        shell: WeakEntity<Self>,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let Some(shell_entity) = shell.upgrade() else {
            return menu;
        };
        let Some((pinned, can_close_left, can_close_right, can_close_other)) = ({
            let shell = shell_entity.read(cx);
            let Some(index) = shell.tab_index(&route) else {
                return menu;
            };
            let can_close_left = shell
                .opened_tabs
                .iter()
                .take(index)
                .any(|opened| !shell.is_route_pinned(opened));
            let can_close_right = shell
                .opened_tabs
                .iter()
                .skip(index + 1)
                .any(|opened| !shell.is_route_pinned(opened));
            let can_close_other = shell
                .opened_tabs
                .iter()
                .any(|opened| opened != &route && !shell.is_route_pinned(opened));
            Some((
                shell.is_route_pinned(&route),
                can_close_left,
                can_close_right,
                can_close_other,
            ))
        }) else {
            return menu;
        };

        menu.min_w(220.0)
            .item(
                PopupMenuItem::new("使用新窗口打开")
                    .icon(IconName::ExternalLink)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                if let Err(error) =
                                    this.open_route_in_new_window(&route, window, cx)
                                {
                                    this.navigation_error = Some(error.to_string());
                                }
                                cx.notify();
                            });
                        }
                    }),
            )
            .separator()
            .item(PopupMenuItem::new("关闭").icon(IconName::Close).on_click({
                let shell = shell.clone();
                let route = route.clone();
                move |_, window, cx| {
                    _ = shell.update(cx, |this, cx| {
                        this.close_tab_route_in(&route, window, cx);
                    });
                }
            }))
            .separator()
            .item(
                PopupMenuItem::new("关闭左侧标签页")
                    .icon(IconName::ArrowLeft)
                    .disabled(!can_close_left)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                this.close_tabs_to_left_in(&route, window, cx);
                            });
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("关闭右侧标签页")
                    .icon(IconName::ArrowRight)
                    .disabled(!can_close_right)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                this.close_tabs_to_right_in(&route, window, cx);
                            });
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("关闭其他标签页")
                    .disabled(!can_close_other)
                    .on_click({
                        let shell = shell.clone();
                        let route = route.clone();
                        move |_, window, cx| {
                            _ = shell.update(cx, |this, cx| {
                                this.close_other_tabs_in(&route, window, cx);
                            });
                        }
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new(if pinned {
                    "取消置顶标签页"
                } else {
                    "置顶标签页"
                })
                .checked(pinned)
                .on_click({
                    move |_, _, cx| {
                        _ = shell.update(cx, |this, cx| {
                            this.toggle_pin_route(&route);
                            cx.notify();
                        });
                    }
                }),
            )
    }

    fn active_reload_availability(&self, cx: &App) -> crate::FeatureReloadAvailability {
        self.feature_instances
            .get(self.active_key())
            .map(|instance| instance.reload_availability(cx))
            .unwrap_or_default()
    }

    fn reload_active_feature(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active_key = self.active_key().to_owned();
        if self.reload_tasks.contains_key(active_key.as_str())
            || self.active_reload_availability(cx) != crate::FeatureReloadAvailability::Available
        {
            return;
        }
        let Some(instance) = self.feature_instances.get(self.active_key()) else {
            return;
        };
        let reload = instance.reload(window, cx);
        let completed_key = active_key.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            reload.await;
            _ = this.update_in(cx, |this, _, cx| {
                this.reload_tasks.remove(completed_key.as_str());
                cx.notify();
            });
        });
        self.reload_tasks.insert(active_key, task);
        cx.notify();
    }

    fn render_tab_bar_prefix(&self, cx: &mut Context<Self>) -> AnyElement {
        let can_navigate_back = self.can_navigate_back();
        let can_navigate_forward = self.can_navigate_forward();
        let reload_availability = self.active_reload_availability(cx);
        let reload_loading = self.reload_tasks.contains_key(self.active_key());

        h_flex()
            .mx_1()
            .flex_shrink_0()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                workspace_icon_button("tabs-back", IconName::ArrowLeft, "后退")
                    .disabled(!can_navigate_back)
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        match this.navigate_back_in(window, cx) {
                            Ok(()) => this.navigation_error = None,
                            Err(error) => this.navigation_error = Some(error.to_string()),
                        }
                        cx.notify();
                    })),
            )
            .child(
                workspace_icon_button("tabs-forward", IconName::ArrowRight, "前进")
                    .disabled(!can_navigate_forward)
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        match this.navigate_forward_in(window, cx) {
                            Ok(()) => this.navigation_error = None,
                            Err(error) => this.navigation_error = Some(error.to_string()),
                        }
                        cx.notify();
                    })),
            )
            .child(
                workspace_icon_button(
                    "tabs-reload",
                    Icon::default().path("icons/rotate-ccw.svg"),
                    "刷新当前页面",
                )
                .loading(reload_loading)
                .disabled(
                    reload_loading
                        || reload_availability != crate::FeatureReloadAvailability::Available,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.reload_active_feature(window, cx);
                })),
            )
            .into_any_element()
    }

    fn render_tab_bar_suffix(&self, cx: &mut Context<Self>) -> AnyElement {
        workspace_icon_button("open-feature-search", IconName::Plus, "打开页面")
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_search(SearchMode::OpenPage, window, cx);
            }))
            .into_any_element()
    }

    fn account_partition_id(&self, cx: &App) -> String {
        #[cfg(feature = "desktop")]
        if self.account_enabled {
            return crate::account::client::login_profile(cx)
                .map(|profile| profile.user.id.clone())
                .unwrap_or_else(|| "anonymous".to_owned());
        }
        "anonymous".to_owned()
    }

    fn feature_search_provider(&self) -> SearchProvider {
        let registry = self.registry.clone();
        let resolver_registry = self.registry.clone();
        let account_enabled = self.account_enabled;
        let resolver_account_enabled = self.account_enabled;
        SearchProvider::new("nexora.features", i32::MIN)
            .modes([SearchMode::Global, SearchMode::OpenPage])
            .on_change(move |request, _, cx| {
                let registry = registry.clone();
                Task::ready(Ok(vec![feature_search_section(
                    &registry,
                    account_enabled,
                    &request,
                    cx,
                )]))
            })
            .on_resolve_history(move |_mode, item_id, _, cx| {
                let item = resolver_registry
                    .navigation_features()
                    .find(|metadata| metadata.id() == item_id)
                    .filter(|metadata| !metadata.path().contains('{'))
                    .filter(|metadata| {
                        Self::feature_visible_for_account(resolver_account_enabled, *metadata, cx)
                    })
                    .map(feature_search_item);
                Task::ready(Ok(item))
            })
    }

    fn open_search(&mut self, mode: SearchMode, window: &mut Window, cx: &mut Context<Self>) {
        // 搜索使用官方 Dialog 栈；在同一窗口已有弹层时直接复用当前弹层，避免连续双击
        // Shift 叠加多个全局搜索实例。
        if window.has_active_dialog(cx) {
            return;
        }
        let mut providers = vec![self.feature_search_provider()];
        if mode != SearchMode::OpenPage {
            providers.extend(installed_search_providers(cx));
        }
        let account_id = self.account_partition_id(cx);
        let search = cx.new(|cx| SearchDialog::new(mode, account_id, providers, window, cx));
        let search_for_dialog = search.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("全局搜索")
                .w(px(680.0))
                .max_w(px(760.0))
                .overlay(true)
                .overlay_closable(true)
                .child(search_for_dialog.clone())
        });
        search.update(cx, |search, cx| search.start(window, cx));
    }

    fn render_global_title_bar_content(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toolbar = shell_toolbar_actions(cx);
        let search_shortcut = ShortcutHint::binding_for_action(&OpenGlobalSearch, None, window);
        let search_colors = cx.theme().semantic_tokens().colors;
        h_flex()
            .relative()
            .flex_1()
            .h_full()
            .min_w_0()
            .items_center()
            .justify_center()
            .child(
                Button::new("open-global-search")
                    .debug_selector(|| "nexora-global-search-trigger".into())
                    .w(px(420.0))
                    .max_w_full()
                    .h(px(30.0))
                    .px_3()
                    .rounded(px(8.0))
                    .bg(search_colors.surface)
                    .border_color(search_colors.input)
                    .text_color(search_colors.muted_foreground)
                    .child(
                        h_flex()
                            .size_full()
                            .min_w_0()
                            .gap_2()
                            .justify_start()
                            .child(Icon::new(IconName::Search).with_size(WORKSPACE_SHELL_ICON_SIZE))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_left()
                                    .child("搜索或跳转到…"),
                            )
                            .when_some(search_shortcut, |this, shortcut| this.child(shortcut)),
                    )
                    .tooltip("全局搜索")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_search(SearchMode::Global, window, cx);
                    })),
            )
            .when(!toolbar.actions.is_empty(), |this| {
                this.child(
                    h_flex()
                        .absolute()
                        .right_0()
                        .pr(toolbar.right_padding)
                        .h_full()
                        .items_center()
                        .gap_1()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(toolbar.actions),
                )
            })
            .into_any_element()
    }

    fn render_tab_bar_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let opened_tabs = self.opened_tabs.clone();
        let pinned_tabs = self.pinned_tabs.clone();
        let active_tab_index = self.tab_index(&self.active_route);
        let shell = cx.entity().downgrade();
        let title_bar_background = cx.theme().tokens.title_bar;

        h_flex()
            .w_full()
            .h(px(42.0))
            .flex_shrink_0()
            .min_w_0()
            .overflow_hidden()
            .items_center()
            .child(
                div()
                    .id("nexora-open-tabs-zone")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .id("nexora-open-tabs-strip")
                            .absolute()
                            .left_0()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .h_full()
                            .min_w_0()
                            .overflow_hidden()
                            .items_center()
                            .child(
                                div()
                                    .id("nexora-tabs-zone")
                                    .relative()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .overflow_hidden()
                                    .child(
                                        self.tab_style
                                            .apply(TabBar::new("nexora-open-tabs"))
                                            .w_full()
                                            .h_full()
                                            .with_size(theme::component_size(cx))
                                            .prefix(self.render_tab_bar_prefix(cx))
                                            .suffix(self.render_tab_bar_suffix(cx))
                                            .track_scroll(&self.tab_scroll_handle)
                                            .menu(!opened_tabs.is_empty())
                                            .when_some(active_tab_index, |this, index| {
                                                this.selected_index(index)
                                            })
                                            .on_click(cx.listener(
                                                |this, index: &usize, window, cx| {
                                                    match this.select_tab_in(*index, window, cx) {
                                                        Ok(()) => this.navigation_error = None,
                                                        Err(error) => {
                                                            this.navigation_error =
                                                                Some(error.to_string())
                                                        }
                                                    }
                                                    cx.notify();
                                                },
                                            ))
                                            .children(opened_tabs.iter().cloned().enumerate().map(
                                                |(index, route)| {
                                                    let is_pinned = pinned_tabs.contains(&route);
                                                    Self::render_tab(
                                                        route,
                                                        index,
                                                        is_pinned,
                                                        shell.clone(),
                                                    )
                                                },
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("nexora-open-tabs-bottom-mask")
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(1.0))
                            .bg(title_bar_background),
                    )
                    .context_menu({
                        let shell = shell.clone();
                        move |menu, _, cx| {
                            let Some(shell_entity) = shell.upgrade() else {
                                return menu;
                            };
                            let Some(route) =
                                shell_entity.update(cx, |this, _| this.tab_context_route.take())
                            else {
                                return menu;
                            };
                            Self::build_tab_context_menu(menu, route, shell.clone(), cx)
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_active_feature(&self) -> AnyElement {
        self.feature_instances
            .get(self.active_key())
            .map(|instance| instance.view().into_any_element())
            .unwrap_or_else(|| div().into_any_element())
    }

    fn render_active_panel_overlay(&self, cx: &App) -> Option<AnyElement> {
        self.feature_instances
            .get(self.active_key())?
            .panel_overlay(cx)
            .map(IntoElement::into_any_element)
    }

    fn active_content_scrollable(&self) -> bool {
        self.registry
            .features()
            .iter()
            .find(|metadata| metadata.id() == self.active_target_id())
            .map(|metadata| metadata.content_scrollable())
            .unwrap_or(true)
    }

    fn render_workspace(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let navigation_error = self.navigation_error.clone();
        let active_feature = div()
            .relative()
            .size_full()
            .child(self.render_active_feature())
            .when_some(navigation_error, |element, message| {
                element.child(
                    div()
                        .absolute()
                        .left_3()
                        .right_3()
                        .bottom_3()
                        .child(Alert::error("nexora-navigation-error", message)),
                )
            });

        let layout = WorkspaceLayout::new(
            self.render_sidebar(window, cx),
            self.render_global_title_bar_content(window, cx),
            self.render_tab_bar_content(cx),
            active_feature,
        )
        .with_sidebar_collapsed(self.sidebar_collapsed)
        .with_content_scrollable(self.active_content_scrollable());
        let layout = match self.render_active_panel_overlay(cx) {
            Some(overlay) => layout.with_panel_overlay(overlay),
            None => layout,
        };
        layout.render(window, cx)
    }
}

#[cfg(feature = "desktop")]
fn account_icon(kind: AccountActionKind) -> IconName {
    match kind {
        AccountActionKind::SignIn => IconName::CircleUser,
        AccountActionKind::Settings => IconName::Settings2,
        AccountActionKind::Updates => IconName::CircleCheck,
        AccountActionKind::SignOut => IconName::CircleX,
    }
}

fn feature_icon(icon: Option<&str>) -> Icon {
    Icon::default()
        .path(format!("icons/{}.svg", icon.unwrap_or("frame")))
        .size_4()
}

fn sidebar_feature_icon(icon: Option<&str>) -> Icon {
    Icon::default()
        .path(format!("icons/{}.svg", icon.unwrap_or("frame")))
        .with_size(WORKSPACE_SHELL_ICON_SIZE)
}

impl Render for ApplicationShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        #[cfg(feature = "desktop")]
        let content = if self.account_enabled && !self.authenticated {
            let navigation_error = self.navigation_error.clone();
            div()
                .relative()
                .size_full()
                .child(self.login_feature.clone())
                .child(
                    div()
                        .debug_selector(|| "nexora-shell-login-title-bar".into())
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .child(
                            TitleBar::new()
                                .border_b(px(0.0))
                                .bg(gpui::transparent_black()),
                        ),
                )
                .when_some(navigation_error, |element, message| {
                    element.child(
                        div()
                            .absolute()
                            .left_3()
                            .right_3()
                            .bottom_3()
                            .child(Alert::error("nexora-login-navigation-error", message)),
                    )
                })
                .into_any_element()
        } else {
            self.render_workspace(window, cx)
        };
        #[cfg(not(feature = "desktop"))]
        let content = self.render_workspace(window, cx);

        let root = div()
            .relative()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &OpenGlobalSearch, window, cx| {
                if !this.account_enabled || this.authenticated {
                    this.open_search(SearchMode::Global, window, cx);
                }
            }))
            .child(content)
            .children(ui::window_layers(window, cx));
        #[cfg(feature = "desktop")]
        let root = root.key_context(account_actions::CONTEXT);
        root.into_any_element()
    }
}

impl gpui::Focusable for ApplicationShell {
    fn focus_handle(&self, _cx: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

fn create_initial_feature(
    registry: &AppRegistry,
    initial_route: RouteMatch,
    window: &mut Window,
    cx: &mut App,
) -> (HashMap<String, FeatureInstance>, Option<String>) {
    let active_key = initial_route.stable_id();
    let mut feature_instances = HashMap::new();
    let navigation_error = match registry.create_feature(initial_route, window, cx) {
        Ok(mut instance) => {
            instance.activate(window, cx);
            feature_instances.insert(active_key, instance);
            None
        }
        Err(error) => Some(error.to_string()),
    };
    (feature_instances, navigation_error)
}
