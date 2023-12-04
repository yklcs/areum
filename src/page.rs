use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use deno_core::v8;
use lightningcss::{
    rules::CssRule,
    selector::Component,
    stylesheet::{ParserFlags, ParserOptions, PrinterOptions, StyleSheet},
};
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
            r#"<!DOCTYPE html>{}"#,
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

    pub async fn eval(&mut self) -> Result<(), anyhow::Error> {
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
            scope_styles(arena_dom, arena)?;

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

fn scope_styles(id: ArenaId, arena: &mut Arena) -> Result<(), anyhow::Error> {
    let element = arena[id].clone();
    if element.tag().as_deref() == Some("style") {
        let unique: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();

        if let Some(parent) = element.parent() {
            fn reclass_children(
                arena: &mut Arena,
                children: &Children<ArenaId>,
                unique: String,
            ) -> Result<(), anyhow::Error> {
                match children {
                    Children::Element(child_id) => {
                        if arena[*child_id].vtag() == None {
                            arena[*child_id]
                                .props_mut()
                                .append_string_space_separated("class".into(), unique.clone())?;
                            if let Some(grandchild) = arena[*child_id].clone().children() {
                                reclass_children(arena, grandchild, unique.clone())?;
                            }
                        }
                        Ok(())
                    }
                    Children::Elements(children_ids) => {
                        for children_id in children_ids {
                            reclass_children(arena, children_id, unique.clone())?;
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }

            let parent_cloned = arena[*parent].clone();
            reclass_children(arena, parent_cloned.children().unwrap(), unique.clone())?;
            arena[*parent]
                .props_mut()
                .append_string_space_separated("class".into(), unique.clone())?;
        }

        if let Some(Children::Text(code)) = element.children() {
            let mut stylesheet = StyleSheet::parse(
                code,
                ParserOptions {
                    flags: ParserFlags::NESTING,
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow!(e.to_string()))?;

            for rule in &mut stylesheet.rules.0 {
                match rule {
                    CssRule::Style(style) => {
                        for selector in style.selectors.0.iter_mut() {
                            selector.append(Component::Class(unique.clone().into()))
                        }
                    }
                    _ => {}
                }
            }

            let css = stylesheet.to_css(PrinterOptions {
                minify: true,
                ..Default::default()
            })?;

            arena[id].children = Some(Children::Text(css.code));
        } else {
            return Err(anyhow!("invalid child of <style>"));
        }
    } else if element.children().is_some() {
        fn walk_children(
            arena: &mut Arena,
            children: &Children<ArenaId>,
        ) -> Result<(), anyhow::Error> {
            match children {
                Children::Element(el) => scope_styles(*el, arena),
                Children::Elements(els) => {
                    for el in els {
                        walk_children(arena, el)?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        walk_children(arena, element.children().unwrap())?;
    }

    Ok(())
}
