#![cfg(all(feature = "desktop", feature = "derive"))]

use nexora::{
    AppRegistry, Feature, FeatureElement, FeatureMetadata, NoPath, NoQuery, Path, Query,
    RegistryError, ResolveError, RouteExtractError, RouteTarget, RouteTargetKind, Window,
    WindowElement,
    gpui::{Context, Empty, IntoElement, Window as GpuiWindow},
};
use serde::Deserialize;

macro_rules! impl_empty_feature_element {
    ($($feature:ty),+ $(,)?) => {
        $(
            impl FeatureElement for $feature {
                fn render(
                    &mut self,
                    _: &mut GpuiWindow,
                    _: &mut Context<Self>,
                ) -> impl IntoElement {
                    Empty
                }
            }
        )+
    };
}

macro_rules! impl_empty_window_element {
    ($($window:ty),+ $(,)?) => {
        $(
            impl WindowElement for $window {
                fn render(
                    &mut self,
                    _: &mut GpuiWindow,
                    _: &mut Context<Self>,
                ) -> impl IntoElement {
                    Empty
                }
            }
        )+
    };
}

#[derive(Default, Feature)]
#[nexora(
    title = "首页",
    path = "/",
    section = "工作台",
    icon = "layout-dashboard",
    order = -10
)]
struct HomeFeature;

#[derive(Default, Feature)]
#[nexora(title = "用户管理", path = "/users", section = "访问控制", order = 10)]
struct UsersFeature;

#[derive(Default, Feature)]
#[nexora(
    title = "用户详情",
    path = "/users/details/:id",
    path_params = DecodedUserDetailsPath,
    navigation = false
)]
struct UserDetailsFeature;

#[derive(Default, Feature)]
#[nexora(title = "新建用户", path = "/users/details/new", navigation = false)]
struct NewUserFeature;

#[derive(Default, Feature)]
#[nexora(
    title = "用户角色",
    path = "/users/roles",
    parent = "users",
    order = 20
)]
struct UserRolesFeature;

impl_empty_feature_element!(
    HomeFeature,
    UsersFeature,
    UserDetailsFeature,
    NewUserFeature,
    UserRolesFeature,
);

#[derive(Default, nexora::SettingsWindow)]
struct SettingsWindow;

#[derive(Default, Window)]
#[nexora(
    title = "用户窗口",
    path = "/windows/users/:id",
    path_params = UserDetailsPath
)]
struct UserWindow;

impl_empty_window_element!(SettingsWindow, UserWindow);

#[test]
fn derive_exposes_stable_feature_and_window_metadata() {
    assert_eq!(HomeFeature::METADATA.id(), "home");
    assert_eq!(HomeFeature::METADATA.path(), "/");
    assert_eq!(HomeFeature::METADATA.section(), Some("工作台"));
    assert_eq!(HomeFeature::METADATA.icon(), Some("layout-dashboard"));
    assert!(HomeFeature::METADATA.navigation());

    assert_eq!(UserDetailsFeature::METADATA.id(), "user-details");
    assert!(!UserDetailsFeature::METADATA.navigation());
    assert_eq!(SettingsWindow::METADATA.id(), "settings");
    assert_eq!(SettingsWindow::METADATA.path(), "/settings");
}

#[test]
fn registry_builds_navigation_and_children_from_metadata() {
    let registry = AppRegistry::builder()
        .feature::<UserRolesFeature>()
        .feature::<UsersFeature>()
        .feature::<HomeFeature>()
        .feature::<UserDetailsFeature>()
        .settings_window::<SettingsWindow>()
        .build()
        .unwrap();

    let navigation_ids = registry
        .navigation_features()
        .map(|metadata| metadata.id())
        .collect::<Vec<_>>();
    assert_eq!(navigation_ids, ["home", "users", "user-roles"]);
    assert_eq!(
        registry
            .children_of("users")
            .map(|metadata| metadata.id())
            .collect::<Vec<_>>(),
        ["user-roles"]
    );
    assert_eq!(registry.windows(), [SettingsWindow::METADATA]);
}

#[test]
fn dynamic_path_extracts_decoded_parameter_and_query() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .build()
        .unwrap();

    let matched = registry
        .resolve("/users/details/%E5%BC%A0%E4%B8%89?tab=roles&tag=a&tag=b")
        .unwrap();

    assert_eq!(matched.target().kind(), RouteTargetKind::Feature);
    assert_eq!(matched.target().id(), "user-details");
    assert_eq!(matched.concrete_path(), "/users/details/%E5%BC%A0%E4%B8%89");
    let Path(path): Path<DecodedUserDetailsPath> = matched.path().unwrap();
    let Query(query): Query<DecodedUserDetailsQuery> = matched.query().unwrap();

    assert_eq!(path.id, "张三");
    assert_eq!(query.tab, "roles");
    assert_eq!(query.tag, ["a", "b"]);
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct DecodedUserDetailsPath {
    id: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct DecodedUserDetailsQuery {
    tab: String,
    tag: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct UserDetailsPath {
    id: u64,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct UserDetailsQuery {
    page: u32,
    tab: String,
    #[serde(default)]
    tag: Vec<String>,
    search: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct SourceQuery {
    source: String,
}

#[test]
fn typed_path_and_query_extractors_deserialize_business_structures() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .build()
        .unwrap();
    let matched = registry
        .resolve("/users/details/42?page=2&tab=roles&tag=a&tag=b")
        .unwrap();

    let path: Path<UserDetailsPath> = matched.path().unwrap();
    let query: Query<UserDetailsQuery> = matched.query().unwrap();

    assert_eq!(path.id, 42);
    assert_eq!(query.page, 2);
    let Path(path) = path;
    let Query(query) = query;

    assert_eq!(path, UserDetailsPath { id: 42 });
    assert_eq!(
        query,
        UserDetailsQuery {
            page: 2,
            tab: "roles".to_owned(),
            tag: vec!["a".to_owned(), "b".to_owned()],
            search: None,
        }
    );
}

#[test]
fn typed_extractors_report_source_target_and_failing_field() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .build()
        .unwrap();
    let matched = registry
        .resolve("/users/details/not-a-number?page=invalid&tab=roles")
        .unwrap();

    let path_error = matched.path::<UserDetailsPath>().unwrap_err();
    assert!(matches!(
        path_error,
        RouteExtractError::Path {
            target,
            field,
            message: _,
        } if target.ends_with("UserDetailsPath") && field == "id"
    ));

    let query_error = matched.query::<UserDetailsQuery>().unwrap_err();
    assert!(matches!(
        query_error,
        RouteExtractError::Query {
            target,
            field,
            message: _,
        } if target.ends_with("UserDetailsQuery") && field == "page"
    ));
}

#[test]
fn equivalent_percent_encodings_share_one_canonical_path() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .build()
        .unwrap();

    let plain_ascii = registry.resolve("/users/details/A").unwrap();
    let encoded_ascii = registry.resolve("/users/details/%41").unwrap();
    let plain_unicode = registry.resolve("/users/details/张三").unwrap();
    let encoded_unicode = registry
        .resolve("/users/details/%E5%BC%A0%E4%B8%89")
        .unwrap();

    assert_eq!(plain_ascii.concrete_path(), encoded_ascii.concrete_path());
    assert_eq!(plain_ascii.concrete_path(), "/users/details/A");
    assert_eq!(
        plain_unicode.concrete_path(),
        encoded_unicode.concrete_path()
    );
    assert_eq!(
        plain_unicode.concrete_path(),
        "/users/details/%E5%BC%A0%E4%B8%89"
    );
}

#[test]
fn malformed_percent_encoding_is_rejected_before_matching() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .build()
        .unwrap();

    assert!(matches!(
        registry.resolve("/users/details/%ZZ"),
        Err(ResolveError::InvalidLocation { .. })
    ));
}

#[test]
fn static_route_has_priority_over_dynamic_route() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .feature::<NewUserFeature>()
        .build()
        .unwrap();

    assert_eq!(
        registry
            .resolve("/users/details/new")
            .unwrap()
            .target()
            .id(),
        "new-user"
    );
    assert_eq!(
        registry.resolve("/users/details/42").unwrap().target().id(),
        "user-details"
    );
}

#[test]
fn custom_scheme_host_and_path_are_normalized() {
    let registry = AppRegistry::builder()
        .feature::<UserDetailsFeature>()
        .build()
        .unwrap();

    let matched = registry
        .resolve("myapp://users/details/42?source=email")
        .unwrap();
    let Path(path): Path<UserDetailsPath> = matched.path().unwrap();
    let Query(query): Query<SourceQuery> = matched.query().unwrap();

    assert_eq!(matched.concrete_path(), "/users/details/42");
    assert_eq!(path.id, 42);
    assert_eq!(query.source, "email");
}

#[test]
fn trailing_slash_is_canonicalized_for_concrete_location() {
    let registry = AppRegistry::builder()
        .feature::<UsersFeature>()
        .build()
        .unwrap();

    let matched = registry.resolve("/users/").unwrap();

    assert_eq!(matched.concrete_path(), "/users");
}

#[test]
fn window_path_resolves_without_becoming_a_feature() {
    let registry = AppRegistry::builder()
        .settings_window::<SettingsWindow>()
        .window::<UserWindow>()
        .build()
        .unwrap();

    let matched = registry.resolve("/windows/users/42").unwrap();
    let Path(path): Path<UserDetailsPath> = matched.path().unwrap();

    assert_eq!(matched.target().kind(), RouteTargetKind::Window);
    assert_eq!(path.id, 42);
    assert!(matches!(matched.target(), RouteTarget::Window(_)));
}

#[derive(Default, Feature)]
#[nexora(
    title = "成员",
    path = "/members/:id",
    path_params = DecodedUserDetailsPath,
    navigation = false
)]
struct MemberByIdFeature;

impl_empty_feature_element!(MemberByIdFeature);

#[derive(Debug, Clone, Deserialize)]
struct MemberWindowPath {
    #[serde(rename = "member_id")]
    _member_id: String,
}

#[derive(Default, Window)]
#[nexora(
    title = "成员窗口",
    path = "/members/:member_id",
    path_params = MemberWindowPath
)]
struct MemberWindow;

impl_empty_window_element!(MemberWindow);

#[test]
fn feature_and_window_share_one_conflict_namespace() {
    let error = AppRegistry::builder()
        .feature::<MemberByIdFeature>()
        .window::<MemberWindow>()
        .build()
        .err()
        .unwrap();

    assert!(matches!(error, RegistryError::RouteConflict { .. }));
}

struct InvalidFeature;

impl nexora::Feature for InvalidFeature {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: FeatureMetadata = FeatureMetadata::new(
        "invalid",
        "非法页面",
        "/users//invalid",
        None,
        None,
        None,
        0,
        false,
    );
}

struct TooManyParametersFeature;

impl nexora::Feature for TooManyParametersFeature {
    type Path = NoPath;
    type Query = NoQuery;

    const METADATA: FeatureMetadata = FeatureMetadata::new(
        "too-many-parameters",
        "参数过多",
        "/:p0/:p1/:p2/:p3/:p4/:p5/:p6/:p7/:p8/:p9/:p10/:p11/:p12/:p13/:p14/:p15/:p16/:p17/:p18/:p19/:p20/:p21/:p22/:p23/:p24/:p25",
        None,
        None,
        None,
        0,
        false,
    );
}

#[test]
fn excessive_dynamic_parameters_return_an_error_instead_of_panicking() {
    let error = AppRegistry::builder()
        .feature::<TooManyParametersFeature>()
        .build()
        .err()
        .unwrap();

    assert!(matches!(
        error,
        RegistryError::InvalidFeature {
            id: "too-many-parameters",
            ..
        }
    ));
}

#[derive(Default, Feature)]
#[nexora(title = "循环页面", path = "/cycle", parent = "cycle")]
struct CycleFeature;

#[derive(Default, Feature)]
#[nexora(title = "隐藏父页面", path = "/hidden-parent", navigation = false)]
struct HiddenParentFeature;

#[derive(Default, Feature)]
#[nexora(
    title = "可见子页面",
    path = "/visible-child",
    parent = "hidden-parent"
)]
struct VisibleChildFeature;

impl_empty_feature_element!(CycleFeature, HiddenParentFeature, VisibleChildFeature);

#[test]
fn invalid_feature_parent_graphs_are_rejected() {
    assert!(matches!(
        AppRegistry::builder()
            .feature::<CycleFeature>()
            .build()
            .err()
            .unwrap(),
        RegistryError::FeatureParentCycle { id: "cycle" }
    ));
    assert!(matches!(
        AppRegistry::builder()
            .feature::<HiddenParentFeature>()
            .feature::<VisibleChildFeature>()
            .build()
            .err()
            .unwrap(),
        RegistryError::HiddenFeatureParent {
            id: "visible-child",
            parent: "hidden-parent"
        }
    ));
}

#[test]
fn manually_implemented_invalid_path_is_rejected_at_build_time() {
    let error = AppRegistry::builder()
        .feature::<InvalidFeature>()
        .build()
        .err()
        .unwrap();

    assert!(matches!(
        error,
        RegistryError::InvalidFeature { id: "invalid", .. }
    ));
}

#[test]
fn unknown_path_returns_normalized_not_found_error() {
    let registry = AppRegistry::builder()
        .feature::<HomeFeature>()
        .build()
        .unwrap();

    assert_eq!(
        registry.resolve("/missing/").unwrap_err(),
        ResolveError::NotFound {
            path: "/missing".to_owned()
        }
    );
}
