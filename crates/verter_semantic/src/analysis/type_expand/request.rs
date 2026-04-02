//! Expansion output types backed by the native type solver.
//!
//! These types preserve the solver's exactness and execution status at the
//! public expansion boundary instead of collapsing back into an Exact/Partial
//! compatibility contract.

use crate::analysis::type_expr::{TypeExpr, TypeParam};
use crate::analysis::type_solver::result::{ExecutionStatus, SolverExactness};

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
                expr: TypeExpr::primitive(crate::analysis::type_expr::PrimitiveName::String),
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
}
