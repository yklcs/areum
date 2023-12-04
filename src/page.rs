use std::{
    io,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use deno_core::v8;
use lol_html::html_content;
use rand::{distributions::Alphanumeric, Rng};

use crate::{
    dom::{
        self,
        arena::{Arena, ArenaElement, ArenaId},
        Children, Element,
    },
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
            let boxed_dom = serde_v8::from_v8::<dom::boxed::BoxedElement>(&mut scope, res)?;
            let arena = &mut Arena::new();
            let arena_dom = ArenaElement::from_boxed(arena, &boxed_dom, None);
            scope_styles(arena_dom, arena);
            let html = arena[arena_dom].to_string(arena);

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

fn scope_styles(id: ArenaId, arena: &mut Arena) {
    let element = arena[id].clone();
    if element.tag().as_deref() == Some("style") {
        if let Some(parent) = element.parent() {
            let id: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(8)
                .map(char::from)
                .collect();

            arena[*parent].props_mut().insert("class".to_string(), id);
        }
    } else if element.children().is_some() {
        fn walk_children(arena: &mut Arena, children: &Children<ArenaId>) {
            match children {
                Children::Element(el) => scope_styles(*el, arena),
                Children::Elements(els) => {
                    for el in els {
                        walk_children(arena, el);
                    }
                }
                _ => {}
            }
        }
        walk_children(arena, element.children().unwrap());
    }
}
