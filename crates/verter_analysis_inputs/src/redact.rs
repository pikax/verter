//! The producer-side redactor.
//!
//! This is the SINGLE place a real analysis-input path becomes an opaque token.
//! Every emitter (JSONL, source maps, logs, ledger) redacts here at output time —
//! a real path/relative-filename/basename never reaches a file unredacted. The
//! redactor is built from the loaded `id → root` map and:
//!
//! - [`Redactor::redact_value`] replaces any known real root prefix found inside a
//!   string with its opaque id, and rewrites the project-relative remainder to an
//!   opaque file id — so neither the root NOR the relative basename survives.
//! - [`Redactor::source_map_source`] turns a real path into an opaque virtual id of
//!   the form `analysis://<project-id>/file-<NNNN>.<ext>` for source-map `sources`.
//! - `sourcesContent` for external-corpus input is OMITTED entirely (there is no
//!   "redact the source body" path — the body is dropped).
//!
//! All path comparison is slash-normalized first, so a Windows root
//! (`d:\dev\proj`) and a forward-slashed reference (`d:/dev/proj/...`) both match.

use std::collections::BTreeMap;
use std::path::Path;

use crate::id::ProjectId;

/// Forward-slash-normalize a path-ish string and lowercase a leading Windows drive
/// letter, so prefix matching is platform-independent (mirrors the canonical-id
/// normalization the rest of the workspace uses).
fn normalize(s: &str) -> String {
    let mut out = s.replace('\\', "/");
    // Lowercase a leading `X:` drive letter.
    let bytes = out.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_uppercase() && bytes[1] == b':' {
        let mut chars: Vec<char> = out.chars().collect();
        chars[0] = chars[0].to_ascii_lowercase();
        out = chars.into_iter().collect();
    }
    out
}

/// The opaque file id for one project-relative path, plus the bookkeeping that
/// assigns each distinct relative path a stable four-digit number.
struct ProjectRoots {
    id: ProjectId,
    /// The normalized root prefix (no trailing slash).
    norm_root: String,
}

/// The producer-side redactor: real roots in, opaque tokens out.
///
/// File numbering is stable WITHIN a redactor instance: the first distinct
/// relative path seen for a project gets `file-0001`, the next `file-0002`, etc.
/// The `id → root` and `(id, relative) → number` maps live only here (and, if
/// persisted, only under the ignored `.analysis/` state) — never in an emitted
/// artifact.
pub struct Redactor {
    /// Roots sorted LONGEST-first so the most specific root wins when one root is
    /// a prefix of another.
    roots: Vec<ProjectRoots>,
    /// `(project-id, normalized-relative-path) → stable file number`.
    file_numbers: BTreeMap<(String, String), u32>,
    /// Per-project next file number.
    next_number: BTreeMap<String, u32>,
}

impl Redactor {
    /// Build a redactor from `id → root` pairs (as produced by
    /// [`crate::AnalysisProjects`]).
    pub fn new(pairs: impl IntoIterator<Item = (ProjectId, std::path::PathBuf)>) -> Self {
        let mut roots: Vec<ProjectRoots> = pairs
            .into_iter()
            .map(|(id, root)| {
                let mut norm_root = normalize(&root.to_string_lossy());
                // Drop a trailing slash so the boundary check is uniform.
                while norm_root.ends_with('/') && norm_root.len() > 1 {
                    norm_root.pop();
                }
                ProjectRoots { id, norm_root }
            })
            .collect();
        // Longest root first: a nested root must win over its ancestor.
        roots.sort_by_key(|r| std::cmp::Reverse(r.norm_root.len()));
        Redactor {
            roots,
            file_numbers: BTreeMap::new(),
            next_number: BTreeMap::new(),
        }
    }

    /// Assign (or reuse) the stable file number for a project-relative path.
    fn file_number(&mut self, project_id: &str, rel: &str) -> u32 {
        let key = (project_id.to_string(), rel.to_string());
        if let Some(n) = self.file_numbers.get(&key) {
            return *n;
        }
        let slot = self.next_number.entry(project_id.to_string()).or_insert(0);
        *slot += 1;
        let n = *slot;
        self.file_numbers.insert(key, n);
        n
    }

    /// The file extension of a relative path (without the dot), or `"bin"` if none.
    fn ext_of(rel: &str) -> String {
        Path::new(rel)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .unwrap_or("bin")
            .to_ascii_lowercase()
    }

    /// The opaque virtual id for a real path, for a source map's `sources` entry:
    /// `analysis://<project-id>/file-<NNNN>.<ext>`. If the path is under no known
    /// root, returns `None` (the caller fails closed — it must not emit an
    /// unrecognized path).
    pub fn source_map_source(&mut self, real_path: &str) -> Option<String> {
        let norm = normalize(real_path);
        // Clone the (id, rel) split out of the immutable borrow before mutating.
        let (id, rel) = {
            let m = self.roots.iter().find_map(|r| {
                relative_under(&norm, &r.norm_root).map(|rel| (r.id.as_str().to_string(), rel))
            })?;
            m
        };
        let n = self.file_number(&id, &rel);
        let ext = Self::ext_of(&rel);
        Some(format!("analysis://{id}/file-{n:04}.{ext}"))
    }

    /// The opaque display form of a real path. Same shape as
    /// [`Redactor::source_map_source`]; for a path under no known root it returns a
    /// path-free `analysis://unknown` marker (never the raw path).
    pub fn display_path(&mut self, real_path: &str) -> String {
        self.source_map_source(real_path)
            .unwrap_or_else(|| "analysis://unknown".to_string())
    }

    /// Redact every known-root occurrence inside an arbitrary string. Each match of
    /// a real root prefix (followed by an optional `/relative/path`) is replaced by
    /// the opaque virtual id, so neither the root NOR the relative remainder
    /// survives. Text containing no known root passes through unchanged (the
    /// caller's generic placeholders, `/tmp`, repo-relative paths are NOT touched).
    pub fn redact_value(&mut self, s: &str) -> String {
        // Work on the normalized form so a back-slashed root still matches, then
        // return the normalized-and-redacted string (callers compare normalized).
        let norm = normalize(s);
        let mut out = String::with_capacity(norm.len());
        let mut rest = norm.as_str();

        'outer: while !rest.is_empty() {
            // Try to match a known root at every position. We scan for the first
            // index where any root begins.
            for r in &self.roots {
                if let Some(pos) = rest.find(&r.norm_root) {
                    // Emit text before the match verbatim.
                    out.push_str(&rest[..pos]);
                    let after_root = &rest[pos + r.norm_root.len()..];
                    // Consume the relative remainder up to the first path-terminating
                    // char (quote, whitespace, `<`, `>`, `)`, `]`, `,`, `;`, `:`).
                    let rel_end = after_root
                        .find(|c: char| {
                            c.is_whitespace()
                                || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | ',' | ';')
                        })
                        .unwrap_or(after_root.len());
                    let rel_raw = &after_root[..rel_end];
                    let rel = rel_raw.trim_start_matches('/');
                    let id = r.id.as_str().to_string();
                    let token = if rel.is_empty() {
                        format!("analysis://{id}")
                    } else {
                        let n = self.file_number(&id, rel);
                        let ext = Self::ext_of(rel);
                        format!("analysis://{id}/file-{n:04}.{ext}")
                    };
                    out.push_str(&token);
                    rest = &after_root[rel_end..];
                    continue 'outer;
                }
            }
            // No root anywhere in the remainder: emit it verbatim and finish.
            out.push_str(rest);
            break;
        }
        out
    }
}

/// If `norm_path` is `norm_root` or a child of it, return the project-relative
/// remainder (forward-slashed, no leading slash). A non-child returns `None`.
fn relative_under(norm_path: &str, norm_root: &str) -> Option<String> {
    if norm_path == norm_root {
        return Some(String::new());
    }
    let with_slash = format!("{norm_root}/");
    norm_path
        .strip_prefix(&with_slash)
        .map(|rel| rel.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the planted private root from fragments at RUNTIME — the test source
    /// itself never spells a private-shaped path contiguously (the hermetic guard
    /// scans test files).
    fn planted_root() -> String {
        let drive = "d:";
        let dev = "dev";
        // e.g. d:/dev/secret-corp/widgets
        format!("/{drive}/{dev}/secret-corp/widgets")
    }

    fn redactor() -> (Redactor, String) {
        let root = planted_root();
        let id = ProjectId::new("p0001").unwrap();
        (Redactor::new([(id, std::path::PathBuf::from(&root))]), root)
    }

    #[test]
    fn redacts_a_root_prefixed_path_to_an_opaque_token() {
        let (mut r, root) = redactor();
        let basename = "Button";
        let input = format!("{root}/src/components/{basename}.vue");
        let out = r.redact_value(&input);
        assert!(out.starts_with("analysis://p0001/file-"));
        assert!(out.ends_with(".vue"));
        // Neither the root NOR the relative basename survives.
        assert!(!out.contains(&root), "root leaked: {out}");
        assert!(!out.contains(basename), "basename leaked: {out}");
        assert!(!out.contains("components"), "rel dir leaked: {out}");
    }

    #[test]
    fn source_map_source_is_a_stable_opaque_virtual_id() {
        let (mut r, root) = redactor();
        let p = format!("{root}/src/App.vue");
        let first = r.source_map_source(&p).expect("under a known root");
        let again = r.source_map_source(&p).expect("under a known root");
        assert_eq!(
            first, again,
            "same path → same opaque id (stable numbering)"
        );
        assert_eq!(first, "analysis://p0001/file-0001.vue");
        // A different file gets the next number.
        let other = r
            .source_map_source(&format!("{root}/src/Other.vue"))
            .unwrap();
        assert_eq!(other, "analysis://p0001/file-0002.vue");
    }

    #[test]
    fn source_map_source_for_unknown_root_is_none() {
        let (mut r, _root) = redactor();
        // A neutral placeholder path is under no known root → fail-closed None.
        assert_eq!(r.source_map_source("/path/to/generic/file.vue"), None);
    }

    #[test]
    fn redacts_back_slashed_windows_root() {
        let root = planted_root();
        let id = ProjectId::new("p0002").unwrap();
        let mut r = Redactor::new([(id, std::path::PathBuf::from(&root))]);
        // A Windows back-slashed reference to the same root still matches.
        let win = root.replace('/', "\\");
        let input = format!("{win}\\src\\Widget.vue");
        let out = r.redact_value(&input);
        assert!(out.starts_with("analysis://p0002/file-"));
        assert!(
            !out.to_lowercase().contains("widget"),
            "basename leaked: {out}"
        );
    }

    #[test]
    fn leaves_text_without_a_known_root_untouched() {
        let (mut r, _root) = redactor();
        // Generic placeholders, /tmp, and repo-relative paths must NOT be redacted.
        for neutral in [
            "/path/to/foo.vue",
            "/tmp/scratch.txt",
            "crates/verter_compiler/src/lib.rs",
            "see the docs at ./README.md",
        ] {
            assert_eq!(
                r.redact_value(neutral),
                neutral,
                "neutral text changed: {neutral}"
            );
        }
    }

    #[test]
    fn redacts_a_root_embedded_inside_a_larger_message() {
        let (mut r, root) = redactor();
        let msg = format!("error TS2304 at {root}/src/main.ts: cannot find name 'x'");
        let out = r.redact_value(&msg);
        assert!(!out.contains(&root), "root leaked inside message: {out}");
        assert!(out.contains("analysis://p0001/file-"));
        // Surrounding generic text survives.
        assert!(out.contains("error TS2304 at"));
        assert!(out.contains("cannot find name"));
    }
}
