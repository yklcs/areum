use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::Context;

#[derive(Clone)]
pub struct SrcFs(Arc<Mutex<SrcFsInner>>);

struct SrcFsInner {
    root: PathBuf,
    entries: Vec<SrcFile>,
}

pub struct SrcFsGuard<'a>(MutexGuard<'a, SrcFsInner>);

impl SrcFsGuard<'_> {
    pub fn iter(&self) -> impl Iterator<Item = &SrcFile> + '_ {
        self.0.entries.iter()
    }

    pub fn iter_pages(&self) -> impl Iterator<Item = &SrcFile> + '_ {
        self.iter().filter(|f| match f.kind {
            SrcKind::Jsx | SrcKind::Mdx if !f.underscore => true,
            _ => false,
        })
    }

    pub fn iter_assets(&self) -> impl Iterator<Item = &SrcFile> + '_ {
        self.iter().filter(|f| match f.kind {
            SrcKind::Jsx | SrcKind::Mdx => false,
            _ => true,
        })
    }
}

impl SrcFs {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let inner = SrcFsInner {
            root: root.as_ref().to_path_buf(),
            entries: Vec::new(),
        };
        let src_fs = SrcFs(Arc::new(Mutex::new(inner)));
        src_fs.scan()?;
        Ok(src_fs)
    }

    pub fn root(&self) -> PathBuf {
        self.0.lock().unwrap().root.clone()
    }

    pub fn scan(&self) -> Result<(), anyhow::Error> {
        let entries = ignore::WalkBuilder::new(&self.0.lock().unwrap().root)
            .add_custom_ignore_filename(".areumignore")
            .build()
            .filter(|x| x.clone().unwrap().file_type().unwrap().is_file())
            .map(|dir| Ok(SrcFile::from(dir?)))
            .collect::<Result<Vec<_>, anyhow::Error>>()?;

        self.0.lock().unwrap().entries = entries;
        Ok(())
    }

    pub fn lock(&self) -> SrcFsGuard<'_> {
        SrcFsGuard(self.0.lock().unwrap())
    }

    pub fn out_file(&self, src: &SrcFile, to: &Path) -> Result<fs::File, anyhow::Error> {
        let out = self.out_fpath(src, to)?;
        fs::create_dir_all(out.parent().unwrap())?;
        Ok(fs::File::create(out)?)
    }

    pub fn copy(&self, src: &SrcFile, to: &Path) -> Result<(), anyhow::Error> {
        let out = self.out_fpath(src, to)?;
        fs::create_dir_all(out.parent().unwrap())?;
        fs::copy(&src.path, out)?;
        Ok(())
    }

    pub fn read(&self, src: &SrcFile) -> Result<Vec<u8>, anyhow::Error> {
        Ok(fs::read(&src.path)?)
    }

    pub fn find(&self, path: impl AsRef<Path>) -> Option<SrcFile> {
        let resolved = self.root().join(path);
        self.lock()
            .iter()
            .find(|&f| f.path == resolved)
            .map(Clone::clone)
    }

    pub fn find_page_src(&self, path: impl AsRef<Path>) -> Option<SrcFile> {
        let resolved = self.root().join(path);
        self.lock()
            .iter_pages()
            .find(
                // looking for /page
                |&f| {
                    f.path.with_extension("") == resolved // /page.jsx
                        || f.path.with_extension("") == resolved.join("index")
                    // /page/index.jsx
                },
            )
            .map(Clone::clone)
    }

    pub fn site_path(&self, src: &SrcFile) -> Result<PathBuf, anyhow::Error> {
        let relative = src.path.strip_prefix(&self.0.lock().unwrap().root)?;
        match src.kind {
            SrcKind::Jsx | SrcKind::Mdx => {
                // /index.tsx -> /
                // /dir/index.tsx -> /dir
                // /dir.tsx -> /dir
                let without_ext = relative.with_extension("");
                let path = if Some(OsStr::new("index")) == without_ext.file_name() {
                    without_ext
                        .parent()
                        .context("unable to get parent")?
                        .to_path_buf()
                } else {
                    without_ext
                };

                Ok(path)
            }
            _ => Ok(relative.to_path_buf()),
        }
    }

    pub fn out_fpath(&self, src: &SrcFile, to: &Path) -> Result<PathBuf, anyhow::Error> {
        let relative = src.path.strip_prefix(&self.0.lock().unwrap().root)?;
        match src.kind {
            SrcKind::Jsx | SrcKind::Mdx => {
                // /index.tsx -> /index.html
                // /dir/index.tsx -> /dir/index.html
                // /dir.tsx -> /dir/index.html
                let site_path = self.site_path(src)?.join("index.html");
                Ok(to.join(site_path))
            }
            _ => Ok(to.join(relative)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrcFile {
    pub path: PathBuf,
    pub kind: SrcKind,
    pub underscore: bool,
}

impl From<ignore::DirEntry> for SrcFile {
    fn from(dir: ignore::DirEntry) -> Self {
        Self {
            path: dir.path().into(),
            kind: SrcKind::from(dir.path()),
            underscore: dir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("_"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SrcKind {
    Jsx,
    Mdx,
    Js,
    Css,
    Other,
}

impl<P> From<P> for SrcKind
where
    P: AsRef<Path>,
{
    fn from(path: P) -> Self {
        let ext = path.as_ref().extension().map(|x| x.to_string_lossy());
        match ext.as_deref() {
            Some("jsx" | "tsx") => Self::Jsx,
            Some("mdx" | "md") => Self::Mdx,
            Some("js" | "ts") => Self::Js,
            Some("css") => Self::Css,
            _ => Self::Other,
        }
    }
}
