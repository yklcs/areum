use std::{collections::HashMap, pin::Pin};

use anyhow::anyhow;
use deno_ast::MediaType;
use deno_core::{futures::FutureExt, ModuleType};
use url::Url;

#[derive(Clone, Default)]
pub struct Loader {
    client: reqwest::Client,
    pub(crate) injected: HashMap<Url, String>,
}

impl Loader {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn inject(&mut self, url: Url, code: String) {
        self.injected.insert(url, code);
    }

    async fn load_to_string(&self, specifier: &Url) -> Result<String, anyhow::Error> {
        if let Some(code) = self.injected.get(specifier) {
            return Ok(code.clone());
        }

        let module_type = module_type(&specifier);
        let code = match specifier.scheme() {
            "file" => {
                let path = specifier.to_file_path().unwrap();
                std::fs::read_to_string(path)?
            }
            "https" => {
                self.client
                    .get(specifier.as_str())
                    .send()
                    .await?
                    .text()
                    .await?
            }
            _ => return Err(anyhow!("invalid scheme in url {}", specifier.to_string())),
        };

        let code = if module_type == ModuleType::JavaScript {
            transpile(&specifier, code)?
        } else {
            code
        };

        Ok(code)
    }
}

impl deno_graph::source::Loader for Loader {
    fn load(
        &mut self,
        specifier: &Url,
        _is_dynamic: bool,
        _cache_setting: deno_graph::source::CacheSetting,
    ) -> deno_graph::source::LoadFuture {
        let specifier = specifier.clone();
        let loader = self.clone();
        async move {
            let code = loader.load_to_string(&specifier).await?;
            Ok(Some(deno_graph::source::LoadResponse::Module {
                content: code.into(),
                specifier: specifier,
                maybe_headers: None,
            }))
        }
        .boxed_local()
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
        let specifier = specifier.clone();
        let module_type = module_type(&specifier);
        let loader = self.clone();
        async move {
            let code = loader.load_to_string(&specifier).await?;
            Ok(deno_core::ModuleSource::new(
                module_type,
                code.into(),
                &specifier,
            ))
        }
        .boxed_local()
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
