//! Owned, `Send + Sync + 'static` post-lowering artifact types (D64).
//!
//! Tier 1A introduces:
//! - [`eval_program::OwnedEvalProgram`] (D17 + D44 + D107 + D116 + D117) —
//!   owned IR for parsed script bodies.
//! - [`type_resolution_context::OwnedTypeResolutionContext`] (D18 + D45
//!   + D65) — owned mirror of the borrowed `TypeResolutionContext` minus
//!   the dropped `source: &'ctx [u8]` field.
//!
//! Both types live here so they can sit in host-owned typed DBs
//! (`EvalEnvCacheDb`, `TypeResolutionContextDb`) without the previous
//! thread-local workarounds. The OXC parser arena drops at the lowering
//! boundary; nothing in `owned_artifacts/` references the allocator.
//!
//! The macro-impact inventory baseline lives at
//! `eval_program_macro_impact_inventory.md` (D116) — a sibling document
//! that the lowering driver and the Step 1A discriminating tests both
//! consult.

pub mod eval_program;
pub mod type_resolution_context;

pub use eval_program::{
    BinaryOp, DeclKind, InternedExpressionId, InternedIdentifierId, InternedIdentifierTable,
    InternedLiteralEntry, InternedLiteralId, InternedLiteralTable, LiteralKind, LoweredExpr,
    LoweredStmt, LoweringDiagnostic, LoweringError, OwnedEvalProgram, SpanId, UnaryOp,
    UnsupportedKind,
};
pub use type_resolution_context::{
    ClassDeclId, CompositeKind, DeclId, DeclarationFingerprint, InterfaceDeclId, OwnedAliasEntry,
    OwnedClassDecl, OwnedInterfaceEntry, OwnedObjectMember, OwnedTupleElement, OwnedTypeExpr,
    OwnedTypeParameter, OwnedTypeResolutionContext, ResolutionDiagnostic, ResolutionDiagnosticKind,
    ResolvedElementsOwned, ResolvedTypeParamBindingCacheKey, SpanArena, TypeAliasDeclId,
    TypeDeclArena, TypeExprId, TypeOperatorKind, TypeParameterDeclId,
};
