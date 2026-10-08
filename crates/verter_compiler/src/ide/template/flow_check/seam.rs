//! Typed input/output seam of the flow-transparent callback check.
//!
//! The production IDE emitter (or a test builder) resolves a template into a
//! [`CheckPlan`]; [`super::generator::generate`] lowers that plan, through one
//! `CodeTransform` over the authored template source, into a [`GeneratedCheck`].
//!
//! Everything here is plain data. The seam names no parser, scope, resolver or
//! projection type, so the generator that consumes it compiles against
//! `CodeTransform` alone (see the compile boundary in
//! `tests/cases/flow_check_boundary.rs`).

use std::fmt;
use std::ops::Range;

/// Half-open authored byte range `[start, end)` in the template source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub const fn contains(self, inner: Span) -> bool {
        self.start <= inner.start && inner.end <= self.end
    }
}

/// Identity of one template lexical scope frame.
///
/// A builder mints exactly one key per template lexical scope handle, so two
/// items share a key exactly when they share a lexical scope. The key carries
/// no names: the authored alias and parameter patterns are emitted verbatim
/// and JavaScript block scoping then mirrors the template scope chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeKey(pub u32);

/// Identity of one conditional branch (the element carrying the directive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BranchKey(pub u32);

/// One run of a resolved expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Authored bytes, emitted in place of nothing else and mapped 1:1.
    Authored(Span),
    /// Resolver text (an accessor prefix such as `__props.`); never mapped.
    Synthetic(String),
}

/// A resolved authored expression: its authored runs in source order, which
/// together cover `span` exactly, with the resolver's synthetic accessor text
/// between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedExpr {
    pub span: Span,
    pub pieces: Vec<Piece>,
}

/// The whole input of one generated check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckPlan {
    /// Declarations the check is typed against (component context, props
    /// type, setup bindings). Emitted verbatim and unmapped.
    pub declarations: String,
    /// Parameter list (with optional type parameters) of the generated check
    /// function, e.g. `<T>(__props: Props<T>)`. Emitted verbatim and unmapped.
    pub parameters: String,
    /// Scope of the template root.
    pub root: ScopeKey,
    pub items: Vec<Item>,
}

/// One checked construct, in template order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Chain(Chain),
    Frame(Frame),
    Callback(Callback),
    Expr(ExprItem),
}

/// One `v-if` / `v-else-if` / `v-else` chain. Branches are in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub branches: Vec<Branch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchKind {
    If,
    ElseIf,
    Else,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub key: BranchKey,
    pub kind: BranchKind,
    /// The branch this one follows. `None` exactly for the opening `v-if`;
    /// every later branch names the branch immediately before it.
    pub predecessor: Option<BranchKey>,
    /// The authored condition; `None` exactly for `v-else`.
    pub condition: Option<ResolvedExpr>,
    /// Lexical scope the condition resolves in (the scope enclosing the chain).
    pub scope: ScopeKey,
    pub items: Vec<Item>,
}

/// A lexical scope a `v-for` or `v-slot` opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub scope: ScopeKey,
    /// The scope this frame is nested in; must be the enclosing frame's scope.
    pub parent: ScopeKey,
    pub binding: FrameBinding,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameBinding {
    /// `v-for="<aliases> in <source>"`. `aliases` is the alias list without
    /// its enclosing parentheses; `arity` counts its top-level aliases.
    VFor {
        aliases: Span,
        arity: u8,
        source: ResolvedExpr,
    },
    /// `v-slot="<pattern>"`, typed by the slot function `contract`.
    Slot { pattern: Span, contract: String },
}

/// A callback whose body must be checked under the narrowing of its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub scope: ScopeKey,
    /// The function type the callback is contextually typed by.
    pub contract: String,
    pub function: CallbackFunction,
    /// Outer references the body reads in its own flow (not inside a nested
    /// function), each with every authored prefix listed before it.
    pub outer_refs: Vec<OuterRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackFunction {
    /// An arrow or function expression the author wrote.
    Authored {
        expr: ResolvedExpr,
        guard: GuardSite,
    },
    /// A handler the emitter wraps around an authored inline statement:
    /// `(<parameters>) => <body>`.
    Handler {
        parameters: String,
        body: ResolvedExpr,
    },
}

/// Where the re-narrowing guard goes inside an authored function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardSite {
    /// Block body: immediately after its opening `{`.
    Block { after: u32 },
    /// Expression body: immediately before the body expression.
    Expression { before: u32 },
}

/// One reference chain (`__props.user.name`) read inside a callback body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OuterRef {
    /// Resolved text with a non-null assertion before each non-optional step,
    /// so the synthetic copies never repeat a diagnostic the authored chain
    /// already reports.
    pub text: String,
    /// One authored occurrence of the chain inside the callback.
    pub occurrence: Span,
}

/// An authored value checked in place (interpolation, bound value, handler
/// reference). With a contract it is checked against that type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprItem {
    pub scope: ScopeKey,
    pub contract: Option<String>,
    pub expr: ResolvedExpr,
}

/// The generated check and its mapping.
#[derive(Debug, Clone)]
pub struct GeneratedCheck {
    pub code: String,
    /// Authored provenance of every generated byte range that has one.
    /// Synthetic scaffolding has none.
    pub mappings: Vec<Mapping>,
    pub work: GenerationWork,
    pub layout: Layout,
}

/// One generated range carrying authored bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    pub generated: Span,
    pub source: Span,
}

impl GeneratedCheck {
    /// Authored byte offset of generated byte `offset`, if it carries one.
    pub fn source_offset(&self, offset: u32) -> Option<u32> {
        let index = self.mappings.partition_point(|m| m.generated.end <= offset);
        let mapping = self.mappings.get(index)?;
        (mapping.generated.start <= offset && offset < mapping.generated.end)
            .then(|| mapping.source.start + (offset - mapping.generated.start))
    }

    /// Generated byte offsets carrying authored byte `offset`, in output order.
    pub fn generated_offsets(&self, offset: u32) -> Vec<u32> {
        self.mappings
            .iter()
            .filter(|m| m.source.start <= offset && offset < m.source.end)
            .map(|m| m.generated.start + (offset - m.source.start))
            .collect()
    }

    /// The authored bytes a generated range carries, when they form one
    /// contiguous authored run (synthetic bytes inside the range are skipped:
    /// a diagnostic on `__props.user` maps to the authored `user`). `None` when
    /// the range carries no authored byte or two discontiguous runs.
    pub fn authored_extent(&self, generated: Range<u32>) -> Option<Span> {
        let first = self
            .mappings
            .partition_point(|m| m.generated.end <= generated.start);
        let mut extent: Option<Span> = None;
        for mapping in &self.mappings[first..] {
            if mapping.generated.start >= generated.end {
                break;
            }
            let start = generated.start.max(mapping.generated.start);
            let end = generated.end.min(mapping.generated.end);
            let piece = Span::new(
                mapping.source.start + (start - mapping.generated.start),
                mapping.source.start + (end - mapping.generated.start),
            );
            extent = Some(match extent {
                None => piece,
                Some(run) if run.end == piece.start => Span::new(run.start, piece.end),
                Some(_) => return None,
            });
        }
        extent
    }
}

/// Deterministic construction work, counted where it is performed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GenerationWork {
    /// Authored conditions emitted (each exactly once).
    pub conditions: u64,
    /// Branch predecessor links read.
    pub predecessor_links: u64,
    /// Scope links read (item, branch and frame scope checks).
    pub scope_links: u64,
    /// Callbacks emitted.
    pub callbacks: u64,
    /// Outer reference re-narrowings emitted.
    pub outer_refs: u64,
    /// `CodeTransform` operations issued.
    pub transform_ops: u64,
    /// Path terms copied into a nested branch (`GuardStrategy::ReplayPath` only).
    pub replayed_terms: u64,
}

/// Byte accounting of the generated check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    /// The fixed helper declarations.
    pub helper_bytes: usize,
    /// The plan's declarations and the check function header and footer.
    pub declaration_bytes: usize,
    /// Every byte of the generated check.
    pub total_bytes: usize,
    /// Authored bytes carried into the check.
    pub authored_bytes: usize,
    /// One entry per emitted callback, in output order.
    pub callbacks: Vec<CallbackSite>,
}

/// Where one callback landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackSite {
    /// Authored span of the function (or the handler body).
    pub authored: Span,
    /// Generated range of the whole checked callback statement.
    pub generated: Span,
    /// Outer references re-narrowed inside it.
    pub outer_refs: usize,
}

/// A plan the generator refuses. Each variant names the input at fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    EmptyChain,
    ChainMustOpenWithIf {
        branch: BranchKey,
    },
    MissingCondition {
        branch: BranchKey,
    },
    ElseWithCondition {
        branch: BranchKey,
    },
    ElseNotLast {
        branch: BranchKey,
    },
    PredecessorMismatch {
        branch: BranchKey,
        expected: Option<BranchKey>,
        found: Option<BranchKey>,
    },
    ScopeMismatch {
        expected: ScopeKey,
        found: ScopeKey,
    },
    FrameParentMismatch {
        frame: ScopeKey,
        expected: ScopeKey,
        found: ScopeKey,
    },
    BadVForArity {
        arity: u8,
    },
    MissingContract {
        at: Span,
    },
    SpanOutsideSource {
        span: Span,
    },
    PiecesDoNotCoverSpan {
        span: Span,
    },
    AuthoredOverlap {
        first: Span,
        second: Span,
    },
    GuardSiteOutsideFunction {
        function: Span,
        site: u32,
    },
    OuterRefOutsideCallback {
        function: Span,
        occurrence: Span,
    },
    EmptyOuterRef {
        occurrence: Span,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl std::error::Error for PlanError {}
