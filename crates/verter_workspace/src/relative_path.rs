//! Single source of truth for relative path resolution and extension stripping
//! across the workspace and session crates.
//!
//! `verter_session::id::resolve_external` delegates to [`join_relative`] for
//! the relative-prefix branch; both `Engine::record_parsed_edges` (when
//! storing `(specifier, kind) -> stem`) and
//! `EdgeStore::replace_exact_resolutions` (when clearing stale stems) MUST
//! canonicalise via [`normalize_relative_specifier`].

/// Path-join an importer-relative specifier against an importer canonical_id.
/// Mirrors the algorithm preserved in this codebase since `verter_session`
/// was written. `parent`-pop guard, root-segment guard, and `//`/`./` elision
/// are retained byte-for-byte.
///
/// `\` is a path separator in module specifiers (TS `normalizeSlashes` — the
/// same `pathIsRelative` class the resolver's [`crate::resolver::join_paths`]
/// route normalizes via `normalize_canonical_id`), so `'..\index'` joins
/// byte-identically to `'../index'`. Without the rewrite a backslash segment
/// survives verbatim and the joined path can never match a canonical id
/// (canonicals are `/`-separated) — an overlay-only helper imported through a
/// backslash spelling would be probed at a path that cannot exist.
///
/// The rewrite is gated on the shared
/// [`crate::resolver::is_relative_specifier`] predicate: a dot-prefixed
/// specifier OUTSIDE the TS `pathIsRelative` class (`.alias\types` — TS:
/// package-ish, a resolution error) keeps its bytes, so its backslash
/// segment stays verbatim and the joined path stays unmatchable
/// (fail-closed) instead of silently resolving against a real file at the
/// slash-rewritten path.
///
/// Preconditions: `specifier.starts_with('.')` (relative). The function does
/// NOT panic on non-relative input — it returns a best-effort path-join, but
/// callers SHOULD guard against passing non-relative specifiers because the
/// result is undefined per the documented contract.
pub fn join_relative(importer_id: &str, specifier: &str) -> String {
    debug_assert!(
        specifier.starts_with('.'),
        "join_relative expects a relative specifier (starts with '.'); got {specifier:?}",
    );
    let specifier: std::borrow::Cow<'_, str> =
        if specifier.contains('\\') && crate::resolver::is_relative_specifier(specifier) {
            std::borrow::Cow::Owned(specifier.replace('\\', "/"))
        } else {
            std::borrow::Cow::Borrowed(specifier)
        };
    let mut parts: Vec<&str> = importer_id.split('/').collect();
    parts.pop(); // remove filename
                 // Track whether the owner had a root prefix (leading empty segment from "/...")
    let had_root = parts.first() == Some(&"");
    for segment in specifier.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                // Guard: don't pop past the root segment (empty string from leading /)
                if parts.len() > 1 || (parts.len() == 1 && !had_root) {
                    let _ = parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Normalise a relative specifier for stem-cleanup matching. Trims a single
/// trailing `/`. Other normalisation (collapsing `./` prefixes, elision of
/// `..` segments) is NOT done here — that work is `join_relative`'s. This
/// helper is only for specifier-string identity matching across writers.
///
/// SCOPE LIMIT: if the project later adds package-export semantics where
/// `./pkg/` and `./pkg` resolve to different files (e.g., one triggers a
/// directory's "exports" field, the other doesn't), this normaliser MUST
/// be revisited. Today's resolver treats both as the same path-join target,
/// so the trim is correct.
pub fn normalize_relative_specifier(specifier: &str) -> String {
    if specifier.len() > 1 && specifier.ends_with('/') {
        specifier[..specifier.len() - 1].to_string()
    } else {
        specifier.to_string()
    }
}

/// Strip the longest matching extension suffix from `path`, returning the
/// stem. Caller is responsible for passing `extensions` already sorted by
/// length DESCENDING — `Engine::set_default_resolve_extensions` does this
/// once, and the strip helper iterates with first-match semantics. Returns
/// `None` if no extension matches.
pub fn strip_extension_first<'a>(
    path: &'a str,
    extensions_sorted_desc: &[String],
) -> Option<&'a str> {
    for ext in extensions_sorted_desc {
        if let Some(stem) = path.strip_suffix(ext.as_str()) {
            return Some(stem);
        }
    }
    None
}

#[cfg(test)]
#[path = "relative_path_tests.rs"]
mod tests;
