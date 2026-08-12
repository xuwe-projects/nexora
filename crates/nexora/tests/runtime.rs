#![cfg(all(feature = "desktop", feature = "derive"))]

use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{
    AnyView, AppContext as _, Context, Empty, Render, TestAppContext, Window, WindowHandle,
};
use nexora::__private::{install_navigation_handler, remove_navigation_handler};
use nexora::{
    AppRegistry, FeatureContextExt as _, FeatureElement, FeatureRoute, FeatureRuntimeError,
    NavigationContextExt as _, NavigationRequestError, Path, Query, RouteExtractError,
    WindowContextExt as _, WindowElement, WindowRuntimeError,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct RuntimePath {
    id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RuntimeQuery {
    tab: Option<String>,
    page: Option<u64>,
}

#[derive(Default, nexora::Feature)]
#[nexora(
    title = "运行时测试",
    path = "/runtime/:id",
    path_params = RuntimePath,
    query_params = RuntimeQuery,
    navigation = false
)]
struct RuntimeFeature {
    id: u64,
    tab: Option<String>,
    page: Option<u64>,
    events: Vec<&'static str>,
    previous_tab: Option<String>,
    overlay: Option<AnyView>,
}

#[derive(Default)]
struct RuntimeOverlay;

impl Render for RuntimeOverlay {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }
}

#[derive(Default)]
struct NotificationObserver {
    notifications: usize,
}

#[derive(Default)]
struct NavigationSource;

impl Render for NavigationSource {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }
}

#[gpui::test]
fn navigation_reports_missing_application_shell(cx: &mut TestAppContext) {
    let source = cx.new(|_| NavigationSource);

    let error = cx
        .update_entity(&source, |_, cx| cx.navigate("/runtime/42"))
        .unwrap_err();

    assert_eq!(error, NavigationRequestError::DispatcherUnavailable);
}

fn install_test_navigation_handler(
    window: WindowHandle<NavigationSource>,
    main_window: bool,
    label: &'static str,
    events: Rc<RefCell<Vec<(String, String)>>>,
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        install_navigation_handler(
            window.window_id(),
            main_window,
            move |location, _| {
                events.borrow_mut().push((label.to_owned(), location));
            },
            cx,
        );
    });
}

#[gpui::test]
fn navigation_targets_source_shell_and_falls_back_to_main(cx: &mut TestAppContext) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let main_window = cx.add_window(|_, _| NavigationSource);
    let extra_window = cx.add_window(|_, _| NavigationSource);
    let main_source = main_window.root(cx).unwrap();
    let extra_source = extra_window.root(cx).unwrap();
    install_test_navigation_handler(main_window, true, "main", events.clone(), cx);
    install_test_navigation_handler(extra_window, false, "extra", events.clone(), cx);

    cx.update_entity(&main_source, |_, cx| cx.navigate("/from-main").unwrap());
    cx.update_entity(&extra_source, |_, cx| cx.navigate("/from-extra").unwrap());
    cx.run_until_parked();

    assert_eq!(
        events.borrow().as_slice(),
        [
            ("main".to_owned(), "/from-main".to_owned()),
            ("extra".to_owned(), "/from-extra".to_owned()),
        ]
    );

    cx.update(|cx| remove_navigation_handler(extra_window.window_id(), cx));
    cx.update_entity(&extra_source, |_, cx| {
        cx.navigate("/fallback-to-main").unwrap()
    });
    cx.run_until_parked();

    assert_eq!(
        events.borrow().last(),
        Some(&("main".to_owned(), "/fallback-to-main".to_owned()))
    );
}

#[gpui::test]
fn app_navigation_targets_active_shell(cx: &mut TestAppContext) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let main_window = cx.add_window(|_, _| NavigationSource);
    let extra_window = cx.add_window(|_, _| NavigationSource);
    install_test_navigation_handler(main_window, true, "main", events.clone(), cx);
    install_test_navigation_handler(extra_window, false, "extra", events.clone(), cx);
    extra_window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();

    cx.update(|cx| cx.navigate("/from-app").unwrap());
    cx.run_until_parked();

    assert_eq!(
        events.borrow().as_slice(),
        [("extra".to_owned(), "/from-app".to_owned())]
    );
}

impl FeatureElement for RuntimeFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }

    fn panel_overlay(&self) -> Option<AnyView> {
        self.overlay.clone()
    }

    fn initialize(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Path(path) = cx.path();
        let Query(query) = cx.query();
        self.id = path.id;
        self.tab = query.tab;
        self.page = query.page;
        self.events.push("initialize");
        self.overlay = Some(cx.new(|_| RuntimeOverlay).into());
    }

    fn activated(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.events.push("activated");
    }

    fn deactivated(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.events.push("deactivated");
    }

    fn route_changed(
        &mut self,
        previous: &FeatureRoute<Self::Path, Self::Query>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.previous_tab = previous.query().tab.clone();
        self.tab = cx.query().tab.clone();
        self.events.push("route_changed");
    }

    fn closing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.events.push("closing");
    }
}

#[gpui::test]
fn runtime_creates_typed_entity_and_dispatches_lifecycle(cx: &mut TestAppContext) {
    let registry = AppRegistry::builder()
        .feature::<RuntimeFeature>()
        .build()
        .unwrap();
    let initial_route = registry.resolve("/runtime/42?tab=summary").unwrap();
    let window = cx.add_window(|_, _| Empty);
    let mut instance = window
        .update(cx, |_, window, cx| {
            registry.create_feature(initial_route, window, cx)
        })
        .unwrap()
        .unwrap();
    let feature = instance.view().downcast::<RuntimeFeature>().unwrap();
    let overlay = window
        .update(cx, |_, _, cx| instance.panel_overlay(cx).unwrap())
        .unwrap();
    let same_overlay = window
        .update(cx, |_, _, cx| instance.panel_overlay(cx).unwrap())
        .unwrap();
    assert_eq!(overlay.entity_id(), same_overlay.entity_id());
    let observer = cx.new(|cx| {
        cx.observe(&feature, |observer: &mut NotificationObserver, _, _| {
            observer.notifications += 1;
        })
        .detach();
        NotificationObserver::default()
    });

    assert_eq!(cx.read_entity(&feature, |feature, _| feature.id), 42);
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.tab.clone()),
        Some("summary".to_owned())
    );
    assert_eq!(cx.read_entity(&feature, |feature, _| feature.page), None);
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        ["initialize"]
    );

    window
        .update(cx, |_, window, cx| instance.activate(window, cx))
        .unwrap();
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        ["initialize", "activated"]
    );

    let updated_route = registry.resolve("/runtime/42?tab=roles").unwrap();
    window
        .update(cx, |_, window, cx| {
            instance.update_route(updated_route, window, cx)
        })
        .unwrap()
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.tab.clone()),
        Some("roles".to_owned())
    );
    assert_eq!(
        cx.update_entity(&feature, |_, cx| cx.query().tab.clone()),
        Some("roles".to_owned())
    );
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.previous_tab.clone()),
        Some("summary".to_owned())
    );
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        ["initialize", "activated", "route_changed"]
    );
    assert_eq!(
        cx.read_entity(&observer, |observer, _| observer.notifications),
        1
    );

    let duplicate_route = registry.resolve("/runtime/42?tab=roles").unwrap();
    window
        .update(cx, |_, window, cx| {
            instance.update_route(duplicate_route, window, cx)
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        ["initialize", "activated", "route_changed"]
    );

    let invalid_route = registry.resolve("/runtime/42?page=invalid").unwrap();
    let error = window
        .update(cx, |_, window, cx| {
            instance.update_route(invalid_route, window, cx)
        })
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        error,
        FeatureRuntimeError::Extract(RouteExtractError::Query { field, .. }) if field == "page"
    ));
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.tab.clone()),
        Some("roles".to_owned())
    );
    assert_eq!(
        cx.update_entity(&feature, |_, cx| cx.query().tab.clone()),
        Some("roles".to_owned())
    );
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        ["initialize", "activated", "route_changed"]
    );

    window
        .update(cx, |_, window, cx| instance.close(window, cx))
        .unwrap();
    window
        .update(cx, |_, window, cx| {
            instance.close(window, cx);
            instance.activate(window, cx);
            instance.deactivate(window, cx);
        })
        .unwrap();
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        [
            "initialize",
            "activated",
            "route_changed",
            "deactivated",
            "closing"
        ]
    );

    let route_after_close = registry.resolve("/runtime/42?tab=closed").unwrap();
    let error = window
        .update(cx, |_, window, cx| {
            instance.update_route(route_after_close, window, cx)
        })
        .unwrap()
        .unwrap_err();
    assert!(matches!(error, FeatureRuntimeError::ClosedInstance { .. }));
    assert_eq!(
        cx.read_entity(&feature, |feature, _| feature.events.clone()),
        [
            "initialize",
            "activated",
            "route_changed",
            "deactivated",
            "closing"
        ]
    );
}

static WINDOW_CONSTRUCTOR_CALLS: AtomicUsize = AtomicUsize::new(0);
static WINDOW_INITIALIZE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Deserialize)]
struct RuntimeWindowPath {
    id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct RuntimeWindowQuery {
    tab: String,
}

#[derive(nexora::Window)]
#[nexora(
    title = "运行时窗口",
    path = "/runtime-window/:id",
    path_params = RuntimeWindowPath,
    query_params = RuntimeWindowQuery,
    factory = create_runtime_window
)]
struct RuntimeWindow {
    id: u64,
    tab: String,
}

fn create_runtime_window(_window: &mut Window, _cx: &mut Context<RuntimeWindow>) -> RuntimeWindow {
    WINDOW_CONSTRUCTOR_CALLS.fetch_add(1, Ordering::Relaxed);
    RuntimeWindow {
        id: 0,
        tab: String::new(),
    }
}

impl WindowElement for RuntimeWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }

    fn initialize(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Path(path) = cx.path();
        let Query(query) = cx.query();
        self.id = path.id;
        self.tab = query.tab;
        WINDOW_INITIALIZE_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

#[gpui::test]
fn window_runtime_validates_typed_route_before_factory_and_dispatches_lifecycle(
    cx: &mut TestAppContext,
) {
    WINDOW_CONSTRUCTOR_CALLS.store(0, Ordering::Relaxed);
    WINDOW_INITIALIZE_CALLS.store(0, Ordering::Relaxed);
    let registry = AppRegistry::builder()
        .window::<RuntimeWindow>()
        .build()
        .unwrap();
    let window = cx.add_window(|_, _| Empty);

    let invalid = registry
        .resolve("/runtime-window/invalid?tab=summary")
        .unwrap();
    let error = window
        .update(cx, |_, window, cx| {
            registry.create_window(invalid, window, cx)
        })
        .unwrap()
        .err()
        .unwrap();
    assert!(matches!(
        error,
        WindowRuntimeError::Extract(RouteExtractError::Path { field, .. }) if field == "id"
    ));
    assert_eq!(WINDOW_CONSTRUCTOR_CALLS.load(Ordering::Relaxed), 0);

    let route = registry.resolve("/runtime-window/42?tab=profile").unwrap();
    let instance = window
        .update(cx, |_, window, cx| {
            registry.create_window(route, window, cx)
        })
        .unwrap()
        .unwrap();
    let entity = instance.view().downcast::<RuntimeWindow>().unwrap();

    assert_eq!(instance.route().concrete_path(), "/runtime-window/42");
    assert_eq!(cx.read_entity(&entity, |window, _| window.id), 42);
    assert_eq!(
        cx.read_entity(&entity, |window, _| window.tab.clone()),
        "profile"
    );
    assert_eq!(WINDOW_CONSTRUCTOR_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(WINDOW_INITIALIZE_CALLS.load(Ordering::Relaxed), 1);

    drop(instance);
    drop(entity);
}

#[gpui::test]
fn registered_windows_share_one_gpui_application_loop(cx: &mut TestAppContext) {
    let first = cx.add_window(|_, _| Empty);
    let second = cx.add_window(|_, _| Empty);

    assert_eq!(cx.windows().len(), 2);
    first
        .update(cx, |_, window, _| window.remove_window())
        .expect("第一个窗口应能独立关闭");
    assert_eq!(cx.windows().len(), 1);
    second
        .update(cx, |_, window, _| window.remove_window())
        .expect("第二个窗口应能独立关闭");
}

static CONSTRUCTOR_CALLS: AtomicUsize = AtomicUsize::new(0);
static INITIALIZE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Deserialize)]
struct ValidatedPath {
    id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ValidatedQuery {
    page: u64,
}

#[derive(nexora::Feature)]
#[nexora(
    title = "创建前校验",
    path = "/validated/:id",
    path_params = ValidatedPath,
    query_params = ValidatedQuery,
    navigation = false,
    factory = create_validated_feature
)]
struct ValidatedFeature;

fn create_validated_feature(
    _window: &mut Window,
    _cx: &mut Context<ValidatedFeature>,
) -> ValidatedFeature {
    CONSTRUCTOR_CALLS.fetch_add(1, Ordering::Relaxed);
    ValidatedFeature
}

impl FeatureElement for ValidatedFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }

    fn initialize(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let path = cx.path();
        let query = cx.query();
        assert_eq!(path.id, 7);
        assert_eq!(query.page, 2);
        INITIALIZE_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

#[gpui::test]
fn typed_route_is_validated_before_entity_factory_runs(cx: &mut TestAppContext) {
    CONSTRUCTOR_CALLS.store(0, Ordering::Relaxed);
    INITIALIZE_CALLS.store(0, Ordering::Relaxed);
    let registry = AppRegistry::builder()
        .feature::<ValidatedFeature>()
        .build()
        .unwrap();
    let window = cx.add_window(|_, _| Empty);

    let invalid_path = registry.resolve("/validated/not-a-number?page=2").unwrap();
    let error = window
        .update(cx, |_, window, cx| {
            registry.create_feature(invalid_path, window, cx)
        })
        .unwrap()
        .err()
        .unwrap();
    assert!(matches!(
        error,
        FeatureRuntimeError::Extract(RouteExtractError::Path { field, .. }) if field == "id"
    ));

    let invalid_query = registry.resolve("/validated/7?page=invalid").unwrap();
    let error = window
        .update(cx, |_, window, cx| {
            registry.create_feature(invalid_query, window, cx)
        })
        .unwrap()
        .err()
        .unwrap();
    assert!(matches!(
        error,
        FeatureRuntimeError::Extract(RouteExtractError::Query { field, .. }) if field == "page"
    ));
    assert_eq!(CONSTRUCTOR_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(INITIALIZE_CALLS.load(Ordering::Relaxed), 0);

    let valid_route = registry.resolve("/validated/7?page=2").unwrap();
    let instance = window
        .update(cx, |_, window, cx| {
            registry.create_feature(valid_route, window, cx)
        })
        .unwrap()
        .unwrap();
    assert_eq!(CONSTRUCTOR_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(INITIALIZE_CALLS.load(Ordering::Relaxed), 1);
    drop(instance);
}

#[derive(Default, nexora::Feature)]
#[nexora(
    id = "runtime",
    title = "其他注册表中的同名页面",
    path = "/foreign/:id",
    path_params = RuntimePath,
    navigation = false
)]
struct ForeignRuntimeFeature;

impl FeatureElement for ForeignRuntimeFeature {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl gpui::IntoElement {
        Empty
    }
}

#[gpui::test]
fn registry_rejects_same_id_route_from_another_registry(cx: &mut TestAppContext) {
    let runtime_registry = AppRegistry::builder()
        .feature::<RuntimeFeature>()
        .build()
        .unwrap();
    let foreign_registry = AppRegistry::builder()
        .feature::<ForeignRuntimeFeature>()
        .build()
        .unwrap();
    let foreign_route = foreign_registry.resolve("/foreign/42").unwrap();
    let window = cx.add_window(|_, _| Empty);

    let error = window
        .update(cx, |_, window, cx| {
            runtime_registry.create_feature(foreign_route, window, cx)
        })
        .unwrap()
        .err()
        .unwrap();

    assert!(matches!(
        error,
        FeatureRuntimeError::FeatureTargetMismatch {
            expected: "runtime",
            expected_path: "/runtime/:id",
            actual: "runtime",
            actual_path: "/foreign/:id",
            path,
        } if path == "/foreign/42"
    ));
}
