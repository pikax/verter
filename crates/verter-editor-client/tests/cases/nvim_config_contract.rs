//! Drift-guard for the Neovim client's init-options key set.
//!
//! Neovim has a built-in native LSP client, so Verter's Neovim support is a pure
//! Lua config module (`editors/nvim/`) — no compiled extension, no server-side
//! change. The Lua `build_init_options` builder is the ONLY editor client not
//! written in Rust, so it is the one client that can silently re-diverge from the
//! shared launch contract ([`verter_editor_client::build_initialization_options`],
//! the SSoT). The Lapce, Zed, and Helix clients are all bound to that SSoT by Rust
//! tests; this test binds Neovim too.
//!
//! It extracts the set of top-level init-option keys that `build_init_options`
//! emits and asserts that set EQUALS the shared SSoT's key set (set equality — a
//! missing key AND an extra key both fail). It is hermetic: no Neovim binary, no
//! `verter-lsp` process.
//!
//! ## Why a bounded structural scan, not a Lua parse
//!
//! Lua is not parseable from Rust without pulling in a Lua-parser dependency, and
//! the dependency policy forbids adding one for a guard. Instead this test does a
//! BOUNDED, STRUCTURE-AWARE extraction: it isolates the `build_init_options`
//! function body, finds the single `return { ... }` table literal it returns, and
//! collects the identifiers assigned at brace-depth 1 of that table (the top-level
//! keys). Brace-depth tracking (not a fixed indentation depth) makes it robust to
//! reformatting: nested keys live at depth >= 2 and are never captured. This is a
//! genuine structural extraction of the returned table's top-level keys, scoped to
//! one function — not a regex scrape of arbitrary file text for meaning.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Locate `editors/nvim/lua/verter/config.lua` relative to this crate's manifest
/// dir (`crates/verter-editor-client`). Built with `Path::join` so the path is
/// correct on Windows, macOS, and Linux (no hardcoded separators).
fn nvim_config_lua_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // repo root
        .join("editors")
        .join("nvim")
        .join("lua")
        .join("verter")
        .join("config.lua")
}

/// Read the shipped `config.lua` source. Reading fails loudly with the resolved
/// path so a relocation is obvious.
fn read_config_lua() -> String {
    let path = nvim_config_lua_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Extract the top-level keys of the table returned by `M.build_init_options`.
///
/// Strategy (bounded, structure-aware — see the module docs for why this is not a
/// full Lua parse):
/// 1. Find the `function M.build_init_options(` line; the body runs until the
///    first line that is exactly `end` (the function lives at module scope, so its
///    closing `end` is unindented).
/// 2. Within the body, find the `return {` that opens the returned table.
/// 3. Walk the body character by character from that `{`, tracking brace depth.
///    When depth is exactly 1 (the table's own top level), an `<ident> =`
///    assignment at the start of a logical entry names a top-level key.
///
/// The key recognizer keys off the table structure: at brace-depth 1, a run of
/// `[%w_]+` immediately followed (after optional spaces) by `=` is a top-level
/// field name. Nested fields (e.g. `enabled = ...` inside `lint = { ... }`) are at
/// depth >= 2 and are skipped.
fn extracted_init_option_keys(source: &str) -> BTreeSet<String> {
    let lines: Vec<&str> = source.lines().collect();

    // (1) Bound the function body.
    let start = lines
        .iter()
        .position(|l| l.contains("function M.build_init_options("))
        .expect("config.lua must define `function M.build_init_options(`");
    // The closing `end` is the first subsequent line whose trimmed content is
    // exactly `end` (module-scope function => unindented `end`).
    let end_rel = lines[start + 1..]
        .iter()
        .position(|l| l.trim_end() == "end")
        .expect("build_init_options must have a closing `end`");
    let body: String = lines[start..=start + 1 + end_rel].join("\n");

    // (2) Find the `return {` that opens the returned table, and start scanning at
    // that brace.
    let return_idx = body
        .find("return")
        .expect("build_init_options must `return` a table");
    let brace_idx = body[return_idx..]
        .find('{')
        .map(|off| return_idx + off)
        .expect("build_init_options must return a `{ ... }` table");

    // (3) Brace-depth walk over the returned table.
    let bytes = body.as_bytes();
    let mut keys = BTreeSet::new();
    let mut depth = 0i32;
    // `at_entry_start` is true when the cursor is positioned where a new table
    // entry could begin at the current depth (right after `{` or a `,`), modulo
    // whitespace and comments. Only at depth 1 and at an entry start do we read a
    // key.
    let mut at_entry_start = false;
    let mut i = brace_idx;
    while i < bytes.len() {
        // Lua comments must be skipped WITHOUT disturbing `at_entry_start` (a
        // comment can sit between `},` and the next key) and without their `{`/`}`
        // /`,` bytes corrupting the brace walk. `--[[ ... ]]` is a block comment;
        // a bare `--` runs to end of line.
        if bytes[i] as char == '-' && i + 1 < bytes.len() && bytes[i + 1] as char == '-' {
            if body[i..].starts_with("--[[") {
                // Block comment: skip to the closing `]]`.
                let rest = &body[i + 4..];
                match rest.find("]]") {
                    Some(off) => i = i + 4 + off + 2,
                    None => break, // unterminated; nothing meaningful follows
                }
            } else {
                // Line comment: skip to the next newline.
                match body[i..].find('\n') {
                    Some(off) => i += off, // leave the '\n' for the whitespace arm
                    None => break,
                }
            }
            continue;
        }

        let c = bytes[i] as char;
        match c {
            '{' => {
                depth += 1;
                at_entry_start = true; // first entry of the new table
                i += 1;
            }
            '}' => {
                depth -= 1;
                at_entry_start = false;
                if depth == 0 {
                    break; // closed the returned table
                }
                i += 1;
            }
            ',' => {
                at_entry_start = true;
                i += 1;
            }
            c if c.is_whitespace() => {
                i += 1;
            }
            c if (c.is_ascii_alphabetic() || c == '_') && depth == 1 && at_entry_start => {
                // Read the identifier run.
                let id_start = i;
                while i < bytes.len() {
                    let ch = bytes[i] as char;
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let ident = &body[id_start..i];
                // It is a key only if the next non-space byte is `=` (assignment).
                let mut j = i;
                while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] as char == '=' {
                    keys.insert(ident.to_string());
                }
                at_entry_start = false;
            }
            _ => {
                at_entry_start = false;
                i += 1;
            }
        }
    }

    keys
}

/// The Neovim `build_init_options` top-level key set MUST equal the shared SSoT's
/// key set — the same six server-read keys the Lapce/Zed/Helix clients ship:
/// `lint, inlayHints, viteConfig, experimental, hover, statistics`. A missing key
/// AND an extra key both fail (set equality). This is the load-bearing tie that
/// keeps the one non-Rust client bound to the shared contract.
#[test]
fn nvim_init_options_keys_equal_shared_ssot() {
    let source = read_config_lua();
    let actual = extracted_init_option_keys(&source);

    let expected: BTreeSet<String> =
        verter_editor_client::build_initialization_options(&serde_json::json!({}))
            .as_object()
            .expect("build_initialization_options must return an object")
            .keys()
            .cloned()
            .collect();

    assert_eq!(
        actual, expected,
        "editors/nvim/.../config.lua build_init_options top-level keys must EQUAL \
         the shared build_initialization_options SSoT key set; got {actual:?}, \
         expected {expected:?}"
    );
}

/// Explicit positive/negative spot checks on the extracted set, stated separately
/// from the set-equality assertion as a regression tripwire:
/// * `statistics` MUST be emitted (the server reads `initializationOptions.statistics`).
/// * `frameworks` MUST NOT be emitted (the server ignores it — dead protocol surface).
#[test]
fn nvim_init_options_include_statistics_and_drop_frameworks() {
    let source = read_config_lua();
    let keys = extracted_init_option_keys(&source);

    assert!(
        keys.contains("statistics"),
        "build_init_options must emit the server-read `statistics` key: {keys:?}"
    );
    assert!(
        !keys.contains("frameworks"),
        "build_init_options must NOT emit the dead `frameworks` key (the server \
         ignores it): {keys:?}"
    );
}

/// Sanity check on the extractor itself: it must not over-capture nested fields.
/// `enabled`, `preset`, `trustedFiles`, etc. live at brace-depth >= 2 and must
/// never appear among the top-level keys. (Guards against an extractor regression
/// that would make the SSoT comparison meaningless.)
#[test]
fn extractor_does_not_capture_nested_keys() {
    let source = read_config_lua();
    let keys = extracted_init_option_keys(&source);
    for nested in [
        "enabled",
        "preset",
        "trustedFiles",
        "conditionalRootNarrowing",
        "strictSlots",
        "provenance",
    ] {
        assert!(
            !keys.contains(nested),
            "the extractor must not capture the nested field {nested:?} as a \
             top-level key: {keys:?}"
        );
    }
}
