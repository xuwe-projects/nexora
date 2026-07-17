#[path = "../src/safety.rs"]
mod safety;

use safety::{DatabaseState, validate_migration_safety};

#[test]
fn empty_database_allows_first_installation() {
    let state = empty_state();

    assert!(validate_migration_safety(&state).is_ok());
}

#[test]
fn unmanaged_account_schema_is_never_initialized_implicitly() {
    let mut state = empty_state();
    state.account_schema_exists = true;

    assert!(validate_migration_safety(&state).is_err());
}

#[test]
fn failed_migration_record_stops_upgrade() {
    let mut state = complete_state();
    state.applied_migrations.push((4, false));

    assert!(validate_migration_safety(&state).is_err());
}

#[test]
fn missing_core_table_stops_existing_database_upgrade() {
    let mut state = complete_state();
    state.users_exists = false;

    assert!(validate_migration_safety(&state).is_err());
}

#[test]
fn missing_initialization_table_stops_version_three_upgrade() {
    let mut state = complete_state();
    state.system_initialization_exists = false;

    assert!(validate_migration_safety(&state).is_err());
}

#[test]
fn complete_existing_database_allows_forward_upgrade() {
    assert!(validate_migration_safety(&complete_state()).is_ok());
}

fn empty_state() -> DatabaseState {
    DatabaseState {
        applied_migrations: Vec::new(),
        account_schema_exists: false,
        users_exists: false,
        roles_exists: false,
        permissions_exists: false,
        role_permissions_exists: false,
        user_roles_exists: false,
        system_initialization_exists: false,
    }
}

fn complete_state() -> DatabaseState {
    DatabaseState {
        applied_migrations: vec![(1, true), (2, true), (3, true)],
        account_schema_exists: true,
        users_exists: true,
        roles_exists: true,
        permissions_exists: true,
        role_permissions_exists: true,
        user_roles_exists: true,
        system_initialization_exists: true,
    }
}
