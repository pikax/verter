//! Phase 0 — correctness gate (Tier 1 + Tier 2 unified test target).
//!
//! Cargo discovers this file as the `correctness` integration-test
//! target. Submodules live at `tests/correctness/<file>.rs` and are
//! included via `#[path]` (matches the repo's existing
//! integration-test convention used by `component_meta_audit.rs`).
//!
//! Run: `cargo test -p verter_session --test correctness`
//!
//! Author-first workflow (§0p.A.0): for every Class A fixture the
//! programmatic expected value lives in
//! `tests/correctness/expected/<id>.rs` and is the single source of
//! truth. The committed `<id>.correctness.snap.json` is GENERATED
//! from that programmatic value via the `--ignored`
//! `generate_class_a_snapshots_from_expected` test. Class B + C
//! fixtures (regression baselines) are captured directly via
//! `UPDATE_SNAPSHOTS=1` on the main test (Phase 0a authors zero such
//! fixtures; Phase 0b adds them).
//!
//! UPDATE_SNAPSHOTS=1 regeneration is GUARDED — see §0p.A.4.

use std::path::PathBuf;

use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[path = "correctness/expected.rs"]
mod expected;
#[path = "correctness/fixtures.rs"]
mod fixtures;
#[path = "correctness/snapshot_view.rs"]
mod snapshot_view;
#[path = "correctness/deferred_fixtures_rule_correct.rs"]
mod deferred_fixtures_rule_correct;

/// Resolve the snapshot path for a fixture using the class-specific
/// suffix discipline (§0p.A.2). Class A → `<id>.correctness.snap.json`,
/// Class B + C → `<id>.regression.snap.json`.
fn snapshot_path_for(fixture: &fixtures::CorrectnessFixture) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/correctness/snapshots")
        .join(format!(
            "{}.{}.snap.json",
            fixture.id,
            fixture.class.suffix()
        ))
}

/// Build a hermetic [`VerterHost`] using the same `MemoryWorkspace`
/// pattern that the `component_meta_audit::harness` already uses
/// (§0.6.1 small decision: adopt the existing fixture helper).
/// `audit_enabled` is on because [`AuditedRequest::resolve`] requires
/// a published `RustAuditRecord` — the gate ignores the record and
/// only consumes the [`ComponentMetaAnalysis`] payload.
fn build_host(files: &[(&str, &str)]) -> std::sync::Arc<VerterHost> {
    let workspace = std::sync::Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (canonical, content) in files {
        workspace.inject_file((*canonical).into(), std::sync::Arc::from(*content));
    }
    let ws_access: std::sync::Arc<dyn WorkspaceAccess> = workspace;
    std::sync::Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

/// Resolve a fixture's target SFC and return its
/// [`ComponentMetaAnalysis`]. The audit machinery is left disabled so
/// the gate does not depend on audit-capture wiring (separate
/// concern from resolved-type correctness).
fn compute_for_fixture(fixture: &fixtures::CorrectnessFixture) -> ComponentMetaAnalysis {
    let host = build_host(fixture.files);
    let req = verter_session::audited_request::AuditedRequest::builder()
        .attach_to(host)
        .resolve(fixture.target);
    match req {
        Ok((analysis, _resolution, _record)) => analysis,
        Err(e) => panic!(
            "Phase 0 correctness fixture `{}` (`{}`) must resolve, got {e}",
            fixture.id, fixture.target,
        ),
    }
}

/// Phase 0 §0p.A.0 author-first workflow (Codex P1-4 r4 fix).
///
/// For Class A fixtures, the .snap.json is GENERATED from the
/// programmatic `expected::<fixture_id>()` constant — NOT captured
/// from Verter's output. This test runs under `--ignored` and
/// regenerates Class A snapshots from `expected.rs`.
///
/// Class B + C fixtures are regression-baseline only; their
/// `.snap.json` is captured from Verter's output via the standard
/// `UPDATE_SNAPSHOTS=1` mode on `correctness_snapshot_for_every_fixture`.
#[test]
#[ignore = "phase-0a/0b: regenerate Class A snapshots from expected.rs"]
fn generate_class_a_snapshots_from_expected() {
    for fixture in fixtures::FIXTURES {
        if !fixture.is_class_a() {
            continue;
        }
        let expected_view = expected::lookup_class_a_expected(fixture.id).unwrap_or_else(|| {
            panic!(
                "Class A fixture `{}` has no entry in expected.rs. \
                 Phase 0 §0p.A.0 author-first workflow REQUIRES a \
                 programmatic expected value before the .snap.json can \
                 be generated. Add `pub fn {}() -> SnapshotView` to \
                 expected.rs and a match arm in \
                 `lookup_class_a_expected`.",
                fixture.id, fixture.id,
            )
        });
        let json =
            serde_json::to_string_pretty(&expected_view).expect("Class A expected serializes");
        let path = snapshot_path_for(fixture);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &json).expect("write Class A snapshot");
    }
}

#[test]
fn correctness_snapshot_for_every_fixture() {
    ensure_class_a_derivation_notes();
    ensure_class_a_expected_matches_snapshot();

    for fixture in fixtures::FIXTURES {
        let analysis = compute_for_fixture(fixture);
        let view = snapshot_view::SnapshotView::from_analysis(&analysis);
        let actual = serde_json::to_string_pretty(&view).expect("serialize");
        let snapshot_path = snapshot_path_for(fixture);

        if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
            // Regeneration is allowed only when the active phase brief
            // declared EXPECTS_SNAPSHOT_REGEN (§0.6.4). For Class A,
            // regen MUST go through generate_class_a_snapshots_from_expected
            // — direct UPDATE_SNAPSHOTS=1 on Class A captures Verter's
            // output and bypasses the author-first workflow.
            if fixture.is_class_a() {
                panic!(
                    "Class A fixture `{}` cannot be regenerated via \
                     UPDATE_SNAPSHOTS=1 — that captures Verter's output, \
                     bypassing the author-first workflow (§0p.A.0). \
                     Run `cargo test --test correctness -- --ignored \
                     generate_class_a_snapshots_from_expected` instead, \
                     after updating `expected.rs`.",
                    fixture.id,
                );
            }
            std::fs::create_dir_all(snapshot_path.parent().unwrap()).unwrap();
            std::fs::write(&snapshot_path, &actual).expect("write snapshot");
            continue;
        }

        let expected = std::fs::read_to_string(&snapshot_path).unwrap_or_else(|e| {
            panic!(
                "snapshot missing for `{}` (id `{}`, class `{:?}`): {e} — \
                 Phase 0 §0p.A.2 must hand-author the expected value in \
                 expected.rs (Class A) or capture via UPDATE_SNAPSHOTS=1 \
                 (Class B+C).",
                fixture.target, fixture.id, fixture.class,
            )
        });
        assert_eq!(
            actual.trim(),
            expected.trim(),
            "Tier-1 correctness mismatch for fixture `{}`. The committed \
             .snap.json is hand-authored from TS spec + Verter rules \
             (§0p.A.0). If actual differs, that is a Verter resolver \
             defect — NOT a snapshot to regenerate. Investigate the diff. \
             If you genuinely intend to change resolved-type output, your \
             phase brief must carry EXPECTS_SNAPSHOT_REGEN (§0.6.4) and \
             the diff must be human-reviewed.",
            fixture.id,
        );
    }
}

/// Phase 0 §0p.A.0 author-first workflow gate (Codex P1-4 r4):
/// Class A's `<id>.correctness.snap.json` MUST byte-equal the JSON
/// serialization of `expected::lookup_class_a_expected(id)`. If they
/// disagree, the worker forgot to run
/// `--ignored generate_class_a_snapshots_from_expected` after editing
/// `expected.rs`. This catches the self-confirming-snapshot trap (F6).
fn ensure_class_a_expected_matches_snapshot() {
    for fixture in fixtures::FIXTURES {
        if !fixture.is_class_a() {
            continue;
        }
        let expected_view = expected::lookup_class_a_expected(fixture.id).unwrap_or_else(|| {
            panic!(
                "Class A fixture `{}` is missing from expected.rs. \
                 Author-first workflow (§0p.A.0) requires a programmatic \
                 expected value AND a derivation note before the \
                 .snap.json is meaningful.",
                fixture.id,
            )
        });
        let expected_json =
            serde_json::to_string_pretty(&expected_view).expect("Class A expected serializes");
        let snapshot = std::fs::read_to_string(snapshot_path_for(fixture)).unwrap_or_else(|e| {
            panic!(
                "Class A snapshot missing for `{}`: {e} — run `--ignored \
                 generate_class_a_snapshots_from_expected` to derive the \
                 .snap.json from expected.rs.",
                fixture.id,
            )
        });
        assert_eq!(
            expected_json.trim(),
            snapshot.trim(),
            "Class A expected.rs and .snap.json are out of sync for `{}`. \
             Run `cargo test --test correctness -- --ignored \
             generate_class_a_snapshots_from_expected` to regenerate the \
             .snap.json from the programmatic expected value.",
            fixture.id,
        );
    }
}

/// Phase 0 §0p.A.4 — every Class A fixture must carry a hand-written
/// derivation note whose first non-blank line cites a rule source
/// (TS spec §, Verter rule, CLAUDE.md §, or .claude/skills/...). The
/// citation regex in the plan body is implemented here as a plain
/// prefix matcher to avoid pulling in a `regex` workspace dependency
/// for a single test-only check (§0.6.1 small decision: match
/// surrounding workspace style).
fn ensure_class_a_derivation_notes() {
    let notes_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/correctness/derivation_notes");
    for fixture in fixtures::FIXTURES {
        if !fixture.is_class_a() {
            continue;
        }
        let path = notes_dir.join(format!("{}.md", fixture.id));
        let content = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "Class A fixture `{}` is missing its derivation note at \
                 {}. Phase 0 §0p.A.0 mandates a TS-spec or Verter-rule \
                 citation before the snapshot can be considered \
                 hand-derived. Without this file, the fixture is \
                 indistinguishable from a self-confirming snapshot of \
                 Verter's current output (the F6+N3 trap).",
                fixture.id,
                path.display(),
            )
        });
        let first_meaningful = content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        assert!(
            citation_line_is_well_formed(first_meaningful),
            "Class A fixture `{}` derivation note's first non-blank line \
             does not cite a rule source. Expected one of: \
             `TS spec §...`, `./.claude/skills/...`, `Verter rule ...`, \
             `CLAUDE.md §...`. Got: `{}`",
            fixture.id,
            first_meaningful,
        );
    }
}

/// Plain-string equivalent of the §0p.A.4 citation regex
/// `(?i)^TS spec\s+§|^[.]/[.]claude/skills/|^Verter rule\b|^CLAUDE[.]md\s+§`.
/// Matches the same set of leading anchors as the regex.
fn citation_line_is_well_formed(line: &str) -> bool {
    // Strip a leading `# ` or `## ` markdown heading prefix so that
    // notes formatted as "# TS spec §4.4 ..." still validate.
    let core = line.trim_start_matches('#').trim_start();
    // 1. `./.claude/skills/...`
    if core.starts_with("./.claude/skills/") {
        return true;
    }
    // 2. `Verter rule` followed by a non-word boundary.
    if let Some(rest) = core.strip_prefix("Verter rule") {
        if rest.is_empty() || !rest.chars().next().unwrap().is_alphanumeric() {
            return true;
        }
    }
    // 3. case-insensitive `TS spec` then whitespace then `§`.
    if check_keyword_then_section(core, "TS spec") {
        return true;
    }
    // 4. `CLAUDE.md` then whitespace then `§`.
    if check_keyword_then_section(core, "CLAUDE.md") {
        return true;
    }
    false
}

fn check_keyword_then_section(core: &str, keyword: &str) -> bool {
    let core_lower = core.to_ascii_lowercase();
    let keyword_lower = keyword.to_ascii_lowercase();
    if !core_lower.starts_with(&keyword_lower) {
        return false;
    }
    let mut rest = &core[keyword.len()..];
    let mut saw_ws = false;
    while let Some(ch) = rest.chars().next() {
        if ch.is_whitespace() {
            saw_ws = true;
            rest = &rest[ch.len_utf8()..];
        } else {
            break;
        }
    }
    saw_ws && rest.starts_with('§')
}

// ═══════════════════════════════════════════════════════════════════════════
// §0p.A.5 — Parametric discriminating self-test (12-property coverage).
// ═══════════════════════════════════════════════════════════════════════════
//
// Each row mutates ONE property of an actually-resolved SnapshotView
// and asserts the gate detects the mutation. Row coverage is the
// contract — adding a new SnapshotView field requires adding a row
// here, otherwise the gate is blind to that field.

#[derive(Debug, Clone, Copy)]
enum MutationKind {
    /// Filter mutation: prop set differs (Pick/Omit/Required-keyset).
    PropExtraKey,
    PropMissingKey,
    /// Defaults: flip default_signature from Some to None.
    PropDefaultDropped,
    /// Required: flip required: true → false.
    PropRequiredFlipped,
    /// Type-shape: change a prop's type_signature string.
    PropTypeChanged,
    /// Slots: drop a slot from a fixture with defineSlots.
    SlotDropped,
    /// Slots: change a slot's typed payload.
    SlotPayloadChanged,
    /// Events: change an event's parameter list.
    EventSignatureChanged,
    /// Models: drop a model from a fixture with defineModel.
    ModelDropped,
    /// Exposed / public instance: drop an exposed method.
    ExposedDropped,
    /// Fallthrough: flip inherit_attrs from true to false.
    FallthroughInheritFlipped,
    /// Fallthrough: change the inherited attr surface.
    FallthroughSurfaceChanged,
}

struct DiscriminatingCase {
    /// Class A fixture exercising the property.
    fixture_id: &'static str,
    /// Property under test.
    mutation: MutationKind,
    /// One-line documentation of the rule the test enforces.
    rule_doc: &'static str,
}

const DISCRIMINATING_CASES: &[DiscriminatingCase] = &[
    DiscriminatingCase {
        fixture_id: "mapped_pick_two_keys",
        mutation: MutationKind::PropExtraKey,
        rule_doc: "Pick<T,K> must filter to keys in K (TS spec §4.4)",
    },
    DiscriminatingCase {
        fixture_id: "mapped_omit_two_keys",
        mutation: MutationKind::PropMissingKey,
        rule_doc: "Omit<T,K> must exclude keys in K and keep the rest",
    },
    DiscriminatingCase {
        fixture_id: "fixture_props_with_defaults",
        mutation: MutationKind::PropDefaultDropped,
        rule_doc: "withDefaults must populate default_signature",
    },
    DiscriminatingCase {
        fixture_id: "mapped_required",
        mutation: MutationKind::PropRequiredFlipped,
        rule_doc: "Required<T> sets required: true on every prop",
    },
    DiscriminatingCase {
        fixture_id: "mapped_pick_two_keys",
        mutation: MutationKind::PropTypeChanged,
        rule_doc: "Pick<T,K> preserves the value type T[K]",
    },
    DiscriminatingCase {
        fixture_id: "fixture_slots_typed",
        mutation: MutationKind::SlotDropped,
        rule_doc: "defineSlots<T> must surface every key of T as a slot",
    },
    DiscriminatingCase {
        fixture_id: "fixture_slots_typed",
        mutation: MutationKind::SlotPayloadChanged,
        rule_doc: "Each slot's payload must reflect T[slot_name]",
    },
    DiscriminatingCase {
        fixture_id: "fixture_events_typed",
        mutation: MutationKind::EventSignatureChanged,
        rule_doc: "defineEmits<T> must preserve T's parameter shape",
    },
    DiscriminatingCase {
        fixture_id: "fixture_models",
        mutation: MutationKind::ModelDropped,
        rule_doc: "defineModel exposes a model entry per call",
    },
    DiscriminatingCase {
        fixture_id: "fixture_exposed_methods",
        mutation: MutationKind::ExposedDropped,
        rule_doc: "defineExpose<T> must surface every key of T",
    },
    DiscriminatingCase {
        fixture_id: "fixture_fallthrough_inherit",
        mutation: MutationKind::FallthroughInheritFlipped,
        rule_doc: "inheritAttrs: false must zero out the fallthrough surface",
    },
    DiscriminatingCase {
        fixture_id: "fixture_fallthrough_root_inherit",
        mutation: MutationKind::FallthroughSurfaceChanged,
        rule_doc: "Single component root propagates the child's accepted surface",
    },
];

#[test]
fn correctness_gate_is_discriminating_for_every_property() {
    for case in DISCRIMINATING_CASES {
        // r5/M6 fix: skip cases whose fixture isn't yet present.
        // Phase 0a authors mapped-type fixtures only; Phase 0b adds
        // the property fixtures. Without this skip, Phase 0a's
        // marker check would block on a panic from missing 0b
        // fixtures.
        let Some(fixture) = fixtures::FIXTURES.iter().find(|f| f.id == case.fixture_id) else {
            continue;
        };
        let analysis = compute_for_fixture(fixture);
        let view = snapshot_view::SnapshotView::from_analysis(&analysis);
        let mutated = apply_mutation(view.clone(), case.mutation).unwrap_or_else(|why| {
            panic!(
                "mutation {:?} could not be applied to fixture `{}`: {why}. \
                 The fixture must exercise the property the mutation \
                 targets — see §0p.A.2 fixture authorship.",
                case.mutation, case.fixture_id,
            )
        });
        let actual = serde_json::to_string_pretty(&view).expect("serialize");
        let mutated_json = serde_json::to_string_pretty(&mutated).expect("serialize");
        assert_ne!(
            actual.trim(),
            mutated_json.trim(),
            "discriminating self-test FAILED for case {:?} on fixture `{}`. \
             Rule: {}. The gate is INSENSITIVE to this property; the \
             projection in §0p.A.3 is missing the field that encodes it. \
             Phase 0 baseline cannot be trusted until this is fixed.",
            case.mutation,
            case.fixture_id,
            case.rule_doc,
        );
    }
}

/// Apply one mutation to a SnapshotView. Returns Err if the fixture
/// does not exercise the property under test (the mutation has
/// nothing to flip).
fn apply_mutation(
    mut view: snapshot_view::SnapshotView,
    mutation: MutationKind,
) -> Result<snapshot_view::SnapshotView, &'static str> {
    use MutationKind as M;
    match mutation {
        M::PropExtraKey => {
            view.props.push(snapshot_view::PropView {
                name: "__injected_extra__".into(),
                type_signature: "string".into(),
                required: true,
                has_default: false,
                default_signature: None,
                doc: None,
            });
        }
        M::PropMissingKey => {
            if view.props.is_empty() {
                return Err("no props to drop");
            }
            view.props.remove(0);
        }
        M::PropDefaultDropped => {
            let prop = view
                .props
                .iter_mut()
                .find(|p| p.default_signature.is_some())
                .ok_or("no prop with default to flip")?;
            prop.default_signature = None;
        }
        M::PropRequiredFlipped => {
            let prop = view
                .props
                .iter_mut()
                .find(|p| p.required)
                .ok_or("no required prop to flip")?;
            prop.required = false;
        }
        M::PropTypeChanged => {
            let prop = view.props.first_mut().ok_or("no props")?;
            prop.type_signature = format!("__mutated__{}", prop.type_signature);
        }
        M::SlotDropped => {
            if view.slots.is_empty() {
                return Err("no slots to drop");
            }
            view.slots.remove(0);
        }
        M::SlotPayloadChanged => {
            let slot = view.slots.first_mut().ok_or("no slots")?;
            slot.payload_signature = format!("__mutated__{}", slot.payload_signature);
        }
        M::EventSignatureChanged => {
            let event = view.events.first_mut().ok_or("no events")?;
            event.params_signature = format!("__mutated__{}", event.params_signature);
        }
        M::ModelDropped => {
            if view.models.is_empty() {
                return Err("no models to drop");
            }
            view.models.remove(0);
        }
        M::ExposedDropped => {
            if view.exposed.is_empty() {
                return Err("no exposed to drop");
            }
            view.exposed.remove(0);
        }
        M::FallthroughInheritFlipped => {
            let ft = view.fallthrough.as_mut().ok_or("no fallthrough")?;
            ft.inherit_attrs = !ft.inherit_attrs;
        }
        M::FallthroughSurfaceChanged => {
            let ft = view.fallthrough.as_mut().ok_or("no fallthrough")?;
            ft.surface_signature = format!("__mutated__{}", ft.surface_signature);
        }
    }
    Ok(view)
}

// ═══════════════════════════════════════════════════════════════════════════
// Self-tests for the citation matcher itself (sanity checks — these
// must FAIL on the pre-change tree, since the harness did not exist
// before this commit).
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn citation_matcher_accepts_ts_spec_section() {
    assert!(citation_line_is_well_formed("TS spec §4.4 — Mapped types"));
    assert!(citation_line_is_well_formed(
        "ts spec §4.6 — Conditional types (case-insensitive)"
    ));
    assert!(citation_line_is_well_formed(
        "# TS spec §4.5 — Indexed access"
    ));
}

#[test]
fn citation_matcher_accepts_claude_md_section() {
    assert!(citation_line_is_well_formed(
        "CLAUDE.md §Component-Meta authority chain"
    ));
}

#[test]
fn citation_matcher_accepts_skill_path_and_verter_rule() {
    assert!(citation_line_is_well_formed(
        "./.claude/skills/component-meta — fallthrough inheritance"
    ));
    assert!(citation_line_is_well_formed(
        "Verter rule: ts-first resolution"
    ));
}

#[test]
fn citation_matcher_rejects_unrelated_text() {
    // Negative: must NOT classify free-form text as a citation.
    assert!(!citation_line_is_well_formed("Just some prose"));
    assert!(!citation_line_is_well_formed(
        "# fixture mapped_pick_two_keys"
    ));
    assert!(!citation_line_is_well_formed("VolarEquivalent: Pick<T,K>"));
    assert!(!citation_line_is_well_formed("TSspec §4.4")); // missing whitespace
    assert!(!citation_line_is_well_formed(
        "CLAUDE.md without section sign"
    ));
}
