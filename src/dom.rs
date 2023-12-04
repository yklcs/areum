use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub trait Element {
    type Ref;

    fn tag(&self) -> Option<String>;

    fn vtag(&self) -> Option<String>;

    fn children(&self) -> Option<&Children<Self::Ref>>;

    fn parent(&self) -> Option<&Self::Ref>;

    fn props(&self) -> &HashMap<String, String>;
    fn props_mut(&mut self) -> &mut HashMap<String, String>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Children<T> {
    Element(T),
    Elements(Vec<Self>),
    Text(String),
}

pub mod arena {
    use std::collections::HashMap;

    use super::{boxed::BoxedElement, Children, Element};

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

    #[derive(Clone, Copy)]
    pub struct ArenaId(usize);

    #[derive(Clone)]
    pub struct ArenaElement {
        tag: Option<String>,
        vtag: Option<String>,
        props: HashMap<String, String>,
        children: Option<Children<ArenaId>>,
        parent: Option<ArenaId>,
    }

    impl Element for ArenaElement {
        type Ref = ArenaId;

        fn tag(&self) -> Option<String> {
            self.tag.clone()
        }

        fn vtag(&self) -> Option<String> {
            self.vtag.clone()
        }

        fn children(&self) -> Option<&Children<Self::Ref>> {
            self.children.as_ref()
        }

        fn parent(&self) -> Option<&Self::Ref> {
            self.parent.as_ref()
        }

        fn props(&self) -> &HashMap<String, String> {
            &self.props
        }

        fn props_mut(&mut self) -> &mut HashMap<String, String> {
            &mut self.props
        }
    }

    impl ArenaElement {
        pub fn to_string(&self, arena: &Arena) -> String {
            let attrs = self
                .props()
                .iter()
                .map(|(k, v)| format!(" {}={}", k, v))
                .collect::<Vec<_>>()
                .join("");

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
                    attrs,
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
    use std::collections::HashMap;

    use super::{Children, Element};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    pub struct BoxedElement {
        tag: Option<String>,
        vtag: Option<String>,
        children: Option<Box<Children<Self>>>,
        props: HashMap<String, String>,
    }

    impl Element for BoxedElement {
        type Ref = Self;

        fn tag(&self) -> Option<String> {
            self.tag.clone()
        }

        fn vtag(&self) -> Option<String> {
            self.vtag.clone()
        }

        fn children(&self) -> Option<&Children<Self::Ref>>
        where
            Self: Sized,
        {
            self.children.as_deref()
        }

        fn parent(&self) -> Option<&Self::Ref>
        where
            Self: Sized,
        {
            None
        }

        fn props(&self) -> &HashMap<String, String> {
            &self.props
        }

        fn props_mut(&mut self) -> &mut HashMap<String, String> {
            &mut self.props
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

    impl ToString for Children<BoxedElement> {
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
}
