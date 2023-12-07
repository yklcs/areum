use std::{
    io,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use deno_ast::EmitOptions;
use deno_core::v8;
use lightningcss::{
    rules::CssRule,
    selector::Component,
    stylesheet::{ParserFlags, ParserOptions, PrinterOptions, StyleSheet},
};
use lol_html::{element, html_content::ContentType, HtmlRewriter};
use rand::{distributions::Alphanumeric, Rng};
use url::Url;

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
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    pub async fn eval(runtime: Runtime, url: &Url) -> Result<Self, anyhow::Error> {
        if url.scheme() != "file" {
            return Err(anyhow!("only file URLs are currently supported for pages"));
        }

        let mut runtime = runtime;

        runtime
            .load_side(
                &Url::from_file_path(runtime.root().join("/areum/jsx-runtime")).unwrap(),
                include_str!("ts/jsx-runtime.ts"),
            )
            .await?;
        runtime
            .load_side(
                &Url::from_file_path(runtime.root().join("__areum.js")).unwrap(),
                include_str!("ts/areum.js"),
            )
            .await?;
        runtime.eval().await?;

        let main = runtime.load_main_from_url(url).await?;
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
            path: url.to_file_path().unwrap(),
            dom,
            arena,
        })
    }

    pub fn render_to_string(&mut self) -> Result<String, anyhow::Error> {
        let mut output = Vec::new();
        self.render(&mut output)?;
        Ok(String::from_utf8(output)?)
    }

    pub fn render(&mut self, writer: &mut impl io::Write) -> Result<(), anyhow::Error> {
        let mut html = self.arena[self.dom].to_string(&self.arena);
        html.insert_str(0, "<!DOCTYPE html>");

        let script = self.inline_bundle()?;
        let mut rewriter = HtmlRewriter::new(
            lol_html::Settings {
                element_content_handlers: vec![element!("head", |el| {
                    let tag = format!("<script>{}</script>", script);
                    el.append(&tag, ContentType::Html);
                    Ok(())
                })],
                ..Default::default()
            },
            |c: &[u8]| {
                writer.write_all(c).unwrap();
            },
        );
        rewriter.write(html.as_bytes())?;
        rewriter.end()?;

        Ok(())
    }

    pub fn process(&mut self) -> Result<(), anyhow::Error> {
        self.scope_styles(self.dom)?;
        Ok(())
    }

    pub fn inline_bundle(&mut self) -> Result<String, anyhow::Error> {
        self.runtime_mut().graph_mut().roots = vec![Url::from_file_path(&self.path).unwrap()];
        let bundle = deno_emit::bundle_graph(
            self.runtime().graph(),
            deno_emit::BundleOptions {
                bundle_type: deno_emit::BundleType::Module,
                emit_options: EmitOptions {
                    inline_source_map: false,
                    ..Default::default()
                },
                emit_ignore_directives: false,
                minify: true,
            },
        )?;
        Ok(bundle.code)
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
