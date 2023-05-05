use proc_macro::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::{braced, Ident};
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
        match_str += format!(
            r#"
            let {0} = if let Some(x) = args.get("{0}") {{
                match x {{
                    {1},
                    _ => return Err("'{0}' was not a valid {2:?}".to_owned()),
                }}
            }} else {{
                return Err("missing variable '{0}'".to_owned());
            }};
        "#,
            f.name,
            match_callback_arg_value(&f.ty, crate_name),
            f.ty
        )
        .as_str();

        args_str += format!(
            r#"
            args.push({crate_name}::debug_ui::CallbackArgumentDefinition {{
                name: "{0}".to_owned(),
                cba_type: {1},
            }});
        "#,
            f.name,
            match_callback_arg_type(&f.ty, crate_name)
        )
        .as_str();

        param_names += format!("{}, ", f.name).as_str();
    }

    param_names = param_names[0..param_names.len() - 2].to_owned();

    let inner_cb = pi.callback;
    let inner_cb = quote! { #inner_cb }.to_string();
    let output_str = format!(
        r#"
            {{
                let cb = move |
                        ccf: {crate_name}::debug_ui::ConsoleCommandInterface,
                        args: std::collections::BTreeMap<String, {crate_name}::debug_ui::CallbackArgumentValue>
                    | {{
                    {match_str}
                    
                    let mut inner_cb = {0};
                    
                    return inner_cb(ccf, {param_names});
                }};
                
                let mut args = Vec::new();
                {args_str}
                {crate_name}::debug_ui::ConsoleCommand::new(cb, args)
            }}
        "#,
        inner_cb,
    );

    output_str.parse().unwrap()
}

fn match_callback_arg_type(field_type: &ParameterType, crate_name: &str) -> String {
    let core = format!("{crate_name}::debug_ui::CallbackArgumentType::");
    match field_type {
        &ParameterType::F32 => core + "Float32",
        &ParameterType::F64 => core + "Float64",
        &ParameterType::I32 => core + "Int32",
        &ParameterType::I64 => core + "Int64",
        &ParameterType::String => core + "String",
        &ParameterType::Bool => core + "Bool",
        &ParameterType::Flag => core + "Flag",
    }
}

fn match_callback_arg_value(field_type: &ParameterType, crate_name: &str) -> String {
    let core = format!("{crate_name}::debug_ui::CallbackArgumentValue::");
    match field_type {
        &ParameterType::F32 => core + "Float32(x) => *x",
        &ParameterType::F64 => core + "Float64(x) => *x",
        &ParameterType::I32 => core + "Int32(x) => *x",
        &ParameterType::I64 => core + "Int64(x) => *x",
        &ParameterType::String => core + "String(x) => *x",
        &ParameterType::Bool => core + "Bool(x) => *x",
        &ParameterType::Flag => core + "Flag(x) => *x",
    }
}

struct ConsoleCommandParseInfo {
    fields: Vec<ParameterParseInfo>,
    callback: syn::Expr,
}

impl Parse for ConsoleCommandParseInfo {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let fields;
        let _ = braced!(fields in input);
        let fields: Vec<ParameterParseInfo> = fields
            .parse_terminated(ParameterParseInfo::parse, Token![,])
            .unwrap()
            .into_iter()
            .collect();

        let _ = input.parse::<Token![,]>()?;

        let callback = input.parse::<syn::Expr>()?;

        Ok(Self { fields, callback })
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
        let optional = input.parse::<kw::opt>().is_ok();
        let name = input.parse::<Ident>()?.to_string();
        let _ = input.parse::<Token![:]>()?;
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
            ParameterType::Flag
        } else {
            panic!(
                "unknown parameter type: {}",
                input
                    .parse::<Ident>()
                    .and_then(|v| Ok(v.to_string()))
                    .unwrap_or_default()
            );
        };

        Ok(Self { name, ty, optional })
    }
}
