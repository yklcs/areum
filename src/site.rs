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

use crate::{page::Page, runtime::RuntimeFactory};

pub struct Site {
    root: PathBuf,
    page_paths: Vec<PathBuf>,
    runtime_factory: RuntimeFactory,
}

impl Site {
    pub fn new() -> Self {
        let pwd = env::current_dir().unwrap();
        Site {
            root: pwd.clone(),
            page_paths: Vec::new(),
            runtime_factory: RuntimeFactory::new(&pwd),
        }
    }

    pub fn new_with_root(root: &Path) -> Result<Self, anyhow::Error> {
        let root = fs::canonicalize(root)?;
        Ok(Site {
            runtime_factory: RuntimeFactory::new(&root),
            root: root,
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

    pub async fn render_to_fs(&self, outdir: &Path) -> Result<(), anyhow::Error> {
        fs::create_dir_all(outdir)?;
        for path in self.page_paths.clone().into_iter() {
            let runtime = self.runtime_factory.spawn(&path);
            let mut page = Page::eval(runtime, &Url::from_file_path(&path).unwrap()).await?;
            page.process()?;

            let fname = outdir
                .join(path.strip_prefix(&self.root)?)
                .with_extension("");

            let fpath = if fname.file_name() == Some(OsStr::new("index")) {
                fname.with_extension("html")
            } else {
                fname.join("index.html")
            };

            fs::create_dir_all(fpath.parent().ok_or(anyhow!("no parent path found"))?)?;

            let f = fs::File::create(fpath)?;
            let mut w = io::BufWriter::new(f);
            page.render(&mut w)?;
            w.flush()?
        }
        Ok(())
    }
}
