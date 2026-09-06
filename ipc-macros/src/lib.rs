//! Keep error serialization uniform without mixing IPC concerns into services.
use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, GenericArgument, ItemFn, PathArguments, ReturnType, Type,
};

#[proc_macro_attribute]
pub fn command(args: TokenStream, input: TokenStream) -> TokenStream {
    let function = parse_macro_input!(input as ItemFn);
    let arguments: proc_macro2::TokenStream = args.into();
    expand_command(arguments, function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_command(
    arguments: proc_macro2::TokenStream,
    mut function: ItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    if let ReturnType::Type(_, output) = &function.sig.output {
        if let Type::Path(path) = output.as_ref() {
            if let Some(segment) = path.path.segments.last() {
                if segment.ident != "Result" {
                    if segment.ident.to_string().ends_with("Result") {
                        return Err(syn::Error::new_spanned(
                            output,
                            "IPC command result aliases are not supported; use Result<T, E> explicitly so errors cannot bypass IpcError conversion",
                        ));
                    }
                    return Ok(quote! { #[tauri::command(#arguments)] #function });
                }
                if let PathArguments::AngleBracketed(types) = &segment.arguments {
                    if let Some(GenericArgument::Type(success)) = types.args.first() {
                        let original = output.clone();
                        let body = function.block.clone();
                        let expression = if function.sig.asyncness.is_some() {
                            quote! { (async move #body).await }
                        } else {
                            quote! { (move || #body)() }
                        };
                        function.sig.output =
                            parse_quote! { -> std::result::Result<#success, crate::ipc::IpcError> };
                        function.block = parse_quote! {{
                            let result: #original = #expression;
                            result.map_err(crate::ipc::IntoIpcError::into_ipc_error)
                        }};
                    } else {
                        return Err(syn::Error::new_spanned(
                            output,
                            "IPC command Result must declare a success type",
                        ));
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        output,
                        "IPC command Result must use Result<T, E>",
                    ));
                }
            }
        }
    }
    Ok(quote! { #[tauri::command(#arguments)] #function })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_result_aliases_instead_of_silently_bypassing_conversion() {
        let function: ItemFn = syn::parse_quote! {
            async fn command() -> RemoteFileResult<String> { todo!() }
        };
        let error = expand_command(proc_macro2::TokenStream::new(), function).unwrap_err();
        assert!(error
            .to_string()
            .contains("result aliases are not supported"));
    }

    #[test]
    fn rewrites_explicit_results() {
        let function: ItemFn = syn::parse_quote! {
            async fn command() -> Result<String, DomainError> { todo!() }
        };
        let expanded = expand_command(proc_macro2::TokenStream::new(), function)
            .unwrap()
            .to_string();
        assert!(expanded.contains("crate :: ipc :: IpcError"));
        assert!(expanded.contains("IntoIpcError :: into_ipc_error"));
    }
}
