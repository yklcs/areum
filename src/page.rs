use std::{convert::Infallible, io};

use anyhow::anyhow;
use deno_ast::EmitOptions;
use lightningcss::{
    selector::{Component, Selector},
    stylesheet::{ParserFlags, ParserOptions, PrinterOptions, StyleSheet},
    visitor::Visit,
};
use lol_html::{element, html_content::ContentType, HtmlRewriter};
use rand::{distributions::Alphanumeric, Rng};
use url::Url;

use crate::{
    dom::{
        arena::{Arena, ArenaId},
        Children, Element,
    },
    runtime::Runtime,
    site::page_dirname,
};

pub struct Page {
    url: Url,
    arena: Arena,
    dom: ArenaId,
    style: String,
    script: String,
}

impl Page {
    pub fn new(url: &Url, arena: Arena, dom: ArenaId) -> Self {
        Self {
            url: url.clone(),
            arena,
            dom,
            style: String::new(),
            script: String::new(),
        }
    }
    pub fn render_to_string(&mut self) -> Result<String, anyhow::Error> {
        let mut output = Vec::new();
        self.render(&mut output)?;
        Ok(String::from_utf8(output)?)
    }

    pub fn render(&mut self, writer: &mut impl io::Write) -> Result<(), anyhow::Error> {
        let mut html = self.arena[self.dom].to_string(&self.arena);
        html.insert_str(0, "<!DOCTYPE html>");

        let mut rewriter = HtmlRewriter::new(
            lol_html::Settings {
                element_content_handlers: vec![
                    element!("body", |el| {
                        let tag = format!("<script>{}</script>", self.script);
                        el.append(&tag, ContentType::Html);
                        Ok(())
                    }),
                    element!("head", |el| {
                        let tag = format!("<style>{}</style>", self.style);
                        el.append(&tag, ContentType::Html);
                        Ok(())
                    }),
                ],

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
        self.apply_styles(self.dom)?;
        Ok(())
    }

    pub fn inline_bundle(&mut self, runtime: &mut Runtime) -> Result<(), anyhow::Error> {
        let script_path = page_dirname(&self.url.to_file_path().unwrap())?.join("__index.js");
        runtime.graph_mut().roots = vec![Url::from_file_path(script_path).unwrap()];
        let bundle = deno_emit::bundle_graph(
            runtime.graph(),
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

        self.script = bundle.code;
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

    pub fn apply_styles(&mut self, id: ArenaId) -> Result<(), anyhow::Error> {
        let element = self.arena[id].clone();

        struct CssVisitor {
            unique: String,
        }
        impl<'i> lightningcss::visitor::Visitor<'i> for CssVisitor {
            type Error = Infallible;

            fn visit_types(&self) -> lightningcss::visitor::VisitTypes {
                lightningcss::visit_types!(SELECTORS)
            }

            fn visit_selector(&mut self, selector: &mut Selector<'i>) -> Result<(), Self::Error> {
                selector.append(Component::Class(self.unique.clone().into()));
                Ok(())
            }
        }

        if let Some(style) = element.style() {
            let mut unique: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(8)
                .map(char::from)
                .collect();
            unique.insert_str(0, "style-");

            let mut stylesheet = StyleSheet::parse(
                &style,
                ParserOptions {
                    flags: ParserFlags::NESTING,
                    ..Default::default()
                },
            )
            .map_err(|e| anyhow!(e.to_string()))?;

            // Rescope stylesheet with unique ID class
            let visitor = &mut CssVisitor {
                unique: unique.clone(),
            };
            stylesheet.visit(visitor)?;

            let css = stylesheet.to_css(PrinterOptions {
                minify: true,
                ..Default::default()
            })?;

            self.style += &css.code;

            // Apply unique class to self
            self.arena[id]
                .props_mut()
                .append_string_space_separated("class".into(), unique.clone())?;

            // Apply unique class to children, except other vtags
            if let Some(children) = element.children() {
                self.walk_children(children, &mut |self_, id| {
                    if self_.arena[id].vtag() == None {
                        let _ = self_.arena[id]
                            .props_mut()
                            .append_string_space_separated("class".into(), unique.clone());
                    }
                    Ok(self_.arena[id].vtag() == None)
                })?;
            }
        }

        if let Some(children) = element.children() {
            self.walk_children(children, &mut |self_, id| {
                self_.apply_styles(id)?;
                Ok(false)
            })?;
        }

        Ok(())
    }
}
