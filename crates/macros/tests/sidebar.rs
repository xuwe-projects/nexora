extern crate self as nexora;

trait SidebarHeader: __private::gpui::Render + 'static {
    const REGISTRATION: __private::SidebarHeaderRegistration;
}

trait SidebarFooter: __private::gpui::Render + 'static {
    const REGISTRATION: __private::SidebarFooterRegistration;
}

mod __private {
    pub use inventory;

    pub type SidebarSlotFactory = fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView;

    #[derive(Clone, Copy)]
    pub struct SidebarHeaderRegistration {
        type_name: &'static str,
        factory: SidebarSlotFactory,
    }

    impl SidebarHeaderRegistration {
        pub const fn new(type_name: &'static str, factory: SidebarSlotFactory) -> Self {
            Self { type_name, factory }
        }

        pub const fn type_name(&self) -> &'static str {
            self.type_name
        }

        pub const fn factory(&self) -> SidebarSlotFactory {
            self.factory
        }
    }

    #[derive(Clone, Copy)]
    pub struct SidebarFooterRegistration {
        type_name: &'static str,
        factory: SidebarSlotFactory,
    }

    impl SidebarFooterRegistration {
        pub const fn new(type_name: &'static str, factory: SidebarSlotFactory) -> Self {
            Self { type_name, factory }
        }

        pub const fn type_name(&self) -> &'static str {
            self.type_name
        }

        pub const fn factory(&self) -> SidebarSlotFactory {
            self.factory
        }
    }

    inventory::collect!(SidebarHeaderRegistration);
    inventory::collect!(SidebarFooterRegistration);

    pub fn create_sidebar_slot<T>(
        window: &mut gpui::Window,
        _cx: &mut gpui::App,
        constructor: fn(&mut gpui::Window, &mut gpui::Context<T>) -> T,
    ) -> gpui::AnyView
    where
        T: gpui::Render,
    {
        let mut cx = gpui::Context::new();
        let _slot = constructor(window, &mut cx);
        gpui::AnyView
    }

    pub mod gpui {
        use std::marker::PhantomData;

        pub struct Window;
        pub struct App;
        pub struct AnyView;
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

#[derive(Default, nexora_macros::SidebarHeader)]
struct Header;

impl __private::gpui::Render for Header {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        Element
    }
}

#[derive(nexora_macros::SidebarFooter)]
#[nexora(factory = Footer::new)]
struct Footer;

impl Footer {
    fn new(
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> Self {
        Self
    }
}

impl __private::gpui::Render for Footer {
    fn render(
        &mut self,
        _window: &mut __private::gpui::Window,
        _cx: &mut __private::gpui::Context<Self>,
    ) -> impl __private::gpui::IntoElement {
        Element
    }
}

fn assert_header<T: SidebarHeader>() {}

fn assert_footer<T: SidebarFooter>() {}

#[test]
fn sidebar_derives_register_default_and_custom_factories_without_generating_render() {
    assert_header::<Header>();
    assert_footer::<Footer>();
    let associated_header = <Header as SidebarHeader>::REGISTRATION;
    let associated_footer = <Footer as SidebarFooter>::REGISTRATION;
    let header = inventory::iter::<__private::SidebarHeaderRegistration>
        .into_iter()
        .next()
        .expect("Header derive 应提交 inventory 注册");
    let footer = inventory::iter::<__private::SidebarFooterRegistration>
        .into_iter()
        .next()
        .expect("Footer derive 应提交 inventory 注册");

    assert!(header.type_name().ends_with("Header"));
    assert!(footer.type_name().ends_with("Footer"));
    assert_eq!(associated_header.type_name(), header.type_name());
    assert_eq!(associated_footer.type_name(), footer.type_name());
    let mut window = __private::gpui::Window;
    let mut app = __private::gpui::App;
    let _ = (associated_header.factory())(&mut window, &mut app);
    let _ = (associated_footer.factory())(&mut window, &mut app);
    let _ = (header.factory())(&mut window, &mut app);
    let _ = (footer.factory())(&mut window, &mut app);

    let mut header = Header;
    let mut context = __private::gpui::Context::new();
    let _ = __private::gpui::Render::render(&mut header, &mut window, &mut context);
}
