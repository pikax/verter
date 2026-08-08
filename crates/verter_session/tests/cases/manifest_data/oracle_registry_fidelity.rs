//! The REGISTRY-side half of the migration-fidelity projection
//! (`docs/arch/u0-oracle-harness-design.md` §Q4).
//!
//! One copy, two consumers. `registry_payload_matches_migration_fingerprint`
//! validates the live registry payload against each lifted row's retained
//! `migration_fingerprint`; the audited lift command computes the same
//! projection at lift time and refuses to write provenance whose registry side
//! disagrees with the original body. Both read THIS module, so the two sides
//! cannot drift into two projections of the same table.
//!
//! Pure over the registry's own types plus the extractor's fidelity value types:
//! no manifest access, no host, no resolver. The row's `ProofShape` is an INPUT
//! (the manifest owns the proof requirement; the lift command requires the
//! body-extracted `Ts7Oracle` shape), never re-derived here.

#![allow(dead_code)]

use super::oracle_migration_extract::{
    content_hash, fidelity_from_registry, FidelityTuple, ProofShape, QueryFidelity,
    SourceLocatorFidelity, WorkspaceFileFidelity,
};
use super::oracle_registry;

/// Project ONE registry `QuerySpec` onto the registry-comparable `QueryFidelity`
/// (the same axes the body extractor recovers, INCLUDING the typed `source_locator`).
pub fn registry_query_fidelity(spec: &oracle_registry::QuerySpec) -> QueryFidelity {
    use oracle_registry::{
        HostSetupKindSpec as Hk, ProjectionModeSpec as M, QueryHelperSpec as H, SymbolSpace as Sp,
    };
    let mode_tag = |m: M| {
        match m {
            M::Shallow => "Shallow",
            M::Navigate => "Navigate",
            M::Expanded => "Expanded",
            M::Skeleton => "Skeleton",
        }
        .to_string()
    };
    let (helper_kind, symbol, type_arg_strs, mode): (&str, String, &[&str], String) =
        match &spec.query_helper {
            H::ResolveExpr {
                symbol,
                type_args,
                projection_mode,
                ..
            } => (
                "ResolveExpr",
                (*symbol).to_string(),
                type_args,
                mode_tag(*projection_mode),
            ),
            H::ShallowSurfaceExpr { symbol } => (
                "ShallowSurfaceExpr",
                (*symbol).to_string(),
                &[],
                "Shallow".to_string(),
            ),
            H::EvaluateExpr {
                expression,
                projection_mode,
            } => (
                "EvaluateExpr",
                (*expression).to_string(),
                &[],
                mode_tag(*projection_mode),
            ),
        };
    let type_arguments: Vec<serde_json::Value> = type_arg_strs
        .iter()
        .map(|s| serde_json::from_str(s).expect("registry type-arg is canonical JSON"))
        .collect();
    let host_setup_kind = match spec.host_project.host_setup_kind {
        Hk::Standalone => "standalone",
        Hk::WorkspaceFootprint => "workspace_footprint",
        Hk::PackageBacked => "package_backed",
    }
    .to_string();
    let source_locator = SourceLocatorFidelity {
        reference_canonical: spec.source_locator.reference_canonical.to_string(),
        reference_name: spec.source_locator.reference_name.to_string(),
        symbol_space: match spec.source_locator.symbol_space {
            Sp::Type => "Type",
            Sp::Value => "Value",
        }
        .to_string(),
    };
    QueryFidelity {
        helper_kind: helper_kind.to_string(),
        primary_canonical: spec.primary_canonical.to_string(),
        symbol_or_expression: symbol,
        type_arguments,
        projection_mode: mode,
        host_setup_kind,
        source_locator,
    }
}

/// The row's registry entries in `query_ordinal` order.
pub fn registry_specs_for_row(
    row_file: &str,
    row_function: &str,
) -> Vec<&'static oracle_registry::QuerySpec> {
    let mut specs: Vec<&'static oracle_registry::QuerySpec> = oracle_registry::ORACLE_QUERY_SPECS
        .iter()
        .filter(|s| s.row_file == row_file && s.row_function == row_function)
        .collect();
    specs.sort_by_key(|s| s.query_ordinal);
    specs
}

/// The row's workspace file set projected onto migration-fidelity coordinates: each
/// `{path, content_hash}` over the registry's UPSERTED SOURCE BYTES (the registry
/// is the source-byte authority), SORTED by path. All of a row's query specs share
/// one workspace (the host is built once per row), so this is the per-row set; a
/// row whose specs disagree on the file set is a registry defect, reported as an
/// `Err` rather than a panic so the lift command can fail loudly without unwinding.
pub fn registry_workspace_files(
    row_file: &str,
    row_function: &str,
) -> Result<Vec<WorkspaceFileFidelity>, String> {
    let specs = registry_specs_for_row(row_file, row_function);
    let project = |spec: &oracle_registry::QuerySpec| -> Vec<WorkspaceFileFidelity> {
        let mut files: Vec<WorkspaceFileFidelity> = spec
            .workspace_files
            .iter()
            .map(|f| WorkspaceFileFidelity {
                path: f.path.to_string(),
                content_hash: content_hash(f.source),
            })
            .collect();
        files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        files
    };
    let first = specs.first().map(|s| project(s)).unwrap_or_default();
    for s in &specs {
        if project(s) != first {
            return Err(format!(
                "{row_file}::{row_function}: query specs disagree on the workspace file set",
            ));
        }
    }
    Ok(first)
}

/// Build the REGISTRY-side fidelity tuple for a row: its registry entries in
/// `query_ordinal` order, plus the caller-supplied proof shape.
pub fn registry_fidelity_for_row(
    row_file: &str,
    row_function: &str,
    proof: ProofShape,
) -> Result<FidelityTuple, String> {
    let queries: Vec<QueryFidelity> = registry_specs_for_row(row_file, row_function)
        .iter()
        .map(|s| registry_query_fidelity(s))
        .collect();
    let workspace_files = registry_workspace_files(row_file, row_function)?;
    Ok(fidelity_from_registry(
        row_file,
        row_function,
        queries,
        proof,
        workspace_files,
    ))
}
