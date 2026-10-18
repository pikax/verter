//! Owned typed IR for indexed program and template value expressions.

use std::sync::Arc;

use crate::{facts::FunctionReturnSource, TypeExpr};

/// A parsed value expression evaluated without reconstructing source text.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub enum IndexedValueExpression {
    /// A call-free value expression lowered through ordinary value inference.
    Value(TypeExpr),
    /// A direct semantic call/construct expression.
    Call(IndexedValueCall),
    /// A member read off a value the indexed domain evaluates — a call's
    /// result (`f().x`), or a member of one (`f().a.b`). As a call's
    /// callee (`b.m().m`) its object is also the call's receiver, which the
    /// call then does not repeat.
    Member {
        object: Box<IndexedValueExpression>,
        name: Arc<str>,
    },
    /// An expression outside the indexed expression domain: a
    /// call-bearing compound, a class field initializer reading `this`, or
    /// a value whose inference exhausted its work or depth fuse.
    UnsupportedCall { point: u32 },
    /// The template strings a tagged template passes as its first
    /// argument: a value of the GLOBAL `TemplateStringsArray` type,
    /// whatever the tag's scope declares under that name.
    TemplateStrings { point: u32 },
}

/// Call vs construct for an indexed value expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, verter_retention::heap_size::HeapSize)]
pub enum IndexedValueCallKind {
    Call,
    Construct,
}

/// The authored literal interpretation of one indexed call argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, verter_retention::heap_size::HeapSize)]
pub enum IndexedValueLiteralMode {
    /// The argument is a bare literal expression: its literal content is
    /// subject to the parameter's widening rule.
    Widened,
    /// The argument's authored form already pins its type: its literal
    /// content is preserved exactly as written.
    Literal,
}

/// One indexed call argument.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
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
    /// Where the checker reports a relation failure of this argument, and
    /// the authored form it elaborates that failure through: the argument
    /// node's start (a spread's `...`, where the checker reports every
    /// argument the spread expands to) and its expression's form.
    pub report: IndexedReportSite,
}

/// Where the checker reports a relation failure an expression's value
/// causes, and how it elaborates the failure into the expression's parts
/// (`elaborateError`).
#[derive(Debug, Clone, PartialEq, Eq, verter_retention::heap_size::HeapSize)]
pub struct IndexedReportSite {
    /// The start of the node the checker reports at.
    pub point: u32,
    /// The expression's elaborated form; `None` for an expression the
    /// checker reports whole.
    pub form: Option<IndexedElaborationForm>,
}

/// An authored expression form the checker elaborates a relation failure
/// into (`elaborateError`), through parentheses.
#[derive(Debug, Clone, PartialEq, Eq, verter_retention::heap_size::HeapSize)]
pub enum IndexedElaborationForm {
    /// An arrow function whose body is one expression and none of whose
    /// parameters is annotated (`elaborateArrowFunction`): its returned
    /// expression.
    Arrow(Box<IndexedReportSite>),
    /// An object literal of plain named members (`elaborateObjectLiteral`),
    /// in authored order.
    Object(Arc<[IndexedElaboratedProperty]>),
    /// An array literal of plain elements (`elaborateArrayLiteral`), in
    /// authored order.
    Array(Arc<[IndexedReportSite]>),
    /// A form the checker elaborates through parts this record does not
    /// carry — an object literal with a spread or a computed name, an array
    /// literal with a spread or a hole, a `const` assertion, an assignment or
    /// comma operand. A failure through it has no site this record can name.
    Unmodelled,
}

/// One member of an [`IndexedElaborationForm::Object`].
#[derive(Debug, Clone, PartialEq, Eq, verter_retention::heap_size::HeapSize)]
pub struct IndexedElaboratedProperty {
    /// The member's name.
    pub name: Arc<str>,
    /// The start of the member's name, where the checker reports a failure
    /// of its value.
    pub point: u32,
    /// The member's initializer; `None` for a shorthand, method or accessor,
    /// whose failure the checker reports at the name.
    pub value: Option<IndexedReportSite>,
}

impl IndexedReportSite {
    /// Move every position this site records by `base`.
    pub fn offset(&mut self, base: u32) {
        let mut pending = vec![self];
        while let Some(site) = pending.pop() {
            site.point = site.point.saturating_add(base);
            match &mut site.form {
                Some(IndexedElaborationForm::Arrow(returned)) => pending.push(returned),
                Some(IndexedElaborationForm::Object(properties)) => {
                    for property in Arc::make_mut(properties) {
                        property.point = property.point.saturating_add(base);
                        if let Some(value) = property.value.as_mut() {
                            pending.push(value);
                        }
                    }
                }
                Some(IndexedElaborationForm::Array(elements)) => {
                    pending.extend(Arc::make_mut(elements).iter_mut());
                }
                Some(IndexedElaborationForm::Unmodelled) | None => {}
            }
        }
    }
}

/// One direct call/construct record. Children are parsed typed IR; no raw
/// expression text is retained.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub struct IndexedValueCall {
    pub point: u32,
    /// The start of the node the checker reports a diagnostic about the
    /// whole call at (`getDiagnosticSpanForCallNode`): a property-access
    /// callee's name, any other callee of a call, and the expression itself
    /// for `new` and a tagged template.
    pub report_point: u32,
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
    /// Move the nested call records this record solely owns onto `out`
    /// (through the objects of a member chain, `f().a.b`), leaving a
    /// template-strings placeholder in their place.
    fn take_nested_calls(&mut self, out: &mut Vec<IndexedValueCall>) {
        fn take(mut expression: &mut IndexedValueExpression, out: &mut Vec<IndexedValueCall>) {
            loop {
                match expression {
                    IndexedValueExpression::Member { object, .. } => expression = object,
                    IndexedValueExpression::Call(_) => {
                        let placeholder = IndexedValueExpression::TemplateStrings { point: 0 };
                        if let IndexedValueExpression::Call(call) =
                            std::mem::replace(expression, placeholder)
                        {
                            out.push(call);
                        }
                        return;
                    }
                    _ => return,
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
