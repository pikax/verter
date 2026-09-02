//! Provenance-stamped wall-clock + allocation records for the CSS style
//! pipeline, and the exact-set / ceiling comparator over them.
//!
//! A record is only comparable when its full measurement-protocol and
//! environment identity match: two records measured under different machines,
//! sampling protocols, toolchains, targets, profiles, or feature sets are not
//! evidence about each other, and the comparator refuses them instead of
//! producing a number. Each record also carries a self-integrity digest so a
//! hand-edited provenance field is detected rather than trusted.
//!
//! The benchmark-identity universe is never written down here: it is derived
//! from [`crate::css_identities`], the same module the criterion bench
//! registers its identities from, and a comparison runs only after the
//! compiled-in universe, the baseline record, and the candidate record are
//! proven to be the same set.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::css_identities::{allocation_category_universe, universe, CssMeasuredOp, SCOPE_ID};

/// Record schema version. Bump on any breaking field change.
pub const RECORD_SCHEMA_VERSION: u32 = 1;

/// Live capture pipeline discriminant. The committed pre-convergence
/// baseline still records [`PIPELINE_LEGACY_LIGHTNINGCSS`]; a capture of
/// the live `style_planner` pipeline must not reuse that name.
pub const PIPELINE_STYLE_PLANNER: &str = "style-planner";
/// Frozen pre-deletion baseline discriminant. Capture must not reuse it.
pub const PIPELINE_LEGACY_LIGHTNINGCSS: &str = "legacy-lightningcss";
/// Default `--pipeline` for `css_latency_gate capture` / `gate`.
pub const CAPTURE_PIPELINE_DEFAULT: &str = PIPELINE_STYLE_PLANNER;

/// Refuse a capture discriminant that names the deleted lightningcss pipeline
/// (or is empty). Compare against the committed legacy baseline must declare
/// `--expect-transition legacy-lightningcss:style-planner`.
pub fn validate_capture_pipeline(pipeline: &str) -> Result<(), String> {
    if pipeline.is_empty() {
        return Err("pipeline discriminant must be non-empty".to_string());
    }
    if pipeline.contains("lightningcss") {
        return Err(format!(
            "lightningcss is not a live capture pipeline (got {pipeline:?}); capture as {CAPTURE_PIPELINE_DEFAULT:?} and compare with --expect-transition {PIPELINE_LEGACY_LIGHTNINGCSS}:{CAPTURE_PIPELINE_DEFAULT}"
        ));
    }
    Ok(())
}

/// Wall-clock ceiling: candidate median must be <= 1.2x baseline median.
pub const WALL_RATIO_CEILING_NUM: u128 = 12;
pub const WALL_RATIO_CEILING_DEN: u128 = 10;

/// The exact sampling protocol this crate's capture implements. A record
/// carries this string; the comparator refuses records whose sampling mode
/// differs from each other.
pub const SAMPLING_MODE: &str = "wall: warmup >=30 iters and >=100ms, then calibrate \
     iters-per-sample so one sample takes >=2ms, then 30 samples (each sample = mean ns \
     over its iters); statistic = median/min/max over the 30 per-sample means. \
     alloc per identity: 1 warm call, then min over 3 single-call (count,bytes) deltas. \
     alloc per category: css::process_style with scoped=true is_module=false, \
     1 warm call, then min over 3 single-call deltas (canary protocol). \
     identities measured sequentially in one process; no concurrent measured runs.";

// =============================================================================
// Record schema
// =============================================================================

/// Machine-class identity: enough to refuse cross-machine comparisons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineClass {
    pub cpu_model: String,
    pub logical_cpus: u32,
    pub physical_ram_bytes: u64,
    pub os: String,
}

/// Full measurement-protocol and environment identity of one record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub schema_version: u32,
    /// Which style pipeline the measured entry points were compiled against.
    pub pipeline: String,
    pub commit_sha: String,
    /// `git rev-parse HEAD^{tree}` of the measured checkout.
    pub tree_object_id: String,
    /// git blob digest of `crates/verter_bench/benches/css_bench.rs`.
    pub css_bench_blob: String,
    /// git blob digest of `crates/verter_bench/src/css_identities.rs`.
    pub css_identities_blob: String,
    pub machine_class: MachineClass,
    /// `rustc --version` of the building toolchain.
    pub toolchain: String,
    /// Host triple (non-cross build).
    pub target_triple: String,
    /// `release` or `debug` (from `debug_assertions` at compile time).
    pub cargo_profile: String,
    pub enabled_features: Vec<String>,
    pub sampling_mode: String,
    /// The load/thermal policy that was actually followed, stated honestly:
    /// what was observed and what the OS does not expose.
    pub load_thermal_policy: String,
    pub load_avg_before: String,
    pub load_avg_after: String,
    pub captured_at_utc: String,
    /// True when the compiling git tree was dirty. Default false so a
    /// pre-field record round-trips without changing its integrity digest.
    #[serde(default, skip_serializing_if = "is_false")]
    pub compiled_dirty: bool,
}

/// Wall-clock + allocation measurement of one benchmark identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityMeasurement {
    pub identity: String,
    pub category: String,
    pub input_len_bytes: u64,
    pub samples: u32,
    pub iters_per_sample: u64,
    pub wall_ns_median: u64,
    pub wall_ns_min: u64,
    pub wall_ns_max: u64,
    pub alloc_count: u64,
    pub alloc_bytes: u64,
    /// SHA-256 of the measured input bytes. Empty on pre-field records.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input_sha256: String,
    /// Fingerprint of the measured operation (inputs/options). Empty on
    /// pre-field records.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub operation: String,
}

/// Allocation count for one generator category under the canary protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryAllocation {
    pub category: String,
    pub input_len_bytes: u64,
    pub alloc_count: u64,
    pub alloc_bytes: u64,
}

/// One complete provenance-stamped measurement record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CssBaselineRecord {
    pub provenance: Provenance,
    pub identities: Vec<IdentityMeasurement>,
    pub allocation_by_category: Vec<CategoryAllocation>,
    /// SHA-256 (hex) of this record's canonical JSON with `integrity` empty.
    pub integrity: String,
}

/// Compute the self-integrity digest of `record` (its canonical JSON with the
/// `integrity` field emptied).
fn is_false(value: &bool) -> bool {
    !*value
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn integrity_digest(record: &CssBaselineRecord) -> String {
    let mut unsealed = record.clone();
    unsealed.integrity = String::new();
    let json = serde_json::to_string(&unsealed).expect("record serializes");
    sha256_hex(json.as_bytes())
}

/// Seal `record` by writing its self-integrity digest.
pub fn seal(record: &mut CssBaselineRecord) {
    record.integrity = integrity_digest(record);
}

// =============================================================================
// Compare
// =============================================================================

/// Comparison policy.
#[derive(Debug, Clone, Default)]
pub struct ComparePolicy {
    /// By default the two records must have been measured against the same
    /// pipeline discriminant. A deliberate cross-pipeline gate (old pipeline
    /// baseline vs new pipeline candidate) must declare the exact transition
    /// `(baseline_pipeline, candidate_pipeline)` here; anything else refuses.
    pub allowed_pipeline_transition: Option<(String, String)>,
    /// When set, the candidate must have been captured by this compiled
    /// gate binary (commit/tree/source digests) and must not be dirty.
    pub required_candidate_binary: Option<CompiledBinaryIdentity>,
}

/// Compile-time identity of the measuring binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledBinaryIdentity {
    pub commit_sha: String,
    pub tree_object_id: String,
    pub dirty: bool,
    pub css_bench_digest: String,
    pub css_identities_digest: String,
}

/// Provenance baked into this binary at compile time, not live git metadata.
pub fn compiled_binary_identity() -> CompiledBinaryIdentity {
    CompiledBinaryIdentity {
        commit_sha: env!("VERTER_BENCH_COMMIT_SHA").to_string(),
        tree_object_id: env!("VERTER_BENCH_TREE_ID").to_string(),
        dirty: env!("VERTER_BENCH_DIRTY") == "1",
        css_bench_digest: sha256_hex(include_bytes!("../benches/css_bench.rs")),
        css_identities_digest: sha256_hex(include_bytes!("css_identities.rs")),
    }
}

/// One per-identity gate row of a successful comparison.
#[derive(Debug, Clone)]
pub struct IdentityRatio {
    pub identity: String,
    pub baseline_wall_ns_median: u64,
    pub candidate_wall_ns_median: u64,
    /// candidate / baseline.
    pub ratio: f64,
}

/// A successful comparison: every check passed, every identity under ceiling.
#[derive(Debug, Clone)]
pub struct CompareReport {
    pub per_identity: Vec<IdentityRatio>,
}

/// Compare a committed baseline against a fresh candidate.
///
/// Refuses (returning every reason found at the failed stage) unless:
/// 1. both records' self-integrity digests verify (a forged/hand-edited
///    provenance or metric field fails here);
/// 2. schema version, machine class, sampling mode, toolchain, target triple,
///    cargo profile, and enabled features are identical across the records;
/// 3. the pipeline discriminants satisfy the policy (identical by default, or
///    exactly the declared transition);
/// 4. the compiled-in identity universe, the baseline's identity set, and the
///    candidate's identity set are the same set (no missing, no extra, no
///    duplicates on either side);
/// 5. every identity's candidate wall-clock median is at most 1.2x its
///    baseline median — a failure names exactly the exceeding identities.
pub fn compare_records(
    universe_ids: &BTreeSet<String>,
    baseline: &CssBaselineRecord,
    candidate: &CssBaselineRecord,
    policy: &ComparePolicy,
) -> Result<CompareReport, Vec<String>> {
    // Stage 1: self-integrity. A record whose digest does not verify has been
    // hand-edited (or truncated) since capture; nothing else in it is trusted.
    let mut failures = Vec::new();
    for (label, record) in [("baseline", baseline), ("candidate", candidate)] {
        let expected = integrity_digest(record);
        if record.integrity != expected {
            failures.push(format!(
                "{label} record failed its self-integrity check: stored digest \
                 {stored:?} does not match the record contents",
                stored = record.integrity,
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    // Stage 2: measurement-protocol and environment identity. Records measured
    // under different conditions are not evidence about each other.
    let b = &baseline.provenance;
    let c = &candidate.provenance;
    if b.schema_version != c.schema_version {
        failures.push(format!(
            "schema version mismatch: baseline {} vs candidate {}",
            b.schema_version, c.schema_version
        ));
    }
    if b.machine_class != c.machine_class {
        failures.push(format!(
            "machine class mismatch: baseline {:?} vs candidate {:?}",
            b.machine_class, c.machine_class
        ));
    }
    if b.sampling_mode != c.sampling_mode {
        failures.push("sampling mode mismatch between baseline and candidate".to_string());
    }
    if b.toolchain != c.toolchain {
        failures.push(format!(
            "toolchain mismatch: baseline {:?} vs candidate {:?}",
            b.toolchain, c.toolchain
        ));
    }
    if b.target_triple != c.target_triple {
        failures.push(format!(
            "target triple mismatch: baseline {:?} vs candidate {:?}",
            b.target_triple, c.target_triple
        ));
    }
    if b.cargo_profile != c.cargo_profile {
        failures.push(format!(
            "cargo profile mismatch: baseline {:?} vs candidate {:?}",
            b.cargo_profile, c.cargo_profile
        ));
    }
    if b.enabled_features != c.enabled_features {
        failures.push(format!(
            "enabled features mismatch: baseline {:?} vs candidate {:?}",
            b.enabled_features, c.enabled_features
        ));
    }
    if b.load_thermal_policy != c.load_thermal_policy {
        failures.push(format!(
            "load/thermal policy mismatch: baseline {:?} vs candidate {:?}",
            b.load_thermal_policy, c.load_thermal_policy
        ));
    }
    if let Some(required) = &policy.required_candidate_binary {
        if c.commit_sha != required.commit_sha {
            failures.push(format!(
                "candidate compiled commit mismatch: record {:?} vs binary {:?}",
                c.commit_sha, required.commit_sha
            ));
        }
        if c.tree_object_id != required.tree_object_id {
            failures.push(format!(
                "candidate compiled tree mismatch: record {:?} vs binary {:?}",
                c.tree_object_id, required.tree_object_id
            ));
        }
        if c.css_bench_blob != required.css_bench_digest {
            failures.push(format!(
                "candidate css_bench digest mismatch: record {:?} vs compiled binary {:?}",
                c.css_bench_blob, required.css_bench_digest
            ));
        }
        if c.css_identities_blob != required.css_identities_digest {
            failures.push(format!(
                "candidate css_identities digest mismatch: record {:?} vs compiled binary {:?}",
                c.css_identities_blob, required.css_identities_digest
            ));
        }
        if c.compiled_dirty || required.dirty {
            failures.push(
                "candidate was captured from a dirty compiled tree; refuse mismatched/dirty builds"
                    .to_string(),
            );
        }
    }

    // Stage 3: pipeline discriminant policy.
    match &policy.allowed_pipeline_transition {
        None => {
            if b.pipeline != c.pipeline {
                failures.push(format!(
                    "pipeline discriminant mismatch: baseline {:?} vs candidate {:?} \
                     (a deliberate cross-pipeline gate must declare the transition)",
                    b.pipeline, c.pipeline
                ));
            }
        }
        Some((from, to)) => {
            if &b.pipeline != from || &c.pipeline != to {
                failures.push(format!(
                    "pipeline transition mismatch: declared {from:?} -> {to:?}, records \
                     are baseline {:?} -> candidate {:?}",
                    b.pipeline, c.pipeline
                ));
            }
        }
    }
    // Same-pipeline records must have been captured against the same
    // css_bench.rs / css_identities.rs blobs. A declared cross-pipeline
    // transition is allowed to rewrite those files (the identity set is
    // still checked below).
    if policy.allowed_pipeline_transition.is_none() {
        if b.css_bench_blob != c.css_bench_blob {
            failures.push(format!(
                "css_bench.rs blob mismatch: baseline {:?} vs candidate {:?}",
                b.css_bench_blob, c.css_bench_blob
            ));
        }
        if b.css_identities_blob != c.css_identities_blob {
            failures.push(format!(
                "css_identities.rs blob mismatch: baseline {:?} vs candidate {:?}",
                b.css_identities_blob, c.css_identities_blob
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    // Exact-set rule over three sets: the compiled-in universe, the baseline's
    // identities, and the candidate's identities.
    for (label, record) in [("baseline", baseline), ("candidate", candidate)] {
        let ids: BTreeSet<&str> = record
            .identities
            .iter()
            .map(|m| m.identity.as_str())
            .collect();
        if ids.len() != record.identities.len() {
            failures.push(format!("{label} record contains duplicate identities"));
        }
        for missing in universe_ids.iter().filter(|u| !ids.contains(u.as_str())) {
            failures.push(format!(
                "{label} record is missing identity {missing:?} from the universe"
            ));
        }
        for extra in ids.iter().filter(|id| !universe_ids.contains(**id)) {
            failures.push(format!(
                "{label} record has extra identity {extra:?} not in the universe"
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    // Immutable per-identity workload (inputs/ops) against the compiled-in
    // universe and across the two records. Identity names matching
    // is not enough — a transition that rewrites generators could keep names
    // while changing the bytes or the measured op.
    let universe_cases = universe();
    let universe_by_id: std::collections::BTreeMap<String, &crate::css_identities::CssBenchCase> =
        universe_cases
            .iter()
            .map(|case| (case.identity(), case))
            .collect();
    let baseline_by_id: std::collections::BTreeMap<&str, &IdentityMeasurement> = baseline
        .identities
        .iter()
        .map(|m| (m.identity.as_str(), m))
        .collect();
    for (label, record) in [("baseline", baseline), ("candidate", candidate)] {
        for row in &record.identities {
            let Some(case) = universe_by_id.get(&row.identity) else {
                continue;
            };
            if row.category != case.category {
                failures.push(format!(
                    "{label} identity {:?} category {:?} does not match compiled-in workload {:?}",
                    row.identity, row.category, case.category
                ));
            }
            if row.input_len_bytes != case.css.len() as u64 {
                failures.push(format!(
                    "{label} identity {:?} input_len {} does not match compiled-in workload {}",
                    row.identity,
                    row.input_len_bytes,
                    case.css.len()
                ));
            }
            let expected_hash = sha256_hex(case.css.as_bytes());
            if !row.input_sha256.is_empty() && row.input_sha256 != expected_hash {
                failures.push(format!(
                    "{label} identity {:?} input_sha256 does not match compiled-in workload",
                    row.identity
                ));
            }
            let expected_op = case.op.fingerprint();
            if !row.operation.is_empty() && row.operation != expected_op {
                failures.push(format!(
                    "{label} identity {:?} operation {:?} does not match compiled-in workload {:?}",
                    row.identity, row.operation, expected_op
                ));
            }
        }
    }
    for cand_row in &candidate.identities {
        let Some(base_row) = baseline_by_id.get(cand_row.identity.as_str()) else {
            continue;
        };
        if base_row.category != cand_row.category {
            failures.push(format!(
                "workload category drift for {:?}: baseline {:?} vs candidate {:?}",
                cand_row.identity, base_row.category, cand_row.category
            ));
        }
        if base_row.input_len_bytes != cand_row.input_len_bytes {
            failures.push(format!(
                "workload input_len drift for {:?}: baseline {} vs candidate {}",
                cand_row.identity, base_row.input_len_bytes, cand_row.input_len_bytes
            ));
        }
        if !base_row.input_sha256.is_empty()
            && !cand_row.input_sha256.is_empty()
            && base_row.input_sha256 != cand_row.input_sha256
        {
            failures.push(format!(
                "workload input_sha256 drift for {:?}",
                cand_row.identity
            ));
        }
        if !base_row.operation.is_empty()
            && !cand_row.operation.is_empty()
            && base_row.operation != cand_row.operation
        {
            failures.push(format!(
                "workload operation drift for {:?}: baseline {:?} vs candidate {:?}",
                cand_row.identity, base_row.operation, cand_row.operation
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    // Stage 5: per-identity wall-clock ceiling. Integer math, no rounding:
    // cand * 10 <= base * 12.
    let mut per_identity = Vec::new();
    for cand_row in &candidate.identities {
        let base_row = baseline_by_id[cand_row.identity.as_str()];
        let base = u128::from(base_row.wall_ns_median);
        let cand = u128::from(cand_row.wall_ns_median);
        if cand * WALL_RATIO_CEILING_DEN > base * WALL_RATIO_CEILING_NUM {
            failures.push(format!(
                "identity {:?} exceeds the 1.2x wall-clock ceiling: candidate {}ns vs \
                 baseline {}ns ({:.3}x)",
                cand_row.identity,
                cand_row.wall_ns_median,
                base_row.wall_ns_median,
                cand as f64 / base.max(1) as f64,
            ));
        }
        per_identity.push(IdentityRatio {
            identity: cand_row.identity.clone(),
            baseline_wall_ns_median: base_row.wall_ns_median,
            candidate_wall_ns_median: cand_row.wall_ns_median,
            ratio: cand as f64 / base.max(1) as f64,
        });
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    Ok(CompareReport { per_identity })
}

// =============================================================================
// Capture
// =============================================================================

/// Allocation-counting hooks supplied by the binary that owns the counting
/// `#[global_allocator]` (a global allocator is process-global, so it cannot
/// live in this library).
#[derive(Clone, Copy)]
pub struct AllocHooks {
    /// Zero the current thread's allocation counters.
    pub reset: fn(),
    /// Read the current thread's `(allocation_count, allocated_bytes)`.
    pub read: fn() -> (u64, u64),
}

fn measure_alloc(hooks: &AllocHooks, mut call: impl FnMut()) -> (u64, u64) {
    // Warm one-time lazy initialisation out of the measured window.
    call();
    let mut best: Option<(u64, u64)> = None;
    for _ in 0..3 {
        (hooks.reset)();
        call();
        let sample = (hooks.read)();
        best = Some(match best {
            Some(prev) if prev.0 <= sample.0 => prev,
            _ => sample,
        });
    }
    best.expect("three allocation samples were taken")
}

struct WallStats {
    iters_per_sample: u64,
    median: u64,
    min: u64,
    max: u64,
    samples: u32,
}

fn measure_wall(mut call: impl FnMut()) -> WallStats {
    const SAMPLES: usize = 30;
    // Warmup: at least 30 iterations and at least 100ms.
    let warmup_start = Instant::now();
    let mut warmup_iters: u64 = 0;
    while warmup_iters < 30 || warmup_start.elapsed() < Duration::from_millis(100) {
        call();
        warmup_iters += 1;
    }
    let per_iter_ns = (warmup_start.elapsed().as_nanos() / u128::from(warmup_iters.max(1))).max(1);
    // Calibrate so one sample runs for at least ~2ms.
    let iters_per_sample = ((2_000_000 / per_iter_ns).max(1)) as u64;

    let mut means = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..iters_per_sample {
            call();
        }
        means.push((start.elapsed().as_nanos() / u128::from(iters_per_sample)) as u64);
    }
    means.sort_unstable();
    let median = (means[SAMPLES / 2 - 1] + means[SAMPLES / 2]) / 2;
    WallStats {
        iters_per_sample,
        median,
        min: means[0],
        max: means[SAMPLES - 1],
        samples: SAMPLES as u32,
    }
}

fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR = <root>/crates/verter_bench at compile time; the
    // binary runs on the machine that built it.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/verter_bench has a workspace root two levels up")
        .to_path_buf()
}

fn run_cmd(program: &str, args: &[&str]) -> String {
    match std::process::Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => format!("<unavailable: {program}>"),
    }
}

fn load_avg() -> String {
    if cfg!(target_os = "macos") {
        run_cmd("sysctl", &["-n", "vm.loadavg"])
    } else {
        std::fs::read_to_string("/proc/loadavg")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "<unavailable>".to_string())
    }
}

fn machine_class() -> MachineClass {
    let cpu_model = if cfg!(target_os = "macos") {
        run_cmd("sysctl", &["-n", "machdep.cpu.brand_string"])
    } else {
        "<unavailable>".to_string()
    };
    let physical_ram_bytes = if cfg!(target_os = "macos") {
        run_cmd("sysctl", &["-n", "hw.memsize"])
            .parse()
            .unwrap_or(0)
    } else {
        0
    };
    MachineClass {
        cpu_model,
        logical_cpus: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(0),
        physical_ram_bytes,
        os: format!("{} {}", std::env::consts::OS, run_cmd("uname", &["-r"])),
    }
}

fn enabled_features() -> Vec<String> {
    let mut features = Vec::new();
    if cfg!(feature = "hotpath") {
        features.push("hotpath".to_string());
    }
    if cfg!(feature = "hotpath-alloc") {
        features.push("hotpath-alloc".to_string());
    }
    if cfg!(feature = "currency_probe") {
        features.push("currency_probe".to_string());
    }
    if cfg!(feature = "attribution") {
        features.push("attribution".to_string());
    }
    features
}

/// `true` when this build is an optimized (non-`debug_assertions`) build.
pub fn build_is_optimized() -> bool {
    !cfg!(debug_assertions)
}

/// Measure every identity in the universe plus every allocation category,
/// against the pipeline this binary was compiled with, and return the sealed
/// provenance-stamped record.
pub fn run_capture(pipeline: &str, hooks: &AllocHooks) -> CssBaselineRecord {
    let load_avg_before = load_avg();

    let mut identities = Vec::new();
    for case in universe() {
        eprintln!("measuring {}", case.identity());
        let wall = measure_wall(|| case.op.run(&case.css));
        let (alloc_count, alloc_bytes) = measure_alloc(hooks, || case.op.run(&case.css));
        identities.push(IdentityMeasurement {
            identity: case.identity(),
            category: case.category.to_string(),
            input_len_bytes: case.css.len() as u64,
            samples: wall.samples,
            iters_per_sample: wall.iters_per_sample,
            wall_ns_median: wall.median,
            wall_ns_min: wall.min,
            wall_ns_max: wall.max,
            alloc_count,
            alloc_bytes,
            input_sha256: sha256_hex(case.css.as_bytes()),
            operation: case.op.fingerprint(),
        });
    }

    // Per-category allocation counts under the canary protocol: the full
    // process_style pipeline with a fixed representative option set, so the
    // per-category numbers are comparable across pipelines rather than
    // confounded by per-benchmark option permutations.
    let category_op = CssMeasuredOp::ProcessStyle {
        scoped: true,
        is_module: false,
    };
    let mut allocation_by_category = Vec::new();
    for (category, css) in allocation_category_universe() {
        eprintln!("measuring category {category}");
        let (alloc_count, alloc_bytes) = measure_alloc(hooks, || category_op.run(&css));
        allocation_by_category.push(CategoryAllocation {
            category: category.to_string(),
            input_len_bytes: css.len() as u64,
            alloc_count,
            alloc_bytes,
        });
    }

    let load_avg_after = load_avg();

    let compiled = compiled_binary_identity();
    let provenance = Provenance {
        schema_version: RECORD_SCHEMA_VERSION,
        pipeline: pipeline.to_string(),
        commit_sha: compiled.commit_sha,
        tree_object_id: compiled.tree_object_id,
        css_bench_blob: compiled.css_bench_digest,
        css_identities_blob: compiled.css_identities_digest,
        machine_class: machine_class(),
        toolchain: run_cmd("rustc", &["--version"]),
        target_triple: run_cmd("rustc", &["-vV"])
            .lines()
            .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
            .unwrap_or_else(|| "<unavailable>".to_string()),
        cargo_profile: if build_is_optimized() {
            "release".to_string()
        } else {
            "debug".to_string()
        },
        enabled_features: enabled_features(),
        sampling_mode: SAMPLING_MODE.to_string(),
        load_thermal_policy: "no other measured run was scheduled by this runner while \
             capturing; system load average recorded before and after (see load_avg_before/\
             load_avg_after); per-core thermal state is not observable on macOS without \
             privileged powermetrics and is NOT recorded"
            .to_string(),
        load_avg_before,
        load_avg_after,
        captured_at_utc: run_cmd("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        compiled_dirty: compiled.dirty,
    };

    let mut record = CssBaselineRecord {
        provenance,
        identities,
        allocation_by_category,
        integrity: String::new(),
    };
    seal(&mut record);
    let _ = SCOPE_ID; // scope id is fixed by the identity module
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css_identities::{
        generate_class_rules, generate_deep_rules, generate_descendant_selectors,
        generate_global_rules, generate_mixed_vue, generate_pseudo_selectors,
        generate_repeated_classes, generate_selector_lists, generate_slotted_rules,
        generate_v_bind_dotted, generate_v_bind_rules, identity_universe, universe, GROUPS,
    };

    fn synthetic_provenance(pipeline: &str) -> Provenance {
        Provenance {
            schema_version: RECORD_SCHEMA_VERSION,
            pipeline: pipeline.to_string(),
            commit_sha: "c0ffee".to_string(),
            tree_object_id: "tree-a".to_string(),
            css_bench_blob: "blob-bench".to_string(),
            css_identities_blob: "blob-ids".to_string(),
            machine_class: MachineClass {
                cpu_model: "Synthetic CPU".to_string(),
                logical_cpus: 8,
                physical_ram_bytes: 24 * 1024 * 1024 * 1024,
                os: "testos 1.0".to_string(),
            },
            toolchain: "rustc 1.0.0-synthetic".to_string(),
            target_triple: "synth-triple".to_string(),
            cargo_profile: "release".to_string(),
            enabled_features: vec![],
            sampling_mode: SAMPLING_MODE.to_string(),
            load_thermal_policy: "synthetic".to_string(),
            load_avg_before: "{ 1.0 1.0 1.0 }".to_string(),
            load_avg_after: "{ 1.0 1.0 1.0 }".to_string(),
            captured_at_utc: "2026-08-25T00:00:00Z".to_string(),
            compiled_dirty: false,
        }
    }

    /// A sealed synthetic record over the real universe with every identity's
    /// wall median set to `wall_ns`.
    fn synthetic_record(pipeline: &str, wall_ns: u64) -> CssBaselineRecord {
        let identities = universe()
            .into_iter()
            .map(|case| IdentityMeasurement {
                identity: case.identity(),
                category: case.category.to_string(),
                input_len_bytes: case.css.len() as u64,
                samples: 30,
                iters_per_sample: 100,
                wall_ns_median: wall_ns,
                wall_ns_min: wall_ns.saturating_sub(1),
                wall_ns_max: wall_ns.saturating_add(1),
                alloc_count: 10,
                alloc_bytes: 1000,
                input_sha256: sha256_hex(case.css.as_bytes()),
                operation: case.op.fingerprint(),
            })
            .collect();
        let mut record = CssBaselineRecord {
            provenance: synthetic_provenance(pipeline),
            identities,
            allocation_by_category: vec![CategoryAllocation {
                category: "class_rules".to_string(),
                input_len_bytes: 100,
                alloc_count: 10,
                alloc_bytes: 1000,
            }],
            integrity: String::new(),
        };
        seal(&mut record);
        record
    }

    fn set_median(record: &mut CssBaselineRecord, identity: &str, wall_ns: u64) {
        let row = record
            .identities
            .iter_mut()
            .find(|m| m.identity == identity)
            .expect("identity present");
        row.wall_ns_median = wall_ns;
        seal(record);
    }

    // -------------------------------------------------------------------------
    // Universe shape
    // -------------------------------------------------------------------------

    #[test]
    fn universe_has_expected_groups_call_sites_and_instances() {
        let cases = universe();
        assert_eq!(cases.len(), 42, "parameterized benchmark instances");
        let groups: BTreeSet<&str> = cases.iter().map(|c| c.group).collect();
        assert_eq!(groups, GROUPS.iter().copied().collect::<BTreeSet<_>>());
        let call_sites: BTreeSet<(&str, &str)> =
            cases.iter().map(|c| (c.group, c.function_id)).collect();
        assert_eq!(
            call_sites.len(),
            19,
            "distinct group/function_id call sites"
        );
        // Identities are unique.
        assert_eq!(identity_universe().len(), 42);
    }

    #[test]
    fn allocation_category_universe_is_the_eleven_generators() {
        let categories: Vec<&str> = allocation_category_universe()
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            categories,
            vec![
                "class_rules",
                "descendant_selectors",
                "pseudo_selectors",
                "selector_lists",
                "v_bind_rules",
                "v_bind_dotted",
                "deep_rules",
                "slotted_rules",
                "mixed_vue",
                "global_rules",
                "repeated_classes",
            ]
        );
        for (name, css) in allocation_category_universe() {
            assert!(!css.is_empty(), "category {name} generates non-empty CSS");
        }
    }

    // -------------------------------------------------------------------------
    // compare_records
    // -------------------------------------------------------------------------

    #[test]
    fn compare_passes_when_sets_match_and_all_ratios_under_ceiling() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let candidate = synthetic_record("legacy-lightningcss", 1100); // 1.1x
        let report = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect("comparison passes");
        assert_eq!(report.per_identity.len(), 42);
        assert!(report.per_identity.iter().all(|r| r.ratio <= 1.2));
    }

    #[test]
    fn compare_fails_on_missing_identity_in_candidate() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        candidate.identities.pop();
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("missing identity must refuse");
        assert!(
            err.iter().any(|e| e.contains("missing")),
            "refusal names the missing identity: {err:?}"
        );
    }

    #[test]
    fn compare_fails_on_extra_identity_in_candidate() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        candidate.identities.push(IdentityMeasurement {
            identity: "process_style/fabricated/99".to_string(),
            category: "class_rules".to_string(),
            input_len_bytes: 1,
            samples: 30,
            iters_per_sample: 1,
            wall_ns_median: 1,
            wall_ns_min: 1,
            wall_ns_max: 1,
            alloc_count: 1,
            alloc_bytes: 1,
            input_sha256: String::new(),
            operation: String::new(),
        });
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("extra identity must refuse");
        assert!(
            err.iter()
                .any(|e| e.contains("extra") && e.contains("process_style/fabricated/99")),
            "refusal names the extra identity: {err:?}"
        );
    }

    #[test]
    fn compare_fails_when_exactly_one_identity_exceeds_ceiling_naming_only_it() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        // 1.21x > 1.2x for exactly one identity; every other stays at 1.0x.
        set_median(&mut candidate, "scoped/single_class", 1210);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("one identity over the ceiling must fail the gate");
        let ceiling_failures: Vec<&String> = err.iter().filter(|e| e.contains("ceiling")).collect();
        assert_eq!(
            ceiling_failures.len(),
            1,
            "exactly the one exceeding identity reddens: {err:?}"
        );
        assert!(
            ceiling_failures[0].contains("scoped/single_class"),
            "the exceeding identity is named: {err:?}"
        );
    }

    #[test]
    fn compare_passes_at_exactly_the_ceiling() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        set_median(&mut candidate, "scoped/single_class", 1200); // exactly 1.2x
        compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect("exactly 1.2x is within the ceiling");
    }

    #[test]
    fn compare_fails_on_forged_tree_object_id() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        // Forge the field WITHOUT resealing: the integrity digest must catch it.
        candidate.provenance.tree_object_id = "tree-forged".to_string();
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("forged tree object id must refuse");
        assert!(
            err.iter().any(|e| e.contains("integrity")),
            "refusal cites the integrity digest: {err:?}"
        );
    }

    #[test]
    fn compare_fails_on_forged_bench_blob_digest() {
        let mut baseline = synthetic_record("legacy-lightningcss", 1000);
        let candidate = synthetic_record("legacy-lightningcss", 1000);
        baseline.provenance.css_bench_blob = "blob-forged".to_string();
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("forged bench blob digest must refuse");
        assert!(
            err.iter().any(|e| e.contains("integrity")),
            "refusal cites the integrity digest: {err:?}"
        );
    }

    #[test]
    fn compare_fails_on_machine_class_mismatch() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        candidate.provenance.machine_class.cpu_model = "Different CPU".to_string();
        seal(&mut candidate); // validly sealed — the mismatch is cross-record
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("machine-class mismatch must refuse");
        assert!(
            err.iter().any(|e| e.contains("machine class")),
            "refusal names the machine class: {err:?}"
        );
    }

    #[test]
    fn compare_fails_on_sampling_mode_mismatch() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let mut candidate = synthetic_record("legacy-lightningcss", 1000);
        candidate.provenance.sampling_mode = "a different protocol".to_string();
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("sampling-mode mismatch must refuse");
        assert!(
            err.iter().any(|e| e.contains("sampling mode")),
            "refusal names the sampling mode: {err:?}"
        );
    }

    #[test]
    fn compare_fails_when_pipeline_discriminant_differs_without_declared_transition() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let candidate = synthetic_record("something-else", 1000);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("differing pipeline discriminants must refuse by default");
        assert!(
            err.iter().any(|e| e.contains("pipeline")),
            "refusal names the pipeline discriminant: {err:?}"
        );
    }

    #[test]
    fn compare_allows_exactly_the_declared_pipeline_transition() {
        let baseline = synthetic_record("legacy-lightningcss", 1000);
        let candidate = synthetic_record("converged", 1000);
        let policy = ComparePolicy {
            allowed_pipeline_transition: Some((
                "legacy-lightningcss".to_string(),
                "converged".to_string(),
            )),
            ..Default::default()
        };
        compare_records(&identity_universe(), &baseline, &candidate, &policy)
            .expect("declared transition passes");
        // The declared transition is exact: reversed direction still refuses.
        let err = compare_records(&identity_universe(), &candidate, &baseline, &policy)
            .expect_err("reversed transition must refuse");
        assert!(err.iter().any(|e| e.contains("pipeline")));
    }

    #[test]
    fn capture_pipeline_default_is_not_lightningcss() {
        assert_eq!(CAPTURE_PIPELINE_DEFAULT, PIPELINE_STYLE_PLANNER);
        assert!(!CAPTURE_PIPELINE_DEFAULT.contains("lightningcss"));
        validate_capture_pipeline(CAPTURE_PIPELINE_DEFAULT).expect("live default is admissible");
    }

    #[test]
    fn validate_capture_pipeline_refuses_lightningcss_and_empty() {
        let err = validate_capture_pipeline("legacy-lightningcss").expect_err("legacy name");
        assert!(err.contains("lightningcss"), "{err}");
        let err = validate_capture_pipeline("").expect_err("empty");
        assert!(err.contains("non-empty"), "{err}");
    }

    #[test]
    fn compare_fails_on_load_thermal_policy_mismatch() {
        let baseline = synthetic_record("style-planner", 1000);
        let mut candidate = synthetic_record("style-planner", 1000);
        candidate.provenance.load_thermal_policy = "a different policy".to_string();
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("load/thermal policy mismatch must refuse");
        assert!(
            err.iter().any(|e| e.contains("load/thermal")),
            "refusal names the load/thermal policy: {err:?}"
        );
    }

    #[test]
    fn compare_fails_on_css_bench_blob_mismatch_same_pipeline() {
        let baseline = synthetic_record("style-planner", 1000);
        let mut candidate = synthetic_record("style-planner", 1000);
        candidate.provenance.css_bench_blob = "different-blob".to_string();
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("css_bench blob mismatch must refuse");
        assert!(
            err.iter().any(|e| e.contains("css_bench.rs blob")),
            "refusal names the css_bench blob: {err:?}"
        );
    }

    #[test]
    fn compare_allows_css_bench_blob_mismatch_on_declared_transition() {
        let mut baseline = synthetic_record(PIPELINE_LEGACY_LIGHTNINGCSS, 1000);
        let mut candidate = synthetic_record(PIPELINE_STYLE_PLANNER, 1000);
        baseline.provenance.css_bench_blob = "legacy-blob".to_string();
        candidate.provenance.css_bench_blob = "new-blob".to_string();
        seal(&mut baseline);
        seal(&mut candidate);
        let policy = ComparePolicy {
            allowed_pipeline_transition: Some((
                PIPELINE_LEGACY_LIGHTNINGCSS.to_string(),
                PIPELINE_STYLE_PLANNER.to_string(),
            )),
            ..Default::default()
        };
        compare_records(&identity_universe(), &baseline, &candidate, &policy)
            .expect("declared transition may rewrite the identities blob");
    }

    fn matching_required(record: &CssBaselineRecord) -> CompiledBinaryIdentity {
        CompiledBinaryIdentity {
            commit_sha: record.provenance.commit_sha.clone(),
            tree_object_id: record.provenance.tree_object_id.clone(),
            dirty: false,
            css_bench_digest: record.provenance.css_bench_blob.clone(),
            css_identities_digest: record.provenance.css_identities_blob.clone(),
        }
    }

    #[test]
    fn compiled_binary_identity_source_digests_are_sha256() {
        let identity = compiled_binary_identity();
        assert_eq!(identity.css_bench_digest.len(), 64, "{identity:?}");
        assert_eq!(identity.css_identities_digest.len(), 64, "{identity:?}");
        assert!(
            identity
                .css_bench_digest
                .chars()
                .all(|c| c.is_ascii_hexdigit()),
            "{identity:?}"
        );
        assert_ne!(identity.css_bench_digest, identity.css_identities_digest);
    }

    #[test]
    fn compare_refuses_candidate_compiled_commit_mismatch() {
        let baseline = synthetic_record("style-planner", 1000);
        let candidate = synthetic_record("style-planner", 1000);
        let mut required = matching_required(&candidate);
        required.commit_sha = "deadbeef".to_string();
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy {
                required_candidate_binary: Some(required),
                ..Default::default()
            },
        )
        .expect_err("mismatched compiled commit must refuse");
        assert!(err.iter().any(|e| e.contains("compiled commit")), "{err:?}");
    }

    #[test]
    fn compare_accepts_candidate_matching_required_binary() {
        let baseline = synthetic_record("style-planner", 1000);
        let candidate = synthetic_record("style-planner", 1000);
        compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy {
                required_candidate_binary: Some(matching_required(&candidate)),
                ..Default::default()
            },
        )
        .expect("matching compiled identity must pass");
    }

    #[test]
    fn compare_refuses_workload_category_drift_across_declared_transition() {
        let baseline = synthetic_record(PIPELINE_LEGACY_LIGHTNINGCSS, 1000);
        let mut candidate = synthetic_record(PIPELINE_STYLE_PLANNER, 1000);
        candidate.identities[0].category = "not-the-universe-category".to_string();
        candidate.identities[0].input_sha256.clear();
        candidate.identities[0].operation.clear();
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy {
                allowed_pipeline_transition: Some((
                    PIPELINE_LEGACY_LIGHTNINGCSS.to_string(),
                    PIPELINE_STYLE_PLANNER.to_string(),
                )),
                ..Default::default()
            },
        )
        .expect_err("workload category drift must refuse even across a declared transition");
        assert!(
            err.iter()
                .any(|e| e.contains("category") && e.contains("not-the-universe-category")),
            "{err:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Generator-mirror byte identity (Copy B = css_identities)
    // -------------------------------------------------------------------------

    const MIRROR_TABLE_JSON: &str =
        include_str!("../../../test-corpora/style-ir/generator-mirror-digests.json");

    fn copy_b_digest_table() -> std::collections::BTreeMap<String, String> {
        let mut table = std::collections::BTreeMap::new();
        const SIZES: [usize; 7] = [1, 5, 8, 20, 40, 50, 100];
        const AXES: [usize; 6] = [1, 5, 10, 20, 50, 100];
        type OneArgGenerator = fn(usize) -> String;
        let ones: [(&str, OneArgGenerator); 10] = [
            ("generate_class_rules", generate_class_rules),
            (
                "generate_descendant_selectors",
                generate_descendant_selectors,
            ),
            ("generate_pseudo_selectors", generate_pseudo_selectors),
            ("generate_selector_lists", generate_selector_lists),
            ("generate_v_bind_rules", generate_v_bind_rules),
            ("generate_v_bind_dotted", generate_v_bind_dotted),
            ("generate_deep_rules", generate_deep_rules),
            ("generate_slotted_rules", generate_slotted_rules),
            ("generate_mixed_vue", generate_mixed_vue),
            ("generate_global_rules", generate_global_rules),
        ];
        for (name, gen) in ones {
            for n in SIZES {
                table.insert(format!("{name}:{n}"), sha256_hex(gen(n).as_bytes()));
            }
        }
        for unique in AXES {
            for repeats in AXES {
                table.insert(
                    format!("generate_repeated_classes:{unique}x{repeats}"),
                    sha256_hex(generate_repeated_classes(unique, repeats).as_bytes()),
                );
            }
        }
        table
    }

    #[test]
    fn css_identities_generators_match_pinned_mirror_digests() {
        let pinned: serde_json::Value =
            serde_json::from_str(MIRROR_TABLE_JSON).expect("mirror table parses");
        let expected = pinned["digests"]
            .as_object()
            .expect("digests object")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("hex digest").to_string()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let actual = copy_b_digest_table();
        let expected_keys: BTreeSet<&str> = expected.keys().map(String::as_str).collect();
        let actual_keys: BTreeSet<&str> = actual.keys().map(String::as_str).collect();
        assert_eq!(
            actual_keys, expected_keys,
            "Copy B generator digest keys must be exactly the pinned set"
        );
        for (key, digest) in &expected {
            assert_eq!(
                actual.get(key).map(String::as_str),
                Some(digest.as_str()),
                "Copy B digest mismatch at {key}"
            );
        }
    }

    #[test]
    fn generator_mirror_control_class_rules_differs_from_deep_rules() {
        let left = generate_class_rules(1);
        let right = generate_deep_rules(1);
        assert_ne!(left.as_bytes(), right.as_bytes());
        let offset = left
            .as_bytes()
            .iter()
            .zip(right.as_bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(left.len().min(right.len()));
        assert_eq!(offset, 0, "control must differ at byte 0, got {offset}");
        assert!(left.starts_with(".class-0 { color: red; padding: 0px; }"));
        assert!(right.starts_with(":deep(.inner-0) { color: red; }"));
    }

    const COMMITTED_BASELINE_JSON: &str =
        include_str!("../../../test-corpora/style-ir/css-baseline-legacy.json");

    fn committed_baseline() -> CssBaselineRecord {
        serde_json::from_str(COMMITTED_BASELINE_JSON).expect("committed baseline parses")
    }

    #[test]
    fn committed_baseline_integrity_and_universe_match() {
        let record = committed_baseline();
        assert_eq!(
            integrity_digest(&record),
            record.integrity,
            "committed baseline must verify its self-integrity digest"
        );
        assert_eq!(record.provenance.pipeline, "legacy-lightningcss");
        assert_eq!(record.provenance.cargo_profile, "release");
        assert_eq!(record.provenance.sampling_mode, SAMPLING_MODE);
        let ids: BTreeSet<String> = record
            .identities
            .iter()
            .map(|m| m.identity.clone())
            .collect();
        assert_eq!(
            ids,
            identity_universe(),
            "committed baseline identity set must equal the compiled-in universe"
        );
        assert_eq!(record.identities.len(), 42);
        assert_eq!(record.allocation_by_category.len(), 11);
    }

    #[test]
    fn committed_baseline_allocation_counts_match_a29_retained_table() {
        const RETAINED: &[(&str, u64)] = &[
            ("class_rules", 422),
            ("descendant_selectors", 371),
            ("pseudo_selectors", 371),
            ("selector_lists", 822),
            ("v_bind_rules", 929),
            ("v_bind_dotted", 929),
            ("deep_rules", 522),
            ("slotted_rules", 472),
            ("mixed_vue", 648),
            ("global_rules", 370),
            ("repeated_classes", 371),
        ];
        let record = committed_baseline();
        let actual: std::collections::BTreeMap<&str, u64> = record
            .allocation_by_category
            .iter()
            .map(|row| (row.category.as_str(), row.alloc_count))
            .collect();
        let expected: std::collections::BTreeMap<&str, u64> = RETAINED.iter().copied().collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn committed_baseline_compared_to_itself_is_under_ceiling() {
        let record = committed_baseline();
        let report = compare_records(
            &identity_universe(),
            &record,
            &record,
            &ComparePolicy::default(),
        )
        .expect("a record compared to itself is 1.0x");
        assert_eq!(report.per_identity.len(), 42);
        assert!(report.per_identity.iter().all(|r| r.ratio == 1.0));
    }

    #[test]
    fn committed_baseline_refuses_missing_identity() {
        let baseline = committed_baseline();
        let mut candidate = baseline.clone();
        let dropped = candidate
            .identities
            .pop()
            .expect("baseline has identities")
            .identity;
        seal(&mut candidate);
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("missing identity must refuse");
        assert!(
            err.iter()
                .any(|e| e.contains("missing") && e.contains(&dropped)),
            "refusal names the dropped identity {dropped}: {err:?}"
        );
    }

    #[test]
    fn committed_baseline_refuses_one_identity_over_ceiling() {
        let baseline = committed_baseline();
        let mut candidate = baseline.clone();
        set_median(&mut candidate, "scoped/single_class", {
            let base = baseline
                .identities
                .iter()
                .find(|m| m.identity == "scoped/single_class")
                .expect("identity present")
                .wall_ns_median;
            (base * 12) / 10 + 1
        });
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("one identity over the ceiling must fail");
        let ceiling: Vec<&String> = err.iter().filter(|e| e.contains("ceiling")).collect();
        assert_eq!(ceiling.len(), 1, "exactly one identity reddens: {err:?}");
        assert!(
            ceiling[0].contains("scoped/single_class"),
            "the exceeding identity is named: {err:?}"
        );
    }

    #[test]
    fn committed_baseline_refuses_forged_integrity() {
        let baseline = committed_baseline();
        let mut candidate = baseline.clone();
        candidate.provenance.tree_object_id = "tree-forged".to_string();
        let err = compare_records(
            &identity_universe(),
            &baseline,
            &candidate,
            &ComparePolicy::default(),
        )
        .expect_err("forged tree object id must refuse");
        assert!(
            err.iter().any(|e| e.contains("integrity")),
            "refusal cites the integrity digest: {err:?}"
        );
    }
}
