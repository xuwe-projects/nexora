use desktop::{Application, ApplicationOptions};
use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Window, div, px, size};

#[derive(Default)]
struct TestApplication {
    options: ApplicationOptions,
}

impl Application for TestApplication {
    type RootView = TestView;

    fn options(&self) -> &ApplicationOptions {
        &self.options
    }

    fn options_mut(&mut self) -> &mut ApplicationOptions {
        &mut self.options
    }

    fn build_root_view(&mut self, _window: &mut Window, cx: &mut App) -> Entity<Self::RootView> {
        cx.new(|_| TestView)
    }
}

struct TestView;

impl Render for TestView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn with_window_size_stores_requested_size() {
    let app = TestApplication::default().with_window_size(900.0, 640.0);

    assert_eq!(app.options().window_size, Some(size(px(900.0), px(640.0))));
}

#[test]
fn with_window_min_size_stores_requested_min_size() {
    let app = TestApplication::default().with_window_min_size(400.0, 300.0);

    assert_eq!(
        app.options().window_min_size,
        Some(size(px(400.0), px(300.0)))
    );
}

#[test]
fn with_daemon_mode_stores_requested_mode() {
    let app = TestApplication::default().with_daemon_mode(true);

    assert!(app.options().daemon_mode);
}
