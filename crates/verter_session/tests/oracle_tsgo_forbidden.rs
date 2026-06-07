//! The `tsgo`-forbidden-at-runtime guards for the TS7 oracle harness
//! (`docs/arch/u0-oracle-harness-design.md` §3 invariant 1, §4
//! `tsgo_not_reachable_from_resolver` / `oracle_consumption_path_has_no_tsgo_spawn`).
//!
//! `tsgo` is GENERATION-ONLY: the resolver / query-time path
//! (`resolve_named_symbol_with_audit` → `project_node_to_type_expr` →
//! `ProjectSemanticDispatch`) and the oracle CONSUMPTION path (the lift driver +
//! the typeinfo test module) must NEVER spawn or contact tsgo. tsgo lives behind
//! `verter_type_runtime`, on which `verter_session` has no dependency (fact 4) —
//! these guards PIN that, so introducing a runtime tsgo call (a
//! `verter_type_runtime` dependency, or a tsgo spawn in the oracle consumption
//! source) FAILS the gate rather than silently shelling to tsgo at query time.

use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

/// `verter_session`'s DEFAULT-build dependency closure excludes
/// `verter_type_runtime` (the crate that owns the tsgo LSP driver). The
/// non-dev `[dependencies]` table is the default build closure; a future
/// feature-gated generator would carry tsgo as an OPTIONAL / dev dependency, not
/// here. Discriminating: adding `verter_type_runtime` (or any `tsgo` crate) to
/// `[dependencies]` FAILS this guard.
#[test]
fn tsgo_not_reachable_from_resolver() {
    let cargo_toml = fs::read_to_string(Path::new(MANIFEST_DIR).join("Cargo.toml"))
        .expect("read verter_session Cargo.toml");
    let parsed: toml::Value = toml::from_str(&cargo_toml).expect("parse Cargo.toml");

    let deps = parsed
        .get("dependencies")
        .and_then(|d| d.as_table())
        .expect("verter_session has a [dependencies] table");

    for name in deps.keys() {
        assert!(
            !name.contains("type_runtime"),
            "verter_session [dependencies] must NOT carry `{name}` — tsgo \
             (behind verter_type_runtime) is GENERATION-ONLY and must never enter \
             the default resolver build closure"
        );
        assert!(
            !name.contains("tsgo"),
            "verter_session [dependencies] must NOT carry a tsgo crate `{name}` \
             on the default build closure"
        );
    }
}

/// The oracle CONSUMPTION path (the lift driver + the oracle test module source)
/// references no tsgo SPAWN / EXECUTION symbol. Scans the oracle source tree for
/// the concrete spawn/driver symbols (NOT the word "tsgo" in prose — the design
/// docs legitimately describe the generation side). Discriminating: a tsgo spawn
/// (`TsgoTypeProvider`, a `verter_type_runtime::` use, a `--lsp --stdio` child,
/// or a `get_hover(` call) added to the consumption path FAILS this guard.
#[test]
fn oracle_consumption_path_has_no_tsgo_spawn() {
    // Spawn/execution symbols only — never prose tokens like "tsgo" or "hover"
    // that the design references in doc comments.
    const FORBIDDEN: &[&str] = &[
        "verter_type_runtime",
        "TsgoTypeProvider",
        "--lsp --stdio",
        ".get_hover(",
        "Command::new",
    ];

    let oracle_root = Path::new(MANIFEST_DIR).join("src/typeinfo/typeinfo_tests/oracle");
    let registry =
        Path::new(MANIFEST_DIR).join("src/typeinfo/typeinfo_tests/oracle_query_specs.rs");

    let mut files: Vec<PathBuf> = Vec::new();
    collect_rs(&oracle_root, &mut files);
    if registry.exists() {
        files.push(registry);
    }
    assert!(
        !files.is_empty(),
        "expected oracle consumption-path source files to scan"
    );

    for file in &files {
        let src =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        for needle in FORBIDDEN {
            assert!(
                !src.contains(needle),
                "oracle consumption-path file {} references the forbidden tsgo \
                 spawn symbol `{needle}` — tsgo is GENERATION-ONLY",
                file.display()
            );
        }
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
