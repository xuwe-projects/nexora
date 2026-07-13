//! Rust 源码、公开文档和 GPUI 生命周期检查。

use std::{collections::HashSet, fs, path::Path};

use proc_macro2::Span;
use syn::{
    Attribute, Expr, ExprAsync, ExprCall, ExprClosure, ExprMacro, ExprMethodCall, ImplItemFn,
    ItemConst, ItemEnum, ItemFn, ItemMod, ItemStatic, ItemStruct, ItemTrait, ItemType, ItemUnion,
    Lit, Meta, ReturnType, Signature, Stmt, TraitItem, Type, Visibility,
    spanned::Spanned as _,
    visit::{self, Visit},
};

use super::{
    CliError, CliResult,
    cargo::{Member, Workspace},
    diagnostic::{Diagnostic, Report},
    relative_path,
};

const EVENT_METHODS: [&str; 9] = [
    "on_action",
    "on_cancel",
    "on_change",
    "on_click",
    "on_confirm",
    "on_select",
    "on_submit",
    "on_toggle",
    "on_value_change",
];
const RENDER_CONTEXT_SIDE_EFFECTS: [&str; 10] = [
    "global_mut",
    "new",
    "notify",
    "observe",
    "observe_global",
    "refresh_windows",
    "set_global",
    "spawn",
    "subscribe",
    "update_global",
];
const LIFECYCLE_METHODS: [&str; 4] = ["observe", "observe_global", "spawn", "subscribe"];
const UNSTABLE_ID_NAMES: [&str; 6] = ["col_ix", "index", "ix", "position", "row_ix", "timestamp"];
const GLOBAL_CONTAINER_NAMES: [&str; 4] = ["LazyLock", "Mutex", "OnceLock", "RwLock"];
const COPIED_GLOBAL_NAMES: [&str; 2] = ["Theme", "ThemeRegistry"];

/// 检查 workspace 中全部生产 Rust 源码。
pub(super) fn check(workspace: &Workspace, report: &mut Report) -> CliResult<()> {
    for member in workspace.members() {
        let source_root = member.directory().join("src");
        if !source_root.is_dir() {
            continue;
        }

        let mut files = Vec::new();
        collect_rust_files(&source_root, &mut files)?;
        files.sort();

        for path in files {
            check_file(workspace, member, &path, report)?;
        }
    }

    Ok(())
}

fn check_file(
    workspace: &Workspace,
    member: &Member,
    path: &Path,
    report: &mut Report,
) -> CliResult<()> {
    if path.file_name().and_then(|name| name.to_str()) == Some("mod.rs") {
        report.push(
            Diagnostic::error(
                "xuwe::forbidden_mod_rs",
                relative_path(workspace.root(), path),
                1,
                1,
                "模块入口禁止使用 `mod.rs`",
            )
            .with_help(
                "把模块入口迁移到同名 `.rs` 文件，例如 `features/mod.rs` 改为 `features.rs`",
            ),
        );
    }

    let source = fs::read_to_string(path).map_err(|error| {
        CliError::new(format!("无法读取 Rust 源码 {}：{error}", path.display()))
    })?;
    let syntax = syn::parse_file(&source).map_err(|error| {
        CliError::new(format!("无法解析 Rust 源码 {}：{error}", path.display()))
    })?;
    let relative = relative_path(workspace.root(), path);

    let mut docs = PublicDocsVisitor {
        path: relative.clone(),
        report,
    };
    docs.visit_file(&syntax);

    let profile = SourceProfile {
        is_actions: member.name() == "actions",
        is_theme: member.name() == "theme",
        is_gpui: member.uses_gpui(),
        uses_axum: member.uses_dependency("axum"),
        uses_sqlx: member.uses_dependency("sqlx"),
        is_contract: member.is_contract(),
    };
    let mut source_rules = SourceRuleVisitor::new(&source, relative, profile, report);
    source_rules.visit_file(&syntax);
    Ok(())
}

struct PublicDocsVisitor<'a> {
    path: std::path::PathBuf,
    report: &'a mut Report,
}

impl PublicDocsVisitor<'_> {
    fn check_item(&mut self, visibility: &Visibility, attrs: &[Attribute], span: Span, kind: &str) {
        if !is_public(visibility) || is_doc_hidden(attrs) {
            return;
        }
        self.check_docs(attrs, span, kind);
    }

    fn check_docs(&mut self, attrs: &[Attribute], span: Span, kind: &str) -> String {
        let docs = doc_text(attrs);
        if docs.trim().is_empty() {
            self.push(
                Diagnostic::error(
                    "xuwe::non_chinese_public_docs",
                    self.path.clone(),
                    span_line(span),
                    span_column(span),
                    format!("公开{kind}缺少 rustdoc 中文说明"),
                )
                .with_help("使用 /// 说明职责、使用时机以及重要参数或返回值语义"),
            );
        } else if !contains_chinese(&docs) {
            self.push(
                Diagnostic::error(
                    "xuwe::non_chinese_public_docs",
                    self.path.clone(),
                    span_line(span),
                    span_column(span),
                    format!("公开{kind}的 rustdoc 不包含中文说明"),
                )
                .with_help("保留必要的英文术语，同时用中文完整解释公开 API"),
            );
        }
        docs
    }

    fn check_function(
        &mut self,
        visibility: &Visibility,
        attrs: &[Attribute],
        signature: &Signature,
        body: Option<&syn::Block>,
    ) {
        if !is_public(visibility) || is_doc_hidden(attrs) {
            return;
        }
        let span = signature.ident.span();
        let docs = self.check_docs(attrs, span, "函数或方法");
        self.check_function_sections(signature, body, &docs, span);
    }

    fn check_trait_function(
        &mut self,
        attrs: &[Attribute],
        signature: &Signature,
        body: Option<&syn::Block>,
    ) {
        if is_doc_hidden(attrs) {
            return;
        }
        let span = signature.ident.span();
        let docs = self.check_docs(attrs, span, "trait 接口");
        self.check_function_sections(signature, body, &docs, span);
    }

    fn check_function_sections(
        &mut self,
        signature: &Signature,
        body: Option<&syn::Block>,
        docs: &str,
        span: Span,
    ) {
        if returns_result(signature) && !docs.contains("# Errors") {
            self.push(
                Diagnostic::error(
                    "xuwe::missing_errors_section",
                    self.path.clone(),
                    span_line(span),
                    span_column(span),
                    format!(
                        "返回 Result 的公开接口 `{}` 缺少 `# Errors`",
                        signature.ident
                    ),
                )
                .with_help("列出每类错误出现的条件以及调用方可以如何处理"),
            );
        }

        let can_panic = body.is_some_and(block_can_panic);
        if can_panic && !docs.contains("# Panics") {
            self.push(
                Diagnostic::error(
                    "xuwe::missing_panics_section",
                    self.path.clone(),
                    span_line(span),
                    span_column(span),
                    format!("公开接口 `{}` 存在明确 panic 路径", signature.ident),
                )
                .with_help("使用 `# Panics` 说明触发条件，或者把失败改成结构化错误"),
            );
        }
    }

    fn push(&mut self, diagnostic: Diagnostic) {
        self.report.push(diagnostic);
    }
}

impl<'ast> Visit<'ast> for PublicDocsVisitor<'_> {
    fn visit_item_const(&mut self, node: &'ast ItemConst) {
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "常量");
        visit::visit_item_const(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        let public = is_public(&node.vis) && !is_doc_hidden(&node.attrs);
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "枚举");
        if public {
            for variant in &node.variants {
                self.check_docs(&variant.attrs, variant.ident.span(), "枚举变体");
                for field in &variant.fields {
                    self.check_docs(&field.attrs, field.span(), "枚举字段");
                }
            }
        }
        visit::visit_item_enum(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.check_function(&node.vis, &node.attrs, &node.sig, Some(&node.block));
        visit::visit_item_fn(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "模块");
        visit::visit_item_mod(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast ItemStatic) {
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "静态变量");
        visit::visit_item_static(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let public = is_public(&node.vis) && !is_doc_hidden(&node.attrs);
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "结构体");
        if public {
            for field in &node.fields {
                if is_public(&field.vis) {
                    self.check_docs(&field.attrs, field.span(), "结构体字段");
                }
            }
        }
        visit::visit_item_struct(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        let public = is_public(&node.vis) && !is_doc_hidden(&node.attrs);
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "trait");
        if public {
            for item in &node.items {
                match item {
                    TraitItem::Const(item) => {
                        self.check_docs(&item.attrs, item.ident.span(), "trait 关联常量");
                    }
                    TraitItem::Fn(item) => {
                        self.check_trait_function(&item.attrs, &item.sig, item.default.as_ref());
                    }
                    TraitItem::Type(item) => {
                        self.check_docs(&item.attrs, item.ident.span(), "trait 关联类型");
                    }
                    _ => {}
                }
            }
        }
        visit::visit_item_trait(self, node);
    }

    fn visit_item_type(&mut self, node: &'ast ItemType) {
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "类型别名");
        visit::visit_item_type(self, node);
    }

    fn visit_item_union(&mut self, node: &'ast ItemUnion) {
        let public = is_public(&node.vis) && !is_doc_hidden(&node.attrs);
        self.check_item(&node.vis, &node.attrs, node.ident.span(), "联合体");
        if public {
            for field in &node.fields.named {
                if is_public(&field.vis) {
                    self.check_docs(&field.attrs, field.span(), "联合体字段");
                }
            }
        }
        visit::visit_item_union(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        self.check_function(&node.vis, &node.attrs, &node.sig, Some(&node.block));
        visit::visit_impl_item_fn(self, node);
    }
}

#[derive(Debug, Clone, Copy)]
struct SourceProfile {
    is_actions: bool,
    is_theme: bool,
    is_gpui: bool,
    uses_axum: bool,
    uses_sqlx: bool,
    is_contract: bool,
}

struct SourceRuleVisitor<'a> {
    source: &'a str,
    path: std::path::PathBuf,
    is_actions: bool,
    is_theme: bool,
    is_gpui: bool,
    uses_axum: bool,
    uses_sqlx: bool,
    is_contract: bool,
    has_body_limit: bool,
    in_render: bool,
    deferred_depth: usize,
    receiver_chain_depth: usize,
    seen: HashSet<(&'static str, usize, usize)>,
    report: &'a mut Report,
}

impl<'a> SourceRuleVisitor<'a> {
    fn new(
        source: &'a str,
        path: std::path::PathBuf,
        profile: SourceProfile,
        report: &'a mut Report,
    ) -> Self {
        Self {
            source,
            path,
            is_actions: profile.is_actions,
            is_theme: profile.is_theme,
            is_gpui: profile.is_gpui,
            uses_axum: profile.uses_axum,
            uses_sqlx: profile.uses_sqlx,
            is_contract: profile.is_contract,
            has_body_limit: source.contains("DefaultBodyLimit"),
            in_render: false,
            deferred_depth: 0,
            receiver_chain_depth: 0,
            seen: HashSet::new(),
            report,
        }
    }

    fn push(&mut self, diagnostic: Diagnostic, span: Span) {
        let line = span_line(span);
        let column = span_column(span);
        let rule = diagnostic.rule();
        if diagnostic.is_warning() && self.is_suppressed(rule, line)
            || !self.seen.insert((rule, line, column))
        {
            return;
        }
        self.report.push(diagnostic);
    }

    fn is_suppressed(&self, rule: &str, line: usize) -> bool {
        let lines = self.source.lines().collect::<Vec<_>>();
        let start = line.saturating_sub(4);
        let end = line.saturating_sub(1).min(lines.len());

        lines[start..end].iter().any(|candidate| {
            candidate.contains("xuwe-lint: allow(")
                && candidate.contains(rule)
                && candidate.contains("reason=")
        })
    }

    fn error(&self, rule: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(
            rule,
            self.path.clone(),
            span_line(span),
            span_column(span),
            message,
        )
    }

    fn warning(&self, rule: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic::warning(
            rule,
            self.path.clone(),
            span_line(span),
            span_column(span),
            message,
        )
    }

    fn visit_function_body(&mut self, name: &syn::Ident, block: &syn::Block) {
        let previous = self.in_render;
        self.in_render = previous || name == "render";
        self.visit_block(block);
        self.in_render = previous;
    }

    fn check_render_method_call(&mut self, node: &ExprMethodCall) {
        if !(self.is_gpui && self.in_render && self.deferred_depth == 0) {
            return;
        }
        let method = node.method.to_string();
        if !RENDER_CONTEXT_SIDE_EFFECTS.contains(&method.as_str())
            || !is_context_receiver(&node.receiver)
        {
            return;
        }

        let diagnostic = self
            .error(
                "xuwe::render_side_effect",
                node.method.span(),
                format!("render() 中直接调用了具有副作用的 `cx.{method}()`"),
            )
            .with_help("把 Entity 创建、订阅和任务启动移动到构造阶段或明确的业务方法中");
        self.push(diagnostic, node.method.span());
    }

    fn check_empty_handler(&mut self, node: &ExprMethodCall) {
        let method = node.method.to_string();
        if !EVENT_METHODS.contains(&method.as_str()) {
            return;
        }
        if !node.args.iter().any(is_empty_handler_expression) {
            return;
        }

        let diagnostic = self
            .error(
                "xuwe::empty_event_handler",
                node.method.span(),
                format!("事件处理器 `{method}` 使用了空闭包"),
            )
            .with_help("实现真实交互；尚未支持的控件应禁用或移除事件处理器");
        self.push(diagnostic, node.method.span());
    }

    fn check_rest_route(&mut self, node: &ExprMethodCall) {
        if !(self.uses_axum && node.method == "route") {
            return;
        }
        let Some(Expr::Lit(literal)) = node.args.first() else {
            return;
        };
        let Lit::Str(path) = &literal.lit else {
            return;
        };
        let path = path.value();
        let invalid_case = path.chars().any(char::is_uppercase) || path.contains('_');
        let action_query = path.contains("?action=") || path.contains("&action=");
        let action_segment = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .any(|segment| {
                let segment = segment.to_ascii_lowercase();
                ["create", "delete", "get", "list", "update"]
                    .into_iter()
                    .any(|verb| {
                        segment == verb
                            || segment.starts_with(&format!("{verb}-"))
                            || segment.starts_with(&format!("{verb}_"))
                    })
            });
        if !(invalid_case || action_query || action_segment) {
            return;
        }

        let diagnostic = self
            .warning(
                "xuwe::non_rest_route",
                literal.span(),
                format!("Axum 路由 `{path}` 使用了动作式、非小写或下划线路径"),
            )
            .with_help("普通 API 使用复数资源名和 HTTP 方法表达动作，例如 GET /api/v1/projects");
        self.push(diagnostic, literal.span());
    }

    fn check_detach(&mut self, node: &ExprMethodCall) {
        if !(self.is_gpui && node.method == "detach") {
            return;
        }
        let diagnostic = self
            .warning(
                "xuwe::detached_lifecycle",
                node.method.span(),
                "GPUI Task 或 Subscription 被 detach，生命周期不再由组件字段显式管理",
            )
            .with_help(
                "优先保存句柄；确需 detach 时在上一行添加带中文原因的 `xuwe-lint: allow(xuwe::detached_lifecycle) reason=...`",
            );
        self.push(diagnostic, node.method.span());
    }

    fn check_unstable_id(&mut self, node: &ExprMethodCall) {
        if !(self.is_gpui && node.method == "id") {
            return;
        }
        let unstable = node.args.iter().any(expression_has_unstable_id);
        if !unstable {
            return;
        }

        let diagnostic = self
            .warning(
                "xuwe::unstable_element_id",
                node.method.span(),
                "ElementId 使用了列表位置、时间或随机值，跨帧身份可能变化",
            )
            .with_help("列表项使用数据库主键、业务编号或其他稳定且唯一的业务 ID");
        self.push(diagnostic, node.method.span());
    }

    fn check_global_refresh(&mut self, node: &ExprMethodCall) {
        if !(self.is_gpui && node.method == "refresh_windows" && !self.is_theme) {
            return;
        }
        let diagnostic = self
            .warning(
                "xuwe::global_refresh_scope",
                node.method.span(),
                "非主题模块调用了 refresh_windows()，可能扩大刷新范围",
            )
            .with_help("局部状态变化使用 cx.notify()；只有主题、语言等全局变化刷新所有窗口");
        self.push(diagnostic, node.method.span());
    }

    fn check_icon_button(&mut self, node: &ExprMethodCall) {
        if !(self.is_gpui && self.receiver_chain_depth == 0) {
            return;
        }
        let Some(chain) = button_chain(node) else {
            return;
        };
        let has_icon = chain.iter().any(|method| method == "icon");
        let has_text_or_name = chain.iter().any(|method| {
            matches!(
                method.as_str(),
                "accessible_name" | "aria_label" | "child" | "label" | "tooltip"
            )
        });
        if !has_icon || has_text_or_name {
            return;
        }

        let diagnostic = self
            .warning(
                "xuwe::icon_button_without_tooltip",
                node.method.span(),
                "纯图标 Button 没有 Tooltip 或可访问名称",
            )
            .with_help("使用 .tooltip(...)，或者添加可见 label / 可访问名称");
        self.push(diagnostic, node.method.span());
    }

    fn check_render_call(&mut self, node: &ExprCall) {
        if !(self.is_gpui && self.in_render && self.deferred_depth == 0) {
            return;
        }
        let Some(path) = expression_path(&node.func) else {
            return;
        };
        let segments = path_segments(path);
        let performs_io = starts_with_segments(&segments, &["std", "fs"])
            || segments.first().is_some_and(|segment| {
                matches!(segment.as_str(), "fs" | "reqwest" | "sqlx" | "ureq")
            })
            || ends_with_segments(&segments, &["File", "open"])
            || ends_with_segments(&segments, &["Command", "new"]);
        if !performs_io {
            return;
        }

        let diagnostic = self
            .error(
                "xuwe::render_side_effect",
                node.span(),
                "render() 中直接执行了文件、网络、数据库或进程操作",
            )
            .with_help("把 I/O 放到生命周期明确的异步任务或业务方法中，再通过状态驱动界面刷新");
        self.push(diagnostic, node.span());
    }

    fn check_dynamic_sql_call(&mut self, node: &ExprCall) {
        if !self.uses_sqlx {
            return;
        }
        let Some(path) = expression_path(&node.func) else {
            return;
        };
        let Some(function) = path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            return;
        };
        if !matches!(
            function.as_str(),
            "query" | "query_as" | "query_as_with" | "query_scalar" | "query_with"
        ) {
            return;
        }
        let Some(sql) = node.args.first() else {
            return;
        };
        if !expression_contains_dynamic_sql(sql) {
            return;
        }

        let diagnostic = self
            .error(
                "xuwe::dynamic_sql_concatenation",
                sql.span(),
                "SQLx 查询通过 format! 或字符串加法动态拼接 SQL",
            )
            .with_help("使用 SQLx 参数绑定、query! 宏或 QueryBuilder::push_bind 传入不可信数据");
        self.push(diagnostic, sql.span());
    }

    fn check_dynamic_query_builder(&mut self, node: &ExprMethodCall) {
        if !(self.uses_sqlx && node.method == "push") {
            return;
        }
        let Some(fragment) = node.args.first() else {
            return;
        };
        if !expression_contains_dynamic_sql(fragment) {
            return;
        }

        let diagnostic = self
            .error(
                "xuwe::dynamic_sql_concatenation",
                fragment.span(),
                "QueryBuilder::push 接收了动态拼接的 SQL 片段",
            )
            .with_help("固定 SQL 结构使用静态片段，动态值使用 push_bind");
        self.push(diagnostic, fragment.span());
    }

    fn check_hardcoded_color(&mut self, node: &ExprCall) {
        if !self.is_gpui || self.is_theme {
            return;
        }
        let Some(path) = expression_path(&node.func) else {
            return;
        };
        let Some(name) = path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            return;
        };
        if !matches!(name.as_str(), "hsl" | "hsla" | "rgb" | "rgba") {
            return;
        }

        let diagnostic = self
            .warning(
                "xuwe::hardcoded_visual_color",
                node.span(),
                format!("业务 UI 直接调用 `{name}()` 写入固定颜色"),
            )
            .with_help("从 cx.theme() 读取语义 token；主题定义本身应集中放在 theme crate");
        self.push(diagnostic, node.span());
    }

    fn check_lifecycle_statement(&mut self, statement: &Stmt) {
        if !self.is_gpui {
            return;
        }
        let Stmt::Expr(expression, Some(_)) = statement else {
            return;
        };
        let Expr::MethodCall(call) = expression else {
            return;
        };
        let method = call.method.to_string();
        if !LIFECYCLE_METHODS.contains(&method.as_str()) {
            return;
        }

        let diagnostic = self
            .warning(
                "xuwe::untracked_task",
                call.method.span(),
                format!("`{method}()` 返回的生命周期句柄被直接丢弃"),
            )
            .with_help(
                "把 Task 或 Subscription 保存到拥有它的 Entity 字段，或明确说明 detach 的生命周期",
            );
        self.push(diagnostic, call.method.span());
    }

    fn check_global_static(&mut self, node: &ItemStatic) {
        if !self.is_gpui {
            return;
        }
        let mutable = matches!(node.mutability, syn::StaticMutability::Mut(_));
        let synchronized_container = type_contains_any(&node.ty, &GLOBAL_CONTAINER_NAMES);
        if !(mutable || synchronized_container) {
            return;
        }

        let diagnostic = self
            .error(
                "xuwe::non_gpui_global_state",
                node.ident.span(),
                format!("静态状态 `{}` 绕过了 GPUI Global", node.ident),
            )
            .with_help("把应用级可变状态实现为私有 GPUI Global，并通过 App 上下文访问");
        self.push(diagnostic, node.ident.span());
    }

    fn check_copied_global(&mut self, node: &ItemStruct) {
        if !self.is_gpui || self.is_theme {
            return;
        }
        for field in &node.fields {
            if !type_contains_any(&field.ty, &COPIED_GLOBAL_NAMES) {
                continue;
            }
            let diagnostic = self
                .warning(
                    "xuwe::copied_global_state",
                    field.span(),
                    "组件字段保存了完整 Theme 或 ThemeRegistry",
                )
                .with_help(
                    "渲染时通过 cx.theme() 或 Global API 读取，只保留轻量选择值和 Entity 句柄",
                );
            self.push(diagnostic, field.span());
        }
    }

    fn check_action_macro(&mut self, node: &syn::Macro) {
        let name = node
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if self.is_actions || !matches!(name.as_deref(), Some("actions" | "impl_actions")) {
            return;
        }
        let diagnostic = self
            .error(
                "xuwe::action_outside_actions",
                node.path.span(),
                "GPUI Action 定义出现在 actions crate 之外",
            )
            .with_help("把 Action 类型和默认快捷键集中到 crates/actions，再由业务 crate 引用");
        self.push(diagnostic, node.path.span());
    }

    fn check_action_derive(&mut self, attribute: &Attribute) {
        if self.is_actions || !attribute.path().is_ident("derive") {
            return;
        }
        let Meta::List(list) = &attribute.meta else {
            return;
        };
        let derives_action = list
            .tokens
            .to_string()
            .split(',')
            .map(str::trim)
            .any(|derive| derive == "Action" || derive.ends_with(":: Action"));
        if !derives_action {
            return;
        }
        let diagnostic = self
            .error(
                "xuwe::action_outside_actions",
                attribute.span(),
                "Action derive 出现在 actions crate 之外",
            )
            .with_help("把 Action 类型集中到 crates/actions");
        self.push(diagnostic, attribute.span());
    }

    fn check_contract_attribute(&mut self, attribute: &Attribute) {
        if !self.is_contract {
            return;
        }
        let path = path_segments(attribute.path());
        let is_sqlx_attribute = path.iter().any(|segment| segment == "sqlx");
        let derives_from_row = attribute.path().is_ident("derive")
            && matches!(&attribute.meta, Meta::List(list) if list.tokens.to_string().contains("FromRow"));
        if !(is_sqlx_attribute || derives_from_row) {
            return;
        }

        let diagnostic = self
            .warning(
                "xuwe::database_entity_in_contract",
                attribute.span(),
                "契约或共享模型类型暴露了 SQLx 数据库映射细节",
            )
            .with_help("数据库实体留在数据访问层，跨端 DTO 只保留序列化定义和必要校验");
        self.push(diagnostic, attribute.span());
    }

    fn check_axum_signature(&mut self, signature: &Signature) {
        if !(self.uses_axum && signature.asyncness.is_some()) {
            return;
        }

        for input in &signature.inputs {
            let syn::FnArg::Typed(argument) = input else {
                continue;
            };
            let Some(name) = type_last_ident(&argument.ty) else {
                continue;
            };
            if matches!(name.as_str(), "Bytes" | "HeaderMap" | "Request" | "String") {
                let diagnostic = self
                    .warning(
                        "xuwe::raw_axum_request",
                        argument.ty.span(),
                        format!("异步 handler 直接接收原始 `{name}` 请求数据"),
                    )
                    .with_help(
                        "优先使用 Path、Query、Json、Form、Multipart、State 或自定义 extractor",
                    );
                self.push(diagnostic, argument.ty.span());
            }

            if !self.has_body_limit
                && matches!(name.as_str(), "Bytes" | "Multipart" | "Request" | "String")
            {
                let diagnostic = self
                    .warning(
                        "xuwe::unbounded_request_body",
                        argument.ty.span(),
                        format!("消费请求正文的 `{name}` 没有在当前模块声明 DefaultBodyLimit"),
                    )
                    .with_help("按接口场景设置 DefaultBodyLimit；若限制在上层统一配置，请使用带原因的局部豁免");
                self.push(diagnostic, argument.ty.span());
            }
        }
    }

    fn check_test_attribute(&mut self, attribute: &Attribute) {
        let is_test_condition = matches!(
            attribute
                .path()
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .as_deref(),
            Some("cfg" | "cfg_attr")
        ) && attribute.meta.to_token_stream_string().contains("test");
        if !is_test_condition {
            return;
        }
        let diagnostic = self
            .error(
                "xuwe::inline_test_module",
                attribute.span(),
                "生产源码中出现了 test 条件编译",
            )
            .with_help("把测试移动到对应 crate 的 tests/ 集成测试目录");
        self.push(diagnostic, attribute.span());
    }
}

impl<'ast> Visit<'ast> for SourceRuleVisitor<'_> {
    fn visit_attribute(&mut self, node: &'ast Attribute) {
        self.check_action_derive(node);
        self.check_contract_attribute(node);
        self.check_test_attribute(node);
        visit::visit_attribute(self, node);
    }

    fn visit_expr_async(&mut self, node: &'ast ExprAsync) {
        self.deferred_depth += 1;
        visit::visit_expr_async(self, node);
        self.deferred_depth -= 1;
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        self.check_render_call(node);
        self.check_dynamic_sql_call(node);
        self.check_hardcoded_color(node);
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast ExprClosure) {
        self.deferred_depth += 1;
        visit::visit_expr_closure(self, node);
        self.deferred_depth -= 1;
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        self.check_action_macro(&node.mac);
        visit::visit_expr_macro(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        self.check_render_method_call(node);
        self.check_empty_handler(node);
        self.check_rest_route(node);
        self.check_dynamic_query_builder(node);
        self.check_detach(node);
        self.check_unstable_id(node);
        self.check_global_refresh(node);
        self.check_icon_button(node);

        self.receiver_chain_depth += 1;
        self.visit_expr(&node.receiver);
        self.receiver_chain_depth -= 1;
        let receiver_depth = self.receiver_chain_depth;
        self.receiver_chain_depth = 0;
        for argument in &node.args {
            self.visit_expr(argument);
        }
        self.receiver_chain_depth = receiver_depth;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        self.check_axum_signature(&node.sig);
        for attribute in &node.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_function_body(&node.sig.ident, &node.block);
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.check_axum_signature(&node.sig);
        for attribute in &node.attrs {
            self.visit_attribute(attribute);
        }
        self.visit_function_body(&node.sig.ident, &node.block);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if node.ident == "tests" {
            let diagnostic = self
                .error(
                    "xuwe::inline_test_module",
                    node.ident.span(),
                    "生产源码中声明了 tests 模块",
                )
                .with_help("把测试移动到对应 crate 的 tests/ 集成测试目录");
            self.push(diagnostic, node.ident.span());
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast ItemStatic) {
        self.check_global_static(node);
        visit::visit_item_static(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        self.check_copied_global(node);
        visit::visit_item_struct(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.check_action_macro(node);
        let name = node
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if self.is_gpui && name.as_deref() == Some("thread_local") {
            let diagnostic = self
                .error(
                    "xuwe::non_gpui_global_state",
                    node.path.span(),
                    "GPUI crate 使用 thread_local! 保存状态",
                )
                .with_help("应用级状态使用 GPUI Global，组件共享状态使用 Entity<Model>");
            self.push(diagnostic, node.path.span());
        }
        visit::visit_macro(self, node);
    }

    fn visit_stmt(&mut self, node: &'ast Stmt) {
        self.check_lifecycle_statement(node);
        visit::visit_stmt(self, node);
    }
}

struct PanicVisitor {
    can_panic: bool,
}

impl<'ast> Visit<'ast> for PanicVisitor {
    fn visit_expr_closure(&mut self, _: &'ast ExprClosure) {}

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if matches!(node.method.to_string().as_str(), "expect" | "unwrap") {
            self.can_panic = true;
            return;
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        let name = node
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if matches!(
            name.as_deref(),
            Some(
                "assert"
                    | "assert_eq"
                    | "assert_ne"
                    | "panic"
                    | "todo"
                    | "unimplemented"
                    | "unreachable"
            )
        ) {
            self.can_panic = true;
            return;
        }
        visit::visit_macro(self, node);
    }
}

trait MetaTokens {
    fn to_token_stream_string(&self) -> String;
}

impl MetaTokens for Meta {
    fn to_token_stream_string(&self) -> String {
        match self {
            Self::List(list) => list.tokens.to_string(),
            Self::NameValue(name_value) => match &name_value.value {
                Expr::Lit(literal) => match &literal.lit {
                    Lit::Str(value) => value.value(),
                    _ => String::new(),
                },
                _ => String::new(),
            },
            Self::Path(path) => path_segments(path).join("::"),
        }
    }
}

fn collect_rust_files(directory: &Path, output: &mut Vec<std::path::PathBuf>) -> CliResult<()> {
    for entry in fs::read_dir(directory).map_err(|error| {
        CliError::new(format!(
            "无法扫描 Rust 源码目录 {}：{error}",
            directory.display()
        ))
    })? {
        let entry =
            entry.map_err(|error| CliError::new(format!("无法读取 Rust 源码目录项：{error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, output)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
    Ok(())
}

fn doc_text(attributes: &[Attribute]) -> String {
    attributes
        .iter()
        .filter_map(|attribute| {
            if !attribute.path().is_ident("doc") {
                return None;
            }
            let Meta::NameValue(name_value) = &attribute.meta else {
                return None;
            };
            let Expr::Lit(literal) = &name_value.value else {
                return None;
            };
            let Lit::Str(value) = &literal.lit else {
                return None;
            };
            Some(value.value())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_doc_hidden(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("doc")
            && matches!(&attribute.meta, Meta::List(list) if list.tokens.to_string().contains("hidden"))
    })
}

fn contains_chinese(value: &str) -> bool {
    value.chars().any(|character| {
        matches!(
            character,
            '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
        )
    })
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn returns_result(signature: &Signature) -> bool {
    let ReturnType::Type(_, output) = &signature.output else {
        return false;
    };
    type_last_ident(output).is_some_and(|ident| ident.ends_with("Result"))
}

fn block_can_panic(block: &syn::Block) -> bool {
    let mut visitor = PanicVisitor { can_panic: false };
    visitor.visit_block(block);
    visitor.can_panic
}

fn type_last_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Group(group) => type_last_ident(&group.elem),
        Type::Paren(paren) => type_last_ident(&paren.elem),
        Type::Reference(reference) => type_last_ident(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn type_contains_any(ty: &Type, names: &[&str]) -> bool {
    struct TypeVisitor<'a> {
        names: &'a [&'a str],
        found: bool,
    }

    impl<'ast> Visit<'ast> for TypeVisitor<'_> {
        fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
            if node
                .path
                .segments
                .iter()
                .any(|segment| self.names.contains(&segment.ident.to_string().as_str()))
            {
                self.found = true;
                return;
            }
            visit::visit_type_path(self, node);
        }
    }

    let mut visitor = TypeVisitor {
        names,
        found: false,
    };
    visitor.visit_type(ty);
    visitor.found
}

fn is_empty_handler_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Closure(closure) => match closure.body.as_ref() {
            Expr::Block(block) => block.block.stmts.is_empty(),
            Expr::Tuple(tuple) => tuple.elems.is_empty(),
            _ => false,
        },
        _ => false,
    }
}

fn expression_has_unstable_id(expression: &Expr) -> bool {
    struct IdVisitor {
        unstable: bool,
    }

    impl<'ast> Visit<'ast> for IdVisitor {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            let path = expression_path(&node.func)
                .map(path_segments)
                .unwrap_or_default();
            if path.iter().any(|segment| {
                matches!(
                    segment.as_str(),
                    "SystemTime" | "now" | "random" | "thread_rng"
                )
            }) {
                self.unstable = true;
                return;
            }
            visit::visit_expr_call(self, node);
        }

        fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
            if node.path.segments.iter().any(|segment| {
                UNSTABLE_ID_NAMES.contains(&segment.ident.to_string().to_lowercase().as_str())
            }) {
                self.unstable = true;
                return;
            }
            visit::visit_expr_path(self, node);
        }
    }

    let mut visitor = IdVisitor { unstable: false };
    visitor.visit_expr(expression);
    visitor.unstable
}

fn expression_contains_dynamic_sql(expression: &Expr) -> bool {
    struct DynamicSqlVisitor {
        dynamic: bool,
    }

    impl<'ast> Visit<'ast> for DynamicSqlVisitor {
        fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
            if matches!(node.op, syn::BinOp::Add(_)) {
                self.dynamic = true;
                return;
            }
            visit::visit_expr_binary(self, node);
        }

        fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
            let name = node
                .mac
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string());
            if name.as_deref() == Some("format") {
                self.dynamic = true;
                return;
            }
            visit::visit_expr_macro(self, node);
        }
    }

    let mut visitor = DynamicSqlVisitor { dynamic: false };
    visitor.visit_expr(expression);
    visitor.dynamic
}

fn is_context_receiver(expression: &Expr) -> bool {
    let Expr::Path(path) = expression else {
        return false;
    };
    path.path.segments.last().is_some_and(|segment| {
        matches!(segment.ident.to_string().as_str(), "app" | "context" | "cx")
    })
}

fn button_chain(node: &ExprMethodCall) -> Option<Vec<String>> {
    let mut methods = vec![node.method.to_string()];
    let mut receiver = node.receiver.as_ref();
    loop {
        match receiver {
            Expr::MethodCall(call) => {
                methods.push(call.method.to_string());
                receiver = call.receiver.as_ref();
            }
            Expr::Call(call) => {
                let path = expression_path(&call.func)?;
                let segments = path_segments(path);
                if !ends_with_segments(&segments, &["Button", "new"]) {
                    return None;
                }
                methods.reverse();
                return Some(methods);
            }
            _ => return None,
        }
    }
}

fn expression_path(expression: &Expr) -> Option<&syn::Path> {
    match expression {
        Expr::Path(path) => Some(&path.path),
        _ => None,
    }
}

fn path_segments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

fn starts_with_segments(actual: &[String], expected: &[&str]) -> bool {
    actual.len() >= expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

fn ends_with_segments(actual: &[String], expected: &[&str]) -> bool {
    actual.len() >= expected.len()
        && actual[actual.len() - expected.len()..]
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

fn span_line(span: Span) -> usize {
    span.start().line
}

fn span_column(span: Span) -> usize {
    span.start().column + 1
}
