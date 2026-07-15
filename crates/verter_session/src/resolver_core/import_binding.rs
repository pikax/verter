/// Import binding form preserved while routing a component reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportBindingKind {
    Named,
    Default,
    Namespace,
}
