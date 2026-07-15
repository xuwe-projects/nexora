use chrono::{TimeZone as _, Utc};
use kernel::{Clock, ExecutionContext, Page, PageRequest, RequestId, ValidationError};

#[test]
fn request_id_accepts_safe_upstream_values_and_rejects_unsafe_values() {
    assert_eq!(
        RequestId::parse("upstream-42")
            .expect("安全请求 ID 应当通过校验")
            .as_str(),
        "upstream-42"
    );
    assert!(RequestId::parse("unsafe request id").is_err());
    assert!(RequestId::generate().as_str().starts_with("req_"));
}

#[test]
fn execution_context_uses_injected_clock() {
    let context = ExecutionContext::start(
        RequestId::parse("request-1").expect("固定请求 ID 应当有效"),
        &FixedClock,
    );

    assert_eq!(context.request_id().as_str(), "request-1");
    assert_eq!(
        context.started_at(),
        Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("固定测试时间应当有效")
    );
}

#[test]
fn page_preserves_validated_request_and_total() {
    let request = PageRequest::new(2, 2).expect("非零分页参数应当有效");
    let page = Page::new(vec!["one", "two"], 7, request);

    assert_eq!(page.items(), &["one", "two"]);
    assert_eq!(page.total(), 7);
    assert_eq!(page.request().number(), 2);
    assert_eq!(page.request().size(), 2);
    assert!(PageRequest::new(0, 10).is_none());
    assert!(PageRequest::new(1, 0).is_none());
}

#[test]
fn validation_error_exposes_stable_field_details() {
    let error = ValidationError::new("name", "名称不能为空");

    assert_eq!(error.field(), "name");
    assert_eq!(error.message(), "名称不能为空");
    assert_eq!(error.to_string(), "字段 name 无效: 名称不能为空");
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0)
            .single()
            .expect("固定测试时间应当有效")
    }
}
