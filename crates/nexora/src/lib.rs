//! Nexora 桌面全栈框架的公开入口。
//!
//! 该 crate 统一提供 Feature、独立窗口、路径路由、应用注册表和强类型配置能力。具体
//! 应用只需要声明自己的页面与配置类型，无需分别维护导航目录、deeplink 表和框架模块
//! 配置访问样板代码。

extern crate self as nexora;

#[cfg(feature = "account-server")]
extern crate account as account_module;

/// 提供按 Cargo feature 启用的 Account 桌面客户端与服务端组合能力。
#[cfg(any(feature = "account-client", feature = "account-server"))]
pub mod account;
/// 提供派生驱动的强类型配置加载、默认路径和模块配置校验。
pub mod config;

#[cfg(feature = "desktop")]
mod application;
#[cfg(feature = "desktop")]
mod metadata;
#[cfg(feature = "desktop")]
mod registry;
#[cfg(feature = "desktop")]
mod route;
#[cfg(feature = "desktop")]
mod runtime;

#[cfg(feature = "desktop")]
pub use application::{Application, ApplicationError, ApplicationOptions};
#[cfg(feature = "desktop")]
pub use gpui;
#[cfg(feature = "derive")]
pub use macros::Settings;
#[cfg(all(feature = "derive", feature = "desktop"))]
pub use macros::{Feature, SidebarFooter, SidebarHeader, Window};
#[cfg(feature = "desktop")]
pub use metadata::{
    Feature, FeatureMetadata, SidebarFooter, SidebarHeader, Window, WindowMetadata,
};
#[cfg(feature = "desktop")]
pub use registry::{AppRegistry, AppRegistryBuilder, RegistryError};
#[cfg(feature = "desktop")]
pub use route::{
    Path, Query, ResolveError, RouteExtractError, RouteMatch, RouteTarget, RouteTargetKind,
};
#[cfg(feature = "desktop")]
pub use runtime::{
    FeatureContextExt, FeatureElement, FeatureInstance, FeatureRoute, FeatureRuntimeError, NoPath,
    NoQuery,
};
#[cfg(feature = "desktop")]
pub use runtime::{
    NavigationContextExt, NavigationRequestError, WindowContextExt, WindowElement, WindowInstance,
    WindowRoute, WindowRuntimeError,
};
#[cfg(feature = "server")]
pub use {axum, sqlx, tokio, tracing};

/// 派生宏用于完成自动注册的内部兼容层。
///
/// 该模块不是稳定的应用开发 API；公开可见仅用于让下游 crate 展开的宏能够引用同一个
/// `inventory` 注册表，而无需使用方额外声明内部依赖。
#[doc(hidden)]
pub mod __private {
    pub use crate::config::__private::{
        ProvidesAccountClientSettings, ProvidesAccountServerSettings,
    };

    #[cfg(feature = "desktop")]
    pub use gpui;
    #[cfg(feature = "desktop")]
    pub use inventory;

    #[cfg(feature = "desktop")]
    use crate::{
        FeatureInstance, FeatureMetadata, FeatureRuntimeError, RouteMatch, WindowMetadata,
        runtime::{WindowInstance, WindowRuntimeError},
    };

    #[cfg(feature = "desktop")]
    pub use crate::runtime::{create_feature, create_sidebar_slot, create_window, window_options};

    /// 派生宏写入注册表的类型擦除 Feature 工厂。
    #[cfg(feature = "desktop")]
    pub type FeatureFactory = fn(
        RouteMatch,
        &mut gpui::Window,
        &mut gpui::App,
    ) -> Result<FeatureInstance, FeatureRuntimeError>;

    /// 一条由 `#[derive(Feature)]` 自动提交的静态注册记录。
    #[cfg(feature = "desktop")]
    #[derive(Debug, Clone, Copy)]
    pub struct FeatureRegistration {
        metadata: FeatureMetadata,
        factory: FeatureFactory,
    }

    #[cfg(feature = "desktop")]
    impl FeatureRegistration {
        /// 创建派生宏使用的 Feature 注册记录。
        pub const fn new(metadata: FeatureMetadata, factory: FeatureFactory) -> Self {
            Self { metadata, factory }
        }

        pub(crate) const fn metadata(&self) -> FeatureMetadata {
            self.metadata
        }

        pub(crate) const fn factory(&self) -> FeatureFactory {
            self.factory
        }
    }

    /// 派生宏写入 Sidebar Header/Footer 注册表的类型擦除 Entity 工厂。
    #[cfg(feature = "desktop")]
    pub type SidebarSlotFactory = fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView;

    /// 一条由 `#[derive(SidebarHeader)]` 自动提交的静态注册记录。
    #[cfg(feature = "desktop")]
    #[derive(Debug, Clone, Copy)]
    pub struct SidebarHeaderRegistration {
        type_name: &'static str,
        factory: SidebarSlotFactory,
    }

    #[cfg(feature = "desktop")]
    impl SidebarHeaderRegistration {
        /// 创建包含实现类型名称和 Entity 工厂的 Header 注册记录。
        pub const fn new(type_name: &'static str, factory: SidebarSlotFactory) -> Self {
            Self { type_name, factory }
        }

        pub(crate) const fn type_name(&self) -> &'static str {
            self.type_name
        }

        pub(crate) const fn factory(&self) -> SidebarSlotFactory {
            self.factory
        }
    }

    /// 一条由 `#[derive(SidebarFooter)]` 自动提交的静态注册记录。
    #[cfg(feature = "desktop")]
    #[derive(Debug, Clone, Copy)]
    pub struct SidebarFooterRegistration {
        type_name: &'static str,
        factory: SidebarSlotFactory,
    }

    #[cfg(feature = "desktop")]
    impl SidebarFooterRegistration {
        /// 创建包含实现类型名称和 Entity 工厂的 Footer 注册记录。
        pub const fn new(type_name: &'static str, factory: SidebarSlotFactory) -> Self {
            Self { type_name, factory }
        }

        pub(crate) const fn type_name(&self) -> &'static str {
            self.type_name
        }

        pub(crate) const fn factory(&self) -> SidebarSlotFactory {
            self.factory
        }
    }

    /// 派生宏写入注册表的类型擦除 Window Entity 工厂。
    #[cfg(feature = "desktop")]
    pub type WindowFactory = fn(
        RouteMatch,
        &mut gpui::Window,
        &mut gpui::App,
    ) -> Result<WindowInstance, WindowRuntimeError>;

    /// 派生宏写入注册表的 Window 原生选项工厂。
    #[cfg(feature = "desktop")]
    pub type WindowOptionsFactory =
        fn(&RouteMatch, &gpui::App) -> Result<gpui::WindowOptions, WindowRuntimeError>;

    /// 一条由 `#[derive(Window)]` 自动提交的静态注册记录。
    #[cfg(feature = "desktop")]
    #[derive(Debug, Clone, Copy)]
    pub struct WindowRegistration {
        metadata: WindowMetadata,
        factory: WindowFactory,
        options_factory: WindowOptionsFactory,
    }

    #[cfg(feature = "desktop")]
    impl WindowRegistration {
        /// 创建包含元数据、Entity 工厂和原生选项工厂的 Window 注册记录。
        pub const fn new(
            metadata: WindowMetadata,
            factory: WindowFactory,
            options_factory: WindowOptionsFactory,
        ) -> Self {
            Self {
                metadata,
                factory,
                options_factory,
            }
        }

        pub(crate) const fn metadata(&self) -> WindowMetadata {
            self.metadata
        }

        pub(crate) const fn factory(&self) -> WindowFactory {
            self.factory
        }

        pub(crate) const fn options_factory(&self) -> WindowOptionsFactory {
            self.options_factory
        }
    }

    #[cfg(feature = "desktop")]
    inventory::collect!(FeatureRegistration);
    #[cfg(feature = "desktop")]
    inventory::collect!(SidebarHeaderRegistration);
    #[cfg(feature = "desktop")]
    inventory::collect!(SidebarFooterRegistration);
    #[cfg(feature = "desktop")]
    inventory::collect!(WindowRegistration);
}
