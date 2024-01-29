use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use blake2::{digest::consts, Blake2b, Digest};
use deno_core::{op2, v8};
use dongjak::runtime::{Runtime, RuntimeOptions};
use rand::{distributions::Alphanumeric, Rng};
// use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    dom::{
        arena::{Arena, ArenaElement},
        boxed::BoxedElement,
    },
    page::{Page, PageProps},
};

pub struct Env {
    pub runtime: Runtime,
    pub bundler: Bundler,
}

impl Env {
    pub const LOADER_FN_KEY: &'static str = "load";
    pub const GENERATOR_LOADER_FN_KEY: &'static str = "loadGenerator";

    pub fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let runtime = Runtime::new(
            root,
            RuntimeOptions {
                jsx_import_source: "/areum".into(),
                extensions: vec![
                    rand_extension::init_ops_and_esm(),
                    print_extension::init_ops_and_esm(),
                ],
            },
        );

        Ok(Env {
            runtime,
            bundler: Bundler::new(),
        })
    }

    pub async fn new_page(&mut self, url: &Url, path: &Path) -> Result<Page, anyhow::Error> {
        self.runtime.add_root(url).await;

        let props = PageProps {
            path: path.to_string_lossy().into(),
            generator: format!("Areum {}", env!("CARGO_PKG_VERSION")),
        };

        let mut arena = Arena::new();
        let boxed: BoxedElement = self
            .runtime
            .call_by_name(Env::LOADER_FN_KEY, &[&url.to_string(), &props])
            .await?;

        let dom = ArenaElement::from_boxed(&mut arena, &boxed, None);

        let hash = Blake2b::<consts::U6>::digest(url.to_string());
        let id = bs58::encode(hash).into_string();

        let script = format!(
            r#"
        import {{ page{} as Page, run }} from "/index.js"
        run(Page, {{}})
        "#,
            id
        );

        let page = Page {
            path: path.to_path_buf(),
            url: url.clone(),
            arena,
            dom,
            style: String::new(),
            scopes: HashSet::new(),
            script,
            id,
            props,
        };

        Ok(page)
    }

    pub async fn new_pages(&mut self, url: &Url) -> Result<Vec<Page>, anyhow::Error> {
        self.runtime.add_root(url).await;

        let path = url
            .to_file_path()
            .unwrap()
            .strip_prefix(self.runtime.root())?
            .parent()
            .unwrap()
            .to_path_buf();

        let props_temp = PageProps {
            path: path.to_string_lossy().into(),
            generator: format!("Areum {}", env!("CARGO_PKG_VERSION")),
        };

        let boxeds: HashMap<String, BoxedElement> = self
            .runtime
            .call_by_name(
                Env::GENERATOR_LOADER_FN_KEY,
                &[&url.to_string(), &props_temp],
            )
            .await?;

        boxeds
            .into_iter()
            .map(|(path, boxed)| {
                let mut arena = Arena::new();
                let dom = ArenaElement::from_boxed(&mut arena, &boxed, None);

                let hash = Blake2b::<consts::U6>::digest(url.to_string());
                let id = bs58::encode(hash).into_string();

                let props = PageProps {
                    path: path.clone(),
                    generator: format!("Areum {}", env!("CARGO_PKG_VERSION")),
                };

                let script = format!(
                    r#"
            import {{ page{} as Page, runScript }} from "/index.js"
            if (!("Deno" in window)) {{
                if (Page.script) {{
                    Page.script()
                }}
                runScript(Page())
            }}
            "#,
                    id
                );

                Ok(Page {
                    path: PathBuf::from_str(&path)?,
                    url: url.clone(),
                    arena,
                    dom,
                    style: String::new(),
                    scopes: HashSet::new(),
                    script,
                    id,
                    props,
                })
            })
            .collect()
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
            .export::<v8::Function>(loader_mod, Self::LOADER_FN_KEY)
            .await?;
        self.runtime
            .functions
            .insert(Self::LOADER_FN_KEY.into(), loader.into());

        let generator_loader = self
            .runtime
            .export::<v8::Function>(loader_mod, Self::GENERATOR_LOADER_FN_KEY)
            .await?;
        self.runtime.functions.insert(
            Self::GENERATOR_LOADER_FN_KEY.into(),
            generator_loader.into(),
        );

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

#[op2]
#[string]
fn randString(n: u32) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(n as usize)
        .map(char::from)
        .collect()
}

deno_core::extension!(
    rand_extension,
    ops = [randString, hashString],
    docs = "Extension providing operations for randomness",
);

#[op2]
#[string]
fn hashString(#[string] str: String) -> String {
    let hash = Blake2b::<consts::U6>::digest(str);
    bs58::encode(hash).into_string()
}

deno_core::extension!(
    print_extension,
    ops = [print, join_path],
    docs = "Extension providing printing",
);

#[op2(fast)]
pub fn print(#[string] msg: &str, is_err: bool) -> Result<(), anyhow::Error> {
    if is_err {
        std::io::stderr().write_all(msg.as_bytes())?;
        std::io::stderr().flush().unwrap();
    } else {
        std::io::stdout().write_all(msg.as_bytes())?;
        std::io::stdout().flush().unwrap();
    }
    Ok(())
}

#[op2]
#[string]
pub fn join_path(#[string] root: &str, #[string] to_join: &str) -> String {
    Path::new(root).join(to_join).to_string_lossy().to_string()
}
