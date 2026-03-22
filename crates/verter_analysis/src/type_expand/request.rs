//! Expansion request, result, and output types.

use crate::type_expr::{TypeExpr, TypeParam};

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Materialized object surface.
///
/// Semantic output answering "what members does this type have?"
/// All `ty` fields are post-expansion normalized — they have been through
/// the symbolic expander and may still contain `Ref`, `Conditional`, or
/// `Mapped` nodes that the expander intentionally preserved.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedObjectShape {
    pub properties: Vec<ExpandedProperty>,
    pub index_signatures: Vec<ExpandedIndexSignature>,
    pub call_signatures: Vec<ExpandedCallSignature>,
}

/// A single property in an expanded object shape.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedProperty {
    pub name: String,
    /// Post-expansion normalized type. Invariant: this has been through
    /// the symbolic expander. Preserved symbolic nodes represent forms
    /// the expander intentionally kept, not forms it failed to reach.
    pub ty: TypeExpr,
    pub optional: bool,
    pub readonly: bool,
}

/// An index signature in an expanded object shape.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedIndexSignature {
    pub key_type: TypeExpr,
    pub value_type: TypeExpr,
    pub readonly: bool,
}

/// A call/construct signature in an expanded object shape.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedCallSignature {
    pub parameters: Vec<ExpandedParameter>,
    pub return_type: TypeExpr,
    pub type_parameters: Vec<TypeParam>,
}

/// A parameter in a call signature.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedParameter {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub rest: bool,
}

/// A `TypeExpr` with references resolved and utility types applied where
/// possible, but complex forms preserved symbolically when exact resolution
/// is not possible.
///
/// Newtype distinguishes "has been through the expander" from raw lowered
/// syntax. The inner `TypeExpr` may still contain `Ref`, `Conditional`,
/// or `Mapped` nodes — those represent forms the expander intentionally
/// preserved.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExpandedNormalizedExpr {
    pub expr: TypeExpr,
}

// ---------------------------------------------------------------------------
// Budget & policy
// ---------------------------------------------------------------------------

/// Configurable limits for the expansion service.
#[derive(Debug, Clone)]
pub struct ExpansionBudget {
    /// Maximum recursion depth. Default: 32.
    pub max_depth: usize,
    /// Maximum union members from template literal expansion. Default: 64.
    pub max_union_expansion: usize,
    /// Maximum keys to expand per mapped type. Default: 128.
    pub max_mapped_keys: usize,
    /// Maximum nested `evaluate_mapped()` calls. Default: 3.
    pub max_mapped_depth: usize,
    /// Safety-net total step limit. Default: 50_000.
    pub max_symbolic_work: usize,
}

impl Default for ExpansionBudget {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_union_expansion: 64,
            max_mapped_keys: 128,
            max_mapped_depth: 3,
            max_symbolic_work: 50_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Result & completeness
// ---------------------------------------------------------------------------

/// Expansion result carrying the output value, completeness status,
/// and any diagnostics explaining partial results.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionResult<T> {
    pub value: T,
    pub completeness: ExpansionCompleteness,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

/// Expansion completeness and diagnostics without the expanded payload.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionMetadata {
    pub completeness: ExpansionCompleteness,
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

/// Whether the expansion is exact or partial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpansionCompleteness {
    Exact,
    Partial,
}

/// A diagnostic explaining why the expansion is partial.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionDiagnostic {
    pub reason: ExpansionStopReason,
    /// Context string, e.g. "evaluating property 'class' of AccordionProps".
    pub context: String,
    /// Which member was affected, if applicable.
    pub property_name: Option<String>,
}

/// Reason why expansion stopped or degraded.
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

// ---------------------------------------------------------------------------
// Convenience
// ---------------------------------------------------------------------------

pub type ExpandedObjectResult = ExpansionResult<ExpandedObjectShape>;
pub type ExpandedExprResult = ExpansionResult<ExpandedNormalizedExpr>;

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
            completeness: ExpansionCompleteness::Exact,
            diagnostics: Vec::new(),
        }
    }

    pub fn partial(value: T, diagnostics: Vec<ExpansionDiagnostic>) -> Self {
        Self {
            value,
            completeness: ExpansionCompleteness::Partial,
            diagnostics,
        }
    }

    pub fn is_exact(&self) -> bool {
        self.completeness == ExpansionCompleteness::Exact
    }

    pub fn metadata(&self) -> ExpansionMetadata {
        ExpansionMetadata {
            completeness: self.completeness,
            diagnostics: self.diagnostics.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Component-level expanded types
// ---------------------------------------------------------------------------

/// Expanded type annotations for a component's metadata fields.
///
/// Replaces `EvaluatedComponentTypes`. Uses the new expander for
/// `define_props` (ObjectShape) and normalized evaluation for
/// individual prop/emit/slot/binding annotations.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedComponentTypes {
    /// Expanded prop annotation types, keyed by prop name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<ExpandedField>,
    /// Expanded full defineProps object shapes keyed by macro index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub define_props: Vec<ExpandedMacroProps>,
    /// Expanded emit payload types, keyed by event name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<ExpandedField>,
    /// Expanded slot binding types, keyed by "slotName.bindingName".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slot_bindings: Vec<ExpandedField>,
    /// Expanded binding types (for expose/value lookups), keyed by binding name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<ExpandedField>,
}

/// A single expanded type field (prop, event, slot binding, or binding).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedField {
    /// The field name (prop name, event name, or slot.binding key).
    pub name: String,
    /// The expanded type expression (post-expansion normalized).
    pub r#type: TypeExpr,
    /// Whether the source field is optional.
    #[serde(default)]
    pub optional: bool,
    /// Whether this field expanded exactly or only partially.
    pub completeness: ExpansionCompleteness,
    /// Diagnostics explaining why the result is partial.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ExpansionDiagnostic>,
}

/// Expanded full prop object for a specific defineProps macro.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedMacroProps {
    pub macro_index: usize,
    /// The expanded object shape with completeness information.
    pub result: ExpansionResult<ExpandedObjectShape>,
}

impl ExpandedComponentTypes {
    pub fn is_empty(&self) -> bool {
        self.props.is_empty()
            && self.define_props.is_empty()
            && self.emits.is_empty()
            && self.slot_bindings.is_empty()
            && self.bindings.is_empty()
    }
}
