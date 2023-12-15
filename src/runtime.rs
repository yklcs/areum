use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::anyhow;
use deno_core::v8;
use deno_graph::ModuleGraph;
use deno_runtime::{
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use url::Url;

use crate::{
    dom::{
        arena::{Arena, ArenaElement},
        boxed::BoxedElement,
    },
    loader::{transpile, Loader},
    page::Page,
    site::page_dirname,
};

pub struct Runtime {
    root: PathBuf,
    worker: deno_runtime::worker::MainWorker,
    main_mod: Option<(Url, usize)>,
    mods: HashMap<Url, usize>,
    graph: ModuleGraph,
    graph_loader: Loader,
}

impl Runtime {
    pub fn new(root: &Path) -> Self {
        let loader = Loader::new();

        let worker = MainWorker::bootstrap_from_options(
            Url::from_file_path(root.join("__index.ts")).unwrap(),
            PermissionsContainer::allow_all(),
            WorkerOptions {
                module_loader: Rc::new(loader),
                ..Default::default()
            },
        );

        Runtime {
            root: root.to_path_buf(),
            worker,
            main_mod: None,
            mods: HashMap::new(),
            graph: ModuleGraph::new(deno_graph::GraphKind::All),
            graph_loader: Loader::new(),
        }
    }

    pub fn graph(&self) -> &ModuleGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut ModuleGraph {
        &mut self.graph
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn eval_page(&mut self, url: &Url) -> Result<Page, anyhow::Error> {
        let jsx = self
            .load_from_string(
                &Url::from_file_path(self.root().join("/areum/jsx-runtime")).unwrap(),
                include_str!("ts/jsx-runtime.ts"),
                false,
            )
            .await?;
        self.eval(jsx).await?;

        // Load and eval page
        let module = self.load_from_url(url, false).await?;
        self.eval(module).await?;

        let script_path = page_dirname(&url.to_file_path().unwrap())?.join("__index.js");
        let script = self
            .load_from_string(
                &Url::from_file_path(script_path).unwrap(),
                format!(
                    r#"
        import Page from "{}"
        import {{ runScript }} from "{}"
        if (!("Deno" in window)) {{
            if (Page.script) {{
                Page.script()
            }}
            runScript(Page())
        }}
        "#,
                    url.to_string(),
                    &Url::from_file_path(self.root().join("/areum/jsx-runtime"))
                        .unwrap()
                        .to_string()
                ),
                false,
            )
            .await?;
        self.eval(script).await?;

        let mut arena = Arena::new();
        let dom = {
            let (default, mut scope) = self.export(module, "default").await?;
            let func = v8::Local::<v8::Function>::try_from(default)?;
            let obj = func
                .call(&mut scope, default, &[])
                .unwrap()
                .to_object(&mut scope)
                .unwrap();

            let style_key = v8::String::new(&mut scope, "style").unwrap();
            let style = func
                .to_object(&mut scope)
                .unwrap()
                .get(&mut scope, style_key.into());

            if let Some(style) = style {
                let style = if style.is_function() {
                    let style_func = v8::Local::<v8::Function>::try_from(style)?;
                    style_func.call(&mut scope, style, &[]).unwrap()
                } else {
                    style
                };
                obj.set(&mut scope, style_key.into(), style);
            }

            let boxed_dom = serde_v8::from_v8::<BoxedElement>(&mut scope, obj.into())?;
            ArenaElement::from_boxed(&mut arena, &boxed_dom, None)
        };

        let page = Page::new(url, arena, dom);
        Ok(page)
    }

    pub async fn load_from_string(
        &mut self,
        url: &Url,
        code: impl ToString,
        main: bool,
    ) -> Result<usize, anyhow::Error> {
        let code = transpile(url, code.to_string())?;

        let module = if main {
            self.worker
                .js_runtime
                .load_main_module(url, Some(code.clone().into()))
                .await?
        } else {
            self.worker
                .js_runtime
                .load_side_module(url, Some(code.clone().into()))
                .await?
        };

        self.mods.insert(url.clone(), module);
        if main {
            self.main_mod = Some((url.clone(), module));
        }

        self.graph_loader.inject(url.clone(), code);
        self.graph
            .build(
                self.mods.iter().map(|(k, _)| k.clone()).collect(),
                &mut self.graph_loader,
                Default::default(),
            )
            .await;

        Ok(module)
    }

    pub async fn load_from_url(&mut self, url: &Url, main: bool) -> Result<usize, anyhow::Error> {
        let module = if main {
            self.worker.js_runtime.load_main_module(url, None).await?
        } else {
            self.worker.js_runtime.load_side_module(url, None).await?
        };

        self.mods.insert(url.clone(), module);
        if main {
            self.main_mod = Some((url.clone(), module));
        }

        self.graph
            .build(
                self.mods.iter().map(|(k, _)| k.clone()).collect(),
                &mut self.graph_loader,
                Default::default(),
            )
            .await;

        Ok(module)
    }

    pub async fn eval(&mut self, module: usize) -> Result<(), anyhow::Error> {
        self.worker.evaluate_module(module).await?;
        self.worker.run_event_loop(false).await?;
        Ok(())
    }

    pub fn module_from_url(&self, url: &Url) -> Option<usize> {
        self.mods.get(url).map(|x| *x)
    }

    /// Gets an export from the runtime by module ID.
    ///
    /// Comparable to doing `import { key } from module`.
    pub async fn export(
        &mut self,
        module: usize,
        key: &str,
    ) -> Result<(v8::Local<v8::Value>, v8::HandleScope), anyhow::Error> {
        let global = self.worker.js_runtime.get_module_namespace(module)?;
        let mut scope = self.worker.js_runtime.handle_scope();
        let local = v8::Local::new(&mut scope, global);

        let key_v8 = v8::String::new(&mut scope, key)
            .ok_or(anyhow!("could not convert key into v8 value"))?;

        let got = local
            .get(&mut scope, key_v8.into())
            .ok_or(anyhow!("could not find {}", key))?;

        Ok((got, scope))
    }

    /// Gets an export from the runtime by module url.
    ///
    /// Comparable to doing `import { key } from module`.
    pub async fn export_by_url(
        &mut self,
        url: &Url,
        key: &str,
    ) -> Result<(v8::Local<v8::Value>, v8::HandleScope), anyhow::Error> {
        let module = self
            .module_from_url(url)
            .ok_or(anyhow!("could not find module {}", url.to_string()))?;
        self.export(module, key).await
    }
}
