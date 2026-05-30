//! Manifest guard for ignored typeinfo tests.
//!
//! Every `#[ignore = "..."]` annotation on a test inside
//! `crates/verter_session/src/typeinfo/typeinfo_tests/**/*.rs` MUST
//! correspond to a structured manifest row that names:
//!
//! - the test file (`apparent_types.rs`, `narrow_typeof.rs`, …),
//! - the test function (`apparent_types_ap01_string_length`, …),
//! - the closed-enum `TargetSubstrate` that will lift the ignore
//!   (`FlowNarrowing`, `MacroResolution`, `UtilityComposition`, …),
//! - a non-empty `unblocker` sentence describing the substrate
//!   change required for the test to pass.
//!
//! The closed enum spans the substrate landings the typeinfo
//! roadmap expects to ship. Adding a new ignored test requires
//! choosing one of the enumerated substrates AND naming the
//! concrete unblocker in plain English — silent drift surfaces
//! here as either a missing row or an extra row.
//!
//! Five sub-tests:
//!
//! 1. `every_ignored_typeinfo_test_has_a_manifest_row` — every
//!    `#[ignore]` site corresponds to a manifest row.
//! 2. `every_manifest_row_corresponds_to_a_live_ignored_test` —
//!    no orphan rows (the inverse audit).
//! 3. `every_manifest_row_has_valid_substrate` — closed-enum
//!    integrity (a row with an unknown substrate would not
//!    compile, but this test guards against future relaxation).
//! 4. `every_manifest_row_has_non_empty_unblocker` — substrate-
//!    quality bar; one-word `"WIP"` reasons fail.
//! 5. `total_ignored_typeinfo_test_count_matches_expected` —
//!    cardinality guard (363 today).
//!
//! Plus the legacy reason-quality test
//! (`every_ignore_reason_meets_minimum_quality_bar`) carries
//! forward unchanged.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

fn typeinfo_tests_dir() -> PathBuf {
    workspace_root().join("crates/verter_session/src/typeinfo/typeinfo_tests")
}

/// Closed-enum classifier naming the substrate landing that will
/// unblock a specific ignored test. Adding an ignored test that does
/// not fit any existing variant requires adding a new variant in
/// the same change (and naming the substrate in the unblocker
/// column).
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
#[allow(dead_code)]
enum TargetSubstrate {
    /// String / Number / Boolean / Object prototype apparent-type
    /// dispatch.
    ApparentTypes,
    /// Audit-side footprint accounting for typeinfo entry-points.
    AuditFootprint,
    /// Cache-invalidation classes the typeinfo store has to honor.
    CacheInvalidation,
    /// Call signature / overload / generic call resolution.
    CallResolution,
    /// Class member projection, decorators, accessibility, static side.
    ClassFeatures,
    /// Composite surface scenarios (menu-like, table-like, message
    /// list, etc.) that combine multiple substrates.
    CompositeSurfaces,
    /// Conditional / infer / recursive-conditional resolution.
    ConditionalInfer,
    /// Contextual typing — narrowing through call-site context.
    ContextualTyping,
    /// Cross-file (multi-module) type resolution.
    CrossFileResolution,
    /// Demand boundary contracts (Identity / Navigate / Shallow /
    /// Expanded / Skeleton query-mode dispatch).
    DemandBoundary,
    /// Enum literal projection and bridging.
    EnumResolution,
    /// Expansion boundary scenarios (budget, depth, generic
    /// instantiation cap).
    ExpansionBoundaries,
    /// Flow narrowing (discriminated union, equality, in-operator,
    /// instanceof, truthiness, typeof, and the broader narrowing
    /// substrate).
    FlowNarrowing,
    /// Index signature projection and key-kind taxonomy.
    IndexSignatures,
    /// JSX intrinsic / component-binding resolution.
    JsxResolution,
    /// Macro-aware resolution for `defineProps` / `defineEmits` /
    /// `defineSlots` / `defineOptions` / `withDefaults`.
    MacroResolution,
    /// Mapped type modifiers + name remap + template literal name
    /// transformation.
    MappedTypes,
    /// Identity / Navigate / Shallow / Expanded / Skeleton
    /// projection-mode boundary invariants.
    ModeBoundary,
    /// Modern TS feature coverage (variance, const-tp, etc.).
    ModernTsFeatures,
    /// Module-level surfaces (ambient module / namespace / module
    /// augmentation).
    ModuleFeatures,
    /// Path projection (deep indexed-access + utility-type chains).
    PathProjection,
    /// Relation-step semantics (assignability, equality, identity).
    RelationSemantics,
    /// Template literal type inference + name-remap interplay.
    TemplateLiteralInference,
    /// Tuple labelling / variadic / rest spread.
    TupleFeatures,
    /// Type parameter features (substitution types, const, variance).
    TypeParameterFeatures,
    /// Direct TypeScript-rule fidelity (assignability, structural
    /// matching, etc.).
    TypeScriptRules,
    /// Union distribution and key-access surfaces.
    UnionDistribution,
    /// Unique-symbol identity and projection.
    UniqueSymbol,
    /// Pick / Omit / Required / Partial / Exclude / Extract / etc.
    UtilityComposition,
    /// Value-side inference (typeof, contextual return shaping).
    ValueInference,
}

/// One manifest row per ignored typeinfo test.
#[derive(Clone, Copy, Debug)]
struct IgnoredTestRow {
    file: &'static str,
    function: &'static str,
    substrate: TargetSubstrate,
    unblocker: &'static str,
}

include!("manifest_data/typeinfo_ignored_test_manifest_rows.rs");

/// Total literal-string `#[ignore = "..."]` annotations across the
/// typeinfo unit-test corpus. The manifest above is the
/// authoritative source — this constant equals
/// `EXPECTED_IGNORE_MANIFEST.len()` and the discriminating test
/// below pins it.
///
/// Why 363 not 384: the substrate's macro-driven test families emit
/// expanded `#[ignore = "..."]` annotations at their call sites;
/// those expansion sites ARE counted (they are real ignore
/// annotations). What the manifest does NOT count are the
/// `#[ignore = $reason]` patterns INSIDE `macro_rules!` bodies —
/// those describe how the macro expands, not the test sites
/// themselves. The macro-defined raw count includes 21 such
/// in-macro-body lines that are not test sites; the live tree has
/// 363 expanded test-site ignores, which is the number every guard
/// below operates against.
const EXPECTED_TOTAL_IGNORED_COUNT: usize = 363;

/// Reason-strings shorter than this are rejected — every
/// `#[ignore = "..."]` must carry a meaningful sentence.
const MIN_REASON_LENGTH: usize = 16;

fn read_dir_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => panic!("read_dir {}: {err}", dir.display()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .map(|ext| ext == "rs")
                .unwrap_or(false)
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Extract `(reason, fn_name)` for every literal-string
/// `#[ignore = "..."]` annotation in `source`. The function name is
/// the next `fn <name>` token after the annotation (up to ~5 lines
/// of attribute stacking — `#[test]`, `#[ignore]`, doc-comments).
/// Bare `#[ignore]` (without a reason) is counted in the `bare`
/// return value so the guard rejects it.
///
/// Macro-defined patterns (`#[ignore = $reason]` inside a
/// `macro_rules!` body) are not test sites; they describe the
/// macro's expansion. Each expansion eventually emits a real
/// `#[ignore = "..."]` annotation that the parser counts at its
/// expansion site.
fn extract_ignored_test_sites(source: &str) -> (Vec<(String, String)>, usize) {
    let mut sites = Vec::new();
    let mut bare = 0usize;
    let lines: Vec<&str> = source.lines().collect();
    for (i, raw_line) in lines.iter().enumerate() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("#[ignore") {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix("=") {
                let after_eq = after_eq.trim_start();
                if let Some(after_quote) = after_eq.strip_prefix('"') {
                    if let Some(end) = after_quote.rfind('"') {
                        let reason = after_quote[..end].to_string();
                        // Scan forward for the next `fn <name>` line.
                        let mut fn_name: Option<String> = None;
                        let look_end = (i + 6).min(lines.len());
                        for next_line in lines.iter().take(look_end).skip(i + 1) {
                            if let Some(name) = parse_fn_name(next_line) {
                                fn_name = Some(name);
                                break;
                            }
                        }
                        if let Some(name) = fn_name {
                            sites.push((reason, name));
                        }
                        continue;
                    }
                }
                // Macro-body `#[ignore = $reason]` — skip.
                continue;
            }
            if line.starts_with("#[ignore]") || line.starts_with("#[ignore ") {
                bare += 1;
            }
        }
    }
    (sites, bare)
}

/// Parse the function name from a line of the form
/// `(async? )?(pub )?(async )?fn <name>(`.
fn parse_fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let after_async = trimmed.strip_prefix("async ").unwrap_or(trimmed);
    let after_pub = after_async.strip_prefix("pub ").unwrap_or(after_async);
    let after_async = after_pub.strip_prefix("async ").unwrap_or(after_pub);
    let after_const = after_async.strip_prefix("const ").unwrap_or(after_async);
    let after_fn = after_const.strip_prefix("fn ")?;
    let name_end = after_fn
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after_fn.len());
    if name_end == 0 {
        return None;
    }
    Some(after_fn[..name_end].to_string())
}

#[test]
fn every_ignored_typeinfo_test_carries_a_reason_string() {
    let dir = typeinfo_tests_dir();
    assert!(
        dir.exists(),
        "typeinfo_tests directory must exist at {}",
        dir.display(),
    );
    let mut total_bare = 0usize;
    for path in read_dir_files(&dir) {
        let source = fs::read_to_string(&path).expect("read file");
        let (_sites, bare) = extract_ignored_test_sites(&source);
        if bare > 0 {
            panic!(
                "ignored-test manifest: {} contains {bare} bare `#[ignore]` \
                 annotation(s) without a reason string. Add a structured \
                 reason explaining which substrate change unblocks the test.",
                path.display(),
            );
        }
        total_bare += bare;
    }
    assert_eq!(total_bare, 0);
}

#[test]
fn every_ignore_reason_meets_minimum_quality_bar() {
    let dir = typeinfo_tests_dir();
    let mut violators: Vec<(String, String)> = Vec::new();
    for path in read_dir_files(&dir) {
        let source = fs::read_to_string(&path).expect("read file");
        let (sites, _bare) = extract_ignored_test_sites(&source);
        for (reason, _fn_name) in sites {
            if reason.len() < MIN_REASON_LENGTH {
                violators.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    reason,
                ));
                continue;
            }
            let lc = reason.to_lowercase();
            let mentions_subject = [
                "typeinfo",
                "graph",
                "narrow",
                "contract",
                "future",
                "publish",
                "support",
                "scenario",
                "macro",
                "resolution",
                "resolve",
                "type",
                "node",
                "carry",
                "infer",
                "shape",
                "lift",
                "regression",
                "ignore",
                "match",
                "expansion",
                "mapped",
                "predicate",
                "instantiation",
                "narrowing",
                "envelope",
                "decl",
                "snapshot",
                "substrate",
                "landing",
            ]
            .iter()
            .any(|needle| lc.contains(needle));
            if !mentions_subject {
                violators.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    reason,
                ));
            }
        }
    }
    if !violators.is_empty() {
        panic!(
            "ignored-test manifest: {} reason string(s) fail the minimum \
             quality bar:\n  {}",
            violators.len(),
            violators
                .iter()
                .map(|(f, r)| format!("{f}: \"{}\"", r))
                .collect::<Vec<_>>()
                .join("\n  "),
        );
    }
}

/// Collect the live `(file, function)` set from the typeinfo
/// test source tree.
fn collect_live_ignored_sites() -> BTreeSet<(String, String)> {
    let mut out: BTreeSet<(String, String)> = BTreeSet::new();
    for path in read_dir_files(&typeinfo_tests_dir()) {
        let source = fs::read_to_string(&path).expect("read file");
        let (sites, _bare) = extract_ignored_test_sites(&source);
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        for (_reason, fn_name) in sites {
            out.insert((file_name.clone(), fn_name));
        }
    }
    out
}

#[test]
fn every_ignored_typeinfo_test_has_a_manifest_row() {
    let live = collect_live_ignored_sites();
    let manifest: BTreeSet<(String, String)> = EXPECTED_IGNORE_MANIFEST
        .iter()
        .map(|row| (row.file.to_string(), row.function.to_string()))
        .collect();
    let mut missing: Vec<(String, String)> = live.difference(&manifest).cloned().collect();
    missing.sort();
    assert!(
        missing.is_empty(),
        "ignored-test manifest: {} live `#[ignore]` site(s) lack a \
         matching `IgnoredTestRow` in `EXPECTED_IGNORE_MANIFEST`. \
         Add a row naming the closed `TargetSubstrate` substrate and \
         the concrete unblocker. Missing rows:\n  {}",
        missing.len(),
        missing
            .iter()
            .map(|(f, fnm)| format!("{f} :: {fnm}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn every_manifest_row_corresponds_to_a_live_ignored_test() {
    let live = collect_live_ignored_sites();
    let mut orphans: Vec<(String, String)> = EXPECTED_IGNORE_MANIFEST
        .iter()
        .map(|row| (row.file.to_string(), row.function.to_string()))
        .filter(|pair| !live.contains(pair))
        .collect();
    orphans.sort();
    assert!(
        orphans.is_empty(),
        "ignored-test manifest: {} `IgnoredTestRow` entries point at \
         tests that no longer exist (or that no longer carry an \
         `#[ignore]` annotation). Remove the stale rows. Orphans:\n  {}",
        orphans.len(),
        orphans
            .iter()
            .map(|(f, fnm)| format!("{f} :: {fnm}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn every_manifest_row_has_non_empty_unblocker() {
    let mut violators: Vec<(String, String)> = Vec::new();
    for row in EXPECTED_IGNORE_MANIFEST {
        if row.unblocker.trim().len() < MIN_REASON_LENGTH {
            violators.push((row.function.to_string(), row.unblocker.to_string()));
        }
    }
    assert!(
        violators.is_empty(),
        "ignored-test manifest: {} row(s) have an empty / too-short \
         `unblocker` column. A well-formed manifest row names the \
         concrete substrate change required to lift the ignore. \
         Violators:\n  {}",
        violators.len(),
        violators
            .iter()
            .map(|(f, u)| format!("{f}: \"{u}\""))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

/// Reason equality: every manifest row's `unblocker` column MUST
/// match the corresponding live `#[ignore = "..."]` reason string
/// byte-for-byte.
///
/// This guard catches generator regressions where the reason regex
/// truncates an escape-bearing string (the round-3 fix replaced a
/// naive `"([^"]*)"` with the escape-aware
/// `"((?:[^"\\]|\\.)*)"`). Pre-fix the 49 reasons containing
/// embedded `\"` truncated at the first internal quote; the
/// manifest row carried a partial sentence that still passed the
/// non-empty / minimum-length guards but no longer matched the
/// live source. Post-fix every row's `unblocker` equals the live
/// reason exactly.
///
/// The Rust extractor walks the source the same way `extract_sites`
/// in `scripts/gen-typeinfo-ignore-manifest.py` does (substring up
/// to the last `"` on the line). Both parsers operate on the
/// SOURCE bytes — backslash and quote sequences appear literally in
/// the captured reason (the row's stored `unblocker` un-escapes the
/// Rust-literal form back to the same source bytes when the row is
/// loaded).
#[test]
fn every_manifest_row_unblocker_matches_live_ignore_reason() {
    let live_by_pair: BTreeMap<(String, String), String> = {
        let mut acc: BTreeMap<(String, String), String> = BTreeMap::new();
        for path in read_dir_files(&typeinfo_tests_dir()) {
            let source = fs::read_to_string(&path).expect("read file");
            let (sites, _bare) = extract_ignored_test_sites(&source);
            let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
            for (reason, fn_name) in sites {
                acc.insert((file_name.clone(), fn_name), reason);
            }
        }
        acc
    };

    let mut mismatches: Vec<(String, String, String, String)> = Vec::new();
    for row in EXPECTED_IGNORE_MANIFEST {
        let key = (row.file.to_string(), row.function.to_string());
        match live_by_pair.get(&key) {
            None => {
                // Orphan rows are covered by the
                // `every_manifest_row_corresponds_to_a_live_ignored_test`
                // guard; reason equality only applies when the live
                // site exists.
                continue;
            }
            Some(live_reason) => {
                if live_reason != row.unblocker {
                    mismatches.push((
                        row.file.to_string(),
                        row.function.to_string(),
                        row.unblocker.to_string(),
                        live_reason.clone(),
                    ));
                }
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "ignored-test manifest: {} row(s) have an `unblocker` that \
         does not match the live `#[ignore = \"...\"]` reason. \
         Re-run `python3 scripts/gen-typeinfo-ignore-manifest.py` \
         after editing the source ignore reason — silent drift here \
         usually means the generator regex truncated an escape-bearing \
         reason. Mismatches (file :: fn  manifest=...  live=...):\n  {}",
        mismatches.len(),
        mismatches
            .iter()
            .map(|(f, fnm, m, l)| format!(
                "{f} :: {fnm}\n      manifest={m:?}\n      live    ={l:?}"
            ))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn every_manifest_row_lists_a_valid_substrate() {
    // The closed enum makes invalid substrate names a compile
    // error; this test pins the structural invariant by exercising
    // every variant in the `Debug` formatter, which would panic if
    // the discriminant were ever out of range.
    let mut seen_variants: HashSet<String> = HashSet::new();
    for row in EXPECTED_IGNORE_MANIFEST {
        seen_variants.insert(format!("{:?}", row.substrate));
    }
    // The substrate set is closed and non-empty.
    assert!(!seen_variants.is_empty());
    // At least the FlowNarrowing variant is exercised (narrow_*
    // files alone produce 100+ rows pointing at this substrate);
    // the cardinality below is a sanity floor, not an upper bound.
    assert!(
        seen_variants.contains("FlowNarrowing"),
        "FlowNarrowing substrate must appear in the manifest (the \
         narrow_* files all point at this substrate)",
    );
}

#[test]
fn total_ignored_typeinfo_test_count_matches_expected() {
    let dir = typeinfo_tests_dir();
    let mut total = 0usize;
    for path in read_dir_files(&dir) {
        let source = fs::read_to_string(&path).expect("read file");
        let (sites, _bare) = extract_ignored_test_sites(&source);
        total += sites.len();
    }
    assert_eq!(
        total, EXPECTED_TOTAL_IGNORED_COUNT,
        "total ignored typeinfo test count drifted from documented baseline ({EXPECTED_TOTAL_IGNORED_COUNT})",
    );
}

#[test]
fn manifest_length_matches_documented_total() {
    assert_eq!(
        EXPECTED_IGNORE_MANIFEST.len(),
        EXPECTED_TOTAL_IGNORED_COUNT,
        "`EXPECTED_IGNORE_MANIFEST` row count must equal the documented \
         total {EXPECTED_TOTAL_IGNORED_COUNT}",
    );
}

#[test]
fn per_file_ignored_test_counts_match_manifest() {
    // Per-file partition check: the manifest row count per file
    // must equal the live `#[ignore]` count per file. Keeps the
    // legacy guard's intent (catch additions/removals/drift) while
    // routing through the new row-shaped manifest.
    let dir = typeinfo_tests_dir();
    let mut observed: BTreeMap<String, usize> = BTreeMap::new();
    for path in read_dir_files(&dir) {
        let source = fs::read_to_string(&path).expect("read file");
        let (sites, _bare) = extract_ignored_test_sites(&source);
        if !sites.is_empty() {
            observed.insert(
                path.file_name().unwrap().to_string_lossy().into_owned(),
                sites.len(),
            );
        }
    }

    let mut expected: BTreeMap<String, usize> = BTreeMap::new();
    for row in EXPECTED_IGNORE_MANIFEST {
        *expected.entry(row.file.to_string()).or_default() += 1;
    }

    if observed != expected {
        let mut report = String::new();
        for (name, count) in &expected {
            let actual = observed.get(name).copied().unwrap_or(0);
            if actual != *count {
                report.push_str(&format!("  {name}: expected {count}, observed {actual}\n"));
            }
        }
        for (name, count) in &observed {
            if !expected.contains_key(name) {
                report.push_str(&format!(
                    "  {name}: NEW FILE with {count} ignored tests (add rows to EXPECTED_IGNORE_MANIFEST)\n"
                ));
            }
        }
        panic!(
            "ignored-test manifest drift between EXPECTED_IGNORE_MANIFEST and the \
             live tree:\n{report}\n\
             Update EXPECTED_IGNORE_MANIFEST when intentionally adding or \
             removing ignored tests."
        );
    }
}
