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
    ) -> Result<Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>, Self::Error>
    {
        self.host.resolve_external_type_from_loaded_files_with_view(
            owner_canonical,
            &dep.import_source,
            &dep.type_name,
            tracked_deps,
            resolution_deps,
            cache,
            visiting,
            true,
            verter_workspace::ResolveRequestKind::TypeImport,
            true,
            profile_hash,
            0,
            self.view,
        )
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
