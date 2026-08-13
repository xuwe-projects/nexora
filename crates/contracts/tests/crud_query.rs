use contracts::{
    crud_query::{CrudFilterControl, CrudFilterPresentation, CrudFilterTrigger, CrudQuery as _},
    pagination::PageQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CityStatus {
    Active,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CitySort {
    NameAsc,
    NameDesc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, contracts::CrudQuery)]
#[nexora(page_size(
    default = 25,
    min = 15,
    max = 100,
    options = [15, 25, 50, 100]
))]
struct CityQuery {
    #[nexora(pagination)]
    #[serde(flatten)]
    page: PageQuery,
    #[nexora(filter(
        label = "关键词",
        placeholder = "名称或编码",
        control = "input",
        pattern = "^[^\\n]+$",
        pattern_message = "关键词不能包含换行"
    ))]
    keyword: Option<String>,
    #[nexora(filter(label = "状态", control = "select", presentation = "quick"))]
    status: Option<CityStatus>,
    #[nexora(sort)]
    sort: Option<CitySort>,
    include_archived: bool,
}

#[test]
fn derive_preserves_flat_page_wire_format_and_metadata() {
    let query = CityQuery {
        page: PageQuery {
            page: 3,
            page_size: 50,
        },
        keyword: Some("sz".to_owned()),
        status: Some(CityStatus::Active),
        sort: Some(CitySort::NameAsc),
        include_archived: false,
    };

    let value = serde_json::to_value(&query).expect("CRUD 查询应当可以序列化");
    assert_eq!(value["page"], 3);
    assert_eq!(value["page_size"], 50);
    assert!(value.get("page_query").is_none());

    let metadata = CityQuery::metadata();
    assert_eq!(metadata.page_size.default, 25);
    assert_eq!(metadata.page_size.options, &[15, 25, 50, 100]);
    assert_eq!(metadata.sort_field, Some("sort"));
    assert_eq!(metadata.filters.len(), 2);
    assert_eq!(metadata.filters[0].name, "keyword");
    assert_eq!(metadata.filters[0].control, CrudFilterControl::Input);
    assert_eq!(
        metadata.filters[0].trigger,
        CrudFilterTrigger::Debounce { milliseconds: 300 }
    );
    assert_eq!(
        metadata.filters[1].presentation,
        CrudFilterPresentation::Quick
    );
    assert_eq!(metadata.filters[1].trigger, CrudFilterTrigger::Immediate);
}

#[test]
fn derive_updates_only_declared_filters_with_strong_types() {
    let mut query = CityQuery {
        page: PageQuery::default(),
        keyword: None,
        status: None,
        sort: None,
        include_archived: false,
    };

    query
        .set_filter_value("status", json!("disabled"))
        .expect("合法枚举筛选值应当可以写入");
    assert_eq!(query.status, Some(CityStatus::Disabled));
    assert_eq!(query.filter_value("status"), Some(json!("disabled")));
    assert!(
        query
            .set_filter_value("include_archived", json!(true))
            .is_err()
    );
    assert!(query.set_filter_value("status", json!(42)).is_err());
}

#[test]
fn normalize_clamps_pagination_and_cache_identity_ignores_page_number() {
    let mut query = CityQuery {
        page: PageQuery {
            page: 0,
            page_size: 500,
        },
        keyword: Some("city".to_owned()),
        status: None,
        sort: Some(CitySort::NameDesc),
        include_archived: false,
    };
    query.normalize();
    assert_eq!(query.page.page, 1);
    assert_eq!(query.page.page_size, 100);

    let identity = query.cache_identity().expect("查询身份应当可以序列化");
    query.page.page = 7;
    assert_eq!(query.cache_identity().unwrap(), identity);
    query.page.page_size = 50;
    assert_ne!(query.cache_identity().unwrap(), identity);
}
