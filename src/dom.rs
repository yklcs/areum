use std::collections::HashMap;

use serde::{Deserialize, Serialize};

trait Element: ToString {
    fn tag(&self) -> Option<String>;

    fn vtag(&self) -> Option<String>;

    fn children(&self) -> Option<&Children<Self>>
    where
        Self: Sized;

    fn parent(&self) -> Option<&Self>
    where
        Self: Sized;

    fn props(&self) -> HashMap<String, String>;
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Children<T> {
    Element(T),
    Elements(Vec<Self>),
    Text(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BoxedElement {
    tag: Option<String>,
    vtag: Option<String>,
    children: Option<Box<Children<Self>>>,
    props: HashMap<String, String>,
}

impl Element for BoxedElement {
    fn tag(&self) -> Option<String> {
        self.tag.clone()
    }

    fn vtag(&self) -> Option<String> {
        self.vtag.clone()
    }

    fn children(&self) -> Option<&Children<Self>>
    where
        Self: Sized,
    {
        self.children.as_deref()
    }

    fn parent(&self) -> Option<&Self>
    where
        Self: Sized,
    {
        None
    }

    fn props(&self) -> HashMap<String, String> {
        self.props.clone()
    }
}

impl ToString for BoxedElement {
    fn to_string(&self) -> String {
        let attrs = self
            .props()
            .iter()
            .map(|(k, v)| format!(" {}={}", k, v))
            .collect::<Vec<_>>()
            .join("");

        if self.vtag().as_deref() == Some("Fragment") {
            self.children.as_ref().map_or("".into(), |c| c.to_string())
        } else {
            format!(
                "<{0}{2}{3}>{1}</{0}>",
                self.tag.clone().unwrap_or("".to_string()),
                self.children.as_ref().map_or("".into(), |c| c.to_string()),
                attrs,
                self.vtag()
                    .map(|s| format!(r#" component="{}""#, s))
                    .unwrap_or("".into())
            )
        }
    }
}

impl<T> ToString for Children<T>
where
    T: ToString,
{
    fn to_string(&self) -> String {
        match self {
            Children::Element(el) => el.to_string(),
            Children::Text(text) => text.clone(),
            Children::Elements(els) => els
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(""),
        }
    }
}
