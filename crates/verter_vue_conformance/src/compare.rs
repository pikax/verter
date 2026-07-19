//! The Vue structural-conformance comparator.
//!
//! Compares a Verter-emitted module against a vendored official Vue RC golden
//! across the in-contract dimensions:
//!
//! - **structure** — the canonical AST tree (statement/expression/property
//!   order, operators, arguments, patch flags, block topology),
//! - **identifier** — contract (source-authored/public/member/imported) names
//!   exact; private bindings by `BindingKey` (scope-aware alpha equivalence),
//! - **literal** — string/template/numeric/bigint/regex payloads exactly,
//! - **import** — module sources, imported names, alias classification,
//!   side-effect sequence, attributes,
//! - **comment** — semantic comments (PURE/license/JSDoc/bundler) anchored to
//!   their AST occurrence node,
//! - **diagnostics** — the ordered `(severity, code, message)` sequence —
//!   never source positions.
//!
//! WAIVED (never compared): trivia whitespace/formatting, redundant parens,
//! ordinary comments, quote delimiters, empty statements, private-binding
//! spellings (alpha), and LINE NUMBERS / generated positions generally — the
//! two compilers structure output differently, so positions are cosmetic.
//! Source maps are NOT a conformance dimension: a source map maps its OWN
//! compiler's output, so Verter-map vs official-map is meaningless here.
//! Verter's source-map CORRECTNESS is verified separately by the
//! position-encoding tests, not by golden comparison.
//!
//! Cosmetic-only differences PASS. Any in-contract difference FAILS with
//! structured reasons (`DiffReason { dim, path, detail }`); collection is
//! bounded so one detection axis cannot cascade into noise.

use std::collections::{BTreeMap, BTreeSet};

use crate::canon::{canonicalize_module, BindingKey, Canon, ImportEntry};

/// One comparison input: module code and the ordered diagnostic sequence
/// recorded for the cell.
#[derive(Debug, Clone)]
pub struct ModuleInput {
    pub code: String,
    pub diagnostics: Vec<DiagnosticRow>,
}

/// A severity-ordered diagnostic row (cross-compiler comparable shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRow {
    pub kind: String,
    pub code: Option<String>,
    pub message: String,
}

/// The comparison dimension a reason belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiffDim {
    Import,
    Structure,
    Identifier,
    Literal,
    Comment,
    Diagnostics,
}

impl DiffDim {
    pub fn as_str(self) -> &'static str {
        match self {
            DiffDim::Import => "import",
            DiffDim::Structure => "structure",
            DiffDim::Identifier => "identifier",
            DiffDim::Literal => "literal",
            DiffDim::Comment => "comment",
            DiffDim::Diagnostics => "diagnostics",
        }
    }
}

/// One in-contract difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReason {
    pub dim: DiffDim,
    pub path: String,
    pub detail: String,
}

impl DiffReason {
    pub fn summary(&self) -> String {
        format!("[{}] {} — {}", self.dim.as_str(), self.path, self.detail)
    }
}

/// The comparison verdict: `reasons.is_empty()` ⇒ PASS (cosmetic-only or
/// identical).
#[derive(Debug, Clone)]
pub struct Comparison {
    pub reasons: Vec<DiffReason>,
    /// Total in-contract differences found (≥ reasons.len(); reasons are
    /// truncated at the caller's cap).
    pub total: usize,
}

impl Comparison {
    pub fn passed(&self) -> bool {
        self.total == 0
    }
}

/// Hard failure before comparison (unparseable module, broken semantic
/// analysis, malformed map) — always in-contract, never swallowed.
#[derive(Debug, Clone)]
pub struct CompareError(pub String);

impl std::fmt::Display for CompareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CompareError {}

/// Compare two modules. `authored` is the SFC identifier set (source-authored
/// provenance — see `canon` docs). `max_reasons` bounds the collected reasons
/// (`total` still counts everything found).
pub fn compare_modules(
    verter: &ModuleInput,
    golden: &ModuleInput,
    authored: &BTreeSet<String>,
    max_reasons: usize,
) -> Result<Comparison, CompareError> {
    let verter_canon = canonicalize_module(&verter.code, authored)
        .map_err(|e| CompareError(format!("verter module: {e}")))?;
    let golden_canon = canonicalize_module(&golden.code, authored)
        .map_err(|e| CompareError(format!("golden module: {e}")))?;

    let mut reasons = Vec::new();
    let mut total = 0usize;

    diff_imports(
        &verter_canon.imports,
        &golden_canon.imports,
        &mut reasons,
        &mut total,
        max_reasons,
    );
    diff_canon(
        &verter_canon.tree,
        &golden_canon.tree,
        &mut String::new(),
        &mut reasons,
        &mut total,
        max_reasons,
    );
    diff_diagnostics(
        &verter.diagnostics,
        &golden.diagnostics,
        &mut reasons,
        &mut total,
        max_reasons,
    );

    Ok(Comparison { reasons, total })
}

fn push_reason(
    reasons: &mut Vec<DiffReason>,
    total: &mut usize,
    max: usize,
    dim: DiffDim,
    path: &str,
    detail: String,
) {
    *total += 1;
    if reasons.len() < max {
        reasons.push(DiffReason {
            dim,
            path: path.to_string(),
            detail,
        });
    }
}

// ---------------------------------------------------------------------------
// Canonical-tree diff.
// ---------------------------------------------------------------------------

fn diff_canon(
    verter: &Canon,
    golden: &Canon,
    path: &mut String,
    reasons: &mut Vec<DiffReason>,
    total: &mut usize,
    max: usize,
) {
    diff_canon_ctx(verter, golden, path, reasons, total, max, false);
}

/// `comment_ctx` is set inside `comment(s)` subtrees so every reason under a
/// semantic-comment anchor lands on the Comment dim.
fn diff_canon_ctx(
    verter: &Canon,
    golden: &Canon,
    path: &mut String,
    reasons: &mut Vec<DiffReason>,
    total: &mut usize,
    max: usize,
    comment_ctx: bool,
) {
    match (verter, golden) {
        (Canon::Node(verter_kind, verter_children), Canon::Node(golden_kind, golden_children)) => {
            let base_len = path.len();
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(golden_kind);
            let is_comment_node = *golden_kind == "comment" || *golden_kind == "comments";
            let comment_ctx = comment_ctx || is_comment_node;
            let structure_dim = if comment_ctx {
                DiffDim::Comment
            } else {
                DiffDim::Structure
            };
            if verter_kind != golden_kind {
                push_reason(
                    reasons,
                    total,
                    max,
                    structure_dim,
                    path,
                    format!("node kind: verter `{verter_kind}` vs golden `{golden_kind}`"),
                );
                path.truncate(base_len);
                return;
            }
            if verter_children.len() != golden_children.len() {
                // A child-count delta caused purely by the presence/absence
                // of anchored `comments` children is a COMMENT difference
                // (dropped/moved semantic comment), not a structure one.
                let (longer, dim) = if verter_children.len() > golden_children.len() {
                    (&verter_children[golden_children.len()..], DiffDim::Comment)
                } else {
                    (&golden_children[verter_children.len()..], DiffDim::Comment)
                };
                let only_comments = !longer.is_empty()
                    && longer
                        .iter()
                        .all(|c| matches!(c, Canon::Node("comments", _)));
                let dim = if only_comments { dim } else { structure_dim };
                push_reason(
                    reasons,
                    total,
                    max,
                    dim,
                    path,
                    format!(
                        "child count: verter {} vs golden {}",
                        verter_children.len(),
                        golden_children.len()
                    ),
                );
                path.truncate(base_len);
                return;
            }
            for (index, (verter_child, golden_child)) in
                verter_children.iter().zip(golden_children).enumerate()
            {
                let child_base = path.len();
                let suffix = format!("[{index}]");
                path.push_str(&suffix);
                diff_canon_ctx(
                    verter_child,
                    golden_child,
                    path,
                    reasons,
                    total,
                    max,
                    comment_ctx,
                );
                path.truncate(child_base);
            }
            path.truncate(base_len);
        }
        (Canon::Leaf(verter_kind, verter_value), Canon::Leaf(golden_kind, golden_value)) => {
            if verter_kind != golden_kind || verter_value != golden_value {
                let dim = if comment_ctx {
                    DiffDim::Comment
                } else if *verter_kind == "ident" || *golden_kind == "ident" {
                    DiffDim::Identifier
                } else {
                    DiffDim::Literal
                };
                let detail = if verter_kind == golden_kind {
                    format!("{verter_kind}: verter `{verter_value}` vs golden `{golden_value}`")
                } else {
                    format!(
                        "leaf kind: verter {verter_kind} `{verter_value}` vs golden {golden_kind} `{golden_value}`"
                    )
                };
                push_reason(reasons, total, max, dim, path, detail);
            }
        }
        (Canon::Alpha(verter_key), Canon::Alpha(golden_key)) => {
            if verter_key != golden_key {
                push_reason(
                    reasons,
                    total,
                    max,
                    DiffDim::Identifier,
                    path,
                    format!(
                        "private binding key: verter {} vs golden {}",
                        render_key(verter_key),
                        render_key(golden_key)
                    ),
                );
            }
        }
        (
            Canon::ImportBinding {
                source: verter_source,
                imported: verter_imported,
            },
            Canon::ImportBinding {
                source: golden_source,
                imported: golden_imported,
            },
        ) => {
            if verter_source != golden_source || verter_imported != golden_imported {
                push_reason(
                    reasons,
                    total,
                    max,
                    DiffDim::Import,
                    path,
                    format!(
                        "helper/import family: verter `{verter_imported}` from `{verter_source}` \
                         vs golden `{golden_imported}` from `{golden_source}`"
                    ),
                );
            }
        }
        (Canon::ImportBinding { .. }, other) | (other, Canon::ImportBinding { .. }) => {
            let side = match other {
                Canon::Leaf(kind, value) => format!("exact `{kind}` `{value}`"),
                Canon::Alpha(_) => "private binding".to_string(),
                Canon::Node(kind, _) => format!("node `{kind}`"),
                Canon::ImportBinding { .. } => unreachable!(),
            };
            push_reason(
                reasons,
                total,
                max,
                DiffDim::Identifier,
                path,
                format!("one side is an import binding, the other {side}"),
            );
        }
        (Canon::Alpha(_), Canon::Leaf(kind, value))
        | (Canon::Leaf(kind, value), Canon::Alpha(_)) => {
            push_reason(
                reasons,
                total,
                max,
                DiffDim::Identifier,
                path,
                format!("one side is a private binding, the other the exact `{kind}` `{value}`"),
            );
        }
        (Canon::Node(kind, _), Canon::Leaf(leaf_kind, value))
        | (Canon::Leaf(leaf_kind, value), Canon::Node(kind, _)) => {
            push_reason(
                reasons,
                total,
                max,
                DiffDim::Structure,
                path,
                format!("node kind `{kind}` vs leaf `{leaf_kind}` `{value}`"),
            );
        }
        (Canon::Node(kind, _), Canon::Alpha(_)) | (Canon::Alpha(_), Canon::Node(kind, _)) => {
            push_reason(
                reasons,
                total,
                max,
                DiffDim::Structure,
                path,
                format!("node kind `{kind}` vs private binding"),
            );
        }
    }
}

fn render_key(key: &BindingKey) -> String {
    format!(
        "(scope#{}, decl#{}, slot{:?}, {:?})",
        key.scope_ordinal, key.declaration_ordinal, key.pattern_slot, key.kind
    )
}

// ---------------------------------------------------------------------------
// Import topology diff.
//
// Declaration GROUPING is cosmetic (specifier imports are hoisted ESM
// bindings): declarations are merged per module source and compared as sets
// of imported names (alias canon via `ImportBinding` — family identity). The
// side-effect import SEQUENCE is contract and compared in order.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MergedImport<'a> {
    default: Option<&'a Canon>,
    namespace: Option<&'a Canon>,
    named: BTreeMap<&'a str, &'a Canon>,
    attributes: BTreeSet<(String, String)>,
    side_effect: bool,
}

fn merge_imports<'a>(
    entries: &'a [ImportEntry],
) -> (BTreeMap<&'a str, MergedImport<'a>>, Vec<&'a str>) {
    let mut merged: BTreeMap<&str, MergedImport> = BTreeMap::new();
    let mut side_effect_sequence = Vec::new();
    for entry in entries {
        if entry.side_effect {
            side_effect_sequence.push(entry.source.as_str());
        }
        let merged_entry = merged.entry(entry.source.as_str()).or_default();
        merged_entry.side_effect |= entry.side_effect;
        if let Some(default) = &entry.default {
            merged_entry.default = Some(default);
        }
        if let Some(namespace) = &entry.namespace {
            merged_entry.namespace = Some(namespace);
        }
        for (imported, alias) in &entry.named {
            merged_entry.named.insert(imported.as_str(), alias);
        }
        for attribute in &entry.attributes {
            merged_entry.attributes.insert(attribute.clone());
        }
    }
    (merged, side_effect_sequence)
}

fn diff_imports(
    verter: &[ImportEntry],
    golden: &[ImportEntry],
    reasons: &mut Vec<DiffReason>,
    total: &mut usize,
    max: usize,
) {
    let (verter_merged, verter_side_effect) = merge_imports(verter);
    let (golden_merged, golden_side_effect) = merge_imports(golden);

    if verter_side_effect != golden_side_effect {
        push_reason(
            reasons,
            total,
            max,
            DiffDim::Import,
            "/imports/side-effect",
            format!(
                "side-effect import sequence: verter {verter_side_effect:?} vs golden {golden_side_effect:?}"
            ),
        );
    }

    let all_sources: BTreeSet<&str> = verter_merged
        .keys()
        .chain(golden_merged.keys())
        .copied()
        .collect();
    for source in all_sources {
        let path = format!("/imports({source})");
        match (verter_merged.get(source), golden_merged.get(source)) {
            (None, Some(_)) => push_reason(
                reasons,
                total,
                max,
                DiffDim::Import,
                &path,
                "missing import source".to_string(),
            ),
            (Some(_), None) => push_reason(
                reasons,
                total,
                max,
                DiffDim::Import,
                &path,
                "unexpected import source".to_string(),
            ),
            (Some(verter_entry), Some(golden_entry)) => {
                if verter_entry.side_effect != golden_entry.side_effect {
                    push_reason(
                        reasons,
                        total,
                        max,
                        DiffDim::Import,
                        &path,
                        format!(
                            "side-effect flag: verter {} vs golden {}",
                            verter_entry.side_effect, golden_entry.side_effect
                        ),
                    );
                }
                diff_import_alias(
                    verter_entry.default,
                    golden_entry.default,
                    "default",
                    &path,
                    reasons,
                    total,
                    max,
                );
                diff_import_alias(
                    verter_entry.namespace,
                    golden_entry.namespace,
                    "namespace",
                    &path,
                    reasons,
                    total,
                    max,
                );
                let verter_named: BTreeSet<&str> = verter_entry.named.keys().copied().collect();
                let golden_named: BTreeSet<&str> = golden_entry.named.keys().copied().collect();
                for imported in &verter_named {
                    if !golden_named.contains(imported) {
                        push_reason(
                            reasons,
                            total,
                            max,
                            DiffDim::Import,
                            &path,
                            format!("unexpected imported helper/name `{imported}`"),
                        );
                    }
                }
                for imported in &golden_named {
                    if !verter_named.contains(imported) {
                        push_reason(
                            reasons,
                            total,
                            max,
                            DiffDim::Import,
                            &path,
                            format!("missing imported helper/name `{imported}`"),
                        );
                    }
                }
                // Alias canon for shared imported names (family identity —
                // both must reference the same (source, imported) family).
                for (imported, golden_alias) in &golden_entry.named {
                    if let Some(verter_alias) = verter_entry.named.get(imported) {
                        let mut alias_path = format!("{path}/{imported}");
                        diff_canon(
                            verter_alias,
                            golden_alias,
                            &mut alias_path,
                            reasons,
                            total,
                            max,
                        );
                    }
                }
                if verter_entry.attributes != golden_entry.attributes {
                    push_reason(
                        reasons,
                        total,
                        max,
                        DiffDim::Import,
                        &path,
                        format!(
                            "import attributes: verter {:?} vs golden {:?}",
                            verter_entry.attributes, golden_entry.attributes
                        ),
                    );
                }
            }
            (None, None) => unreachable!(),
        }
    }
}

fn diff_import_alias(
    verter: Option<&Canon>,
    golden: Option<&Canon>,
    label: &str,
    path: &str,
    reasons: &mut Vec<DiffReason>,
    total: &mut usize,
    max: usize,
) {
    match (verter, golden) {
        (Some(verter), Some(golden)) => {
            let mut alias_path = format!("{path}/{label}");
            diff_canon(verter, golden, &mut alias_path, reasons, total, max);
        }
        (None, None) => {}
        (verter, golden) => push_reason(
            reasons,
            total,
            max,
            DiffDim::Import,
            path,
            format!(
                "{label} import presence: verter {} vs golden {}",
                verter.is_some(),
                golden.is_some()
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// Diagnostics diff (ordered sequence).
// ---------------------------------------------------------------------------

fn diff_diagnostics(
    verter: &[DiagnosticRow],
    golden: &[DiagnosticRow],
    reasons: &mut Vec<DiffReason>,
    total: &mut usize,
    max: usize,
) {
    if verter.len() != golden.len() {
        push_reason(
            reasons,
            total,
            max,
            DiffDim::Diagnostics,
            "/diagnostics",
            format!("count: verter {} vs golden {}", verter.len(), golden.len()),
        );
    }
    for (index, (verter_row, golden_row)) in verter.iter().zip(golden.iter()).enumerate() {
        if verter_row != golden_row {
            push_reason(
                reasons,
                total,
                max,
                DiffDim::Diagnostics,
                &format!("/diagnostics[{index}]"),
                format!(
                    "verter ({}, {:?}, {:?}) vs golden ({}, {:?}, {:?})",
                    verter_row.kind,
                    verter_row.code,
                    verter_row.message,
                    golden_row.kind,
                    golden_row.code,
                    golden_row.message,
                ),
            );
        }
    }
}
