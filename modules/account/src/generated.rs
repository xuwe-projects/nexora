//! 由 gRPC 官方 Rust Protobuf 工具在构建期生成的内部 ZITADEL 线上契约。

pub(crate) mod zitadel {
    pub(crate) mod project {
        #[allow(dead_code, unused_imports, nonstandard_style, clippy::all)]
        pub(crate) mod v2 {
            grpc::include_proto!("zitadel/project/v2", "project_service");
        }
    }

    pub(crate) mod org {
        #[allow(dead_code, unused_imports, nonstandard_style, clippy::all)]
        pub(crate) mod v2 {
            grpc::include_proto!("zitadel/org/v2", "org_service");
        }
    }

    pub(crate) mod authorization {
        #[allow(dead_code, unused_imports, nonstandard_style, clippy::all)]
        pub(crate) mod v2 {
            grpc::include_proto!("zitadel/authorization/v2", "authorization_service");
        }
    }

    pub(crate) mod user {
        #[allow(dead_code, unused_imports, nonstandard_style, clippy::all)]
        pub(crate) mod v2 {
            grpc::include_proto!("zitadel/user/v2", "user_service");
        }
    }
}
