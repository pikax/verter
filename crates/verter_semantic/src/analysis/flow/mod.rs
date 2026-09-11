//! Demand-sliced flow substrate: the arena-free [`FunctionBodySkeleton`] and
//! the per-function [`flow_graph::FunctionFlowGraph`] built from it.
//!
//! The skeleton is a SHALLOW structural index over one authored function
//! body: a statement / control-region skeleton, a return-site index, a
//! lexical binding index, and an assignment / kill summary, plus per-site
//! read / write / call footprints and object-literal property footprints.
//! It is built once per function content version from the retained parse
//! snapshot and never rebuilt per query or demand; it borrows no OXC node
//! (`Send + Sync + 'static`) and performs NO type lowering — every stored
//! leaf is an interned name, ordinal, span, or id, certified transitively
//! `TypeExpr`-free by the `NoTypeExpr` marker on every carrier.
//!
//! The [`flow_graph::FunctionFlowGraph`] is the sparse typed-edge dependence
//! structure the flow demand planner computes reachability over; it is built
//! from sealed indexed structure ([`flow_graph::build_function_flow_graph`] takes
//! only `&PreparedFunctionBodySkeleton`), so a graph build never resolves names or re-walks the AST
//! or observe a query demand.
//!
//! On top of the graph: [`peeker::ReturnPathPeeker`] plans a demand slice
//! as graph reachability (the two-frontier rule as edge classes) into a
//! [`flow_ir::ReturnSlicePlan`]; [`hashing::compute_flow_slice_hash`]
//! folds exactly that selected subgraph into the opaque
//! [`hashing::FlowSliceHash`]; and [`lower::lower_slice_plan`] lowers only
//! the plan into the arena-free [`flow_ir::FlowSliceIR`].

use std::sync::Arc;

use crate::analysis::function_program::{
    FlowBindingIdentity, FunctionNestedCaptures, FunctionProgramEntry,
};
use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentTarget, AssignmentTargetMaybeDefault,
    AssignmentTargetProperty, BindingPattern, Expression, Function, ObjectExpression,
    SimpleAssignmentTarget, Statement,
};
use oxc_ast_visit::{walk, Visit};
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_no_typeexpr::NoTypeExpr;

pub use binding::{
    FlowBindingMap, FlowBindingMapError, FlowBindingOccurrence, FlowBindingRef,
    FlowRuntimeBindingShape,
};
pub use frame_span::FrameSpan;

pub mod binding;
pub mod flow_graph;
pub mod flow_ir;
pub mod frame_span;
pub mod hashing;
pub mod lower;
pub mod peeker;
pub mod value_descent;

pub use value_descent::{
    expression_contains_call, object_entry_descent, object_entry_key, static_property_key_text,
    value_composes_unmodeled_call, value_descent, value_is_unmodeled_call, ObjectEntryDescent,
    ObjectEntryKey, ObjectEntryKind, ValueDescent,
};

#[cfg(test)]
#[path = "skeleton_tests.rs"]
mod skeleton_tests;

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

/// Interned identifier / property-key name within one skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowNameId(u32);

impl FlowNameId {
    /// Index into [`FunctionBodySkeleton::names`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One entry of the lexical binding index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonBindingId(u32);

impl SkeletonBindingId {
    /// Index into [`FunctionBodySkeleton::bindings`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One control region of the statement skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonRegionId(u32);

impl SkeletonRegionId {
    /// Index into [`FunctionBodySkeleton::regions`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One tracked expression site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonExprSiteId(u32);

impl SkeletonExprSiteId {
    /// Index into [`FunctionBodySkeleton::expr_sites`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One authored nested callable inside one expression site's closure
/// inventory, in authored order.
///
/// A site is NOT a callback identity: several callables share one
/// expression site whenever the site is a compound the skeleton does not
/// open per-argument or per-element (`f(() => a, () => b)` records both
/// on the CALL's site, `[() => a, () => b]` records both on the array's).
/// This ordinal is what separates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, NoTypeExpr)]
pub struct SkeletonClosureId(u32);

impl SkeletonClosureId {
    /// Index into [`SkeletonExprSite::closures`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The record at site-local ordinal `index`, as enumerated from
    /// [`SkeletonExprSite::closures`].
    #[must_use]
    pub const fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One `return` site of the indexed function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonReturnSiteId(u32);

impl SkeletonReturnSiteId {
    /// Index into [`FunctionBodySkeleton::return_sites`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

// ---------------------------------------------------------------------------
// Regions
// ---------------------------------------------------------------------------

/// The kind of one control region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonRegionKind {
    /// The function body itself — the root region.
    FunctionBody,
    /// A block statement.
    Block,
    /// The consequent arm of an `if`.
    IfConsequent,
    /// The alternate arm of an `if`.
    IfAlternate,
    /// A loop body (`for` / `for-in` / `for-of` / `while` / `do-while`).
    Loop,
    /// A `switch` statement.
    Switch,
    /// One `case` / `default` arm of a `switch`.
    SwitchCase,
    /// The `try` block of a `try` statement.
    TryBlock,
    /// A `catch` clause.
    CatchClause,
    /// A `finally` block.
    FinallyBlock,
    /// The body of a labeled statement.
    LabeledBody,
}

/// One control region: kind, parent nesting, the controlling expression
/// site (an `if` / loop condition or `switch` discriminant), whether the
/// region's statement subtree returns from the indexed function, and the
/// region's span.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonRegion {
    /// The region kind.
    pub kind: SkeletonRegionKind,
    /// The enclosing region (`None` for the function-body root).
    pub parent: Option<SkeletonRegionId>,
    /// The controlling expression site, when the region is predicated.
    pub control_input: Option<SkeletonExprSiteId>,
    /// Whether the region's subtree contains a `return` of the indexed
    /// function (nested function bodies never contribute).
    pub has_return: bool,
    /// The region statement's span.
    pub span: FrameSpan,
}

// ---------------------------------------------------------------------------
// Bindings
// ---------------------------------------------------------------------------

/// The kind of one lexical binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonBindingKind {
    /// A formal parameter (destructured identifiers included).
    Param,
    /// A `const` / `using` / `await using` declarator (block-scoped).
    Const,
    /// A `let` declarator.
    Let,
    /// A `var` declarator — the only function-scoped declarator kind.
    Var,
    /// A nested function declaration's name.
    NestedFunction,
    /// A local class declaration's name.
    Class,
    /// A `catch` clause parameter.
    CatchParam,
    /// A local `enum` declaration's name.
    Enum,
    /// A local `namespace` / `module` declaration's name.
    Namespace,
    /// A local `import x = …` declaration's name.
    ImportEquals,
    /// A local `type X = …` alias declaration's name. TYPE space only.
    TypeAlias,
    /// A local `interface X { … }` declaration's name. TYPE space only.
    Interface,
}

/// The name MEANING one lookup demands, mirroring the TypeScript
/// `SymbolFlags` split that decides which declarations can answer it.
///
/// A BARE reference (`x as N`) demands `Type`; the HEAD of a QUALIFIED
/// reference (`x as N.B`) demands `Namespace`
/// (`SymbolFlags.Namespace = ValueModule | NamespaceModule | Enum` — a
/// class is NOT in it). The two are genuinely different questions about
/// the same name: a local `class N` shadows the bare reference but not
/// the qualified head, and a local `namespace N` does the reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum NameMeaning {
    /// A type reference's own name (`N`).
    Type,
    /// A qualified type reference's HEAD (`N` in `N.B`).
    Namespace,
}

impl SkeletonBindingKind {
    /// Whether this kind declares a VALUE.
    ///
    /// `type` / `interface` declare a type and NOTHING else, so a value
    /// lookup must walk straight past them to the enclosing scope —
    /// exactly as [`FunctionBodySkeleton::declares_meaning_in_scope`]
    /// walks past a value-only kind in type space. The two spaces are
    /// symmetric: whichever space a lookup asks about, a declaration
    /// that does not occupy it is transparent.
    #[must_use]
    pub const fn declares_value(self) -> bool {
        !matches!(self, Self::TypeAlias | Self::Interface)
    }

    /// Whether this kind declares `meaning`.
    ///
    /// Oracle-anchored against `tsc --strict`, one fixture per cell:
    ///
    /// | kind                    | `Type` | `Namespace` |
    /// |-------------------------|--------|-------------|
    /// | `class` (static or not) | yes    | no          |
    /// | `enum` / `const enum`   | yes    | yes         |
    /// | `namespace`             | no     | yes         |
    /// | `type` / `interface`    | yes    | no          |
    /// | `import x = …`          | yes\*  | yes\*       |
    /// | value-only kinds        | no     | no          |
    ///
    /// \* An `import x = …` is MEANING-TRANSPARENT: it occupies exactly
    /// the spaces its target occupies, and the target is not decidable
    /// from the skeleton. It therefore answers both meanings — a
    /// deliberate over-fire that can only FAIL CLOSED, never publish a
    /// wrong answer. (An `import =` inside a function body is TS1232
    /// anyway; the skeleton still records the recovered binding.)
    ///
    /// DECLARATION MERGING needs no special case: `class N` + `namespace
    /// N` are two separate bindings of the same name in the same region,
    /// and the scope walk asks `any`, so the merge answers both meanings
    /// while `function N` + `namespace N` answers only `Namespace`.
    #[must_use]
    pub const fn declares(self, meaning: NameMeaning) -> bool {
        match (self, meaning) {
            (Self::Enum | Self::ImportEquals, _) => true,
            (Self::Class | Self::TypeAlias | Self::Interface, NameMeaning::Type) => true,
            (Self::Namespace, NameMeaning::Namespace) => true,
            (Self::Class | Self::TypeAlias | Self::Interface, NameMeaning::Namespace)
            | (Self::Namespace, NameMeaning::Type) => false,
            (
                Self::Param
                | Self::Const
                | Self::Let
                | Self::Var
                | Self::NestedFunction
                | Self::CatchParam,
                _,
            ) => false,
        }
    }
}

/// The binding kind of one variable declaration. `using` / `await using`
/// are BLOCK-scoped resource declarations (the `const` scoping rule plus
/// disposal), never function-scoped `var`s.
fn skeleton_binding_kind(kind: oxc_ast::ast::VariableDeclarationKind) -> SkeletonBindingKind {
    match kind {
        oxc_ast::ast::VariableDeclarationKind::Const
        | oxc_ast::ast::VariableDeclarationKind::Using
        | oxc_ast::ast::VariableDeclarationKind::AwaitUsing => SkeletonBindingKind::Const,
        oxc_ast::ast::VariableDeclarationKind::Let => SkeletonBindingKind::Let,
        oxc_ast::ast::VariableDeclarationKind::Var => SkeletonBindingKind::Var,
    }
}

/// One entry of the lexical binding index.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonBinding {
    /// The binding name.
    pub name: FlowNameId,
    /// The indexed runtime variable shared by hoisted declaration aliases.
    pub runtime_binding: Option<SkeletonBindingId>,
    /// The binding kind.
    pub kind: SkeletonBindingKind,
    /// The region the binding is declared in.
    pub region: SkeletonRegionId,
    /// The binding identifier's span.
    pub span: FrameSpan,
    /// The declarator initializer / parameter default site, when present.
    pub initializer: Option<SkeletonExprSiteId>,
    /// Authored annotation of a whole-identifier local declarator. Parameters
    /// use signature authority; destructured locals retain their typed boundary.
    pub annotation_span: Option<FrameSpan>,
    /// Whether the identifier is bound by a DESTRUCTURING pattern (an
    /// object / array pattern element) rather than a plain binding
    /// identifier. Consumers that model only whole-slot declarators read
    /// this to tell "a binding I can model" from "a binding I resolved
    /// but cannot model" — the latter must fail closed, never fall
    /// through to an outer same-named declaration.
    pub destructured: bool,
    /// The sites the declaring pattern evaluates while it binds — nested
    /// defaults and computed keys, in evaluation order. Producing the
    /// binding runs every one of them, so each is an evaluation effect of
    /// the binding: a callable authored there is created and retains its
    /// captures whenever the binding is demanded.
    pub pattern_sites: Arc<[SkeletonExprSiteId]>,
}

// ---------------------------------------------------------------------------
// Expression sites
// ---------------------------------------------------------------------------

/// One identifier read inside a site (child sites and nested function
/// bodies excluded — their reads belong to their own site / frame).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonRead {
    /// The read name (a local slot when the name binds in this frame,
    /// otherwise a free / captured name).
    pub name: FlowNameId,
    /// The exact identifier occurrence, rebased to this function.
    pub span: FrameSpan,
    /// The indexed runtime variable; absent only for a free/global reference
    /// or a structural skeleton built without an indexed binding context.
    pub binding: Option<FlowBindingRef>,
    /// The statically known projection path under the root. Empty is a
    /// whole-root read; a computed segment conservatively aliases every key.
    pub path: Arc<[SkeletonPathSegment]>,
    /// Whether the read provides this expression result or is consumed independently.
    pub kind: FlowReadKind,
}

/// A type-query dependency of an indexed call input, independent of runtime
/// operand reads, effects and closure capture receipts.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonSourceTypeQuery {
    pub span: FrameSpan,
    pub binding: Option<FlowBindingRef>,
}

/// How a read depends on a demand for the containing expression result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum FlowReadKind {
    /// The read provides the result; append the demanded suffix to its path.
    Result,
    /// The read is a computation input; retain its own path with no result suffix.
    Input,
}

/// The callee shape of one call / construct site.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonCallee {
    /// A bare identifier callee (`g()`).
    Named(FlowNameId),
    /// A static member path rooted at an identifier (`a.b.c()`), root
    /// first.
    Path(Arc<[FlowNameId]>),
    /// Any other callee shape (computed, call-result, `this`-rooted).
    Opaque,
}

/// One call / construct footprint entry.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonCall {
    /// The callee shape.
    pub callee: SkeletonCallee,
    /// Whether this is a `new` construct site.
    pub new_construct: bool,
    /// The call expression's span.
    pub span: FrameSpan,
    /// Exact callee root occurrence, when the callee has an identifier root.
    pub root_span: Option<FrameSpan>,
    /// Indexed identity of that root; `None` is a known free or opaque callee.
    pub binding: Option<FlowBindingRef>,
}

/// The key of one object-literal property entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonObjectKey {
    /// A statically-known property key.
    Static(FlowNameId),
    /// A computed key; the key expression is its own child site (its
    /// evaluation effects survive independently of the named value).
    Computed(SkeletonExprSiteId),
}

/// The authored kind of one object-literal property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonPropertyKind {
    /// A plain `key: value` initializer.
    Init,
    /// A method shorthand (`m() {}`).
    Method,
    /// A `get` / `set` accessor.
    Accessor,
}

/// One object-literal entry of a site's shape footprint.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonObjectEntry {
    /// A property provisioning one key.
    Property {
        /// The property key.
        key: SkeletonObjectKey,
        /// The property value's child site.
        value: SkeletonExprSiteId,
        /// The authored property kind.
        kind: SkeletonPropertyKind,
    },
    /// A spread entry (`...src`) — an optional / unknown write of every
    /// key, whose source evaluation effect survives a later definite
    /// write.
    Spread {
        /// The spread source's child site.
        source: SkeletonExprSiteId,
    },
}

/// The recorded shape of one expression site.
///
/// The variants other than [`Self::Other`] are exactly the
/// [`ValueDescent`] dispositions that HAVE value-providing children:
/// each one records the child sites the flow graph turns into
/// value-provider edges, so the demand planner reaches every
/// sub-expression the content half lowers.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonExprShape {
    /// An object literal with its property footprint, in authored order.
    ObjectLiteral {
        /// The entries in authored order.
        entries: Arc<[SkeletonObjectEntry]>,
    },
    /// A branch JOIN (a conditional expression): every arm site provides
    /// the WHOLE value of this site, so a demand for this site's value —
    /// or for a projection under it — is a demand for each arm's, at the
    /// same remaining path.
    BranchJoin {
        /// The arm sites, in authored order (consequent, alternate).
        arms: Arc<[SkeletonExprSiteId]>,
    },
    /// Any other expression shape (footprint-only).
    Other,
}

/// Whether the indexed program correlated one authored nested callable.
///
/// The distinction is the whole point of the record: an authored callable
/// with no correlated index record asserts NOTHING about what it captures,
/// and a consumer must fail closed on it rather than read its empty
/// capture list as "captures nothing".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonClosureCorrelation {
    /// The indexed program named this callable, so
    /// [`SkeletonClosure::captures`] is its EXACT and EXHAUSTIVE capture
    /// set. Empty means the callable provably captures nothing — a
    /// positive fact, not an absence of information.
    Exact,
    /// The indexed program named this callable, but its body creates a
    /// callable no index record serves (a class, or a parameter-list
    /// callable), so [`SkeletonClosure::captures`] is only a LOWER BOUND:
    /// every listed capture is real, and more may exist.
    Partial,
    /// The authored callable has no correlated record in the indexed
    /// program (a class, a callable in a parameter default, or any
    /// position the index does not serve), so no capture set can be
    /// asserted for it. [`SkeletonClosure::captures`] is empty and means
    /// nothing.
    Uncorrelated,
}

/// One authored nested callable (arrow, function expression,
/// object-literal method, or class) evaluated at one expression site.
///
/// The site-level [`SkeletonExprSite::capture_bindings`] is the UNION of
/// every callable at the site, deduplicated: it answers "does anything
/// here retain this cell", never "which callback retains it". This record
/// is the per-callback partition of that union, so a consumer can carry
/// one obligation per (callback, captured binding) and can tell a
/// capture-free callback from an absent one.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonClosure {
    /// The callable's own span — its identity within the frame.
    pub span: FrameSpan,
    /// Whether the indexed program correlated this callable.
    pub correlation: SkeletonClosureCorrelation,
    /// This callable's OWN captured bindings (its free variables bound by
    /// an enclosing frame, transitively through its own nested
    /// callables), deduplicated in the indexed source order. Genuinely
    /// free / global reads bind to no declaration and never appear here.
    pub captures: Arc<[FlowBindingRef]>,
    /// The subset of `captures` this callable actually READS — its own
    /// free reads, deduplicated in the indexed source order. The
    /// remaining captures are retained for their cell alone (a
    /// write-only capture), and demand exactly the callable's execution,
    /// never a value product.
    ///
    /// Recorded per callable because the site-level
    /// [`SkeletonExprSite::reads`] merges every callable at the site with
    /// the site's own reads: at `f(() => a, () => { a = 1 })` the merged
    /// footprint says `a` is read HERE and cannot say by WHICH callable,
    /// so a site-level answer makes the write-only callback look like a
    /// value consumer.
    pub read_captures: Arc<[FlowBindingRef]>,
}

/// One tracked expression site: span, region membership, containment
/// parent, shape, and the read / call footprint attributed to this site
/// (child sites carry their own).
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonExprSite {
    /// The expression's span.
    pub span: FrameSpan,
    /// The region the site evaluates in.
    pub region: SkeletonRegionId,
    /// The containing site (`None` for a root site owned by a statement,
    /// declarator, return, or control input).
    pub parent: Option<SkeletonExprSiteId>,
    /// The recorded expression shape.
    pub shape: SkeletonExprShape,
    /// Identifier reads attributed to this site.
    pub reads: Arc<[SkeletonRead]>,
    pub source_type_queries: Arc<[SkeletonSourceTypeQuery]>,
    /// The captured names of the nested function value (arrow / function
    /// expression) this site holds: the nested frame's free reads, already
    /// interned into THIS frame's name table, deduplicated by name. A
    /// subset of `reads`' roots, recorded separately so a consumer can
    /// tell "the closure here captures this enclosing name" from "this
    /// expression itself reads the name". Empty for a non-closure site.
    pub captures: Arc<[FlowNameId]>,
    /// Exact enclosing bindings captured by nested values at this site.
    /// Prepared construction resolves these in this frame; names above
    /// remain diagnostic metadata only.
    pub capture_bindings: Arc<[FlowBindingRef]>,
    /// The authored nested callables evaluated at this site, in authored
    /// order — the per-callback partition of `capture_bindings`. Empty
    /// for a site holding no closure.
    pub closures: Arc<[SkeletonClosure]>,
    /// Call / construct footprints attributed to this site.
    pub calls: Arc<[SkeletonCall]>,
}

// ---------------------------------------------------------------------------
// Writes (assignment / kill summary)
// ---------------------------------------------------------------------------

/// The root target of one write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, NoTypeExpr)]
pub enum SkeletonWriteTarget {
    /// A named root (a local slot when the name binds in this frame,
    /// otherwise a free name).
    Named(FlowNameId),
    /// An unresolvable target root (call result, `this`, computed root).
    Opaque,
}

/// One segment of a write's projection path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonPathSegment {
    /// A statically-known property key.
    Static(FlowNameId),
    /// A computed / unknown key.
    Computed,
}

/// Whether a write definitely happens when its site evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SkeletonWriteCertainty {
    /// The write happens whenever the site evaluates.
    Definite,
    /// The write is conditional on the site's own evaluation (logical
    /// assignment, iteration-provided values).
    Optional,
}

/// One write of the assignment / kill summary, in source order.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonWrite {
    /// The write's root target.
    pub target: SkeletonWriteTarget,
    /// Exact target identifier occurrence (absent for an opaque target).
    pub target_span: Option<FrameSpan>,
    /// The exact indexed runtime variable written by this site.
    pub binding: Option<FlowBindingRef>,
    /// The projection path under the root (empty = whole-slot write).
    pub path: Arc<[SkeletonPathSegment]>,
    /// Whether the write definitely happens when the site evaluates.
    pub certainty: SkeletonWriteCertainty,
    /// The site providing the written value (`None` for self-referential
    /// update writes like `x++`).
    pub value: Option<SkeletonExprSiteId>,
    /// The tracked site whose evaluation performs the write.
    pub site: SkeletonExprSiteId,
    /// The region the write evaluates in.
    pub region: SkeletonRegionId,
    /// The write expression's span.
    pub span: FrameSpan,
}

// ---------------------------------------------------------------------------
// Return sites
// ---------------------------------------------------------------------------

/// One `return` site of the indexed function, in source order.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonReturnSite {
    /// Source-order ordinal.
    pub ordinal: u32,
    /// The region the return site evaluates in.
    pub region: SkeletonRegionId,
    /// The returned expression's site (`None` for bare `return;`).
    pub argument: Option<SkeletonExprSiteId>,
    /// Whether the site is the implicit return of an expression-bodied
    /// arrow.
    pub implicit: bool,
    /// The return statement's span.
    pub span: FrameSpan,
}

// ---------------------------------------------------------------------------
// The skeleton
// ---------------------------------------------------------------------------

/// The arena-free shallow skeleton of one authored function body: the
/// statement / control-region skeleton, the return-site index, the lexical
/// binding index, the assignment / kill summary, and per-site read / write /
/// call / object-shape footprints.
///
/// Built once per function content version from the retained parse
/// snapshot; never rebuilt per query or demand. Stores NO lowered type —
/// every leaf is an interned name, ordinal, span, or id.
///
/// **Every span here is a [`FrameSpan`]**, relative to the function's own
/// start ([`FunctionBodySource::anchor`]) — an absolute file offset cannot
/// be stored here because it does not have the type. The skeleton is
/// content-addressed and reused across every file content its key admits,
/// and an absolute offset is not a property of that content: a blank line
/// above the function moves all of them while changing nothing the key can
/// see. Consumers rebase a live position through [`FrameSpan::rebase`]
/// before comparing.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FunctionBodySkeleton {
    /// The interned name table.
    pub names: Arc<[Arc<str>]>,
    /// The control regions; index 0 is the function-body root.
    pub regions: Arc<[SkeletonRegion]>,
    /// The lexical binding index.
    pub bindings: Arc<[SkeletonBinding]>,
    /// The tracked expression sites (parents precede their children).
    pub expr_sites: Arc<[SkeletonExprSite]>,
    /// The return-site index, in source order.
    pub return_sites: Arc<[SkeletonReturnSite]>,
    /// The assignment / kill summary, in source order.
    pub writes: Arc<[SkeletonWrite]>,
}

impl FunctionBodySkeleton {
    /// The interned text of `name`.
    #[must_use]
    pub fn name(&self, name: FlowNameId) -> &str {
        &self.names[name.index()]
    }

    /// The id of an interned name, when present.
    #[must_use]
    pub fn name_id(&self, text: &str) -> Option<FlowNameId> {
        self.names
            .iter()
            .position(|candidate| candidate.as_ref() == text)
            .and_then(|index| u32::try_from(index).ok())
            .map(FlowNameId)
    }

    /// Every binding of `name` in this frame, in declaration order.
    pub fn bindings_named(&self, name: FlowNameId) -> impl Iterator<Item = SkeletonBindingId> + '_ {
        self.bindings
            .iter()
            .enumerate()
            .filter(move |(_, binding)| binding.name == name)
            .filter_map(|(index, _)| u32::try_from(index).ok().map(SkeletonBindingId))
    }

    /// The region record for `id`.
    #[must_use]
    pub fn region(&self, id: SkeletonRegionId) -> &SkeletonRegion {
        &self.regions[id.index()]
    }

    /// The binding record for `id`.
    #[must_use]
    pub fn binding(&self, id: SkeletonBindingId) -> &SkeletonBinding {
        &self.bindings[id.index()]
    }

    /// The expression-site record for `id`.
    #[must_use]
    pub fn expr_site(&self, id: SkeletonExprSiteId) -> &SkeletonExprSite {
        &self.expr_sites[id.index()]
    }

    /// The return-site record for `id`.
    #[must_use]
    pub fn return_site(&self, id: SkeletonReturnSiteId) -> &SkeletonReturnSite {
        &self.return_sites[id.index()]
    }

    /// The innermost control region whose span CONTAINS `span` — the
    /// region an authored position evaluates in. Regions are
    /// statement-scoped and properly nested, so the smallest containing
    /// region is unique; a position outside every nested region (the
    /// body's own top level) resolves to the function-body root.
    #[must_use]
    pub fn innermost_region_containing(&self, span: FrameSpan) -> SkeletonRegionId {
        let mut best = SkeletonRegionId(0);
        let mut best_width = u32::MAX;
        for (index, region) in self.regions.iter().enumerate() {
            if !region.span.contains(span) {
                continue;
            }
            let width = region.span.width();
            if width <= best_width {
                best_width = width;
                best = SkeletonRegionId(u32::try_from(index).unwrap_or(u32::MAX));
            }
        }
        best
    }

    /// THE lexical binding authority: every binding `name` resolves to
    /// when read/written/called from `region`.
    ///
    /// A reference binds to the declaration(s) of the NEAREST enclosing
    /// region carrying that name — an innermost-first walk of the region
    /// parent chain. A shadowed same-named OUTER binding is therefore
    /// never returned, so a consumer can never conflate two distinct
    /// slots that share a name. Only when the enclosing chain carries NO
    /// declaration does resolution fall back to same-name bindings of
    /// the HOISTING kinds — `var` and nested function declarations hoist
    /// to function scope wherever they are written; block-scoped kinds
    /// (`let` / `const` / `using` / class / catch-param / `enum` /
    /// `namespace` / `import =`) never do.
    ///
    /// An EMPTY result means the name is FREE in this frame (a module- or
    /// outer-scope reference), never "unknown".
    ///
    /// The walk is MEANING-FILTERED, symmetrically with
    /// [`Self::declares_meaning_in_scope`]: a TYPE-ONLY declaration
    /// (`type` / `interface`) occupies no value space, so it is
    /// transparent here at EVERY hop rather than stopping the walk. A
    /// filter applied only at the first hit would report "free" the
    /// moment `type Info = …` shadowed an enclosing `const Info`.
    #[must_use]
    pub fn bindings_of_name_in_scope(
        &self,
        name: FlowNameId,
        region: SkeletonRegionId,
    ) -> Vec<SkeletonBindingId> {
        let mut current = Some(region);
        while let Some(enclosing) = current {
            let mut hits: Vec<SkeletonBindingId> = Vec::new();
            for (index, binding) in self.bindings.iter().enumerate() {
                if binding.name == name
                    && binding.region == enclosing
                    && binding.kind.declares_value()
                {
                    hits.push(SkeletonBindingId::from_index(index as u32));
                }
            }
            if !hits.is_empty() {
                // The FUNCTION-scope frame is the parameters PLUS every
                // hoisting-kind binding of the name, wherever it is
                // written: a `var` redeclaring a parameter shares the
                // parameter's slot, so a root-region resolution unions
                // both (`function f(x) { { var x = "s"; } return x }`
                // must reach the block declarator). Inner block-scoped
                // frames stay exact — shadowing is preserved.
                if self.regions[enclosing.index()].parent.is_none() {
                    for hoisted in self.hoisting_bindings_of_name(name) {
                        if !hits.contains(&hoisted) {
                            hits.push(hoisted);
                        }
                    }
                }
                return hits;
            }
            current = self.regions[enclosing.index()].parent;
        }
        self.hoisting_bindings_of_name(name)
    }

    /// The TYPE-SPACE twin of [`Self::bindings_of_name_in_scope`]:
    /// whether this frame declares `name` in `meaning` anywhere on the
    /// region chain enclosing `region`.
    ///
    /// The spaces are resolved SEPARATELY, not by filtering the value
    /// lookup's answer. `bindings_of_name_in_scope` stops at the nearest
    /// region binding the name in VALUE space, so filtering its result by
    /// kind reports "not type-bound" the moment a value-only binding
    /// (`const` / `let` / `var` / a parameter / a nested function
    /// declaration) shadows a type-declaring OUTER binding of the same
    /// frame — `class Info {}` at the frame root with `const Info = 1` in
    /// an inner block still owns `Info` in type space at that inner
    /// block. TypeScript's `resolveName` with a given meaning SKIPS a
    /// scope whose symbol lacks that meaning and continues outward, so
    /// this walk filters at EVERY hop instead of at the first hit only.
    ///
    /// Which kind answers which meaning is
    /// [`SkeletonBindingKind::declares`]. (`namespace` and `import =` are
    /// illegal inside a function body — TS1235 / TS1232 — but the
    /// skeleton still records the recovered binding, and it genuinely
    /// occupies its spaces when it does.)
    ///
    /// There is no hoisting union here: no hoisting kind (`var`, a nested
    /// function declaration) declares a type or a namespace, so the
    /// function-scope hoisting fallback that
    /// [`Self::bindings_of_name_in_scope`] applies can contribute nothing.
    #[must_use]
    pub fn declares_meaning_in_scope(
        &self,
        name: FlowNameId,
        region: SkeletonRegionId,
        meaning: NameMeaning,
    ) -> bool {
        let mut current = Some(region);
        while let Some(enclosing) = current {
            if self.bindings.iter().any(|binding| {
                binding.name == name
                    && binding.region == enclosing
                    && binding.kind.declares(meaning)
            }) {
                return true;
            }
            current = self.regions[enclosing.index()].parent;
        }
        false
    }

    /// Every same-name binding that reaches FUNCTION scope, wherever in
    /// the frame it is written.
    ///
    /// `var` hoists unconditionally — that is the whole of its scoping
    /// rule. A function DECLARATION does not: in strict-mode code (every
    /// ES module, which is every carrier surface this substrate serves) a
    /// block-level function declaration is BLOCK-scoped, and only
    /// Annex-B sloppy-mode semantics create the function-scoped alias. So
    /// a nested function declaration reaches function scope exactly when
    /// it is written at the frame's ROOT region; one inside a block, an
    /// `if` arm, or a loop body stays where it was written, and a
    /// function-scope read of that name resolves to whatever encloses the
    /// frame — never to the block's function.
    fn hoisting_bindings_of_name(&self, name: FlowNameId) -> Vec<SkeletonBindingId> {
        self.bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                binding.name == name
                    && match binding.kind {
                        SkeletonBindingKind::Var => true,
                        SkeletonBindingKind::NestedFunction => {
                            self.regions[binding.region.index()].parent.is_none()
                        }
                        _ => false,
                    }
            })
            .map(|(index, _)| SkeletonBindingId::from_index(index as u32))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Producer input
// ---------------------------------------------------------------------------

/// The authored positions one skeleton is built from: formal parameters
/// plus the body statement list of exactly one function / arrow, borrowed
/// from the retained parse snapshot for the duration of the build only.
pub struct FunctionBodySource<'a, 'ast> {
    /// The formal parameters.
    pub params: &'a oxc_ast::ast::FormalParameters<'ast>,
    /// The body statements.
    pub statements: &'a [Statement<'ast>],
    /// Whether the body is an expression-bodied arrow (the single
    /// expression statement is the implicit return).
    pub expression_body: bool,
    /// The body span.
    pub body_span: verter_span::Span,
    /// The FUNCTION's own start offset — the anchor every [`FrameSpan`]
    /// the skeleton records is stored RELATIVE to.
    ///
    /// The skeleton is a CONTENT-ADDRESSED artifact: it is memoized per
    /// function content version and reused for any file content its key
    /// admits. Absolute source positions are not a property of that
    /// content — a blank line ANYWHERE above the function moves every
    /// one of them while changing nothing the key can see — so the
    /// skeleton stores no absolute position at all. Consumers rebase
    /// live positions onto the same anchor before asking the skeleton
    /// anything.
    ///
    /// The anchor precedes the parameter list and a named function
    /// expression's own name, so every recorded span is non-negative.
    pub anchor: u32,
    /// A NAMED FUNCTION EXPRESSION's own name, which binds INSIDE its own
    /// body and nowhere else (`const g = function h() { … h … }`). It is
    /// part of this frame's lexical inventory, not the enclosing one, so
    /// the skeleton must carry it or a read of `h` looks free and falls
    /// through to whatever the enclosing (or module) scope has under that
    /// name.
    ///
    /// `None` for an arrow, for an anonymous function expression, and for
    /// a function DECLARATION — a declaration's name binds in the
    /// ENCLOSING scope, so it is that frame's inventory, never this one's.
    pub self_binding: Option<&'a oxc_ast::ast::BindingIdentifier<'ast>>,
}

impl<'a, 'ast> FunctionBodySource<'a, 'ast> {
    /// The source positions of a bodied function DECLARATION (`None` for
    /// a bodiless overload signature). The declaration's own name binds
    /// in the enclosing scope, so it is not part of this body's frame —
    /// a named function EXPRESSION uses
    /// [`Self::from_function_expression`] instead.
    #[must_use]
    pub fn from_function(function: &'a Function<'ast>) -> Option<Self> {
        let body = function.body.as_ref()?;
        Some(Self {
            params: &function.params,
            statements: &body.statements,
            expression_body: false,
            body_span: body.span.into(),
            anchor: function.span.start,
            self_binding: None,
        })
    }

    /// The source positions of a bodied function EXPRESSION: as
    /// [`Self::from_function`], plus the expression's own name as a
    /// binding of THIS frame.
    #[must_use]
    pub fn from_function_expression(function: &'a Function<'ast>) -> Option<Self> {
        Some(Self {
            self_binding: function.id.as_ref(),
            ..Self::from_function(function)?
        })
    }

    /// The source positions of an arrow function expression.
    #[must_use]
    pub fn from_arrow(arrow: &'a ArrowFunctionExpression<'ast>) -> Self {
        Self {
            params: &arrow.params,
            statements: &arrow.body.statements,
            expression_body: arrow.expression,
            body_span: arrow.body.span.into(),
            anchor: arrow.span.start,
            self_binding: None,
        }
    }
}

/// Build the [`FunctionBodySkeleton`] of one authored function body. One
/// structural walk; no type lowering, no resolution, no fact production —
/// nested function / arrow / class bodies are never entered (they are their
/// own frames).
#[must_use]
pub fn build_function_body_skeleton(source: &FunctionBodySource<'_, '_>) -> FunctionBodySkeleton {
    build_body_skeleton(source, None)
}

/// A complete indexed structural artifact, built once before graph publication.
pub struct PreparedFunctionBodySkeleton {
    skeleton: FunctionBodySkeleton,
    bindings: FlowBindingMap,
}

impl PreparedFunctionBodySkeleton {
    pub fn skeleton(&self) -> &FunctionBodySkeleton {
        &self.skeleton
    }

    pub fn bindings(&self) -> &FlowBindingMap {
        &self.bindings
    }

    pub fn into_parts(self) -> (FunctionBodySkeleton, FlowBindingMap) {
        (self.skeleton, self.bindings)
    }
}

/// Build one current frame using the index's exact nested access facts. No child
/// skeleton or graph is constructed to recover closure dependencies.
pub fn build_indexed_function_body_skeleton(
    source: &FunctionBodySource<'_, '_>,
    entry: &FunctionProgramEntry,
) -> Result<PreparedFunctionBodySkeleton, FlowBindingMapError> {
    let skeleton = build_body_skeleton(source, Some(entry));
    prepare_function_body_skeleton(skeleton, entry)
}

/// Resolve the authored access occurrences against one exact indexed inventory.
/// The map is returned with the skeleton so the graph bundle can publish both.
fn prepare_function_body_skeleton(
    mut skeleton: FunctionBodySkeleton,
    entry: &FunctionProgramEntry,
) -> Result<PreparedFunctionBodySkeleton, FlowBindingMapError> {
    let mut bindings =
        FlowBindingMap::build(&skeleton, &entry.bindings, &entry.key, entry.span.start)?;
    bindings.prepare_occurrences(entry)?;
    for (ordinal, binding) in Arc::make_mut(&mut skeleton.bindings).iter_mut().enumerate() {
        binding.runtime_binding = binding
            .kind
            .declares_value()
            .then(|| bindings.canonical_local(SkeletonBindingId::from_index(ordinal as u32)));
    }
    for site in Arc::make_mut(&mut skeleton.expr_sites) {
        for capture in Arc::make_mut(&mut site.capture_bindings) {
            if let FlowBindingRef::Captured(identity) = capture {
                *capture = bindings.resolve_identity(identity)?;
            }
        }
        // The per-callable partition resolves through the SAME map as the
        // site union above, so a subject can never be named one way in the
        // union and another way in the callback that produced it.
        for closure in Arc::make_mut(&mut site.closures) {
            for capture in Arc::make_mut(&mut closure.captures) {
                if let FlowBindingRef::Captured(identity) = capture {
                    *capture = bindings.resolve_identity(identity)?;
                }
            }
            // The callable's own read subjects resolve through the SAME
            // map, so a cell named one way in `captures` is named the
            // same way here and the two lists stay comparable.
            for capture in Arc::make_mut(&mut closure.read_captures) {
                if let FlowBindingRef::Captured(identity) = capture {
                    *capture = bindings.resolve_identity(identity)?;
                }
            }
        }
        for read in Arc::make_mut(&mut site.reads) {
            read.binding = match &read.binding {
                Some(FlowBindingRef::Captured(identity)) => {
                    Some(bindings.resolve_identity(identity)?)
                }
                _ => bindings.required_occurrence(read.span)?,
            };
        }
        for query in Arc::make_mut(&mut site.source_type_queries) {
            query.binding = bindings.required_source_type_query(query.span)?;
        }
        for call in Arc::make_mut(&mut site.calls) {
            call.binding = call
                .root_span
                .map(|span| bindings.required_occurrence(span))
                .transpose()?
                .flatten();
        }
    }
    for write in Arc::make_mut(&mut skeleton.writes) {
        if let Some(span) = write.target_span {
            write.binding = bindings.required_occurrence(span)?;
        }
    }
    Ok(PreparedFunctionBodySkeleton { skeleton, bindings })
}

fn build_body_skeleton(
    source: &FunctionBodySource<'_, '_>,
    entry: Option<&FunctionProgramEntry>,
) -> FunctionBodySkeleton {
    let mut builder = SkeletonBuilder::new(source.anchor, source.body_span, entry);
    // A named function expression's own name is an immutable binding of
    // its own frame, in scope over the parameters and the whole body. It
    // is recorded as a nested-function-kind binding: a function-valued
    // local this substrate does not evaluate, so a read or call of it
    // resolves HERE and fails closed rather than escaping to an outer
    // same-name declaration.
    if let Some(self_binding) = source.self_binding {
        builder.push_binding(
            self_binding.name.as_str(),
            SkeletonBindingKind::NestedFunction,
            self_binding.span.into(),
            None,
            false,
        );
    }
    builder.collect_params(source.params);
    if source.expression_body {
        if let [Statement::ExpressionStatement(statement)] = source.statements {
            let argument = builder.open_root_site(&statement.expression);
            builder.push_implicit_return(argument, statement.span.into());
            return builder.finish();
        }
    }
    for statement in source.statements {
        builder.visit_statement(statement);
    }
    builder.finish()
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

struct SiteDraft {
    span: FrameSpan,
    region: SkeletonRegionId,
    parent: Option<SkeletonExprSiteId>,
    shape: SkeletonExprShape,
    reads: Vec<SkeletonRead>,
    source_type_queries: Vec<SkeletonSourceTypeQuery>,
    captures: Vec<FlowNameId>,
    capture_bindings: Vec<FlowBindingRef>,
    closures: Vec<SkeletonClosure>,
    calls: Vec<SkeletonCall>,
}

struct SkeletonBuilder<'entry> {
    /// The function's own start offset — the anchor [`Self::frame_span`]
    /// rebases every OXC position onto, at ingress. Nothing downstream of
    /// that one call can hold an absolute offset: the record fields are
    /// typed [`FrameSpan`].
    anchor: u32,
    names: Vec<Arc<str>>,
    name_lookup: FxHashMap<Arc<str>, FlowNameId>,
    regions: Vec<SkeletonRegion>,
    region_stack: Vec<usize>,
    bindings: Vec<SkeletonBinding>,
    sites: Vec<SiteDraft>,
    site_stack: Vec<usize>,
    read_kind: FlowReadKind,
    return_sites: Vec<SkeletonReturnSite>,
    writes: Vec<SkeletonWrite>,
    nested_captures: FxHashMap<verter_span::Span, &'entry FunctionNestedCaptures>,
    capture_subjects: FxHashSet<(SkeletonExprSiteId, FlowBindingRef)>,
    capture_names: FxHashSet<(SkeletonExprSiteId, FlowNameId)>,
}

impl<'entry> SkeletonBuilder<'entry> {
    fn new(
        anchor: u32,
        body_span: verter_span::Span,
        entry: Option<&'entry FunctionProgramEntry>,
    ) -> Self {
        let root = SkeletonRegion {
            kind: SkeletonRegionKind::FunctionBody,
            parent: None,
            control_input: None,
            has_return: false,
            span: FrameSpan::rebase(anchor, body_span),
        };
        Self {
            anchor,
            names: Vec::new(),
            name_lookup: FxHashMap::default(),
            regions: vec![root],
            region_stack: vec![0],
            bindings: Vec::new(),
            sites: Vec::new(),
            site_stack: Vec::new(),
            read_kind: FlowReadKind::Input,
            return_sites: Vec::new(),
            writes: Vec::new(),
            capture_subjects: FxHashSet::default(),
            capture_names: FxHashSet::default(),
            nested_captures: entry
                .map(|entry| {
                    entry
                        .nested_captures
                        .iter()
                        .map(|child| (child.span, child))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// THE ingress crossing: every OXC position this builder ever sees
    /// becomes a [`FrameSpan`] here and nowhere else.
    fn frame_span(&self, span: verter_span::Span) -> FrameSpan {
        FrameSpan::rebase(self.anchor, span)
    }

    fn intern(&mut self, text: &str) -> FlowNameId {
        if let Some(id) = self.name_lookup.get(text) {
            return *id;
        }
        let interned: Arc<str> = Arc::from(text);
        let id = FlowNameId(u32::try_from(self.names.len()).unwrap_or(u32::MAX));
        self.names.push(Arc::clone(&interned));
        self.name_lookup.insert(interned, id);
        id
    }

    fn current_region(&self) -> SkeletonRegionId {
        SkeletonRegionId(
            u32::try_from(
                *self
                    .region_stack
                    .last()
                    .expect("region stack is never empty"),
            )
            .unwrap_or(u32::MAX),
        )
    }

    fn open_region(
        &mut self,
        kind: SkeletonRegionKind,
        span: verter_span::Span,
        control_input: Option<SkeletonExprSiteId>,
    ) -> usize {
        let parent = self.current_region();
        let index = self.regions.len();
        let span = self.frame_span(span);
        self.regions.push(SkeletonRegion {
            kind,
            parent: Some(parent),
            control_input,
            has_return: false,
            span,
        });
        self.region_stack.push(index);
        index
    }

    fn close_region(&mut self) {
        self.region_stack.pop();
    }

    fn alloc_site(
        &mut self,
        span: verter_span::Span,
        parent: Option<SkeletonExprSiteId>,
    ) -> SkeletonExprSiteId {
        let id = SkeletonExprSiteId(u32::try_from(self.sites.len()).unwrap_or(u32::MAX));
        let span = self.frame_span(span);
        self.sites.push(SiteDraft {
            span,
            region: self.current_region(),
            parent,
            shape: SkeletonExprShape::Other,
            reads: Vec::new(),
            source_type_queries: Vec::new(),
            captures: Vec::new(),
            capture_bindings: Vec::new(),
            closures: Vec::new(),
            calls: Vec::new(),
        });
        id
    }

    fn current_site(&self) -> Option<SkeletonExprSiteId> {
        self.site_stack
            .last()
            .copied()
            .map(|index| SkeletonExprSiteId(u32::try_from(index).unwrap_or(u32::MAX)))
    }

    /// The site footprint currently accumulating; a footprint observed
    /// outside any tracked site opens a one-shot anonymous site so no
    /// read / write / call is ever dropped.
    fn footprint_site(&mut self, span: verter_span::Span) -> SkeletonExprSiteId {
        match self.current_site() {
            Some(site) => site,
            None => self.alloc_site(span, None),
        }
    }

    /// Guarantee a current site for a multi-record expression handler;
    /// `true` when an anonymous scope was opened (the caller pops it).
    fn ensure_site_scope(&mut self, span: verter_span::Span) -> bool {
        if self.site_stack.is_empty() {
            let id = self.alloc_site(span, None);
            self.site_stack.push(id.index());
            true
        } else {
            false
        }
    }

    /// Track one root expression position (statement expression, return
    /// argument, declarator initializer, control input).
    fn open_root_site(&mut self, expression: &Expression<'_>) -> SkeletonExprSiteId {
        self.open_site(expression, None)
    }

    /// Track one expression position, descending through exactly the
    /// forms the SHARED classifier ([`value_descent`]) says have
    /// value-providing children.
    ///
    /// This is the planner-side twin of `flow_slice_content`'s
    /// `lower_expr`: every position the content half lowers is a
    /// position opened here, and both dispatch on the same verdict — so
    /// a form the content half descends into cannot be a form this half
    /// skips. The site's SPAN stays the OUTERMOST expression's
    /// throughout the descent, which is the span the content half
    /// rebases and asks the selection about.
    fn open_site(
        &mut self,
        expression: &Expression<'_>,
        parent: Option<SkeletonExprSiteId>,
    ) -> SkeletonExprSiteId {
        self.open_site_at(expression, parent, expression.span().into())
    }

    fn open_site_at(
        &mut self,
        expression: &Expression<'_>,
        parent: Option<SkeletonExprSiteId>,
        span: verter_span::Span,
    ) -> SkeletonExprSiteId {
        match value_descent(expression) {
            // Both carriers descend HERE. The type carrier is the one
            // asymmetric disposition: the content half leaf-lowers it,
            // so this side over-selects — see [`ValueDescent`].
            ValueDescent::Transparent(inner) | ValueDescent::TypeCarrier(inner) => {
                self.open_site_at(inner, parent, span)
            }
            ValueDescent::Object(object) => self.open_object_site(object, parent, span),
            ValueDescent::Branches(conditional) => self.open_branch_site(conditional, parent, span),
            // An unmodeled CALL POSITION has no value-providing child
            // either — a call's arguments do not provide its value — so
            // it takes the leaf disposition here. Its sub-expressions are
            // still visited for their evaluation effects; the CONTENT
            // half is where the verdict differs (it fails closed).
            disposition @ (ValueDescent::Reference
            | ValueDescent::Logical
            | ValueDescent::Sequence
            | ValueDescent::Leaf
            | ValueDescent::UnmodeledCall) => {
                let id = self.alloc_site(span, parent);
                self.site_stack.push(id.index());
                let kind = if matches!(
                    disposition,
                    ValueDescent::Reference | ValueDescent::Logical | ValueDescent::Sequence
                ) {
                    FlowReadKind::Result
                } else {
                    FlowReadKind::Input
                };
                let previous = std::mem::replace(&mut self.read_kind, kind);
                self.visit_expression(expression);
                self.read_kind = previous;
                self.site_stack.pop();
                id
            }
        }
    }

    /// A branch JOIN site: the test's footprint stays on this site (its
    /// value is never consumed, its evaluation effects are), and each arm
    /// becomes its own child site so the graph can name it a value
    /// provider of the whole join.
    fn open_branch_site(
        &mut self,
        conditional: &oxc_ast::ast::ConditionalExpression<'_>,
        parent: Option<SkeletonExprSiteId>,
        span: verter_span::Span,
    ) -> SkeletonExprSiteId {
        let id = self.alloc_site(span, parent);
        self.site_stack.push(id.index());
        let previous = std::mem::replace(&mut self.read_kind, FlowReadKind::Input);
        self.visit_expression(&conditional.test);
        self.read_kind = previous;
        self.site_stack.pop();
        let consequent = self.open_site(&conditional.consequent, Some(id));
        let alternate = self.open_site(&conditional.alternate, Some(id));
        self.sites[id.index()].shape = SkeletonExprShape::BranchJoin {
            arms: Arc::from(vec![consequent, alternate].into_boxed_slice()),
        };
        id
    }

    fn open_object_site(
        &mut self,
        object: &ObjectExpression<'_>,
        parent: Option<SkeletonExprSiteId>,
        span: verter_span::Span,
    ) -> SkeletonExprSiteId {
        let id = self.alloc_site(span, parent);
        let mut entries = Vec::new();
        for property in &object.properties {
            // THE per-entry disposition, taken from the ONE shared
            // classifier the content half also dispatches on — so a
            // spread cannot be a value provider on this side and an
            // abandoned structural lowering on the other.
            match object_entry_descent(property) {
                ObjectEntryDescent::Property {
                    key,
                    value,
                    kind,
                    property: _,
                } => {
                    let key = match key {
                        ObjectEntryKey::Static(name) => {
                            SkeletonObjectKey::Static(self.intern(name))
                        }
                        ObjectEntryKey::Computed(key_expression) => {
                            SkeletonObjectKey::Computed(self.open_site(key_expression, Some(id)))
                        }
                        // A key form neither half models: the entry is
                        // not tracked at all, and the content half
                        // leaf-lowers the whole literal.
                        ObjectEntryKey::Unmodeled => continue,
                    };
                    let kind = match kind {
                        ObjectEntryKind::Init => SkeletonPropertyKind::Init,
                        ObjectEntryKind::Method => SkeletonPropertyKind::Method,
                        ObjectEntryKind::Get | ObjectEntryKind::Set => {
                            SkeletonPropertyKind::Accessor
                        }
                    };
                    let value = self.open_site(value, Some(id));
                    entries.push(SkeletonObjectEntry::Property { key, value, kind });
                }
                ObjectEntryDescent::Spread { source } => {
                    let source = self.open_site(source, Some(id));
                    entries.push(SkeletonObjectEntry::Spread { source });
                }
            }
        }
        self.sites[id.index()].shape = SkeletonExprShape::ObjectLiteral {
            entries: Arc::from(entries.into_boxed_slice()),
        };
        id
    }

    /// An authored callable no index record serves asserts nothing about
    /// what it captures: it is recorded as such, never omitted.
    fn push_unserved_callable(&mut self, span: verter_span::Span) {
        let site = self.footprint_site(span);
        let closure_span = self.frame_span(span);
        self.sites[site.index()].closures.push(SkeletonClosure {
            span: closure_span,
            correlation: SkeletonClosureCorrelation::Uncorrelated,
            captures: Arc::from([]),
            read_captures: Arc::from([]),
        });
    }

    fn push_read(&mut self, name: FlowNameId, span: verter_span::Span) {
        self.push_read_path(name, Arc::from(Vec::new().into_boxed_slice()), span);
    }

    fn push_read_path(
        &mut self,
        name: FlowNameId,
        path: Arc<[SkeletonPathSegment]>,
        span: verter_span::Span,
    ) {
        let site = self.footprint_site(span);
        let span = self.frame_span(span);
        self.sites[site.index()].reads.push(SkeletonRead {
            name,
            path,
            span,
            binding: None,
            kind: self.read_kind,
        });
    }

    /// A nested function retains each captured cell, including write-only
    /// captures. Actual free reads separately carry the paths whose reaching
    /// outer definitions seed the nested evaluator.
    /// Exact subjects are retained in `capture_bindings`; `captures`
    /// keeps their interned display names as diagnostic metadata.
    ///
    /// EVERY authored nested callable also records its own
    /// [`SkeletonClosure`] on the site, whether or not the indexed program
    /// correlated it and whether or not it captures anything. Without that
    /// per-callable record the site-level union is the only fact available,
    /// and a site is not a callback: two callbacks sharing one call or one
    /// array literal are indistinguishable, a capture-free callback is
    /// indistinguishable from no callback at all, and a callable the index
    /// does not serve is silently invisible rather than a typed gap.
    fn push_nested_callable(&mut self, span: verter_span::Span) {
        let Some(captures) = self.nested_captures.get(&span).copied() else {
            self.push_unserved_callable(span);
            return;
        };
        let site = self.footprint_site(span);
        let closure_span = self.frame_span(span);
        let mut own: Vec<FlowBindingRef> = Vec::new();
        let mut own_seen = FxHashSet::default();
        for identity in captures.bindings.0.iter() {
            let binding = FlowBindingRef::Captured(identity.clone());
            if own_seen.insert(binding.clone()) {
                own.push(binding.clone());
            }
            if self.capture_subjects.insert((site, binding.clone())) {
                self.sites[site.index()].capture_bindings.push(binding);
            }
            let name = self.intern(&identity.name);
            if self.capture_names.insert((site, name)) {
                self.sites[site.index()].captures.push(name);
            }
        }
        // This callable's OWN read subjects, kept apart from the site
        // footprint below: the site's `reads` merge every callable here,
        // so only this per-callable list can say that a sibling — and not
        // this callable — is the one consuming the cell's value.
        let mut own_reads: Vec<FlowBindingRef> = Vec::new();
        let mut own_read_seen = FxHashSet::default();
        for read in captures.reads.iter() {
            let binding = FlowBindingRef::Captured(read.binding.clone());
            if own_read_seen.insert(binding.clone()) {
                own_reads.push(binding);
            }
        }
        self.sites[site.index()].closures.push(SkeletonClosure {
            span: closure_span,
            correlation: if captures.exhaustive {
                SkeletonClosureCorrelation::Exact
            } else {
                SkeletonClosureCorrelation::Partial
            },
            captures: Arc::from(own.into_boxed_slice()),
            read_captures: Arc::from(own_reads.into_boxed_slice()),
        });
        for read in captures.reads.iter() {
            let name = self.intern(&read.binding.name);
            let path: Arc<[_]> = read
                .path
                .iter()
                .map(|segment| SkeletonPathSegment::Static(self.intern(segment)))
                .collect::<Vec<_>>()
                .into();
            let span = self.frame_span(read.span);
            self.sites[site.index()].reads.push(SkeletonRead {
                name,
                path,
                span,
                binding: Some(FlowBindingRef::Captured(read.binding.clone())),
                kind: FlowReadKind::Input,
            });
        }
    }

    /// `span` is the call expression's ABSOLUTE position; the recorded
    /// [`SkeletonCall::span`] is its frame-relative twin, so a call effect
    /// and a write effect are ordered in the SAME coordinate system.
    fn push_call(
        &mut self,
        expression: &Expression<'_>,
        new_construct: bool,
        span: verter_span::Span,
    ) -> SkeletonExprSiteId {
        let callee = self.extract_callee(expression);
        let root_span = crate::analysis::function_program::access::expression_root(expression)
            .map(|root| self.frame_span(root.span.into()));
        let site = self.footprint_site(span);
        let span = self.frame_span(span);
        self.sites[site.index()].calls.push(SkeletonCall {
            callee,
            new_construct,
            span,
            root_span,
            binding: None,
        });
        site
    }

    fn push_write(
        &mut self,
        target: SkeletonWriteTarget,
        path: Arc<[SkeletonPathSegment]>,
        certainty: SkeletonWriteCertainty,
        value: Option<SkeletonExprSiteId>,
        span: verter_span::Span,
        target_span: Option<verter_span::Span>,
    ) {
        let site = self.footprint_site(span);
        let region = self.current_region();
        let span = self.frame_span(span);
        self.writes.push(SkeletonWrite {
            target,
            target_span: target_span.map(|span| self.frame_span(span)),
            binding: None,
            path,
            certainty,
            value,
            site,
            region,
            span,
        });
    }

    fn push_binding(
        &mut self,
        name: &str,
        kind: SkeletonBindingKind,
        span: verter_span::Span,
        initializer: Option<SkeletonExprSiteId>,
        destructured: bool,
    ) {
        let name = self.intern(name);
        let region = self.current_region();
        let span = self.frame_span(span);
        self.bindings.push(SkeletonBinding {
            name,
            runtime_binding: None,
            kind,
            region,
            span,
            initializer,
            annotation_span: None,
            destructured,
            pattern_sites: Arc::from([]),
        });
    }

    /// Open every expression one binding pattern evaluates while it binds
    /// as its own root site, so no footprint is dropped; the pattern's
    /// bindings then own the sites as evaluation effects.
    fn open_pattern_sites(&mut self, evaluated: Vec<&Expression<'_>>) -> Arc<[SkeletonExprSiteId]> {
        evaluated
            .into_iter()
            .map(|expression| self.open_root_site(expression))
            .collect()
    }

    fn push_implicit_return(&mut self, argument: SkeletonExprSiteId, span: verter_span::Span) {
        for index in &self.region_stack {
            self.regions[*index].has_return = true;
        }
        let span = self.frame_span(span);
        self.return_sites.push(SkeletonReturnSite {
            ordinal: u32::try_from(self.return_sites.len()).unwrap_or(u32::MAX),
            region: self.current_region(),
            argument: Some(argument),
            implicit: true,
            span,
        });
    }

    fn collect_params(&mut self, params: &oxc_ast::ast::FormalParameters<'_>) {
        for param in &params.items {
            self.collect_param_pattern(&param.pattern, param.initializer.as_deref());
        }
        if let Some(rest) = params.rest.as_ref() {
            self.collect_param_pattern(&rest.rest.argument, None);
        }
    }

    fn collect_param_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        default: Option<&Expression<'_>>,
    ) {
        let (inner, initializer) = match pattern {
            BindingPattern::AssignmentPattern(assignment) => {
                let site = self.open_root_site(&assignment.right);
                (&assignment.left, Some(site))
            }
            other => (other, default.map(|default| self.open_root_site(default))),
        };
        let mut identifiers = Vec::new();
        let mut defaults = Vec::new();
        collect_binding_pattern(inner, false, &mut identifiers, &mut defaults);
        let pattern_sites = self.open_pattern_sites(defaults);
        for (name, span, destructured) in identifiers {
            self.push_binding(
                &name,
                SkeletonBindingKind::Param,
                span,
                initializer,
                destructured,
            );
            self.bindings.last_mut().unwrap().pattern_sites = Arc::clone(&pattern_sites);
        }
    }

    fn collect_declarator_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        kind: SkeletonBindingKind,
        initializer: Option<SkeletonExprSiteId>,
        annotation_span: Option<FrameSpan>,
    ) {
        let mut identifiers = Vec::new();
        let mut defaults = Vec::new();
        collect_binding_pattern(pattern, false, &mut identifiers, &mut defaults);
        let pattern_sites = self.open_pattern_sites(defaults);
        for (name, span, destructured) in identifiers {
            self.push_binding(&name, kind, span, initializer, destructured);
            self.bindings.last_mut().unwrap().pattern_sites = Arc::clone(&pattern_sites);
            if !destructured {
                self.bindings.last_mut().unwrap().annotation_span = annotation_span;
            }
        }
    }

    fn record_assignment_targets(
        &mut self,
        target: &AssignmentTarget<'_>,
        certainty: SkeletonWriteCertainty,
        compound_read: bool,
        value: Option<SkeletonExprSiteId>,
    ) {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = self.intern(identifier.name.as_str());
                if compound_read {
                    self.push_read(name, identifier.span.into());
                }
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    Arc::from(Vec::new().into_boxed_slice()),
                    certainty,
                    value,
                    identifier.span.into(),
                    Some(identifier.span.into()),
                );
            }
            AssignmentTarget::StaticMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Static(member), certainty, value);
            }
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Computed(member), certainty, value);
            }
            AssignmentTarget::PrivateFieldExpression(member) => {
                self.record_member_write_target(MemberRef::Private(member), certainty, value);
            }
            AssignmentTarget::TSAsExpression(as_expression) => {
                self.record_expression_write_target(&as_expression.expression, certainty, value);
            }
            AssignmentTarget::TSSatisfiesExpression(satisfies) => {
                self.record_expression_write_target(&satisfies.expression, certainty, value);
            }
            AssignmentTarget::TSNonNullExpression(non_null) => {
                self.record_expression_write_target(&non_null.expression, certainty, value);
            }
            AssignmentTarget::TSTypeAssertion(assertion) => {
                self.record_expression_write_target(&assertion.expression, certainty, value);
            }
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for element in array.elements.iter().flatten() {
                    self.record_maybe_default_target(element, value);
                }
                if let Some(rest) = array.rest.as_ref() {
                    self.record_assignment_targets(
                        &rest.target,
                        SkeletonWriteCertainty::Definite,
                        false,
                        value,
                    );
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                for property in &object.properties {
                    match property {
                        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(
                            identifier,
                        ) => {
                            let name = self.intern(identifier.binding.name.as_str());
                            if let Some(init) = identifier.init.as_ref() {
                                let parent = self.current_site();
                                let _ = self.open_site(init, parent);
                            }
                            self.push_write(
                                SkeletonWriteTarget::Named(name),
                                Arc::from(Vec::new().into_boxed_slice()),
                                SkeletonWriteCertainty::Definite,
                                value,
                                identifier.span.into(),
                                Some(identifier.binding.span.into()),
                            );
                        }
                        AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                            if let Some(key) = property.name.as_expression() {
                                self.visit_expression(key);
                            }
                            self.record_maybe_default_target(&property.binding, value);
                        }
                    }
                }
                if let Some(rest) = object.rest.as_ref() {
                    self.record_assignment_targets(
                        &rest.target,
                        SkeletonWriteCertainty::Definite,
                        false,
                        value,
                    );
                }
            }
        }
    }

    fn record_maybe_default_target(
        &mut self,
        target: &AssignmentTargetMaybeDefault<'_>,
        value: Option<SkeletonExprSiteId>,
    ) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
                let parent = self.current_site();
                let _ = self.open_site(&with_default.init, parent);
                self.record_assignment_targets(
                    &with_default.binding,
                    SkeletonWriteCertainty::Definite,
                    false,
                    value,
                );
            }
            _ => {
                self.record_assignment_targets(
                    target.to_assignment_target(),
                    SkeletonWriteCertainty::Definite,
                    false,
                    value,
                );
            }
        }
    }

    /// A member-expression write target: builds the projection path and
    /// records the root read (the root object is read to perform a member
    /// write).
    fn record_member_write_target(
        &mut self,
        member: MemberRef<'_, '_>,
        certainty: SkeletonWriteCertainty,
        value: Option<SkeletonExprSiteId>,
    ) {
        let write_span = member.span();
        let mut segments = Vec::new();
        let mut current = member;
        let root = loop {
            let object = match current {
                MemberRef::Static(static_member) => {
                    segments.push(SkeletonPathSegment::Static(
                        self.intern(static_member.property.name.as_str()),
                    ));
                    &static_member.object
                }
                MemberRef::Computed(computed) => {
                    self.visit_expression(&computed.expression);
                    segments.push(SkeletonPathSegment::Computed);
                    &computed.object
                }
                MemberRef::Private(private) => {
                    let name = format!("#{}", private.field.name);
                    segments.push(SkeletonPathSegment::Static(self.intern(&name)));
                    &private.object
                }
            };
            match object {
                Expression::Identifier(identifier) => break Some(identifier),
                Expression::StaticMemberExpression(parent) => {
                    current = MemberRef::Static(parent);
                }
                Expression::ComputedMemberExpression(parent) => {
                    current = MemberRef::Computed(parent);
                }
                Expression::PrivateFieldExpression(parent) => {
                    current = MemberRef::Private(parent);
                }
                other => {
                    self.visit_expression(other);
                    break None;
                }
            }
        };
        segments.reverse();
        let path: Arc<[SkeletonPathSegment]> = Arc::from(segments.into_boxed_slice());
        match root {
            Some(identifier) => {
                let name = self.intern(identifier.name.as_str());
                self.push_read(name, identifier.span.into());
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    path,
                    certainty,
                    value,
                    write_span,
                    Some(identifier.span.into()),
                );
            }
            None => {
                self.push_write(
                    SkeletonWriteTarget::Opaque,
                    path,
                    certainty,
                    value,
                    write_span,
                    None,
                );
            }
        }
    }

    /// A TS-carrier-wrapped write target (`(x as T) = v`): unwrap to the
    /// inner identifier / member target.
    fn record_expression_write_target(
        &mut self,
        expression: &Expression<'_>,
        certainty: SkeletonWriteCertainty,
        value: Option<SkeletonExprSiteId>,
    ) {
        match unwrap_expression_carriers(expression) {
            Expression::Identifier(identifier) => {
                let name = self.intern(identifier.name.as_str());
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    Arc::from(Vec::new().into_boxed_slice()),
                    certainty,
                    value,
                    identifier.span.into(),
                    Some(identifier.span.into()),
                );
            }
            Expression::StaticMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Static(member), certainty, value);
            }
            Expression::ComputedMemberExpression(member) => {
                self.record_member_write_target(MemberRef::Computed(member), certainty, value);
            }
            Expression::PrivateFieldExpression(member) => {
                self.record_member_write_target(MemberRef::Private(member), certainty, value);
            }
            other => {
                self.push_write(
                    SkeletonWriteTarget::Opaque,
                    Arc::from(Vec::new().into_boxed_slice()),
                    certainty,
                    value,
                    other.span().into(),
                    None,
                );
                self.visit_expression(other);
            }
        }
    }

    fn extract_callee(&mut self, callee: &Expression<'_>) -> SkeletonCallee {
        let callee = unwrap_expression_carriers(callee);
        match callee {
            Expression::Identifier(identifier) => {
                SkeletonCallee::Named(self.intern(identifier.name.as_str()))
            }
            Expression::StaticMemberExpression(member) => {
                let mut names = Vec::new();
                if collect_static_callee_path(member, &mut names) {
                    let interned: Vec<FlowNameId> =
                        names.iter().map(|name| self.intern(name)).collect();
                    SkeletonCallee::Path(Arc::from(interned.into_boxed_slice()))
                } else {
                    SkeletonCallee::Opaque
                }
            }
            _ => SkeletonCallee::Opaque,
        }
    }

    fn visit_statement_list(&mut self, statements: &[Statement<'_>]) {
        for statement in statements {
            self.visit_statement(statement);
        }
    }

    /// Publish the skeleton.
    ///
    /// A plain MOVE, deliberately: there is no per-record-family span
    /// mapping here to apply to five families and forget on two. Every span
    /// crossed into [`FrameSpan`] at ingress ([`Self::frame_span`]), and a
    /// draft field that still held an absolute offset would not have the
    /// type its published counterpart needs.
    fn finish(self) -> FunctionBodySkeleton {
        FunctionBodySkeleton {
            names: Arc::from(self.names.into_boxed_slice()),
            regions: Arc::from(self.regions.into_boxed_slice()),
            bindings: Arc::from(self.bindings.into_boxed_slice()),
            expr_sites: Arc::from(
                self.sites
                    .into_iter()
                    .map(|draft| SkeletonExprSite {
                        span: draft.span,
                        region: draft.region,
                        parent: draft.parent,
                        shape: draft.shape,
                        reads: Arc::from(draft.reads.into_boxed_slice()),
                        source_type_queries: draft.source_type_queries.into(),
                        captures: Arc::from(draft.captures.into_boxed_slice()),
                        capture_bindings: Arc::from(draft.capture_bindings.into_boxed_slice()),
                        closures: Arc::from(draft.closures.into_boxed_slice()),
                        calls: Arc::from(draft.calls.into_boxed_slice()),
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            return_sites: Arc::from(self.return_sites.into_boxed_slice()),
            writes: Arc::from(self.writes.into_boxed_slice()),
        }
    }
}

impl<'a> Visit<'a> for SkeletonBuilder<'_> {
    // Type positions are never value footprint: a type name is not a read.
    fn visit_ts_type(&mut self, _it: &oxc_ast::ast::TSType<'a>) {}

    fn visit_ts_type_annotation(&mut self, _it: &oxc_ast::ast::TSTypeAnnotation<'a>) {}

    fn visit_ts_type_parameter_instantiation(
        &mut self,
        _it: &oxc_ast::ast::TSTypeParameterInstantiation<'a>,
    ) {
    }

    fn visit_ts_type_parameter_declaration(
        &mut self,
        _it: &oxc_ast::ast::TSTypeParameterDeclaration<'a>,
    ) {
    }

    // Nested function / arrow / class bodies are their own frames.
    fn visit_function(&mut self, it: &Function<'a>, _flags: oxc_syntax::scope::ScopeFlags) {
        self.push_nested_callable(it.span.into());
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        self.push_nested_callable(it.span.into());
    }

    // A class expression is itself a callable (its constructor) and
    // creates more (members, field initializers); no index record serves
    // any of them. A class DECLARATION binds a name instead and is never
    // walked here.
    fn visit_class(&mut self, it: &oxc_ast::ast::Class<'a>) {
        self.push_unserved_callable(it.span.into());
    }

    fn visit_statement(&mut self, it: &Statement<'a>) {
        match it {
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = function.id.as_ref() {
                    self.push_binding(
                        id.name.as_str(),
                        SkeletonBindingKind::NestedFunction,
                        id.span.into(),
                        None,
                        false,
                    );
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    self.push_binding(
                        id.name.as_str(),
                        SkeletonBindingKind::Class,
                        id.span.into(),
                        None,
                        false,
                    );
                }
            }
            // A local `enum` / `namespace` / `import =` declares a VALUE
            // binding in this frame. It must be indexed even though no
            // consumer models its value: an unindexed name reads as FREE
            // and silently resolves to an unrelated module-scope (or
            // imported) declaration of the same name.
            Statement::TSEnumDeclaration(enum_declaration) => {
                self.push_binding(
                    enum_declaration.id.name.as_str(),
                    SkeletonBindingKind::Enum,
                    enum_declaration.id.span.into(),
                    None,
                    false,
                );
                walk::walk_statement(self, it);
            }
            Statement::TSModuleDeclaration(module) => {
                if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &module.id {
                    self.push_binding(
                        id.name.as_str(),
                        SkeletonBindingKind::Namespace,
                        id.span.into(),
                        None,
                        false,
                    );
                }
                walk::walk_statement(self, it);
            }
            Statement::TSImportEqualsDeclaration(import_equals) => {
                self.push_binding(
                    import_equals.id.name.as_str(),
                    SkeletonBindingKind::ImportEquals,
                    import_equals.id.span.into(),
                    None,
                    false,
                );
                walk::walk_statement(self, it);
            }
            // A local `type` / `interface` declares a TYPE-ONLY binding.
            // It is invisible to every value lookup (the meaning filter
            // in `bindings_of_name_in_scope` walks past it), but it OWNS
            // the name in type space: an unindexed one reads as FREE and
            // silently resolves `x as Info` to an unrelated module-scope
            // (or imported) declaration of the same name. The bodies are
            // pure type positions, which this visitor never enters.
            Statement::TSTypeAliasDeclaration(alias) => {
                self.push_binding(
                    alias.id.name.as_str(),
                    SkeletonBindingKind::TypeAlias,
                    alias.id.span.into(),
                    None,
                    false,
                );
            }
            Statement::TSInterfaceDeclaration(interface) => {
                self.push_binding(
                    interface.id.name.as_str(),
                    SkeletonBindingKind::Interface,
                    interface.id.span.into(),
                    None,
                    false,
                );
            }
            _ => walk::walk_statement(self, it),
        }
    }

    fn visit_block_statement(&mut self, it: &oxc_ast::ast::BlockStatement<'a>) {
        self.open_region(SkeletonRegionKind::Block, it.span.into(), None);
        self.visit_statement_list(&it.body);
        self.close_region();
    }

    fn visit_if_statement(&mut self, it: &oxc_ast::ast::IfStatement<'a>) {
        let condition = self.open_root_site(&it.test);
        self.open_region(
            SkeletonRegionKind::IfConsequent,
            it.consequent.span().into(),
            Some(condition),
        );
        self.visit_statement(&it.consequent);
        self.close_region();
        if let Some(alternate) = it.alternate.as_ref() {
            self.open_region(
                SkeletonRegionKind::IfAlternate,
                alternate.span().into(),
                Some(condition),
            );
            self.visit_statement(alternate);
            self.close_region();
        }
    }

    fn visit_while_statement(&mut self, it: &oxc_ast::ast::WhileStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let condition = self.open_root_site(&it.test);
        self.regions[region].control_input = Some(condition);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_do_while_statement(&mut self, it: &oxc_ast::ast::DoWhileStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let condition = self.open_root_site(&it.test);
        self.regions[region].control_input = Some(condition);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_for_statement(&mut self, it: &oxc_ast::ast::ForStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        if let Some(init) = it.init.as_ref() {
            match init {
                oxc_ast::ast::ForStatementInit::VariableDeclaration(declaration) => {
                    self.visit_variable_declaration(declaration);
                }
                _ => {
                    if let Some(expression) = init.as_expression() {
                        self.open_root_site(expression);
                    }
                }
            }
        }
        if let Some(test) = it.test.as_ref() {
            let condition = self.open_root_site(test);
            self.regions[region].control_input = Some(condition);
        }
        if let Some(update) = it.update.as_ref() {
            self.open_root_site(update);
        }
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_for_in_statement(&mut self, it: &oxc_ast::ast::ForInStatement<'a>) {
        self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let source = self.open_root_site(&it.right);
        self.record_for_left(&it.left, source);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_for_of_statement(&mut self, it: &oxc_ast::ast::ForOfStatement<'a>) {
        self.open_region(SkeletonRegionKind::Loop, it.span.into(), None);
        let source = self.open_root_site(&it.right);
        self.record_for_left(&it.left, source);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_switch_statement(&mut self, it: &oxc_ast::ast::SwitchStatement<'a>) {
        let region = self.open_region(SkeletonRegionKind::Switch, it.span.into(), None);
        let discriminant = self.open_root_site(&it.discriminant);
        self.regions[region].control_input = Some(discriminant);
        for case in &it.cases {
            self.open_region(SkeletonRegionKind::SwitchCase, case.span.into(), None);
            if let Some(test) = case.test.as_ref() {
                self.open_root_site(test);
            }
            self.visit_statement_list(&case.consequent);
            self.close_region();
        }
        self.close_region();
    }

    fn visit_try_statement(&mut self, it: &oxc_ast::ast::TryStatement<'a>) {
        self.open_region(SkeletonRegionKind::TryBlock, it.block.span.into(), None);
        self.visit_statement_list(&it.block.body);
        self.close_region();
        if let Some(handler) = it.handler.as_ref() {
            self.open_region(SkeletonRegionKind::CatchClause, handler.span.into(), None);
            if let Some(param) = handler.param.as_ref() {
                self.collect_declarator_pattern(
                    &param.pattern,
                    SkeletonBindingKind::CatchParam,
                    None,
                    None,
                );
            }
            self.visit_statement_list(&handler.body.body);
            self.close_region();
        }
        if let Some(finalizer) = it.finalizer.as_ref() {
            self.open_region(
                SkeletonRegionKind::FinallyBlock,
                finalizer.span.into(),
                None,
            );
            self.visit_statement_list(&finalizer.body);
            self.close_region();
        }
    }

    fn visit_labeled_statement(&mut self, it: &oxc_ast::ast::LabeledStatement<'a>) {
        self.open_region(SkeletonRegionKind::LabeledBody, it.span.into(), None);
        self.visit_statement(&it.body);
        self.close_region();
    }

    fn visit_return_statement(&mut self, it: &oxc_ast::ast::ReturnStatement<'a>) {
        for index in &self.region_stack {
            self.regions[*index].has_return = true;
        }
        let argument = it
            .argument
            .as_ref()
            .map(|argument| self.open_root_site(argument));
        let span = self.frame_span(it.span.into());
        self.return_sites.push(SkeletonReturnSite {
            ordinal: u32::try_from(self.return_sites.len()).unwrap_or(u32::MAX),
            region: self.current_region(),
            argument,
            implicit: false,
            span,
        });
    }

    fn visit_expression_statement(&mut self, it: &oxc_ast::ast::ExpressionStatement<'a>) {
        self.open_root_site(&it.expression);
    }

    fn visit_throw_statement(&mut self, it: &oxc_ast::ast::ThrowStatement<'a>) {
        self.open_root_site(&it.argument);
    }

    fn visit_variable_declaration(&mut self, it: &oxc_ast::ast::VariableDeclaration<'a>) {
        let kind = skeleton_binding_kind(it.kind);
        for declarator in &it.declarations {
            let initializer = declarator
                .init
                .as_ref()
                .map(|init| self.open_root_site(init));
            let annotation_span = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.frame_span(annotation.span.into()));
            self.collect_declarator_pattern(&declarator.id, kind, initializer, annotation_span);
        }
    }

    fn visit_expression(&mut self, it: &Expression<'a>) {
        let previous = self.read_kind;
        if matches!(
            value_descent(it),
            ValueDescent::Leaf | ValueDescent::UnmodeledCall
        ) {
            self.read_kind = FlowReadKind::Input;
        }
        walk::walk_expression(self, it);
        self.read_kind = previous;
    }

    fn visit_sequence_expression(&mut self, it: &oxc_ast::ast::SequenceExpression<'a>) {
        let Some((last, discarded)) = it.expressions.split_last() else {
            return;
        };
        let previous = std::mem::replace(&mut self.read_kind, FlowReadKind::Input);
        for expression in discarded {
            self.visit_expression(expression);
        }
        self.read_kind = previous;
        self.visit_expression(last);
    }

    fn visit_conditional_expression(&mut self, it: &oxc_ast::ast::ConditionalExpression<'a>) {
        let previous = std::mem::replace(&mut self.read_kind, FlowReadKind::Input);
        self.visit_expression(&it.test);
        self.read_kind = previous;
        self.visit_expression(&it.consequent);
        self.visit_expression(&it.alternate);
    }

    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        let name = self.intern(it.name.as_str());
        self.push_read(name, it.span.into());
    }

    fn visit_static_member_expression(&mut self, it: &oxc_ast::ast::StaticMemberExpression<'a>) {
        if let Some(reference) =
            crate::analysis::function_program::access::static_member_reference(it)
        {
            let root = self.intern(&reference.name);
            let path: Arc<[SkeletonPathSegment]> = Arc::from(
                reference
                    .path
                    .iter()
                    .map(|name| SkeletonPathSegment::Static(self.intern(name)))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            self.push_read_path(root, path, reference.span);
        } else {
            let previous = std::mem::replace(&mut self.read_kind, FlowReadKind::Input);
            walk::walk_static_member_expression(self, it);
            self.read_kind = previous;
        }
    }

    fn visit_object_expression(&mut self, it: &ObjectExpression<'a>) {
        // An object literal nested inside a non-object expression tree
        // becomes its own child site so its property footprint stays
        // path-precise.
        let parent = self.current_site();
        self.open_object_site(it, parent, it.span.into());
    }

    fn visit_assignment_expression(&mut self, it: &oxc_ast::ast::AssignmentExpression<'a>) {
        let scoped = self.ensure_site_scope(it.span.into());
        let containing = self
            .current_site()
            .expect("assignment scope guarantees a current site");
        let certainty = match it.operator {
            oxc_ast::ast::AssignmentOperator::LogicalAnd
            | oxc_ast::ast::AssignmentOperator::LogicalOr
            | oxc_ast::ast::AssignmentOperator::LogicalNullish => SkeletonWriteCertainty::Optional,
            _ => SkeletonWriteCertainty::Definite,
        };
        let compound_read = !matches!(it.operator, oxc_ast::ast::AssignmentOperator::Assign);
        let value = self.open_site(&it.right, Some(containing));
        self.record_assignment_targets(&it.left, certainty, compound_read, Some(value));
        if scoped {
            self.site_stack.pop();
        }
    }

    fn visit_update_expression(&mut self, it: &oxc_ast::ast::UpdateExpression<'a>) {
        let scoped = self.ensure_site_scope(it.span.into());
        match &it.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(identifier) => {
                let name = self.intern(identifier.name.as_str());
                self.push_read(name, identifier.span.into());
                self.push_write(
                    SkeletonWriteTarget::Named(name),
                    Arc::from(Vec::new().into_boxed_slice()),
                    SkeletonWriteCertainty::Definite,
                    None,
                    it.span.into(),
                    Some(identifier.span.into()),
                );
            }
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.record_member_write_target(
                    MemberRef::Static(member),
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.record_member_write_target(
                    MemberRef::Computed(member),
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::PrivateFieldExpression(member) => {
                self.record_member_write_target(
                    MemberRef::Private(member),
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSAsExpression(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSSatisfiesExpression(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSNonNullExpression(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
            SimpleAssignmentTarget::TSTypeAssertion(inner) => {
                self.record_expression_write_target(
                    &inner.expression,
                    SkeletonWriteCertainty::Definite,
                    None,
                );
            }
        }
        if scoped {
            self.site_stack.pop();
        }
    }

    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
        let site = self.push_call(&it.callee, false, it.span.into());
        crate::analysis::type_eval_build::for_each_indexed_call_source_type_query(it, |query| {
            let span = self.frame_span(query.span.into());
            self.sites[site.index()]
                .source_type_queries
                .push(SkeletonSourceTypeQuery {
                    span,
                    binding: None,
                });
        });
        walk::walk_call_expression(self, it);
    }

    fn visit_new_expression(&mut self, it: &oxc_ast::ast::NewExpression<'a>) {
        self.push_call(&it.callee, true, it.span.into());
        walk::walk_new_expression(self, it);
    }
}

impl SkeletonBuilder<'_> {
    fn record_for_left(
        &mut self,
        left: &oxc_ast::ast::ForStatementLeft<'_>,
        source: SkeletonExprSiteId,
    ) {
        match left {
            oxc_ast::ast::ForStatementLeft::VariableDeclaration(declaration) => {
                let kind = skeleton_binding_kind(declaration.kind);
                for declarator in &declaration.declarations {
                    let mut identifiers = Vec::new();
                    let mut defaults = Vec::new();
                    collect_binding_pattern(&declarator.id, false, &mut identifiers, &mut defaults);
                    let pattern_sites = self.open_pattern_sites(defaults);
                    for (name, span, destructured) in identifiers {
                        let interned = self.intern(&name);
                        self.push_binding(&name, kind, span, None, destructured);
                        self.bindings.last_mut().unwrap().pattern_sites =
                            Arc::clone(&pattern_sites);
                        if !destructured {
                            let annotation_span = declarator
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.frame_span(annotation.span.into()));
                            self.bindings.last_mut().unwrap().annotation_span = annotation_span;
                        }
                        let region = self.current_region();
                        let span = self.frame_span(span);
                        self.writes.push(SkeletonWrite {
                            target: SkeletonWriteTarget::Named(interned),
                            target_span: Some(span),
                            binding: None,
                            path: Arc::from(Vec::new().into_boxed_slice()),
                            certainty: SkeletonWriteCertainty::Optional,
                            value: Some(source),
                            site: source,
                            region,
                            span,
                        });
                    }
                }
            }
            _ => {
                if let Some(target) = left.as_assignment_target() {
                    // Iteration writes attribute to the iteration source's
                    // site: evaluating the source drives the per-iteration
                    // write.
                    self.site_stack.push(source.index());
                    self.record_assignment_targets(
                        target,
                        SkeletonWriteCertainty::Optional,
                        false,
                        Some(source),
                    );
                    self.site_stack.pop();
                }
            }
        }
    }
}

/// Unwrap value-transparent carriers (parentheses, TS assertion / cast
/// carriers) for shape classification; the carriers stay part of the
/// site's span.
fn unwrap_expression_carriers<'a, 'ast>(expression: &'a Expression<'ast>) -> &'a Expression<'ast> {
    let mut current = expression;
    loop {
        current = match current {
            Expression::ParenthesizedExpression(inner) => &inner.expression,
            Expression::TSAsExpression(inner) => &inner.expression,
            Expression::TSSatisfiesExpression(inner) => &inner.expression,
            Expression::TSNonNullExpression(inner) => &inner.expression,
            Expression::TSTypeAssertion(inner) => &inner.expression,
            Expression::TSInstantiationExpression(inner) => &inner.expression,
            _ => return current,
        };
    }
}

/// One borrowed member-expression step of a write-target chain.
enum MemberRef<'a, 'ast> {
    Static(&'a oxc_ast::ast::StaticMemberExpression<'ast>),
    Computed(&'a oxc_ast::ast::ComputedMemberExpression<'ast>),
    Private(&'a oxc_ast::ast::PrivateFieldExpression<'ast>),
}

impl MemberRef<'_, '_> {
    fn span(&self) -> verter_span::Span {
        match self {
            MemberRef::Static(member) => member.span.into(),
            MemberRef::Computed(member) => member.span.into(),
            MemberRef::Private(member) => member.span.into(),
        }
    }
}

/// Collect the dotted callee path of `a.b.c()` (root first). `false` for
/// non-identifier roots.
fn collect_static_callee_path(
    member: &oxc_ast::ast::StaticMemberExpression<'_>,
    names: &mut Vec<String>,
) -> bool {
    let mut properties = Vec::new();
    let mut current = member;
    loop {
        properties.push(current.property.name.to_string());
        match &current.object {
            Expression::Identifier(identifier) => {
                names.push(identifier.name.to_string());
                break;
            }
            Expression::StaticMemberExpression(parent) => current = parent,
            _ => return false,
        }
    }
    properties.reverse();
    names.extend(properties);
    true
}

/// Collect every bound identifier (name, span) and every expression one
/// binding pattern evaluates at bind time — default values AND computed
/// keys, in evaluation order. A computed key is as much an evaluated
/// position as a default: a callable authored there is created and
/// retains its captures, so dropping it would hide it from the frame.
fn collect_binding_pattern<'a, 'ast>(
    pattern: &'a BindingPattern<'ast>,
    destructured: bool,
    identifiers: &mut Vec<(String, verter_span::Span, bool)>,
    defaults: &mut Vec<&'a Expression<'ast>>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => {
            identifiers.push((
                identifier.name.to_string(),
                identifier.span.into(),
                destructured,
            ));
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                if property.computed {
                    if let Some(key) = property.key.as_expression() {
                        defaults.push(key);
                    }
                }
                collect_binding_pattern(&property.value, true, identifiers, defaults);
            }
            if let Some(rest) = object.rest.as_ref() {
                collect_binding_pattern(&rest.argument, true, identifiers, defaults);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                collect_binding_pattern(element, true, identifiers, defaults);
            }
            if let Some(rest) = array.rest.as_ref() {
                collect_binding_pattern(&rest.argument, true, identifiers, defaults);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            // A default does not itself destructure: `f(a = 1)` binds `a`
            // as a plain identifier, `f({ a } = {})` inherits the
            // pattern's flag from the recursion above.
            defaults.push(&assignment.right);
            collect_binding_pattern(&assignment.left, destructured, identifiers, defaults);
        }
    }
}
