use rustc_hash::FxHashSet;
use verter_analysis::ExportSignature;
use verter_span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportGraphFileKind {
    VueSfc,
    NonSfc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSurface {
    pub file_kind: ExportGraphFileKind,
    pub export_signatures: Vec<ExportSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGraphExport {
    pub name: String,
    pub is_type: bool,
    pub source_canonical_id: Option<String>,
    pub source_name: String,
}

pub trait ExportGraphResolver {
    fn export_surface(&self, canonical_id: &str) -> Option<ExportSurface>;

    fn local_export_span(&self, canonical_id: &str, binding_name: &str) -> Option<Span>;

    fn resolve_reexport_target(
        &self,
        canonical_id: &str,
        source: &str,
        sig: &ExportSignature,
    ) -> Option<String>;
}

pub fn get_export_span_follow_reexports_from_graph<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
    binding_name: &str,
) -> Option<(String, u32, u32)> {
    let mut visited = FxHashSet::default();
    follow_reexport_chain_from_graph(resolver, canonical_id, binding_name, &mut visited)
}

pub fn resolve_exports_from_graph<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
) -> Vec<ResolvedGraphExport> {
    resolve_exports_from_graph_with_mode(resolver, canonical_id, true)
}

pub fn resolve_exports_from_graph_best_effort<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
) -> Vec<ResolvedGraphExport> {
    resolve_exports_from_graph_with_mode(resolver, canonical_id, false)
}

fn resolve_exports_from_graph_with_mode<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
    strict_missing_reexports: bool,
) -> Vec<ResolvedGraphExport> {
    let mut visiting = FxHashSet::default();
    collect_resolved_exports_from_graph(
        resolver,
        canonical_id,
        &mut visiting,
        strict_missing_reexports,
    )
}

fn follow_reexport_chain_from_graph<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
    binding_name: &str,
    visited: &mut FxHashSet<(String, String)>,
) -> Option<(String, u32, u32)> {
    if !visited.insert((canonical_id.to_string(), binding_name.to_string())) {
        return None;
    }

    let surface = resolver.export_surface(canonical_id)?;

    if let Some(local_span) = resolver.local_export_span(canonical_id, binding_name) {
        if local_span.start > 0
            || local_span.end > 0
            || surface.file_kind == ExportGraphFileKind::VueSfc
        {
            return Some((canonical_id.to_string(), local_span.start, local_span.end));
        }
    }

    if surface.file_kind == ExportGraphFileKind::VueSfc {
        return None;
    }

    let sig = surface
        .export_signatures
        .iter()
        .find(|sig| sig.name == binding_name)?;

    if let (Some(source), Some(local_name)) = (&sig.reexport_source, &sig.reexport_local) {
        let target_canonical = resolver.resolve_reexport_target(canonical_id, source, sig)?;
        return follow_reexport_chain_from_graph(resolver, &target_canonical, local_name, visited);
    }

    if sig.span.start > 0 || sig.span.end > 0 {
        Some((canonical_id.to_string(), sig.span.start, sig.span.end))
    } else {
        None
    }
}

fn collect_resolved_exports_from_graph<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
    visiting: &mut FxHashSet<String>,
    strict_missing_reexports: bool,
) -> Vec<ResolvedGraphExport> {
    if !visiting.insert(canonical_id.to_string()) {
        return Vec::new();
    }

    let Some(surface) = resolver.export_surface(canonical_id) else {
        visiting.remove(canonical_id);
        return Vec::new();
    };

    let mut results = Vec::new();

    let has_default_signature = surface
        .export_signatures
        .iter()
        .any(|sig| sig.name == "default");
    if surface.file_kind == ExportGraphFileKind::VueSfc && !has_default_signature {
        results.push(ResolvedGraphExport {
            name: "default".to_string(),
            is_type: false,
            source_canonical_id: None,
            source_name: "default".to_string(),
        });
    }

    for sig in &surface.export_signatures {
        if sig.name == "*" {
            if let Some(source) = &sig.reexport_source {
                if let Some(target) = resolver.resolve_reexport_target(canonical_id, source, sig) {
                    let nested = collect_resolved_exports_from_graph(
                        resolver,
                        &target,
                        visiting,
                        strict_missing_reexports,
                    );
                    for mut export in nested {
                        if export.source_canonical_id.is_none() {
                            export.source_canonical_id = Some(target.clone());
                        }
                        results.push(export);
                    }
                }
            }
            continue;
        }

        if let (Some(source), Some(local_name)) = (&sig.reexport_source, &sig.reexport_local) {
            let Some(target) = resolver.resolve_reexport_target(canonical_id, source, sig) else {
                if strict_missing_reexports {
                    continue;
                }
                results.push(ResolvedGraphExport {
                    name: sig.name.clone(),
                    is_type: sig.is_type,
                    source_canonical_id: None,
                    source_name: local_name.clone(),
                });
                continue;
            };
            let Some((source_canonical_id, source_name)) = resolve_single_export_from_graph(
                resolver,
                &target,
                local_name,
                visiting,
                strict_missing_reexports,
            ) else {
                if strict_missing_reexports {
                    continue;
                }
                results.push(ResolvedGraphExport {
                    name: sig.name.clone(),
                    is_type: sig.is_type,
                    source_canonical_id: Some(target.clone()),
                    source_name: local_name.clone(),
                });
                continue;
            };
            results.push(ResolvedGraphExport {
                name: sig.name.clone(),
                is_type: sig.is_type,
                source_canonical_id: Some(source_canonical_id),
                source_name,
            });
            continue;
        }

        results.push(ResolvedGraphExport {
            name: sig.name.clone(),
            is_type: sig.is_type,
            source_canonical_id: None,
            source_name: sig.name.clone(),
        });
    }

    visiting.remove(canonical_id);
    results
}

fn resolve_single_export_from_graph<R: ExportGraphResolver>(
    resolver: &R,
    canonical_id: &str,
    name: &str,
    visiting: &mut FxHashSet<String>,
    strict_missing_reexports: bool,
) -> Option<(String, String)> {
    let surface = resolver.export_surface(canonical_id)?;

    if surface.file_kind == ExportGraphFileKind::VueSfc {
        if name == "default" || surface.export_signatures.iter().any(|sig| sig.name == name) {
            return Some((canonical_id.to_string(), name.to_string()));
        }
        return None;
    }

    let sig = surface
        .export_signatures
        .iter()
        .find(|sig| sig.name == name)?;
    if let (Some(source), Some(local_name)) = (&sig.reexport_source, &sig.reexport_local) {
        if visiting.contains(canonical_id) {
            return Some((canonical_id.to_string(), name.to_string()));
        }

        visiting.insert(canonical_id.to_string());
        let target = resolver.resolve_reexport_target(canonical_id, source, sig);
        visiting.remove(canonical_id);

        let Some(target_id) = target else {
            return (!strict_missing_reexports)
                .then(|| (canonical_id.to_string(), name.to_string()));
        };
        let resolved = resolve_single_export_from_graph(
            resolver,
            &target_id,
            local_name,
            visiting,
            strict_missing_reexports,
        );
        if strict_missing_reexports {
            return resolved;
        }
        return resolved.or(Some((target_id, local_name.clone())));
    }

    Some((canonical_id.to_string(), name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        get_export_span_follow_reexports_from_graph, resolve_exports_from_graph,
        resolve_exports_from_graph_best_effort, ExportGraphFileKind, ExportGraphResolver,
        ExportSurface, ResolvedGraphExport,
    };
    use rustc_hash::FxHashMap;
    use verter_analysis::{ExportSignature, Hash16};
    use verter_span::Span;

    #[derive(Default)]
    struct TestResolver {
        surfaces: FxHashMap<String, ExportSurface>,
        local_spans: FxHashMap<(String, String), Span>,
        routes: FxHashMap<(String, String, bool), String>,
    }

    impl ExportGraphResolver for TestResolver {
        fn export_surface(&self, canonical_id: &str) -> Option<ExportSurface> {
            self.surfaces.get(canonical_id).cloned()
        }

        fn local_export_span(&self, canonical_id: &str, binding_name: &str) -> Option<Span> {
            self.local_spans
                .get(&(canonical_id.to_string(), binding_name.to_string()))
                .copied()
        }

        fn resolve_reexport_target(
            &self,
            canonical_id: &str,
            source: &str,
            sig: &ExportSignature,
        ) -> Option<String> {
            self.routes
                .get(&(canonical_id.to_string(), source.to_string(), sig.is_type))
                .cloned()
        }
    }

    fn export_sig(
        name: &str,
        is_type: bool,
        span: Span,
        reexport_source: Option<&str>,
        reexport_local: Option<&str>,
    ) -> ExportSignature {
        ExportSignature {
            name: name.to_string(),
            declaration_hash: Hash16::default(),
            is_type,
            span,
            reexport_source: reexport_source.map(str::to_string),
            reexport_local: reexport_local.map(str::to_string),
            local_span: None,
        }
    }

    #[test]
    fn resolve_exports_from_graph_follows_direct_reexports() {
        let mut resolver = TestResolver::default();
        resolver.surfaces.insert(
            "/src/index.ts".to_string(),
            ExportSurface {
                file_kind: ExportGraphFileKind::NonSfc,
                export_signatures: vec![export_sig(
                    "Props",
                    true,
                    Span::default(),
                    Some("./types"),
                    Some("Props"),
                )],
            },
        );
        resolver.surfaces.insert(
            "/src/types.ts".to_string(),
            ExportSurface {
                file_kind: ExportGraphFileKind::NonSfc,
                export_signatures: vec![export_sig("Props", true, Span::new(10, 20), None, None)],
            },
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./types".to_string(), true),
            "/src/types.ts".to_string(),
        );

        let exports = resolve_exports_from_graph(&resolver, "/src/index.ts");
        assert_eq!(
            exports,
            vec![ResolvedGraphExport {
                name: "Props".to_string(),
                is_type: true,
                source_canonical_id: Some("/src/types.ts".to_string()),
                source_name: "Props".to_string(),
            }]
        );
    }

    #[test]
    fn get_export_span_follow_reexports_from_graph_uses_local_target_span() {
        let mut resolver = TestResolver::default();
        resolver.surfaces.insert(
            "/src/index.ts".to_string(),
            ExportSurface {
                file_kind: ExportGraphFileKind::NonSfc,
                export_signatures: vec![export_sig(
                    "Props",
                    true,
                    Span::default(),
                    Some("./types"),
                    Some("Props"),
                )],
            },
        );
        resolver.surfaces.insert(
            "/src/types.ts".to_string(),
            ExportSurface {
                file_kind: ExportGraphFileKind::VueSfc,
                export_signatures: vec![],
            },
        );
        resolver.local_spans.insert(
            ("/src/types.ts".to_string(), "Props".to_string()),
            Span::new(21, 31),
        );
        resolver.routes.insert(
            ("/src/index.ts".to_string(), "./types".to_string(), true),
            "/src/types.ts".to_string(),
        );

        assert_eq!(
            get_export_span_follow_reexports_from_graph(&resolver, "/src/index.ts", "Props"),
            Some(("/src/types.ts".to_string(), 21, 31))
        );
    }

    #[test]
    fn get_export_span_follow_reexports_from_graph_keeps_vue_default_zero_span() {
        let mut resolver = TestResolver::default();
        resolver.surfaces.insert(
            "/src/App.vue".to_string(),
            ExportSurface {
                file_kind: ExportGraphFileKind::VueSfc,
                export_signatures: vec![],
            },
        );
        resolver.local_spans.insert(
            ("/src/App.vue".to_string(), "default".to_string()),
            Span::default(),
        );

        assert_eq!(
            get_export_span_follow_reexports_from_graph(&resolver, "/src/App.vue", "default"),
            Some(("/src/App.vue".to_string(), 0, 0))
        );
    }

    #[test]
    fn resolve_exports_from_graph_best_effort_keeps_alias_when_target_unresolved() {
        let mut resolver = TestResolver::default();
        resolver.surfaces.insert(
            "/src/index.ts".to_string(),
            ExportSurface {
                file_kind: ExportGraphFileKind::NonSfc,
                export_signatures: vec![export_sig(
                    "Props",
                    true,
                    Span::default(),
                    Some("./types"),
                    Some("Props"),
                )],
            },
        );

        let exports = resolve_exports_from_graph_best_effort(&resolver, "/src/index.ts");
        assert_eq!(
            exports,
            vec![ResolvedGraphExport {
                name: "Props".to_string(),
                is_type: true,
                source_canonical_id: None,
                source_name: "Props".to_string(),
            }]
        );
    }
}
