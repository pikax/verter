//! r15/F11 — `ScopeShadowing` resolver context.
//!
//! Captures, once per resolver context, the set of bare type names the
//! owner scope already declares. The dispatch fast-path
//! ([`crate::project_semantic_dispatch::lower::ProjectSemanticDispatch::shallow_lower_type_expr`])
//! and the graph-native root-identity predicates
//! (`meta_resolve::graph_predicates`) both consult `is_shadowing_lib(name)` before routing through the
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

use std::sync::Arc;

use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::prepared_decl::PreparedDeclBundle;
// `from_host_scope` migrates to `&dyn ResolverContext`; the
// `crate::VerterHost` type is no longer needed in this file.

/// The bare type names the owner scope already declares. Consumed by the
/// dispatch fast-path and the materialise-path identity gate so `Pick<…>`
/// / `Omit<…>` / etc. resolve to the userland declaration when the SFC's
/// same-file scope already declares one.
///
/// A VIEW over the owner scope's declaration-scope payload, as
/// [`DeclarationScopePayload`] itself is: construction is one refcount
/// bump, and a probe reads the prepared bundle's three name surfaces in
/// place. Folding them into a set of its own made every construction walk
/// every name the file declares — one construction per instantiation, so
/// a file's instantiations cost the square of its size.
///
/// See module docs for the construction-source matrix.
#[derive(Debug, Clone)]
pub(crate) struct ScopeShadowing {
    /// The owner scope's payload; `None` shadows nothing.
    payload: Option<DeclarationScopePayload>,
}

impl ScopeShadowing {
    /// The empty shadow set. Used by lowering call sites that have no
    /// declaration-scope payload on hand (`NodeScopeId::Global`,
    /// pre-bundle test fixtures). Equivalent to "no userland
    /// declaration shadows any builtin" — the ambient-lib fast-path
    /// stays active.
    pub(crate) fn empty() -> Self {
        Self { payload: None }
    }

    /// The shadow set of a [`DeclarationScopePayload`] — dispatch-path
    /// entry point. The payload's `scope_type_names` (covering
    /// script-setup type params + scope-local type aliases),
    /// `scope_type_bindings` (covering script-setup generics), AND
    /// `import_bindings` (covering imported names) each shadow a
    /// same-named ambient-lib builtin per the foundation (`524f469d`)
    /// gate.
    ///
    /// `import_bindings` membership is load-bearing for the carrier
    /// head-resolution path, which rehydrates an EMPTY `name_resolution`
    /// from the scope payload: the eager `Ref` path suppresses the builtin
    /// fast-path because an imported name (e.g. `import type { Partial }`)
    /// lives in `name_resolution`, but the carrier path has none — so the
    /// import binding must shadow the builtin THROUGH this set instead, or
    /// an imported `Partial` would wrongly resolve to `__builtin__.Partial`.
    pub(crate) fn from_scope_payload(payload: Option<&DeclarationScopePayload>) -> Self {
        Self {
            payload: payload.cloned(),
        }
    }

    /// The shadow set of `(host, scope_canonical_id)` — materialise-path
    /// entry point. Mirrors the dispatch-path shape by going through the
    /// host's prepared decl bundle so both paths observe identical
    /// scope-type-name / scope-type-binding sets.
    ///
    /// Returns [`ScopeShadowing::empty`] when the host has no bundle
    /// for the canonical id (e.g. the file is unknown to the
    /// scheduler). This matches the dispatch path's behaviour when
    /// `scope_payload` is `None`.
    pub(crate) fn from_host_scope(
        ctx: &dyn crate::resolver_core::ResolverContext,
        scope_canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Self {
        match ctx.prepared_decl_bundle(scope_canonical_id) {
            Some(bundle) => Self::from_prepared_decl_bundle(&bundle, owner),
            None => Self::empty(),
        }
    }

    /// The shadow set of a [`PreparedDeclBundle`]'s owner scope: the SAME
    /// three bundle surfaces [`Self::from_scope_payload`] reads through the
    /// payload view (`scope_type_names` + `script_setup_type_bindings`
    /// keys + `import_bindings` keys). Keeping the two construction shapes
    /// aligned is the load-bearing invariant: the dispatch path and the
    /// materialise path MUST observe the same shadow set per scope.
    pub(crate) fn from_prepared_decl_bundle(
        bundle: &Arc<PreparedDeclBundle>,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Self {
        Self::from_scope_payload(Some(&DeclarationScopePayload::from_bundle(bundle, owner)))
    }

    /// Returns `true` when `name` is declared in the owner scope and
    /// therefore shadows a same-named ambient-lib builtin
    /// (`Pick`, `Omit`, `Partial`, `Exclude`, …). The dispatch
    /// fast-path and the materialise-path identity gate suppress
    /// their `__builtin__` route when this returns `true`,
    /// dispatching through the standard `ResolveDecl` path so the
    /// userland declaration wins. Three hash probes, whatever the scope's
    /// size.
    pub(crate) fn is_shadowing_lib(&self, name: &str) -> bool {
        Self::scope_payload_shadows_lib(self.payload.as_ref(), name)
    }

    /// Whether `name` is shadowed in `payload`'s scope, answered without
    /// constructing a shadow set: it probes the three sources
    /// [`Self::is_shadowing_lib`] reads — pinned by
    /// `payload_probe_agrees_with_the_folded_shadow_set`.
    pub(crate) fn scope_payload_shadows_lib(
        payload: Option<&DeclarationScopePayload>,
        name: &str,
    ) -> bool {
        payload.is_some_and(|payload| {
            payload.scope_type_names().contains(name)
                || payload.scope_type_bindings().contains_key(name)
                || payload.import_bindings().contains_key(name)
        })
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
        let (bundle, owner) = bundle_with_imports(names, type_bindings, import_names);
        DeclarationScopePayload::from_bundle(&bundle, owner)
    }

    fn bundle_with_imports(
        names: &[&str],
        type_bindings: &[&str],
        import_names: &[&str],
    ) -> (Arc<PreparedDeclBundle>, verter_type_expr::TopLevelOwnerId) {
        use crate::resolver_core::prepared_decl::{
            build_prepared_decl_bundle, ImportBinding, ImportCanonicalization,
        };
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
        // The payload is a shared view over a prepared-decl bundle:
        // build a minimal bundle and stamp the fixture's surfaces onto
        // its (pub) scope fields.
        let state = crate::resolver_core::ShallowFileState::service_backed_for_test("");
        let interner =
            Arc::new(crate::identity_interner::IdentityInterner::with_process_local_account());
        let mut bundle = build_prepared_decl_bundle(
            "/shadow-fixture.ts",
            state,
            FxHashMap::default(),
            bindings.clone(),
            ImportCanonicalization::default(),
            &interner,
        );
        let owner = verter_type_expr::TopLevelOwnerId::instance(0);
        let owner_scope = bundle.owner_scopes.entry(owner).or_default();
        owner_scope.scope_type_names = scope_type_names;
        owner_scope.import_bindings = import_bindings;
        owner_scope.script_setup_type_bindings = bindings;
        (Arc::new(bundle), owner)
    }

    #[test]
    fn payload_probe_agrees_with_the_folded_shadow_set() {
        let payload = payload_with_imports(&["Local"], &["Binding"], &["Awaited"]);
        let folded = ScopeShadowing::from_scope_payload(Some(&payload));
        for name in ["Local", "Binding", "Awaited", "Partial", ""] {
            assert_eq!(
                ScopeShadowing::scope_payload_shadows_lib(Some(&payload), name),
                folded.is_shadowing_lib(name),
                "{name:?}"
            );
        }
        for source in ["Local", "Binding", "Awaited"] {
            assert!(
                ScopeShadowing::scope_payload_shadows_lib(Some(&payload), source),
                "each of the three sources shadows on its own: {source:?}"
            );
        }
        assert!(!ScopeShadowing::scope_payload_shadows_lib(None, "Awaited"));
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

    /// A shadow set is a view: constructing one takes a reference to the
    /// owner scope's prepared bundle and copies none of its names, so a
    /// scope with thousands of declared types costs one refcount per
    /// construction — the dispatch constructs one per instantiation.
    #[test]
    fn a_shadow_set_views_its_bundle_without_copying_names() {
        let names: Vec<String> = (0..3000).map(|index| format!("T{index}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let (bundle, owner) = bundle_with_imports(&names, &[], &[]);
        let before = Arc::strong_count(&bundle);
        let shadows: Vec<ScopeShadowing> = (0..8)
            .map(|_| ScopeShadowing::from_prepared_decl_bundle(&bundle, owner))
            .collect();
        assert_eq!(
            Arc::strong_count(&bundle),
            before + shadows.len(),
            "each shadow set holds the bundle it reads"
        );
        for shadow in &shadows {
            let payload = shadow.payload.as_ref().expect("a bundle-backed shadow set");
            assert!(Arc::ptr_eq(payload.bundle_for_tests(), &bundle));
            assert!(shadow.is_shadowing_lib("T2999"));
        }
        drop(shadows);
        assert_eq!(Arc::strong_count(&bundle), before);
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
        // Single-source-of-truth invariant: the payload path and the bundle
        // path read the same three surfaces of the same owner scope.
        let (bundle, owner) = bundle_with_imports(&["Pick", "Cfg"], &["T"], &["Imported"]);
        let shadow_from_payload = ScopeShadowing::from_scope_payload(Some(
            &DeclarationScopePayload::from_bundle(&bundle, owner),
        ));
        let shadow_from_bundle = ScopeShadowing::from_prepared_decl_bundle(&bundle, owner);
        for name in ["Pick", "Cfg", "T", "Imported", "Omit", ""] {
            assert_eq!(
                shadow_from_payload.is_shadowing_lib(name),
                shadow_from_bundle.is_shadowing_lib(name),
                "{name:?}"
            );
        }
        for name in ["Pick", "Cfg", "T", "Imported"] {
            assert!(shadow_from_bundle.is_shadowing_lib(name), "{name:?}");
        }
        // Negative: an unrelated builtin remains unshadowed via
        // BOTH construction paths.
        assert!(!shadow_from_payload.is_shadowing_lib("Omit"));
        assert!(!shadow_from_bundle.is_shadowing_lib("Omit"));
        // Another owner scope of the same bundle declares nothing.
        let other = ScopeShadowing::from_prepared_decl_bundle(
            &bundle,
            verter_type_expr::TopLevelOwnerId::instance(1),
        );
        assert!(!other.is_shadowing_lib("Pick"));
    }
}
