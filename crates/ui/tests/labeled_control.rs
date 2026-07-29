use gpui::{
    Context, IntoElement, Render, SharedString, TestAppContext, Window, div, prelude::*, px,
};
use gpui_component::{Sizable as _, Size};
use ui::{FieldValue, LabeledControl};

const LABELED_CONTROL_SOURCE: &str = include_str!("../src/labeled_control.rs");

struct LabeledControlTestRoot {
    case: LabeledControlCase,
}

enum LabeledControlCase {
    Basic,
    Complete,
}

impl Render for LabeledControlTestRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let control = match self.case {
            LabeledControlCase::Basic => LabeledControl::new("关键词", child_control())
                .width(px(280.0))
                .with_size(Size::Medium)
                .into_any_element(),
            LabeledControlCase::Complete => LabeledControl::new("邮箱", child_control())
                .description("用于接收通知")
                .required()
                .error("邮箱格式不正确")
                .with_size(Size::Small)
                .into_any_element(),
        };

        div().size_full().p_4().child(control)
    }
}

fn child_control() -> impl IntoElement {
    div()
        .debug_selector(|| "labeled-control-child".into())
        .h(px(24.0))
        .w_full()
        .child("控件")
}

#[gpui::test]
fn labeled_control_renders_label_and_child(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(|_, _| LabeledControlTestRoot {
        case: LabeledControlCase::Basic,
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    let control = cx
        .debug_bounds("labeled-control")
        .expect("LabeledControl 应当渲染容器");
    let label = cx
        .debug_bounds("labeled-control-label")
        .expect("LabeledControl 应当渲染 label 行");
    let child = cx
        .debug_bounds("labeled-control-child")
        .expect("LabeledControl 应当渲染 child control");

    assert_eq!(control.size.width, px(280.0));
    assert!(label.origin.y < child.origin.y);
    assert!(child.size.height > px(0.0));
}

#[gpui::test]
fn labeled_control_orders_description_child_and_error(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let (_root, cx) = cx.add_window_view(|_, _| LabeledControlTestRoot {
        case: LabeledControlCase::Complete,
    });

    cx.update(|window, cx| {
        _ = window.draw(cx);
    });

    let label = cx
        .debug_bounds("labeled-control-label")
        .expect("LabeledControl 应当渲染 label 行");
    let required = cx
        .debug_bounds("labeled-control-required")
        .expect("required() 应当渲染必填星号");
    let description = cx
        .debug_bounds("labeled-control-description")
        .expect("description() 应当渲染说明文本");
    let child = cx
        .debug_bounds("labeled-control-child")
        .expect("LabeledControl 应当渲染 child control");
    let error = cx
        .debug_bounds("labeled-control-error")
        .expect("error() 应当渲染错误文本");

    assert!(required.origin.x >= label.origin.x);
    assert!(label.origin.y < description.origin.y);
    assert!(description.origin.y < child.origin.y);
    assert!(child.origin.y < error.origin.y);
}

#[test]
fn source_contract_keeps_labeled_control_visual_tokens_and_size_mapping() {
    assert!(LABELED_CONTROL_SOURCE.contains("Label::new(data.label)"));
    assert!(LABELED_CONTROL_SOURCE.contains(".text_xs()"));
    assert!(LABELED_CONTROL_SOURCE.contains("cx.theme().muted_foreground"));
    assert!(LABELED_CONTROL_SOURCE.contains("cx.theme().danger"));
    assert!(LABELED_CONTROL_SOURCE.contains("Size::XSmall => this.gap_0p5()"));
    assert!(LABELED_CONTROL_SOURCE.contains("Size::Large => this.gap_2()"));
    assert!(LABELED_CONTROL_SOURCE.contains("Size::Small | Size::Medium | Size::Size(_)"));
    assert!(LABELED_CONTROL_SOURCE.contains(".child(data.child)"));
    assert!(LABELED_CONTROL_SOURCE.contains("EventCommand::SetError"));
    assert!(LABELED_CONTROL_SOURCE.contains("field.revision != revision"));
}

#[test]
fn field_value_required_semantics_match_form_rules() {
    assert!(!SharedString::is_present(&"  ".into(), None));
    assert!(SharedString::is_present(&" alice ".into(), None));
    assert!(!bool::is_present(&"false".into(), Some(&false)));
    assert!(bool::is_present(&"true".into(), Some(&true)));
    assert!(!Option::<i64>::is_present(&"".into(), Some(&None)));
    assert!(Option::<i64>::is_present(&"42".into(), Some(&Some(42))));
}

#[test]
fn number_field_values_parse_without_losing_invalid_drafts() {
    assert_eq!(i64::parse_field_value(&"42".into(), false), Ok(42));
    assert!(i64::parse_field_value(&"-".into(), false).is_err());
    assert_eq!(f64::parse_field_value(&"3.5".into(), false), Ok(3.5));
    assert_eq!(
        Option::<i64>::parse_field_value(&"".into(), false),
        Ok(None)
    );
    assert_eq!(
        Option::<i64>::parse_field_value(&"7".into(), false),
        Ok(Some(7))
    );
    assert!(Option::<i64>::parse_field_value(&"7x".into(), false).is_err());
}
