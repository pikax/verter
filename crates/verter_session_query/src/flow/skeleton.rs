//! The arena-free function-body skeleton: ids, regions, bindings, sites and the
//! skeleton carrier the flow graph is built from. Built by the parser front-end.

use crate::flow::binding::FlowBindingMap;
use crate::flow::binding::FlowBindingMapError;
use crate::flow::binding::FlowBindingRef;
use crate::flow::frame_span::FrameSpan;
use crate::flow::span_index::SkeletonSpanIndex;
use crate::function_program::FunctionProgramEntry;
use std::sync::Arc;
use verter_no_typeexpr::NoTypeExpr;

/// Interned identifier / property-key name within one skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowNameId(pub u32);

impl FlowNameId {
    /// Index into [`FunctionBodySkeleton::names`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// One entry of the lexical binding index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonBindingId(pub u32);

impl SkeletonBindingId {
    /// Index into [`FunctionBodySkeleton::bindings`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// One control region of the statement skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct SkeletonRegionId(pub u32);

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
pub struct SkeletonExprSiteId(pub u32);

impl SkeletonExprSiteId {
    /// Index into [`FunctionBodySkeleton::expr_sites`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: u32) -> Self {
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
pub struct SkeletonClosureId(pub u32);

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
pub struct SkeletonReturnSiteId(pub u32);

impl SkeletonReturnSiteId {
    /// Index into [`FunctionBodySkeleton::return_sites`].
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_index(index: u32) -> Self {
        Self(index)
    }
}

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
    /// Whether the declaration has the checker's EVOLVING-array form: an
    /// unannotated whole-identifier declarator initialised to an empty array
    /// literal (`const a = []`). Under `noImplicitAny` its type follows the
    /// operations that reach each read — `push` / `unshift` calls and
    /// element writes ([`evolving_array_mutation_root`],
    /// [`evolving_array_element_write_root`]) — so each records a write of
    /// the values it adds into the binding.
    pub evolving_array: bool,
    /// Whether the binding is a function declaration WITHOUT a body — an
    /// overload signature. A runtime variable with one is an overloaded
    /// function, called through its signatures, never its implementation
    /// (the checker's `getSignaturesOfSymbol`). Recorded here, in the one
    /// discovery pass, so a reader never walks the program to find the
    /// declaration's siblings.
    pub overload_signature: bool,
}

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
    /// An array literal: every element site (a spread's argument site for
    /// a spread element) is a whole-value input of this site's value,
    /// whatever part of the array is demanded.
    ArrayLiteral {
        /// The element sites, in authored order (elisions have none).
        elements: Arc<[SkeletonExprSiteId]>,
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
    /// The function's authored KIND (`async` / `generator` flags of the
    /// declaration or arrow itself — never of an enclosing form). The flow
    /// body's stable hash already folds the flags, so the fact is a property
    /// of the same content version the skeleton is memoized under.
    pub kind: FunctionBodyKind,
    /// The control regions; index 0 is the function-body root.
    pub regions: Arc<[SkeletonRegion]>,
    /// The lexical binding index.
    pub bindings: Arc<[SkeletonBinding]>,
    /// The tracked expression sites (parents precede their children).
    pub expr_sites: Arc<[SkeletonExprSite]>,
    /// The return-site index, in source order.
    pub return_sites: Arc<[SkeletonReturnSite]>,
    /// The argument site of every statement-position `yield x` (not
    /// `yield*`), in source order. A generator's yield type is the join of
    /// these values, so a whole-return demand is a demand for each of them
    /// as well as for the return sites.
    pub yield_sites: Arc<[SkeletonExprSiteId]>,
    /// The assignment / kill summary, in source order.
    pub writes: Arc<[SkeletonWrite]>,
    /// The bindings this frame declares that a nested callable ASSIGNS
    /// whole, at any depth ([`FunctionProgramEntry::descendant_assignments`]).
    /// With the whole-binding entries of [`Self::writes`] it is every
    /// assignment the checker's `isSymbolAssigned` reads; a closure that
    /// only reads a binding, or writes one of its members, adds nothing.
    pub closure_assignments: Arc<[SkeletonBindingId]>,
    /// The containment / read index over the tables above. Empty until
    /// [`prepare_function_body_skeleton`] builds it from the prepared
    /// tables; a consumer asking which sites, writes or bindings a span
    /// contains, or which reads follow it, asks this index.
    pub span_index: SkeletonSpanIndex,
    /// The name lookup over [`Self::names`] and [`Self::bindings`]:
    /// [`SkeletonNameIndex::build`] derives it from the two tables when
    /// the skeleton is assembled.
    pub name_index: SkeletonNameIndex,
}

/// Each interned name's id and the bindings that declare it, so a name
/// resolves by lookup rather than by a scan of the name or binding table.
#[derive(Debug, Clone, Default, PartialEq, Eq, NoTypeExpr)]
pub struct SkeletonNameIndex {
    ids: Arc<rustc_hash::FxHashMap<Arc<str>, FlowNameId>>,
    /// The bindings of each name, in declaration order: those of name `n`
    /// are `bindings[offsets[n]..offsets[n + 1]]`. A name interned after
    /// the index was built declares none.
    offsets: Arc<[u32]>,
    bindings: Arc<[SkeletonBindingId]>,
}

impl SkeletonNameIndex {
    /// Index `names` and the bindings declaring each.
    #[must_use]
    pub fn build(names: &[Arc<str>], bindings: &[SkeletonBinding]) -> Self {
        let ids = names
            .iter()
            .enumerate()
            .map(|(index, name)| (Arc::clone(name), FlowNameId(index as u32)))
            .collect();
        let mut offsets = vec![0u32; names.len() + 1];
        for binding in bindings {
            if let Some(count) = offsets.get_mut(binding.name.index() + 1) {
                *count += 1;
            }
        }
        for index in 1..offsets.len() {
            offsets[index] += offsets[index - 1];
        }
        let mut next = offsets.clone();
        let mut by_name = vec![SkeletonBindingId(0); offsets[names.len()] as usize];
        for (index, binding) in bindings.iter().enumerate() {
            if let Some(at) = next[..names.len()].get_mut(binding.name.index()) {
                by_name[*at as usize] = SkeletonBindingId::from_index(index as u32);
                *at += 1;
            }
        }
        Self {
            ids: Arc::new(ids),
            offsets: offsets.into(),
            bindings: by_name.into(),
        }
    }

    /// The id of `text`, when interned.
    #[must_use]
    pub fn id(&self, text: &str) -> Option<FlowNameId> {
        self.ids.get(text).copied()
    }

    /// The bindings declaring `name`, in declaration order.
    #[must_use]
    pub fn bindings_of(&self, name: FlowNameId) -> &[SkeletonBindingId] {
        match (
            self.offsets.get(name.index()),
            self.offsets.get(name.index() + 1),
        ) {
            (Some(start), Some(end)) => &self.bindings[*start as usize..*end as usize],
            _ => &[],
        }
    }

    /// What the index holds right now, read from its tables: the names it
    /// maps, the binding entries it lists, and the backing storage of its
    /// map (by capacity) and its two arrays. The interned names themselves
    /// are the skeleton's name table's, not counted here.
    #[must_use]
    pub fn occupancy(&self) -> SkeletonNameIndexOccupancy {
        SkeletonNameIndexOccupancy {
            names: self.ids.len(),
            bindings: self.bindings.len(),
            backing_bytes: self.ids.capacity() * std::mem::size_of::<(Arc<str>, FlowNameId)>()
                + std::mem::size_of_val(&*self.offsets)
                + std::mem::size_of_val(&*self.bindings),
        }
    }

    /// An identity of the index's storage, equal for two clones sharing it
    /// while either is alive, so a reader summing the indexes it retains
    /// counts each once.
    #[must_use]
    pub fn storage_identity(&self) -> usize {
        Arc::as_ptr(&self.ids).cast::<()>() as usize
    }

    /// Record a name interned after the index was built.
    fn insert(&mut self, text: Arc<str>, id: FlowNameId) {
        Arc::make_mut(&mut self.ids).insert(text, id);
    }
}

/// What one [`SkeletonNameIndex`] holds: a production occupancy count,
/// available in every build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkeletonNameIndexOccupancy {
    /// Names the index maps to their ids.
    pub names: usize,
    /// Binding entries listed under their names.
    pub bindings: usize,
    /// Backing storage of the name map (by capacity) and the offset and
    /// binding arrays, in bytes.
    pub backing_bytes: usize,
}

impl SkeletonNameIndexOccupancy {
    /// Add `other`'s counts to these: the occupancy of two distinct
    /// indexes.
    pub fn accumulate(&mut self, other: &Self) {
        self.names += other.names;
        self.bindings += other.bindings;
        self.backing_bytes += other.backing_bytes;
    }
}

/// The authored kind of one function body — the `async` and `generator`
/// flags the language's return-type rule keys on. A plain function or
/// arrow is [`Self::Plain`]; an arrow can never be a generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum FunctionBodyKind {
    Plain,
    Async,
    Generator,
    AsyncGenerator,
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
        self.name_index.id(text)
    }

    /// Every binding of `name` in this frame, in declaration order.
    pub fn bindings_named(&self, name: FlowNameId) -> impl Iterator<Item = SkeletonBindingId> + '_ {
        self.name_index.bindings_of(name).iter().copied()
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
        let named = self.name_index.bindings_of(name);
        let mut current = Some(region);
        while let Some(enclosing) = current {
            let mut hits: Vec<SkeletonBindingId> = Vec::new();
            for &id in named {
                let binding = self.binding(id);
                if binding.region == enclosing && binding.kind.declares_value() {
                    hits.push(id);
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
        let named = self.name_index.bindings_of(name);
        let mut current = Some(region);
        while let Some(enclosing) = current {
            if named.iter().any(|id| {
                let binding = self.binding(*id);
                binding.region == enclosing && binding.kind.declares(meaning)
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
        self.name_index
            .bindings_of(name)
            .iter()
            .copied()
            .filter(|id| {
                let binding = self.binding(*id);
                match binding.kind {
                    SkeletonBindingKind::Var => true,
                    SkeletonBindingKind::NestedFunction => {
                        self.regions[binding.region.index()].parent.is_none()
                    }
                    _ => false,
                }
            })
            .collect()
    }
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

/// Resolve the authored access occurrences against one exact indexed inventory.
/// The map is returned with the skeleton so the graph bundle can publish both.
pub fn prepare_function_body_skeleton(
    mut skeleton: FunctionBodySkeleton,
    entry: &FunctionProgramEntry,
) -> Result<PreparedFunctionBodySkeleton, FlowBindingMapError> {
    let mut bindings =
        FlowBindingMap::build(&skeleton, entry.bindings(), entry.key(), entry.span().start)?;
    bindings.prepare_occurrences(entry)?;
    skeleton.closure_assignments = entry
        .descendant_assignments()
        .iter()
        .filter_map(|identity| bindings.local(identity))
        .collect::<Vec<_>>()
        .into();
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
    attach_declaration_closures(&mut skeleton, &bindings, entry)?;
    skeleton.span_index = SkeletonSpanIndex::build(&skeleton, &bindings);
    Ok(PreparedFunctionBodySkeleton { skeleton, bindings })
}

/// A local function DECLARATION is hoisted: its value is created at its
/// frame's entry, not at an expression site, and is read wherever its name
/// is. Each site that reads (or calls) such a declaration of this frame
/// therefore retains the declaration's captures exactly as a site creating
/// the callable would — its own [`SkeletonClosure`], its capture subjects
/// and its captured reads — so demanding the read selects what the
/// declaration's body reads from this frame.
fn attach_declaration_closures(
    skeleton: &mut FunctionBodySkeleton,
    bindings: &FlowBindingMap,
    entry: &FunctionProgramEntry,
) -> Result<(), FlowBindingMapError> {
    let anchor = entry.span().start;
    let declaration_of = |binding: &FlowBindingRef| -> Option<SkeletonBindingId> {
        let FlowBindingRef::Local(local) = binding else {
            return None;
        };
        (skeleton.bindings[local.index()].kind == SkeletonBindingKind::NestedFunction)
            .then_some(*local)
    };
    let mut attachments: Vec<(usize, SkeletonBindingId)> = Vec::new();
    for (index, site) in skeleton.expr_sites.iter().enumerate() {
        let mut seen: rustc_hash::FxHashSet<SkeletonBindingId> = rustc_hash::FxHashSet::default();
        let referenced = site
            .reads
            .iter()
            .filter_map(|read| read.binding.as_ref())
            .chain(site.calls.iter().filter_map(|call| call.binding.as_ref()));
        for binding in referenced {
            if let Some(local) = declaration_of(binding) {
                if seen.insert(local) {
                    attachments.push((index, local));
                }
            }
        }
    }
    if attachments.is_empty() {
        return Ok(());
    }
    // The nested callables, by start: siblings never overlap, so the one
    // holding a declared name is the last starting at or before it.
    let mut nested: Vec<_> = entry.nested_captures().collect();
    nested.sort_by_key(|child| child.span().start);
    let mut names = SkeletonNameTable::of(skeleton);
    // Each declaration's closure, built once however many sites read it.
    let mut closures: rustc_hash::FxHashMap<SkeletonBindingId, Option<DeclarationClosure>> =
        rustc_hash::FxHashMap::default();
    let mut sites = skeleton.expr_sites.to_vec();
    let mut attachments = attachments.into_iter().peekable();
    while let Some(&(index, _)) = attachments.peek() {
        let site = &mut sites[index];
        let mut capture_bindings = site.capture_bindings.to_vec();
        let mut bound: rustc_hash::FxHashSet<FlowBindingRef> =
            capture_bindings.iter().cloned().collect();
        let mut captures = site.captures.to_vec();
        let mut captured: rustc_hash::FxHashSet<FlowNameId> = captures.iter().copied().collect();
        let mut site_closures = site.closures.to_vec();
        let mut reads = site.reads.to_vec();
        while let Some((_, local)) = attachments.next_if(|(at, _)| *at == index) {
            let closure = match closures.entry(local) {
                std::collections::hash_map::Entry::Occupied(built) => built.into_mut(),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    // The declaration's own capture record: the nested
                    // callable whose span holds the declared name.
                    let name = skeleton.bindings[local.index()].span.to_absolute(anchor);
                    let holder = nested
                        .partition_point(|child| child.span().start <= name.start)
                        .checked_sub(1)
                        .map(|at| nested[at])
                        .filter(|child| child.span().end >= name.end);
                    slot.insert(match holder {
                        Some(captures) => Some(DeclarationClosure::build(
                            captures, bindings, &mut names, anchor,
                        )?),
                        None => None,
                    })
                }
            };
            let Some(closure) = closure else {
                continue;
            };
            for binding in closure.closure.captures.iter() {
                if bound.insert(binding.clone()) {
                    capture_bindings.push(binding.clone());
                }
            }
            for name in &closure.names {
                if captured.insert(*name) {
                    captures.push(*name);
                }
            }
            site_closures.push(closure.closure.clone());
            reads.extend(closure.reads.iter().cloned());
        }
        site.capture_bindings = capture_bindings.into();
        site.captures = captures.into();
        site.closures = site_closures.into();
        site.reads = reads.into();
    }
    skeleton.expr_sites = sites.into();
    names.store(skeleton);
    Ok(())
}

/// What one hoisted local function declaration attaches to each site that
/// reads it.
struct DeclarationClosure {
    closure: SkeletonClosure,
    /// The interned names of its captures.
    names: Vec<FlowNameId>,
    reads: Vec<SkeletonRead>,
}

impl DeclarationClosure {
    fn build(
        captures: crate::function_program::FunctionCaptures<'_>,
        bindings: &FlowBindingMap,
        names: &mut SkeletonNameTable,
        anchor: u32,
    ) -> Result<Self, FlowBindingMapError> {
        let mut own: Vec<FlowBindingRef> = Vec::new();
        let mut own_seen = rustc_hash::FxHashSet::default();
        for identity in captures.bindings() {
            let binding = bindings.resolve_identity(identity)?;
            if own_seen.insert(binding.clone()) {
                own.push(binding);
            }
        }
        let mut own_reads: Vec<FlowBindingRef> = Vec::new();
        let mut own_read_seen = rustc_hash::FxHashSet::default();
        let mut reads: Vec<SkeletonRead> = Vec::new();
        for read in captures.reads() {
            let binding = bindings.resolve_identity(&read.binding)?;
            if own_read_seen.insert(binding.clone()) {
                own_reads.push(binding.clone());
            }
            let name = names.intern(&read.binding.name);
            let path: Arc<[SkeletonPathSegment]> = read
                .path
                .iter()
                .map(|segment| SkeletonPathSegment::Static(names.intern(segment)))
                .collect::<Vec<_>>()
                .into();
            reads.push(SkeletonRead {
                name,
                path,
                span: FrameSpan::rebase(anchor, read.span),
                binding: Some(binding),
                kind: FlowReadKind::Input,
            });
        }
        let capture_names = captures
            .bindings()
            .map(|identity| names.intern(&identity.name))
            .collect();
        Ok(Self {
            closure: SkeletonClosure {
                span: FrameSpan::rebase(anchor, captures.span()),
                correlation: if captures.exhaustive() {
                    SkeletonClosureCorrelation::Exact
                } else {
                    SkeletonClosureCorrelation::Partial
                },
                captures: Arc::from(own.into_boxed_slice()),
                read_captures: Arc::from(own_reads.into_boxed_slice()),
            },
            names: capture_names,
            reads,
        })
    }
}

/// A skeleton's name table open for interning: the names grow in place and
/// are stored back once.
struct SkeletonNameTable {
    names: Vec<Arc<str>>,
    index: SkeletonNameIndex,
}

impl SkeletonNameTable {
    fn of(skeleton: &FunctionBodySkeleton) -> Self {
        Self {
            names: skeleton.names.to_vec(),
            index: skeleton.name_index.clone(),
        }
    }

    /// The id of `text`, interning it when new.
    fn intern(&mut self, text: &str) -> FlowNameId {
        if let Some(id) = self.index.id(text) {
            return id;
        }
        let id = FlowNameId(u32::try_from(self.names.len()).unwrap_or(u32::MAX));
        let text: Arc<str> = Arc::from(text);
        self.names.push(Arc::clone(&text));
        self.index.insert(text, id);
        id
    }

    fn store(self, skeleton: &mut FunctionBodySkeleton) {
        if self.names.len() != skeleton.names.len() {
            skeleton.names = self.names.into();
            skeleton.name_index = self.index;
        }
    }
}
