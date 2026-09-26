//! `OwnedEvalProgram` — owned, `Send + Sync + 'static` lowered IR for the
//! script body of a Vue SFC (or non-SFC dependency).
//!
//! ## Lowering-boundary contract
//!
//! `OwnedEvalProgram` is a post-parse, post-lowering owned representation
//! of a script body. The borrowed `oxc_parser::Parser` arena is dropped
//! at the lowering boundary; after construction, no field of
//! `OwnedEvalProgram` retains any pointer into the OXC allocator. The
//! struct is `Clone + Send + Sync + 'static` so it can sit in host-owned
//! typed DBs without thread-local workarounds. Production does not build
//! or cache `OwnedEvalProgram` values yet — the live per-file `EvalEnv`
//! lives on `IndexedReady`, and the transient `ParsedEvalProgram` is
//! threaded by reference within a cold flight; this type and its tests
//! pin the arena-free lowering contract.
//!
//! Identifier and literal payloads are interned through compact
//! `InternedIdentifierTable` / `InternedLiteralTable` arenas (deduplicated
//! by content). Statements and expressions are stored in flat `Arc<[…]>`
//! slabs and reference each other by `InternedExpressionId` indices.
//!
//! The macro-impact inventory baseline lives at
//! `crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md`.
//! The `LoweringError` variants align one-to-one with the
//! "FAIL on Unsupported" rows of that inventory; "Diagnostic-only on
//! Unsupported" rows lower to `LoweredStmt::Unsupported` / `LoweredExpr::Unsupported`
//! with no `LoweringError`.
//!
//! Consumer-visible contract: when lowering produces a
//! `LoweringError`, the parse-stage diagnostic is recorded on the
//! payload's `macro_expansion_diagnostics`. NAPI does NOT throw
//! exceptions for macro failures — they are surfaced as structured
//! payload entries.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

#[cfg(test)]
#[path = "eval_program_tests.rs"]
mod eval_program_tests;

// ─────────────────────────────────────────────────────────────────────
// Interned identifier / literal arenas
// ─────────────────────────────────────────────────────────────────────

/// Index into [`InternedIdentifierTable`]. Distinct from byte-spans —
/// equal `InternedIdentifierId` values mean two occurrences refer to the
/// same identifier text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InternedIdentifierId(pub u32);

/// Index into [`InternedLiteralTable`]. Equal `InternedLiteralId` values
/// mean two occurrences carry the same literal payload (same text bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InternedLiteralId(pub u32);

/// Index into the flat [`OwnedEvalProgram::expressions`] slab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InternedExpressionId(pub u32);

/// Owned identifier intern table. Interns identifier source text into
/// `Arc<str>`-backed entries deduplicated by content. Empty tables are
/// `Send + Sync + 'static`.
#[derive(Debug, Clone, Default)]
pub struct InternedIdentifierTable {
    entries: Vec<Arc<str>>,
}

impl InternedIdentifierTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern an identifier by content. Returns the existing id when the
    /// text is already present; otherwise pushes a new entry.
    pub fn intern(&mut self, text: &str) -> InternedIdentifierId {
        if let Some((idx, _)) = self
            .entries
            .iter()
            .enumerate()
            .find(|(_, existing)| existing.as_ref() == text)
        {
            return InternedIdentifierId(idx as u32);
        }
        let id = InternedIdentifierId(self.entries.len() as u32);
        self.entries.push(Arc::from(text));
        id
    }

    /// Resolve an interned id back to its source text. Returns `None`
    /// when the id is out of range (defensive — production callers
    /// have a valid id by construction).
    #[must_use]
    pub fn lookup(&self, id: InternedIdentifierId) -> Option<&str> {
        self.entries.get(id.0 as usize).map(|s| s.as_ref())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Owned literal intern table. Distinguishes between string / numeric /
/// boolean / null payloads via [`LiteralKind`]. Equal `(kind, raw_text)`
/// pairs deduplicate to the same id.
#[derive(Debug, Clone, Default)]
pub struct InternedLiteralTable {
    entries: Vec<InternedLiteralEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InternedLiteralEntry {
    pub kind: LiteralKind,
    pub raw_text: Arc<str>,
}

/// Discriminator for [`InternedLiteralEntry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiteralKind {
    String,
    Number,
    Boolean,
    Null,
    /// Template literal *quasi* fragment (raw text between expressions).
    /// Distinct from `String` because template fragments are not
    /// JS-quoted.
    TemplateFragment,
}

impl InternedLiteralTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, kind: LiteralKind, raw_text: &str) -> InternedLiteralId {
        if let Some((idx, _)) =
            self.entries.iter().enumerate().find(|(_, existing)| {
                existing.kind == kind && existing.raw_text.as_ref() == raw_text
            })
        {
            return InternedLiteralId(idx as u32);
        }
        let id = InternedLiteralId(self.entries.len() as u32);
        self.entries.push(InternedLiteralEntry {
            kind,
            raw_text: Arc::from(raw_text),
        });
        id
    }

    #[must_use]
    pub fn lookup(&self, id: InternedLiteralId) -> Option<&InternedLiteralEntry> {
        self.entries.get(id.0 as usize)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Span identity (compact representation; offsets only)
// ─────────────────────────────────────────────────────────────────────

/// Compact span identity for lowered nodes. Equivalent to OXC's `Span`
/// but `Send + Sync + 'static` (no allocator-lifetime bound). Encoded as
/// `(start, end)` byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SpanId {
    pub start: u32,
    pub end: u32,
}

impl SpanId {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Lowered statement / expression
// ─────────────────────────────────────────────────────────────────────

/// Variable / declaration kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclKind {
    Const,
    Let,
    Var,
    Function,
    Class,
    TypeAlias,
    Interface,
    Enum,
    /// Imported binding (resolved through a separate import statement).
    Import,
}

/// Binary operator kind for [`LoweredExpr::Binary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    NotEq,
    StrictEq,
    StrictNotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    LogicalAnd,
    LogicalOr,
    NullishCoalesce,
    BitAnd,
    BitOr,
    BitXor,
    LeftShift,
    RightShift,
    UnsignedRightShift,
    InstanceOf,
    In,
}

/// Unary operator kind for [`LoweredExpr::Unary`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Negate,
    Plus,
    Not,
    BitNot,
    TypeOf,
    Void,
    Delete,
}

/// Macro-impact-classified description of an unsupported AST kind. When
/// the construct is *macro-impacting*, lowering returns
/// [`LoweringError`] instead of a `Lowered*::Unsupported` variant
/// (D107). When non-macro-impacting, the `Unsupported` variant is
/// emitted with `kind` populated for diagnostics; the program continues
/// to lower.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UnsupportedKind {
    /// `if`/`for`/`while`/`try`/`switch`/`label`/`block`/`debugger`/`return`/
    /// `break`/`continue` at module top — non-macro-impacting per
    /// `eval_program_macro_impact_inventory.md`.
    NonMacroImpactingTopLevelControlFlow(&'static str),
    /// Empty statement (`;`).
    EmptyStatement,
    /// `throw` at top level.
    ThrowStatement,
    /// Property key shapes the resolver currently silent-skips and the
    /// inventory categorizes as Diagnostic-only (`PropertyKey::NumericLiteral`,
    /// `PropertyKey::PrivateIdentifier`).
    DiagnosticOnlyPropertyKey(&'static str),
    /// `unique` operator (`unique symbol`) — Diagnostic-only per
    /// inventory.
    UniqueTypeOperator,
    /// `TSTypePredicate` at non-meaningful position — Diagnostic-only.
    TypePredicate,
    /// Other (free-form description).
    Other(&'static str),
}

/// Lowered top-level statement.
///
/// `Unsupported` is emitted ONLY for non-macro-impacting kinds. Macro-
/// impacting unsupported constructs (e.g., `defineProps` argument is a
/// `ConditionalExpression`) abort lowering with a [`LoweringError`].
#[derive(Debug, Clone)]
pub enum LoweredStmt {
    /// `import { … } from "…"` — value or type-only.
    Import {
        specifier: InternedIdentifierId,
        names: SmallVec<[InternedIdentifierId; 4]>,
    },
    /// `export { name }` or `export … from "…"`.
    Export {
        name: InternedIdentifierId,
        source: Option<InternedIdentifierId>,
    },
    /// Variable / type / function / class / enum declaration.
    Declaration {
        name: InternedIdentifierId,
        kind: DeclKind,
        init: Option<InternedExpressionId>,
    },
    /// Top-level assignment expression.
    Assignment {
        target: InternedIdentifierId,
        value: InternedExpressionId,
    },
    /// Bare top-level return.
    Return { value: Option<InternedExpressionId> },
    /// Top-level `if` (sub-statements stored as expression-id blocks for
    /// uniform indexing).
    If {
        test: InternedExpressionId,
        consequent: SmallVec<[u32; 8]>,
        alternate: SmallVec<[u32; 4]>,
    },
    /// Diagnostic-only unsupported construct (no abort).
    Unsupported { kind: UnsupportedKind, span: SpanId },
}

/// Lowered expression.
#[derive(Debug, Clone)]
pub enum LoweredExpr {
    Identifier(InternedIdentifierId),
    Literal(InternedLiteralId),
    Call {
        callee: InternedExpressionId,
        args: SmallVec<[InternedExpressionId; 4]>,
    },
    Member {
        object: InternedExpressionId,
        property: InternedIdentifierId,
    },
    Binary {
        op: BinaryOp,
        left: InternedExpressionId,
        right: InternedExpressionId,
    },
    Unary {
        op: UnaryOp,
        operand: InternedExpressionId,
    },
    Object {
        properties: SmallVec<[(InternedIdentifierId, InternedExpressionId); 4]>,
    },
    Array {
        elements: SmallVec<[InternedExpressionId; 4]>,
    },
    /// Template literal — interleaves quasis (raw fragments) with
    /// expression placeholders. `quasis.len() == exprs.len() + 1` per
    /// JS template-literal grammar.
    TemplateLiteral {
        quasis: SmallVec<[InternedLiteralId; 4]>,
        exprs: SmallVec<[InternedExpressionId; 4]>,
    },
    /// Diagnostic-only unsupported expression — emitted alongside a
    /// `LoweredStmt::Unsupported` containing the parent diagnostic.
    Unsupported {
        kind: UnsupportedKind,
        span: SpanId,
    },
}

// ─────────────────────────────────────────────────────────────────────
// Lowering diagnostic + error
// ─────────────────────────────────────────────────────────────────────

/// Non-fatal lowering diagnostic — produced for `Unsupported` rows. Used
/// by the host-side parse pipeline to populate
/// `macro_expansion_diagnostics`. Distinct from [`LoweringError`], which
/// aborts lowering.
#[derive(Debug, Clone)]
pub struct LoweringDiagnostic {
    pub kind: UnsupportedKind,
    pub span: SpanId,
    pub message: Arc<str>,
}

/// Macro-impacting lowering failure. Each variant aligns one-to-one
/// with a "FAIL on Unsupported" row in
/// `eval_program_macro_impact_inventory.md`.
///
/// Contract: a lowering driver converts this error to a structured
/// payload diagnostic at the parse stage. The downstream
/// `getComponentMeta` API still returns `Option<ComponentMetaPayload>`;
/// macro failures surface through the payload's
/// `macro_expansion_diagnostics` field. Production lowering does not
/// construct `LoweringError` yet; the structural contract is pinned by
/// `macro_impacting_constructs_fail_lowering_not_silent_skip`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringError {
    /// Macro argument is an unsupported expression shape (e.g.,
    /// `ConditionalExpression`, `SequenceExpression`, `AwaitExpression`,
    /// `YieldExpression`, `SpreadElement`, `ComputedMemberExpression` as
    /// property key, `TemplateLiteral` as property key, `Computed*` as
    /// property key).
    UnsupportedMacroArgumentShape {
        macro_name: Arc<str>,
        span: SpanId,
        kind: UnsupportedKind,
    },
    /// `import` shape that the resolver cannot classify (currently
    /// unused — every shape is recognized — but reserved per inventory's
    /// "no FAIL rows" disclosure for stability with D117).
    UnsupportedTopLevelImport { specifier: Arc<str>, span: SpanId },
    /// Macro-relevant TS construct in a type position the resolver
    /// rejects (e.g., `TSConstructorType` in `defineEmits<T>`,
    /// `TSInferType` outside `TSConditionalType.extendsType`).
    UnsupportedMacroRelevantConstruct { construct: Arc<str>, span: SpanId },
}

impl std::fmt::Display for LoweringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoweringError::UnsupportedMacroArgumentShape {
                macro_name,
                span,
                kind,
            } => write!(
                f,
                "macro `{}` received unsupported argument shape ({:?}) at {}..{}",
                macro_name, kind, span.start, span.end
            ),
            LoweringError::UnsupportedTopLevelImport { specifier, span } => write!(
                f,
                "unsupported top-level import `{}` at {}..{}",
                specifier, span.start, span.end
            ),
            LoweringError::UnsupportedMacroRelevantConstruct { construct, span } => write!(
                f,
                "unsupported macro-relevant TS construct `{}` at {}..{}",
                construct, span.start, span.end
            ),
        }
    }
}

impl std::error::Error for LoweringError {}

// ─────────────────────────────────────────────────────────────────────
// Owned eval program top-level shape
// ─────────────────────────────────────────────────────────────────────

/// Owned, lowered representation of a parsed script body. `Send + Sync +
/// 'static` — safe for host-owned caches. Currently constructed only by
/// tests; production parses eval programs once per cold materialise and
/// threads the transient `ParsedEvalProgram` by reference instead of
/// caching an owned program.
#[derive(Debug, Clone)]
pub struct OwnedEvalProgram {
    /// Top-level statements in source order.
    pub statements: Arc<[LoweredStmt]>,
    /// Identifier intern table. Shared via `Arc` so multiple programs
    /// can share interned slabs when content matches; per-program
    /// instances are still legal.
    pub identifiers: Arc<InternedIdentifierTable>,
    /// Literal intern table. Same `Arc` sharing semantics as
    /// `identifiers`.
    pub literals: Arc<InternedLiteralTable>,
    /// Flat expression slab; statement nodes reference by
    /// [`InternedExpressionId`].
    pub expressions: Arc<[LoweredExpr]>,
    /// Per-import set of identifiers brought in, available so an
    /// eval-env builder can seed import-symbol facts without
    /// rescanning.
    pub import_symbols: FxHashMap<Arc<str>, FxHashSet<InternedIdentifierId>>,
    /// Non-fatal lowering diagnostics — populated for `Unsupported`
    /// rows. Macro-impacting failures abort lowering with
    /// [`LoweringError`] instead.
    pub lowering_diagnostics: Vec<LoweringDiagnostic>,
}

impl OwnedEvalProgram {
    /// Construct an empty program (no statements, fresh intern tables).
    /// Used as a sentinel by callers that need a `Send + Sync +
    /// 'static` value when parsing failed and a real program is
    /// unavailable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            statements: Arc::from(Vec::<LoweredStmt>::new()),
            identifiers: Arc::new(InternedIdentifierTable::new()),
            literals: Arc::new(InternedLiteralTable::new()),
            expressions: Arc::from(Vec::<LoweredExpr>::new()),
            import_symbols: FxHashMap::default(),
            lowering_diagnostics: Vec::new(),
        }
    }

    /// Build from owned slabs. Currently exercised only by tests; no
    /// production lowering path constructs owned programs yet.
    #[must_use]
    pub fn from_parts(
        statements: Vec<LoweredStmt>,
        identifiers: InternedIdentifierTable,
        literals: InternedLiteralTable,
        expressions: Vec<LoweredExpr>,
        import_symbols: FxHashMap<Arc<str>, FxHashSet<InternedIdentifierId>>,
        lowering_diagnostics: Vec<LoweringDiagnostic>,
    ) -> Self {
        Self {
            statements: Arc::from(statements),
            identifiers: Arc::new(identifiers),
            literals: Arc::new(literals),
            expressions: Arc::from(expressions),
            import_symbols,
            lowering_diagnostics,
        }
    }
}

// Compile-time `Send + Sync + 'static` guard. The discriminating test
// `owned_eval_program_is_send_sync_static` (in `eval_program_tests.rs`)
// asserts the same property at runtime via `assert_impl_all!`.
const _: fn() = || {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<OwnedEvalProgram>();
    assert_send_sync_static::<LoweringError>();
    assert_send_sync_static::<LoweredStmt>();
    assert_send_sync_static::<LoweredExpr>();
};
