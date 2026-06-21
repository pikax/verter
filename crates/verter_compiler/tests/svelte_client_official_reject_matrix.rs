//! The OFFICIAL-REJECT PARITY MATRIX — the "official rejects ⇒ Verter must reject"
//! quadrant of the Svelte client convergence gate.
//!
//! The valid-output topology oracle (`svelte_client_emit_topology.rs`) proves Verter
//! EMITS the right module for a supported input; the fail-closed breadth matrix
//! (`svelte_client_fail_matrix.rs`) proves Verter REFUSES an unsupported FEATURE. This
//! matrix proves the MISSING quadrant: a §1.2-core-shaped input the official
//! `svelte@5.56.3` compiler COMPILE-ERRORS (a duplicate / mis-`context`-ed `<script>`,
//! a `$`-prefixed binding, a duplicate declaration, a global `$foo` / `$$foo`
//! reference, an invalid HTML placement, a rune-arity error, a duplicate attribute, an
//! invalid `<svelte:options>`) must ALSO fail closed in Verter — never an emitted
//! `Main`.
//!
//! The corpus is a first-class committed REJECT CORPUS:
//! `tests/svelte_oracle_corpus/rejects/block4_core/*.svelte` (the canonical malformed
//! source) paired with a `*.json` metadata `{ "rule": "<CoreOfficialValidationRule>",
//! "official_code": "<official-error-code>" }`. A future official-reject rule lands as
//! a new corpus row + (if a new class) a `CoreOfficialValidationRule` variant.
//!
//! Three gates:
//! - DEFAULT hermetic: every reject row → `compile_client` returns non-`Ok` with NO
//!   `Main` (node-free; the committed corpus only).
//! - EXACT-RULE: every reject row maps to its declared `CoreOfficialValidationRule`,
//!   and every `CoreOfficialValidationRule::ALL` variant has ≥1 corpus row (exhaustive
//!   coverage).
//! - FRESHNESS (behind `svelte-oracle`): the PINNED `svelte@5.56.3` STILL rejects each
//!   committed row with its recorded `official_code`.
//!
//! There is NO `KNOWN_ACCEPTED_INVALID` allowlist — a row Verter accepts is a hard
//! failure of the hermetic gate.

use std::fs;
use std::path::PathBuf;

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{
    compile_client, ClientCompileError, CoreOfficialValidationRule, SvelteRuntimeOptions,
};

/// The reject-corpus root.
fn reject_corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus/rejects/block4_core")
}

/// One committed reject-corpus row: the fixture basename, its malformed `.svelte`
/// source (the canonical authority), the declared `CoreOfficialValidationRule`, and
/// the recorded official error code.
struct RejectRow {
    name: String,
    source: String,
    rule: CoreOfficialValidationRule,
    official_code: String,
}

/// Load every reject-corpus row (`<name>.svelte` + `<name>.json`), sorted by name.
/// The `.svelte` file is the canonical source; the `.json` carries the declared rule
/// plus the recorded official code. Panics on a malformed metadata file (a missing or
/// unknown `rule`, a missing `official_code`) — the corpus is a closed contract.
fn load_reject_corpus() -> Vec<RejectRow> {
    let dir = reject_corpus_dir();
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read reject corpus dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "svelte").unwrap_or(false))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "the reject corpus must not be empty: {}",
        dir.display()
    );

    names
        .into_iter()
        .map(|name| {
            let source = fs::read_to_string(dir.join(format!("{name}.svelte")))
                .unwrap_or_else(|e| panic!("read {name}.svelte: {e}"));
            let json_text = fs::read_to_string(dir.join(format!("{name}.json")))
                .unwrap_or_else(|e| panic!("read {name}.json: {e}"));
            let (rule_name, official_code) = parse_metadata(&name, &json_text);
            let rule = CoreOfficialValidationRule::from_name(&rule_name).unwrap_or_else(|| {
                panic!("{name}.json: unknown rule `{rule_name}` (not a CoreOfficialValidationRule)")
            });
            RejectRow {
                name,
                source,
                rule,
                official_code,
            }
        })
        .collect()
}

/// Parse the two string fields (`rule`, `official_code`) from a reject-row metadata
/// JSON without a JSON dependency — the metadata is a flat two-key object. Returns
/// `(rule, official_code)`; panics on a missing key.
fn parse_metadata(name: &str, json_text: &str) -> (String, String) {
    let rule = json_string_field(json_text, "rule")
        .unwrap_or_else(|| panic!("{name}.json: missing `rule` field"));
    let code = json_string_field(json_text, "official_code")
        .unwrap_or_else(|| panic!("{name}.json: missing `official_code` field"));
    (rule, code)
}

/// Extract a `"key": "value"` string field from a flat metadata JSON object. Returns
/// the value, or `None` when the key is absent. (The corpus metadata is a closed,
/// hand-authored flat object — a minimal scanner avoids a JSON crate dependency in the
/// test binary.)
fn json_string_field(json_text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after_key = &json_text[json_text.find(&needle)? + needle.len()..];
    let after_colon = &after_key[after_key.find(':')? + 1..];
    let open = after_colon.find('"')? + 1;
    let rest = &after_colon[open..];
    let close = rest.find('"')?;
    Some(rest[..close].to_string())
}

/// Compile a reject-row source through the client backend.
fn compile(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// The `CoreOfficialValidationRule` a `compile_client` REFUSAL classifies to, or an
/// error string when the row did not fail closed (an emitted `Main` — the accept-
/// invalid leak) or failed on an unmappable surface.
///
/// A refusal classifies through TWO channels: the official-reject gate carries the
/// rule directly; an already-fail-closed unsupported surface maps via
/// [`CoreOfficialValidationRule::from_unsupported_surface`]. The `AdvancedRune`
/// surface is intentionally NOT auto-mapped (it covers BOTH genuine rune-arity rejects
/// AND deferrable unsupported rune forms), so a rune-domain reject row whose declared
/// rule is `RuneInvalidArguments` / `PropsInvalidPattern` is accepted when it fails
/// closed as an advanced-rune surface (the declared rule is the authority for the
/// expected class; the surface only has to prove fail-closed).
fn classify_refusal(
    declared: CoreOfficialValidationRule,
    result: &Result<String, ClientCompileError>,
) -> Result<CoreOfficialValidationRule, String> {
    match result {
        Ok(js) => Err(format!("emitted a Main (accept-invalid leak):\n{js}")),
        Err(ClientCompileError::OfficialReject(rejection)) => Ok(rejection.rule),
        Err(ClientCompileError::Unsupported(surface)) => {
            if let Some(rule) = CoreOfficialValidationRule::from_unsupported_surface(surface) {
                Ok(rule)
            } else if is_rune_domain_reject(declared)
                && matches!(
                    surface,
                    verter_compiler::svelte::runtime::UnsupportedSvelteRuntimeSurface::AdvancedRune { .. }
                )
            {
                // A rune-arity / `$props()`-pattern official reject Verter routes
                // through the advanced-rune fail-closed surface. The declared rule is
                // the authority for the class; the surface proves fail-closed.
                Ok(declared)
            } else {
                Err(format!(
                    "fails closed as an unsupported surface `{}` that does not map to an \
                     official-reject rule (declared {declared:?})",
                    surface.diagnostic_code()
                ))
            }
        }
        Err(other) => Err(format!("unexpected refusal {other:?}")),
    }
}

/// Whether a rule is a rune-domain official reject (routed through Verter's
/// advanced-rune fail-closed surface rather than the official-reject gate).
fn is_rune_domain_reject(rule: CoreOfficialValidationRule) -> bool {
    matches!(
        rule,
        CoreOfficialValidationRule::RuneInvalidArguments
            | CoreOfficialValidationRule::PropsInvalidPattern
    )
}

/// The EXACT official code a `compile_client` refusal CARRIES, or `None` when the row
/// fails closed through the `Unsupported` (unsupported-FEATURE) channel — that channel
/// proves fail-closed but does NOT carry an official diagnostic code (the surface is an
/// `UnsupportedSvelteRuntimeSurface`, not an `OfficialRejection`). An emitted `Main` (the
/// accept-invalid leak) returns an `Err` string.
fn carried_official_code(
    result: &Result<String, ClientCompileError>,
) -> Result<Option<String>, String> {
    match result {
        Ok(js) => Err(format!("emitted a Main (accept-invalid leak):\n{js}")),
        Err(ClientCompileError::OfficialReject(rejection)) => {
            Ok(Some(rejection.official_code.to_string()))
        }
        Err(ClientCompileError::Unsupported(_)) => Ok(None),
        Err(other) => Err(format!("unexpected refusal {other:?}")),
    }
}

/// The EXPLICIT, audited set of reject-corpus rows that fail closed through the
/// `Unsupported` (unsupported-FEATURE) channel rather than an `OfficialReject` carrying the
/// exact official code — each because Verter already owns the input on the unsupported-
/// surface rail (a template-element duplicate attribute, a duplicate `<svelte:options>`, a
/// rune-arity / `$props()`-pattern reject). A row here asserts ONLY "fails closed (no
/// `Main`) AND classifies to its declared rule" — it is NOT held to the exact-code
/// assertion (the surface carries no official code). Every other reject row MUST be an
/// `OfficialReject` whose `official_code` equals the committed code.
///
/// This is NOT a leniency escape hatch — every entry still fails closed; it only records
/// WHICH refusal owner is responsible so the exact-code gate stays discriminating for the
/// `OfficialReject`-channel families.
const OFFICIAL_CODE_LESS_ROWS: &[(&str, &str)] = &[
    // NOTE: a TEMPLATE-element duplicate attribute (`attribute_duplicate_id`) and a duplicate
    // `<svelte:options>` (`options_invalid_duplicate`) are NO LONGER code-less — they are
    // EXACT-CODE parser facts the official-reject gate carries (`attribute_duplicate` /
    // `svelte_meta_duplicate`), so they are held to the exact-code assertion like every other
    // OfficialReject-channel row.
    (
        "rune_invalid_arguments_props",
        "a rune-arity reject Verter owns on the unsupported AdvancedRune surface",
    ),
    (
        "rune_invalid_arguments_state",
        "a rune-arity reject Verter owns on the unsupported AdvancedRune surface",
    ),
    (
        "props_invalid_pattern_duplicate",
        "a duplicate `$props()` Verter owns on the unsupported AdvancedRune surface",
    ),
];

/// Whether a reject row is an audited code-less (unsupported-channel) exception.
fn is_official_code_less_row(name: &str) -> bool {
    OFFICIAL_CODE_LESS_ROWS.iter().any(|(n, _)| *n == name)
}

// ─────────────────────────────────────────────────────────────────────────────
// (1) DEFAULT hermetic gate — every reject row fails closed (no `Main`).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_reject_row_fails_closed_with_no_main() {
    let mut leaks = Vec::new();
    for row in load_reject_corpus() {
        // Any typed refusal (official-reject OR unsupported surface) is a pass for the
        // hermetic gate — the row produced NO `Main`. Only an emitted `Main` is a leak.
        if let Ok(js) = compile(&row.source) {
            leaks.push(format!(
                "{}: official REJECTS this ({}), but Verter emitted a Main:\n{js}",
                row.name, row.official_code
            ));
        }
    }
    assert!(
        leaks.is_empty(),
        "the official-reject corpus has accept-invalid leaks (official rejects, Verter \
         must reject):\n{}",
        leaks.join("\n\n")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (2) EXACT-RULE gate — every row maps to its declared rule; every rule variant is
//     covered by ≥1 corpus row.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_reject_row_maps_to_its_declared_rule() {
    let mut wrong = Vec::new();
    for row in load_reject_corpus() {
        let result = compile(&row.source);
        match classify_refusal(row.rule, &result) {
            Ok(actual) if actual == row.rule => {}
            Ok(actual) => wrong.push(format!(
                "{}: declared rule {:?}, but the refusal classified to {actual:?}",
                row.name, row.rule
            )),
            Err(why) => wrong.push(format!("{}: {why}", row.name)),
        }
    }
    assert!(
        wrong.is_empty(),
        "reject rows whose refusal did not classify to the declared rule:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn every_reject_row_carries_its_exact_official_code() {
    // The strengthened exact-code gate: every reject row that fails closed as an
    // `OfficialReject` MUST carry the EXACT committed `official_code` (not merely the right
    // rule, and not the rule's REPRESENTATIVE code). This is what forces a multi-code rule
    // to construct a SITE-SPECIFIC rejection (e.g. `script_invalid_module_value` →
    // `script_invalid_attribute_value`, a `$$props` global ref → `legacy_props_invalid`, a
    // same-scope declaration duplicate → `js_parse_error`) instead of stamping the
    // representative code. A row that fails closed through the `Unsupported` channel carries
    // NO official code and must be an audited `OFFICIAL_CODE_LESS_ROWS` exception.
    let mut wrong = Vec::new();
    for row in load_reject_corpus() {
        let result = compile(&row.source);
        match carried_official_code(&result) {
            Ok(Some(code)) => {
                if code != row.official_code {
                    wrong.push(format!(
                        "{}: refusal carries official code `{code}`, but the committed code is \
                         `{}`",
                        row.name, row.official_code
                    ));
                }
            }
            Ok(None) => {
                // A code-less (Unsupported-channel) refusal is acceptable ONLY for an
                // audited exception row.
                if !is_official_code_less_row(&row.name) {
                    wrong.push(format!(
                        "{}: fails closed through the Unsupported channel (no official code), but \
                         the row is not an audited OFFICIAL_CODE_LESS_ROWS exception (expected \
                         `{}`)",
                        row.name, row.official_code
                    ));
                }
            }
            Err(why) => wrong.push(format!("{}: {why}", row.name)),
        }
    }
    assert!(
        wrong.is_empty(),
        "reject rows whose carried official code did not equal the committed code (a leak, a \
         representative-code stamp, or an un-audited code-less refusal):\n{}",
        wrong.join("\n")
    );
}

#[test]
fn official_code_less_exception_rows_are_real_unsupported_channel_rows() {
    // Every audited OFFICIAL_CODE_LESS_ROWS entry must name a REAL reject corpus row that
    // ACTUALLY fails closed through the Unsupported channel (carries no official code) — a
    // stale exception (a row that now carries an exact code, or no longer exists) is removed
    // so the exception list cannot silently outlive its reason and weaken the exact-code gate.
    let corpus = load_reject_corpus();
    let mut stale = Vec::new();
    for (name, _reason) in OFFICIAL_CODE_LESS_ROWS {
        let row = corpus.iter().find(|r| &r.name == name);
        match row {
            None => stale.push(format!("{name}: not a corpus row")),
            Some(row) => match carried_official_code(&compile(&row.source)) {
                Ok(None) => {}
                Ok(Some(code)) => stale.push(format!(
                    "{name}: now carries an exact official code `{code}` (it is an \
                     OfficialReject) — remove it from OFFICIAL_CODE_LESS_ROWS",
                )),
                Err(why) => stale.push(format!("{name}: {why}")),
            },
        }
    }
    assert!(
        stale.is_empty(),
        "these OFFICIAL_CODE_LESS_ROWS entries are stale (not a real Unsupported-channel \
         reject row) — remove them:\n{}",
        stale.join("\n")
    );
}

#[test]
fn every_official_validation_rule_variant_has_a_corpus_row() {
    let corpus = load_reject_corpus();
    let mut missing = Vec::new();
    for &rule in CoreOfficialValidationRule::ALL {
        if !corpus.iter().any(|row| row.rule == rule) {
            missing.push(format!(
                "{rule:?} ({})",
                rule.representative_official_code()
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "every CoreOfficialValidationRule variant must have ≥1 reject-corpus row; \
         missing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn no_reject_row_leaks_a_main() {
    // The architect's bar, proven STRUCTURALLY (not by a constant): iterate every
    // reject-corpus row and assert NONE emits a `Main`. This is the real "no
    // accepted-invalid" guarantee — it FAILS if a leak is reintroduced, with NO
    // `KNOWN_ACCEPTED_INVALID` escape hatch. (A constant-only `&[] .is_empty()` assertion
    // passes even with the production change reverted; this does not.)
    let corpus = load_reject_corpus();
    assert!(!corpus.is_empty(), "the reject corpus must not be empty");
    let mut leaks = Vec::new();
    for row in &corpus {
        if let Ok(js) = compile(&row.source) {
            leaks.push(format!(
                "{}: official REJECTS this ({}) but Verter leaked a Main:\n{js}",
                row.name, row.official_code
            ));
        }
    }
    assert!(
        leaks.is_empty(),
        "reject-corpus rows that leaked a Main (no accepted-invalid escape hatch \
         exists):\n{}",
        leaks.join("\n\n")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (3) FRESHNESS gate (svelte-oracle) — the pinned compiler STILL rejects each row
//     with its recorded official code.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "svelte-oracle")]
mod freshness {
    use super::*;
    use std::collections::BTreeMap;
    use std::process::Command;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("crate is <ws>/crates/verter_compiler")
            .to_path_buf()
    }

    /// Run the reject oracle (`scripts/svelte-reject-oracle.mjs`) through the PINNED
    /// `svelte@5.56.3` compiler, returning a `{ name → official-code | "ACCEPT" }` map.
    /// Opting into `--features svelte-oracle` asserts the live toolchain is present, so
    /// a missing / failing node run is a HARD failure here (never a silent skip).
    fn run_reject_oracle() -> BTreeMap<String, String> {
        let root = workspace_root();
        let script = root.join("scripts/svelte-reject-oracle.mjs");
        assert!(
            script.exists(),
            "reject oracle missing: {}",
            script.display()
        );

        let output = Command::new("node")
            .arg(&script)
            .current_dir(&root)
            .output()
            .expect("run svelte-reject-oracle.mjs (node must be on PATH under svelte-oracle)");
        assert!(
            output.status.success(),
            "svelte-reject-oracle.mjs failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("oracle stdout is utf8");
        parse_oracle_json(&stdout)
    }

    /// Parse the reject oracle's flat `{ "name": "code" }` JSON object (no JSON crate
    /// in the test binary — the oracle output is a closed flat string→string map).
    fn parse_oracle_json(text: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let body = text
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim();
        if body.is_empty() {
            return map;
        }
        for entry in body.split(",\n") {
            let entry = entry.trim().trim_end_matches(',');
            if entry.is_empty() {
                continue;
            }
            let (k, v) = entry.split_once(':').expect("oracle entry has a colon");
            let key = k.trim().trim_matches('"').to_string();
            let val = v.trim().trim_matches('"').to_string();
            map.insert(key, val);
        }
        map
    }

    #[test]
    fn pinned_compiler_still_rejects_every_committed_row_with_its_official_code() {
        let oracle = run_reject_oracle();
        let mut wrong = Vec::new();
        for row in load_reject_corpus() {
            match oracle.get(&row.name) {
                Some(code) if *code == row.official_code => {}
                Some(code) if code == "ACCEPT" => wrong.push(format!(
                    "{}: the pinned compiler now ACCEPTS this — it is no longer an official \
                     reject (was `{}`); remove the corpus row or re-classify it",
                    row.name, row.official_code
                )),
                Some(code) => wrong.push(format!(
                    "{}: recorded official_code `{}`, but the pinned compiler emits `{code}`",
                    row.name, row.official_code
                )),
                None => wrong.push(format!(
                    "{}: no oracle outcome (the `.svelte` fixture was not compiled — a \
                     corpus/oracle mismatch)",
                    row.name
                )),
            }
        }
        assert!(
            wrong.is_empty(),
            "reject-corpus rows that drifted from the pinned svelte@5.56.3 compiler:\n{}",
            wrong.join("\n")
        );
    }

    #[test]
    fn pinned_compiler_rejects_count_matches_corpus_size() {
        // Every committed `.svelte` fixture must be compiled by the oracle (a 1:1
        // corpus↔oracle correspondence) — a stray oracle entry or an uncompiled
        // fixture is a structural mismatch.
        let oracle = run_reject_oracle();
        let corpus = load_reject_corpus();
        assert_eq!(
            oracle.len(),
            corpus.len(),
            "the reject oracle compiled {} fixtures but the corpus has {} rows",
            oracle.len(),
            corpus.len()
        );
    }
}
