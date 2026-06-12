//! Owned, `Send + Sync + 'static` post-lowering artifact types.
//!
//! - [`eval_program::OwnedEvalProgram`] — owned IR for parsed script
//!   bodies.
//! - [`type_resolution_context::OwnedTypeResolutionContext`] — owned
//!   mirror of the borrowed `TypeResolutionContext` minus the dropped
//!   `source: &'ctx [u8]` field.
//!
//! Both types are arena-free so they can sit in host-owned typed DBs
//! (`ProjectTypeStore::type_resolution_context_cache()`) without
//! thread-local workarounds. The OXC parser arena drops at the
//! lowering boundary; nothing in `owned_artifacts/` references the
//! allocator. Production lowering does not populate these artifacts
//! yet — the typed DB is exercised only by tests. The live per-file
//! `EvalEnv` lives on `IndexedReady`, and the transient
//! `ParsedEvalProgram` is threaded by reference within a cold flight,
//! never cached host-wide.
//!
//! The macro-impact inventory baseline lives at
//! `eval_program_macro_impact_inventory.md` — a sibling document the
//! lowering tests consult.

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
