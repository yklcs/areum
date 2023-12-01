use std::{
    collections::{HashMap, HashSet},
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

use crate::loader::{transpile, Loader};

pub struct Runtime {
    root: PathBuf,
    worker: deno_runtime::worker::MainWorker,
    main_mod: Option<(PathBuf, usize)>,
    mods: HashMap<PathBuf, usize>,
    mods_evaled: HashSet<usize>,
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
        let code = transpile(specifier, code.to_string())?;

        let mod_id = self
            .worker
            .js_runtime
            .load_side_module(specifier, Some(code.into()))
            .await?;

        self.mods.insert(path.to_path_buf(), mod_id);
        Ok(mod_id)
    }

    pub async fn load_main(
        &mut self,
        path: &Path,
        code: impl ToString,
    ) -> Result<usize, anyhow::Error> {
        let specifier = &Url::from_file_path(path).unwrap();
        let code = transpile(specifier, code.to_string())?;

        let mod_id = self
            .worker
            .js_runtime
            .load_main_module(specifier, Some(code.into()))
            .await?;

        self.mods.insert(path.to_path_buf(), mod_id);
        self.main_mod = Some((path.to_path_buf(), mod_id));
        Ok(mod_id)
    }

    pub async fn eval(&mut self) -> Result<(), anyhow::Error> {
        for &mod_id in self.mods.values() {
            if !self.mods_evaled.contains(&mod_id) {
                self.worker.evaluate_module(mod_id).await?;
                self.mods_evaled.insert(mod_id);
            }
        }
        self.worker.run_event_loop(false).await?;
        Ok(())
    }

    pub async fn get_export(
        &mut self,
        key: &str,
    ) -> Result<(v8::Local<v8::Value>, v8::HandleScope), anyhow::Error> {
        let global = self.worker.js_runtime.get_module_namespace(
            self.main_mod
                .clone()
                .ok_or(anyhow!("main module not evaluated"))?
                .1,
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
            main_mod: None,
            mods: HashMap::new(),
            mods_evaled: HashSet::new(),
        }
    }
}
