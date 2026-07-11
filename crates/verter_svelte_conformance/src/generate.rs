//! The canonical CSS-manifest plan and the committed fixture corpus it
//! materializes.
//!
//! [`build_plan`] projects the typed [`manifest`] into an ordered, fully
//! rendered [`CoveragePlan`]; [`plan_json`] is the machine-readable emit-plan
//! wire the Node golden façade consumes (Node never re-derives the matrix —
//! it only compiles each `source` with the pinned official compiler under the
//! case's `compileOptions`). [`coverage_index_json`] and
//! [`coverage_summary_md`] are DERIVED review artifacts, never authorities.
//! [`write_corpus`] / [`check_corpus`] own the committed corpus surface: the
//! `fixtures/` subtree plus the two review artifacts at the corpus root — and
//! nothing else.
//!
//! Everything here is deterministic: equal manifests produce byte-identical
//! plans, JSON, markdown, and fixture trees (no timestamps, no map-order or
//! environment leaks).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::value::RawValue;

use crate::manifest::{manifest, SCHEMA_VERSION};
use crate::model::{
    CompileTarget, Disposition, ManifestCompileOptions, MatchOutcome, RowLevels,
    SelectorValueRepresentation, TemplateValueRepresentation, FACTOR_COUNT,
};

/// Stable per-factor names, indexed by covering-factor index. Presentation
/// vocabulary for the review artifacts (the typed authority stays the model
/// enums).
const FACTOR_NAMES: [&str; FACTOR_COUNT] = [
    "selector-kind",
    "template-value",
    "selector-value",
    "target",
    "quoting",
    "element-region",
    "css-source",
    "structural-kind",
    "match-outcome",
];

/// The corpus-relative fixture subtree this module owns exclusively.
const FIXTURES_DIR: &str = "fixtures";

/// The corpus-root review artifacts this module owns.
const INDEX_FILE: &str = "coverage-index.json";
const SUMMARY_FILE: &str = "coverage-summary.md";

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/// One fully rendered conformance case of the plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanEntry {
    /// Stable case identity (also the fixture stem under `fixtures/`).
    pub slug: String,
    /// The rendered `.svelte` fixture source.
    pub source: String,
    /// Typed per-case compile options.
    pub compile_options: ManifestCompileOptions,
    /// The typed classification of the case.
    pub disposition: Disposition,
    /// The declared-expected match verdict.
    pub expected_outcome: MatchOutcome,
    /// Full backend expansion: every case compiles on BOTH backends.
    pub backends: [CompileTarget; 2],
}

/// A strengthened interaction group, projected to factor names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanGroup {
    /// Factor names, in the group's factor-index order.
    pub factors: Vec<String>,
    /// The group's interaction strength.
    pub strength: u8,
}

/// The canonical plan: every manifest case rendered, slug-ordered, plus the
/// coverage proof/inventory summary the review artifacts project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoveragePlan {
    /// The manifest schema version the plan was built from.
    pub schema_version: u32,
    /// The manifest content hash the plan was built from.
    pub manifest_hash: String,
    /// Every case, strictly ascending by slug.
    pub entries: Vec<PlanEntry>,
    /// The full candidate-space product (all factor cardinalities multiplied).
    pub full_product: u64,
    /// Full-space Supported row count.
    pub supported_rows: u64,
    /// Full-space row count per refusal kind id.
    pub refused: Vec<(String, u64)>,
    /// Full-space row count per oracle-reject diagnostic id.
    pub oracle_rejected: Vec<(String, u64)>,
    /// Full-space row count per invalid-constraint kind id.
    pub invalid: Vec<(String, u64)>,
    /// The strengthened interaction groups, by group index.
    pub groups: Vec<PlanGroup>,
    /// The independently verified covering-array proof, rendered.
    pub covering_proof: String,
}

/// Build the canonical plan from the typed manifest.
#[must_use]
pub fn build_plan() -> CoveragePlan {
    let manifest = manifest();

    let mut entries: Vec<PlanEntry> = manifest
        .cases()
        .iter()
        .map(|case| PlanEntry {
            slug: case.slug.clone(),
            source: case.render_source(),
            compile_options: case.compile_options,
            disposition: case.disposition,
            expected_outcome: case.expected_outcome,
            backends: case.backends,
        })
        .collect();
    entries.sort_by(|a, b| a.slug.cmp(&b.slug));

    let full_product = manifest
        .spec()
        .cardinalities
        .iter()
        .map(|&cardinality| u64::from(cardinality))
        .product();

    let groups = manifest
        .spec()
        .interaction_groups
        .iter()
        .map(|group| PlanGroup {
            factors: group
                .factors
                .iter()
                .map(|&index| FACTOR_NAMES[usize::from(index)].to_string())
                .collect(),
            strength: group.strength,
        })
        .collect();

    CoveragePlan {
        schema_version: SCHEMA_VERSION,
        manifest_hash: manifest.manifest_hash().to_string(),
        entries,
        full_product,
        supported_rows: manifest.supported_row_count(),
        refused: manifest
            .refused_inventory()
            .iter()
            .map(|(kind, count)| (kind.id().to_string(), *count))
            .collect(),
        oracle_rejected: manifest
            .oracle_rejected_inventory()
            .iter()
            .map(|(kind, count)| (kind.id().to_string(), *count))
            .collect(),
        invalid: manifest
            .invalid_inventory()
            .iter()
            .map(|(kind, count)| (kind.id().to_string(), *count))
            .collect(),
        groups,
        covering_proof: manifest.proof().render(),
    }
}

// ---------------------------------------------------------------------------
// Wire vocabulary
// ---------------------------------------------------------------------------

/// The wire spelling of a disposition (`supported`, `refused:<kind>`,
/// `oracle-rejected:<kind>`, `invalid:<kind>`).
fn disposition_wire(disposition: Disposition) -> String {
    match disposition {
        Disposition::Supported => "supported".to_string(),
        Disposition::Refused(kind) => format!("refused:{}", kind.id()),
        Disposition::OracleRejected(kind) => format!("oracle-rejected:{}", kind.id()),
        Disposition::Invalid(kind) => format!("invalid:{}", kind.id()),
    }
}

/// The wire spelling of a declared outcome.
fn outcome_wire(outcome: MatchOutcome) -> &'static str {
    match outcome {
        MatchOutcome::Match => "match",
        MatchOutcome::NoMatch => "no-match",
        MatchOutcome::Maybe => "maybe",
    }
}

/// The wire spelling of the backend expansion.
fn backends_wire(backends: [CompileTarget; 2]) -> [&'static str; 2] {
    [backends[0].id(), backends[1].id()]
}

/// The compile-options wire value: the typed rendering embedded verbatim, so
/// [`ManifestCompileOptions::to_json`] stays the single shape authority.
pub(crate) fn compile_options_wire(options: &ManifestCompileOptions) -> Box<RawValue> {
    RawValue::from_string(options.to_json()).expect("typed compile options render valid JSON")
}

// ---------------------------------------------------------------------------
// emit-plan JSON
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePlan<'a> {
    schema_version: u32,
    manifest_hash: &'a str,
    cases: Vec<WirePlanCase<'a>>,
}

#[derive(Serialize)]
struct WirePlanCase<'a> {
    slug: &'a str,
    source: &'a str,
    #[serde(rename = "compileOptions")]
    compile_options: Box<RawValue>,
    disposition: String,
    #[serde(rename = "expectedOutcome")]
    expected_outcome: &'static str,
    backends: [&'static str; 2],
}

/// The emit-plan wire: compact JSON, stable key order, trailing newline.
#[must_use]
pub fn plan_json(plan: &CoveragePlan) -> String {
    let wire = WirePlan {
        schema_version: plan.schema_version,
        manifest_hash: &plan.manifest_hash,
        cases: plan
            .entries
            .iter()
            .map(|entry| WirePlanCase {
                slug: &entry.slug,
                source: &entry.source,
                compile_options: compile_options_wire(&entry.compile_options),
                disposition: disposition_wire(entry.disposition),
                expected_outcome: outcome_wire(entry.expected_outcome),
                backends: backends_wire(entry.backends),
            })
            .collect(),
    };
    let mut text = serde_json::to_string(&wire).expect("plan serializes");
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// coverage-index.json (derived review artifact)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireIndex<'a> {
    schema_version: u32,
    manifest_hash: &'a str,
    cases: Vec<WireIndexCase<'a>>,
    proof: WireProof<'a>,
}

#[derive(Serialize)]
struct WireIndexCase<'a> {
    slug: &'a str,
    disposition: String,
    #[serde(rename = "expectedOutcome")]
    expected_outcome: &'static str,
    backends: [&'static str; 2],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireProof<'a> {
    selected_cases: u64,
    full_product: u64,
    compression: String,
    partitions: WirePartitions,
    groups: Vec<WireGroup<'a>>,
    covering_proof: &'a str,
}

#[derive(Serialize)]
struct WirePartitions {
    supported: u64,
    refused: BTreeMap<String, u64>,
    #[serde(rename = "oracleRejected")]
    oracle_rejected: BTreeMap<String, u64>,
    invalid: BTreeMap<String, u64>,
}

#[derive(Serialize)]
struct WireGroup<'a> {
    factors: &'a [String],
    strength: u8,
}

/// The `coverage-index.json` review artifact: the light per-case projection
/// (no sources) plus the coverage proof summary. Pretty-printed, trailing
/// newline. Derived — never an authority.
#[must_use]
pub fn coverage_index_json(plan: &CoveragePlan) -> String {
    let wire = WireIndex {
        schema_version: plan.schema_version,
        manifest_hash: &plan.manifest_hash,
        cases: plan
            .entries
            .iter()
            .map(|entry| WireIndexCase {
                slug: &entry.slug,
                disposition: disposition_wire(entry.disposition),
                expected_outcome: outcome_wire(entry.expected_outcome),
                backends: backends_wire(entry.backends),
            })
            .collect(),
        proof: WireProof {
            selected_cases: plan.entries.len() as u64,
            full_product: plan.full_product,
            compression: format!("{}/{}", plan.entries.len(), plan.full_product),
            partitions: WirePartitions {
                supported: plan.supported_rows,
                refused: plan.refused.iter().cloned().collect(),
                oracle_rejected: plan.oracle_rejected.iter().cloned().collect(),
                invalid: plan.invalid.iter().cloned().collect(),
            },
            groups: plan
                .groups
                .iter()
                .map(|group| WireGroup {
                    factors: &group.factors,
                    strength: group.strength,
                })
                .collect(),
            covering_proof: &plan.covering_proof,
        },
    };
    let mut text = serde_json::to_string_pretty(&wire).expect("index serializes");
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// coverage-summary.md (derived review artifact)
// ---------------------------------------------------------------------------

/// The `coverage-summary.md` review artifact: partition tallies, the combined
/// representation-axes projection, and the strengthened-group inventory.
/// Derived — never an authority.
#[must_use]
pub fn coverage_summary_md(plan: &CoveragePlan) -> String {
    let manifest = manifest();
    let mut out = String::new();

    let selected = plan.entries.len() as u64;
    // Integer basis points, so the percentage needs no float formatting.
    let hundredths = selected * 10_000 / plan.full_product;

    let _ = writeln!(out, "# Svelte CSS-scoping coverage summary");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Derived review artifact. The typed Rust manifest \
         (`crates/verter_svelte_conformance`) is the sole authority; regenerate \
         this file and the fixture corpus with \
         `cargo run -p verter_svelte_conformance -- write`."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "- schema version: {}", plan.schema_version);
    let _ = writeln!(out, "- manifest hash: `{}`", plan.manifest_hash);
    let _ = writeln!(
        out,
        "- selected cases: {selected} of {} candidate rows ({}.{:02}%)",
        plan.full_product,
        hundredths / 100,
        hundredths % 100
    );
    let _ = writeln!(
        out,
        "- fixtures: `fixtures/<slug>.svelte`, one per case; each case \
         compiles on both backends (`client`, `server`)"
    );

    // Selected cases by partition.
    let mut selected_by_partition: BTreeMap<String, u64> = BTreeMap::new();
    for entry in &plan.entries {
        *selected_by_partition
            .entry(disposition_wire(entry.disposition))
            .or_insert(0) += 1;
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## Selected cases by partition");
    let _ = writeln!(out);
    let _ = writeln!(out, "| partition | cases |");
    let _ = writeln!(out, "| --- | ---: |");
    for (partition, count) in &selected_by_partition {
        let _ = writeln!(out, "| {partition} | {count} |");
    }

    // Full candidate space by partition.
    let _ = writeln!(out);
    let _ = writeln!(out, "## Full candidate space by partition");
    let _ = writeln!(out);
    let _ = writeln!(out, "| partition | kind | rows |");
    let _ = writeln!(out, "| --- | --- | ---: |");
    let _ = writeln!(out, "| supported | — | {} |", plan.supported_rows);
    for (kind, count) in &plan.refused {
        let _ = writeln!(out, "| refused | {kind} | {count} |");
    }
    for (kind, count) in &plan.oracle_rejected {
        let _ = writeln!(out, "| oracle-rejected | {kind} | {count} |");
    }
    for (kind, count) in &plan.invalid {
        let _ = writeln!(out, "| invalid | {kind} | {count} |");
    }

    // The two representation axes, combined: selected-case counts per
    // (template spelling × selector spelling) cell.
    let mut by_axes: BTreeMap<(u16, u16), u64> = BTreeMap::new();
    for entry in &plan.entries {
        let levels: RowLevels = manifest
            .case_for_slug(&entry.slug)
            .expect("plan slugs resolve in the manifest")
            .levels;
        *by_axes
            .entry((
                levels.template_value.ordinal(),
                levels.selector_value.ordinal(),
            ))
            .or_insert(0) += 1;
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "## Representation axes (selected cases: template spelling × selector spelling)"
    );
    let _ = writeln!(out);
    let mut header = String::from("| template \\ selector |");
    let mut rule = String::from("| --- |");
    for &selector in SelectorValueRepresentation::ALL {
        let _ = write!(header, " `{}` |", selector.id());
        rule.push_str(" ---: |");
    }
    let _ = writeln!(out, "{header}");
    let _ = writeln!(out, "{rule}");
    for &template in TemplateValueRepresentation::ALL {
        let _ = write!(out, "| `{}` |", template.id());
        for &selector in SelectorValueRepresentation::ALL {
            let count = by_axes
                .get(&(template.ordinal(), selector.ordinal()))
                .copied()
                .unwrap_or(0);
            let _ = write!(out, " {count} |");
        }
        let _ = writeln!(out);
    }

    // Strengthened interaction groups.
    let _ = writeln!(out);
    let _ = writeln!(out, "## Strengthened interaction groups");
    let _ = writeln!(out);
    for (index, group) in plan.groups.iter().enumerate() {
        let _ = writeln!(
            out,
            "- group {index} (strength {}): {}",
            group.strength,
            group.factors.join(" × ")
        );
    }

    // The independently verified covering proof, verbatim.
    let _ = writeln!(out);
    let _ = writeln!(out, "## Covering-array proof");
    let _ = writeln!(out);
    let _ = writeln!(out, "```");
    out.push_str(&plan.covering_proof);
    let _ = writeln!(out, "```");
    out
}

// ---------------------------------------------------------------------------
// Corpus write / check
// ---------------------------------------------------------------------------

/// The conformance crate's own committed corpus root
/// (`crates/verter_svelte_conformance/corpus`).
#[must_use]
pub fn default_corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// What [`write_corpus`] materialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReport {
    /// Number of `.svelte` fixtures written under `fixtures/`.
    pub fixtures_written: usize,
}

/// A reconciliation finding from [`check_corpus`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DriftKind {
    /// An expected file is absent (or unreadable).
    Missing,
    /// An expected file exists with different content.
    Drifted,
    /// An on-disk entry under `fixtures/` maps to no plan case.
    Stale,
}

/// One drift finding: the kind plus the corpus-relative `/`-joined path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Drift {
    /// What kind of drift.
    pub kind: DriftKind,
    /// Corpus-relative path, `/`-joined (stable across platforms).
    pub path: String,
}

impl Drift {
    /// One-line report rendering (`MISSING|DRIFTED|STALE <path>`).
    #[must_use]
    pub fn render(&self) -> String {
        let label = match self.kind {
            DriftKind::Missing => "MISSING",
            DriftKind::Drifted => "DRIFTED",
            DriftKind::Stale => "STALE",
        };
        format!("{label} {}", self.path)
    }
}

/// Every corpus-relative expected file of a plan, path → content. Paths are
/// `/`-joined.
fn expected_files(plan: &CoveragePlan) -> io::Result<BTreeMap<String, String>> {
    let mut expected = BTreeMap::new();
    for entry in &plan.entries {
        validate_slug(&entry.slug)?;
        expected.insert(
            format!("{FIXTURES_DIR}/{}.svelte", entry.slug),
            entry.source.clone(),
        );
    }
    expected.insert(INDEX_FILE.to_string(), coverage_index_json(plan));
    expected.insert(SUMMARY_FILE.to_string(), coverage_summary_md(plan));
    Ok(expected)
}

/// Reject any slug that is not a single, portable path component (defense in
/// depth: the model already renders `[a-z0-9-]` ids).
fn validate_slug(slug: &str) -> io::Result<()> {
    let portable = !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if portable {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("slug {slug:?} is not a portable path component"),
        ))
    }
}

/// Resolve a corpus-relative `/`-joined path under `root` (cross-platform).
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in rel.split('/') {
        path.push(component);
    }
    path
}

/// Write the fixture corpus: a clean rewrite of `fixtures/` (and ONLY that
/// subtree) plus `coverage-index.json` / `coverage-summary.md` at the corpus
/// root. Byte-idempotent for an unchanged manifest.
///
/// # Errors
///
/// Propagates filesystem errors; rejects a non-portable slug with
/// [`io::ErrorKind::InvalidData`] before touching the tree.
pub fn write_corpus(root: &Path) -> io::Result<WriteReport> {
    let plan = build_plan();
    let expected = expected_files(&plan)?;

    let fixtures_dir = root.join(FIXTURES_DIR);
    debug_assert_eq!(
        fixtures_dir.parent(),
        Some(root),
        "the clean-rewrite target must be the corpus-owned fixtures subtree"
    );
    if fixtures_dir.exists() {
        std::fs::remove_dir_all(&fixtures_dir)?;
    }
    std::fs::create_dir_all(&fixtures_dir)?;

    let mut fixtures_written = 0usize;
    for (rel, content) in &expected {
        std::fs::write(resolve(root, rel), content)?;
        if rel.ends_with(".svelte") {
            fixtures_written += 1;
        }
    }
    Ok(WriteReport { fixtures_written })
}

/// Line-ending-normalized equality (checkouts may rewrite LF as CRLF).
fn same_text(actual: &str, expected: &str) -> bool {
    actual.replace("\r\n", "\n") == expected.replace("\r\n", "\n")
}

/// Reconcile the on-disk corpus against a freshly built plan.
///
/// # Errors
///
/// Returns every [`Drift`] finding, sorted by path then kind: `Missing` for
/// an absent/unreadable expected file, `Drifted` for content divergence, and
/// `Stale` for an entry under `fixtures/` no plan case produces.
pub fn check_corpus(root: &Path) -> Result<(), Vec<Drift>> {
    let plan = build_plan();
    let expected = expected_files(&plan).expect("manifest slugs are portable");

    let mut drifts = Vec::new();
    for (rel, want) in &expected {
        match std::fs::read_to_string(resolve(root, rel)) {
            Ok(actual) if same_text(&actual, want) => {}
            Ok(_) => drifts.push(Drift {
                kind: DriftKind::Drifted,
                path: rel.clone(),
            }),
            Err(_) => drifts.push(Drift {
                kind: DriftKind::Missing,
                path: rel.clone(),
            }),
        }
    }

    // Orphan scan: everything under the owned subtree must map to a case.
    let fixtures_dir = root.join(FIXTURES_DIR);
    if let Ok(entries) = std::fs::read_dir(&fixtures_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = format!("{FIXTURES_DIR}/{name}");
            if entry.path().is_dir() || !expected.contains_key(&rel) {
                drifts.push(Drift {
                    kind: DriftKind::Stale,
                    path: rel,
                });
            }
        }
    }

    drifts.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.kind.cmp(&b.kind)));
    if drifts.is_empty() {
        Ok(())
    } else {
        Err(drifts)
    }
}

#[cfg(test)]
#[path = "generate_tests.rs"]
mod generate_tests;
