use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use deno_ast::EmitOptions;
use deno_core::{v8, Extension, JsRuntime, PollEventLoopOptions};
use deno_graph::ModuleGraph;
use serde::de::DeserializeOwned;
use url::Url;

use crate::loader::{transpile, Loader, LoaderOptions};

pub struct RuntimeOptions {
    pub jsx_import_source: String,
    pub extensions: Vec<Extension>,
}

pub struct Runtime {
    root: PathBuf,
    js_runtime: JsRuntime,
    main_mod: Option<(Url, usize)>,
    mods: HashMap<Url, usize>,
    graph: Arc<Mutex<ModuleGraph>>,
    pub graph_loader: Loader,
    pub functions: HashMap<String, Function>,
    jsx_import_source: String,
}

impl Runtime {
    pub async fn add_root(&mut self, root: &Url) {
        self.graph
            .lock()
            .unwrap()
            .build(
                vec![root.clone()],
                &mut self.graph_loader,
                Default::default(),
            )
            .await;
    }

    pub fn new(root: &Path, options: RuntimeOptions) -> Self {
        let loader = Loader::new(LoaderOptions {
            jsx_import_source: options.jsx_import_source.clone(),
        });

        let js_runtime = JsRuntime::new(deno_core::RuntimeOptions {
            module_loader: Some(Rc::new(loader.clone())),
            extensions: options.extensions,
            ..Default::default()
        });

        Runtime {
            root: root.to_path_buf(),
            js_runtime,
            main_mod: None,
            mods: HashMap::new(),
            graph: Arc::new(Mutex::new(ModuleGraph::new(deno_graph::GraphKind::All))),
            graph_loader: loader,
            functions: HashMap::new(),
            jsx_import_source: options.jsx_import_source,
        }
    }

    pub fn scope(&mut self) -> v8::HandleScope {
        self.js_runtime.handle_scope()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn bundle(&mut self, url: &Url) -> Result<String, anyhow::Error> {
        let mut graph = self.graph.lock().unwrap().clone();
        graph.roots = vec![url.clone()];
        let bundle = deno_emit::bundle_graph(
            &graph,
            deno_emit::BundleOptions {
                bundle_type: deno_emit::BundleType::Module,
                emit_options: EmitOptions {
                    inline_source_map: false,
                    ..Default::default()
                },
                emit_ignore_directives: false,
                minify: true,
            },
        )?;

        Ok(bundle.code)
    }

    pub async fn load_from_string(
        &mut self,
        url: &Url,
        code: impl ToString,
        main: bool,
    ) -> Result<usize, anyhow::Error> {
        let code = transpile(url, &code.to_string(), &self.jsx_import_source)?;

        let module = if main {
            self.js_runtime
                .load_main_module(url, Some(code.clone().into()))
                .await?
        } else {
            self.js_runtime
                .load_side_module(url, Some(code.clone().into()))
                .await?
        };

        self.mods.insert(url.clone(), module);
        if main {
            self.main_mod = Some((url.clone(), module));
        }

        self.graph_loader.inject(url.clone(), code);
        self.graph
            .lock()
            .unwrap()
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
            self.js_runtime.load_main_module(url, None).await?
        } else {
            self.js_runtime.load_side_module(url, None).await?
        };

        self.mods.insert(url.clone(), module);
        if main {
            self.main_mod = Some((url.clone(), module));
        }

        self.graph
            .lock()
            .unwrap()
            .build(
                self.mods.iter().map(|(k, _)| k.clone()).collect(),
                &mut self.graph_loader,
                Default::default(),
            )
            .await;

        Ok(module)
    }

    pub async fn eval(&mut self, module: usize) -> Result<(), anyhow::Error> {
        self.js_runtime.mod_evaluate(module).await?;
        self.js_runtime.run_event_loop(Default::default()).await?;
        Ok(())
    }

    pub fn module_from_url(&self, url: &Url) -> Option<usize> {
        self.mods.get(url).map(|x| *x)
    }

    /// Gets an export from the runtime by module ID.
    ///
    /// Comparable to doing `import { key } from module`.
    pub async fn export<T>(
        &mut self,
        module: usize,
        key: &str,
    ) -> Result<v8::Global<T>, anyhow::Error>
    where
        for<'a> v8::Local<'a, T>: TryFrom<v8::Local<'a, v8::Value>, Error = v8::DataError>,
    {
        let global = self.js_runtime.get_module_namespace(module)?;
        let mut scope = self.js_runtime.handle_scope();
        let local = v8::Local::new(&mut scope, global);

        let key_v8 = v8::String::new(&mut scope, key)
            .ok_or(anyhow!("could not convert key into v8 value"))?;

        let got: v8::Local<T> = local
            .get(&mut scope, key_v8.into())
            .ok_or(anyhow!("could not find {}", key))?
            .try_into()?;

        let global = v8::Global::new(&mut scope, got);
        Ok(global)
    }

    pub async fn call<T>(
        &mut self,
        func: &Function,
        args: &[&dyn erased_serde::Serialize],
    ) -> Result<T, anyhow::Error>
    where
        T: DeserializeOwned,
    {
        let args_v8: Vec<_> = {
            let scope: &mut v8::HandleScope<'_> = &mut self.js_runtime.handle_scope();
            args.into_iter()
                .map(|arg| {
                    let local = serde_v8::to_v8(scope, arg).unwrap();
                    v8::Global::new(scope, local)
                })
                .collect()
        };

        let promise = self.js_runtime.call_with_args(&func.0, &args_v8);
        let result_global = self
            .js_runtime
            .with_event_loop_promise(promise, PollEventLoopOptions::default())
            .await?;
        let scope = &mut self.js_runtime.handle_scope();
        let result_local = v8::Local::new(scope, result_global);
        let result: T = serde_v8::from_v8(scope, result_local)?;
        Ok(result)
    }

    pub async fn call_by_name<T>(
        &mut self,
        func: &str,
        args: &[&dyn erased_serde::Serialize],
    ) -> Result<T, anyhow::Error>
    where
        T: DeserializeOwned,
    {
        let func = self
            .functions
            .get(func)
            .ok_or(anyhow!("could not find function {}", func))?
            .clone();
        self.call(&func, args).await
    }
}

#[derive(Clone)]
pub struct Function(pub v8::Global<v8::Function>);

impl From<v8::Global<v8::Function>> for Function {
    fn from(value: v8::Global<v8::Function>) -> Self {
        Self(value)
    }
}
