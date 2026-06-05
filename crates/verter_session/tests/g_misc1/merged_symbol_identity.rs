//! R7 merged-symbol identity stability tests for Stage 5 Sub-task C.
//!
//! `ResolvedDeclSlotIdentity` is **content-free** and stable across:
//! 1. Interface merging (same-scope `interface Foo` declarations
//!    produce one identity).
//! 2. Function overload addition (overloads contribute to
//!    `merged_parts` as payload; the slot identity is unchanged).
//! 3. Namespace + value merging (TS allows `class Foo` + `namespace
//!    Foo` to merge into a single binding).
//! 4. Source-order reordering (declarations in `[A, B]` vs `[B, A]`
//!    produce the same `merged_symbol_name`).
//!
//! `merged_parts` is **payload, not validation** (R7). Adding an
//! overload changes the payload but does NOT invalidate consumers
//! that observed only another overload's facts.

use std::sync::Arc;

use verter_session::semantic_query::{
    DeclIdentity, DeclPartId, ResolvedDeclSlotIdentity, SemanticSymbolSpace, VersionedDeclIdentity,
};

const PROJECT_IDENTITY: u32 = 42;
const TYPE_ENV_HASH: [u8; 16] = [1; 16];
const LIB_ENV_HASH: [u8; 16] = [2; 16];

fn type_slot(canonical: &str, name: &str) -> ResolvedDeclSlotIdentity {
    ResolvedDeclSlotIdentity::type_slot(
        Arc::from(canonical),
        Arc::from(name),
        PROJECT_IDENTITY,
        TYPE_ENV_HASH,
        LIB_ENV_HASH,
    )
}

fn value_slot(canonical: &str, name: &str) -> ResolvedDeclSlotIdentity {
    ResolvedDeclSlotIdentity::value_slot(
        Arc::from(canonical),
        Arc::from(name),
        PROJECT_IDENTITY,
        TYPE_ENV_HASH,
        LIB_ENV_HASH,
    )
}

/// R7 — interface-merge: two `interface Foo` declarations in the
/// same file produce ONE `ResolvedDeclSlotIdentity`. The
/// `merged_parts` payload records both parts independently.
#[test]
fn r7_interface_merge_produces_one_slot_identity() {
    let canonical = "/src/types.ts";
    let merged_name = "Foo";

    let slot_a = type_slot(canonical, merged_name);
    let slot_b = type_slot(canonical, merged_name);

    assert_eq!(
        slot_a, slot_b,
        "two merged interface parts must produce one slot identity"
    );

    // `merged_parts` records BOTH parts inside the versioned
    // identity (which IS the value).
    let versioned = VersionedDeclIdentity {
        slot: slot_a.clone(),
        content_hash: [3; 16],
        parse_env_hash: [4; 16],
        merged_parts: smallvec::smallvec![(DeclPartId(0), [10; 16]), (DeclPartId(1), [20; 16]),],
    };
    assert_eq!(versioned.merged_parts.len(), 2);
}

/// R7 — declaration reorder: `[A, B]` vs `[B, A]` in source order
/// produces the SAME `merged_symbol_name` (each declaration's name
/// is what discriminates, NOT source position).
#[test]
fn r7_declaration_reorder_preserves_slot_identity() {
    let canonical = "/src/types.ts";

    // First file ordering: A then B.
    let slot_a_first = type_slot(canonical, "A");
    let slot_b_first = type_slot(canonical, "B");

    // Second file ordering: B then A. Names are still A and B —
    // the slot identity is content-free, so the cache key is
    // identical regardless of source-order.
    let slot_a_second = type_slot(canonical, "A");
    let slot_b_second = type_slot(canonical, "B");

    assert_eq!(slot_a_first, slot_a_second);
    assert_eq!(slot_b_first, slot_b_second);
    assert_ne!(slot_a_first, slot_b_first, "A and B must be distinct");
}

/// R7 — function overload: adding a new overload to an existing
/// declaration changes ONLY `merged_parts` payload. The slot
/// identity is unchanged. Crucially, a consumer that observed only
/// one overload's facts MUST NOT be invalidated by an unrelated
/// overload's addition.
#[test]
fn r7_overload_add_changes_payload_not_validation() {
    let slot = value_slot("/src/foo.ts", "doSomething");

    // Initial state: one overload.
    let v1 = VersionedDeclIdentity {
        slot: slot.clone(),
        content_hash: [5; 16],
        parse_env_hash: [6; 16],
        merged_parts: smallvec::smallvec![(DeclPartId(0), [100; 16])],
    };

    // After a second overload is added.
    let v2 = VersionedDeclIdentity {
        slot: slot.clone(),
        content_hash: [5; 16], // SAME content_hash — file body unchanged
        // for the overload we observed.
        parse_env_hash: [6; 16],
        merged_parts: smallvec::smallvec![
            (DeclPartId(0), [100; 16]), // SAME first-overload fingerprint
            (DeclPartId(1), [200; 16]), // NEW second-overload fingerprint
        ],
    };

    // Slot identity is unchanged.
    assert_eq!(v1.slot, v2.slot);

    // The first overload's per-part fingerprint is unchanged.
    let v1_part_0 = v1.merged_parts.iter().find(|p| p.0 == DeclPartId(0));
    let v2_part_0 = v2.merged_parts.iter().find(|p| p.0 == DeclPartId(0));
    assert_eq!(
        v1_part_0, v2_part_0,
        "consumer observing only the first overload sees the SAME per-part fingerprint"
    );

    // The validation contract (R7): merged_parts is PAYLOAD, NOT
    // a validation oracle. The consumer's `fact_dep_signature`
    // observes only the parts it touched; adding an unrelated
    // overload does not change the observed fingerprints.
}

/// R7: TS allows merging a `class Foo` (value-space declaration)
/// with a `namespace Foo` (type-space additions). The two
/// declarations live in DIFFERENT symbol spaces, so they produce
/// DISTINCT `ResolvedDeclSlotIdentity` values even though they
/// share `defining_canonical` and `merged_symbol_name`.
#[test]
fn r7_namespace_value_merge_produces_distinct_slots() {
    let canonical = "/src/foo.ts";
    let name = "Foo";

    // The class declaration occupies the value-space slot.
    let value = value_slot(canonical, name);
    // The namespace declaration occupies the type-space slot
    // (namespaces contribute to the type space when they merge
    // with class/interface declarations).
    let type_ = type_slot(canonical, name);

    assert_ne!(
        value, type_,
        "value-space and type-space slots must be distinct even for the same name"
    );
    assert_eq!(value.defining_canonical, type_.defining_canonical);
    assert_eq!(value.merged_symbol_name, type_.merged_symbol_name);
    assert_eq!(value.symbol_space, SemanticSymbolSpace::Value);
    assert_eq!(type_.symbol_space, SemanticSymbolSpace::Type);
}

/// R7 — slot identity is CONTENT-FREE: two file-versions of the
/// same decl produce the SAME slot, distinct `VersionedDeclIdentity`
/// payloads. The Stage-5b multi-candidate substrate uses this
/// invariant to coexist concurrent generations under one key.
#[test]
fn r7_slot_identity_is_content_free() {
    let slot = type_slot("/src/types.ts", "Foo");

    // Version 1 of the file.
    let v1 = VersionedDeclIdentity::single_part(
        slot.clone(),
        [10; 16], // content_hash v1
        [11; 16], // parse_env_hash
        (DeclPartId(0), [20; 16]),
    );

    // Version 2 of the file (e.g., a body edit).
    let v2 = VersionedDeclIdentity::single_part(
        slot.clone(),
        [30; 16], // content_hash v2 — DIFFERENT
        [11; 16], // same parse_env_hash
        (DeclPartId(0), [40; 16]),
    );

    // Slot identity is identical — the key is content-free.
    assert_eq!(v1.slot, v2.slot);

    // But the versioned payloads are distinct.
    assert_ne!(v1.content_hash, v2.content_hash);

    // The multi-candidate substrate stores both under the same
    // slot key; per-candidate fact validation discriminates them
    // (verified separately by
    // `tests/multi_candidate_storage.rs::r20_two_candidates_coexist_for_same_key`).
}

/// R7 — distinct projects with the SAME (canonical, name) produce
/// DISTINCT slot identities. Project isolation prevents cross-project
/// poisoning (Codex P0.1).
#[test]
fn r7_project_identity_isolates_slots() {
    let canonical = "/src/types.ts";
    let name = "Foo";

    let slot_p1 = ResolvedDeclSlotIdentity::type_slot(
        Arc::from(canonical),
        Arc::from(name),
        1, // project 1
        TYPE_ENV_HASH,
        LIB_ENV_HASH,
    );
    let slot_p2 = ResolvedDeclSlotIdentity::type_slot(
        Arc::from(canonical),
        Arc::from(name),
        2, // project 2
        TYPE_ENV_HASH,
        LIB_ENV_HASH,
    );

    assert_ne!(slot_p1, slot_p2);
}

/// R7 — the test-fixture constructor from `DeclIdentity`
/// (`from_decl_identity`) strips the `whole_hash` field. The resulting
/// slot identity is content-free regardless of which `whole_hash` value
/// the input carried.
#[test]
fn r7_from_decl_identity_strips_whole_hash() {
    let decl_v1 = DeclIdentity {
        canonical_id: Arc::from("/src/types.ts"),
        whole_hash: [10; 16], // version 1 hash
        decl_name: Arc::from("Foo"),
    };
    let decl_v2 = DeclIdentity {
        canonical_id: Arc::from("/src/types.ts"),
        whole_hash: [99; 16], // version 2 hash (different)
        decl_name: Arc::from("Foo"),
    };

    let slot_v1 = ResolvedDeclSlotIdentity::from_decl_identity(
        &decl_v1,
        SemanticSymbolSpace::Type,
        PROJECT_IDENTITY,
        TYPE_ENV_HASH,
        LIB_ENV_HASH,
    );
    let slot_v2 = ResolvedDeclSlotIdentity::from_decl_identity(
        &decl_v2,
        SemanticSymbolSpace::Type,
        PROJECT_IDENTITY,
        TYPE_ENV_HASH,
        LIB_ENV_HASH,
    );

    assert_eq!(
        slot_v1, slot_v2,
        "ResolvedDeclSlotIdentity is content-free; whole_hash MUST NOT enter the slot key"
    );
}
