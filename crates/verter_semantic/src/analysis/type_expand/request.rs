//! Expansion output types — now backed by the native type solver.
//!
//! These types use `SolverExactness` and `SolverResult` from `type_solver`
//! instead of the old `ExpansionCompleteness` / `ExpansionBudget` model.
//! The old lightweight evaluator is gone — all expansion goes through
//! `type_solver::solve::solve_type()`.

use crate::analysis::type_expr::{TypeExpr, TypeParam};
use crate::analysis::type_solver::result::SolverExactness;

// ---------------------------------------------------------------------------
// Re-export solver types as the canonical expansion types
// ---------------------------------------------------------------------------

/// Expansion completeness — re-exported from solver for backward compat.
/// `Exact` = solver returned `ExactConcrete` or `ExactSymbolic`.
/// `Partial` = solver returned `Incomplete`.
pub type ExpansionCompleteness = SolverExactCompat;

/// Compat enum that maps 1:1 with the old ExpansionCompleteness.
/// Serializes as "exact" / "partial" for protocol stability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SolverExactCompat {
    Exact,
    Partial,
}

impl From<SolverExactness> for SolverExactCompat {
    fn from(e: SolverExactness) -> Self {
        match e {
            SolverExactness::ExactConcrete | SolverExactness::ExactSymbolic => Self::Exact,
            SolverExactness::Incomplete => Self::Partial,
        }
    }
}

// ---------------------------------------------------------------------------
// Output types — same shape, solver-backed
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
    pub completeness: ExpansionCompleteness,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionMetadata {
    pub completeness: ExpansionCompleteness,
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
    pub fn exact(value: T) -> Self {
        Self {
            value,
            completeness: SolverExactCompat::Exact,
            diagnostics: Vec::new(),
        }
    }

    pub fn partial(value: T, diagnostics: Vec<ExpansionDiagnostic>) -> Self {
        Self {
            value,
            completeness: SolverExactCompat::Partial,
            diagnostics,
        }
    }

    pub fn is_exact(&self) -> bool {
        self.completeness == SolverExactCompat::Exact
    }

    pub fn metadata(&self) -> ExpansionMetadata {
        ExpansionMetadata {
            completeness: self.completeness,
            diagnostics: self.diagnostics.clone(),
        }
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
    pub completeness: ExpansionCompleteness,
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
