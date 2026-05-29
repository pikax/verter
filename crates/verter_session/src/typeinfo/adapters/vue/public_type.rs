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
//! [`TypeInfoSurface`] **through the shared typeinfo surface path** (the same
//! lowering + empty-path `Shallow` projection `resolve_shallow_surface` uses) —
//! so a `.vue`'s public type resolves through typeinfo WITHOUT any
//! component-meta call. This is the [`TypeInfoQueryLevel::PublicType`] level.
//!
//! Scope: U3a builds the DIRECT `.vue` public type (the one-level
//! `{ $props, $emit, $slots }` instance surface). Recursive `.vue`-import
//! expansion (a `.vue` that imports another `.vue` component) is Stage 3 and
//! out of scope here.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};
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

        // The synthesized `default` value symbol's construct-signature return
        // type is the instance object `{ $props, $emit, $slots }`
        // (`vue_default_synth::synthesise_vue_default_value_symbol`). It lives
        // on the SFC's cache-owned `ShallowFileState`, materialized once per
        // content hash through the shared host path — we read it, never rebuild
        // it. A non-`.vue` / macro-less file has no such symbol and yields
        // `None`.
        let shallow = self.shallow_file_state(canonical_id)?;
        let default_symbol = shallow.value_symbol("default")?;
        let instance_shape: &TypeExpr = default_symbol
            .function_signature
            .as_ref()?
            .return_type
            .as_ref()?;

        // Lower the synthesized instance object through the shared lowering
        // dispatch in the SFC's scope, then project the one-level surface via
        // the same empty-path `Shallow` synthesiser the named-declaration
        // accessor uses. The instance object is an inline `TSTypeLiteral`-shaped
        // `ObjectExpr`; `Navigate` lowering keeps member values shallow
        // (shallow-by-default), and the publication terminal walks it under
        // `Published(Shallow)`.
        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        let base = dispatch.lower_type_expr_in_scope_with_mode(
            canonical_id,
            instance_shape,
            ProjectionMode::Navigate,
        )?;

        // The public component type is a plain structural object
        // (`{ $props, $emit, $slots }`) — no macro own-body provenance applies
        // to the synthesized instance members, so the structural
        // `published(Shallow)` context is correct.
        self.project_shallow_surface_from_base(
            &host_ctx,
            &dispatch,
            base,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
    }
}
