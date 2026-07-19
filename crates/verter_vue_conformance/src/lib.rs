//! `verter_vue_conformance` — hermetic official Vue 3.6 RC compiler
//! conformance goldens.
//!
//! This crate houses the corpus and the VENDORED oracle artifacts produced by
//! the pinned official Vue RC toolchain (`packages/vue-conformance-oracle`,
//! generator `gen-vue-goldens.mjs`, pin authority `vue-golden-lib.mjs`):
//!
//! - `corpus/cases/**.vue` — the seed SFC corpus (one focused feature each).
//! - `corpus/support/` — locally vendored shared files so compilation never
//!   reads outside `corpus/`.
//! - `corpus/goldens/<vue-version>/{vdom,vapor}/**` — the official emitted
//!   render/component module (`.js`), its source map (`.map.json`), and a
//!   per-cell metadata file (`.meta.json`) per case per backend.
//! - `corpus/manifest.json` — case-id → SFC → per-backend artifact paths and
//!   disposition.
//!
//! The library is the typed loader for that tree: manifest/metadata schemas,
//! portable corpus-relative paths, LF-normalized text reads, and SHA-256
//! hashing for artifact freshness assertions. The default test surface is
//! fully hermetic — it reads only committed files and never shells out to
//! Node or loads the live Vue compiler. The opt-in `vue-oracle-live` feature
//! re-runs the JS generator's `--check` mode and fails loudly when Node is
//! unavailable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub mod canon;
pub mod compare;
pub mod sourcemap;

/// Root of the vendored corpus tree (`<crate>/corpus`).
pub fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// Repository workspace root (`<crate>` is `<ws>/crates/verter_vue_conformance`).
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <ws>/crates/verter_vue_conformance")
        .to_path_buf()
}

/// Resolve a corpus-relative POSIX path (the only path shape recorded in the
/// manifest and metadata) to a host path. Portable: never string-concatenates
/// separators.
pub fn corpus_file(corpus_root: &Path, rel_posix: &str) -> PathBuf {
    rel_posix
        .split('/')
        .fold(corpus_root.to_path_buf(), |acc, part| acc.join(part))
}

/// Read a committed text artifact, normalizing CRLF to LF. The corpus is
/// committed with `eol=lf` (`.gitattributes`); normalizing keeps content
/// hashes stable for byte comparisons over checked-out text on any platform.
pub fn read_text_normalized(path: &Path) -> Result<String, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(raw.replace("\r\n", "\n"))
}

/// SHA-256 (lowercase hex) of a text artifact's bytes. The generator records
/// hashes over LF-normalized UTF-8 text; pair with `read_text_normalized`.
pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

/// Every `cases/**.vue` path in the corpus, as sorted corpus-relative POSIX
/// strings (the manifest `sfc` shape).
pub fn case_sfc_paths(corpus_root: &Path) -> Result<BTreeSet<String>, String> {
    let cases_root = corpus_root.join("cases");
    let mut out = BTreeSet::new();
    let mut stack = vec![cases_root.clone()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("read dir {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "vue") {
                let rel = path
                    .strip_prefix(corpus_root)
                    .map_err(|e| format!("strip prefix {}: {e}", path.display()))?;
                let rel_posix = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(rel_posix);
            }
        }
    }
    Ok(out)
}

/// Source-authored identifier provenance for the comparator's alpha
/// classification: every identifier-shaped token appearing anywhere in the
/// SFC source (script, template, styles — a superset is safe: it can only
/// over-mark a name as contract/exact, which is the design's conservative
/// fallback; compiler-generated names like `_hoisted_1`, `t0`, `n0`, `_ctx`
/// never appear in the SFC).
///
/// The official RC source maps ship empty `names` arrays, so maps cannot
/// supply this provenance; the SFC identifier set is the documented
/// substitute.
pub fn authored_identifiers(sfc_source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut current = String::new();
    for ch in sfc_source.chars() {
        let is_ident = ch == '_' || ch == '$' || ch.is_ascii_alphanumeric();
        if is_ident {
            current.push(ch);
        } else if !current.is_empty() {
            out.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.insert(current);
    }
    out
}

/// The two compile backends every case is compiled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Vdom,
    Vapor,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::Vdom, Backend::Vapor];

    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Vdom => "vdom",
            Backend::Vapor => "vapor",
        }
    }
}

/// Per-cell outcome recorded by the oracle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    /// The official compiler emitted a module; `.js` + `.map.json` artifacts exist.
    Compiled,
    /// The official compiler rejected the SFC; only `.meta.json` exists and
    /// `diagnostics` carries the ordered reject sequence.
    Rejected,
}

/// `generator` block shared by the manifest and every metadata file.
#[derive(Debug, Clone, Deserialize)]
pub struct GeneratorInfo {
    pub name: String,
    pub version: u32,
}

/// Per-backend cell of one manifest case.
#[derive(Debug, Clone, Deserialize)]
pub struct BackendCell {
    pub disposition: Disposition,
    /// Corpus-relative POSIX path of the emitted module (`None` when rejected).
    pub golden: Option<String>,
    /// Corpus-relative POSIX path of the source map (`None` when rejected).
    pub map: Option<String>,
    /// Corpus-relative POSIX path of the metadata file (always present).
    pub meta: String,
}

/// One manifest case: case-id → SFC → per-backend artifact paths.
#[derive(Debug, Clone, Deserialize)]
pub struct CaseEntry {
    pub id: String,
    pub sfc: String,
    pub backends: BTreeMap<Backend, BackendCell>,
}

/// `corpus/manifest.json` — the generated case/artifact index.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    pub generator: GeneratorInfo,
    pub vue_version: String,
    pub packages: BTreeMap<String, String>,
    pub cases: Vec<CaseEntry>,
}

impl Manifest {
    pub fn load(corpus_root: &Path) -> Result<Self, String> {
        let path = corpus_root.join("manifest.json");
        let text = read_text_normalized(&path)?;
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
    }
}

/// `source` block of a metadata file: the SFC and its content hash.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceRef {
    pub path: String,
    pub sha256: String,
}

/// `options` block of a metadata file: hash of the canonical options summary
/// plus the summary itself (a version bump or option change is visible).
#[derive(Debug, Clone, Deserialize)]
pub struct OptionsRef {
    pub sha256: String,
    pub summary: serde_json::Value,
}

/// A hashed artifact reference inside a metadata file.
#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactRef {
    pub path: String,
    pub sha256: String,
    /// Byte length (emitted-module artifacts only).
    pub bytes: Option<u64>,
}

/// `artifacts` block of a metadata file (`None` arms when rejected).
#[derive(Debug, Clone, Deserialize)]
pub struct Artifacts {
    pub code: Option<ArtifactRef>,
    pub map: Option<ArtifactRef>,
}

/// One ordered diagnostic emitted by the official compiler for a cell.
#[derive(Debug, Clone, Deserialize)]
pub struct Diagnostic {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    #[serde(default)]
    pub loc: Option<DiagnosticLoc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiagnosticLoc {
    pub line: u32,
    pub column: u32,
}

/// One `from "vue"` named import of the emitted module: helper identity plus
/// the (waivable) alias the compiler chose.
#[derive(Debug, Clone, Deserialize)]
pub struct HelperImport {
    pub imported: String,
    pub alias: String,
}

/// A per-cell `.meta.json` file: schema, all package versions, source and
/// options SHA-256, artifact hashes, backend, disposition, the interleaved
/// diagnostic sequence, helper inventory, and generator version.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoldenMeta {
    pub schema: u32,
    pub case_id: String,
    pub backend: Backend,
    pub generator: GeneratorInfo,
    pub versions: BTreeMap<String, String>,
    pub source: SourceRef,
    pub options: OptionsRef,
    pub artifacts: Artifacts,
    pub disposition: Disposition,
    pub diagnostics: Vec<Diagnostic>,
    pub helpers: Vec<HelperImport>,
}

impl GoldenMeta {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = read_text_normalized(path)?;
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
    }
}

// ---------------------------------------------------------------------------
// Tracked known-divergence dispositions (the parity backlog).
// ---------------------------------------------------------------------------

/// `corpus/known-divergences.json` — the tracked known-divergence
/// dispositions for the seed conformance run. Every cell Verter currently
/// FAILS against the official oracle is listed with its exact divergence
/// signature (the comparator's reason summaries); a cell that starts PASSING
/// with an entry still present is a stale entry and fails the suite (parity
/// improved — remove the entry). Regenerate with
/// `VERTER_CONFORMANCE_UPDATE=1 cargo test -p verter_vue_conformance --tests seed_conformance`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnownDivergences {
    pub schema: u32,
    pub cells: Vec<KnownDivergenceCell>,
}

/// One cell's tracked divergence signature.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnownDivergenceCell {
    #[serde(rename = "case")]
    pub case_id: String,
    pub backend: Backend,
    /// Exact expected comparator reason summaries (`DiffReason::summary()`).
    pub reasons: Vec<String>,
    /// Total number of in-contract differences (≥ reasons.len() when capped).
    pub total: usize,
    /// Curated explanation of the divergence class (the backlog item).
    pub note: String,
}

impl KnownDivergences {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = read_text_normalized(path)?;
        serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))
    }

    pub fn find(&self, case_id: &str, backend: Backend) -> Option<&KnownDivergenceCell> {
        self.cells
            .iter()
            .find(|c| c.case_id == case_id && c.backend == backend)
    }
}
