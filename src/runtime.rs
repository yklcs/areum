use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::anyhow;
use deno_core::v8;
use deno_runtime::{
    permissions::PermissionsContainer,
    worker::{MainWorker, WorkerOptions},
};
use url::Url;

use crate::loader::{determine_type, transpile, Loader};

pub struct Runtime {
    root: PathBuf,
    worker: deno_runtime::worker::MainWorker,
    main_mod_id: Option<usize>,
    mod_ids: Vec<usize>,
}

impl Runtime {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn load_side(
        &mut self,
        path: &Path,
        code: impl ToString,
    ) -> Result<usize, anyhow::Error> {
        let specifier = &Url::from_file_path(path).unwrap();
        let (media_type, _, should_transpile) = determine_type(specifier);
        let code = if should_transpile {
            transpile(specifier, media_type, code.to_string())?
        } else {
            code.to_string()
        };

        let mod_id = self
            .worker
            .js_runtime
            .load_side_module(specifier, Some(code.into()))
            .await?;
        self.mod_ids.push(mod_id);
        Ok(mod_id)
    }

    pub async fn load_main(
        &mut self,
        path: &Path,
        code: impl ToString,
    ) -> Result<usize, anyhow::Error> {
        let specifier = &Url::from_file_path(path).unwrap();
        let (media_type, _, should_transpile) = determine_type(specifier);
        let code = if should_transpile {
            transpile(specifier, media_type, code.to_string())?
        } else {
            code.to_string()
        };

        let mod_id = self
            .worker
            .js_runtime
            .load_main_module(&Url::from_file_path(path).unwrap(), Some(code.into()))
            .await?;
        self.mod_ids.push(mod_id);
        self.main_mod_id = Some(mod_id);
        Ok(mod_id)
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        for &mod_id in self.mod_ids.iter() {
            self.worker.evaluate_module(mod_id).await?;
        }
        self.worker.run_event_loop(false).await?;
        Ok(())
    }

    pub async fn get_export(
        &mut self,
        key: &str,
    ) -> Result<(v8::Local<v8::Value>, v8::HandleScope), anyhow::Error> {
        let global = self.worker.js_runtime.get_module_namespace(
            self.main_mod_id
                .ok_or(anyhow!("main module not evaluated"))?,
        )?;
        let mut scope = self.worker.js_runtime.handle_scope();
        let local = v8::Local::new(&mut scope, global);

        let key_v8 = v8::String::new(&mut scope, key)
            .ok_or(anyhow!("could not convert key into v8 value"))?;

        let got = local
            .get(&mut scope, key_v8.into())
            .ok_or(anyhow!("could not find {}", key))?;

        Ok((got, scope))
    }
}

pub struct RuntimeFactory {
    root: PathBuf,
    loader: Rc<Loader>,
}

impl RuntimeFactory {
    pub fn new(root: &Path) -> Self {
        RuntimeFactory {
            root: root.to_path_buf(),
            loader: Default::default(),
        }
    }

    pub fn spawn(&self, main_module: &Path) -> Runtime {
        let worker = MainWorker::bootstrap_from_options(
            Url::from_file_path(main_module).unwrap(),
            PermissionsContainer::allow_all(),
            WorkerOptions {
                module_loader: Rc::<Loader>::clone(&self.loader),
                ..Default::default()
            },
        );
        Runtime {
            root: self.root.clone(),
            worker,
            main_mod_id: None,
            mod_ids: Vec::new(),
        }
    }
}
