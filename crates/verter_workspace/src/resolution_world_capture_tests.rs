//! Capture-rooted validation of the resolve-imports resolution arm.
//!
//! These cases pin the two properties a consumer view depends on when it
//! retains an [`crate::resolution_currency::CapturedResolutionWorld`]:
//!
//! 1. a capture answers for the world it captured — an admitted witness
//!    validates against its own capture and stops validating against a
//!    capture taken after the observed value moved;
//! 2. the composition is population-scoped — a session-population witness
//!    never validates against a base capture or another session's capture.

use std::sync::Arc;

use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::resolution_currency::CapturedResolutionWorld;
use crate::traits::{WorkspaceAccess, WorkspaceRead};
use crate::{ReadSetSignature, ResolutionPublication};
use verter_semantic::resolver_core::{
    ResolutionContext, ResolutionPopulation, ResolvePhase, ResolveRequestKind, SessionFingerprint,
};

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

const OWNER: &str = "/p/main.ts";

fn workspace() -> MemoryWorkspace {
    let workspace = MemoryWorkspace::new(MemoryOptions::default());
    workspace.inject_file(
        OWNER.to_string(),
        Arc::from("import { value } from './dep'\nexport { value }\n"),
    );
    workspace
}

/// The admitted witness of one resolution demand. A refusal fails the test:
/// these cases are about what a CACHEABLE witness validates against, so a
/// non-admitted attempt would silently make every assertion vacuous.
fn admitted_signature(
    workspace: &MemoryWorkspace,
    specifier: &str,
) -> (Option<String>, ReadSetSignature) {
    let outcome = WorkspaceRead::resolve_import_outcome(workspace, OWNER, specifier, CONTEXT);
    let target = outcome.result().map(|result| result.source_id.clone());
    match outcome.into_publication() {
        ResolutionPublication::Admitted(admitted) => (target, admitted.signature().clone()),
        ResolutionPublication::Refused(refusal) => panic!(
            "precondition: resolving {specifier} must admit a witness; refused {:?}",
            refusal.reason()
        ),
    }
}

fn capture(workspace: &MemoryWorkspace) -> Arc<CapturedResolutionWorld> {
    WorkspaceRead::capture_resolution_world(workspace)
        .expect("an engine-backed workspace publishes a resolution world")
}

fn capture_population(
    workspace: &MemoryWorkspace,
    population: ResolutionPopulation,
) -> Arc<CapturedResolutionWorld> {
    workspace
        .engine
        .capture_published_resolution_world(population)
        .expect("a settled world is capturable for any population")
}

#[test]
fn a_witness_validates_against_its_own_capture_and_not_against_a_later_one() {
    let workspace = workspace();
    let (missed, witness) = admitted_signature(&workspace, "./dep");
    assert_eq!(
        missed, None,
        "precondition: the dependency must be absent for the first demand"
    );
    let captured_at_miss = capture(&workspace);
    assert!(
        witness.validates(captured_at_miss.as_ref()),
        "the admitted miss witness must validate against the world it was minted in"
    );

    workspace.inject_file(
        "/p/dep.ts".to_string(),
        Arc::from("export const value = 1\n"),
    );
    let captured_after_appearance = capture(&workspace);

    assert!(
        !witness.validates(captured_after_appearance.as_ref()),
        "the appearance moved an observed path probe, so the miss witness must \
         stop validating against a capture taken after it"
    );
    assert!(
        witness.validates(captured_at_miss.as_ref()),
        "the earlier capture is immutable: it keeps answering for the world it \
         captured instead of re-reading the live registry"
    );

    // Mutation recipe: make the arm answer from the engine's live world
    // instead of the captured composition. The second assertion then flips —
    // the retained capture starts reporting post-appearance versions.
}

#[test]
fn a_session_witness_never_validates_against_a_base_or_foreign_session_capture() {
    let workspace = workspace();
    workspace.inject_file(
        "/p/dep.ts".to_string(),
        Arc::from("export const value = 1\n"),
    );
    let (resolved, witness) = admitted_signature(&workspace, "./dep");
    assert_eq!(
        resolved.as_deref(),
        Some("/p/dep.ts"),
        "precondition: the demand must resolve positively through the session population"
    );

    let session = capture(&workspace);
    let base = capture_population(&workspace, ResolutionPopulation::Base);
    let foreign = capture_population(
        &workspace,
        ResolutionPopulation::Session(SessionFingerprint::from_raw(0x5153_5f43_3541)),
    );

    assert!(
        witness.validates(session.as_ref()),
        "the witness must validate against a capture of its own population"
    );
    assert!(
        !witness.validates(base.as_ref()),
        "a session-population witness must not validate against a base capture"
    );
    assert!(
        !witness.validates(foreign.as_ref()),
        "a session-population witness must not validate against another session's capture"
    );
}

#[test]
fn an_overlay_only_fact_stays_inside_its_session_capture() {
    let workspace = workspace();
    // Overlay-only dependency: the base population never observed this path,
    // so the advanced version lives exclusively on the session root.
    WorkspaceAccess::notify_upsert(
        &workspace,
        "/p/dep.ts",
        Arc::from("export const value = 1\n"),
    );
    let (resolved, witness) = admitted_signature(&workspace, "./dep");
    assert_eq!(
        resolved.as_deref(),
        Some("/p/dep.ts"),
        "precondition: the overlay-only dependency must resolve"
    );

    let session = capture(&workspace);
    let base = capture_population(&workspace, ResolutionPopulation::Base);
    let foreign = capture_population(
        &workspace,
        ResolutionPopulation::Session(SessionFingerprint::from_raw(0x5153_5f43_3542)),
    );

    assert!(
        witness.validates(session.as_ref()),
        "the overlay witness must validate against its own session capture"
    );
    assert!(
        !witness.validates(base.as_ref()),
        "an overlay-advanced fact must not leak into base validation"
    );
    assert!(
        !witness.validates(foreign.as_ref()),
        "an overlay-advanced fact must not cross into another session's capture"
    );
}
