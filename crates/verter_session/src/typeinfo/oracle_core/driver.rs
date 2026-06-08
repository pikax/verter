//! The shared registry driver — the consumption-side orchestrator the
//! `#[oracle_row]` attribute proc-macro synthesizes a call to
//! (`docs/arch/u0-oracle-harness-design.md` §2 "Consumption" + §Q4).
//!
//! A lifted `TypeExpr`-projection row body is replaced by
//! `oracle::run_row(file!(), "<fn>")`. The driver:
//!
//! 1. BASENAME-normalizes `file!()` (a full source path) to the bare filename
//!    the registry / manifest key on (`Path::file_name()`, §Q4) — the manifest
//!    discovery key is `path.file_name()`, so the row key is the bare filename;
//! 2. looks up the row's ordered registry entries in [`ORACLE_QUERY_SPECS`];
//! 3. for each entry runs the `support.rs` helper named by `query_helper` to
//!    produce Verter's in-process `TypeExpr`;
//! 4. re-derives the entry's `snapshot_id` from REGISTRY-ONLY, tsgo-free inputs
//!    (the registry spec + the pinned env, including the STABLE `env_corpus_id`);
//! 5. loads the snapshot via runtime `std::fs::read` from the full
//!    `CARGO_MANIFEST_DIR`-rooted path (NO `include_str!` / `include_dir!` — a
//!    second embedded artifact would be a shadow registry that drifts, §Q1);
//! 6. strictly decodes it, validates its stored env-pins + `snapshot_id` +
//!    `row_ref` against the registry-derived expectation, re-enumerates the
//!    vendored corpus, and recomputes its `oracle_env_hash`;
//! 7. normalizes Verter's `TypeExpr` and asserts structural equality against the
//!    snapshot's stored (already-normalized) `oracle_value` under the same
//!    normalization. NO tsgo at consumption time.
//!
//! The registry seats the 7 lifted rows (the two index-signature
//! publication queries + the two built-in modifier-utility queries + the three
//! U2 IndexedAccess-reduction carve-out queries), so `run_row`
//! IS invoked at runtime by those rows' `oracle::run_row` bodies. Its pure
//! sub-functions are additionally exercised directly by discriminating unit
//! tests; the orchestrator is the real path every lifted row rides.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use verter_type_expr::{type_expr_from_json, TypeExpr};

use super::identity::{
    self, HostProject, HostSetupKind, OracleValueKind, PinnedEnv, QueryHelperKind,
    SnapshotIdentity, WorkspaceFileRef,
};
use super::normalize::{self, ProjectionModeKind};
use super::query_specs::{
    HostProjectSpec, HostSetupKindSpec, OracleValueKindSpec, ProjectionModeSpec, QueryHelperSpec,
    QuerySpec, COMPILER_OPTIONS_HASH, CURRENT_ENV_CORPUS_ID, ORACLE_QUERY_SPECS,
};
use super::snapshot::{self, OracleSnapshot};

/// The relative infix from `CARGO_MANIFEST_DIR` (= `crates/verter_session/`) to
/// the snapshot tree. The FULL infix is REQUIRED — joining only
/// `oracle_snapshots/` to the manifest dir would read from
/// `crates/verter_session/oracle_snapshots/`, the wrong place (§Q1).
pub(crate) const SNAPSHOT_TREE_INFIX: &str = "src/typeinfo/typeinfo_tests/oracle_snapshots";

/// The corpus-root infix from `CARGO_MANIFEST_DIR` to the vendored env corpus
/// (`oracle_env/<env_corpus_id>/`). The driver re-enumerates this on read.
pub(crate) const ORACLE_ENV_INFIX: &str = "src/typeinfo/typeinfo_tests/oracle_env";

/// Why the driver could not validate a `(row, query)` against its snapshot. The
/// pure sub-functions return this for testability; [`run_row`] turns any error
/// into a panic (the test-failure surface).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DriverError {
    /// A `Lifted` row reached the driver but the registry holds NO entry for its
    /// `(row_file, row_function)` key — a coverage hole the registry cross-check
    /// guards also catch, surfaced here so a stray lift fails loudly.
    NoRegistryEntries {
        row_file: String,
        row_function: String,
    },
    /// A type-argument string in the registry entry did not parse as JSON, or did
    /// not decode through `type_expr_from_json` — an un-printable / malformed arg.
    BadTypeArgument { index: usize, detail: String },
    /// `identity` carried the same canonical path twice — a schema violation
    /// (there is exactly one final content per path, §Q1).
    DuplicateWorkspacePath { path: String },
    /// The snapshot failed strict decode.
    Decode(snapshot::SnapshotDecodeError),
    /// A stored env-pin / `snapshot_id` / `row_ref` field did not match the
    /// registry-derived expectation (a stale or misfiled snapshot).
    EnvPinMismatch {
        field: String,
        expected: String,
        found: String,
    },
    /// The vendored corpus directory's CURRENT listing did not set-equal the
    /// snapshot's stored `oracle_env_files.manifest` (a member was added or
    /// removed under the corpus root, §Q5 `oracle_env_corpus_is_closed`).
    CorpusMembershipDrift { detail: String },
    /// The recomputed `oracle_env_hash` (re-hashing `oracle_env_files.files`
    /// against current on-disk corpus content) did not equal the stored value.
    OracleEnvHashDrift { expected: String, found: String },
    /// The normalizer rejected one side (a non-confluent / non-admissible
    /// construct reached the compare).
    NormalizeReject(normalize::NormalizeReject),
    /// Verter's normalized `TypeExpr` did not structurally equal the snapshot's
    /// normalized `oracle_value` — a real parity divergence.
    ValueMismatch { verter: String, oracle: String },
}

// ---------------------------------------------------------------------------
// Pure sub-functions (each exercised by discriminating unit tests)
// ---------------------------------------------------------------------------

/// Basename-normalize a `file!()` source path to the bare filename the registry
/// and manifest key on (`path.file_name()`, §Q4). A path with no final component
/// (impossible from `file!()`) falls back to the input verbatim, so the lookup
/// fails loudly rather than panicking inside the driver.
#[allow(dead_code)]
pub(crate) fn row_basename(file: &str) -> &str {
    Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file)
}

/// The pinned env the registry + every guard read to derive a `snapshot_id`
/// WITHOUT opening a snapshot or running tsgo (§Q4). The version constants live
/// on `identity`; the corpus / option pins live in the registry's pinned-env
/// block (filled by the snapshot generator).
#[allow(dead_code)]
pub(crate) fn pinned_env() -> PinnedEnv {
    PinnedEnv {
        tsgo_version: identity::TSGO_VERSION.to_string(),
        oracle_schema_version: identity::ORACLE_SCHEMA_VERSION,
        normalizer_version: normalize::NORMALIZER_VERSION,
        probe_synthesis_version: identity::PROBE_SYNTHESIS_VERSION,
        compiler_options_hash: COMPILER_OPTIONS_HASH.to_string(),
        env_corpus_id: CURRENT_ENV_CORPUS_ID.to_string(),
    }
}

/// The row's registry entries, in `query_ordinal` order. Filters
/// `(row_file == basename, row_function == function)`.
#[allow(dead_code)]
pub(crate) fn lookup_row_entries<'a>(
    specs: &'a [QuerySpec],
    basename: &str,
    function: &str,
) -> Vec<&'a QuerySpec> {
    let mut entries: Vec<&QuerySpec> = specs
        .iter()
        .filter(|s| s.row_file == basename && s.row_function == function)
        .collect();
    entries.sort_by_key(|s| s.query_ordinal);
    entries
}

fn map_helper_kind(helper: &QueryHelperSpec) -> QueryHelperKind {
    match helper {
        QueryHelperSpec::ResolveExpr { .. } => QueryHelperKind::ResolveExpr,
        QueryHelperSpec::ShallowSurfaceExpr { .. } => QueryHelperKind::ShallowSurfaceExpr,
        QueryHelperSpec::EvaluateExpr { .. } => QueryHelperKind::EvaluateExpr,
    }
}

fn map_mode_kind(mode: ProjectionModeSpec) -> ProjectionModeKind {
    match mode {
        ProjectionModeSpec::Shallow => ProjectionModeKind::Shallow,
        ProjectionModeSpec::Navigate => ProjectionModeKind::Navigate,
        ProjectionModeSpec::Expanded => ProjectionModeKind::Expanded,
        ProjectionModeSpec::Skeleton => ProjectionModeKind::Skeleton,
    }
}

fn map_resolver_mode(mode: ProjectionModeSpec) -> crate::semantic_query::ProjectionMode {
    use crate::semantic_query::ProjectionMode;
    match mode {
        ProjectionModeSpec::Shallow => ProjectionMode::Shallow,
        ProjectionModeSpec::Navigate => ProjectionMode::Navigate,
        ProjectionModeSpec::Expanded => ProjectionMode::Expanded,
        ProjectionModeSpec::Skeleton => ProjectionMode::Skeleton,
    }
}

fn map_host_setup(kind: HostSetupKindSpec) -> HostSetupKind {
    match kind {
        HostSetupKindSpec::Standalone => HostSetupKind::Standalone,
        HostSetupKindSpec::WorkspaceFootprint => HostSetupKind::WorkspaceFootprint,
        HostSetupKindSpec::PackageBacked => HostSetupKind::PackageBacked,
    }
}

fn map_host_project(spec: &HostProjectSpec) -> HostProject {
    HostProject {
        project_root: spec.project_root.to_string(),
        workspace_root: spec.workspace_root.to_string(),
        tsconfig_path: spec.tsconfig_path.to_string(),
        host_setup_kind: map_host_setup(spec.host_setup_kind),
    }
}

/// The symbol or expression the entry queries (the `symbol_or_expression`
/// identity axis): the symbol for `ResolveExpr` / `ShallowSurfaceExpr`, the
/// expression for `EvaluateExpr`.
fn symbol_or_expression(helper: &QueryHelperSpec) -> String {
    match helper {
        QueryHelperSpec::ResolveExpr { symbol, .. } => (*symbol).to_string(),
        QueryHelperSpec::ShallowSurfaceExpr { symbol } => (*symbol).to_string(),
        QueryHelperSpec::EvaluateExpr { expression, .. } => (*expression).to_string(),
    }
}

/// The entry's projection mode: derived from the helper payload, always
/// `Shallow` for `ShallowSurfaceExpr` (empty-path Shallow by construction).
fn mode_of(helper: &QueryHelperSpec) -> ProjectionModeSpec {
    match helper {
        QueryHelperSpec::ResolveExpr {
            projection_mode, ..
        } => *projection_mode,
        QueryHelperSpec::ShallowSurfaceExpr { .. } => ProjectionModeSpec::Shallow,
        QueryHelperSpec::EvaluateExpr {
            projection_mode, ..
        } => *projection_mode,
    }
}

/// The canonical `TypeExpr`-JSON `Value`s of the entry's type arguments
/// (`ResolveExpr` only). Each stored string MUST parse as JSON AND decode
/// through `type_expr_from_json` (a printable, admissible arg) — a malformed /
/// un-decodable arg is a `BadTypeArgument`, never a silently-passed `Unknown`.
fn type_argument_values(helper: &QueryHelperSpec) -> Result<Vec<Value>, DriverError> {
    let raw: &[&'static str] = match helper {
        QueryHelperSpec::ResolveExpr { type_args, .. } => type_args,
        _ => &[],
    };
    let mut out = Vec::with_capacity(raw.len());
    for (index, s) in raw.iter().enumerate() {
        let value: Value = serde_json::from_str(s).map_err(|e| DriverError::BadTypeArgument {
            index,
            detail: e.to_string(),
        })?;
        if type_expr_from_json(&value).is_none() {
            return Err(DriverError::BadTypeArgument {
                index,
                detail: "type argument did not decode through type_expr_from_json".to_string(),
            });
        }
        out.push(value);
    }
    Ok(out)
}

/// Convert a registry entry to the value-affecting [`SnapshotIdentity`] the
/// `snapshot_id` derivation hashes. Content-hashes each workspace file's source
/// (the registry is the source-byte authority), sorts the set by canonical path,
/// and REJECTS a duplicate path (a schema violation, §Q1).
#[allow(dead_code)]
pub(crate) fn identity_from_spec(spec: &QuerySpec) -> Result<SnapshotIdentity, DriverError> {
    let mut workspace_files: Vec<WorkspaceFileRef> = spec
        .workspace_files
        .iter()
        .map(|f| WorkspaceFileRef {
            path: f.path.to_string(),
            content_hash: identity::content_hash(f.source),
        })
        .collect();
    workspace_files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    for pair in workspace_files.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(DriverError::DuplicateWorkspacePath {
                path: pair[0].path.clone(),
            });
        }
    }

    let oracle_value_kind = match spec.oracle_value_kind {
        OracleValueKindSpec::StructuredTypeExpr => OracleValueKind::StructuredTypeExpr,
    };

    Ok(SnapshotIdentity {
        row_file: spec.row_file.to_string(),
        row_function: spec.row_function.to_string(),
        query_ordinal: spec.query_ordinal,
        query_helper_kind: map_helper_kind(&spec.query_helper),
        workspace_files,
        primary_canonical: spec.primary_canonical.to_string(),
        symbol_or_expression: symbol_or_expression(&spec.query_helper),
        type_arguments: type_argument_values(&spec.query_helper)?,
        projection_mode: map_mode_kind(mode_of(&spec.query_helper)),
        host_project: map_host_project(&spec.host_project),
        oracle_value_kind,
    })
}

/// The snapshot's tail relative to the snapshot tree root:
/// `<oracle_family>/<snapshot_id>.json`. The driver knows `oracle_family` from
/// the registry entry, so it can name the family sub-directory at test-body time
/// (§Q4).
#[allow(dead_code)]
pub(crate) fn snapshot_relative_tail(family: &str, snapshot_id: &str) -> String {
    format!("{family}/{snapshot_id}.json")
}

/// The ABSOLUTE on-disk snapshot path, rooted at `CARGO_MANIFEST_DIR` (=
/// `crates/verter_session/`) under the FULL `SNAPSHOT_TREE_INFIX`. Keeps the read
/// hermetic + absolute-path-free (it resolves to the in-repo crate dir).
#[allow(dead_code)]
pub(crate) fn snapshot_abs_path(family: &str, snapshot_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(SNAPSHOT_TREE_INFIX)
        .join(snapshot_relative_tail(family, snapshot_id))
}

/// The ABSOLUTE on-disk vendored-corpus root for the CURRENT corpus
/// (`oracle_env/<env_corpus_id>/`), rooted at `CARGO_MANIFEST_DIR`.
#[allow(dead_code)]
pub(crate) fn corpus_root(env_corpus_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(ORACLE_ENV_INFIX)
        .join(env_corpus_id)
}

/// Validate the snapshot's stored env-pins / `snapshot_id` / `row_ref` against
/// the registry-derived expectation (§3 invariant 3). A `compiler_options_hash`
/// match ALONE does not validate a snapshot — the full pinned-env set + the
/// re-derived `snapshot_id` must match (the corpus re-enumeration is a separate
/// step). Returns the first mismatch, so a stale snapshot is a hard failure.
#[allow(dead_code)]
pub(crate) fn validate_env_pins(
    snapshot: &OracleSnapshot,
    spec: &QuerySpec,
    identity: &SnapshotIdentity,
    env: &PinnedEnv,
) -> Result<String, DriverError> {
    let derived_id = identity::derive_snapshot_id(identity, env);

    let checks: [(&str, &str, &str); 9] = [
        ("tsgo_version", &snapshot.tsgo_version, &env.tsgo_version),
        (
            "oracle_schema_version",
            &snapshot.oracle_schema_version.to_string(),
            &env.oracle_schema_version.to_string(),
        ),
        (
            "normalizer_version",
            &snapshot.normalizer_version.to_string(),
            &env.normalizer_version.to_string(),
        ),
        (
            "probe_synthesis_version",
            &snapshot.probe_synthesis_version.to_string(),
            &env.probe_synthesis_version.to_string(),
        ),
        (
            "compiler_options_hash",
            &snapshot.compiler_options_hash,
            &env.compiler_options_hash,
        ),
        ("env_corpus_id", &snapshot.env_corpus_id, &env.env_corpus_id),
        ("snapshot_id", &snapshot.snapshot_id, &derived_id),
        (
            "row_ref.row_file",
            &snapshot.row_ref.row_file,
            spec.row_file,
        ),
        (
            "row_ref.row_function",
            &snapshot.row_ref.row_function,
            spec.row_function,
        ),
    ];
    for (field, found, expected) in checks {
        if found != expected {
            return Err(DriverError::EnvPinMismatch {
                field: field.to_string(),
                expected: expected.to_string(),
                found: found.to_string(),
            });
        }
    }
    if snapshot.row_ref.query_ordinal != spec.query_ordinal {
        return Err(DriverError::EnvPinMismatch {
            field: "row_ref.query_ordinal".to_string(),
            expected: spec.query_ordinal.to_string(),
            found: snapshot.row_ref.query_ordinal.to_string(),
        });
    }
    Ok(derived_id)
}

/// Re-enumerate the vendored corpus directory + recompute `oracle_env_hash`
/// (§Q5 `oracle_env_corpus_is_closed` + offline env re-derivation). Asserts the
/// directory's CURRENT recursive listing set-equals the stored `manifest`
/// (catching an ADDED file as well as an edit/delete) BEFORE content-hashing,
/// then recomputes the BLAKE3 `oracle_env_hash` over `oracle_env_files.files`
/// against current on-disk content and compares to the stored value. Never runs
/// tsgo.
#[allow(dead_code)]
pub(crate) fn validate_env_corpus(
    snapshot: &OracleSnapshot,
    corpus_root: &Path,
) -> Result<(), DriverError> {
    // (a) membership: the on-disk listing set-equals the stored manifest.
    let mut on_disk = enumerate_corpus(corpus_root);
    on_disk.sort();
    on_disk.dedup();
    let mut stored: Vec<String> = snapshot.oracle_env_files.manifest.clone();
    stored.sort();
    stored.dedup();
    if on_disk != stored {
        return Err(DriverError::CorpusMembershipDrift {
            detail: format!("on_disk={on_disk:?} stored_manifest={stored:?}"),
        });
    }

    // (b) content: recompute oracle_env_hash from the stored files list against
    // current on-disk content.
    let recomputed = recompute_oracle_env_hash(&snapshot.oracle_env_files.files, corpus_root);
    if recomputed != snapshot.oracle_env_hash {
        return Err(DriverError::OracleEnvHashDrift {
            expected: snapshot.oracle_env_hash.clone(),
            found: recomputed,
        });
    }
    Ok(())
}

/// Recursively enumerate the corpus directory, returning each file's
/// corpus-relative `/`-separated path (the canonical spelling the manifest
/// stores). A non-existent root yields the empty set.
fn enumerate_corpus(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let rel = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &rel, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out
}

/// Recompute the `oracle_env_hash` (§Q1): BLAKE3, domain-separated under the
/// `oracle_env_hash` tag, over the canonical-path-sorted `{ path, content_hash }`
/// list — `content_hash` recomputed from CURRENT on-disk corpus content under
/// the pinned content normalization. Distinct DOMAIN tag from `env_corpus_id`
/// (different role) — the two are intentionally distinct digests over the same
/// file set.
fn recompute_oracle_env_hash(files: &[snapshot::EnvFileEntry], corpus_root: &Path) -> String {
    let mut sorted: Vec<(&str, String)> = files
        .iter()
        .map(|f| {
            let abs = corpus_root.join(&f.path);
            let content = std::fs::read_to_string(&abs).unwrap_or_default();
            (f.path.as_str(), identity::content_hash(&content))
        })
        .collect();
    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let listing = Value::Array(
        sorted
            .iter()
            .map(|(path, hash)| serde_json::json!({ "path": path, "content_hash": hash }))
            .collect(),
    );
    let canonical = normalize::canonical_json_string(&listing);
    let mut input = Vec::new();
    input.extend_from_slice(b"verter.oracle.oracle_env_hash.v1");
    input.extend_from_slice(canonical.as_bytes());
    format!("blake3:{}", blake3::hash(&input).to_hex())
}

/// Normalize Verter's in-process `TypeExpr` and the snapshot's stored
/// `oracle_value` under the SAME normalization (the query's mode) and assert
/// structural equality. The stored value is already normalized at generation;
/// re-normalizing it is idempotent, so the compare is symmetric + confluent.
/// Compares canonical-JSON strings — structural equality, never display text.
#[allow(dead_code)]
pub(crate) fn compare_oracle_value(
    verter_expr: &TypeExpr,
    snapshot: &OracleSnapshot,
    mode: ProjectionModeKind,
) -> Result<(), DriverError> {
    let verter_canonical = normalize::normalized_canonical_json(verter_expr, mode)
        .map_err(DriverError::NormalizeReject)?;
    let oracle_expr =
        type_expr_from_json(&snapshot.oracle_value).ok_or(DriverError::ValueMismatch {
            verter: verter_canonical.clone(),
            oracle: "<oracle_value did not decode to TypeExpr>".to_string(),
        })?;
    let oracle_canonical = normalize::normalized_canonical_json(&oracle_expr, mode)
        .map_err(DriverError::NormalizeReject)?;
    if verter_canonical != oracle_canonical {
        return Err(DriverError::ValueMismatch {
            verter: verter_canonical,
            oracle: oracle_canonical,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper dispatch (host construction + the support.rs helper call)
// ---------------------------------------------------------------------------

/// Build the host the entry's `host_setup_kind` names, upsert the entry's
/// workspace files, and run the `support.rs` helper named by `query_helper` to
/// produce Verter's in-process `TypeExpr`.
#[allow(dead_code)]
fn run_helper(spec: &QuerySpec) -> Result<TypeExpr, DriverError> {
    use crate::typeinfo::typeinfo_tests::support::{
        evaluate_expr, make_host_with_footprint, make_host_with_workspace_files_footprint,
        resolve_expr, shallow_surface_expr, upsert_ts,
    };

    let host = match spec.host_project.host_setup_kind {
        HostSetupKindSpec::Standalone => {
            let host = make_host_with_footprint();
            for f in spec.workspace_files {
                upsert_ts(&host, f.path, f.source);
            }
            host
        }
        HostSetupKindSpec::WorkspaceFootprint | HostSetupKindSpec::PackageBacked => {
            let files: Vec<(&str, &str)> = spec
                .workspace_files
                .iter()
                .map(|f| (f.path, f.source))
                .collect();
            make_host_with_workspace_files_footprint(&files)
        }
    };

    let expr = match &spec.query_helper {
        QueryHelperSpec::ResolveExpr {
            symbol,
            type_args,
            projection_mode,
        } => {
            let type_arg_exprs: Vec<Arc<TypeExpr>> = type_args
                .iter()
                .enumerate()
                .map(|(index, s)| {
                    let value: Value =
                        serde_json::from_str(s).map_err(|e| DriverError::BadTypeArgument {
                            index,
                            detail: e.to_string(),
                        })?;
                    type_expr_from_json(&value)
                        .map(Arc::new)
                        .ok_or(DriverError::BadTypeArgument {
                            index,
                            detail: "type argument did not decode through type_expr_from_json"
                                .to_string(),
                        })
                })
                .collect::<Result<_, _>>()?;
            let (expr, _record) = resolve_expr(
                &host,
                spec.primary_canonical,
                symbol,
                &type_arg_exprs,
                map_resolver_mode(*projection_mode),
            );
            expr
        }
        QueryHelperSpec::ShallowSurfaceExpr { symbol } => {
            shallow_surface_expr(&host, spec.primary_canonical, symbol)
        }
        QueryHelperSpec::EvaluateExpr {
            expression,
            projection_mode,
        } => {
            let (expr, _record) = evaluate_expr(
                &host,
                spec.primary_canonical,
                expression,
                map_resolver_mode(*projection_mode),
            );
            expr
        }
    };
    Ok(expr)
}

// ---------------------------------------------------------------------------
// The orchestrator the #[oracle_row] macro calls
// ---------------------------------------------------------------------------

/// The shared registry driver a lifted row's synthesized body calls
/// (`oracle::run_row(file!(), "<fn>")`). Runs every registry query the row owns
/// and asserts each against its checked-in snapshot. Panics (the test-failure
/// surface) on any divergence — a missing snapshot, a stale env-pin, a corpus
/// drift, or a real `TypeExpr` parity mismatch.
#[allow(dead_code)]
pub(crate) fn run_row(file: &str, function: &str) {
    let basename = row_basename(file);
    let entries = lookup_row_entries(ORACLE_QUERY_SPECS, basename, function);
    if entries.is_empty() {
        panic!(
            "{}",
            describe(DriverError::NoRegistryEntries {
                row_file: basename.to_string(),
                row_function: function.to_string(),
            })
        );
    }
    let env = pinned_env();
    for spec in entries {
        if let Err(err) = run_one(spec, &env) {
            panic!(
                "oracle::run_row({basename}::{function}#{}): {}",
                spec.query_ordinal,
                describe(err)
            );
        }
    }
}

/// Validate ONE registry entry against its snapshot. Split out so its error path
/// is testable without a panic.
fn run_one(spec: &QuerySpec, env: &PinnedEnv) -> Result<(), DriverError> {
    let verter_expr = run_helper(spec)?;
    let identity = identity_from_spec(spec)?;
    let snapshot_id = identity::derive_snapshot_id(&identity, env);
    let path = snapshot_abs_path(spec.oracle_family, &snapshot_id);
    let bytes = std::fs::read(&path).map_err(|e| DriverError::EnvPinMismatch {
        field: "snapshot_file".to_string(),
        expected: path.display().to_string(),
        found: e.to_string(),
    })?;
    let json: Value = serde_json::from_slice(&bytes)
        .map_err(|e| DriverError::Decode(snapshot::SnapshotDecodeError::Envelope(e.to_string())))?;
    let snapshot = snapshot::decode_strict(&json).map_err(DriverError::Decode)?;
    validate_env_pins(&snapshot, spec, &identity, env)?;
    validate_env_corpus(&snapshot, &corpus_root(&env.env_corpus_id))?;
    compare_oracle_value(&verter_expr, &snapshot, identity.projection_mode)?;
    Ok(())
}

fn describe(err: DriverError) -> String {
    format!("{err:?}")
}

#[cfg(test)]
mod tests;
