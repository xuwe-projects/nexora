use gpui::px;
use gpui_component::table::{Column, TableEvent};
use ui::{
    DataTableColumnLayout, DataTableLayout, DataTableLayoutError, DataTableLayoutKey,
    apply_data_table_layout, data_table_layout_from_event,
};

fn keys(columns: &[Column]) -> Vec<&str> {
    columns.iter().map(|column| column.key.as_ref()).collect()
}

#[test]
fn layout_restores_known_columns_and_appends_new_columns_in_code_order() {
    let mut columns = vec![
        Column::new("id", "ID"),
        Column::new("name", "名称"),
        Column::new("created_at", "创建时间"),
    ];
    let layout = DataTableLayout {
        columns: vec![
            DataTableColumnLayout {
                key: "name".to_owned(),
                width: 180.0,
            },
            DataTableColumnLayout {
                key: "deleted".to_owned(),
                width: 50.0,
            },
            DataTableColumnLayout {
                key: "id".to_owned(),
                width: 8.0,
            },
        ],
    };

    apply_data_table_layout(&mut columns, &layout).expect("有效布局应当恢复");

    assert_eq!(keys(&columns), ["name", "id", "created_at"]);
    assert_eq!(f32::from(columns[0].width), 180.0);
    assert_eq!(columns[1].width, columns[1].min_width);
}

#[test]
fn layout_keeps_fixed_and_non_movable_columns_in_constraint_slots() {
    let mut columns = vec![
        Column::new("selection", "")
            .fixed_left()
            .movable(false)
            .resizable(false)
            .width(px(42.0)),
        Column::new("name", "名称"),
        Column::new("id", "ID"),
        Column::new("actions", "操作").movable(false),
    ];
    let layout = DataTableLayout {
        columns: vec![
            DataTableColumnLayout {
                key: "actions".to_owned(),
                width: 300.0,
            },
            DataTableColumnLayout {
                key: "id".to_owned(),
                width: 90.0,
            },
            DataTableColumnLayout {
                key: "selection".to_owned(),
                width: 200.0,
            },
            DataTableColumnLayout {
                key: "name".to_owned(),
                width: 140.0,
            },
        ],
    };

    apply_data_table_layout(&mut columns, &layout).expect("约束列应当安全恢复");

    assert_eq!(keys(&columns), ["selection", "id", "name", "actions"]);
    assert_eq!(f32::from(columns[0].width), 42.0);
}

#[test]
fn duplicate_column_keys_return_clear_diagnostics_without_mutation() {
    let mut columns = vec![Column::new("id", "ID"), Column::new("id", "重复")];
    let original = keys(&columns)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();

    let error = apply_data_table_layout(&mut columns, &DataTableLayout::default())
        .expect_err("重复 key 必须失败");

    assert_eq!(
        error,
        DataTableLayoutError::DuplicateCurrentColumnKey {
            key: "id".to_owned()
        }
    );
    assert_eq!(keys(&columns), original);
}

#[test]
fn width_event_clamps_only_resizable_columns() {
    let mut columns = vec![
        Column::new("id", "ID")
            .min_width(px(50.0))
            .max_width(px(100.0)),
        Column::new("actions", "操作")
            .width(px(80.0))
            .resizable(false),
    ];
    let event = TableEvent::ColumnWidthsChanged(vec![px(200.0), px(300.0)]);

    let layout = data_table_layout_from_event(&mut columns, &event)
        .expect("宽度事件应当生成 patch")
        .expect("宽度事件必须有关联布局");

    assert_eq!(f32::from(columns[0].width), 100.0);
    assert_eq!(f32::from(columns[1].width), 80.0);
    assert_eq!(layout.columns[0].width, 100.0);
}

#[test]
fn composite_table_identity_rejects_display_or_path_values() {
    assert!(DataTableLayoutKey::new("users", "main").is_ok());
    assert!(DataTableLayoutKey::new(" 用户管理 ", "main").is_err());
    assert!(DataTableLayoutKey::new("users", "tables/main").is_err());
}
