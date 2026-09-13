//! Corpus adapters: the only place a pinned external workload is located on
//! disk and read.
//!
//! An adapter maps a stable case id (`<framework>/<relative-path-in-corpus>`)
//! to the bytes a probe compiles. It never decides an expectation, never
//! derives an outcome, and never treats the corpus as an oracle: what the
//! upstream project asserts about its own fixtures is not evidence about
//! Verter.
//!
//! There is ONE adapter implementation. A corpus differs only in the DATA it
//! declares — which framework it belongs to, where the workflow checks it out,
//! and which subtree holds its cases — so [`Corpus`] carries that data and
//! owns every mechanic built on it: reading the checkout's own commit,
//! discovering the case inventory, and loading one case's bytes. A second
//! corpus is a second `Corpus` constant, never a second walker, a second
//! Git-state reader, or a second case-id convention.

pub mod svelte_benchmarks;
pub mod vue_benchmarks;

use std::fmt;
use std::path::PathBuf;

#[cfg(feature = "external-corpus")]
use std::path::Path;

use crate::manifest::Framework;

/// Where the probe workflow provisions EVERY pinned corpus, relative to the
/// repository root.
///
/// One directory for the lane rather than one per adapter: where a checkout
/// lands is the workflow's convention, and a corpus declares only which
/// directory inside it is its own.
#[cfg(feature = "external-corpus")]
pub const CHECKOUT_ROOT: &str = ".integration-tests/repos";

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

/// One pinned external corpus: the DATA an adapter declares, and every
/// mechanic built on it.
///
/// `framework` is a case attribute, never a behaviour switch: the only place
/// it is matched on is [`Corpus::is_case_path`], where the file-language
/// registry — the single classification authority — is asked which carrier a
/// path is. Nothing else in this module branches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corpus {
    /// The framework every case of this corpus belongs to.
    pub framework: Framework,
    /// This corpus's directory inside [`CHECKOUT_ROOT`].
    pub directory: &'static str,
    /// The subtree the lane draws its cases from. Everything else in the
    /// corpus (its harness, its lockfile, its results) is
    /// workload-irrelevant.
    pub fixture_root: &'static str,
}

impl Corpus {
    /// The corpus a framework's cases come from.
    ///
    /// Exhaustive over the closed framework set, so a new framework is a
    /// compile error here rather than a lane that silently runs one corpus.
    pub const fn for_framework(framework: Framework) -> Corpus {
        match framework {
            Framework::Vue => vue_benchmarks::CORPUS,
            Framework::Svelte => svelte_benchmarks::CORPUS,
        }
    }

    /// The stable case id of a corpus-relative path.
    pub fn case_id_for(&self, relative_path: &str) -> String {
        format!("{}/{relative_path}", self.framework.as_str())
    }

    /// The corpus-relative path a case id names, checked to belong to this
    /// adapter and to stay inside the corpus.
    pub fn relative_path_of<'a>(&self, case_id: &'a str) -> Result<&'a str, CorpusError> {
        let prefix = format!("{}/", self.framework.as_str());
        let relative = case_id
            .strip_prefix(&prefix)
            .ok_or_else(|| CorpusError::ForeignCaseId {
                case_id: case_id.to_string(),
            })?;
        if relative_path_is_inside(relative) {
            Ok(relative)
        } else {
            Err(CorpusError::UnsafeCaseId {
                case_id: case_id.to_string(),
            })
        }
    }

    /// Whether a corpus path is a case of THIS corpus's carrier language.
    ///
    /// Asked of the language registry rather than matched by extension here:
    /// the registry is the single classification authority, and a hand-rolled
    /// suffix check in a corpus walker is exactly how a second one starts.
    pub fn is_case_path(&self, path: &str) -> bool {
        let resolution = verter_language::LanguageRegistry::global()
            .classify_static(path)
            .static_resolution();
        match self.framework {
            Framework::Vue => resolution.is_vue(),
            Framework::Svelte => resolution.is_svelte(),
        }
    }

    /// The pinned checkout's root on this machine.
    #[cfg(feature = "external-corpus")]
    pub fn checkout_root(&self) -> PathBuf {
        let root = resolve_relative(&workspace_root(), CHECKOUT_ROOT)
            .expect("the checkout root is a fixed relative path");
        resolve_relative(&root, self.directory)
            .expect("a corpus directory is a fixed relative path")
    }

    /// The commit the checkout is actually AT, read from its own Git state.
    ///
    /// The pinned revision is recorded in the manifest and in the workflow,
    /// and every summary artifact republishes it. None of that is evidence
    /// that the bytes the lane compiled came from that commit: a checkout
    /// pointed at a different revision whose case set happens to match the
    /// inventory would publish a revision its cases did not come from, which
    /// corrupts the one fact the lane's evidence is anchored on. Reading the
    /// commit closes that.
    ///
    /// Only a resolvable commit id answers; a checkout whose Git state cannot
    /// be read is reported rather than guessed at.
    #[cfg(feature = "external-corpus")]
    pub fn checkout_revision(&self) -> Result<String, CorpusError> {
        let root = self.checkout_root();
        if !crate::disk::is_directory(&root) {
            return Err(CorpusError::CheckoutMissing { root });
        }
        let git_dir = git_dir(&root)?;
        let head = read_git_file(&git_dir, "HEAD")?;
        let head = head.trim();
        // Detached at the pinned commit, which is what an exact-SHA checkout
        // does.
        if is_commit_id(head) {
            return Ok(head.to_string());
        }
        let reference = head.strip_prefix("ref: ").map(str::trim).ok_or_else(|| {
            CorpusError::RevisionUnreadable {
                root: root.clone(),
                message: "the checkout's HEAD is neither a commit id nor a symbolic ref"
                    .to_string(),
            }
        })?;
        // A loose ref file, then the packed ref table: the two places a
        // checked-out branch's commit can be.
        if let Ok(loose) = read_git_file(&git_dir, reference) {
            let loose = loose.trim();
            if is_commit_id(loose) {
                return Ok(loose.to_string());
            }
        }
        let packed = read_git_file(&git_dir, "packed-refs").unwrap_or_default();
        for line in packed.lines() {
            let Some((commit, name)) = line.split_once(' ') else {
                continue;
            };
            if name.trim() == reference && is_commit_id(commit) {
                return Ok(commit.to_string());
            }
        }
        Err(CorpusError::RevisionUnreadable {
            root,
            message: format!("the checkout's HEAD ref `{reference}` resolves to no commit"),
        })
    }

    /// Every case in the checkout, as sorted stable case ids.
    ///
    /// The inventory is DISCOVERED, never declared by the corpus: the
    /// manifest's own inventory is then checked against this set in both
    /// directions, so a case added or removed upstream fails the lane instead
    /// of silently shrinking or growing what it runs.
    #[cfg(feature = "external-corpus")]
    pub fn discover_case_ids(&self) -> Result<Vec<String>, CorpusError> {
        let root = self.checkout_root();
        if !crate::disk::is_directory(&root) {
            return Err(CorpusError::CheckoutMissing { root });
        }
        let fixtures = resolve_relative(&root, self.fixture_root)
            .expect("a fixture root is a fixed relative path");
        if !crate::disk::is_directory(&fixtures) {
            return Err(CorpusError::CheckoutMissing { root: fixtures });
        }
        let mut out = Vec::new();
        self.collect_carriers(&fixtures, self.fixture_root, &mut out)?;
        out.sort();
        Ok(out)
    }

    #[cfg(feature = "external-corpus")]
    fn collect_carriers(
        &self,
        dir: &Path,
        relative: &str,
        out: &mut Vec<String>,
    ) -> Result<(), CorpusError> {
        let children: Vec<PathBuf> =
            crate::disk::sorted_children(dir).map_err(CorpusError::from)?;
        for child in children {
            let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let child_relative = format!("{relative}/{name}");
            if crate::disk::is_directory(&child) {
                self.collect_carriers(&child, &child_relative, out)?;
            } else if self.is_case_path(&child_relative) {
                out.push(self.case_id_for(&child_relative));
            }
        }
        Ok(())
    }

    /// Read one case's bytes.
    ///
    /// CRLF is normalized to LF so a Windows checkout and a Linux checkout
    /// feed the compiler and the reference producer the same bytes — the
    /// source is also what a structural comparator derives its authored-
    /// identifier set from, so an EOL difference must not reach either side.
    /// It is the normalized text a manifest's per-case digest is taken over,
    /// for the same reason.
    #[cfg(feature = "external-corpus")]
    pub fn load_case(&self, case_id: &str) -> Result<CorpusCase, CorpusError> {
        let relative_path = self.relative_path_of(case_id)?;
        let root = self.checkout_root();
        let path =
            resolve_relative(&root, relative_path).ok_or_else(|| CorpusError::UnsafeCaseId {
                case_id: case_id.to_string(),
            })?;
        if !crate::disk::is_file(&path) {
            return Err(CorpusError::CaseMissing {
                case_id: case_id.to_string(),
                path,
            });
        }
        let raw = crate::disk::read_text(&path).map_err(CorpusError::from)?;
        Ok(CorpusCase {
            case_id: case_id.to_string(),
            relative_path: relative_path.to_string(),
            source: raw.replace("\r\n", "\n"),
        })
    }
}

/// Whether a corpus-relative path stays inside the corpus.
fn relative_path_is_inside(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.contains('\\')
        && relative
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// The checkout's Git directory, following a `gitdir:` pointer file when the
/// checkout is a worktree rather than a plain clone.
#[cfg(feature = "external-corpus")]
fn git_dir(root: &Path) -> Result<PathBuf, CorpusError> {
    let dot_git = root.join(".git");
    if crate::disk::is_directory(&dot_git) {
        return Ok(dot_git);
    }
    if crate::disk::is_file(&dot_git) {
        let pointer = crate::disk::read_text(&dot_git).map_err(CorpusError::from)?;
        if let Some(target) = pointer.trim().strip_prefix("gitdir:") {
            let target = Path::new(target.trim());
            return Ok(if target.is_absolute() {
                target.to_path_buf()
            } else {
                root.join(target)
            });
        }
    }
    Err(CorpusError::RevisionUnreadable {
        root: root.to_path_buf(),
        message: "the checkout carries no readable Git state".to_string(),
    })
}

#[cfg(feature = "external-corpus")]
fn read_git_file(git_dir: &Path, relative: &str) -> Result<String, CorpusError> {
    let path =
        resolve_relative(git_dir, relative).ok_or_else(|| CorpusError::RevisionUnreadable {
            root: git_dir.to_path_buf(),
            message: format!("`{relative}` does not name a path inside the Git directory"),
        })?;
    crate::disk::read_text(&path).map_err(CorpusError::from)
}

/// Whether `value` is a full lowercase commit id.
#[cfg(feature = "external-corpus")]
fn is_commit_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

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
