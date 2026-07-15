//! Adapter that wires the `ExternalMacroTypeCollectorHost` trait into the
//! real `VerterHost`. Used by
//! [`crate::resolver_core::collect_external_macro_types`] to drive the
//! per-macro-type-dep loop without exposing the concrete host type to the
//! resolver core.

use super::frontier_helpers::ExternalTypeCache;
use crate::session_view::SessionView;
use crate::VerterHost;

pub(super) struct HostExternalMacroTypeCollector<'a> {
    pub host: &'a VerterHost,
    /// Active session overlay (when the collector is driven from a
    /// session-bearing cold-compute path). `None` for base callers — the
    /// underlying type resolution then routes through the base-only path.
    pub view: Option<&'a dyn SessionView>,
    /// Request-bound resolver context plumbed from the cold-compute
    /// entry-point. Routes carrier reads through the overlay-aware
    /// view rather than rebuild a workspace snapshot per call.
    pub ctx: &'a dyn crate::resolver_core::resolver_context::ResolverContext,
}

impl crate::resolver_core::ExternalMacroTypeCollectorHost for HostExternalMacroTypeCollector<'_> {
    type Error = crate::types::ExternalTypeResolveError;

    fn resolve_external_macro_type(
        &self,
        owner_canonical: &str,
        dep: &verter_semantic::analysis::MacroTypeDep,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut ExternalTypeCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        profile_hash: Option<u64>,
    ) -> Result<
        Option<verter_parser::utils::oxc::script::type_surface::ResolvedElements>,
        Self::Error,
    > {
        // The legacy path runs FIRST: it owns dependency tracking (the
        // frontier closure records `tracked_deps` for invalidation) and the
        // missing-dependency error semantics (`Err` propagates unchanged).
        let legacy = self
            .host
            .resolve_external_type_from_loaded_files_with_view(
                self.ctx,
                owner_canonical,
                &dep.import_source,
                &dep.type_name,
                tracked_deps,
                resolution_deps,
                cache,
                visiting,
                true,
                verter_workspace::ResolveRequestKind::TypeImport,
                // `use_host_cache = false`: this legacy `ResolvedElements`
                // compatibility path has no persistent warm admission;
                // request-local dedupe (the `cache` above) is allowed.
                false,
                profile_hash,
                0,
                self.view,
            )?;
        if legacy.is_some() {
            return Ok(legacy);
        }
        // The legacy frontier element payload is severed (an honest miss), so
        // an imported macro type argument resolves through the ONE shared
        // engine instead: the shared macro-surface authority + the shared
        // shallow-surface projection, thin-normalized into the parser-consumed
        // `ResolvedElements` shape (`shared_resolve(type) + normalise`).
        match dep.macro_kind {
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineEmits => Ok(
                crate::typeinfo::framework_surface::vue_exec::imported_emits_resolved_elements(
                    self.ctx,
                    owner_canonical,
                    dep.macro_index,
                    &dep.type_name,
                ),
            ),
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps => {
                // Bare-named macro argument (`defineProps<Props>()`) — the
                // macro-surface route (own-body provenance, indexed-access
                // support). A COMPOSITE argument (`defineProps<A & B>()`)
                // misses the by-name gate and resolves PER NAME instead —
                // the parser folds each referenced name independently.
                if let Some(elements) =
                    crate::typeinfo::framework_surface::vue_exec::imported_props_resolved_elements(
                        self.ctx,
                        owner_canonical,
                        dep.macro_index,
                        &dep.type_name,
                    )
                {
                    return Ok(Some(elements));
                }
                Ok(
                    crate::typeinfo::framework_surface::vue_exec::imported_named_props_resolved_elements(
                        self.ctx,
                        owner_canonical,
                        &dep.import_source,
                        &dep.type_name,
                    ),
                )
            }
            _ => Ok(None),
        }
    }

    fn map_external_macro_type_error(
        &self,
        owner_canonical: &str,
        dep: &verter_semantic::analysis::MacroTypeDep,
        import_span: Option<verter_span::Span>,
        error: &Self::Error,
    ) -> crate::resolver_core::ExternalMacroTypeDiagnostic {
        let (code, message) = match error {
            crate::types::ExternalTypeResolveError::MissingRootDependency => (
                "HOST_MISSING_MACRO_TYPE_DEP".to_string(),
                format!(
                    "missing macro type dependency '{}' for type '{}' in '{}'",
                    dep.import_source, dep.type_name, owner_canonical
                ),
            ),
            crate::types::ExternalTypeResolveError::DepthLimitExceeded {
                limit,
                type_name,
                last_dep,
            } => (
                "HOST_EXTERNAL_TYPE_DEPTH_LIMIT".to_string(),
                format!(
                    "external type resolution depth limit ({}) exceeded for type '{}' (last dep: '{}')",
                    limit, type_name, last_dep
                ),
            ),
            crate::types::ExternalTypeResolveError::StepLimitExceeded {
                limit,
                type_name,
                last_dep,
            } => (
                "HOST_EXTERNAL_TYPE_STEP_LIMIT".to_string(),
                format!(
                    "external type resolution step budget ({}) exceeded for type '{}' (last dep: '{}')",
                    limit, type_name, last_dep
                ),
            ),
        };

        crate::resolver_core::ExternalMacroTypeDiagnostic {
            code,
            message,
            span: import_span,
        }
    }
}
