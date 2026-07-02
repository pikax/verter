//! Freshness guard for the generated typeinfo manifest data.
//!
//! Every file under `crates/verter_session/tests/manifest_data/`
//! (`typeinfo_ignored_test_manifest_rows.rs`,
//! `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`) is
//! produced from `scripts/gen-typeinfo-ignore-manifest.py`
//! (`pnpm gen:typeinfo-manifest`) — the SOLE writer of all three files.
//! The authoritative §10.4.1 row→block partition feeds ONLY each
//! `IgnoredTestRow`'s `block_id` (joined with the live `#[ignore]`
//! discovery and the Capability Map). The `AdditionalProofRow` table and
//! the `TYPEINFO_PARITY_BLOCKS` block contracts (each block's
//! required_guards/verification_labels/prereqs/mechanisms) come from the
//! generator's own Python maps, NOT from §10.4.1. Whenever the generator
//! or its inputs change, the committed files must be regenerated and
//! committed in the same change.
//!
//! This guard mirrors the proto-bindings freshness pattern
//! (`crates/verter_protocol/tests/typeinfo_proto_ts_freshness.rs`): it
//! invokes the generator in `--check` mode, which regenerates each
//! tracked output in memory and byte-compares against the committed file
//! WITHOUT writing the tree, exiting non-zero (status 6) on any drift and
//! naming the stale file(s). A hand-edit to ANY generated manifest file —
//! or a generator change without regen — makes this test FAIL.
//!
//! The check gracefully skips when `python3` is absent (running `cargo
//! test` on a machine without python), exactly as the proto freshness
//! test skips when `buf` is absent. CI ships python3, so the
//! discrimination holds in CI.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

/// Locate a runnable `python3` interpreter.
///
/// 1. Prefer an explicit `PYTHON3` / `PYTHON` env override (CI hook).
/// 2. Fall back to `python3` / `python` on `PATH`.
/// 3. Every candidate must pass [`python_is_functional`] — existence alone
///    is NOT enough: the Microsoft Store "App Execution Alias" ships a
///    zero-byte `python.exe` reparse point under `WindowsApps` for which
///    `is_file()` is true but which only prints an install hint and exits
///    non-zero. Accepting it would make the freshness check report a bogus
///    "manifest is STALE" instead of skipping.
///
/// Returns `Err(rejected)` — every probed-and-rejected candidate — when no
/// functional interpreter resolves; the caller then skips gracefully with
/// an informative reason, mirroring how the proto freshness test skips
/// when `buf` is absent.
fn locate_python(workspace_root: &Path) -> Result<PathBuf, Vec<PathBuf>> {
    let _ = workspace_root;
    let mut rejected: Vec<PathBuf> = Vec::new();
    for var in ["PYTHON3", "PYTHON"] {
        let Some(val) = std::env::var_os(var) else {
            continue;
        };
        let named = PathBuf::from(&val);
        // An ABSOLUTE override always resolves through `which_on_path`, so
        // Windows executable-extension variants are probed alongside the
        // bare path (`PYTHON3=C:\Python312\python` → `python.exe`) even when
        // the extension-less name itself is not a file. A relative existing
        // file is probed as-is; a bare name resolves via `PATH`.
        let candidates = if !named.is_absolute() && named.is_file() {
            vec![named]
        } else {
            which_on_path(&named)
        };
        match locate_first_functional(candidates) {
            Ok(python) => return Ok(python),
            Err(mut probed) => rejected.append(&mut probed),
        }
    }
    for name in ["python3", "python"] {
        match locate_first_functional(which_on_path(Path::new(name))) {
            Ok(python) => return Ok(python),
            Err(mut probed) => rejected.append(&mut probed),
        }
    }
    Err(rejected)
}

/// Probe `candidates` in order with [`python_is_functional`] and return the
/// FIRST functional interpreter; `Err` carries every probed-and-rejected
/// candidate. Iterating past rejected entries (instead of stopping at the
/// first candidate) is what lets the locator skip a non-functional
/// front-of-`PATH` stub — e.g. the MS-Store alias shadowing a real install
/// later on `PATH` — instead of skipping vacuously.
fn locate_first_functional(candidates: Vec<PathBuf>) -> Result<PathBuf, Vec<PathBuf>> {
    let mut rejected = Vec::new();
    for candidate in candidates {
        if python_is_functional(&candidate) {
            return Ok(candidate);
        }
        rejected.push(candidate);
    }
    Err(rejected)
}

/// Minimal `which`: returns EVERY existing entry for `name` on `PATH`
/// (honouring Windows executable extensions), in `PATH` order. An ABSOLUTE
/// `name` skips `PATH` but gets the SAME Windows extension expansion (bare
/// name first, then `.exe`/`.bat`/`.cmd` variants), so an extension-less
/// absolute override (`PYTHON3=C:\Python312\python`) still resolves the
/// `python.exe` beside it. Returning all matches (not the first) lets the
/// caller probe past a non-functional front-of-`PATH` entry — e.g. the
/// MS-Store alias stub shadowing a real install later on `PATH`.
fn which_on_path(name: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if name.is_absolute() {
        if name.is_file() {
            found.push(name.to_path_buf());
        }
        if cfg!(windows) {
            for ext in ["exe", "bat", "cmd"] {
                let candidate = name.with_extension(ext);
                if candidate.is_file() {
                    found.push(candidate);
                }
            }
        }
        return found;
    }
    let Some(path) = std::env::var_os("PATH") else {
        return found;
    };
    for dir in std::env::split_paths(&path) {
        let base = dir.join(name);
        if base.is_file() {
            found.push(base.clone());
        }
        if cfg!(windows) {
            for ext in ["exe", "bat", "cmd"] {
                let candidate = base.with_extension(ext);
                if candidate.is_file() {
                    found.push(candidate);
                }
            }
        }
    }
    found
}

/// Functional probe — the acceptance authority for python candidates. A
/// candidate is a usable interpreter ONLY when `<candidate> --version`
/// spawns, exits 0, AND prints a real `Python 3.x` version banner (matched
/// case-insensitively on the trimmed combined stdout+stderr). The MS-Store
/// alias stub fails on both counts (prints "Python was not found ..." and
/// exits non-zero); a broken shim that exits 0 without a banner also fails.
fn python_is_functional(candidate: &Path) -> bool {
    let Ok(output) = Command::new(candidate).arg("--version").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let combined = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    combined.trim().to_lowercase().contains("python 3.")
}

/// `true` when any path component is `WindowsApps` (case-insensitive) — the
/// Microsoft Store app-execution-alias directory. Used ONLY to enrich the
/// skip reason; [`python_is_functional`] is the acceptance authority.
fn candidate_is_windows_apps_alias(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("WindowsApps")
    })
}

/// Byte-equality freshness discriminator: run the manifest generator in
/// `--check` mode (regenerate-in-memory + byte-compare, no tree write) and
/// assert it reports NO drift across EVERY tracked output file
/// (`typeinfo_ignored_test_manifest_rows.rs`,
/// `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`). Any
/// divergence — a hand-edit to a generated file, or a generator change
/// committed without regenerating — surfaces as a non-zero exit (status 6)
/// and the generator's named-file diff is echoed in the panic message.
#[test]
fn typeinfo_manifest_files_are_byte_equal_to_regenerated_generator_output() {
    // `locate_python` reads the process-global `PATH`; hold the probe-env
    // lock so a fixture `PATH` installed by a parallel probe test can never
    // bleed into the real interpreter lookup.
    let _env = lock_probe_env();
    let root = workspace_root();
    let script = root.join("scripts").join("gen-typeinfo-ignore-manifest.py");
    assert!(
        script.is_file(),
        "manifest generator script missing at {}",
        script.display(),
    );

    let python = match locate_python(&root) {
        Ok(python) => python,
        Err(rejected) => {
            // Skip gracefully — and loudly — when no FUNCTIONAL python3
            // resolves (e.g. running `cargo test` on a python-free machine,
            // or one where only the MS-Store alias stub exists), exactly as
            // the proto freshness test skips when `buf` is absent. CI ships
            // python3, so the discrimination holds in CI.
            let mut reason = String::from(
                "skipping manifest freshness check: no functional `python3`/`python` \
                 found via $PYTHON3/$PYTHON or on `PATH`.",
            );
            for candidate in &rejected {
                let note = if candidate_is_windows_apps_alias(candidate) {
                    " (Microsoft Store app-execution-alias stub, not a real interpreter)"
                } else {
                    ""
                };
                reason.push_str(&format!(
                    "\n  probed and rejected: {}{note}",
                    candidate.display(),
                ));
            }
            reason.push_str(
                "\nInstall python3 (CI ships it) to run \
                 `python3 scripts/gen-typeinfo-ignore-manifest.py --check`.",
            );
            eprintln!("{reason}");
            return;
        }
    };

    let output = Command::new(&python)
        .arg(&script)
        .arg("--check")
        .current_dir(&root)
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "invoke `{} {} --check`: {err}",
                python.display(),
                script.display(),
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the committed typeinfo manifest data is STALE w.r.t. \
         `scripts/gen-typeinfo-ignore-manifest.py`. The generator is the SOLE \
         writer of `crates/verter_session/tests/manifest_data/*.rs`; regenerate \
         with `pnpm gen:typeinfo-manifest` and commit the result.\n\
         generator exit: {status}\n\
         --- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
        status = output.status,
    );
}

/// Discriminating per-block-count pin for the lifted rows. Each assertion is
/// pinned to the exact committed lift partition — so reverting (or
/// mis-counting) any lift's manifest re-partition breaks this test. The
/// committed partition: `U2.QUERY_VALUE_DOMAIN` owns 21 rows (all lifted —
/// the 2 index-signature publications, the 8 utility-reducer lifts, the
/// 10 class-surface-era pure-reduction lifts, and the 1 module-augmentation
/// namespace alias-chain lift, whose measured trace terminates at
/// {ResolveDecl, Instantiate(, TypeOf)}); `U2.INDEXED_ACCESS` owns 23 (14
/// lifted — incl. the 2 JSX parametric intrinsic-lookup rows re-homed in from
/// `U2.JSX_FOUNDATIONS`); `U2.UTILITIES` owns 32; `U2.MAPPED_TEMPLATE` owns 19 (6 lifted);
/// `U2.CLASS_SURFACES` owns 38 (5 lifted — the class typeof-path rows whose
/// trace dispatches `ResolveClassSurface` + `ProjectPath`);
/// `U2.MODULE_AUGMENTATION` owns 4 (0 lifted — the 4 remaining rows whose
/// `as const` / bare `typeof` / `typeof import` / `import("…").X` source
/// bodies are gate-rejected by the oracle source-walk stay `Ignored`; the
/// `typeof import(...)["default"]` row RE-LIFTED to `U2.INDEXED_ACCESS` once
/// the readonly was fixed by decoupling per-property `as const` from
/// readonly).
#[test]
fn manifest_block_counts_reflect_lifts() {
    let rows = workspace_root()
        .join("crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs");
    let src =
        std::fs::read_to_string(&rows).unwrap_or_else(|e| panic!("read {}: {e}", rows.display()));
    let count = |needle: &str| src.matches(needle).count();

    // Per-block generated row counts (the honest override distribution).
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2QueryValueDomain,"),
        21,
        "U2.QUERY_VALUE_DOMAIN must own 21 rows: the 2 lifted index-signature \
         publication rows, the 8 U2.UTILITIES reducer rows, the 10 \
         class-surface-era pure-reduction rows, and the 1 module-augmentation \
         namespace alias-chain row — every row whose measured trace terminates \
         at {{ResolveDecl, Instantiate(, TypeOf)}}",
    );
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2IndexedAccess,"),
        23,
        "U2.INDEXED_ACCESS must own 23 rows after the 2 brand-tag index chains \
         and the 2 decoration-invariance indexed-access rows moved IN from \
         U2.CLASS_SURFACES (14 → 18), the 3 module-augmentation indexed-member \
         projection rows (the `as const` typeof indexed member + the `typeof \
         import(...)[\"default\"]` default-export value projection + the `typeof \
         import(...)[\"leafName\"]` named-value projection) moved IN from \
         U2.MODULE_AUGMENTATION (18 → 21), and the 2 JSX parametric \
         intrinsic-lookup rows (`IntrinsicPropsFor<\"div\">` / \
         `IntrinsicPropsFor<\"span\">`) moved IN from U2.JSX_FOUNDATIONS on their \
         measured-trace lifts (21 → 23)",
    );
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2Utilities,"),
        32,
        "U2.UTILITIES must own 32 rows after the 2 built-in modifier-utility rows \
         moved to U2.MAPPED_TEMPLATE (42 → 40) and the 8 utility-reducer lifts \
         moved to their measured production block U2.QUERY_VALUE_DOMAIN (40 → 32)",
    );
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2MappedTemplate,"),
        19,
        "U2.MAPPED_TEMPLATE must own 19 rows after the 2 built-in modifier-utility \
         rows arrived lifted (16 → 18) and `wide_deep_projected_token` moved in on \
         the IndexedAccess-reduction lift (18 → 19)",
    );

    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2ModuleAugmentation,"),
        4,
        "U2.MODULE_AUGMENTATION must own 4 rows after the 4 lifted rows (the \
         `as const` typeof indexed member + the `typeof import(...)[\"default\"]` \
         default-export value projection + the `typeof import(...)[\"leafName\"]` \
         named-value projection + the namespace alias-chain row) re-homed OUT on \
         their measured-trace lifts (8 → 4); the remaining 4 rows stay `Ignored` \
         (all gate-rejected at the oracle source-walk: `Reject(ConstAssertion)`, \
         two `Reject(DeferredConstruct(\"typeof\"/\"typeof-import\"))`, and \
         `Reject(DeferredConstruct(\"import-type\"))`)",
    );

    // Lifted-status counts.
    assert_eq!(
        count("status: IgnoreStatus::Lifted {"),
        46,
        "exactly 46 IgnoredTestRows must carry `status: Lifted` (the 19 \
         pre-class-surface lifts + the 19 class-surface-era lifts + the 4 \
         module-augmentation-era lifts + the 2 JSX-era lifts + the 2 \
         mapped-template-era lifts)",
    );
    assert_eq!(
        count(
            "status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2QueryValueDomain }"
        ),
        21,
        "the 2 index-signature lifts, the 8 utility-reducer lifts, the 10 \
         class-surface-era pure-reduction lifts, and the 1 module-augmentation \
         namespace alias-chain lift must record their lifting block \
         as U2.QUERY_VALUE_DOMAIN",
    );
    assert_eq!(
        count("status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2IndexedAccess }"),
        14,
        "the 2 terminal indexed-access projection lifts, the 3 keyof-expansion \
         lifts, the 2 brand-tag index-chain lifts, the 2 \
         decoration-invariance lifts, the 3 module-augmentation \
         indexed-member projection lifts, and the 2 JSX parametric \
         intrinsic-lookup lifts must record their lifting block as \
         U2.INDEXED_ACCESS",
    );
    assert_eq!(
        count("status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2MappedTemplate }"),
        6,
        "the 2 built-in modifier-utility lifts + the wide/deep literal-union \
         projection lift + the `-?` optional-remover (`mapped_modifier_minus_optional`) \
         lift + the 2 mapped-template-era lifts (the `RecordTemplateRootSlot` \
         string-literal index-chain row + the `CounterHandlers` key-remap \
         mapped-type row) must record their lifting block as U2.MAPPED_TEMPLATE",
    );

    assert_eq!(
        count("status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2ClassSurfaces }"),
        5,
        "the 5 class typeof-path lifts (static inheritance ×2, static-generic \
         instantiation, `.prototype` extraction ×2 — the rows whose measured \
         trace dispatches ResolveClassSurface + ProjectPath) must record their \
         lifting block as U2.CLASS_SURFACES",
    );

    // Total ignored (status: Ignored) rows after 46 lifts.
    assert_eq!(
        count("status: IgnoreStatus::Ignored"),
        316,
        "exactly 316 IgnoredTestRows must remain `Ignored` (362 total − 46 lifted)",
    );
}

// ---------------------------------------------------------------------------
// No-vacuous-skip guardrails for the functional python probe.
//
// `is_file()`-only acceptance regressed on Windows machines without a real
// python install: the Microsoft Store app-execution-alias stub (a zero-byte
// `python.exe` reparse point under `WindowsApps` for which `is_file()` is
// true) passed the existence check, then failed at generator time, and the
// freshness assertion panicked with a bogus "manifest is STALE". These tests
// pin BOTH poles of the probe: a stub is rejected (the skip fires instead of
// a false STALE) and a functional interpreter is accepted (the freshness body
// actually runs when python is installed — the skip cannot become vacuous).
// ---------------------------------------------------------------------------

/// Serializes every test that READS or WRITES the process-global `PATH`
/// variable. `std::env::set_var` mutates whole-process state and the test
/// harness runs tests on parallel threads, so a fixture `PATH` installed by
/// one test must never be observable from another test's `PATH` lookup
/// (`which_on_path` / `locate_python`).
static PROBE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_probe_env() -> std::sync::MutexGuard<'static, ()> {
    PROBE_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Scope guard that swaps the process `PATH` to a fixture value and restores
/// the prior value on drop — INCLUDING on panic (drop runs during unwind) —
/// so a fixture `PATH` can never leak past the owning test.
struct PathEnvGuard {
    saved: Option<std::ffi::OsString>,
}

impl PathEnvGuard {
    fn swap(fixture_path: &std::ffi::OsStr) -> Self {
        let saved = std::env::var_os("PATH");
        std::env::set_var("PATH", fixture_path);
        Self { saved }
    }
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        match self.saved.take() {
            Some(previous) => std::env::set_var("PATH", previous),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// Temp-dir guard: removes the fixture directory (and the fakes inside)
/// even when the owning test panics.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unique_probe_fixture_dir(tag: &str) -> (PathBuf, TempDirGuard) {
    let dir = std::env::temp_dir().join(format!(
        "verter_manifest_python_probe_{tag}_{pid}",
        pid = std::process::id(),
    ));
    std::fs::create_dir_all(&dir).expect("create probe fixture dir");
    let guard = TempDirGuard(dir.clone());
    (dir, guard)
}

/// Writes a fake "python" executable that prints `banner` and exits with
/// `exit_code`. Windows: a `.cmd` shim; elsewhere: an executable `sh`
/// script. Hermetic — lives entirely under [`std::env::temp_dir`].
fn write_fake_interpreter(dir: &Path, stem: &str, banner: &str, exit_code: i32) -> PathBuf {
    #[cfg(windows)]
    {
        let path = dir.join(format!("{stem}.cmd"));
        std::fs::write(
            &path,
            format!("@echo off\r\necho {banner}\r\nexit /b {exit_code}\r\n"),
        )
        .expect("write fake interpreter");
        path
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(stem);
        std::fs::write(
            &path,
            format!("#!/bin/sh\necho \"{banner}\"\nexit {exit_code}\n"),
        )
        .expect("write fake interpreter");
        let mut perms = std::fs::metadata(&path)
            .expect("stat fake interpreter")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod fake interpreter");
        path
    }
}

/// The MS-Store-style stub — prints "Python was not found", exits non-zero —
/// is rejected, and so is an exit-0 shim without a version banner: existence
/// (`is_file()`) is NOT functionality.
#[test]
fn python_probe_rejects_non_functional_store_stub() {
    let (dir, _guard) = unique_probe_fixture_dir("stub");
    let stub = write_fake_interpreter(&dir, "python_stub", "Python was not found", 9009);
    assert!(
        stub.is_file(),
        "fixture must reproduce the trap: the stub IS a file on disk",
    );
    assert!(
        !python_is_functional(&stub),
        "a stub that prints an install hint and exits non-zero must be \
         rejected — accepting it turns the graceful skip into a bogus \
         \"manifest is STALE\" failure",
    );

    let banner_less = write_fake_interpreter(&dir, "python_banner_less", "hello", 0);
    assert!(
        !python_is_functional(&banner_less),
        "an exit-0 shim without a `Python 3.x` banner must be rejected — \
         exit status alone is not a functional-interpreter proof",
    );
}

/// Present-forces-run: a functional interpreter (real `Python 3.x.y` banner,
/// exit 0) is ACCEPTED, so the freshness body actually runs whenever python
/// is installed — the graceful skip can never become vacuous.
#[test]
fn python_probe_accepts_functional_interpreter() {
    let (dir, _guard) = unique_probe_fixture_dir("good");
    let good = write_fake_interpreter(&dir, "python_good", "Python 3.12.0", 0);
    assert!(
        python_is_functional(&good),
        "a candidate printing a `Python 3.x.y` banner with exit 0 must be \
         accepted, otherwise the freshness check would skip vacuously on \
         machines WITH python",
    );
}

/// One harness, both poles: the probe DISCRIMINATES the stub from the
/// functional interpreter — an always-true probe fails on the stub half, an
/// always-false probe fails on the functional half.
#[test]
fn python_probe_discriminates_stub_from_functional_interpreter() {
    let (dir, _guard) = unique_probe_fixture_dir("both");
    let stub = write_fake_interpreter(&dir, "python_stub", "Python was not found", 9009);
    let good = write_fake_interpreter(&dir, "python_good", "Python 3.12.0", 0);
    assert!(
        !python_is_functional(&stub),
        "probe must reject the non-functional stub (always-true regression)",
    );
    assert!(
        python_is_functional(&good),
        "probe must accept the functional interpreter (always-false regression)",
    );
}

/// Probe-past-a-shadowing-stub: with a non-functional stub FIRST and a
/// functional interpreter SECOND, the locator must select the FUNCTIONAL
/// candidate and record the stub as rejected. A first-match-only locator
/// (probing only the front-of-`PATH` entry) stops at the stub and skips
/// vacuously even though a real interpreter is available — this test is
/// RED against that regression. The stub-only half pins the `Err` shape:
/// every probed-and-rejected candidate is reported so the caller can skip
/// loudly with the full probe trail.
#[test]
fn locate_probes_past_shadowing_stub_to_functional_candidate() {
    let (dir, _guard) = unique_probe_fixture_dir("shadow");
    let stub = write_fake_interpreter(&dir, "python_stub", "Python was not found", 9009);
    let good = write_fake_interpreter(&dir, "python_good", "Python 3.12.0", 0);

    assert_eq!(
        locate_first_functional(vec![stub.clone(), good.clone()]),
        Ok(good),
        "the locator must probe PAST the non-functional front stub and \
         select the functional candidate shadowed behind it — stopping at \
         the first candidate re-introduces the vacuous skip",
    );

    assert_eq!(
        locate_first_functional(vec![stub.clone()]),
        Err(vec![stub]),
        "with only the stub available the locator must reject it AND report \
         it in the rejected trail, so the caller skips loudly instead of \
         accepting a non-functional interpreter",
    );
}

/// `which_on_path` `PATH` contract: EVERY `PATH` match is returned, in
/// `PATH` order — never just the first hit. With a NON-functional stub dir
/// FIRST on `PATH` and a functional-interpreter dir SECOND (the MS-Store
/// shadowing layout), the returned candidate vec must contain BOTH shims,
/// stub first, functional after. A first-match-only regression (an early
/// `return` / `.first()` at the `PATH` loop) returns only the stub — the
/// first assertion goes RED — and would re-introduce the vacuous skip:
/// `locate_first_functional([stub])` is `Err`, so the freshness check would
/// skip even though a real interpreter sits later on `PATH`. The terminal
/// assertion pins the end-to-end chain over the vec `which_on_path`
/// ACTUALLY returned: the locator selects the functional interpreter PAST
/// the shadowing stub.
#[test]
fn which_on_path_returns_all_path_matches_in_path_order() {
    let _env = lock_probe_env();
    let (stub_dir, _stub_guard) = unique_probe_fixture_dir("path_order_stub");
    let (good_dir, _good_guard) = unique_probe_fixture_dir("path_order_good");
    let stub = write_fake_interpreter(&stub_dir, "python", "Python was not found", 9009);
    let good = write_fake_interpreter(&good_dir, "python", "Python 3.12.0", 0);

    let fixture_path =
        std::env::join_paths([stub_dir.clone(), good_dir.clone()]).expect("join fixture PATH dirs");
    let _path = PathEnvGuard::swap(&fixture_path);

    let candidates = which_on_path(Path::new("python"));
    assert_eq!(
        candidates,
        vec![stub.clone(), good.clone()],
        "`which_on_path` must return EVERY `PATH` match in `PATH` order — \
         the shadowing stub first, the functional interpreter after. A \
         first-match-only regression returns only the front-of-`PATH` stub \
         and re-introduces the vacuous skip",
    );

    assert_eq!(
        locate_first_functional(candidates),
        Ok(good),
        "the multi-match candidate vec must let the locator probe PAST the \
         front-of-`PATH` stub to the functional interpreter shadowed behind \
         it — first-match-only shadowing turns an installed interpreter \
         into a vacuous skip",
    );
}

/// Windows absolute-override extension resolution: an explicit
/// `PYTHON3=C:\Python312\python` (no extension) is a usable command when an
/// executable variant (`python.exe` / `.bat` / `.cmd`) exists at that path —
/// `which_on_path` must expand the same executable extensions for an
/// ABSOLUTE name as it does for `PATH` lookups (bare name first, extension
/// variants after), instead of returning empty and losing the freshness
/// assertion to a vacuous skip.
#[cfg(windows)]
#[test]
fn absolute_override_without_extension_resolves_windows_executable_variants() {
    let (dir, _guard) = unique_probe_fixture_dir("abs_ext");
    // `write_fake_interpreter` emits a `.cmd` shim on Windows — one of the
    // executable extensions the PATH branch already expands.
    let shim = write_fake_interpreter(&dir, "python_abs", "Python 3.12.0", 0);
    let bare = dir.join("python_abs");
    assert!(
        bare.is_absolute() && !bare.is_file(),
        "fixture must reproduce the trap: the extension-less absolute \
         override is NOT itself a file",
    );

    assert_eq!(
        which_on_path(&bare),
        vec![shim],
        "an absolute extension-less override must resolve its Windows \
         executable-extension variant, exactly as a `PATH` lookup would",
    );
}
