//! Guard: `no_inferred_project_init_on_tsgo`.
//!
//! Owned tsgo is PROJECT-BOUND: the owned `--api` checker requires an EXPLICIT
//! configured tsconfig binding (resolved by `require_owned_tsconfig`) and a
//! version-gated attach BEFORE any LSP traffic, and FAILS CLOSED otherwise. The
//! bare config-less / inferred-project owned startup — a `tsgo --lsp` provider
//! wrapped resilient WITHOUT a configured project — is DELETED. The standard LSP
//! handshake (`rootUri` / `workspaceFolders`) is RETAINED as transport metadata;
//! the ban is on the config-less owned WRAPPING path, NOT the rootUri field.
//!
//! This STATIC guard is the source-level backstop for that deletion. It walks the
//! owned-tsgo startup PRODUCTION source (`crates/verter_lsp/src/main.rs` +
//! `crates/verter_lsp/src/tsgo/resilient.rs`, excluding `*_tests.rs`) and asserts:
//!
//!   1. `main.rs` routes owned startup through `require_owned_tsconfig` by
//!      actually CALLING it (not merely defining the helper).
//!   2. `resilient.rs` has NO `struct TsgoBackend` (the config-less restart backend).
//!   3. `resilient.rs` has NO `impl ResilientBackend<TsgoTypeProvider> for TsgoBackend`.
//!   4. `resilient.rs` has NO production `pub fn new(` (the config-less constructor;
//!      `pub fn new_owned(` — the project-bound one — stays and is NOT matched).
//!   5. `main.rs` no longer calls `tsgo_resilient::new(` (the config-less wrap;
//!      `tsgo_resilient::new_owned(` — the project-bound one — stays and is NOT
//!      matched).
//!
//! It does NOT assert absence of `root_uri` / `rootUri` / `workspaceFolders` —
//! those are retained transport metadata.
//!
//! DISCRIMINATING: the self-test below proves each predicate FIRES on the
//! pre-deletion shape (a tree with `struct TsgoBackend` + `pub fn new(` +
//! `tsgo_resilient::new(`) and is CLEAN on the project-bound shape (which keeps
//! only `new_owned` + `require_owned_tsconfig`). So this guard is RED on the
//! pre-D1/D3 tree and GREEN after.

use std::fs;
use std::path::PathBuf;

/// Repo root (two parents up from `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// The owned-tsgo startup PRODUCTION source under `verter_lsp`. We target the two
/// files that own the owned-startup lifecycle directly (NOT a recursive walk):
/// `main.rs` (the `try_spawn_tsgo` entry) and `tsgo/resilient.rs` (the resilient
/// respawn backends). `*_tests.rs` siblings are excluded by construction (neither
/// path is a test file).
fn main_rs() -> PathBuf {
    workspace_root().join("crates/verter_lsp/src/main.rs")
}

fn resilient_rs() -> PathBuf {
    workspace_root().join("crates/verter_lsp/src/tsgo/resilient.rs")
}

/// Read a source file, panicking with its path so a missing/renamed file is a
/// loud failure rather than a silent skip.
fn read(path: &PathBuf) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Does a non-comment line of `src` match `needle`? (Comment lines — final-state
/// prose explaining WHY a symbol is gone — are allowed; a live declaration/call
/// is not.)
fn has_live(src: &str, needle: &str) -> bool {
    src.lines().any(|line| {
        let trimmed = line.trim_start();
        let is_comment = trimmed.starts_with("//") || trimmed.starts_with("/*");
        !is_comment && line.contains(needle)
    })
}

/// Does a non-comment line CALL `require_owned_tsconfig` (a `require_owned_tsconfig(`
/// invocation), as opposed to merely DEFINING it (`fn require_owned_tsconfig(`)?
///
/// The mere presence of the resolver symbol is satisfied by its own definition,
/// so it does not prove the owned-startup path actually routes through it; a
/// regression that deleted only the call site (leaving the helper unused) would
/// slip past a presence check. This asserts a real call: a line containing
/// `require_owned_tsconfig(` whose match is NOT the `fn …` definition.
fn calls_require_owned_tsconfig(src: &str) -> bool {
    src.lines().any(|line| {
        let trimmed = line.trim_start();
        let is_comment = trimmed.starts_with("//") || trimmed.starts_with("/*");
        if is_comment {
            return false;
        }
        // A definition line (`fn require_owned_tsconfig(` / `pub fn …`) is not a
        // call; only an invocation counts.
        let is_definition = line.contains("fn require_owned_tsconfig(");
        !is_definition && line.contains("require_owned_tsconfig(")
    })
}

#[test]
fn no_inferred_project_init_on_tsgo() {
    let main_src = read(&main_rs());
    let resilient_src = read(&resilient_rs());

    let mut violations: Vec<String> = Vec::new();

    // 1. main.rs must route owned startup through `require_owned_tsconfig` — and
    //    it must actually CALL it (a `require_owned_tsconfig(` invocation), not
    //    merely define the helper. A presence-only check is satisfied by the
    //    definition even if `try_spawn_tsgo` stopped calling the resolver.
    if !calls_require_owned_tsconfig(&main_src) {
        violations.push(
            "crates/verter_lsp/src/main.rs: owned tsgo startup must CALL `require_owned_tsconfig` \
             (the explicit-binding resolver) on the owned-startup path — no live call site found \
             (defining the helper without calling it does not bind the project)"
                .to_string(),
        );
    }

    // 2. resilient.rs must NOT define the config-less restart backend.
    if has_live(&resilient_src, "struct TsgoBackend") {
        violations.push(
            "crates/verter_lsp/src/tsgo/resilient.rs: `struct TsgoBackend` (the config-less \
             owned restart backend) must be deleted — only `TsgoOwnedBackend` (project-bound) \
             remains"
                .to_string(),
        );
    }

    // 3. resilient.rs must NOT impl the resilient backend for the config-less type.
    if has_live(
        &resilient_src,
        "impl ResilientBackend<TsgoTypeProvider> for TsgoBackend",
    ) {
        violations.push(
            "crates/verter_lsp/src/tsgo/resilient.rs: `impl ResilientBackend<TsgoTypeProvider> \
             for TsgoBackend` (the config-less owned restart strategy) must be deleted"
                .to_string(),
        );
    }

    // 4. resilient.rs must NOT define the config-less `pub fn new(` constructor.
    //    `pub fn new_owned(` (the project-bound one) is NOT matched by `pub fn new(`.
    if has_live(&resilient_src, "pub fn new(") {
        violations.push(
            "crates/verter_lsp/src/tsgo/resilient.rs: the config-less `pub fn new(` constructor \
             must be deleted — owned startup uses only `pub fn new_owned(`"
                .to_string(),
        );
    }

    // 5. main.rs must NOT call the config-less `tsgo_resilient::new(`.
    //    `tsgo_resilient::new_owned(` is NOT matched by `tsgo_resilient::new(`.
    if has_live(&main_src, "tsgo_resilient::new(") {
        violations.push(
            "crates/verter_lsp/src/main.rs: the config-less `tsgo_resilient::new(` wrap must be \
             deleted — owned startup wraps only via `tsgo_resilient::new_owned(`"
                .to_string(),
        );
    }

    assert!(
        violations.is_empty(),
        "owned tsgo startup must route through `require_owned_tsconfig` and `new_owned`; \
         config-less resilient `TsgoTypeProvider` wrapping is forbidden on the production \
         owned path.\n{}",
        violations.join("\n")
    );
}

/// DISCRIMINATING self-test: each predicate FIRES on the exact pre-deletion shape
/// and is CLEAN on the project-bound shape. Without this the static scan could
/// silently pass a permissive rename.
#[test]
fn no_inferred_project_init_on_tsgo_self_test_discriminates() {
    // ── `require_owned_tsconfig` CALL predicate (not mere presence) ──
    // A tree WITHOUT the resolver call fires the "absent" branch.
    let pre_main_no_resolver =
        "    let tsconfig_path = std::path::Path::new(root).join(\"tsconfig.json\");";
    assert!(
        !calls_require_owned_tsconfig(pre_main_no_resolver),
        "the pre-deletion main.rs (inline tsconfig.json join, no resolver call) must read as \
         `require_owned_tsconfig` NOT CALLED"
    );
    // The post-deletion shape CALLS the resolver.
    let post_main = "    let tsconfig = require_owned_tsconfig(workspace_root)?;";
    assert!(
        calls_require_owned_tsconfig(post_main),
        "the post-deletion main.rs must read as `require_owned_tsconfig` CALLED"
    );
    // CRITICAL discrimination: a tree that DEFINES the helper but never CALLS it
    // (the call site removed) must read as NOT CALLED — a presence-only check
    // would wrongly pass here because the symbol still appears.
    let definition_only = "fn require_owned_tsconfig(workspace_root: &std::path::Path) \
                           -> Result<String, String> {";
    assert!(
        !calls_require_owned_tsconfig(definition_only),
        "a tree that only DEFINES `fn require_owned_tsconfig(` (no call site) must read as \
         NOT CALLED — the call predicate must distinguish a call from the definition"
    );
    // A definition followed by a real call reads as CALLED (the live main.rs
    // shape: helper defined AND invoked from `try_spawn_tsgo`).
    let definition_and_call = "fn require_owned_tsconfig(root: &std::path::Path) -> R {}\n\
                               let tsconfig_str = require_owned_tsconfig(workspace_root)?;";
    assert!(
        calls_require_owned_tsconfig(definition_and_call),
        "a tree that defines AND calls `require_owned_tsconfig` must read as CALLED"
    );
    // A comment naming the call must NOT count as a live call.
    assert!(
        !calls_require_owned_tsconfig(
            "    // owned startup calls require_owned_tsconfig(workspace_root)."
        ),
        "a comment mentioning `require_owned_tsconfig(` must NOT read as a live call"
    );

    // ── `struct TsgoBackend` predicate ──
    assert!(
        has_live(
            "struct TsgoBackend {\n    tsgo_bin: String,\n}",
            "struct TsgoBackend"
        ),
        "the config-less backend struct must trip the guard"
    );
    // `TsgoOwnedBackend` (project-bound) must NOT trip the `struct TsgoBackend` needle.
    assert!(
        !has_live(
            "struct TsgoOwnedBackend {\n    tsgo_bin: String,\n}",
            "struct TsgoBackend"
        ),
        "the project-bound `TsgoOwnedBackend` must NOT trip the config-less needle"
    );

    // ── `impl ResilientBackend<TsgoTypeProvider> for TsgoBackend` predicate ──
    assert!(
        has_live(
            "impl ResilientBackend<TsgoTypeProvider> for TsgoBackend {",
            "impl ResilientBackend<TsgoTypeProvider> for TsgoBackend"
        ),
        "the config-less restart impl must trip the guard"
    );
    assert!(
        !has_live(
            "impl ResilientBackend<TsgoOwnedProvider> for TsgoOwnedBackend {",
            "impl ResilientBackend<TsgoTypeProvider> for TsgoBackend"
        ),
        "the project-bound owned restart impl must NOT trip the config-less needle"
    );

    // ── `pub fn new(` vs `pub fn new_owned(` discrimination ──
    assert!(
        has_live(
            "pub fn new(\n    provider: TsgoTypeProvider,",
            "pub fn new("
        ),
        "the config-less `pub fn new(` constructor must trip the guard"
    );
    assert!(
        !has_live(
            "pub fn new_owned(\n    provider: TsgoOwnedProvider,",
            "pub fn new("
        ),
        "the project-bound `pub fn new_owned(` must NOT trip the `pub fn new(` needle"
    );

    // ── `tsgo_resilient::new(` vs `tsgo_resilient::new_owned(` discrimination ──
    assert!(
        has_live(
            "            let resilient = tsgo_resilient::new(",
            "tsgo_resilient::new("
        ),
        "the config-less `tsgo_resilient::new(` call must trip the guard"
    );
    assert!(
        !has_live(
            "            let resilient = tsgo_resilient::new_owned(",
            "tsgo_resilient::new("
        ),
        "the project-bound `tsgo_resilient::new_owned(` call must NOT trip the \
         `tsgo_resilient::new(` needle"
    );

    // ── a comment mentioning a deleted symbol is final-state prose, not a violation ──
    assert!(
        !has_live(
            "    // The config-less `tsgo_resilient::new(` wrap was deleted.",
            "tsgo_resilient::new("
        ),
        "a comment naming the deleted call must NOT trip the guard"
    );
}
