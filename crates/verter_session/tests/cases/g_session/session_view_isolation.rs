//! Discriminating tests — sessions do not mutate the host.
//!
//! Binds **R17** (sessions are views over the base host; a
//! `SessionView` never mutates the host; `host.upsert` is not called
//! from any query path) and **R20** (multi-candidate storage isolates
//! concurrent overlay variants).
//!
//! There is no overlay-mutation lifecycle, so:
//!
//! 1. Two sessions can hold conflicting overlays simultaneously
//!    without CAS-claiming a shared overlay slot, without
//!    serialising on a project-wide gate, and without one session
//!    waiting on another's host-revert.
//! 2. Query paths (`get_analysis`, `evaluate_types`,
//!    `get_component_meta`, etc.) never invoke `host.upsert(...)`
//!    on the shared host.
//!
//! The host's `upsert_count` provenance counter is the discriminating
//! signal: an overlay-mutation lifecycle would increment this counter
//! (apply + later revert), but the counter does not move during query
//! paths.
//!
//! This file's verification is about the *absence of host mutation*;
//! overlay-correct read semantics are exercised by the
//! `SessionView`-routed cache-read + multi-candidate-storage tests.

use std::sync::Arc;

use verter_session::meta::MetaProject;
use verter_session::{CompileErrorPolicy, HostConfig, VerterHost};

fn fresh_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

fn upsert_base(project: &Arc<MetaProject>, canonical: &str, source: &str) {
    project
        .upsert_base(canonical, source)
        .expect("upsert_base succeeds");
}

fn host_upsert_count(project: &Arc<MetaProject>) -> u64 {
    // The host's `provenance().host_upsert_calls` is bumped once
    // per `VerterHost::upsert(...)` invocation (the increment lives
    // in `host_upsert.rs`). We snapshot the value
    // before and after session queries to assert no query path
    // triggered an upsert internally.
    project
        .host()
        .provenance()
        .host_upsert_calls
        .load(std::sync::atomic::Ordering::Acquire)
}

#[test]
fn two_concurrent_sessions_with_conflicting_overlays_both_succeed() {
    // R17 — sessions never mutate the host. Two sessions with
    // diverging overlay sources for the SAME canonical can run
    // queries concurrently without one waiting on the other's
    // host-revert. A CAS-claim loop in `with_overlay_target_context`
    // would have serialised them through `active_overlay_session`
    // (one session would have had to revert the other's overlay
    // before claiming the host). There is no CAS, no host-revert, no
    // shared gate; both sessions complete without observable
    // serialisation.

    let project = fresh_project();
    upsert_base(&project, "/x.ts", "export const a = 1;");

    let session_a = project.open_session().expect("session A");
    let session_b = project.open_session().expect("session B");

    session_a
        .upsert("/x.ts", r#"export const a = 100;"#.to_string())
        .expect("overlay A");
    session_b
        .upsert("/x.ts", r#"export const a = 200;"#.to_string())
        .expect("overlay B");

    // Both sessions issue a query through their own session
    // surface. An overlay-mutation design would have caused
    // session B's query to revert session A's overlay (or vice
    // versa) — measurable through a non-zero increment of the
    // host's upsert provenance counter. The counter does not move
    // during these query paths.

    let upserts_before = host_upsert_count(&project);

    let analysis_a = session_a
        .get_analysis("/x.ts")
        .expect("session A get_analysis");
    let analysis_b = session_b
        .get_analysis("/x.ts")
        .expect("session B get_analysis");

    let upserts_after = host_upsert_count(&project);

    // Both queries return Some (the file exists in the base host).
    assert!(
        analysis_a.is_some(),
        "session A's query MUST succeed without serialisation"
    );
    assert!(
        analysis_b.is_some(),
        "session B's query MUST succeed without serialisation"
    );

    // R17 — query paths do NOT call host.upsert. The counter is
    // invariant under sessions' query traffic. The two overlays
    // we set via `session.upsert(...)` only mutate the session's
    // own `SessionState.overlays` map, not the host's source
    // store; they do NOT increment `upsert_calls`.
    assert_eq!(
        upserts_before, upserts_after,
        "R17: query paths MUST NOT call host.upsert. \
         Counter moved from {upserts_before} → {upserts_after} \
         during two sessions' get_analysis queries."
    );
}

#[test]
fn session_upsert_does_not_increment_host_upsert_counter() {
    // Stronger R17 — even calling `MetaSession::upsert(...)`
    // (which stores an overlay in the session) MUST NOT mutate
    // the host. The session's overlay map is local state; only
    // the user-facing `MetaProject::upsert_base(...)` path (or
    // an explicit `host.upsert(...)`) increments the host's
    // counter.

    let project = fresh_project();

    // Baseline: `upsert_base` increments the counter.
    let before_base = host_upsert_count(&project);
    upsert_base(&project, "/x.ts", "export const a = 1;");
    let after_base = host_upsert_count(&project);
    assert!(
        after_base > before_base,
        "upsert_base MUST increment host.upsert_calls (control assertion: \
         {before_base} → {after_base})"
    );

    // The invariant: session.upsert MUST NOT increment.
    let session = project.open_session().expect("session");
    let before_overlay = host_upsert_count(&project);
    session
        .upsert("/x.ts", "export const a = 999;".to_string())
        .expect("overlay upsert");
    let after_overlay = host_upsert_count(&project);
    assert_eq!(
        before_overlay, after_overlay,
        "R17: session.upsert MUST NOT increment host.upsert_calls. \
         Counter moved from {before_overlay} → {after_overlay} during \
         a single session.upsert call."
    );
}

#[test]
fn session_close_does_not_call_host_upsert() {
    // R17 — releasing a session does NOT trigger host upserts.
    // An overlay-mutation design would have CAS-cleared
    // `active_overlay_session` on close and called
    // `revert_other_session_overlays` (which invokes
    // `host.upsert(...)` to restore base sources). Session close is
    // a pure state removal.

    let project = fresh_project();
    upsert_base(&project, "/x.ts", "export const a = 1;");

    let session = project.open_session().expect("session");
    session
        .upsert("/x.ts", "export const a = 999;".to_string())
        .expect("overlay");

    let before_close = host_upsert_count(&project);
    drop(session); // Triggers `release_session` via Drop.
    let after_close = host_upsert_count(&project);

    assert_eq!(
        before_close, after_close,
        "R17: closing a session MUST NOT call host.upsert. \
         Counter moved from {before_close} → {after_close} during session drop."
    );
}

#[test]
fn many_concurrent_sessions_do_not_serialise_on_a_project_gate() {
    // R20 stress — N sessions × M overlays. A CAS-based design
    // would have funneled every overlay-bearing query through a
    // single `active_overlay_session` slot, serialising concurrent
    // overlay-bearing queries; there is no project-wide gate.
    //
    // The discriminating signal: zero `host.upsert` calls during
    // an N-session test. An overlay-mutation design would have
    // incremented `upsert_calls` on any session.get_analysis (apply
    // overlay; later revert when another session claimed the slot).
    // This locks in the counter invariant.

    let project = fresh_project();
    upsert_base(&project, "/shared.ts", "export const x = 0;");

    const N: usize = 8;
    let mut sessions = Vec::with_capacity(N);
    for i in 0..N {
        let s = project.open_session().expect("session open");
        s.upsert("/shared.ts", format!("export const x = {i};"))
            .expect("overlay");
        sessions.push(s);
    }

    let upserts_before = host_upsert_count(&project);
    for s in &sessions {
        let _ = s.get_analysis("/shared.ts").expect("get_analysis");
    }
    let upserts_after = host_upsert_count(&project);

    assert_eq!(
        upserts_before, upserts_after,
        "R17, R20: {N} concurrent overlay-bearing sessions MUST \
         NOT trigger a single host.upsert during query paths. Counter moved \
         from {upserts_before} → {upserts_after}."
    );

    // Sanity: all sessions returned Some — none deadlocked or
    // returned an error.
    for (idx, s) in sessions.iter().enumerate() {
        let analysis = s.get_analysis("/shared.ts").expect("get_analysis");
        assert!(
            analysis.is_some(),
            "session {idx} MUST return Some without serialising"
        );
    }

    // Counter still invariant after the second pass.
    let upserts_final = host_upsert_count(&project);
    assert_eq!(
        upserts_after, upserts_final,
        "R17: no host.upsert across two query passes either"
    );
}

#[test]
fn upsert_base_is_the_only_documented_host_upsert_source() {
    // Locks in the architectural invariant: `MetaProject::upsert_base`
    // is THE user-facing entry point that mutates the host. Anything
    // else MUST NOT.
    //
    // The test exercises a representative slice of the session
    // surface and verifies the counter only moves when
    // `upsert_base` is called.

    let project = fresh_project();
    let session = project.open_session().expect("session");

    // No upserts yet → 0.
    let initial = host_upsert_count(&project);

    // One upsert_base call → counter moves.
    upsert_base(&project, "/a.ts", "export const a = 1;");
    let after_base = host_upsert_count(&project);
    assert!(
        after_base > initial,
        "upsert_base bumps the counter ({initial} → {after_base})"
    );

    // Session-side operations → counter does NOT move.
    let before_session_ops = after_base;
    session.upsert("/a.ts", "export a = 99;".to_string()).ok();
    session.delete("/a.ts").ok();
    session.reset("/a.ts").ok();
    let _ = session.get_analysis("/a.ts");
    let _ = session.get_component_meta("/a.ts");
    let _ = session.get_effective_source("/a.ts");
    let after_session_ops = host_upsert_count(&project);
    assert_eq!(
        before_session_ops, after_session_ops,
        "R17: the full session surface (upsert/delete/reset/get_*) \
         MUST NOT bump host.upsert_calls. {before_session_ops} → {after_session_ops}"
    );
}
