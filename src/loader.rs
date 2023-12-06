use std::{path::Path, pin::Pin};

use deno_ast::MediaType;
use deno_core::{futures::FutureExt, ModuleType};
use url::Url;

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
    ) -> Result<Url, deno_core::error::AnyError> {
        deno_core::resolve_import(specifier, referrer).map_err(|e| e.into())
    }

    fn load(
        &self,
        specifier: &Url,
        _maybe_referrer: Option<&Url>,
        _is_dyn_import: bool,
    ) -> Pin<Box<deno_core::ModuleSourceFuture>> {
        async fn _load(
            specifier: Url,
            client: reqwest::Client,
        ) -> Result<deno_core::ModuleSource, anyhow::Error> {
            let module_type = module_type(&specifier);
            let code = match specifier.scheme() {
                "file" => {
                    let path = specifier.to_file_path().unwrap();
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
                "https" => client.get(specifier.as_str()).send().await?.text().await?,
                _ => panic!(),
            };

            let code = if module_type == ModuleType::JavaScript {
                transpile(&specifier, code)?
            } else {
                code
            };

            Ok(deno_core::ModuleSource::new(
                module_type,
                code.into(),
                &specifier,
            ))
        }

        _load(specifier.clone(), self.client.clone()).boxed_local()
    }
}

/// Transpiles code if required
pub(crate) fn transpile(specifier: &Url, code: String) -> Result<String, anyhow::Error> {
    let media_type = if MediaType::from_specifier(specifier) == MediaType::Unknown {
        MediaType::TypeScript
    } else {
        MediaType::from_specifier(specifier)
    };

    let should_transpile = match media_type {
        MediaType::JavaScript | MediaType::Cjs | MediaType::Mjs => false,
        _ => true,
    };

    let code = if should_transpile {
        let parsed = deno_ast::parse_module(deno_ast::ParseParams {
            specifier: specifier.to_string(),
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
        transpiled.text
    } else {
        code
    };

    Ok(code)
}

fn module_type(specifier: &Url) -> ModuleType {
    let media_type = MediaType::from_specifier(specifier);
    match media_type {
        MediaType::Json => ModuleType::Json,
        _ => ModuleType::JavaScript,
    }
}
