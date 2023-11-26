use globset::{Glob, GlobSet, GlobSetBuilder};
use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
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

    pub fn new_with_root(root: &Path) -> Self {
        Site {
            root: root.to_path_buf(),
            page_paths: Vec::new(),
            runtime_factory: RuntimeFactory::new(root),
        }
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

    pub async fn render_to_fs(&self, out: &Path) -> Result<(), anyhow::Error> {
        fs::create_dir_all(out)?;
        for path in self.page_paths.clone().into_iter() {
            let runtime = self.runtime_factory.spawn(&path);
            let mut page = Page::new(runtime, &path);
            page.run().await?;

            let fpath = out
                .join(path.strip_prefix(&self.root)?)
                .with_extension("html");
            let f = fs::File::create(fpath)?;
            let mut w = io::BufWriter::new(f);
            page.render(&mut w)?;
            w.flush()?
        }
        Ok(())
    }
}
