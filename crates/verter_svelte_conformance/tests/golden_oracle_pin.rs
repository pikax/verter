//! Hermetic oracle-pin guard over the committed conformance goldens: EVERY
//! `corpus/goldens/*.json` must carry `oracleVersion` EQUAL to the sole
//! `SVELTE_ORACLE_VERSION` authority in `scripts/svelte-golden-lib.mjs` —
//! non-empty is NOT enough. A stale golden (an old `oracleVersion` surviving
//! a `svelte` bump) or a hand-edited restamp would otherwise keep validating
//! the differential's empty `KNOWN_DIVERGENCES` ledger against the wrong
//! oracle.
//!
//! Single-authority discipline: the pin is PARSED from the lib (exactly as
//! `verter_compiler`'s `svelte_goldens_in_sync` guard parses it) — this guard
//! re-declares no version anywhere.

use std::path::{Path, PathBuf};

/// The committed corpus size (one fixture per manifest case, two backend
/// goldens each) — the shared test-side pin. A manifest change legitimately
/// moves it, in lockstep across every conformance gate.
#[path = "common/case_count.rs"]
mod case_count;
use case_count::CASE_COUNT;

/// The conformance crate's committed goldens directory.
fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus")
        .join("goldens")
}

/// `scripts/svelte-golden-lib.mjs` — the sole JS pin authority.
fn golden_lib_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the conformance crate lives two levels under the repo root")
        .join("scripts")
        .join("svelte-golden-lib.mjs")
}

/// Read the `SVELTE_ORACLE_VERSION = "x.y.z"` pin constant from the lib
/// source — the same single-authority parse `svelte_goldens_in_sync` uses.
fn oracle_pin_version(lib_src: &str) -> String {
    for line in lib_src.lines() {
        let trimmed = line.trim();
        // `export const SVELTE_ORACLE_VERSION = "5.56.3";`
        if let Some(rest) = trimmed.strip_prefix("export const SVELTE_ORACLE_VERSION") {
            let after_eq = rest.split('=').nth(1).expect("pin assignment has `=`");
            let quoted = after_eq.trim().trim_end_matches(';').trim();
            return quoted.trim_matches('"').to_string();
        }
    }
    panic!("SVELTE_ORACLE_VERSION pin constant not found in svelte-golden-lib.mjs");
}

/// The per-golden verdict: `None` when the payload's `oracleVersion` EQUALS
/// `pin`, `Some(violation)` otherwise (missing field, non-string, non-JSON,
/// and any non-equal version all fail).
fn golden_pin_violation(name: &str, text: &str, pin: &str) -> Option<String> {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => return Some(format!("{name}: not valid JSON: {error}")),
    };
    match value
        .get("oracleVersion")
        .and_then(serde_json::Value::as_str)
    {
        None => Some(format!("{name}: missing string `oracleVersion`")),
        Some(version) if version != pin => Some(format!(
            "{name}: `oracleVersion` {version:?} does NOT equal the oracle pin {pin:?}"
        )),
        Some(_) => None,
    }
}

/// EVERY committed conformance golden — both backends, every disposition
/// shape — carries `oracleVersion` equal to the parsed pin authority.
#[test]
fn committed_conformance_goldens_match_oracle_pin() {
    let lib_src = std::fs::read_to_string(golden_lib_path())
        .expect("scripts/svelte-golden-lib.mjs reads (repo-root layout)");
    let pin = oracle_pin_version(&lib_src);
    assert!(!pin.is_empty(), "the parsed oracle pin must be non-empty");

    let mut violations: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for entry in std::fs::read_dir(goldens_dir()).expect("committed goldens dir reads") {
        let entry = entry.expect("golden dir entry reads");
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            violations.push(format!("non-UTF-8 entry under goldens/: {name:?}"));
            continue;
        };
        if entry.path().is_dir() || !name.ends_with(".json") {
            // The bijection gate reports foreign entries precisely.
            continue;
        }
        scanned += 1;
        let text = std::fs::read_to_string(entry.path()).expect("committed golden reads");
        if let Some(violation) = golden_pin_violation(name, &text, &pin) {
            violations.push(violation);
        }
    }

    assert_eq!(
        scanned,
        CASE_COUNT * 2,
        "the pin guard must scan every committed golden (both backends)"
    );
    assert!(
        violations.is_empty(),
        "{} committed conformance golden(s) do NOT carry the oracle pin ({pin}). A \
         `svelte` bump is a reviewed oracle delta: re-pin SVELTE_ORACLE_VERSION in \
         scripts/svelte-golden-lib.mjs, regenerate with `node \
         scripts/gen-svelte-goldens.mjs --conformance`, and review the diff. \
         Violations:\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────────

/// The pin parser extracts exactly the quoted version from the export line.
#[test]
fn pin_parser_extracts_the_authority_constant() {
    let src = "// header\nexport const SVELTE_ORACLE_VERSION = \"9.9.9\";\nconst x = 1;\n";
    assert_eq!(oracle_pin_version(src), "9.9.9");
}

/// The per-golden verdict DISCRIMINATES: a real committed golden passes
/// against the real pin, and the SAME golden restamped with a different
/// `oracleVersion` — the stale-after-a-bump shape the previous non-empty
/// check accepted — fails.
#[test]
fn stale_or_restamped_golden_version_fails_the_pin_guard() {
    let lib_src = std::fs::read_to_string(golden_lib_path()).expect("golden lib reads");
    let pin = oracle_pin_version(&lib_src);

    // A real committed golden passes as-is (the guard is exercisable).
    let sample = std::fs::read_dir(goldens_dir())
        .expect("goldens dir reads")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .min()
        .expect("at least one committed golden exists");
    let raw = std::fs::read_to_string(&sample).expect("sample golden reads");
    assert_eq!(
        golden_pin_violation("sample", &raw, &pin),
        None,
        "a committed golden must pass against the live pin"
    );

    // The restamp: same payload, different version — MUST fail.
    let stale_version = format!("{pin}-stale");
    let mut value: serde_json::Value = serde_json::from_str(&raw).expect("sample parses");
    value["oracleVersion"] = serde_json::Value::String(stale_version.clone());
    let stale = serde_json::to_string(&value).expect("restamp serializes");
    let verdict = golden_pin_violation("sample", &stale, &pin);
    assert!(
        verdict
            .as_deref()
            .is_some_and(|violation| violation.contains(&stale_version)),
        "a golden whose `oracleVersion` no longer equals the pin must fail \
         (previously a merely NON-EMPTY version passed); verdict: {verdict:?}"
    );

    // Non-empty is NOT enough: a missing field fails too.
    let mut missing: serde_json::Value = serde_json::from_str(&raw).expect("sample parses");
    missing
        .as_object_mut()
        .expect("golden is an object")
        .remove("oracleVersion");
    let missing = serde_json::to_string(&missing).expect("serializes");
    assert!(
        golden_pin_violation("sample", &missing, &pin).is_some(),
        "a golden with no `oracleVersion` must fail the pin guard"
    );
}
