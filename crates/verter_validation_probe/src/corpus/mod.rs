//! Corpus adapters: the only place a pinned external workload is located on
//! disk and read.
//!
//! An adapter maps a stable case id (`<framework>/<relative-path-in-corpus>`)
//! to the bytes a probe compiles. It never decides an expectation, never
//! derives an outcome, and never treats the corpus as an oracle: what the
//! upstream project asserts about its own fixtures is not evidence about
//! Verter.

pub mod vue_benchmarks;

use std::fmt;
use std::path::PathBuf;

/// One workload case: its stable id, its corpus-relative path, and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusCase {
    /// `<framework>/<relative-path-in-corpus>`, the manifest's case id.
    pub case_id: String,
    /// The path inside the corpus checkout, POSIX-separated. This is what the
    /// canonical request carries as `identity.filename`.
    pub relative_path: String,
    /// The component text, as UTF-8.
    pub source: String,
}

/// Why a corpus could not be read. Absence is never silently an empty slice:
/// a lane whose corpus is missing fails rather than reporting zero cases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusError {
    /// The pinned checkout is not present.
    CheckoutMissing {
        /// Where the adapter looked.
        root: PathBuf,
    },
    /// A case the manifest inventories does not exist in the checkout.
    CaseMissing {
        /// The case id.
        case_id: String,
        /// Where the adapter looked.
        path: PathBuf,
    },
    /// A case id does not belong to this adapter's framework.
    ForeignCaseId {
        /// The case id.
        case_id: String,
    },
    /// A case id resolves outside the corpus, or is not a relative path.
    UnsafeCaseId {
        /// The case id.
        case_id: String,
    },
    /// The checkout's own commit could not be read, so which revision the
    /// cases came from cannot be established.
    RevisionUnreadable {
        /// The checkout root.
        root: PathBuf,
        /// What could not be resolved.
        message: String,
    },
    /// The checkout could not be walked or a case could not be read.
    Io {
        /// What was being read.
        path: PathBuf,
        /// The operating system's message.
        message: String,
    },
}

impl fmt::Display for CorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CorpusError::CheckoutMissing { root } => write!(
                f,
                "the pinned corpus checkout is absent at {}",
                root.display()
            ),
            CorpusError::CaseMissing { case_id, path } => write!(
                f,
                "inventoried case `{case_id}` does not exist at {}",
                path.display()
            ),
            CorpusError::ForeignCaseId { case_id } => {
                write!(f, "case id `{case_id}` belongs to another framework")
            }
            CorpusError::UnsafeCaseId { case_id } => {
                write!(
                    f,
                    "case id `{case_id}` does not name a path inside the corpus"
                )
            }
            CorpusError::RevisionUnreadable { root, message } => write!(
                f,
                "the pinned corpus checkout at {} does not say which commit it is at: {message}",
                root.display()
            ),
            CorpusError::Io { path, message } => {
                write!(f, "reading {}: {message}", path.display())
            }
        }
    }
}

impl From<crate::disk::DiskError> for CorpusError {
    fn from(error: crate::disk::DiskError) -> Self {
        CorpusError::Io {
            path: error.path,
            message: error.message,
        }
    }
}

impl std::error::Error for CorpusError {}

/// The repository root: this crate is `<root>/crates/verter_validation_probe`.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("crate lives at <root>/crates/verter_validation_probe")
        .to_path_buf()
}

/// Resolve a POSIX corpus-relative path against `root` without ever joining a
/// caller-controlled separator or traversal segment.
#[cfg(feature = "external-corpus")]
pub(crate) fn resolve_relative(root: &std::path::Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.contains('\\') {
        return None;
    }
    let mut out = root.to_path_buf();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        out.push(segment);
    }
    Some(out)
}
