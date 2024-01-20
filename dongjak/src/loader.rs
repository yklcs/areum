use std::{
    collections::HashMap,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use deno_ast::MediaType;
use deno_core::{futures::FutureExt, ModuleSourceCode, ModuleType, RequestedModuleType};
use mdxjs::{MdxConstructs, MdxParseOptions};
use url::Url;

#[derive(Clone)]
pub struct LoaderOptions {
    pub jsx_import_source: String,
}

#[derive(Clone)]
pub struct Loader {
    client: reqwest::Client,
    pub(crate) injected: Arc<Mutex<HashMap<Url, String>>>,
    options: LoaderOptions,
}

impl Loader {
    pub fn new(options: LoaderOptions) -> Self {
        Self {
            client: reqwest::Client::new(),
            injected: Arc::new(Mutex::new(HashMap::new())),
            options,
        }
    }

    pub fn inject(&self, url: Url, code: String) {
        self.injected.lock().unwrap().insert(url, code);
    }

    pub fn get_injected(&self, url: &Url) -> Option<String> {
        self.injected.lock().unwrap().get(url).map(|s| s.clone())
    }

    async fn load_to_string(&self, specifier: &Url) -> Result<String, anyhow::Error> {
        if let Some(code) = self.get_injected(specifier) {
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
            transpile(&specifier, &code, &self.options.jsx_import_source)?
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
            loader.inject(specifier.clone(), code.clone());
            Ok(Some(deno_graph::source::LoadResponse::Module {
                content: code.into(),
                specifier,
                maybe_headers: Some(HashMap::from([("content-type".into(), "text/tsx".into())])),
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
        _requested_module_type: RequestedModuleType,
    ) -> Pin<Box<deno_core::ModuleSourceFuture>> {
        let specifier = specifier.clone();
        let module_type = module_type(&specifier);
        let loader = self.clone();
        async move {
            let code = loader.load_to_string(&specifier).await?;
            loader.inject(specifier.clone(), code.clone());
            Ok(deno_core::ModuleSource::new(
                module_type,
                ModuleSourceCode::String(code.into()),
                &specifier,
            ))
        }
        .boxed_local()
    }
}

/// Transpiles code if required
pub(crate) fn transpile(
    specifier: &Url,
    code: &str,
    jsx_import_source: &str,
) -> Result<String, anyhow::Error> {
    let code = match Path::new(specifier.path())
        .extension()
        .map(|ext| ext.to_str().unwrap())
    {
        Some("mdx" | "md") => {
            let code = mdxjs::compile(
                &code,
                &mdxjs::Options {
                    parse: MdxParseOptions {
                        constructs: MdxConstructs {
                            attention: true,
                            block_quote: true,
                            character_escape: true,
                            character_reference: true,
                            code_fenced: true,
                            code_text: true,
                            definition: true,
                            frontmatter: true,
                            gfm_autolink_literal: false,
                            gfm_label_start_footnote: false,
                            gfm_footnote_definition: false,
                            gfm_strikethrough: false,
                            gfm_table: false,
                            gfm_task_list_item: false,
                            hard_break_escape: true,
                            hard_break_trailing: true,
                            heading_atx: true,
                            heading_setext: true,
                            label_start_image: true,
                            label_start_link: true,
                            label_end: true,
                            list_item: true,
                            math_flow: true,
                            math_text: true,
                            thematic_break: true,
                        },
                        gfm_strikethrough_single_tilde: false,
                        math_text_single_dollar: true,
                    },
                    jsx_import_source: Some(jsx_import_source.into()),
                    ..Default::default()
                },
            )
            .map_err(|err| anyhow!(err))?;
            code.into()
        }
        _ => code.into(),
    };

    let media_type = if MediaType::from_specifier(specifier) == MediaType::Unknown {
        MediaType::Tsx
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
            jsx_import_source: Some(jsx_import_source.into()),
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
