use anyhow::anyhow;
use deno_core::v8;
use dongjak::runtime::Runtime;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;
use walkdir::{DirEntry, WalkDir};

use crate::page::Page;

pub struct Site {
    root: PathBuf,
    page_paths: Vec<PathBuf>,
    runtime: Runtime,
}

impl Site {
    pub const LOADER_FN_KEY: &'static str = "loader";

    pub async fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let root = fs::canonicalize(root)?;
        let mut site = Site {
            runtime: Runtime::new(&root),
            root,
            page_paths: Vec::new(),
        };
        site.bootstrap().await?;
        Ok(site)
    }

    async fn bootstrap(&mut self) -> Result<(), anyhow::Error> {
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

    pub fn read_root(&mut self) -> anyhow::Result<()> {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new(".git/")?);
        builder.add(Glob::new("target/")?);
        let ignore_set = builder.build()?;

        fn is_site(entry: &DirEntry, ignore_set: &GlobSet) -> bool {
            if !ignore_set.matches(entry.path()).is_empty() {
                return false;
            }
            if entry.file_type().is_dir() {
                return true;
            }
            if entry.file_name().to_str().unwrap().starts_with("_") {
                return false;
            }
            if let Some(ext) = entry.path().extension() {
                match ext.to_str().unwrap() {
                    "tsx" => true,
                    "jsx" => true,
                    _ => false,
                }
            } else {
                false
            }
        }

        for entry in WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| is_site(e, &ignore_set))
        {
            let entry = entry?;
            if !entry.file_type().is_dir() {
                self.page_paths.push(entry.into_path());
            }
        }
        Ok(())
    }

    pub async fn render_to_fs(&mut self, outdir: &Path) -> Result<(), anyhow::Error> {
        fs::create_dir_all(outdir)?;

        let mut bundle = String::new();
        let bundle_url = Url::from_file_path(self.root.join("__index.ts")).unwrap();

        for path in self.page_paths.clone().into_iter() {
            let url = Url::from_file_path(&path).unwrap();
            let mut page = Page::new(&mut self.runtime, &url).await?;
            page.process()?;
            // page.inline_bundle(&mut self.runtime)?;

            let fpath = page_dirname(&path)?; // /root/dir
            let fpath = fpath.strip_prefix(&self.root)?; // /dir
            let fpath = outdir.join(fpath).join("index.html"); // /out/dir/index.html

            fs::create_dir_all(fpath.parent().ok_or(anyhow!("no parent path found"))?)?;

            let f = fs::File::create(fpath)?;
            let mut w = io::BufWriter::new(f);
            page.render(&mut w)?;
            w.flush()?;

            bundle.push_str(&format!(
                r#"export {{ default as page{} }} from "{}"
                "#,
                page.id(),
                url.to_string()
            ))
        }

        bundle.push_str(&format!(
            r#"export {{ runScript }} from "{}""#,
            &Url::from_file_path(self.root.join("/areum/jsx-runtime"))
                .unwrap()
                .to_string()
        ));

        let bundle_mod = self
            .runtime
            .load_from_string(&bundle_url, bundle, true)
            .await?;
        self.runtime.eval(bundle_mod).await?;

        let bundled = self.runtime.bundle(&bundle_url).await?;
        fs::write(outdir.join("index.js"), bundled)?;

        Ok(())
    }
}

/// Get the page's dirname
///
/// /index.html -> /
/// /dir/index.html -> /dir
/// /dir.html -> /dir
pub fn page_dirname(path: &Path) -> Result<PathBuf, anyhow::Error> {
    let fname = path.with_extension("");
    if fname.file_name() == Some(OsStr::new("index")) {
        Ok(fname
            .parent()
            .ok_or(anyhow!("could not find parent"))?
            .to_path_buf())
    } else {
        Ok(fname)
    }
}
