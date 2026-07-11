//! Svelte reference-drift hermetic guards.
//!
//! These guards run in the DEFAULT canonical suite and are FULLY HERMETIC:
//! they read only committed files (the goldens + the lockfile + the generator
//! pin constant). They NEVER shell `node`, NEVER load the live svelte
//! compiler, and NEVER require `pnpm install` — the default run
//! (`cargo nextest run --workspace` + `cargo test -p verter_session --tests`)
//! must stay node-free and compiler-free.
//!
//! The LIVE golden-vs-pinned-compiler drift `--check` lives in two non-default
//! homes: the feature-gated Rust oracle harness
//! (`svelte_oracle_harness.rs`, `--features svelte-oracle`) and the JS
//! `--check` (`node scripts/gen-svelte-goldens.mjs --check`). Both run in CI:
//! the feature-gated harness in the dedicated `Svelte Oracle (live,
//! feature-gated)` job, and the JS `--check` in the pnpm `JS Build & Test` job
//! (both in `.github/workflows/ci.yml`).
//!
//! - `committed_svelte_goldens_are_structurally_valid` — loads EVERY committed
//!   golden and asserts it parses + carries the required topology fields, so the
//!   native-Svelte runtime codegen can rely on the goldens being loadable. A
//!   missing / corrupt / structurally-invalid golden fails the guard. Pure
//!   file reads — no node, no live compiler.
//! - `svelte_lockfile_matches_oracle_pin` — asserts the resolved `svelte`
//!   version in `pnpm-lock.yaml` EQUALS the `SVELTE_ORACLE_VERSION` pin
//!   declared in `scripts/svelte-golden-lib.mjs` (the single JS pin authority
//!   every Svelte golden generator imports). A `svelte` bump without a
//!   re-pin (+ golden regen) fails — silent drift is impossible.

use std::path::{Path, PathBuf};

use serde::Deserialize;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <ws>/crates/verter_compiler")
        .to_path_buf()
}

fn goldens_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus/goldens")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus/fixtures")
}

/// The committed backends every `.svelte` fixture is expected to carry a golden
/// for. Mirrors `BACKENDS` in `scripts/gen-svelte-goldens.mjs`.
const EXPECTED_BACKENDS: [&str; 2] = ["client", "server"];

/// Read the `SVELTE_ORACLE_VERSION = "x.y.z"` pin constant from
/// `scripts/svelte-golden-lib.mjs` — the single JS pin authority every Svelte
/// golden generator imports. Parsing the lib keeps ONE authority — the guard
/// does not re-declare the version.
fn oracle_pin_version(lib_src: &str) -> String {
    for line in lib_src.lines() {
        let t = line.trim();
        // `export const SVELTE_ORACLE_VERSION = "5.56.3";`
        if let Some(rest) = t.strip_prefix("export const SVELTE_ORACLE_VERSION") {
            let after_eq = rest.split('=').nth(1).expect("pin assignment has `=`");
            let quoted = after_eq.trim().trim_end_matches(';').trim();
            return quoted.trim_matches('"').to_string();
        }
    }
    panic!("SVELTE_ORACLE_VERSION pin constant not found in svelte-golden-lib.mjs");
}

/// Extract the resolved `svelte` version from `pnpm-lock.yaml`. The lockfile
/// `packages:` section lists `  svelte@<version>:` for the resolved version.
/// We take the FIRST such entry (there is exactly one resolved `svelte`).
fn lockfile_svelte_version(lock_src: &str) -> Option<String> {
    for line in lock_src.lines() {
        // Match a `  svelte@<version>:` package key. Guard against
        // `@sveltejs/...` and scoped names by requiring the line (trimmed) to
        // start with `svelte@`.
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("svelte@") {
            if let Some(version) = rest.strip_suffix(':') {
                // A bare resolved package key has no parenthesised peer suffix
                // and no `/` (which a path-style entry would carry).
                if !version.contains('(') && !version.contains('/') && !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The committed-golden topology schema (the REQUIRED structural fields the
// native-Svelte runtime codegen loads). This MIRRORS the feature-gated oracle
// harness schema and the `gen-svelte-goldens.mjs` normalizer — but here it is
// consumed purely from committed files (no live compiler).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ImportRow {
    source: String,
    kind: String,
    #[allow(dead_code)]
    names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExportDefault {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    params: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TemplateSkeleton {
    factory: String,
    #[allow(dead_code)]
    html: String,
    #[allow(dead_code)]
    flag: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CssTopology {
    present: bool,
    hash: Option<String>,
    code: Option<String>,
}

/// The committed normalized golden. `deny_unknown_fields` makes the structural
/// guard NON-vacuous: a golden missing a required topology field OR carrying an
/// unexpected one fails to deserialize, failing the guard.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommittedGolden {
    slug: String,
    backend: String,
    #[serde(rename = "oracleVersion")]
    oracle_version: String,
    imports: Vec<ImportRow>,
    #[serde(rename = "exportDefault")]
    export_default: Option<ExportDefault>,
    #[serde(rename = "helperSequence")]
    helper_sequence: Vec<String>,
    #[serde(rename = "helperSet")]
    helper_set: Vec<String>,
    #[serde(rename = "helperCounts")]
    helper_counts: std::collections::BTreeMap<String, u32>,
    /// The ordered delegated event-type set (the module `$.delegate([...])`
    /// declaration), client backend only (empty on the server). NO `default`: every
    /// committed golden MUST carry this field, so a golden missing it fails the
    /// structural deserialize rather than silently defaulting to an empty list.
    #[serde(rename = "delegatedEvents")]
    delegated_events: Vec<String>,
    templates: Vec<TemplateSkeleton>,
    /// The FULL normalized official module (the emitted-JS equivalence oracle) —
    /// present on the CLIENT backend, `null` on the server. Optional so the
    /// `deny_unknown_fields` server golden (which carries `clientModule: null`)
    /// still deserializes.
    #[serde(rename = "clientModule")]
    client_module: Option<String>,
    css: CssTopology,
}

/// Recursively collect every committed `*.json` golden path under `dir`, EXCEPT
/// the top-level `generated/` subtree. That subtree is the SEPARATE differential
/// -parity corpus (the EXPANDED golden schema, owned by
/// `scripts/gen-svelte-diff-corpus.mjs` + the `diff_oracle_tests` matrix); its
/// goldens carry additional fields the hand-vendored [`CommittedGolden`]
/// `deny_unknown_fields` schema intentionally rejects, and they are validated by
/// `gen-svelte-diff-corpus.mjs --check` instead — so this hand-vendored hermetic
/// guard skips them.
fn collect_golden_paths(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let generated_top = dir.join("generated");
    while let Some(d) = stack.pop() {
        for entry in
            std::fs::read_dir(&d).unwrap_or_else(|e| panic!("read dir {}: {e}", d.display()))
        {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                if p == generated_top {
                    continue;
                }
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("json") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Structural-validity verdict for a single committed golden. `Ok(())` means
/// the golden parsed AND every required topology invariant held; `Err(msg)`
/// names the first violation. The verdict is shared by the default guard and
/// its discrimination self-test so both exercise the SAME logic.
fn validate_committed_golden(path: &Path, raw: &str) -> Result<(), String> {
    let golden: CommittedGolden = serde_json::from_str(raw)
        .map_err(|e| format!("{}: failed to parse golden JSON ({e})", path.display()))?;

    // Topology-presence invariants the native-Svelte runtime codegen relies on.
    // These are structural assertions over the parsed golden — not byte equality.
    if golden.slug.is_empty() {
        return Err(format!("{}: empty `slug`", path.display()));
    }
    if golden.backend != "client" && golden.backend != "server" {
        return Err(format!(
            "{}: `backend` must be `client` or `server`, got {:?}",
            path.display(),
            golden.backend
        ));
    }
    if golden.oracle_version.is_empty() {
        return Err(format!("{}: empty `oracleVersion`", path.display()));
    }

    // The helper TOPOLOGY must be internally consistent: the unique-set is the
    // sorted dedup of the sequence, and `helperCounts` is the EXACT per-helper
    // occurrence tally of `helperSequence`. Both are DERIVED from the sequence
    // and compared for full equality, so a corrupted set OR a wrong count (a
    // count that no longer matches the sequence it claims to tally) is rejected
    // — not merely a cardinality / positivity mismatch.
    let mut derived_set: Vec<String> = golden.helper_sequence.clone();
    derived_set.sort();
    derived_set.dedup();
    if derived_set != golden.helper_set {
        return Err(format!(
            "{}: `helperSet` is not the sorted unique set of `helperSequence` \
             (set={:?}, seq-derived={:?})",
            path.display(),
            golden.helper_set,
            derived_set
        ));
    }
    let mut derived_counts: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();
    for helper in &golden.helper_sequence {
        *derived_counts.entry(helper.clone()).or_insert(0) += 1;
    }
    if derived_counts != golden.helper_counts {
        return Err(format!(
            "{}: `helperCounts` is not the exact per-helper tally of `helperSequence` \
             (counts={:?}, seq-derived={:?})",
            path.display(),
            golden.helper_counts,
            derived_counts
        ));
    }

    // The full-module equivalence oracle (`clientModule`) is CLIENT-only: present
    // and non-empty on a client golden, `null` on a server golden. (The client gate
    // compares Verter's normalized output against it; the server backend has no
    // client-module consumer.)
    match (golden.backend.as_str(), &golden.client_module) {
        ("client", None) => {
            return Err(format!(
                "{}: a client golden must carry a `clientModule` (the full-module oracle)",
                path.display()
            ));
        }
        ("client", Some(m)) if m.is_empty() => {
            return Err(format!(
                "{}: a client golden's `clientModule` must be non-empty",
                path.display()
            ));
        }
        ("server", Some(_)) => {
            return Err(format!(
                "{}: a server golden must carry `clientModule: null`",
                path.display()
            ));
        }
        _ => {}
    }

    // The server backend never emits client DOM template skeletons.
    if golden.backend == "server" && !golden.templates.is_empty() {
        return Err(format!(
            "{}: server golden must carry an empty `templates` list, got {} entries",
            path.display(),
            golden.templates.len()
        ));
    }

    // The delegated-event set is client-only and internally consistent: the server
    // backend declares none; a non-empty client set must coincide with the
    // `delegate` helper (the module-level `$.delegate([...])` declaration).
    if golden.backend == "server" && !golden.delegated_events.is_empty() {
        return Err(format!(
            "{}: server golden must carry an empty `delegatedEvents` list, got {:?}",
            path.display(),
            golden.delegated_events
        ));
    }
    if golden.backend == "client" {
        let has_delegate = golden.helper_set.iter().any(|h| h == "delegate");
        if golden.delegated_events.is_empty() == has_delegate {
            return Err(format!(
                "{}: `delegatedEvents` non-emptiness ({:?}) must match the `delegate` helper presence ({has_delegate})",
                path.display(),
                golden.delegated_events
            ));
        }
    }
    for tpl in &golden.templates {
        if !matches!(
            tpl.factory.as_str(),
            "from_html" | "from_svg" | "from_mathml" | "from_tree"
        ) {
            return Err(format!(
                "{}: template factory {:?} is not a known DOM factory",
                path.display(),
                tpl.factory
            ));
        }
    }

    // Every import row carries a non-empty source + a known kind.
    for imp in &golden.imports {
        if imp.source.is_empty() {
            return Err(format!(
                "{}: an import row has an empty `source`",
                path.display()
            ));
        }
        if !matches!(
            imp.kind.as_str(),
            "sideEffect" | "namespace" | "named" | "default" | "defaultAndNamed"
        ) {
            return Err(format!(
                "{}: import row for {:?} has unknown `kind` {:?}",
                path.display(),
                imp.source,
                imp.kind
            ));
        }
    }

    // The CSS topology is internally consistent: a present artifact carries
    // a code (possibly EMPTY — the official `css.code` is `""` for an
    // existing-but-empty `<style>` body) and a hash EXACTLY when the masked
    // code carries an observable scope token (`extractScopeHash` reads the
    // hash from the code, so a token-less code — empty, or every rule pruned
    // unscoped — pins `hash: null`); an absent artifact carries neither.
    match (golden.css.present, &golden.css.hash, &golden.css.code) {
        (true, hash, Some(code)) => {
            let has_scope_token = code.contains("svelte-<scoped>");
            if hash.is_some() != has_scope_token {
                return Err(format!(
                    "{}: `css.hash` presence ({:?}) disagrees with the masked scope \
                     token in `css.code` (token present={has_scope_token})",
                    path.display(),
                    golden.css.hash,
                ));
            }
        }
        (false, None, None) => {}
        _ => {
            return Err(format!(
                "{}: inconsistent `css` topology (present={}, hash={:?}, code present={})",
                path.display(),
                golden.css.present,
                golden.css.hash,
                golden.css.code.is_some()
            ));
        }
    }

    // `exportDefault` is optional (a CSS-only / empty fixture may have none),
    // but when present its name must be non-empty.
    if let Some(export) = &golden.export_default {
        if export.name.is_empty() {
            return Err(format!(
                "{}: `exportDefault` has an empty `name`",
                path.display()
            ));
        }
    }

    Ok(())
}

/// Verdict for the hermetic version-stamp guard: `Ok(())` when the committed
/// golden's `oracleVersion` EQUALS `pin`, `Err(msg)` otherwise (including a
/// golden that fails to parse). Shared by the guard and its discrimination
/// self-test so both exercise the SAME equality logic — a mismatched stamp can
/// never pass one path while failing the other. Pure file content in, no node.
fn golden_oracle_version_matches_pin(path: &Path, raw: &str, pin: &str) -> Result<(), String> {
    let golden: CommittedGolden = serde_json::from_str(raw)
        .map_err(|e| format!("{}: failed to parse golden JSON ({e})", path.display()))?;
    if golden.oracle_version != pin {
        return Err(format!(
            "{}: `oracleVersion` {:?} does NOT equal the oracle pin {:?}",
            path.display(),
            golden.oracle_version,
            pin
        ));
    }
    Ok(())
}

/// Collect every committed `.svelte` fixture under `dir`, as `/`-joined slugs
/// relative to `dir` with the `.svelte` suffix stripped — the SAME slug the
/// generator (`fixtureSlug` + `goldenPathFor`) derives golden paths from. The
/// top-level `generated/` subtree is EXCLUDED (it is the differential-parity
/// corpus, owned by `gen-svelte-diff-corpus.mjs`), so the hand-vendored
/// fixture↔golden coverage check stays consistent with [`collect_golden_paths`].
fn collect_fixture_slugs(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let generated_top = dir.join("generated");
    while let Some(d) = stack.pop() {
        for entry in
            std::fs::read_dir(&d).unwrap_or_else(|e| panic!("read dir {}: {e}", d.display()))
        {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                if p == generated_top {
                    continue;
                }
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("svelte") {
                let rel = p
                    .strip_prefix(dir)
                    .expect("under fixtures dir")
                    .to_string_lossy()
                    .replace('\\', "/");
                let slug = rel.strip_suffix(".svelte").unwrap_or(&rel).to_string();
                out.push(slug);
            }
        }
    }
    out.sort();
    out
}

/// Collect every committed `.json` golden under `dir`, as `/`-joined paths
/// relative to `dir` (the golden's stable identity, including the
/// `.<backend>.json` suffix).
fn collect_golden_rels(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for p in collect_golden_paths(dir) {
        let rel = p
            .strip_prefix(dir)
            .expect("under goldens dir")
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
    out.sort();
    out
}

/// Pure fixture↔golden COVERAGE verdict over `(fixtures_dir, goldens_dir)`:
/// `Ok(())` iff every committed `.svelte` fixture has EXACTLY its expected
/// client+server goldens AND there are NO orphan goldens (a `.json` golden with
/// no corresponding fixture/backend). `Err(violations)` lists every coverage
/// gap. Shared by the default guard and its discrimination self-tests so both
/// exercise the SAME coverage logic. Pure file reads — no node, no compiler.
fn coverage_violations(fixtures: &Path, goldens: &Path) -> Result<(), Vec<String>> {
    let slugs = collect_fixture_slugs(fixtures);
    let golden_rels: std::collections::BTreeSet<String> =
        collect_golden_rels(goldens).into_iter().collect();

    // Every fixture must carry EXACTLY its expected goldens.
    let mut expected: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut violations = Vec::new();
    for slug in &slugs {
        for backend in EXPECTED_BACKENDS {
            let rel = format!("{slug}.{backend}.json");
            expected.insert(rel.clone());
            if !golden_rels.contains(&rel) {
                violations.push(format!(
                    "MISSING golden for fixture {slug}.svelte (backend {backend}): expected {rel}"
                ));
            }
        }
    }

    // No orphan goldens: every committed golden must map back to a fixture+backend.
    for rel in &golden_rels {
        if !expected.contains(rel) {
            violations.push(format!(
                "ORPHAN golden {rel}: no committed `.svelte` fixture/backend produces it"
            ));
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        violations.sort();
        Err(violations)
    }
}

#[test]
fn committed_svelte_goldens_are_structurally_valid() {
    let dir = goldens_dir();
    let paths = collect_golden_paths(&dir);
    assert!(
        !paths.is_empty(),
        "no committed Svelte goldens found under {} — the corpus + goldens must \
         be committed so the native-Svelte runtime codegen can load them \
         hermetically",
        dir.display()
    );

    let mut failures = Vec::new();
    for path in &paths {
        let raw = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
        if let Err(msg) = validate_committed_golden(path, &raw) {
            failures.push(msg);
        }
    }

    assert!(
        failures.is_empty(),
        "{} committed Svelte golden(s) are structurally invalid. Regenerate with \
         `node scripts/gen-svelte-goldens.mjs` and review the diff as the oracle \
         delta; do NOT hand-edit the goldens.\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn svelte_lockfile_matches_oracle_pin() {
    let root = workspace_root();

    let lib_src = std::fs::read_to_string(root.join("scripts/svelte-golden-lib.mjs"))
        .expect("read svelte-golden-lib.mjs");
    let pin = oracle_pin_version(&lib_src);

    let lock_src =
        std::fs::read_to_string(root.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    let resolved = lockfile_svelte_version(&lock_src)
        .expect("a resolved `svelte@<version>:` entry in pnpm-lock.yaml");

    assert_eq!(
        resolved, pin,
        "the resolved `svelte` version in pnpm-lock.yaml ({resolved}) does NOT equal \
         the oracle pin SVELTE_ORACLE_VERSION ({pin}) in svelte-golden-lib.mjs. A \
         `svelte` bump is a reviewed oracle delta: re-pin SVELTE_ORACLE_VERSION, \
         bump the lockfile, run `node scripts/gen-svelte-goldens.mjs`, and review \
         the golden diff."
    );
}

#[test]
fn committed_svelte_goldens_match_oracle_pin() {
    let root = workspace_root();
    let lib_src = std::fs::read_to_string(root.join("scripts/svelte-golden-lib.mjs"))
        .expect("read svelte-golden-lib.mjs");
    let pin = oracle_pin_version(&lib_src);

    let dir = goldens_dir();
    let paths = collect_golden_paths(&dir);
    assert!(
        !paths.is_empty(),
        "no committed Svelte goldens found under {} — the corpus + goldens must \
         be committed so the version stamp can be verified hermetically",
        dir.display()
    );

    let mut failures = Vec::new();
    for path in &paths {
        let raw = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
        if let Err(msg) = golden_oracle_version_matches_pin(path, &raw, &pin) {
            failures.push(msg);
        }
    }

    assert!(
        failures.is_empty(),
        "{} committed Svelte golden(s) carry an `oracleVersion` that does NOT equal \
         the oracle pin SVELTE_ORACLE_VERSION ({pin}) in svelte-golden-lib.mjs. A \
         `svelte` bump is a reviewed oracle delta: re-pin SVELTE_ORACLE_VERSION, \
         bump the lockfile, run `node scripts/gen-svelte-goldens.mjs` (which restamps \
         every golden with the new pin), and review the golden diff. Do NOT hand-edit \
         the goldens.\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Discrimination proofs — the parsers/guards must FAIL on a drift/bump/corrupt
// golden, not pass vacuously (Stub Prevention).
// ---------------------------------------------------------------------------

#[test]
fn version_stamp_guard_discriminates_a_mismatched_oracle_version() {
    // DISCRIMINATION proof for `committed_svelte_goldens_match_oracle_pin`: a
    // committed golden stamped with the live pin PASSES, and the SAME golden
    // restamped with a different `oracleVersion` FAILS. This proves the guard
    // actually compares the stamp against the pin — a `SVELTE_ORACLE_VERSION` +
    // lockfile bump that leaves STALE goldens (carrying the old version) cannot
    // slip through the default hermetic suite. Pure file reads, no node.
    let root = workspace_root();
    let lib_src = std::fs::read_to_string(root.join("scripts/svelte-golden-lib.mjs"))
        .expect("read svelte-golden-lib.mjs");
    let pin = oracle_pin_version(&lib_src);

    let dir = goldens_dir();
    let paths = collect_golden_paths(&dir);
    let sample = paths.first().expect("at least one committed golden");
    let raw = std::fs::read_to_string(sample)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", sample.display()));

    // The committed golden's stamp equals the live pin.
    assert!(
        golden_oracle_version_matches_pin(sample, &raw, &pin).is_ok(),
        "the committed golden {} must carry the live oracle pin {pin} before perturbation",
        sample.display()
    );

    // Restamp with a version that cannot equal any real pin, then assert the
    // guard rejects it. The phantom is built from the live pin so the test does
    // not hardcode (and drift from) the pin string.
    let stale_version = format!("{pin}-stale-phantom");
    assert_ne!(
        stale_version, pin,
        "the phantom stale version must differ from the pin"
    );
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).expect("committed golden parses as JSON");
    value
        .as_object_mut()
        .expect("golden is a JSON object")
        .insert(
            "oracleVersion".to_string(),
            serde_json::Value::String(stale_version.clone()),
        );
    let stale_src = serde_json::to_string(&value).expect("serialize stale-version golden");

    let verdict = golden_oracle_version_matches_pin(sample, &stale_src, &pin);
    assert!(
        verdict.is_err(),
        "a golden whose `oracleVersion` ({stale_version}) no longer equals the oracle \
         pin ({pin}) MUST fail the version-stamp guard; the guard returned Ok, which \
         would let a STALE golden (old oracleVersion after a bump) pass the default suite"
    );
}

#[test]
fn oracle_pin_parser_extracts_the_declared_version() {
    let src = "// header\nexport const SVELTE_ORACLE_VERSION = \"9.9.9\";\nconst x = 1;\n";
    assert_eq!(oracle_pin_version(src), "9.9.9");
}

#[test]
fn lockfile_parser_extracts_the_bare_resolved_version() {
    // A realistic snippet: the package key is the bare `svelte@<version>:`,
    // while scoped + peer-suffixed entries must NOT be mistaken for it.
    let lock = "\
  '@sveltejs/acorn-typescript@1.0.10(acorn@8.16.0)':
    resolution: {integrity: sha512-aaa==}
  oxfmt@0.52.0(svelte@5.56.3):
    resolution: {integrity: sha512-bbb==}
  svelte@5.56.3:
    resolution: {integrity: sha512-ccc==}
";
    assert_eq!(
        lockfile_svelte_version(lock).as_deref(),
        Some("5.56.3"),
        "must extract the bare resolved svelte version, ignoring peer-suffixed entries"
    );
}

#[test]
fn lockfile_guard_discriminates_a_version_bump() {
    // If the lockfile resolves a DIFFERENT version than the pin, the equality
    // the guard asserts must NOT hold — proving a bump fails the guard.
    let bumped_lock = "  svelte@5.99.0:\n    resolution: {integrity: sha512-zzz==}\n";
    let resolved = lockfile_svelte_version(bumped_lock).expect("resolved version");
    let pin = oracle_pin_version("export const SVELTE_ORACLE_VERSION = \"5.56.3\";\n");
    assert_ne!(
        resolved, pin,
        "a lockfile svelte bump (5.99.0) must NOT equal the pin (5.56.3) — \
         the guard would fail, as required"
    );
}

#[test]
fn structural_guard_discriminates_a_corrupted_golden() {
    // DISCRIMINATION proof for the hermetic default guard: a CLEAN committed
    // golden validates, and a PERTURBED copy of it (helperSet no longer the
    // dedup of helperSequence) FAILS. This proves the structural-validity
    // check is non-vacuous — it actually inspects the topology, so a corrupt
    // committed golden cannot slip through the default suite. Pure file reads,
    // no node, no live compiler.
    let dir = goldens_dir();
    let paths = collect_golden_paths(&dir);
    let sample = paths.first().expect("at least one committed golden");
    let raw = std::fs::read_to_string(sample)
        .unwrap_or_else(|e| panic!("read golden {}: {e}", sample.display()));

    // Clean copy passes.
    assert!(
        validate_committed_golden(sample, &raw).is_ok(),
        "the committed golden {} must validate cleanly before perturbation",
        sample.display()
    );

    // Perturb: inject a phantom helper into `helperSet` ONLY (not the sequence),
    // breaking the set-is-dedup-of-sequence invariant. The guard must reject it.
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).expect("committed golden parses as JSON");
    let set = value
        .get_mut("helperSet")
        .and_then(|v| v.as_array_mut())
        .expect("golden has a `helperSet` array");
    set.push(serde_json::Value::String("__phantom_helper__".to_string()));
    let perturbed = serde_json::to_string(&value).expect("serialize perturbed golden");

    let verdict = validate_committed_golden(sample, &perturbed);
    assert!(
        verdict.is_err(),
        "a golden whose `helperSet` is no longer the dedup of `helperSequence` \
         MUST fail the structural guard; the guard returned Ok, which would let a \
         corrupt committed golden pass the default suite"
    );

    // A second perturbation: drop a required topology field entirely. With
    // a missing field the typed deserialize fails, so the guard rejects it.
    let mut missing: serde_json::Value =
        serde_json::from_str(&raw).expect("committed golden parses as JSON");
    missing
        .as_object_mut()
        .expect("golden is a JSON object")
        .remove("helperSequence");
    let missing_src = serde_json::to_string(&missing).expect("serialize golden missing a field");
    assert!(
        validate_committed_golden(sample, &missing_src).is_err(),
        "a golden missing the required `helperSequence` field MUST fail the guard"
    );

    // A third perturbation: a WRONG `helperCounts` value (the `helperSet` and
    // `helperSequence` stay intact, so cardinality + positivity still hold) MUST
    // fail. This pins that the guard tallies `helperSequence` and compares the
    // FULL counts map — a count that no longer matches the sequence is rejected.
    // Pick any committed golden that has a `helperCounts` entry so the test does
    // not depend on which golden happens to sort first.
    let counted = paths
        .iter()
        .find_map(|p| {
            let src = std::fs::read_to_string(p).ok()?;
            let v: serde_json::Value = serde_json::from_str(&src).ok()?;
            let has_count = v
                .get("helperCounts")
                .and_then(|c| c.as_object())
                .is_some_and(|m| !m.is_empty());
            has_count.then_some((p.clone(), src))
        })
        .expect("at least one committed golden carries a non-empty helperCounts map");
    let (counted_path, counted_raw) = counted;
    assert!(
        validate_committed_golden(&counted_path, &counted_raw).is_ok(),
        "the committed golden {} must validate cleanly before the count perturbation",
        counted_path.display()
    );
    let mut wrong_count: serde_json::Value =
        serde_json::from_str(&counted_raw).expect("committed golden parses as JSON");
    {
        let counts = wrong_count
            .get_mut("helperCounts")
            .and_then(|v| v.as_object_mut())
            .expect("golden has a `helperCounts` object");
        // Bump the FIRST helper's count so it no longer matches the sequence
        // tally, while every key (and the set / sequence) is untouched — so the
        // cardinality + positivity invariants the old guard checked still pass.
        let key = counts
            .keys()
            .next()
            .expect("helperCounts is non-empty")
            .clone();
        let current = counts
            .get(&key)
            .and_then(|v| v.as_u64())
            .expect("count is a number");
        counts.insert(key, serde_json::Value::from(current + 1));
    }
    let wrong_count_src =
        serde_json::to_string(&wrong_count).expect("serialize wrong-count golden");
    assert!(
        validate_committed_golden(&counted_path, &wrong_count_src).is_err(),
        "a golden whose `helperCounts` no longer equals the per-helper tally of \
         `helperSequence` (right keys, WRONG count) MUST fail the structural guard; \
         the guard returned Ok, which would let a wrong-count golden pass the \
         default suite"
    );
}

// ---------------------------------------------------------------------------
// Fixture↔golden CORPUS COVERAGE — the structural guard above validates each
// committed golden's SHAPE; this guard validates the CORPUS COVERAGE: every
// `.svelte` fixture has exactly its 2 goldens, and there are no orphan goldens.
// Adding a fixture without its goldens, or leaving an orphan golden after
// deleting a fixture, fails here in the DEFAULT hermetic suite (pure file reads).
// ---------------------------------------------------------------------------

#[test]
fn committed_svelte_fixtures_have_exactly_their_goldens() {
    let fixtures = fixtures_dir();
    let goldens = goldens_dir();

    let slugs = collect_fixture_slugs(&fixtures);
    assert!(
        !slugs.is_empty(),
        "no committed `.svelte` fixtures found under {} — the corpus + goldens must \
         be committed so the native-Svelte runtime codegen can load them hermetically",
        fixtures.display()
    );

    if let Err(violations) = coverage_violations(&fixtures, &goldens) {
        panic!(
            "{} Svelte fixture↔golden coverage violation(s). Every committed `.svelte` \
             fixture must have EXACTLY its `client` + `server` goldens, and no orphan \
             goldens may remain. Regenerate with `node scripts/gen-svelte-goldens.mjs` \
             (it sweeps the corpus, writes both goldens per fixture, and prunes orphans). \
             Do NOT hand-edit the goldens.\n{}",
            violations.len(),
            violations.join("\n")
        );
    }
}

/// Hermetic proof on the COMMITTED goldens: a fixture authoring a literal
/// `$.<ident>` in MARKUP text must NOT pollute the helper topology. The
/// `regression/markup_dollar_member_text` fixture places `$.effect`, `$.state`,
/// `$.from_html`, `$.template_effect`, `$.derived` in markup TEXT; the generator
/// masks string/template-text/comment regions before scanning, so those phantom
/// helpers must be ABSENT from `helperSequence` / `helperSet` — while the markup
/// text itself is preserved verbatim in the template skeleton. Pure file reads.
#[test]
fn markup_dollar_member_does_not_pollute_helper_topology() {
    let goldens = goldens_dir();
    let client = goldens.join("regression/markup_dollar_member_text.client.json");
    let server = goldens.join("regression/markup_dollar_member_text.server.json");
    assert!(
        client.exists() && server.exists(),
        "the regression fixture's goldens must be committed at {} and {}",
        client.display(),
        server.display()
    );

    // The phantom "helpers" that appear ONLY as literal markup text — every one
    // must be absent from BOTH the sequence and the set on BOTH backends.
    let phantoms = ["effect", "state", "template_effect", "derived"];

    for (label, path) in [("client", &client), ("server", &server)] {
        let raw = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read golden {}: {e}", path.display()));
        let golden: CommittedGolden =
            serde_json::from_str(&raw).expect("regression golden parses + validates structurally");

        for phantom in phantoms {
            assert!(
                !golden.helper_sequence.iter().any(|h| h == phantom),
                "{label}: phantom helper {phantom:?} (a literal `$.{phantom}` in MARKUP text) \
                 leaked into `helperSequence` {:?} — the extractor regex is polluting from a \
                 non-code region",
                golden.helper_sequence
            );
            assert!(
                !golden.helper_set.iter().any(|h| h == phantom),
                "{label}: phantom helper {phantom:?} leaked into `helperSet` {:?}",
                golden.helper_set
            );
        }
    }

    // POSITIVE side: the markup text IS captured verbatim in the client template
    // skeleton (presence preserved — the masking excludes it from the helper
    // topology, not from the structural template). This pins that the phantoms
    // are genuinely PRESENT in the source markup, so their absence above is the
    // masking working — not a vacuous pass over an empty fixture.
    let raw = std::fs::read_to_string(&client).expect("read client golden");
    let golden: CommittedGolden = serde_json::from_str(&raw).expect("client golden parses");
    assert!(
        golden
            .templates
            .iter()
            .any(|t| t.html.contains("$.from_html($.template_effect($.derived))")),
        "the client template skeleton must preserve the literal markup text \
         `$.from_html($.template_effect($.derived))` verbatim, proving the phantom \
         `$.<ident>` tokens are really present in the markup (so their absence from \
         the helper topology is the masking, not an empty fixture); got {:?}",
        golden.templates
    );
}

// ---------------------------------------------------------------------------
// Coverage discrimination self-tests — the coverage verdict must FAIL on a
// missing golden and on an orphan golden, not pass vacuously (Stub Prevention).
// Built over synthetic temp trees so the committed corpus is never mutated.
// ---------------------------------------------------------------------------

/// Build a minimal synthetic corpus: one fixture `<slug>.svelte` plus the
/// listed `<slug>.<backend>.json` goldens. Returns `(tmp, fixtures, goldens)`;
/// the `TempDir` must outlive the dirs.
fn synthetic_corpus(slug: &str, backends: &[&str]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let fixtures = tmp.path().join("fixtures");
    let goldens = tmp.path().join("goldens");

    let fixture_path = fixtures.join(format!("{slug}.svelte"));
    std::fs::create_dir_all(fixture_path.parent().expect("fixture parent"))
        .expect("mkdir fixture parent");
    std::fs::write(&fixture_path, "<p>x</p>\n").expect("write fixture");

    for backend in backends {
        let golden_path = goldens.join(format!("{slug}.{backend}.json"));
        std::fs::create_dir_all(golden_path.parent().expect("golden parent"))
            .expect("mkdir golden parent");
        std::fs::write(&golden_path, "{}\n").expect("write golden");
    }

    (tmp, fixtures, goldens)
}

#[test]
fn coverage_guard_passes_when_every_fixture_has_both_goldens() {
    // A fixture with EXACTLY its client+server goldens is fully covered.
    let (_tmp, fixtures, goldens) = synthetic_corpus("runes/sample", &["client", "server"]);
    assert!(
        coverage_violations(&fixtures, &goldens).is_ok(),
        "a fixture with exactly its client+server goldens (and no orphans) must PASS \
         the coverage verdict"
    );
}

#[test]
fn coverage_guard_discriminates_a_fixture_missing_a_golden() {
    // A fixture with only its client golden (server golden absent) MUST fail —
    // adding a `.svelte` fixture without regenerating both goldens is a gap the
    // default suite must catch.
    let (_tmp, fixtures, goldens) = synthetic_corpus("runes/sample", &["client"]);
    let verdict = coverage_violations(&fixtures, &goldens);
    let violations = verdict.expect_err(
        "a fixture missing its `server` golden MUST fail the coverage verdict; it \
         returned Ok, which would let a half-covered fixture pass the default suite",
    );
    assert!(
        violations
            .iter()
            .any(|v| v.contains("MISSING golden") && v.contains("server")),
        "the coverage failure must name the MISSING server golden; got {violations:?}"
    );
}

#[test]
fn coverage_guard_discriminates_an_orphan_golden() {
    // Both goldens present for the fixture, PLUS a golden for a slug with no
    // fixture (an orphan left behind after deleting a fixture) MUST fail.
    let (_tmp, fixtures, goldens) = synthetic_corpus("runes/sample", &["client", "server"]);
    let orphan = goldens.join("runes/deleted_fixture.client.json");
    std::fs::create_dir_all(orphan.parent().expect("orphan parent")).expect("mkdir orphan parent");
    std::fs::write(&orphan, "{}\n").expect("write orphan golden");

    let verdict = coverage_violations(&fixtures, &goldens);
    let violations = verdict.expect_err(
        "an orphan golden with no fixture MUST fail the coverage verdict; it returned \
         Ok, which would let a stale golden survive a fixture deletion",
    );
    assert!(
        violations
            .iter()
            .any(|v| v.contains("ORPHAN golden") && v.contains("deleted_fixture")),
        "the coverage failure must name the ORPHAN golden; got {violations:?}"
    );
}

// ---------------------------------------------------------------------------
// CODE-POSITION REGEX-LITERAL INVARIANT (tracked under D-19) — the golden-side
// JS normalizer (`scripts/svelte-golden-lib.mjs::normalizeModuleForComparison`
// + the Rust mirror `normalize_module_for_comparison`) is NOT a JS lexer and
// would mangle a code-position REGEX LITERAL (collapse internal whitespace,
// mis-read `//` as a line comment). The comparator-side `RegExpLiteral.raw`
// axis is correct, so this is a golden-DATA limitation, not a comparator bug,
// and it does NOT reproduce while the corpus carries ZERO code-position regex
// literals. The guard below PINS that invariant: a future official Svelte
// client golden that introduces a code-position regex literal FAILS it,
// forcing a lexer-backed normalizer before that golden is accepted.
// ---------------------------------------------------------------------------

/// A value-ending character: after one of these (an identifier/number char or a
/// closing bracket), a `/` is DIVISION, not the start of a regex literal. The
/// keyword exception (an identifier char whose preceding word is an
/// expression-context keyword like `return`) is applied by the caller against
/// the tracked previous word — see [`first_code_position_regex_literal`].
fn is_value_ending(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$' || matches!(c, ')' | ']' | '}')
}

/// The expression-context keywords after which a `/` begins a regex literal (the
/// keyword precedes an EXPRESSION, so the slash is not division). Mirrors the
/// well-known regex-vs-division previous-token rule; a value-keyword
/// (`this`/`true`/`false`/`null`/`super`) is deliberately ABSENT — after one of
/// those a `/` is division. This is the bounded previous-token check the
/// architect ruled acceptable for the corpus guard.
fn is_expression_context_keyword(word: &str) -> bool {
    matches!(
        word,
        "return"
            | "typeof"
            | "instanceof"
            | "in"
            | "of"
            | "do"
            | "else"
            | "delete"
            | "void"
            | "new"
            | "throw"
            | "yield"
            | "await"
            | "case"
    )
}

/// Scan a regex BODY starting just after an opening `/` (at index `start`). A
/// regex body ends at the first UNESCAPED `/` that is NOT inside a `[...]`
/// character class (where `/` is literal). Returns the index of the closing `/`
/// (so `code[open..=close]` is the `/.../` literal sans flags), or `None` if no
/// closing `/` is reached before EOL/EOF (then the opening `/` was NOT a regex —
/// a regex literal cannot span a raw newline). `chars` is the char vector,
/// `start` the index of the first body char.
fn scan_regex_body_end(chars: &[char], start: usize) -> Option<usize> {
    let n = chars.len();
    let mut i = start;
    let mut in_class = false;
    while i < n {
        let c = chars[i];
        match c {
            // A raw newline terminates the line WITHOUT a close — not a regex.
            '\n' => return None,
            '\\' => {
                // Escape: skip the escaped char (a `\/`, `\[`, `\]`, etc. is literal).
                i += 2;
                continue;
            }
            '[' => {
                in_class = true;
                i += 1;
            }
            ']' => {
                in_class = false;
                i += 1;
            }
            '/' if !in_class => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Detect the FIRST CODE-POSITION regex literal in `code` and return its
/// `/.../`-fragment (sans flags), or `None` if there is none. "Code position"
/// means OUTSIDE strings, template TEXT, `${}` interpolations are scanned as
/// code, line comments (`//`), and block comments (`/* */`). The scanner mirrors
/// the `normalize_module_for_comparison` state machine (template-frame `${}`
/// depth, string/comment states) PLUS a previous-token check: a `/` begins a
/// regex only when the previous significant token allows it (start-of-input, an
/// operator/punctuator, or an expression-context keyword) — after a value-ending
/// token a `/` is DIVISION. The check is intentionally bounded (no full
/// division/regex disambiguation): it is SOUND enough to answer "is there a
/// regex literal at all" over machine-generated Svelte client JS and MUST NOT
/// false-positive on the committed corpus.
fn first_code_position_regex_literal(code: &str) -> Option<String> {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    // Template-literal frames: each tracks the `${}` interpolation depth
    // (0 = in template TEXT). Mirrors `normalize_module_for_comparison`.
    let mut tmpl: Vec<i32> = Vec::new();
    // The previous significant (non-whitespace, non-comment, code-position)
    // character, and the previous identifier WORD (reset on any non-identifier
    // significant char). Both persist across whitespace.
    let mut prev_significant: Option<char> = None;
    let mut prev_word = String::new();
    let mut i = 0;
    while i < n {
        // Inside template TEXT (frame depth 0): everything is masked text until
        // `` ` `` (close) or `${` (enter interpolation = code). A `/` here is
        // template text, never a regex.
        let in_tmpl_text = tmpl.last().copied() == Some(0);
        if in_tmpl_text {
            let ch = chars[i];
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == '`' {
                tmpl.pop();
                // A template literal is a VALUE — after the closing backtick a
                // `/` is division.
                prev_significant = Some('`');
                prev_word.clear();
                i += 1;
                continue;
            }
            if ch == '$' && i + 1 < n && chars[i + 1] == '{' {
                *tmpl.last_mut().unwrap() = 1;
                // Entering interpolation is entering CODE — `${` opens an
                // expression position, so a following `/` is regex-allowed.
                prev_significant = Some('{');
                prev_word.clear();
                i += 2;
                continue;
            }
            // Template TEXT — masked.
            i += 1;
            continue;
        }
        let ch = chars[i];
        let next = if i + 1 < n { chars[i + 1] } else { '\0' };
        // Line comment — masked to EOL.
        if ch == '/' && next == '/' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // Block comment — masked to `*/`.
        if ch == '/' && next == '*' {
            i += 2;
            while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        // String literal — masked to the closing quote.
        if ch == '\'' || ch == '"' {
            let quote = ch;
            i += 1;
            while i < n && chars[i] != quote {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            if i < n {
                i += 1;
            }
            // A string is a VALUE — after it a `/` is division.
            prev_significant = Some(quote);
            prev_word.clear();
            continue;
        }
        // Template literal open — push a TEXT frame.
        if ch == '`' {
            tmpl.push(0);
            i += 1;
            continue;
        }
        // `${}` interpolation brace bookkeeping (we are in interpolation CODE
        // when the top frame depth > 0).
        if let Some(depth) = tmpl.last_mut() {
            if *depth > 0 {
                if ch == '{' {
                    *depth += 1;
                    prev_significant = Some('{');
                    prev_word.clear();
                    i += 1;
                    continue;
                }
                if ch == '}' {
                    *depth -= 1;
                    // Leaving an inner brace; `}` is treated as value-ending.
                    prev_significant = Some('}');
                    prev_word.clear();
                    i += 1;
                    continue;
                }
            }
        }
        // Whitespace — does NOT reset the previous-token state (a regex can be
        // preceded by whitespace, e.g. `return /re/`).
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        // A code-position `/` that is neither `//` nor `/*` (handled above): it
        // is a regex literal IFF the previous token allows a regex here.
        if ch == '/' {
            let regex_allowed = match prev_significant {
                None => true,
                Some(p) if is_value_ending(p) => {
                    // After a value-ending char a `/` is division — UNLESS the
                    // value-ending char closes an expression-context KEYWORD
                    // (e.g. `return`/`typeof`), in which case it is a regex.
                    (p.is_ascii_alphanumeric() || p == '_' || p == '$')
                        && is_expression_context_keyword(&prev_word)
                }
                Some(_) => true,
            };
            if regex_allowed {
                if let Some(close) = scan_regex_body_end(&chars, i + 1) {
                    let frag: String = chars[i..=close].iter().collect();
                    return Some(frag);
                }
            }
            // Not a regex (division, or no closing `/`): `/` is an operator, so
            // the NEXT `/` is again regex-allowed.
            prev_significant = Some('/');
            prev_word.clear();
            i += 1;
            continue;
        }
        // An identifier character extends the current word.
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            prev_word.push(ch);
            prev_significant = Some(ch);
            i += 1;
            continue;
        }
        // Any other significant char (operator/punctuator) resets the word and
        // becomes the previous significant token.
        prev_significant = Some(ch);
        prev_word.clear();
        i += 1;
    }
    None
}

#[test]
fn code_position_regex_detector_discriminates() {
    // Detected (code-position regex):
    assert!(first_code_position_regex_literal("var r = /a  b/;").is_some());
    assert!(first_code_position_regex_literal("if (x) return /foo/g.test(s);").is_some());
    assert!(first_code_position_regex_literal("const re = /a\\/b/;").is_some());
    // NOT detected (the corpus shapes — must stay clean):
    assert!(
        first_code_position_regex_literal("var h = $.from_html(`<div></div>`);").is_none(),
        "closing tag in template"
    );
    assert!(
        first_code_position_regex_literal("import {x} from './foo.js';").is_none(),
        "path in string"
    );
    assert!(
        first_code_position_regex_literal("var s = 'a/b/c';").is_none(),
        "slashes in string"
    );
    assert!(
        first_code_position_regex_literal("var q = `a ${b/c} d`;").is_none(),
        "division in interpolation"
    );
    assert!(
        first_code_position_regex_literal("// a/b/c comment").is_none(),
        "slashes in line comment"
    );
    assert!(
        first_code_position_regex_literal("var n = a / b / c;").is_none(),
        "division, not regex"
    );
}

#[test]
fn committed_client_goldens_carry_no_code_position_regex_literal() {
    // INVARIANT (tracked under D-19): the golden-side JS normalizer
    // (`scripts/svelte-golden-lib.mjs::normalizeModuleForComparison` + the Rust mirror
    // `normalize_module_for_comparison`) is NOT a JS lexer and would mangle a code-position REGEX
    // LITERAL (collapse internal whitespace / mis-read `//`). The comparator-side `RegExpLiteral.raw`
    // axis is correct, so this is a golden-DATA limitation, not a comparator bug. It does not
    // reproduce while the corpus has ZERO code-position regex literals. This guard PINS that: if a
    // future official Svelte client golden introduces a code-position regex literal, this FAILS,
    // forcing a lexer-backed normalizer before that golden is accepted.
    let mut offenders: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for path in collect_golden_paths(&goldens_dir()) {
        let raw = std::fs::read_to_string(&path).unwrap();
        let golden: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let Some(cm) = golden.get("clientModule").and_then(|v| v.as_str()) else {
            continue;
        };
        scanned += 1;
        if let Some(frag) = first_code_position_regex_literal(cm) {
            offenders.push(format!("{}: {}", path.display(), frag));
        }
    }
    assert!(
        scanned > 0,
        "no committed client golden carried a `clientModule` to scan — the corpus must be committed \
         so this invariant is non-vacuous"
    );
    assert!(
        offenders.is_empty(),
        "a committed client golden now carries a CODE-POSITION regex literal — the golden-side \
         normalizer mangles regex literals (D-19). Replace `normalizeModuleForComparison` (+ the Rust \
         mirror) with a lexer-backed implementation before accepting these goldens:\n{}",
        offenders.join("\n")
    );
}
