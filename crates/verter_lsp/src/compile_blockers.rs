use std::collections::HashSet;
use std::sync::Arc;

use verter_host::{FileKind, UpsertRequest, VerterHost};

/// Reader that resolves content from the host first, then falls back to disk.
pub struct HostFsProjectResolverReader<'a> {
    host: &'a VerterHost,
}

impl<'a> HostFsProjectResolverReader<'a> {
    pub fn new(host: &'a VerterHost) -> Self {
        Self { host }
    }
}

fn normalize_fs_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if let Some(stripped) = normalized.strip_prefix("//?/UNC/") {
        return format!("//{stripped}");
    }
    normalized
        .strip_prefix("//?/")
        .unwrap_or(normalized.as_str())
        .to_string()
}

impl crate::project_resolver::ProjectResolverReader for HostFsProjectResolverReader<'_> {
    fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
        self.host.get_source(canonical_id).or_else(|| {
            let normalized = normalize_fs_path(canonical_id);
            std::fs::read_to_string(&normalized)
                .ok()
                .map(Arc::<str>::from)
        })
    }

    fn file_exists(&self, canonical_id: &str) -> bool {
        self.host.get_source(canonical_id).is_some()
            || std::path::Path::new(&normalize_fs_path(canonical_id)).is_file()
    }

    fn realpath(&self, canonical_id: &str) -> Option<String> {
        if self.host.get_source(canonical_id).is_some() {
            return Some(normalize_fs_path(canonical_id));
        }

        std::fs::canonicalize(normalize_fs_path(canonical_id))
            .ok()
            .map(|path| normalize_fs_path(&path.to_string_lossy()))
    }
}

pub fn hydrate_vue_compile_blockers(
    host: &VerterHost,
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    canonical_id: &str,
) {
    let mut pending = vec![canonical_id.to_string()];
    let mut seen = HashSet::new();

    while let Some(source_id) = pending.pop() {
        if !seen.insert(source_id.clone()) {
            continue;
        }

        if !load_resolved_file_into_host(host, reader, &source_id) {
            continue;
        }

        if source_id.ends_with(".vue") {
            if let Some(blockers) = host.get_compile_blockers(&source_id) {
                for request in blockers.external_source_requests {
                    if let Some(loaded_id) = resolve_and_load_blocker(
                        host,
                        resolver,
                        reader,
                        &source_id,
                        &request.specifier,
                        crate::project_resolver::ResolveRequestKind::SfcSrcAttr,
                    ) {
                        pending.push(loaded_id);
                    }
                }

                for dep in blockers.macro_type_deps.iter() {
                    if let Some(loaded_id) = resolve_and_load_blocker(
                        host,
                        resolver,
                        reader,
                        &source_id,
                        &dep.import_source,
                        crate::project_resolver::ResolveRequestKind::TypeImport,
                    ) {
                        pending.push(loaded_id);
                    }
                }
            }
        }

        let Some(analysis) = host.get_analysis(&source_id) else {
            continue;
        };
        for (specifier, dep_id) in collect_resolved_codegen_dependencies(
            resolver,
            reader,
            &source_id,
            &analysis.module_references,
        ) {
            if dep_id == source_id {
                continue;
            }
            if track_loaded_dependency(host, reader, &source_id, &specifier, &dep_id) {
                pending.push(dep_id);
            }
        }
    }
}

fn file_kind_for_canonical_id(canonical_id: &str) -> FileKind {
    if canonical_id.ends_with(".vue") {
        FileKind::VueSfc
    } else {
        FileKind::NonSfc
    }
}

fn load_resolved_file_into_host(
    host: &VerterHost,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    canonical_id: &str,
) -> bool {
    if host.get_source(canonical_id).is_some() {
        return true;
    }

    let Some(source) = reader.read_text(canonical_id) else {
        return false;
    };

    host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source,
        file_kind: file_kind_for_canonical_id(canonical_id),
        aliases: Vec::new(),
    })
    .is_ok()
}

fn track_loaded_dependency(
    host: &VerterHost,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    owner_id: &str,
    specifier: &str,
    dep_id: &str,
) -> bool {
    if owner_id == dep_id {
        return false;
    }

    if !load_resolved_file_into_host(host, reader, dep_id) {
        return false;
    }

    host.set_import_dependencies(
        owner_id,
        vec![verter_host::DependencyResolution {
            specifier: specifier.to_string(),
            resolved_canonical_id: Some(dep_id.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    true
}

fn resolve_and_load_blocker(
    host: &VerterHost,
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    owner_id: &str,
    specifier: &str,
    kind: crate::project_resolver::ResolveRequestKind,
) -> Option<String> {
    let resolved = resolver.resolve_with_reader(
        reader,
        &crate::project_resolver::ResolveRequest {
            importer_id: owner_id.to_string(),
            specifier: specifier.to_string(),
            kind,
            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
        },
    )?;

    let dep_id = resolved.source_id;
    track_loaded_dependency(host, reader, owner_id, specifier, &dep_id).then_some(dep_id)
}

fn analyzed_module_reference_request_kind(
    reference: &verter_analysis::AnalyzedModuleReference,
) -> crate::project_resolver::ResolveRequestKind {
    if reference.is_type_only {
        crate::project_resolver::ResolveRequestKind::TypeImport
    } else if reference.semantics == verter_analysis::ModuleReferenceSemantics::Require {
        crate::project_resolver::ResolveRequestKind::RequireCall
    } else {
        crate::project_resolver::ResolveRequestKind::EsmImport
    }
}

/// Returns `(specifier, resolved_canonical_id)` pairs.
fn collect_resolved_codegen_dependencies(
    resolver: &crate::project_resolver::NativeProjectResolver,
    reader: &dyn crate::project_resolver::ProjectResolverReader,
    importer_id: &str,
    module_references: &[verter_analysis::AnalyzedModuleReference],
) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for reference in module_references {
        let kind = analyzed_module_reference_request_kind(reference);
        match reference.analyzability {
            verter_analysis::ModuleReferenceAnalyzability::Exact => {
                if let Some(specifier) = &reference.literal_specifier {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
                        },
                    ) {
                        if seen.insert(result.source_id.clone()) {
                            resolved.push((specifier.clone(), result.source_id));
                        }
                    }
                }
            }
            verter_analysis::ModuleReferenceAnalyzability::FiniteSet => {
                for specifier in &reference.finite_specifiers {
                    if let Some(result) = resolver.resolve_with_reader(
                        reader,
                        &crate::project_resolver::ResolveRequest {
                            importer_id: importer_id.to_string(),
                            specifier: specifier.clone(),
                            kind,
                            phase: crate::project_resolver::ResolvePhase::CodegenBlocker,
                        },
                    ) {
                        if seen.insert(result.source_id.clone()) {
                            resolved.push((specifier.clone(), result.source_id));
                        }
                    }
                }
            }
            verter_analysis::ModuleReferenceAnalyzability::UnknownDynamic => {}
        }
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use verter_host::{CompileErrorPolicy, CompileProfile, HostConfig, HostError};

    #[derive(Default)]
    struct TestResolverReader {
        files: HashSet<String>,
        texts: HashMap<String, Arc<str>>,
    }

    impl TestResolverReader {
        fn with_texts(entries: &[(&str, &str)]) -> Self {
            let mut reader = Self::default();
            for (path, text) in entries {
                let normalized = path.replace('\\', "/");
                reader.files.insert(normalized.clone());
                reader.texts.insert(normalized, Arc::<str>::from(*text));
            }
            reader
        }
    }

    impl crate::project_resolver::ProjectResolverReader for TestResolverReader {
        fn read_text(&self, canonical_id: &str) -> Option<Arc<str>> {
            self.texts.get(&canonical_id.replace('\\', "/")).cloned()
        }

        fn file_exists(&self, canonical_id: &str) -> bool {
            self.files.contains(&canonical_id.replace('\\', "/"))
        }

        fn realpath(&self, canonical_id: &str) -> Option<String> {
            let normalized = canonical_id.replace('\\', "/");
            self.file_exists(&normalized).then_some(normalized)
        }
    }

    fn strict_host() -> VerterHost {
        VerterHost::new(HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        })
    }

    fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(id.to_string()),
                input_id: id.to_string(),
                source: Arc::from(source),
                file_kind: FileKind::VueSfc,
                aliases: Vec::new(),
            })
            .unwrap();
    }

    #[test]
    fn hydrate_vue_compile_blockers_loads_external_and_transitive_type_deps() {
        let host = strict_host();
        let source = "<template src=\"@/partials/panel.html\"></template>\n<script setup lang=\"ts\">\nimport type { Props } from '@/types'\nconst props = defineProps<Props>()\n</script>";
        upsert_vue(&host, "/workspace/src/App.vue", source);
        let ide_profile = CompileProfile {
            target: verter_host::CompileTarget::BUNDLER | verter_host::CompileTarget::TSX,
            ..CompileProfile::default()
        };

        let initial = host.ensure_compiled("/workspace/src/App.vue", &CompileProfile::default());
        assert!(
            matches!(initial, Err(HostError::CompileError { .. })),
            "compile should fail before blocker hydration"
        );

        let mut project = crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        );
        project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
            base_url: Some("/workspace".to_string()),
            paths: vec![("@/*".to_string(), vec!["src/*".to_string()])],
        };
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![project]);
        let reader = TestResolverReader::with_texts(&[
            (
                "/workspace/src/partials/panel.html",
                "<div>{{ props.msg }}</div>",
            ),
            (
                "/workspace/src/types.ts",
                "import type { Nested } from '@/nested'\nexport interface Props { msg: Nested }",
            ),
            ("/workspace/src/nested.ts", "export type Nested = string"),
        ]);

        hydrate_vue_compile_blockers(&host, &resolver, &reader, "/workspace/src/App.vue");

        assert!(
            host.get_source("/workspace/src/partials/panel.html")
                .is_some(),
            "external template source should be loaded into the host"
        );
        assert!(
            host.get_source("/workspace/src/types.ts").is_some(),
            "macro type dependency should be loaded into the host"
        );
        assert!(
            host.get_source("/workspace/src/nested.ts").is_some(),
            "transitive type dependency should be loaded into the host"
        );

        host.ensure_compiled("/workspace/src/App.vue", &ide_profile)
            .expect("compile should succeed once codegen blockers are hydrated");
        assert!(
            host.get_ide("/workspace/src/App.vue", &ide_profile)
                .is_some(),
            "hydrated compile should restore IDE output"
        );
    }

    #[test]
    fn collect_resolved_codegen_dependencies_uses_codegen_blocker_phase() {
        let mut project = crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.app.json".to_string()),
        );
        project.compiler_options = crate::project_resolver::IdeProjectCompilerOptions {
            base_url: Some("/workspace/src".to_string()),
            paths: vec![("@/*".to_string(), vec!["*".to_string()])],
        };
        let resolver = crate::project_resolver::NativeProjectResolver::new(vec![project]);
        let reader = TestResolverReader::with_texts(&[
            ("/workspace/src/types.ts", "export interface Props {}"),
            ("/workspace/src/nested.ts", "export type Nested = string"),
        ]);
        let refs = vec![
            verter_analysis::AnalyzedModuleReference {
                syntax: verter_analysis::ModuleReferenceSyntax::StaticImport,
                semantics: verter_analysis::ModuleReferenceSemantics::Import,
                is_type_only: true,
                raw_text: "'@/types'".to_string(),
                literal_specifier: Some("@/types".to_string()),
                finite_specifiers: Vec::new(),
                static_prefix: None,
                analyzability: verter_analysis::ModuleReferenceAnalyzability::Exact,
                span: verter_span::Span::new(0, 8),
                expr_span: verter_span::Span::new(0, 8),
            },
            verter_analysis::AnalyzedModuleReference {
                syntax: verter_analysis::ModuleReferenceSyntax::StaticImport,
                semantics: verter_analysis::ModuleReferenceSemantics::Import,
                is_type_only: false,
                raw_text: "'./nested'".to_string(),
                literal_specifier: Some("./nested".to_string()),
                finite_specifiers: Vec::new(),
                static_prefix: None,
                analyzability: verter_analysis::ModuleReferenceAnalyzability::Exact,
                span: verter_span::Span::new(9, 19),
                expr_span: verter_span::Span::new(9, 19),
            },
        ];

        let resolved = collect_resolved_codegen_dependencies(
            &resolver,
            &reader,
            "/workspace/src/App.vue",
            &refs,
        );

        assert_eq!(
            resolved,
            vec![
                ("@/types".to_string(), "/workspace/src/types.ts".to_string()),
                (
                    "./nested".to_string(),
                    "/workspace/src/nested.ts".to_string()
                )
            ]
        );
    }
}
