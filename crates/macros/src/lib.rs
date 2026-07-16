//! Nexora 面向 Feature、独立窗口、Sidebar 插槽与强类型应用配置的派生宏。
//!
//! Feature 与 Window 宏把类型属性转换成静态元数据并提交给 Nexora 运行时；Settings
//! 宏在调用方 crate 中记录应用包名，并生成框架模块所需的配置段访问和校验能力。

use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Error, Expr, ExprGroup, ExprLit, ExprParen, ExprPath, ExprUnary,
    Fields, Ident, Lit, LitBool, LitInt, LitStr, Result, Type, UnOp, parse_macro_input,
    spanned::Spanned as _,
};

/// 为类型生成 Nexora Feature 元数据实现。
///
/// 调用方必须通过 `#[nexora(title = "...", path = "/...")]` 提供标题和路由路径，
/// 还可以配置 `id`、`section`、`icon`、`parent`、`order`、`navigation` 与
/// `content_scrollable`。动态路径使用
/// `:name` 声明参数，并且必须显式设置 `path_params = SomeType` 与
/// `navigation = false`。查询参数类型可以通过 `query_params = SomeType` 声明；不能实现
/// [`Default`] 的 Feature 可以通过 `factory = SomeType::new` 提供构造函数。
/// 自行管理滚动视口的页面应设置 `content_scrollable = false`，以关闭 Shell 的外层滚动。
/// 宏会生成 `gpui::Render` 实现，并把调用转发到 `nexora::FeatureElement::render`；页面
/// 只需要实现 `nexora::FeatureElement`，不应再手写第二份 `gpui::Render` 实现。
#[proc_macro_derive(Feature, attributes(nexora))]
pub fn derive_feature(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_feature(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// 为类型生成 Nexora 独立窗口元数据实现。
///
/// 调用方必须通过 `#[nexora(title = "...", path = "/...")]` 提供标题和路由路径，
/// 还可以配置 `id`、`icon`、`order`、`path_params`、`query_params` 与 `factory`。动态
/// 路径必须声明对应的强类型参数。宏会生成转发到 `nexora::WindowElement::render` 的
/// GPUI `Render` 实现，以及供应用注册表打开原生窗口的 Entity 与窗口选项工厂。
#[proc_macro_derive(Window, attributes(nexora))]
pub fn derive_window(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_window(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// 为直接实现 `gpui::Render` 的类型注册 Account 登录页覆盖实现。
///
/// 一个应用最多只能派生一个 Login Feature。没有应用级实现时，Nexora 会使用
/// `account-client` 提供的默认登录页；`#[nexora(factory = Type::new)]` 可以覆盖默认的
/// [`Default`] 构造方式。
#[proc_macro_derive(LoginFeature, attributes(nexora))]
pub fn derive_login_feature(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_login_feature(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// 为类型生成应用级 Settings Window 覆盖实现。
///
/// 设置窗口使用框架保留的 `settings` 标识和 `/settings` 路径。宏会生成
/// `nexora::Window` 与 GPUI `Render` 实现，并把渲染转发给应用实现的
/// `nexora::WindowElement`；`#[nexora(factory = Type::new)]` 可以提供自定义构造函数。
#[proc_macro_derive(SettingsWindow, attributes(nexora))]
pub fn derive_settings_window(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_settings_window(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// 为直接实现 `gpui::Render` 的类型注册主 Sidebar Header 插槽。
///
/// 默认使用 [`Default`] 创建一次 GPUI Entity；状态化类型可以通过
/// `#[nexora(factory = Type::new)]` 提供接收 `Window` 与 `Context<Type>` 的构造函数。
#[proc_macro_derive(SidebarHeader, attributes(nexora))]
pub fn derive_sidebar_header(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_sidebar_slot(input, SidebarSlotKind::Header)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// 为直接实现 `gpui::Render` 的类型注册主 Sidebar Footer 插槽。
///
/// 默认使用 [`Default`] 创建一次 GPUI Entity；状态化类型可以通过
/// `#[nexora(factory = Type::new)]` 提供接收 `Window` 与 `Context<Type>` 的构造函数。
#[proc_macro_derive(SidebarFooter, attributes(nexora))]
pub fn derive_sidebar_footer(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_sidebar_slot(input, SidebarSlotKind::Footer)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

/// 为应用根配置生成 Nexora 强类型配置能力。
///
/// 宏会在调用方 crate 中记录 `CARGO_PKG_NAME`，使 `nexora::config::initialize(None)`
/// 默认读取当前应用对应的配置文件。命名字段可以分别使用
/// `#[nexora(account_client)]` 与 `#[nexora(account_server)]` 标记桌面和服务端 Account
/// 配置段；宏会生成对应的隐藏 provider 实现，并在配置加载后调用配置段校验。
#[proc_macro_derive(Settings, attributes(nexora))]
pub fn derive_settings(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_settings(input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[derive(Default)]
struct FeatureArguments {
    id: Option<LitStr>,
    title: Option<LitStr>,
    path: Option<LitStr>,
    section: Option<LitStr>,
    icon: Option<LitStr>,
    parent: Option<LitStr>,
    order: Option<i32>,
    navigation: Option<LitBool>,
    content_scrollable: Option<LitBool>,
    path_params: Option<Type>,
    query_params: Option<Type>,
    factory: Option<ExprPath>,
}

#[derive(Default)]
struct WindowArguments {
    id: Option<LitStr>,
    title: Option<LitStr>,
    path: Option<LitStr>,
    icon: Option<LitStr>,
    order: Option<i32>,
    path_params: Option<Type>,
    query_params: Option<Type>,
    factory: Option<ExprPath>,
}

#[derive(Clone, Copy)]
enum SidebarSlotKind {
    Header,
    Footer,
}

struct SettingsField {
    ident: Ident,
    ty: proc_macro2::TokenStream,
}

#[derive(Default)]
struct SettingsFields {
    account_client: Option<SettingsField>,
    account_server: Option<SettingsField>,
}

fn expand_feature(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    reject_generics(&input, "Feature")?;
    let arguments = parse_feature_arguments(&input.attrs)?;
    let title = required_string(arguments.title, "title")?;
    let path = required_string(arguments.path, "path")?;
    let dynamic = validate_route_path(&path)?;
    if dynamic && arguments.path_params.is_none() {
        return Err(Error::new(
            path.span(),
            "包含动态参数的 Feature 路径必须声明 path_params = SomeType",
        ));
    }
    let navigation = arguments
        .navigation
        .as_ref()
        .map(LitBool::value)
        .unwrap_or(true);
    let content_scrollable = arguments
        .content_scrollable
        .as_ref()
        .map(LitBool::value)
        .unwrap_or(true);
    if dynamic && navigation {
        return Err(Error::new(
            path.span(),
            "包含动态参数的 Feature 路径必须显式设置 navigation = false",
        ));
    }

    let ident = &input.ident;
    let id = arguments.id.unwrap_or_else(|| {
        LitStr::new(
            &default_id(ident.to_string().as_str(), "Feature"),
            ident.span(),
        )
    });
    let section = optional_string(arguments.section);
    let icon = optional_string(arguments.icon);
    let parent = optional_string(arguments.parent);
    let order = arguments.order.unwrap_or_default();
    let nexora = nexora_path();
    let path_params = arguments
        .path_params
        .map_or_else(|| quote!(#nexora::NoPath), |path| quote!(#path));
    let query_params = arguments
        .query_params
        .map_or_else(|| quote!(#nexora::NoQuery), |query| quote!(#query));
    let constructor = arguments.factory.map_or_else(
        || quote!(|_, _| ::core::default::Default::default()),
        |factory| quote!(#factory),
    );
    let type_name = ident.to_string();
    let factory_function = Ident::new(
        &format!(
            "__nexora_feature_factory_{}",
            type_name.strip_prefix("r#").unwrap_or(&type_name)
        ),
        ident.span(),
    );
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #nexora::Feature for #ident #type_generics #where_clause {
            type Path = #path_params;
            type Query = #query_params;

            const METADATA: #nexora::FeatureMetadata = #nexora::FeatureMetadata::new(
                #id,
                #title,
                #path,
                #section,
                #icon,
                #parent,
                #order,
                #navigation,
            ).with_content_scrollable(#content_scrollable);

            const REGISTRATION: ::core::option::Option<#nexora::__private::FeatureRegistration> =
                ::core::option::Option::Some(#nexora::__private::FeatureRegistration::new(
                    Self::METADATA,
                    #factory_function,
                ));
        }

        impl #impl_generics #nexora::__private::gpui::Render for #ident #type_generics #where_clause {
            fn render(
                &mut self,
                window: &mut #nexora::__private::gpui::Window,
                cx: &mut #nexora::__private::gpui::Context<Self>,
            ) -> impl #nexora::__private::gpui::IntoElement {
                <Self as #nexora::FeatureElement>::render(self, window, cx)
            }
        }

        #[allow(non_snake_case, reason = "派生宏工厂名称包含原始 Feature 类型名以避免冲突")]
        fn #factory_function(
            route: #nexora::RouteMatch,
            window: &mut #nexora::__private::gpui::Window,
            cx: &mut #nexora::__private::gpui::App,
        ) -> ::core::result::Result<#nexora::FeatureInstance, #nexora::FeatureRuntimeError> {
            #nexora::__private::create_feature::<#ident>(
                route,
                window,
                cx,
                #constructor,
            )
        }

        #nexora::__private::inventory::submit! {
            #nexora::__private::FeatureRegistration::new(
                <#ident as #nexora::Feature>::METADATA,
                #factory_function,
            )
        }
    })
}

fn expand_window(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    reject_generics(&input, "Window")?;
    let arguments = parse_window_arguments(&input.attrs)?;
    let title = required_string(arguments.title, "title")?;
    let path = required_string(arguments.path, "path")?;
    let dynamic = validate_route_path(&path)?;
    if dynamic && arguments.path_params.is_none() {
        return Err(Error::new(
            path.span(),
            "包含动态参数的 Window 路径必须声明 path_params = SomeType",
        ));
    }

    let ident = &input.ident;
    let id = arguments.id.unwrap_or_else(|| {
        LitStr::new(
            &default_id(ident.to_string().as_str(), "Window"),
            ident.span(),
        )
    });
    let icon = optional_string(arguments.icon);
    let order = arguments.order.unwrap_or_default();
    let nexora = nexora_path();
    let path_params = arguments
        .path_params
        .map_or_else(|| quote!(#nexora::NoPath), |path| quote!(#path));
    let query_params = arguments
        .query_params
        .map_or_else(|| quote!(#nexora::NoQuery), |query| quote!(#query));
    let constructor = arguments.factory.map_or_else(
        || quote!(|_, _| ::core::default::Default::default()),
        |factory| quote!(#factory),
    );
    let type_name = ident.to_string();
    let type_name = type_name.strip_prefix("r#").unwrap_or(&type_name);
    let factory_function = Ident::new(
        &format!("__nexora_window_factory_{type_name}"),
        ident.span(),
    );
    let options_function = Ident::new(
        &format!("__nexora_window_options_{type_name}"),
        ident.span(),
    );
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #nexora::Window for #ident #type_generics #where_clause {
            type Path = #path_params;
            type Query = #query_params;

            const METADATA: #nexora::WindowMetadata = #nexora::WindowMetadata::new(
                #id,
                #title,
                #path,
                #icon,
                #order,
            );

            const REGISTRATION: ::core::option::Option<#nexora::__private::WindowRegistration> =
                ::core::option::Option::Some(#nexora::__private::WindowRegistration::new(
                    Self::METADATA,
                    #factory_function,
                    #options_function,
                ));
        }

        impl #impl_generics #nexora::__private::gpui::Render for #ident #type_generics #where_clause {
            fn render(
                &mut self,
                window: &mut #nexora::__private::gpui::Window,
                cx: &mut #nexora::__private::gpui::Context<Self>,
            ) -> impl #nexora::__private::gpui::IntoElement {
                <Self as #nexora::WindowElement>::render(self, window, cx)
            }
        }

        #[allow(non_snake_case, reason = "派生宏工厂名称包含原始 Window 类型名以避免冲突")]
        fn #factory_function(
            route: #nexora::RouteMatch,
            window: &mut #nexora::__private::gpui::Window,
            cx: &mut #nexora::__private::gpui::App,
        ) -> ::core::result::Result<#nexora::WindowInstance, #nexora::WindowRuntimeError> {
            #nexora::__private::create_window::<#ident>(
                route,
                window,
                cx,
                #constructor,
            )
        }

        #[allow(non_snake_case, reason = "派生宏选项工厂名称包含原始 Window 类型名以避免冲突")]
        fn #options_function(
            route: &#nexora::RouteMatch,
            cx: &#nexora::__private::gpui::App,
        ) -> ::core::result::Result<
            #nexora::__private::gpui::WindowOptions,
            #nexora::WindowRuntimeError,
        > {
            #nexora::__private::window_options::<#ident>(route, cx)
        }

        #nexora::__private::inventory::submit! {
            #nexora::__private::WindowRegistration::new(
                <#ident as #nexora::Window>::METADATA,
                #factory_function,
                #options_function,
            )
        }
    })
}

fn expand_login_feature(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    reject_generics(&input, "LoginFeature")?;
    let factory = parse_optional_factory(&input.attrs, "LoginFeature")?;
    let ident = &input.ident;
    let nexora = nexora_path();
    let constructor = factory.map_or_else(
        || quote!(|_, _| ::core::default::Default::default()),
        |factory| quote!(#factory),
    );
    let type_name = ident.to_string();
    let type_name = type_name.strip_prefix("r#").unwrap_or(&type_name);
    let factory_function = Ident::new(
        &format!("__nexora_login_feature_factory_{type_name}"),
        ident.span(),
    );
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #nexora::LoginFeature for #ident #type_generics #where_clause {
            const REGISTRATION: #nexora::__private::LoginFeatureRegistration =
                #nexora::__private::LoginFeatureRegistration::new(
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                    #factory_function,
                );
        }

        #[allow(non_snake_case, reason = "派生宏工厂名称包含原始 Login Feature 类型名以避免冲突")]
        fn #factory_function(
            window: &mut #nexora::__private::gpui::Window,
            cx: &mut #nexora::__private::gpui::App,
        ) -> #nexora::__private::gpui::AnyView {
            #nexora::__private::create_login_feature::<#ident>(
                window,
                cx,
                #constructor,
            )
        }

        #nexora::__private::inventory::submit! {
            #nexora::__private::LoginFeatureRegistration::new(
                ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                #factory_function,
            )
        }
    })
}

fn expand_settings_window(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    reject_generics(&input, "SettingsWindow")?;
    let factory = parse_optional_factory(&input.attrs, "SettingsWindow")?;
    let ident = &input.ident;
    let nexora = nexora_path();
    let constructor = factory.map_or_else(
        || quote!(|_, _| ::core::default::Default::default()),
        |factory| quote!(#factory),
    );
    let type_name = ident.to_string();
    let type_name = type_name.strip_prefix("r#").unwrap_or(&type_name);
    let factory_function = Ident::new(
        &format!("__nexora_settings_window_factory_{type_name}"),
        ident.span(),
    );
    let options_function = Ident::new(
        &format!("__nexora_settings_window_options_{type_name}"),
        ident.span(),
    );
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #nexora::Window for #ident #type_generics #where_clause {
            type Path = #nexora::NoPath;
            type Query = #nexora::NoQuery;

            const METADATA: #nexora::WindowMetadata = #nexora::WindowMetadata::new(
                "settings",
                "设置",
                "/settings",
                ::core::option::Option::Some("settings"),
                0,
            );

            const REGISTRATION: ::core::option::Option<#nexora::__private::WindowRegistration> =
                ::core::option::Option::Some(#nexora::__private::WindowRegistration::new_settings(
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                    Self::METADATA,
                    #factory_function,
                    #options_function,
                ));
        }

        impl #impl_generics #nexora::SettingsWindow for #ident #type_generics #where_clause {
            const REGISTRATION: #nexora::__private::SettingsWindowRegistration =
                #nexora::__private::SettingsWindowRegistration::new(
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                    #nexora::__private::WindowRegistration::new_settings(
                        ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                        <Self as #nexora::Window>::METADATA,
                        #factory_function,
                        #options_function,
                    ),
                );
        }

        impl #impl_generics #nexora::__private::gpui::Render for #ident #type_generics #where_clause {
            fn render(
                &mut self,
                window: &mut #nexora::__private::gpui::Window,
                cx: &mut #nexora::__private::gpui::Context<Self>,
            ) -> impl #nexora::__private::gpui::IntoElement {
                <Self as #nexora::WindowElement>::render(self, window, cx)
            }
        }

        #[allow(non_snake_case, reason = "派生宏工厂名称包含原始 Settings Window 类型名以避免冲突")]
        fn #factory_function(
            route: #nexora::RouteMatch,
            window: &mut #nexora::__private::gpui::Window,
            cx: &mut #nexora::__private::gpui::App,
        ) -> ::core::result::Result<#nexora::WindowInstance, #nexora::WindowRuntimeError> {
            #nexora::__private::create_window::<#ident>(
                route,
                window,
                cx,
                #constructor,
            )
        }

        #[allow(non_snake_case, reason = "派生宏选项工厂名称包含原始 Settings Window 类型名以避免冲突")]
        fn #options_function(
            route: &#nexora::RouteMatch,
            cx: &#nexora::__private::gpui::App,
        ) -> ::core::result::Result<
            #nexora::__private::gpui::WindowOptions,
            #nexora::WindowRuntimeError,
        > {
            #nexora::__private::window_options::<#ident>(route, cx)
        }

        #nexora::__private::inventory::submit! {
            #nexora::__private::SettingsWindowRegistration::new(
                ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                #nexora::__private::WindowRegistration::new_settings(
                    ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                    <#ident as #nexora::Window>::METADATA,
                    #factory_function,
                    #options_function,
                ),
            )
        }
    })
}

fn expand_sidebar_slot(
    input: DeriveInput,
    kind: SidebarSlotKind,
) -> Result<proc_macro2::TokenStream> {
    let kind_name = match kind {
        SidebarSlotKind::Header => "SidebarHeader",
        SidebarSlotKind::Footer => "SidebarFooter",
    };
    reject_generics(&input, kind_name)?;
    let factory = parse_optional_factory(&input.attrs, kind_name)?;
    let ident = &input.ident;
    let nexora = nexora_path();
    let constructor = factory.map_or_else(
        || quote!(|_, _| ::core::default::Default::default()),
        |factory| quote!(#factory),
    );
    let type_name = ident.to_string();
    let type_name = type_name.strip_prefix("r#").unwrap_or(&type_name);
    let function_prefix = match kind {
        SidebarSlotKind::Header => "__nexora_sidebar_header_factory",
        SidebarSlotKind::Footer => "__nexora_sidebar_footer_factory",
    };
    let factory_function = Ident::new(&format!("{function_prefix}_{type_name}"), ident.span());
    let (trait_name, registration_name) = match kind {
        SidebarSlotKind::Header => (
            quote!(#nexora::SidebarHeader),
            quote!(#nexora::__private::SidebarHeaderRegistration),
        ),
        SidebarSlotKind::Footer => (
            quote!(#nexora::SidebarFooter),
            quote!(#nexora::__private::SidebarFooterRegistration),
        ),
    };
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #trait_name for #ident #type_generics #where_clause {
            const REGISTRATION: #registration_name = #registration_name::new(
                ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                #factory_function,
            );
        }

        #[allow(non_snake_case, reason = "派生宏工厂名称包含原始 Sidebar 插槽类型名以避免冲突")]
        fn #factory_function(
            window: &mut #nexora::__private::gpui::Window,
            cx: &mut #nexora::__private::gpui::App,
        ) -> #nexora::__private::gpui::AnyView {
            #nexora::__private::create_sidebar_slot::<#ident>(
                window,
                cx,
                #constructor,
            )
        }

        #nexora::__private::inventory::submit! {
            #registration_name::new(
                ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                #factory_function,
            )
        }
    })
}

fn expand_settings(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    let fields = parse_settings_fields(&input)?;
    let ident = &input.ident;
    let nexora = nexora_path();
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let client_validation = fields.account_client.as_ref().map(|field| {
        let field_ident = &field.ident;
        let field_type = &field.ty;
        quote! {
            <#field_type as #nexora::config::AccountClientSection>::validate_account_client(
                &self.#field_ident,
            )?;
        }
    });
    let server_validation = fields.account_server.as_ref().map(|field| {
        let field_ident = &field.ident;
        let field_type = &field.ty;
        quote! {
            <#field_type as #nexora::config::AccountServerSection>::validate_account_server(
                &self.#field_ident,
            )?;
        }
    });
    let client_provider = fields.account_client.as_ref().map(|field| {
        let field_ident = &field.ident;
        let field_type = &field.ty;
        quote! {
            impl #impl_generics #nexora::__private::ProvidesAccountClientSettings
                for #ident #type_generics #where_clause
            {
                type AccountClientSettings = #field_type;

                fn account_client_settings(&self) -> &Self::AccountClientSettings {
                    &self.#field_ident
                }
            }
        }
    });
    let server_provider = fields.account_server.as_ref().map(|field| {
        let field_ident = &field.ident;
        let field_type = &field.ty;
        quote! {
            impl #impl_generics #nexora::__private::ProvidesAccountServerSettings
                for #ident #type_generics #where_clause
            {
                type AccountServerSettings = #field_type;

                fn account_server_settings(&self) -> &Self::AccountServerSettings {
                    &self.#field_ident
                }
            }
        }
    });

    Ok(quote! {
        impl #impl_generics #nexora::config::Settings for #ident #type_generics #where_clause {
            const APP_NAME: &'static str = env!("CARGO_PKG_NAME");

            fn validate(
                &self,
            ) -> ::core::result::Result<(), #nexora::config::ConfigError> {
                #client_validation
                #server_validation
                ::core::result::Result::Ok(())
            }
        }

        #client_provider
        #server_provider
    })
}

fn parse_settings_fields(input: &DeriveInput) -> Result<SettingsFields> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "Settings 只能派生在具有命名字段的结构体上",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            &data.fields,
            "Settings 只能派生在具有命名字段的结构体上",
        ));
    };

    let mut settings_fields = SettingsFields::default();
    for field in &fields.named {
        let mut account_client = false;
        let mut account_server = false;
        for attribute in field
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("nexora"))
        {
            let mut parsed_marker = false;
            attribute.parse_nested_meta(|meta| {
                parsed_marker = true;
                if meta.path.is_ident("account_client") {
                    if account_client {
                        return Err(meta.error("account_client 只能声明一次"));
                    }
                    account_client = true;
                    Ok(())
                } else if meta.path.is_ident("account_server") {
                    if account_server {
                        return Err(meta.error("account_server 只能声明一次"));
                    }
                    account_server = true;
                    Ok(())
                } else {
                    Err(meta
                        .error("不支持的 settings 字段属性；允许 account_client 和 account_server"))
                }
            })?;
            if !parsed_marker {
                return Err(Error::new_spanned(
                    attribute,
                    "settings 字段属性必须声明 account_client 或 account_server",
                ));
            }
        }
        if account_client && account_server {
            return Err(Error::new_spanned(
                field,
                "同一个配置字段不能同时标记为 account_client 和 account_server",
            ));
        }

        let Some(field_ident) = field.ident.clone() else {
            continue;
        };
        if account_client {
            let field_type = &field.ty;
            set_once(
                &mut settings_fields.account_client,
                SettingsField {
                    ident: field_ident.clone(),
                    ty: quote!(#field_type),
                },
                field_ident.span(),
                "account_client",
            )?;
        }
        if account_server {
            let field_type = &field.ty;
            set_once(
                &mut settings_fields.account_server,
                SettingsField {
                    ident: field_ident.clone(),
                    ty: quote!(#field_type),
                },
                field_ident.span(),
                "account_server",
            )?;
        }
    }

    Ok(settings_fields)
}

fn reject_generics(input: &DeriveInput, kind: &str) -> Result<()> {
    if input.generics.params.is_empty() {
        return Ok(());
    }

    Err(Error::new_spanned(
        &input.generics,
        format!("{kind} 必须是可自动注册的具体类型，不能声明泛型参数"),
    ))
}

fn nexora_path() -> proc_macro2::TokenStream {
    match crate_name("nexora") {
        Ok(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, proc_macro2::Span::call_site());
            quote!(::#ident)
        }
        Ok(FoundCrate::Itself) | Err(_) => quote!(::nexora),
    }
}

fn parse_feature_arguments(attributes: &[Attribute]) -> Result<FeatureArguments> {
    let attribute = single_attribute(attributes)?;
    let mut arguments = FeatureArguments::default();

    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            set_once(&mut arguments.id, parse_string(&meta)?, meta.path.span(), "id")
        } else if meta.path.is_ident("title") {
            set_once(
                &mut arguments.title,
                parse_string(&meta)?,
                meta.path.span(),
                "title",
            )
        } else if meta.path.is_ident("path") {
            set_once(
                &mut arguments.path,
                parse_string(&meta)?,
                meta.path.span(),
                "path",
            )
        } else if meta.path.is_ident("section") {
            set_once(
                &mut arguments.section,
                parse_string(&meta)?,
                meta.path.span(),
                "section",
            )
        } else if meta.path.is_ident("icon") {
            set_once(
                &mut arguments.icon,
                parse_string(&meta)?,
                meta.path.span(),
                "icon",
            )
        } else if meta.path.is_ident("parent") {
            set_once(
                &mut arguments.parent,
                parse_string(&meta)?,
                meta.path.span(),
                "parent",
            )
        } else if meta.path.is_ident("order") {
            let order = parse_order(&meta)?;
            set_once(&mut arguments.order, order, meta.path.span(), "order")
        } else if meta.path.is_ident("navigation") {
            let navigation = meta.value()?.parse::<LitBool>()?;
            set_once(
                &mut arguments.navigation,
                navigation,
                meta.path.span(),
                "navigation",
            )
        } else if meta.path.is_ident("content_scrollable") {
            let content_scrollable = meta.value()?.parse::<LitBool>()?;
            set_once(
                &mut arguments.content_scrollable,
                content_scrollable,
                meta.path.span(),
                "content_scrollable",
            )
        } else if meta.path.is_ident("path_params") {
            let path_params = meta.value()?.parse::<Type>()?;
            set_once(
                &mut arguments.path_params,
                path_params,
                meta.path.span(),
                "path_params",
            )
        } else if meta.path.is_ident("query_params") {
            let query_params = meta.value()?.parse::<Type>()?;
            set_once(
                &mut arguments.query_params,
                query_params,
                meta.path.span(),
                "query_params",
            )
        } else if meta.path.is_ident("factory") {
            let factory = meta.value()?.parse::<ExprPath>()?;
            set_once(
                &mut arguments.factory,
                factory,
                meta.path.span(),
                "factory",
            )
        } else {
            Err(meta.error("不支持的 feature 属性；允许 id、title、path、section、icon、parent、order、navigation、content_scrollable、path_params、query_params 和 factory"))
        }
    })?;

    Ok(arguments)
}

fn parse_window_arguments(attributes: &[Attribute]) -> Result<WindowArguments> {
    let attribute = single_attribute(attributes)?;
    let mut arguments = WindowArguments::default();

    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            set_once(
                &mut arguments.id,
                parse_string(&meta)?,
                meta.path.span(),
                "id",
            )
        } else if meta.path.is_ident("title") {
            set_once(
                &mut arguments.title,
                parse_string(&meta)?,
                meta.path.span(),
                "title",
            )
        } else if meta.path.is_ident("path") {
            set_once(
                &mut arguments.path,
                parse_string(&meta)?,
                meta.path.span(),
                "path",
            )
        } else if meta.path.is_ident("icon") {
            set_once(
                &mut arguments.icon,
                parse_string(&meta)?,
                meta.path.span(),
                "icon",
            )
        } else if meta.path.is_ident("order") {
            let order = parse_order(&meta)?;
            set_once(&mut arguments.order, order, meta.path.span(), "order")
        } else if meta.path.is_ident("path_params") {
            let path_params = meta.value()?.parse::<Type>()?;
            set_once(
                &mut arguments.path_params,
                path_params,
                meta.path.span(),
                "path_params",
            )
        } else if meta.path.is_ident("query_params") {
            let query_params = meta.value()?.parse::<Type>()?;
            set_once(
                &mut arguments.query_params,
                query_params,
                meta.path.span(),
                "query_params",
            )
        } else if meta.path.is_ident("factory") {
            let factory = meta.value()?.parse::<ExprPath>()?;
            set_once(
                &mut arguments.factory,
                factory,
                meta.path.span(),
                "factory",
            )
        } else {
            Err(meta.error("不支持的 window 属性；允许 id、title、path、icon、order、path_params、query_params 和 factory"))
        }
    })?;

    Ok(arguments)
}

fn parse_optional_factory(attributes: &[Attribute], kind: &str) -> Result<Option<ExprPath>> {
    let mut matching = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("nexora"));
    let Some(attribute) = matching.next() else {
        return Ok(None);
    };
    if let Some(duplicate) = matching.next() {
        return Err(Error::new(
            duplicate.path().span(),
            "#[nexora(...)] 属性只能声明一次",
        ));
    }

    let mut factory = None;
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("factory") {
            let value = meta.value()?.parse::<ExprPath>()?;
            set_once(&mut factory, value, meta.path.span(), "factory")
        } else {
            Err(meta.error(format!("{kind} 只支持 factory 属性")))
        }
    })?;
    Ok(factory)
}

fn single_attribute(attributes: &[Attribute]) -> Result<&Attribute> {
    let mut matching = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("nexora"));
    let Some(attribute) = matching.next() else {
        return Err(Error::new(
            proc_macro2::Span::call_site(),
            "缺少 #[nexora(...)] 属性",
        ));
    };
    if let Some(duplicate) = matching.next() {
        return Err(Error::new(
            duplicate.path().span(),
            "#[nexora(...)] 属性只能声明一次",
        ));
    }

    Ok(attribute)
}

fn parse_string(meta: &syn::meta::ParseNestedMeta<'_>) -> Result<LitStr> {
    meta.value()?.parse()
}

fn parse_order(meta: &syn::meta::ParseNestedMeta<'_>) -> Result<i32> {
    let expression = meta.value()?.parse::<Expr>()?;
    let value = parse_order_expression(expression)?;

    i32::try_from(value)
        .map_err(|_| Error::new(meta.path.span(), "order 必须位于 i32 可表示的范围内"))
}

fn parse_order_expression(expression: Expr) -> Result<i64> {
    let value = match expression {
        Expr::Lit(ExprLit {
            lit: Lit::Int(value),
            ..
        }) => parse_integer(&value)?,
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => match *expr {
            Expr::Lit(ExprLit {
                lit: Lit::Int(value),
                ..
            }) => parse_integer(&value)?
                .checked_neg()
                .ok_or_else(|| Error::new(value.span(), "order 必须位于 i32 可表示的范围内"))?,
            expression => {
                return Err(Error::new_spanned(expression, "order 必须是整数常量"));
            }
        },
        Expr::Group(ExprGroup { expr, .. }) | Expr::Paren(ExprParen { expr, .. }) => {
            return parse_order_expression(*expr);
        }
        expression => return Err(Error::new_spanned(expression, "order 必须是整数常量")),
    };
    Ok(value)
}

fn parse_integer(value: &LitInt) -> Result<i64> {
    value
        .base10_parse::<i64>()
        .map_err(|_| Error::new(value.span(), "order 必须位于 i32 可表示的范围内"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, span: proc_macro2::Span, name: &str) -> Result<()> {
    if slot.is_some() {
        return Err(Error::new(span, format!("{name} 只能声明一次")));
    }

    *slot = Some(value);
    Ok(())
}

fn required_string(value: Option<LitStr>, name: &str) -> Result<LitStr> {
    value.ok_or_else(|| {
        Error::new(
            proc_macro2::Span::call_site(),
            format!("#[nexora(...)] 缺少必填字段 {name}"),
        )
    })
}

fn optional_string(value: Option<LitStr>) -> proc_macro2::TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

fn validate_route_path(path: &LitStr) -> Result<bool> {
    let value = path.value();
    if !value.starts_with('/') {
        return Err(Error::new(path.span(), "路由 path 必须以 / 开头"));
    }
    if value.contains("://") {
        return Err(Error::new(path.span(), "路由 path 不能包含 URL scheme"));
    }
    if value.contains('?') {
        return Err(Error::new(path.span(), "路由 path 不能包含查询字符串"));
    }
    if value.contains('#') {
        return Err(Error::new(path.span(), "路由 path 不能包含 fragment"));
    }

    let mut parameters = HashSet::new();
    for segment in value.split('/').filter(|segment| !segment.is_empty()) {
        if !segment.contains(':') {
            continue;
        }
        let Some(name) = segment.strip_prefix(':') else {
            return Err(Error::new(
                path.span(),
                "动态参数必须占据完整路径段，例如 /users/:id",
            ));
        };
        if !valid_parameter_name(name) {
            return Err(Error::new(
                path.span(),
                format!(
                    "动态参数 :{name} 的名称无效；名称必须以字母或下划线开头，且只能包含字母、数字和下划线"
                ),
            ));
        }
        if !parameters.insert(name.to_owned()) {
            return Err(Error::new(
                path.span(),
                format!("动态参数 :{name} 在同一路径中重复声明"),
            ));
        }
        if parameters.len() > 25 {
            return Err(Error::new(path.span(), "单条 path 最多支持 25 个动态参数"));
        }
    }

    Ok(!parameters.is_empty())
}

fn valid_parameter_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn default_id(type_name: &str, suffix: &str) -> String {
    let type_name = type_name.strip_prefix("r#").unwrap_or(type_name);
    let type_name = type_name
        .strip_suffix(suffix)
        .filter(|name| !name.is_empty())
        .unwrap_or(type_name);
    kebab_case(type_name)
}

fn kebab_case(value: &str) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    let mut result = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        if character == '_' || character == '-' || character.is_whitespace() {
            if !result.is_empty() && !result.ends_with('-') {
                result.push('-');
            }
            continue;
        }

        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        let starts_word = character.is_uppercase()
            && !result.is_empty()
            && !result.ends_with('-')
            && (previous.is_some_and(|previous| previous.is_lowercase() || previous.is_numeric())
                || previous.is_some_and(|previous| previous.is_uppercase())
                    && next.is_some_and(|next| next.is_lowercase()));
        if starts_word {
            result.push('-');
        }
        result.extend(character.to_lowercase());
    }

    result.trim_matches('-').to_owned()
}
