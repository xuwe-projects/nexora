use std::error::Error as _;

use account::StoreError;

#[test]
fn database_operation_error_keeps_stage_and_sqlx_root_cause() {
    let error = StoreError::DatabaseOperation {
        stage: "system_initialization.mark_super_admin",
        source: sqlx::Error::TypeNotFound {
            type_name: "user_status".to_owned(),
        },
    };

    assert_eq!(
        error.to_string(),
        "数据库操作失败（阶段: system_initialization.mark_super_admin）"
    );
    assert_eq!(
        error.source().map(ToString::to_string).as_deref(),
        Some("type named user_status not found")
    );
}
