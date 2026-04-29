//! Phase 5h §5.B.5.1 r15/F9 — rule-correctness gate for deferred Class A
//! fixtures authored by 5h.
//!
//! Counting fixtures does not prove they discriminate.
//! `ensure_class_a_expected_matches_snapshot()` only enforces
//! consistency between expected and snapshot, NOT correctness of
//! expected. A worker that fails the "rule-correct" gate could
//! commit a self-confirming snapshot where post-fix snapshot equals
//! (still-wrong) expected.
//!
//! The §5.B.5.1 r15 rule-correctness gate adds a programmatic
//! byte-equality test: read the rule-correct expected value as
//! DATA from the workspace's `phase-00-tier1-mismatches.md` (a
//! fenced ```json``` block per fixture id), then compare it
//! byte-for-byte against the post-Phase-5h-fix Verter output for
//! the same fixture.
//!
//! Discrimination: this test FAILS pre-Phase-5h-fix (Verter
//! produces `["alpha"]` while the rule-correct expected is
//! `["alpha", "beta", "gamma"]`); PASSES post-fix (Verter's
//! resolver-context shadow gate routes through the userland
//! declaration → all three Cfg members surface).
//!
//! Negative assertion: refusal to regenerate. The test PANICS if
//! `UPDATE_SNAPSHOTS=1` is set in the environment when the test
//! runs — this catches the self-confirming-snapshot anti-pattern
//! the §0p.A.4 case-2 self-test guards against.

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

/// Workspace path to `phase-00b-tier1-mismatches.md` (the Phase 0b
/// deferred-fixture log). Same layout as
/// [`mismatches_md_path`] but for Phase 0b's two row-1/row-2 entries
/// (`fixture_slots_typed`, `fixture_models`) authored by Phase 5j
/// §5.12.
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
/// expected (i.e., the §5.B.5.1 STOP — the .md file needs the
/// block authored).
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
/// [`read_rule_correct_block_from`] for Phase 0b deferrals (rows 1
/// and 2, owned by Phase 5j §5.12).
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
        .attach_to(host)
        .resolve(fixture.target);
    let analysis = match req {
        Ok((analysis, _resolution, _record)) => analysis,
        Err(e) => panic!(
            "Phase 5h §5.B.5.1 rule-correctness gate: fixture `{fixture_id}` \
             (`{}`) must resolve, got {e}",
            fixture.target,
        ),
    };
    SnapshotView::from_analysis(&analysis)
}

/// Phase 5h §5.B.5.1 rule-correctness gate for the
/// `userland_shadowing_pick` deferred fixture.
///
/// Reads the rule-correct expected `SnapshotView` from the JSON
/// block in `phase-00-tier1-mismatches.md` (row 5 — TS-first /
/// userland shadow). Compares it byte-for-byte against the
/// post-Phase-5h-fix Verter output for the same fixture.
///
/// **Discrimination:**
///   - Pre-Phase-5h-fix: Verter resolves `Pick<Cfg, 'alpha'>` to
///     the lib's mapped Pick → surfaces only `alpha`. Test FAILS
///     (rule-correct expected has all three: alpha + beta + gamma).
///   - Post-Phase-5h-fix: the resolver-context shadow gate routes
///     through the userland declaration → all three members
///     surface. Test PASSES.
///
/// **Negative assertion:** refusal to regenerate. The test PANICS
/// if `UPDATE_SNAPSHOTS=1` is set in the environment, catching
/// the self-confirming-snapshot anti-pattern.
#[test]
fn deferred_fixture_userland_shadowing_pick_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (§5.B.5.1 r15/F9 — would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md("userland_shadowing_pick").unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `userland_shadowing_pick` not found in \
                 phase-00-tier1-mismatches.md. Per §5.B.5.1 STOP CONDITIONS, the \
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
        "Phase 5h §5.B.5.1 rule-correctness gate: post-fix output for \
         `userland_shadowing_pick` MUST byte-equal the rule-correct expected \
         from phase-00-tier1-mismatches.md. Either the §5.10 ScopeShadowing \
         thread did not fully close the userland-shadow-pick gap (5h STOP) \
         OR the rule-correct expected was wrong (escalate to user, do NOT \
         regenerate)."
    );
}

/// Phase 5i §5.B.5.1 rule-correctness gate for the `mapped_exclude`
/// deferred fixture (row 1).
///
/// **Discrimination:**
///   - Pre-Phase-5i-fix: Verter surfaces `kind: /*unknown*/
///     semanticMiss` because `Exclude<>` falls into
///     `build_builtin_utility`'s deferred catch-all `_` arm.
///   - Post-Phase-5i-fix: the new `Extract` / `Exclude` arm
///     dispatches per-member through `relate_nodes` and
///     reconstitutes survivors as `"a" | "c"`.
///
/// **Negative assertion:** UPDATE_SNAPSHOTS=1 PANICS — the
/// self-confirming-snapshot trap is closed.
#[test]
fn deferred_fixture_mapped_exclude_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (§5.B.5.1 r15/F9 — would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected = read_rule_correct_block_from_mismatches_md("mapped_exclude")
        .unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `mapped_exclude` not found in \
                 phase-00-tier1-mismatches.md. Per §5.B.5.1 STOP CONDITIONS, the \
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
        "Phase 5i §5.B.5.1 rule-correctness gate: post-fix output for \
         `mapped_exclude` MUST byte-equal the rule-correct expected from \
         phase-00-tier1-mismatches.md row 1. Either the §5.11 \
         Exclude/Extract reduction did not fully close the gap (5i STOP) \
         OR the rule-correct expected was wrong (escalate to user, do NOT \
         regenerate)."
    );
}

/// Phase 5i §5.B.5.1 rule-correctness gate for the `mapped_extract`
/// deferred fixture (row 2).
///
/// **Discrimination:**
///   - Pre-Phase-5i-fix: Verter surfaces `/*unknown*/ semanticMiss`
///     for the same reason as `mapped_exclude` (deferred utility
///     catch-all).
///   - Post-Phase-5i-fix: the per-member relation engine dispatch
///     keeps `'a'` and `'b'` (assignable to filter union
///     `'a' | 'b'`) and drops `'c'`, producing `"a" | "b"`.
///
/// **Negative assertion:** UPDATE_SNAPSHOTS=1 PANICS.
#[test]
fn deferred_fixture_mapped_extract_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (§5.B.5.1 r15/F9 — would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected = read_rule_correct_block_from_mismatches_md("mapped_extract")
        .unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `mapped_extract` not found in \
                 phase-00-tier1-mismatches.md. Per §5.B.5.1 STOP CONDITIONS, the \
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
        "Phase 5i §5.B.5.1 rule-correctness gate: post-fix output for \
         `mapped_extract` MUST byte-equal the rule-correct expected from \
         phase-00-tier1-mismatches.md row 2. Either the §5.11 \
         Exclude/Extract reduction did not fully close the gap (5i STOP) \
         OR the rule-correct expected was wrong (escalate to user, do NOT \
         regenerate)."
    );
}

/// Phase 5i §5.B.5.1 rule-correctness gate for the
/// `template_literal_as_key` deferred fixture (row 3, re-homed
/// from 5k per §5.13 r15 table).
///
/// **Discrimination:**
///   - Pre-Phase-5i-fix: Verter's mapped-type evaluator does not
///     apply `mapper.name_remap`, so iteration produces props
///     keyed by the SOURCE union (`A`, `B`) rather than the
///     remapped names. The pre-existing snapshot would diverge
///     from rule-correct.
///   - Post-Phase-5i-fix: `build_mapped_type` substitutes the
///     iteration key into the remap expression and folds the
///     `TemplateLiteral` evaluator's output to a `Literal::String`,
///     producing `prefixA` and `prefixB`.
///
/// **Negative assertion:** UPDATE_SNAPSHOTS=1 PANICS.
#[test]
fn deferred_fixture_template_literal_as_key_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (§5.B.5.1 r15/F9 — would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md("template_literal_as_key").unwrap_or_else(
            || {
                panic!(
                    "Rule-correct expected for `template_literal_as_key` not found in \
                     phase-00-tier1-mismatches.md. Per §5.B.5.1 STOP CONDITIONS, the \
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
        "Phase 5i §5.B.5.1 rule-correctness gate: post-fix output for \
         `template_literal_as_key` MUST byte-equal the rule-correct \
         expected from phase-00-tier1-mismatches.md row 3. Either the \
         §5.11 mapper name_remap + TemplateLiteral fold did not fully \
         close the gap (5i STOP) OR the rule-correct expected was wrong \
         (escalate to user, do NOT regenerate)."
    );
}

/// Phase 5j §5.B.5.1 rule-correctness gate for the
/// `fixture_slots_typed` deferred fixture
/// (`phase-00b-tier1-mismatches.md` row 1).
///
/// **Discrimination:**
///   - Pre-Phase-5j-fix: Verter dispatches
///     `ProjectPath { base, [Member(slot), Member(binding)],
///     Expanded }` directly. The walker hits the slot's `Function`
///     value with `Member(binding)` remaining and falls through to
///     `opaque_miss` (per `walk.rs` Function arm catch-all). The
///     binding raises to `Unknown { raw: "semanticMiss" }`.
///   - Post-Phase-5j-fix: the new
///     `ProjectSemanticDispatch::project_slot_binding_member`
///     helper composes existing variants to descend through
///     `Function` -> `params[0].ty` -> `Member(binding)`. The
///     binding raises to `Primitive(String)` / `Primitive(Number)`,
///     and the `SnapshotView`'s slot `payload_signature` renders
///     `{ item: string }` / `{ row: number }`.
///
/// **Negative assertion:** `UPDATE_SNAPSHOTS=1` PANICS — the
/// self-confirming-snapshot trap is closed.
#[test]
fn deferred_fixture_fixture_slots_typed_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (§5.B.5.1 r15/F9 — would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected =
        read_rule_correct_block_from_mismatches_md_00b("fixture_slots_typed").unwrap_or_else(
            || {
                panic!(
                    "Rule-correct expected for `fixture_slots_typed` not found in \
                     phase-00b-tier1-mismatches.md. Per §5.B.5.1 STOP CONDITIONS, the \
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
        "Phase 5j §5.B.5.1 rule-correctness gate: post-fix output for \
         `fixture_slots_typed` MUST byte-equal the rule-correct expected \
         from phase-00b-tier1-mismatches.md row 1. Either the §5.12 \
         `project_slot_binding_member` helper did not fully close the \
         slot-binding lowering gap (5j STOP) OR the rule-correct expected \
         was wrong (escalate to user, do NOT regenerate)."
    );
}

/// Phase 5j §5.B.5.1 rule-correctness gate for the `fixture_models`
/// deferred fixture (`phase-00b-tier1-mismatches.md` row 2,
/// re-homed from 5k to 5j per parent §5.13 r15 table).
///
/// **Discrimination:**
///   - Pre-Phase-5j-fix: Verter dispatches
///     `ProjectPath { base, [Member(model)], Expanded }` on a
///     `parsed_type_argument` that IS the field type (typically a
///     `Primitive`, `Ref`, or `Union` with no member to navigate).
///     The dispatch always misses; the closure produces
///     `Unknown { raw: "semanticMiss" }` for both the model
///     `type_expr` and the synthesised prop's `type_signature`.
///   - Post-Phase-5j-fix: the `expand_field_expr`'s `DefineModel`
///     branch in `host_manage.rs::compute_evaluated_types*`
///     lower+raises `parsed_type_argument` directly. The model's
///     `type_expr` becomes `Primitive(String)` / `Primitive(Number)`;
///     the prop's `type_signature` becomes `string | undefined` /
///     `number | undefined` (Vue's optional-by-default contract);
///     the `update:<name>` event's payload is unchanged
///     (`[value: T | undefined]`, already correct pre-Phase-5j).
///
/// **Negative assertion:** `UPDATE_SNAPSHOTS=1` PANICS.
#[test]
fn deferred_fixture_fixture_models_byte_equal_to_rule_correct_expected() {
    if std::env::var("UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        panic!(
            "UPDATE_SNAPSHOTS=1 is FORBIDDEN for deferred fixtures \
             (§5.B.5.1 r15/F9 — would lock in self-confirming snapshot). \
             Either fix the resolver (rule-correct expected stays) OR \
             escalate to user (rule-correct expected was wrong)."
        );
    }

    let rule_correct_expected = read_rule_correct_block_from_mismatches_md_00b("fixture_models")
        .unwrap_or_else(|| {
            panic!(
                "Rule-correct expected for `fixture_models` not found in \
                 phase-00b-tier1-mismatches.md. Per §5.B.5.1 STOP CONDITIONS, the \
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
        "Phase 5j §5.B.5.1 rule-correctness gate: post-fix output for \
         `fixture_models` MUST byte-equal the rule-correct expected from \
         phase-00b-tier1-mismatches.md row 2 (re-homed from 5k to 5j per \
         §5.13 r15 table). Either the §5.12 `expand_field_expr` \
         `DefineModel` branch did not fully close the macro-payload \
         lowering gap (5j STOP) OR the rule-correct expected was wrong \
         (escalate to user, do NOT regenerate)."
    );
}
