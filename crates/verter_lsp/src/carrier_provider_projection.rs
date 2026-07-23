//! Provider-only carrier import projection.
//!
//! TypeScript projects that explicitly enable `allowImportingTsExtensions`
//! consume authored `.vue`/`.svelte` specifiers through the framework plugin.
//! Every other project receives the `.verter.ts` compatibility specifier in
//! the generated provider buffer. Compiler output remains unchanged.

use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_semantic::analysis::{build_script_analysis_with_scope, AnalysisScope};
use verter_workspace::workspace_snapshot::{ConfiguredOwnerResolution, ProjectPayload};
use verter_workspace::{FilesystemWorkspace, WorkspaceRead, CARRIER_API_VIRTUAL_SUFFIX};

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::GeneratedRewriteMapper;
use crate::project_resolver::{ResolvePhase, ResolveRequest, ResolveRequestKind};

#[derive(Clone)]
pub(crate) struct PreparedCarrierProviderContent {
    pub(crate) content: Arc<str>,
    pub(crate) rewrites: GeneratedRewriteMapper,
}

pub(crate) struct PreparedVerterTypesVirtualContent {
    pub(crate) content: Arc<str>,
    pub(crate) virtual_path: String,
}

fn owner_resolves_verter_types(
    workspace: Option<&FilesystemWorkspace>,
    canonical_id: &str,
) -> bool {
    let resolved = workspace
        .and_then(FilesystemWorkspace::load_published)
        .and_then(|published| {
            let workspace = workspace?;
            published.snapshot.resolver.resolve_with_reader(
                workspace,
                &ResolveRequest {
                    importer_id: canonical_id.to_string(),
                    specifier: "@verter/types".to_string(),
                    kind: ResolveRequestKind::TypeImport,
                    phase: ResolvePhase::ProviderGraph,
                },
            )
        });
    if resolved.is_some() {
        return true;
    }

    let mut current = std::path::Path::new(canonical_id).parent();
    while let Some(directory) = current {
        let manifest = directory.join("node_modules/@verter/types/package.json");
        let exists = workspace.map_or_else(
            || manifest.is_file(),
            |workspace| workspace.file_exists(&manifest.to_string_lossy().replace('\\', "/")),
        );
        if exists {
            return true;
        }
        current = directory.parent();
    }
    false
}

/// Project a missing `@verter/types` package onto an adjacent provider-only
/// declaration overlay. The relative specifier lets tsgo resolve the off-disk
/// document through its normal overlay probe without writing into node_modules.
pub(crate) fn prepare_tsgo_verter_types_virtual(
    workspace: Option<&FilesystemWorkspace>,
    canonical_id: &str,
    provider_path: &str,
    generated: &str,
) -> Option<PreparedVerterTypesVirtualContent> {
    if owner_resolves_verter_types(workspace, canonical_id) {
        return None;
    }

    let file_name = std::path::Path::new(provider_path)
        .file_name()?
        .to_string_lossy();
    let provider_specifier = format!("./{file_name}.__verter_types");
    let virtual_path = format!("{provider_path}.__verter_types.d.ts");
    let allocator = Allocator::new();
    let analysis = build_script_analysis_with_scope(
        generated,
        SourceType::tsx(),
        &allocator,
        AnalysisScope::IMPORTS,
    );
    let mut replacements = Vec::new();
    for reference in analysis.module_references {
        if reference.analyzability != verter_semantic::analysis::ModuleReferenceAnalyzability::Exact
            || reference.literal_specifier.as_deref() != Some("@verter/types")
        {
            continue;
        }
        let start = reference.expr_span.start as usize;
        let end = reference.expr_span.end as usize;
        replacements.push((
            start,
            end,
            crate::server::server_utils::quote_wrapped_specifier(
                &reference.raw_text,
                &provider_specifier,
            ),
        ));
    }
    if replacements.is_empty() {
        return None;
    }
    replacements.sort_by_key(|replacement| replacement.0);
    Some(PreparedVerterTypesVirtualContent {
        content: Arc::from(crate::server::server_utils::apply_specifier_replacements(
            generated,
            &replacements,
        )),
        virtual_path,
    })
}

/// Whether every exact configured owner explicitly permits authored carrier
/// extension imports. Missing, false, bootstrap, and unowned all use the
/// compatibility projection.
pub(crate) fn configured_owners_allow_authored_carrier_specifiers(
    snapshot: &verter_workspace::WorkspaceSnapshot,
    canonical_id: &str,
) -> bool {
    let owner_allows = |id: verter_workspace::workspace_snapshot::ProjectId| {
        matches!(
            &snapshot.project(id).payload,
            ProjectPayload::Configured { compiler_options, .. }
                if compiler_options.allow_importing_ts_extensions
        )
    };

    match snapshot.configured_owner_resolution_for_file(canonical_id) {
        ConfiguredOwnerResolution::Unique(owner) => owner_allows(owner),
        ConfiguredOwnerResolution::Ambiguous(owners) => {
            !owners.is_empty() && owners.into_iter().all(owner_allows)
        }
        ConfiguredOwnerResolution::None => false,
    }
}

pub(crate) fn prepare_carrier_provider_imports(
    workspace: Option<&FilesystemWorkspace>,
    canonical_id: &str,
    generated: &str,
    encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> PreparedCarrierProviderContent {
    prepare_carrier_provider_imports_with_verter_types(
        workspace,
        canonical_id,
        generated,
        encoding,
        None,
    )
}

fn prepare_carrier_provider_imports_with_verter_types(
    workspace: Option<&FilesystemWorkspace>,
    canonical_id: &str,
    generated: &str,
    encoding: tower_lsp_server::ls_types::PositionEncodingKind,
    verter_types_specifier: Option<&str>,
) -> PreparedCarrierProviderContent {
    let published = workspace.and_then(FilesystemWorkspace::load_published);
    let rewrite_carrier_imports = !published.as_ref().is_some_and(|published| {
        published.ownership_ready
            && configured_owners_allow_authored_carrier_specifiers(
                &published.snapshot,
                canonical_id,
            )
    });
    if !rewrite_carrier_imports && verter_types_specifier.is_none() {
        return PreparedCarrierProviderContent {
            content: Arc::from(generated),
            rewrites: GeneratedRewriteMapper::new(&[], &LineIndex::new(generated, encoding)),
        };
    }

    let allocator = Allocator::new();
    let analysis = build_script_analysis_with_scope(
        generated,
        SourceType::tsx(),
        &allocator,
        AnalysisScope::IMPORTS,
    );
    let mut replacements = Vec::new();
    for reference in analysis.module_references {
        if reference.analyzability != verter_semantic::analysis::ModuleReferenceAnalyzability::Exact
        {
            continue;
        }
        let Some(specifier) = reference.literal_specifier.as_deref() else {
            continue;
        };
        let provider_specifier = if specifier == "@verter/types" {
            let Some(provider_specifier) = verter_types_specifier else {
                continue;
            };
            provider_specifier.to_owned()
        } else {
            if !rewrite_carrier_imports {
                continue;
            }
            let resolved = published.as_ref().and_then(|published| {
                let workspace = workspace?;
                published.snapshot.resolver.resolve_with_reader(
                    workspace,
                    &ResolveRequest {
                        importer_id: canonical_id.to_string(),
                        specifier: specifier.to_string(),
                        kind: if reference.is_type_only {
                            ResolveRequestKind::TypeImport
                        } else {
                            ResolveRequestKind::EsmImport
                        },
                        phase: ResolvePhase::ProviderGraph,
                    },
                )
            });
            match resolved {
                Some(resolved) if verter_workspace::path_is_carrier(&resolved.source_id) => {
                    resolved.provider_specifier
                }
                _ if verter_workspace::path_is_carrier(specifier) => {
                    format!("{specifier}{CARRIER_API_VIRTUAL_SUFFIX}")
                }
                _ => continue,
            }
        };
        let start = reference.expr_span.start as usize;
        let end = reference.expr_span.end as usize;
        let Some(original) = generated.get(start..end) else {
            continue;
        };
        let replacement = crate::server::server_utils::quote_wrapped_specifier(
            &reference.raw_text,
            &provider_specifier,
        );
        if original != replacement {
            replacements.push((start, end, replacement));
        }
    }
    replacements.sort_by_key(|replacement| replacement.0);
    let line_index = LineIndex::new(generated, encoding);
    let rewrites = GeneratedRewriteMapper::new(&replacements, &line_index);
    let content =
        crate::server::server_utils::apply_specifier_replacements(generated, &replacements);
    PreparedCarrierProviderContent {
        content: Arc::from(content),
        rewrites,
    }
}

/// Recover the generated-to-provider rewrite map from an already projected
/// carrier buffer. This is used by background/store recorders that receive the
/// exact delivered bytes rather than the pre-projection compiler bytes.
pub(crate) fn infer_carrier_provider_rewrites(
    provider_content: &str,
    encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> GeneratedRewriteMapper {
    let allocator = Allocator::new();
    let analysis = build_script_analysis_with_scope(
        provider_content,
        SourceType::tsx(),
        &allocator,
        AnalysisScope::IMPORTS,
    );
    let mut reverse = Vec::new();
    let mut verter_types_specifier = None;
    for reference in analysis.module_references {
        let Some(specifier) = reference.literal_specifier.as_deref() else {
            continue;
        };
        let original_specifier =
            if specifier.starts_with("./") && specifier.ends_with(".__verter_types") {
                verter_types_specifier = Some(specifier.to_owned());
                "@verter/types"
            } else {
                let Some(original_specifier) = specifier.strip_suffix(CARRIER_API_VIRTUAL_SUFFIX)
                else {
                    continue;
                };
                if !verter_workspace::path_is_carrier(original_specifier) {
                    continue;
                }
                original_specifier
            };
        reverse.push((
            reference.expr_span.start as usize,
            reference.expr_span.end as usize,
            crate::server::server_utils::quote_wrapped_specifier(
                &reference.raw_text,
                original_specifier,
            ),
        ));
    }
    if reverse.is_empty() {
        return GeneratedRewriteMapper::new(&[], &LineIndex::new(provider_content, encoding));
    }
    let original =
        crate::server::server_utils::apply_specifier_replacements(provider_content, &reverse);
    prepare_carrier_provider_imports_with_verter_types(
        None,
        "",
        &original,
        encoding,
        verter_types_specifier.as_deref(),
    )
    .rewrites
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_span::TsPosition;

    #[test]
    fn non_true_policy_rewrites_only_framework_carriers_and_tracks_columns() {
        let source = "import C from './C.vue'; import x from './x.ts'; void C; void x;\n";
        let prepared = prepare_carrier_provider_imports(
            None,
            "/ws/App.vue",
            source,
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
        );
        assert!(prepared.content.contains("'./C.vue.verter.ts'"));
        assert!(prepared.content.contains("'./x.ts'"));

        let original_semicolon = source.find("; import x").unwrap() as u32;
        assert_eq!(
            prepared
                .rewrites
                .generated_to_provider(TsPosition::new(0, original_semicolon))
                .unwrap()
                .character,
            original_semicolon + CARRIER_API_VIRTUAL_SUFFIX.len() as u32
        );
    }

    #[test]
    fn rewrite_inference_recovers_mapping_from_delivered_bytes() {
        let provider = "import C from './C.svelte.verter.ts'; void C;\n";
        let mapper = infer_carrier_provider_rewrites(
            provider,
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
        );
        assert!(!mapper.is_empty());
    }

    #[test]
    fn rewrite_inference_recovers_virtual_verter_types_column_mapping() {
        let original = "import type { X } from '@verter/types'; const value: X = {} as X;\n";
        let provider =
            "import type { X } from './App.vue.tsx.__verter_types'; const value: X = {} as X;\n";
        let mapper = infer_carrier_provider_rewrites(
            provider,
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
        );
        let generated_column = original.find("const value").unwrap() as u32;
        let provider_column = provider.find("const value").unwrap() as u32;

        assert_eq!(
            mapper
                .generated_to_provider(TsPosition::new(0, generated_column))
                .expect("post-import position maps")
                .character,
            provider_column
        );
        assert_eq!(
            mapper
                .provider_to_generated(TsPosition::new(0, provider_column))
                .expect("provider position maps back")
                .character,
            generated_column
        );
    }
}
