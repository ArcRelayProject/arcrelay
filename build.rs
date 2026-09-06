use heck::ToLowerCamelCase;
use quote::ToTokens;
use std::{
    fs,
    path::{Path, PathBuf},
};
use syn::{FnArg, Item, Pat, ReturnType, Type};

fn main() {
    tauri_build::build();
    println!("cargo:rerun-if-changed=src");
    generate_command_contracts(Path::new("src"));
}

fn generate_command_contracts(directory: &Path) {
    for entry in fs::read_dir(directory).expect("source directory") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            generate_command_contracts(&path);
            continue;
        }
        if path.extension().and_then(|p| p.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let parsed =
            syn::parse_file(&source).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let mut generated=String::from("#[allow(unused_mut)]\npub(crate) fn register_ipc_contract(registry: &mut crate::ipc_contract::Registry) {\n");
        let mut count = 0;
        for item in parsed.items {
            let Item::Fn(function) = item else { continue };
            if !function.attrs.iter().any(|attr| {
                attr.path()
                    .segments
                    .last()
                    .is_some_and(|p| p.ident == "command")
            }) {
                continue;
            }
            count += 1;
            let cfg = function
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("cfg"))
                .map(|a| a.to_token_stream().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            generated.push_str(&format!(
                "{cfg}\n{{\nlet mut args = std::collections::BTreeMap::new();\n"
            ));
            for arg in &function.sig.inputs {
                let FnArg::Typed(arg) = arg else { continue };
                let Pat::Ident(name) = arg.pat.as_ref() else {
                    panic!("IPC argument must be named")
                };
                if let Type::Path(ty) = arg.ty.as_ref() {
                    if ty.path.segments.last().is_some_and(|segment| {
                        matches!(
                            segment.ident.to_string().as_str(),
                            "State" | "AppHandle" | "WebviewWindow" | "Window"
                        )
                    }) {
                        continue;
                    }
                }
                generated.push_str(&format!(
                    "args.insert({:?}.to_string(), registry.ty::<{}>());\n",
                    name.ident.to_string().to_lower_camel_case(),
                    arg.ty.to_token_stream()
                ));
            }
            let output = match &function.sig.output {
                ReturnType::Default => "()".into(),
                ReturnType::Type(_, ty) => {
                    if let Type::Path(path) = ty.as_ref() {
                        let last = path.path.segments.last().unwrap();
                        if last.ident == "Result" {
                            let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
                                panic!("Result needs arguments")
                            };
                            args.args.first().unwrap().to_token_stream().to_string()
                        } else {
                            ty.to_token_stream().to_string()
                        }
                    } else {
                        ty.to_token_stream().to_string()
                    }
                }
            };
            generated.push_str(&format!("let result=registry.ty::<{output}>();\nregistry.command({:?}, args, result);\n}}\n",function.sig.ident.to_string()));
        }
        if count == 0 {
            continue;
        }
        generated.push_str("}\n");
        let name = path
            .to_string_lossy()
            .replace(['/', '\\'], "_")
            .replace(".rs", "");
        let destination =
            PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join(format!("{name}_ipc.rs"));
        fs::write(destination, generated).unwrap();
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
