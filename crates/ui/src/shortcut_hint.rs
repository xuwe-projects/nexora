//! 由真实 GPUI Action 绑定生成的完整快捷键提示。

use gpui::{
    Action, App, AsKeystroke as _, IntoElement, KeyContext, Keystroke, ParentElement as _,
    RenderOnce, Styled as _, Window,
};
use gpui_component::{h_flex, kbd::Kbd};

/// 使用官方 [`Kbd`] 逐项展示一个 Action 的完整按键序列。
///
/// `gpui-component` 的 `Kbd::binding_for_action` 只返回绑定序列中的第一个按键；该薄包装
/// 保留 GPUI 的绑定解析和优先级规则，并组合多个官方 `Kbd`，因此可正确显示
/// `shift shift` 等多段快捷键。
#[derive(IntoElement)]
pub struct ShortcutHint {
    keystrokes: Vec<Keystroke>,
}

impl ShortcutHint {
    /// 查找 Action 在给定上下文中的最高优先级绑定，并生成完整快捷键提示。
    ///
    /// 没有有效绑定、上下文表达式无法解析或绑定不包含按键时返回 `None`。
    pub fn binding_for_action(
        action: &dyn Action,
        context: Option<&str>,
        window: &Window,
    ) -> Option<Self> {
        let key_context = context.and_then(|context| KeyContext::parse(context).ok());
        let binding = match key_context {
            Some(context) => {
                window.highest_precedence_binding_for_action_in_context(action, context)
            }
            None => window.highest_precedence_binding_for_action(action),
        }?;
        let keystrokes = binding
            .keystrokes()
            .iter()
            .map(|keystroke| keystroke.as_keystroke().clone())
            .collect::<Vec<_>>();
        (!keystrokes.is_empty()).then_some(Self { keystrokes })
    }

    /// 返回当前提示按绑定顺序保存的全部按键。
    pub fn keystrokes(&self) -> &[Keystroke] {
        self.keystrokes.as_slice()
    }
}

impl RenderOnce for ShortcutHint {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        h_flex()
            .gap_1()
            .children(self.keystrokes.into_iter().map(Kbd::new))
    }
}
