//! The per-file function program index: function positions, their structural records and
//! the keyed lookups that hand them out. Built by the parser front-end.

use crate::analysis::types::Hash16;
use crate::facts::SymbolSpace;
use std::sync::Arc;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::facts::{FunctionReturnSource, ProgramExpressionIdentity};
use verter_type_expr::span_origins::DeclContributorAnchor;

mod capture_summary;
mod class_index;
pub use capture_summary::{
    CaptureBindings, CaptureSummaryCounts, CapturedReads, FunctionCaptures, NestedCaptures,
};
use capture_summary::{CaptureSummaries, FrameCaptureHandle};
pub use class_index::{
    ClassBase, ClassBaseMatch, ClassIndex, ClassIndexMatch, ClassSyntaxDiscovery, ClassSyntaxRecord,
};

/// The declaration a served function position belongs to (content-free;
/// the owner discriminates script-block owners, the name is the registered
/// merged-symbol name — namespaces qualify `Ns.Name` exactly like the eval
/// env registration).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, verter_no_typeexpr::NoTypeExpr)]
pub struct FunctionDeclarationRef {
    /// Lexical top-level owner of the contributing statement.
    pub owner: verter_type_expr::TopLevelOwnerId,
    /// Registered merged-symbol name.
    pub name: Arc<str>,
    /// Type-space vs value-space discriminator (functions are value-space;
    /// the discriminator keeps the ref shape aligned with slot identity).
    pub space: SymbolSpace,
}

/// One ordinal step from a contributing top-level statement down to the
/// function node. Named positions / small ordinals only — never a byte
/// span, never a lowered type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionDescentStep {
    /// The statement IS the function declaration.
    FunctionDeclaration,
    /// The init of the variable declarator at `declarator_ordinal` (the
    /// init is the arrow / function expression).
    VariableInitializer { declarator_ordinal: u32 },
    /// The class member at `member_ordinal` (`ClassBody.body` index).
    ClassMember { member_ordinal: u32 },
    /// The object-literal method at `member_ordinal` inside the current
    /// initializer object expression.
    ObjectMember { member_ordinal: u32 },
    /// The object-literal method at `member_ordinal` inside an
    /// `export default { … }` object expression.
    ExportDefaultObjectMember { member_ordinal: u32 },
    /// The statement at `statement_ordinal` inside a namespace block —
    /// ONE step per nesting level, so a nested namespace's member carries
    /// every enclosing block's step in order (`N.M.make` descends into
    /// `N`'s block, then `M`'s). A descent that kept only the innermost
    /// ordinal would resolve it in the OUTER block and serve a different
    /// declaration's body.
    NamespaceMember { statement_ordinal: u32 },
    /// The statement at `statement_ordinal` inside the enclosing
    /// function's body (a hoisted nested function declaration).
    BodyStatement { statement_ordinal: u32 },
    /// The argument at `arg_ordinal` of the enclosing body's
    /// `call_ordinal`-th call site (source order) — a callback position.
    CallArgument { call_ordinal: u32, arg_ordinal: u32 },
    /// The CALLEE of the enclosing body's `call_ordinal`-th call site
    /// (source order) when that callee is itself a function / arrow
    /// expression — an immediately-invoked function expression.
    CallCallee { call_ordinal: u32 },
    /// A directly nested callable in the shared source-order inventory.
    NestedCallable { ordinal: u32 },
    /// The `extends` EXPRESSION of the class declaration — an indexed
    /// program expression (`class K extends Mixin(Base) {}`), never a
    /// function position.
    ClassHeritage,
}

/// Arena-free locator for one function's body inside the retained parse
/// snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionBodyLocator {
    /// The contributing top-level statement.
    pub contributor: DeclContributorAnchor,
    /// Ordinal descent from the contributing statement to the function node.
    pub descent: FunctionDescent,
}

/// The ordinal descent of a [`FunctionBodyLocator`], from the contributing
/// statement to the function node. A descent shares every prefix with the
/// descents of the functions enclosing it, so a function nested `n` deep
/// adds one step to its parent's descent rather than copying `n`, and
/// every locator of a nest costs its own step only.
#[derive(Clone, Default)]
pub struct FunctionDescent(Option<Arc<DescentLink>>);

/// One step of a [`FunctionDescent`] and the descent it extends.
struct DescentLink {
    step: FunctionDescentStep,
    len: usize,
    /// The whole descent's hash, folded from its parent's and this step:
    /// hashing a descent reads it rather than walking the path.
    hash: u64,
    /// Whether any step of the whole descent enters a namespace block.
    namespace_member: bool,
    parent: FunctionDescent,
}

impl FunctionDescent {
    /// The empty descent: the contributing statement itself.
    #[must_use]
    pub fn new() -> Self {
        Self(None)
    }

    /// This descent extended by `step`, sharing this one.
    #[must_use]
    pub fn then(&self, step: FunctionDescentStep) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = rustc_hash::FxHasher::default();
        self.path_hash().hash(&mut hasher);
        step.hash(&mut hasher);
        Self(Some(Arc::new(DescentLink {
            step,
            len: self.len() + 1,
            hash: hasher.finish(),
            namespace_member: self.has_namespace_member()
                || matches!(step, FunctionDescentStep::NamespaceMember { .. }),
            parent: self.clone(),
        })))
    }

    /// Whether any step of this descent enters a namespace block, read
    /// without walking the path.
    pub fn has_namespace_member(&self) -> bool {
        self.0.as_ref().is_some_and(|link| link.namespace_member)
    }

    /// The hash of the whole path (0 for the empty descent).
    fn path_hash(&self) -> u64 {
        self.0.as_ref().map_or(0, |link| link.hash)
    }

    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |link| link.len)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// The step that lands on the function node.
    pub fn last(&self) -> Option<&FunctionDescentStep> {
        self.0.as_ref().map(|link| &link.step)
    }

    /// Whether this descent is `parent` extended by one step, sharing it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn extends(&self, parent: &FunctionDescent) -> bool {
        self.0
            .as_ref()
            .is_some_and(|link| match (&link.parent.0, &parent.0) {
                (Some(shared), Some(parent)) => Arc::ptr_eq(shared, parent),
                (None, None) => true,
                _ => false,
            })
    }

    /// The steps, from the last to the first.
    fn links(&self) -> impl Iterator<Item = &DescentLink> {
        std::iter::successors(self.0.as_deref(), |link| link.parent.0.as_deref())
    }

    /// The steps, from the contributing statement down.
    pub fn to_vec(&self) -> Vec<FunctionDescentStep> {
        let mut steps: Vec<FunctionDescentStep> = self.links().map(|link| link.step).collect();
        steps.reverse();
        steps
    }
}

impl From<&[FunctionDescentStep]> for FunctionDescent {
    fn from(steps: &[FunctionDescentStep]) -> Self {
        steps
            .iter()
            .fold(Self::new(), |descent, step| descent.then(*step))
    }
}

impl PartialEq for FunctionDescent {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let (mut left, mut right) = (self.0.as_ref(), other.0.as_ref());
        while let (Some(l), Some(r)) = (left, right) {
            // A shared link shares the rest of the descent.
            if Arc::ptr_eq(l, r) {
                return true;
            }
            if l.step != r.step {
                return false;
            }
            (left, right) = (l.parent.0.as_ref(), r.parent.0.as_ref());
        }
        true
    }
}

impl Eq for FunctionDescent {}

impl std::hash::Hash for FunctionDescent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        self.path_hash().hash(state);
    }
}

impl std::fmt::Debug for FunctionDescent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.to_vec()).finish()
    }
}

/// A descent is as long as the nest it addresses, and the derived drop
/// would release it a native level per step: the links this descent solely
/// owns are released from this loop.
impl Drop for FunctionDescent {
    fn drop(&mut self) {
        let mut next = self.0.take();
        while let Some(link) = next {
            next = match Arc::try_unwrap(link) {
                Ok(mut link) => link.parent.0.take(),
                Err(_) => None,
            };
        }
    }
}

/// The full program identity of one served function position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, verter_no_typeexpr::NoTypeExpr)]
pub struct FunctionProgramKey {
    /// The owning declaration.
    pub declaration: FunctionDeclarationRef,
    /// Which authored position of the declaration this callable occupies.
    pub part: FunctionPartIdentity,
    /// Signature ordinal inside an overload group, in source order (the
    /// trailing implementation is the last ordinal). Zero outside overload
    /// groups.
    pub overload_ordinal: u32,
}

/// One formal parameter's binding fact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionParamRecord {
    /// The binding name (`None` for a destructured parameter).
    pub name: Option<Arc<str>>,
    /// Whether the parameter is optional (`?`).
    pub optional: bool,
    /// Whether this is the rest parameter.
    pub rest: bool,
    /// Whether the parameter carries an authored TS type annotation.
    pub has_ts_annotation: bool,
    /// The name the authored annotation spells when it is a bare type
    /// reference without type arguments (`x: T`), which names a type
    /// parameter when one is in scope. `None` for any other annotation and
    /// for the rest parameter.
    pub annotation_reference: Option<Arc<str>>,
}

/// The kind of one local binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub enum FunctionBindingKind {
    /// A formal parameter.
    Param,
    /// A `const` declarator.
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` declarator.
    Var,
    /// A nested function declaration's name.
    NestedFunction,
    /// A local class declaration.
    Class,
    /// A catch parameter, including pattern elements.
    CatchParam,
    /// A local enum declaration.
    Enum,
    /// A local namespace declaration.
    Namespace,
    /// A local import-equals declaration.
    ImportEquals,
}

/// One local binding (parameter, variable declarator, nested function name).
///
/// The frame's binding list is the frame's FULL source-order inventory: no
/// name deduplication and no reordering, so two same-name bindings in
/// different lexical scopes of one frame stay distinct entries at distinct
/// slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionBindingRecord {
    /// The binding name.
    pub name: Arc<str>,
    /// The binding kind.
    pub kind: FunctionBindingKind,
    /// The binding's span.
    pub span: verter_span::Span,
    /// The span of the lexical scope the binding is visible in: the
    /// innermost enclosing block-like region for a `const` / `let` /
    /// nested function declaration, the whole frame for a parameter or a
    /// `var`. A reference resolves to the same-name binding whose scope
    /// CONTAINS the reference and is innermost among those.
    pub scope_span: verter_span::Span,
    /// Whether the declarator has the checker's EVOLVING-array form: an
    /// unannotated whole-identifier declarator initialised to an empty
    /// array literal (`const a = []`). Under `noImplicitAny` its declared
    /// type is `autoArrayType`, which every frame referencing it — the
    /// defining one and each capturing one — types by the checker's
    /// evolving-array rule.
    pub evolving_array: bool,
}

/// Runtime-variable equivalence over exact declaration slots. Hoisted
/// redeclarations share their authored scope; block lexicals remain distinct.
pub fn canonical_runtime_binding_slots(bindings: &[FunctionBindingRecord]) -> Vec<u32> {
    let mut groups = rustc_hash::FxHashMap::default();
    let hoists = |kind| {
        matches!(
            kind,
            FunctionBindingKind::Param
                | FunctionBindingKind::Var
                | FunctionBindingKind::NestedFunction
        )
    };
    for (slot, binding) in bindings.iter().enumerate() {
        if hoists(binding.kind) {
            let key = (
                binding.name.as_ref(),
                binding.scope_span.start,
                binding.scope_span.end,
            );
            let canonical = groups.entry(key).or_insert(slot as u32);
            if binding.kind == FunctionBindingKind::Param {
                *canonical = slot as u32;
            }
        }
    }
    bindings
        .iter()
        .enumerate()
        .map(|(slot, binding)| {
            if hoists(binding.kind) {
                groups[&(
                    binding.name.as_ref(),
                    binding.scope_span.start,
                    binding.scope_span.end,
                )]
            } else {
                slot as u32
            }
        })
        .collect()
}

/// One identifier reference in the current function body (nested function
/// bodies excluded — their references resolve in their own frames).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionReferenceRecord {
    /// The referenced name.
    pub name: Arc<str>,
    /// The reference span.
    pub span: verter_span::Span,
    /// Exact source-owned lexical disposition, including unsupported class locals.
    pub binding: FunctionReferenceBinding,
    /// Syntactic evaluation role, independent of expression-site grouping.
    /// `None` is a write-only occurrence, which still captures its binding.
    pub read_role: Option<FunctionReadRole>,
    /// A static member projection, or empty for a whole/dynamic root read.
    pub path: Arc<[Arc<str>]>,
}

/// A whole authored `typeof name` result consumed by indexed call lowering.
/// Separate from runtime reads: it neither executes nor captures its operand.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSourceTypeQuery {
    pub name: Arc<str>,
    pub span: verter_span::Span,
    pub binding: FunctionReferenceBinding,
}

/// A `typeof name` written in a TYPE position of this frame: its own
/// parameter list, a declarator's annotation, or a type an expression
/// carries (`as`, `satisfies`, a type assertion, a call's type
/// arguments). Only a bare identifier without type arguments is recorded.
/// Like [`FunctionSourceTypeQuery`], it neither executes nor captures its
/// operand; it names the value whose type the position reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionTypeQuery {
    pub name: Arc<str>,
    pub span: verter_span::Span,
    pub binding: FunctionReferenceBinding,
    pub position: FunctionTypeQueryPosition,
}

/// Where a [`FunctionTypeQuery`] sits, which decides when the frame's
/// evaluation reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionTypeQueryPosition {
    /// The frame's own parameter list: read whenever the frame evaluates.
    Parameter,
    /// The annotation of the declarator binding at this span: read with
    /// that binding's value.
    Declarator(verter_span::Span),
    /// A type an expression carries: read with that expression.
    Expression,
}

/// The exact lexical answer for an indexed occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionReferenceBinding {
    Resolved(FlowBindingIdentity),
    Free,
    UnmodeledLocal,
}

impl FunctionReferenceBinding {
    /// A modeled runtime identity only; neither other disposition is a capture.
    pub fn resolved(&self) -> Option<&FlowBindingIdentity> {
        match self {
            Self::Resolved(identity) => Some(identity),
            Self::Free | Self::UnmodeledLocal => None,
        }
    }
}

/// A value read may also govern control or supply a call's callee/arguments.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionReadRole {
    #[default]
    Value,
    CallInput,
    ControlInput,
    CallAndControlInput,
}

impl FunctionReadRole {
    pub fn is_effect_input(self) -> bool {
        self != Self::Value
    }

    pub fn with_call(self) -> Self {
        match self {
            Self::Value | Self::CallInput => Self::CallInput,
            Self::ControlInput | Self::CallAndControlInput => Self::CallAndControlInput,
        }
    }

    pub fn with_control(self) -> Self {
        match self {
            Self::Value | Self::ControlInput => Self::ControlInput,
            Self::CallInput | Self::CallAndControlInput => Self::CallAndControlInput,
        }
    }
}

/// One `return` site of the current function, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionReturnSite {
    /// Source-order ordinal among the function's return sites.
    pub ordinal: u32,
    /// Whether the site carries an argument expression (bare `return;`
    /// contributes `undefined`).
    pub has_argument: bool,
    /// The return statement's span.
    pub span: verter_span::Span,
}

/// The authored literal shape of one call argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionCallArgLiteralMode {
    /// An ordinary (widened) argument position.
    Widened,
    /// A fresh literal argument position (string / number / boolean /
    /// template / object / array literal).
    Literal,
}

/// One argument of an indexed call site, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionCallArgRecord {
    /// The argument expression's program point.
    pub point: u32,
    /// Whether the argument is a spread element (`...xs`).
    pub spread: bool,
    /// The authored literal shape of the argument.
    pub literal_mode: FunctionCallArgLiteralMode,
    /// Whether the argument is a function / arrow expression (a callback
    /// position — its return is a served function position).
    pub is_function_value: bool,
    /// Exact return carrier when this argument is an indexed callback value.
    pub function_return_source: Option<FunctionReturnSource>,
}

/// One indexed call site in the current function body: the program point,
/// the callee carrier, the exact same-file target, and the per-argument
/// facts. This is the unified call record every call-shaped consumer
/// reads — never a raw-string reparse, never a synthesized call type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionCallSiteRecord {
    /// The call expression's span (the program point's offset identity).
    pub span: verter_span::Span,
    /// The callee shape.
    pub callee: FunctionEffectCallee,
    /// The exact same-file served function this site calls, when the
    /// callee is a bare identifier the enclosing frame's lexical scope
    /// binds to an indexed position (direct same-slot recursion
    /// included). `None` for every other callee shape, and for a site
    /// indexed outside a function frame (a top-level indexed expression
    /// has no frame-local lexical scope to resolve against).
    pub target: Option<FunctionProgramKey>,
    /// The ordered argument facts.
    pub args: Arc<[FunctionCallArgRecord]>,
}

/// How one indexed expression supplies its value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProgramExpressionSource {
    /// A call-free value expression, lowered lazily from the retained AST.
    Value,
    /// A direct semantic call/construct record.
    SemanticCall {
        kind: ProgramExpressionCallKind,
        site: FunctionCallSiteRecord,
    },
    /// An indexed callback/function value's exact return carrier.
    FunctionReturn(FunctionReturnSource),
    /// A class field's initializer served as its own position
    /// ([`FunctionNode::Initializer`]): the field's value is the
    /// position's body-derived return, its fresh literals widened unless
    /// the field is `readonly`.
    FieldInitializer {
        source: FunctionReturnSource,
        readonly: bool,
    },
    /// A call-bearing compound outside the indexed expression domain.
    UnsupportedCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramExpressionCallKind {
    Call,
    Construct,
}

/// One declaration/callback expression indexed by content-free program point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramExpressionRecord {
    pub point: ProgramExpressionIdentity,
    pub span: verter_span::Span,
    pub locator: FunctionBodyLocator,
    pub source: ProgramExpressionSource,
}

/// One captured binding's content-free identity: the frame that DECLARES
/// the binding plus the binding's stable source-order slot in that frame's
/// full binding inventory, alongside the binding's name and kind. The
/// `(defining_function, binding_slot)` pair is the identity — it separates
/// two same-name binders in different frames AND two same-name binders in
/// different lexical scopes of one frame, neither of which a name (or a
/// per-capture-list ordinal) can distinguish. NEVER a node id, a type, a
/// content hash, or a span — capture types rehydrate from indexed binding /
/// reaching-definition facts under the final type substitution.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct FlowBindingIdentity {
    /// The binding name.
    pub name: Arc<str>,
    /// The binding kind in the DEFINING frame.
    pub kind: FunctionBindingKind,
    /// The frame whose binding inventory declares this binding.
    pub defining_function: FunctionProgramKey,
    /// The binding's source-order slot in that frame's binding inventory.
    pub binding_slot: u32,
    /// [`FunctionBindingRecord::evolving_array`] of the binding in the
    /// DEFINING frame. Metadata like [`Self::kind`]: identity is the slot.
    pub evolving_array: bool,
}

impl PartialEq for FlowBindingIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.defining_function == other.defining_function && self.binding_slot == other.binding_slot
    }
}

impl Eq for FlowBindingIdentity {}

impl std::hash::Hash for FlowBindingIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.defining_function, state);
        std::hash::Hash::hash(&self.binding_slot, state);
    }
}

/// The content-free capture environment of a nested function position:
/// capture binding identities (and their deterministic source order)
/// only. Until non-empty narrowing lands, a capture whose type cannot be
/// reconstructed from the indexed binding / reaching-definition facts is
/// a typed ReturnOnly, never guessed or separately keyed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct CanonicalCaptureIdentity(pub Arc<[FlowBindingIdentity]>);

/// The callee shape of one evaluation-effect call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionEffectCallee {
    /// A bare identifier callee (`g()`).
    Identifier(Arc<str>),
    /// A static member path (`a.b.c()`).
    StaticMember(Arc<[Arc<str>]>),
    /// Any other callee shape (computed, call-result, `this`-rooted).
    Other,
}

/// One evaluation-effect call site in the current function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionEffectRecord {
    /// The call expression's span.
    pub span: verter_span::Span,
    /// The callee shape.
    pub callee: FunctionEffectCallee,
}

/// One write site (assignment or update) in the current function body.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionWriteRecord {
    /// The write expression's span.
    pub span: verter_span::Span,
    /// Every authored target root, including destructuring elements.
    pub targets: Arc<[FunctionWriteTarget]>,
}

/// Whether a write replaces the variable or mutates one of its members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionWriteKind {
    Whole,
    Member,
}

/// Exact write-root evidence. A computed key is a read, never the target root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionWriteTarget {
    Binding {
        reference: FunctionReferenceRecord,
        kind: FunctionWriteKind,
    },
    Unsupported {
        span: verter_span::Span,
    },
}

/// A captured value dependency, retaining its exact root and static path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionCapturedRead {
    pub binding: FlowBindingIdentity,
    pub path: Arc<[Arc<str>]>,
    pub span: verter_span::Span,
}

/// The exact closure subjects and read dependencies of one callable in a
/// frame's PARAMETER LIST (`cb = () => a`): no index entry serves it, but
/// the frame's own resolution names everything it reads and writes from
/// around it, so its capture set is exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionParameterCallableCaptures {
    pub span: verter_span::Span,
    pub bindings: CanonicalCaptureIdentity,
    pub reads: Arc<[FunctionCapturedRead]>,
}

/// The control-region kind of one skeleton region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionControlKind {
    /// A block statement.
    Block,
    /// An `if` statement (consequent / alternate arms nest as regions).
    If,
    /// A loop (`for` / `for-in` / `for-of` / `while` / `do-while`).
    Loop,
    /// A `switch` statement.
    Switch,
    /// A `try` statement.
    Try,
    /// A labeled statement.
    Labeled,
}

/// One control-region skeleton entry (the current function's statement
/// tree, nested function bodies excluded).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionControlRegion {
    /// The region kind.
    pub kind: FunctionControlKind,
    /// Whether the region's statement subtree contains a `return` of the
    /// current function (drives return-transparency: return-free loop /
    /// labeled constructs are fall-through transparent; return-bearing
    /// loop / labeled regions, and every switch / try, are unsupported).
    pub has_return: bool,
    /// The region statement's span.
    pub span: verter_span::Span,
}

/// One exact direct local call: the callee is a bare identifier bound to a
/// function in the same index (a same-file, syntactically exact target —
/// direct same-slot recursion included).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionDirectCall {
    /// The call expression's span.
    pub span: verter_span::Span,
    /// The callee's program identity.
    pub target: FunctionProgramKey,
}

/// One parameter of an indexed function's OWN type-parameter clause.
///
/// Purely syntactic: a name, whether the parameter authored a DEFAULT,
/// and the smallest formal-parameter ordinal whose authored type
/// annotation names it. The default's TYPE is deliberately absent —
/// this index is a shallow declaration fact, never a body lowering — so
/// a caller that needs the default's meaning demands it through the
/// shared lazy body service, and pays for it only on the clauses that
/// have one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionProgramTypeParam {
    /// The type-parameter name.
    pub name: Arc<str>,
    /// Whether the parameter authored a default (`<T = D>`).
    pub has_default: bool,
    /// The SMALLEST formal-parameter ordinal whose authored type
    /// annotation references this name, or `None` when no parameter
    /// type mentions it at all.
    ///
    /// This is the caller's inference oracle, and the ONLY fact a
    /// caller needs to apply TypeScript's actual default rule: a
    /// declared default resolves the parameter only when inference
    /// produced NO candidate, and inference can produce a candidate
    /// only from an argument the call actually supplies at an ordinal
    /// whose parameter type names the parameter. `f<T = number>(x:
    /// string)` therefore takes its default even at an
    /// argument-bearing call, and `f<T = number>(a: string, b?: T)`
    /// takes it at `f("a")`.
    ///
    /// A REST parameter occupies its own ordinal and covers every
    /// later one, so the same `ordinal < argument_count` test holds:
    /// `f<T = number>(...xs: T[])` has occurrence ordinal 0, which no
    /// zero-argument call supplies.
    ///
    /// Shadowing-aware: a nested function / constructor type inside a
    /// parameter annotation that RE-DECLARES the name owns its own
    /// subtree, so `f<T = number>(cb: <T>(y: T) => T)` records `None`
    /// for the outer `T` — which is what the checker answers.
    pub first_parameter_occurrence: Option<u32>,
}

/// The references each parameter-list callable makes to names it does not
/// declare, keyed by the callable's span.
pub type ParameterCallableReferences = Arc<[(verter_span::Span, Arc<[FunctionReferenceRecord]>)]>;

/// One DISCOVERED function position, as the parser front-end's discovery
/// walk records it: identity, body locator, structural inventory, and the
/// whole-function stable hash.
///
/// A discovery record is mutable working state: the walk fills it in over
/// several passes (captures, call targets, nested reads, hashes), so its
/// fields are public. It is never served.
/// [`FunctionProgramIndex::from_discovery`] seals each record into a
/// read-only [`FunctionProgramEntry`] and derives the index's keyed
/// lookups from the sealed inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionProgramDiscovery {
    /// The program identity.
    pub key: FunctionProgramKey,
    /// The authored function node's span.
    pub span: verter_span::Span,
    /// The body environment, excluding the separate parameter-default environment.
    pub body_span: verter_span::Span,
    /// Arena-free body locator into the retained snapshot.
    pub locator: FunctionBodyLocator,
    /// Formal parameter facts (source order).
    pub params: Arc<[FunctionParamRecord]>,
    /// Local bindings (parameters, declarators, nested function names).
    pub bindings: Arc<[FunctionBindingRecord]>,
    pub unmodeled_bindings: Arc<[FunctionBindingRecord]>,
    /// Identifier references in the current function body.
    pub references: Arc<[FunctionReferenceRecord]>,
    pub source_type_queries: Arc<[FunctionSourceTypeQuery]>,
    /// Every `typeof name` in a type position of this frame, in source
    /// order.
    pub type_queries: Arc<[FunctionTypeQuery]>,
    /// Return sites in source order.
    pub return_sites: Arc<[FunctionReturnSite]>,
    /// Write sites (assignments / updates).
    pub writes: Arc<[FunctionWriteRecord]>,
    /// Variables declared by this frame that any descendant callable writes.
    /// Intervening local bindings retain their own identities and are excluded.
    pub descendant_writes: Arc<[FlowBindingIdentity]>,
    /// The subset of [`Self::descendant_writes`] a descendant callable
    /// ASSIGNS whole — an assignment, an update or a destructuring target,
    /// never a member write. With this frame's own whole writes it is
    /// every assignment the checker's `isSymbolAssigned` reads.
    pub descendant_assignments: Arc<[FlowBindingIdentity]>,
    /// The whole-binding assignments code no entry serves makes to names it
    /// does not itself declare: a class's members and initializers, and a
    /// callable in the parameter list. Each resolves in this frame's
    /// lexical scope and joins the defining frame's
    /// [`Self::descendant_assignments`] (this frame's own included).
    pub unserved_assignments: Arc<[FunctionReferenceRecord]>,
    /// The references each parameter-list callable makes to names it does
    /// not declare, by the callable's span: resolved with the frame's own
    /// references into [`Self::parameter_callable_captures`].
    pub parameter_callable_references: ParameterCallableReferences,
    /// The exact captures of each parameter-list callable whose every
    /// reference resolved (the others keep their typed gap).
    pub parameter_callable_captures: Arc<[FunctionParameterCallableCaptures]>,
    /// Evaluation-effect call sites.
    pub effects: Arc<[FunctionEffectRecord]>,
    /// Indexed call sites: program point, callee carrier, exact same-file
    /// target, and per-argument facts — the unified call record every
    /// call-shaped consumer reads.
    pub call_sites: Arc<[FunctionCallSiteRecord]>,
    /// Control-region skeleton.
    pub control: Arc<[FunctionControlRegion]>,
    /// Exact direct local call targets.
    pub direct_calls: Arc<[FunctionDirectCall]>,
    /// This function's OWN type-parameter clause, in declaration order.
    ///
    /// A shallow syntactic FACT, not a lowering: the names are what a
    /// CALLER needs to instantiate the callee's clause, and the caller
    /// cannot read them off the callee's declared or body-derived return
    /// (a parameter that never bound interns as a deferred name
    /// reference, indistinguishable from an unrelated free name).
    ///
    /// This index answers for every position it INDEXES, which is what a
    /// direct-call target is by construction, and which the value
    /// registry is not: a namespace-scoped function has no prepared
    /// declaration at all. It is NOT an inventory of every declared
    /// signature — an overload group is indexed once, at its
    /// implementation, so a caller reaching a VISIBLE overload's clause
    /// through here would be reading the implementation's.
    pub type_parameters: Arc<[FunctionProgramTypeParam]>,
    /// The enclosing function position for a NESTED served position
    /// (a hoisted nested function declaration or a call-argument
    /// function value); `None` for a top-level position.
    pub lexical_parent: Option<Box<FunctionProgramKey>>,
    /// Whether this NESTED position is a class expression's method or
    /// accessor (the checker types its body-derived return as a class
    /// method's, never as a function expression's). `false` for every
    /// other position.
    pub class_member: bool,
    /// The authored binding name of a HOISTED NESTED FUNCTION DECLARATION
    /// (`function inner() { … }` inside another body). `None` for every
    /// other position — a top-level position, a callback value, an
    /// initializer arrow. It is the lexical name a bare-identifier call in
    /// the parent frame binds to.
    pub nested_declaration_name: Option<Arc<str>>,
    /// Whether THIS frame creates no callable no entry serves — a class
    /// (its constructor, member bodies and field initializers) or a
    /// callable in a parameter list. A cell retained there is named by no
    /// record. Sealing folds it over the nested frames into
    /// [`FunctionProgramEntry::captures_exhaustive`].
    ///
    /// The transitive captures themselves are not discovery data: sealing
    /// derives them once for the whole file from the resolved
    /// [`Self::references`], [`Self::writes`] and [`Self::lexical_parent`]
    /// links ([`FunctionProgramEntry::captures`]).
    pub captures_exhaustive: bool,
    /// The whole-function stable hash (structural content only — the
    /// parser / language / parse-env identity folds in at the artifact
    /// boundary).
    pub flow_body_stable_hash: Hash16,
    /// The EXACT byte hash of the function's own source text.
    ///
    /// [`Self::flow_body_stable_hash`] is an AST fold that
    /// alpha-normalizes binding and reference identifiers and sees no
    /// whitespace, which is exactly what makes it a good SHARING key —
    /// and exactly what makes it unusable on its own as the key of an
    /// artifact carrying SOURCE POSITIONS. Two contents that fold alike
    /// (`const aa = 1` vs `const aaaa = 1`) place every position inside
    /// the body differently, including positions measured relative to
    /// the function's own start.
    ///
    /// This is the axis that makes such an artifact genuinely
    /// content-addressed. It is deliberately per-FUNCTION rather than
    /// per-file: an edit to a sibling function changes neither this hash
    /// nor any anchor-relative position, so the untouched function's
    /// own-byte identities stay equal. Graph artifact reuse also requires
    /// the exact serving parse identity, which pins lexical capture context.
    ///
    /// `None` when the recorded function span does not lie within the
    /// source that produced this entry — a typed MISS, not a hash. It was
    /// a `unwrap_or_default()` over an out-of-range slice, which hashed
    /// the EMPTY string: every entry whose span fell out of range then
    /// shared one constant, collapsing exactly the axis this field exists
    /// to be. A consumer that cannot address the body's own bytes must
    /// not build a content-addressed key at all.
    pub flow_body_exact_hash: Option<Hash16>,
}

/// One SERVED function position: a sealed [`FunctionProgramDiscovery`].
///
/// The fields are private and there are no setters, so an entry is
/// sealed from discovery data only by
/// [`FunctionProgramIndex::from_discovery`] and never changes after it
/// is sealed: no code outside this module can assemble one with a struct
/// literal or rewrite a field of a served one. The one in-module
/// derivation, [`FunctionProgramIndex::map_stable_hashes`], builds a
/// refolded COPY whose entries form a new index — a witness of the
/// original is not served by it. Each accessor documents
/// the matching [`FunctionProgramDiscovery`] field.
///
/// Sealing guarantees STRUCTURE, not provenance: `from_discovery` is
/// public, so any caller can seal discovery data of its own. What binds an
/// entry to an actual served source is the [`FunctionProgramMatch`]
/// witness, which records the index that answered the lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionProgramEntry {
    key: FunctionProgramKey,
    span: verter_span::Span,
    body_span: verter_span::Span,
    locator: FunctionBodyLocator,
    params: Arc<[FunctionParamRecord]>,
    bindings: Arc<[FunctionBindingRecord]>,
    unmodeled_bindings: Arc<[FunctionBindingRecord]>,
    references: Arc<[FunctionReferenceRecord]>,
    source_type_queries: Arc<[FunctionSourceTypeQuery]>,
    type_queries: Arc<[FunctionTypeQuery]>,
    return_sites: Arc<[FunctionReturnSite]>,
    writes: Arc<[FunctionWriteRecord]>,
    descendant_writes: Arc<[FlowBindingIdentity]>,
    descendant_assignments: Arc<[FlowBindingIdentity]>,
    unserved_assignments: Arc<[FunctionReferenceRecord]>,
    parameter_callable_references: ParameterCallableReferences,
    parameter_callable_captures: Arc<[FunctionParameterCallableCaptures]>,
    effects: Arc<[FunctionEffectRecord]>,
    call_sites: Arc<[FunctionCallSiteRecord]>,
    control: Arc<[FunctionControlRegion]>,
    direct_calls: Arc<[FunctionDirectCall]>,
    type_parameters: Arc<[FunctionProgramTypeParam]>,
    lexical_parent: Option<Box<FunctionProgramKey>>,
    class_member: bool,
    nested_declaration_name: Option<Arc<str>>,
    /// This position's place in the file's shared capture summary.
    captures: FrameCaptureHandle,
    flow_body_stable_hash: Hash16,
    flow_body_exact_hash: Option<Hash16>,
}

impl FunctionProgramEntry {
    /// Test-support copy of this entry as a discovery record, for tests
    /// that reseal a deliberately altered entry into an index of its own.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn unsealed_for_test(&self) -> FunctionProgramDiscovery {
        let entry = self.clone();
        FunctionProgramDiscovery {
            key: entry.key,
            span: entry.span,
            body_span: entry.body_span,
            locator: entry.locator,
            params: entry.params,
            bindings: entry.bindings,
            unmodeled_bindings: entry.unmodeled_bindings,
            references: entry.references,
            source_type_queries: entry.source_type_queries,
            type_queries: entry.type_queries,
            return_sites: entry.return_sites,
            writes: entry.writes,
            descendant_writes: entry.descendant_writes,
            descendant_assignments: entry.descendant_assignments,
            unserved_assignments: entry.unserved_assignments,
            parameter_callable_references: entry.parameter_callable_references,
            parameter_callable_captures: entry.parameter_callable_captures,
            effects: entry.effects,
            call_sites: entry.call_sites,
            control: entry.control,
            direct_calls: entry.direct_calls,
            type_parameters: entry.type_parameters,
            lexical_parent: entry.lexical_parent,
            class_member: entry.class_member,
            nested_declaration_name: entry.nested_declaration_name,
            captures_exhaustive: entry.captures.own_exhaustive(),
            flow_body_stable_hash: entry.flow_body_stable_hash,
            flow_body_exact_hash: entry.flow_body_exact_hash,
        }
    }

    /// Seal one discovery record at its place in the file's capture
    /// summary. Every field MOVES: no payload is cloned.
    fn seal(discovery: FunctionProgramDiscovery, captures: FrameCaptureHandle) -> Self {
        let FunctionProgramDiscovery {
            key,
            span,
            body_span,
            locator,
            params,
            bindings,
            unmodeled_bindings,
            references,
            source_type_queries,
            type_queries,
            return_sites,
            writes,
            descendant_writes,
            descendant_assignments,
            unserved_assignments,
            parameter_callable_references,
            parameter_callable_captures,
            effects,
            call_sites,
            control,
            direct_calls,
            type_parameters,
            lexical_parent,
            class_member,
            nested_declaration_name,
            captures_exhaustive: _,
            flow_body_stable_hash,
            flow_body_exact_hash,
        } = discovery;
        Self {
            key,
            span,
            body_span,
            locator,
            params,
            bindings,
            unmodeled_bindings,
            references,
            source_type_queries,
            type_queries,
            return_sites,
            writes,
            descendant_writes,
            descendant_assignments,
            unserved_assignments,
            parameter_callable_references,
            parameter_callable_captures,
            effects,
            call_sites,
            control,
            direct_calls,
            type_parameters,
            lexical_parent,
            class_member,
            nested_declaration_name,
            captures,
            flow_body_stable_hash,
            flow_body_exact_hash,
        }
    }

    /// See [`FunctionProgramDiscovery::key`].
    #[must_use]
    pub fn key(&self) -> &FunctionProgramKey {
        &self.key
    }

    /// See [`FunctionProgramDiscovery::span`].
    #[must_use]
    pub fn span(&self) -> verter_span::Span {
        self.span
    }

    /// See [`FunctionProgramDiscovery::body_span`].
    #[must_use]
    pub fn body_span(&self) -> verter_span::Span {
        self.body_span
    }

    /// See [`FunctionProgramDiscovery::locator`].
    #[must_use]
    pub fn locator(&self) -> &FunctionBodyLocator {
        &self.locator
    }

    /// See [`FunctionProgramDiscovery::params`].
    #[must_use]
    pub fn params(&self) -> &Arc<[FunctionParamRecord]> {
        &self.params
    }

    /// See [`FunctionProgramDiscovery::bindings`].
    #[must_use]
    pub fn bindings(&self) -> &Arc<[FunctionBindingRecord]> {
        &self.bindings
    }

    /// See [`FunctionProgramDiscovery::unmodeled_bindings`].
    #[must_use]
    pub fn unmodeled_bindings(&self) -> &Arc<[FunctionBindingRecord]> {
        &self.unmodeled_bindings
    }

    /// See [`FunctionProgramDiscovery::references`].
    #[must_use]
    pub fn references(&self) -> &Arc<[FunctionReferenceRecord]> {
        &self.references
    }

    /// See [`FunctionProgramDiscovery::source_type_queries`].
    #[must_use]
    pub fn source_type_queries(&self) -> &Arc<[FunctionSourceTypeQuery]> {
        &self.source_type_queries
    }

    /// See [`FunctionProgramDiscovery::type_queries`].
    #[must_use]
    pub fn type_queries(&self) -> &Arc<[FunctionTypeQuery]> {
        &self.type_queries
    }

    /// See [`FunctionProgramDiscovery::return_sites`].
    #[must_use]
    pub fn return_sites(&self) -> &Arc<[FunctionReturnSite]> {
        &self.return_sites
    }

    /// See [`FunctionProgramDiscovery::writes`].
    #[must_use]
    pub fn writes(&self) -> &Arc<[FunctionWriteRecord]> {
        &self.writes
    }

    /// See [`FunctionProgramDiscovery::descendant_writes`].
    #[must_use]
    pub fn descendant_writes(&self) -> &Arc<[FlowBindingIdentity]> {
        &self.descendant_writes
    }

    /// See [`FunctionProgramDiscovery::descendant_assignments`].
    #[must_use]
    pub fn descendant_assignments(&self) -> &Arc<[FlowBindingIdentity]> {
        &self.descendant_assignments
    }

    /// See [`FunctionProgramDiscovery::unserved_assignments`].
    #[must_use]
    pub fn unserved_assignments(&self) -> &Arc<[FunctionReferenceRecord]> {
        &self.unserved_assignments
    }

    /// Own and transitively nested captured reads, excluding this frame's
    /// locals: each binding and static path once, in source order.
    #[must_use]
    pub fn captured_reads(&self) -> CapturedReads<'_> {
        self.captures.view().reads()
    }

    /// The functions nested directly in this one — their creation sites —
    /// with their captures and captured reads.
    #[must_use]
    pub fn nested_captures(&self) -> NestedCaptures<'_> {
        self.captures.view().nested()
    }

    /// See [`FunctionProgramDiscovery::parameter_callable_references`].
    #[must_use]
    pub fn parameter_callable_references(&self) -> &ParameterCallableReferences {
        &self.parameter_callable_references
    }

    /// See [`FunctionProgramDiscovery::parameter_callable_captures`].
    #[must_use]
    pub fn parameter_callable_captures(&self) -> &Arc<[FunctionParameterCallableCaptures]> {
        &self.parameter_callable_captures
    }

    /// See [`FunctionProgramDiscovery::effects`].
    #[must_use]
    pub fn effects(&self) -> &Arc<[FunctionEffectRecord]> {
        &self.effects
    }

    /// See [`FunctionProgramDiscovery::call_sites`].
    #[must_use]
    pub fn call_sites(&self) -> &Arc<[FunctionCallSiteRecord]> {
        &self.call_sites
    }

    /// See [`FunctionProgramDiscovery::control`].
    #[must_use]
    pub fn control(&self) -> &Arc<[FunctionControlRegion]> {
        &self.control
    }

    /// See [`FunctionProgramDiscovery::direct_calls`].
    #[must_use]
    pub fn direct_calls(&self) -> &Arc<[FunctionDirectCall]> {
        &self.direct_calls
    }

    /// See [`FunctionProgramDiscovery::type_parameters`].
    #[must_use]
    pub fn type_parameters(&self) -> &Arc<[FunctionProgramTypeParam]> {
        &self.type_parameters
    }

    /// See [`FunctionProgramDiscovery::lexical_parent`].
    #[must_use]
    pub fn lexical_parent(&self) -> Option<&FunctionProgramKey> {
        self.lexical_parent.as_deref()
    }

    /// See [`FunctionProgramDiscovery::class_member`].
    #[must_use]
    pub fn class_member(&self) -> bool {
        self.class_member
    }

    /// See [`FunctionProgramDiscovery::nested_declaration_name`].
    #[must_use]
    pub fn nested_declaration_name(&self) -> Option<&Arc<str>> {
        self.nested_declaration_name.as_ref()
    }

    /// The content-free capture environment: every binding this function
    /// or a function nested in it references from an enclosing frame,
    /// write-only ones included, each once in first-reference source
    /// order (a nested function's at its start). Empty for a top-level
    /// position.
    #[must_use]
    pub fn captures(&self) -> CaptureBindings<'_> {
        self.captures.view().bindings()
    }

    /// Whether [`Self::captures`] is EXHAUSTIVE. `false` when this frame,
    /// or any callable nested in it, creates a callable no entry serves
    /// ([`FunctionProgramDiscovery::captures_exhaustive`]): `captures` is
    /// then only a lower bound.
    #[must_use]
    pub fn captures_exhaustive(&self) -> bool {
        self.captures.view().exhaustive()
    }

    /// The physical records of the capture summary this entry's file
    /// shares, for measurement.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    #[must_use]
    pub fn capture_summary_counts(&self) -> CaptureSummaryCounts {
        self.captures.occupancy()
    }

    /// Whether `other` reads the same capture summary allocation.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    #[must_use]
    pub fn shares_capture_summary(&self, other: &Self) -> bool {
        self.captures.summary_ptr() == other.captures.summary_ptr()
    }

    /// See [`FunctionProgramDiscovery::flow_body_stable_hash`].
    #[must_use]
    pub fn flow_body_stable_hash(&self) -> Hash16 {
        self.flow_body_stable_hash
    }

    /// See [`FunctionProgramDiscovery::flow_body_exact_hash`].
    #[must_use]
    pub fn flow_body_exact_hash(&self) -> Option<Hash16> {
        self.flow_body_exact_hash
    }
}

/// A LOOKUP-PROVEN entry: what ONE index answered when asked for one
/// specific function position.
///
/// The fields are private and there is no public constructor, so the type
/// IS the witness: it is obtained only through a keyed lookup on a
/// [`FunctionProgramIndex`] ([`FunctionProgramIndex::get`],
/// [`FunctionProgramIndex::value_function`],
/// [`FunctionProgramIndex::nested_at`],
/// [`FunctionProgramIndex::matches_named`]), and it names both the
/// position and the index that answered.
///
/// What it guarantees, exactly:
///
/// - **Keyed lookup.** No entry leaves an index except through a lookup
///   that names its position. The defeat this closes was an index MISS
///   falling back to `index.entries.first()` — a real entry, for the
///   wrong function, handed to a reader that trusted the reference as
///   proof of a successful lookup.
/// - **Structural sealing.** The entry is a sealed
///   [`FunctionProgramEntry`]: no struct literal, no field write, and
///   every lookup table of the answering index is derived from the same
///   sealed inventory by [`FunctionProgramIndex::from_discovery`].
/// - **Inventory binding.** [`Self::is_served_by`] tells a consumer, in
///   constant time, whether the witness came from a given index's
///   inventory (or a clone sharing it). `from_discovery` is public, so
///   any caller can build an index of its own and obtain witnesses from
///   it; a source that serves content for a witness checks it against
///   the inventory it actually serves and refuses a foreign one.
///
/// It does NOT guarantee parser provenance (sealing accepts any discovery
/// data) or snapshot freshness (a source still re-checks the retained
/// parse it lowers from against the entry it is handed).
#[derive(Clone, Copy)]
pub struct FunctionProgramMatch<'a> {
    index: &'a FunctionProgramIndex,
    ordinal: usize,
}

impl std::fmt::Debug for FunctionProgramMatch<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionProgramMatch")
            .field("ordinal", &self.ordinal)
            .field("key", self.key())
            .finish_non_exhaustive()
    }
}

impl<'a> FunctionProgramMatch<'a> {
    /// The matched entry's structural record.
    #[must_use]
    pub fn entry(self) -> &'a FunctionProgramEntry {
        &self.index.entries[self.ordinal]
    }

    /// The position this lookup matched — the entry's own identity, so a
    /// caller can cross-check what it asked for against what it got.
    #[must_use]
    pub fn key(self) -> &'a FunctionProgramKey {
        &self.entry().key
    }

    /// Whether this witness was answered by `index`'s inventory: the
    /// same sealed entry allocation, which every clone of an index shares
    /// and no separately built index does. Constant time.
    #[must_use]
    pub fn is_served_by(self, index: &FunctionProgramIndex) -> bool {
        Arc::ptr_eq(&self.index.entries, &index.entries)
    }
}

/// The per-file function program index.
///
/// `entries` is PRIVATE and there is no positional accessor: every way
/// out of this index is a lookup that NAMES the position it wants
/// ([`Self::get`], [`Self::value_function`], [`Self::matches_named`]),
/// and each returns a [`FunctionProgramMatch`]. `index.entries.first()`
/// — the shape a callee-lookup miss actually fell back to — does not
/// exist to be written, and neither does any spelling that reaches an
/// entry without naming its function first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunctionProgramIndex {
    /// Every served function position, in source order.
    entries: Arc<[FunctionProgramEntry]>,
    by_key: Arc<rustc_hash::FxHashMap<FunctionProgramKey, usize>>,
    value_functions: Arc<ValueFunctionLookup>,
    nested: Arc<rustc_hash::FxHashMap<(FunctionProgramKey, verter_span::Span), usize>>,
    /// Indexed declaration/callback expressions, in source order.
    expressions: Arc<[ProgramExpressionRecord]>,
    /// Each indexed expression's program point → its position in
    /// `expressions`; where two records share a point the first wins.
    expressions_by_point: Arc<rustc_hash::FxHashMap<ProgramExpressionIdentity, usize>>,
    /// Every class the file authors: syntactic data recorded by the same
    /// build, prepared into keyed lookups, owned by this index and
    /// released with it.
    classes: ClassIndex,
}

impl FunctionProgramIndex {
    /// What this file's capture summary holds: the one summary every entry
    /// shares, counted once. An index serving no function holds none.
    #[must_use]
    pub fn capture_summary_occupancy(&self) -> CaptureSummaryCounts {
        self.entries
            .first()
            .map(|entry| entry.captures.occupancy())
            .unwrap_or_default()
    }

    /// An identity of this file's capture summary, equal for two indexes
    /// sharing it while either is alive, so a reader summing the summaries
    /// it retains counts each once. `None` when the index serves no
    /// function.
    #[must_use]
    pub fn capture_summary_identity(&self) -> Option<usize> {
        self.entries
            .first()
            .map(|entry| entry.captures.summary_ptr() as usize)
    }

    /// Test-support read of `entries`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn entries_for_test(&self) -> &Arc<[FunctionProgramEntry]> {
        &self.entries
    }
}

impl FunctionProgramIndex {
    /// Seal one file's discovered inventory: the entries in source order,
    /// the indexed expressions (sorted by start) and the authored classes
    /// with their resolved bases ([`ClassIndex::from_discovery`]).
    /// The file's transitive closure captures freeze here once into one
    /// summary every entry shares ([`FunctionProgramEntry::captures`]).
    /// The keyed lookups are derived here from the entries, so every way out
    /// of the index stays consistent with what discovery recorded.
    ///
    /// This is the only way to seal discovery data into a
    /// [`FunctionProgramEntry`] (the in-module `map_stable_hashes` only
    /// derives a refolded copy of already-sealed entries, as a new index):
    /// each discovery record is sealed by moving its fields, in source order.
    /// Where two records share a lookup key the FIRST in source order wins.
    /// Sealing proves structure and lookup consistency, not parser
    /// provenance — any caller may seal discovery data of its own, which is
    /// why a serving source checks [`FunctionProgramMatch::is_served_by`].
    pub fn from_discovery(
        entries: Vec<FunctionProgramDiscovery>,
        expressions: Vec<ProgramExpressionRecord>,
        classes: Vec<ClassSyntaxDiscovery>,
    ) -> Self {
        let mut by_key = rustc_hash::FxHashMap::default();
        let mut value_functions = ValueFunctionLookup::default();
        let mut nested = rustc_hash::FxHashMap::default();
        let summary = Arc::new(CaptureSummaries::freeze(&entries));
        let entries: Vec<FunctionProgramEntry> = entries
            .into_iter()
            .enumerate()
            .map(|(ordinal, discovery)| {
                FunctionProgramEntry::seal(discovery, FrameCaptureHandle::new(&summary, ordinal))
            })
            .collect();
        for (ordinal, entry) in entries.iter().enumerate() {
            by_key.entry(entry.key.clone()).or_insert(ordinal);
            if entry.key.declaration.space == SymbolSpace::Value {
                value_functions
                    .entry(Arc::clone(&entry.key.declaration.name))
                    .or_default()
                    .entry((
                        entry.key.declaration.owner,
                        entry.key.part.clone(),
                        entry.key.overload_ordinal,
                    ))
                    .or_insert(ordinal);
            }
            if let Some(parent) = &entry.lexical_parent {
                nested
                    .entry((parent.as_ref().clone(), entry.span))
                    .or_insert(ordinal);
            }
        }
        let mut expressions_by_point = rustc_hash::FxHashMap::with_capacity_and_hasher(
            expressions.len(),
            rustc_hash::FxBuildHasher,
        );
        for (ordinal, record) in expressions.iter().enumerate() {
            expressions_by_point
                .entry(record.point.clone())
                .or_insert(ordinal);
        }
        FunctionProgramIndex {
            by_key: Arc::new(by_key),
            value_functions: Arc::new(value_functions),
            nested: Arc::new(nested),
            entries: Arc::from(entries.into_boxed_slice()),
            expressions: Arc::from(expressions.into_boxed_slice()),
            expressions_by_point: Arc::new(expressions_by_point),
            classes: ClassIndex::from_discovery(classes),
        }
    }
}

pub type ValueFunctionLookup = rustc_hash::FxHashMap<
    Arc<str>,
    rustc_hash::FxHashMap<(verter_type_expr::TopLevelOwnerId, FunctionPartIdentity, u32), usize>,
>;

#[cfg(any(test, feature = "test-support"))]
std::thread_local! { pub static FUNCTION_KEY_LOOKUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(any(test, feature = "test-support"))]
std::thread_local! { pub static FUNCTION_VALUE_LOOKUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
std::thread_local! {
    /// Indexed-expression records examined by
    /// [`FunctionProgramIndex::expression`] on this thread: one per lookup,
    /// whatever the file's expression count.
    pub static PROGRAM_EXPRESSION_LOOKUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl FunctionProgramIndex {
    /// Locate one exact child position in the retained file inventory.
    pub fn nested_at(
        &self,
        parent: &FunctionProgramKey,
        span: verter_span::Span,
    ) -> Option<FunctionProgramMatch<'_>> {
        let ordinal = *self.nested.get(&(parent.clone(), span))?;
        Some(FunctionProgramMatch {
            index: self,
            ordinal,
        })
    }
    /// The entry for `key`, when the position is served by this file.
    #[must_use]
    pub fn get(&self, key: &FunctionProgramKey) -> Option<FunctionProgramMatch<'_>> {
        #[cfg(any(test, feature = "test-support"))]
        FUNCTION_KEY_LOOKUP_VISITS.with(|visits| visits.set(visits.get() + 1));
        let ordinal = *self.by_key.get(key)?;
        Some(FunctionProgramMatch {
            index: self,
            ordinal,
        })
    }

    /// The entry for a value-space function declaration / initializer of
    /// `name` at `overload_ordinal`, when present.
    #[must_use]
    pub fn value_function(
        &self,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        part: &FunctionPartIdentity,
        overload_ordinal: u32,
    ) -> Option<FunctionProgramMatch<'_>> {
        let by_name = self.value_functions.get(name)?;
        #[cfg(any(test, feature = "test-support"))]
        FUNCTION_VALUE_LOOKUP_VISITS.with(|visits| visits.set(visits.get() + 1));
        let ordinal = *by_name.get(&(owner, part.clone(), overload_ordinal))?;
        Some(FunctionProgramMatch {
            index: self,
            ordinal,
        })
    }

    /// Every served position DECLARED under `name`, in source order —
    /// the keyed lookup for a declaration whose part / overload ordinal
    /// the caller does not know up front (a class's members, an overload
    /// group's contributors).
    pub fn matches_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = FunctionProgramMatch<'a>> + 'a {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, entry)| entry.key.declaration.name.as_ref() == name)
            .map(|(ordinal, _)| FunctionProgramMatch {
                index: self,
                ordinal,
            })
    }

    /// How many function positions this file serves.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this file serves no function position.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// This index with every entry's `flow_body_stable_hash` re-folded
    /// through `mix`.
    ///
    /// The artifact boundary mixes the parser / language / parse-env
    /// identity into the semantic walk's body-content hash. It is
    /// expressed as a fold HERE rather than as a rebuild at the consumer
    /// because rebuilding needs to construct entries, and constructing an
    /// entry outside this module is exactly what must stay impossible.
    #[must_use]
    pub fn map_stable_hashes(&self, mix: impl Fn(&Hash16) -> Hash16) -> Self {
        Self {
            entries: Arc::from(
                self.entries
                    .iter()
                    .map(|entry| {
                        let mut folded = entry.clone();
                        folded.flow_body_stable_hash = mix(&entry.flow_body_stable_hash);
                        folded
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            expressions: Arc::clone(&self.expressions),
            expressions_by_point: Arc::clone(&self.expressions_by_point),
            by_key: Arc::clone(&self.by_key),
            value_functions: Arc::clone(&self.value_functions),
            nested: Arc::clone(&self.nested),
            classes: self.classes.clone(),
        }
    }

    /// The file's class index.
    #[must_use]
    pub fn classes(&self) -> &ClassIndex {
        &self.classes
    }

    /// Indexed expression at the exact content-free program point.
    #[must_use]
    pub fn expression(
        &self,
        point: &ProgramExpressionIdentity,
    ) -> Option<&ProgramExpressionRecord> {
        self.expressions_by_point.get(point).map(|&ordinal| {
            // One visit per record examined: the index yields only the
            // record at this point.
            #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
            PROGRAM_EXPRESSION_LOOKUP_VISITS.with(|visits| visits.set(visits.get() + 1));
            &self.expressions[ordinal]
        })
    }
}
