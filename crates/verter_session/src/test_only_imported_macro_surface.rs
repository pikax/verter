//! Test-only probes for the typed-IR `ImportedMacroSurface` bridge
//! and its eager `ResolvedMacroMeta` counterpart.
//!
//! Lets the hermetic integration tests in
//! `tests/imported_macro_surface_bridge.rs` and
//! `tests/stage2b1_macro_authority_equivalence.rs` exercise the
//! bridge's projection accessors AND the eager/lazy macro-authority
//! equivalence against a `&VerterHost` without exposing the
//! `pub(crate)` `ResolverContext` trait to the public API. Production
//! code MUST NOT consume this module — `tests/architecture_guards.rs`
//! enforces (`test_only_module_is_only_consumed_by_test_files`).
//!
//! Extracted from `lib.rs` to keep the crate root under its line
//! ceiling (`tests/no_lib_rs_growth.rs`); re-attached via
//! `#[path]` under `pub mod test_only`.

use std::sync::Arc;

use crate::resolver_core::{
    with_bare_host_ctx_for_test, ImportedDeclarationIdentity, ImportedMacroSurface,
    ResolvedMacroSurface,
};
use crate::semantic_query::{ProjectionMode, QueryResult, SemanticNodeId};
use crate::VerterHost;

/// Build a bridge identity from `(canonical, type_name)`
/// plus a `whole_hash` byte array. Returns a probe whose
/// methods take a `&VerterHost` and dispatch through the
/// crate-internal `ResolverContext`.
///
/// Test fixtures own the byte array directly so the probe
/// does not need to know about `verter_session`'s
/// internal `Hash16` alias.
pub struct ImportedMacroSurfaceProbe(ImportedMacroSurface);

impl ImportedMacroSurfaceProbe {
    /// Build a probe targeting the imported declaration
    /// `(canonical, type_name)` at content hash
    /// `whole_hash`.
    #[inline]
    #[must_use]
    pub fn new(canonical: Arc<str>, type_name: Arc<str>, whole_hash: [u8; 16]) -> Self {
        Self(ImportedMacroSurface::new(ImportedDeclarationIdentity::new(
            canonical, type_name, whole_hash,
        )))
    }

    /// Resolve the imported declaration to its root
    /// `SemanticNodeId`. Drives
    /// `ImportedMacroSurface::resolve_root` through a
    /// hermetic `&dyn ResolverContext` constructed via
    /// `with_bare_host_ctx_for_test`.
    ///
    /// Returns the raw `QueryResult` so the test can
    /// assert on `Value` / `Error(Miss)` /
    /// `Error(DeclPlaceholder)` shapes.
    pub fn resolve_root(&self, host: &VerterHost) -> QueryResult<SemanticNodeId> {
        with_bare_host_ctx_for_test(host, |ctx| self.0.resolve_root(ctx))
    }

    /// Project a named member of the imported
    /// declaration. Drives
    /// `ImportedMacroSurface::project_named_member`
    /// through the same hermetic context as
    /// [`Self::resolve_root`].
    pub fn project_named_member(
        &self,
        host: &VerterHost,
        name: &str,
        mode: ProjectionMode,
    ) -> QueryResult<SemanticNodeId> {
        with_bare_host_ctx_for_test(host, |ctx| self.0.project_named_member(ctx, name, mode))
    }

    /// Enumerate the named members of the imported
    /// declaration. Drives
    /// `ImportedMacroSurface::enumerate_member_names`
    /// through the same hermetic context as
    /// [`Self::resolve_root`].
    ///
    /// Returns the raw `QueryResult` so the test can assert
    /// on the `Value(names)` set as well as the
    /// `Error(_)` / `Recursive(_)` shapes.
    pub fn enumerate_member_names(&self, host: &VerterHost) -> QueryResult<Vec<Arc<str>>> {
        with_bare_host_ctx_for_test(host, |ctx| self.0.enumerate_member_names(ctx))
    }

    /// Reconstruct the `defineProps` member set via the LAZY
    /// arm — wraps the bridge in
    /// `ResolvedMacroSurface::LazyImported` and drives
    /// `prop_members` through the hermetic context.
    #[must_use]
    pub fn lazy_prop_members(
        &self,
        host: &VerterHost,
    ) -> Vec<verter_semantic::analysis::AnalyzedPropField> {
        let surface = ResolvedMacroSurface::LazyImported(self.0.clone());
        with_bare_host_ctx_for_test(host, |ctx| surface.prop_members(ctx))
    }

    /// Reconstruct the `defineEmits` member set via the LAZY
    /// arm. The call-signature event-name extraction (codex's
    /// flagged case) runs here.
    #[must_use]
    pub fn lazy_emit_members(
        &self,
        host: &VerterHost,
    ) -> Vec<verter_semantic::analysis::AnalyzedEmitField> {
        let surface = ResolvedMacroSurface::LazyImported(self.0.clone());
        with_bare_host_ctx_for_test(host, |ctx| surface.emit_members(ctx))
    }

    /// Reconstruct the `defineSlots` member set via the LAZY
    /// arm. The non-function-member filtering (codex's flagged
    /// case) runs here.
    #[must_use]
    pub fn lazy_slot_members(
        &self,
        host: &VerterHost,
    ) -> Vec<verter_semantic::analysis::AnalyzedSlotField> {
        let surface = ResolvedMacroSurface::LazyImported(self.0.clone());
        with_bare_host_ctx_for_test(host, |ctx| surface.slot_members(ctx))
    }
}

/// Build an eager `ResolvedMacroMeta` for an imported macro
/// target by running the REAL OXC eager producer
/// (`host.resolve_macro_elements` → `project_macro_surfaces`),
/// then read its member set back through the SAME
/// `ResolvedMacroSurface::Eager` accessors the lazy probe uses.
///
/// This is the eager counterpart of [`ImportedMacroSurfaceProbe`]:
/// the eager/lazy macro-authority equivalence discriminators
/// compare the lazy reconstruction against this eager output
/// member-for-member. Both sides flow through the shared
/// `prop_members` / `emit_members` / `slot_members`
/// interpretation so the test asserts arm-to-arm equivalence,
/// not arm-to-hand-built.
pub struct EagerMacroSurfaceProbe(crate::resolver_core::ResolvedMacroMeta);

impl EagerMacroSurfaceProbe {
    /// Resolve `(import_source, exported_name)` from
    /// `owner_canonical` through the eager OXC rail and capture
    /// the projected `ResolvedMacroMeta`. Returns `None` when the
    /// eager resolver cannot reach the declaration (the test then
    /// fails loudly rather than silently comparing empty sets).
    #[must_use]
    pub fn resolve(
        host: &VerterHost,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
    ) -> Option<Self> {
        use crate::resolver_core::component_meta::ComponentMetaResolverHost;
        use crate::resolver_core::DeclarationMetadataResolver;
        // The `ComponentMetaResolverHost` impl lives on the
        // `HostComponentMetaResolver` wrapper (not on `VerterHost`
        // directly), so the eager producer is driven through the
        // same wrapper the production cold resolver uses.
        let resolver_host =
            crate::host_manage::jsdoc_resolve::HostComponentMetaResolver { host, ctx: host };
        let mut tracked_deps = std::collections::BTreeSet::new();
        let mut resolution_deps = std::collections::BTreeSet::new();
        let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
        let mut visiting = std::collections::HashSet::default();
        let elements = resolver_host.resolve_macro_elements(
            owner_canonical,
            import_source,
            exported_name,
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
        )?;
        let projected = crate::resolver_core::project_macro_surfaces(None, macro_kind, &elements);
        let declaration = resolver_host.resolve_type_declaration(
            resolver_host
                .resolve_type_dependency_canonical(owner_canonical, import_source)?
                .as_str(),
            exported_name,
        );
        Some(Self(crate::resolver_core::ResolvedMacroMeta {
            macro_index: 0,
            macro_kind,
            type_name: exported_name.to_string(),
            import_source: import_source.to_string(),
            surface_is_authoritative: true,
            declaration,
            native_props: projected.native_props,
            props: projected.props,
            emits: projected.emits,
            slots: projected.slots,
            jsdoc: None,
        }))
    }

    /// Read the `defineProps` member set via the EAGER arm
    /// (`ResolvedMacroSurface::Eager` → `prop_members`, which
    /// returns the stored `.props` verbatim).
    #[must_use]
    pub fn eager_prop_members(
        &self,
        host: &VerterHost,
    ) -> Vec<verter_semantic::analysis::AnalyzedPropField> {
        let surface = ResolvedMacroSurface::from_eager_meta(&self.0);
        with_bare_host_ctx_for_test(host, |ctx| surface.prop_members(ctx))
    }

    /// Read the `defineEmits` member set via the EAGER arm.
    #[must_use]
    pub fn eager_emit_members(
        &self,
        host: &VerterHost,
    ) -> Vec<verter_semantic::analysis::AnalyzedEmitField> {
        let surface = ResolvedMacroSurface::from_eager_meta(&self.0);
        with_bare_host_ctx_for_test(host, |ctx| surface.emit_members(ctx))
    }

    /// Read the `defineSlots` member set via the EAGER arm.
    #[must_use]
    pub fn eager_slot_members(
        &self,
        host: &VerterHost,
    ) -> Vec<verter_semantic::analysis::AnalyzedSlotField> {
        let surface = ResolvedMacroSurface::from_eager_meta(&self.0);
        with_bare_host_ctx_for_test(host, |ctx| surface.slot_members(ctx))
    }
}
