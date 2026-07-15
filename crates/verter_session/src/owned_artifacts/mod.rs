//! Owned, `Send + Sync + 'static` post-lowering artifact types.
//!
//! - [`eval_program::OwnedEvalProgram`] — owned IR for parsed script
//!   bodies.
//!
//! The type is arena-free so it can sit in host-owned typed DBs without
//! thread-local workarounds. The OXC parser arena drops at the
//! lowering boundary; nothing in `owned_artifacts/` references the
//! allocator. The live per-file `EvalEnv` lives on `IndexedReady`, and
//! the transient `ParsedEvalProgram` is threaded by reference within a
//! cold flight, never cached host-wide.
//!
//! The macro-impact inventory baseline lives at
//! `eval_program_macro_impact_inventory.md` — a sibling document the
//! lowering tests consult.

pub mod eval_program;

pub use eval_program::{
    BinaryOp, DeclKind, InternedExpressionId, InternedIdentifierId, InternedIdentifierTable,
    InternedLiteralEntry, InternedLiteralId, InternedLiteralTable, LiteralKind, LoweredExpr,
    LoweredStmt, LoweringDiagnostic, LoweringError, OwnedEvalProgram, SpanId, UnaryOp,
    UnsupportedKind,
};
