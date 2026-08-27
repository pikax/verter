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
pub fn integrity_digest(record: &CssBaselineRecord) -> String {
    let mut unsealed = record.clone();
    unsealed.integrity = String::new();
    let json = serde_json::to_string(&unsealed).expect("record serializes");
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
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
    if !failures.is_empty() {
        return Err(failures);
    }

    // Stage 4: exact-set rule over three sets — the compiled-in universe, the
    // baseline's identities, the candidate's identities.
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

    // Stage 5: per-identity wall-clock ceiling. Integer math, no rounding:
    // cand * 10 <= base * 12.
    let baseline_by_id: std::collections::BTreeMap<&str, &IdentityMeasurement> = baseline
        .identities
        .iter()
        .map(|m| (m.identity.as_str(), m))
        .collect();
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

    let provenance = Provenance {
        schema_version: RECORD_SCHEMA_VERSION,
        pipeline: pipeline.to_string(),
        commit_sha: run_cmd("git", &["rev-parse", "HEAD"]),
        tree_object_id: run_cmd("git", &["rev-parse", "HEAD^{tree}"]),
        css_bench_blob: run_cmd(
            "git",
            &["hash-object", "crates/verter_bench/benches/css_bench.rs"],
        ),
        css_identities_blob: run_cmd(
            "git",
            &["hash-object", "crates/verter_bench/src/css_identities.rs"],
        ),
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
    use crate::css_identities::{identity_universe, universe, GROUPS};

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
        }
    }

    /// A sealed synthetic record over the real universe with every identity's
    /// wall median set to `wall_ns`.
    fn synthetic_record(pipeline: &str, wall_ns: u64) -> CssBaselineRecord {
        let identities = identity_universe()
            .into_iter()
            .map(|identity| IdentityMeasurement {
                identity,
                category: "class_rules".to_string(),
                input_len_bytes: 100,
                samples: 30,
                iters_per_sample: 100,
                wall_ns_median: wall_ns,
                wall_ns_min: wall_ns - 1,
                wall_ns_max: wall_ns + 1,
                alloc_count: 10,
                alloc_bytes: 1000,
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
        };
        compare_records(&identity_universe(), &baseline, &candidate, &policy)
            .expect("declared transition passes");
        // The declared transition is exact: reversed direction still refuses.
        let err = compare_records(&identity_universe(), &candidate, &baseline, &policy)
            .expect_err("reversed transition must refuse");
        assert!(err.iter().any(|e| e.contains("pipeline")));
    }
}
