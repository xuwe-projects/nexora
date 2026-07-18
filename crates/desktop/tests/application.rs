use std::borrow::Cow;

use desktop::{
    Application, ApplicationOptions, apply_window_display_preference, centered_window_bounds,
    find_display_id_by_uuid,
};
use gpui::{
    App, AppContext, AssetSource, Bounds, Context, Entity, IntoElement, Render, SharedString,
    TestAppContext, Window, div, point, px, size,
};

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

struct TestAssets;

impl AssetSource for TestAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match path {
            "icons/app.svg" => Ok(Some(Cow::Borrowed(b"<svg/>"))),
            _ => Ok(None),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(["icons/app.svg"]
            .into_iter()
            .filter(|asset| asset.starts_with(path))
            .map(Into::into)
            .collect())
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

#[test]
fn with_startup_display_uuid_stores_stable_identifier() {
    let app = TestApplication::default().with_startup_display_uuid("display-uuid");

    assert_eq!(
        app.options().startup_display_uuid.as_deref(),
        Some("display-uuid")
    );
}

#[test]
fn with_asset_source_stores_application_assets() {
    let app = TestApplication::default().with_asset_source(TestAssets);
    let assets = app
        .options()
        .application_assets
        .as_ref()
        .expect("应当保存应用资产源");

    assert_eq!(
        assets.load("icons/app.svg").unwrap().unwrap().as_ref(),
        b"<svg/>"
    );
    assert_eq!(
        assets.list("icons/").unwrap(),
        vec![SharedString::from("icons/app.svg")]
    );
}

#[test]
fn centers_window_on_secondary_display_with_positive_origin() {
    let display_bounds = Bounds::new(point(px(1920.0), px(24.0)), size(px(2560.0), px(1400.0)));

    assert_eq!(
        centered_window_bounds(display_bounds, size(px(900.0), px(640.0))),
        Bounds::new(point(px(2750.0), px(404.0)), size(px(900.0), px(640.0)),)
    );
}

#[test]
fn centers_window_on_secondary_display_with_negative_origin() {
    let display_bounds = Bounds::new(point(px(-1600.0), px(-900.0)), size(px(1600.0), px(860.0)));

    assert_eq!(
        centered_window_bounds(display_bounds, size(px(900.0), px(640.0))),
        Bounds::new(point(px(-1250.0), px(-790.0)), size(px(900.0), px(640.0)),)
    );
}

#[gpui::test]
fn display_uuid_resolves_current_display_and_rejects_unknown_value(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供主显示器");
        let uuid = display.uuid().expect("测试显示器应当提供稳定 UUID");

        assert_eq!(
            find_display_id_by_uuid(uuid.to_string().as_str(), cx),
            Some(display.id())
        );
        assert_eq!(find_display_id_by_uuid("missing-display", cx), None);
    });
}

#[gpui::test]
fn window_display_preference_centers_on_selected_display(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供主显示器");
        let uuid = display.uuid().expect("测试显示器应当提供稳定 UUID");
        let window_size = size(px(860.0), px(680.0));
        let mut options = gpui::WindowOptions::default();

        apply_window_display_preference(
            &mut options,
            Some(uuid.to_string().as_str()),
            Some(window_size),
            cx,
        );

        assert_eq!(options.display_id, Some(display.id()));
        assert_eq!(
            options
                .window_bounds
                .expect("应当生成窗口边界")
                .get_bounds(),
            centered_window_bounds(display.visible_bounds(), window_size)
        );
    });
}

#[gpui::test]
fn missing_window_display_preference_falls_back_to_primary_display(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let display = cx.primary_display().expect("测试平台应当提供主显示器");
        let window_size = size(px(860.0), px(680.0));
        let mut options = gpui::WindowOptions {
            display_id: Some(display.id()),
            ..Default::default()
        };

        apply_window_display_preference(
            &mut options,
            Some("missing-display"),
            Some(window_size),
            cx,
        );

        assert_eq!(options.display_id, None);
        assert_eq!(
            options
                .window_bounds
                .expect("应当生成回退窗口边界")
                .get_bounds(),
            centered_window_bounds(display.visible_bounds(), window_size)
        );
    });
}
