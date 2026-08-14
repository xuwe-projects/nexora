//! `CrudTableRow` 单列展示与状态组件规则。

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use proc_macro2::Span;
use syn::{
    Attribute, Expr, ExprCall, ExprField, ExprMacro, ExprMatch, ExprMethodCall, ExprPath, Field,
    FnArg, ImplItem, ImplItemFn, Item, ItemFn, ItemImpl, ItemStruct, Member, Pat, Signature, Token,
    Type,
    parse::Parser as _,
    punctuated::Punctuated,
    spanned::Spanned as _,
    visit::{self, Visit},
};

use super::diagnostic::{Diagnostic, Report};

const MERGED_COLUMN_RULE: &str = "nexora::crud_table_merged_column";
const BOOLEAN_STATUS_RULE: &str = "nexora::crud_table_boolean_status_without_switch";
const STATUS_TAG_RULE: &str = "nexora::crud_table_status_without_tag";
const INVALID_STATUS_TAG_RULE: &str = "nexora::crud_table_invalid_status_tag";

/// 检查单个 Rust 文件中的 `CrudTableRow` 派生结构体。
pub(super) fn check_file(syntax: &syn::File, path: PathBuf, report: &mut Report) {
    let functions = FunctionIndex::new(syntax);
    for item in &syntax.items {
        let Item::Struct(row) = item else {
            continue;
        };
        if derives_crud_table_row(&row.attrs) {
            check_row(row, &functions, &path, report);
        }
    }
}

fn check_row(row: &ItemStruct, functions: &FunctionIndex<'_>, path: &Path, report: &mut Report) {
    let fields = row
        .fields
        .iter()
        .filter_map(parse_row_field)
        .collect::<Vec<_>>();
    let row_id = fields
        .iter()
        .find(|field| field.row_id)
        .map(|field| field.name.as_str());

    for field in fields.iter().filter(|field| field.column.is_some()) {
        let column = field.column.as_ref().expect("已过滤非列字段");
        let obvious_status = is_obvious_status(field);
        let is_status = column.status || obvious_status;
        if obvious_status && !column.status {
            report.push(
                error(
                    STATUS_TAG_RULE,
                    path,
                    field.span,
                    format!("明显的状态字段 `{}` 缺少 column(status) 声明", field.name),
                )
                .with_help(
                    "为业务状态列添加 status、render 与 text；分类标签不要使用 status 字段名或 Status/State 类型",
                ),
            );
        }

        let Some(render) = column.render.as_ref() else {
            continue;
        };
        let Some(function) = functions.resolve(row, render) else {
            report.push(
                error(
                    MERGED_COLUMN_RULE,
                    path,
                    render.span(),
                    format!(
                        "列 `{}` 的渲染器 `{}` 无法在当前文件中解析",
                        field.name,
                        path_text(render)
                    ),
                )
                .with_help("把渲染器保留在当前模块，并让它只接收和展示本列字段"),
            );
            continue;
        };

        let row_parameter = row_parameter_name(function.signature);
        let mut analysis = RenderAnalysis::new(row_parameter, &field.name, row_id);
        analysis.visit_block(function.block);
        report_merged_column(field, &analysis, path, report);

        if is_status {
            report_status(field, column, &analysis, path, report);
        }
    }
}

fn report_merged_column(
    field: &RowField,
    analysis: &RenderAnalysis<'_>,
    path: &Path,
    report: &mut Report,
) {
    if let Some((foreign, span)) = &analysis.foreign_field {
        report.push(
            error(
                MERGED_COLUMN_RULE,
                path,
                *span,
                format!("列 `{}` 的渲染器读取了其他行字段 `{foreign}`", field.name),
            )
            .with_help("把每个业务值拆成独立列；行 ID 只能用于 TableSwitchCell 的稳定元素 ID"),
        );
        return;
    }
    if let Some(span) = analysis.whole_row {
        report.push(
            error(
                MERGED_COLUMN_RULE,
                path,
                span,
                format!("列 `{}` 的渲染器把完整行传给了不透明逻辑", field.name),
            )
            .with_help("只向格式化 helper 传入本列字段，不要传入完整 row/self"),
        );
        return;
    }
    if let Some(span) = analysis.multiline {
        report.push(
            error(
                MERGED_COLUMN_RULE,
                path,
                span,
                format!("列 `{}` 使用了纵向、折行或多行单元格布局", field.name),
            )
            .with_help("当前表格正文没有独立行高，需改为单行内容或拆成独立列"),
        );
    }
}

fn report_status(
    field: &RowField,
    column: &ColumnMetadata,
    analysis: &RenderAnalysis<'_>,
    path: &Path,
    report: &mut Report,
) {
    let boolean_status = is_bool(field) || is_binary_status_name(&field.name);
    if boolean_status {
        if !analysis.has_table_switch {
            report.push(
                error(
                    BOOLEAN_STATUS_RULE,
                    path,
                    column
                        .render
                        .as_ref()
                        .map_or(field.span, |render| render.span()),
                    format!(
                        "布尔或开关状态列 `{}` 没有使用 TableSwitchCell",
                        field.name
                    ),
                )
                .with_help(
                    "使用 nexora::desktop::TableSwitchCell；不要在标准 CRUD 表格中直接使用文本、Tag 或裸 Switch",
                ),
            );
        }
        return;
    }

    if !analysis.has_tag {
        report.push(
            error(
                STATUS_TAG_RULE,
                path,
                column
                    .render
                    .as_ref()
                    .map_or(field.span, |render| render.span()),
                format!("非布尔状态列 `{}` 没有使用官方 Tag", field.name),
            )
            .with_help("使用 Tag 的 Primary、Secondary、Info、Success、Warning 或 Danger 填充变体"),
        );
        return;
    }

    let invalid_style = analysis.has_outline
        || analysis.has_custom_or_color
        || analysis.semantic_variants.is_empty();
    let uniform_mapping = analysis.branch_count > 1 && analysis.semantic_variants.len() < 2;
    if invalid_style || uniform_mapping {
        report.push(
            error(
                INVALID_STATUS_TAG_RULE,
                path,
                analysis.invalid_tag_span.unwrap_or(field.span),
                format!("状态列 `{}` 没有使用有效的填充语义颜色映射", field.name),
            )
            .with_help(
                "禁止 outline、Custom 和 Color；按状态语义使用至少两个 Primary/Secondary/Info/Success/Warning/Danger 变体",
            ),
        );
    }
}

#[derive(Default)]
struct ColumnMetadata {
    status: bool,
    render: Option<ExprPath>,
}

struct RowField {
    name: String,
    type_name: Option<String>,
    span: Span,
    row_id: bool,
    column: Option<ColumnMetadata>,
}

fn parse_row_field(field: &Field) -> Option<RowField> {
    let name = field.ident.as_ref()?.to_string();
    let mut row_id = false;
    let mut column = None;
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("nexora"))
    {
        let _ = attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("row_id") {
                row_id = true;
                return Ok(());
            }
            if meta.path.is_ident("column") {
                let mut metadata = ColumnMetadata::default();
                if !meta.input.is_empty() {
                    meta.parse_nested_meta(|nested| {
                        if nested.path.is_ident("status") {
                            metadata.status = true;
                            return Ok(());
                        }
                        if nested.path.is_ident("render") {
                            metadata.render = Some(nested.value()?.parse()?);
                            return Ok(());
                        }
                        consume_nested_input(nested)
                    })?;
                }
                column = Some(metadata);
                return Ok(());
            }
            consume_nested_input(meta)
        });
    }
    Some(RowField {
        name,
        type_name: type_last_ident(&field.ty),
        span: field.span(),
        row_id,
        column,
    })
}

fn consume_nested_input(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(Token![=]) {
        let _: Expr = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(consume_nested_input)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct FunctionBody<'ast> {
    signature: &'ast Signature,
    block: &'ast syn::Block,
}

struct FunctionIndex<'ast> {
    methods: HashMap<(String, String), FunctionBody<'ast>>,
    functions: HashMap<String, FunctionBody<'ast>>,
}

impl<'ast> FunctionIndex<'ast> {
    fn new(syntax: &'ast syn::File) -> Self {
        let mut index = Self {
            methods: HashMap::new(),
            functions: HashMap::new(),
        };
        for item in &syntax.items {
            match item {
                Item::Fn(function) => index.insert_function(function),
                Item::Impl(implementation) => index.insert_impl(implementation),
                _ => {}
            }
        }
        index
    }

    fn insert_function(&mut self, function: &'ast ItemFn) {
        self.functions.insert(
            function.sig.ident.to_string(),
            FunctionBody {
                signature: &function.sig,
                block: &function.block,
            },
        );
    }

    fn insert_impl(&mut self, implementation: &'ast ItemImpl) {
        let Some(type_name) = type_last_ident(&implementation.self_ty) else {
            return;
        };
        for item in &implementation.items {
            let ImplItem::Fn(function) = item else {
                continue;
            };
            self.insert_method(&type_name, function);
        }
    }

    fn insert_method(&mut self, type_name: &str, function: &'ast ImplItemFn) {
        self.methods.insert(
            (type_name.to_owned(), function.sig.ident.to_string()),
            FunctionBody {
                signature: &function.sig,
                block: &function.block,
            },
        );
    }

    fn resolve(&self, row: &ItemStruct, path: &ExprPath) -> Option<FunctionBody<'ast>> {
        let function = path.path.segments.last()?.ident.to_string();
        let method = self
            .methods
            .get(&(row.ident.to_string(), function.clone()))
            .copied();
        if path.path.segments.len() == 1
            || path
                .path
                .segments
                .first()
                .is_some_and(|segment| segment.ident == "Self" || segment.ident == row.ident)
        {
            method.or_else(|| self.functions.get(&function).copied())
        } else {
            None
        }
    }
}

struct RenderAnalysis<'a> {
    row_parameter: Option<String>,
    owner: &'a str,
    row_id: Option<&'a str>,
    allow_row_id_depth: usize,
    field_base_depth: usize,
    foreign_field: Option<(String, Span)>,
    whole_row: Option<Span>,
    multiline: Option<Span>,
    has_table_switch: bool,
    has_tag: bool,
    has_outline: bool,
    has_custom_or_color: bool,
    invalid_tag_span: Option<Span>,
    semantic_variants: std::collections::HashSet<String>,
    branch_count: usize,
}

impl<'a> RenderAnalysis<'a> {
    fn new(row_parameter: Option<String>, owner: &'a str, row_id: Option<&'a str>) -> Self {
        Self {
            row_parameter,
            owner,
            row_id,
            allow_row_id_depth: 0,
            field_base_depth: 0,
            foreign_field: None,
            whole_row: None,
            multiline: None,
            has_table_switch: false,
            has_tag: false,
            has_outline: false,
            has_custom_or_color: false,
            invalid_tag_span: None,
            semantic_variants: std::collections::HashSet::new(),
            branch_count: 0,
        }
    }

    fn inspect_component_call(&mut self, node: &ExprCall) -> bool {
        let Some(path) = expression_path(&node.func) else {
            return false;
        };
        let segments = path_segments(path);
        if ends_with(&segments, &["TableSwitchCell", "new"]) {
            self.has_table_switch = true;
            self.visit_expr(&node.func);
            for (index, argument) in node.args.iter().enumerate() {
                if index == 0 {
                    self.allow_row_id_depth += 1;
                }
                self.visit_expr(argument);
                if index == 0 {
                    self.allow_row_id_depth -= 1;
                }
            }
            return true;
        }
        if segments.len() >= 2 && segments[segments.len() - 2] == "Tag" {
            self.has_tag = true;
            let constructor = segments.last().expect("已检查 Tag 构造器");
            self.record_tag_variant(constructor, node.span());
        }
        false
    }

    fn record_tag_variant(&mut self, variant: &str, span: Span) {
        match variant {
            "primary" | "Primary" => {
                self.semantic_variants.insert("Primary".to_owned());
            }
            "secondary" | "Secondary" => {
                self.semantic_variants.insert("Secondary".to_owned());
            }
            "info" | "Info" => {
                self.semantic_variants.insert("Info".to_owned());
            }
            "success" | "Success" => {
                self.semantic_variants.insert("Success".to_owned());
            }
            "warning" | "Warning" => {
                self.semantic_variants.insert("Warning".to_owned());
            }
            "danger" | "Danger" => {
                self.semantic_variants.insert("Danger".to_owned());
            }
            "color" | "Color" | "custom" | "Custom" => {
                self.has_custom_or_color = true;
                self.invalid_tag_span.get_or_insert(span);
            }
            _ => {}
        }
    }

    fn visit_macro_arguments(&mut self, node: &ExprMacro) {
        let name = node.mac.path.segments.last().map(|segment| &segment.ident);
        if !name.is_some_and(|name| name == "format" || name == "format_args") {
            return;
        }
        let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
        if let Ok(arguments) = parser.parse2(node.mac.tokens.clone()) {
            for argument in &arguments {
                self.visit_expr(argument);
            }
        }
    }
}

impl<'ast> Visit<'ast> for RenderAnalysis<'_> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.inspect_component_call(node) {
            return;
        }
        if expression_path(&node.func)
            .and_then(|path| path.segments.last())
            .is_some_and(|segment| segment.ident == "v_flex")
        {
            self.multiline.get_or_insert(node.span());
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast ExprField) {
        if let Some(field) = row_field_name(node, self.row_parameter.as_deref())
            && field != self.owner
            && !(self.row_id == Some(field.as_str()) && self.allow_row_id_depth > 0)
        {
            self.foreign_field.get_or_insert((field, node.span()));
        }

        self.field_base_depth += 1;
        self.visit_expr(&node.base);
        self.field_base_depth -= 1;
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        self.visit_macro_arguments(node);
    }

    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        self.branch_count = self.branch_count.max(node.arms.len());
        visit::visit_expr_match(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method = node.method.to_string();
        if matches!(
            method.as_str(),
            "flex_col" | "flex_wrap" | "whitespace_normal"
        ) {
            self.multiline.get_or_insert(node.method.span());
        }
        if method == "outline" {
            self.has_outline = true;
            self.invalid_tag_span.get_or_insert(node.method.span());
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        let segments = path_segments(&node.path);
        if segments.len() >= 2 && segments[segments.len() - 2] == "TagVariant" {
            self.record_tag_variant(
                segments.last().expect("已检查 TagVariant 路径"),
                node.span(),
            );
        }
        if self.field_base_depth == 0
            && self.row_parameter.as_deref().is_some_and(|row| {
                segments.len() == 1 && segments.first().is_some_and(|segment| segment == row)
            })
        {
            self.whole_row.get_or_insert(node.span());
        }
        visit::visit_expr_path(self, node);
    }
}

fn row_parameter_name(signature: &Signature) -> Option<String> {
    let first = signature.inputs.first()?;
    match first {
        FnArg::Receiver(_) => Some("self".to_owned()),
        FnArg::Typed(argument) => match argument.pat.as_ref() {
            Pat::Ident(ident) => Some(ident.ident.to_string()),
            _ => None,
        },
    }
}

fn row_field_name(field: &ExprField, row_parameter: Option<&str>) -> Option<String> {
    let row_parameter = row_parameter?;
    let root = root_expression(&field.base);
    let Expr::Path(path) = root else {
        return None;
    };
    if path.path.segments.len() != 1 || path.path.segments.first()?.ident != row_parameter {
        return None;
    }
    root_member_name(field)
}

fn root_expression(expression: &Expr) -> &Expr {
    match expression {
        Expr::Field(field) => root_expression(&field.base),
        Expr::Paren(paren) => root_expression(&paren.expr),
        Expr::Reference(reference) => root_expression(&reference.expr),
        _ => expression,
    }
}

fn root_member_name(field: &ExprField) -> Option<String> {
    match field.base.as_ref() {
        Expr::Field(parent) => root_member_name(parent),
        Expr::Paren(paren) => match paren.expr.as_ref() {
            Expr::Field(parent) => root_member_name(parent),
            _ => member_name(&field.member),
        },
        _ => member_name(&field.member),
    }
}

fn member_name(member: &Member) -> Option<String> {
    match member {
        Member::Named(ident) => Some(ident.to_string()),
        Member::Unnamed(_) => None,
    }
}

fn derives_crud_table_row(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("derive")
            && attribute
                .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .is_ok_and(|paths| {
                    paths.iter().any(|path| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == "CrudTableRow")
                    })
                })
    })
}

fn is_obvious_status(field: &RowField) -> bool {
    is_status_name(&field.name)
        || field
            .type_name
            .as_deref()
            .is_some_and(|name| name.ends_with("Status") || name.ends_with("State"))
}

fn is_status_name(name: &str) -> bool {
    matches!(
        name,
        "status" | "state" | "enabled" | "disabled" | "active" | "locked"
    ) || name.ends_with("_status")
        || name.ends_with("_state")
}

fn is_binary_status_name(name: &str) -> bool {
    matches!(name, "enabled" | "disabled" | "active" | "locked")
        || name.ends_with("_enabled")
        || name.ends_with("_disabled")
        || name.ends_with("_active")
        || name.ends_with("_locked")
}

fn is_bool(field: &RowField) -> bool {
    field.type_name.as_deref() == Some("bool")
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

fn ends_with(actual: &[String], expected: &[&str]) -> bool {
    actual.len() >= expected.len()
        && actual[actual.len() - expected.len()..]
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

fn path_text(path: &ExprPath) -> String {
    path.path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn error(rule: &'static str, path: &Path, span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        rule,
        path.to_path_buf(),
        span.start().line,
        span.start().column + 1,
        message,
    )
}
