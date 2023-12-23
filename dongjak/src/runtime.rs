use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::anyhow;
use deno_ast::EmitOptions;
use deno_core::{v8, PollEventLoopOptions};
use deno_graph::ModuleGraph;
use deno_runtime::{
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use serde::{de::DeserializeOwned, Serialize};
use url::Url;

use crate::loader::{transpile, Loader};

pub struct Runtime {
    root: PathBuf,
    worker: deno_runtime::worker::MainWorker,
    main_mod: Option<(Url, usize)>,
    mods: HashMap<Url, usize>,
    graph: ModuleGraph,
    graph_loader: Loader,
    pub functions: HashMap<String, Function>,
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
            functions: HashMap::new(),
        }
    }

    pub fn scope(&mut self) -> v8::HandleScope {
        self.worker.js_runtime.handle_scope()
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

    pub async fn bundle(&mut self, url: &Url) -> Result<String, anyhow::Error> {
        self.graph.roots = vec![url.clone()];
        let bundle = deno_emit::bundle_graph(
            &self.graph,
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
    pub async fn export<T>(
        &mut self,
        module: usize,
        key: &str,
    ) -> Result<v8::Global<T>, anyhow::Error>
    where
        for<'a> v8::Local<'a, T>: TryFrom<v8::Local<'a, v8::Value>, Error = v8::DataError>,
    {
        let global = self.worker.js_runtime.get_module_namespace(module)?;
        let mut scope = self.worker.js_runtime.handle_scope();
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

    pub async fn call<A, R>(&mut self, func: &Function, args: &[A]) -> Result<R, anyhow::Error>
    where
        A: Serialize,
        R: DeserializeOwned,
    {
        let args_v8: Vec<_> = {
            let scope: &mut v8::HandleScope<'_> = &mut self.worker.js_runtime.handle_scope();
            args.into_iter()
                .map(|arg| {
                    let local = serde_v8::to_v8(scope, arg).unwrap();
                    v8::Global::new(scope, local)
                })
                .collect()
        };

        let promise = self.worker.js_runtime.call_with_args(&func.0, &args_v8);
        let result_global = self
            .worker
            .js_runtime
            .with_event_loop_promise(promise, PollEventLoopOptions::default())
            .await?;
        let scope = &mut self.worker.js_runtime.handle_scope();
        let result_local = v8::Local::new(scope, result_global);
        let result: R = serde_v8::from_v8(scope, result_local)?;
        Ok(result)
    }

    pub async fn call_by_name<A, R>(&mut self, func: &str, args: &[A]) -> Result<R, anyhow::Error>
    where
        A: Serialize,
        R: DeserializeOwned,
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
