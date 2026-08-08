//! Owned typed IR for indexed program and template value expressions.

use std::sync::Arc;

use crate::{facts::FunctionReturnSource, TypeExpr};

/// A parsed value expression evaluated without reconstructing source text.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexedValueExpression {
    /// A call-free value expression lowered through ordinary value inference.
    Value(TypeExpr),
    /// A direct semantic call/construct expression.
    Call(IndexedValueCall),
    /// A call-bearing compound outside the indexed expression domain.
    UnsupportedCall { point: u32 },
}

/// Call vs construct for an indexed value expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexedValueCallKind {
    Call,
    Construct,
}

/// The authored literal interpretation of one indexed call argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexedValueLiteralMode {
    /// The argument is a bare literal expression: its literal content is
    /// subject to the parameter's widening rule.
    Widened,
    /// The argument's authored form already pins its type: its literal
    /// content is preserved exactly as written.
    Literal,
}

/// One indexed call argument.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedValueCallArg {
    pub expression: IndexedValueExpression,
    pub point: u32,
    pub spread: bool,
    pub literal_mode: IndexedValueLiteralMode,
    /// Whether the argument is a function value at least one of whose
    /// parameters carries no authored type annotation. Such an argument is
    /// withheld from the call's first inference pass.
    pub context_sensitive: bool,
    /// Exact return carrier for an inline callback argument.
    pub function_return_source: Option<FunctionReturnSource>,
}

/// One direct call/construct record. Children are parsed typed IR; no raw
/// expression text is retained.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedValueCall {
    pub point: u32,
    pub kind: IndexedValueCallKind,
    pub callee: Box<IndexedValueExpression>,
    pub receiver: Option<Box<IndexedValueExpression>>,
    pub args: Arc<[IndexedValueCallArg]>,
    pub explicit_type_args: Arc<[TypeExpr]>,
}
