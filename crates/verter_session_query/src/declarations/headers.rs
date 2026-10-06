//! Declaration header records shared by the header index and its consumers.

/// Where one enum member is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumMemberPosition {
    /// The member's source start offset.
    pub start: u32,
    /// The member is in an ambient context (a `declare enum`, an enum of a
    /// declaration file or of an ambient namespace), where the checker
    /// reads a reference to a later declaration as declared before its use.
    pub ambient: bool,
}
