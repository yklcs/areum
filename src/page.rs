use std::{
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
    dom: ArenaId,
    arena: Arena,
}

impl Page {
    pub async fn eval(runtime: Runtime, path: &Path) -> Result<Self, anyhow::Error> {
        let code = std::fs::read_to_string(path)?;
        let mut runtime = runtime;

        runtime
            .load_side(
                &runtime.root().join("/areum/jsx-runtime"),
                include_str!("ts/jsx-runtime.js"),
            )
            .await?;
        runtime
            .load_side(
                &runtime.root().join("__areum.js"),
                include_str!("ts/areum.js"),
            )
            .await?;
        runtime.eval().await?;

        let main = runtime.load_main(&path, code).await?;
        runtime.eval().await?;

        let mut arena = Arena::new();
        let dom = {
            let (default, mut scope) = runtime.export(main, "default").await?;
            let func = v8::Local::<v8::Function>::try_from(default)?;
            let res = func.call(&mut scope, default, &[]).unwrap();
            let boxed_dom = serde_v8::from_v8::<dom::boxed::BoxedElement>(&mut scope, res)?;
            ArenaElement::from_boxed(&mut arena, &boxed_dom, None)
        };

        Ok(Page {
            runtime,
            path: path.to_path_buf(),
            dom,
            arena,
        })
    }

    pub fn render_to_string(&self) -> Result<String, anyhow::Error> {
        let mut output = Vec::new();
        self.render(&mut output)?;
        Ok(String::from_utf8(output)?)
    }

    pub fn render(&self, writer: &mut impl io::Write) -> Result<(), anyhow::Error> {
        let mut html = self.arena[self.dom].to_string(&self.arena);
        html.insert_str(0, "<!DOCTYPE html>");
        writer.write_all(html.as_bytes())?;
        Ok(())
    }

    pub fn process(&mut self) -> Result<(), anyhow::Error> {
        self.scope_styles(self.dom)?;
        Ok(())
    }

    pub fn walk_children(
        &mut self,
        children: &Children<ArenaId>,
        f: &mut impl FnMut(&mut Self, ArenaId) -> Result<bool, anyhow::Error>,
    ) -> Result<(), anyhow::Error> {
        match children {
            Children::Element(child) => {
                let propagate = f(self, *child)?;
                if propagate {
                    if let Some(grandchild) = self.arena[*child].clone().children() {
                        self.walk_children(grandchild, f)?;
                    }
                }
            }
            Children::Elements(children) => {
                for child in children {
                    self.walk_children(child, f)?;
                }
            }
            _ => {}
        };

        Ok(())
    }

    pub fn scope_styles(&mut self, id: ArenaId) -> Result<(), anyhow::Error> {
        let element = self.arena[id].clone();

        if element.tag().as_deref() == Some("style") {
            let unique: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(8)
                .map(char::from)
                .collect();

            if let Some(parent) = element.parent() {
                let parent_cloned = self.arena[*parent].clone();
                self.walk_children(parent_cloned.children().unwrap(), &mut |self_, id| {
                    if self_.arena[id].vtag() == None {
                        let _ = self_.arena[id]
                            .props_mut()
                            .append_string_space_separated("class".into(), unique.clone());
                    }
                    Ok(self_.arena[id].vtag() == None)
                })?;

                self.arena[*parent]
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

                self.arena[id].children = Some(Children::Text(css.code));
            } else {
                return Err(anyhow!("invalid child of <style>"));
            }
        } else if let Some(children) = element.children() {
            self.walk_children(children, &mut |self_, id| {
                self_.scope_styles(id)?;
                Ok(false)
            })?;
        }

        Ok(())
    }
}
