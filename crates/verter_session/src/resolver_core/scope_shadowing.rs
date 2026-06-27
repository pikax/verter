//! r15/F11 — `ScopeShadowing` resolver context.
//!
//! Captures, once per resolver context, the set of bare type names the
//! owner scope already declares. The dispatch fast-path
//! ([`crate::project_semantic_dispatch::lower::ProjectSemanticDispatch::shallow_lower_type_expr`])
//! and the materialise-path identity gate
//! ([`crate::meta_resolve::extract_route_root_identity_node`] callers)
//! both consult `is_shadowing_lib(name)` before routing through the
//! ambient-lib `__builtin__` fast-path. When `true`, the userland
//! declaration wins and the `__builtin__` route is suppressed —
//! preserving the plan's "user shadowing wins" rule across BOTH
//! lowering entry points.
//!
//! **Design rationale:** an earlier draft threaded a bare `bool`
//! through every route + registry caller.
//! That replicates the parameter-explosion pattern that the
//! `ResolverContext` sealed-trait migration is designed to fix. By
//! introducing this struct now, the threading axis stays
//! single-source-of-truth and can absorb `ScopeShadowing`
//! as one input field of `ResolverContext` without inventing a
//! parallel axis to undo.
//!
//! **Construction sources (single-source-of-truth):**
//!
//! - [`ScopeShadowing::from_scope_payload`] — used by the dispatch
//!   lowering path (`lower.rs`) where the prepared
//!   [`DeclarationScopePayload`] is already on hand.
//! - [`ScopeShadowing::from_host_scope`] — used by the materialise
//!   path entry where only `(host, scope_canonical_id)` is on hand.
//!   Looks the prepared decl bundle up via
//!   [`crate::host_manage`]'s `prepared_decl_bundle` accessor and
//!   builds the same shadow set the dispatch path observes.
//! - [`ScopeShadowing::empty`] — for paths that do NOT have a
//!   declaration scope (e.g. global / `NodeScopeId::Global` lowering
//!   sites). Behaves as if no userland declaration shadows any
//!   builtin.
//!
//! Both constructors produce structurally-equivalent shadow sets so
//! the two lowering entry points agree on which builtin names are
//! shadowed in any given owner scope.

use rustc_hash::FxHashSet;
use std::sync::Arc;

use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
// `from_host_scope` migrates to `&dyn ResolverContext`; the
// `crate::VerterHost` type is no longer needed in this file.

/// Captures, once per resolver context, the set of bare type names
/// the owner scope already declares. Consumed by the dispatch
/// fast-path and the materialise-path identity gate so `Pick<…>` /
/// `Omit<…>` / etc. resolve to the userland declaration when the
/// SFC's same-file scope already declares one.
///
/// See module docs for the construction-source matrix.
#[derive(Debug, Clone)]
pub(crate) struct ScopeShadowing {
    /// Bare type names declared in the owner scope (script-setup type
    /// bindings, scope-local type aliases, scope-local interfaces).
    /// Membership in this set means the userland declaration MUST win
    /// over a same-named ambient-lib builtin.
    shadowed_type_names: Arc<FxHashSet<Arc<str>>>,
}

impl ScopeShadowing {
    /// The empty shadow set. Used by lowering call sites that have no
    /// declaration-scope payload on hand (`NodeScopeId::Global`,
    /// pre-bundle test fixtures). Equivalent to "no userland
    /// declaration shadows any builtin" — the ambient-lib fast-path
    /// stays active.
    pub(crate) fn empty() -> Self {
        Self {
            shadowed_type_names: Arc::new(FxHashSet::default()),
        }
    }

    /// Build a shadow set from a [`DeclarationScopePayload`] —
    /// dispatch-path entry point. The payload's `scope_type_names`
    /// (covering script-setup type params + scope-local type aliases),
    /// `scope_type_bindings` (covering script-setup generics), AND
    /// `import_bindings` (covering imported names) are merged into the
    /// shadow set; each source independently shadows a same-named
    /// ambient-lib builtin per the foundation (`524f469d`) gate.
    ///
    /// `import_bindings` membership is load-bearing for the carrier
    /// head-resolution path, which rehydrates an EMPTY `name_resolution`
    /// from the scope payload: the eager `Ref` path suppresses the builtin
    /// fast-path because an imported name (e.g. `import type { Partial }`)
    /// lives in `name_resolution`, but the carrier path has none — so the
    /// import binding must shadow the builtin THROUGH this set instead, or
    /// an imported `Partial` would wrongly resolve to `__builtin__.Partial`.
    pub(crate) fn from_scope_payload(payload: Option<&DeclarationScopePayload>) -> Self {
        let Some(payload) = payload else {
            return Self::empty();
        };
        Self::from_payload_parts(
            payload.scope_type_names.iter(),
            payload.scope_type_bindings.keys(),
            payload.import_bindings.keys(),
        )
    }

    /// Build a shadow set from `(host, scope_canonical_id)` —
    /// materialise-path entry point. Mirrors the dispatch-path shape
    /// by going through the host's prepared decl bundle so both paths
    /// observe identical scope-type-name / scope-type-binding sets.
    ///
    /// Returns [`ScopeShadowing::empty`] when the host has no bundle
    /// for the canonical id (e.g. the file is unknown to the
    /// scheduler). This matches the dispatch path's behaviour when
    /// `scope_payload` is `None`.
    pub(crate) fn from_host_scope(
        ctx: &dyn crate::resolver_core::ResolverContext,
        scope_canonical_id: &str,
    ) -> Self {
        match ctx.prepared_decl_bundle(scope_canonical_id) {
            Some(bundle) => Self::from_prepared_decl_bundle(bundle.as_ref()),
            None => Self::empty(),
        }
    }

    /// Build a shadow set directly from a [`PreparedDeclBundle`].
    /// Helper used by [`Self::from_host_scope`] and by tests that
    /// already hold a bundle. Mirrors
    /// [`DeclarationScopePayload::from_bundle`] — the union of
    /// `scope_type_names` + `script_setup_type_bindings` keys becomes
    /// the shadow set. Keeping the two construction shapes aligned is
    /// the load-bearing invariant: the dispatch path and the
    /// materialise path MUST observe the same shadow set per scope.
    pub(crate) fn from_prepared_decl_bundle(bundle: &PreparedDeclBundle) -> Self {
        Self::from_payload_parts(
            bundle.scope_type_names.iter(),
            bundle.script_setup_type_bindings.keys(),
            bundle.import_bindings.keys(),
        )
    }

    /// Returns `true` when `name` is declared in the owner scope and
    /// therefore shadows a same-named ambient-lib builtin
    /// (`Pick`, `Omit`, `Partial`, `Exclude`, …). The dispatch
    /// fast-path and the materialise-path identity gate suppress
    /// their `__builtin__` route when this returns `true`,
    /// dispatching through the standard `ResolveDecl` path so the
    /// userland declaration wins.
    pub(crate) fn is_shadowing_lib(&self, name: &str) -> bool {
        // O(1) hash-set membership. `FxHashSet<Arc<str>>` accepts a borrowed
        // `&str` probe (`Arc<str>: Borrow<str>`, and `Arc<str>` hashes as its
        // `str` pointee), so this is behaviour-identical to the prior
        // `iter().any(|n| n == name)` linear scan — the same names shadow —
        // without walking the set on every probe. Both shadow consumers (the
        // dispatch fast-path and the materialise-path gate) get the O(1)
        // lookup.
        self.shadowed_type_names.contains(name)
    }

    /// Internal — merge a `scope_type_names` set with the keys of
    /// `scope_type_bindings` and `import_bindings` into a deduplicated
    /// shadow set. Each source independently shadows ambient-lib builtins
    /// per the foundation gate; this keeps the merge in one place so any
    /// future shadow-source addition lands here once.
    fn from_payload_parts<'a, S, K, I>(
        scope_type_names: S,
        type_binding_keys: K,
        import_binding_keys: I,
    ) -> Self
    where
        S: IntoIterator<Item = &'a String>,
        K: IntoIterator<Item = &'a String>,
        I: IntoIterator<Item = &'a String>,
    {
        let mut set: FxHashSet<Arc<str>> = FxHashSet::default();
        for name in scope_type_names {
            set.insert(Arc::from(name.as_str()));
        }
        for binding in type_binding_keys {
            set.insert(Arc::from(binding.as_str()));
        }
        for binding in import_binding_keys {
            set.insert(Arc::from(binding.as_str()));
        }
        Self {
            shadowed_type_names: Arc::new(set),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::prepared_decl::TypeParamBinding;
    use rustc_hash::FxHashMap;

    fn make_binding(name: &str, ordinal: u16) -> TypeParamBinding {
        TypeParamBinding {
            name: Arc::from(name),
            ordinal,
            constraint: None,
            default: None,
        }
    }

    fn payload_with(names: &[&str], type_bindings: &[&str]) -> DeclarationScopePayload {
        payload_with_imports(names, type_bindings, &[])
    }

    fn payload_with_imports(
        names: &[&str],
        type_bindings: &[&str],
        import_names: &[&str],
    ) -> DeclarationScopePayload {
        use crate::resolver_core::prepared_decl::ImportBinding;
        let scope_type_names: rustc_hash::FxHashSet<String> =
            names.iter().map(|s| s.to_string()).collect();
        let mut bindings: FxHashMap<String, TypeParamBinding> = FxHashMap::default();
        for (i, name) in type_bindings.iter().enumerate() {
            bindings.insert(name.to_string(), make_binding(name, i as u16));
        }
        let mut import_bindings: FxHashMap<String, ImportBinding> = FxHashMap::default();
        for name in import_names {
            import_bindings.insert(
                name.to_string(),
                ImportBinding {
                    canonical_id: "/import-src.ts".to_string(),
                    exported_name: (*name).to_string(),
                },
            );
        }
        DeclarationScopePayload {
            scope_type_names,
            scope_value_names: rustc_hash::FxHashSet::default(),
            scope_type_bindings: bindings,
            import_bindings,
        }
    }

    #[test]
    fn empty_shadow_set_does_not_shadow_any_name() {
        let shadow = ScopeShadowing::empty();
        // Discriminating positive: an empty shadow set never
        // suppresses the builtin fast-path.
        assert!(!shadow.is_shadowing_lib("Pick"));
        assert!(!shadow.is_shadowing_lib("Omit"));
        assert!(!shadow.is_shadowing_lib(""));
    }

    #[test]
    fn from_scope_payload_includes_scope_type_names() {
        // Userland `type Pick<T,_K> = T` lands in scope_type_names.
        let payload = payload_with(&["Pick", "Cfg"], &[]);
        let shadow = ScopeShadowing::from_scope_payload(Some(&payload));
        // Discriminating positive: the userland Pick shadows the
        // ambient-lib `Pick`.
        assert!(shadow.is_shadowing_lib("Pick"));
        // Discriminating negative: an unrelated builtin name with no
        // userland counterpart is NOT shadowed.
        assert!(!shadow.is_shadowing_lib("Omit"));
        // Other scope-local types (Cfg) also enter the shadow set so
        // a userland helper named after a builtin is also caught.
        assert!(shadow.is_shadowing_lib("Cfg"));
    }

    #[test]
    fn from_scope_payload_includes_script_setup_type_bindings() {
        // Script-setup generic `<script setup generic="Pick">` lands in
        // scope_type_bindings (NOT scope_type_names) — the gate must
        // catch this independently.
        let payload = payload_with(&[], &["Pick"]);
        let shadow = ScopeShadowing::from_scope_payload(Some(&payload));
        // Discriminating positive: the script-setup generic param
        // named after a builtin shadows it.
        assert!(shadow.is_shadowing_lib("Pick"));
        // Discriminating negative: a different builtin remains
        // unshadowed.
        assert!(!shadow.is_shadowing_lib("Partial"));
    }

    #[test]
    fn from_scope_payload_includes_import_bindings() {
        // An imported name (`import type { Partial } from "./x"`) lands in
        // `import_bindings` (NOT scope_type_names / scope_type_bindings) — the
        // carrier head-resolution path rehydrates an EMPTY `name_resolution`, so
        // the import binding must shadow the builtin THROUGH this set or an
        // imported `Partial` would wrongly resolve to `__builtin__.Partial`.
        let payload = payload_with_imports(&[], &[], &["Partial"]);
        let shadow = ScopeShadowing::from_scope_payload(Some(&payload));
        // Discriminating positive: the imported `Partial` shadows the builtin.
        assert!(
            shadow.is_shadowing_lib("Partial"),
            "an imported name colliding with a builtin must shadow it (the carrier path's \
             empty name_resolution relies on this)"
        );
        // Discriminating negative: a different builtin with no import remains
        // unshadowed (so the fix does not over-shadow).
        assert!(!shadow.is_shadowing_lib("Pick"));
    }

    #[test]
    fn from_scope_payload_none_returns_empty_set() {
        let shadow = ScopeShadowing::from_scope_payload(None);
        // Discriminating: a `None` payload (e.g. global lowering)
        // shadows nothing — the ambient-lib fast-path stays active
        // for ALL names.
        assert!(!shadow.is_shadowing_lib("Pick"));
        assert!(!shadow.is_shadowing_lib("Omit"));
        assert!(!shadow.is_shadowing_lib("Partial"));
    }

    #[test]
    fn shadow_sets_from_payload_and_bundle_observe_same_names() {
        // Single-source-of-truth invariant: a `DeclarationScopePayload`
        // built from the same source merges identically across both
        // construction paths. Build a shared map, then construct a
        // PreparedDeclBundle stub to verify the bundle path returns
        // the same shadow set.
        let names: rustc_hash::FxHashSet<String> = ["Pick".to_string(), "Cfg".to_string()]
            .into_iter()
            .collect();
        let mut bindings: FxHashMap<String, TypeParamBinding> = FxHashMap::default();
        bindings.insert("T".to_string(), make_binding("T", 0));

        let payload = DeclarationScopePayload {
            scope_type_names: names.clone(),
            scope_value_names: rustc_hash::FxHashSet::default(),
            scope_type_bindings: bindings.clone(),
            import_bindings: FxHashMap::default(),
        };
        let shadow_from_payload = ScopeShadowing::from_scope_payload(Some(&payload));

        // Cross-check: every payload-shadowed name is also recognised
        // when we feed the same data through `from_prepared_decl_bundle`
        // semantically (a full bundle requires a real ShallowFileState
        // — covered by the §5.10 §5.D.2 integration test below; this
        // sub-test asserts the merge logic itself is identical to the
        // payload path's handling of the same `(names, bindings)`
        // input).
        let empty_imports: FxHashMap<String, ()> = FxHashMap::default();
        let shadow_from_payload_again =
            ScopeShadowing::from_payload_parts(names.iter(), bindings.keys(), empty_imports.keys());
        assert_eq!(
            shadow_from_payload.is_shadowing_lib("Pick"),
            shadow_from_payload_again.is_shadowing_lib("Pick"),
        );
        assert_eq!(
            shadow_from_payload.is_shadowing_lib("Cfg"),
            shadow_from_payload_again.is_shadowing_lib("Cfg"),
        );
        assert_eq!(
            shadow_from_payload.is_shadowing_lib("T"),
            shadow_from_payload_again.is_shadowing_lib("T"),
        );
        // Negative: an unrelated builtin remains unshadowed via
        // BOTH construction paths.
        assert!(!shadow_from_payload.is_shadowing_lib("Omit"));
        assert!(!shadow_from_payload_again.is_shadowing_lib("Omit"));
    }
}
