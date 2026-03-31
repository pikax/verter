use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use verter_analysis::{AnalyzedImport, MacroTypeDep};
use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
use verter_span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalMacroTypeDiagnostic {
    pub code: String,
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct ExternalMacroTypeCollection {
    pub resolved: Option<FxHashMap<String, ResolvedElements>>,
    pub diagnostics: Vec<ExternalMacroTypeDiagnostic>,
    pub tracked_dependencies: BTreeSet<String>,
}

pub trait ExternalMacroTypeCollectorHost {
    type Error;

    #[allow(clippy::too_many_arguments)]
    fn resolve_external_macro_type(
        &self,
        owner_canonical: &str,
        dep: &MacroTypeDep,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
        cache: &mut crate::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
        profile_hash: Option<u64>,
    ) -> Result<Option<ResolvedElements>, Self::Error>;

    fn map_external_macro_type_error(
        &self,
        owner_canonical: &str,
        dep: &MacroTypeDep,
        import_span: Option<Span>,
        error: &Self::Error,
    ) -> ExternalMacroTypeDiagnostic;
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
    let mut cache = crate::ExternalTypeBodyCache::default();
    let mut visiting = rustc_hash::FxHashSet::default();

    for dep in macro_type_deps {
        let mut resolution_deps = BTreeSet::new();
        match host.resolve_external_macro_type(
            owner_canonical,
            dep,
            &mut tracked_deps,
            &mut resolution_deps,
            &mut cache,
            &mut visiting,
            profile_hash,
        ) {
            Ok(Some(elements)) => {
                resolved.insert(dep.type_name.clone(), elements);
            }
            Ok(None) => {}
            Err(error) => {
                let import_span = script_imports
                    .iter()
                    .find(|import| import.source == dep.import_source)
                    .map(|import| import.span);
                diagnostics.push(host.map_external_macro_type_error(
                    owner_canonical,
                    dep,
                    import_span,
                    &error,
                ));
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
    use verter_analysis::{AnalyzedImport, MacroTypeDep};
    use verter_compiler::utils::oxc::vue::resolve_type::{ResolvedElements, RuntimeType};
    use verter_span::Span;

    #[derive(Default)]
    struct TestHost {
        results: BTreeMap<String, Result<Option<ResolvedElements>, String>>,
    }

    impl ExternalMacroTypeCollectorHost for TestHost {
        type Error = String;

        fn resolve_external_macro_type(
            &self,
            _owner_canonical: &str,
            dep: &MacroTypeDep,
            _tracked_deps: &mut BTreeSet<String>,
            _resolution_deps: &mut BTreeSet<String>,
            _cache: &mut crate::ExternalTypeBodyCache,
            _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
            _profile_hash: Option<u64>,
        ) -> Result<Option<ResolvedElements>, Self::Error> {
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
            }
        }
    }

    fn empty_elements() -> ResolvedElements {
        ResolvedElements {
            props: Vec::new(),
            emits: Vec::new(),
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
                macro_kind: verter_analysis::types::AnalyzedMacroKind::DefineProps,
                macro_span: Span::new(1, 10),
            },
            MacroTypeDep {
                macro_index: 1,
                import_source: "./bad".to_string(),
                type_name: "Bad".to_string(),
                macro_kind: verter_analysis::types::AnalyzedMacroKind::DefineProps,
                macro_span: Span::new(11, 20),
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
}
