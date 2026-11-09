use std::collections::HashSet;

use crate::resolve_known_dependency_id;
use verter_session_query::resolution::{build_known_file_index, normalize_known_file_id};

/// The span-free facts of one module reference that specifier collection and
/// known-file resolution read: how analyzable it is and which specifiers it
/// names. Positions are not an input of either operation, so a caller that
/// holds a reference in a wire encoding it cannot convert (a read-back
/// without its source) supplies these facts without inventing a span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvableModuleReference {
    pub analyzability: verter_session_query::analysis::types::ModuleReferenceAnalyzability,
    pub literal_specifier: Option<String>,
    pub finite_specifiers: Vec<String>,
}

#[must_use]
pub fn collect_resolvable_module_reference_specifiers(
    module_references: &[ResolvableModuleReference],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut specifiers = Vec::new();

    for reference in module_references {
        let candidates: &[String] = match reference.analyzability {
            verter_session_query::analysis::types::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = reference.literal_specifier.as_ref() {
                    if !specifier.is_empty() && seen.insert(specifier.clone()) {
                        specifiers.push(specifier.clone());
                    }
                }
                continue;
            }
            verter_session_query::analysis::types::ModuleReferenceAnalyzability::FiniteSet => {
                &reference.finite_specifiers
            }
            verter_session_query::analysis::types::ModuleReferenceAnalyzability::UnknownDynamic => {
                continue
            }
        };

        for specifier in candidates {
            if !specifier.is_empty() && seen.insert(specifier.clone()) {
                specifiers.push(specifier.clone());
            }
        }
    }
    specifiers
}

#[must_use]
pub fn resolve_known_module_reference_dependencies(
    owner_id: &str,
    module_references: &[ResolvableModuleReference],
    known_ids: &[String],
    extensions: &[String],
) -> Vec<String> {
    let known_index = build_known_file_index(known_ids);
    let owner_id = normalize_known_file_id(owner_id);
    let mut resolved = Vec::new();
    let mut seen = HashSet::new();

    for specifier in collect_resolvable_module_reference_specifiers(module_references) {
        let Some(match_id) =
            resolve_known_dependency_id(owner_id.as_str(), &specifier, &known_index, extensions)
        else {
            continue;
        };
        let match_id_normalized = normalize_known_file_id(&match_id);
        if match_id_normalized != owner_id && seen.insert(match_id_normalized) {
            resolved.push(match_id);
        }
    }
    resolved
}

#[cfg(test)]
#[path = "module_reference_resolution_tests.rs"]
mod module_reference_resolution_tests;
