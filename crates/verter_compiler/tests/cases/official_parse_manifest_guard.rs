use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const EXPECTED_VUE_OWNED: usize = 509;
const EXPECTED_SVELTE_OWNED: usize = 590;
const EXPECTED_TOTAL_OWNED: usize = 1_099;
// A `parse()` call under `{ ignoreEmpty: false }` (`parse.spec.ts:243`,
// "ignoreEmpty: false") selects a non-default `SFCParseOptions` toggle
// Verter's carrier compiler has no equivalent for; the verify script now
// correctly flags it `unrepresentable_syntax_profile` instead of silently
// verifying it against default-option semantics — one more bounded
// unverifiable residual than before.
const EXPECTED_UNVERIFIABLE: usize = 68;
const EXPECTED_VUE_SHA256: &str =
    "620feadba653db459fddca73635a91b576413df7b22be68be50741bd70d7ef51";
const EXPECTED_SVELTE_SHA256: &str =
    "0ba28efe7aafde6463d0a0977d8297561525d1c6d4161ffec33d0b8369eaaa3c";

/// One recorded `- invocations:` line's parsed `key=\`value\`` pairs (the
/// per-invocation expected/actual/variant/match detail this guard checks for
/// internal consistency against the section's own classification).
struct EvidenceInvocation {
    matches: Option<bool>,
}

fn parse_invocation_line(line: &str) -> Option<EvidenceInvocation> {
    let rest = line.trim_start().strip_prefix("- expected=")?;
    let matches = rest
        .split_whitespace()
        .find_map(|token| token.strip_prefix("matches=`"))
        .and_then(|value| value.strip_suffix('`'))
        .and_then(|value| value.parse::<bool>().ok());
    Some(EvidenceInvocation { matches })
}

/// One `### <CASE-ID>` section's recorded facts, parsed from a generated
/// framework-parse-facet evidence record.
struct EvidenceSection {
    classification: String,
    source_locator: String,
    /// The recorded verdict hash — required on every section; its absence
    /// means the evidence file was written by the OLD writer that discarded
    /// invocation-level detail (the exact regression this guard exists to
    /// catch).
    verdict_hash: Option<String>,
    /// Each recorded `- expected=... matches=...` invocation line under
    /// this section's `- invocations:` block.
    invocations: Vec<EvidenceInvocation>,
}

/// One in-progress `### <case_id>` section's accumulated fields:
/// `(case_id, classification, source_locator, verdict_hash, invocations)`.
type PendingEvidenceSection = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<EvidenceInvocation>,
);

/// Parses every `### <case_id>` section out of an evidence record file into
/// a lookup by case_id, so a manifest row's `evidence_id` reference can be
/// RESOLVED against real recorded content — not merely pattern-matched by
/// string shape. Each section's `- classification: \`x\``,
/// `- source_locator: \`y\``, `- verdict_hash: \`z\``, and `- invocations:`
/// detail lines are the fields this guard cross-checks.
fn parse_evidence_sections(text: &str) -> std::collections::HashMap<String, EvidenceSection> {
    let mut sections = std::collections::HashMap::new();
    let mut current: Option<PendingEvidenceSection> = None;
    let flush = |sections: &mut std::collections::HashMap<String, EvidenceSection>,
                 entry: PendingEvidenceSection| {
        let (id, classification, source_locator, verdict_hash, invocations) = entry;
        if let (Some(classification), Some(source_locator)) = (classification, source_locator) {
            sections.insert(
                id,
                EvidenceSection {
                    classification,
                    source_locator,
                    verdict_hash,
                    invocations,
                },
            );
        }
    };
    for line in text.lines() {
        if let Some(case_id) = line.strip_prefix("### ") {
            if let Some(entry) = current.take() {
                flush(&mut sections, entry);
            }
            current = Some((case_id.trim().to_string(), None, None, None, Vec::new()));
            continue;
        }
        let Some((_, classification, source_locator, verdict_hash, invocations)) = current.as_mut()
        else {
            continue;
        };
        if let Some(value) = line.strip_prefix("- classification: `") {
            *classification = value.strip_suffix('`').map(str::to_string);
        } else if let Some(value) = line.strip_prefix("- source_locator: `") {
            *source_locator = value.strip_suffix('`').map(str::to_string);
        } else if let Some(value) = line.strip_prefix("- verdict_hash: `") {
            *verdict_hash = value.strip_suffix('`').map(str::to_string);
        } else if let Some(invocation) = parse_invocation_line(line) {
            invocations.push(invocation);
        }
    }
    if let Some(entry) = current {
        flush(&mut sections, entry);
    }
    sections
}

#[derive(Debug)]
struct ManifestCounts {
    owned: usize,
    unverifiable: usize,
    residuals: ResidualCounts,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ResidualCounts {
    co_owned_error: usize,
    unresolved_carrier: usize,
    non_carrier_sample: usize,
    injected_parser: usize,
    error_invocation_association: usize,
    unrepresentable_syntax_profile: usize,
    co_owned_style_error: usize,
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("compiler crate is nested under the repository root")
        .to_path_buf()
}

/// Cache of `evidence_id`-referenced record files, loaded and parsed once
/// per manifest inspection rather than per row.
struct EvidenceRecords<'a> {
    dir: &'a Path,
    parsed: std::cell::RefCell<
        std::collections::HashMap<String, std::collections::HashMap<String, EvidenceSection>>,
    >,
}

impl<'a> EvidenceRecords<'a> {
    fn new(dir: &'a Path) -> Self {
        Self {
            dir,
            parsed: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// Resolves `evidence_id` (`<file>#<case_id>:<classification>`) against
    /// real, committed evidence content: the referenced file must exist,
    /// must contain a `### <case_id>` section, and that section's own
    /// recorded classification must match the tag's classification and the
    /// row's own `source_locator`.
    fn resolve(&self, evidence_id: &str, case_id: &str, row_source_locator: &str) -> String {
        let (file_and_anchor, classification) = evidence_id
            .rsplit_once(':')
            .unwrap_or_else(|| panic!("evidence_id {evidence_id:?} has no classification tag"));
        let (file, anchor) = file_and_anchor.split_once('#').unwrap_or_else(|| {
            panic!("evidence_id {evidence_id:?} has no `<file>#<anchor>` reference")
        });
        assert_eq!(
            anchor,
            case_id.to_lowercase(),
            "evidence_id {evidence_id:?} anchor does not name its own row's case_id"
        );
        let mut cache = self.parsed.borrow_mut();
        if !cache.contains_key(file) {
            let text = std::fs::read_to_string(self.dir.join(file)).unwrap_or_else(|error| {
                panic!("evidence_id {evidence_id:?} references unreadable file {file}: {error}")
            });
            cache.insert(file.to_string(), parse_evidence_sections(&text));
        }
        let section = cache
            .get(file)
            .and_then(|sections| sections.get(case_id))
            .unwrap_or_else(|| {
                panic!("evidence_id {evidence_id:?}: {file} has no `### {case_id}` section")
            });
        assert_eq!(
            section.classification, classification,
            "{file}#{case_id}: evidence_id says {classification:?} but the record says {:?}",
            section.classification
        );
        assert_eq!(
            section.source_locator, row_source_locator,
            "{file}#{case_id}: evidence record's source_locator does not match the manifest row"
        );
        assert!(
            section
                .verdict_hash
                .as_deref()
                .is_some_and(|hash| !hash.is_empty()),
            "{file}#{case_id}: evidence record is missing its verdict_hash — a reviewer cannot \
             tell what was executed or why it passed from classification/locator alone"
        );
        // A `pass` verdict is unresolvable without the invocation-level detail
        // that PRODUCED it (expected outcome, actual outcome, rejection
        // variant, match result) — the exact regression this guard exists to
        // catch. Every recorded invocation under a `pass` section must itself
        // report `matches=true`; a `pass` section with a mismatched or absent
        // invocation is an internally-inconsistent record, not a resolvable
        // one.
        if classification == "pass" {
            assert!(
                !section.invocations.is_empty(),
                "{file}#{case_id}: a `pass` record has no recorded invocations — unresolvable"
            );
            assert!(
                section
                    .invocations
                    .iter()
                    .all(|invocation| invocation.matches == Some(true)),
                "{file}#{case_id}: a `pass` record has an invocation that did not match, or \
                 whose match result was not recorded"
            );
        }
        classification.to_string()
    }
}

fn inspect_manifest(path: &Path, owner_column: &str, evidence: &EvidenceRecords) -> ManifestCounts {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut lines = text.lines();
    let header = lines.next().expect("manifest header");
    let columns: Vec<&str> = header.split('\t').collect();
    let owner_index = columns
        .iter()
        .position(|column| *column == owner_column)
        .expect("owner column");
    let evidence_index = columns
        .iter()
        .position(|column| *column == "evidence_id")
        .expect("evidence column");
    let case_index = columns
        .iter()
        .position(|column| *column == "case_id")
        .expect("case column");
    let reason_index = columns
        .iter()
        .position(|column| *column == "reason")
        .expect("reason column");
    let source_locator_index = columns
        .iter()
        .position(|column| *column == "source_locator")
        .expect("source_locator column");
    let mut owned = 0;
    let mut unverifiable = 0;
    let mut residuals = ResidualCounts::default();
    for (offset, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            columns.len(),
            "{}:{} has the wrong column count",
            path.display(),
            offset + 2
        );
        if !fields[owner_index].split('/').any(|owner| owner == "B2") {
            continue;
        }
        owned += 1;
        let evidence_id = fields[evidence_index];
        let case_id = fields[case_index];
        let classification = evidence.resolve(evidence_id, case_id, fields[source_locator_index]);
        assert_ne!(
            classification,
            "fail",
            "{}:{} owned case {} has a FAILING resolved parse-facet evidence record \
             — a known-wrong parse-facet result is a bug to fix, never a value to ship",
            path.display(),
            offset + 2,
            case_id
        );
        assert!(
            classification == "pass" || classification == "unverifiable",
            "{}:{} owned case {} has an unrecognized resolved classification {classification:?}",
            path.display(),
            offset + 2,
            case_id
        );
        if classification == "unverifiable" {
            unverifiable += 1;
            let reason = fields[reason_index];
            if reason.contains("official error is retained for the co-owner") {
                residuals.co_owned_error += 1;
            } else if reason.contains("statically recoverable carrier") {
                residuals.unresolved_carrier += 1;
            } else if reason.contains("no Svelte carrier input") {
                residuals.non_carrier_sample += 1;
            } else if reason.contains("injects a parser implementation") {
                residuals.injected_parser += 1;
            } else if reason.contains("multiple frontend invocations") {
                residuals.error_invocation_association += 1;
            } else if reason.contains("finite exact tag set")
                || reason.contains("has no production equivalent")
            {
                residuals.unrepresentable_syntax_profile += 1;
            } else if reason.contains("outside carrier parsing") {
                residuals.co_owned_style_error += 1;
            } else {
                panic!(
                    "{}:{} owned case {} has an unclassified residual reason {reason:?}",
                    path.display(),
                    offset + 2,
                    fields[case_index]
                );
            }
        }
    }
    ManifestCounts {
        owned,
        unverifiable,
        residuals,
    }
}

fn assert_manifest_hash(path: &Path, expected: &str) {
    let bytes =
        std::fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let actual = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual, expected, "{} bytes changed", path.display());
}

#[test]
fn every_owned_official_case_has_passing_or_bounded_unverifiable_parse_evidence() {
    let root = repository_root().join("docs/arch/refactor/rev11/evidence/framework-conformance");
    let vue_path = root.join("vue-official-cases.tsv");
    let svelte_path = root.join("svelte-official-cases.tsv");
    assert_manifest_hash(&vue_path, EXPECTED_VUE_SHA256);
    assert_manifest_hash(&svelte_path, EXPECTED_SVELTE_SHA256);
    let evidence = EvidenceRecords::new(&root);
    let vue = inspect_manifest(&vue_path, "provisional_owner", &evidence);
    let svelte = inspect_manifest(&svelte_path, "provisional_owner", &evidence);

    assert_eq!(vue.owned, EXPECTED_VUE_OWNED);
    assert_eq!(svelte.owned, EXPECTED_SVELTE_OWNED);
    assert_eq!(vue.owned + svelte.owned, EXPECTED_TOTAL_OWNED);
    assert_eq!(
        vue.unverifiable + svelte.unverifiable,
        EXPECTED_UNVERIFIABLE,
        "the reviewed unverifiable residual must not grow or be silently reclassified"
    );
    assert_eq!(
        vue.residuals,
        ResidualCounts {
            co_owned_error: 22,
            unresolved_carrier: 9,
            injected_parser: 1,
            error_invocation_association: 1,
            unrepresentable_syntax_profile: 2,
            ..ResidualCounts::default()
        }
    );
    assert_eq!(
        svelte.residuals,
        ResidualCounts {
            non_carrier_sample: 24,
            co_owned_style_error: 9,
            ..ResidualCounts::default()
        }
    );
}
