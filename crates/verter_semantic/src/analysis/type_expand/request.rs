//! Expansion output types backed by the native type solver.
//!
//! These types preserve the solver's exactness and execution status at the
//! public expansion boundary instead of collapsing back into an Exact/Partial
//! compatibility contract.

use std::sync::Arc;

use crate::analysis::type_solver::result::{ExecutionStatus, SolverExactness};
use verter_type_expr::facts::{NarrowTypeParam, SemanticTypeSource, SourcePosition};
use verter_type_expr::locators::AuthoredBodyLocator;

pub type ExpansionExactness = SolverExactness;
pub type ExpansionExecutionStatus = ExecutionStatus;

// ---------------------------------------------------------------------------
// Output types - same shape, solver-backed
// ---------------------------------------------------------------------------

/// Materialized object surface from the solver.
#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedObjectShape {
    pub properties: Vec<ExpandedProperty>,
    pub index_signatures: Vec<ExpandedIndexSignature>,
    pub call_signatures: Vec<ExpandedCallSignature>,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedProperty {
    pub name: String,
    /// The member value's three-state SOURCE POSITION: a present faithful
    /// source, a proven schema absence, or a typed source-construction
    /// failure at this REQUIRED member-value position. A `Failed` position
    /// fails output materialization — it is never rendered as an `unknown`
    /// success.
    pub ty: SourcePosition,
    pub optional: bool,
    pub readonly: bool,
    /// Declared accessibility of the member, carried verbatim from the source
    /// member ([`verter_type_expr::MemberVisibility`] on the IR
    /// `ObjectProperty` / `MethodSignature`, or the graph
    /// `SurfaceMember::visibility`). `Public`
    /// for every non-class origin; a class member carries its
    /// `TSAccessibility`. This is the visibility carrier the shallow-by-default
    /// shape projection MUST preserve so any DERIVATION that filters by key
    /// (`Pick` / `Omit` over a `Partial<C>` mapped surface in the utility-route
    /// fallback) can re-apply the public-keyspace gate — a `keyof` / `Pick` /
    /// `Omit` derivation is public-only, so a non-public member must never be
    /// retained on the derived surface. The full member set (incl. non-public)
    /// stays recorded for the keep-all `native_props` carrier; only the
    /// derivations gate. Serialized with `#[serde(default)]` so a non-public
    /// value survives a roundtrip and pre-existing JSON without the field
    /// deserializes as `Public`.
    #[serde(default)]
    pub visibility: verter_type_expr::MemberVisibility,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source). See
    /// [`verter_parser::utils::oxc::script::type_surface::ResolvedProp::declared_in_macro_type_arg`]
    /// for the structural definition. Propagated by
    /// `macro_shapes`-side materialisation and the prepared-surface walker
    /// from the upstream `SurfaceMember` source.
    #[serde(default)]
    pub declared_in_macro_type_arg: bool,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedIndexSignature {
    /// The declared key domain's three-state SOURCE POSITION. A genuinely
    /// OPEN key domain (`[key: string]`) is a PRESENT closed leaf — semantic
    /// openness is a valid success, never a failure.
    pub key_type: SourcePosition,
    /// The declared value type's three-state SOURCE POSITION. A REQUIRED
    /// index value position richer than the producer's faithful vocabulary
    /// is a typed source-construction `Failed` — it marks the result
    /// non-complete instead of degrading to a fabricated `unknown` success.
    pub value_type: SourcePosition,
    pub readonly: bool,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedCallSignature {
    pub parameters: Vec<ExpandedParameter>,
    pub return_type: SemanticTypeSource,
    /// The signature's own type parameters, narrowed to the fact-shaped
    /// [`NarrowTypeParam`] carrier (name + ordinal) — never the raw
    /// [`verter_type_expr::TypeParam`], which structurally owns
    /// `constraint`/`default: Option<Arc<TypeExpr>>`. This mirrors how the
    /// sibling [`verter_type_expr::facts::FunctionSignatureFact`] carries a
    /// signature's type parameters and is produced by
    /// [`crate::analysis::type_eval_build::narrow_signature_type_params`].
    /// A signature-scoped bound has no addressable authored slot (bounds are
    /// recovered whole-signature on demand), so `constraint`/`default`
    /// fail closed to `None` — no producer emits a bound locator here.
    pub type_parameters: Arc<[NarrowTypeParam]>,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedParameter {
    pub name: String,
    pub ty: SemanticTypeSource,
    pub optional: bool,
    pub rest: bool,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
pub struct ExpandedNormalizedExpr {
    pub expr: SemanticTypeSource,
}

// ---------------------------------------------------------------------------
// Result & diagnostics
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionResult<T> {
    pub value: T,
    pub exactness: ExpansionExactness,
    pub execution_status: ExpansionExecutionStatus,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionMetadata {
    pub exactness: ExpansionExactness,
    pub execution_status: ExpansionExecutionStatus,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionDiagnostic {
    pub reason: ExpansionStopReason,
    pub context: String,
    pub property_name: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub enum ExpansionStopReason {
    BudgetExceeded,
    /// The connected projection/evaluation demand exhausted its total work
    /// envelope while semantic identity continued to change.
    ProjectionWorkLimit,
    /// The separate connected host-query nesting limit was exhausted.
    ConnectedQueryDepthLimit,
    MappedDepthExceeded,
    UnresolvedReference,
    IndeterminateConditional,
    InfiniteKeySpace,
    UnsupportedOperator,
    /// The solver truncated the conditional context because more frames were
    /// available than the capture limit. This is a non-semantic observability
    /// diagnostic — it does not affect exactness.
    ConditionalContextTruncated,
    /// `T & T` — duplicate intersection arm short-circuited. The walker
    /// terminates the offending arm without contribution because it
    /// would not add new members.
    IdempotentArm,
    /// True graph cycle detected during the synthesis walk; the walker
    /// terminates the offending alias / heritage chain without
    /// contribution. The owning declaration name (when known) is
    /// surfaced through `ExpansionDiagnostic.context`.
    CyclicReference,
    /// `Instantiate` returned `Recursive` — a declaration referenced
    /// itself transitively during synthesis. The owning declaration
    /// identity is surfaced through `ExpansionDiagnostic.context`.
    CyclicInstantiation,
    /// `Instantiate` returned an `Error(QueryError)` — a fatal
    /// query-level failure during synthesis. The declaration identity
    /// and the underlying error are surfaced through
    /// `ExpansionDiagnostic.context`.
    InstantiationError,
    /// One arm of a Union evaluated to a non-Object surface; the
    /// merged surface drops members for that arm per the union rule
    /// (member surface = members in ALL arms). The arm index is
    /// surfaced through `ExpansionDiagnostic.context`.
    EmptyUnionArm,
}

impl ExpandedObjectShape {
    pub fn empty() -> Self {
        Self {
            properties: Vec::new(),
            index_signatures: Vec::new(),
            call_signatures: Vec::new(),
        }
    }
}

impl<T> ExpansionResult<T> {
    pub fn exact_concrete(value: T) -> Self {
        Self {
            value,
            exactness: SolverExactness::ExactConcrete,
            execution_status: ExecutionStatus::Completed,
            diagnostics: Vec::new(),
        }
    }

    pub fn exact_symbolic(value: T) -> Self {
        Self {
            value,
            exactness: SolverExactness::ExactSymbolic,
            execution_status: ExecutionStatus::Completed,
            diagnostics: Vec::new(),
        }
    }

    pub fn incomplete(
        value: T,
        execution_status: ExpansionExecutionStatus,
        diagnostics: Vec<ExpansionDiagnostic>,
    ) -> Self {
        Self {
            value,
            exactness: SolverExactness::Incomplete,
            execution_status,
            diagnostics,
        }
    }

    pub fn is_exact(&self) -> bool {
        self.exactness.is_exact()
    }

    pub fn metadata(&self) -> ExpansionMetadata {
        ExpansionMetadata {
            exactness: self.exactness,
            execution_status: self.execution_status,
            diagnostics: self.diagnostics.clone(),
        }
    }

    #[cfg(test)]
    pub fn exact(value: T) -> Self {
        Self::exact_concrete(value)
    }

    #[cfg(test)]
    pub fn partial(value: T, diagnostics: Vec<ExpansionDiagnostic>) -> Self {
        Self::incomplete(value, ExecutionStatus::Completed, diagnostics)
    }
}

// ---------------------------------------------------------------------------
// Component-level types
// ---------------------------------------------------------------------------

#[derive(
    Debug, Clone, Default, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedComponentTypes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<ExpandedField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub define_props: Vec<ExpandedMacroProps>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub define_emits: Vec<ExpandedMacroObjectShape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<ExpandedField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub define_slots: Vec<ExpandedMacroObjectShape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_bindings: Vec<ExpandedField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<ExpandedField>,
    /// Per-macro `defineExpose` projected surface lane: the projector's
    /// published surface members for each type-based `defineExpose` macro.
    /// The exposed-analysis join pairs entries by the stable
    /// `(macro_index, member name)` identity to set
    /// `ExposedAnalysis.type_source` from the projected field's `r#type`;
    /// an authored object-literal exposure of the same name keeps its own
    /// (binding-derived) source — the lane only fills what the literal form
    /// does not provide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exposed: Vec<ExpandedMacroExposed>,
}

/// One `defineExpose` macro's projected surface members. `macro_index` is
/// the macro's position in the analyzer's macro list — the same ordinal
/// every other per-macro lane (`ExpandedMacroProps` /
/// `ExpandedMacroObjectShape`) keys on.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedMacroExposed {
    pub macro_index: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ExpandedField>,
}

#[derive(
    Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr,
)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedField {
    pub name: String,
    /// The field value's three-state SOURCE POSITION (see
    /// [`ExpandedProperty::ty`]). A REQUIRED payload position (an emit
    /// field's payload) whose faithful source could not be constructed is
    /// `Failed` — never a fabricated `unknown` success.
    pub r#type: SourcePosition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    #[serde(default)]
    pub optional: bool,
    pub exactness: ExpansionExactness,
    pub execution_status: ExpansionExecutionStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ExpansionDiagnostic>,
    /// The authored SHALLOW source of the field's annotation — the single
    /// paired identity replacing the pre-expansion syntactic sidecar: the
    /// producing-canonical scope is subsumed by the locator anchor.
    /// Populated by the producer at `expand_macro_types_impl_with_expander`
    /// from the analyzer-side `AnalyzedPropField.payload` /
    /// `AnalyzedEmitField.payload` / `AnalyzedSlotFieldBinding.payload`
    /// authored position. `None` when the analyzer's shallow source was
    /// `None` (e.g. Options-API binding entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shallow_source: Option<AuthoredBodyLocator>,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source). See
    /// [`verter_parser::utils::oxc::script::type_surface::ResolvedProp::declared_in_macro_type_arg`]
    /// for the structural definition. Propagated by
    /// `expand_macro_types_impl_with_expander` and
    /// `surface_member_to_expanded_field` from the upstream `SurfaceMember`
    /// / `AnalyzedPropField` source. Consumed by component-meta's
    /// `extract_props_from_macro` to disambiguate cross-file imported
    /// macro provenance.
    #[serde(default)]
    pub declared_in_macro_type_arg: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedMacroProps {
    pub macro_index: usize,
    pub result: ExpansionResult<ExpandedObjectShape>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedMacroObjectShape {
    pub macro_index: usize,
    pub result: ExpansionResult<ExpandedObjectShape>,
}

impl ExpandedComponentTypes {
    pub fn is_empty(&self) -> bool {
        self.props.is_empty()
            && self.define_props.is_empty()
            && self.define_emits.is_empty()
            && self.emits.is_empty()
            && self.define_slots.is_empty()
            && self.slot_bindings.is_empty()
            && self.bindings.is_empty()
            && self.exposed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_result_metadata_preserves_exactness_and_execution_status() {
        let result = ExpansionResult {
            value: ExpandedNormalizedExpr {
                expr: SemanticTypeSource::Closed(verter_type_expr::facts::ClosedTypeFact::Leaf(
                    verter_type_expr::facts::LeafTypeFact::Primitive(
                        verter_type_expr::PrimitiveName::String,
                    ),
                )),
            },
            exactness: SolverExactness::ExactSymbolic,
            execution_status: ExecutionStatus::Interrupted,
            diagnostics: vec![ExpansionDiagnostic {
                reason: ExpansionStopReason::UnsupportedOperator,
                context: "kept symbolic".to_string(),
                property_name: None,
            }],
        };

        let metadata = result.metadata();
        assert_eq!(metadata.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(metadata.execution_status, ExecutionStatus::Interrupted);
        assert_eq!(metadata.diagnostics.len(), 1);
    }

    #[test]
    fn solver_diagnostic_converts_to_expansion_diagnostic_without_downgrading_exactness() {
        use crate::analysis::type_solver::result::{SolverDiagnostic, SolverResult};

        // A solver result that is exact but has a non-semantic diagnostic
        let mut solver_result = SolverResult::exact_concrete(SemanticTypeSource::Closed(
            verter_type_expr::facts::ClosedTypeFact::Leaf(
                verter_type_expr::facts::LeafTypeFact::Primitive(
                    verter_type_expr::PrimitiveName::String,
                ),
            ),
        ));
        solver_result
            .diagnostics
            .push(SolverDiagnostic::ConditionalContextTruncated {
                available: 12,
                captured: 8,
            });

        let expansion =
            crate::analysis::type_expand::solver_result_to_normalized_expansion(solver_result);

        // Exactness must NOT be downgraded
        assert_eq!(expansion.exactness, SolverExactness::ExactConcrete);
        assert_ne!(expansion.exactness, SolverExactness::Incomplete);
        assert_eq!(expansion.execution_status, ExecutionStatus::Completed);

        // Diagnostic is present
        assert_eq!(expansion.diagnostics.len(), 1);
        assert_eq!(
            expansion.diagnostics[0].reason,
            ExpansionStopReason::ConditionalContextTruncated
        );
        assert!(expansion.diagnostics[0].context.contains("12"));
        assert!(expansion.diagnostics[0].context.contains("8"));
    }

    // Structural witness — every struct in the serde-persisted Expanded* family
    // derives `#[derive(verter_no_typeexpr::NoTypeExpr)]`, so the per-struct
    // derive is itself the always-compiled rail: it fails the build if any field
    // regresses to a raw `TypeExpr` carrier (as the former
    // `ExpandedCallSignature.type_parameters: Vec<TypeParam>` did). These asserts
    // pin the current family members explicitly and greppably; a NEW family
    // struct stays closed only if it likewise derives `NoTypeExpr` (members
    // reachable from `ExpandedComponentTypes` are additionally proven
    // transitively through it).
    use static_assertions::assert_impl_all;
    use verter_no_typeexpr::NoTypeExpr;

    assert_impl_all!(ExpandedObjectShape: NoTypeExpr);
    assert_impl_all!(ExpandedProperty: NoTypeExpr);
    assert_impl_all!(ExpandedIndexSignature: NoTypeExpr);
    assert_impl_all!(ExpandedCallSignature: NoTypeExpr);
    assert_impl_all!(ExpandedParameter: NoTypeExpr);
    assert_impl_all!(ExpandedNormalizedExpr: NoTypeExpr);
    assert_impl_all!(ExpandedField: NoTypeExpr);
    assert_impl_all!(ExpandedComponentTypes: NoTypeExpr);

    #[test]
    fn expanded_call_signature_type_params_narrow_to_fact_carrier_with_bounds_dropped() {
        use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact};
        use verter_type_expr::{PrimitiveName, TypeExpr, TypeParam};

        // A call signature `<T extends string, U = string>` — BOTH source params
        // carry a raw `TypeExpr` bound (`constraint` / `default`).
        let source_params = vec![
            TypeParam {
                name: "T".to_string(),
                constraint: Some(Arc::new(TypeExpr::primitive(PrimitiveName::String))),
                default: None,
            },
            TypeParam {
                name: "U".to_string(),
                constraint: None,
                default: Some(Arc::new(TypeExpr::primitive(PrimitiveName::String))),
            },
        ];

        let sig = ExpandedCallSignature {
            parameters: Vec::new(),
            return_type: SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                PrimitiveName::Void,
            ))),
            type_parameters: crate::analysis::type_eval_build::narrow_signature_type_params(
                &source_params,
            ),
        };

        // Names + ordinals are retained on the fact-shaped carrier...
        assert_eq!(sig.type_parameters.len(), 2);
        assert_eq!(sig.type_parameters[0].name, "T");
        assert_eq!(sig.type_parameters[0].ordinal, 0);
        assert_eq!(sig.type_parameters[1].name, "U");
        assert_eq!(sig.type_parameters[1].ordinal, 1);

        // ...and the raw `TypeExpr` bounds fail closed to `None` — the narrowed
        // carrier no longer owns `constraint`/`default: Option<Arc<TypeExpr>>`.
        // If the fix ever smuggled the bound back through, these would be `Some`.
        assert!(sig.type_parameters[0].constraint.is_none());
        assert!(sig.type_parameters[0].default.is_none());
        assert!(sig.type_parameters[1].constraint.is_none());
        assert!(sig.type_parameters[1].default.is_none());
    }
}
