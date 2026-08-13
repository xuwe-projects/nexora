use gpui::SharedString;
use ui::FieldValue;

const FORM_FIELD_STATE_SOURCE: &str = include_str!("../src/form_field_state.rs");

#[test]
fn field_state_has_no_visual_or_control_rendering_contract() {
    assert!(!FORM_FIELD_STATE_SOURCE.contains("impl<V: FieldValue> Render"));
    assert!(!FORM_FIELD_STATE_SOURCE.contains("impl RenderOnce"));
    assert!(!FORM_FIELD_STATE_SOURCE.contains("Label::new"));
    assert!(!FORM_FIELD_STATE_SOURCE.contains("Input::new"));
    assert!(!FORM_FIELD_STATE_SOURCE.contains("Checkbox::new"));
    assert!(FORM_FIELD_STATE_SOURCE.contains("EventCommand::SetError"));
    assert!(FORM_FIELD_STATE_SOURCE.contains("field.revision != revision"));
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
