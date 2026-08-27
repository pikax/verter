use std::collections::HashSet;

use super::{build_known_file_index, normalize_known_file_id, resolve_known_dependency_id};

#[must_use]
pub fn collect_resolvable_module_reference_specifiers(
    module_references: &[crate::analysis::AnalyzedModuleReference],
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut specifiers = Vec::new();

    for reference in module_references {
        let candidates: &[String] = match reference.analyzability {
            crate::analysis::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = reference.literal_specifier.as_ref() {
                    if !specifier.is_empty() && seen.insert(specifier.clone()) {
                        specifiers.push(specifier.clone());
                    }
                }
                continue;
            }
            crate::analysis::ModuleReferenceAnalyzability::FiniteSet => {
                &reference.finite_specifiers
            }
            crate::analysis::ModuleReferenceAnalyzability::UnknownDynamic => continue,
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
    module_references: &[crate::analysis::AnalyzedModuleReference],
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
