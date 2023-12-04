use std::{
    cell::RefCell,
    io,
    path::{Path, PathBuf},
    rc::Rc,
};

use anyhow::anyhow;
use deno_core::v8;
use lol_html::html_content;

use crate::{
    dom::{self},
    runtime::Runtime,
};

pub struct Page {
    runtime: Runtime,
    path: PathBuf,
    content: Option<String>,
    styles: Option<String>,
    scripts: Option<String>,
}

impl Page {
    pub fn new(runtime: Runtime, path: &Path) -> Self {
        Page {
            runtime,
            path: path.to_path_buf(),
            content: None,
            styles: None,
            scripts: None,
        }
    }

    pub fn render_to_string(&self) -> Result<String, anyhow::Error> {
        let mut output = Vec::new();
        self.render(&mut output)?;
        Ok(String::from_utf8(output)?)
    }

    pub fn render(&self, writer: &mut impl io::Write) -> Result<(), anyhow::Error> {
        let html = format!(
            r#"
<!DOCTYPE html>
<html>
<head></head>
<body>
    {}
</body>
</html>
        "#,
            self.content.clone().ok_or(anyhow!("empty content"))?
        );

        let element_content_handlers = vec![lol_html::element!("head", |el| {
            el.append(
                &format!(
                    "<style>{}</style>",
                    self.styles.clone().unwrap_or("".to_string())
                ),
                html_content::ContentType::Html,
            );
            Ok(())
        })];
        let output = lol_html::rewrite_str(
            &html,
            lol_html::RewriteStrSettings {
                element_content_handlers,
                ..Default::default()
            },
        )?;

        writer.write_all(output.as_bytes())?;
        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), anyhow::Error> {
        let code = std::fs::read_to_string(&self.path)?;

        self.runtime
            .load_side(
                &self.runtime.root().join("/areum/jsx-runtime"),
                include_str!("ts/jsx-runtime.js"),
            )
            .await?;
        self.runtime
            .load_side(
                &self.runtime.root().join("__areum.js"),
                include_str!("ts/areum.js"),
            )
            .await?;
        self.runtime.eval().await?;

        let main = self.runtime.load_main(&self.path, code).await?;
        self.runtime.eval().await?;

        self.content = {
            let (default, mut scope) = self.runtime.export(main, "default").await?;
            let func = v8::Local::<v8::Function>::try_from(default)?;
            let res = func.call(&mut scope, default, &[]).unwrap();
            let dom = serde_v8::from_v8::<dom::BoxedElement>(&mut scope, res)?;
            let html = dom.to_string();
            Some(html)
        };

        self.styles = {
            let (styles, mut scope) = self.runtime.export(main, "styles").await?;
            if styles.is_null_or_undefined() {
                None
            } else {
                styles
                    .to_string(&mut scope)
                    .map(|x| x.to_rust_string_lossy(&mut scope))
            }
        };

        self.scripts = {
            let (scripts, mut scope) = self.runtime.export(main, "scripts").await?;
            if scripts.is_null_or_undefined() {
                None
            } else {
                scripts
                    .to_string(&mut scope)
                    .map(|x| x.to_rust_string_lossy(&mut scope))
            }
        };

        Ok(())
    }
}
