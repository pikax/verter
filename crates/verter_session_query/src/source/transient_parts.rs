//! Owned TRANSIENT lowered declaration parts: fact-production intermediates
//! a source re-lowering returns by value to its single demanding consumer.
//! None of these records is ever stored in a cache or retained artifact.

use std::sync::Arc;

use verter_type_expr::{FunctionParam, ObjectExpr, TypeExpr, TypeParam, TypePredicate};

/// Where a transient signature's authored function node lives, relative to its
/// owning declaration statement — drives the minted [`FunctionSpansOrigin`](verter_type_expr::span_origins::FunctionSpansOrigin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweredSignatureOrigin {
    /// The declaration statement's body IS the function (a `function` decl, an
    /// arrow / function-expression initializer).
    DeclBody,
    /// A member of the produced object shape at this ordinal (a class
    /// constructor / static method in the `typeof C` constructor shape).
    ShapeMember { ordinal: u32 },
    /// Genuinely synthesized — no authored function node (a class with no
    /// declared constructor).
    Synthetic,
}

/// TRANSIENT lowered parts of one function/method signature: the typed-IR
/// parameter / return / type-parameter forms JSDoc enrichment and inference
/// operate on. The stored form is the minted `FunctionSignatureFact`.
#[derive(Debug, Clone)]
pub struct LoweredSignatureParts {
    pub parameters: Vec<FunctionParam>,
    /// The AUTHORED return carrier (a TS annotation or a JSDoc `@returns`
    /// recovery). An unannotated function's return is body-derived and names
    /// its served function position instead — never a body scan.
    pub return_type: Option<TypeExpr>,
    /// The authored return's type predicate (`x is T`, `asserts x`, …),
    /// beside a `boolean` / `void` [`Self::return_type`].
    pub predicate: Option<Arc<TypePredicate>>,
    pub type_parameters: Vec<TypeParam>,
    /// Whether this signature is backed by an implementation body (vs. a
    /// bodiless overload / ambient declaration). Projection-time overload
    /// visibility reads the stored fact's copy of this flag.
    pub has_implementation_body: bool,
    /// Whether the function carried an explicit AUTHORED TS return annotation
    /// (`(): T`). Only an authored return position mints a `FunctionReturn`
    /// body locator — an inferred / JSDoc-filled return has no authored
    /// `TSType` node to address and is recovered whole-signature on demand.
    pub has_authored_return: bool,
    /// Whether the return carrier was recovered from a JSDoc `@returns`
    /// payload (no authored TS annotation present). Set only by the shared
    /// JSDoc enrichment; distinguishes the declared-recovery provenance.
    pub jsdoc_return: bool,
    /// Span-recovery origin of the authored function node.
    pub origin: LoweredSignatureOrigin,
}

/// Owned TRANSIENT value-declaration parts of one demanded symbol, re-lowered
/// from the retained snapshot by the decl-body memo's transient value re-lowering for
/// the locator-deref worker: the merged contributor view over the per-
/// statement `LoweredValueDeclParts` (last-wins annotation / object shape;
/// signatures concatenated in contributor order, so the vector index IS the
/// GROUP-level `ValueSignature` ordinal the producer-minted locators carry).
/// Fact-production intermediates — returned owned, never stored.
#[derive(Debug, Clone, Default)]
pub struct TransientValueParts {
    pub type_annotation: Option<TypeExpr>,
    pub object_shape: Option<ObjectExpr>,
    pub signatures: Vec<LoweredSignatureParts>,
    /// The owning declaration's kind (strict last-wins, the annotation rule)
    /// — the whole-signature recovery reads it to name the served function
    /// position of a body-derived return (declaration body vs initializer).
    pub kind: Option<crate::declarations::ValueDeclKind>,
    /// The owning declaration's HEADER type parameters — populated for a
    /// dual-space declaration (a `class K<T>` whose VALUE side's constructor
    /// shape references `T`) from the SAME statements' type-side parts,
    /// unioned first-seen-by-name. Empty for plain value declarations.
    pub type_parameters: Vec<TypeParam>,
}

/// Owned TRANSIENT type-declaration parts of one demanded symbol, re-lowered
/// from the retained snapshot by the decl-body memo's transient type re-lowering
/// for the locator-deref worker: the ordered contributor bodies (source /
/// binder order, a JSDoc-`@typedef` payload appended) plus the header type
/// parameters unioned first-seen-by-name across contributors in that same
/// order — the SAME union the demanded lowering folded. Fact-production
/// intermediates — returned owned, never stored.
#[derive(Debug, Clone, Default)]
pub struct TransientTypeParts {
    pub bodies: Vec<TypeExpr>,
    pub type_parameters: Vec<TypeParam>,
}
