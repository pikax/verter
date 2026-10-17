use std::sync::Arc;

use verter_session_query::retention::allocation::{arc_block_bytes, box_bytes, AliasSet};

use super::family_key_heap_bytes;
use crate::semantic_query::{
    HashValue, InstantiateBodySource, RelateMemoKey, RelationContext, ResolvedDeclSlotIdentity,
    ScopeId, SemanticNodeId, ValueRootKey, ValueRootSlotIdentity,
};
use crate::semantic_query_memo::family::{FamilyKey, FamilySlots};
use crate::semantic_query_memo::family_intern::InternedRelateKey;
use crate::semantic_query_memo::retention::family_container_bytes;

/// The inline records a family occupies whatever its key holds: its map
/// entry with the control byte, and its ledger record.
fn inline_records() -> usize {
    std::mem::size_of::<(FamilyKey, FamilySlots)>() + 1 + std::mem::size_of::<(u64, FamilyKey)>()
}

fn owner() -> verter_type_expr::TopLevelOwnerId {
    verter_type_expr::TopLevelOwnerId::ordinary_file()
}

fn text(len: usize) -> Arc<str> {
    Arc::from("x".repeat(len))
}

fn project_member(member: Arc<str>) -> FamilyKey {
    FamilyKey::ProjectMember {
        base: SemanticNodeId(1),
        member,
    }
}

fn instantiate(args: usize) -> FamilyKey {
    FamilyKey::Instantiate {
        base: ResolvedDeclSlotIdentity::type_slot_unscoped(
            verter_type_expr::OwnedStr::from("/a.ts"),
            owner(),
            verter_type_expr::OwnedStr::from("A"),
        ),
        args: (0..args as u64).map(SemanticNodeId).collect(),
        resolve_env_hash: HashValue::default(),
        body_source: InstantiateBodySource::NonFile,
        provenance: Default::default(),
        merge_role: Default::default(),
        vue_heritage_policy: Default::default(),
    }
}

fn type_of(canonical: Arc<str>, name: Arc<str>, path: Vec<Arc<str>>) -> FamilyKey {
    FamilyKey::TypeOf {
        value_root: ValueRootSlotIdentity::new(
            ValueRootKey {
                scope: ScopeId::file(canonical, owner()),
                name: name.into(),
            },
            0,
            HashValue::default(),
            HashValue::default(),
        ),
        path: Box::new(verter_type_expr::OwnedSlice::from(path)),
        resolve_env_hash: HashValue::default(),
        provenance: Default::default(),
        merge_role: Default::default(),
        vue_heritage_policy: Default::default(),
    }
}

/// The block an `Arc<[T]>` of `count` elements adds over an empty one.
fn slice_growth<T>(count: usize) -> usize {
    arc_block_bytes(count * std::mem::size_of::<T>()) - arc_block_bytes(0)
}

#[test]
fn a_family_pays_for_the_member_name_its_key_retains() {
    const NAME: usize = 20_000;
    let small = family_container_bytes(&project_member(text(1)));
    let large = family_container_bytes(&project_member(text(NAME)));
    assert_eq!(
        large - small,
        arc_block_bytes(NAME) - arc_block_bytes(1),
        "the name is one shared allocation: the map key counts it, the ledger's clone shares it"
    );
}

#[test]
fn a_family_pays_for_the_argument_list_its_key_retains() {
    const ARGS: usize = 5_000;
    let small = family_container_bytes(&instantiate(0));
    let large = family_container_bytes(&instantiate(ARGS));
    assert_eq!(large - small, slice_growth::<SemanticNodeId>(ARGS));
}

#[test]
fn a_family_pays_for_the_typeof_path_its_key_retains() {
    const SEGMENTS: usize = 64;
    const SEGMENT: usize = 1_000;
    let small = family_container_bytes(&type_of(Arc::from("/a.ts"), Arc::from("v"), vec![]));
    let large = family_container_bytes(&type_of(
        Arc::from("/a.ts"),
        Arc::from("v"),
        (0..SEGMENTS).map(|_| text(SEGMENT)).collect(),
    ));
    assert_eq!(
        large - small,
        slice_growth::<Arc<str>>(SEGMENTS) + SEGMENTS * arc_block_bytes(SEGMENT)
    );
}

#[test]
fn the_ledger_clone_pays_for_its_own_box_and_shares_every_arc() {
    const SEGMENT: usize = 3_000;
    let canonical: Arc<str> = Arc::from("/a.ts");
    let name: Arc<str> = Arc::from("v");
    let segment = text(SEGMENT);
    let key = type_of(
        Arc::clone(&canonical),
        Arc::clone(&name),
        vec![Arc::clone(&segment)],
    );
    let path_box = box_bytes::<Arc<[Arc<str>]>>();
    let map_key = arc_block_bytes(canonical.len())
        + arc_block_bytes(name.len())
        + path_box
        + arc_block_bytes(std::mem::size_of::<Arc<str>>())
        + arc_block_bytes(SEGMENT);
    // The ledger's clone deep-copies the boxed path handle and shares the
    // path slice and every name.
    let ledger_key = path_box;
    assert_eq!(
        family_container_bytes(&key),
        inline_records() + map_key + ledger_key
    );
}

#[test]
fn an_allocation_a_key_reaches_twice_counts_once() {
    const LEN: usize = 10_000;
    let shared = text(LEN);
    let key = type_of(
        Arc::clone(&shared),
        Arc::clone(&shared),
        vec![Arc::clone(&shared), Arc::clone(&shared)],
    );
    assert_eq!(
        family_key_heap_bytes(&key, &mut AliasSet::new()),
        arc_block_bytes(LEN)
            + box_bytes::<Arc<[Arc<str>]>>()
            + arc_block_bytes(2 * std::mem::size_of::<Arc<str>>())
    );
}

#[test]
fn an_interned_relation_record_charges_itself_and_its_holders_pay_only_their_handles() {
    let interned = InternedRelateKey::intern(
        None,
        RelateMemoKey::assignable(
            SemanticNodeId(0xFA_0001),
            SemanticNodeId(0xFA_0002),
            RelationContext::default(),
        ),
    );
    assert!(
        interned.record_charge_bytes() >= arc_block_bytes(std::mem::size_of::<RelateMemoKey>()),
        "the record holding the relation payload is charged as itself"
    );
    let key = FamilyKey::Relate { key: interned };
    assert_eq!(
        family_key_heap_bytes(&key, &mut AliasSet::new()),
        0,
        "a family naming the record holds only its handle"
    );
}

/// What a cost receipt's identity keeps reachable when it names the
/// prepared identity of `key`.
fn prepared_identity_bytes(key: crate::semantic_query::SemanticQueryKey) -> usize {
    crate::project_semantic_dispatch::cost_receipt::CostIdentity::of_producer(
        crate::semantic_query_memo::prepared::PreparedKeyHandle::prepare(key).as_task_producer(),
    )
    .owned_heap_bytes(&mut AliasSet::new())
}

#[test]
fn a_cost_identity_pays_for_the_member_name_its_prepared_key_retains() {
    const NAME: usize = 20_000;
    let member = |member: Arc<str>| crate::semantic_query::SemanticQueryKey::ProjectMember {
        base: SemanticNodeId(1),
        member: member.into(),
        mode: crate::semantic_query::ProjectionMode::Navigate,
    };
    let small = prepared_identity_bytes(member(text(1)));
    let large = prepared_identity_bytes(member(text(NAME)));
    assert!(
        large >= small + arc_block_bytes(NAME) - arc_block_bytes(1),
        "large {large} must exceed small {small} by the name's backing"
    );
}

#[test]
fn a_cost_identity_pays_for_the_member_list_its_prepared_key_retains() {
    const MEMBERS: usize = 5_000;
    let union = |count: usize| crate::semantic_query::SemanticQueryKey::ReduceUnion {
        members: (0..count as u64).map(SemanticNodeId).collect(),
        nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
    };
    let small = prepared_identity_bytes(union(0));
    let large = prepared_identity_bytes(union(MEMBERS));
    assert_eq!(
        large - small,
        slice_growth::<SemanticNodeId>(MEMBERS),
        "the key and its family share one member list"
    );
}
