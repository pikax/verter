//! Tests for the project-bound VFS sync seam of the external-TypeScript-engine
//! path.
//!
//! These exercise the seam as a pure planning/gating layer — no live engine. Every
//! test is framework-agnostic (Vue AND Svelte) and uses an in-memory store /
//! explicit generation rails so they run without a provider binary.

use std::sync::Arc;

use verter_session::external_ts::{
    CarrierOwnershipResolution, EnvDims, ProjectBinding, QueryFeature, SnapshotRole,
};
use verter_session::file_artifact_store::ProjectIdentity;

use super::*;
use crate::carrier_cache::{EngineRecheckState, RegenKey};
use crate::provider_surface_store::{ProviderSurfaceKind, ProviderSurfaceStore, RecordSurface};

// ── shared fixtures ──────────────────────────────────────────────────────────

const TSCONFIG: &str = "/proj/tsconfig.json";

fn env_dims() -> EnvDims {
    EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: ProjectIdentity([4u8; 16]),
    }
}

fn binding() -> ProjectBinding {
    ProjectBinding::new_for_test(
        "/proj",
        TSCONFIG,
        "7.0.1",
        env_dims(),
        Vec::new(),
        verter_workspace::ProjectId(0),
        verter_workspace::SnapshotGeneration(1),
    )
}

fn regen_key() -> RegenKey {
    RegenKey {
        source_content_hash: [1u8; 16],
        parse_env_hash: [2u8; 16],
        compile_profile_hash: 7,
        file_language_row_hash: [3u8; 16],
        helper_runtime_version: 1,
    }
}

fn recheck() -> EngineRecheckState {
    EngineRecheckState {
        import_signature_hash: [9u8; 16],
        closure_generation: 5,
        project_recheck_generation: 1,
    }
}

/// A test [`SpanMapperView`] that maps an explicit allow-list of provider lines
/// back to source AS A WHOLE RANGE (a range maps iff BOTH endpoints' lines are in
/// the allow-list). No source text, no path matching — structural, like the real
/// `tsx_range_to_carrier`.
struct StubSpanMapper {
    mapped_lines: Vec<u32>,
}

impl StubSpanMapper {
    fn new() -> Self {
        Self {
            mapped_lines: Vec::new(),
        }
    }

    fn with_mapped(mut self, provider_line: u32) -> Self {
        self.mapped_lines.push(provider_line);
        self
    }
}

impl SpanMapperView for StubSpanMapper {
    fn provider_range_maps_to_source(
        &self,
        start_line: u32,
        _start_char: u32,
        end_line: u32,
        _end_char: u32,
    ) -> bool {
        // Maps as a whole only when BOTH endpoint lines are mapped — a range that
        // starts in a mapped region and ends in an unmapped (synthetic) region does
        // NOT map (the straddle case).
        self.mapped_lines.contains(&start_line) && self.mapped_lines.contains(&end_line)
    }
}

/// Record a published, project-owned surface directly into the store with the
/// full extended cache columns (the project-bound publish path's record shape).
fn record_owned(
    store: &ProviderSurfaceStore,
    provider_path: &str,
    kind: ProviderSurfaceKind,
    source_canonical: &str,
    content: &str,
) -> Arc<crate::provider_surface_store::ProviderSurfaceSnapshot> {
    store.record(RecordSurface {
        provider_path: provider_path.to_string(),
        kind,
        source_canonical: source_canonical.to_string(),
        provider_content: Arc::from(content),
        source_map: None,
        carrier_source: Arc::from("<source>"),
        map_hash: [0x42u8; 16],
        project_owner: Some(Arc::from(TSCONFIG)),
        regen_key: Some(regen_key()),
        engine_recheck: Some(recheck()),
    })
}

// ── Guard 1: generated_only_spans_suppressed (typed subject, range-based) ──────

/// A range whose provider position has NO user-source correlation (a synthetic
/// helper region inside a Verter carrier companion) classifies `GeneratedOnly` and
/// is SUPPRESSED — it never escapes to the user.
#[test]
fn generated_only_spans_suppressed() {
    let mapper = StubSpanMapper::new().with_mapped(10);

    // The helper region (line 0) → GeneratedOnly → suppressed.
    let helper = classify_provider_range(&mapper, SpanSubjectKind::Companion, 0, 0, 0, 5);
    assert_eq!(
        helper,
        SpanClass::GeneratedOnly,
        "a synthetic helper-region span inside a carrier companion must be GeneratedOnly"
    );
    assert!(
        helper.is_suppressed(),
        "a GeneratedOnly span is suppressed and never escapes to the user"
    );

    // The real region (line 10) → SourceMappable → escapes.
    let real = classify_provider_range(&mapper, SpanSubjectKind::Companion, 10, 0, 10, 8);
    assert_eq!(real, SpanClass::SourceMappable);
    assert!(!real.is_suppressed());
}

/// Framework parity: same suppression for a Svelte carrier companion subject.
#[test]
fn generated_only_spans_suppressed_svelte() {
    let mapper = StubSpanMapper::new().with_mapped(20);
    let helper = classify_provider_range(&mapper, SpanSubjectKind::Companion, 1, 0, 1, 3);
    assert_eq!(helper, SpanClass::GeneratedOnly);
    assert!(helper.is_suppressed());
}

/// RANGE STRADDLE (the reviewer's range-based case): a span whose START maps but
/// whose END crosses into a synthetic region must NOT classify as mappable — it is
/// GeneratedOnly (suppressed). A point-based classifier would wrongly pass the
/// start and leak generated content.
#[test]
fn range_straddling_generated_boundary_is_suppressed() {
    let mapper = StubSpanMapper::new().with_mapped(10); // line 10 mapped, line 11 NOT
    let straddle = classify_provider_range(&mapper, SpanSubjectKind::Companion, 10, 0, 11, 4);
    assert_eq!(
        straddle,
        SpanClass::GeneratedOnly,
        "a span starting in a mapped region but ending in a synthetic region must be \
         suppressed (range-based, not point-based) — generated content never escapes"
    );
    assert!(straddle.is_suppressed());
}

/// A span on a REAL on-disk `.ts` (typed `External` subject) is returned AS-IS —
/// never suppressed, regardless of whether the mapper maps it.
#[test]
fn external_real_ts_span_returned_as_is() {
    let mapper = StubSpanMapper::new(); // maps nothing
    let class = classify_provider_range(&mapper, SpanSubjectKind::External, 0, 0, 0, 3);
    assert_eq!(
        class,
        SpanClass::External,
        "a span on a real on-disk .ts is external and returned as-is"
    );
    assert!(!class.is_suppressed());
}

/// TYPED-OWNERSHIP WINS (the reviewer's path-heuristic-disagreement case): the
/// companion-vs-real decision is the typed [`SpanSubjectKind`], NOT a path suffix.
/// A path that "looks like" a real `.ts` but is typed `Companion` still suppresses
/// its synthetic region; a path that "looks like" a `.vue.tsx` carrier but is typed
/// `External` is never suppressed. A path-suffix classifier would get both
/// backwards.
#[test]
fn typed_subject_wins_over_path_shape() {
    let mapper = StubSpanMapper::new(); // maps nothing (everything is synthetic)

    let companion = classify_provider_range(&mapper, SpanSubjectKind::Companion, 0, 0, 0, 4);
    assert_eq!(
        companion,
        SpanClass::GeneratedOnly,
        "typed Companion suppresses its synthetic region regardless of path shape"
    );

    let external = classify_provider_range(&mapper, SpanSubjectKind::External, 0, 0, 0, 4);
    assert_eq!(
        external,
        SpanClass::External,
        "typed External is never suppressed regardless of path shape"
    );
}

/// The typed subject is derived from the contract role — the SINGLE structural
/// companion-vs-real authority.
#[test]
fn span_subject_kind_from_role_is_structural() {
    assert_eq!(
        SpanSubjectKind::from_role(SnapshotRole::CarrierIde),
        SpanSubjectKind::Companion
    );
    assert_eq!(
        SpanSubjectKind::from_role(SnapshotRole::CarrierApi),
        SpanSubjectKind::Companion
    );
    assert_eq!(
        SpanSubjectKind::from_role(SnapshotRole::Shadow),
        SpanSubjectKind::Companion
    );
    assert_eq!(
        SpanSubjectKind::from_role(SnapshotRole::Real),
        SpanSubjectKind::External
    );
}

/// The classifier rides the REAL [`ProviderPositionMapper`], not just a stub: a
/// `SelfFile` rune-module mapper's synthetic PRELUDE region (provider lines below
/// `prelude_line_count`) has no source correlate, so a span there classifies
/// `GeneratedOnly` (suppressed); a span in the user-source region maps back.
#[test]
fn classify_rides_real_provider_position_mapper() {
    use crate::documents::line_index::LineIndex;
    use crate::documents::provider_projection::{ProviderPositionMapper, SelfFileProviderMapper};

    let src = "export const x = 1;\nexport const y = 2;\n";
    let line_index = LineIndex::new_utf16(src);
    let self_file = SelfFileProviderMapper::new(/*prelude_line_count*/ 2, &[], &line_index);
    let mapper = ProviderPositionMapper::SelfFile(self_file);

    // A self-file rune module is a companion surface; a prelude-region span (line 0)
    // is GeneratedOnly (suppressed).
    let prelude = classify_provider_range(&mapper, SpanSubjectKind::Companion, 0, 0, 0, 6);
    assert_eq!(
        prelude,
        SpanClass::GeneratedOnly,
        "a real mapper's synthetic prelude region classifies GeneratedOnly (suppressed)"
    );
    assert!(prelude.is_suppressed());

    // A user-source-region span (line 2 = first user line) maps back.
    let user = classify_provider_range(&mapper, SpanSubjectKind::Companion, 2, 0, 2, 5);
    assert_eq!(
        user,
        SpanClass::SourceMappable,
        "a real mapper's user-source region maps back to the carrier source"
    );
}

/// A degenerate empty-range point delegates to range classification.
#[test]
fn classify_point_is_range_with_equal_endpoints() {
    let mapper = StubSpanMapper::new().with_mapped(5);
    assert_eq!(
        classify_provider_point(&mapper, SpanSubjectKind::Companion, 5, 0),
        SpanClass::SourceMappable
    );
    assert_eq!(
        classify_provider_point(&mapper, SpanSubjectKind::Companion, 0, 0),
        SpanClass::GeneratedOnly
    );
}

// ── Guard 2: stale_generation_result_dropped (project-bound, multi-file) ───────

/// A result whose touched provider file changed mid-flight (generation advanced
/// between before/after) is DROPPED.
#[test]
fn stale_generation_result_dropped() {
    let store = ProviderSurfaceStore::new();
    let companion = "/proj/src/App.vue.tsx";
    record_owned(
        &store,
        companion,
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/App.vue",
        "v1\n",
    );

    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record_owned(
        &store,
        companion,
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/App.vue",
        "v2\n",
    );
    let after = RequestEpoch::capture_project(&store, TSCONFIG);

    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a generation advance between before/after must drop the result"
    );
}

/// Multi-file awareness via the PROJECT-BOUND capture: a result spans many project
/// surfaces; if ANY ONE of them changes mid-flight (not just the queried file), the
/// project-bound before/after disagree and the result is dropped.
#[test]
fn stale_generation_result_dropped_multi_file() {
    let store = ProviderSurfaceStore::new();
    let a = "/proj/src/A.vue.tsx";
    let b = "/proj/src/B.vue.verter.ts";
    let c = "/proj/src/C.svelte.tsx";
    record_owned(
        &store,
        a,
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    record_owned(
        &store,
        b,
        ProviderSurfaceKind::CarrierApi,
        "/proj/src/B.vue",
        "b\n",
    );
    record_owned(
        &store,
        c,
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/C.svelte",
        "c\n",
    );

    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    assert_eq!(
        before.captured_len(),
        3,
        "project capture covers every owned surface"
    );

    // Only C (not the queried A) changes mid-flight.
    record_owned(
        &store,
        c,
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/C.svelte",
        "c2\n",
    );

    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a multi-file result must drop when ANY project surface changed mid-flight, \
         not only the queried file"
    );
}

/// A surface that APPEARS mid-flight (a new project member added between
/// before/after) is detected by the project-bound capture (set size grows) and
/// drops the result.
#[test]
fn surface_appearing_mid_flight_drops_result() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record_owned(
        &store,
        "/proj/src/New.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/New.vue",
        "n\n",
    );
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a project surface appearing mid-flight must drop the result (set membership change)"
    );
}

/// Order independence: the same surface set captured in any order is fresh.
#[test]
fn project_capture_is_order_independent() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    record_owned(
        &store,
        "/proj/src/B.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/B.vue",
        "b\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        RequestEpoch::result_is_fresh(&before, &after),
        "an unchanged project surface set is fresh regardless of capture order"
    );
}

/// `map_hash` change mid-flight (same content bytes) drops the result.
#[test]
fn stale_map_hash_drops_result() {
    let store = ProviderSurfaceStore::new();
    let companion = "/proj/src/App.vue.tsx";
    let rec = |map_hash: [u8; 16]| {
        store.record(RecordSurface {
            provider_path: companion.to_string(),
            kind: ProviderSurfaceKind::CarrierIde,
            source_canonical: "/proj/src/App.vue".to_string(),
            provider_content: Arc::from("same bytes\n"),
            source_map: None,
            carrier_source: Arc::from("<source>"),
            map_hash,
            project_owner: Some(Arc::from(TSCONFIG)),
            regen_key: Some(regen_key()),
            engine_recheck: Some(recheck()),
        });
    };
    rec([0x11u8; 16]);
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    rec([0x22u8; 16]);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a map_hash change mid-flight must drop the result"
    );
}

/// The happy path: an unchanged project surface set keeps the result.
#[test]
fn unchanged_epoch_keeps_result() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/App.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/App.vue",
        "v1\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(RequestEpoch::result_is_fresh(&before, &after));
}

/// A project surface CLOSED mid-flight drops the result (the closed surface leaves
/// the project set).
#[test]
fn closed_surface_mid_flight_drops_result() {
    let store = ProviderSurfaceStore::new();
    let companion = "/proj/src/App.vue.tsx";
    record_owned(
        &store,
        companion,
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/App.vue",
        "v1\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    let _token = store.forget(companion);
    let after = RequestEpoch::capture_project(&store, TSCONFIG);
    assert!(
        !RequestEpoch::result_is_fresh(&before, &after),
        "a project surface closed mid-flight must drop the result"
    );
}

// ── Returned-path validation (discovered-from-engine-response touched set) ─────

/// A returned COMPANION path that was NOT in the before-capture (a surface that
/// appeared mid-flight) FAILS the returned-path validation — the STORE is the
/// companion-vs-external authority (no caller heuristic).
#[test]
fn returned_companion_path_absent_from_before_fails_closed() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);

    // The engine returns a span in B — a companion that was NOT captured before.
    record_owned(
        &store,
        "/proj/src/B.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/B.vue",
        "b\n",
    );
    let returned: Vec<Arc<str>> = vec![
        Arc::from("/proj/src/A.vue.tsx"),
        Arc::from("/proj/src/B.vue.tsx"),
    ];

    assert!(
        !before.returned_paths_all_fresh(&store, &returned),
        "a returned companion path absent from the before-capture must fail closed \
         (a surface that appeared mid-flight)"
    );
}

/// A returned `CarrierApi` (`.vue.verter.ts`) companion absent from the
/// before-capture FAILS closed — the store authority classifies it as a companion
/// even though a `.vue.tsx`/`.svelte.tsx` suffix check would misclassify it as
/// external (the reviewer's fail-open case).
#[test]
fn returned_carrier_api_companion_absent_from_before_fails_closed() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);

    // A CarrierApi (.vue.verter.ts) surface appears mid-flight — a companion the
    // store knows, NOT an external file.
    record_owned(
        &store,
        "/proj/src/B.vue.verter.ts",
        ProviderSurfaceKind::CarrierApi,
        "/proj/src/B.vue",
        "b\n",
    );
    let returned: Vec<Arc<str>> = vec![
        Arc::from("/proj/src/A.vue.tsx"),
        Arc::from("/proj/src/B.vue.verter.ts"),
    ];

    assert!(
        !before.returned_paths_all_fresh(&store, &returned),
        "a returned .vue.verter.ts CarrierApi companion absent from the before-capture must \
         fail closed (the store — not a suffix heuristic — is the companion authority)"
    );
}

/// A returned genuinely-EXTERNAL path the store NEVER synced is fresh (a real
/// on-disk file the engine resolved on its own — no project epoch).
#[test]
fn returned_external_path_absent_from_before_is_fresh() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    // util.ts is NOT synced into the store (a genuine on-disk file).
    let returned: Vec<Arc<str>> = vec![
        Arc::from("/proj/src/A.vue.tsx"),
        Arc::from("/proj/src/util.ts"),
    ];
    assert!(
        before.returned_paths_all_fresh(&store, &returned),
        "a returned external .ts the store never synced is fresh (no project epoch)"
    );
}

/// A returned captured companion whose epoch ADVANCED fails the returned-path
/// validation.
#[test]
fn returned_captured_path_changed_fails_closed() {
    let store = ProviderSurfaceStore::new();
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a\n",
    );
    let before = RequestEpoch::capture_project(&store, TSCONFIG);
    record_owned(
        &store,
        "/proj/src/A.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/A.vue",
        "a2\n",
    );
    let returned: Vec<Arc<str>> = vec![Arc::from("/proj/src/A.vue.tsx")];
    assert!(
        !before.returned_paths_all_fresh(&store, &returned),
        "a returned captured companion whose epoch advanced must fail closed"
    );
}

// ── Guard 3: closed_carrier_in_autoimport_index ───────────────────────────────

/// A CLOSED `.vue`/`.svelte` component's public-API surface is in the eager index.
#[test]
fn closed_carrier_in_autoimport_index() {
    let owned_sources = [
        Arc::<str>::from("/proj/src/Button.vue"),
        Arc::<str>::from("/proj/src/Card.svelte"),
    ];
    let plan = EagerApiIndexPlan::for_owned_sources(
        TSCONFIG,
        owned_sources.iter().map(Arc::clone),
        |source: &str| Some(Arc::<str>::from(format!("{source}.verter.ts").as_str())),
    );

    assert_eq!(plan.api_companions().len(), 2);
    assert!(plan.contains_api_for("/proj/src/Button.vue"));
    assert!(plan.contains_api_for("/proj/src/Card.svelte"));
}

/// The eager index force-materializes ONLY `CarrierApi` — `CarrierIde` stays lazy.
/// Count asserted BEFORE the `all` so the role assertion is not vacuous on empty.
#[test]
fn eager_index_force_materializes_only_carrier_api_not_ide() {
    let owned_sources = [Arc::<str>::from("/proj/src/Button.vue")];
    let plan = EagerApiIndexPlan::for_owned_sources(
        TSCONFIG,
        owned_sources.iter().map(Arc::clone),
        |source: &str| Some(Arc::<str>::from(format!("{source}.verter.ts").as_str())),
    );
    assert_eq!(
        plan.api_companions().len(),
        1,
        "non-vacuous: exactly one companion"
    );
    assert!(
        plan.api_companions()
            .iter()
            .all(|c| c.role == SnapshotRole::CarrierApi),
        "the eager index force-materializes ONLY the CarrierApi surface; CarrierIde stays lazy"
    );
}

/// A source for which no API companion path can be derived is skipped (fail closed).
#[test]
fn eager_index_skips_source_with_no_api_companion() {
    let owned_sources = [
        Arc::<str>::from("/proj/src/Button.vue"),
        Arc::<str>::from("/proj/src/NoCompanion.vue"),
    ];
    let plan = EagerApiIndexPlan::for_owned_sources(
        TSCONFIG,
        owned_sources.iter().map(Arc::clone),
        |source: &str| {
            if source == "/proj/src/NoCompanion.vue" {
                None
            } else {
                Some(Arc::<str>::from(format!("{source}.verter.ts").as_str()))
            }
        },
    );
    assert_eq!(plan.api_companions().len(), 1);
    assert!(!plan.contains_api_for("/proj/src/NoCompanion.vue"));
}

// ── project-bound sync planner (§2.5) — binding-gated ──────────────────────────

/// A batch is built from a resolved [`ProjectBinding`] (binding-gated): the owning
/// project URI is taken from the binding, and the published snapshot carries it.
#[test]
fn planner_builds_per_project_atomic_batch_from_binding() {
    let store = ProviderSurfaceStore::new();
    let snap = record_owned(
        &store,
        "/proj/src/App.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "/proj/src/App.vue",
        "export default {}\n",
    );
    let b = binding();
    let planned = PlannedFile::from_snapshot(&b, &snap).expect("snapshot owned by the binding");

    let batch =
        ProjectSyncBatch::for_binding(&b, vec![planned], /*res_map*/ 42, /*fs*/ 100);
    let snapshot = batch.into_publish_snapshot();
    assert_eq!(&*snapshot.project, TSCONFIG);
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.resolution_map_version, 42);
    assert_eq!(snapshot.fs_generation, 100);
    assert_eq!(&*snapshot.files[0].provider_uri, "/proj/src/App.vue.tsx");
    assert_eq!(snapshot.files[0].role, SnapshotRole::CarrierIde);
    assert_eq!(
        snapshot.files[0].script_kind,
        verter_session::external_ts::ScriptKind::Tsx
    );
}

/// `PlannedFile::from_snapshot` FAILS CLOSED when the snapshot belongs to a
/// DIFFERENT project than the binding (its project_owner mismatches).
#[test]
fn planned_file_from_foreign_project_snapshot_fails_closed() {
    let store = ProviderSurfaceStore::new();
    let snap = store.record(RecordSurface {
        provider_path: "/other/src/X.vue.tsx".to_string(),
        kind: ProviderSurfaceKind::CarrierIde,
        source_canonical: "/other/src/X.vue".to_string(),
        provider_content: Arc::from("x\n"),
        source_map: None,
        carrier_source: Arc::from("<source>"),
        map_hash: [0x42u8; 16],
        project_owner: Some(Arc::from("/other/tsconfig.json")),
        regen_key: Some(regen_key()),
        engine_recheck: Some(recheck()),
    });
    let b = binding(); // owns /proj/tsconfig.json
    assert!(
        PlannedFile::from_snapshot(&b, &snap).is_none(),
        "a snapshot owned by a different project must not be planned into this binding's batch"
    );
}

/// `PlannedFile::from_snapshot` fails closed on a project-owner-less (legacy) record.
#[test]
fn planned_file_from_ownerless_snapshot_fails_closed() {
    let store = ProviderSurfaceStore::new();
    let snap = store.record(RecordSurface::carrier_api_legacy(
        "/proj/src/Legacy.vue.verter.ts".to_string(),
        "/proj/src/Legacy.vue".to_string(),
        Arc::from("legacy\n"),
        None,
        Arc::from("<source>"),
    ));
    let b = binding();
    assert!(
        PlannedFile::from_snapshot(&b, &snap).is_none(),
        "a project-owner-less legacy record must not leak into a project-bound batch"
    );
}

/// No-owner ⇒ no binding ⇒ no batch.
#[test]
fn planner_no_owner_yields_no_binding() {
    assert!(plan_publish_for_resolution(&CarrierOwnershipResolution::NoProject).is_none());
    assert!(
        plan_publish_for_resolution(&CarrierOwnershipResolution::Ambiguous {
            candidates: Vec::new(),
            cause: verter_session::external_ts::AmbiguityCause::MultipleOwners,
        })
        .is_none()
    );
    assert!(plan_publish_for_resolution(&CarrierOwnershipResolution::NotReady).is_none());
}

/// A resolved binding yields itself.
#[test]
fn planner_project_binding_yields_binding() {
    let resolution = CarrierOwnershipResolution::Bound(binding());
    let got =
        plan_publish_for_resolution(&resolution).expect("a ProjectBinding yields its binding");
    assert_eq!(got.tsconfig_uri(), TSCONFIG);
}

// ── query de-dupe / cancellation (§2.7) ───────────────────────────────────────

fn key(offset: u32, feature: QueryFeature) -> QueryDedupeKey {
    QueryDedupeKey {
        project: Arc::from(TSCONFIG),
        provider_uri: Arc::from("/proj/src/App.vue.tsx"),
        carrier_offset: offset,
        feature,
        content_hash: [1u8; 16],
        map_hash: [2u8; 16],
        required_version: 1,
        feature_param: [0u8; 16],
    }
}

/// Two concurrent identical queries JOIN one in-flight slot.
#[test]
fn identical_queries_join_one_inflight_slot() {
    let registry = QueryDedupeRegistry::new();
    let first = registry.admit(key(120, QueryFeature::Hover));
    assert!(
        first.is_leader(),
        "the first identical query leads the slot"
    );
    let second = registry.admit(key(120, QueryFeature::Hover));
    assert!(
        !second.is_leader(),
        "an identical concurrent query joins, does not duplicate"
    );
    assert!(
        second.shares_cancellation_with(&first),
        "joiners share the leader's cancellation slot"
    );
}

/// A DIFFERENT offset leads its own slot.
#[test]
fn different_offset_leads_its_own_slot() {
    let registry = QueryDedupeRegistry::new();
    let _a = registry.admit(key(120, QueryFeature::Hover));
    let b = registry.admit(key(999, QueryFeature::Hover));
    assert!(b.is_leader());
}

/// A query against a NEWER snapshot version does NOT join a pre-edit slot — the
/// `required_version` dimension distinguishes them (the version-gate fix).
#[test]
fn newer_required_version_does_not_join_stale_slot() {
    let registry = QueryDedupeRegistry::new();
    let mut k_old = key(120, QueryFeature::Hover);
    k_old.required_version = 1;
    let mut k_new = k_old.clone();
    k_new.required_version = 2; // same content_hash + map_hash, newer version

    let leader = registry.admit(k_old);
    assert!(leader.is_leader());
    let newer = registry.admit(k_new);
    assert!(
        newer.is_leader(),
        "a query against a newer snapshot version must NOT join the stale in-flight slot \
         (required_version is part of the dedupe identity)"
    );
}

/// Two renames at the SAME offset with DIFFERENT replacement text are DIFFERENT
/// work and must not join — the `feature_param` dimension distinguishes them.
#[test]
fn rename_with_different_param_does_not_join() {
    let registry = QueryDedupeRegistry::new();
    let mut k1 = key(120, QueryFeature::Rename);
    k1.feature_param = [0xAAu8; 16];
    let mut k2 = key(120, QueryFeature::Rename);
    k2.feature_param = [0xBBu8; 16];

    let r1 = registry.admit(k1);
    assert!(r1.is_leader());
    let r2 = registry.admit(k2);
    assert!(
        r2.is_leader(),
        "a rename with a different replacement (feature_param) must not join — different work"
    );
}

/// Completing (dropping) the leader retires the slot.
#[test]
fn completing_leader_retires_slot() {
    let registry = QueryDedupeRegistry::new();
    let k = key(120, QueryFeature::Completion);
    {
        let leader = registry.admit(k.clone());
        assert!(leader.is_leader());
    }
    let next = registry.admit(k);
    assert!(
        next.is_leader(),
        "after the leader completes, a later identical query leads fresh"
    );
}

/// A supersession cancels the in-flight engine work via the shared token.
#[test]
fn supersession_cancels_inflight_via_token() {
    let registry = QueryDedupeRegistry::new();
    let k = key(7, QueryFeature::Definition);
    let leader = registry.admit(k.clone());
    let token = leader.cancellation_token();
    assert!(!token.is_cancelled());
    registry.cancel(&k);
    assert!(
        token.is_cancelled(),
        "a supersession cancels the in-flight work via the shared token"
    );
}

/// After a `cancel` retires a slot, a re-admit installs a FRESH slot; the stale
/// leader's drop must NOT evict the fresh slot (the slot-id guard).
#[test]
fn stale_leader_drop_does_not_evict_fresh_slot() {
    let registry = QueryDedupeRegistry::new();
    let k = key(7, QueryFeature::Hover);
    let stale_leader = registry.admit(k.clone());
    registry.cancel(&k); // retires the slot
    let fresh = registry.admit(k.clone());
    assert!(fresh.is_leader());
    drop(stale_leader); // stale leader's slot id no longer matches the live slot
    let joiner = registry.admit(k);
    assert!(
        !joiner.is_leader(),
        "the fresh slot survived the stale leader's drop (slot-id guard) — a re-admit joins it"
    );
}
