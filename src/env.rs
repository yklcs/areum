use std::path::{Path, PathBuf};

use deno_core::{op2, v8, OpState};
use dongjak::runtime::{Runtime, RuntimeOptions};
use rand::{distributions::Alphanumeric, Rng};
use url::Url;

pub struct Env {
    pub runtime: Runtime,
    pub bundler: Bundler,
}

impl Env {
    pub const LOADER_FN_KEY: &'static str = "loader";

    pub fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let runtime = Runtime::new(
            root,
            RuntimeOptions {
                jsx_import_source: "/areum".into(),
                extensions: vec![],
            },
        );

        Ok(Env {
            runtime,
            bundler: Bundler::new(),
        })
    }

    pub async fn bundle(&mut self) -> Result<String, anyhow::Error> {
        let mut unique: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();
        unique.insert_str(0, "__");
        unique.push_str(".ts");

        let url = Url::from_file_path(self.runtime.root().join(unique)).unwrap();

        self.runtime
            .graph_loader
            .inject(url.clone(), self.bundler.code.clone());
        self.runtime.add_root(&url).await;
        let bundled = self.runtime.bundle(&url).await?;

        Ok(bundled)
    }

    pub async fn bootstrap(&mut self) -> Result<(), anyhow::Error> {
        let jsx_mod = self
            .runtime
            .load_from_string(
                &Url::from_file_path(self.runtime.root().join("/areum/jsx-runtime")).unwrap(),
                include_str!("ts/jsx-runtime.ts"),
                false,
            )
            .await?;
        self.runtime.eval(jsx_mod).await?;

        let loader_mod = self
            .runtime
            .load_from_string(
                &Url::from_file_path(self.runtime.root().join("__loader.ts")).unwrap(),
                include_str!("ts/loader.ts"),
                false,
            )
            .await?;
        self.runtime.eval(loader_mod).await?;

        let loader = self
            .runtime
            .export::<v8::Function>(loader_mod, "default")
            .await?;
        self.runtime
            .functions
            .insert(Self::LOADER_FN_KEY.into(), loader.into());

        Ok(())
    }
}

pub struct Bundler {
    code: String,
}

impl Bundler {
    pub fn new() -> Self {
        Bundler {
            code: String::new(),
        }
    }

    pub fn push(&mut self, code: impl AsRef<str>) {
        self.code.push_str(code.as_ref())
    }

    pub fn clear(&mut self) {
        self.code.clear()
    }
}
