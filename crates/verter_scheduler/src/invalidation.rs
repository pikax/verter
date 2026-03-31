//! Three-tier smart invalidation logic.
//!
//! When a dependency's exports change, this module determines whether
//! a dependent file needs recompilation:
//!
//! - **Tier 1**: No export signatures → always invalidate
//! - **Tier 2**: Export-level hashing → invalidate only if macro-consumed exports changed
//! - **Tier 3**: Cross-file type resolution → invalidate only if resolved type shape changed
//!
//! Ported from `verter_session::deps`.

use std::collections::BTreeSet;

/// 128-bit hash, same as `verter_session::types::Hash16`.
pub type Hash16 = [u8; 16];

/// Result of invalidation check for a single dependent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationDecision {
    /// Dependent must be recompiled.
    Invalidate,
    /// Dependent does not need recompilation.
    Skip,
}

/// An export signature from a dependency file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSig {
    pub name: String,
    pub declaration_hash: Hash16,
    pub is_type: bool,
}

/// A macro type dependency from a dependent file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroTypeDep {
    pub type_name: String,
    pub import_source: String,
}

/// An import from a dependent file's script analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    pub source: String,
    pub is_type_only: bool,
}

/// Compute which export names changed between old and new signature sets.
pub fn compute_changed_exports(old_sigs: &[ExportSig], new_sigs: &[ExportSig]) -> BTreeSet<String> {
    let mut changed = BTreeSet::new();

    // Exports removed (present in old but not new)
    for old in old_sigs {
        if !new_sigs.iter().any(|n| n.name == old.name) {
            changed.insert(old.name.clone());
        }
    }

    // Exports added or hash changed
    for new in new_sigs {
        match old_sigs.iter().find(|o| o.name == new.name) {
            None => {
                changed.insert(new.name.clone());
            }
            Some(old) if old.declaration_hash != new.declaration_hash => {
                changed.insert(new.name.clone());
            }
            _ => {}
        }
    }

    changed
}

/// Context for checking whether a dependent should be invalidated.
pub struct InvalidationContext<'a> {
    /// Macro type deps from the dependent's analysis.
    pub macro_type_deps: &'a [MacroTypeDep],
    /// Imports from the dependent's script analysis.
    pub imports: &'a [ImportInfo],
    /// Whether the dependent has the dependency in its forward dep set.
    pub has_dependency_registered: bool,
    /// Callback to check if an import resolves to the dependency.
    /// Takes `(import_source, dependency_id)` and returns `true` if they match.
    pub import_resolves: &'a dyn Fn(&str, &str) -> bool,
}

/// Determine whether a dependent should be invalidated given changed exports.
///
/// This is a pure decision function — it does not mutate any state.
/// Tier 3 (cross-file type resolution with re-hashing) is handled externally
/// by the caller, which can provide resolved type hashes.
pub fn should_invalidate(
    dependency_id: &str,
    changed_exports: &BTreeSet<String>,
    no_signatures: bool,
    ctx: &InvalidationContext<'_>,
) -> InvalidationDecision {
    // Tier 1: No signatures → always invalidate
    if no_signatures {
        return InvalidationDecision::Invalidate;
    }

    // No exports changed → no invalidation needed
    if changed_exports.is_empty() {
        return InvalidationDecision::Skip;
    }

    // Check macro type deps on this dependency
    let matching_deps: Vec<&MacroTypeDep> = ctx
        .macro_type_deps
        .iter()
        .filter(|dep| (ctx.import_resolves)(&dep.import_source, dependency_id))
        .collect();

    if !matching_deps.is_empty() {
        // Tier 2: Check if any macro-consumed types are in changed exports
        let has_changed = matching_deps
            .iter()
            .any(|dep| changed_exports.contains(&dep.type_name));

        if !has_changed {
            return InvalidationDecision::Skip;
        }

        // Changed macro-consumed exports → invalidate (Tier 3 handled externally)
        return InvalidationDecision::Invalidate;
    }

    // Check runtime imports
    let has_runtime_import = ctx
        .imports
        .iter()
        .any(|imp| !imp.is_type_only && (ctx.import_resolves)(&imp.source, dependency_id));

    if has_runtime_import {
        return InvalidationDecision::Invalidate;
    }

    // Registered dependency but no matching imports in analysis
    if ctx.has_dependency_registered
        && ctx
            .imports
            .iter()
            .all(|imp| !(ctx.import_resolves)(&imp.source, dependency_id))
    {
        return InvalidationDecision::Invalidate;
    }

    // Type-only imports not used by macros: no invalidation
    InvalidationDecision::Skip
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sigs(names: &[(&str, Hash16)]) -> Vec<ExportSig> {
        names
            .iter()
            .map(|&(name, hash)| ExportSig {
                name: name.to_string(),
                declaration_hash: hash,
                is_type: false,
            })
            .collect()
    }

    fn always_resolves(_import: &str, _dep: &str) -> bool {
        true
    }

    fn never_resolves(_import: &str, _dep: &str) -> bool {
        false
    }

    fn resolves_source(import: &str, dep: &str) -> bool {
        // Simple: import "./types" resolves to "/src/types.ts" if dep ends with types.ts
        dep.contains(&import.trim_start_matches("./"))
    }

    // ── compute_changed_exports ──

    #[test]
    fn no_changes_returns_empty() {
        let sigs = make_sigs(&[("Foo", [1; 16]), ("Bar", [2; 16])]);
        let changed = compute_changed_exports(&sigs, &sigs);
        assert!(changed.is_empty());
    }

    #[test]
    fn added_export_detected() {
        let old = make_sigs(&[("Foo", [1; 16])]);
        let new = make_sigs(&[("Foo", [1; 16]), ("Bar", [2; 16])]);
        let changed = compute_changed_exports(&old, &new);
        assert_eq!(changed, BTreeSet::from(["Bar".to_string()]));
    }

    #[test]
    fn removed_export_detected() {
        let old = make_sigs(&[("Foo", [1; 16]), ("Bar", [2; 16])]);
        let new = make_sigs(&[("Foo", [1; 16])]);
        let changed = compute_changed_exports(&old, &new);
        assert_eq!(changed, BTreeSet::from(["Bar".to_string()]));
    }

    #[test]
    fn hash_change_detected() {
        let old = make_sigs(&[("Foo", [1; 16])]);
        let new = make_sigs(&[("Foo", [99; 16])]);
        let changed = compute_changed_exports(&old, &new);
        assert_eq!(changed, BTreeSet::from(["Foo".to_string()]));
    }

    // ── Tier 1: No signatures ──

    #[test]
    fn tier1_no_signatures_always_invalidates() {
        let ctx = InvalidationContext {
            macro_type_deps: &[],
            imports: &[],
            has_dependency_registered: false,
            import_resolves: &never_resolves,
        };
        let result = should_invalidate(
            "/dep.ts",
            &BTreeSet::new(),
            true, // no signatures
            &ctx,
        );
        assert_eq!(result, InvalidationDecision::Invalidate);
    }

    // ── Tier 2: Export-level macro deps ──

    #[test]
    fn tier2_no_macro_deps_on_changed_exports_skips() {
        let macro_deps = vec![MacroTypeDep {
            type_name: "FooProps".to_string(),
            import_source: "./types".to_string(),
        }];
        let ctx = InvalidationContext {
            macro_type_deps: &macro_deps,
            imports: &[],
            has_dependency_registered: false,
            import_resolves: &always_resolves,
        };
        // "BarProps" changed, but macro only uses "FooProps"
        let changed = BTreeSet::from(["BarProps".to_string()]);
        let result = should_invalidate("/dep.ts", &changed, false, &ctx);
        assert_eq!(result, InvalidationDecision::Skip);
    }

    #[test]
    fn tier2_macro_dep_on_changed_export_invalidates() {
        let macro_deps = vec![MacroTypeDep {
            type_name: "FooProps".to_string(),
            import_source: "./types".to_string(),
        }];
        let ctx = InvalidationContext {
            macro_type_deps: &macro_deps,
            imports: &[],
            has_dependency_registered: false,
            import_resolves: &always_resolves,
        };
        let changed = BTreeSet::from(["FooProps".to_string()]);
        let result = should_invalidate("/dep.ts", &changed, false, &ctx);
        assert_eq!(result, InvalidationDecision::Invalidate);
    }

    // ── Runtime imports ──

    #[test]
    fn runtime_import_on_any_change_invalidates() {
        let imports = vec![ImportInfo {
            source: "./dep".to_string(),
            is_type_only: false,
        }];
        let ctx = InvalidationContext {
            macro_type_deps: &[],
            imports: &imports,
            has_dependency_registered: false,
            import_resolves: &always_resolves,
        };
        let changed = BTreeSet::from(["anything".to_string()]);
        let result = should_invalidate("/dep.ts", &changed, false, &ctx);
        assert_eq!(result, InvalidationDecision::Invalidate);
    }

    #[test]
    fn type_only_import_without_macro_skips() {
        let imports = vec![ImportInfo {
            source: "./types".to_string(),
            is_type_only: true,
        }];
        let ctx = InvalidationContext {
            macro_type_deps: &[],
            imports: &imports,
            has_dependency_registered: false,
            import_resolves: &always_resolves,
        };
        let changed = BTreeSet::from(["SomeType".to_string()]);
        let result = should_invalidate("/dep.ts", &changed, false, &ctx);
        assert_eq!(result, InvalidationDecision::Skip);
    }

    // ── Registered dependency without matching imports ──

    #[test]
    fn registered_dep_no_matching_imports_invalidates() {
        let ctx = InvalidationContext {
            macro_type_deps: &[],
            imports: &[], // no imports match
            has_dependency_registered: true,
            import_resolves: &never_resolves,
        };
        let changed = BTreeSet::from(["Foo".to_string()]);
        let result = should_invalidate("/dep.ts", &changed, false, &ctx);
        assert_eq!(result, InvalidationDecision::Invalidate);
    }

    // ── No changes = no invalidation ──

    #[test]
    fn no_changed_exports_skips() {
        let ctx = InvalidationContext {
            macro_type_deps: &[],
            imports: &[],
            has_dependency_registered: true,
            import_resolves: &always_resolves,
        };
        let result = should_invalidate("/dep.ts", &BTreeSet::new(), false, &ctx);
        assert_eq!(result, InvalidationDecision::Skip);
    }

    // ── Import resolution filtering ──

    #[test]
    fn macro_dep_only_checked_for_matching_imports() {
        let macro_deps = vec![MacroTypeDep {
            type_name: "FooProps".to_string(),
            import_source: "./other".to_string(), // doesn't resolve to dep
        }];
        let ctx = InvalidationContext {
            macro_type_deps: &macro_deps,
            imports: &[],
            has_dependency_registered: false,
            import_resolves: &never_resolves, // ./other doesn't resolve to /dep.ts
        };
        let changed = BTreeSet::from(["FooProps".to_string()]);
        let result = should_invalidate("/dep.ts", &changed, false, &ctx);
        // No matching macro deps → skip (type-only imports check also empty)
        assert_eq!(result, InvalidationDecision::Skip);
    }
}
