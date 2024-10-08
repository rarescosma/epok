use crate::ResourceLike;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Interface {
    pub name: String,
    pub is_external: bool,
}

impl Interface {
    pub fn new<N: AsRef<str>>(name: N) -> Self {
        Self { name: name.as_ref().to_owned(), is_external: false }
    }

    pub fn external(self) -> Self { Self { is_external: true, ..self } }
}

impl ResourceLike for Interface {
    fn id(&self) -> String { self.name.to_owned() }
    fn is_active(&self) -> bool { true }
}
