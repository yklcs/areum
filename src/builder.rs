use anyhow::anyhow;
use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;

use crate::{env::Env, page::Page, vfs::VFSys};

pub struct Builder {
    root: PathBuf,
    env: Env,
    vfs: VFSys,
}

impl Builder {
    pub async fn new(root: &Path) -> Result<Self, anyhow::Error> {
        let root = fs::canonicalize(root)?;
        let mut env = Env::new(&root)?;
        env.bootstrap().await?;

        Ok(Builder {
            env,
            vfs: VFSys::new(&root)?,
            root,
        })
    }

    pub async fn build(&mut self, outdir: &Path) -> Result<(), anyhow::Error> {
        self.vfs.scan()?;
        fs::create_dir_all(outdir)?;

        for src in self.vfs.iter_pages() {
            let url = Url::from_file_path(&src.path).unwrap();
            let mut page = Page::new(&mut self.env, &url).await?;

            let fpath = page_dirname(&src.path)?; // /root/dir
            let fpath = fpath.strip_prefix(&self.root)?; // /dir
            let fpath = outdir.join(fpath).join("index.html"); // /out/dir/index.html

            fs::create_dir_all(fpath.parent().ok_or(anyhow!("no parent path found"))?)?;

            let f = fs::File::create(fpath)?;
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
