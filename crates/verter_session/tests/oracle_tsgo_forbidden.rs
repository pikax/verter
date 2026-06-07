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
/// `verter_type_runtime` (the crate that owns the tsgo LSP driver). tsgo is
/// GENERATION-ONLY; the snapshot generator + the §4 generation SPIKE live behind
/// the `oracle-gen` feature (`docs/arch/u0-oracle-harness-design.md` §3 inv 1 —
/// "a separate dev-only / feature-gated tool target `#[cfg(feature = "oracle-gen")]`").
/// So `verter_type_runtime` is allowed in `[dependencies]` ONLY as an OPTIONAL dep
/// that the `default` feature set does NOT activate and that ONLY the `oracle-gen`
/// feature turns on — never an unconditional dep. With the feature off (the default
/// gate, production, the canonical `cargo nextest run --workspace` / `cargo test -p
/// verter_session` runs), tsgo is absent from the closure.
///
/// Discriminating: an UNCONDITIONAL `verter_type_runtime` dep, an `optional = false`
/// one, a `default`-feature that activates it, or a `tsgo`-named crate all FAIL.
#[test]
fn tsgo_not_reachable_from_resolver() {
    let cargo_toml = fs::read_to_string(Path::new(MANIFEST_DIR).join("Cargo.toml"))
        .expect("read verter_session Cargo.toml");
    let parsed: toml::Value = toml::from_str(&cargo_toml).expect("parse Cargo.toml");

    if let Err(why) = tsgo_dep_is_generation_only(&parsed) {
        panic!(
            "verter_session Cargo.toml violates the tsgo-generation-only rule: {why}"
        );
    }
}

/// PURE checker for the tsgo-generation-only rule (so it is discriminating over
/// SYNTHETIC manifests, not only the live one): tsgo, behind `verter_type_runtime`,
/// may appear in `[dependencies]` ONLY as `optional = true`, with `default` NOT
/// activating it and the `oracle-gen` feature activating it. Any `tsgo`-named dep is
/// banned outright.
fn tsgo_dep_is_generation_only(parsed: &toml::Value) -> Result<(), String> {
    let deps = parsed
        .get("dependencies")
        .and_then(|d| d.as_table())
        .ok_or("missing [dependencies] table")?;

    // No tsgo-named crate may appear at all.
    for name in deps.keys() {
        if name.contains("tsgo") {
            return Err(format!(
                "[dependencies] carries a tsgo crate `{name}` — tsgo is generation-only"
            ));
        }
    }

    let empty = toml::value::Table::new();
    let features = parsed
        .get("features")
        .and_then(|f| f.as_table())
        .unwrap_or(&empty);
    let feature_activates = |feat: &str, dep: &str| -> bool {
        features
            .get(feat)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().any(|e| {
                    e.as_str()
                        .map(|s| s == dep || s == format!("dep:{dep}") || s.starts_with(&format!("{dep}/")) || s.starts_with(&format!("dep:{dep}/")))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    match deps.get("verter_type_runtime") {
        None => {
            // Absent from the default closure — the strongest form of the invariant.
            // Belt-and-suspenders: no feature may reference a dep that is not declared.
            if feature_activates("oracle-gen", "verter_type_runtime") {
                return Err(
                    "`oracle-gen` activates `dep:verter_type_runtime` but it is not declared \
                     in [dependencies]"
                        .into(),
                );
            }
            Ok(())
        }
        Some(spec) => {
            // Present — must be an OPTIONAL dep the default set does not activate and
            // only `oracle-gen` turns on.
            let table = spec.as_table().ok_or(
                "`verter_type_runtime` must be a table `{ path = .., optional = true }`, \
                 not a bare version string (a bare entry is non-optional)",
            )?;
            let optional = table
                .get("optional")
                .and_then(|o| o.as_bool())
                .unwrap_or(false);
            if !optional {
                return Err(
                    "`verter_type_runtime` is in [dependencies] but NOT `optional = true` — \
                     tsgo would enter the default build closure"
                        .into(),
                );
            }
            if feature_activates("default", "verter_type_runtime") {
                return Err(
                    "the `default` feature activates `verter_type_runtime` — tsgo must stay off \
                     the default gate"
                        .into(),
                );
            }
            if !feature_activates("oracle-gen", "verter_type_runtime") {
                return Err(
                    "`verter_type_runtime` is optional but the `oracle-gen` feature does not \
                     activate `dep:verter_type_runtime` — it would be unreachable / mis-gated"
                        .into(),
                );
            }
            Ok(())
        }
    }
}

/// Discriminating self-test of `tsgo_dep_is_generation_only` over SYNTHETIC
/// manifests — proves the checker rejects every way tsgo could leak into the
/// default closure and accepts ONLY the optional + `oracle-gen`-gated form,
/// independent of the live Cargo.toml's current state.
#[test]
fn tsgo_generation_only_checker_discriminates() {
    let parse = |s: &str| toml::from_str::<toml::Value>(s).expect("parse synthetic manifest");

    // (1) Absent dep + no feature reference → OK (the pre-cutover state).
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\nserde = \"1\"\n[features]\ndefault = []\n"
    ))
    .is_ok());

    // (2) Correct optional + oracle-gen-gated form → OK (the post-cutover state).
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\nverter_type_runtime = { path = \"../x\", optional = true }\n\
         [features]\ndefault = []\noracle-gen = [\"dep:verter_type_runtime\"]\n"
    ))
    .is_ok());

    // (3) UNCONDITIONAL (bare version string) dep → REJECT.
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\nverter_type_runtime = \"1\"\n[features]\ndefault = []\n"
    ))
    .is_err());

    // (4) Present as a table but optional = false → REJECT.
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\nverter_type_runtime = { path = \"../x\", optional = false }\n\
         [features]\noracle-gen = [\"dep:verter_type_runtime\"]\n"
    ))
    .is_err());

    // (5) Optional but the `default` feature activates it → REJECT.
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\nverter_type_runtime = { path = \"../x\", optional = true }\n\
         [features]\ndefault = [\"dep:verter_type_runtime\"]\noracle-gen = [\"dep:verter_type_runtime\"]\n"
    ))
    .is_err());

    // (6) Optional but NO feature activates it → REJECT (mis-gated / unreachable).
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\nverter_type_runtime = { path = \"../x\", optional = true }\n\
         [features]\ndefault = []\noracle-gen = []\n"
    ))
    .is_err());

    // (7) A tsgo-named crate is banned outright.
    assert!(tsgo_dep_is_generation_only(&parse(
        "[dependencies]\ntsgo_client = \"1\"\n[features]\ndefault = []\n"
    ))
    .is_err());
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

/// The tsgo generator pins `@typescript/native-preview` to the EXACT preview
/// version the snapshots were captured under (`docs/arch/u0-oracle-harness-design.md`
/// §Q2 "Env pinning" — `tsgo_version = "7.0.0-dev.20260526.1"`). A floating
/// `"latest"` would let an upgrade silently change tsgo answers without
/// invalidating any snapshot. Discriminating: reverting the workspace
/// `package.json` to `"latest"` (or any other version) FAILS this guard.
#[test]
fn tsgo_version_is_pinned() {
    const PINNED_TSGO_VERSION: &str = "7.0.0-dev.20260526.1";

    // MANIFEST_DIR is `crates/verter_session`; the workspace `package.json` is at
    // the repo root (two levels up).
    let pkg_json = Path::new(MANIFEST_DIR)
        .join("../../package.json")
        .canonicalize()
        .expect("canonicalize workspace package.json path");
    let raw = fs::read_to_string(&pkg_json)
        .unwrap_or_else(|e| panic!("read {}: {e}", pkg_json.display()));
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("parse package.json");

    let pinned = parsed
        .get("devDependencies")
        .and_then(|d| d.get("@typescript/native-preview"))
        .or_else(|| {
            parsed
                .get("dependencies")
                .and_then(|d| d.get("@typescript/native-preview"))
        })
        .and_then(|v| v.as_str())
        .expect("package.json declares @typescript/native-preview");

    assert_eq!(
        pinned, PINNED_TSGO_VERSION,
        "@typescript/native-preview MUST be pinned to the exact oracle tsgo \
         version `{PINNED_TSGO_VERSION}`, not `{pinned}` — a floating version \
         would let an upgrade silently change tsgo hover answers without \
         invalidating the checked-in snapshots"
    );
    assert_ne!(
        pinned, "latest",
        "the tsgo pin must not be the floating `latest` tag"
    );
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
