use std::{
    borrow::Borrow,
    fs,
    path::{Path, PathBuf},
};

pub struct Fsys {
    root: PathBuf,
    pub entries: Vec<File>,
}

impl Fsys {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        let mut fs = Fsys {
            root: root.as_ref().to_path_buf(),
            entries: Vec::new(),
        };
        fs.scan()?;
        Ok(fs)
    }

    pub fn scan(&mut self) -> Result<&mut Self, anyhow::Error> {
        let entries = ignore::WalkBuilder::new(&self.root)
            .add_custom_ignore_filename(".areumignore")
            .build()
            .filter(|x| x.clone().unwrap().file_type().unwrap().is_file())
            .map(|dir| Ok(File::from(dir?)))
            .collect::<Result<Vec<_>, anyhow::Error>>()?;

        self.entries = entries;
        Ok(self)
    }
}

#[derive(Debug)]
pub struct File {
    pub path: PathBuf,
    pub kind: FileKind,
    pub underscore: bool,
}

impl From<ignore::DirEntry> for File {
    fn from(dir: ignore::DirEntry) -> Self {
        Self {
            path: dir.path().into(),
            kind: FileKind::from(dir.path()),
            underscore: dir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("_"),
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum FileKind {
    Jsx,
    Mdx,
    Js,
    Css,
    Other,
}

impl<P> From<P> for FileKind
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
