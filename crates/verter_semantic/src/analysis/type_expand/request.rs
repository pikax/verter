//! Expansion output types backed by the native type solver.
//!
//! These types preserve the solver's exactness and execution status at the
//! public expansion boundary instead of collapsing back into an Exact/Partial
//! compatibility contract.

use crate::analysis::type_solver::result::{ExecutionStatus, SolverExactness};
use verter_type_expr::{TypeExpr, TypeExprScope, TypeParam};

pub type ExpansionExactness = SolverExactness;
pub type ExpansionExecutionStatus = ExecutionStatus;

// ---------------------------------------------------------------------------
// Output types - same shape, solver-backed
// ---------------------------------------------------------------------------

/// Materialized object surface from the solver.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedObjectShape {
    pub properties: Vec<ExpandedProperty>,
    pub index_signatures: Vec<ExpandedIndexSignature>,
    pub call_signatures: Vec<ExpandedCallSignature>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedProperty {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source). See
    /// [`verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp::declared_in_macro_type_arg`]
    /// for the structural definition. Propagated by
    /// `macro_shapes`-side materialisation and the prepared-surface walker
    /// from the upstream `SurfaceMember` / `ProjectedMember` source.
    #[serde(default)]
    pub declared_in_macro_type_arg: bool,
    /// Provenance sidecar for synthetic carrier
    /// references mirrored into the `define_props` / `define_emits` /
    /// `define_slots` shape. `Some(_)` only when the source
    /// `ExpandedField` carried a `CarrierProvenance`; otherwise
    /// `None`. Drives downstream universal-cache and registry
    /// short-circuits without forcing consumers to re-key off the
    /// bare `TypeExpr::Ref { name }` carrier.
    /// `#[serde(skip)]` — resolver-internal; never reaches the FFI /
    /// TS-side meta payload.
    #[serde(skip)]
    pub carrier_provenance: Option<CarrierProvenance>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedIndexSignature {
    pub key_type: TypeExpr,
    pub value_type: TypeExpr,
    pub readonly: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedCallSignature {
    pub parameters: Vec<ExpandedParameter>,
    pub return_type: TypeExpr,
    pub type_parameters: Vec<TypeParam>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedParameter {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExpandedNormalizedExpr {
    pub expr: TypeExpr,
}

// ---------------------------------------------------------------------------
// Result & diagnostics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionResult<T> {
    pub value: T,
    pub exactness: ExpansionExactness,
    pub execution_status: ExpansionExecutionStatus,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionMetadata {
    pub exactness: ExpansionExactness,
    pub execution_status: ExpansionExecutionStatus,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionDiagnostic {
    pub reason: ExpansionStopReason,
    pub context: String,
    pub property_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExpansionStopReason {
    BudgetExceeded,
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

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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
}

/// Opaque graph-node identifier for the slot-binding carrier verdict
/// cache. `verter_semantic` treats this as an opaque `u64`; the
/// downstream `verter_session` carrier-verdict cache projects it back
/// to its own `SemanticNodeId` newtype for cache-key lookup.
///
/// The value is the inner `u64` of the
/// `verter_session::semantic_query::SemanticNodeId` the producer
/// minted the symbolic carrier from. Together with the owner scope,
/// this disambiguates same-named bindings across distinct slots
/// inside a single component, preventing the name-only cache-key
/// poisoning that a same-named real type alias would otherwise cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarrierValueNodeId(pub u64);

/// Discriminates which published-surface family a carrier was minted
/// for so the carrier-verdict cache key can distinguish a
/// `slot_bindings`-side carrier from a `bindings`-side carrier even
/// when the slot name and binding name collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublishedSurfaceKind {
    /// `ExpandedComponentTypes::slot_bindings` — positional bindings of
    /// a slot's function parameter object.
    SlotBinding,
    /// `ExpandedComponentTypes::bindings` — defineSlots T's
    /// `MergedMember` bindings (slot-derived re-publication).
    Binding,
}

/// Provenance sidecar attached to the symbolic
/// `TypeExpr::Ref { name: <binding_name> }` carrier the slot-binding
/// graph publisher produces when no parser-path `binding_expr` is
/// available for a graph-native `(slot_name, binding_name)` pair.
///
/// Cache-identity contract: the carrier-verdict cache keys on the
/// full identity (scope, surface, slot name, binding name,
/// value-node) so a synthetic slot parameter named `foo` cannot
/// poison or be poisoned by:
///
/// * a real workspace-owned `type foo = …` alias with the same name,
/// * a different slot's same-named binding (different `value_node`),
/// * a different surface family's same-named entry.
///
/// `CarrierProvenance` is internal to the resolver pipeline; it is
/// `#[serde(skip)]`-d on `ExpandedField` / `ExpandedProperty` so it
/// does not leak through the FFI / TS-side meta payload. Downstream
/// FFI consumers continue to observe only the bare
/// `TypeExpr::Ref { name }` carrier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CarrierProvenance {
    /// Canonical file ID of the component / macro that minted the carrier.
    pub scope_canonical_id: std::sync::Arc<str>,
    /// Which published-surface family the carrier lives in.
    pub surface_kind: PublishedSurfaceKind,
    /// For `SlotBinding`, the slot name (e.g. `"default"`, `"header"`).
    /// For `Binding` (defineSlots merged member), the defining slot
    /// name when known.
    pub slot_name: Option<std::sync::Arc<str>>,
    /// The binding's identifier — same string as the synthetic
    /// `TypeExpr::Ref { name }` carrier's `name`.
    pub binding_name: std::sync::Arc<str>,
    /// Stable identity of the graph node the carrier was minted from.
    /// Together with `scope_canonical_id`, this is the disambiguator
    /// that name-only keys cannot supply.
    pub value_node: CarrierValueNodeId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedField {
    pub name: String,
    pub r#type: TypeExpr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    #[serde(default)]
    pub optional: bool,
    pub exactness: ExpansionExactness,
    pub execution_status: ExpansionExecutionStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ExpansionDiagnostic>,
    /// Shallow lowered typed form preserved alongside the post-expansion
    /// `r#type`. Carries the bare annotation expression the user wrote
    /// (e.g. `TypeExpr::Ref { name: "ImportedAlias" }`) so consumers that
    /// need to recover the syntactic shape of the prop / emit / slot-binding
    /// annotation do not have to reparse `raw_type`. Populated by the
    /// producer at `expand_macro_types_impl_with_expander` from the
    /// analyzer-side `AnalyzedPropField.type_expr` /
    /// `AnalyzedEmitField.payload_expr` / `AnalyzedSlotFieldBinding.binding_expr`
    /// shallow source. `None` when the analyzer's shallow source was
    /// `None` (e.g. Options-API binding entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shallow_type_expr: Option<TypeExpr>,
    /// Scope of `shallow_type_expr`: canonical_id of the file whose OXC
    /// parse produced the shallow expression. Pairing invariant:
    /// `shallow_type_expr.is_some() <=> shallow_type_expr_scope.is_some()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shallow_type_expr_scope: Option<TypeExprScope>,
    /// Whether this member was explicitly declared in the macro's type
    /// argument's own body (vs reached via heritage / Omit / intersection
    /// from an external source). See
    /// [`verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp::declared_in_macro_type_arg`]
    /// for the structural definition. Propagated by
    /// `expand_macro_types_impl_with_expander` and
    /// `surface_member_to_expanded_field` from the upstream `SurfaceMember`
    /// / `AnalyzedPropField` source. Consumed by component-meta's
    /// `extract_props_from_macro` to disambiguate cross-file imported
    /// macro provenance.
    #[serde(default)]
    pub declared_in_macro_type_arg: bool,
    /// Provenance sidecar for synthetic
    /// `TypeExpr::Ref { name }` carriers produced by the slot-binding
    /// graph publisher when no parser-path `binding_expr` is
    /// available. `Some(_)` for synthetic carriers; `None` for every
    /// other published field (real macro props/emits, parser-path slot
    /// bindings, projected surface members). Drives the
    /// carrier-verdict cache and `published_reducer` /
    /// component-meta-registry collection short-circuits.
    /// `#[serde(skip)]` — resolver-internal; never reaches the FFI /
    /// TS-side meta payload.
    #[serde(skip)]
    pub carrier_provenance: Option<CarrierProvenance>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedMacroProps {
    pub macro_index: usize,
    pub result: ExpansionResult<ExpandedObjectShape>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_result_metadata_preserves_exactness_and_execution_status() {
        let result = ExpansionResult {
            value: ExpandedNormalizedExpr {
                expr: TypeExpr::primitive(verter_type_expr::PrimitiveName::String),
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
        let mut solver_result = SolverResult::exact_concrete(TypeExpr::primitive(
            verter_type_expr::PrimitiveName::String,
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
}
