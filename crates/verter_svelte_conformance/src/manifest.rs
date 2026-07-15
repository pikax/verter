//! The executable coverage manifest: the typed Rust authority every other
//! surface (fixture generation, golden reconciliation, differential and
//! metamorphic suites, the emit-plan CLI) consumes.
//!
//! [`manifest`] builds the singleton [`CoverageManifest`] once: it assembles
//! the [`CoverageSpec`] from the exhaustive model enums, runs the
//! deterministic covering-array engine over the full candidate space with
//! [`classify_row`] as the sole classification authority, retains the
//! selected rows as typed [`ManifestCase`]s (each expanding to both compile
//! backends), and self-verifies the resulting coverage proof. A generation or
//! verification failure is a hard panic: the manifest itself is broken, not a
//! caller input.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::covering_array::{
    generate, verify, ClassifiedRow, CoverageProof, CoverageSpec, InteractionGroup, Partition, Row,
};
use crate::model::{
    classify, compile_options, factor_cardinalities, render_fixture, semantic_value_families, slug,
    CompileTarget, ConstraintKind, DiagnosticKind, Disposition, ManifestCompileOptions,
    MatchOutcome, RefusalKind, RowLevels, SemanticValueFamily, FACTOR_COUNT, FACTOR_MATCH_OUTCOME,
    FACTOR_QUOTING, FACTOR_SELECTOR_KIND, FACTOR_SELECTOR_VALUE, FACTOR_STRUCTURAL_KIND,
    FACTOR_TARGET, FACTOR_TEMPLATE_VALUE,
};

/// Version of the manifest schema (bumped on any breaking change to the
/// spec shape, the classification vocabulary, or the case contract).
/// v2: the `refused:*` disposition vocabulary emptied — the former
/// `refused:legacy-slot-scope-unprovable` cells reclassified `supported`
/// (the `<slot>` fallback region lowers + scopes through the shared matcher).
/// v3: the legacy VALUE-WRAP × SURFACE coverage axis added
/// ([`crate::value_wrap`]) — the typed `ValueWrapSurface` vocabulary, the
/// exhaustive `classify_value_wrap` cells, and the executable per-cell
/// observation gate (`tests/cases/value_wrap_cells.rs`).
/// v4: the `fragments: 'tree'` compile-option carrier added to
/// [`ManifestCompileOptions`] (a new `to_json` shape) — the scoped-CSS tree
/// cell pins tree-mode `$.from_tree` delivery via the manifest-cell assertion
/// plus the committed `--conformance` golden. (The css scope token is baked
/// identically in html and tree by the shared static-attribute authority, so
/// the oracle differential's scope-token axis is scope-token-identical across
/// the flip and does not itself discriminate it — see
/// [`crate::model::compile_options`].)
pub const SCHEMA_VERSION: u32 = 4;

/// One selected covering row, expanded into its executable conformance case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestCase {
    /// Stable, cross-platform-safe identity (also the fixture stem).
    pub slug: String,
    /// The ordinal covering row.
    pub row: Row<FACTOR_COUNT>,
    /// The decoded typed levels.
    pub levels: RowLevels,
    /// The typed disposition [`classify_row`] assigned.
    pub disposition: Disposition,
    /// The declared-expected match verdict (factor 8).
    pub expected_outcome: MatchOutcome,
    /// Full backend expansion: every case compiles on BOTH backends.
    pub backends: [CompileTarget; 2],
    /// Typed per-case compile options.
    pub compile_options: ManifestCompileOptions,
}

impl ManifestCase {
    /// Render this case's `.svelte` fixture source.
    #[must_use]
    pub fn render_source(&self) -> String {
        render_fixture(&self.levels)
    }
}

/// The built manifest: spec, selected cases, families, proof, inventories.
#[derive(Debug)]
pub struct CoverageManifest {
    spec: CoverageSpec<FACTOR_COUNT>,
    cases: Vec<ManifestCase>,
    families: Vec<SemanticValueFamily>,
    proof: CoverageProof,
    hash: String,
    supported_rows: u64,
    refused_inventory: Vec<(RefusalKind, u64)>,
    oracle_rejected_inventory: Vec<(DiagnosticKind, u64)>,
    invalid_inventory: Vec<(ConstraintKind, u64)>,
    slug_index: BTreeMap<String, usize>,
}

impl CoverageManifest {
    /// The coverage demands the covering array was generated against.
    #[must_use]
    pub fn spec(&self) -> &CoverageSpec<FACTOR_COUNT> {
        &self.spec
    }

    /// Classify a row (delegates to the sole authority [`classify_row`]).
    #[must_use]
    pub fn classify(&self, row: Row<FACTOR_COUNT>) -> Partition {
        classify_row(row)
    }

    /// The selected covering cases, in ascending row-ordinal order.
    #[must_use]
    pub fn cases(&self) -> &[ManifestCase] {
        &self.cases
    }

    /// The static semantic-value equivalence families.
    #[must_use]
    pub fn families(&self) -> &[SemanticValueFamily] {
        &self.families
    }

    /// The independently verified coverage proof of [`cases`](Self::cases).
    #[must_use]
    pub fn proof(&self) -> &CoverageProof {
        &self.proof
    }

    /// Stable content hash of the spec, the selected case inventory, the
    /// families, the coverage proof, and the FULL-SPACE classification (the
    /// complete row-classification stream digest plus every per-partition
    /// inventory) — `fnv1a64-<16 hex digits>`.
    #[must_use]
    pub fn manifest_hash(&self) -> &str {
        &self.hash
    }

    /// Look up a case by slug.
    #[must_use]
    pub fn case_for_slug(&self, slug: &str) -> Option<&ManifestCase> {
        self.slug_index.get(slug).map(|&index| &self.cases[index])
    }

    /// Every case slug, in case order.
    #[must_use]
    pub fn all_slugs(&self) -> Vec<&str> {
        self.cases.iter().map(|case| case.slug.as_str()).collect()
    }

    /// Total Supported rows in the full candidate space.
    #[must_use]
    pub fn supported_row_count(&self) -> u64 {
        self.supported_rows
    }

    /// Full-space row count per declared refusal kind (every kind listed).
    #[must_use]
    pub fn refused_inventory(&self) -> &[(RefusalKind, u64)] {
        &self.refused_inventory
    }

    /// Full-space row count per oracle-reject diagnostic (every kind listed).
    #[must_use]
    pub fn oracle_rejected_inventory(&self) -> &[(DiagnosticKind, u64)] {
        &self.oracle_rejected_inventory
    }

    /// Full-space row count per invalid-constraint kind (every kind listed).
    #[must_use]
    pub fn invalid_inventory(&self) -> &[(ConstraintKind, u64)] {
        &self.invalid_inventory
    }
}

/// The coverage demands: the nine factor level counts, global 3-wise
/// coverage, a strengthened 5-wise group over {template representation ×
/// target × quoting × element region × match outcome}, and a strengthened
/// 4-wise group over {selector kind × selector representation × structural
/// kind × match outcome}.
#[must_use]
pub fn coverage_spec() -> CoverageSpec<FACTOR_COUNT> {
    CoverageSpec {
        cardinalities: factor_cardinalities(),
        global_strength: 3,
        interaction_groups: vec![
            InteractionGroup {
                factors: vec![
                    FACTOR_TEMPLATE_VALUE as u8,
                    FACTOR_TARGET as u8,
                    FACTOR_QUOTING as u8,
                    FACTOR_ELEMENT_REGION_U8,
                    FACTOR_MATCH_OUTCOME as u8,
                ],
                strength: 5,
            },
            InteractionGroup {
                factors: vec![
                    FACTOR_SELECTOR_KIND as u8,
                    FACTOR_SELECTOR_VALUE as u8,
                    FACTOR_STRUCTURAL_KIND as u8,
                    FACTOR_MATCH_OUTCOME as u8,
                ],
                strength: 4,
            },
        ],
    }
}

// `FACTOR_ELEMENT_REGION` as `u8` (kept next to the spec so the group rows
// read uniformly).
const FACTOR_ELEMENT_REGION_U8: u8 = crate::model::FACTOR_ELEMENT_REGION as u8;

/// The SOLE classification authority for ordinal rows: decode, apply the
/// typed constraint functions, bridge to the engine partition. A row with any
/// out-of-range level is outside the candidate universe and maps to
/// [`Partition::Invalid`].
#[must_use]
pub fn classify_row(row: Row<FACTOR_COUNT>) -> Partition {
    match RowLevels::decode(row) {
        Some(levels) => classify(&levels).partition(),
        None => Partition::Invalid,
    }
}

/// The process-wide manifest singleton.
///
/// # Panics
///
/// Panics when covering-array generation or verification fails: the manifest
/// (spec + constraints) is internally broken, which is a defect, not an
/// input error.
#[must_use]
pub fn manifest() -> &'static CoverageManifest {
    static MANIFEST: OnceLock<CoverageManifest> = OnceLock::new();
    MANIFEST.get_or_init(build_manifest)
}

/// Build a fresh manifest (exposed for determinism tests; production code
/// uses the [`manifest`] singleton).
#[doc(hidden)]
#[must_use]
pub fn build_manifest() -> CoverageManifest {
    let spec = coverage_spec();
    let array = generate(&spec, classify_row)
        .unwrap_or_else(|error| panic!("coverage manifest generation failed: {error:?}"));

    let cases: Vec<ManifestCase> = array
        .rows
        .iter()
        .map(|classified| {
            let levels = RowLevels::decode(classified.row)
                .expect("selected rows decode (generate enumerates in-range rows only)");
            let disposition = classify(&levels);
            debug_assert_eq!(disposition.partition(), classified.partition);
            ManifestCase {
                slug: slug(&levels),
                row: classified.row,
                levels,
                disposition,
                expected_outcome: levels.outcome,
                backends: [CompileTarget::Client, CompileTarget::Server],
                compile_options: compile_options(&levels),
            }
        })
        .collect();

    // Re-verify through the shared engine entry (the proof retained on the
    // manifest is the independently recomputed one).
    let classified: Vec<ClassifiedRow<FACTOR_COUNT>> = cases
        .iter()
        .map(|case| ClassifiedRow {
            row: case.row,
            partition: case.disposition.partition(),
        })
        .collect();
    let proof = verify(&spec, &classified, classify_row)
        .unwrap_or_else(|error| panic!("coverage manifest self-verification failed: {error:?}"));

    let facts = full_space_facts(&spec, classify);

    let families = semantic_value_families();
    let slug_index: BTreeMap<String, usize> = cases
        .iter()
        .enumerate()
        .map(|(index, case)| (case.slug.clone(), index))
        .collect();
    let hash = content_hash(&spec, &cases, &families, &proof, &facts);

    CoverageManifest {
        spec,
        cases,
        families,
        proof,
        hash,
        supported_rows: facts.supported_rows,
        refused_inventory: facts.refused,
        oracle_rejected_inventory: facts.oracle_rejected,
        invalid_inventory: facts.invalid,
        slug_index,
    }
}

/// The full-candidate-space classification facts: the per-partition
/// inventories plus the digest of the COMPLETE row-classification stream.
struct FullSpaceFacts {
    supported_rows: u64,
    refused: Vec<(RefusalKind, u64)>,
    oracle_rejected: Vec<(DiagnosticKind, u64)>,
    invalid: Vec<(ConstraintKind, u64)>,
    /// FNV-1a 64 digest of every row's disposition in ascending ordinal
    /// order (partition tag byte + kind ordinal per row) — pins the exact
    /// row→partition assignment, which per-kind counts alone cannot (a
    /// count-preserving reshuffle between kinds leaves every count intact).
    classification_digest: u64,
}

/// Enumerate the full candidate space once under `classify_levels`: tally
/// every partition keyed by the typed model enums (every declared kind
/// appears, zero-count rows included, so the inventory is exhaustive by
/// construction) and digest the complete classification stream.
fn full_space_facts(
    spec: &CoverageSpec<FACTOR_COUNT>,
    classify_levels: impl Fn(&RowLevels) -> Disposition,
) -> FullSpaceFacts {
    let mut supported = 0u64;
    let mut refused: Vec<(RefusalKind, u64)> =
        RefusalKind::ALL.iter().map(|&kind| (kind, 0)).collect();
    let mut oracle: Vec<(DiagnosticKind, u64)> =
        DiagnosticKind::ALL.iter().map(|&kind| (kind, 0)).collect();
    let mut invalid: Vec<(ConstraintKind, u64)> =
        ConstraintKind::ALL.iter().map(|&kind| (kind, 0)).collect();
    let mut digest = Fnv1a64::new();

    for_each_row(spec, |levels| {
        let (tag, kind_ordinal) = match classify_levels(&levels) {
            Disposition::Supported => {
                supported += 1;
                (0u8, 0u16)
            }
            Disposition::Refused(kind) => {
                refused[usize::from(kind.ordinal())].1 += 1;
                (1u8, kind.ordinal())
            }
            Disposition::OracleRejected(kind) => {
                oracle[usize::from(kind.ordinal())].1 += 1;
                (2u8, kind.ordinal())
            }
            Disposition::Invalid(kind) => {
                invalid[usize::from(kind.ordinal())].1 += 1;
                (3u8, kind.ordinal())
            }
        };
        digest.write(&[tag]);
        digest.write(&kind_ordinal.to_le_bytes());
    });

    FullSpaceFacts {
        supported_rows: supported,
        refused,
        oracle_rejected: oracle,
        invalid,
        classification_digest: digest.0,
    }
}

/// Visit every decoded row of the candidate space in ascending ordinal order.
fn for_each_row(spec: &CoverageSpec<FACTOR_COUNT>, mut visit: impl FnMut(RowLevels)) {
    let cards = spec.cardinalities;
    let mut levels = [0u16; FACTOR_COUNT];
    loop {
        let decoded = RowLevels::decode(Row(levels)).expect("in-range row decodes");
        visit(decoded);
        // Mixed-radix increment, factor 8 least significant.
        let mut factor = FACTOR_COUNT;
        loop {
            if factor == 0 {
                return;
            }
            factor -= 1;
            levels[factor] += 1;
            if levels[factor] < cards[factor] {
                break;
            }
            levels[factor] = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Content hash (FNV-1a 64, std-only, deterministic)
// ---------------------------------------------------------------------------

struct Fnv1a64(u64);

impl Fnv1a64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Fnv1a64(Self::OFFSET_BASIS)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn write_str(&mut self, text: &str) {
        self.write(text.as_bytes());
        // Length-delimit so concatenation cannot alias across fields.
        self.write(&(text.len() as u64).to_le_bytes());
    }
}

fn content_hash(
    spec: &CoverageSpec<FACTOR_COUNT>,
    cases: &[ManifestCase],
    families: &[SemanticValueFamily],
    proof: &CoverageProof,
    facts: &FullSpaceFacts,
) -> String {
    let mut hasher = Fnv1a64::new();
    hasher.write(&SCHEMA_VERSION.to_le_bytes());
    for &cardinality in &spec.cardinalities {
        hasher.write(&cardinality.to_le_bytes());
    }
    hasher.write(&[spec.global_strength]);
    for group in &spec.interaction_groups {
        hasher.write(&group.factors);
        hasher.write(&[group.strength]);
    }
    for case in cases {
        hasher.write_str(&case.slug);
        for &level in &case.row.0 {
            hasher.write(&level.to_le_bytes());
        }
        hasher.write_str(&disposition_tag(case.disposition));
        hasher.write_str(case.expected_outcome.id());
        hasher.write_str(&case.compile_options.to_json());
    }
    for family in families {
        hasher.write_str(family.name);
        hasher.write_str(family.base_value);
        for rendering in &family.renderings {
            hasher.write_str(rendering.kind.id());
            hasher.write_str(rendering.rendered);
        }
        hasher.write_str(family.verdict.selector);
        hasher.write_str(family.verdict.outcome.id());
    }
    hasher.write_str(&proof.render());
    // The full-space classification: the complete row-classification stream
    // digest plus every per-partition inventory, so the hash pins the
    // partition proof it labels — reassigning rows between kinds (even
    // count-preservingly) moves the hash.
    hasher.write(&facts.classification_digest.to_le_bytes());
    hasher.write(&facts.supported_rows.to_le_bytes());
    for (kind, count) in &facts.refused {
        hasher.write_str(kind.id());
        hasher.write(&count.to_le_bytes());
    }
    for (kind, count) in &facts.oracle_rejected {
        hasher.write_str(kind.id());
        hasher.write(&count.to_le_bytes());
    }
    for (kind, count) in &facts.invalid {
        hasher.write_str(kind.id());
        hasher.write(&count.to_le_bytes());
    }
    format!("fnv1a64-{:016x}", hasher.0)
}

/// A stable textual tag for a disposition (hash input only).
fn disposition_tag(disposition: Disposition) -> String {
    match disposition {
        Disposition::Supported => "supported".to_string(),
        Disposition::Refused(kind) => format!("refused:{}", kind.id()),
        Disposition::OracleRejected(kind) => format!("oracle-rejected:{}", kind.id()),
        Disposition::Invalid(kind) => format!("invalid:{}", kind.id()),
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod manifest_tests;
