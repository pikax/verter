//! Cross-language drift guard for `PublishedSurfacePolicy`'s
//! constants (the named `published_surface_constants_match_ts_port`
//! guard cited in `packages/component-meta/src/published-surface.ts`
//! and demanded by the R20-fix-cycle brief).
//!
//! The Rust source of truth is `crates/verter_audit/src/published_surface.rs`.
//! The TS consumer-side mirror is
//! `packages/component-meta/src/published-surface.ts`. Both files
//! redeclare two constant lists:
//!
//!   * `COMPAT_BLOCKED_SLOT_NAMES` (vue-component-meta-equivalent slot
//!     blocklist; consumed by the `Compat` and `Refined` policies).
//!   * `VUE_INTRINSIC_ATTR_NAMES` (Vue intrinsic attribute names that
//!     `Refined` strips unless the author explicitly re-declared them
//!     in the macro type arg).
//!
//! This test loads the TS file, parses both arrays via a deliberately
//! conservative line walker (no regex over TS source; we ban that
//! pattern from the typed-IR resolver — using it here in a
//! cross-language drift guard test is the only place the project
//! genuinely needs to read raw TS, and even here we restrict
//! ourselves to a literal substring + token-quote walk), and asserts
//! exact set equality with the Rust constants. Any drift produces a
//! detailed diff. The guard also covers the `Refined` event-shadow
//! derivation — both Rust and TS compute `eventNameToOnPropName`
//! identically against a small fixed set of payloads.
//!
//! Discriminating property: changing either side without updating
//! the other (or accidentally desync-ing a single entry) MUST cause
//! this test to fail with a precise diff. A trivial pass-through
//! that always returns OK would not satisfy that.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use verter_audit::published_surface::{
    event_name_to_on_prop_name, COMPAT_BLOCKED_SLOT_NAMES, VUE_INTRINSIC_ATTR_NAMES,
};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is set per-crate; walk up to the workspace root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("pnpm-workspace.yaml").exists())
        .map(Path::to_path_buf)
        .expect("workspace root with pnpm-workspace.yaml should exist above the verter_audit crate")
}

fn ts_port_source() -> String {
    let path = workspace_root().join("packages/component-meta/src/published-surface.ts");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read TS port at {path:?}: {e}"))
}

/// Parse the contents of an `export const NAME: readonly string[] = [ ... ] as const;`
/// declaration in the TS file. The walker is intentionally minimal:
/// it finds the literal `export const <name>` prefix, then scans the
/// subsequent characters for double-quoted token strings until the
/// closing `]`. Any non-quoted content is ignored.
fn parse_ts_string_array(source: &str, name: &str) -> Vec<String> {
    let needle = format!("export const {name}");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("TS port should declare `export const {name}`"));
    let tail = &source[start + needle.len()..];
    // Look for `= [` (the assignment) — not just `[`, because the
    // type annotation `: readonly string[]` would otherwise grab the
    // `[` inside `string[]`.
    let assign_rel = tail.find("= [").unwrap_or_else(|| {
        panic!("TS port `export const {name}` should be assigned an array via `= [`")
    });
    let after_open = &tail[assign_rel + 3..];
    let close_rel = after_open
        .find(']')
        .unwrap_or_else(|| panic!("TS port `export const {name}` array should be closed by `]`"));
    let array_body = &after_open[..close_rel];

    let mut out = Vec::new();
    let mut chars = array_body.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut token = String::new();
            for nc in chars.by_ref() {
                if nc == '"' {
                    break;
                }
                token.push(nc);
            }
            out.push(token);
        }
    }
    out
}

#[test]
fn published_surface_constants_match_ts_port() {
    let ts_source = ts_port_source();

    let ts_compat = parse_ts_string_array(&ts_source, "COMPAT_BLOCKED_SLOT_NAMES");
    let ts_intrinsics = parse_ts_string_array(&ts_source, "VUE_INTRINSIC_ATTR_NAMES");

    let rust_compat: Vec<String> = COMPAT_BLOCKED_SLOT_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rust_intrinsics: Vec<String> = VUE_INTRINSIC_ATTR_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Exact order equality — both languages list the entries in the
    // same order, and we want drift in EITHER direction (re-ordering,
    // additions, deletions) to surface here.
    assert_eq!(
        rust_compat, ts_compat,
        "COMPAT_BLOCKED_SLOT_NAMES drift between Rust source of truth \
         (`crates/verter_audit/src/published_surface.rs`) and TS port \
         (`packages/component-meta/src/published-surface.ts`).\n\
         Rust: {rust_compat:?}\nTS:   {ts_compat:?}"
    );
    assert_eq!(
        rust_intrinsics, ts_intrinsics,
        "VUE_INTRINSIC_ATTR_NAMES drift between Rust source of truth \
         and TS port.\nRust: {rust_intrinsics:?}\nTS:   {ts_intrinsics:?}"
    );

    // Set equality cross-check (catches subtle dup / case bugs the
    // ordered assertion might miss).
    let rust_compat_set: HashSet<&String> = rust_compat.iter().collect();
    let ts_compat_set: HashSet<&String> = ts_compat.iter().collect();
    assert_eq!(rust_compat_set, ts_compat_set);

    let rust_intrinsics_set: HashSet<&String> = rust_intrinsics.iter().collect();
    let ts_intrinsics_set: HashSet<&String> = ts_intrinsics.iter().collect();
    assert_eq!(rust_intrinsics_set, ts_intrinsics_set);
}

/// `event_name_to_on_prop_name` (Rust) and the TS port's
/// `eventNameToOnPropName` MUST agree on a fixed set of payloads.
/// The TS implementation is not invocable from Rust, but the
/// Rust side is the canonical reference — we test the Rust function
/// here against the SAME table the TS-side spec asserts (see
/// `packages/component-meta/src/published-surface.spec.ts`). If
/// either side changes the algorithm in a way that breaks one
/// row, both this test and the TS vitest fail; landing only
/// "this side passes" is not a real fix.
#[test]
fn event_name_to_on_prop_name_matches_ts_port_fixed_cases() {
    let cases: &[(&str, &str)] = &[
        ("submit", "onSubmit"),
        ("click", "onClick"),
        ("state-change", "onStateChange"),
        ("update:modelValue", "onUpdateModelValue"),
        ("camelCaseEvt", "onCamelCaseEvt"),
        ("two_words", "onTwoWords"),
        ("multi-segment-name", "onMultiSegmentName"),
        ("Already:Pascal", "onAlreadyPascal"),
    ];
    for (input, expected) in cases {
        let actual = event_name_to_on_prop_name(input);
        assert_eq!(
            &actual, expected,
            "event_name_to_on_prop_name({input:?}) drifted from TS port: \
             got {actual:?}, expected {expected:?}"
        );
    }
}
