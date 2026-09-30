//! A request overlay resolves for its own effective view: an answer it
//! changes lives in the overlay lane and never serves the workspace, and a
//! workspace answer is reused under the overlay exactly when the overlay
//! cannot reach anything that answer observed.

use std::sync::Arc;

use verter_semantic::resolver_core::{ResolutionContext, ResolvePhase, ResolveRequestKind};

use crate::memory::MemoryWorkspace;
use crate::resolution_currency::{ResolutionOutcome, ResolutionOverlaySnapshot};
use crate::traits::WorkspaceRead;

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::CodegenBlocker,
    kind: ResolveRequestKind::TypeImport,
};

const MAIN: &str = "/p/main.ts";

fn workspace(files: &[(&str, &str)]) -> MemoryWorkspace {
    let workspace = MemoryWorkspace::new(Default::default());
    workspace.inject_file(MAIN.to_string(), Arc::from("export {}\n"));
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
    workspace
}

fn overlay(upserts: &[(&str, &str)], tombstones: &[&str]) -> ResolutionOverlaySnapshot {
    ResolutionOverlaySnapshot::new(
        upserts
            .iter()
            .map(|(path, source)| ((*path).to_string(), Arc::<str>::from(*source))),
        tombstones.iter().map(|path| (*path).to_string()),
    )
}

fn target(outcome: &ResolutionOutcome) -> Option<&str> {
    outcome.result().map(|result| result.source_id.as_str())
}

fn through(
    workspace: &MemoryWorkspace,
    overlay: &ResolutionOverlaySnapshot,
    specifier: &str,
) -> ResolutionOutcome {
    workspace.resolve_import_outcome_with_overlay(overlay, MAIN, specifier, CONTEXT)
}

fn producer_runs(workspace: &MemoryWorkspace) -> u64 {
    workspace
        .vfs_provenance_snapshot()
        .import_resolution_cache_miss_count
}

#[test]
fn an_overlay_answer_lives_in_its_own_lane_and_never_serves_the_workspace() {
    let workspace = workspace(&[]);
    let creates_dep = overlay(&[("/p/dep.ts", "export const dep = 1\n")], &[]);

    let cold = through(&workspace, &creates_dep, "./dep");
    assert_eq!(target(&cold), Some("/p/dep.ts"));
    assert!(cold.trace().published(), "the overlay answer is admitted");
    assert_eq!(
        workspace.resource_snapshot().overlay_resolution_slots,
        1,
        "into the overlay lane"
    );

    let base = workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT);
    assert_eq!(target(&base), None, "the workspace has no dep");
    assert!(
        !base.trace().reused(),
        "and never adopts the overlay answer"
    );

    let warm = through(&workspace, &creates_dep, "./dep");
    assert_eq!(target(&warm), Some("/p/dep.ts"));
    assert!(warm.trace().reused(), "the overlay answer is warm");
}

#[test]
fn a_workspace_answer_the_overlay_cannot_reach_is_reused_under_it() {
    let workspace = workspace(&[("/p/a.ts", "export const a = 1\n")]);
    let base = workspace.resolve_import_outcome(MAIN, "./a", CONTEXT);
    assert_eq!(target(&base), Some("/p/a.ts"));
    let runs = producer_runs(&workspace);

    let unrelated = overlay(&[("/q/scratch.ts", "export {}\n")], &[]);
    let under = through(&workspace, &unrelated, "./a");
    assert_eq!(target(&under), Some("/p/a.ts"));
    assert!(under.trace().reused(), "the workspace candidate is reused");
    assert_eq!(
        producer_runs(&workspace),
        runs,
        "without running the resolver"
    );
    assert_eq!(
        workspace.resource_snapshot().overlay_resolution_slots,
        0,
        "and nothing enters the overlay lane"
    );
}

#[test]
fn a_workspace_answer_the_overlay_reaches_is_refused_under_it() {
    let workspace = workspace(&[("/p/helper.d.ts", "export interface P {}\n")]);
    let base = workspace.resolve_import_outcome(MAIN, "./helper", CONTEXT);
    assert_eq!(target(&base), Some("/p/helper.d.ts"));

    // A higher-priority candidate appears only in the overlay: the
    // workspace answer's negative probe for it no longer holds.
    let shadows = overlay(&[("/p/helper.ts", "export interface P {}\n")], &[]);
    let under = through(&workspace, &shadows, "./helper");
    assert_eq!(target(&under), Some("/p/helper.ts"));
    assert!(!under.trace().reused());

    let again = workspace.resolve_import_outcome(MAIN, "./helper", CONTEXT);
    assert_eq!(target(&again), Some("/p/helper.d.ts"));
    assert!(again.trace().reused(), "the workspace answer is untouched");
}

#[test]
fn a_tombstone_miss_never_serves_the_workspace() {
    let workspace = workspace(&[("/p/dep.ts", "export const dep = 1\n")]);
    let deletes_dep = overlay(&[], &["/p/dep.ts"]);

    assert_eq!(target(&through(&workspace, &deletes_dep, "./dep")), None);
    assert_eq!(
        target(&workspace.resolve_import_outcome(MAIN, "./dep", CONTEXT)),
        Some("/p/dep.ts")
    );
    let warm = through(&workspace, &deletes_dep, "./dep");
    assert_eq!(target(&warm), None);
    assert!(
        warm.trace().reused(),
        "the overlay miss is warm in its lane"
    );
}

#[test]
fn an_overlay_answer_that_depends_only_on_existence_survives_body_edits() {
    let workspace = workspace(&[]);
    let first = overlay(&[("/p/dep.ts", "export const dep = 1\n")], &[]);
    assert_eq!(
        target(&through(&workspace, &first, "./dep")),
        Some("/p/dep.ts")
    );
    let runs = producer_runs(&workspace);

    // Another request, another snapshot, the same file with another body.
    let edited = overlay(&[("/p/dep.ts", "export const dep = 2\n")], &[]);
    let warm = through(&workspace, &edited, "./dep");
    assert_eq!(target(&warm), Some("/p/dep.ts"));
    assert!(warm.trace().reused());
    assert_eq!(
        producer_runs(&workspace),
        runs,
        "a body edit runs no resolver"
    );
    assert_eq!(edited.work_counts().probes, 0, "nor probes a path");
}

#[test]
fn a_workspace_change_under_an_overlay_answer_refuses_it() {
    let workspace = workspace(&[("/p/dep.d.ts", "export const dep: 1\n")]);
    let unrelated = overlay(&[("/q/scratch.ts", "export {}\n")], &[]);
    assert_eq!(
        target(&through(&workspace, &unrelated, "./dep")),
        Some("/p/dep.d.ts")
    );

    // The workspace itself gains a higher-priority candidate.
    workspace.inject_file("/p/dep.ts".to_string(), Arc::from("export const dep = 1\n"));
    assert_eq!(
        target(&through(&workspace, &unrelated, "./dep")),
        Some("/p/dep.ts"),
        "an overlay view sees every workspace change it does not shadow"
    );
}

#[test]
fn one_snapshot_composes_its_effective_root_once_per_underlying_world() {
    let workspace = workspace(&[("/p/a.ts", "export const a = 1\n")]);
    let creates = overlay(&[("/p/dep.ts", "export const dep = 1\n")], &[]);
    for _ in 0..8 {
        let _ = through(&workspace, &creates, "./dep");
        let _ = through(&workspace, &creates, "./a");
    }
    assert_eq!(creates.work_counts().root_builds, 1);
}

/// An overlay over a symlinked path changes the path's effective realpath
/// (an overlay file is its own realpath) and nothing about its existence;
/// an overlay over a path that is already its own realpath changes nothing,
/// and composes to the captured world itself.
#[test]
fn an_overlay_changes_a_realpath_only_when_the_effective_value_moves() {
    use crate::resolution_currency::{
        CanonicalResolutionId, CapturedResolutionWorld, ResolutionFactKey, ResolutionWorldRoot,
    };
    use verter_semantic::resolver_core::{PathProbe, ResolutionPopulation, ResolutionWorldId};

    let world_with_realpath = |realpath: &str| {
        let mut root = ResolutionWorldRoot::bootstrap(ResolutionWorldId::from_raw(1));
        root.path_probes
            .insert("/p/link.ts".to_string(), PathProbe::File);
        root.realpaths
            .insert("/p/link.ts".to_string(), Some(realpath.to_string()));
        Arc::new(CapturedResolutionWorld {
            base: Arc::new(root),
            session: None,
            population: ResolutionPopulation::Base,
            overlay: None,
            overlay_values: None,
        })
    };
    let probe = ResolutionFactKey::PathProbe {
        canonical: CanonicalResolutionId::new("/p/link.ts"),
        population: ResolutionPopulation::Base,
    };
    let realpath = ResolutionFactKey::Realpath {
        requested: CanonicalResolutionId::new("/p/link.ts"),
        population: ResolutionPopulation::Base,
    };
    let upsert = overlay(&[("/p/link.ts", "export {}\n")], &[]);

    let symlinked = world_with_realpath("/real/link.ts");
    let effective = upsert.effective_world(&symlinked);
    assert_eq!(
        effective.fact_version(&probe),
        symlinked.fact_version(&probe),
        "the path exists either way"
    );
    assert_ne!(
        effective.fact_version(&realpath),
        symlinked.fact_version(&realpath),
        "the overlay file is its own realpath"
    );

    let direct = world_with_realpath("/p/link.ts");
    let unchanged = overlay(&[("/p/link.ts", "export {}\n")], &[]).effective_world(&direct);
    assert!(
        Arc::ptr_eq(&unchanged, &direct),
        "an overlay that moves no effective value is the captured world"
    );
}

// ---------------------------------------------------------------------------
// Residency: overlay answers and overlay value versions live while an
// overlay authority holds them, and are charged while they live.
// ---------------------------------------------------------------------------

fn lane_and_values(workspace: &MemoryWorkspace) -> (usize, usize) {
    let resources = workspace.resource_snapshot();
    (
        resources.overlay_resolution_slots,
        resources.overlay_value_versions,
    )
}

#[test]
fn a_request_overlays_answers_go_when_its_snapshot_is_superseded() {
    let workspace = workspace(&[]);
    {
        let creates_dep = overlay(&[("/p/dep.ts", "export const dep = 1\n")], &[]);
        assert_eq!(
            target(&through(&workspace, &creates_dep, "./dep")),
            Some("/p/dep.ts")
        );
        assert_eq!(
            lane_and_values(&workspace),
            (1, 1),
            "one overlay answer, one interned directory-membership value"
        );
    }
    assert_eq!(
        lane_and_values(&workspace),
        (0, 0),
        "the snapshot's authority released both with the snapshot"
    );
}

#[test]
fn a_session_authority_keeps_its_answers_across_requests_until_it_closes() {
    let workspace = workspace(&[]);
    let session = crate::overlay_residency::OverlayAuthority::new();
    let request = |authority: &crate::overlay_residency::OverlayAuthority| {
        ResolutionOverlaySnapshot::new_under(
            Some(authority.clone()),
            [(
                "/p/dep.ts".to_string(),
                Arc::<str>::from("export const dep = 1\n"),
            )],
            [],
        )
    };

    let first = request(&session);
    assert!(!through(&workspace, &first, "./dep").trace().reused());
    drop(first);
    assert_eq!(
        lane_and_values(&workspace),
        (1, 1),
        "the session still holds its answer after its request ends"
    );

    let second = request(&session);
    assert!(
        through(&workspace, &second, "./dep").trace().reused(),
        "the next request of the session reuses it"
    );
    drop(second);
    drop(session);
    assert_eq!(
        lane_and_values(&workspace),
        (0, 0),
        "closing the session releases it"
    );
}

#[test]
fn an_answer_another_authority_reused_outlives_the_one_that_produced_it() {
    let workspace = workspace(&[]);
    let producing = crate::overlay_residency::OverlayAuthority::new();
    let reusing = crate::overlay_residency::OverlayAuthority::new();
    let request = |authority: &crate::overlay_residency::OverlayAuthority| {
        ResolutionOverlaySnapshot::new_under(
            Some(authority.clone()),
            [(
                "/p/dep.ts".to_string(),
                Arc::<str>::from("export const dep = 1\n"),
            )],
            [],
        )
    };
    let _ = through(&workspace, &request(&producing), "./dep");
    assert!(through(&workspace, &request(&reusing), "./dep")
        .trace()
        .reused());
    drop(producing);
    assert_eq!(
        lane_and_values(&workspace),
        (1, 1),
        "the reusing authority still holds the answer and its value"
    );
    drop(reusing);
    assert_eq!(lane_and_values(&workspace), (0, 0));
}

/// A retention account that admits or refuses every reservation and
/// tracks the bytes its live charges hold.
struct CountingAccount {
    admit: bool,
    charged: Arc<std::sync::atomic::AtomicUsize>,
}

struct CountingCharge {
    bytes: usize,
    charged: Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for CountingCharge {
    fn drop(&mut self) {
        self.charged
            .fetch_sub(self.bytes, std::sync::atomic::Ordering::SeqCst);
    }
}

impl crate::overlay_residency::ResolutionRetentionAccount for CountingAccount {
    fn reserve_retained(
        &self,
        bytes: usize,
    ) -> Option<crate::overlay_residency::ResolutionRetentionCharge> {
        if !self.admit {
            return None;
        }
        self.charged
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
        Some(crate::overlay_residency::ResolutionRetentionCharge::new(
            CountingCharge {
                bytes,
                charged: Arc::clone(&self.charged),
            },
        ))
    }
}

#[test]
fn resident_overlay_state_is_charged_while_it_lives() {
    use crate::traits::WorkspaceAccess;
    let workspace = workspace(&[]);
    let charged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    workspace.install_resolution_retention(Arc::new(CountingAccount {
        admit: true,
        charged: Arc::clone(&charged),
    }));
    {
        let creates_dep = overlay(&[("/p/dep.ts", "export const dep = 1\n")], &[]);
        let _ = through(&workspace, &creates_dep, "./dep");
        assert!(
            charged.load(std::sync::atomic::Ordering::SeqCst) > 0,
            "the overlay answer and its value are charged"
        );
    }
    assert_eq!(
        charged.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "and released with them"
    );
}

#[test]
fn a_refused_charge_serves_the_overlay_answer_uncached() {
    use crate::traits::WorkspaceAccess;
    let workspace = workspace(&[]);
    workspace.install_resolution_retention(Arc::new(CountingAccount {
        admit: false,
        charged: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));
    let creates_dep = overlay(&[("/p/dep.ts", "export const dep = 1\n")], &[]);
    for _ in 0..2 {
        let outcome = through(&workspace, &creates_dep, "./dep");
        assert_eq!(
            target(&outcome),
            Some("/p/dep.ts"),
            "the answer is complete"
        );
        assert!(!outcome.trace().reused(), "and never retained");
    }
    assert_eq!(lane_and_values(&workspace), (0, 0));
}

#[test]
fn overlay_churn_leaves_no_residency_behind() {
    let workspace = workspace(&[]);
    for cycle in 0..200 {
        let path = format!("/p/dep{cycle}.ts");
        let session = crate::overlay_residency::OverlayAuthority::new();
        for edit in 0..2 {
            let snapshot = ResolutionOverlaySnapshot::new_under(
                Some(session.clone()),
                [(
                    path.clone(),
                    Arc::<str>::from(format!("export const v = {edit}\n")),
                )],
                [],
            );
            let specifier = format!("./dep{cycle}");
            assert_eq!(
                target(&through(&workspace, &snapshot, &specifier)),
                Some(path.as_str())
            );
        }
        drop(session);
        assert_eq!(lane_and_values(&workspace), (0, 0), "cycle {cycle}");
    }
    assert_eq!(workspace.engine.overlay_lane_item_count_for_test(), 0);
    assert!(
        workspace.engine.overlay_residency_queue_len_for_test() <= 64,
        "released slots do not accumulate in the eviction queue"
    );
}
