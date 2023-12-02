use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data")]
pub enum Element {
    #[serde(rename = "html")]
    Html(HtmlElement),
    #[serde(rename = "virtual")]
    Virtual(VirtualElement),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HtmlElement {
    element: String,
    children: Option<Child>,
    props: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VirtualElement {
    vtag: String,
    inner: Box<Element>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
enum Child {
    Null,
    Node(Box<Element>),
    Text(String),
    Array(Vec<Child>),
}

fn render_props(props: &HashMap<String, String>) -> String {
    props
        .iter()
        .map(|(k, v)| format!(" {}={}", k, v))
        .collect::<Vec<_>>()
        .join("")
}

impl Element {
    pub fn reify(&self) -> HtmlElement {
        match self {
            Element::Html(html) => html.to_owned(),
            Element::Virtual(virt) => virt.inner.reify(),
        }
    }

    pub fn render(&self) -> String {
        let html = self.reify();
        let attrs = render_props(&html.props);

        format!(
            "<{0}{2}>{3}{1}</{0}>",
            html.element,
            html.children.as_ref().map_or("".into(), |c| c.render()),
            attrs,
            match self {
                Element::Html(_) => "".into(),
                Element::Virtual(virt) => format!("<!--{}-->", &virt.vtag),
            }
        )
    }
}

impl Child {
    fn render(&self) -> String {
        match self {
            Child::Null => "".to_string(),
            Child::Node(node) => node.render(),
            Child::Text(text) => text.clone(),
            Child::Array(array) => array
                .iter()
                .map(|c| c.render())
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}
