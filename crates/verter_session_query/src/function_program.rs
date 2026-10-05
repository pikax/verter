//! The per-file function program index: function positions, their structural records and
//! the keyed lookups that hand them out. Built by the parser front-end.

use crate::analysis::types::Hash16;
use crate::facts::SymbolSpace;
use std::sync::Arc;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::facts::{FunctionReturnSource, ProgramExpressionIdentity};
use verter_type_expr::span_origins::DeclContributorAnchor;

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

/// Exact closure subjects and read dependencies of one immediately nested callable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionNestedCaptures {
    pub function: FunctionProgramKey,
    pub span: verter_span::Span,
    pub bindings: CanonicalCaptureIdentity,
    pub reads: Arc<[FunctionCapturedRead]>,
    /// The child's [`FunctionProgramEntry::captures_exhaustive`].
    pub exhaustive: bool,
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

/// One served function position: identity, body locator, structural
/// inventory, and the whole-function stable hash.
///
/// An entry is a STATEMENT about an authored function the parser
/// front-end's discovery walk found, and the flow substrate's callee rail
/// reads a clause off exactly this record. The discovery walk lives in the
/// front-end crate, so this record cannot be `#[non_exhaustive]`; a
/// consumer reaches an entry only through a keyed lookup on
/// [`FunctionProgramIndex`], which hands out the [`FunctionProgramMatch`]
/// witness for the position it named. Fields are public: the value is a
/// shallow structural fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionProgramEntry {
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
    /// Own and transitively nested captured reads, excluding this frame's locals.
    pub captured_reads: Arc<[FunctionCapturedRead]>,
    /// Immediate child creation sites and their retained read-path dependencies.
    pub nested_captures: Arc<[FunctionNestedCaptures]>,
    /// The references each parameter-list callable makes to names it does
    /// not declare, by the callable's span: resolved with the frame's own
    /// references into [`Self::parameter_callable_captures`].
    pub parameter_callable_references: Arc<[(verter_span::Span, Arc<[FunctionReferenceRecord]>)]>,
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
    /// The content-free capture environment (empty for a top-level
    /// position).
    pub captures: CanonicalCaptureIdentity,
    /// Whether [`Self::captures`] is EXHAUSTIVE. `false` when this frame,
    /// or any callable nested in it, creates a callable no entry serves —
    /// a class (its constructor, member bodies and field initializers) or
    /// a callable in a parameter list. A cell retained there is named by
    /// no record, so `captures` is then only a lower bound.
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

/// A LOOKUP-PROVEN entry: what THIS index answered when asked for one
/// specific function position.
///
/// The field is private and there is no public constructor, so the type
/// IS the witness: it cannot be forged, and it cannot be manufactured
/// from an entry obtained any other way — including a legitimately
/// obtained entry belonging to a DIFFERENT callee.
///
/// That second case is the one that mattered. While the flow rail's
/// clause reader took a bare `&FunctionProgramEntry`, two defeats
/// compiled. The first was a struct literal assembled out of nothing
/// (now separately impossible: [`FunctionProgramEntry`] is
/// `#[non_exhaustive]`). The second, and the realistic one, was an index
/// MISS falling back to `index.entries.first()` — a real entry, for the
/// wrong function, handed to a reader whose doc claimed the reference
/// itself was proof of a successful lookup. Both are closed by
/// construction now: this index hands out no entry except through a
/// KEYED lookup, and what it hands out is this witness.
#[derive(Debug, Clone, Copy)]
pub struct FunctionProgramMatch<'a> {
    entry: &'a FunctionProgramEntry,
}

impl<'a> FunctionProgramMatch<'a> {
    /// The matched entry's structural record.
    #[must_use]
    pub fn entry(self) -> &'a FunctionProgramEntry {
        self.entry
    }

    /// The position this lookup matched — the entry's own identity, so a
    /// caller can cross-check what it asked for against what it got.
    #[must_use]
    pub fn key(self) -> &'a FunctionProgramKey {
        &self.entry.key
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
    /// Every class the file authors, in source order: syntactic data
    /// recorded by the same build, owned by this index and released with
    /// it.
    classes: Arc<[ClassSyntaxRecord]>,
}

impl FunctionProgramIndex {
    /// Test-support read of `entries`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn entries_for_test(&self) -> &Arc<[FunctionProgramEntry]> {
        &self.entries
    }
}

impl FunctionProgramIndex {
    /// Seal one file's discovered inventory: the entries in source order,
    /// the indexed expressions (sorted by start) and the authored classes.
    /// The keyed lookups are derived here from the entries, so every way out
    /// of the index stays consistent with what discovery recorded.
    pub fn from_discovery(
        entries: Vec<FunctionProgramEntry>,
        expressions: Vec<ProgramExpressionRecord>,
        classes: Vec<ClassSyntaxRecord>,
    ) -> Self {
        let mut by_key = rustc_hash::FxHashMap::default();
        let mut value_functions = ValueFunctionLookup::default();
        let mut nested = rustc_hash::FxHashMap::default();
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
        FunctionProgramIndex {
            by_key: Arc::new(by_key),
            value_functions: Arc::new(value_functions),
            nested: Arc::new(nested),
            entries: Arc::from(entries.into_boxed_slice()),
            expressions: Arc::from(expressions.into_boxed_slice()),
            classes: Arc::from(classes.into_boxed_slice()),
        }
    }
}

/// One class the file authors — a declaration at any depth or a class
/// expression — recorded syntactically at index time. A member's
/// declaring class is the class whose body declares it directly, so a
/// class records the span of each member it declares: each class element,
/// and each constructor parameter that declares a property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassSyntaxRecord {
    /// The class node's span.
    pub span: verter_span::Span,
    /// Whether the class is an expression rather than a declaration.
    pub expression: bool,
    /// Whether the class has an `extends` clause.
    pub has_heritage: bool,
    /// The span of each member the class declares directly, in source
    /// order.
    pub members: Arc<[verter_span::Span]>,
}

pub type ValueFunctionLookup = rustc_hash::FxHashMap<
    Arc<str>,
    rustc_hash::FxHashMap<(verter_type_expr::TopLevelOwnerId, FunctionPartIdentity, u32), usize>,
>;

#[cfg(any(test, feature = "test-support"))]
std::thread_local! { pub static FUNCTION_KEY_LOOKUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(any(test, feature = "test-support"))]
std::thread_local! { pub static FUNCTION_VALUE_LOOKUP_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

impl FunctionProgramIndex {
    /// Locate one exact child position in the retained file inventory.
    pub fn nested_at(
        &self,
        parent: &FunctionProgramKey,
        span: verter_span::Span,
    ) -> Option<FunctionProgramMatch<'_>> {
        let ordinal = *self.nested.get(&(parent.clone(), span))?;
        Some(FunctionProgramMatch {
            entry: &self.entries[ordinal],
        })
    }
    /// The entry for `key`, when the position is served by this file.
    #[must_use]
    pub fn get(&self, key: &FunctionProgramKey) -> Option<FunctionProgramMatch<'_>> {
        #[cfg(any(test, feature = "test-support"))]
        FUNCTION_KEY_LOOKUP_VISITS.with(|visits| visits.set(visits.get() + 1));
        let ordinal = *self.by_key.get(key)?;
        Some(FunctionProgramMatch {
            entry: &self.entries[ordinal],
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
            entry: &self.entries[ordinal],
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
            .filter(move |entry| entry.key.declaration.name.as_ref() == name)
            .map(|entry| FunctionProgramMatch { entry })
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
            by_key: Arc::clone(&self.by_key),
            value_functions: Arc::clone(&self.value_functions),
            nested: Arc::clone(&self.nested),
            classes: Arc::clone(&self.classes),
        }
    }

    /// The class whose body declares a member directly at `declaration`,
    /// the member's declaration span: a class element's span, or a
    /// property-declaring constructor parameter's.
    #[must_use]
    pub fn class_declaring_member(
        &self,
        declaration: verter_span::Span,
    ) -> Option<&ClassSyntaxRecord> {
        self.classes
            .iter()
            .find(|class| class.members.contains(&declaration))
    }

    /// Whether another class of the file encloses the class at `span`.
    #[must_use]
    pub fn class_encloses(&self, span: verter_span::Span) -> bool {
        self.classes.iter().any(|class| {
            class.span != span && class.span.start <= span.start && span.end <= class.span.end
        })
    }

    /// Indexed expression at the exact content-free program point.
    #[must_use]
    pub fn expression(
        &self,
        point: &ProgramExpressionIdentity,
    ) -> Option<&ProgramExpressionRecord> {
        self.expressions
            .iter()
            .find(|record| &record.point == point)
    }
}
