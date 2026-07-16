extern crate self as nexora;

use std::sync::atomic::{AtomicBool, Ordering};

trait LoginFeature: __private::gpui::Render + 'static {
    const REGISTRATION: __private::LoginFeatureRegistration;
}

trait Window: 'static {
    type Path;
    type Query;

    const METADATA: WindowMetadata;
    const REGISTRATION: Option<__private::WindowRegistration> = None;
}

trait SettingsWindow: Window {
    const REGISTRATION: __private::SettingsWindowRegistration;
}

trait WindowElement: Window + __private::gpui::Render + Sized {
    fn render(
        &mut self,
        window: &mut __private::gpui::Window,
        cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowMetadata {
    id: &'static str,
    title: &'static str,
    path: &'static str,
    icon: Option<&'static str>,
    order: i32,
}

impl WindowMetadata {
    const fn new(
        id: &'static str,
        title: &'static str,
        path: &'static str,
        icon: Option<&'static str>,
        order: i32,
    ) -> Self {
        Self {
            id,
            title,
            path,
            icon,
            order,
        }
    }
}

struct NoPath;
struct NoQuery;
struct RouteMatch;
struct WindowInstance;
struct WindowRuntimeError;

mod __private {
    pub use inventory;

    use super::WindowMetadata;

    pub type LoginFeatureFactory = fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView;

    #[derive(Clone, Copy)]
    pub struct LoginFeatureRegistration {
        type_name: &'static str,
        factory: LoginFeatureFactory,
    }

    impl LoginFeatureRegistration {
        pub const fn new(type_name: &'static str, factory: LoginFeatureFactory) -> Self {
            Self { type_name, factory }
        }

        pub const fn type_name(&self) -> &'static str {
            self.type_name
        }

        pub const fn factory(&self) -> LoginFeatureFactory {
            self.factory
        }
    }

    pub type WindowFactory = fn(
        super::RouteMatch,
        &mut gpui::Window,
        &mut gpui::App,
    ) -> Result<super::WindowInstance, super::WindowRuntimeError>;
    pub type WindowOptionsFactory = fn(
        &super::RouteMatch,
        &gpui::App,
    )
        -> Result<gpui::WindowOptions, super::WindowRuntimeError>;

    #[derive(Clone, Copy)]
    pub struct WindowRegistration {
        metadata: WindowMetadata,
        factory: WindowFactory,
        options_factory: WindowOptionsFactory,
    }

    impl WindowRegistration {
        pub const fn new(
            metadata: WindowMetadata,
            factory: WindowFactory,
            options_factory: WindowOptionsFactory,
        ) -> Self {
            Self {
                metadata,
                factory,
                options_factory,
            }
        }

        pub const fn new_settings(
            _type_name: &'static str,
            metadata: WindowMetadata,
            factory: WindowFactory,
            options_factory: WindowOptionsFactory,
        ) -> Self {
            Self::new(metadata, factory, options_factory)
        }

        pub const fn metadata(&self) -> WindowMetadata {
            self.metadata
        }

        pub const fn factory(&self) -> WindowFactory {
            self.factory
        }

        pub const fn options_factory(&self) -> WindowOptionsFactory {
            self.options_factory
        }
    }

    #[derive(Clone, Copy)]
    pub struct SettingsWindowRegistration {
        type_name: &'static str,
        window: WindowRegistration,
    }

    impl SettingsWindowRegistration {
        pub const fn new(type_name: &'static str, window: WindowRegistration) -> Self {
            Self { type_name, window }
        }

        pub const fn type_name(&self) -> &'static str {
            self.type_name
        }

        pub const fn window(&self) -> WindowRegistration {
            self.window
        }
    }

    inventory::collect!(LoginFeatureRegistration);
    inventory::collect!(SettingsWindowRegistration);
    inventory::collect!(WindowRegistration);

    pub fn create_login_feature<T>(
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
        constructor: fn(&mut gpui::Window, &mut gpui::Context<T>) -> T,
    ) -> gpui::AnyView
    where
        T: gpui::Render,
    {
        let mut context = gpui::Context::new();
        let _feature = constructor(window, &mut context);
        gpui::AnyView
    }

    pub fn create_window<T>(
        _route: super::RouteMatch,
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
        constructor: fn(&mut gpui::Window, &mut gpui::Context<T>) -> T,
    ) -> Result<super::WindowInstance, super::WindowRuntimeError>
    where
        T: super::WindowElement,
    {
        let mut context = gpui::Context::new();
        let _window = constructor(window, &mut context);
        Ok(super::WindowInstance)
    }

    pub fn window_options<T>(
        _route: &super::RouteMatch,
        _cx: &gpui::App,
    ) -> Result<gpui::WindowOptions, super::WindowRuntimeError>
    where
        T: super::WindowElement,
    {
        Ok(gpui::WindowOptions)
    }

    pub mod gpui {
        use std::marker::PhantomData;

        pub struct Window;
        pub struct App;
        pub struct AnyView;
        pub struct WindowOptions;
        pub struct Context<T>(PhantomData<T>);

        impl<T> Context<T> {
            pub const fn new() -> Self {
                Self(PhantomData)
            }
        }

        pub trait IntoElement {}

        pub trait Render: Sized + 'static {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
        }
    }
}

struct Element;

impl __private::gpui::IntoElement for Element {}

static LOGIN_CREATED: AtomicBool = AtomicBool::new(false);

#[derive(nexora_macros::LoginFeature)]
#[nexora(factory = CustomLogin::new)]
struct CustomLogin {
    rendered: bool,
}

impl CustomLogin {
    fn new(
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> Self {
        LOGIN_CREATED.store(true, Ordering::SeqCst);
        Self { rendered: false }
    }
}

impl __private::gpui::Render for CustomLogin {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        self.rendered = true;
        Element
    }
}

static SETTINGS_CREATED: AtomicBool = AtomicBool::new(false);

#[derive(nexora_macros::SettingsWindow)]
#[nexora(factory = CustomSettings::new)]
struct CustomSettings {
    rendered: bool,
}

impl CustomSettings {
    fn new(
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> Self {
        SETTINGS_CREATED.store(true, Ordering::SeqCst);
        Self { rendered: false }
    }
}

impl WindowElement for CustomSettings {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        self.rendered = true;
        Element
    }
}

#[test]
fn login_feature_derive_registers_direct_render_override_and_factory() {
    let registration = inventory::iter::<__private::LoginFeatureRegistration>
        .into_iter()
        .next()
        .expect("Login Feature derive 应提交用户覆盖注册");
    assert!(registration.type_name().ends_with("CustomLogin"));
    assert_eq!(
        <CustomLogin as LoginFeature>::REGISTRATION.type_name(),
        registration.type_name()
    );

    let mut window = __private::gpui::Window;
    let mut app = __private::gpui::App;
    let _view = (registration.factory())(&mut window, &mut app);
    assert!(LOGIN_CREATED.load(Ordering::SeqCst));

    let mut login = CustomLogin { rendered: false };
    let mut context = __private::gpui::Context::new();
    let _element = __private::gpui::Render::render(&mut login, &mut window, &mut context);
    drop(_element);
    assert!(login.rendered);
}

#[test]
fn settings_window_derive_uses_fixed_metadata_and_only_specialized_inventory() {
    assert_eq!(
        CustomSettings::METADATA,
        WindowMetadata::new("settings", "设置", "/settings", Some("settings"), 0)
    );
    assert!(<CustomSettings as Window>::REGISTRATION.is_some());
    assert_eq!(
        inventory::iter::<__private::WindowRegistration>
            .into_iter()
            .count(),
        0
    );

    let registration = inventory::iter::<__private::SettingsWindowRegistration>
        .into_iter()
        .next()
        .expect("Settings Window derive 应提交用户覆盖注册");
    assert!(registration.type_name().ends_with("CustomSettings"));
    assert_eq!(
        <CustomSettings as SettingsWindow>::REGISTRATION.type_name(),
        registration.type_name()
    );
    assert_eq!(registration.window().metadata(), CustomSettings::METADATA);

    let mut window = __private::gpui::Window;
    let mut app = __private::gpui::App;
    let _instance = (registration.window().factory())(RouteMatch, &mut window, &mut app);
    assert!(SETTINGS_CREATED.load(Ordering::SeqCst));
    let _options = (registration.window().options_factory())(&RouteMatch, &app);

    let mut settings = CustomSettings { rendered: false };
    let mut context = __private::gpui::Context::new();
    let _element = __private::gpui::Render::render(&mut settings, &mut window, &mut context);
    drop(_element);
    assert!(settings.rendered);
}
