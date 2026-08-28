//! J1-A9: capability-matrix CSS-family product×dialect×operation closure.
//!
//! The checked-in CSS capability matrix replaces the
//! single aggregate `CSS | parse/format/index/transform` seed row with an
//! exact, closed 2 (product) × 5 (dialect) × 4 (operation) = 40-row table
//! carrying the original §1 row schema plus the two columns A9 adds
//! (`Dialect`, `Disposition`) and one further column A9's own gate text
//! requires (`Evidence`). This test parses that table DIRECTLY out of the
//! committed markdown file (structured-table-parse — never a hand-copied
//! constant the doc could drift away from unnoticed) and asserts:
//!
//! (a) the collected `(Product, Dialect, Operation)` triple set equals
//!     exactly the 40-triple universe — no missing, no duplicate row;
//! (b) every triple's `Disposition` equals the exact expected value per the
//!     charter's rule (not mere enum membership), and its `Evidence` cell is
//!     non-empty;
//! (c) every `Native`-dispositioned triple's `Status` is concrete
//!     (`Ratified`) — Css transform is Native for BOTH products; H046 DEFER
//!     is not authority and must not reopen a Native row to `VERIFY`;
//! (d) the still-`VERIFY` `Status` set equals exactly the non-`Native`
//!     subset — no `Native` row hides behind `VERIFY`, and no non-`Native`
//!     row is falsely marked closed;
//! (e) the table's separator row structurally matches the header's column
//!     count (a malformed `|---|` collapsed-column separator does not slip
//!     through unnoticed).

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

const CAPABILITY_MATRIX_RELATIVE_PATH: &str = "test-corpora/style-ir/capability-matrix.md";

const SECTION_HEADING: &str = "## 2.2 CSS-family capability matrix (J1-A9 closure)";
const NEXT_SECTION_PREFIX: &str = "# 3. Rules";

const PRODUCTS: [&str; 2] = ["Vue", "Svelte"];
const DIALECTS: [&str; 5] = ["Css", "Scss", "Sass", "Less", "Stylus"];
const OPERATIONS: [&str; 4] = ["parse", "index", "transform", "format"];

const EXPECTED_HEADER: [&str; 14] = [
    "Framework/product",
    "Dialect",
    "Operation",
    "Route/backend",
    "Maturity",
    "Default",
    "Semantic profile(s)",
    "Oracle/conformance corpus",
    "Exact unsupported/degradation behavior",
    "Zero-work negative proof",
    "Compatibility promise",
    "Disposition",
    "Status",
    "Evidence",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Triple {
    product: String,
    dialect: String,
    operation: String,
}

impl fmt::Display for Triple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {})",
            self.product, self.dialect, self.operation
        )
    }
}

#[derive(Debug, Clone)]
struct MatrixRow {
    triple: Triple,
    disposition: String,
    status: String,
    evidence: String,
}

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

/// Split a markdown table row (`| a | b | c |`) into trimmed cells, dropping
/// the leading/trailing empty strings produced by the boundary pipes.
fn split_row_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed.strip_prefix('|').unwrap_or(trimmed));
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

/// A markdown table separator cell is one or more `-` characters, optionally
/// bounded by AT MOST ONE leading and AT MOST ONE trailing `:` alignment
/// marker (GFM: `---`, `:---`, `---:`, `:---:`) — never empty, never
/// containing anything else, and never more than one colon per side (`::--`
/// or `--::` are not valid GFM alignment syntax and must not slip through as
/// if they were). This rejects both a collapsed/malformed separator row
/// (e.g. a single `---` cell standing in for a 14-column header, caught by
/// the caller's column-count check) and a per-cell malformed delimiter (e.g.
/// `::---::`) that a looser `trim_matches(':')` would wrongly accept.
fn is_valid_separator_cell(cell: &str) -> bool {
    let mut core = cell;
    if let Some(rest) = core.strip_prefix(':') {
        core = rest;
    }
    if let Some(rest) = core.strip_suffix(':') {
        core = rest;
    }
    !core.is_empty() && core.chars().all(|c| c == '-') && !core.contains(':')
}

/// Extract and parse the §2.2 CSS-family table's data rows from the
/// committed capability-matrix document.
fn parse_css_family_matrix() -> Vec<MatrixRow> {
    let path = workspace_root().join(CAPABILITY_MATRIX_RELATIVE_PATH);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let section_start = content
        .find(SECTION_HEADING)
        .unwrap_or_else(|| panic!("section heading {SECTION_HEADING:?} not found in {path:?}"));
    let after_heading = &content[section_start..];
    let section_end = after_heading.find(NEXT_SECTION_PREFIX).unwrap_or_else(|| {
        panic!("next-section marker {NEXT_SECTION_PREFIX:?} not found after §2.2")
    });
    let section = &after_heading[..section_end];

    let mut lines = section.lines();
    let header_line = loop {
        let line = lines
            .next()
            .expect("table header row not found in §2.2 before end of section");
        if line.trim_start().starts_with("| Framework/product ") {
            break line;
        }
    };
    let header_cells = split_row_cells(header_line);
    assert_eq!(
        header_cells,
        EXPECTED_HEADER.to_vec(),
        "§2.2 table header shape changed — update this test's column assumptions"
    );

    let separator_line = lines
        .next()
        .expect("separator row missing directly after §2.2 table header");
    let separator_cells = split_row_cells(separator_line);
    assert_eq!(
        separator_cells.len(),
        header_cells.len(),
        "§2.2 separator row has {} cells, expected {} (one per header column) — got: {:?}",
        separator_cells.len(),
        header_cells.len(),
        separator_line
    );
    for (i, cell) in separator_cells.iter().enumerate() {
        assert!(
            is_valid_separator_cell(cell),
            "§2.2 separator row cell {i} is not a valid markdown separator (`---`-shaped): {cell:?}"
        );
    }

    let mut rows = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('|') {
            break;
        }
        let cells = split_row_cells(line);
        assert_eq!(
            cells.len(),
            EXPECTED_HEADER.len(),
            "malformed §2.2 table row (expected {} cells): {line:?}",
            EXPECTED_HEADER.len()
        );
        rows.push(MatrixRow {
            triple: Triple {
                product: cells[0].clone(),
                dialect: cells[1].clone(),
                operation: cells[2].clone(),
            },
            disposition: cells[11].clone(),
            status: cells[12].clone(),
            evidence: cells[13].clone(),
        });
    }
    rows
}

/// The charter's own disposition rule (J1-A9): parse/index are `Native` for
/// every dialect on both products; transform is `Native` for `Css` on both
/// products (no H046 weakening), `External` for the four preprocessor
/// dialects; format is `Unsupported` everywhere.
fn expected_disposition(dialect: &str, operation: &str) -> &'static str {
    match operation {
        "parse" | "index" => "Native",
        "transform" => {
            if dialect == "Css" {
                "Native"
            } else {
                "External"
            }
        }
        "format" => "Unsupported",
        other => panic!("unexpected operation {other:?} — not one of parse/index/transform/format"),
    }
}

fn full_universe() -> BTreeSet<Triple> {
    let mut universe = BTreeSet::new();
    for product in PRODUCTS {
        for dialect in DIALECTS {
            for operation in OPERATIONS {
                universe.insert(Triple {
                    product: product.to_string(),
                    dialect: dialect.to_string(),
                    operation: operation.to_string(),
                });
            }
        }
    }
    universe
}

#[test]
fn css_family_matrix_covers_exactly_the_forty_triple_universe() {
    let rows = parse_css_family_matrix();
    assert_eq!(
        rows.len(),
        40,
        "expected exactly 40 rows, found {}",
        rows.len()
    );

    let collected: BTreeSet<Triple> = rows.iter().map(|r| r.triple.clone()).collect();
    assert_eq!(
        collected.len(),
        rows.len(),
        "duplicate (Product, Dialect, Operation) triple found in the §2.2 table"
    );

    let expected = full_universe();
    let missing: Vec<_> = expected.difference(&collected).collect();
    let extra: Vec<_> = collected.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "§2.2 triple set diverges from the exact 40-triple universe — missing: {missing:?}, extra (unexpected): {extra:?}"
    );
}

#[test]
fn css_family_matrix_dispositions_match_the_charter_rule_with_cited_evidence() {
    let rows = parse_css_family_matrix();
    for row in &rows {
        let expected = expected_disposition(&row.triple.dialect, &row.triple.operation);
        assert_eq!(
            row.disposition, expected,
            "triple {} has Disposition {:?}, expected {:?} per J1-A9's rule",
            row.triple, row.disposition, expected
        );
        assert!(
            !row.evidence.trim().is_empty(),
            "triple {} has an empty Evidence cell — every Disposition requires a cited evidence source",
            row.triple
        );
    }
}

#[test]
fn css_family_matrix_native_rows_close_to_a_concrete_status() {
    let rows = parse_css_family_matrix();
    let mut saw_css_transform = 0usize;
    for row in &rows {
        if row.triple.operation == "transform" && row.triple.dialect == "Css" {
            saw_css_transform += 1;
            assert_eq!(
                row.disposition, "Native",
                "triple {} must keep Disposition=Native for Css transform — H046 DEFER is not authority",
                row.triple
            );
        }
        if row.disposition != "Native" {
            continue;
        }
        assert_ne!(
            row.status, "VERIFY",
            "triple {} is Native-dispositioned but its Status is still VERIFY — Native rows must \
             close fully within J1; H046 must not reopen them",
            row.triple
        );
        assert_eq!(
            row.status, "Ratified",
            "triple {} is Native but Status is {:?}, expected the concrete `Ratified` value",
            row.triple, row.status
        );
        assert!(
            !row.evidence.contains("DEFER"),
            "triple {} is Native/Ratified but its Evidence still cites a DEFER weakening: {:?}",
            row.triple,
            row.evidence
        );
    }
    assert_eq!(
        saw_css_transform, 2,
        "expected Vue and Svelte Css transform rows"
    );
}

#[test]
fn css_family_matrix_verify_status_set_equals_exactly_the_non_native_subset() {
    let rows = parse_css_family_matrix();

    let verify_triples: BTreeSet<Triple> = rows
        .iter()
        .filter(|r| r.status == "VERIFY")
        .map(|r| r.triple.clone())
        .collect();
    let expected_verify_triples: BTreeSet<Triple> = rows
        .iter()
        .filter(|r| r.disposition != "Native")
        .map(|r| r.triple.clone())
        .collect();

    let unexpectedly_verify: Vec<_> = verify_triples
        .difference(&expected_verify_triples)
        .collect();
    let unexpectedly_closed: Vec<_> = expected_verify_triples
        .difference(&verify_triples)
        .collect();

    assert!(
        unexpectedly_verify.is_empty(),
        "these triples are Native-dispositioned yet still marked VERIFY: {unexpectedly_verify:?}"
    );
    assert!(
        unexpectedly_closed.is_empty(),
        "these non-Native triples are NOT marked VERIFY: {unexpectedly_closed:?}"
    );

    // 8 External (transform, non-Css, both products) + 10 Unsupported (format,
    // all dialects, both products) = 18 VERIFY; the remaining 22 Native rows
    // (parse×10 + index×10 + Css-transform×2) are Ratified.
    assert_eq!(
        expected_verify_triples.len(),
        18,
        "expected 18 VERIFY rows (all non-Native; Css transform must not be among them)"
    );
    assert_eq!(
        rows.len() - expected_verify_triples.len(),
        22,
        "expected 22 concrete Ratified rows"
    );
}
