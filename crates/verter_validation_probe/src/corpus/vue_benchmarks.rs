//! The `pikax/vue-benchmarks` corpus adapter.
//!
//! The corpus is an external WORKLOAD, never an oracle: this adapter reads its
//! `.vue` fixtures and nothing else. The upstream project's own expectations,
//! its `known-failures.json`, and its benchmark results are not read, not
//! mirrored, and never become an expected output for Verter.
//!
//! The checkout is pinned by commit in `manifest/vue.toml` and provisioned only
//! by the dedicated probe workflow. Every path into it is behind
//! `feature = "external-corpus"`, so the canonical hermetic run neither reads
//! nor requires it.

#[cfg(feature = "external-corpus")]
use std::path::{Path, PathBuf};

use super::CorpusError;
#[cfg(feature = "external-corpus")]
use super::{resolve_relative, CorpusCase};
#[cfg(feature = "external-corpus")]
use crate::disk;
use crate::manifest::Framework;

/// The framework every case of this corpus belongs to.
pub const FRAMEWORK: Framework = Framework::Vue;

/// The fixture subtree the lane draws its cases from. Everything else in the
/// corpus (its harness, its lockfile, its results) is workload-irrelevant.
pub const FIXTURE_ROOT: &str = "tests/confirm/fixtures";

/// Where the workflow checks the pinned corpus out, relative to the
/// repository root.
#[cfg(feature = "external-corpus")]
pub const CHECKOUT_RELATIVE_ROOT: &str = ".integration-tests/repos/vue-benchmarks";

/// The pinned checkout's root on this machine.
#[cfg(feature = "external-corpus")]
pub fn checkout_root() -> PathBuf {
    resolve_relative(&super::workspace_root(), CHECKOUT_RELATIVE_ROOT)
        .expect("the checkout root is a fixed relative path")
}

/// The commit the checkout is actually AT, read from its own Git state.
///
/// The pinned revision is recorded in the manifest and in the workflow, and
/// every summary artifact republishes it. None of that is evidence that the
/// bytes the lane compiled came from that commit: a checkout pointed at a
/// different revision whose fixture set happens to match the inventory would
/// publish a revision its cases did not come from, which corrupts the one fact
/// the lane's evidence is anchored on. Reading the commit closes that.
///
/// Only a resolvable commit id answers; a checkout whose Git state cannot be
/// read is reported rather than guessed at.
#[cfg(feature = "external-corpus")]
pub fn checkout_revision() -> Result<String, CorpusError> {
    let root = checkout_root();
    if !disk::is_directory(&root) {
        return Err(CorpusError::CheckoutMissing { root });
    }
    let git_dir = git_dir(&root)?;
    let head = read_git_file(&git_dir, "HEAD")?;
    let head = head.trim();
    // Detached at the pinned commit, which is what an exact-SHA checkout does.
    if is_commit_id(head) {
        return Ok(head.to_string());
    }
    let reference = head.strip_prefix("ref: ").map(str::trim).ok_or_else(|| {
        CorpusError::RevisionUnreadable {
            root: root.clone(),
            message: "the checkout's HEAD is neither a commit id nor a symbolic ref".to_string(),
        }
    })?;
    // A loose ref file, then the packed ref table: the two places a checked-out
    // branch's commit can be.
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

/// The checkout's Git directory, following a `gitdir:` pointer file when the
/// checkout is a worktree rather than a plain clone.
#[cfg(feature = "external-corpus")]
fn git_dir(root: &Path) -> Result<PathBuf, CorpusError> {
    let dot_git = root.join(".git");
    if disk::is_directory(&dot_git) {
        return Ok(dot_git);
    }
    if disk::is_file(&dot_git) {
        let pointer = disk::read_text(&dot_git).map_err(CorpusError::from)?;
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
    disk::read_text(&path).map_err(CorpusError::from)
}

/// Whether `value` is a full lowercase commit id.
#[cfg(feature = "external-corpus")]
fn is_commit_id(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Every `.vue` case in the checkout, as sorted stable case ids.
///
/// The inventory is DISCOVERED, never declared by the corpus: the manifest's
/// own inventory is then checked against this set in both directions, so a
/// fixture added or removed upstream fails the lane instead of silently
/// shrinking or growing what it runs.
#[cfg(feature = "external-corpus")]
pub fn discover_case_ids() -> Result<Vec<String>, CorpusError> {
    let root = checkout_root();
    if !disk::is_directory(&root) {
        return Err(CorpusError::CheckoutMissing { root });
    }
    let fixtures = resolve_relative(&root, FIXTURE_ROOT).expect("the fixture root is fixed");
    if !disk::is_directory(&fixtures) {
        return Err(CorpusError::CheckoutMissing { root: fixtures });
    }
    let mut out = Vec::new();
    collect_carriers(&fixtures, FIXTURE_ROOT, &mut out)?;
    out.sort();
    Ok(out)
}

/// Whether a corpus path is a case of THIS adapter's carrier language.
///
/// Asked of the language registry rather than matched by extension here: the
/// registry is the single classification authority, and a hand-rolled suffix
/// check in a corpus walker is exactly how a second one starts.
pub fn is_case_path(path: &str) -> bool {
    verter_language::LanguageRegistry::global()
        .classify_static(path)
        .static_resolution()
        .is_vue()
}

#[cfg(feature = "external-corpus")]
fn collect_carriers(dir: &Path, relative: &str, out: &mut Vec<String>) -> Result<(), CorpusError> {
    let children: Vec<PathBuf> = disk::sorted_children(dir).map_err(CorpusError::from)?;
    for child in children {
        let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let child_relative = format!("{relative}/{name}");
        if disk::is_directory(&child) {
            collect_carriers(&child, &child_relative, out)?;
        } else if is_case_path(&child_relative) {
            out.push(case_id_for(&child_relative));
        }
    }
    Ok(())
}

/// The stable case id of a corpus-relative path.
pub fn case_id_for(relative_path: &str) -> String {
    format!("{}/{relative_path}", FRAMEWORK.as_str())
}

/// The corpus-relative path a case id names, checked to belong to this
/// adapter and to stay inside the corpus.
pub fn relative_path_of(case_id: &str) -> Result<&str, CorpusError> {
    let prefix = format!("{}/", FRAMEWORK.as_str());
    let relative = case_id
        .strip_prefix(&prefix)
        .ok_or_else(|| CorpusError::ForeignCaseId {
            case_id: case_id.to_string(),
        })?;
    let inside = !relative.is_empty()
        && !relative.contains('\\')
        && relative
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if inside {
        Ok(relative)
    } else {
        Err(CorpusError::UnsafeCaseId {
            case_id: case_id.to_string(),
        })
    }
}

/// Read one case's bytes.
///
/// CRLF is normalized to LF so a Windows checkout and a Linux checkout feed
/// the compiler and the reference producer the same bytes — the source is also
/// what the structural comparator derives its authored-identifier set from, so
/// an EOL difference must not reach either side.
#[cfg(feature = "external-corpus")]
pub fn load_case(case_id: &str) -> Result<CorpusCase, CorpusError> {
    let relative_path = relative_path_of(case_id)?;
    let root = checkout_root();
    let path = resolve_relative(&root, relative_path).ok_or_else(|| CorpusError::UnsafeCaseId {
        case_id: case_id.to_string(),
    })?;
    if !disk::is_file(&path) {
        return Err(CorpusError::CaseMissing {
            case_id: case_id.to_string(),
            path,
        });
    }
    let raw = disk::read_text(&path).map_err(CorpusError::from)?;
    Ok(CorpusCase {
        case_id: case_id.to_string(),
        relative_path: relative_path.to_string(),
        source: raw.replace("\r\n", "\n"),
    })
}
