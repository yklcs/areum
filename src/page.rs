use std::{
    convert::Infallible,
    io,
    path::{Path, PathBuf},
};

use anyhow::anyhow;

use lightningcss::{
    css_modules,
    selector::{Component, PseudoClass, Selector},
    stylesheet::{ParserFlags, ParserOptions, PrinterOptions, StyleSheet},
    visitor::Visit,
};
use lol_html::{element, html_content::ContentType, HtmlRewriter};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    dom::{
        arena::{Arena, ArenaElement, ArenaId},
        boxed::BoxedElement,
        Children,
    },
    env::Env,
};

pub struct Page {
    url: Url,
    arena: Arena,
    dom: ArenaId,
    style: String,
    pub(crate) script: String,
    id: String,
}

#[derive(Serialize)]
struct PageProps {
    path: String,
    generator: String,
}

impl Page {
    pub async fn new(env: &mut Env, url: &Url, path: &Path) -> Result<Self, anyhow::Error> {
        env.runtime.add_root(url).await;

        let props = PageProps {
            path: path.to_string_lossy().into(),
            generator: format!("Areum {}", env!("CARGO_PKG_VERSION")),
        };

        let mut arena = Arena::new();
        let boxed: BoxedElement = env
            .runtime
            .call_by_name(Env::LOADER_FN_KEY, &[&url.to_string(), &props])
            .await?;
        let dom = ArenaElement::from_boxed(&mut arena, &boxed, None);

        let id = format!("{:x}", Sha256::digest(url.to_string()));

        let script = format!(
            r#"
        import {{ page{} as Page, runScript }} from "/index.js"
        if (!("Deno" in window)) {{
            if (Page.script) {{
                Page.script()
            }}
            runScript(Page())
        }}
        "#,
            id
        );

        let page = Self {
            url: url.clone(),
            arena,
            dom,
            style: String::new(),
            script,
            id,
        };

        Ok(page)
    }

    pub fn id(&self) -> String {
        self.id.clone()
    }

    pub fn render_to_string(&mut self) -> Result<String, anyhow::Error> {
        let mut output = Vec::new();
        self.render(&mut output)?;
        Ok(String::from_utf8(output)?)
    }

    pub fn render(&mut self, writer: &mut impl io::Write) -> Result<(), anyhow::Error> {
        self.process()?;

        let mut html = self.arena[self.dom].to_string(&self.arena);

        html.insert_str(0, "<!DOCTYPE html>");

        let mut rewriter = HtmlRewriter::new(
            lol_html::Settings {
                element_content_handlers: vec![
                    element!("body", |el| {
                        let tag = format!(r#"<script type="module">{}</script>"#, self.script);
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

    fn process(&mut self) -> Result<(), anyhow::Error> {
        self.process_scopes(self.dom)?;
        self.process_styles(self.dom)?;
        Ok(())
    }

    fn walk_children(
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

    fn process_scopes(&mut self, id: ArenaId) -> Result<(), anyhow::Error> {
        let element = self.arena[id].clone();

        if let ArenaElement::Intrinsic { ref scope, .. } = element {
            let unique = format!("s{scope}");
            self.arena[id]
                .props_mut()
                .append_string_space_separated("class".into(), unique.clone())?;
        }

        if let Some(children) = element.children() {
            self.walk_children(children, &mut |self_, id| {
                self_.process_scopes(id)?;
                Ok(false)
            })?;
        }

        Ok(())
    }

    fn process_styles(&mut self, id: ArenaId) -> Result<(), anyhow::Error> {
        let element = self.arena[id].clone();

        if let ArenaElement::Virtual {
            style: Some(ref style),
            ref scope,
            ..
        } = element
        {
            let unique = format!("s{scope}");
            self.style += &process_css(&style, &unique)?;
        }

        if let Some(children) = element.children() {
            self.walk_children(children, &mut |self_, id| {
                self_.process_styles(id)?;
                Ok(false)
            })?;
        }

        Ok(())
    }
}

struct CssVisitor {
    unique: String,
}

impl<'i> lightningcss::visitor::Visitor<'i> for CssVisitor {
    type Error = Infallible;

    fn visit_types(&self) -> lightningcss::visitor::VisitTypes {
        lightningcss::visit_types!(SELECTORS)
    }

    fn visit_selector(&mut self, selector: &mut Selector<'i>) -> Result<(), Self::Error> {
        let v: Vec<_> = selector
            .iter_raw_parse_order_from(0)
            .map(|x| x.clone())
            .flat_map(|mut c| match c {
                Component::ID(_) | Component::Class(_) | Component::LocalName(_) => {
                    vec![c, Component::Class(self.unique.clone().into())]
                }
                Component::Is(ref mut inner)
                | Component::Has(ref mut inner)
                | Component::Negation(ref mut inner)
                | Component::Where(ref mut inner) => {
                    for i in inner.iter_mut() {
                        self.visit_selector(i);
                    }
                    vec![c]
                }
                Component::NonTSPseudoClass(pseudo) => match pseudo {
                    PseudoClass::Global { selector } => {
                        let inner = *selector;
                        inner
                            .iter_raw_parse_order_from(0)
                            .map(|x| x.clone())
                            .collect()
                    }
                    _ => {
                        vec![Component::NonTSPseudoClass(pseudo)]
                    }
                },
                _ => {
                    vec![c]
                }
            })
            .collect();

        *selector = v.try_into()?;

        Ok(())
    }
}

fn process_css(style: &str, unique: &str) -> Result<String, anyhow::Error> {
    let mut stylesheet = StyleSheet::parse(
        &style,
        ParserOptions {
            flags: ParserFlags::NESTING,
            css_modules: Some(css_modules::Config {
                pattern: css_modules::Pattern {
                    segments: vec![css_modules::Segment::Local].into(),
                },
                dashed_idents: false,
            }),
            ..Default::default()
        },
    )
    .map_err(|e| anyhow!(e.to_string()))?;

    // Rescope stylesheet with unique ID class
    let visitor = &mut CssVisitor {
        unique: unique.to_string(),
    };
    stylesheet.visit(visitor)?;

    let css = stylesheet.to_css(PrinterOptions {
        minify: true,
        ..Default::default()
    })?;

    Ok(css.code)
}
