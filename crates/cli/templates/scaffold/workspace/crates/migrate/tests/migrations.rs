use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn framework_and_application_migrations_use_independent_histories(pool: PgPool) {
    nexora::server::migrate(&pool)
        .await
        .expect("Nexora 框架迁移应当成功");
    migrate::migrate(&pool)
        .await
        .expect("应用业务迁移应当成功");

    let histories = sqlx::query_as::<_, (bool, bool)>(
        r#"
        SELECT
            to_regclass('nexora._sqlx_migrations') IS NOT NULL,
            to_regclass('public._sqlx_migrations') IS NOT NULL
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("应当可以检查独立迁移历史表");
    assert_eq!(histories, (true, true));
}
