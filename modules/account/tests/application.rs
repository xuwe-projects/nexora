use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use account::{
    AccessProfile, AccountApplication, AccountError, AccountsStore, CreateRole, ExternalIdentity,
    Page, PageRequest, Permission, Role, StoreError, UpdateRole, User, UserStatus, permission,
};
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

#[tokio::test]
async fn suspended_user_is_rejected_after_identity_sync() {
    let mut profile = profile_with_permissions([]);
    profile.user.status = UserStatus::Suspended;
    let store = Arc::new(FakeStore::new(profile));
    let application = AccountApplication::new(store);

    let error = application
        .authenticate(&identity("member-subject"))
        .await
        .expect_err("停用用户不应获得授权快照");

    assert!(matches!(error, AccountError::UserSuspended));
}

#[tokio::test]
async fn super_administrator_binding_rejects_non_url_issuer() {
    let store = Arc::new(FakeStore::new(profile_with_permissions([])));
    let application = AccountApplication::new(store);
    let mut invalid_identity = identity("super-admin");
    invalid_identity.issuer = "not-an-issuer-url".to_owned();

    let error = application
        .bind_super_admin(&invalid_identity)
        .await
        .expect_err("无效 OIDC issuer 不应进入 store");

    assert!(matches!(error, AccountError::InvalidIdentity));
}

#[test]
fn permission_check_distinguishes_allowed_and_denied_operations() {
    let profile = profile_with_permissions([permission::ROLES_READ]);
    let application = AccountApplication::new(Arc::new(FakeStore::new(profile.clone())));

    assert!(
        application
            .require_permission(&profile, permission::ROLES_READ)
            .is_ok()
    );
    assert!(matches!(
        application.require_permission(&profile, permission::ROLES_WRITE),
        Err(AccountError::Forbidden(permission::ROLES_WRITE))
    ));
}

#[tokio::test]
async fn invalid_role_key_is_rejected_before_store_write() {
    let store = Arc::new(FakeStore::new(profile_with_permissions([])));
    let application = AccountApplication::new(store.clone());

    let error = application
        .create_role(CreateRole {
            key: "Invalid Key".to_owned(),
            name: "无效角色".to_owned(),
            description: None,
            permission_ids: Vec::new(),
        })
        .await
        .expect_err("无效角色键应当被拒绝");

    assert!(matches!(
        error,
        AccountError::InvalidInput(validation) if validation.field() == "key"
    ));
    assert_eq!(*store.create_role_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn user_pagination_validates_page_and_bounds_page_size() {
    let store = Arc::new(FakeStore::new(profile_with_permissions([])));
    let application = AccountApplication::new(store.clone());

    let error = application
        .list_users(0, 25)
        .await
        .expect_err("零页码应当在 application 边界被拒绝");
    assert!(matches!(
        error,
        AccountError::InvalidInput(validation) if validation.field() == "page"
    ));

    application
        .list_users(1, 0)
        .await
        .expect("零页大小应当按最小值一处理");
    assert_eq!(
        store
            .last_page_request
            .lock()
            .unwrap()
            .expect("store 应当收到分页请求")
            .size(),
        1
    );

    application
        .list_users(2, 1_000)
        .await
        .expect("过大页大小应当被限制");
    let request = store
        .last_page_request
        .lock()
        .unwrap()
        .expect("store 应当收到分页请求");
    assert_eq!(request.number(), 2);
    assert_eq!(request.size(), 100);
}

struct FakeStore {
    profile: AccessProfile,
    create_role_calls: Mutex<u32>,
    last_page_request: Mutex<Option<PageRequest>>,
}

impl FakeStore {
    fn new(profile: AccessProfile) -> Self {
        Self {
            profile,
            create_role_calls: Mutex::new(0),
            last_page_request: Mutex::new(None),
        }
    }
}

#[async_trait]
impl AccountsStore for FakeStore {
    async fn sync_identity(&self, _identity: &ExternalIdentity) -> Result<User, StoreError> {
        Ok(self.profile.user.clone())
    }

    async fn super_admin(&self) -> Result<Option<User>, StoreError> {
        Ok(self
            .profile
            .user
            .is_super_admin
            .then(|| self.profile.user.clone()))
    }

    async fn bind_super_admin(&self, _identity: &ExternalIdentity) -> Result<User, StoreError> {
        let mut user = self.profile.user.clone();
        user.is_super_admin = true;
        Ok(user)
    }

    async fn access_profile(&self, _user_id: Uuid) -> Result<AccessProfile, StoreError> {
        Ok(self.profile.clone())
    }

    async fn list_users(&self, request: PageRequest) -> Result<Page<User>, StoreError> {
        *self.last_page_request.lock().unwrap() = Some(request);
        Ok(Page::new(vec![self.profile.user.clone()], 1, request))
    }

    async fn user(&self, _user_id: Uuid) -> Result<User, StoreError> {
        Ok(self.profile.user.clone())
    }

    async fn set_user_status(
        &self,
        _user_id: Uuid,
        _status: UserStatus,
    ) -> Result<User, StoreError> {
        Ok(self.profile.user.clone())
    }

    async fn list_roles(&self) -> Result<Vec<Role>, StoreError> {
        Ok(self.profile.roles.clone())
    }

    async fn role(&self, _role_id: Uuid) -> Result<Role, StoreError> {
        self.profile
            .roles
            .first()
            .cloned()
            .ok_or(StoreError::NotFound("角色"))
    }

    async fn create_role(&self, _input: &CreateRole) -> Result<Role, StoreError> {
        *self.create_role_calls.lock().unwrap() += 1;
        Err(StoreError::NotFound("角色"))
    }

    async fn update_role(&self, _role_id: Uuid, _input: &UpdateRole) -> Result<Role, StoreError> {
        Err(StoreError::NotFound("角色"))
    }

    async fn delete_role(&self, _role_id: Uuid) -> Result<(), StoreError> {
        Err(StoreError::NotFound("角色"))
    }

    async fn replace_role_permissions(
        &self,
        _role_id: Uuid,
        _permission_ids: &[Uuid],
    ) -> Result<Role, StoreError> {
        Err(StoreError::NotFound("角色"))
    }

    async fn list_permissions(&self) -> Result<Vec<Permission>, StoreError> {
        Ok(Vec::new())
    }

    async fn replace_user_roles(
        &self,
        _user_id: Uuid,
        _role_ids: &[Uuid],
        _granted_by: Uuid,
    ) -> Result<AccessProfile, StoreError> {
        Ok(self.profile.clone())
    }
}

fn identity(subject: &str) -> ExternalIdentity {
    ExternalIdentity {
        issuer: "https://id.example.com/".to_owned(),
        subject: subject.to_owned(),
        email: Some("user@example.com".to_owned()),
        display_name: "测试用户".to_owned(),
        avatar_url: None,
    }
}

fn profile_with_permissions(permissions: impl IntoIterator<Item = &'static str>) -> AccessProfile {
    let now = Utc::now();
    AccessProfile {
        user: User {
            id: Uuid::now_v7(),
            issuer: "https://id.example.com/".to_owned(),
            subject: "member-subject".to_owned(),
            email: Some("user@example.com".to_owned()),
            display_name: "测试用户".to_owned(),
            avatar_url: None,
            status: UserStatus::Active,
            is_super_admin: false,
            created_at: now,
            updated_at: now,
            last_login_at: now,
        },
        roles: Vec::new(),
        permissions: permissions
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
    }
}
