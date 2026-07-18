//! Adapter that wires the `ExternalMacroTypeCollectorHost` trait into the
//! real `VerterHost`. Used by
//! [`crate::resolver_core::collect_external_macro_types`] to drive the
//! per-macro-type-dep loop without exposing the concrete host type to the
//! resolver core.
//!
//! ## Export-surface probe ownership (facts-only adjudication)
//!
//! The transitive export-surface probe in this module
//! (`export_surface_verdict` / `follow_export_route`) is a
//! DIAGNOSTICS-ONLY severity adjudicator, not a resolver: it decides
//! whether an unresolved macro-surface arm is PROVABLY absent (fatal) or
//! merely unknowable (silent). Its contract:
//!
//! - it consumes ONLY host-indexed facts (`IndexedReady` import/export
//!   signatures reached through `ensure_indexed_ready_serve` +
//!   `resolve_loaded_dependency_canonical`) — never a parser re-walk, never
//!   the type engine (guard
//!   `export_probe_consumes_indexed_facts_only`);
//! - it FAILS OPEN: package-backed targets, cycles, depth cap, and any gap
//!   in the fact surface adjudicate `Unknowable` (silent), never fatal;
//! - KNOWN GAP: names contributed only through ambient module augmentation
//!   (`declare module './x'`) are not modeled by raw export signatures, so
//!   an augmentation-only name can adjudicate `Absent` (false fatal). The
//!   durable fix is consolidating this probe onto the augmentation-aware
//!   `EffectiveExportSet` (the shared export-surface authority) instead of
//!   raw `export_signatures`; until then this module owns that gap and the
//!   probe must not grow further private export-surface semantics.

use super::frontier_helpers::ExternalTypeCache;
use crate::session_view::SessionView;
use crate::VerterHost;

/// Verdict of the transitive export-surface probe: only PROVABLE absence is
/// fatal; an unknowable surface stays silent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportSurfaceVerdict {
    Present,
    Absent,
    Unknowable,
}

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

impl HostExternalMacroTypeCollector<'_> {
    /// Classify one unresolved SURFACE-composition arm of a RESOLVED macro
    /// type dep. IMPORT-BACKED misses are fatal: the arm's head name is an
    /// import binding of its declaring file, and that import either fails to
    /// resolve to a loaded file or resolves to a file that does not export
    /// the requested name. A name with no owning import (ambient / lib /
    /// global heritage) stays silent — the shared engine does not model lib
    /// declarations, so erroring there would be a false positive.
    fn import_backed_unresolved_arm(
        &self,
        arm: &crate::typeinfo::framework_surface::vue_exec::UnresolvedSurfaceArm,
    ) -> bool {
        let Some(serve) = self
            .ctx
            .ensure_indexed_ready_serve(arm.owner_canonical.as_ref())
        else {
            return false;
        };
        let Some((import, binding)) = serve.indexed.snapshot.imports.iter().find_map(|import| {
            import
                .bindings
                .iter()
                .find(|binding| binding.name == arm.name.as_ref())
                .map(|binding| (import, binding))
        }) else {
            return false;
        };
        match self.host.resolve_loaded_dependency_canonical(
            arm.owner_canonical.as_ref(),
            &import.source,
            verter_workspace::ResolveRequestKind::TypeImport,
        ) {
            // The owning import does not resolve to a loaded file — the
            // surface arm can never materialise.
            None => true,
            // The import resolves; fatal only when the requested name is
            // PROVABLY absent from the target's export surface. The probe
            // walks the indexed export-route facts transitively (named
            // re-export chains, `export *` barrels), so absence is decided
            // by the same route inventory the resolver consumes — an
            // unknowable surface (unindexed hop, budget) stays silent
            // rather than misreport a transient.
            Some(dep_canonical) => {
                let requested = binding.imported_name.as_deref().unwrap_or(&binding.name);
                let mut visited = rustc_hash::FxHashSet::default();
                matches!(
                    self.export_surface_verdict(&dep_canonical, requested, &mut visited, 0),
                    ExportSurfaceVerdict::Absent
                )
            }
        }
    }

    /// Transitive export-surface probe over the retained indexed facts: does
    /// `name` reach a declaration through `canonical`'s export surface —
    /// local exports, named re-export chains (`export { X } from …`), and
    /// `export *` barrels? `Unknowable` (unindexed hop, unresolvable
    /// re-export source, cycle, depth cap) is never treated as absence.
    fn export_surface_verdict(
        &self,
        canonical: &str,
        name: &str,
        visited: &mut rustc_hash::FxHashSet<String>,
        depth: usize,
    ) -> ExportSurfaceVerdict {
        const MAX_EXPORT_ROUTE_DEPTH: usize = 8;
        if depth > MAX_EXPORT_ROUTE_DEPTH || !visited.insert(canonical.to_string()) {
            return ExportSurfaceVerdict::Unknowable;
        }
        // A package-backed file's export surface may use forms the shallow
        // index does not model (`export =`, CommonJS interop, ambient
        // merges) — absence is never provable there. Only workspace-owned
        // files can decide `Absent`.
        if self.ctx.workspace_is_package_backed(canonical) {
            return ExportSurfaceVerdict::Unknowable;
        }
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical) else {
            return ExportSurfaceVerdict::Unknowable;
        };
        let exports = &serve.indexed.snapshot.export_signatures;
        let mut unknowable = false;
        // Named match first: a local export decides Present immediately; a
        // named re-export follows its source for the ORIGINAL name.
        for export in exports.iter().filter(|e| e.name == name) {
            let Some(source) = export.reexport_source.as_deref() else {
                return ExportSurfaceVerdict::Present;
            };
            let original = export.reexport_local.as_deref().unwrap_or(name);
            match self.follow_export_route(canonical, source, original, visited, depth) {
                ExportSurfaceVerdict::Present => return ExportSurfaceVerdict::Present,
                ExportSurfaceVerdict::Unknowable => unknowable = true,
                ExportSurfaceVerdict::Absent => {}
            }
        }
        // Star re-exports: the name may live behind any of them.
        for export in exports.iter().filter(|e| e.name == "*") {
            let Some(source) = export.reexport_source.as_deref() else {
                unknowable = true;
                continue;
            };
            match self.follow_export_route(canonical, source, name, visited, depth) {
                ExportSurfaceVerdict::Present => return ExportSurfaceVerdict::Present,
                ExportSurfaceVerdict::Unknowable => unknowable = true,
                ExportSurfaceVerdict::Absent => {}
            }
        }
        if unknowable {
            ExportSurfaceVerdict::Unknowable
        } else {
            ExportSurfaceVerdict::Absent
        }
    }

    fn follow_export_route(
        &self,
        from_canonical: &str,
        source: &str,
        name: &str,
        visited: &mut rustc_hash::FxHashSet<String>,
        depth: usize,
    ) -> ExportSurfaceVerdict {
        match self.host.resolve_loaded_dependency_canonical(
            from_canonical,
            source,
            verter_workspace::ResolveRequestKind::TypeImport,
        ) {
            Some(target) => self.export_surface_verdict(&target, name, visited, depth + 1),
            None => ExportSurfaceVerdict::Unknowable,
        }
    }

    /// Mint one Error-severity `HOST_MISSING_MACRO_TYPE_DEP` diagnostic per
    /// import-backed unresolved surface arm of a RESOLVED dep type. Arms
    /// arrive name-sorted from the surface resolution, so emission order is
    /// deterministic. Spans stay `None` here; the collect loop anchors them
    /// to the dep's owning import statement.
    fn push_unresolved_surface_arm_diags(
        &self,
        owner_canonical: &str,
        dep: &verter_semantic::analysis::MacroTypeDep,
        arms: &[crate::typeinfo::framework_surface::vue_exec::UnresolvedSurfaceArm],
        diagnostics: &mut Vec<crate::resolver_core::ExternalMacroTypeDiagnostic>,
    ) {
        for arm in arms {
            if !self.import_backed_unresolved_arm(arm) {
                continue;
            }
            diagnostics.push(crate::resolver_core::ExternalMacroTypeDiagnostic {
                code: "HOST_MISSING_MACRO_TYPE_DEP".to_string(),
                message: format!(
                    "missing macro type dependency: type '{}' resolves but its surface references unresolvable type '{}' (declared in '{}') in '{}'",
                    dep.type_name, arm.name, arm.owner_canonical, owner_canonical
                ),
                span: None,
                severity: crate::HostSeverity::Error,
            });
        }
    }
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
        diagnostics: &mut Vec<crate::resolver_core::ExternalMacroTypeDiagnostic>,
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
        // The legacy frontier element payload is severed (an honest miss) —
        // the call above exists for dependency tracking and the missing-root
        // error semantics only. Elements always come from the ONE shared
        // engine below: the shared macro-surface authority + the shared
        // shallow-surface projection, thin-normalized into the parser-consumed
        // `ResolvedElements` shape (`shared_resolve(type) + normalise`) — an
        // early return on a legacy payload would bypass the surface-arm
        // diagnostics. A RESOLVED dep whose surface dropped unresolvable
        // import-backed arms reports each miss through the `diagnostics` sink.
        debug_assert!(
            legacy.is_none(),
            "the legacy frontier element payload is severed; elements resolve \
             through the shared engine"
        );
        match dep.macro_kind {
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineEmits => {
                let mut arms = Vec::new();
                let elements =
                    crate::typeinfo::framework_surface::vue_exec::imported_emits_resolved_elements(
                        self.ctx,
                        owner_canonical,
                        dep.macro_index,
                        &dep.type_name,
                        &mut arms,
                    );
                self.push_unresolved_surface_arm_diags(owner_canonical, dep, &arms, diagnostics);
                Ok(elements)
            }
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps => {
                // Bare-named macro argument (`defineProps<Props>()`) — the
                // macro-surface route (own-body provenance, indexed-access
                // support). A COMPOSITE argument (`defineProps<A & B>()`)
                // misses the by-name gate and resolves PER NAME instead —
                // the parser folds each referenced name independently.
                let mut arms = Vec::new();
                if let Some(elements) =
                    crate::typeinfo::framework_surface::vue_exec::imported_props_resolved_elements(
                        self.ctx,
                        owner_canonical,
                        dep.macro_index,
                        &dep.type_name,
                        &mut arms,
                    )
                {
                    self.push_unresolved_surface_arm_diags(
                        owner_canonical,
                        dep,
                        &arms,
                        diagnostics,
                    );
                    return Ok(Some(elements));
                }
                // The by-name gate missed — the per-name route re-projects
                // the same declaration surface, so its arms REPLACE (not
                // append to) the macro route's.
                arms.clear();
                let named =
                    crate::typeinfo::framework_surface::vue_exec::imported_named_props_resolved_elements(
                        self.ctx,
                        owner_canonical,
                        &dep.import_source,
                        &dep.type_name,
                        &mut arms,
                    );
                self.push_unresolved_surface_arm_diags(owner_canonical, dep, &arms, diagnostics);
                Ok(named)
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
        let (code, message, severity) = match error {
            crate::types::ExternalTypeResolveError::MissingRootDependency => {
                // Tier by the dep's structural position: a MEMBER-position
                // miss (`defineProps<{ foo: Missing }>()`) degrades that
                // member's runtime type to `null` — a warning, never fatal.
                // A SURFACE-position miss (direct type argument, heritage,
                // intersection/union arm) blocks enumerating the runtime
                // surface — an error.
                let member_miss =
                    dep.usage == verter_semantic::analysis::types::MacroTypeDepUsage::Member;
                let severity = if member_miss {
                    crate::HostSeverity::Warning
                } else {
                    crate::HostSeverity::Error
                };
                let detail = if member_miss {
                    " (member runtime type degrades to null)"
                } else {
                    ""
                };
                (
                    "HOST_MISSING_MACRO_TYPE_DEP".to_string(),
                    format!(
                        "missing macro type dependency '{}' for type '{}' in '{}'{}",
                        dep.import_source, dep.type_name, owner_canonical, detail
                    ),
                    severity,
                )
            }
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
                // Resolution RESOURCE exhaustion is never softened by
                // structural position — a pathological/too-deep type stays
                // fatal regardless of where it was referenced.
                crate::HostSeverity::Error,
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
                crate::HostSeverity::Error,
            ),
        };

        crate::resolver_core::ExternalMacroTypeDiagnostic {
            code,
            message,
            span: import_span,
            severity,
        }
    }
}
