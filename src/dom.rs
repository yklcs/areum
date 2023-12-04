use std::collections::HashMap;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

pub trait Element<R> {
    fn tag(&self) -> Option<String>;

    fn vtag(&self) -> Option<String>;

    fn children(&self) -> Option<&Children<R>>;

    fn parent(&self) -> Option<&R>;

    fn props(&self) -> &Props;
    fn props_mut(&mut self) -> &mut Props;
}

type PropValue = serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Props(HashMap<String, PropValue>);

impl Props {
    pub fn get(&self, key: &str) -> Option<&PropValue> {
        self.0.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut PropValue> {
        self.0.get_mut(key)
    }

    pub fn set(&mut self, key: String, val: serde_json::Value) -> Option<PropValue> {
        self.0.insert(key, val)
    }

    pub fn remove(&mut self, key: &str) -> Option<PropValue> {
        self.0.remove(key)
    }

    pub fn append_string_space_separated(
        &mut self,
        key: String,
        val: String,
    ) -> Result<(), anyhow::Error> {
        match self.get_mut(&key) {
            None => {
                self.set(key, val.into());
                Ok(())
            }
            Some(serde_json::Value::String(str)) => {
                str.push(' ');
                str.push_str(&val);
                Ok(())
            }
            Some(other) => Err(anyhow!(
                "could not append {} to prop {} with non-string value {}",
                val,
                key,
                other
            )),
        }
    }
}

impl ToString for Props {
    fn to_string(&self) -> String {
        let mut stringified = self
            .0
            .iter()
            .map(|kv| Prop::from(kv).to_string())
            .collect::<Vec<_>>()
            .join(" ");

        if !stringified.is_empty() {
            stringified.insert(0, ' ');
        }

        stringified
    }
}

struct Prop(String, serde_json::Value);

impl From<(&String, &serde_json::Value)> for Prop {
    fn from(kv: (&String, &serde_json::Value)) -> Self {
        Self(kv.0.clone(), kv.1.clone())
    }
}

impl ToString for Prop {
    fn to_string(&self) -> String {
        let mut stringified = String::new();

        fn push_prefix(str: &mut String, key: &str) {
            str.push_str(key);
            str.push_str(r#"=""#);
        }

        match &self.1 {
            PropValue::Bool(true) => stringified.push_str(&self.0),
            PropValue::Number(num) => {
                push_prefix(&mut stringified, &self.0);
                stringified.push_str(&num.to_string());
                stringified.push('"');
            }
            PropValue::String(str) => {
                push_prefix(&mut stringified, &self.0);
                stringified.push_str(&str);
                stringified.push('"');
            }
            PropValue::Array(_) => {
                push_prefix(&mut stringified, &self.0);
                stringified.push_str(r#"[Array]""#)
            }
            PropValue::Object(_) => {
                stringified.push_str(&self.0);
                stringified.push_str(r#"[Object]""#)
            }
            _ => {}
        }

        stringified
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Children<T> {
    Elements(Vec<Self>),
    Element(T),
    Text(String),
}

pub mod arena {
    use super::{boxed::BoxedElement, Children, Element, Props};

    pub struct Arena {
        arena: Vec<ArenaElement>,
    }

    impl Arena {
        pub fn new() -> Self {
            Arena { arena: Vec::new() }
        }
    }

    impl std::ops::Index<ArenaId> for Arena {
        type Output = ArenaElement;

        fn index(&self, index: ArenaId) -> &Self::Output {
            &self.arena[index.0]
        }
    }

    impl std::ops::IndexMut<ArenaId> for Arena {
        fn index_mut(&mut self, index: ArenaId) -> &mut Self::Output {
            &mut self.arena[index.0]
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct ArenaId(usize);

    #[derive(Clone)]
    pub struct ArenaElement {
        tag: Option<String>,
        vtag: Option<String>,
        props: Props,
        pub children: Option<Children<ArenaId>>,
        parent: Option<ArenaId>,
    }

    impl Element<ArenaId> for ArenaElement {
        fn tag(&self) -> Option<String> {
            self.tag.clone()
        }

        fn vtag(&self) -> Option<String> {
            self.vtag.clone()
        }

        fn children(&self) -> Option<&Children<ArenaId>> {
            self.children.as_ref()
        }

        fn parent(&self) -> Option<&ArenaId> {
            self.parent.as_ref()
        }

        fn props(&self) -> &Props {
            &self.props
        }

        fn props_mut(&mut self) -> &mut Props {
            &mut self.props
        }
    }

    impl ArenaElement {
        pub fn to_string(&self, arena: &Arena) -> String {
            if self.vtag().as_deref() == Some("Fragment") {
                self.children
                    .as_ref()
                    .map_or("".into(), |c| c.to_string(arena))
            } else {
                format!(
                    "<{0}{2}{3}>{1}</{0}>",
                    self.tag.clone().unwrap_or("".to_string()),
                    self.children
                        .as_ref()
                        .map_or("".into(), |c| c.to_string(arena)),
                    self.props.to_string(),
                    self.vtag()
                        .map(|s| format!(r#" component="{}""#, s))
                        .unwrap_or("".into())
                )
            }
        }
    }

    impl ArenaElement {
        pub fn from_boxed(
            arena: &mut Arena,
            boxed: &BoxedElement,
            parent: Option<ArenaId>,
        ) -> ArenaId {
            let element = ArenaElement {
                tag: boxed.tag(),
                vtag: boxed.vtag(),
                props: boxed.props().clone(),
                children: None,
                parent,
            };

            arena.arena.push(element);
            let id = ArenaId(arena.arena.len() - 1);

            if boxed.children().is_none() {
                return id;
            }

            fn from_boxed_children(
                arena: &mut Arena,
                children: &Children<BoxedElement>,
                parent: Option<ArenaId>,
            ) -> Children<ArenaId> {
                match children {
                    Children::Text(text) => Children::Text(text.clone()),
                    Children::Element(el) => {
                        Children::Element(ArenaElement::from_boxed(arena, el, parent))
                    }
                    Children::Elements(els) => Children::Elements(
                        els.iter()
                            .map(|el| from_boxed_children(arena, el, parent))
                            .collect(),
                    ),
                }
            }

            let children = from_boxed_children(arena, boxed.children().unwrap(), Some(id));
            arena[id].children = Some(children);

            id
        }
    }

    impl Children<ArenaId> {
        fn to_string(&self, arena: &Arena) -> String {
            match self {
                Children::Element(el) => arena[*el].to_string(arena),
                Children::Text(text) => text.clone(),
                Children::Elements(els) => els
                    .iter()
                    .map(|el| el.to_string(arena))
                    .collect::<Vec<_>>()
                    .join(""),
            }
        }
    }
}

pub mod boxed {
    use super::{Children, Element, Props};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    pub struct BoxedElement {
        tag: Option<String>,
        vtag: Option<String>,
        children: Option<Box<Children<Self>>>,
        props: Props,
    }

    impl Element<Self> for BoxedElement {
        fn tag(&self) -> Option<String> {
            self.tag.clone()
        }

        fn vtag(&self) -> Option<String> {
            self.vtag.clone()
        }

        fn children(&self) -> Option<&Children<Self>> {
            self.children.as_deref()
        }

        fn parent(&self) -> Option<&Self> {
            None
        }

        fn props(&self) -> &Props {
            &self.props
        }

        fn props_mut(&mut self) -> &mut Props {
            &mut self.props
        }
    }
}
