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
    /// The template strings a tagged template passes as its first
    /// argument: a value of the GLOBAL `TemplateStringsArray` type,
    /// whatever the tag's scope declares under that name.
    TemplateStrings { point: u32 },
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

/// A record nests without bound (a call in a call's argument, a receiver
/// that is a call), and the derived drop glue would drop it a native level
/// per level. Dropping moves each nested record this record solely owns
/// onto an explicit stack first, so a nest however deep drops from this
/// loop. Arguments behind a shared `Arc` another owner still holds are left
/// to that owner.
impl Drop for IndexedValueCall {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.take_nested_calls(&mut pending);
        while let Some(mut call) = pending.pop() {
            call.take_nested_calls(&mut pending);
        }
    }
}

impl IndexedValueCall {
    /// Move the nested call records this record solely owns onto `out`,
    /// leaving a template-strings placeholder in their place.
    fn take_nested_calls(&mut self, out: &mut Vec<IndexedValueCall>) {
        fn take(expression: &mut IndexedValueExpression, out: &mut Vec<IndexedValueCall>) {
            if matches!(expression, IndexedValueExpression::Call(_)) {
                let placeholder = IndexedValueExpression::TemplateStrings { point: 0 };
                if let IndexedValueExpression::Call(call) =
                    std::mem::replace(expression, placeholder)
                {
                    out.push(call);
                }
            }
        }
        take(&mut self.callee, out);
        if let Some(receiver) = self.receiver.as_mut() {
            take(receiver, out);
        }
        if let Some(args) = Arc::get_mut(&mut self.args) {
            for argument in args.iter_mut() {
                take(&mut argument.expression, out);
            }
        }
    }
}
