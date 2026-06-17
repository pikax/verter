//! Reusable Svelte golden topology-diff comparison engine.
//!
//! The NORMALIZED topology golden schema (`NormalizedGolden`), the field-precise
//! divergence enum (`TopologyDivergence`), the structure + helper-call TOPOLOGY
//! diff (`topology_diff`), and the golden loaders (`load_golden` /
//! `load_all_goldens`) are the shared seam every consumer uses to diff a
//! normalized candidate against a committed golden. The engine is importable
//! from one module so each consumer runs the SAME identity + topology comparison
//! rather than its own fork.
//!
//! Today the sole consumer is the Svelte reference-drift gate
//! (`tests/svelte_oracle_harness.rs`), which diffs the committed goldens against
//! the PINNED official Svelte compiler. A Verter-side conformance consumer that
//! diffs VERTER's own emitted output against these goldens is a follow-up for
//! when the native Svelte codegen lands (`svelte-native-compiler-plan.md`).
//!
//! This module is gated behind the `svelte-oracle` Cargo feature, so the DEFAULT
//! canonical run never compiles it; the feature is excluded from the default
//! workspace test set and never enters the default build path. Consumers opt in
//! with `cargo test -p verter_compiler --features svelte-oracle` and import via
//! `verter_compiler::svelte_oracle::{topology_diff, NormalizedGolden, …}`.
//!
//! The diff is normalized STRUCTURE + helper-call TOPOLOGY (helper families, the
//! call sequence, the import set, the export shape, the template skeletons, the
//! scope-hash topology), NOT bytes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// The normalized topology golden schema (mirrors gen-svelte-goldens.mjs).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ImportRow {
    pub source: String,
    pub kind: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExportDefault {
    pub name: String,
    pub params: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TemplateSkeleton {
    pub factory: String,
    pub html: String,
    pub flag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CssTopology {
    pub present: bool,
    pub hash: Option<String>,
    pub code: Option<String>,
}

/// The NORMALIZED helper-topology golden — structure + helper-call topology,
/// NOT bytes. This is the golden unit a normalized candidate is diffed against
/// (today: the committed goldens vs the pinned Svelte compiler).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NormalizedGolden {
    pub slug: String,
    pub backend: String,
    #[serde(rename = "oracleVersion")]
    pub oracle_version: String,
    pub imports: Vec<ImportRow>,
    #[serde(rename = "exportDefault")]
    pub export_default: Option<ExportDefault>,
    #[serde(rename = "helperSequence")]
    pub helper_sequence: Vec<String>,
    #[serde(rename = "helperSet")]
    pub helper_set: Vec<String>,
    #[serde(rename = "helperCounts")]
    pub helper_counts: BTreeMap<String, u32>,
    pub templates: Vec<TemplateSkeleton>,
    pub css: CssTopology,
}

// ---------------------------------------------------------------------------
// The topology diff engine (the shared seam every golden-diff consumer uses).
// ---------------------------------------------------------------------------

/// A single identity / structural / helper-topology divergence between two
/// normalized goldens. Field-precise so the diff names exactly which axis
/// diverged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyDivergence {
    Slug {
        expected: String,
        actual: String,
    },
    OracleVersion {
        expected: String,
        actual: String,
    },
    Backend {
        expected: String,
        actual: String,
    },
    ImportSet {
        expected: Vec<ImportRow>,
        actual: Vec<ImportRow>,
    },
    ExportShape {
        expected: Option<ExportDefault>,
        actual: Option<ExportDefault>,
    },
    HelperSet {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    HelperSequence {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    HelperCounts {
        expected: BTreeMap<String, u32>,
        actual: BTreeMap<String, u32>,
    },
    Templates {
        expected: Vec<TemplateSkeleton>,
        actual: Vec<TemplateSkeleton>,
    },
    Css {
        expected: CssTopology,
        actual: CssTopology,
    },
}

/// Diff two normalized goldens on IDENTITY + STRUCTURE + helper-call TOPOLOGY
/// (NOT bytes). `expected` is the committed oracle golden; `actual` is the
/// candidate normalized against it. Identity is checked FIRST: a candidate
/// paired with the wrong fixture (`slug`) or stamped by a mismatched oracle
/// (`oracle_version`) is a divergence even if its structure happens to match,
/// so a wrong-fixture or stale-stamp candidate can never false-parity. Returns
/// every divergence; an empty Vec is full identity + topology parity.
pub fn topology_diff(
    expected: &NormalizedGolden,
    actual: &NormalizedGolden,
) -> Vec<TopologyDivergence> {
    let mut out = Vec::new();
    if expected.slug != actual.slug {
        out.push(TopologyDivergence::Slug {
            expected: expected.slug.clone(),
            actual: actual.slug.clone(),
        });
    }
    if expected.oracle_version != actual.oracle_version {
        out.push(TopologyDivergence::OracleVersion {
            expected: expected.oracle_version.clone(),
            actual: actual.oracle_version.clone(),
        });
    }
    if expected.backend != actual.backend {
        out.push(TopologyDivergence::Backend {
            expected: expected.backend.clone(),
            actual: actual.backend.clone(),
        });
    }
    if expected.imports != actual.imports {
        out.push(TopologyDivergence::ImportSet {
            expected: expected.imports.clone(),
            actual: actual.imports.clone(),
        });
    }
    if expected.export_default != actual.export_default {
        out.push(TopologyDivergence::ExportShape {
            expected: expected.export_default.clone(),
            actual: actual.export_default.clone(),
        });
    }
    if expected.helper_set != actual.helper_set {
        out.push(TopologyDivergence::HelperSet {
            expected: expected.helper_set.clone(),
            actual: actual.helper_set.clone(),
        });
    }
    if expected.helper_sequence != actual.helper_sequence {
        out.push(TopologyDivergence::HelperSequence {
            expected: expected.helper_sequence.clone(),
            actual: actual.helper_sequence.clone(),
        });
    }
    if expected.helper_counts != actual.helper_counts {
        out.push(TopologyDivergence::HelperCounts {
            expected: expected.helper_counts.clone(),
            actual: actual.helper_counts.clone(),
        });
    }
    if expected.templates != actual.templates {
        out.push(TopologyDivergence::Templates {
            expected: expected.templates.clone(),
            actual: actual.templates.clone(),
        });
    }
    if expected.css != actual.css {
        out.push(TopologyDivergence::Css {
            expected: expected.css.clone(),
            actual: actual.css.clone(),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Golden loading.
// ---------------------------------------------------------------------------

/// Load + parse a single normalized golden JSON file.
pub fn load_golden(path: &Path) -> NormalizedGolden {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse golden {}: {e}", path.display()))
}

/// Recursively collect every `*.json` golden under `dir`, keyed by its path
/// relative to `dir` (the stable identity the topology diff keys on).
pub fn load_all_goldens(dir: &Path) -> BTreeMap<String, NormalizedGolden> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in
            std::fs::read_dir(&d).unwrap_or_else(|e| panic!("read dir {}: {e}", d.display()))
        {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("json") {
                let rel = p
                    .strip_prefix(dir)
                    .expect("under goldens dir")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, load_golden(&p));
            }
        }
    }
    out
}
