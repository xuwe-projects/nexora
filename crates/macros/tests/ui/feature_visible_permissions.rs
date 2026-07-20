extern crate self as nexora;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FeatureMetadata;

impl FeatureMetadata {
    #[allow(clippy::too_many_arguments)]
    const fn new(
        _id: &'static str,
        _title: &'static str,
        _path: &'static str,
        _section: Option<&'static str>,
        _icon: Option<&'static str>,
        _parent: Option<&'static str>,
        _order: i32,
        _navigation: bool,
    ) -> Self {
        Self
    }

    const fn with_content_scrollable(self, _content_scrollable: bool) -> Self {
        self
    }

    const fn with_visible_permissions_any(
        self,
        _permissions: &'static [&'static str],
    ) -> Self {
        self
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
    use super::FeatureMetadata;

    pub struct FeatureRegistration;

    impl FeatureRegistration {
        pub const fn new(
            _metadata: FeatureMetadata,
            _factory: fn(
                super::RouteMatch,
                &mut gpui::Window,
                &mut gpui::App,
            ) -> Result<super::FeatureInstance, super::FeatureRuntimeError>,
        ) -> Self {
            Self
        }
    }

    pub fn create_feature<T>(
        _route: super::RouteMatch,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
        _constructor: fn(&mut gpui::Window, &mut gpui::Context<T>) -> T,
    ) -> Result<super::FeatureInstance, super::FeatureRuntimeError> {
        Ok(super::FeatureInstance)
    }

    pub mod gpui {
        use std::marker::PhantomData;

        pub struct Window;
        pub struct App;

        pub struct Context<T>(PhantomData<T>);

        pub trait IntoElement {}

        pub trait Render: 'static + Sized {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
        }
    }
}

#[derive(Default, nexora_macros::Feature)]
#[nexora(
    title = "Employees",
    path = "/employees",
    visible_permissions(any = "employees:read")
)]
struct EmployeesFeature;

fn main() {}
