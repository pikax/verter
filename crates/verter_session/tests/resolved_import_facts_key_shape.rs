//! Source-shape architecture guard for
//! [`verter_session::resolved_import_facts::ResolvedImportFactsKey`].
//!
//! Pins R21 at the source level: the key MUST carry
//! `canonical`, `content_hash`, `parse_env_hash`,
//! `resolve_env_hash`, and `resolver_version`, and MUST NOT carry
//! `lib_env_hash`. The compile-time + runtime tests in
//! `resolved_import_facts_invariants.rs` exercise the behavioural
//! consequence; this test pins the source-level negative assertion
//! so a future change adding `lib_env_hash` to the key fails this
//! guard before any consumer test would catch it.

const RESOLVED_IMPORT_FACTS_SRC: &str = include_str!("../src/resolved_import_facts.rs");

#[test]
fn key_struct_carries_required_dimensions() {
    let needles = [
        "pub struct ResolvedImportFactsKey",
        "pub canonical: Arc<str>",
        "pub content_hash: Hash16",
        "pub parse_env_hash: Hash16",
        "pub resolve_env_hash: Hash16",
        "pub resolver_version: u32",
    ];
    for needle in needles {
        assert!(
            RESOLVED_IMPORT_FACTS_SRC.contains(needle),
            "ResolvedImportFactsKey source MUST contain `{needle}` — required key dimension missing"
        );
    }
}

#[test]
fn key_struct_excludes_lib_env_hash() {
    // Walk every line of the source. A line is part of the key
    // struct body iff it appears between the struct opener and its
    // closing brace. Inside that span, the literal `lib_env_hash`
    // MUST NOT appear (R21 scoping rule).
    let mut inside_key_struct = false;
    let mut brace_depth: i32 = 0;
    for (lineno, line) in RESOLVED_IMPORT_FACTS_SRC.lines().enumerate() {
        if line.contains("pub struct ResolvedImportFactsKey") {
            inside_key_struct = true;
            brace_depth = 0;
        }
        if inside_key_struct {
            for ch in line.chars() {
                if ch == '{' {
                    brace_depth += 1;
                } else if ch == '}' {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        inside_key_struct = false;
                        break;
                    }
                }
            }
            if inside_key_struct && line.contains("lib_env_hash") {
                panic!(
                    "ResolvedImportFactsKey source line {} contains `lib_env_hash`: `{line}`. \
                     R21 scoping rule: the resolved-import facts cache key MUST NOT depend on \
                     `lib_env_hash`.",
                    lineno + 1
                );
            }
        }
    }
}
