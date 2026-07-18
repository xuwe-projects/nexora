#[cfg(feature = "desktop")]
mod client {
    use std::{
        io::{Read as _, Write as _},
        net::TcpListener,
        thread,
    };

    use nexora::{
        config::Settings as _,
        desktop::{
            AccountClient, AccountOidcSettings, AccountSettings, ApiSettings, client_config,
            oidc_config,
        },
    };
    use serde::Deserialize;

    #[derive(Debug, Deserialize, nexora::Settings)]
    struct DesktopSettings {
        api: ApiSettings,
        #[nexora(account_client)]
        account: AccountSettings,
    }

    #[test]
    fn derived_desktop_settings_build_oidc_config() {
        let settings = DesktopSettings {
            api: ApiSettings {
                endpoint: "http://127.0.0.1:3000".to_owned(),
            },
            account: AccountSettings {
                oidc: AccountOidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    client_id: "desktop-client".to_owned(),
                    scopes: vec!["openid".to_owned(), "profile".to_owned()],
                    redirect_uri: "http://127.0.0.1:0/auth/callback".to_owned(),
                },
            },
        };

        settings.validate().expect("有效桌面配置应通过校验");
        let oidc = oidc_config(&settings).expect("派生配置应能创建 OIDC 配置");
        assert_eq!(oidc.client_id(), "desktop-client");
        let client = client_config(&settings, &settings.api)
            .expect("派生配置应能创建完整 Account 客户端配置");
        assert_eq!(client.api_endpoint().as_str(), "http://127.0.0.1:3000/");
    }

    #[test]
    fn client_config_rejects_remote_http_api_endpoint() {
        let settings = DesktopSettings {
            api: ApiSettings {
                endpoint: "http://api.example.com".to_owned(),
            },
            account: AccountSettings {
                oidc: AccountOidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    client_id: "desktop-client".to_owned(),
                    scopes: vec!["openid".to_owned()],
                    redirect_uri: "http://127.0.0.1:0/auth/callback".to_owned(),
                },
            },
        };

        assert!(settings.validate().is_ok());
        assert!(client_config(&settings, &settings.api).is_err());
    }

    #[test]
    fn account_client_applies_bearer_token_and_decodes_me_contract() {
        let body = r#"{
            "user": {
                "id": "User0001",
                "identity_id": "subject-1",
                "email": "user@example.com",
                "display_name": "测试用户",
                "avatar_url": null,
                "status": "active",
                "is_super_admin": false,
                "created_at": 1,
                "updated_at": 2,
                "last_login_at": 3
            },
            "roles": [],
            "permissions": []
        }"#;
        let listener = TcpListener::bind("127.0.0.1:0").expect("应能监听 loopback 测试端口");
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("客户端应连接测试服务");
            let mut request = [0_u8; 4096];
            let size = stream.read(&mut request).expect("应能读取测试请求");
            stream
                .write_all(response.as_bytes())
                .expect("应能写入测试响应");
            String::from_utf8_lossy(&request[..size]).into_owned()
        });
        let settings = DesktopSettings {
            api: ApiSettings { endpoint },
            account: AccountSettings {
                oidc: AccountOidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    client_id: "desktop-client".to_owned(),
                    scopes: vec!["openid".to_owned()],
                    redirect_uri: "http://127.0.0.1:0/auth/callback".to_owned(),
                },
            },
        };
        let config = client_config(&settings, &settings.api).expect("测试客户端配置应有效");
        let profile = AccountClient::new(&config)
            .expect("应能创建 Account 客户端")
            .session("access-token")
            .me()
            .expect("应能解析 /me 契约");
        let request = server.join().expect("测试服务线程应结束");

        assert_eq!(profile.user.identity_id, "subject-1");
        assert!(request.starts_with("GET /me HTTP/1.1\r\n"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer access-token\r\n")
        );
    }
}

#[cfg(feature = "server")]
mod server {
    use axum::{Router, extract::FromRef, http::StatusCode, routing::get};
    use nexora::{
        Server,
        config::Settings as _,
        server::{
            Account, AccountOidcSettings as OidcSettings, AccountSettings as Settings,
            AuthenticatedUser, Authorized, DirectoryUser, ExternalIdentity, PermissionDefinition,
            PermissionKey, RequiredPermission, Setup, SetupCompletionRequest, SetupUnlockRequest,
            create_permissions, create_role, create_user, create_user_with_roles, migrations,
            replace_role_permissions, replace_user_roles,
        },
    };
    use serde::Deserialize;

    #[derive(Debug, Deserialize, nexora::Settings)]
    struct ServerSettings {
        #[nexora(account_server)]
        account: Settings,
    }

    #[derive(Clone)]
    struct HostState {
        account: Account,
    }

    impl FromRef<HostState> for Account {
        fn from_ref(state: &HostState) -> Self {
            state.account.clone()
        }
    }

    struct ReadProjects;

    impl RequiredPermission for ReadProjects {
        const KEY: PermissionKey = PermissionKey::from_static("projects:read");
    }

    async fn authenticated_handler(_authenticated: AuthenticatedUser) -> StatusCode {
        StatusCode::OK
    }

    async fn authorized_handler(_authorization: Authorized<ReadProjects>) -> StatusCode {
        StatusCode::NO_CONTENT
    }

    #[test]
    fn derived_server_settings_validate_standard_account_section() {
        let settings = ServerSettings {
            account: Settings {
                oidc: OidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    audience: "nexora-api".to_owned(),
                    #[cfg(feature = "server")]
                    project_id: "project-1".to_owned(),
                    #[cfg(feature = "server")]
                    personal_access_token: "test-personal-access-token".to_owned(),
                },
            },
        };

        settings.validate().expect("有效服务端配置应通过校验");
    }

    #[test]
    fn server_settings_reject_empty_audience() {
        let settings = ServerSettings {
            account: Settings {
                oidc: OidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    audience: "  ".to_owned(),
                    #[cfg(feature = "server")]
                    project_id: "project-1".to_owned(),
                    #[cfg(feature = "server")]
                    personal_access_token: "test-personal-access-token".to_owned(),
                },
            },
        };

        assert!(settings.validate().is_err());
    }

    #[test]
    fn server_settings_allow_loopback_http_only_for_development() {
        let loopback = ServerSettings {
            account: Settings {
                oidc: OidcSettings {
                    issuer_url: "http://127.0.0.1:8080".to_owned(),
                    audience: "nexora-api".to_owned(),
                    #[cfg(feature = "server")]
                    project_id: "project-1".to_owned(),
                    #[cfg(feature = "server")]
                    personal_access_token: "test-personal-access-token".to_owned(),
                },
            },
        };
        let remote = ServerSettings {
            account: Settings {
                oidc: OidcSettings {
                    issuer_url: "http://identity.example.com".to_owned(),
                    audience: "nexora-api".to_owned(),
                    #[cfg(feature = "server")]
                    project_id: "project-1".to_owned(),
                    #[cfg(feature = "server")]
                    personal_access_token: "test-personal-access-token".to_owned(),
                },
            },
        };

        assert!(loopback.validate().is_ok());
        assert!(remote.validate().is_err());
    }

    #[test]
    fn nexora_facade_exposes_host_account_management_and_authorization() {
        fn assert_capabilities(account: &Account) {
            let definitions = [PermissionDefinition {
                key: "projects:read".to_owned(),
                name: "查看项目".to_owned(),
                description: None,
            }];
            _ = account.register_permissions(&definitions);
            _ = account.create_role("project-reader", "项目查看者", None, &[]);
            _ = account.permissions();
            _ = account.roles();
            _ = account.users(1, 20);
            _ = account.authorize("access-token", PermissionKey::from_static("projects:read"));
        }

        _ = assert_capabilities as fn(&Account);
    }

    #[test]
    fn server_account_extractors_accept_a_host_defined_state() {
        let router: Router<HostState> = Router::new()
            .route("/profile", get(authenticated_handler))
            .route("/projects", get(authorized_handler));
        drop(router);

        let server = Server::new();
        assert!(server.account().is_none());
    }

    #[test]
    fn server_facade_exposes_pool_based_account_management_and_migrations() {
        fn assert_management_api(pool: &sqlx::PgPool) {
            let identity = ExternalIdentity {
                identity_id: "identity-1".to_owned(),
                username: Some("tester".to_owned()),
                email: None,
                display_name: "测试用户".to_owned(),
                avatar_url: None,
            };
            let definitions = [PermissionDefinition {
                key: "projects:read".to_owned(),
                name: "查看项目".to_owned(),
                description: None,
            }];

            _ = create_user(pool, identity.clone());
            _ = create_user_with_roles(pool, identity, &[], "Admin001");
            _ = create_permissions(pool, &definitions);
            _ = create_role(pool, "project-reader", "项目查看者", None, &[]);
            _ = replace_role_permissions(pool, 1, &[]);
            _ = replace_user_roles(pool, "User0001", &[], "Admin001");
        }

        _ = assert_management_api as fn(&sqlx::PgPool);
        let migrations = migrations();
        assert!(!migrations.is_empty());
        assert!(
            migrations
                .iter()
                .any(|migration| migration.migration_type.is_up_migration())
        );
    }

    #[cfg(feature = "server")]
    #[test]
    fn zitadel_settings_require_project_and_pat_without_exposing_pat_in_debug() {
        let settings = ServerSettings {
            account: Settings {
                oidc: OidcSettings {
                    issuer_url: "https://identity.example.com".to_owned(),
                    audience: "nexora-api".to_owned(),
                    project_id: String::new(),
                    personal_access_token: "secret-personal-access-token".to_owned(),
                },
            },
        };

        assert!(settings.validate().is_err());
        let debug = format!("{:?}", settings.account.oidc);
        assert!(!debug.contains("secret-personal-access-token"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[cfg(feature = "server")]
    #[test]
    fn custom_setup_can_define_required_form_models_and_into_responses() {
        fn assert_setup<T: Setup>() {}

        assert_setup::<CustomSetup>();
    }

    #[cfg(feature = "server")]
    #[derive(Deserialize)]
    struct CustomUnlock {
        passphrase: String,
    }

    #[cfg(feature = "server")]
    impl SetupUnlockRequest for CustomUnlock {
        fn setup_secret(&self) -> &str {
            self.passphrase.as_str()
        }
    }

    #[cfg(feature = "server")]
    #[derive(Deserialize)]
    struct CustomCompletion {
        token: String,
        administrator: String,
    }

    #[cfg(feature = "server")]
    impl SetupCompletionRequest for CustomCompletion {
        fn setup_token(&self) -> &str {
            self.token.as_str()
        }

        fn super_admin_identity_id(&self) -> &str {
            self.administrator.as_str()
        }
    }

    #[cfg(feature = "server")]
    #[derive(Clone)]
    struct CustomSetup;

    #[cfg(feature = "server")]
    impl Setup for CustomSetup {
        type UnlockRequest = CustomUnlock;
        type CompletionRequest = CustomCompletion;

        fn unlock_response(&self, _error: Option<&str>) -> impl axum::response::IntoResponse {
            axum::http::StatusCode::OK
        }

        fn selection_response(
            &self,
            _users: &[DirectoryUser],
            _setup_token: &str,
        ) -> impl axum::response::IntoResponse {
            axum::http::StatusCode::OK
        }

        fn completed_response(
            &self,
            _super_admin: &DirectoryUser,
        ) -> impl axum::response::IntoResponse {
            axum::http::StatusCode::OK
        }

        fn error_response(&self) -> impl axum::response::IntoResponse {
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        }

        fn not_found_response(&self) -> impl axum::response::IntoResponse {
            axum::http::StatusCode::NOT_FOUND
        }
    }
}
