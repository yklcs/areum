use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Node {
    element: String,
    children: Child,
    props: HashMap<String, serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Child {
    Null,
    Node(Box<Node>),
    Text(String),
    Array(Vec<Child>),
}

impl Node {
    pub fn render(&self) -> String {
        let attrs = self
            .props
            .iter()
            .map(|(k, v)| format!(" {}={}", k, v.to_string()))
            .collect::<Vec<_>>()
            .join("");

        format!(
            "<{0}{2}>{1}</{0}>",
            self.element,
            self.children.render(),
            attrs
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
