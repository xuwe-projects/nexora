//! gpui-component 窗口级遮罩层组合。

use gpui::{AnyElement, App, IntoElement as _, Window};
use gpui_component::Root;

/// 收集当前窗口需要渲染的 Sheet、Dialog 与 Notification 遮罩层。
///
/// `gpui_component::Root` 负责保存窗口级遮罩状态，但不会自动把这些遮罩加入业务视图
/// 的元素树。每个窗口的业务根视图都应在主内容之后渲染本函数返回的元素。
pub fn window_layers(window: &mut Window, cx: &mut App) -> Vec<AnyElement> {
    let mut layers = Vec::with_capacity(3);

    if let Some(layer) = Root::render_sheet_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(layer) = Root::render_dialog_layer(window, cx) {
        layers.push(layer.into_any_element());
    }
    if let Some(layer) = Root::render_notification_layer(window, cx) {
        layers.push(layer.into_any_element());
    }

    layers
}
