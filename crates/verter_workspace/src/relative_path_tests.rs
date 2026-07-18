//! Unit tests for [`crate::relative_path`].
//!
//! Per sub- — these tests verify the single source of truth for
//! relative path resolution and stem normalization is correctly implemented
//! BEFORE any session-side delegation lands.

use super::{join_relative, normalize_relative_specifier, strip_extension_first};

#[test]
fn join_relative_corpus() {
    // Direct test of `join_relative`. Corpus regenerated against the actual
    // algorithm output (R4 fix: pops to `[""]`, joins to `""`. Not `"/"`).
    assert_eq!(join_relative("/src/Comp.vue", "./types"), "/src/types");
    assert_eq!(join_relative("/src/Comp.vue", "../shared"), "/shared");
    assert_eq!(join_relative("/src/a/b/c.vue", "../../d/e"), "/src/d/e");
    assert_eq!(join_relative("Comp.vue", "./types"), "types");
    assert_eq!(join_relative("/src/Comp.vue", "./"), "/src");
    assert_eq!(join_relative("/src/Comp.vue", "."), "/src");
    // R4: pops to `[""]`, joins to `""`. Not `"/"`.
    assert_eq!(join_relative("/src/Comp.vue", "../"), "");
    assert_eq!(join_relative("/src/Comp.vue", "./../sibling"), "/sibling");
    assert_eq!(join_relative("/src/Comp.vue", "./a//b/./c"), "/src/a/b/c");
    // R4: had_root + parts.len()==1 means no pop; join leaves `""`. Not `"/"`.
    assert_eq!(join_relative("/Comp.vue", ".."), "");
}

#[test]
fn join_relative_backslash_separators_join_identically_to_slash_forms() {
    // TS `normalizeSlashes` parity: `\` is a separator in module
    // specifiers, so every backslash spelling joins byte-identically to
    // its `/` twin. Without the rewrite the backslash SEGMENT survives
    // verbatim (`/src/Listbox/..\index`) and can never match a
    // `/`-separated canonical id.
    assert_eq!(
        join_relative("/src/Listbox/Filter.vue", "..\\index"),
        join_relative("/src/Listbox/Filter.vue", "../index"),
    );
    assert_eq!(
        join_relative("/src/Listbox/Filter.vue", "..\\index"),
        "/src/index",
    );
    assert_eq!(join_relative("/src/Comp.vue", ".\\types"), "/src/types");
    assert_eq!(
        join_relative("/src/a/b/c.vue", "..\\..\\d\\e"),
        join_relative("/src/a/b/c.vue", "../../d/e"),
    );
    // Mixed separators collapse the same way.
    assert_eq!(
        join_relative("/src/Listbox/Filter.vue", "..\\Primitive/index"),
        "/src/Primitive/index",
    );
    // Negative: the rewrite must NOT alter slash-form results (byte-pin
    // against the corpus above).
    assert_eq!(join_relative("/src/Comp.vue", "./types"), "/src/types");
}

#[test]
fn join_relative_backslash_rewrite_applies_only_to_the_relative_class() {
    // The `\` → `/` rewrite is separator normalization for the TS
    // `pathIsRelative` class ONLY (`.`/`..` bare + `./`/`../`/`.\`/`..\`
    // prefixes). A dot-prefixed specifier OUTSIDE that class
    // (`.alias\types`, `..foo\bar` — TS: package-ish, a resolution error)
    // must keep its bytes: the backslash segment survives verbatim, so the
    // joined path can never match a `/`-separated canonical id
    // (fail-closed), instead of silently resolving against a real file at
    // the slash-rewritten path (`/src/.alias/types.ts`) — a wrong
    // resolution AND a wrong dependency edge, diverging from TS.
    let joined = join_relative("/src/Comp.vue", ".alias\\types");
    assert!(
        joined.contains('\\'),
        "non-relative dot-prefixed specifier must keep its backslash \
         (fail-closed unmatchable join), got {joined:?}"
    );
    assert_eq!(joined, "/src/.alias\\types");

    let joined = join_relative("/src/Comp.vue", "..foo\\bar");
    assert!(
        joined.contains('\\'),
        "'..foo\\bar' is not in the pathIsRelative class (no separator \
         after '..'); its backslash must survive, got {joined:?}"
    );
    assert_eq!(joined, "/src/..foo\\bar");

    // The genuinely-relative spellings still normalize (regression pin
    // for the gated rewrite).
    assert_eq!(join_relative("/src/Comp.vue", ".\\x"), "/src/x");
    assert_eq!(join_relative("/src/a/Comp.vue", "..\\x"), "/src/x");
}

#[test]
fn normalize_relative_specifier_trims_trailing_slash() {
    assert_eq!(normalize_relative_specifier("./types/"), "./types");
    assert_eq!(normalize_relative_specifier("./types"), "./types");
    // Edge case — one slash trimmed; produces a partial spec.
    assert_eq!(normalize_relative_specifier("../"), "..");
    assert_eq!(normalize_relative_specifier("."), ".");
}

#[test]
fn normalize_relative_specifier_directory_style_imports() {
    // Directory-style imports must not collide
    // pathologically with file imports.
    assert_eq!(normalize_relative_specifier("./pkg/"), "./pkg");
    assert_eq!(normalize_relative_specifier("./pkg/index"), "./pkg/index");
    // Algorithm only trims one trailing `/`; deep paths unaffected.
    assert_eq!(normalize_relative_specifier("./pkg/."), "./pkg/.");
    // Only ONE trailing slash trimmed; double-trailing stays partial.
    assert_eq!(normalize_relative_specifier("./pkg//"), "./pkg/");
}

#[test]
fn strip_extension_first_uses_caller_provided_order() {
    // Caller pre-sorts longest-first. `.d.ts` matches before `.ts`.
    let sorted_desc = vec![
        ".d.ts".to_string(),
        ".d.mts".to_string(),
        ".d.cts".to_string(),
        ".tsx".to_string(),
        ".jsx".to_string(),
        ".mts".to_string(),
        ".mjs".to_string(),
        ".cts".to_string(),
        ".cjs".to_string(),
        ".vue".to_string(),
        ".ts".to_string(),
        ".js".to_string(),
    ];
    assert_eq!(
        strip_extension_first("/types.d.ts", &sorted_desc),
        Some("/types"),
    );
    assert_eq!(
        strip_extension_first("/types.ts", &sorted_desc),
        Some("/types"),
    );
    assert_eq!(
        strip_extension_first("/types.vue", &sorted_desc),
        Some("/types"),
    );

    // Caller provides wrong order — helper takes first match `.ts` returning
    // `/types.d`. Documents the contract that callers must sort.
    let unsorted = vec![".ts".to_string(), ".d.ts".to_string()];
    assert_eq!(
        strip_extension_first("/types.d.ts", &unsorted),
        Some("/types.d"),
    );

    // No match — returns None.
    let svelte_only = vec![".svelte".to_string()];
    assert_eq!(strip_extension_first("/types.ts", &svelte_only), None);
}
