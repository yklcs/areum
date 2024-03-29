use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;

use crate::{env::Env, src_fs::SrcFs};

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
            src_fs: SrcFs::new(&root),
            root,
        })
    }

    pub async fn build(&mut self, outdir: &Path) -> Result<(), anyhow::Error> {
        self.src_fs.scan().await?;
        fs::create_dir_all(outdir)?;

        let mut pages = Vec::new();

        for src in self.src_fs.lock().await.iter_pages() {
            let url = Url::from_file_path(&src.path).unwrap();
            let path = self.src_fs.site_path(src).await?;
            let page = self.env.new_page(&url, &path).await?;
            pages.push(page);
        }

        for src in self.src_fs.lock().await.iter_generators() {
            let url = Url::from_file_path(&src.path).unwrap();
            let mut pages_ = self.env.new_pages(&url).await?;
            pages.append(&mut pages_);
        }

        for mut page in pages {
            let out = outdir.join(&page.path).join("index.html");
            fs::create_dir_all(out.parent().unwrap())?;
            let f = fs::File::create(out)?;

            let mut w = io::BufWriter::new(f);
            page.render(&mut w)?;
            w.flush()?;

            self.env.bundler.push(format!(
                r#"export {{ default as page{} }} from "{}"
                "#,
                page.id(),
                page.url.to_string()
            ));
        }

        for asset in self.src_fs.lock().await.iter_assets() {
            self.src_fs.copy(asset, outdir).await?;
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
