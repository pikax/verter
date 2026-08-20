//! 36-cell seed matrix through the production assembler and the harness
//! authored-source mapping oracle.
//!
//! Applicability comes from the locked manifest's `options.sourceMap`
//! request — never from whether a map turned up. Map-enabled cells:
//! well-formed v3 artifact, byte-stable serialization, decoded artifact
//! equals the independent JS reference. Map-disabled cells produce no
//! map. The oracle RAN (`mapping` reports `ran`); its mapping *verdict*
//! is not gated (fragment-emitter residuals are not this composition's).
//! Wire subset (`map-presence` / `map-version` / `mappings-decode`) is
//! owned here. Code bytes do not move vs the reference, or vs the
//! map-disabled twin.
//!
//! Child of the equality harness: uses that bridge, no second DTO
//! projection.
//!
//! `cargo test -p verter_session --lib --features bf2-authoritative
//! bf2_seed_matrix -- --test-threads=1 --nocapture`. One cell at a time;
//! `--test-threads=1` keeps harness scratch single-occupancy.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::*;
use crate::compile::AssembledVueModule;

/// One `check-candidate.mjs` run's ceiling. The oracle links and executes
/// pinned framework artifacts, so a few seconds is normal and a hang is not;
/// this bounds the latter without ever being reached by the former.
pub(super) const ORACLE_TIMEOUT: Duration = Duration::from_secs(180);

// The locked seed manifest

// `pub(super)` on this module's manifest-reading/compile/oracle-invocation
// plumbing (`Backend`, `SeedCell`, `read_seed_matrix`, `compile_cell`,
// `assemble`, `TempCandidate`, `Finished`, `run_bounded`, `ORACLE_TIMEOUT`) is
// deliberate reuse surface for the sibling `bf2_full_axis_gate` module (same
// `map_equality_tests` parent, same crate — no crate-boundary violation): the
// full-axis gate reads the exact same locked manifest and drives the exact
// same oracle CLI, and a second hand-written copy of this digest-verification
// and subprocess-handling logic is exactly the kind of common-mode error a
// second reader is supposed to catch, not reproduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Backend {
    Vdom,
    Vapor,
    Ssr,
}

impl Backend {
    fn parse(raw: &str) -> Self {
        match raw {
            "vdom" => Self::Vdom,
            "vapor" => Self::Vapor,
            "ssr" => Self::Ssr,
            other => panic!("the locked manifest names an unknown Vue backend `{other}`"),
        }
    }
}

/// One seed cell, as the LOCKED manifest defines it.
#[derive(Debug, Clone)]
pub(super) struct SeedCell {
    /// The manifest's logical name, e.g. `vue/slots__ssr__map1__prod0`.
    pub(super) golden_name: String,
    /// The fixture file name the record names, e.g. `slots.vue`.
    pub(super) fixture: String,
    pub(super) backend: Backend,
    /// THE applicability partition. Read from the record's own `options.sourceMap`
    /// request input — the compile axis BF2 asked the official compiler for.
    pub(super) source_map: bool,
    pub(super) is_production: bool,
}

pub(super) fn harness_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/framework-conformance-harness")
}

pub(super) fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, byte| {
            use std::fmt::Write;
            let _ = write!(acc, "{byte:02x}");
            acc
        })
}

pub(super) fn member<'a>(value: &'a Value, path: &[&str], context: &str) -> &'a Value {
    let mut current = value;
    for key in path {
        current = current
            .get(key)
            .unwrap_or_else(|| panic!("{context}: no member `{}`", path.join(".")));
    }
    current
}

pub(super) fn bool_member(value: &Value, path: &[&str], context: &str) -> bool {
    member(value, path, context)
        .as_bool()
        .unwrap_or_else(|| panic!("{context}: `{}` is not a boolean", path.join(".")))
}

pub(super) fn str_member(value: &Value, path: &[&str], context: &str) -> String {
    member(value, path, context)
        .as_str()
        .unwrap_or_else(|| panic!("{context}: `{}` is not a string", path.join(".")))
        .to_string()
}

/// Read the seed matrix out of the committed golden manifest.
///
/// The manifest names the valid record set; each record is content-addressed by
/// the digest the manifest lists, and that digest is VERIFIED here — a record
/// whose bytes do not hash to its manifest name is not the locked record, and
/// reading its request axis would be reading something else's.
pub(super) fn read_seed_matrix() -> Vec<SeedCell> {
    let goldens = harness_root().join("goldens");
    let manifest = read_json(&goldens.join("manifest.json"));
    let entries = manifest
        .get("entries")
        .and_then(Value::as_object)
        .expect("the golden manifest carries an `entries` map");

    let mut cells: Vec<SeedCell> = entries
        .iter()
        .filter(|(name, _)| name.starts_with("vue/"))
        .map(|(name, digest)| {
            let digest = digest
                .as_str()
                .unwrap_or_else(|| panic!("{name}: the manifest entry is not a digest string"));
            let record_path = goldens.join("records").join(format!("{digest}.json"));
            let record_bytes = std::fs::read(&record_path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", record_path.display()));
            let actual = sha256_hex(&record_bytes);
            assert_eq!(
                actual,
                digest,
                "{name}: the record at {} hashes to {actual}, not the digest the manifest names",
                record_path.display()
            );
            let record: Value = serde_json::from_slice(&record_bytes)
                .unwrap_or_else(|error| panic!("{}: not JSON: {error}", record_path.display()));

            let fixture_path = str_member(&record, &["fixture", "path"], name);
            let fixture = fixture_path
                .strip_prefix("fixtures/vue/")
                .unwrap_or_else(|| {
                    panic!(
                        "{name}: the record's fixture path `{fixture_path}` is not a Vue fixture"
                    )
                })
                .to_string();

            // The fixture the cell will be compiled from must be the exact one
            // the golden was generated from; otherwise the candidate and the
            // oracle would be describing different authored sources.
            let on_disk = harness_root().join(&fixture_path);
            let bytes = std::fs::read(&on_disk)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", on_disk.display()));
            let recorded = str_member(&record, &["fixture", "sha256"], name);
            let normalized = String::from_utf8(bytes.clone())
                .expect("the fixture is UTF-8")
                .replace("\r\n", "\n");
            assert!(
                sha256_hex(&bytes) == recorded || sha256_hex(normalized.as_bytes()) == recorded,
                "{name}: {} does not match the authored source the golden records \
                 ({recorded}) — the candidate would describe a different fixture",
                on_disk.display()
            );

            SeedCell {
                golden_name: name.clone(),
                fixture,
                backend: Backend::parse(&str_member(&record, &["options", "backend"], name)),
                source_map: bool_member(&record, &["options", "sourceMap"], name),
                is_production: bool_member(&record, &["options", "isProd"], name),
            }
        })
        .collect();

    cells.sort_by(|left, right| left.golden_name.cmp(&right.golden_name));
    cells
}

/// Compile and profile one cell through the same real-fixture path the
/// cross-implementation cases use.
///
/// `inline: Some(false)` is not a default: the harness generates every Vue
/// golden through official `compileScript({ inlineTemplate: false })`, on the
/// production axis as well, so a candidate at the carrier's own
/// prod-implies-inline default would be a different topology from the artifact
/// the oracle is handed alongside it.
pub(super) fn compile_cell(cell: &SeedCell) -> RealCompile {
    let mut compiled = compile_fixture(
        &cell.fixture,
        CompileAxes {
            ssr: cell.backend == Backend::Ssr,
            is_production: cell.is_production,
            source_map: cell.source_map,
            inline: Some(false),
            force_vapor: cell.backend == Backend::Vapor,
        },
    );
    compiled.id = cell.golden_name.clone();
    // The golden set is generated statically (never inside a live Vite
    // dev server), so it carries neither `__file` nor an HMR block for any
    // cell, dev or prod — official `@vitejs/plugin-vue`'s real
    // `transformMain` gates both on `devServer` being present, not on
    // `isProduction` alone. `compile_fixture`'s shared `HmrStrategy::Vite`
    // default is correct for its OTHER caller (the broader corpus in
    // `map_equality_tests.rs`, a different comparison this matrix doesn't
    // own), so it's overridden here rather than changed at the shared
    // helper.
    compiled.profile.hmr_strategy = HmrStrategy::None;
    // Same reasoning, one axis over: the golden's SSR cells are a bare
    // `@vue/compiler-sfc`-equivalent assembly (compileScript +
    // compileTemplate stitched together directly), never the full
    // `@vitejs/plugin-vue` bundler transform — so they carry no
    // `useSSRContext`/`ssrContext.modules` wrapper either, even though a
    // REAL bundled SSR build (dev or prod) always would (confirmed
    // directly against `@vitejs/plugin-vue`'s source — unconditional on
    // `ssr`, no dev-server gate). See `emit_ssr_module_registration`'s own
    // doc comment.
    compiled.profile.emit_ssr_module_registration = false;
    compiled
}

pub(super) fn assemble(case: &RealCompile) -> AssembledVueModule {
    assemble_vue_main_module(
        &case.canonical_id,
        &case.compiled,
        &case.meta,
        &case.profile,
    )
    .unwrap_or_else(|failure| {
        panic!(
            "{}: the production assembler failed closed on genuine compiler output: \
                 {failure:?}. Every seed cell must assemble; a fail-closed outcome here is \
                 either a composition defect or an uncomposable fragment map, and both are \
                 hard failures rather than a skipped cell.",
            case.id
        )
    })
}

// Required exit 1 — applicability from the locked manifest

/// Every cell is accounted for, and the accounting reads the manifest's own
/// request input rather than anything the candidate produced.
#[test]
fn seed_matrix_applicability_is_partitioned_from_the_locked_manifest() {
    let cells = read_seed_matrix();
    assert_eq!(
        cells.len(),
        36,
        "the locked manifest holds {} Vue seed cells, not the 36 the matrix is defined over",
        cells.len()
    );

    let enabled: Vec<&SeedCell> = cells.iter().filter(|cell| cell.source_map).collect();
    let disabled: Vec<&SeedCell> = cells.iter().filter(|cell| !cell.source_map).collect();
    assert_eq!(
        enabled.len() + disabled.len(),
        cells.len(),
        "some cell is neither map-enabled nor map-disabled"
    );
    assert_eq!(enabled.len(), 18, "map-enabled cells: {}", enabled.len());
    assert_eq!(disabled.len(), 18, "map-disabled cells: {}", disabled.len());

    // The manifest's LOGICAL NAME encodes the same axis. It is not the source of
    // the partition above — `options.sourceMap` is — but the two disagreeing
    // would mean the locked manifest is internally inconsistent, which must be
    // loud rather than silently resolved in favour of whichever was read.
    for cell in &cells {
        assert_eq!(
            cell.golden_name.contains("__map1__"),
            cell.source_map,
            "{}: the manifest name and the record's own `options.sourceMap` disagree",
            cell.golden_name
        );
        assert_eq!(
            cell.golden_name.ends_with("__prod1"),
            cell.is_production,
            "{}: the manifest name and the record's own `options.isProd` disagree",
            cell.golden_name
        );
    }

    // The axis space is covered exactly once: 3 fixtures × 3 backends × 2 × 2.
    let axes: BTreeSet<(String, Backend, bool, bool)> = cells
        .iter()
        .map(|cell| {
            (
                cell.fixture.clone(),
                cell.backend,
                cell.source_map,
                cell.is_production,
            )
        })
        .collect();
    assert_eq!(
        axes.len(),
        36,
        "the 36 cells cover only {} distinct axis combinations — some combination is \
         duplicated and some is missing",
        axes.len()
    );
    let fixtures: BTreeSet<&str> = cells.iter().map(|cell| cell.fixture.as_str()).collect();
    assert_eq!(
        fixtures,
        BTreeSet::from(["basic-interpolation.vue", "props-emit.vue", "slots.vue"]),
        "the seed fixtures moved"
    );
}

// Required exits 2 and 3 — the production result, per cell

/// Map presence follows the manifest's request input, the emitted map is a
/// well-formed flat v3 artifact in bounds of its own module, and production's
/// SERIALIZATION is byte-stable across repeated identical invocations.
///
/// Byte-stability is checked on the RAW serialized map, separately from the
/// decoded-artifact equality the reference comparison performs: two valid but
/// differently-encoded serializations of one logical artifact would pass a
/// decoded comparison while defeating any hash taken over the emitted bytes.
/// It is checked twice — once re-invoking assembly on the very same inputs
/// (which catches per-instance hash-map iteration order and anything else that
/// varies between two calls), and once over an independently recompiled bundle
/// (which catches the same class one layer up, in the carrier).
#[test]
fn map_presence_wire_validity_and_serialization_stability_hold_for_every_cell() {
    let cells = read_seed_matrix();
    let mut mapless_enabled = Vec::new();
    let mut segment_counts = BTreeMap::new();

    for cell in &cells {
        let case = compile_cell(cell);
        let first = assemble(&case);
        let second = assemble(&case);
        assert_eq!(
            first.code, second.code,
            "{}: two identical assembly invocations produced different code",
            cell.golden_name
        );
        assert_eq!(
            first.source_map, second.source_map,
            "{}: two identical assembly invocations produced different serialized map bytes",
            cell.golden_name
        );

        let recompiled = compile_cell(cell);
        let third = assemble(&recompiled);
        assert_eq!(
            first.code, third.code,
            "{}: an independent recompile of the same fixture produced different code",
            cell.golden_name
        );
        assert_eq!(
            first.source_map, third.source_map,
            "{}: an independent recompile of the same fixture produced different serialized \
             map bytes",
            cell.golden_name
        );

        if cell.source_map {
            let raw = first.source_map.as_ref().unwrap_or_else(|| {
                panic!(
                    "{}: the manifest requested a source map and the assembler returned code \
                     without one",
                    cell.golden_name
                )
            });
            // Decoding through the validating reader is the wire check: it
            // rejects anything that is not a flat v3 map, and rejects any
            // coordinate outside the code it describes.
            let artifact = compared_artifact("production", raw, &first.code);
            assert!(
                !artifact.sources.is_empty(),
                "{}: the emitted map declares no sources",
                cell.golden_name
            );
            if artifact.segments.is_empty() {
                mapless_enabled.push(cell.golden_name.clone());
            }
            segment_counts.insert(cell.golden_name.clone(), artifact.segments.len());
        } else {
            assert!(
                first.source_map.is_none(),
                "{}: no source map was requested, yet the assembler returned one",
                cell.golden_name
            );
        }
    }

    // An artifact with zero segments is well-formed but describes nothing, so a
    // matrix of them would compare and validate cleanly while proving nothing
    // about composition.
    assert!(
        mapless_enabled.is_empty(),
        "these map-enabled cells composed an artifact with no segments at all, so their \
         comparison and validation are vacuous: {mapless_enabled:?}"
    );
    println!("segments per map-enabled cell: {segment_counts:#?}");
}

// Required exits 2 and 5 — equality with the independent reference

/// Every cell, through both implementations, compared exactly.
///
/// [`assert_real_compile_equality`] runs the genuine production triple through
/// the shipped assembler and the §3.3 DTO projected out of that same triple
/// through the independent JavaScript reference, then compares the whole
/// outcome: the assembled CODE byte for byte, and the decoded map artifact
/// field for field and position for position including the ordered segment
/// sequence. That is simultaneously the artifact-equality exit and the
/// code-bytes-do-not-move exit at the BF2-fixture level — the reference derives
/// the code from the inputs and the frozen write grammar alone, so agreement
/// means production's bytes are the specified bytes rather than merely stable
/// ones.
#[test]
fn every_seed_matrix_cell_composes_identically_to_the_independent_reference() {
    let cells = read_seed_matrix();
    let cases: Vec<RealCompile> = cells.iter().map(compile_cell).collect();
    let agreed = assert_real_compile_equality(&cases);

    for (cell, outcome) in cells.iter().zip(&agreed) {
        match outcome {
            ComposeOutcome::Composed { map, code } => {
                assert!(
                    !code.is_empty(),
                    "{}: both sides agreed on an empty module",
                    cell.golden_name
                );
                assert_eq!(
                    map.is_some(),
                    cell.source_map,
                    "{}: both implementations agree, but on the wrong map presence for this \
                     cell's request axis",
                    cell.golden_name
                );
            }
            other => panic!(
                "{}: both implementations agree, but on a fail-closed outcome ({other:?}). \
                 Every seed cell must compose.",
                cell.golden_name
            ),
        }
    }
}

// Required exit 5 — assembled code bytes

/// Turning source maps on changes no assembled byte.
///
/// The map-disabled arm of each `(fixture, backend, prod)` triple runs the
/// assembler with composition switched off entirely — no input validation, no
/// chaining, no placement bookkeeping — so it is the pre-composition behaviour
/// of the byte-producing writes. Requiring its map-enabled twin to be
/// byte-identical is the direct statement that composition perturbs nothing.
#[test]
fn enabling_source_maps_perturbs_no_assembled_code_byte() {
    let cells = read_seed_matrix();
    let mut arms: BTreeMap<(String, Backend, bool), BTreeMap<bool, String>> = BTreeMap::new();
    for cell in &cells {
        let code = assemble(&compile_cell(cell)).code;
        let previous = arms
            .entry((cell.fixture.clone(), cell.backend, cell.is_production))
            .or_default()
            .insert(cell.source_map, code);
        assert!(previous.is_none(), "{}: duplicate arm", cell.golden_name);
    }

    assert_eq!(arms.len(), 18, "expected 18 map-on/map-off pairs");
    for (key, pair) in &arms {
        let enabled = pair
            .get(&true)
            .unwrap_or_else(|| panic!("{key:?}: no map-enabled arm"));
        let disabled = pair
            .get(&false)
            .unwrap_or_else(|| panic!("{key:?}: no map-disabled arm"));
        assert_eq!(
            enabled, disabled,
            "{key:?}: enabling source maps changed the assembled code bytes"
        );
    }
}

// Required exit 4 — BF2's authored-source oracle, once per cell

/// A candidate file that removes itself, at a path unique to this process and
/// this call.
pub(super) struct TempCandidate {
    pub(super) path: PathBuf,
}

impl TempCandidate {
    pub(super) fn write(cell: &str, body: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let slug: String = cell
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect();
        let path = std::env::temp_dir().join(format!(
            "verter-bf2-candidate-{}-{}-{slug}.json",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::write(&path, body)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
        Self { path }
    }
}

impl Drop for TempCandidate {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One finished subprocess run, or the fact that it had to be killed.
pub(super) struct Finished {
    pub(super) code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

/// Run a command to completion under an explicit deadline.
///
/// The pipes are drained on their own threads, so a child that fills a pipe
/// buffer cannot deadlock the deadline loop that is supposed to kill it.
pub(super) fn run_bounded(command: &mut Command, timeout: Duration) -> Finished {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "cannot run the conformance harness oracle: `node` failed to start ({error}).\n\
                 This suite's whole purpose is proving that oracle RAN over every cell; without \
                 Node nothing ran.\n\
                 Install Node (the workspace already requires it for `scripts/gate.mjs`) and \
                 re-run."
            )
        });

    let mut out_pipe = child.stdout.take().expect("stdout was piped");
    let mut err_pipe = child.stderr.take().expect("stderr was piped");
    let out_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = out_pipe.read_to_end(&mut buffer);
        buffer
    });
    let err_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = err_pipe.read_to_end(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait().expect("the child can be polled") {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };

    Finished {
        code: status.and_then(|status| status.code()),
        timed_out,
        stdout: String::from_utf8_lossy(&out_reader.join().expect("the stdout reader joins"))
            .into_owned(),
        stderr: String::from_utf8_lossy(&err_reader.join().expect("the stderr reader joins"))
            .into_owned(),
    }
}

/// The oracle rules that describe the artifact's OWN well-formedness rather
/// than where its coordinates point.
///
/// These are composition's to own, so they are gated. Every other rule the
/// oracle can raise is about whether a coordinate tells the truth about the
/// authored fixture — a fragment-emitter property this composition carries
/// forward opaquely — and is recorded, not gated.
const WIRE_RULES: &[&str] = &["map-presence", "map-version", "mappings-decode"];

/// The genuine production result of every cell, through the harness's accepted
/// entry point, unchanged, in authoritative mode.
///
/// GATED: that the oracle RAN (a real report, with a real mapping-axis result
/// carrying real statistics) for all 36 cells, and that no WIRE-level rule was
/// violated. NOT GATED: the mapping verdict itself, or the aggregate verdict.
/// Residual authored-truthfulness violations belong to the fragment emitters,
/// not to assembly; what the oracle said about each cell is printed and written
/// out so that work has its input.
#[test]
fn bf2_authored_source_oracle_runs_over_every_seed_matrix_cell() {
    let harness = harness_root();
    let entry = harness.join("bin/check-candidate.mjs");
    assert!(
        entry.exists(),
        "the harness's accepted entry point is missing at {}",
        entry.display()
    );

    let cells = read_seed_matrix();
    let mut records = serde_json::Map::new();
    let mut not_run = Vec::new();
    let mut wire_violations = Vec::new();
    let mut summary = Vec::new();

    for cell in &cells {
        let case = compile_cell(cell);
        let assembled = assemble(&case);

        // Diagnostics travel as the compiler produced them. All 36 goldens
        // record none, and Verter emits none for these fixtures, so the shape
        // question (how a Verter severity would map onto the official
        // parse/script/template phase vocabulary) does not arise — and is
        // asserted not to arise rather than answered by invention.
        assert!(
            case.compiled.diagnostics.is_empty(),
            "{}: the compile emitted diagnostics ({:?}); they have no defined projection onto \
             the golden's phase-kind vocabulary, so this suite can no longer hand the oracle a \
             faithful candidate",
            cell.golden_name,
            case.compiled.diagnostics
        );

        let map: Value = match &assembled.source_map {
            Some(raw) => serde_json::from_str(raw).unwrap_or_else(|error| {
                panic!("{}: the emitted map is not JSON: {error}", cell.golden_name)
            }),
            None => Value::Null,
        };
        let candidate = TempCandidate::write(
            &cell.golden_name,
            &json!({ "code": assembled.code, "map": map, "diagnostics": [] }).to_string(),
        );

        let mut command = Command::new("node");
        command
            .arg(&entry)
            .arg("--golden")
            .arg(&cell.golden_name)
            .arg("--candidate")
            .arg(&candidate.path)
            .arg("--authoritative")
            .current_dir(&harness);
        let finished = run_bounded(&mut command, ORACLE_TIMEOUT);

        assert!(
            !finished.timed_out,
            "{}: the oracle did not finish within {ORACLE_TIMEOUT:?} — it was killed, so it did \
             not run.\nstderr:\n{}",
            cell.golden_name, finished.stderr
        );
        // 0 = pass, 1 = a comparison reported differences, 2 = authoritative
        // mode saw a skipped axis. Anything else is the CLI failing rather than
        // reporting, which is not a run.
        assert!(
            matches!(finished.code, Some(0..=2)),
            "{}: the oracle exited with {:?} instead of reporting.\nstdout:\n{}\nstderr:\n{}",
            cell.golden_name,
            finished.code,
            finished.stdout,
            finished.stderr
        );

        let report: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
            panic!(
                "{}: the oracle emitted no JSON report ({error}), so nothing proves it ran.\n\
                 stdout:\n{}\nstderr:\n{}",
                cell.golden_name, finished.stdout, finished.stderr
            )
        });
        assert_eq!(
            report.get("goldenName").and_then(Value::as_str),
            Some(cell.golden_name.as_str()),
            "{}: the oracle reported on a different golden",
            cell.golden_name
        );

        let mapping_status = report
            .get("axes")
            .and_then(|axes| axes.get("mapping"))
            .and_then(|mapping| mapping.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("<absent>")
            .to_string();
        // A status label alone would be satisfied by a stub. The mapping axis
        // ALSO surfaces the validator's own result, whose statistics count the
        // segments it actually walked — so requiring it present and populated
        // is what separates "the oracle ran" from "the oracle was labelled".
        let stats = report
            .get("report")
            .and_then(|report| report.get("mapping"))
            .and_then(|mapping| mapping.get("stats"))
            .cloned();
        let walked = stats
            .as_ref()
            .and_then(|stats| {
                let bearing = stats.get("sourceBearingSegments")?.as_u64()?;
                let sourceless = stats.get("sourcelessSegments")?.as_u64()?;
                Some(bearing + sourceless)
            })
            .unwrap_or(0);
        if mapping_status != "ran" || stats.is_none() || (cell.source_map && walked == 0) {
            not_run.push(format!(
                "{} (status {mapping_status}, stats {})",
                cell.golden_name,
                stats
                    .as_ref()
                    .map_or_else(|| "absent".to_string(), ToString::to_string)
            ));
        }

        let violations: Vec<(String, String)> = report
            .get("report")
            .and_then(|report| report.get("mapping"))
            .and_then(|mapping| mapping.get("violations"))
            .and_then(Value::as_array)
            .map(|violations| {
                violations
                    .iter()
                    .map(|violation| {
                        (
                            violation
                                .get("rule")
                                .and_then(Value::as_str)
                                .unwrap_or("<unnamed>")
                                .to_string(),
                            violation
                                .get("detail")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (rule, detail) in &violations {
            if WIRE_RULES.contains(&rule.as_str()) {
                wire_violations.push(format!("{}: {rule} — {detail}", cell.golden_name));
            }
        }

        let mapping_ok = report
            .get("report")
            .and_then(|report| report.get("mapping"))
            .and_then(|mapping| mapping.get("ok"))
            .and_then(Value::as_bool);
        let rule_names: BTreeSet<&str> = violations.iter().map(|(rule, _)| rule.as_str()).collect();
        summary.push(format!(
            "{:<48} exit={:<3} mapping={mapping_status} ok={:<7} segments={walked:<4} rules={:?}",
            cell.golden_name,
            finished.code.unwrap_or(-1),
            mapping_ok.map_or("absent".to_string(), |ok| ok.to_string()),
            rule_names,
        ));

        records.insert(
            cell.golden_name.clone(),
            json!({
                "exitCode": finished.code,
                "sourceMapRequested": cell.source_map,
                "verdict": report.get("verdict"),
                "axes": report.get("axes"),
                "reasons": report.get("reasons"),
                "mapping": report.get("report").and_then(|report| report.get("mapping")),
            }),
        );
    }

    let evidence = std::env::temp_dir().join(format!(
        "verter-bf2-seed-matrix-report-{}.json",
        std::process::id()
    ));
    std::fs::write(
        &evidence,
        serde_json::to_string_pretty(&Value::Object(records)).expect("the record serializes"),
    )
    .expect("the evidence file is writable");

    println!(
        "BF2 authored-source oracle, {} cells:\n{}\nfull reports: {}",
        cells.len(),
        summary.join("\n"),
        evidence.display()
    );

    assert!(
        not_run.is_empty(),
        "the oracle's mapping axis did not genuinely run for {} of {} cells: {not_run:#?}",
        not_run.len(),
        cells.len()
    );
    assert!(
        wire_violations.is_empty(),
        "the oracle reported WIRE-level violations, which describe the emitted artifact's own \
         well-formedness and are therefore composition's own defects:\n{}",
        wire_violations.join("\n")
    );
}
