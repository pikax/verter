use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use verter_parser::utils::oxc::script::type_surface::ResolvedElements;
use verter_semantic::analysis::{AnalyzedImport, MacroTypeDep};
use verter_span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalMacroTypeDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<Span>,
    /// Structural tier of the failure (see
    /// [`verter_semantic::analysis::types::MacroTypeDepUsage`]): a SURFACE
    /// dependency miss is an error (the macro's runtime surface cannot be
    /// enumerated); a MEMBER dependency miss is a warning (the member's
    /// runtime type degrades to `null`).
    pub severity: crate::HostSeverity,
}

#[derive(Debug, Clone)]
pub struct ExternalMacroTypeCollection {
    pub resolved: Option<FxHashMap<String, ResolvedElements>>,
    pub diagnostics: Vec<ExternalMacroTypeDiagnostic>,
    pub tracked_dependencies: BTreeSet<String>,
}

pub trait ExternalMacroTypeCollectorHost {
    type Error;

    /// Resolve one macro type dep to its legacy elements. `diagnostics` is
    /// the side-band sink for diagnostics a SUCCESSFUL resolve carries — a
    /// dep type that resolves but whose surface references an unresolvable
    /// import-backed arm reports the miss here without failing the resolve.
    /// Span-less sink entries are anchored to the dep's owning import
    /// statement by [`collect_external_macro_types`]; `Err` keeps its
    /// existing [`Self::map_external_macro_type_error`] channel.
    #[allow(clippy::too_many_arguments)]
    fn resolve_external_macro_type(
        &self,
        owner_canonical: &str,
        dep: &MacroTypeDep,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        profile_hash: Option<u64>,
        diagnostics: &mut Vec<ExternalMacroTypeDiagnostic>,
    ) -> Result<Option<ResolvedElements>, Self::Error>;

    fn map_external_macro_type_error(
        &self,
        owner_canonical: &str,
        dep: &MacroTypeDep,
        import_span: Option<Span>,
        error: &Self::Error,
    ) -> ExternalMacroTypeDiagnostic;
}

/// Span of the import statement that OWNS a dep's binding: the import whose
/// bindings contain the dep's type name wins over a mere source match, so two
/// separate imports from the same module anchor to the right statement.
fn owning_import_span(script_imports: &[AnalyzedImport], dep: &MacroTypeDep) -> Option<Span> {
    let same_source = || {
        script_imports
            .iter()
            .filter(|i| i.source == dep.import_source)
    };
    same_source()
        .find(|import| {
            import
                .bindings
                .iter()
                .any(|binding| binding.name == dep.type_name)
        })
        .or_else(|| same_source().next())
        .map(|import| import.span)
}

pub fn collect_external_macro_types<H: ExternalMacroTypeCollectorHost>(
    host: &H,
    owner_canonical: &str,
    macro_type_deps: &[MacroTypeDep],
    script_imports: &[AnalyzedImport],
    profile_hash: Option<u64>,
) -> ExternalMacroTypeCollection {
    let mut resolved = FxHashMap::default();
    let mut diagnostics = Vec::new();
    let mut tracked_deps = BTreeSet::new();
    let mut cache = crate::resolver_core::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();

    for dep in macro_type_deps {
        let mut resolution_deps = BTreeSet::new();
        let mut dep_diagnostics = Vec::new();
        match host.resolve_external_macro_type(
            owner_canonical,
            dep,
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            profile_hash,
            &mut dep_diagnostics,
        ) {
            Ok(Some(elements)) => {
                resolved.insert(dep.type_name.clone(), elements);
            }
            Ok(None) => {}
            Err(error) => {
                let import_span = owning_import_span(script_imports, dep);
                diagnostics.push(host.map_external_macro_type_error(
                    owner_canonical,
                    dep,
                    import_span,
                    &error,
                ));
            }
        }
        // Side-band diagnostics from a resolve that did not `Err` (a resolved
        // dep whose surface references an unresolvable import-backed arm).
        // Span-less entries anchor to the dep's owning import statement —
        // the same anchor the `Err` channel uses.
        if !dep_diagnostics.is_empty() {
            let import_span = owning_import_span(script_imports, dep);
            for mut diag in dep_diagnostics {
                if diag.span.is_none() {
                    diag.span = import_span;
                }
                diagnostics.push(diag);
            }
        }
    }

    ExternalMacroTypeCollection {
        resolved: (!resolved.is_empty()).then_some(resolved),
        diagnostics,
        tracked_dependencies: tracked_deps,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_external_macro_types, ExternalMacroTypeCollectorHost, ExternalMacroTypeDiagnostic,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use verter_parser::utils::oxc::script::type_surface::{ResolvedElements, RuntimeType};
    use verter_semantic::analysis::{AnalyzedImport, MacroTypeDep};
    use verter_span::Span;

    #[derive(Default)]
    struct TestHost {
        results: BTreeMap<String, Result<Option<ResolvedElements>, String>>,
        /// Per-type side-band diagnostics pushed into the resolve sink even
        /// when the resolve itself succeeds.
        sink_diagnostics: BTreeMap<String, Vec<ExternalMacroTypeDiagnostic>>,
    }

    impl ExternalMacroTypeCollectorHost for TestHost {
        type Error = String;

        fn resolve_external_macro_type(
            &self,
            _owner_canonical: &str,
            dep: &MacroTypeDep,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
            _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
            _profile_hash: Option<u64>,
            diagnostics: &mut Vec<ExternalMacroTypeDiagnostic>,
        ) -> Result<Option<ResolvedElements>, Self::Error> {
            if let Some(sink) = self.sink_diagnostics.get(&dep.type_name) {
                diagnostics.extend(sink.iter().cloned());
            }
            self.results
                .get(&dep.type_name)
                .cloned()
                .unwrap_or(Ok(None))
        }

        fn map_external_macro_type_error(
            &self,
            _owner_canonical: &str,
            dep: &MacroTypeDep,
            import_span: Option<Span>,
            error: &Self::Error,
        ) -> ExternalMacroTypeDiagnostic {
            ExternalMacroTypeDiagnostic {
                code: format!("ERR_{}", dep.type_name),
                message: error.clone(),
                span: import_span,
                severity: crate::HostSeverity::Error,
            }
        }
    }

    fn empty_elements() -> ResolvedElements {
        ResolvedElements {
            props: Vec::new(),
            call_signatures: Vec::new(),
            has_call_signature: false,
            root_runtime_types: vec![RuntimeType::Object],
        }
    }

    #[test]
    fn collect_external_macro_types_accumulates_results_and_diagnostics() {
        let mut host = TestHost::default();
        host.results
            .insert("Props".to_string(), Ok(Some(empty_elements())));
        host.results
            .insert("Bad".to_string(), Err("boom".to_string()));

        let deps = vec![
            MacroTypeDep {
                macro_index: 0,
                import_source: "./types".to_string(),
                type_name: "Props".to_string(),
                macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps,
                macro_span: Span::new(1, 10),
                usage: verter_semantic::analysis::types::MacroTypeDepUsage::Surface,
            },
            MacroTypeDep {
                macro_index: 1,
                import_source: "./bad".to_string(),
                type_name: "Bad".to_string(),
                macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps,
                macro_span: Span::new(11, 20),
                usage: verter_semantic::analysis::types::MacroTypeDepUsage::Surface,
            },
        ];
        let imports = vec![
            AnalyzedImport {
                source: "./types".to_string(),
                is_type_only: true,
                bindings: Vec::new(),
                span: Span::new(1, 10),
                resolved_canonical_id: None,
            },
            AnalyzedImport {
                source: "./bad".to_string(),
                is_type_only: true,
                bindings: Vec::new(),
                span: Span::new(11, 20),
                resolved_canonical_id: None,
            },
        ];

        let collected = collect_external_macro_types(&host, "/src/Comp.vue", &deps, &imports, None);

        assert!(collected
            .resolved
            .as_ref()
            .is_some_and(|resolved| resolved.contains_key("Props")));
        assert_eq!(collected.diagnostics.len(), 1);
        assert_eq!(collected.diagnostics[0].code, "ERR_Bad");
        assert_eq!(collected.diagnostics[0].span, Some(Span::new(11, 20)));
    }

    #[test]
    fn collect_external_macro_types_routes_successful_resolve_sink_diagnostics() {
        let mut host = TestHost::default();
        host.results
            .insert("Props".to_string(), Ok(Some(empty_elements())));
        host.sink_diagnostics.insert(
            "Props".to_string(),
            vec![ExternalMacroTypeDiagnostic {
                code: "HOST_MISSING_MACRO_TYPE_DEP".to_string(),
                message: "surface arm miss".to_string(),
                span: None,
                severity: crate::HostSeverity::Error,
            }],
        );

        let deps = vec![MacroTypeDep {
            macro_index: 0,
            import_source: "./types".to_string(),
            type_name: "Props".to_string(),
            macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps,
            macro_span: Span::new(1, 10),
            usage: verter_semantic::analysis::types::MacroTypeDepUsage::Surface,
        }];
        let imports = vec![AnalyzedImport {
            source: "./types".to_string(),
            is_type_only: true,
            bindings: Vec::new(),
            span: Span::new(1, 10),
            resolved_canonical_id: None,
        }];

        let collected = collect_external_macro_types(&host, "/src/Comp.vue", &deps, &imports, None);

        // The resolve SUCCEEDED — the elements land in `resolved` — while the
        // sink diagnostic still reaches the collection, anchored to the dep's
        // owning import statement.
        assert!(collected
            .resolved
            .as_ref()
            .is_some_and(|resolved| resolved.contains_key("Props")));
        assert_eq!(collected.diagnostics.len(), 1);
        assert_eq!(collected.diagnostics[0].code, "HOST_MISSING_MACRO_TYPE_DEP");
        assert_eq!(
            collected.diagnostics[0].severity,
            crate::HostSeverity::Error
        );
        assert_eq!(
            collected.diagnostics[0].span,
            Some(Span::new(1, 10)),
            "span-less sink diagnostics anchor to the dep's owning import"
        );
    }
}
