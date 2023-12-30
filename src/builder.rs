use anyhow::anyhow;
use deno_core::v8;
use dongjak::runtime::{Runtime, RuntimeOptions};
use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;

use crate::{page::Page, server::root_extension, vfs::VFSys};

pub struct Builder {
    root: PathBuf,
    runtime: Runtime,
    vfs: VFSys,
}

impl Builder {
    pub const LOADER_FN_KEY: &'static str = "loader";

    pub async fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let root = fs::canonicalize(root)?;
        let mut site = Builder {
            runtime: Runtime::new(
                &root,
                RuntimeOptions {
                    jsx_import_source: "/areum".into(),
                    extensions: vec![root_extension::init_ops_and_esm(root.clone())],
                },
            ),
            vfs: VFSys::new(&root)?,
            root,
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

        let areum_mod = self
            .runtime
            .load_from_string(
                &Url::from_file_path(self.runtime.root().join("/areum")).unwrap(),
                include_str!("ts/areum.ts"),
                false,
            )
            .await?;
        self.runtime.eval(areum_mod).await?;

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

    pub async fn build(&mut self, outdir: &Path) -> Result<(), anyhow::Error> {
        self.vfs.scan()?;
        fs::create_dir_all(outdir)?;

        let mut bundle = String::new();
        let bundle_url = Url::from_file_path(self.root.join("__index.ts")).unwrap();

        for src in self.vfs.iter_pages() {
            let url = Url::from_file_path(&src.path).unwrap();
            let mut page = Page::new(&mut self.runtime, &url).await?;

            let fpath = page_dirname(&src.path)?; // /root/dir
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
