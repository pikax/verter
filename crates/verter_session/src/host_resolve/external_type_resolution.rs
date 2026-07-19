//! `impl VerterHost` — component-meta macro element/surface entry points.
//!
//! Owns the component-meta macro hooks that build on routed TypeInfo facts:
//! - `resolve_component_meta_native_props_target_with_view`
//! - `build_imported_macro_declaration_from_target`
//! - `resolve_component_meta_macro_surface_with_view`
//! - `resolve_component_meta_native_props_with_view`
//!
//! Each `_with_view` helper has a base wrapper (`#[cfg(test)]`-gated) that
//! passes `view = None`; production paths use `HostComponentMetaResolver`.

use super::frontier_helpers::DirectComponentMetaDeclarationResolver;
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
    /// View-aware macro-elements-target resolver.
    ///
    /// Resolves the routed root through the request-bound ImportedRootDb and
    /// projects its native rows through the shared TypeInfo dispatch.
    ///
    /// The resolved payload is the component-meta-owned keep-all native row
    /// set, memoized only in the request-local native projection cache.
    #[allow(clippy::too_many_arguments)]
    fn resolve_component_meta_native_props_target_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::component_meta::NativePropProjectionCache,
    ) -> Option<(
        String,
        String,
        String,
        Vec<crate::resolver_core::ResolvedNativeProp>,
    )> {
        let dep_canonical = self.resolve_loaded_dependency_canonical(
            owner_canonical,
            import_source,
            verter_workspace::ResolveRequestKind::TypeImport,
        )?;

        tracked_deps.insert(dep_canonical.clone());
        resolution_deps.insert(dep_canonical.clone());

        let cache_key = (dep_canonical.clone(), type_name.to_string());
        if let Some(cached) = cache.get(&cache_key).cloned() {
            let resolution = cached?;
            // Re-query the project-global `ImportedRootDb` for the target
            // identity. It collapses concurrent cold requests internally, so
            // repeated calls are cheap warm hits — there is no need for a
            // second per-request memo layer above it. Route through `ctx`
            // so request-bound callers exercise the overlay-aware view.
            let (target_canonical, target_name) =
                ctx.resolve_imported_type_root(dep_canonical.as_str(), type_name);
            tracked_deps.insert(target_canonical.clone());
            resolution_deps.insert(target_canonical.clone());
            return Some((dep_canonical, target_canonical, target_name, resolution));
        }

        let (seed_canonical, seed_type_name) =
            ctx.resolve_imported_type_root(dep_canonical.as_str(), type_name);
        tracked_deps.insert(seed_canonical.clone());
        resolution_deps.insert(seed_canonical.clone());

        let seed_target_key = (seed_canonical.clone(), seed_type_name.clone());
        if let Some(cached) = cache.get(&seed_target_key).cloned() {
            cache.insert(cache_key, cached.clone());
            let resolution = cached?;
            return Some((dep_canonical, seed_canonical, seed_type_name, resolution));
        }

        // ImportedRootDb is the sole routed target authority. It already
        // resolves direct, named-reexport, and wildcard-barrel hops under the
        // request-bound store view; a second frontier walk would duplicate
        // routing and dependency facts.
        let (effective_dep_canonical, effective_type_name) = (seed_canonical, seed_type_name);

        tracked_deps.insert(effective_dep_canonical.clone());
        resolution_deps.insert(effective_dep_canonical.clone());

        let final_target_key = (effective_dep_canonical.clone(), effective_type_name.clone());
        if let Some(cached) = cache.get(&final_target_key).cloned() {
            cache.insert(cache_key, cached.clone());
            let resolution = cached?;
            return Some((
                dep_canonical,
                effective_dep_canonical,
                effective_type_name,
                resolution,
            ));
        }

        // The owner's DIRECT imports are part of its macro type environment:
        // an ambient side-effect import (`import './aug'`) can contribute
        // `declare module` augmentations to the routed target's declaration.
        // Ensure them BEFORE the dispatch terminal — the `MergedDecl`
        // augmentation stitch enumerates augmenters over INDEXED files, so
        // resolving the target before its augmenter is indexed would memoize
        // an un-stitched surface for the whole request.
        self.ensure_owner_direct_imports_indexed(ctx, owner_canonical);

        // Dispatch-owned terminal (same rail as the loaded-files path above):
        // resolve the routed target's declaration carrier through the ONE
        // shared dispatch and request its EMPTY-PATH one-level `Shallow`
        // surface; the thin combined normalize preserves member values as
        // semantic carriers and projects the keep-all `native_props` rows
        // from the SAME surface. The component-meta REGISTRY entry is
        // published SEPARATELY as the original symbolic Ref (the cold
        // resolver's shallow seed). This native-row projection is not the
        // registry/type authority, so resolving here cannot fold surfaces the
        // registry must keep shallow (the `keeps_*_symbolic` meta trackers pin
        // that). `None` means a genuine unresolved
        // route/declaration; a transient recursion back-edge returns
        // un-cached.
        let outcome = crate::resolver_core::component_meta::named_native_props_outcome(
            ctx,
            effective_dep_canonical.as_str(),
            effective_type_name.as_str(),
        );
        use crate::resolver_core::component_meta::ResolvedNativePropsOutcome;
        let resolved = match outcome {
            ResolvedNativePropsOutcome::Resolved(resolution) => Some(resolution),
            // Transient back-edge — never a cacheable negative.
            ResolvedNativePropsOutcome::Recursive => return None,
            ResolvedNativePropsOutcome::Miss => None,
        };

        cache.insert(cache_key, resolved.clone());
        cache.insert(final_target_key, resolved.clone());
        resolved.map(|resolution| {
            (
                dep_canonical,
                effective_dep_canonical,
                effective_type_name,
                resolution,
            )
        })
    }

    /// Ensure every DIRECT import target of `owner_canonical` is indexed —
    /// including bindingless side-effect imports (`import './aug'`), the
    /// carriers of ambient `declare module` augmentations. One bounded hop
    /// over the owner's own import list (never a transitive walk); each
    /// ensure is the canonical once-per-generation materialization.
    fn ensure_owner_direct_imports_indexed(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
    ) {
        let Some(serve) = ctx.ensure_indexed_ready_serve(owner_canonical) else {
            return;
        };
        let specifiers: Vec<String> = serve
            .indexed
            .snapshot
            .imports
            .iter()
            .map(|import| import.source.clone())
            .collect();
        for specifier in specifiers {
            if let Some(dep_canonical) =
                self.resolve_type_dependency_canonical(owner_canonical, specifier.as_str())
            {
                let _ = ctx.ensure_indexed_ready_serve(dep_canonical.as_str());
            }
        }
    }

    fn build_imported_macro_declaration_from_target(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        dep_canonical: &str,
        requested_name: &str,
        target_canonical: &str,
        target_name: &str,
    ) -> crate::resolver_core::ResolvedTypeDeclaration {
        self.provenance
            .imported_macro_declaration_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolver = DirectComponentMetaDeclarationResolver { host: self };
        let mut declaration = crate::resolver_core::resolve_direct_local_type_declaration(
            &resolver,
            target_canonical,
            target_name,
        )
        .unwrap_or_else(|| {
            crate::meta_resolve::resolve_type_declaration_with_context(
                self,
                ctx,
                dep_canonical,
                requested_name,
            )
        });
        declaration.requested_name = requested_name.to_string();
        if declaration.resolved_name.is_empty() {
            declaration.resolved_name = target_name.to_string();
        }
        if declaration.canonical_source.is_empty() {
            declaration.canonical_source = target_canonical.to_string();
        }
        declaration
    }

    /// Base wrapper that fixes `view = None`. Test-only.
    #[cfg(test)]
    pub(crate) fn resolve_component_meta_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::component_meta::NativePropProjectionCache,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.resolve_component_meta_macro_surface_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                None,
            )
        })
    }

    /// View-aware variant of [`Self::resolve_component_meta_macro_surface`].
    ///
    /// Threads `view` into the macro-target resolver so the resolved-type
    /// cache slot and dep-source reads honour the active session overlay.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_component_meta_macro_surface_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::component_meta::NativePropProjectionCache,
        _view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        component_meta_trace_custom!(
            "resolve_component_meta_native_props",
            format!(
                "owner={} import={} type={} store_view={} cache_entries={}",
                owner_canonical,
                import_source,
                type_name,
                false,
                cache.len(),
            ),
        );

        let (dep_canonical, effective_dep_canonical, effective_type_name, resolution) = self
            .resolve_component_meta_native_props_target_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
            )?;
        Some(crate::resolver_core::ResolvedImportedMacroSurface {
            declaration: self.build_imported_macro_declaration_from_target(
                ctx,
                dep_canonical.as_str(),
                type_name,
                effective_dep_canonical.as_str(),
                effective_type_name.as_str(),
            ),
            native_props: resolution,
        })
    }

    /// Base wrapper that fixes `view = None`. Test-only.
    #[cfg(test)]
    pub(crate) fn resolve_component_meta_native_props(
        &self,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::component_meta::NativePropProjectionCache,
    ) -> Option<Vec<crate::resolver_core::ResolvedNativeProp>> {
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.resolve_component_meta_native_props_with_view(
                ctx,
                owner_canonical,
                import_source,
                type_name,
                tracked_deps,
                resolution_deps,
                cache,
                None,
            )
        })
    }

    /// View-aware variant of [`Self::resolve_component_meta_native_props`].
    ///
    /// Threads `view` into the macro-target resolver so cross-file macro type
    /// resolution observes the session overlay.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_component_meta_native_props_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        import_source: &str,
        type_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::component_meta::NativePropProjectionCache,
        _view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<Vec<crate::resolver_core::ResolvedNativeProp>> {
        self.resolve_component_meta_native_props_target_with_view(
            ctx,
            owner_canonical,
            import_source,
            type_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
        .map(|(_, _, _, resolution)| resolution)
    }
}
