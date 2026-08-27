use super::*;

#[test]
fn resolution_world_id_unbound_placeholder_is_zero_and_distinct_from_real() {
    let real = ResolutionWorldId::from_raw(1);
    assert_ne!(real, ResolutionWorldId::UNBOUND_PLACEHOLDER);
}

#[test]
#[should_panic(expected = "resolution world ids must be non-zero")]
fn resolution_world_id_from_raw_rejects_zero() {
    let _ = ResolutionWorldId::from_raw(0);
}

#[test]
fn workspace_authority_id_unbound_placeholder_is_zero_and_distinct_from_real() {
    let real = WorkspaceAuthorityId::from_raw(1);
    assert_ne!(real, WorkspaceAuthorityId::UNBOUND_PLACEHOLDER);
}

#[test]
fn workspace_authority_id_from_raw_permits_zero() {
    // Unlike ResolutionWorldId/SessionFingerprint, WorkspaceAuthorityId's
    // 0 IS the placeholder value itself — from_raw(0) must equal it exactly
    // rather than panicking, since a real authority counter could
    // legitimately start at 0 in some host implementations before this
    // type's placeholder convention was introduced.
    assert_eq!(
        WorkspaceAuthorityId::from_raw(0),
        WorkspaceAuthorityId::UNBOUND_PLACEHOLDER
    );
}

#[test]
#[should_panic(expected = "session fingerprints must be non-zero")]
fn session_fingerprint_from_raw_rejects_zero() {
    let _ = SessionFingerprint::from_raw(0);
}

#[test]
fn session_fingerprint_is_deterministic_and_input_sensitive() {
    assert_eq!(
        SessionFingerprint::from_raw(7),
        SessionFingerprint::from_raw(7)
    );
    assert_ne!(
        SessionFingerprint::from_raw(7),
        SessionFingerprint::from_raw(8)
    );
}

#[test]
fn resolution_population_base_and_session_are_distinct() {
    let session = ResolutionPopulation::Session(SessionFingerprint::from_raw(1));
    assert_ne!(ResolutionPopulation::Base, session);
}

#[test]
fn resolution_population_distinguishes_sessions() {
    let a = ResolutionPopulation::Session(SessionFingerprint::from_raw(1));
    let b = ResolutionPopulation::Session(SessionFingerprint::from_raw(2));
    assert_ne!(a, b);
}
