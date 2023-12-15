use anyhow::anyhow;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use url::Url;
use walkdir::{DirEntry, WalkDir};

use crate::runtime::Runtime;

pub struct Site {
    root: PathBuf,
    page_paths: Vec<PathBuf>,
    runtime: Runtime,
}

impl Site {
    pub fn new() -> Self {
        let pwd = env::current_dir().unwrap();
        Site {
            root: pwd.clone(),
            page_paths: Vec::new(),
            runtime: Runtime::new(&pwd),
        }
    }

    pub fn new_with_root(root: &Path) -> Result<Self, anyhow::Error> {
        let root = fs::canonicalize(root)?;
        Ok(Site {
            runtime: Runtime::new(&root),
            root,
            page_paths: Vec::new(),
        })
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
        for path in self.page_paths.clone().into_iter() {
            let url = Url::from_file_path(&path).unwrap();
            let mut page = self.runtime.eval_page(&url).await?;
            page.process()?;
            page.inline_bundle(&mut self.runtime)?;

            let fpath = page_dirname(&path)?; // /root/dir
            let fpath = fpath.strip_prefix(&self.root)?; // /dir
            let fpath = outdir.join(fpath).join("index.html"); // /out/dir/index.html

            fs::create_dir_all(fpath.parent().ok_or(anyhow!("no parent path found"))?)?;

            let f = fs::File::create(fpath)?;
            let mut w = io::BufWriter::new(f);
            page.render(&mut w)?;
            w.flush()?
        }
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
