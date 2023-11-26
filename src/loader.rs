use std::{path::Path, pin::Pin};

use deno_ast::MediaType;
use deno_core::{futures::FutureExt, ModuleType};

pub struct Loader {
    client: reqwest::Client,
}

impl Default for Loader {
    fn default() -> Self {
        Loader {
            client: Default::default(),
        }
    }
}

impl Loader {
    pub fn new() -> Self {
        Default::default()
    }
}

impl deno_core::ModuleLoader for Loader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: deno_core::ResolutionKind,
    ) -> Result<deno_core::ModuleSpecifier, deno_core::error::AnyError> {
        deno_core::resolve_import(specifier, referrer).map_err(|e| e.into())
    }

    fn load(
        &self,
        module_specifier: &deno_core::ModuleSpecifier,
        _maybe_referrer: Option<&deno_core::ModuleSpecifier>,
        _is_dyn_import: bool,
    ) -> Pin<Box<deno_core::ModuleSourceFuture>> {
        async fn _load(
            module_specifier: deno_core::ModuleSpecifier,
            client: reqwest::Client,
        ) -> Result<deno_core::ModuleSource, anyhow::Error> {
            let (media_type, module_type, should_transpile) = determine_type(&module_specifier);

            let code = match module_specifier.scheme() {
                "file" => {
                    let path = module_specifier.to_file_path().unwrap();
                    let ext = match path.extension() {
                        None => {
                            let exts = ["tsx", "ts", "jsx", "js"];
                            exts.into_iter()
                                .filter(|&ext| Path::exists(&path.with_extension(ext)))
                                .last()
                                .unwrap()
                        }
                        Some(ext) => ext.to_str().unwrap(),
                    };
                    let path = path.with_extension(ext);
                    std::fs::read_to_string(path)?
                }
                "https" => {
                    client
                        .get(module_specifier.as_str())
                        .send()
                        .await?
                        .text()
                        .await?
                }
                _ => panic!(),
            };
            let code = if should_transpile {
                transpile(&module_specifier, media_type, code)?
            } else {
                code
            };

            Ok(deno_core::ModuleSource::new(
                module_type,
                code.into(),
                &module_specifier,
            ))
        }

        _load(module_specifier.clone(), self.client.clone()).boxed_local()
    }
}

pub(crate) fn transpile(
    module_specifier: &deno_core::ModuleSpecifier,
    media_type: MediaType,
    code: String,
) -> Result<String, anyhow::Error> {
    let parsed = deno_ast::parse_module(deno_ast::ParseParams {
        specifier: module_specifier.to_string(),
        text_info: deno_ast::SourceTextInfo::from_string(code),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })?;
    let transpiled = parsed.transpile(&deno_ast::EmitOptions {
        jsx_import_source: Some("/areum".to_string()),
        jsx_automatic: true,
        ..Default::default()
    })?;
    Ok(transpiled.text)
}

pub(crate) fn determine_type(url: &deno_core::ModuleSpecifier) -> (MediaType, ModuleType, bool) {
    let media_type = MediaType::from_specifier(url);
    let (module_type, should_transpile) = match media_type {
        MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs => (ModuleType::JavaScript, false),
        MediaType::Jsx => (ModuleType::JavaScript, true),
        MediaType::TypeScript
        | MediaType::Mts
        | MediaType::Cts
        | MediaType::Dts
        | MediaType::Dmts
        | MediaType::Dcts
        | MediaType::Tsx => (ModuleType::JavaScript, true),
        MediaType::Json => (ModuleType::Json, false),
        _ => (ModuleType::JavaScript, true),
    };

    (media_type, module_type, should_transpile)
}
