//! `CrudQuery` 派生宏实现。

use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Error, Expr, ExprArray, Fields, GenericArgument, Ident, LitBool,
    LitInt, LitStr, PathArguments, Result, Token, Type, meta::ParseNestedMeta,
    spanned::Spanned as _,
};

use crate::{contracts_path, parse_string, reject_generics, set_once};

const DEFAULT_PAGE_SIZE: u32 = 25;
const DEFAULT_MIN_PAGE_SIZE: u32 = 15;
const DEFAULT_MAX_PAGE_SIZE: u32 = 100;
const DEFAULT_PAGE_SIZE_OPTIONS: &[u32] = &[15, 25, 50, 100];

#[derive(Default)]
struct PageSizeArguments {
    default: Option<(u32, proc_macro2::Span)>,
    min: Option<(u32, proc_macro2::Span)>,
    max: Option<(u32, proc_macro2::Span)>,
    options: Option<(Vec<u32>, proc_macro2::Span)>,
}

struct PageSizeConfig {
    default: u32,
    min: u32,
    max: u32,
    options: Vec<u32>,
}

#[derive(Default)]
struct FilterArguments {
    label: Option<LitStr>,
    description: Option<LitStr>,
    placeholder: Option<LitStr>,
    control: Option<LitStr>,
    presentation: Option<LitStr>,
    trigger: Option<LitStr>,
    required: Option<bool>,
    required_message: Option<LitStr>,
    pattern: Option<LitStr>,
    pattern_message: Option<LitStr>,
    parse_error: Option<LitStr>,
    width: Option<LitInt>,
}

struct FilterField {
    ident: Ident,
    inferred_control: String,
    arguments: FilterArguments,
}

struct ParsedCrudQuery {
    pagination: Ident,
    sort: Option<(Ident, proc_macro2::TokenStream)>,
    filters: Vec<FilterField>,
    page_size: PageSizeConfig,
}

pub(crate) fn expand_crud_query(input: DeriveInput) -> Result<proc_macro2::TokenStream> {
    reject_generics(&input, "CrudQuery")?;
    let parsed = parse_crud_query(&input)?;
    let contracts = contracts_path();
    let ident = &input.ident;
    let pagination = &parsed.pagination;
    let filter_metadata = parsed
        .filters
        .iter()
        .map(|field| expand_filter_metadata(field, &contracts))
        .collect::<Result<Vec<_>>>()?;
    let filter_value_arms = parsed.filters.iter().map(|field| {
        let ident = &field.ident;
        let name = field.ident.to_string();
        quote!(#name => #contracts::__private::serde_json::to_value(&self.#ident).ok())
    });
    let set_filter_arms = parsed.filters.iter().map(|field| {
        let ident = &field.ident;
        let name = field.ident.to_string();
        quote! {
            #name => {
                self.#ident = #contracts::crud_query::decode_filter_value(#name, value)?;
                ::core::result::Result::Ok(())
            }
        }
    });
    let PageSizeConfig {
        default,
        min,
        max,
        options,
    } = parsed.page_size;
    let (sort_type, sort_field, sort_getter, sort_setter) = match parsed.sort {
        Some((sort, ty)) => {
            let name = sort.to_string();
            (
                quote!(#ty),
                quote!(::core::option::Option::Some(#name)),
                quote!(self.#sort.as_ref()),
                quote!(self.#sort = sort;),
            )
        }
        None => (
            quote!(#contracts::crud_query::NoCrudSort),
            quote!(::core::option::Option::None),
            quote!(::core::option::Option::None),
            quote! {
                if sort.is_some() {
                    unreachable!("没有排序字段的 CrudQuery 不能设置排序值");
                }
            },
        ),
    };

    Ok(quote! {
        impl #contracts::crud_query::CrudQuery for #ident {
            type Sort = #sort_type;

            fn pagination(&self) -> &#contracts::pagination::PageQuery {
                &self.#pagination
            }

            fn pagination_mut(&mut self) -> &mut #contracts::pagination::PageQuery {
                &mut self.#pagination
            }

            fn sort(&self) -> ::core::option::Option<&Self::Sort> {
                #sort_getter
            }

            fn set_sort(&mut self, sort: ::core::option::Option<Self::Sort>) {
                #sort_setter
            }

            fn metadata() -> &'static #contracts::crud_query::CrudQueryMetadata {
                static FILTERS: &[#contracts::crud_query::CrudFilterMetadata] = &[
                    #(#filter_metadata),*
                ];
                static METADATA: #contracts::crud_query::CrudQueryMetadata =
                    #contracts::crud_query::CrudQueryMetadata {
                        page_size: #contracts::crud_query::CrudPageSizeMetadata {
                            default: #default,
                            min: #min,
                            max: #max,
                            options: &[#(#options),*],
                        },
                        filters: FILTERS,
                        sort_field: #sort_field,
                    };
                &METADATA
            }

            fn filter_value(
                &self,
                name: &str,
            ) -> ::core::option::Option<#contracts::__private::serde_json::Value> {
                match name {
                    #(#filter_value_arms,)*
                    _ => ::core::option::Option::None,
                }
            }

            fn set_filter_value(
                &mut self,
                name: &str,
                value: #contracts::__private::serde_json::Value,
            ) -> ::core::result::Result<(), ::std::string::String> {
                match name {
                    #(#set_filter_arms,)*
                    _ => ::core::result::Result::Err(
                        ::std::format!("筛选字段 `{name}` 不存在"),
                    ),
                }
            }
        }
    })
}

fn parse_crud_query(input: &DeriveInput) -> Result<ParsedCrudQuery> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            input,
            "CrudQuery 只能派生在具有命名字段的结构体上",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            &data.fields,
            "CrudQuery 只能派生在具有命名字段的结构体上",
        ));
    };
    let page_size = parse_page_size(&input.attrs)?;
    let mut pagination = None;
    let mut sort = None;
    let mut filters = Vec::new();

    for field in &fields.named {
        let Some(ident) = field.ident.clone() else {
            continue;
        };
        let mut is_pagination = false;
        let mut is_sort = false;
        let mut filter = None;
        for attribute in field
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("nexora"))
        {
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("pagination") {
                    if is_pagination {
                        return Err(meta.error("pagination 只能声明一次"));
                    }
                    is_pagination = true;
                    Ok(())
                } else if meta.path.is_ident("sort") {
                    if is_sort {
                        return Err(meta.error("sort 只能声明一次"));
                    }
                    is_sort = true;
                    Ok(())
                } else if meta.path.is_ident("filter") {
                    if filter.is_some() {
                        return Err(meta.error("filter 只能声明一次"));
                    }
                    let mut arguments = FilterArguments::default();
                    meta.parse_nested_meta(|nested| parse_filter_argument(nested, &mut arguments))?;
                    filter = Some(arguments);
                    Ok(())
                } else {
                    Err(meta.error("CrudQuery 字段属性只支持 pagination、sort 或 filter(...)"))
                }
            })?;
        }
        if [is_pagination, is_sort, filter.is_some()]
            .into_iter()
            .filter(|value| *value)
            .count()
            > 1
        {
            return Err(Error::new_spanned(
                field,
                "同一个字段不能同时承担分页、排序和筛选职责",
            ));
        }
        if is_pagination {
            if pagination.replace(ident.clone()).is_some() {
                return Err(Error::new_spanned(
                    field,
                    "CrudQuery 必须且只能声明一个 pagination 字段",
                ));
            }
        }
        if is_sort {
            if sort.is_some() {
                return Err(Error::new_spanned(
                    field,
                    "CrudQuery 最多声明一个 sort 字段",
                ));
            }
            let Some(sort_type) = option_inner(&field.ty) else {
                return Err(Error::new_spanned(
                    &field.ty,
                    "CrudQuery 的 sort 字段必须是 Option<T>",
                ));
            };
            sort = Some((ident.clone(), quote!(#sort_type)));
        }
        if let Some(arguments) = filter {
            if arguments.label.is_none() {
                return Err(Error::new_spanned(
                    field,
                    "CrudQuery filter 必须声明 label = \"...\"",
                ));
            }
            filters.push(FilterField {
                ident,
                inferred_control: infer_control(&field.ty),
                arguments,
            });
        }
    }

    let Some(pagination) = pagination else {
        return Err(Error::new_spanned(
            input,
            "CrudQuery 必须声明一个 #[nexora(pagination)] 字段",
        ));
    };

    Ok(ParsedCrudQuery {
        pagination,
        sort,
        filters,
        page_size,
    })
}

fn parse_page_size(attributes: &[Attribute]) -> Result<PageSizeConfig> {
    let mut arguments = PageSizeArguments::default();
    for attribute in attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("nexora"))
    {
        attribute.parse_nested_meta(|meta| {
            if !meta.path.is_ident("page_size") {
                return Err(meta.error("CrudQuery 类型属性只支持 page_size(...)"));
            }
            meta.parse_nested_meta(|nested| parse_page_size_argument(nested, &mut arguments))
        })?;
    }
    let default = arguments.default.map_or(DEFAULT_PAGE_SIZE, |value| value.0);
    let min = arguments.min.map_or(DEFAULT_MIN_PAGE_SIZE, |value| value.0);
    let max = arguments.max.map_or(DEFAULT_MAX_PAGE_SIZE, |value| value.0);
    let options = arguments.options.as_ref().map_or_else(
        || DEFAULT_PAGE_SIZE_OPTIONS.to_vec(),
        |value| value.0.clone(),
    );
    let span = arguments
        .default
        .map(|value| value.1)
        .or_else(|| arguments.options.as_ref().map(|value| value.1))
        .unwrap_or(proc_macro2::Span::call_site());
    if min == 0 || max == 0 || min > max {
        return Err(Error::new(span, "page_size 必须满足 0 < min <= max"));
    }
    if default < min || default > max {
        return Err(Error::new(
            span,
            "page_size default 必须位于 min 与 max 之间",
        ));
    }
    if options.is_empty() {
        return Err(Error::new(span, "page_size options 不能为空"));
    }
    if !options.contains(&default) {
        return Err(Error::new(span, "page_size default 必须属于 options"));
    }
    if options.iter().any(|value| *value < min || *value > max) {
        return Err(Error::new(
            span,
            "page_size options 必须全部位于 min 与 max 之间",
        ));
    }
    if options.windows(2).any(|values| values[0] >= values[1]) {
        return Err(Error::new(span, "page_size options 必须严格递增且不能重复"));
    }
    Ok(PageSizeConfig {
        default,
        min,
        max,
        options,
    })
}

fn parse_page_size_argument(
    meta: ParseNestedMeta<'_>,
    arguments: &mut PageSizeArguments,
) -> Result<()> {
    if meta.path.is_ident("default") || meta.path.is_ident("min") || meta.path.is_ident("max") {
        let value = meta.value()?.parse::<LitInt>()?;
        let number = value.base10_parse::<u32>()?;
        let target = if meta.path.is_ident("default") {
            &mut arguments.default
        } else if meta.path.is_ident("min") {
            &mut arguments.min
        } else {
            &mut arguments.max
        };
        if target.replace((number, value.span())).is_some() {
            return Err(meta.error("page_size 参数只能声明一次"));
        }
        Ok(())
    } else if meta.path.is_ident("options") {
        let expression = meta.value()?.parse::<Expr>()?;
        let Expr::Array(ExprArray { elems, .. }) = expression else {
            return Err(Error::new_spanned(
                expression,
                "page_size options 必须是整数数组",
            ));
        };
        let mut values = Vec::with_capacity(elems.len());
        for expression in elems {
            let Expr::Lit(literal) = expression else {
                return Err(Error::new_spanned(
                    expression,
                    "page_size options 只能包含整数字面量",
                ));
            };
            let syn::Lit::Int(value) = literal.lit else {
                return Err(Error::new_spanned(
                    literal,
                    "page_size options 只能包含整数字面量",
                ));
            };
            values.push(value.base10_parse::<u32>()?);
        }
        let span = meta.path.span();
        if arguments.options.replace((values, span)).is_some() {
            return Err(meta.error("page_size options 只能声明一次"));
        }
        Ok(())
    } else {
        Err(meta.error("page_size 只支持 default、min、max 和 options"))
    }
}

fn parse_filter_argument(meta: ParseNestedMeta<'_>, arguments: &mut FilterArguments) -> Result<()> {
    if meta.path.is_ident("label") {
        set_once(
            &mut arguments.label,
            parse_string(&meta)?,
            meta.path.span(),
            "label",
        )
    } else if meta.path.is_ident("description") {
        set_once(
            &mut arguments.description,
            parse_string(&meta)?,
            meta.path.span(),
            "description",
        )
    } else if meta.path.is_ident("placeholder") {
        set_once(
            &mut arguments.placeholder,
            parse_string(&meta)?,
            meta.path.span(),
            "placeholder",
        )
    } else if meta.path.is_ident("control") {
        set_once(
            &mut arguments.control,
            parse_string(&meta)?,
            meta.path.span(),
            "control",
        )
    } else if meta.path.is_ident("presentation") {
        set_once(
            &mut arguments.presentation,
            parse_string(&meta)?,
            meta.path.span(),
            "presentation",
        )
    } else if meta.path.is_ident("trigger") {
        set_once(
            &mut arguments.trigger,
            parse_string(&meta)?,
            meta.path.span(),
            "trigger",
        )
    } else if meta.path.is_ident("required") {
        let required = if meta.input.peek(Token![=]) {
            meta.value()?.parse::<LitBool>()?.value()
        } else {
            true
        };
        set_once(
            &mut arguments.required,
            required,
            meta.path.span(),
            "required",
        )
    } else if meta.path.is_ident("required_message") {
        set_once(
            &mut arguments.required_message,
            parse_string(&meta)?,
            meta.path.span(),
            "required_message",
        )
    } else if meta.path.is_ident("pattern") {
        set_once(
            &mut arguments.pattern,
            parse_string(&meta)?,
            meta.path.span(),
            "pattern",
        )
    } else if meta.path.is_ident("pattern_message") {
        set_once(
            &mut arguments.pattern_message,
            parse_string(&meta)?,
            meta.path.span(),
            "pattern_message",
        )
    } else if meta.path.is_ident("parse_error") {
        set_once(
            &mut arguments.parse_error,
            parse_string(&meta)?,
            meta.path.span(),
            "parse_error",
        )
    } else if meta.path.is_ident("width") {
        set_once(
            &mut arguments.width,
            meta.value()?.parse::<LitInt>()?,
            meta.path.span(),
            "width",
        )
    } else {
        Err(meta.error("filter 支持 label、description、placeholder、control、presentation、trigger、required、required_message、pattern、pattern_message、parse_error 和 width"))
    }
}

fn expand_filter_metadata(
    field: &FilterField,
    contracts: &proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let name = field.ident.to_string();
    let arguments = &field.arguments;
    let label = arguments.label.as_ref().expect("已校验 filter label");
    let description = expand_optional_string(arguments.description.as_ref());
    let placeholder = expand_optional_string(arguments.placeholder.as_ref());
    let required_message = expand_optional_string(arguments.required_message.as_ref());
    let pattern = expand_optional_string(arguments.pattern.as_ref());
    let pattern_message = expand_optional_string(arguments.pattern_message.as_ref());
    let parse_error = expand_optional_string(arguments.parse_error.as_ref());
    let width = arguments.width.as_ref().map_or_else(
        || quote!(::core::option::Option::None),
        |width| quote!(::core::option::Option::Some(#width)),
    );
    let control = expand_control(
        arguments.control.as_ref(),
        field.inferred_control.as_str(),
        field.ident.span(),
        contracts,
    )?;
    let presentation = match arguments
        .presentation
        .as_ref()
        .map(LitStr::value)
        .as_deref()
    {
        None | Some("form") => quote!(#contracts::crud_query::CrudFilterPresentation::Form),
        Some("quick") => quote!(#contracts::crud_query::CrudFilterPresentation::Quick),
        Some(_) => {
            return Err(Error::new(
                arguments.presentation.as_ref().unwrap().span(),
                "presentation 必须是 \"form\" 或 \"quick\"",
            ));
        }
    };
    let default_trigger = match control.to_string().as_str() {
        value if value.ends_with("Input") || value.ends_with("NumberInput") => "debounce",
        _ => "immediate",
    };
    let trigger = match arguments
        .trigger
        .as_ref()
        .map(LitStr::value)
        .as_deref()
        .unwrap_or(default_trigger)
    {
        "debounce" => {
            quote!(#contracts::crud_query::CrudFilterTrigger::Debounce { milliseconds: 300 })
        }
        "immediate" => quote!(#contracts::crud_query::CrudFilterTrigger::Immediate),
        "manual" => quote!(#contracts::crud_query::CrudFilterTrigger::Manual),
        _ => {
            return Err(Error::new(
                arguments.trigger.as_ref().unwrap().span(),
                "trigger 必须是 \"debounce\"、\"immediate\" 或 \"manual\"",
            ));
        }
    };
    let required = arguments.required.unwrap_or(false);

    Ok(quote! {
        #contracts::crud_query::CrudFilterMetadata {
            name: #name,
            label: #label,
            description: #description,
            placeholder: #placeholder,
            control: #control,
            presentation: #presentation,
            trigger: #trigger,
            required: #required,
            required_message: #required_message,
            pattern: #pattern,
            pattern_message: #pattern_message,
            parse_error: #parse_error,
            width: #width,
        }
    })
}

fn expand_control(
    explicit: Option<&LitStr>,
    inferred: &str,
    field_span: proc_macro2::Span,
    contracts: &proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let control = explicit
        .map(LitStr::value)
        .unwrap_or_else(|| inferred.to_owned());
    let variant = match control.as_str() {
        "input" => quote!(#contracts::crud_query::CrudFilterControl::Input),
        "number_input" | "number" => quote!(#contracts::crud_query::CrudFilterControl::NumberInput),
        "switch" => quote!(#contracts::crud_query::CrudFilterControl::Switch),
        "select" => quote!(#contracts::crud_query::CrudFilterControl::Select),
        "date_picker" | "date" => quote!(#contracts::crud_query::CrudFilterControl::DatePicker),
        "custom" => quote!(#contracts::crud_query::CrudFilterControl::Custom),
        _ => {
            let span = explicit.map_or(field_span, LitStr::span);
            return Err(Error::new(
                span,
                "control 必须是 input、number_input、switch、select、date_picker 或 custom",
            ));
        }
    };
    Ok(variant)
}

fn infer_control(ty: &Type) -> String {
    let ty = option_inner(ty).unwrap_or(ty);
    let Type::Path(path) = ty else {
        return "custom".to_owned();
    };
    let name = path
        .path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();
    match name.as_str() {
        "String" | "str" => "input",
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" | "f32" | "f64" => "number_input",
        "bool" => "switch",
        "NaiveDate" | "Date" => "date_picker",
        _ => "select",
    }
    .to_owned()
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

fn expand_optional_string(value: Option<&LitStr>) -> proc_macro2::TokenStream {
    value.map_or_else(
        || quote!(::core::option::Option::None),
        |value| quote!(::core::option::Option::Some(#value)),
    )
}
