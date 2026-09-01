use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    },
};

use walkdir::WalkDir;

static WORKSPACE_SOURCE_SCANS: AtomicUsize = AtomicUsize::new(0);
static WORKSPACE_SOURCE_READS: AtomicUsize = AtomicUsize::new(0);
static SOURCE_CORPUS: OnceLock<SourceCorpus> = OnceLock::new();

#[derive(Debug)]
pub(super) struct SourceCorpus {
    session_production_rs: Vec<(String, String)>,
    workspace_crate_production_rs: Vec<(String, String)>,
}

impl SourceCorpus {
    pub(super) fn session_production_rs(&self) -> &[(String, String)] {
        &self.session_production_rs
    }

    pub(super) fn workspace_crate_production_rs(&self) -> &[(String, String)] {
        &self.workspace_crate_production_rs
    }

    pub(super) fn workspace_crate_source(&self, rel: &str) -> Option<&str> {
        self.workspace_crate_production_rs
            .binary_search_by(|(candidate, _)| candidate.as_str().cmp(rel))
            .ok()
            .map(|index| self.workspace_crate_production_rs[index].1.as_str())
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("source-policy crate must live under <workspace>/crates")
        .to_path_buf()
}

pub(super) fn is_test_source(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || rel.ends_with("/tests.rs")
        || rel.contains("/tests/")
        || rel.contains("/tests_")
}

fn scan_workspace_sources() -> SourceCorpus {
    WORKSPACE_SOURCE_SCANS.fetch_add(1, Ordering::SeqCst);
    let crates_root = workspace_root().join("crates");
    let mut session_production_rs = Vec::new();
    let mut workspace_crate_production_rs = Vec::new();

    for entry in WalkDir::new(&crates_root) {
        let entry = entry.expect("walk workspace crates");
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|ext| ext.to_str()) != Some("rs")
        {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&crates_root)
            .expect("source path must be under workspace crates")
            .to_string_lossy()
            .replace('\\', "/");
        let Some((crate_name, in_crate)) = rel.split_once('/') else {
            continue;
        };
        if !in_crate.starts_with("src/") || is_test_source(&rel) {
            continue;
        }
        let source = std::fs::read_to_string(entry.path())
            .unwrap_or_else(|error| panic!("read {rel}: {error}"));
        WORKSPACE_SOURCE_READS.fetch_add(1, Ordering::SeqCst);
        if crate_name == "verter_session" {
            session_production_rs.push((in_crate.to_string(), source.clone()));
        }
        workspace_crate_production_rs.push((rel, source));
    }
    session_production_rs.sort_by(|left, right| left.0.cmp(&right.0));
    workspace_crate_production_rs.sort_by(|left, right| left.0.cmp(&right.0));
    SourceCorpus {
        session_production_rs,
        workspace_crate_production_rs,
    }
}

pub(super) fn source_corpus() -> &'static SourceCorpus {
    SOURCE_CORPUS.get_or_init(scan_workspace_sources)
}

pub(super) fn session_production_src_files() -> &'static [(String, String)] {
    source_corpus().session_production_rs()
}

pub(super) fn session_production_source(rel: &str) -> Option<&'static str> {
    source_corpus()
        .session_production_rs()
        .binary_search_by(|(candidate, _)| candidate.as_str().cmp(rel))
        .ok()
        .map(|index| source_corpus().session_production_rs()[index].1.as_str())
}

pub(super) fn workspace_crate_production_src_files() -> &'static [(String, String)] {
    source_corpus().workspace_crate_production_rs()
}

pub(super) fn workspace_crate_source_if_present(rel: &str) -> Option<&'static str> {
    source_corpus().workspace_crate_source(rel)
}

pub(super) fn assert_repeated_policy_queries_share_one_source_scan() {
    let first = source_corpus();
    let second = source_corpus();

    assert!(
        std::ptr::eq(first, second),
        "policy queries in one process must share the same immutable corpus"
    );
    assert_eq!(
        WORKSPACE_SOURCE_SCANS.load(Ordering::SeqCst),
        1,
        "the production source tree must be walked exactly once per policy process"
    );
    assert!(!first.session_production_rs().is_empty());
    assert_eq!(
        WORKSPACE_SOURCE_READS.load(Ordering::SeqCst),
        first.workspace_crate_production_rs().len(),
        "each workspace crate production source must be read exactly once while building the corpus"
    );
    assert!(
        first
            .session_production_rs()
            .iter()
            .all(|(_, source)| !source.is_empty()),
        "the shared corpus must retain the source bytes used by policy facts"
    );
    assert!(
        first.workspace_crate_production_rs().len() > first.session_production_rs().len(),
        "the shared corpus must include production sources outside verter_session"
    );
}
