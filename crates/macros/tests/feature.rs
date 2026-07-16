extern crate self as nexora;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureMetadata {
    id: &'static str,
    title: &'static str,
    path: &'static str,
}

impl FeatureMetadata {
    #[allow(
        clippy::too_many_arguments,
        reason = "测试替身必须保持与 Nexora FeatureMetadata 构造函数相同的宏展开签名"
    )]
    const fn new(
        id: &'static str,
        title: &'static str,
        path: &'static str,
        _section: Option<&'static str>,
        _icon: Option<&'static str>,
        _parent: Option<&'static str>,
        _order: i32,
        _navigation: bool,
    ) -> Self {
        Self { id, title, path }
    }
}

trait Feature: 'static {
    type Path;
    type Query;

    const METADATA: FeatureMetadata;
    const REGISTRATION: Option<__private::FeatureRegistration> = None;
}

trait FeatureElement: Feature + __private::gpui::Render + Sized {
    fn render(
        &mut self,
        window: &mut __private::gpui::Window,
        cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement;
}

struct NoPath;
struct NoQuery;

struct RouteMatch;
struct FeatureInstance;
struct FeatureRuntimeError;

mod __private {
    pub use inventory;

    use super::FeatureMetadata;

    pub struct FeatureRegistration {
        metadata: FeatureMetadata,
        factory: fn(
            super::RouteMatch,
            &mut gpui::Window,
            &mut gpui::App,
        ) -> Result<super::FeatureInstance, super::FeatureRuntimeError>,
    }

    impl FeatureRegistration {
        pub const fn new(
            metadata: FeatureMetadata,
            factory: fn(
                super::RouteMatch,
                &mut gpui::Window,
                &mut gpui::App,
            ) -> Result<super::FeatureInstance, super::FeatureRuntimeError>,
        ) -> Self {
            Self { metadata, factory }
        }

        pub const fn metadata(&self) -> FeatureMetadata {
            self.metadata
        }

        pub const fn factory(
            &self,
        ) -> fn(
            super::RouteMatch,
            &mut gpui::Window,
            &mut gpui::App,
        ) -> Result<super::FeatureInstance, super::FeatureRuntimeError> {
            self.factory
        }
    }

    inventory::collect!(FeatureRegistration);

    pub fn create_feature<T>(
        _route: super::RouteMatch,
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
        constructor: fn(&mut gpui::Window, &mut gpui::Context<T>) -> T,
    ) -> Result<super::FeatureInstance, super::FeatureRuntimeError> {
        let mut entity_cx = gpui::Context::new();
        let _feature = constructor(window, &mut entity_cx);
        Ok(super::FeatureInstance)
    }

    pub mod gpui {
        use std::marker::PhantomData;

        pub struct Window;
        pub struct App;

        pub struct Context<T>(PhantomData<T>);

        impl<T> Context<T> {
            pub const fn new() -> Self {
                Self(PhantomData)
            }
        }

        pub trait IntoElement {}

        pub trait Render: 'static + Sized {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
        }
    }
}

struct Element;

impl __private::gpui::IntoElement for Element {}

struct UserPath {
    id: u64,
}

struct UserQuery {
    tab: Option<String>,
}

#[derive(Default, nexora_macros::Feature)]
#[nexora(
    title = "用户详情",
    path = "/users/:id",
    path_params = UserPath,
    query_params = UserQuery,
    navigation = false
)]
struct UserFeature {
    rendered: bool,
}

impl FeatureElement for UserFeature {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        self.rendered = true;
        Element
    }
}

#[derive(Default, nexora_macros::Feature)]
#[nexora(title = "首页", path = "/")]
struct HomeFeature;

impl FeatureElement for HomeFeature {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        Element
    }
}

fn create_factory_feature(
    _window: &mut __private::gpui::Window,
    _cx: &mut __private::gpui::Context<FactoryFeatureDefinition>,
) -> FactoryFeatureDefinition {
    FactoryFeatureDefinition
}

#[derive(nexora_macros::Feature)]
#[nexora(
    title = "工厂页面",
    path = "/factory",
    factory = create_factory_feature
)]
struct FactoryFeatureDefinition;

impl FeatureElement for FactoryFeatureDefinition {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        Element
    }
}

fn assert_user_route_types<T>()
where
    T: Feature<Path = UserPath, Query = UserQuery>,
{
}

fn assert_empty_route_types<T>()
where
    T: Feature<Path = NoPath, Query = NoQuery>,
{
}

fn assert_feature_element<T>()
where
    T: FeatureElement,
{
}

#[test]
fn feature_derive_connects_route_types_factory_and_registration() {
    assert_user_route_types::<UserFeature>();
    assert_empty_route_types::<HomeFeature>();
    assert_feature_element::<UserFeature>();
    assert!(<UserFeature as Feature>::REGISTRATION.is_some());

    let mut feature = UserFeature::default();
    let mut window = __private::gpui::Window;
    let mut cx = __private::gpui::Context::<UserFeature>::new();
    let _element =
        <UserFeature as __private::gpui::Render>::render(&mut feature, &mut window, &mut cx);
    drop(_element);
    assert!(feature.rendered);

    let mut registrations = inventory::iter::<__private::FeatureRegistration>
        .into_iter()
        .map(__private::FeatureRegistration::metadata)
        .collect::<Vec<_>>();
    registrations.sort_by_key(|metadata| metadata.path);

    assert_eq!(
        registrations,
        vec![
            FeatureMetadata {
                id: "home",
                title: "首页",
                path: "/",
            },
            FeatureMetadata {
                id: "factory-feature-definition",
                title: "工厂页面",
                path: "/factory",
            },
            FeatureMetadata {
                id: "user",
                title: "用户详情",
                path: "/users/:id",
            },
        ]
    );

    let factory = inventory::iter::<__private::FeatureRegistration>
        .into_iter()
        .find(|registration| registration.metadata().path == "/factory")
        .map(__private::FeatureRegistration::factory)
        .expect("工厂页面应被注册");
    let result = factory(
        RouteMatch,
        &mut __private::gpui::Window,
        &mut __private::gpui::App,
    );
    assert!(result.is_ok());
}

#[test]
fn declared_route_types_are_regular_business_types() {
    let path = UserPath { id: 42 };
    let query = UserQuery {
        tab: Some("roles".to_owned()),
    };

    assert_eq!(path.id, 42);
    assert_eq!(query.tab.as_deref(), Some("roles"));
}
