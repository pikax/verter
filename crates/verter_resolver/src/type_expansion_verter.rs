//! Verter-native `TypeExpander` implementation.
//!
//! Wraps the existing OXC-based cross-file resolver behind the [`TypeExpander`]
//! trait. No external processes — pure Rust.
//!
//! The async boundary exists at host/source-loading edges. Core semantic work
//! stays synchronous.

use std::sync::Arc;

use verter_analysis::type_expr::{PrimitiveName, TypeExpr};
#[cfg(test)]
use verter_span::Span;

use crate::type_expansion::{
    ExpandedMember, ExpanderFuture, ExpansionCompleteness, TypeExpander, TypeExpansionError,
    TypeExpansionRequest, TypeExpansionResult,
};
use crate::ResolvedMacroMeta;

// ---------------------------------------------------------------------------
// VerterComponentMetaProvider trait
// ---------------------------------------------------------------------------

/// Trait for hosts that can provide component-meta results.
///
/// Implemented by `ComponentMetaHost` in `verter_host`. This allows
/// `VerterTypeExpander` to call into the existing resolution pipeline
/// without depending on `verter_host` directly.
pub trait VerterComponentMetaProvider: Send + Sync {
    /// Get full component metadata for a file (uses Verter's native resolver).
    fn get_component_meta(
        &self,
        canonical_id: &str,
    ) -> Option<verter_analysis::component_meta::ComponentMetaAnalysis>;

    /// Get the resolved macro metadata for a file (for type expansion).
    fn get_resolved_macros(&self, canonical_id: &str) -> Vec<ResolvedMacroMeta> {
        // Default: not available. Implementations override.
        let _ = canonical_id;
        vec![]
    }
}

// ---------------------------------------------------------------------------
// VerterTypeExpander
// ---------------------------------------------------------------------------

/// Native Verter `TypeExpander` — wraps the existing OXC-based resolver.
///
/// Resolves types using Verter's cross-file import graph traversal and
/// type evaluation. Does not spawn external processes.
///
/// Calls `VerterComponentMetaProvider::get_component_meta()` and extracts
/// the matching macro's resolved data for the requested span.
pub struct VerterTypeExpander {
    provider: Arc<dyn VerterComponentMetaProvider>,
}

impl VerterTypeExpander {
    pub fn new(provider: Arc<dyn VerterComponentMetaProvider>) -> Self {
        Self { provider }
    }
}

impl TypeExpander for VerterTypeExpander {
    fn expand_type<'a>(
        &'a self,
        request: &'a TypeExpansionRequest,
    ) -> ExpanderFuture<'a, TypeExpansionResult> {
        Box::pin(async move {
            let meta = self
                .provider
                .get_component_meta(&request.canonical_id)
                .ok_or(TypeExpansionError::SourceUnavailable)?;

            // Return all props as expanded members (the Verter backend
            // resolves the full component surface, not a single type).
            let members: Vec<ExpandedMember> = meta
                .props
                .iter()
                .map(|p| ExpandedMember {
                    name: p.name.clone(),
                    type_expr: p.type_expr.clone(),
                    raw_type: p.raw_type.clone(),
                    optional: !p.required,
                    description: p.description.clone(),
                })
                .collect();

            let completeness = if members.is_empty() {
                ExpansionCompleteness::LowerBound
            } else {
                ExpansionCompleteness::Exact
            };

            Ok(TypeExpansionResult {
                type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
                members,
                completeness,
            })
        })
    }
}

/// Convert a resolved macro's props to `ExpandedMember` entries.
pub fn resolved_macro_to_members(macro_meta: &ResolvedMacroMeta) -> Vec<ExpandedMember> {
    let mut members = Vec::new();

    for prop in &macro_meta.props {
        let type_expr = prop
            .type_annotation
            .as_deref()
            .map(crate::type_text_parser::parse_type_text)
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Unknown));
        members.push(ExpandedMember {
            name: prop.name.clone(),
            type_expr,
            raw_type: prop.type_annotation.clone(),
            optional: prop.is_optional,
            description: prop.description.clone(),
        });
    }

    for emit in &macro_meta.emits {
        let type_expr = emit
            .payload_type
            .as_deref()
            .map(crate::type_text_parser::parse_type_text)
            .unwrap_or_else(|| TypeExpr::primitive(PrimitiveName::Unknown));
        members.push(ExpandedMember {
            name: emit.name.clone(),
            type_expr,
            raw_type: emit.payload_type.clone(),
            optional: false,
            description: emit.description.clone(),
        });
    }

    for slot in &macro_meta.slots {
        members.push(ExpandedMember {
            name: slot.name.clone(),
            type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
            raw_type: slot.return_type.clone(),
            optional: !slot.is_required,
            description: slot.description.clone(),
        });
    }

    members
}

/// Build a `TypeExpansionResult` from a resolved macro.
pub fn resolved_macro_to_expansion(macro_meta: &ResolvedMacroMeta) -> TypeExpansionResult {
    let members = resolved_macro_to_members(macro_meta);
    let completeness = if members.is_empty() {
        ExpansionCompleteness::LowerBound
    } else {
        ExpansionCompleteness::Exact
    };

    let type_expr = if !macro_meta.type_name.is_empty() {
        TypeExpr::named(&macro_meta.type_name)
    } else {
        TypeExpr::primitive(PrimitiveName::Unknown)
    };

    TypeExpansionResult {
        type_expr,
        members,
        completeness,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResolvedTypeDeclaration;
    use verter_analysis::types::AnalyzedMacroKind;

    fn make_resolved_macro() -> ResolvedMacroMeta {
        ResolvedMacroMeta {
            macro_index: 0,
            macro_kind: AnalyzedMacroKind::DefineProps,
            type_name: "ButtonProps".to_string(),
            import_source: "./types".to_string(),
            declaration: ResolvedTypeDeclaration {
                requested_name: "ButtonProps".to_string(),
                declaration_id: None,
                resolved_name: "ButtonProps".to_string(),
                canonical_source: "/src/types.ts".to_string(),
                span: Span::new(10, 50),
                kind: crate::ResolvedDeclarationKind::Interface,
                text: None,
            },
            native_props: vec![],
            props: vec![
                verter_analysis::AnalyzedPropField {
                    name: "msg".to_string(),
                    is_optional: false,
                    type_annotation: Some("string".to_string()),
                    span: Span::new(20, 30),
                    description: None,
                    tags: vec![],
                    resolution_source: Default::default(),
                    resolution_error: None,
                },
                verter_analysis::AnalyzedPropField {
                    name: "count".to_string(),
                    is_optional: true,
                    type_annotation: Some("number".to_string()),
                    span: Span::new(35, 45),
                    description: None,
                    tags: vec![],
                    resolution_source: Default::default(),
                    resolution_error: None,
                },
            ],
            emits: vec![],
            slots: vec![],
            jsdoc: None,
        }
    }

    #[test]
    fn resolved_macro_to_members_extracts_props() {
        let macro_meta = make_resolved_macro();
        let members = resolved_macro_to_members(&macro_meta);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "msg");
        assert!(!members[0].optional);
        assert_eq!(members[1].name, "count");
        assert!(members[1].optional);
    }

    #[test]
    fn resolved_macro_to_members_does_not_include_unrelated_fields() {
        let macro_meta = make_resolved_macro();
        let members = resolved_macro_to_members(&macro_meta);
        for m in &members {
            assert!(
                m.name == "msg" || m.name == "count",
                "unexpected member: {}",
                m.name
            );
        }
    }

    #[test]
    fn resolved_macro_to_expansion_has_correct_completeness() {
        let macro_meta = make_resolved_macro();
        let result = resolved_macro_to_expansion(&macro_meta);
        assert_eq!(result.completeness, ExpansionCompleteness::Exact);
        assert_eq!(result.members.len(), 2);
    }

    #[test]
    fn resolved_macro_to_expansion_empty_macro_is_lower_bound() {
        let mut macro_meta = make_resolved_macro();
        macro_meta.props.clear();
        let result = resolved_macro_to_expansion(&macro_meta);
        assert_eq!(result.completeness, ExpansionCompleteness::LowerBound);
    }

    #[test]
    fn resolved_macro_to_expansion_type_name_is_reference() {
        let macro_meta = make_resolved_macro();
        let result = resolved_macro_to_expansion(&macro_meta);
        match &result.type_expr {
            TypeExpr::Ref { name, .. } => assert_eq!(name.as_ref(), "ButtonProps"),
            other => panic!("expected Ref, got: {other:?}"),
        }
    }
}
