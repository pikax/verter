//! The TS7 `TypeExpr`-projection oracle SNAPSHOT GENERATOR — the build/test-time
//! tool that drives the pinned tsgo and writes the checked-in snapshots
//! (`docs/arch/u0-oracle-harness-design.md` §2 "Generation", §4 generator-side
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
//! invokes. It walks the oracle-query-spec registry (the 11 lifted rows — two
//! index-signature publications + two built-in modifier utilities + three U2
//! IndexedAccess-reduction carve-outs + the mapped-modifier `-?` carve-out at
//! U2.MAPPED_TEMPLATE + three keyof-expansion carve-outs) and writes one
//! snapshot per spec. The per-spec pipeline ([`generate_snapshot`]) is also
//! exercised end-to-end against the pinned tsgo over a SYNTHETIC spec by
//! `gen_tests::oracle_gen_is_idempotent`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::{json, Value};
use verter_compiler::utils::oxc::vue::raw_surface::{
    RawDeclKind, RawKey, RawMemberKind, RawSourceSurface, SymbolSpace as RawSymbolSpace,
    TupleElementShape,
};
use verter_type_runtime::tsgo::ipc::{find_tsgo_binary, TsgoTypeProvider};
use verter_type_runtime::{path_to_file_uri_string, TypeProvider};

use crate::resolver_core::{CanonicalCompletionOverlay, HostResolverContext};
use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

use super::admission::{self, AdmissionVerdict, SourceContributor, SourceWalkResult};
use super::hover_extract;
use super::identity::{
    self, HostProject, HostSetupKind, OracleValueKind, PinnedEnv, ProbeRhsKind, QueryHelperKind,
    SnapshotIdentity, WorkspaceFileRef,
};
use super::normalize::{self, ProjectionModeKind};
use super::probe;
use super::query_specs::{
    HostSetupKindSpec, ProbeRhsSpec, ProjectionModeSpec, QueryHelperSpec, QuerySpec, SymbolSpace,
    COMPILER_OPTIONS_HASH, CURRENT_ENV_CORPUS_ID, ORACLE_QUERY_SPECS,
};
use super::snapshot::{self, ProbeLocator};
use super::source_walk::{resolve_source_declarations, SourceLocator};

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
/// per `ORACLE_QUERY_SPECS` entry — the 11 lifted rows). The per-spec body
/// ([`generate_snapshot`]) is additionally exercised against real tsgo by the
/// idempotence test.
pub fn run_oracle_gen() -> Result<usize, GenError> {
    let config = GenConfig::checked_in();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| GenError::Runtime(e.to_string()))?;
    let mut written = 0usize;
    for spec in ORACLE_QUERY_SPECS {
        let document = runtime.block_on(generate_snapshot(spec, &config))?;
        write_snapshot(&config, spec.oracle_family, &document)?;
        written += 1;
    }
    Ok(written)
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
pub(crate) async fn generate_snapshot(
    spec: &QuerySpec,
    config: &GenConfig,
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
    );
    Ok(document)
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

/// The upserted source bytes the spec's registry entry carries for `canonical`
/// (the registry is the source-byte authority).
fn workspace_file_source<'a>(spec: &'a QuerySpec, canonical: &str) -> Option<&'a str> {
    spec.workspace_files
        .iter()
        .find(|f| f.path == canonical)
        .map(|f| f.source)
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
    let tsgo_bin = find_tsgo_binary().map_err(|e| GenError::TsgoUnavailable(e.to_string()))?;
    let sandbox = tempfile::tempdir().map_err(|e| GenError::Io(e.to_string()))?;

    // (a) Seed the vendored corpus. The canonical `oracle.tsconfig.json` becomes
    //     the root `tsconfig.json` tsgo reads; every other corpus file keeps its
    //     corpus-relative path.
    seed_corpus(&config.corpus_root, sandbox.path())?;

    // (b) Write the per-row workspace files (the primary one REPLACED by the probe
    //     source).
    let primary_rel = sandbox_relative(spec.primary_canonical);
    let mut files_to_open: Vec<(PathBuf, String)> = Vec::new();
    for f in spec.workspace_files {
        let rel = sandbox_relative(f.path);
        let content = if f.path == spec.primary_canonical {
            synth.source.clone()
        } else {
            f.source.to_string()
        };
        let abs = sandbox.path().join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| GenError::Io(e.to_string()))?;
        }
        std::fs::write(&abs, &content).map_err(|e| GenError::Io(e.to_string()))?;
        files_to_open.push((abs, content));
    }
    let primary_abs = sandbox.path().join(primary_rel);

    // (c) Spawn the tsgo LSP and open every program file. The root URI goes
    // through the shared Windows-safe builder (drive letters, backslashes).
    let root_uri = path_to_file_uri_string(&sandbox.path().to_string_lossy());
    let provider = match tokio::time::timeout(
        Duration::from_secs(30),
        TsgoTypeProvider::spawn(&tsgo_bin, &root_uri),
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
        provider.get_hover(
            &primary_abs.to_string_lossy(),
            synth.probe_name_offset as u32,
        ),
    )
    .await
    .map_err(|_| GenError::TsgoDriver("hover timed out".to_string()))?
    .map_err(|e| GenError::TsgoDriver(e.to_string()))?
    .ok_or(GenError::NoHover)?;
    Ok(hover.contents)
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

/// Build a standalone `VerterHost` from the spec's workspace files and walk the
/// queried symbol's source-side declaration graph through the shared resolver
/// (`resolve_source_declarations`). This is the SAME construction the consumption
/// path uses; it adds NO tsgo and NO query-time resolution beyond the shared
/// resolver. Only the `standalone` host kind is first-class (§Scope).
fn source_side_walk(spec: &QuerySpec) -> SourceWalkResult {
    let host = build_source_host(spec);
    // Build-time oracle generator: build a quiescent owned view over the
    // freshly-constructed standalone host. The raw-view escape hatch is
    // allowlisted for this build-tool driver-snapshot rail.
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(CanonicalCompletionOverlay::new());
    let ctx = HostResolverContext::new(&host, &store_view, overlay);
    let locator = SourceLocator {
        reference_canonical: spec.source_locator.reference_canonical.to_string(),
        reference_name: spec.source_locator.reference_name.to_string(),
        symbol_space: to_walk_space(spec.source_locator.symbol_space),
    };
    resolve_source_declarations(&ctx, &locator)
}

/// The reducer PREFLIGHT (`docs/arch/u0-oracle-harness-design.md` §Q2 —
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
        .resolve_named_symbol_with_audit(spec.primary_canonical, symbol, &[], Some(mode))
        .into_parts();
    let node = outcome.ok().flatten().ok_or_else(|| {
        GenError::PreflightUnclean(format!(
            "{}::{}: resolver produced an opaque miss (no bound node) for `{symbol}`",
            spec.row_file, spec.row_function
        ))
    })?;
    let projected = host.project_node_to_type_expr(node).ok_or_else(|| {
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

/// Map the registry's `SymbolSpace` onto the resolver's raw-surface `SymbolSpace`
/// (the `SourceLocator` axis). Two distinct enums, one meaning.
fn to_walk_space(space: SymbolSpace) -> RawSymbolSpace {
    match space {
        SymbolSpace::Type => RawSymbolSpace::Type,
        SymbolSpace::Value => RawSymbolSpace::Value,
    }
}

/// Construct the standalone footprint host for the source-side walk and upsert
/// every workspace file (the `make_host_with_footprint` shape — the only
/// admissible host class currently). `workspace_footprint` /
/// package-backed kinds are deferred (§Scope); a spec carrying one still
/// constructs a host so the walk runs, but the snapshot's `host_setup_kind`
/// (set in [`build_identity`]) will fail `standalone_host_is_default_canonical_config`.
fn build_source_host(spec: &QuerySpec) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    for f in spec.workspace_files {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(f.path.to_string()),
            input_id: f.path.to_string(),
            source: Arc::from(f.source),
            file_kind: FileKind::from_path(f.path),
            aliases: Vec::new(),
        });
    }
    host
}

/// Assemble the `source_admission_digest` (§Q1) from the resolved contributor(s).
/// For the admitted single-contributor class the digest carries exactly one
/// contributor entry; its `decl_span` is recovered by re-parsing the primary
/// fixture source (a deterministic, offline-reproducible generation step), and
/// its `raw_surface` is rendered canonically from the parse-time raw-fact record.
fn build_source_digest(
    spec: &QuerySpec,
    contributors: &[SourceContributor],
) -> Result<Value, GenError> {
    let locator = &spec.source_locator;
    let space_tag = symbol_space_tag(locator.symbol_space);

    // The observed source-declaration files: each recorded contributor's defining
    // file, with the content hash of the registry source bytes for that path.
    let mut observed: Vec<(String, String)> = Vec::new();
    let mut entries: Vec<Value> = Vec::new();
    for c in contributors {
        // The defining file. `RawSourceSurface.decl_canonical` is stamped by the
        // file-aware storage layer and may be empty on this read path; for the
        // admitted PROVABLY-SINGLE-CONTRIBUTOR class (no import / re-export hop)
        // the defining file IS the locator's reference file (§Scope), so fall
        // back to it when the stamp is absent.
        let decl_canonical = if c.raw_surface.decl_canonical.is_empty() {
            locator.reference_canonical.to_string()
        } else {
            c.raw_surface.decl_canonical.clone()
        };
        let source = workspace_file_source(spec, &decl_canonical)
            .ok_or_else(|| GenError::MissingPrimaryFile(decl_canonical.clone()))?;
        let (start, end) = find_decl_span(source, locator.reference_name, locator.symbol_space)
            .ok_or_else(|| GenError::DeclSpanNotFound(locator.reference_name.to_string()))?;
        let content_hash = identity::content_hash(source);
        if !observed.iter().any(|(p, _)| p == &decl_canonical) {
            observed.push((decl_canonical.clone(), content_hash.clone()));
        }
        entries.push(json!({
            "contributor_ordinal": c.ordinal,
            "decl_span": { "file": decl_canonical, "start": start, "end": end },
            "decl_canonical": decl_canonical,
            "name": locator.reference_name,
            "symbol_space": space_tag,
            "decl_kind": raw_decl_kind_tag(c.raw_surface.decl_kind),
            "raw_surface": raw_surface_to_json(&c.raw_surface),
            "lowered_body": c.lowered_body.to_json_value(),
            "verdict": "Admit",
        }));
    }

    let observed_source_files: Vec<Value> = observed
        .iter()
        .map(|(path, hash)| json!({ "path": path, "content_hash": hash }))
        .collect();

    Ok(json!({
        "source_locator": {
            "reference_canonical": locator.reference_canonical,
            "reference_name": locator.reference_name,
            "symbol_space": space_tag,
        },
        "observed_source_files": observed_source_files,
        "contributors": entries,
        "final_verdict": "Admit",
    }))
}

/// Locate the `(start, end)` span of the top-level declaration named `name` in
/// `source` by re-parsing it with OXC. A deterministic, offline-reproducible
/// generation step (the `source_admission_digest_consistent` guard re-derives the
/// same span from current source). Type-space binds a type alias / interface /
/// enum / class; value-space binds a `const`/`let`/`var`, function, or class.
fn find_decl_span(source: &str, name: &str, space: SymbolSpace) -> Option<(u32, u32)> {
    use oxc_ast::ast::Declaration;
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if ret.panicked {
        return None;
    }
    for stmt in &ret.program.body {
        // A top-level declaration is either a bare declaration statement or one
        // wrapped in `export { ... }` / `export default`. Unwrap both.
        let decl: Option<&Declaration> = match stmt {
            Statement::ExportNamedDeclaration(e) => e.declaration.as_ref(),
            other => other.as_declaration(),
        };
        let Some(decl) = decl else { continue };
        let hit = match (space, decl) {
            (SymbolSpace::Type, Declaration::TSTypeAliasDeclaration(d)) => d.id.name == name,
            (SymbolSpace::Type, Declaration::TSInterfaceDeclaration(d)) => d.id.name == name,
            (SymbolSpace::Type, Declaration::TSEnumDeclaration(d)) => d.id.name == name,
            (_, Declaration::ClassDeclaration(d)) => {
                d.id.as_ref().map(|i| i.name == name).unwrap_or(false)
            }
            (SymbolSpace::Value, Declaration::FunctionDeclaration(d)) => {
                d.id.as_ref().map(|i| i.name == name).unwrap_or(false)
            }
            (SymbolSpace::Value, Declaration::VariableDeclaration(d)) => {
                d.declarations.iter().any(|v| {
                    v.id.get_binding_identifier()
                        .map(|i| i.name == name)
                        .unwrap_or(false)
                })
            }
            _ => false,
        };
        if hit {
            let span = decl.span();
            return Some((span.start, span.end));
        }
    }
    None
}

/// Render a `RawSourceSurface` to its canonical JSON (§Q1 example) — the
/// parse-time raw-fact record the digest stores. `RawSourceSurface` carries no
/// `Serialize`, so each field is rendered explicitly; the offline
/// `source_admission_digest_consistent` guard (first-lift) re-derives the SAME
/// shape from current source.
fn raw_surface_to_json(raw: &RawSourceSurface) -> Value {
    json!({
        "raw_member_keys": raw.raw_member_keys.iter().map(raw_key_tag).collect::<Vec<_>>(),
        "member_kinds": raw.member_kinds.iter().map(|k| raw_member_kind_tag(*k)).collect::<Vec<_>>(),
        "member_visibility": raw
            .member_visibility
            .iter()
            .map(|v| member_visibility_tag(*v))
            .collect::<Vec<_>>(),
        "unique_symbol_ops": vec![Value::Null; raw.unique_symbol_ops.len()]
            .iter()
            .map(|_| json!("UniqueSymbol"))
            .collect::<Vec<_>>(),
        "abstract_ctor": raw.abstract_ctor,
        "type_param_modifiers": raw
            .type_param_modifiers
            .iter()
            .map(|m| json!({
                "is_const": m.is_const,
                "variance_in": m.variance_in,
                "variance_out": m.variance_out,
            }))
            .collect::<Vec<_>>(),
        "this_type_or_param": raw.this_type_or_param,
        "value_const_assertion": raw.value_const_assertion,
        "overload_signatures": vec![Value::Null; raw.overload_signatures.len()]
            .iter()
            .map(|_| json!("OverloadSignature"))
            .collect::<Vec<_>>(),
        "tuple_element_shape": raw
            .tuple_element_shape
            .iter()
            .map(|t| tuple_element_shape_tag(*t))
            .collect::<Vec<_>>(),
        "utility_referent_names": raw.utility_referent_names.clone(),
        "transitive_referents": raw
            .transitive_referents
            .iter()
            .map(|r| json!({ "reference_name": r.reference_name }))
            .collect::<Vec<_>>(),
    })
}

fn raw_key_tag(key: &RawKey) -> Value {
    match key {
        RawKey::Static(s) => json!(format!("Static({s})")),
        other => json!(format!("{other:?}")),
    }
}

fn raw_member_kind_tag(kind: RawMemberKind) -> Value {
    json!(format!("{kind:?}"))
}

fn member_visibility_tag(v: verter_type_expr::MemberVisibility) -> Value {
    json!(format!("{v:?}"))
}

fn tuple_element_shape_tag(t: TupleElementShape) -> Value {
    json!(format!("{t:?}"))
}

fn raw_decl_kind_tag(kind: RawDeclKind) -> &'static str {
    match kind {
        RawDeclKind::TypeAlias => "TypeAlias",
        RawDeclKind::Interface => "Interface",
        RawDeclKind::Enum => "Enum",
        RawDeclKind::Class => "Class",
        RawDeclKind::Function => "Function",
        RawDeclKind::Variable => "Variable",
    }
}

fn symbol_space_tag(space: SymbolSpace) -> &'static str {
    match space {
        SymbolSpace::Type => "Type",
        SymbolSpace::Value => "Value",
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
