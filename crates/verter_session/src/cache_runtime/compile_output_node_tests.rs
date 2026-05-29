//! Inline tests for the typed compile-output cache nodes.
//!
//! Pin the shape of [`CompileOutputNodePureContent`] and
//! [`CompileOutputNodeFactValidatedSession`] independent of any
//! particular host wiring: the pure-content node owns its own
//! `DashMap`; the fact-validated session node delegates storage to a
//! caller-supplied [`ProfileState`] and validates the slot against
//! live override / semantic hashes and a closure-driven fact rail.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{
    CompileOutputNodeFactValidatedSession, CompileOutputNodePureContent, CompileOutputPureContentKey,
    CompileOutputValue, SessionPublishOutcome,
};
use crate::cache_runtime::admission::SignatureAdmission;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::FactVersionRef;
use crate::types::{CachedVirtualFile, DiagnosticsSnapshot, Hash16, ProfileState, VirtualNodeKind};

fn k(canonical: &str, content: Hash16) -> CompileOutputPureContentKey {
    CompileOutputPureContentKey {
        canonical_id: Arc::<str>::from(canonical),
        content_hash: content,
        parse_env_hash: [0x11; 16],
        resolve_env_hash: [0x22; 16],
        type_env_hash: [0x33; 16],
        lib_env_hash: [0x44; 16],
        project_identity: [0x55; 16],
        compile_cache_mode_hash: [0x66; 16],
        source_map_policy_hash: [0x77; 16],
        compiler_version: [0x88; 16],
        plugin_versions: [0x99; 16],
    }
}

fn value(semantic_hash: Hash16) -> CompileOutputValue {
    CompileOutputValue::from_compile_record(
        semantic_hash,
        0u64,
        0u64,
        FxHashMap::default(),
        DiagnosticsSnapshot::default(),
        None,
        None,
        None,
    )
}

#[test]
fn pure_content_node_starts_empty_and_peek_misses() {
    let node = CompileOutputNodePureContent::new();
    assert_eq!(node.entry_count(), 0);
    assert!(node.peek(&k("/a.vue", [0u8; 16])).is_none());
}

#[test]
fn pure_content_publish_admits_value_addressable_by_full_key() {
    let node = CompileOutputNodePureContent::new();
    let key = k("/a.vue", [1u8; 16]);
    node.publish_content(key.clone(), value([0xAA; 16]), 7);
    let hit = node.peek(&key).expect("warm hit after publish");
    assert_eq!(hit.semantic_hash, [0xAA; 16]);

    // Different content_hash → distinct key → no hit.
    let other = k("/a.vue", [2u8; 16]);
    assert!(node.peek(&other).is_none());

    // Distinct parse_env_hash → distinct key → no hit.
    let mut other = key.clone();
    other.parse_env_hash = [0xFF; 16];
    assert!(node.peek(&other).is_none());
}

#[test]
fn pure_content_remove_drops_entry() {
    let node = CompileOutputNodePureContent::new();
    let key = k("/a.vue", [3u8; 16]);
    node.publish_content(key.clone(), value([0xBB; 16]), 9);
    assert_eq!(node.entry_count(), 1);
    node.remove(&key);
    assert_eq!(node.entry_count(), 0);
    assert!(node.peek(&key).is_none());
}

#[test]
fn session_node_misses_when_no_slot_present() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let state = ProfileState::default();
    let semantic = [0u8; 16];
    let hit = node.lookup(&state, 42, &semantic, 0, 0, |_| true);
    assert!(hit.is_none(), "no slot for profile_hash → no warm hit");
}

#[test]
fn session_publish_then_lookup_round_trips_under_matching_hashes() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(
        Vec::<FactVersionRef>::new().as_slice(),
    )));
    let outcome = node.publish(&mut state, 42, admission, value(semantic), 0);
    assert_eq!(outcome, SessionPublishOutcome::Admitted);
    let hit = node.lookup(&state, 42, &semantic, 0, 0, |_| true);
    assert!(hit.is_some(), "matching hashes → warm hit");
}

#[test]
fn session_lookup_misses_when_semantic_hash_differs() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(
        Vec::<FactVersionRef>::new().as_slice(),
    )));
    node.publish(&mut state, 42, admission, value(semantic), 0);
    // Live semantic_hash differs → miss.
    let other = [0xFF; 16];
    let hit = node.lookup(&state, 42, &other, 0, 0, |_| true);
    assert!(
        hit.is_none(),
        "differing semantic_hash MUST miss the warm slot"
    );
}

#[test]
fn session_lookup_misses_when_validate_facts_returns_false() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/dep.ts".to_string(),
        hash: [0xAB; 16],
    }]);
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(facts));
    node.publish(&mut state, 42, admission, value(semantic), 0);

    let hit = node.lookup(&state, 42, &semantic, 0, 0, |_sig| false);
    assert!(
        hit.is_none(),
        "fact-validation closure returning false MUST miss the warm slot"
    );

    let hit = node.lookup(&state, 42, &semantic, 0, 0, |_sig| true);
    assert!(
        hit.is_some(),
        "fact-validation closure returning true → warm hit"
    );
}

#[test]
fn session_publish_non_cacheable_removes_prior_slot() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let semantic = [0x12u8; 16];
    // First publish: cacheable.
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(
        Vec::<FactVersionRef>::new().as_slice(),
    )));
    node.publish(&mut state, 42, admission, value(semantic), 0);
    assert!(node.lookup(&state, 42, &semantic, 0, 0, |_| true).is_some());

    // Second publish: NonCacheable (overflow). Must REMOVE the prior
    // slot so the carrier invariant `present ⇒ admitted cacheable`
    // holds across re-publishes.
    let admission = SignatureAdmission::NonCacheable(
        verter_audit::NonAdmissionReason::SignatureOverflow,
    );
    let outcome = node.publish(&mut state, 42, admission, value(semantic), 1);
    match outcome {
        SessionPublishOutcome::Refused(
            verter_audit::NonAdmissionReason::SignatureOverflow,
        ) => {}
        other => panic!("expected Refused(SignatureOverflow), got {other:?}"),
    }
    assert!(
        node.lookup(&state, 42, &semantic, 0, 0, |_| true).is_none(),
        "non-cacheable publish MUST drop the prior slot"
    );
}

#[test]
fn session_peek_signature_round_trips_admitted_signature() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let facts: Arc<[FactVersionRef]> = Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/dep.ts".to_string(),
        hash: [0xCD; 16],
    }]);
    let signature = ReadSetSignature::new(facts);
    let admission = SignatureAdmission::Cacheable(signature.clone());
    node.publish(&mut state, 42, admission, value([0u8; 16]), 0);
    let observed = node.peek_signature(&state, 42).expect("admitted signature");
    assert_eq!(observed.facts.len(), 1);
    assert!(!observed.overflowed);
}

#[test]
fn session_peek_output_returns_per_kind_pair() {
    let node = CompileOutputNodeFactValidatedSession::new();
    let mut state = ProfileState::default();
    let mut outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile> = FxHashMap::default();
    let file = CachedVirtualFile {
        code: Arc::<str>::from("/* main */"),
        source_map: None,
        lang: None,
        meta: crate::types::VirtualMeta::default(),
    };
    outputs.insert(VirtualNodeKind::Main, file.clone());
    let value = CompileOutputValue::from_compile_record(
        [0u8; 16],
        0,
        0,
        outputs,
        DiagnosticsSnapshot::default(),
        None,
        None,
        None,
    );
    let admission = SignatureAdmission::Cacheable(ReadSetSignature::new(Arc::from(
        Vec::<FactVersionRef>::new().as_slice(),
    )));
    node.publish(&mut state, 42, admission, value, 0);
    let (got, _diag) = node
        .peek_output(&state, 42, &VirtualNodeKind::Main)
        .expect("output for Main");
    assert_eq!(&*got.code, "/* main */");
}
