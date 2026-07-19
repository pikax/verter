//! PERF-0 Rail B — diagnostic-SET parity characterization (drives the real
//! `verter-tsc` binary; skips ONLY on genuine tsgo / node_modules absence).
//!
//! Pins the COMPLETE current (Full-mode, `HostConfig::default()`) tsgo diagnostic
//! MULTISET produced by `verter-tsc --noEmit -p <cases/fixtures/diagnostics/tsconfig.json>`,
//! and asserts EXACT MULTISET EQUALITY over the tuple key `(file, line, col, ts_code)`
//! WITH per-key COUNT:
//!   - a DROPPED diagnostic (missing tuple OR reduced count) -> RED;
//!   - an ADDED diagnostic (extra tuple OR raised count)     -> RED;
//!   - a CHANGED diagnostic (different code / line / col)     -> RED.
//!
//! Multiplicity is pinned (a `BTreeMap<key, count>`, not a `BTreeSet`): dropping 2
//! of 3 duplicate diagnostics at the same `(file,line,col,code)` MUST fail, so the
//! current 70 raw diagnostics are asserted, not a deduped 67.
//!
//! WHY THIS EXISTS (the parity oracle): the perf campaign (see
//! `docs/arch/host-mode-perf-design.md` §3/§4) will run verter-tsc as a Batch host
//! and later swap its checker backend to an in-memory tsgo `--api` client. The
//! codex ruling requires the diagnostic SET to stay IDENTICAL across those changes
//! ("Full <-> Batch — SAME tsc-parity diagnostic SET"; Fix 1 lands "only AFTER
//! PERF-0 parity passes"). This test lives next to the binary entry the future
//! mode swaps are wired into (`run()`/`invoke_checker`), so it automatically
//! re-runs as the parity oracle against every future mode. It also pins the
//! binary's EXIT STATUS (verter-tsc exits 1 when diagnostics exist), so a backend
//! that prints the same diagnostics but returns the wrong code, or crashes after
//! partial output, fails here.
//!
//! STRICTER THAN `diagnostics.rs`: that complementary test asserts PRESENCE
//! (`assert_has_error` / `assert_min_errors`); this one pins the COMPLETE multiset,
//! so it also catches a SPURIOUS added diagnostic and a SILENTLY DROPPED one.
//!
//! DISCRIMINATION CONTRACT:
//!   - The MULTISET key is `(file, line, col, ts_code)` with a count — it
//!     deliberately EXCLUDES the raw message text (version-volatile; TS2305 embeds
//!     the per-run random temp-dir path). Each expected tuple instead pins the most
//!     SPECIFIC STABLE message SUBSTRING that ALL `count` live diagnostics at the key
//!     must contain — the fixture-derived concrete types (`'number' is not
//!     assignable to type 'string'`), NOT framework-internal type names
//!     (`IntrinsicAttributes`, `InferDefault`) that drift with vue versions, and NOT
//!     the volatile temp path (TS2305 pins only `has no exported member
//!     'NonExistent'`). The message check is PER-OCCURRENCE (matched-substring count
//!     == pinned count), NOT `.any()`: for a duplicate key (count N>1) a drift in a
//!     SINGLE one of the N occurrences fails — the message shape is coupled to the
//!     multiplicity, not decoupled from it.
//!   - File paths are normalized to the FIXTURE-RELATIVE path (`src/Foo.vue`),
//!     preserving directory/membership so a wrong-dir / project-membership /
//!     source-map regression stays visible; only the unstable generated-stub
//!     content hash (and its per-run temp dir) is stripped (`Foo_<hex>.vue.ts` ->
//!     `Foo.vue.ts`). No absolute / platform / temp path is embedded
//!     (cross-platform rule).
//!   - A CLEAN-SFC rail asserts no diagnostic lands on a known-clean fixture.
//!
//! SKIP-vs-FAIL (NOT a stub, NOT `#[ignore]`, NOT a feature gate): a deps/tsgo
//! PREFLIGHT decides availability up front (mirroring `diagnostics.rs`'s
//! `setup_temp_project()` plus the binary's own `find_tsgo` discovery). When the
//! fixture node_modules OR tsgo are genuinely absent it prints `SKIP` and returns
//! (the pinned set is tsgo-specific; the tsc fallback's set differs). When tsgo IS
//! present (the canonical gate) it HARD-ASSERTS the full multiset — and a run that
//! parses ZERO diagnostics over the intentional-error fixture is a regression, NOT
//! a skip, so it PANICS. Never skip on `diags.is_empty()` alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Pinned expected diagnostic MULTISET (sorted) ────────────────────────
// Captured from the current `verter-tsc` run over
// `crates/verter_tsc/tests/cases/fixtures/diagnostics/`. Tuple =
// `(fixture_relative_path, line, col, ts_code, count, stable_message_substring)`.
// `count` pins multiplicity (raw total = 70; two keys repeat:
// `src/DirectiveErrors.vue(1,1) TS7006` x3, `src/GenericComp.vue(1,1) TS6196` x2).
// The final entry is the whole-program non-root diagnostic in `src/nonRootBad.ts` (the
// old per-root loop dropped it; the whole-program call surfaces it).
// Regenerate ONLY when the pinned pipeline output legitimately changes (a later
// perf block must prove this multiset is identical, or consciously re-pin it).
#[rustfmt::skip]
const EXPECTED: &[(&str, u32, u32, u32, usize, &str)] = &[
    ("src/ComposableErrors.vue", 11, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/ComposableErrors.vue", 11, 1, 6133, 1, "'bad' is declared but its value is never read"),
    ("src/ComposableErrors.vue", 13, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/CrossComponentErrors.vue", 6, 1, 2322, 1, "'boolean' is not assignable to type 'number'"),
    ("src/CrossComponentErrors.vue", 7, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/CrossComponentErrors.vue", 8, 1, 2322, 1, "'null' is not assignable to type 'string'"),
    ("src/CrossComponentErrors.vue", 9, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/CrossComponentErrors.vue", 13, 7, 2322, 1, "'\"unknown\"' is not assignable to type 'Status'"),
    ("src/CrossComponentErrors.vue", 16, 7, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/DirectiveErrors.vue", 1, 1, 7006, 3, "Parameter '___VERTER___slotInstance' implicitly has an 'any' type"),
    ("src/DirectiveErrors.vue", 4, 1, 6133, 1, "'vColor' is declared but its value is never read"),
    ("src/EmitErrors.vue", 8, 1, 2769, 1, "No overload matches this call"),
    ("src/EmitErrors.vue", 10, 1, 2769, 1, "No overload matches this call"),
    ("src/EmitErrors.vue", 12, 1, 2769, 1, "No overload matches this call"),
    ("src/GenericComp.vue", 1, 1, 2315, 1, "Type '___VERTER___attributes' is not generic"),
    ("src/GenericComp.vue", 1, 1, 6196, 2, "'T' is declared but never used"),
    ("src/GenericErrors.vue", 6, 1, 2322, 1, "'string' is not assignable to type 'User[]'"),
    ("src/GenericErrors.vue", 7, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/GenericErrors.vue", 8, 1, 2322, 1, "'boolean' is not assignable to type 'number'"),
    ("src/GenericErrors.vue", 17, 7, 2353, 1, "and 'name' does not exist in type '{ id: number; }'"),
    ("src/GenericInstanceErrors.vue", 5, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/GenericInstanceErrors.vue", 5, 1, 6133, 1, "'name' is declared but its value is never read"),
    ("src/ImportErrors.vue", 3, 1, 2305, 1, "has no exported member 'NonExistent'"),
    ("src/ImportErrors.vue", 3, 1, 6133, 1, "'NonExistent' is declared but its value is never read"),
    ("src/ImportErrors.vue", 9, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/ImportErrors.vue", 10, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/ImportErrors.vue", 11, 1, 2322, 1, "'boolean' is not assignable to type 'string'"),
    ("src/ImportErrors.vue", 12, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    // Generated public-API stub diagnostics (hash + per-run temp dir stripped).
    ("OptionsApiAdvanced.vue.ts", 25, 13, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/OptionsApiAdvanced.vue", 1, 1, 6196, 1, "'___VERTER___attributes' is declared but never used"),
    ("src/OptionsApiAdvanced.vue", 26, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    // The numeric prop on line 5 is valid. Vue slot-body isolation removes the
    // former foreign-React `children` diagnostic, leaving only the real bad value.
    ("src/OptionsApiConsumer.vue", 6, 24, 2322, 1, "'string' is not assignable to type 'number'"),
    ("OptionsApiErrors.vue.ts", 21, 13, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/OptionsApiErrors.vue", 1, 1, 6196, 1, "'___VERTER___attributes' is declared but never used"),
    ("src/OptionsApiErrors.vue", 22, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/PropErrors.vue", 6, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/PropErrors.vue", 7, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/PropErrors.vue", 8, 1, 2322, 1, "'boolean' is not assignable to type 'string'"),
    ("src/PropErrors.vue", 9, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/PropErrors.vue", 13, 7, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/PropErrors.vue", 15, 7, 2322, 1, "'boolean' is not assignable to type 'string'"),
    ("src/ReactivityErrors.vue", 6, 7, 2345, 1, "'number' is not assignable to parameter of type 'string[]'"),
    ("src/ReactivityErrors.vue", 10, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/ReactivityErrors.vue", 11, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/ReactivityErrors.vue", 12, 1, 2322, 1, "'boolean' is not assignable to type 'string'"),
    ("src/ReactivityErrors.vue", 13, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/ReactivityErrors.vue", 19, 1, 2322, 1, "'string[]' is not assignable to type 'number'"),
    ("src/ReactivityErrors.vue", 19, 1, 6133, 1, "'bad' is declared but its value is never read"),
    ("src/ScriptSetupErrors.vue", 6, 7, 2345, 1, "'string' is not assignable to parameter of type 'number'"),
    ("src/ScriptSetupErrors.vue", 10, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/ScriptSetupErrors.vue", 14, 1, 6133, 1, "'unusedVar' is declared but its value is never read"),
    ("src/ScriptSetupErrors.vue", 17, 1, 6133, 1, "'user' is declared but its value is never read"),
    ("src/ScriptSetupErrors.vue", 18, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/ScriptSetupErrors.vue", 19, 1, 2322, 1, "'number' is not assignable to type 'string'"),
    ("src/ScriptSetupErrors.vue", 20, 1, 2322, 1, "'boolean' is not assignable to type 'string'"),
    ("src/ScriptSetupErrors.vue", 21, 1, 2322, 1, "'string' is not assignable to type 'number'"),
    ("src/SlotErrors.vue", 10, 1, 2339, 1, "Property 'toFixed' does not exist on type 'boolean'"),
    ("src/SlotErrors.vue", 11, 1, 2339, 1, "Property 'toLowerCase' does not exist on type 'number'"),
    ("src/TemplateExprErrors.vue", 9, 10, 2339, 1, "Property 'length' does not exist on type '42'"),
    ("src/TemplateExprErrors.vue", 11, 10, 2345, 1, "'string' is not assignable to parameter of type 'number'"),
    ("src/TemplateExprErrors.vue", 13, 10, 2362, 1, "left-hand side of an arithmetic operation must be of type"),
    ("src/VModelErrors.vue", 5, 7, 2345, 1, "'number' is not assignable to parameter of type 'string'"),
    ("src/VModelErrors.vue", 8, 7, 2345, 1, "'boolean' is not assignable to parameter of type 'number'"),
    ("src/VModelErrors.vue", 11, 7, 2345, 1, "'string' is not assignable to parameter of type 'string[]'"),
    // WithDefaults: the assignable-TO type is `InferDefault<LooseRequired<Props>, …>`
    // (vue-version-volatile), so pin only the fixture-stable assignable-FROM type.
    ("src/WithDefaultsErrors.vue", 11, 1, 2322, 1, "'string' is not assignable to type"),
    ("src/WithDefaultsErrors.vue", 12, 1, 2322, 1, "'number' is not assignable to type"),
    // WHOLE-PROGRAM (non-root) diagnostic: `NonRootImport.vue` imports a clean
    // symbol from `src/nonRootBad.ts`, which ALSO carries a real TS2322. This file
    // is NOT a synthetic-tsconfig root (it enters the program only transitively),
    // so the old per-root loop NEVER queried it and the error was dropped; the
    // whole-program semantic call surfaces it, homed on its OWN path at (8,14).
    // This tuple is a real whole-program addition, not a relaxation.
    ("src/nonRootBad.ts", 8, 14, 2322, 1, "'string' is not assignable to type 'number'"),
];

/// Verter-tsc's expected exit code when the project has diagnostics. The binary
/// prints diagnostics then `process::exit(1)` (`crates/verter_tsc/src/main.rs:194`);
/// `2` is a fatal config error (`main.rs:134`), `0` is a clean check. The
/// intentional-error fixture always yields diagnostics, so the code is pinned at 1.
const EXPECTED_EXIT_CODE: i32 = 1;

/// Fixtures that must carry ZERO diagnostics in the current pipeline. Empirically
/// derived (every file absent from `EXPECTED`). NOTE: `src/GenericComp.vue` is NOT
/// clean — it currently emits TS2315 + TS6196 (a pre-existing generic-attrs
/// codegen artifact, pinned in `EXPECTED`), so it is deliberately excluded here.
const CLEAN_SFCS: &[&str] = &[
    "src/BaseButton.vue",
    "src/GenericList.vue",
    "src/StatusBadge.vue",
    "src/types.ts",
    // NonRootImport.vue is itself clean — its only role is to pull the erroring
    // non-root `nonRootBad.ts` into the program. Asserting it clean guards against
    // the whole-program error being mis-homed onto the importing SFC.
    "src/NonRootImport.vue",
];

// ── Diagnostic parsing (mirrors diagnostics.rs:34 `parse_diag_line`) ─────

#[derive(Debug)]
struct Diag {
    file: String,
    line: u32,
    col: u32,
    ts_code: u32,
    message: String,
}

fn parse_diagnostics(output: &str) -> Vec<Diag> {
    output.lines().filter_map(parse_diag_line).collect()
}

fn parse_diag_line(line: &str) -> Option<Diag> {
    let paren_start = line.find('(')?;
    let paren_end = line[paren_start..].find(')')? + paren_start;

    let file = &line[..paren_start];
    let coords = &line[paren_start + 1..paren_end];

    let mut parts = coords.splitn(2, ',');
    let line_n: u32 = parts.next()?.trim().parse().ok()?;
    let col_n: u32 = parts.next()?.trim().parse().ok()?;

    let rest = line[paren_end + 1..].trim();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim();

    let rest = if let Some(after) = rest.strip_prefix("error ") {
        after
    } else {
        rest.strip_prefix("warning ")?
    };

    let rest = rest.strip_prefix("TS")?;
    let colon = rest.find(':')?;
    let ts_code: u32 = rest[..colon].parse().ok()?;
    let message = rest[colon + 1..].trim().to_string();

    let file = file.replace('\\', "/");

    Some(Diag {
        file,
        line: line_n,
        col: col_n,
        ts_code,
        message,
    })
}

/// Normalize a diagnostic's file path to a stable, FIXTURE-RELATIVE identity.
///
/// `root` is the per-run temp PROJECT ROOT. Stripping it keeps a real source
/// diagnostic's membership-bearing path (`src/Foo.vue`) so a wrong-dir /
/// project-membership / source-map regression stays visible (it does NOT collapse
/// to a bare basename). The generated public-API stub surfaces as
/// `<transient TempDir>/Name_<contenthash>.vue.ts`: the containing dir is a per-run
/// `TempDir::new_in(root)` (non-reproducible) and `_<hex>` is a content hash —
/// strip BOTH so the stub identity is the stable `Name.vue.ts`. (PERF-3 replaces
/// this with deterministic in-project virtual names, at which point the directory
/// itself becomes pinnable and can be reinstated.)
fn normalize_file(raw: &str, root: &str) -> String {
    let raw = raw.replace('\\', "/");
    let root = root.replace('\\', "/");
    let root_trim = root.strip_suffix('/').unwrap_or(root.as_str());

    let rel = raw
        .strip_prefix(root_trim)
        .map(|r| r.trim_start_matches('/').to_string())
        // Fallback if tsgo canonicalized the prefix differently from `root`: anchor
        // on the fixture `src/` dir so a source diagnostic still pins `src/Foo.vue`.
        .or_else(|| raw.find("/src/").map(|i| raw[i + 1..].to_string()))
        .unwrap_or_else(|| raw.clone());

    if let Some(base) = rel.rsplit('/').next() {
        if let Some(stem) = base.strip_suffix(".vue.ts") {
            if let Some((name, hash)) = stem.rsplit_once('_') {
                if !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return format!("{name}.vue.ts");
                }
            }
        }
    }
    rel
}

// ── Setup helpers (mirror diagnostics.rs:203 `setup_temp_project`) ───────

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root")
        .to_path_buf()
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
        .join("fixtures")
        .join("diagnostics")
}

/// Copy the fixture project to a temp dir and link `node_modules`. Returns `None`
/// (skip) when the workspace's example `node_modules/vue` is absent.
fn setup_temp_project() -> Option<(tempfile::TempDir, PathBuf)> {
    let node_modules_src = workspace_root()
        .join("packages")
        .join("example")
        .join("node_modules");

    if !node_modules_src.join("vue").exists() {
        eprintln!("SKIP: packages/example/node_modules/vue not found — run `pnpm install` first");
        return None;
    }

    let temp = tempfile::TempDir::new().expect("failed to create temp dir");
    let temp_path = temp.path().to_path_buf();
    copy_dir_recursive(&fixture_dir(), &temp_path).expect("failed to copy fixture");

    let nm_dest = temp_path.join("node_modules");
    create_junction_or_symlink(&node_modules_src, &nm_dest);

    if !nm_dest.join("vue").exists() {
        eprintln!("SKIP: failed to create node_modules junction/symlink");
        return None;
    }

    Some((temp, temp_path))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(dest)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if let Ok(s) = status {
        if !s.success() {
            let _ = std::os::windows::fs::symlink_dir(src, dest);
        }
    }
}

#[cfg(not(windows))]
fn create_junction_or_symlink(src: &Path, dest: &Path) {
    let _ = std::os::unix::fs::symlink(src, dest);
}

// ── gated `--api` engine preflight ───────────────────────────────────────
//
// The `--noEmit` typecheck now runs the gated in-memory tsgo `--api` backend,
// which requires the `@typescript/typescript-*` native engine the wire gate
// pins (`typescript@7.0.2`, a workspace-root devDep installed by
// `pnpm install --frozen-lockfile`). The temp project's junctioned
// `packages/example/node_modules` does NOT carry that engine (it pins an older
// `typescript`), and the retired native-preview `tsgo` (npx cache) fails the wire
// gate — so this preflight resolves the gated engine once (an explicit
// `VERTER_TSGO_BIN` wins; else the SHARED `verter_tsgo_api` discovery against the
// workspace root) and threads it to the verter-tsc subprocess via
// `VERTER_TSGO_BIN`. A genuinely-absent engine SKIPs — the pinned multiset is
// engine-specific, so never assert against a missing or wrong engine.

/// Resolve the gated `--api` engine through the 4-tier toolchain resolver
/// (`VERTER_TSGO_BIN` wins; then shared PATH, project-local `node_modules`
/// under the workspace root — where `pnpm install --frozen-lockfile` installs
/// `typescript@7.0.2` — the update cache, and the bundled sidecar),
/// version-checked against the support policy.
fn resolve_gated_engine() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    verter_tsgo_api::toolchain::discovery::find_version_checked(&request)
        .ok()
        .map(|resolution| resolution.path)
}

// ── The parity oracle ───────────────────────────────────────────────────

#[test]
fn verter_tsc_diagnostic_set_parity() {
    // PREFLIGHT 1 — fixture deps. `None` ⇒ node_modules/vue genuinely absent.
    let (temp_dir, temp_path) = match setup_temp_project() {
        Some(t) => t,
        None => {
            // SKIP: fixture deps unavailable (tsgo cannot run hermetically).
            return;
        }
    };

    // PREFLIGHT 2 — the gated `--api` engine (what the in-memory typecheck now
    // uses). The pinned multiset is engine-specific: when the engine is genuinely
    // absent we SKIP rather than assert against a missing/wrong engine. The resolved
    // engine is threaded to the verter-tsc subprocess via `VERTER_TSGO_BIN` (the temp
    // project's junctioned node_modules does not carry the gated engine).
    let gated_engine = match resolve_gated_engine() {
        Some(p) => p,
        None => {
            eprintln!(
                "SKIP: gated tsgo `--api` engine (typescript@7.0.2) not found — the pinned set \
                 is engine-specific; set VERTER_TSGO_BIN or run `pnpm install --frozen-lockfile` \
                 in the workspace so the pinned engine is discoverable"
            );
            drop(temp_dir);
            return;
        }
    };

    let bin = env!("CARGO_BIN_EXE_verter-tsc");
    let output = Command::new(bin)
        .env("VERTER_TSGO_BIN", &gated_engine)
        .arg("--noEmit")
        .arg("-p")
        .arg(temp_path.join("tsconfig.json"))
        .output()
        .expect("failed to execute verter-tsc");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!(
        "=== gated tsgo --api engine: {} ===",
        gated_engine.display()
    );
    eprintln!("=== verter-tsc STDERR ===\n{stderr}");
    eprintln!("=== verter-tsc STDOUT ===\n{stdout}");

    // EXIT-STATUS pin (axis-B "exit code + diagnostic set match"): verter-tsc exits
    // 1 when diagnostics exist. A backend that prints the same diagnostics but
    // returns success, or crashes (wrong code) after partial output, fails here.
    assert_eq!(
        output.status.code(),
        Some(EXPECTED_EXIT_CODE),
        "verter-tsc exit code drifted: expected {EXPECTED_EXIT_CODE} (diagnostics present), got \
         {:?}.\nSTDERR:\n{stderr}\nSTDOUT:\n{stdout}",
        output.status.code()
    );

    let diags = parse_diagnostics(&stdout);

    // HARD FAIL (NOT skip): tsgo is present (preflight 2 passed), so a run that
    // parsed ZERO diagnostics over an intentional-error fixture is a real
    // regression — all diagnostics dropped, the output format changed, or the
    // binary exited early. The pinned multiset is non-empty, so this can never be a
    // legitimate empty run.
    assert!(
        !diags.is_empty(),
        "REGRESSION: the gated tsgo `--api` engine IS present ({}) but verter-tsc parsed ZERO \
         diagnostics over the intentional-error fixture (all diagnostics dropped / output format \
         changed / early exit / engine failed to connect).\nSTDERR:\n{stderr}\nSTDOUT:\n{stdout}",
        gated_engine.display()
    );

    let root_str = temp_path.to_string_lossy().replace('\\', "/");
    let actual: Vec<(String, u32, u32, u32, String)> = diags
        .iter()
        .map(|d| {
            (
                normalize_file(&d.file, &root_str),
                d.line,
                d.col,
                d.ts_code,
                d.message.clone(),
            )
        })
        .collect();

    // MULTISET equality over `(file, line, col, ts_code)` WITH per-key count. A
    // `BTreeSet` would dedup multiplicity (dropping 2 of 3 duplicate TS7006 stays
    // green); the count map pins the exact 70 raw diagnostics.
    type Key = (String, u32, u32, u32);
    let mut actual_counts: BTreeMap<Key, usize> = BTreeMap::new();
    for (f, l, c, code, _) in &actual {
        *actual_counts.entry((f.clone(), *l, *c, *code)).or_insert(0) += 1;
    }
    let mut expected_counts: BTreeMap<Key, usize> = BTreeMap::new();
    for (f, l, c, code, count, _) in EXPECTED {
        *expected_counts
            .entry(((*f).to_string(), *l, *c, *code))
            .or_insert(0) += *count;
    }

    let mut mismatches: Vec<String> = Vec::new();
    for (k, &ec) in &expected_counts {
        let ac = actual_counts.get(k).copied().unwrap_or(0);
        if ac != ec {
            mismatches.push(format!("{k:?}: pinned count {ec}, actual {ac}"));
        }
    }
    for (k, &ac) in &actual_counts {
        if !expected_counts.contains_key(k) {
            mismatches.push(format!("{k:?}: NOT in pinned set, actual count {ac}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "diagnostic MULTISET drifted from the pinned parity set (dropped / added / changed / \
         re-multiplied diagnostic):\n{}\n\nFull current multiset:\n{actual_counts:#?}",
        mismatches.join("\n")
    );

    // PER-OCCURRENCE message-shape check: at every pinned key the COUNT of live
    // diagnostics whose message contains the stable substring must EQUAL the key's
    // pinned `count` — `matched == count`, NOT `.any()` (some-match). This is the
    // multiplicity↔message COUPLING: for a duplicate key (count N>1 — the two such
    // keys, `DirectiveErrors.vue(1,1) TS7006` ×3 and `GenericComp.vue(1,1) TS6196`
    // ×2, each carry N IDENTICAL messages, empirically verified) it pins ALL N
    // occurrences, so if ANY single one of the N drifts (its message loses the
    // substring while the other N-1 keep it ⇒ matched == N-1) the check goes RED.
    // A bare `.any()` would stay green on that partial drift (one surviving match is
    // enough), a false-green this coupling closes. Together with the multiset count
    // check above (total live at the key == count), this proves EVERY one of the
    // `count` live diagnostics at the key carries the pinned message shape. Still
    // robust to tsgo patch-bump / vue-version rewording: the substrings pin
    // fixture-derived concrete types, not framework-internal ones.
    for &(f, l, c, code, count, sub) in EXPECTED {
        let matched = actual
            .iter()
            .filter(|(af, al, ac, acode, msg)| {
                af == f && *al == l && *ac == c && *acode == code && msg.contains(sub)
            })
            .count();
        assert_eq!(
            matched,
            count,
            "message-shape parity drift for TS{code} at {f}:({l},{c}): expected ALL {count} live \
             diagnostic(s) at that key to contain {sub:?}, but {matched} did (a single \
             duplicate-occurrence message drift fails here, not only an all-occurrence \
             drift).\nLive messages at that key: {:#?}",
            actual
                .iter()
                .filter(|(af, al, ac, acode, _)| af == f && *al == l && *ac == c && *acode == code)
                .map(|(_, _, _, _, m)| m)
                .collect::<Vec<_>>()
        );
    }

    // CLEAN-SFC rail: a known-clean fixture must carry NO diagnostic — catches a
    // spurious added diagnostic on a file that should type-check cleanly.
    for (f, l, c, code, msg) in &actual {
        assert!(
            !CLEAN_SFCS.contains(&f.as_str()),
            "spurious diagnostic on known-clean fixture {f}: TS{code} at ({l},{c}): {msg}"
        );
        // Source-map remapping guard: no diagnostic may point at a raw `.tsx`
        // temp carrier (it must remap to the `.vue` source or a stable stub).
        assert!(
            !f.ends_with(".tsx"),
            "diagnostic points at a raw .tsx temp carrier (source-map remap regression): \
             {f} TS{code} at ({l},{c})"
        );
    }

    eprintln!(
        "PARITY OK: {} live diagnostics exactly matched the pinned multiset (sum {})",
        actual.len(),
        expected_counts.values().sum::<usize>()
    );
    drop(temp_dir);
}
