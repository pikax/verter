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
use verter_workspace::{FilesystemWorkspace, CARRIER_API_VIRTUAL_SUFFIX};

use crate::documents::line_index::LineIndex;
use crate::documents::provider_projection::GeneratedRewriteMapper;
use crate::project_resolver::{ResolvePhase, ResolveRequest, ResolveRequestKind};

#[derive(Clone)]
pub(crate) struct PreparedCarrierProviderContent {
    pub(crate) content: Arc<str>,
    pub(crate) rewrites: GeneratedRewriteMapper,
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
    let published = workspace.and_then(FilesystemWorkspace::load_published);
    if published.as_ref().is_some_and(|published| {
        published.ownership_ready
            && configured_owners_allow_authored_carrier_specifiers(
                &published.snapshot,
                canonical_id,
            )
    }) {
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
        let provider_specifier = match resolved {
            Some(resolved) if verter_workspace::path_is_carrier(&resolved.source_id) => {
                resolved.provider_specifier
            }
            _ if verter_workspace::path_is_carrier(specifier) => {
                format!("{specifier}{CARRIER_API_VIRTUAL_SUFFIX}")
            }
            _ => continue,
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
    for reference in analysis.module_references {
        let Some(specifier) = reference.literal_specifier.as_deref() else {
            continue;
        };
        let Some(original_specifier) = specifier.strip_suffix(CARRIER_API_VIRTUAL_SUFFIX) else {
            continue;
        };
        if !verter_workspace::path_is_carrier(original_specifier) {
            continue;
        }
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
    prepare_carrier_provider_imports(None, "", &original, encoding).rewrites
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
}
