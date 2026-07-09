use actions::account::AccountActionKind;
use console::features::{
    FeatureId, feature_catalog,
    home::{next_steps, virtual_form_rows, virtual_form_view_modes},
    projects::project_rows,
    root::RootView,
    settings::setting_groups,
    tasks::task_rows,
    virtual_scroll::{
        virtual_scroll_default_row_count, virtual_scroll_stock_seeds, virtual_scroll_table_columns,
    },
};

#[test]
fn feature_catalog_has_stable_navigation_order() {
    let ids = feature_catalog()
        .iter()
        .map(|feature| feature.id())
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        vec![
            FeatureId::Home,
            FeatureId::Projects,
            FeatureId::Tasks,
            FeatureId::VirtualScroll,
            FeatureId::Reports,
            FeatureId::Analytics,
            FeatureId::Releases,
            FeatureId::Secrets,
            FeatureId::Integrations,
            FeatureId::AuditLogs,
            FeatureId::Team,
            FeatureId::Automation,
            FeatureId::Notifications,
            FeatureId::Billing,
            FeatureId::HelpCenter,
            FeatureId::Experiments,
            FeatureId::Settings,
        ]
    );
}

#[test]
fn feature_ids_expose_display_metadata() {
    assert_eq!(FeatureId::default(), FeatureId::Home);
    assert_eq!(FeatureId::Projects.title(), "项目");
    assert_eq!(FeatureId::Tasks.description(), "查看构建、打包和发布任务");
    assert_eq!(FeatureId::VirtualScroll.title(), "虚拟滚动");
}

#[test]
fn projects_navigation_exposes_child_routes() {
    let projects = feature_catalog()
        .iter()
        .find(|feature| feature.id() == FeatureId::Projects)
        .unwrap();
    let child_ids = projects
        .children()
        .iter()
        .map(|child| child.id())
        .collect::<Vec<_>>();

    assert_eq!(
        child_ids,
        vec![
            FeatureId::Projects,
            FeatureId::ProjectTemplates,
            FeatureId::ProjectEnvironments,
        ]
    );
    assert_eq!(projects.children()[1].title(), "模板项目");
    assert!(projects.contains(FeatureId::ProjectEnvironments));
}

#[test]
fn feature_catalog_includes_scroll_overflow_examples() {
    let overflow_items = feature_catalog()
        .iter()
        .filter(|feature| feature.section() == "扩展示例")
        .collect::<Vec<_>>();
    let overflow_ids = overflow_items
        .iter()
        .map(|feature| feature.id())
        .collect::<Vec<_>>();

    assert!(overflow_items.len() >= 12);
    assert_eq!(overflow_ids[0], FeatureId::VirtualScroll);
    assert_eq!(overflow_ids[8], FeatureId::Automation);
    assert_eq!(overflow_ids[12], FeatureId::Experiments);
}

#[test]
fn virtual_scroll_feature_uses_stock_table_shape() {
    let columns = virtual_scroll_table_columns();

    assert_eq!(virtual_scroll_default_row_count(), 5000);
    assert_eq!(
        &columns[..8],
        [
            "ID", "Market", "Name", "Symbol", "Price", "Chg", "Chg%", "Volume"
        ]
    );
    assert!(columns.contains(&"Market Cap"));
    assert!(columns.contains(&"Bid"));
    assert!(columns.len() >= 20);
    assert_eq!(virtual_scroll_stock_seeds()[0].symbol(), "AAPL");
    assert_eq!(virtual_scroll_stock_seeds()[0].market(), "US");
}

#[test]
fn home_next_steps_keep_template_order() {
    assert_eq!(
        next_steps(),
        [
            "把首页替换成真实工作台数据",
            "为项目、任务、设置补充独立 Entity",
            "把常用命令接入 actions 和快捷键",
        ]
    );
}

#[test]
fn home_virtual_form_keeps_table_sample_data() {
    let rows = virtual_form_rows();

    assert!(rows.len() >= 8);
    assert_eq!(rows[0].id(), "REQ-2401");
    assert_eq!(rows[0].owner(), "Jason Lee");
    assert_eq!(rows[0].status(), "待审核");
    assert_eq!(rows[0].priority(), "高");
    assert_eq!(rows[0].amount(), "$12,480");
    assert_eq!(
        virtual_form_view_modes(),
        ["全部记录", "只看待审核", "只看高优先级"]
    );
}

#[test]
fn project_rows_keep_template_projects() {
    let rows = project_rows();
    let names = rows.iter().map(|row| row.name()).collect::<Vec<_>>();

    assert_eq!(names, vec!["Console", "Desktop Runtime", "Xuwe CLI"]);
    assert_eq!(rows[0].status().label(), "active");
}

#[test]
fn task_rows_keep_pipeline_order() {
    let rows = task_rows();
    let commands = rows.iter().map(|row| row.command()).collect::<Vec<_>>();

    assert_eq!(
        commands,
        vec![
            "cargo check --workspace",
            "xuwecli build --mode local",
            "codesign + notarytool",
            "sha256 sidecar",
        ]
    );
    assert_eq!(rows[2].status().label(), "blocked");
}

#[test]
fn setting_groups_keep_template_configuration() {
    let groups = setting_groups();
    let titles = groups.iter().map(|group| group.title()).collect::<Vec<_>>();

    assert_eq!(titles, vec!["窗口", "运行", "发布"]);
    assert_eq!(groups[0].items()[0], ("默认尺寸", "900 x 640"));
}

#[test]
fn root_view_defaults_to_home_feature() {
    let view = RootView::new();

    assert_eq!(view.active_feature(), FeatureId::Home);
    assert_eq!(view.account_display_name(), "Jason Lee");
    assert_eq!(view.account_plan_label(), "Free");
}

#[test]
fn root_view_exposes_account_menu_actions() {
    let view = RootView::new();
    let actions = view.account_menu_actions();

    assert_eq!(
        actions
            .iter()
            .map(|action| action.label())
            .collect::<Vec<_>>(),
        vec!["Jason Lee", "设置", "退出登录"]
    );
    assert_eq!(actions[0].kind(), AccountActionKind::Profile);
    assert_eq!(actions[0].shortcut(), Some("Cmd+Shift+P"));
    assert!(actions[0].uses_account_avatar());
    assert_eq!(actions[1].kind(), AccountActionKind::Settings);
    assert_eq!(actions[1].shortcut(), Some("Cmd+,"));
    assert_eq!(actions[2].kind(), AccountActionKind::SignOut);
    assert_eq!(actions[2].shortcut(), Some("Cmd+Shift+Q"));
}

#[test]
fn root_view_can_select_active_feature() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);

    assert_eq!(view.active_feature(), FeatureId::Tasks);
}

#[test]
fn root_view_tracks_opened_tabs_without_duplicates() {
    let mut view = RootView::new();

    assert_eq!(view.opened_tabs(), &[FeatureId::Home]);

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.select_feature(FeatureId::Tasks);

    assert_eq!(view.active_feature(), FeatureId::Tasks);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Home, FeatureId::Tasks, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_tab_index(), Some(1));
}

#[test]
fn root_view_navigates_browser_like_history() {
    let mut view = RootView::new();

    assert_eq!(view.navigation_history(), &[FeatureId::Home]);
    assert!(!view.can_navigate_back());
    assert!(!view.can_navigate_forward());

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);

    assert_eq!(
        view.navigation_history(),
        &[FeatureId::Home, FeatureId::Tasks, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_feature(), FeatureId::VirtualScroll);
    assert!(view.can_navigate_back());
    assert!(!view.can_navigate_forward());

    view.navigate_back();
    assert_eq!(view.active_feature(), FeatureId::Tasks);
    assert!(view.can_navigate_back());
    assert!(view.can_navigate_forward());

    view.navigate_back();
    assert_eq!(view.active_feature(), FeatureId::Home);
    assert!(!view.can_navigate_back());
    assert!(view.can_navigate_forward());

    view.navigate_forward();
    assert_eq!(view.active_feature(), FeatureId::Tasks);
    assert!(view.can_navigate_back());
    assert!(view.can_navigate_forward());

    view.select_feature(FeatureId::Settings);
    assert_eq!(
        view.navigation_history(),
        &[FeatureId::Home, FeatureId::Tasks, FeatureId::Settings]
    );
    assert_eq!(view.active_feature(), FeatureId::Settings);
    assert!(!view.can_navigate_forward());
}

#[test]
fn root_view_closes_tabs_with_active_fallback() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.close_tab(FeatureId::Tasks);

    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Home, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_feature(), FeatureId::VirtualScroll);

    view.close_tab(FeatureId::VirtualScroll);
    assert_eq!(view.opened_tabs(), &[FeatureId::Home]);
    assert_eq!(view.active_feature(), FeatureId::Home);

    view.close_tab(FeatureId::Home);
    assert_eq!(view.opened_tabs(), &[FeatureId::Home]);
    assert_eq!(view.active_feature(), FeatureId::Home);
}

#[test]
fn root_view_bulk_closes_tabs_while_preserving_pinned_tabs() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Projects);
    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::Projects);

    assert_eq!(view.pinned_tabs(), &[FeatureId::Projects]);
    assert_eq!(
        view.opened_tabs(),
        &[
            FeatureId::Projects,
            FeatureId::Home,
            FeatureId::Tasks,
            FeatureId::VirtualScroll,
        ]
    );

    view.close_tabs_to_left(FeatureId::VirtualScroll);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Projects, FeatureId::VirtualScroll]
    );
    assert_eq!(view.active_feature(), FeatureId::VirtualScroll);

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::Settings);
    view.close_tabs_to_right(FeatureId::Tasks);
    assert_eq!(
        view.opened_tabs(),
        &[
            FeatureId::Projects,
            FeatureId::VirtualScroll,
            FeatureId::Tasks
        ]
    );

    view.close_other_tabs(FeatureId::Tasks);
    assert_eq!(view.opened_tabs(), &[FeatureId::Projects, FeatureId::Tasks]);
    assert_eq!(view.active_feature(), FeatureId::Tasks);
}

#[test]
fn root_view_toggles_pinned_tabs_at_the_front() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::Tasks);

    assert!(view.is_tab_pinned(FeatureId::VirtualScroll));
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::VirtualScroll, FeatureId::Tasks, FeatureId::Home]
    );
    assert_eq!(
        view.pinned_tabs(),
        &[FeatureId::VirtualScroll, FeatureId::Tasks]
    );

    view.toggle_pin_tab(FeatureId::VirtualScroll);
    assert_eq!(
        view.opened_tabs(),
        &[FeatureId::Tasks, FeatureId::VirtualScroll, FeatureId::Home]
    );
    assert_eq!(view.pinned_tabs(), &[FeatureId::Tasks]);
}

#[test]
fn root_view_keeps_pinned_tabs_out_of_regular_scroll_tabs() {
    let mut view = RootView::new();

    view.select_feature(FeatureId::Projects);
    view.select_feature(FeatureId::Tasks);
    view.select_feature(FeatureId::VirtualScroll);
    view.toggle_pin_tab(FeatureId::Projects);
    view.toggle_pin_tab(FeatureId::VirtualScroll);

    assert_eq!(
        view.pinned_tabs(),
        &[FeatureId::Projects, FeatureId::VirtualScroll]
    );
    assert_eq!(view.regular_tabs(), &[FeatureId::Home, FeatureId::Tasks]);

    view.toggle_pin_tab(FeatureId::Projects);

    assert_eq!(view.pinned_tabs(), &[FeatureId::VirtualScroll]);
    assert_eq!(
        view.regular_tabs(),
        &[FeatureId::Projects, FeatureId::Home, FeatureId::Tasks]
    );
}
