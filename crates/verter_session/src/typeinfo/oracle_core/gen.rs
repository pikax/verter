//! The TS7 `TypeExpr`-projection oracle SNAPSHOT GENERATOR — the build/test-time
//! tool that drives the pinned tsgo and writes the checked-in snapshots
//! (the TS7 oracle contract §2 "Generation", §4 generator-side
//! table). Behind the `oracle-gen` feature ONLY, so the default resolver build
//! and the default test gate stay tsgo-free (§3 inv 1,
//! `oracle_tsgo_forbidden::tsgo_not_reachable_from_resolver`). It is NEVER on the
//! consumption path: lifted rows compare against the checked-in snapshots via the
//! `#[cfg(test)]` `driver`, never against a live tsgo.
//!
//! The pipeline, per registry query spec (§4 generator-side row):
//!
//! 1. seed a hermetic tsgo sandbox from the CLOSED, VENDORED env corpus (the
//!    canonical `oracle.tsconfig.json` + the vendored lib/ambient `.d.ts` set) and
//!    the spec's per-row workspace files;
//! 2. synthesize the FIXED, VERSIONED probe per `query_helper_kind` (same-file
//!    append for `ResolveExpr` / `ShallowSurfaceExpr`, §Q2);
//! 3. drive tsgo's `textDocument/hover` over the probe (Q3 — the LSP driver);
//! 4. extract the probe RHS from the markdown hover via the versioned grammar;
//! 5. run the TWO-SIDED positive-allowlist admission (default-REJECT) — the hover
//!    AST AND the real fixture SOURCE walked through the shared resolver
//!    (`resolve_source_declarations`), restricted to the PROVABLY
//!    SINGLE-CONTRIBUTOR class (§Scope `source_is_provably_single_contributor`);
//! 6. lower the admitted hover RHS to a `TypeExpr` and normalize it;
//! 7. record the `source_admission_digest` (the recorded source-side admission),
//!    the `raw_capture`, and the `oracle_env_files` / `oracle_env_hash`, then
//!    assemble the canonical snapshot document
//!    (`snapshot::assemble_snapshot_document`) and write it.
//!
//! [`run_oracle_gen`] is the single `pub` entry the `src/bin/oracle_gen` binary
//! invokes. It walks the oracle-query-spec registry (every LIFTED row — the
//! authoritative seated set lives in `ORACLE_QUERY_SPECS`, pinned exactly by
//! `oracle_query_specs_registry_holds_the_lifted_rows_and_is_well_formed`) and
//! writes one snapshot per spec. The per-spec pipeline ([`generate_snapshot`])
//! is also exercised end-to-end against the pinned tsgo over a SYNTHETIC spec
//! by `gen_tests::oracle_gen_is_idempotent`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use verter_type_runtime::tsgo::ipc::TsgoTypeProvider;
use verter_type_runtime::{path_to_file_uri_string, TypeProvider};

use super::admission::{self, AdmissionVerdict, SourceWalkResult};
use super::hover_extract;
use super::identity::{
    self, HostProject, HostSetupKind, OracleValueKind, PinnedEnv, ProbeRhsKind, QueryHelperKind,
    SnapshotIdentity, WorkspaceFileRef,
};
use super::normalize::{self, ProjectionModeKind};
use super::probe;
use super::query_specs::{
    HostSetupKindSpec, LiftMigrationProvenance, ProbeRhsSpec, ProjectionModeSpec, QueryHelperSpec,
    QuerySpec, COMPILER_OPTIONS_HASH, CURRENT_ENV_CORPUS_ID, LIFTED_ROW_MIGRATIONS,
    ORACLE_QUERY_SPECS,
};
use super::relation_probe;
use super::snapshot::{self, ProbeLocator};
use super::source_digest::{
    build_source_digest, build_source_host, source_side_walk, workspace_file_source,
    SourceDigestError,
};

/// The snapshot-tree infix from `CARGO_MANIFEST_DIR` (= `crates/verter_session/`).
/// MIRRORS the consumption driver's `SNAPSHOT_TREE_INFIX` (which the
/// `snapshot_loading_is_runtime_fs` guard pins on the READ side); generation
/// WRITES into the SAME on-disk tree.
const SNAPSHOT_TREE_INFIX: &str = "src/typeinfo/typeinfo_tests/oracle_snapshots";
// The vendored-corpus-root infix + the on-disk dir-name mapping are owned by
// `identity` (`identity::ORACLE_ENV_INFIX` / `identity::env_corpus_dir_name`),
// shared with the consumption driver; generation WRITES into the SAME tree
// the driver reads.

/// The domain-separation tag for the `oracle_env_hash` digest (§Q1). MUST equal
/// the consumption driver's `recompute_oracle_env_hash` tag — the generation side
/// and the read side hash the corpus under the SAME recipe, else a freshly
/// generated snapshot would not re-validate on read.
const ORACLE_ENV_HASH_DOMAIN_TAG: &str = "verter.oracle.oracle_env_hash.v1";
/// The domain-separation tag for the `env_corpus_id` digest (§Q1) — a DISTINCT
/// domain from `oracle_env_hash` (different role), over the same corpus listing.
const ENV_CORPUS_ID_DOMAIN_TAG: &str = "verter.oracle.env_corpus_id.v1";

/// Why a generation run could not produce a snapshot. The generator NEVER writes
/// a partial / guessed snapshot — every failure is loud.
#[derive(Debug)]
pub enum GenError {
    /// The pinned tsgo binary is not installed — generation is skipped, not
    /// failed (mirrors the spike's SKIP posture in a tsgo-less environment).
    TsgoUnavailable(String),
    /// The tsgo LSP driver failed to spawn / respond.
    TsgoDriver(String),
    /// tsgo returned no hover at the probe offset.
    NoHover,
    /// The hover-extraction grammar could not recover the probe RHS.
    HoverExtract(String),
    /// The two-sided admission gate REJECTED the capture (a non-allowlisted
    /// construct on the hover OR the source side, or a non-single-contributor
    /// source walk). Carries the verdict's debug rendering for diagnosis (a
    /// `String`, not the `pub(crate)` `AdmissionVerdict`, so the `pub` `GenError`
    /// leaks no crate-private type).
    Rejected(String),
    /// The admitted hover RHS did not lower to a `TypeExpr`.
    LoweringFailed,
    /// The admitted value failed normalization.
    NormalizeFailed(String),
    /// The primary fixture file named by the spec is absent from its
    /// `workspace_files`.
    MissingPrimaryFile(String),
    /// The queried declaration's span could not be located by re-parsing the
    /// primary fixture source (so the `source_admission_digest.decl_span` could
    /// not be recorded).
    DeclSpanNotFound(String),
    /// The `EvaluateExpr` scratch-prelude generation model is not yet wired
    /// (it needs the scope's `eval_source` prelude — a generation-time
    /// step gated on its own spike). A loud rejection, never a silent skip.
    UnsupportedHelperKind(&'static str),
    /// An I/O error writing the snapshot / seeding the sandbox.
    Io(String),
    /// The async runtime could not be built.
    Runtime(String),
    /// The reducer preflight failed: Verter's OWN resolver did not produce
    /// a clean, operator-free reduced value for the spec's query (an opaque miss,
    /// a carrier / operator shell, or an `Unknown`/`any`/`never`). The snapshot is
    /// NOT written — admitting a source root requires the resolver to actually
    /// reduce it, so no snapshot can MASK an unresolved indexed/mapped shell.
    PreflightUnclean(String),
}

/// The shared `source_admission_digest` derivation lives in `source_digest`; its
/// failures map onto the generator's loud, never-silent `GenError` surface.
impl From<SourceDigestError> for GenError {
    fn from(e: SourceDigestError) -> Self {
        match e {
            SourceDigestError::WalkNotResolved(detail) => GenError::Rejected(format!(
                "source-side walk did not resolve to a contributor vector: {detail}"
            )),
            SourceDigestError::MissingFile(path) => GenError::MissingPrimaryFile(path),
            SourceDigestError::DeclSpanNotFound(name) => GenError::DeclSpanNotFound(name),
        }
    }
}

/// The generation config: where the vendored corpus lives, where snapshots are
/// written, and the pinned env that enters every `snapshot_id`.
pub(crate) struct GenConfig {
    /// The vendored env-corpus root
    /// (`oracle_env/<env_corpus_dir_name(env_corpus_id)>/`).
    pub(crate) corpus_root: PathBuf,
    /// The snapshot output tree (`oracle_snapshots/`).
    pub(crate) snapshot_root: PathBuf,
    /// The pinned env + algorithm versions.
    pub(crate) env: PinnedEnv,
}

impl GenConfig {
    /// The default checked-in config: the corpus + snapshot trees under
    /// `CARGO_MANIFEST_DIR`, and the pinned env from the registry constants.
    fn checked_in() -> Self {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let env = PinnedEnv {
            tsgo_version: identity::TSGO_VERSION.to_string(),
            oracle_schema_version: identity::ORACLE_SCHEMA_VERSION,
            normalizer_version: normalize::NORMALIZER_VERSION,
            probe_synthesis_version: identity::PROBE_SYNTHESIS_VERSION,
            compiler_options_hash: COMPILER_OPTIONS_HASH.to_string(),
            env_corpus_id: CURRENT_ENV_CORPUS_ID.to_string(),
        };
        GenConfig {
            corpus_root: Path::new(manifest_dir)
                .join(identity::ORACLE_ENV_INFIX)
                .join(identity::env_corpus_dir_name(&env.env_corpus_id)),
            snapshot_root: Path::new(manifest_dir).join(SNAPSHOT_TREE_INFIX),
            env,
        }
    }
}

/// Generate + write every registry snapshot, returning the count written (one
/// per `ORACLE_QUERY_SPECS` entry plus one per `RELATION_QUERY_SPECS` entry).
/// The per-spec bodies ([`generate_snapshot`] / [`generate_relation_snapshot`])
/// are additionally exercised against real tsgo by the idempotence tests.
pub fn run_oracle_gen() -> Result<usize, GenError> {
    let config = GenConfig::checked_in();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| GenError::Runtime(e.to_string()))?;
    let mut written = 0usize;
    for spec in ORACLE_QUERY_SPECS {
        // Attribute a rejection to the row that produced it — a bare verdict
        // is undiagnosable across a multi-row registry.
        let document = runtime
            .block_on(generate_snapshot(spec, &config))
            .map_err(|e| match e {
                GenError::Rejected(msg) => {
                    GenError::Rejected(format!("{}::{} — {msg}", spec.row_file, spec.row_function))
                }
                other => other,
            })?;
        write_snapshot(&config, spec.oracle_family, &document)?;
        written += 1;
    }
    // The v4 `relation_verdict` capture family (the relation tuple-wire probe —
    // a SEPARATE path from the TypeExpr pipeline above, never through the
    // two-sided admission, which correctly rejects Conditional/Infer).
    for spec in super::query_specs::RELATION_QUERY_SPECS {
        let document = runtime
            .block_on(generate_relation_snapshot(spec, &config))
            .map_err(|e| match e {
                GenError::Rejected(msg) => {
                    GenError::Rejected(format!("{}::{} — {msg}", spec.row_file, spec.row_function))
                }
                other => other,
            })?;
        write_snapshot(&config, spec.oracle_family, &document)?;
        written += 1;
    }
    Ok(written)
}

/// Deterministic, TSGO-FREE v3→v4 snapshot re-keying (§Q4 +
/// the TS7 oracle contract). The v4 schema change
/// ADDS the closed `relation_verdict` value kind; for the EXISTING
/// `structured_type_expr` snapshots the ONLY change is `oracle_schema_version`
/// 3→4 flowing into `snapshot_id` through `PinnedEnv` (the v3-family hash-input
/// field set is UNCHANGED — the domain tag stays v2). Every tsgo-derived byte
/// (oracle_value / raw_capture / source_admission_digest / oracle_env_*) is
/// UNCHANGED, so each snapshot is re-keyed by bumping the stored version,
/// recomputing the (now-changed) `snapshot_id`, writing the new file, and
/// removing the stale one. Re-running is byte-idempotent: an already-v4
/// snapshot recomputes the SAME id (so it overwrites itself, never deleting
/// the file it just wrote). `relation_verdict` files (generated FRESH at v4,
/// never re-keyed) are skipped. Returns `(written, deleted)`. NEVER drives
/// tsgo — the proof is byte-equality of the tsgo-derived content plus
/// idempotence of the id redrive.
pub fn upgrade_snapshots_to_v4() -> Result<(usize, usize), GenError> {
    let config = GenConfig::checked_in();
    upgrade_snapshots_to_v4_in(&config.snapshot_root)
}

/// The root-parameterized body of [`upgrade_snapshots_to_v4`] (the checked-in
/// entry delegates with the checked-in snapshot tree). Split out so the
/// deterministic tsgo-free re-key proof runs over a SYNTHETIC temp tree —
/// never mutating the checked-in snapshots from a test.
pub(crate) fn upgrade_snapshots_to_v4_in(root: &Path) -> Result<(usize, usize), GenError> {
    // Collect every current snapshot path FIRST, so the newly-written v4 files are
    // not re-processed mid-walk.
    let mut files: Vec<PathBuf> = Vec::new();
    for fam in std::fs::read_dir(root).map_err(|e| GenError::Io(e.to_string()))? {
        let fam_dir = fam.map_err(|e| GenError::Io(e.to_string()))?.path();
        if !fam_dir.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&fam_dir).map_err(|e| GenError::Io(e.to_string()))? {
            let p = entry.map_err(|e| GenError::Io(e.to_string()))?.path();
            if p.extension().and_then(|e| e.to_str()) == Some("json") {
                files.push(p);
            }
        }
    }
    files.sort();

    let mut written = 0usize;
    let mut deleted = 0usize;
    for path in files {
        let bytes = std::fs::read(&path).map_err(|e| GenError::Io(e.to_string()))?;
        let mut json: Value = serde_json::from_slice(&bytes)
            .map_err(|e| GenError::Io(format!("{}: {e}", path.display())))?;

        // Only `structured_type_expr` snapshots re-key; the relation family is
        // generated fresh at v4.
        let kind = json["oracle_value_kind"].as_str().ok_or_else(|| {
            GenError::Io(format!("{}: missing oracle_value_kind", path.display()))
        })?;
        if kind != "structured_type_expr" {
            continue;
        }
        let family = json["oracle_family"]
            .as_str()
            .ok_or_else(|| GenError::Io(format!("{}: missing oracle_family", path.display())))?
            .to_string();
        let stored_version = json["oracle_schema_version"].as_u64().ok_or_else(|| {
            GenError::Io(format!("{}: missing oracle_schema_version", path.display()))
        })?;
        if stored_version != 3 && stored_version != identity::ORACLE_SCHEMA_VERSION as u64 {
            return Err(GenError::Rejected(format!(
                "{}: unexpected oracle_schema_version {stored_version} (the v3→v4 re-key only \
                 upgrades v3 files)",
                path.display()
            )));
        }

        {
            let obj = json
                .as_object_mut()
                .ok_or_else(|| GenError::Io(format!("{}: not a JSON object", path.display())))?;
            obj.insert(
                "oracle_schema_version".to_string(),
                json!(identity::ORACLE_SCHEMA_VERSION),
            );
        }

        // Recompute the snapshot_id from the now-v4 envelope (the schema-version
        // bump flows into the id through `PinnedEnv`). decode_strict doubles as a
        // sanity gate that the re-keyed envelope is well-formed.
        let snapshot =
            snapshot::decode_strict(&json).map_err(|e| GenError::Rejected(format!("{e:?}")))?;
        let new_id = snapshot::redrive_snapshot_id(&snapshot)
            .map_err(|e| GenError::Rejected(format!("{e:?}")))?;
        json.as_object_mut()
            .unwrap()
            .insert("snapshot_id".to_string(), json!(new_id));

        let new_path = root.join(&family).join(format!("{new_id}.json"));
        let text = normalize::canonical_json_string(&json);
        std::fs::write(&new_path, text).map_err(|e| GenError::Io(e.to_string()))?;
        written += 1;
        if new_path != path {
            std::fs::remove_file(&path).map_err(|e| GenError::Io(e.to_string()))?;
            deleted += 1;
        }
    }
    Ok((written, deleted))
}

/// Write `document` to `oracle_snapshots/<family>/<snapshot_id>.json` under the
/// canonical encoding. The `snapshot_id` is read back from the assembled
/// document (which derived it from identity + env) so the filename and the stored
/// id can never disagree.
fn write_snapshot(config: &GenConfig, family: &str, document: &Value) -> Result<(), GenError> {
    let snapshot_id = document
        .get("snapshot_id")
        .and_then(Value::as_str)
        .ok_or_else(|| GenError::Io("assembled document missing snapshot_id".to_string()))?;
    let family_dir = config.snapshot_root.join(family);
    std::fs::create_dir_all(&family_dir).map_err(|e| GenError::Io(e.to_string()))?;
    let path = family_dir.join(format!("{snapshot_id}.json"));
    let text = normalize::canonical_json_string(document);
    std::fs::write(&path, text).map_err(|e| GenError::Io(e.to_string()))?;
    Ok(())
}

/// The per-spec generation pipeline (§4 generator-side row). Produces the full
/// canonical snapshot document for one `(row, query)`. Drives the pinned tsgo;
/// returns [`GenError::TsgoUnavailable`] (skip) when tsgo is not installed.
/// Requires the row's retained migration provenance (a LIFTED row MUST carry
/// it — a missing entry is a loud generation failure); the provenance
/// parameter is split out ([`generate_snapshot_with_migration`]) so the
/// generator's own idempotence proof can drive the SAME pipeline over a
/// SYNTHETIC spec with a synthetic provenance record.
pub(crate) async fn generate_snapshot(
    spec: &QuerySpec,
    config: &GenConfig,
) -> Result<Value, GenError> {
    let migration = lookup_migration(spec)?;
    generate_snapshot_with_migration(spec, config, migration).await
}

/// The [`generate_snapshot`] body with the retained migration provenance
/// handed in explicitly (the production entry resolves it through
/// [`lookup_migration`]; the generator's idempotence proof supplies a
/// synthetic record).
pub(crate) async fn generate_snapshot_with_migration(
    spec: &QuerySpec,
    config: &GenConfig,
    migration: &LiftMigrationProvenance,
) -> Result<Value, GenError> {
    // (0) reducer preflight — BEFORE driving tsgo or writing anything, prove
    //     Verter's own resolver reduces the query to a clean, operator-free
    //     value. Keeps the source-root carve-out sound: a snapshot is never
    //     written for a row whose resolver result is an unresolved shell.
    preflight_reduces_clean(spec)?;

    // (1) The probe — same-file append for the two append-model helpers.
    let synth = synthesize_probe(spec)?;

    // (2) The source side — walk the real fixture declaration through the shared
    //     resolver, restricted to the provably-single-contributor class — and
    //     CROSS-CHECK the declared capture strategy against the live carve-out
    //     classification BEFORE driving tsgo or assembling anything: a spec
    //     cannot claim the keyof scaffold for a non-keyof row.
    let source_walk = source_side_walk(spec);
    cross_check_probe_strategy(spec, &source_walk)?;

    // (3) Drive tsgo over the corpus-seeded sandbox + the probe.
    let hover_contents = drive_hover(config, spec, &synth).await?;

    // (4) Extract the probe RHS from the markdown hover.
    let probe_name = probe::probe_name(spec.query_ordinal);
    let hover_rhs = hover_extract::extract_probe_rhs(&hover_contents, &probe_name)
        .map_err(|e| GenError::HoverExtract(format!("{e:?}")))?;

    // (5) Two-sided admission (default-REJECT) + single-contributor restriction.
    let mode = projection_mode_of(spec);
    let verdict = admission::admit_query(&hover_rhs, &source_walk, mode);
    if !matches!(verdict, AdmissionVerdict::Admit) {
        return Err(GenError::Rejected(format!("{verdict:?}")));
    }
    let contributors = match &source_walk {
        SourceWalkResult::Resolved { contributors } if contributors.len() == 1 => contributors,
        // >1 contributor (import/merge/augmentation/transitive hop) is the
        // DEFERRED multi-contributor class — default-reject (§Scope).
        other => {
            return Err(GenError::Rejected(format!(
                "non-single-contributor source walk: {other:?}"
            )))
        }
    };

    // (6) Lower + normalize the admitted hover RHS → oracle_value.
    let lowered = admission::lower_hover_rhs(&hover_rhs).ok_or(GenError::LoweringFailed)?;
    let normalized = normalize::normalize(&lowered, mode)
        .map_err(|e| GenError::NormalizeFailed(format!("{e:?}")))?;
    let oracle_value = normalized.to_json_value();

    // (7) Assemble the sub-objects + the envelope.
    let identity = build_identity(spec);
    let probe_locator = ProbeLocator {
        probe_name: probe_name.clone(),
        offset: synth.probe_name_offset as u64,
    };
    let raw_capture = json!({
        "probe_name": probe_name,
        "probe_header": probe::probe_header(spec.query_ordinal, &synth.rhs),
        "probe_scaffold": synth.scaffold,
        "hover_contents": hover_contents,
    });
    let source_admission_digest = build_source_digest(spec, contributors)?;
    let (oracle_env_files, oracle_env_hash) = build_env_files(config)?;

    let document = snapshot::assemble_snapshot_document(
        spec.oracle_family,
        &identity,
        &config.env,
        &oracle_value,
        &probe_locator,
        &raw_capture,
        &oracle_env_files,
        &oracle_env_hash,
        &source_admission_digest,
        migration.migration_fingerprint_version,
        migration.migration_fingerprint,
    );
    Ok(document)
}

/// The retained migration provenance for a lifted row (§Q4). A lifted row MUST
/// carry retained provenance — a missing entry is a loud generation failure, not
/// a silently-omitted snapshot mirror.
fn lookup_migration(spec: &QuerySpec) -> Result<&'static LiftMigrationProvenance, GenError> {
    LIFTED_ROW_MIGRATIONS
        .iter()
        .find(|m| m.row_file == spec.row_file && m.row_function == spec.row_function)
        .ok_or_else(|| {
            GenError::Rejected(format!(
                "{}::{}: no retained LIFTED_ROW_MIGRATIONS provenance for the lifted row",
                spec.row_file, spec.row_function
            ))
        })
}

/// The synthesized probe: the cloned primary-file source with the versioned probe
/// appended, plus the RHS text and the probe-name offset for the hover request.
struct Synthesized {
    /// The probe file source (primary file content + appended probe).
    source: String,
    /// The probe RHS as written into the header (for `raw_capture.probe_header`).
    rhs: String,
    /// The scaffold helper declaration emitted before the probe line
    /// (`DistributiveIdentity` only; `None` for the bare strategy). Recorded as
    /// `raw_capture.probe_scaffold` so the offline audit re-derives the FULL
    /// synthesized probe text from version + spec.
    scaffold: Option<String>,
    /// The byte offset of the probe NAME in `source` (the hover request point).
    probe_name_offset: usize,
}

/// Synthesize the fixed, versioned probe per `query_helper_kind` (§Q2). The
/// append-model helpers (`ResolveExpr` / `ShallowSurfaceExpr`) clone the primary
/// file and append `type __oracle_probe__N = <rhs>;` so the probe resolves in the
/// same scope Verter did. A `DistributiveIdentity` strategy additionally emits
/// the per-query identity helper immediately before the probe line and wraps the
/// symbol (`__oracle_probe_dist__N<Symbol>`); it requires EMPTY `type_args`
/// (the parameterized printer is its own deferred spike). `EvaluateExpr` (the
/// scratch + `eval_source`-prelude model) is not yet wired — a loud rejection.
fn synthesize_probe(spec: &QuerySpec) -> Result<Synthesized, GenError> {
    let primary_source = workspace_file_source(spec, spec.primary_canonical)
        .ok_or_else(|| GenError::MissingPrimaryFile(spec.primary_canonical.to_string()))?;
    let (rhs, scaffold) = match &spec.query_helper {
        QueryHelperSpec::ResolveExpr {
            symbol,
            type_args,
            probe_rhs,
            ..
        } => match probe_rhs {
            ProbeRhsSpec::Bare => {
                // `resolve_expr_probe_rhs` takes `&[String]`; the registry carries
                // `&[&str]`. A non-empty set defers (parameterized probe-RHS printer
                // spike, §Q2) — a loud rejection from the helper, not a guess.
                let owned: Vec<String> = type_args.iter().map(|s| (*s).to_string()).collect();
                let rhs = probe::resolve_expr_probe_rhs(symbol, &owned)
                    .map_err(|e| GenError::HoverExtract(format!("probe RHS synthesis: {e:?}")))?;
                (rhs, None)
            }
            ProbeRhsSpec::DistributiveIdentity => {
                if !type_args.is_empty() {
                    return Err(GenError::HoverExtract(
                        "probe RHS synthesis: DistributiveIdentity requires empty type_args \
                         (the parameterized printer is its own deferred spike)"
                            .to_string(),
                    ));
                }
                let scaffold = probe::distributive_identity_scaffold(spec.query_ordinal, symbol);
                (scaffold.rhs, Some(scaffold.helper_decl))
            }
        },
        QueryHelperSpec::ShallowSurfaceExpr { symbol } => ((*symbol).to_string(), None),
        QueryHelperSpec::EvaluateExpr { .. } => {
            return Err(GenError::UnsupportedHelperKind("EvaluateExpr"))
        }
    };
    let appended = probe::append_probe_with_scaffold(
        primary_source,
        spec.query_ordinal,
        &rhs,
        scaffold.as_deref(),
    );
    Ok(Synthesized {
        source: appended.source,
        rhs,
        scaffold,
        probe_name_offset: appended.probe_name_offset,
    })
}

/// Map a canonical leading-slash path (`/fixtures/foo.ts`) to a corpus/sandbox
/// relative path (`fixtures/foo.ts`) — the spelling tsgo's file-system root sees.
fn sandbox_relative(canonical: &str) -> &str {
    canonical.strip_prefix('/').unwrap_or(canonical)
}

/// Drive tsgo's `textDocument/hover` over a hermetic sandbox seeded from the
/// vendored corpus (the canonical `tsconfig.json` + the vendored libs) and the
/// spec's per-row workspace files, with the probe written in place of the primary
/// fixture. Returns the raw hover contents, or [`GenError::TsgoUnavailable`] when
/// tsgo is not installed (a SKIP, mirroring the spike).
async fn drive_hover(
    config: &GenConfig,
    spec: &QuerySpec,
    synth: &Synthesized,
) -> Result<String, GenError> {
    let tsgo_bin = resolve_tsgo_bin()?;
    // The per-row workspace files (the primary one REPLACED by the probe source).
    let primary_rel = sandbox_relative(spec.primary_canonical);
    let mut files: Vec<(String, String)> = Vec::new();
    for f in spec.workspace_files {
        let rel = sandbox_relative(f.path);
        let content = if f.path == spec.primary_canonical {
            synth.source.clone()
        } else {
            f.source.to_string()
        };
        files.push((rel.to_string(), content));
    }
    drive_hover_over_files(
        config,
        &tsgo_bin,
        &files,
        primary_rel,
        synth.probe_name_offset as u32,
    )
    .await
}

/// Resolve the PINNED tsgo binary the oracle harness generates with — EXACTLY
/// `@typescript/native-preview` `identity::TSGO_VERSION` (`7.0.0-dev.20260526.1`).
/// The dev-only generation harness does NOT ride the product toolchain
/// resolver: the resolver's stable-only support window (`>=7.0.2, <7.1.0`)
/// governs the PRODUCT's engine provisioning, while this harness pins an exact
/// nightly — every snapshot records that version and the consumption driver
/// validates it, so ANY other engine (a stable the product would accept
/// included) would produce snapshots the env-pin rail rejects. Resolution
/// order: (1) `VERTER_TSGO_BIN` naming an existing file, then (2) the
/// project-local `node_modules` install (flat + pnpm-store layouts) walking
/// the crate manifest's ancestors. Every candidate must report `--version`
/// EXACTLY `Version {TSGO_VERSION}` — a version mismatch is a generation
/// error, never a silent fallthrough. [`GenError::TsgoUnavailable`] (skip)
/// when no exact-pin binary is found.
fn resolve_tsgo_bin() -> Result<String, GenError> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(override_path) = std::env::var_os("VERTER_TSGO_BIN").filter(|v| !v.is_empty()) {
        candidates.push(PathBuf::from(override_path));
    }
    let exe_name = if cfg!(windows) { "tsgo.exe" } else { "tsgo" };
    let platform = tsgo_platform_key().ok_or_else(|| {
        GenError::TsgoUnavailable(format!(
            "no @typescript/native-preview platform package for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    for ancestor in Path::new(env!("CARGO_MANIFEST_DIR")).ancestors() {
        // Flat layout: `node_modules/@typescript/native-preview-<platform>/lib/tsgo`.
        candidates.push(
            ancestor
                .join("node_modules")
                .join("@typescript")
                .join(format!("native-preview-{platform}"))
                .join("lib")
                .join(exe_name),
        );
        // pnpm-store layout:
        // `node_modules/.pnpm/@typescript+native-preview-<platform>@<version>/node_modules/…`.
        candidates.push(
            ancestor
                .join("node_modules")
                .join(".pnpm")
                .join(format!(
                    "@typescript+native-preview-{platform}@{}",
                    identity::TSGO_VERSION
                ))
                .join("node_modules")
                .join("@typescript")
                .join(format!("native-preview-{platform}"))
                .join("lib")
                .join(exe_name),
        );
    }
    let expected = format!("Version {}", identity::TSGO_VERSION);
    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let output = std::process::Command::new(&candidate)
            .arg("--version")
            .output()
            .map_err(|e| {
                GenError::TsgoUnavailable(format!(
                    "{}: --version probe failed: {e}",
                    candidate.display()
                ))
            })?;
        let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if reported != expected {
            return Err(GenError::TsgoUnavailable(format!(
                "{} reports `{reported}`, not the harness pin `{expected}` — generation with a \
                 different engine would produce snapshots the env-pin rail rejects",
                candidate.display()
            )));
        }
        return Ok(candidate.to_string_lossy().into_owned());
    }
    Err(GenError::TsgoUnavailable(format!(
        "no @typescript/native-preview {platform} binary at the exact harness pin {} \
         (searched VERTER_TSGO_BIN + project-local node_modules)",
        identity::TSGO_VERSION
    )))
}

/// The `@typescript/native-preview-<platform>` package key for this host.
fn tsgo_platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("macos", "x86_64") => Some("darwin-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("linux", "x86_64") => Some("linux-x64"),
        ("windows", "aarch64") => Some("win32-arm64"),
        ("windows", "x86_64") => Some("win32-x64"),
        _ => None,
    }
}

/// The shared hover drive: seed the corpus sandbox, write `files`
/// (sandbox-relative path + content), spawn the pinned tsgo LSP, open every
/// file, and hover `hover_rel` at `hover_offset`. Both capture families ride
/// this ONE drive — the v3 append-model probes and the v4 relation tuple-wire
/// probe differ only in the file set + hover point they hand in.
async fn drive_hover_over_files(
    config: &GenConfig,
    tsgo_bin: &str,
    files: &[(String, String)],
    hover_rel: &str,
    hover_offset: u32,
) -> Result<String, GenError> {
    let sandbox = tempfile::tempdir().map_err(|e| GenError::Io(e.to_string()))?;

    // (a) Seed the vendored corpus. The canonical `oracle.tsconfig.json` becomes
    //     the root `tsconfig.json` tsgo reads; every other corpus file keeps its
    //     corpus-relative path.
    seed_corpus(&config.corpus_root, sandbox.path())?;

    // (b) Write the workspace files.
    let mut files_to_open: Vec<(PathBuf, String)> = Vec::new();
    for (rel, content) in files {
        let abs = sandbox.path().join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| GenError::Io(e.to_string()))?;
        }
        std::fs::write(&abs, content).map_err(|e| GenError::Io(e.to_string()))?;
        files_to_open.push((abs, content.clone()));
    }
    let hover_abs = sandbox.path().join(hover_rel);

    // (c) Spawn the tsgo LSP and open every program file. The root URI goes
    // through the shared Windows-safe builder (drive letters, backslashes).
    let root_uri = path_to_file_uri_string(&sandbox.path().to_string_lossy());
    let provider = match tokio::time::timeout(
        Duration::from_secs(30),
        TsgoTypeProvider::spawn(tsgo_bin, &root_uri),
    )
    .await
    {
        Ok(Ok(p)) => p,
        Ok(Err(e)) => return Err(GenError::TsgoDriver(e.to_string())),
        Err(_) => return Err(GenError::TsgoDriver("spawn timed out".to_string())),
    };
    for (abs, content) in &files_to_open {
        let _ = provider.open_file(&abs.to_string_lossy(), content).await;
    }

    // (d) Hover the probe name.
    let hover = tokio::time::timeout(
        Duration::from_secs(15),
        provider.get_hover(&hover_abs.to_string_lossy(), hover_offset),
    )
    .await
    .map_err(|_| GenError::TsgoDriver("hover timed out".to_string()))?
    .map_err(|e| GenError::TsgoDriver(e.to_string()))?
    .ok_or(GenError::NoHover)?;
    Ok(hover.contents)
}

/// The per-spec v4 relation generation pipeline
/// (the TS7 oracle contract): a relation-specific
/// capture path BESIDE the TypeExpr pipeline — NOT through the two-sided
/// admission (which correctly rejects the Conditional/Infer the probe embodies)
/// and with NO reducer preflight (a capture-only relation row asserts nothing
/// about Verter's own reduction; the engine-observation boundary is the
/// consumption driver's concern). Steps: derive the registry-derivable v4
/// identity (tsgo-free, loud on any operand/binder inconsistency) → synthesize
/// the versioned tuple-wire probe file → drive the pinned tsgo's hover over the
/// probe name → extract the RHS → STRICT tuple-wire decode (anything off the
/// fixed grammar is a loud generation error) → cross-check the wire's binder
/// names against the declared layout (ordinal AND name, in preorder) →
/// assemble the canonical `relation_verdict` document and prove it strictly
/// decodes before writing.
pub(crate) async fn generate_relation_snapshot(
    spec: &super::query_specs::RelationQuerySpec,
    config: &GenConfig,
) -> Result<Value, GenError> {
    // (1) The registry-derivable identity — tsgo-free; a bad operand text or an
    //     inconsistent binder layout is a loud rejection, never a guessed axis.
    let identity = relation_probe::relation_identity_from_spec(spec).map_err(|e| {
        GenError::Rejected(format!(
            "{}: relation identity derivation: {e:?}",
            spec.row_function
        ))
    })?;

    // (2) The versioned probe file + hover point.
    let probe_source = relation_probe::relation_probe_source(
        spec.row_function,
        spec.query_ordinal,
        spec.source_text,
        spec.target_text,
        &identity.binder_layout,
    );
    let probe_header = relation_probe::relation_probe_header(
        spec.query_ordinal,
        spec.source_text,
        spec.target_text,
        &identity.binder_layout,
    );
    let probe_name = probe::probe_name(spec.query_ordinal);
    let probe_name_offset = probe_source.find(&probe_name).ok_or_else(|| {
        GenError::HoverExtract(format!(
            "{}: synthesized probe source does not contain `{probe_name}`",
            spec.row_function
        ))
    })?;
    let canonical_path = relation_probe::relation_probe_canonical_path(spec.row_function);
    let rel = sandbox_relative(&canonical_path).to_string();

    // (3) Drive the pinned tsgo over the corpus-seeded sandbox + the probe file.
    let tsgo_bin = resolve_tsgo_bin()?;
    let hover_contents = drive_hover_over_files(
        config,
        &tsgo_bin,
        &[(rel.clone(), probe_source)],
        &rel,
        probe_name_offset as u32,
    )
    .await?;

    // (4) Extract the probe RHS from the markdown hover and STRICT-decode the
    //     tuple wire — hover is ONLY the transport; anything off the fixed
    //     grammar is a loud generation error.
    let hover_rhs = hover_extract::extract_probe_rhs(&hover_contents, &probe_name)
        .map_err(|e| GenError::HoverExtract(format!("{e:?}")))?;
    let value = relation_probe::decode_tuple_wire(&hover_rhs).map_err(|e| {
        GenError::Rejected(format!("{}: tuple-wire decode: {e:?}", spec.row_function))
    })?;

    // (5) Cross-check the wire's bindings against the DECLARED binder layout.
    //     An ASSIGNABLE verdict binds EVERY declared binder — ordinal AND name
    //     must match in target-pattern preorder (the strict decoder already
    //     enforced the ordinal sequence + uniqueness; a name divergence from
    //     the registry layout is a capture the registry cannot explain). A
    //     FAILED-infer capture (`not_assignable`, non-empty layout) records
    //     ZERO matched bindings by definition (the wire's false branch is
    //     always the empty tuple) — no preorder check applies to it.
    let bindings_consistent = match value.verdict {
        relation_probe::RelationVerdict::Assignable => {
            value.bindings.len() == identity.binder_layout.len()
                && value
                    .bindings
                    .iter()
                    .zip(identity.binder_layout.iter())
                    .all(|(binding, layout)| {
                        binding.ordinal == layout.ordinal && binding.name == layout.name
                    })
        }
        relation_probe::RelationVerdict::NotAssignable => value.bindings.is_empty(),
    };
    if !bindings_consistent {
        return Err(GenError::Rejected(format!(
            "{}: wire verdict `{}` with bindings {:?} does not match the declared binder layout {:?}",
            spec.row_function,
            value.verdict.tag(),
            value
                .bindings
                .iter()
                .map(|b| (b.ordinal, b.name.as_str()))
                .collect::<Vec<_>>(),
            identity
                .binder_layout
                .iter()
                .map(|b| (b.ordinal, b.name.as_str()))
                .collect::<Vec<_>>(),
        )));
    }

    // (5b) Bound⊢constraint verification for CONSTRAINED binders (`infer X
    //     extends C`): re-ask the pinned tsgo — through the SAME anti-
    //     distribution tuple wire in a SECOND, generation-only probe file —
    //     whether each captured bound is assignable to its declared
    //     constraint. A violation is a loud generation error, never a silent
    //     escape. A `not_assignable` main verdict carries no bindings, so
    //     there is nothing to verify; constraint-free rows skip the second
    //     drive entirely. The check probes are verification-only and are NOT
    //     persisted in the snapshot.
    let checks: Vec<(u16, String, String)> =
        if value.verdict == relation_probe::RelationVerdict::Assignable {
            spec.binder_layout
                .iter()
                .filter_map(|declared| {
                    declared.constraint.map(|constraint_text| {
                        let binding = &value.bindings[declared.ordinal as usize];
                        let bound_text = binding.bound_text.clone().ok_or_else(|| {
                            GenError::Rejected(format!(
                                "{}: captured binding `{}` carries no wire bound text for the \
                             constraint check",
                                spec.row_function, binding.name
                            ))
                        })?;
                        Ok((declared.ordinal, bound_text, constraint_text.to_string()))
                    })
                })
                .collect::<Result<_, GenError>>()?
        } else {
            Vec::new()
        };
    if !checks.is_empty() {
        let check_source = relation_probe::relation_check_probe_source(spec.row_function, &checks);
        let check_rel = format!(
            "fixtures/relation_verdict/{}_constraint_checks.ts",
            spec.row_function
        );
        let tsgo_bin = resolve_tsgo_bin()?;
        for (ordinal, _, constraint_text) in &checks {
            let check_name = relation_probe::relation_check_probe_name(*ordinal);
            let check_offset = check_source.find(&check_name).ok_or_else(|| {
                GenError::HoverExtract(format!(
                    "{}: synthesized check source does not contain `{check_name}`",
                    spec.row_function
                ))
            })?;
            let check_hover = drive_hover_over_files(
                config,
                &tsgo_bin,
                &[(check_rel.clone(), check_source.clone())],
                &check_rel,
                check_offset as u32,
            )
            .await?;
            let check_rhs = hover_extract::extract_probe_rhs(&check_hover, &check_name)
                .map_err(|e| GenError::HoverExtract(format!("{e:?}")))?;
            let check_verdict = relation_probe::decode_tuple_wire(&check_rhs).map_err(|e| {
                GenError::Rejected(format!(
                    "{}: constraint-check wire decode: {e:?}",
                    spec.row_function
                ))
            })?;
            if check_verdict.verdict != relation_probe::RelationVerdict::Assignable {
                return Err(GenError::Rejected(format!(
                    "{}: captured bound for binder ordinal {ordinal} VIOLATES the declared \
                     constraint `{constraint_text}` — a bound violating a present constraint is \
                     a generation error, never a silent escape",
                    spec.row_function
                )));
            }
        }
    }

    // (6) Assemble the oracle value + the envelope. Each bound is the canonical
    //     normalized TypeExpr JSON under the ONE relation-binding projection
    //     (the strict decoder projected it); NO SemanticNodeId anywhere.
    let oracle_value = json!({
        "verdict": value.verdict.tag(),
        "bindings": value
            .bindings
            .iter()
            .map(|b| json!({
                "ordinal": b.ordinal,
                "name": b.name,
                "bound": b.bound.to_json_value(),
            }))
            .collect::<Vec<_>>(),
    });
    let probe_locator = ProbeLocator {
        probe_name: probe_name.clone(),
        offset: probe_name_offset as u64,
    };
    let raw_capture = json!({
        "probe_name": probe_name,
        "probe_header": probe_header,
        "probe_scaffold": Value::Null,
        "hover_contents": hover_contents,
    });
    let (oracle_env_files, oracle_env_hash) = build_env_files(config)?;
    let document = snapshot::assemble_relation_snapshot_document(
        spec.oracle_family,
        &identity,
        &config.env,
        &oracle_value,
        &probe_locator,
        &raw_capture,
        &oracle_env_files,
        &oracle_env_hash,
    );

    // (7) Self-check: the assembled document must STRICTLY DECODE (the v4
    //     raw-capture rail re-decodes the recorded hover to the stored value) —
    //     a snapshot that cannot re-validate offline is never written.
    snapshot::decode_strict(&document).map_err(|e| {
        GenError::Rejected(format!(
            "{}: assembled relation snapshot failed strict decode: {e:?}",
            spec.row_function
        ))
    })?;
    Ok(document)
}

/// Recursively copy every file under `corpus_root` into `dest`, mapping the
/// canonical `oracle.tsconfig.json` to the root `tsconfig.json` tsgo reads. A
/// missing/empty corpus dir copies nothing (the synthetic-corpus idempotence test
/// supplies its own sandbox seed instead).
fn seed_corpus(corpus_root: &Path, dest: &Path) -> Result<(), GenError> {
    fn walk(dir: &Path, rel: &str, dest: &Path) -> Result<(), GenError> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let child_rel = if rel.is_empty() {
                name.to_string()
            } else {
                format!("{rel}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &child_rel, dest)?;
            } else {
                // The canonical oracle tsconfig is delivered as the root config.
                let target_rel = if child_rel == "oracle.tsconfig.json" {
                    "tsconfig.json".to_string()
                } else {
                    child_rel
                };
                let abs = dest.join(&target_rel);
                if let Some(parent) = abs.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| GenError::Io(e.to_string()))?;
                }
                std::fs::copy(&path, &abs).map_err(|e| GenError::Io(e.to_string()))?;
            }
        }
        Ok(())
    }
    walk(corpus_root, "", dest)
}

/// The generator capture-strategy cross-check (§Q2): the DECLARED `probe_rhs`
/// strategy must agree with the LIVE source-walk carve-out classification.
/// `DistributiveIdentity` is admissible ONLY when (a) the query's projection
/// mode is `Expanded` AND (b) the walk resolved EXACTLY ONE contributor whose
/// body classifies into the keyof carve-out family (`KeyofBareRef` /
/// `KeyofSelfIndex` — the scaffold is applied UNIFORMLY to the family, never
/// branched on predicted tsgo display behavior). Any mismatch is a loud
/// [`GenError::Rejected`]. A `Bare` declaration is never constrained here —
/// under-delivery (a bare probe whose hover echoes `keyof …`) is caught by the
/// retained hover-side `KeyOf` reject, while THIS gate prevents over-claiming.
fn cross_check_probe_strategy(
    spec: &QuerySpec,
    source_walk: &SourceWalkResult,
) -> Result<(), GenError> {
    let QueryHelperSpec::ResolveExpr {
        probe_rhs: ProbeRhsSpec::DistributiveIdentity,
        projection_mode,
        ..
    } = &spec.query_helper
    else {
        return Ok(());
    };
    let reject = |why: &str| {
        Err(GenError::Rejected(format!(
            "{}::{}: declared DistributiveIdentity strategy does not match the live \
             classification: {why}",
            spec.row_file, spec.row_function
        )))
    };
    if *projection_mode != ProjectionModeSpec::Expanded {
        return reject("projection mode is not Expanded");
    }
    let SourceWalkResult::Resolved { contributors } = source_walk else {
        return reject("source walk did not resolve");
    };
    let [contributor] = contributors.as_slice() else {
        return reject("source walk is not single-contributor");
    };
    match admission::classify_source_root(&contributor.lowered_body) {
        admission::SourceRootShape::KeyofBareRef | admission::SourceRootShape::KeyofSelfIndex => {
            Ok(())
        }
        other => reject(&format!(
            "source root classifies as {other:?}, not the keyof carve-out family"
        )),
    }
}

/// The reducer PREFLIGHT (the TS7 oracle contract §Q2 —
/// "reducer-preflight before writing carve-out snapshots"). Before a snapshot is
/// assembled, run the SPEC'S query through Verter's ONE shared resolver in the
/// declared projection mode and require the projected result is CLEAN: NO opaque
/// miss (the resolver bound a node), NO carrier / operator shell (the node
/// projects to a `TypeExpr`), and the value passes the SAME positive-allowlist
/// predicate the oracle VALUE must clear (`admit_type_expr`: operator-free, no
/// `Unknown`/`any`/`never`). This is the soundness gate that lets the source-root
/// carve-out admit `keyof Root` / `Root["a"]["b"]...` ONLY when Verter actually
/// reduces them — a tsgo snapshot can never mask an unresolved indexed/mapped
/// shell. Adds NO tsgo (Verter resolution only) and NO second resolution engine
/// (the same `resolve_named_symbol_with_audit` every consumer rides).
fn preflight_reduces_clean(spec: &QuerySpec) -> Result<(), GenError> {
    let (symbol, mode) = match &spec.query_helper {
        QueryHelperSpec::ResolveExpr {
            symbol,
            type_args,
            projection_mode,
            ..
        } => {
            if !type_args.is_empty() {
                // A parameterized query needs the versioned type-argument printer;
                // no carve-out row carries one — defer loudly rather than preflight
                // a partial.
                return Err(GenError::PreflightUnclean(format!(
                    "{}::{}: parameterized ResolveExpr preflight is deferred",
                    spec.row_file, spec.row_function
                )));
            }
            (*symbol, resolver_mode_of(*projection_mode))
        }
        // `ShallowSurfaceExpr` / `EvaluateExpr` do not drive a named-symbol
        // resolution and no carve-out row uses them; their reducer-cleanliness is
        // covered by the two-sided admission, so the preflight is a pass-through.
        QueryHelperSpec::ShallowSurfaceExpr { .. } | QueryHelperSpec::EvaluateExpr { .. } => {
            return Ok(())
        }
    };

    let host = build_source_host(spec);
    let (outcome, _record) = host
        .resolve_named_symbol_with_audit(spec.primary_canonical, symbol, Some(mode))
        .into_parts();
    let node = outcome.ok().flatten().ok_or_else(|| {
        GenError::PreflightUnclean(format!(
            "{}::{}: resolver produced an opaque miss (no bound node) for `{symbol}`",
            spec.row_file, spec.row_function
        ))
    })?;
    let projected = host
        .project_node_to_type_expr_for_test(node)
        .ok_or_else(|| {
            GenError::PreflightUnclean(format!(
                "{}::{}: resolved node did not project to a TypeExpr (carrier/operator shell)",
                spec.row_file, spec.row_function
            ))
        })?;
    match admission::admit_type_expr(&projected) {
        AdmissionVerdict::Admit => Ok(()),
        verdict => Err(GenError::PreflightUnclean(format!(
            "{}::{}: reduced value is not operator-free/clean ({verdict:?}): {projected:?}",
            spec.row_file, spec.row_function
        ))),
    }
}

/// Map the registry's `ProjectionModeSpec` onto the resolver's `ProjectionMode`
/// (the mode the preflight resolves the query in — the same mapping the
/// consumption driver's `map_resolver_mode` uses).
fn resolver_mode_of(mode: ProjectionModeSpec) -> crate::semantic_query::ProjectionMode {
    use crate::semantic_query::ProjectionMode;
    match mode {
        ProjectionModeSpec::Shallow => ProjectionMode::Shallow,
        ProjectionModeSpec::Navigate => ProjectionMode::Navigate,
        ProjectionModeSpec::Expanded => ProjectionMode::Expanded,
        ProjectionModeSpec::Skeleton => ProjectionMode::Skeleton,
    }
}

/// Build the registry-derivable `SnapshotIdentity` (§Q1) for the spec. The
/// workspace files carry the canonical path + the SHA-256 content hash (never the
/// source bytes — the registry is the byte authority).
fn build_identity(spec: &QuerySpec) -> SnapshotIdentity {
    let workspace_files = spec
        .workspace_files
        .iter()
        .map(|f| WorkspaceFileRef {
            path: f.path.to_string(),
            content_hash: identity::content_hash(f.source),
        })
        .collect();
    let (symbol_or_expression, type_arguments) = match &spec.query_helper {
        QueryHelperSpec::ResolveExpr {
            symbol, type_args, ..
        } => (
            (*symbol).to_string(),
            type_args
                .iter()
                .filter_map(|t| serde_json::from_str::<Value>(t).ok())
                .collect(),
        ),
        QueryHelperSpec::ShallowSurfaceExpr { symbol } => ((*symbol).to_string(), Vec::new()),
        QueryHelperSpec::EvaluateExpr { expression, .. } => ((*expression).to_string(), Vec::new()),
    };
    SnapshotIdentity {
        row_file: spec.row_file.to_string(),
        row_function: spec.row_function.to_string(),
        query_ordinal: spec.query_ordinal,
        query_helper_kind: query_helper_kind(spec),
        workspace_files,
        primary_canonical: spec.primary_canonical.to_string(),
        symbol_or_expression,
        type_arguments,
        projection_mode: projection_mode_of(spec),
        probe_rhs_kind: probe_rhs_kind_of(spec),
        host_project: HostProject {
            project_root: spec.host_project.project_root.to_string(),
            workspace_root: spec.host_project.workspace_root.to_string(),
            tsconfig_path: spec.host_project.tsconfig_path.to_string(),
            host_setup_kind: host_setup_kind(spec.host_project.host_setup_kind),
        },
        oracle_value_kind: OracleValueKind::StructuredTypeExpr,
    }
}

/// The spec's declared capture strategy mapped onto the identity axis (only
/// `ResolveExpr` can carry a non-`Bare` one).
fn probe_rhs_kind_of(spec: &QuerySpec) -> ProbeRhsKind {
    match &spec.query_helper {
        QueryHelperSpec::ResolveExpr { probe_rhs, .. } => match probe_rhs {
            ProbeRhsSpec::Bare => ProbeRhsKind::Bare,
            ProbeRhsSpec::DistributiveIdentity => ProbeRhsKind::DistributiveIdentity,
        },
        QueryHelperSpec::ShallowSurfaceExpr { .. } | QueryHelperSpec::EvaluateExpr { .. } => {
            ProbeRhsKind::Bare
        }
    }
}

fn query_helper_kind(spec: &QuerySpec) -> QueryHelperKind {
    match &spec.query_helper {
        QueryHelperSpec::ResolveExpr { .. } => QueryHelperKind::ResolveExpr,
        QueryHelperSpec::ShallowSurfaceExpr { .. } => QueryHelperKind::ShallowSurfaceExpr,
        QueryHelperSpec::EvaluateExpr { .. } => QueryHelperKind::EvaluateExpr,
    }
}

fn projection_mode_of(spec: &QuerySpec) -> ProjectionModeKind {
    let mode = match &spec.query_helper {
        QueryHelperSpec::ResolveExpr {
            projection_mode, ..
        } => *projection_mode,
        QueryHelperSpec::ShallowSurfaceExpr { .. } => ProjectionModeSpec::Shallow,
        QueryHelperSpec::EvaluateExpr {
            projection_mode, ..
        } => *projection_mode,
    };
    match mode {
        ProjectionModeSpec::Shallow => ProjectionModeKind::Shallow,
        ProjectionModeSpec::Navigate => ProjectionModeKind::Navigate,
        ProjectionModeSpec::Expanded => ProjectionModeKind::Expanded,
        ProjectionModeSpec::Skeleton => ProjectionModeKind::Skeleton,
    }
}

fn host_setup_kind(kind: HostSetupKindSpec) -> HostSetupKind {
    match kind {
        HostSetupKindSpec::Standalone => HostSetupKind::Standalone,
        HostSetupKindSpec::WorkspaceFootprint => HostSetupKind::WorkspaceFootprint,
        HostSetupKindSpec::PackageBacked => HostSetupKind::PackageBacked,
    }
}

/// Build the `oracle_env_files` manifest + `oracle_env_hash` over the vendored
/// corpus (§Q1). The manifest is the canonical-path-sorted listing; the hash is
/// BLAKE3 domain-separated under [`ORACLE_ENV_HASH_DOMAIN_TAG`] over the
/// `{ path, content_hash }` list — the SAME recipe the consumption driver
/// recomputes on read.
fn build_env_files(config: &GenConfig) -> Result<(Value, String), GenError> {
    let mut listing = enumerate_corpus(&config.corpus_root);
    listing.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut files: Vec<(String, String)> = Vec::new();
    for rel in &listing {
        let abs = config.corpus_root.join(rel);
        let content = std::fs::read_to_string(&abs).map_err(|e| GenError::Io(e.to_string()))?;
        files.push((rel.clone(), identity::content_hash(&content)));
    }
    let manifest: Vec<Value> = listing.iter().map(|p| json!(p)).collect();
    let files_json: Vec<Value> = files
        .iter()
        .map(|(path, hash)| json!({ "path": path, "content_hash": hash }))
        .collect();
    let oracle_env_files = json!({ "manifest": manifest, "files": files_json });
    let oracle_env_hash = hash_listing(ORACLE_ENV_HASH_DOMAIN_TAG, &files);
    Ok((oracle_env_files, oracle_env_hash))
}

/// Compute the `env_corpus_id` (§Q1): BLAKE3 domain-separated under
/// [`ENV_CORPUS_ID_DOMAIN_TAG`] over the canonical-path-sorted corpus listing.
/// Used when (re-)vendoring a corpus to pin `CURRENT_ENV_CORPUS_ID` — a distinct
/// domain from `oracle_env_hash` over the same file set.
#[allow(dead_code)]
pub(crate) fn compute_env_corpus_id(corpus_root: &Path) -> Result<String, GenError> {
    let mut listing = enumerate_corpus(corpus_root);
    listing.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut files: Vec<(String, String)> = Vec::new();
    for rel in &listing {
        let abs = corpus_root.join(rel);
        let content = std::fs::read_to_string(&abs).map_err(|e| GenError::Io(e.to_string()))?;
        files.push((rel.clone(), identity::content_hash(&content)));
    }
    Ok(hash_listing(ENV_CORPUS_ID_DOMAIN_TAG, &files))
}

/// The shared digest recipe: `blake3:` over `domain_tag || canonical_json(listing)`
/// where `listing` is the canonical-path-sorted `[{ path, content_hash }]` array.
fn hash_listing(domain_tag: &str, files: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let listing = Value::Array(
        sorted
            .iter()
            .map(|(path, hash)| json!({ "path": path, "content_hash": hash }))
            .collect(),
    );
    let canonical = normalize::canonical_json_string(&listing);
    let mut input = Vec::new();
    input.extend_from_slice(domain_tag.as_bytes());
    input.extend_from_slice(canonical.as_bytes());
    format!("blake3:{}", blake3::hash(&input).to_hex())
}

/// Recursively enumerate the corpus directory's CORPUS-RELATIVE file listing
/// (forward-slash separators, no leading slash). An absent corpus dir yields an
/// empty listing.
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

#[cfg(test)]
mod gen_tests;
