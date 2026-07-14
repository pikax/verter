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

/// True iff `c` can be inside a single path SEGMENT (between slashes) — used for
/// name-segment validation. Mirrors the hermetic leak guard's `is_path_seg_char`
/// EXACTLY (stops at whitespace, `/`, and the quote/angle-bracket terminators), so
/// the redactor and the guard agree on what a name segment is.
fn is_path_seg_char(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '/' | '"' | '\'' | '<' | '>'))
}

/// True iff `c` can be inside a contiguous path RUN (a whole path, slashes included)
/// when CONSUMING a confirmed private shape. A path in prose is bounded only by
/// whitespace and quote/angle-bracket delimiters — NOT by `,`/`;`/`)`/`]`, which are
/// LEGAL filename bytes. Consuming through them is FAIL-CLOSED: once the leading root
/// is confirmed private, every following path byte (up to a true prose boundary) is
/// private and must be swallowed, so no basename tail can survive.
fn is_run_char(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
}

/// True iff a shape may BEGIN at byte `start` of `norm`: a word boundary, i.e.
/// start-of-string OR the preceding byte is not an ASCII word char. This mirrors the
/// hermetic leak guard's `\b` / `(^|[^A-Za-z0-9_])` boundary so a repo-relative
/// `src/Users/...` (where `/Users` is preceded by the `c` of `src`) or a mid-word
/// `foox:/dev/...` is NOT a false-positive shape.
fn boundary_ok(norm: &str, start: usize) -> bool {
    if start == 0 {
        return true;
    }
    // The byte immediately before `start`. `norm` is the (slash-normalized) text;
    // `start` is always a char boundary here, so `start - 1` is the previous byte.
    let prev = norm.as_bytes()[start - 1];
    !(prev.is_ascii_alphanumeric() || prev == b'_')
}

/// The byte length of an unknown-root ABSOLUTE-path shape starting at byte `start`
/// of the already slash-normalized `norm`, or `None` if no shape starts there. The
/// shapes mirror the hermetic leak guard's producer-boundary net (including its
/// word-boundary requirement):
///
/// - a Windows drive root into `Users/<seg>` or `dev/` (`x:/Users/…`, `x:/dev/…`),
///   the drive letter at a word boundary
/// - a POSIX home root (`/Users/<name>/…`, `/home/<name>/…`), the leading `/` at a
///   word boundary
/// - a `file:///Users/…` / `file:///home/…` URI
///
/// Neutral placeholders (`/path/to/…`, `/tmp/…`), repo-relative paths (incl.
/// `src/Users/…`), and bare words are NOT shapes, so they pass through untouched.
fn unknown_shape_len(norm: &str, start: usize) -> Option<usize> {
    let rest = &norm[start..];
    let lower = rest.to_ascii_lowercase();

    // file:///Users/... or file:///home/... — matched on the MARKER directly, exactly
    // as the hermetic leak guard's `absolute_path_shape` does (`file:///users/` or
    // `file:///home/` present is itself the shape). The whole run is then consumed, so
    // `file:///Users/alice` (no trailing slash) is redacted, not left verbatim.
    for marker in ["file:///users/", "file:///home/"] {
        if lower.starts_with(marker) {
            return Some(shape_run_len(rest));
        }
    }

    // Windows drive root: `x:/Users/<seg>` or `x:/dev/`, drive letter at a boundary.
    let rb = rest.as_bytes();
    if boundary_ok(norm, start)
        && rb.len() >= 3
        && rb[0].is_ascii_alphabetic()
        && rb[1] == b':'
        && rb[2] == b'/'
    {
        let after = &lower[3..];
        if after.starts_with("dev/") {
            return Some(shape_run_len(rest));
        }
        if after.starts_with("users/") {
            // require a real name segment after `Users/`
            if rest[3 + 6..].chars().next().is_some_and(is_path_seg_char) {
                return Some(shape_run_len(rest));
            }
        }
    }

    // POSIX home root at this position: `/Users/<name>/` or `/home/<name>/`, the
    // leading `/` at a word boundary (so `src/Users/…` is NOT matched here).
    if boundary_ok(norm, start) {
        for (kw, kwlen) in [("/users/", 7usize), ("/home/", 6usize)] {
            if lower.starts_with(kw) {
                let name = &rest[kwlen..];
                // require a non-empty name segment terminated by `/`
                if let Some(slash) = name.find('/') {
                    if slash > 0 && name[..slash].chars().all(is_path_seg_char) {
                        return Some(shape_run_len(rest));
                    }
                }
            }
        }
    }

    None
}

/// The byte length of the contiguous path run starting at the beginning of `s` — the
/// whole confirmed-private path, consuming slashes and all legal filename bytes
/// (incl. `,`/`;`/`)`/`]`) up to the first PROSE boundary (whitespace or a
/// quote/angle bracket). This is what makes the redaction FAIL-CLOSED: a basename
/// after a `,` (`/Users/a/My,Docs/Secret.ts`) is swallowed, not left dangling.
fn shape_run_len(s: &str) -> usize {
    s.char_indices()
        .find(|&(_, c)| !is_run_char(c))
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

/// Redact every unknown-root absolute-path SHAPE inside a verbatim text segment to
/// the path-free `analysis://unknown` marker, leaving neutral text untouched. This
/// is the FAIL-CLOSED net for a real private path under a root the redactor was not
/// configured with — it must never ride a verbatim segment out unredacted.
fn redact_unknown_shapes(norm_segment: &str) -> String {
    let mut out = String::with_capacity(norm_segment.len());
    let mut i = 0;
    while i < norm_segment.len() {
        match unknown_shape_len(norm_segment, i) {
            Some(len) if len > 0 => {
                out.push_str("analysis://unknown");
                i += len;
            }
            _ => {
                // Copy one whole char verbatim (indices from `char_indices` are
                // always on UTF-8 boundaries).
                let ch = norm_segment[i..]
                    .chars()
                    .next()
                    .expect("i is a char boundary");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
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
    /// survives. Neutral text (the caller's generic placeholders, `/tmp`,
    /// repo-relative paths) passes through unchanged — BUT any UNKNOWN-root
    /// absolute-path SHAPE in a verbatim segment is FAIL-CLOSED to
    /// `analysis://unknown`, so a real private path under a root the redactor was
    /// not configured with can never ride out unredacted.
    pub fn redact_value(&mut self, s: &str) -> String {
        // Work on the normalized form so a back-slashed root still matches, then
        // return the normalized-and-redacted string (callers compare normalized).
        let norm = normalize(s);
        let mut out = String::with_capacity(norm.len());
        let mut rest = norm.as_str();

        while !rest.is_empty() {
            // Find the EARLIEST match position across ALL roots. `self.roots` is
            // sorted longest-first, so for a tie at the same position the most
            // specific (longest) root wins — but the WINNER is decided by position
            // first, never by root length alone. (The old "first root with any
            // match" rule emitted everything before the LONGEST root's match
            // verbatim, leaking a SHORTER root that appeared earlier in the string.)
            let mut best: Option<(usize, &ProjectRoots)> = None;
            for r in &self.roots {
                if let Some(pos) = rest.find(&r.norm_root) {
                    match best {
                        // strictly earlier position wins; equal position keeps the
                        // earlier-iterated (longer) root because roots is sorted
                        // longest-first.
                        Some((bpos, _)) if pos >= bpos => {}
                        _ => best = Some((pos, r)),
                    }
                }
            }

            let Some((pos, r)) = best else {
                // No known root anywhere in the remainder: fail-closed scan the
                // whole remainder for unknown-root shapes, then finish.
                out.push_str(&redact_unknown_shapes(rest));
                break;
            };

            // Emit text before the match — fail-closed scanned for unknown shapes.
            out.push_str(&redact_unknown_shapes(&rest[..pos]));
            let after_root = &rest[pos + r.norm_root.len()..];
            // Consume the WHOLE project-relative remainder (slashes + all legal
            // filename bytes) up to the first PROSE boundary. Using `is_run_char`
            // (not a narrower `,`/`;`/`)`/`]` set) is FAIL-CLOSED: a relative basename
            // after a `,` (`/root/My,Docs/Secret.ts`) is folded into the single opaque
            // file id instead of leaking as a leftover tail. The opaque output token
            // is identical regardless of what the relative remainder contains.
            let rel_end = after_root
                .find(|c: char| !is_run_char(c))
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
            // Repo-relative paths whose segments merely CONTAIN `users`/`home`/a
            // drive-looking token must NOT be redacted: the leak guard's word
            // boundary (`\b` / `(^|[^A-Za-z0-9_])`) is mirrored, so a `/Users` or
            // `x:` not at a boundary is left alone.
            "src/Users/widget.ts",
            "crates/verter_lsp/src/Users.rs",
            "packages/home/index.ts",
            "myhome/users/x.ts",
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

    // ── B-b: multi-root ordering. Two known roots; the SHORTER root appears
    // EARLIER in the string. The old "first root with any match wins" rule emitted
    // everything before the LONGEST root's match verbatim, leaking the shorter
    // root. Both must be redacted regardless of order. ──

    fn two_root_redactor() -> (Redactor, String, String) {
        // `short` is a strict ancestor-shaped (shorter) root; `long` is a longer,
        // unrelated root. They are NOT prefixes of each other.
        let short = format!("/{}/{}/alpha", "d:", "dev");
        let long = format!("/{}/{}/beta-corp/widgets", "d:", "dev");
        let r = Redactor::new([
            (
                ProjectId::new("p0001").unwrap(),
                std::path::PathBuf::from(&short),
            ),
            (
                ProjectId::new("p0002").unwrap(),
                std::path::PathBuf::from(&long),
            ),
        ]);
        (r, short, long)
    }

    #[test]
    fn redacts_all_known_roots_regardless_of_order_in_the_string() {
        let (mut r, short, long) = two_root_redactor();
        // SHORTER root first, LONGER root second.
        let msg = format!("a {short}/src/A.vue then b {long}/src/B.vue end");
        let out = r.redact_value(&msg);
        assert!(!out.contains(&short), "shorter root leaked: {out}");
        assert!(!out.contains(&long), "longer root leaked: {out}");
        assert!(out.contains("analysis://p0001/file-"), "short→p0001: {out}");
        assert!(out.contains("analysis://p0002/file-"), "long→p0002: {out}");
        // The same holds with the LONGER root appearing first.
        let msg2 = format!("x {long}/src/B.vue y {short}/src/A.vue z");
        let out2 = r.redact_value(&msg2);
        assert!(
            !out2.contains(&short),
            "shorter root leaked (order 2): {out2}"
        );
        assert!(
            !out2.contains(&long),
            "longer root leaked (order 2): {out2}"
        );
    }

    #[test]
    fn nested_root_still_wins_over_its_ancestor_at_the_same_position() {
        // `ancestor` is a prefix of `nested`. A path under the nested root must
        // redact to the nested project's id (most specific wins on a positional tie).
        let ancestor = format!("/{}/{}/mono", "d:", "dev");
        let nested = format!("{ancestor}/packages/ui");
        let mut r = Redactor::new([
            (
                ProjectId::new("p0001").unwrap(),
                std::path::PathBuf::from(&ancestor),
            ),
            (
                ProjectId::new("p0002").unwrap(),
                std::path::PathBuf::from(&nested),
            ),
        ]);
        let out = r.redact_value(&format!("{nested}/src/Comp.vue"));
        assert!(
            out.starts_with("analysis://p0002/file-"),
            "nested wins: {out}"
        );
        assert!(!out.contains(&ancestor), "ancestor root leaked: {out}");
    }

    // ── C-b: fail-closed for UNKNOWN-root absolute-path shapes. A real private path
    // under a root the redactor was NOT configured with must NOT ride out verbatim. ──

    #[test]
    fn redact_value_fails_closed_on_unknown_root_absolute_path_shapes() {
        let (mut r, _root) = redactor(); // configured only with the planted root
                                         // Each of these is a private-SHAPED absolute path under no known root.
        let secret_name = "Sekret";
        let cases = [
            format!("/Users/alice/proj/src/{secret_name}.vue"),
            format!("/home/bob/app/src/{secret_name}.ts"),
            format!("c:/dev/other-corp/{secret_name}.tsx"),
            format!("c:/Users/carol/work/{secret_name}.vue"),
            format!("file:///Users/dave/x/{secret_name}.ts"),
        ];
        for input in cases {
            let out = r.redact_value(&input);
            assert!(
                out.contains("analysis://unknown"),
                "unknown-root shape not failed-closed: {input} -> {out}"
            );
            assert!(
                !out.to_lowercase().contains(&secret_name.to_lowercase()),
                "private basename leaked through an unknown-root shape: {out}"
            );
        }
    }

    #[test]
    fn fail_closed_on_bare_file_uri_home_no_trailing_slash() {
        // The leak guard's `absolute_path_shape` matches `file:///users/` /
        // `file:///home/` on the MARKER alone, so the redactor MUST too — a bare
        // `file:///Users/<name>` (no further slash) must NOT ride out verbatim.
        let (mut r, _root) = redactor();
        for input in ["file:///Users/alice", "file:///home/bob"] {
            let out = r.redact_value(input);
            assert_eq!(
                out, "analysis://unknown",
                "bare file-URI home leaked: {out}"
            );
        }
    }

    #[test]
    fn fail_closed_consumes_basename_after_a_comma_or_paren_no_tail_leak() {
        // A legal filename byte (`,` `;` `)` `]`) MID-path must NOT split the run and
        // leave a private basename tail. Both the unknown-shape path AND the
        // known-root path must swallow the whole run.
        let (mut r, root) = redactor();
        let secret = "Sekret";
        // unknown root with a comma in a segment
        let out1 = r.redact_value(&format!("/Users/al/My,Docs/{secret}.ts"));
        assert!(
            !out1.to_lowercase().contains(&secret.to_lowercase()),
            "comma-tail basename leaked (unknown): {out1}"
        );
        assert!(
            !out1.contains("Docs"),
            "rel-dir tail leaked (unknown): {out1}"
        );
        // unknown root with a `)` (e.g. inside a parenthesized message)
        let out2 = r.redact_value(&format!("(/home/bo/a]b/{secret}.vue)"));
        assert!(
            !out2.to_lowercase().contains(&secret.to_lowercase()),
            "bracket-tail basename leaked (unknown): {out2}"
        );
        // KNOWN root with a comma in a segment — folds into the opaque id, no tail.
        let out3 = r.redact_value(&format!("{root}/My,Docs/{secret}.ts"));
        assert!(
            !out3.to_lowercase().contains(&secret.to_lowercase()),
            "comma-tail basename leaked (known root): {out3}"
        );
        assert!(
            !out3.contains("Docs"),
            "rel-dir tail leaked (known): {out3}"
        );
        assert!(out3.contains("analysis://p0001/file-"));
    }

    #[test]
    fn fail_closed_redaction_embedded_in_a_message_keeps_neutral_text() {
        let (mut r, _root) = redactor();
        let msg = "error TS2307 at /Users/eve/secret/main.ts: cannot find module";
        let out = r.redact_value(msg);
        assert!(!out.contains("/Users/eve"), "unknown root leaked: {out}");
        assert!(!out.contains("secret"), "private segment leaked: {out}");
        assert!(out.contains("analysis://unknown"));
        // Neutral surrounding text survives.
        assert!(out.contains("error TS2307 at"));
        assert!(out.contains("cannot find module"));
    }

    #[test]
    fn display_path_fails_closed_for_unknown_private_shape() {
        let (mut r, _root) = redactor();
        // An unknown-root private path → the path-free marker, never the raw path.
        let shown = r.display_path("/Users/frank/app/src/Secret.vue");
        assert_eq!(shown, "analysis://unknown");
        assert!(!shown.contains("frank"));
        assert!(!shown.contains("Secret"));
    }
}
