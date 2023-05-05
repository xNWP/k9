#![feature(proc_macro_diagnostic)]

use proc_macro::{TokenStream, Diagnostic};
use quote::quote;
use quote::spanned::Spanned;
use syn::parse::Parse;
use syn::{braced, Ident, LitStr};
use syn::{parse_macro_input, Token};

#[proc_macro]
pub fn console_command(tokens: TokenStream) -> TokenStream {
    console_command_base(tokens, false)
}

#[proc_macro]
pub fn console_command_internal(tokens: TokenStream) -> TokenStream {
    console_command_base(tokens, true)
}

fn console_command_base(tokens: TokenStream, internal: bool) -> TokenStream {
    let pi = parse_macro_input!(tokens as ConsoleCommandParseInfo);

    let mut match_str = "".to_owned();
    let mut args_str = "".to_owned();
    let mut param_names = "".to_owned();

    let crate_name = if internal { "crate" } else { "k9" };

    for f in pi.fields {
        if f.optional{
            match_str += format!(
                r#"
                let {0}: {3} = if let Some(x) = args.get("{0}") {{
                    match x {{
                        {1},
                        _ => return Err("'{0}' was not a valid {2:?}".to_owned()),
                    }}
                }} else {{
                    None
                }};
                "#,
                f.name,
                match_callback_arg_value(&f, crate_name),
                f.ty,
                match_callback_arg_type_annotation(&f),
            ).as_str();
        } else {
            match_str += format!(
                r#"
                let {0}: {3} = if let Some(x) = args.get("{0}") {{
                    match x {{
                        {1},
                        _ => return Err("'{0}' was not a valid {2:?}".to_owned()),
                    }}
                }} else {{
                    return Err("missing variable '{0}'".to_owned());
                }};
                "#,
                f.name,
                match_callback_arg_value(&f, crate_name),
                f.ty,
                match_callback_arg_type_annotation(&f),
            )
            .as_str();
        }

        args_str += format!(
            r#"
            args.push({crate_name}::debug_ui::console::CallbackArgumentDefinition {{
                name: "{0}".to_owned(),
                cba_type: {1},
                optional: {2},
            }});
            "#,
            f.name,
            match_callback_arg_type_core(&f, crate_name),
            f.optional,
        )
        .as_str();

        param_names += format!("{}, ", f.name).as_str();
    }

    if !param_names.is_empty() {
        param_names = param_names[0..param_names.len() - 2].to_owned();
    }
    
    let inner_cb = pi.callback;
    let inner_cb = quote! { #inner_cb }.to_string();
    let output_str = format!(
        r#"
        {{
            let cb = move |
            ccf: {crate_name}::debug_ui::console::ConsoleCommandInterface,
            args: std::collections::BTreeMap<String, {crate_name}::debug_ui::console::CallbackArgumentValue>
            | {{
                {match_str}
                let mut inner_cb = {0};
                return inner_cb(ccf, {param_names});
            }};
            
            let mut args = Vec::new();
            {args_str}
            {crate_name}::debug_ui::ConsoleCommand::new(cb, args, "{1}".to_owned())
        }}
        "#,
        inner_cb,
        pi.description,
    );
    //println!("{output_str}");
    output_str.parse().unwrap()
}

fn match_callback_arg_type_core(field: &ParameterParseInfo, crate_name: &str) -> String {
    let core = format!("{crate_name}::debug_ui::console::CallbackArgumentType::");
    match &field.ty {
        &ParameterType::F32 => core + "Float32",
        &ParameterType::F64 => core + "Float64",
        &ParameterType::I32 => core + "Int32",
        &ParameterType::I64 => core + "Int64",
        &ParameterType::String => core + "String",
        &ParameterType::Bool => core + "Bool",
        &ParameterType::Flag => core + "Flag",
    }
}

fn match_callback_arg_type_annotation(field: &ParameterParseInfo) -> String {
    let core = match field.ty {
        ParameterType::Bool => "bool",
        ParameterType::F32 => "f32",
        ParameterType::F64 => "f64",
        ParameterType::Flag => "bool",
        ParameterType::I32 => "i32",
        ParameterType::I64 => "i64",
        ParameterType::String => "String",
    };

    if field.optional {
        format!("Option<{core}>")
    } else {
        core.to_owned()
    }
}

fn match_callback_arg_value(field: &ParameterParseInfo, crate_name: &str) -> String {
    let core = format!("{crate_name}::debug_ui::console::CallbackArgumentValue::");
    let value = if field.optional {
        "Some(*x)"
    } else {
        "*x"
    };

    match field.ty {
        ParameterType::F32 => core + format!("Float32(x) => {value}").as_str(),
        ParameterType::F64 => core + format!("Float64(x) => {value}").as_str(),
        ParameterType::I32 => core + format!("Int32(x) => {value}").as_str(),
        ParameterType::I64 => core + format!("Int64(x) => {value}").as_str(),
        ParameterType::String => core + format!("String(x) => {value}").as_str(),
        ParameterType::Bool => core + format!("Bool(x) => {value}").as_str(),
        ParameterType::Flag => core + format!("Flag(x) => {value}").as_str(),
    }
}

struct ConsoleCommandParseInfo {
    description: String,
    fields: Vec<ParameterParseInfo>,
    callback: syn::Expr,
}

impl Parse for ConsoleCommandParseInfo {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let description = input.parse::<LitStr>()?.value();

        let _ = input.parse::<Token![,]>()?;

        let fields;
        let _ = braced!(fields in input);
        let fields: Vec<ParameterParseInfo> = fields
            .parse_terminated(ParameterParseInfo::parse, Token![,])
            .unwrap()
            .into_iter()
            .collect();

        let _ = input.parse::<Token![,]>()?;

        let callback = input.parse::<syn::Expr>()?;

        Ok(Self { description, fields, callback })
    }
}

#[derive(Debug)]
struct ParameterParseInfo {
    name: String,
    ty: ParameterType,
    optional: bool,
}
#[derive(Debug)]
enum ParameterType {
    F32,
    F64,
    I32,
    I64,
    String,
    Bool,
    Flag,
}
mod kw {
    use syn::custom_keyword;
    custom_keyword!(f32);
    custom_keyword!(f64);
    custom_keyword!(i32);
    custom_keyword!(i64);
    custom_keyword!(String);
    custom_keyword!(bool);
    custom_keyword!(Flag);
    custom_keyword!(opt);
}
impl Parse for ParameterParseInfo {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let optional = input.parse::<kw::opt>();
        let name = input.parse::<Ident>()?.to_string();
        let colon = input.parse::<Token![:]>()?;
        let ty = if input.parse::<kw::f32>().is_ok() {
            ParameterType::F32
        } else if input.parse::<kw::f64>().is_ok() {
            ParameterType::F64
        } else if input.parse::<kw::i32>().is_ok() {
            ParameterType::I32
        } else if input.parse::<kw::i64>().is_ok() {
            ParameterType::I64
        } else if input.parse::<kw::bool>().is_ok() {
            ParameterType::Bool
        } else if input.parse::<kw::String>().is_ok() {
            ParameterType::String
        } else if input.parse::<kw::Flag>().is_ok() {
            if let Ok(opt) = optional {
                Diagnostic::spanned(opt.span.unwrap(), proc_macro::Level::Warning, "flag marked optional, flags are always considered optional.").emit();
            }

            ParameterType::Flag
        } else {
            let (span, msg) = if let Ok(bad_type) = input.parse::<Ident>() {
                (
                    bad_type.span(),
                    format!("unknown parameter type: {}", bad_type.to_string())
                )
            } else {
                (
                    colon.span,
                    "expected parameter type".to_owned(),
                )
            };
            //diag.emit();
            return Err(syn::Error::new(span, msg));
        };

        let optional = optional.is_ok();
        Ok(Self { name, ty, optional })
    }
}
