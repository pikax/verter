//! Rule-correctness gate for deferred Class A fixtures.
//!
//! Counting fixtures does not prove they discriminate.
//! `ensure_class_a_expected_matches_snapshot()` only enforces
//! consistency between expected and snapshot, NOT correctness of
//! expected. A worker that fails the "rule-correct" gate could
//! commit a self-confirming snapshot where post-fix snapshot equals
//! (still-wrong) expected.
//!
//! The rule-correctness gate adds a programmatic byte-equality test:
//! read the rule-correct expected value as DATA from the workspace's
//! `phase-00-tier1-mismatches.md` (a fenced ```json``` block per
//! fixture id), then compare it byte-for-byte against the Verter
//! output for the same fixture.
//!
//! Discrimination: this test asserts Verter's resolver-context shadow
//! gate routes through the userland declaration so all three Cfg
//! members surface (`["alpha", "beta", "gamma"]`), not just
//! `["alpha"]`.
//!
//! Negative assertion: refusal to regenerate. The test PANICS if
//! `UPDATE_SNAPSHOTS=1` is set in the environment when the test
//! runs — this catches the self-confirming-snapshot anti-pattern.

use std::path::PathBuf;
use std::sync::Arc;

use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use verter_session::{HostConfig, VerterHost};

use super::fixtures::{CorrectnessFixture, FIXTURES};
use super::snapshot_view::SnapshotView;

/// Workspace path to `phase-00-tier1-mismatches.md`. The .md file
/// lives at the worktree root; tests run from the crate manifest
/// dir, so we walk two levels up.
fn mismatches_md_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // worktree root
        .join("phase-00-tier1-mismatches.md")
}

/// Workspace path to `phase-00b-tier1-mismatches.md` (the
/// component-meta-property deferred-fixture log). Same layout as
/// [`mismatches_md_path`] but for the row-1/row-2 entries
/// (`fixture_slots_typed`, `fixture_models`).
fn mismatches_md_path_00b() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..") // crates/
        .join("..") // worktree root
        .join("phase-00b-tier1-mismatches.md")
}

/// Parse the rule-correct expected `SnapshotView` for `fixture_id`
/// from `path` (a fenced ```json``` block per fixture id with the
/// shape `{ "fixture_id": "<id>", "expected": <SnapshotView> }`).
///
/// Returns `Some(view)` when the block is present and parses;
/// `None` when the fixture has no machine-readable rule-correct
/// expected (the .md file needs the block authored).
fn read_rule_correct_block_from(path: &PathBuf, fixture_id: &str) -> Option<SnapshotView> {
    let md = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "{} must be readable from the worktree root: {e}",
            path.display()
        )
    });
    let mut blocks = md.split("```json");
    blocks.next(); // skip prefix before any block
    for raw in blocks {
        // Each block starts with the JSON body and is terminated
        // by a "```" fence. Trim everything past the closing fence.
        let body = raw.split("```").next().unwrap_or("");
        // Skip blocks that don't carry a fixture_id field; tolerate
        // arbitrary whitespace + leading newline.
        let parsed: serde_json::Value = match serde_json::from_str(body.trim()) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = parsed
            .get("fixture_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if id != fixture_id {
            continue;
        }
        let expected = parsed.get("expected")?.clone();
        let view: SnapshotView = serde_json::from_value(expected)
            .expect("rule-correct expected block must deserialize as SnapshotView");
        return Some(view);
    }
    None
}

/// Parse the rule-correct expected `SnapshotView` for `fixture_id`
/// from `phase-00-tier1-mismatches.md`. Wrapper around
/// [`read_rule_correct_block_from`].
fn read_rule_correct_block_from_mismatches_md(fixture_id: &str) -> Option<SnapshotView> {
    read_rule_correct_block_from(&mismatches_md_path(), fixture_id)
}

/// Parse the rule-correct expected `SnapshotView` for `fixture_id`
/// from `phase-00b-tier1-mismatches.md`. Wrapper around
/// [`read_rule_correct_block_from`] for the component-meta-property
/// deferrals (rows 1 and 2).
fn read_rule_correct_block_from_mismatches_md_00b(fixture_id: &str) -> Option<SnapshotView> {
    read_rule_correct_block_from(&mismatches_md_path_00b(), fixture_id)
}

/// Build a hermetic host using the same MemoryWorkspace pattern
/// the main correctness test uses; audit_enabled mirrors that
/// pattern so AuditedRequest::resolve runs without complaint.
fn build_host(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), Arc::from(*content));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

/// Compute the post-fix `SnapshotView` for `fixture_id` by running
/// Verter's resolver against the fixture's SFC + dep files and
/// projecting through `SnapshotView::from_analysis`.
fn run_resolver_under_audit_and_serialize(fixture_id: &str) -> SnapshotView {
    let fixture: &CorrectnessFixture = FIXTURES
        .iter()
        .find(|f| f.id == fixture_id)
        .unwrap_or_else(|| panic!("fixture `{fixture_id}` must be registered in FIXTURES"));
    let host = build_host(fixture.files);
    let req = verter_session::audited_request::AuditedRequest::builder()
        .attach_to(std::sync::Arc::clone(&host))
        .resolve_component_meta(fixture.target);
    let analysis = match req {
        Ok((analysis, _resolution, _record)) => analysis,
        Err(e) => panic!(
            "rule-correctness gate: fixture `{fixture_id}` \
             (`{}`) must resolve, got {e}",
            fixture.target,
        ),
    };
    SnapshotView::from_analysis(&host, &analysis)
}

/// Rule-correctness gate for the `userland_shadowing_pick` deferred
/// fixture.
///
/// Reads the rule-correct expected `SnapshotView` from the JSON
/// block in `phase-00-tier1-mismatches.md` (row 5 — TS-first /
/// userland shadow). Compares it byte-for-byte against the
/// Verter output for the same fixture.
///
/// **Discrimination:** the resolver-context shadow gate routes
/// through the userland declaration so all three Cfg members
/// (alpha + beta + gamma) surface, not just `alpha` from the lib's
/// mapped Pick.
///
/// **Negative assertion:** refusal to regenerate. The test PANICS
/// if `UPDATE_SNAPSHOTS=1` is set in the environment, catching
/// the self-confirming-snapshot anti-pattern.
#[test]
fn deferred_fixture_userland_shadowing_pick_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md("userland_shadowing_pick").unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `userland_shadowing_pick` not found in \
                 phase-00-tier1-mismatches.md. The \
                 .md file must carry a machine-readable JSON block \
                 (`{{ \"fixture_id\": \"userland_shadowing_pick\", \"expected\": <SnapshotView> }}`). \
                 Surface to user."
            );
        });

    let actual = run_resolver_under_audit_and_serialize("userland_shadowing_pick");

    // Byte-equal comparison via canonical JSON serialization. The
    // SnapshotView projection sorts collections by name in
    // `from_analysis`, so the serialized output is stable across
    // runs.
    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `userland_shadowing_pick` MUST byte-equal the rule-correct expected \
         from phase-00-tier1-mismatches.md. Either the ScopeShadowing \
         thread did not fully close the userland-shadow-pick gap \
         OR the rule-correct expected was wrong (escalate to user, do NOT \
         regenerate)."
    );
}

/// Rule-correctness gate for the `mapped_exclude` deferred fixture
/// (row 1).
///
/// **Discrimination:** the `Extract` / `Exclude` arm dispatches
/// per-member through `relate_nodes` and reconstitutes survivors as
/// `"a" | "c"`, rather than surfacing `kind: /*unknown*/
/// semanticMiss`.
///
/// **Negative assertion:** UPDATE_SNAPSHOTS=1 PANICS — the
/// self-confirming-snapshot trap is closed.
#[test]
fn deferred_fixture_mapped_exclude_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected = read_rule_correct_block_from_mismatches_md("mapped_exclude")
        .unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `mapped_exclude` not found in \
                 phase-00-tier1-mismatches.md. The \
                 .md file must carry a machine-readable JSON block \
                 (`{{ \"fixture_id\": \"mapped_exclude\", \"expected\": <SnapshotView> }}`). \
                 Surface to user."
            );
        });

    let actual = run_resolver_under_audit_and_serialize("mapped_exclude");

    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `mapped_exclude` MUST byte-equal the rule-correct expected from \
         phase-00-tier1-mismatches.md row 1. Either the \
         Exclude/Extract reduction did not fully close the gap \
         OR the rule-correct expected was wrong (escalate to user, do NOT \
         regenerate)."
    );
}

/// Rule-correctness gate for the `mapped_extract` deferred fixture
/// (row 2).
///
/// **Discrimination:** the per-member relation engine dispatch keeps
/// `'a'` and `'b'` (assignable to filter union `'a' | 'b'`) and drops
/// `'c'`, producing `"a" | "b"`, rather than surfacing
/// `/*unknown*/ semanticMiss`.
///
/// **Negative assertion:** UPDATE_SNAPSHOTS=1 PANICS.
#[test]
fn deferred_fixture_mapped_extract_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected = read_rule_correct_block_from_mismatches_md("mapped_extract")
        .unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `mapped_extract` not found in \
                 phase-00-tier1-mismatches.md. The \
                 .md file must carry a machine-readable JSON block \
                 (`{{ \"fixture_id\": \"mapped_extract\", \"expected\": <SnapshotView> }}`). \
                 Surface to user."
            );
        });

    let actual = run_resolver_under_audit_and_serialize("mapped_extract");

    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `mapped_extract` MUST byte-equal the rule-correct expected from \
         phase-00-tier1-mismatches.md row 2. Either the \
         Exclude/Extract reduction did not fully close the gap \
         OR the rule-correct expected was wrong (escalate to user, do NOT \
         regenerate)."
    );
}

/// Rule-correctness gate for the `template_literal_as_key` deferred
/// fixture (row 3).
///
/// **Discrimination:** `build_mapped_type` substitutes the iteration
/// key into the remap expression and folds the `TemplateLiteral`
/// evaluator's output to a `Literal::String`, producing `prefixA` and
/// `prefixB` — not props keyed by the SOURCE union (`A`, `B`) that a
/// mapped-type evaluator without `mapper.name_remap` would emit.
///
/// **Negative assertion:** UPDATE_SNAPSHOTS=1 PANICS.
#[test]
fn deferred_fixture_template_literal_as_key_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md("template_literal_as_key").unwrap_or_else(
            || {
                panic!(
                    "Rule-correct expected for `template_literal_as_key` not found in \
                     phase-00-tier1-mismatches.md. The \
                     .md file must carry a machine-readable JSON block \
                     (`{{ \"fixture_id\": \"template_literal_as_key\", \"expected\": <SnapshotView> }}`). \
                     Surface to user."
                );
            },
        );

    let actual = run_resolver_under_audit_and_serialize("template_literal_as_key");

    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `template_literal_as_key` MUST byte-equal the rule-correct \
         expected from phase-00-tier1-mismatches.md row 3. Either the \
         mapper name_remap + TemplateLiteral fold did not fully \
         close the gap OR the rule-correct expected was wrong \
         (escalate to user, do NOT regenerate)."
    );
}

/// Rule-correctness gate for the `fixture_slots_typed` deferred
/// fixture (`phase-00b-tier1-mismatches.md` row 1).
///
/// **Discrimination:** the
/// `ProjectSemanticDispatch::project_slot_binding_member` helper
/// composes existing variants to descend through `Function` ->
/// `params[0].ty` -> `Member(binding)`. The binding raises to
/// `Primitive(String)` / `Primitive(Number)`, and the
/// `SnapshotView`'s slot `payload_signature` renders
/// `{ item: string }` / `{ row: number }`, rather than raising to
/// `Unknown { raw: "semanticMiss" }`.
///
/// **Negative assertion:** `UPDATE_SNAPSHOTS=1` PANICS — the
/// self-confirming-snapshot trap is closed.
#[test]
fn deferred_fixture_fixture_slots_typed_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md_00b("fixture_slots_typed").unwrap_or_else(
            || {
                panic!(
                    "Rule-correct expected for `fixture_slots_typed` not found in \
                     phase-00b-tier1-mismatches.md. The \
                     .md file must carry a machine-readable JSON block \
                     (`{{ \"fixture_id\": \"fixture_slots_typed\", \"expected\": <SnapshotView> }}`). \
                     Surface to user."
                );
            },
        );

    let actual = run_resolver_under_audit_and_serialize("fixture_slots_typed");

    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `fixture_slots_typed` MUST byte-equal the rule-correct expected \
         from phase-00b-tier1-mismatches.md row 1. Either the \
         `project_slot_binding_member` helper did not fully close the \
         slot-binding lowering gap OR the rule-correct expected \
         was wrong (escalate to user, do NOT regenerate)."
    );
}

/// Rule-correctness gate for the `generic_substitution_via_typeof`
/// deferred fixture (`phase-00-tier1-mismatches.md` row 4).
///
/// **Discrimination:** the lowering attempts single-segment root
/// resolution first (`sample`), succeeds, projects the remaining
/// `["id"]` path through `ProjectPath { mode: Navigate }` to
/// `string`, then substitutes `T → string`. The snapshot renders
/// `id: string`, rather than the bare type-parameter token `id: T`
/// that a `TypeExpr::TypeOf` arm joining `value_ref.path[0..2]` into
/// `"sample.id"` would produce (missed lookup → `Opaque(Miss)` →
/// unsubstituted T).
///
/// **Negative assertion:** `UPDATE_SNAPSHOTS=1` PANICS — the
/// self-confirming-snapshot trap is closed.
#[test]
fn deferred_fixture_generic_substitution_via_typeof_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md("generic_substitution_via_typeof")
            .unwrap_or_else(|| {
                panic!(
                    "Rule-correct expected for `generic_substitution_via_typeof` not found in \
                     phase-00-tier1-mismatches.md. The \
                     .md file must carry a machine-readable JSON block \
                     (`{{ \"fixture_id\": \"generic_substitution_via_typeof\", \"expected\": <SnapshotView> }}`). \
                     Surface to user."
                );
            });

    let actual = run_resolver_under_audit_and_serialize("generic_substitution_via_typeof");

    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `generic_substitution_via_typeof` MUST byte-equal the rule-correct \
         expected from phase-00-tier1-mismatches.md row 4. Either the \
         single-segment-first lookup in `shallow_lower_type_expr`'s \
         `TypeExpr::TypeOf` arm did not fully close the value-member \
         typeof gap OR the rule-correct expected was wrong \
         (escalate to user, do NOT regenerate)."
    );
}

/// Rule-correctness gate for the `fixture_models` deferred fixture
/// (`phase-00b-tier1-mismatches.md` row 2).
///
/// **Discrimination:** the `expand_field_expr`'s `DefineModel`
/// branch in `host_manage.rs::compute_evaluated_types*`
/// lower+raises `parsed_type_argument` directly. The model's
/// `type_expr` becomes `Primitive(String)` / `Primitive(Number)`;
/// the prop's `type_signature` surfaces the same BARE `string` /
/// `number` (the native snapshot renders the published bare carrier;
/// the `T | undefined` optional-model display is a compat-layer
/// projection); the `update:<name>` event's display payload is
/// `[value: T | undefined]`. Without the branch, dispatching
/// `ProjectPath { base, [Member(model)], Expanded }` on a
/// `parsed_type_argument` that IS the field type misses and produces
/// `Unknown { raw: "semanticMiss" }`.
///
/// **Negative assertion:** `UPDATE_SNAPSHOTS=1` PANICS.
#[test]
fn deferred_fixture_fixture_models_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected = read_rule_correct_block_from_mismatches_md_00b("fixture_models")
        .unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `fixture_models` not found in \
                 phase-00b-tier1-mismatches.md. The \
                 .md file must carry a machine-readable JSON block \
                 (`{{ \"fixture_id\": \"fixture_models\", \"expected\": <SnapshotView> }}`). \
                 Surface to user."
            );
        });

    let actual = run_resolver_under_audit_and_serialize("fixture_models");

    let rule_correct_json = serde_json::to_string_pretty(&rule_correct_expected)
        .expect("rule-correct SnapshotView serializes");
    let actual_json =
        serde_json::to_string_pretty(&actual).expect("post-fix SnapshotView serializes");
    assert_eq!(
        actual_json, rule_correct_json,
        "rule-correctness gate: output for \
         `fixture_models` MUST byte-equal the rule-correct expected from \
         phase-00b-tier1-mismatches.md row 2. Either the `expand_field_expr` \
         `DefineModel` branch did not fully close the macro-payload \
         lowering gap OR the rule-correct expected was wrong \
         (escalate to user, do NOT regenerate)."
    );
}
