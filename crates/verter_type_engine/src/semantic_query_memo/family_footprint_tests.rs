use std::sync::Arc;

use verter_session_query::retention::allocation::{
    arc_block_bytes, box_bytes, heap_block_bytes, AliasSet,
};
use verter_session_query::retention::owned::{OwnedSlice, OwnedStr};

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
        member: member.into(),
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

fn type_of(canonical: OwnedStr, name: OwnedStr, path: Vec<OwnedStr>) -> FamilyKey {
    FamilyKey::TypeOf {
        value_root: ValueRootSlotIdentity::new(
            ValueRootKey {
                scope: ScopeId::file(canonical, owner()),
                name,
            },
            0,
            HashValue::default(),
            HashValue::default(),
        ),
        path: Box::new(OwnedSlice::from(path)),
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
fn a_family_holds_the_member_name_its_key_retains_by_the_names_own_cell() {
    const NAME: usize = 20_000;
    let small =
        crate::semantic_query_memo::retention::family_container_footprint(&project_member(text(1)));
    let large = crate::semantic_query_memo::retention::family_container_footprint(&project_member(
        text(NAME),
    ));
    assert_eq!(
        large.bytes, small.bytes,
        "the container pays only its records: the name charges itself"
    );
    let account = verter_session_query::retention::SemanticRetentionAccount::new(
        verter_session_query::retention::RetentionLimits::defaults(),
    );
    let ctx = verter_session_query::retention::owned::ChargeCtx::new(&account);
    let holds = large.handles.hold(ctx).expect("admitted");
    assert_eq!(
        account.snapshot().retained_bytes,
        OwnedStr::charge_for_len(NAME),
        "holding the key retains its name once, however many key copies share it"
    );
    drop(holds);
    drop(small);
}

#[test]
fn a_family_pays_for_the_argument_list_its_key_retains() {
    const ARGS: usize = 5_000;
    let small = family_container_bytes(&instantiate(0));
    let large = family_container_bytes(&instantiate(ARGS));
    assert_eq!(large - small, slice_growth::<SemanticNodeId>(ARGS));
}

#[test]
fn a_typeof_family_holds_its_path_by_the_paths_own_cell() {
    const SEGMENTS: usize = 64;
    const SEGMENT: usize = 1_000;
    let small = type_of("/a.ts".into(), "v".into(), vec![]);
    let large = type_of(
        "/a.ts".into(),
        "v".into(),
        (0..SEGMENTS)
            .map(|_| OwnedStr::from("x".repeat(SEGMENT)))
            .collect(),
    );
    let small = crate::semantic_query_memo::retention::family_container_footprint(&small);
    let large = crate::semantic_query_memo::retention::family_container_footprint(&large);
    assert_eq!(
        large.bytes, small.bytes,
        "the container pays only for its key boxes: the path and its names charge themselves"
    );
    let account = verter_session_query::retention::SemanticRetentionAccount::new(
        verter_session_query::retention::RetentionLimits::defaults(),
    );
    let ctx = verter_session_query::retention::owned::ChargeCtx::new(&account);
    let holds = large.handles.hold(ctx).expect("admitted");
    assert!(
        account.snapshot().retained_bytes >= SEGMENTS * SEGMENT,
        "holding the key retains every segment's text, each by its own cell"
    );
    drop(holds);
    drop(small);
}

#[test]
fn the_ledger_clone_pays_for_its_own_box_and_shares_every_handle() {
    const SEGMENT: usize = 3_000;
    let key = type_of(
        "/a.ts".into(),
        "v".into(),
        vec![OwnedStr::from("x".repeat(SEGMENT))],
    );
    let path_box = box_bytes::<OwnedSlice<OwnedStr>>();
    // The map key and the ledger's clone each pay for their boxed path
    // handle; the path, its names and the key's other names are owned
    // allocations both copies share, charged by their own cells.
    assert_eq!(
        family_container_bytes(&key),
        inline_records() + 2 * path_box
    );
}

#[test]
fn an_allocation_a_key_reaches_twice_counts_once() {
    const LEN: usize = 10_000;
    let shared = OwnedStr::from("x".repeat(LEN));
    let key = type_of(
        shared.clone(),
        shared.clone(),
        vec![shared.clone(), shared.clone()],
    );
    let path = OwnedSlice::<OwnedStr>::cell_block_bytes()
        + heap_block_bytes(2 * std::mem::size_of::<OwnedStr>());
    assert_eq!(
        family_key_heap_bytes(&key, &mut AliasSet::estimating()),
        OwnedStr::charge_for_len(LEN) + box_bytes::<OwnedSlice<OwnedStr>>() + path
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
        family_key_heap_bytes(&key, &mut AliasSet::estimating()),
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
    .owned_heap_bytes(&mut AliasSet::estimating())
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
