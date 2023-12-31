use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;

use crate::{env::Env, page::Page, src_fs::SrcFs};

pub struct Builder {
    root: PathBuf,
    env: Env,
    src_fs: SrcFs,
}

impl Builder {
    pub async fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let root = fs::canonicalize(root)?;
        let mut env = Env::new(&root)?;
        env.bootstrap().await?;

        Ok(Builder {
            env,
            src_fs: SrcFs::new(&root)?,
            root,
        })
    }

    pub async fn build(&mut self, outdir: &Path) -> Result<(), anyhow::Error> {
        self.src_fs.scan()?;
        fs::create_dir_all(outdir)?;

        for src in self.src_fs.lock().iter_pages() {
            let url = Url::from_file_path(&src.path).unwrap();
            let mut page = Page::new(&mut self.env, &url).await?;

            let f = self.src_fs.out_file(&src, outdir)?;
            let mut w = io::BufWriter::new(f);
            page.render(&mut w)?;
            w.flush()?;

            self.env.bundler.push(format!(
                r#"export {{ default as page{} }} from "{}"
                "#,
                page.id(),
                url.to_string()
            ));
        }

        for asset in self.src_fs.lock().iter_assets() {
            self.src_fs.copy(asset, outdir)?;
        }

        self.env.bundler.push(format!(
            r#"export {{ runScript }} from "{}""#,
            &Url::from_file_path(self.root.join("/areum/jsx-runtime"))
                .unwrap()
                .to_string()
        ));

        let bundled = self.env.bundle().await?;
        fs::write(outdir.join("index.js"), bundled)?;

        Ok(())
    }
}
