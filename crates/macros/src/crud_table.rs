//! `CrudTableRow` 派生宏实现。

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Error, Expr, ExprLit, ExprPath, Field, Fields, Ident, Lit,
    LitBool, LitStr, Result, Token, meta::ParseNestedMeta, spanned::Spanned as _,
};

use crate::{nexora_path, parse_string, reject_generics, set_once};

#[derive(Default)]
struct ColumnArguments {
    key: Option<LitStr>,
    name: Option<LitStr>,
    width: Option<Expr>,
    min_width: Option<Expr>,
    max_width: Option<Expr>,
    sort: Option<ColumnSortMode>,
    server_sort: Option<ServerSortMapping>,
    fixed_left: bool,
    resizable: Option<bool>,
    movable: Option<bool>,
    selectable: Option<bool>,
    header_align: Option<Align>,
    cell_align: Option<Align>,
    vertical_align: Option<VerticalAlign>,
    status: Option<proc_macro2::Span>,
    render: Option<ExprPath>,
    text: Option<ExprPath>,
}

struct ServerSortMapping {
    ascending: ExprPath,
    descending: ExprPath,
}

#[derive(Clone, Copy)]
enum ColumnSortMode {
    Default(proc_macro2::Span),
    Ascending(proc_macro2::Span),
    Descending(proc_macro2::Span),
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy)]
enum VerticalAlign {
    Top,
    Center,
    Bottom,
}

struct ColumnField {
    field_ident: Ident,
    key: LitStr,
    name: LitStr,
    arguments: ColumnArguments,
}

struct RowIdField {
    field_ident: Ident,
    ty: TokenStream,
}

struct ParsedCrudTableRow {
    columns: Vec<ColumnField>,
    row_id: RowIdField,
}

pub(crate) fn expand_crud_table_row(input: DeriveInput) -> Result<TokenStream> {
    reject_generics(&input, "CrudTableRow")?;
    let parsed = parse_crud_table_row(&input)?;
    let fields = parsed.columns;
    if fields.is_empty() {
        return Err(Error::new_spanned(
            input,
            "CrudTableRow 至少需要一个 #[nexora(column)] 字段",
        ));
    }
    validate_unique_keys(&fields)?;

    let ident = &input.ident;
    let nexora = nexora_path();
    let column_definitions = fields
        .iter()
        .map(|field| expand_column_definition(field, &nexora))
        .collect::<Vec<_>>();
    let header_arms = fields
        .iter()
        .map(|field| expand_header_alignment_arm(field, &nexora));
    let cell_align_arms = fields
        .iter()
        .map(|field| expand_cell_alignment_arm(field, &nexora));
    let vertical_align_arms = fields
        .iter()
        .map(|field| expand_vertical_alignment_arm(field, &nexora));
    let render_arms = fields
        .iter()
        .map(|field| expand_render_cell_arm(field, &nexora));
    let text_arms = fields.iter().map(expand_cell_text_arm);
    let server_sort_arms = fields.iter().filter_map(expand_server_sort_arm);
    let sort_type = infer_sort_type(&fields, &nexora)?;
    let row_id_ident = &parsed.row_id.field_ident;
    let row_id_ty = &parsed.row_id.ty;

    Ok(quote! {
        impl #nexora::desktop::CrudTableRow for #ident {
            type Id = #row_id_ty;
            type Sort = #sort_type;

            fn row_id(&self) -> &Self::Id {
                &self.#row_id_ident
            }

            fn columns() -> ::std::vec::Vec<#nexora::__private::gpui_component::table::Column> {
                ::std::vec![#(#column_definitions),*]
            }

            fn server_sort(
                key: &str,
                sort: #nexora::desktop::CrudColumnSort,
            ) -> ::core::option::Option<Self::Sort> {
                match (key, sort) {
                    #(#server_sort_arms,)*
                    _ => ::core::option::Option::None,
                }
            }

            fn header_alignment(key: &str) -> #nexora::__private::gpui::TextAlign {
                match key {
                    #(#header_arms,)*
                    _ => #nexora::__private::gpui::TextAlign::Center,
                }
            }

            fn cell_alignment(key: &str) -> #nexora::__private::gpui::TextAlign {
                match key {
                    #(#cell_align_arms,)*
                    _ => #nexora::__private::gpui::TextAlign::Left,
                }
            }

            fn cell_vertical_alignment(key: &str) -> #nexora::desktop::TableCellVerticalAlign {
                match key {
                    #(#vertical_align_arms,)*
                    _ => #nexora::desktop::TableCellVerticalAlign::Center,
                }
            }

            fn render_cell(
                &self,
                key: &str,
                window: &mut #nexora::__private::gpui::Window,
                cx: &mut #nexora::__private::gpui::App,
            ) -> #nexora::__private::gpui::AnyElement {
                match key {
                    #(#render_arms,)*
                    _ => #nexora::__private::gpui::IntoElement::into_any_element(
                        #nexora::__private::gpui::Empty,
                    ),
                }
            }

            fn cell_text(&self, key: &str, cx: &#nexora::__private::gpui::App) -> ::std::string::String {
                match key {
                    #(#text_arms,)*
                    _ => ::std::string::String::new(),
                }
            }
        }
    })
}

fn parse_crud_table_row(input: &DeriveInput) -> Result<ParsedCrudTableRow> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "CrudTableRow 只能派生在具有命名字段的结构体上",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            &data.fields,
            "CrudTableRow 只能派生在具有命名字段的结构体上",
        ));
    };

    let mut columns = Vec::new();
    let mut row_id = None;
    for field in &fields.named {
        let parsed = parse_field(field)?;
        if parsed.row_id {
            if row_id.is_some() {
                return Err(Error::new_spanned(
                    field,
                    "CrudTableRow 必须且只能声明一个 #[nexora(row_id)] 字段",
                ));
            }
            let Some(field_ident) = field.ident.clone() else {
                return Err(Error::new_spanned(
                    field,
                    "CrudTableRow 只能派生在具有命名字段的结构体上",
                ));
            };
            let ty = &field.ty;
            row_id = Some(RowIdField {
                field_ident,
                ty: quote!(#ty),
            });
        }
        if let Some(column) = parsed.column {
            columns.push(column);
        }
    }
    let Some(row_id) = row_id else {
        return Err(Error::new_spanned(
            input,
            "CrudTableRow 必须声明一个 #[nexora(row_id)] 字段",
        ));
    };

    Ok(ParsedCrudTableRow { columns, row_id })
}

struct ParsedField {
    column: Option<ColumnField>,
    row_id: bool,
}

fn parse_field(field: &Field) -> Result<ParsedField> {
    let mut column = None;
    let mut skip = false;
    let mut row_id = false;
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("nexora"))
    {
        parse_field_attribute(attribute, &mut column, &mut skip, &mut row_id)?;
    }

    if skip && column.is_some() {
        return Err(Error::new_spanned(
            field,
            "同一个字段不能同时声明 skip 和 column",
        ));
    }
    if skip {
        return Ok(ParsedField {
            column: None,
            row_id,
        });
    }
    let Some(mut arguments) = column else {
        return Ok(ParsedField {
            column: None,
            row_id,
        });
    };
    let Some(field_ident) = field.ident.clone() else {
        return Err(Error::new_spanned(
            field,
            "CrudTableRow 只能派生在具有命名字段的结构体上",
        ));
    };
    let default_name = field_ident.to_string();
    let default_name = default_name.strip_prefix("r#").unwrap_or(&default_name);
    let key = arguments
        .key
        .take()
        .unwrap_or_else(|| LitStr::new(default_name, field_ident.span()));
    let name = arguments
        .name
        .take()
        .unwrap_or_else(|| LitStr::new(default_name, field_ident.span()));
    if arguments.sort.is_some() && arguments.server_sort.is_none() {
        return Err(Error::new_spanned(
            field,
            "可排序列必须声明 sort(asc = Sort::Asc, desc = Sort::Desc) 服务端映射",
        ));
    }
    if arguments.sort.is_none() && arguments.server_sort.is_some() {
        return Err(Error::new_spanned(
            field,
            "sort(asc = ..., desc = ...) 必须与 sortable、ascending 或 descending 同时声明",
        ));
    }
    if let Some(status_span) = arguments.status {
        if row_id {
            return Err(Error::new(
                status_span,
                "状态列不能同时声明为 CrudTableRow row_id",
            ));
        }
        if arguments.render.is_none() {
            return Err(Error::new(
                status_span,
                "状态列必须声明 render = Self::render_status",
            ));
        }
        if arguments.text.is_none() {
            return Err(Error::new(
                status_span,
                "状态列必须声明 text = Self::status_text",
            ));
        }
    }

    Ok(ParsedField {
        column: Some(ColumnField {
            field_ident,
            key,
            name,
            arguments,
        }),
        row_id,
    })
}

fn parse_field_attribute(
    attribute: &Attribute,
    column: &mut Option<ColumnArguments>,
    skip: &mut bool,
    row_id: &mut bool,
) -> Result<()> {
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("column") {
            if column.is_some() {
                return Err(meta.error("column 只能声明一次"));
            }
            let mut arguments = ColumnArguments::default();
            if !meta.input.is_empty() {
                meta.parse_nested_meta(|nested| parse_column_argument(nested, &mut arguments))?;
            }
            *column = Some(arguments);
            Ok(())
        } else if meta.path.is_ident("skip") {
            if *skip {
                return Err(meta.error("skip 只能声明一次"));
            }
            *skip = true;
            Ok(())
        } else if meta.path.is_ident("row_id") {
            if *row_id {
                return Err(meta.error("row_id 只能声明一次"));
            }
            *row_id = true;
            Ok(())
        } else {
            Err(meta.error("CrudTableRow 字段属性只支持 row_id、column(...) 或 skip"))
        }
    })
}

fn parse_column_argument(meta: ParseNestedMeta<'_>, arguments: &mut ColumnArguments) -> Result<()> {
    if meta.path.is_ident("key") {
        set_once(
            &mut arguments.key,
            parse_string(&meta)?,
            meta.path.span(),
            "key",
        )
    } else if meta.path.is_ident("name") || meta.path.is_ident("title") {
        set_once(
            &mut arguments.name,
            parse_string(&meta)?,
            meta.path.span(),
            "name/title",
        )
    } else if meta.path.is_ident("width") {
        set_once(
            &mut arguments.width,
            parse_dimension(&meta)?,
            meta.path.span(),
            "width",
        )
    } else if meta.path.is_ident("min_width") {
        set_once(
            &mut arguments.min_width,
            parse_dimension(&meta)?,
            meta.path.span(),
            "min_width",
        )
    } else if meta.path.is_ident("max_width") {
        set_once(
            &mut arguments.max_width,
            parse_dimension(&meta)?,
            meta.path.span(),
            "max_width",
        )
    } else if meta.path.is_ident("sortable") {
        set_sort(arguments, ColumnSortMode::Default(meta.path.span()))
    } else if meta.path.is_ident("ascending") {
        set_sort(arguments, ColumnSortMode::Ascending(meta.path.span()))
    } else if meta.path.is_ident("descending") {
        set_sort(arguments, ColumnSortMode::Descending(meta.path.span()))
    } else if meta.path.is_ident("sort") {
        if arguments.server_sort.is_some() {
            return Err(meta.error("sort(...) 只能声明一次"));
        }
        let mut ascending = None;
        let mut descending = None;
        meta.parse_nested_meta(|nested| {
            if nested.path.is_ident("asc") {
                set_once(
                    &mut ascending,
                    nested.value()?.parse::<ExprPath>()?,
                    nested.path.span(),
                    "sort.asc",
                )
            } else if nested.path.is_ident("desc") {
                set_once(
                    &mut descending,
                    nested.value()?.parse::<ExprPath>()?,
                    nested.path.span(),
                    "sort.desc",
                )
            } else {
                Err(nested.error("sort(...) 只支持 asc 和 desc"))
            }
        })?;
        let Some(ascending) = ascending else {
            return Err(meta.error("sort(...) 必须声明 asc = Sort::Asc"));
        };
        let Some(descending) = descending else {
            return Err(meta.error("sort(...) 必须声明 desc = Sort::Desc"));
        };
        arguments.server_sort = Some(ServerSortMapping {
            ascending,
            descending,
        });
        Ok(())
    } else if meta.path.is_ident("fixed_left") {
        arguments.fixed_left = parse_bool_or_true(&meta)?;
        Ok(())
    } else if meta.path.is_ident("resizable") {
        set_once(
            &mut arguments.resizable,
            parse_bool_or_true(&meta)?,
            meta.path.span(),
            "resizable",
        )
    } else if meta.path.is_ident("movable") {
        set_once(
            &mut arguments.movable,
            parse_bool_or_true(&meta)?,
            meta.path.span(),
            "movable",
        )
    } else if meta.path.is_ident("selectable") {
        set_once(
            &mut arguments.selectable,
            parse_bool_or_true(&meta)?,
            meta.path.span(),
            "selectable",
        )
    } else if meta.path.is_ident("header_align") {
        set_once(
            &mut arguments.header_align,
            parse_align(&meta)?,
            meta.path.span(),
            "header_align",
        )
    } else if meta.path.is_ident("align") || meta.path.is_ident("cell_align") {
        set_once(
            &mut arguments.cell_align,
            parse_align(&meta)?,
            meta.path.span(),
            "align/cell_align",
        )
    } else if meta.path.is_ident("vertical_align") {
        set_once(
            &mut arguments.vertical_align,
            parse_vertical_align(&meta)?,
            meta.path.span(),
            "vertical_align",
        )
    } else if meta.path.is_ident("status") {
        if arguments.status.is_some() {
            return Err(meta.error("status 只能声明一次"));
        }
        if meta.input.peek(Token![=]) || meta.input.peek(syn::token::Paren) {
            return Err(meta.error("status 是无值标记，应写作 status"));
        }
        arguments.status = Some(meta.path.span());
        Ok(())
    } else if meta.path.is_ident("render") {
        set_once(
            &mut arguments.render,
            meta.value()?.parse::<ExprPath>()?,
            meta.path.span(),
            "render",
        )
    } else if meta.path.is_ident("text") {
        set_once(
            &mut arguments.text,
            meta.value()?.parse::<ExprPath>()?,
            meta.path.span(),
            "text",
        )
    } else {
        Err(meta.error("column 属性支持 key、name/title、width、min_width、max_width、sortable、ascending、descending、sort(...)、fixed_left、resizable、movable、selectable、header_align、align/cell_align、vertical_align、status、render 和 text"))
    }
}

fn parse_dimension(meta: &ParseNestedMeta<'_>) -> Result<Expr> {
    let expression = meta.value()?.parse::<Expr>()?;
    match &expression {
        Expr::Lit(ExprLit {
            lit: Lit::Float(_) | Lit::Int(_),
            ..
        }) => Ok(expression),
        _ => Err(Error::new_spanned(
            expression,
            "列宽必须是数字字面量，例如 width = 120. 或 width = 120",
        )),
    }
}

fn parse_bool_or_true(meta: &ParseNestedMeta<'_>) -> Result<bool> {
    if meta.input.peek(Token![=]) {
        Ok(meta.value()?.parse::<LitBool>()?.value())
    } else {
        Ok(true)
    }
}

fn parse_align(meta: &ParseNestedMeta<'_>) -> Result<Align> {
    let value = parse_string(meta)?;
    match value.value().as_str() {
        "left" => Ok(Align::Left),
        "center" => Ok(Align::Center),
        "right" => Ok(Align::Right),
        _ => Err(Error::new(
            value.span(),
            "对齐方式必须是 \"left\"、\"center\" 或 \"right\"",
        )),
    }
}

fn parse_vertical_align(meta: &ParseNestedMeta<'_>) -> Result<VerticalAlign> {
    let value = parse_string(meta)?;
    match value.value().as_str() {
        "top" => Ok(VerticalAlign::Top),
        "center" | "middle" => Ok(VerticalAlign::Center),
        "bottom" => Ok(VerticalAlign::Bottom),
        _ => Err(Error::new(
            value.span(),
            "垂直对齐方式必须是 \"top\"、\"center\"、\"middle\" 或 \"bottom\"",
        )),
    }
}

fn set_sort(arguments: &mut ColumnArguments, mode: ColumnSortMode) -> Result<()> {
    if let Some(previous) = arguments.sort {
        return Err(Error::new(
            mode.span(),
            format!("排序方式只能声明一次，已声明 {}", previous.attribute_name()),
        ));
    }

    arguments.sort = Some(mode);
    Ok(())
}

fn validate_unique_keys(fields: &[ColumnField]) -> Result<()> {
    let mut keys = HashSet::new();
    for field in fields {
        let key = field.key.value();
        if !keys.insert(key.clone()) {
            return Err(Error::new(
                field.key.span(),
                format!("CrudTableRow 列 key `{key}` 重复"),
            ));
        }
    }
    Ok(())
}

fn expand_column_definition(field: &ColumnField, nexora: &TokenStream) -> TokenStream {
    let key = &field.key;
    let name = &field.name;
    let mut column = quote! {
        #nexora::__private::gpui_component::table::Column::new(#key, #name)
    };

    if let Some(width) = &field.arguments.width {
        column = quote!(#column.width(#nexora::__private::gpui::px(#width)));
    }
    if let Some(min_width) = &field.arguments.min_width {
        column = quote!(#column.min_width(#nexora::__private::gpui::px(#min_width)));
    }
    if let Some(max_width) = &field.arguments.max_width {
        column = quote!(#column.max_width(#nexora::__private::gpui::px(#max_width)));
    }
    if let Some(sort) = field.arguments.sort {
        column = match sort {
            ColumnSortMode::Default(_) => quote!(#column.sortable()),
            ColumnSortMode::Ascending(_) => quote!(#column.ascending()),
            ColumnSortMode::Descending(_) => quote!(#column.descending()),
        };
    }
    if field.arguments.fixed_left {
        column = quote!(#column.fixed_left());
    }
    if let Some(resizable) = field.arguments.resizable {
        column = quote!(#column.resizable(#resizable));
    }
    if let Some(movable) = field.arguments.movable {
        column = quote!(#column.movable(#movable));
    }
    if let Some(selectable) = field.arguments.selectable {
        column = quote!(#column.selectable(#selectable));
    }
    match field.arguments.cell_align {
        Some(Align::Center) => column = quote!(#column.text_center()),
        Some(Align::Right) => column = quote!(#column.text_right()),
        Some(Align::Left) | None => {}
    }

    column
}

fn expand_header_alignment_arm(field: &ColumnField, nexora: &TokenStream) -> TokenStream {
    let key = &field.key;
    let align = expand_align(
        field.arguments.header_align.unwrap_or(Align::Center),
        nexora,
    );
    quote!(#key => #align)
}

fn expand_cell_alignment_arm(field: &ColumnField, nexora: &TokenStream) -> TokenStream {
    let key = &field.key;
    let align = expand_align(field.arguments.cell_align.unwrap_or(Align::Left), nexora);
    quote!(#key => #align)
}

fn expand_vertical_alignment_arm(field: &ColumnField, nexora: &TokenStream) -> TokenStream {
    let key = &field.key;
    let align = expand_vertical_align(
        field
            .arguments
            .vertical_align
            .unwrap_or(VerticalAlign::Center),
        nexora,
    );
    quote!(#key => #align)
}

fn expand_render_cell_arm(field: &ColumnField, nexora: &TokenStream) -> TokenStream {
    let key = &field.key;
    let field_ident = &field.field_ident;
    if let Some(render) = &field.arguments.render {
        return quote! {
            #key => #nexora::__private::gpui::IntoElement::into_any_element(
                #render(self, window, cx),
            )
        };
    }

    quote! {
        #key => #nexora::__private::gpui::IntoElement::into_any_element(
            #nexora::desktop::TableCell::new(
                ::std::string::ToString::to_string(&self.#field_ident),
            )
            .align(<Self as #nexora::desktop::CrudTableRow>::cell_alignment(#key))
            .vertical_align(<Self as #nexora::desktop::CrudTableRow>::cell_vertical_alignment(#key)),
        )
    }
}

fn expand_cell_text_arm(field: &ColumnField) -> TokenStream {
    let key = &field.key;
    let field_ident = &field.field_ident;
    if let Some(text) = &field.arguments.text {
        return quote!(#key => #text(self, cx));
    }

    quote!(#key => ::std::string::ToString::to_string(&self.#field_ident))
}

fn expand_server_sort_arm(field: &ColumnField) -> Option<TokenStream> {
    let mapping = field.arguments.server_sort.as_ref()?;
    let key = &field.key;
    let ascending = &mapping.ascending;
    let descending = &mapping.descending;
    let nexora = nexora_path();
    Some(quote! {
        (#key, #nexora::desktop::CrudColumnSort::Ascending) =>
            ::core::option::Option::Some(#ascending),
        (#key, #nexora::desktop::CrudColumnSort::Descending) =>
            ::core::option::Option::Some(#descending)
    })
}

fn infer_sort_type(fields: &[ColumnField], nexora: &TokenStream) -> Result<TokenStream> {
    let mut inferred: Option<(TokenStream, String)> = None;
    for expression in fields
        .iter()
        .filter_map(|field| {
            field
                .arguments
                .server_sort
                .as_ref()
                .map(|mapping| [&mapping.ascending, &mapping.descending])
        })
        .flatten()
    {
        if expression.qself.is_some() || expression.path.segments.len() < 2 {
            return Err(Error::new_spanned(
                expression,
                "排序枚举值必须使用 SortType::Variant 路径",
            ));
        }
        let segments = expression
            .path
            .segments
            .iter()
            .take(expression.path.segments.len() - 1)
            .collect::<Vec<_>>();
        let sort_type = if expression.path.leading_colon.is_some() {
            quote!(::#(#segments)::* )
        } else {
            quote!(#(#segments)::* )
        };
        let identity = sort_type.to_string();
        if let Some((_, expected)) = &inferred {
            if expected != &identity {
                return Err(Error::new_spanned(
                    expression,
                    "同一个 CrudTableRow 的所有排序列必须使用同一种排序枚举",
                ));
            }
        } else {
            inferred = Some((sort_type, identity));
        }
    }

    Ok(inferred.map_or_else(
        || quote!(#nexora::desktop::NoCrudSort),
        |(sort_type, _)| sort_type,
    ))
}

fn expand_align(align: Align, nexora: &TokenStream) -> TokenStream {
    match align {
        Align::Left => quote!(#nexora::__private::gpui::TextAlign::Left),
        Align::Center => quote!(#nexora::__private::gpui::TextAlign::Center),
        Align::Right => quote!(#nexora::__private::gpui::TextAlign::Right),
    }
}

fn expand_vertical_align(align: VerticalAlign, nexora: &TokenStream) -> TokenStream {
    match align {
        VerticalAlign::Top => quote!(#nexora::desktop::TableCellVerticalAlign::Top),
        VerticalAlign::Center => quote!(#nexora::desktop::TableCellVerticalAlign::Center),
        VerticalAlign::Bottom => quote!(#nexora::desktop::TableCellVerticalAlign::Bottom),
    }
}

impl ColumnSortMode {
    fn span(self) -> proc_macro2::Span {
        match self {
            Self::Default(span) | Self::Ascending(span) | Self::Descending(span) => span,
        }
    }

    fn attribute_name(self) -> &'static str {
        match self {
            Self::Default(_) => "sortable",
            Self::Ascending(_) => "ascending",
            Self::Descending(_) => "descending",
        }
    }
}
