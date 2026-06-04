#![deny(missing_docs)]
//! Build a `.vue` SFC's PUBLIC component type through typeinfo.
//!
//! A TS consumer that writes `import Foo from './Foo.vue'` sees the SFC's
//! synthesized public component type: the instance surface carrying
//! `$props` / `$emit` / `$slots` (and, in later stages, expose). That surface
//! is synthesized from the SFC's macro type-arguments by
//! [`crate::resolver_core::vue_default_synth`], which injects a `default`
//! value symbol whose construct-signature return type IS the instance object.
//!
//! This module projects that synthesized instance object into the span-rich
//! [`TypeInfoSurface`] **through the shared typeinfo surface path** — it
//! dispatches the first-class `SemanticQueryKey::Instantiate{ .vue, "default", [] }`
//! query (whose `build_instantiate` branch lowers the synthesized instance
//! object to an `Object` surface) then runs the empty-path `Shallow`
//! projection — so a `.vue`'s public type resolves through typeinfo WITHOUT any
//! component-meta call. This is the [`TypeInfoQueryLevel::PublicType`] level.
//!
//! `Instantiate{ .vue, "default", [] }` is the SOLE semantic identity for a
//! `.vue`'s public instance: a `.vue`-importing-`.vue` reference
//! (`Ref("Foo")` → `DeclRef{Foo.vue, "default"}` → `Instantiate`) resolves
//! through the SAME keyed query, so recursive `.vue`-import expansion — a
//! `.vue` whose public surface embeds another imported `.vue` component's
//! surface — flows through this one shared resolver. Termination is by query
//! identity (the memo's same-key recursion sentinel + the
//! `push_instantiate_active` discipline), so a circular `A.vue ↔ B.vue` import
//! cannot hang.

use std::sync::Arc;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput,
};
use crate::typeinfo::surface::TypeInfoSurface;
use crate::typeinfo::types::TypeInfoQueryLevel;
use crate::VerterHost;

impl VerterHost {
    /// Resolve a `.vue` SFC's PUBLIC component type to its span-rich one-level
    /// [`TypeInfoSurface`] — the synthesized `{ $props, $emit, $slots }`
    /// instance surface — through typeinfo, WITHOUT calling component-meta.
    ///
    /// Returns `None` when `canonical_id` is not a loaded `.vue` carrying a
    /// synthesized `default` instance object (a plain `.ts` file, or a `.vue`
    /// with no type-based macros — there is no public component surface to
    /// build).
    ///
    /// `level` is accepted for symmetry with the level-aware request surface;
    /// the public component type IS the [`TypeInfoQueryLevel::PublicType`]
    /// projection, so callers pass `PublicType`. The argument keeps the public
    /// entry point honest about which level it serves and lets the caller's
    /// cache identity carry the level.
    #[must_use]
    pub fn resolve_vue_public_type(
        &self,
        canonical_id: &str,
        level: TypeInfoQueryLevel,
    ) -> Option<TypeInfoSurface> {
        debug_assert_eq!(
            level,
            TypeInfoQueryLevel::PublicType,
            "resolve_vue_public_type serves the PublicType level"
        );
        let _ = level;

        // The `.vue`'s synthesized public instance is the first-class semantic
        // query `Instantiate{ DeclIdentity(canonical, whole_hash, "default"), [] }`
        // (`build_instantiate`'s `.vue default` branch lowers the synthesized
        // `{ $props, $emit, $slots }` instance object to an `Object` surface).
        // This is the SAME keyed identity a `.vue`-importing-`.vue` reference
        // resolves through (`Ref("Foo")` → `DeclRef{Foo.vue, "default"}` →
        // `Instantiate`), so the public API and import recursion share ONE
        // semantic identity — there is no second resolver and no unkeyed
        // direct-lowering route.
        //
        // Materialize the `.vue`'s `IndexedReady` first (idempotent — warm hits
        // reuse it) to observe the live `whole_hash`. Gate on the SYNTHESIZED
        // `default` instance symbol's STRUCTURAL PROVENANCE flag BEFORE
        // dispatching so a plain `.ts` file (no synthesized `default`), a `.vue`
        // with no type-based macros, or a `.vue` carrying a USERLAND
        // `export default` (synthesis skipped) returns `None` here — the public
        // API stays honest about which canonicals have a synthesized public
        // component type, matching the `build_instantiate` branch's own
        // `is_synthesised_vue_default` gate.
        let indexed = self.ensure_indexed_ready(canonical_id)?;
        let default_symbol = indexed.shallow_state.value_symbol("default")?;
        if !default_symbol.is_synthesised_vue_default {
            return None;
        }
        // The synthesized default carries a construct-signature return type (the
        // instance object); its absence means no public instance surface.
        default_symbol
            .function_signature
            .as_ref()?
            .return_type
            .as_ref()?;
        let _whole_hash = indexed.whole_hash;

        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Intermediate-hop demand: the keyed query lowers the instance object in
        // `structural_transit(Navigate)` so member values stay shallow
        // (shallow-by-default). The empty-path `Shallow` terminal below
        // synthesises the one-level surface under publication demand.
        let base = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base: crate::semantic_query::DeclKey {
                canonical_id: Arc::from(canonical_id),
                decl_name: Arc::from("default"),
            },
            args: Arc::from(Vec::new().into_boxed_slice()),
            context: ProjectionReductionContext::structural_transit_with_mode(
                ProjectionMode::Navigate,
            ),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        // The public component type is a plain structural object
        // (`{ $props, $emit, $slots }`) — no macro own-body provenance applies
        // to the synthesized instance members, so the structural
        // `published(Shallow)` context is correct.
        self.project_shallow_surface_from_base(
            &host_ctx,
            &dispatch,
            base,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
    }
}
