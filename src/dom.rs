use std::collections::HashMap;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

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
    use super::{boxed::BoxedElement, Children, Props};

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
    pub enum ArenaElement {
        Intrinsic {
            props: Props,
            children: Option<Children<ArenaId>>,
            scope: String,

            tag: String,
        },
        Virtual {
            props: Props,
            children: Option<Children<ArenaId>>,
            scope: String,

            style: Option<String>,
        },
    }

    impl ArenaElement {
        pub fn props(&self) -> Props {
            match self {
                Self::Intrinsic { props, .. } => props.clone(),
                Self::Virtual { props, .. } => props.clone(),
            }
        }

        pub fn props_mut(&mut self) -> &mut Props {
            match self {
                Self::Intrinsic { props, .. } => props,
                Self::Virtual { props, .. } => props,
            }
        }

        pub fn children(&self) -> Option<&Children<ArenaId>> {
            match self {
                Self::Intrinsic { children, .. } => children.as_ref(),
                Self::Virtual { children, .. } => children.as_ref(),
            }
        }

        pub fn children_mut(&mut self) -> &mut Option<Children<ArenaId>> {
            match self {
                Self::Intrinsic { children, .. } => children,
                Self::Virtual { children, .. } => children,
            }
        }

        pub fn scope(&self) -> String {
            match self {
                Self::Intrinsic { scope, .. } => scope.clone(),
                Self::Virtual { scope, .. } => scope.clone(),
            }
        }

        pub fn to_string(&self, arena: &Arena) -> String {
            match self {
                Self::Intrinsic {
                    props,
                    children,
                    tag,
                    ..
                } => {
                    format!(
                        "<{tag}{1}>{0}</{tag}>",
                        children.clone().map_or("".into(), |c| c.to_string(arena)),
                        props.to_string(),
                    )
                }
                Self::Virtual { children, .. } => match children {
                    Some(children) => children.to_string(arena),
                    None => "".into(),
                },
            }
        }
    }

    impl ArenaElement {
        pub fn from_boxed(
            arena: &mut Arena,
            boxed: &BoxedElement,
            _parent: Option<ArenaId>,
        ) -> ArenaId {
            let element = match boxed {
                BoxedElement::Intrinsic {
                    props,
                    children: _,
                    scope,
                    tag,
                } => ArenaElement::Intrinsic {
                    props: props.clone(),
                    children: None,
                    scope: scope.clone(),
                    tag: tag.clone(),
                },
                BoxedElement::Virtual {
                    props,
                    children: _,
                    scope,
                    style,
                } => ArenaElement::Virtual {
                    props: props.clone(),
                    children: None,
                    scope: scope.clone(),
                    style: style.clone(),
                },
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

            let children = from_boxed_children(arena, &boxed.children().unwrap(), Some(id));
            *arena[id].children_mut() = Some(children);

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
    use super::{Children, Props};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "kind")]
    #[serde(rename_all = "lowercase")]
    pub enum BoxedElement {
        Intrinsic {
            props: Props,
            children: Option<Box<Children<Self>>>,
            scope: String,

            tag: String,
        },
        Virtual {
            props: Props,
            children: Option<Box<Children<Self>>>,
            scope: String,

            style: Option<String>,
        },
    }

    impl BoxedElement {
        pub fn props(&self) -> Props {
            match self {
                Self::Intrinsic { props, .. } => props.clone(),
                Self::Virtual { props, .. } => props.clone(),
            }
        }

        pub fn children(&self) -> Option<Box<Children<Self>>> {
            match self {
                Self::Intrinsic { children, .. } => children.clone(),
                Self::Virtual { children, .. } => children.clone(),
            }
        }

        pub fn scope(&self) -> String {
            match self {
                Self::Intrinsic { scope, .. } => scope.clone(),
                Self::Virtual { scope, .. } => scope.clone(),
            }
        }
    }
}
