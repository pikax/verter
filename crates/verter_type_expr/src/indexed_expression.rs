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

/// One step from a call argument into one of its authored nested
/// positions: an object literal's member by its statically known key, an
/// array literal's element by its index, or the one element type the
/// elements of an array literal holding a spread or an elision join into.
#[derive(Debug, Clone, PartialEq, Eq, Hash, verter_retention::heap_size::HeapSize)]
pub enum IndexedLiteralStep {
    Member(crate::TypePropertyKey),
    Element(u32),
    /// The element type of an array literal whose elements are no tuple's
    /// (a spread or an elision makes their positions unknown). Each
    /// element, and each element of an array literal a spread spreads, is
    /// a separate arm of it: it widens its own fresh literals before the
    /// elements join.
    Elements,
}

/// A contributor to an authored position whose literals are REGULAR: the
/// checker types it as the literal it already is, never as a fresh literal
/// a mutable location widens (`getWidenedLiteralType` widens fresh
/// literals only).
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub enum IndexedRegularLiteralSource {
    /// A value whose authored form pins its type, typed as this: a type
    /// assertion's type (`"a" as "a"`), or a literal inside a const
    /// assertion (`["a"] as const`).
    Pinned(TypeExpr),
    /// An object or array literal in a const context (`["a"] as const`):
    /// its whole type is pinned, and it holds no literal at its own top
    /// level.
    PinnedComposite,
    /// A read of a value binding. Its literals are regular unless the
    /// binding's declaration keeps a widening literal (`const s = "a"` is
    /// read as the fresh `"a"`), which only the binding's resolution knows.
    /// `root` is the span of the identifier the read starts from: the
    /// scope the call is in resolves the binding it names there, so a
    /// parameter or local that shadows a top-level binding is read as
    /// itself.
    Read {
        value: crate::ValueRef,
        root: verter_span::Span,
    },
}

/// One authored nested position of a call argument that a regular
/// contributor writes.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub struct IndexedLiteralPosition {
    /// The steps from the argument to the position.
    pub path: Arc<[IndexedLiteralStep]>,
    /// The expressions writing the position, one per separate object or
    /// array arm of a conditional on the way to it (`c ? { v: x } : { v: y
    /// }` writes `v` in two arms), else the position's one expression.
    pub arms: Arc<[IndexedLiteralArm]>,
}

/// The contributors one expression writes at an authored position of a
/// call argument. Each separate arm is an expression of its own: the
/// checker types it, and widens its fresh literals, before the arms join,
/// so a literal one arm writes regular is fresh where another arm writes
/// it fresh (`c ? { v: "a" as "a" } : { v: "a" }` widens `v`).
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub struct IndexedLiteralArm {
    /// The arm's regular contributors: the branches of the conditional
    /// the arm's expression is, or its one expression.
    pub regular: Arc<[IndexedRegularLiteralSource]>,
    /// Whether another contributor writes the arm's expression too: a
    /// fresh literal, an object or array literal, or an expression this
    /// classification does not read. Only the regular contributors' own
    /// literals are then regular (`c ? "a" : sl` with `sl: "a"` is the
    /// regular `"a"`, since a union of a fresh literal and its regular
    /// counterpart is the regular one); otherwise every literal at the
    /// position and nested in it is.
    pub mixed: bool,
    /// The fresh scalar literals the arm's expression writes as written.
    pub fresh: Arc<[crate::LiteralValue]>,
}

/// The literal provenance of a call argument's authored nested positions:
/// each position a regular contributor writes. A position not listed holds
/// only fresh literals — the bare literals a mutable location widens.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub struct IndexedLiteralProvenance {
    pub positions: Arc<[IndexedLiteralPosition]>,
}

/// One step from a declaration's inferred type into a nested position its
/// initializer writes: an object literal's member, or an array literal's
/// element type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DeclaredReadStep {
    /// The member with the statically known `key` that the authored
    /// property declared at `at` writes. The declaration span tells the
    /// position apart from a member of the same key another object writes:
    /// another object arm of a conditional (`c ? { v: s } : { v: sl }`
    /// writes two `v` positions, each widened by its own reads), or the
    /// object literal an inline spread's operand is, whose members the
    /// spreading object holds as its own.
    Member {
        key: crate::TypePropertyKey,
        at: verter_span::Span,
    },
    /// The member the authored property declared at `at` writes under a
    /// computed key (`{ [k]: s }`), which only the key's resolution names.
    ComputedMember {
        at: verter_span::Span,
    },
    Element,
}

/// One position a declaration's initializer closes in a mutable location
/// whose widening waits on the bindings it reads: a read is as fresh as its
/// binding (`const s = "a"` is read as the fresh `"a"`, which a mutable
/// location widens), which only the binding's resolution knows, and a
/// fresh literal the position holds stays the regular literal a regular
/// read beside it holds (`c ? "a" : sl` with `sl: "a"` is the regular
/// `"a"`). The inferred type keeps the position's fresh literals as
/// written.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredReadPosition {
    /// The steps from the declaration's inferred type to the position.
    pub path: Arc<[DeclaredReadStep]>,
    /// The fresh literals the position holds as written.
    pub fresh: Arc<[crate::LiteralValue]>,
    /// The regular literals the position holds as written: asserted or
    /// const-context literals.
    pub regular: Arc<[crate::LiteralValue]>,
    /// The value bindings the position reads.
    pub reads: Arc<[crate::ValueRef]>,
}

/// The positions of a declaration's inferred type whose widening waits on
/// the bindings they read ([`DeclaredReadPosition`]). The elements of an
/// array literal are each a position of their own at the array's element
/// type: an element closes before the elements join.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredReadProvenance {
    pub positions: Arc<[DeclaredReadPosition]>,
}

/// One indexed call argument.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
pub struct IndexedValueCallArg {
    pub expression: IndexedValueExpression,
    pub point: u32,
    pub spread: bool,
    pub literal_mode: IndexedValueLiteralMode,
    /// The regular literals the argument nests at its authored positions;
    /// `None` when it nests none.
    pub literal_provenance: Option<Arc<IndexedLiteralProvenance>>,
    /// Whether the argument is a function value at least one of whose
    /// parameters carries no authored type annotation. Such an argument is
    /// withheld from the call's first inference pass.
    pub context_sensitive: bool,
    /// Exact return carrier for an inline callback argument.
    pub function_return_source: Option<FunctionReturnSource>,
}

/// One direct call/construct record. Children are parsed typed IR; no raw
/// expression text is retained.
#[derive(Debug, Clone, PartialEq, verter_retention::heap_size::HeapSize)]
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
