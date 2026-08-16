//! Layer-2 vector-inventory reproduction — production side.
//!
//! The JavaScript reference's own test
//! (`packages/framework-conformance-harness/test/assembled-map-composition-vectors.spec.mjs`)
//! reads `vectors/assembled-map-composition.vectors.json` and drives every one
//! of its entries — every positive vector plus every fail-closed vector the
//! suite currently declares, whatever that count is — straight through
//! `composeAssembledVueMainModule`. This module is production's counterpart: it
//! reads the SAME file, builds the SAME [`AssembleInput`] DTO this harness
//! already bridges to the real production entry point through
//! [`production_outcome`], and asserts EXACT agreement with each vector's own
//! frozen `expected`. Both implementations reproducing EVERY vector is not
//! met by a hand-transcribed subset (the earlier
//! `map_tests.rs` `vector_v*`/`vector_f*` functions cover only V1–V7/F1–F7,
//! predating this suite's later completion) — it requires the full inventory,
//! with the EXECUTED ids asserted against the suite's own id inventory. No
//! count is hardcoded anywhere in this module: the expected inventory is
//! derived from the loaded arrays themselves, so it moves with the suite and
//! a driver that silently skips an entry fails the parity assertion.
//!
//! This module adds NO comparison semantics of its own: it reuses
//! [`AssembleInput`], [`production_outcome`], [`ComposeOutcome`],
//! [`compared_artifact`] and [`claimed_segment`] from the parent module
//! unchanged — the same bridge the cross-implementation equality suite runs
//! through — rather than growing a second DTO projection or a second
//! comparator.
//!
//! Unlike the rest of this harness, this module does NOT spawn the JavaScript
//! reference: the vectors file's `expected` is the frozen, hand-derived answer
//! layer 1 already commits to, so comparing production against it directly is
//! strictly what "reproduces the vector" means. Cross-implementation agreement
//! on live input is separately covered by `assert_cross_implementation_equality`
//! and its callers elsewhere in this file.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::*;

fn vectors_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json")
}

fn load_suite() -> Value {
    let path = vectors_path();
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read the layer-2 vector suite at {}: {error}",
            path.display()
        )
    });
    serde_json::from_str(&raw)
        .unwrap_or_else(|error| panic!("the layer-2 vector suite is not valid JSON: {error}"))
}

fn array_member<'a>(value: &'a Value, member: &str) -> &'a [Value] {
    value
        .get(member)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the vector suite is missing its `{member}` array"))
}

fn str_member(value: &Value, member: &str) -> String {
    value
        .get(member)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("vector entry is missing its string member `{member}`"))
        .to_string()
}

fn opt_str_member(value: &Value, member: &str) -> Option<String> {
    match value.get(member) {
        None | Some(Value::Null) => None,
        Some(other) => Some(
            other
                .as_str()
                .unwrap_or_else(|| panic!("`{member}` is present and non-null but not a string"))
                .to_string(),
        ),
    }
}

fn bool_member(value: &Value, member: &str) -> bool {
    value
        .get(member)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("vector entry is missing its boolean member `{member}`"))
}

fn u32_member(value: &Value, member: &str) -> u32 {
    let raw = value
        .get(member)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("vector entry is missing its uint member `{member}`"));
    u32::try_from(raw).unwrap_or_else(|_| panic!("`{member}` does not fit a uint32: {raw}"))
}

impl AssembleInput {
    /// Parse the §3.3 DTO out of a vector's `input` object — the exact reverse
    /// of [`AssembleInput::to_dto_json`], read from the frozen suite rather
    /// than built by a test fixture.
    fn from_dto_json(value: &Value) -> Self {
        let script = match value.get("script") {
            None | Some(Value::Null) => None,
            Some(script) => Some(ScriptFragment {
                code: str_member(script, "code"),
                source_map: str_member(script, "sourceMap"),
            }),
        };
        let template = match value.get("template") {
            None | Some(Value::Null) => None,
            Some(template) => Some(TemplateFragment {
                code: str_member(template, "code"),
                imports: array_member(template, "imports")
                    .iter()
                    .map(|entry| {
                        entry
                            .as_str()
                            .expect("template import entries are strings")
                            .to_string()
                    })
                    .collect(),
                ssr_imports: array_member(template, "ssrImports")
                    .iter()
                    .map(|entry| {
                        entry
                            .as_str()
                            .expect("template ssrImport entries are strings")
                            .to_string()
                    })
                    .collect(),
                source_map: str_member(template, "sourceMap"),
            }),
        };
        let authored = value
            .get("authored")
            .unwrap_or_else(|| panic!("vector input is missing its `authored` object"));
        let hmr_strategy = match str_member(value, "hmrStrategy").as_str() {
            "vite" => HmrStrategy::Vite,
            "webpack" => HmrStrategy::Webpack,
            "none" => HmrStrategy::None,
            other => panic!("unknown `hmrStrategy` spelling `{other}`"),
        };

        Self {
            canonical_id: str_member(value, "canonicalId"),
            style_count: u32_member(value, "styleCount"),
            custom_block_count: u32_member(value, "customBlockCount"),
            style_langs: array_member(value, "styleLangs")
                .iter()
                .map(|entry| entry.as_str().map(str::to_string))
                .collect(),
            custom_types: array_member(value, "customTypes")
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .expect("customTypes entries are strings")
                        .to_string()
                })
                .collect(),
            script,
            template,
            scope_id: str_member(value, "scopeId"),
            runtime_module_name: opt_str_member(value, "runtimeModuleName"),
            is_production: bool_member(value, "isProduction"),
            ssr: bool_member(value, "ssr"),
            ssr_module_id: opt_str_member(value, "ssrModuleId"),
            emit_ssr_module_registration: bool_member(value, "emitSsrModuleRegistration"),
            hmr_strategy,
            source_map_requested: bool_member(value, "sourceMapRequested"),
            authored_script: bool_member(authored, "script"),
            authored_template: bool_member(authored, "template"),
        }
    }
}

/// Parse a vector's frozen `expected` into the same [`ComposeOutcome`] shape
/// [`production_outcome`] returns, so the two can be compared directly.
///
/// This mirrors `reference_outcome`'s three arms exactly, but reads the
/// suite's own static `expected` object instead of a live reference-driver
/// response, and therefore skips that function's reference-self-consistency
/// checks (segments-vs-mappings cross-decode, provenance-length) — there is no
/// second surfaced-segments field here to cross-check against; the suite's
/// `expected.segments` IS the authority, and comparing it against production's
/// own `mappings`-decoded segments (via [`compared_artifact`]) is exactly the
/// proof this module exists to run.
fn expected_outcome(id: &str, expected: &Value) -> ComposeOutcome {
    let outcome = expected
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{id}: `expected` is missing its `outcome` member"));
    match outcome {
        "composed" => {
            let code = str_member(expected, "code");
            let map_value = expected
                .get("map")
                .unwrap_or_else(|| panic!("{id}: a composed `expected` is missing `map`"));
            let map = if map_value.is_null() {
                None
            } else {
                let raw = serde_json::to_string(map_value).unwrap_or_else(|error| {
                    panic!("{id}: expected map does not re-serialize: {error}")
                });
                Some(compared_artifact(
                    &format!("{id} (frozen expected)"),
                    &raw,
                    &code,
                ))
            };
            ComposeOutcome::Composed { code, map }
        }
        "MissingRequiredInputMap" => ComposeOutcome::MissingRequiredInputMap {
            fragment: str_member(expected, "fragment"),
        },
        "UncomposableInputMap" => ComposeOutcome::UncomposableInputMap {
            fragment: str_member(expected, "fragment"),
            family: str_member(expected, "family"),
            code: str_member(expected, "code"),
        },
        other => panic!("{id}: unknown `expected.outcome` `{other}`"),
    }
}

/// Drive every entry of one suite array through production and compare each
/// against its own frozen `expected`, returning the ids that were ACTUALLY
/// driven (in array order) plus every divergence found.
///
/// The executed-id list is the coverage evidence the callers assert against
/// the suite's own inventory — a driver change that silently skips an entry
/// shrinks this list and fails the parity assertions, rather than passing by
/// omission. An id counts as executed once production RAN it, independent of
/// whether the outcome diverged; divergence is asserted separately.
fn reproduce(vectors: &[Value]) -> (Vec<String>, Vec<String>) {
    let mut executed = Vec::with_capacity(vectors.len());
    let mut divergences = Vec::new();
    for entry in vectors {
        let id = str_member(entry, "id");
        let input = AssembleInput::from_dto_json(
            entry
                .get("input")
                .unwrap_or_else(|| panic!("{id}: vector entry is missing `input`")),
        );
        let actual = production_outcome(&input);
        executed.push(id.clone());
        let expected = expected_outcome(
            &id,
            entry
                .get("expected")
                .unwrap_or_else(|| panic!("{id}: vector entry is missing `expected`")),
        );
        if actual != expected {
            divergences.push(format!(
                "── {id} ──\n  production: {actual:#?}\n  frozen expected: {expected:#?}"
            ));
        }
    }
    (executed, divergences)
}

/// The ids one suite array declares, in array order — the inventory the
/// executed ids must exactly reproduce.
fn suite_ids(vectors: &[Value]) -> Vec<String> {
    vectors
        .iter()
        .map(|entry| str_member(entry, "id"))
        .collect()
}

/// Every positive vector, reproduced by production against its own frozen
/// `expected` — mirrors the JavaScript reference's `describe("layer-2 seed
/// vectors — positive (§9)")` block, one production run per vector. The
/// executed ids are asserted against the suite's own inventory, so the
/// expected count is DERIVED from the loaded array, never hardcoded.
#[test]
fn every_positive_vector_reproduces_its_frozen_expected() {
    let suite = load_suite();
    let vectors = array_member(&suite, "vectors");
    assert!(
        !vectors.is_empty(),
        "the suite's positive `vectors` array is empty — nothing to reproduce"
    );

    let (executed, divergences) = reproduce(vectors);
    assert_eq!(
        executed,
        suite_ids(vectors),
        "the positive vectors actually driven through production do not match the suite's own \
         inventory — a vector was skipped or run out of order"
    );
    assert!(
        divergences.is_empty(),
        "production diverges from the frozen layer-2 `expected` on {} of {} positive vectors:\n\n{}",
        divergences.len(),
        vectors.len(),
        divergences.join("\n\n")
    );
}

/// Every fail-closed vector, reproduced by production against its own frozen
/// `expected` — mirrors the JavaScript reference's `describe("layer-2 seed
/// vectors — fail-closed (§9)")` block. Same derived executed-id parity as
/// the positive arm.
#[test]
fn every_fail_closed_vector_reproduces_its_frozen_expected() {
    let suite = load_suite();
    let vectors = array_member(&suite, "failClosedVectors");
    assert!(
        !vectors.is_empty(),
        "the suite's `failClosedVectors` array is empty — nothing to reproduce"
    );

    let (executed, divergences) = reproduce(vectors);
    assert_eq!(
        executed,
        suite_ids(vectors),
        "the fail-closed vectors actually driven through production do not match the suite's own \
         inventory — a vector was skipped or run out of order"
    );
    assert!(
        divergences.is_empty(),
        "production diverges from the frozen layer-2 `expected` on {} of {} fail-closed vectors:\n\n{}",
        divergences.len(),
        vectors.len(),
        divergences.join("\n\n")
    );
}

/// The executed inventory, asserted against the suite's OWN id inventory —
/// never a hardcoded count that could be bumped independently of the real
/// array contents. This runs the same driver the two reproduction tests run
/// and asserts exact id-sequence parity across BOTH arrays, so "a vector was
/// silently not exercised" is a structural failure here even if someone
/// edits one reproduction test without the other.
#[test]
fn every_vector_in_the_suite_was_exercised() {
    let suite = load_suite();
    let positives = array_member(&suite, "vectors");
    let fail_closed = array_member(&suite, "failClosedVectors");

    let mut expected_ids = suite_ids(positives);
    expected_ids.extend(suite_ids(fail_closed));
    assert!(
        !expected_ids.is_empty(),
        "the suite declares no vectors at all — nothing to exercise"
    );
    {
        let mut unique = expected_ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            expected_ids.len(),
            "the suite declares a duplicate vector id — id parity would be ambiguous"
        );
    }

    let mut executed = reproduce(positives).0;
    executed.extend(reproduce(fail_closed).0);
    assert_eq!(
        executed, expected_ids,
        "the vectors actually driven through production do not exactly reproduce the suite's own \
         id inventory (both arrays, in array order) — a vector was skipped, duplicated, or \
         fabricated"
    );
}
