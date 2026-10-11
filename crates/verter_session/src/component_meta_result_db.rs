//! Session publication policy for the engine-owned final component store.

use std::sync::Arc;
use verter_session_query::analysis::types::Hash16;
pub use verter_type_engine::component_meta_result_db::{
    classify_final_component_retention, final_component_refusal_status, footprint,
    refusal_identity_for, ComponentMetaOptionsFingerprint, ComponentMetaRefusalClass,
    ComponentMetaRefusalIdentity, ComponentMetaRefusalQuery, ComponentMetaResultDb,
    ComponentMetaResultEntry, ComponentMetaResultKey, ComponentMetaRetainedRefusal,
    FinalComponentAdmissionProof, FinalComponentRetention,
};
use verter_type_engine::semantic_query::{FactStatus, ResultCompleteness};

/// Evidence carrier for the exact final-result entry admitted by one cold
/// component-meta computation.
///
/// This remains crate-private: downstream consumers may retain a projection
/// witness derived from it, but may not inspect or substitute the admitted
/// entry's fact signature.
#[derive(Clone)]
pub(crate) struct AdmittedComponentMetaResult<P> {
    pub(crate) key: ComponentMetaResultKey,
    pub(crate) owner_whole_hash: Hash16,
    pub(crate) entry: Arc<ComponentMetaResultEntry<P>>,
}

/// Caller-supplied, value-side portion of a component-meta admission
/// decision. It deliberately carries no fact signature: only
/// the selected engine `MemoPublish` can attach finalized evidence from its
/// request-local tracer scope.
pub(crate) enum ComponentMetaPublishDecision<P> {
    /// The cold result is complete and its publish fence is still live.
    Publish {
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        payload: Arc<P>,
        validated_at_generation: u64,
    },
    /// A valid caller-visible value must not warm this cache.
    ReturnOnly(verter_audit::NonAdmissionReason),
    /// Retain an eligible refusal. It is not a successful candidate.
    RetainRefusal {
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        identity: ComponentMetaRefusalIdentity,
        status: FactStatus,
        completeness: ResultCompleteness,
        recovery: Arc<P>,
        validated_at_generation: u64,
    },
    /// The cold computation produced no result to retain.
    NoValue,
}

impl<P> ComponentMetaPublishDecision<P> {
    #[inline]
    pub(crate) fn publish(
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        payload: Arc<P>,
        validated_at_generation: u64,
    ) -> Self {
        Self::Publish {
            key,
            owner_whole_hash,
            payload,
            validated_at_generation,
        }
    }

    #[inline]
    pub(crate) fn return_only(reason: verter_audit::NonAdmissionReason) -> Self {
        Self::ReturnOnly(reason)
    }

    #[inline]
    pub(crate) fn retain_refusal(
        key: ComponentMetaResultKey,
        owner_whole_hash: Hash16,
        identity: ComponentMetaRefusalIdentity,
        status: FactStatus,
        completeness: ResultCompleteness,
        recovery: Arc<P>,
        validated_at_generation: u64,
    ) -> Self {
        Self::RetainRefusal {
            key,
            owner_whole_hash,
            identity,
            status,
            completeness,
            recovery,
            validated_at_generation,
        }
    }

    #[inline]
    pub(crate) fn no_value() -> Self {
        Self::NoValue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn empty_sig() -> verter_session_query::facts::fact_cache::ReadSetSignature {
        verter_session_query::facts::fact_cache::ReadSetSignature::empty()
    }

    /// Build a slot key with zero env axes — the substrate-level DB unit
    /// tests exercise candidate/eviction mechanics, not env discrimination
    /// (that is covered by the R21 guard in
    /// `tests/cases/g_cache/r6_r21_query_identity_keys.rs`), so a uniform zero
    /// env keeps these tests focused on the bounded-candidate behaviour.
    fn mk_result_key(owner: &str, options_fingerprint: Hash16) -> ComponentMetaResultKey {
        ComponentMetaResultKey {
            owner_canonical: Arc::from(owner),
            options_fingerprint,
            project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
            parse_env_hash: [0u8; 16],
            resolve_env_hash: [0u8; 16],
            type_env_hash: [0u8; 16],
            lib_env_hash: [0u8; 16],
        }
    }

    #[test]
    fn db_owned_compute_attaches_traced_facts_and_refuses_hazards() {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = mk_result_key("/w/owner.vue", [0u8; 16]);
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some("/w/owner.vue".into()),
                input_id: "/w/owner.vue".into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let owner_hash = host
            .ensure_indexed_ready("/w/owner.vue")
            .unwrap()
            .whole_hash;
        let owner_fact = verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: "/w/owner.vue".to_string(),
            hash: owner_hash,
        };

        let dispatch =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
        let results =
            crate::component_meta_result_admission::ComponentMetaResultPublish::new(&dispatch, &db);
        let value = results.compute_and_admit(
            "/w/owner.vue",
            "unit-test",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                41u32
            },
            |_value| {
                ComponentMetaPublishDecision::publish(
                    key.clone(),
                    owner_hash,
                    Arc::new(41u32),
                    dispatch.current_project_generation(),
                )
            },
        );
        assert_eq!(value, 41);
        let admitted = db
            .get(&key, owner_hash)
            .expect("DB-owned computation must admit its traced candidate");
        assert_eq!(
            admitted.read_set_signature.facts.as_ref(),
            std::slice::from_ref(&owner_fact)
        );

        let refused_key = mk_result_key("/w/refused.vue", [0u8; 16]);
        let refused_hash = [8u8; 16];
        let refused_value = results.compute_and_admit(
            "/w/refused.vue",
            "unit-test",
            || {
                verter_type_engine::fact_tracing::note_non_cacheable_read_fan_out(
                    verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
                );
                42u32
            },
            |_value| {
                ComponentMetaPublishDecision::publish(
                    refused_key.clone(),
                    refused_hash,
                    Arc::new(42u32),
                    0,
                )
            },
        );
        assert_eq!(refused_value, 42, "ReturnOnly still serves the cold caller");
        assert!(
            db.get(&refused_key, refused_hash).is_none(),
            "a transitive derivation hazard must never reach storage"
        );

        let edited = results.compute_and_admit(
            "/w/owner.vue",
            "publication-edit",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                43u32
            },
            |_| {
                // The edit occurs after tracer finalization, before storage.
                let _ = host
                    .upsert(crate::UpsertRequest {
                        canonical_id: Some("/w/owner.vue".into()),
                        input_id: "/w/owner.vue".into(),
                        source: Arc::from(
                            "<script setup lang=\"ts\">defineProps<{ x: number }>()</script>",
                        ),
                        file_language: crate::FileLanguage::vue(),
                        aliases: Vec::new(),
                    })
                    .unwrap();
                ComponentMetaPublishDecision::publish(
                    key.clone(),
                    owner_hash,
                    Arc::new(43u32),
                    dispatch.current_project_generation(),
                )
            },
        );
        assert_eq!(edited, 43, "the winner retains its own computed result");
        assert_eq!(
            *db.get(&key, owner_hash).unwrap().payload,
            41,
            "superseded evidence never replaces the resident candidate"
        );
        assert!(
            dispatch
                .read_component_meta_result(&db, &key, owner_hash)
                .is_none(),
            "the resident candidate's old dependency is no longer valid"
        );

        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some("/w/dep.ts".into()),
                input_id: "/w/dep.ts".into(),
                source: Arc::from("export type Value = string;"),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let owner_hash = host
            .ensure_indexed_ready("/w/owner.vue")
            .unwrap()
            .whole_hash;
        let dep_hash = host.ensure_indexed_ready("/w/dep.ts").unwrap().whole_hash;
        let (_, admitted) = results.compute_and_admit_with_entry(
            "/w/owner.vue",
            "dependency-publication-edit",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                        canonical_id: "/w/dep.ts".into(),
                        hash: dep_hash,
                    },
                );
                44u32
            },
            |_| {
                let _ = host
                    .upsert(crate::UpsertRequest {
                        canonical_id: Some("/w/dep.ts".into()),
                        input_id: "/w/dep.ts".into(),
                        source: Arc::from("export type Value = number;"),
                        file_language: crate::FileLanguage::script_ts(),
                        aliases: Vec::new(),
                    })
                    .unwrap();
                ComponentMetaPublishDecision::publish(
                    key.clone(),
                    owner_hash,
                    Arc::new(44u32),
                    dispatch.current_project_generation(),
                )
            },
        );
        assert!(
            admitted.is_none(),
            "a dependency edit breaks the publication fence even when the owner is unchanged"
        );
    }

    #[test]
    fn insert_and_get_roundtrip() {
        #[derive(Clone, PartialEq, Eq, Debug)]
        struct MockPayload(u32);
        impl verter_session_query::retention::RetainedFootprint for MockPayload {
            fn retained_footprint_bytes(&self) -> usize {
                std::mem::size_of::<Self>()
            }
        }
        let db: ComponentMetaResultDb<MockPayload> = ComponentMetaResultDb::new();
        let key = mk_result_key("/w/Accordion.vue", [9u8; 16]);
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(MockPayload(42)),
            read_set_signature: verter_session_query::facts::fact_cache::ReadSetSignature::new(
                Arc::from(vec![
                    verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                        canonical_id: "/w/Accordion.vue".to_string(),
                        hash: [1u8; 16],
                    },
                ]),
            ),
            validated_at_generation: 0,
        };
        db.insert(key.clone(), [1u8; 16], entry);
        let hit = db.get(&key, [1u8; 16]).unwrap();
        assert_eq!(*hit.payload, MockPayload(42));
        assert_eq!(hit.read_set_signature.facts.len(), 1);
    }

    #[test]
    fn distinct_options_fingerprints_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let k1 = mk_result_key("/w/o.vue", [1u8; 16]);
        let k2 = mk_result_key("/w/o.vue", [2u8; 16]);
        db.insert(
            k1.clone(),
            [1u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                read_set_signature: empty_sig(),
                validated_at_generation: 0,
            },
        );
        assert!(db.get(&k1, [1u8; 16]).is_some());
        assert!(db.get(&k2, [1u8; 16]).is_none());
    }

    /// Distinct owner content versions are distinct candidates inside
    /// one slot — a lookup for one version never returns another's
    /// payload.
    #[test]
    fn distinct_owner_hashes_are_distinct_candidates() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = mk_result_key("/w/o.vue", [9u8; 16]);
        db.insert(
            key.clone(),
            [1u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                read_set_signature: empty_sig(),
                validated_at_generation: 0,
            },
        );
        db.insert(
            key.clone(),
            [2u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(2u32),
                read_set_signature: empty_sig(),
                validated_at_generation: 0,
            },
        );
        assert_eq!(*db.get(&key, [1u8; 16]).unwrap().payload, 1);
        assert_eq!(*db.get(&key, [2u8; 16]).unwrap().payload, 2);
        assert!(db.get(&key, [3u8; 16]).is_none());
        // Both versions coexist as candidates in the one slot.
        assert_eq!(db.len(), 2);
    }

    #[test]
    fn remove_clears_entry() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = mk_result_key("/w/o.vue", [0u8; 16]);
        db.insert(
            key.clone(),
            [1u8; 16],
            ComponentMetaResultEntry {
                payload: Arc::new(5u32),
                read_set_signature: empty_sig(),
                validated_at_generation: 0,
            },
        );
        assert!(db.remove(&key, [1u8; 16]).is_some());
        assert!(db.get(&key, [1u8; 16]).is_none());
    }

    /// The per-slot candidate cap bounds how many content versions of
    /// one owner are retained. DISCRIMINATES: an unbounded slot would
    /// retain every version.
    #[test]
    fn per_slot_cap_bounds_owner_versions() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = mk_result_key("/w/o.vue", [0u8; 16]);
        // Insert one more version than the per-slot cap.
        let versions = ComponentMetaResultDb::<u32>::PER_SLOT_CANDIDATE_CAP + 3;
        for v in 0..versions {
            let mut hash = [0u8; 16];
            hash[0] = v as u8;
            db.insert(
                key.clone(),
                hash,
                ComponentMetaResultEntry {
                    payload: Arc::new(v as u32),
                    read_set_signature: empty_sig(),
                    validated_at_generation: 0,
                },
            );
        }
        assert_eq!(
            db.len(),
            ComponentMetaResultDb::<u32>::PER_SLOT_CANDIDATE_CAP,
            "the slot must retain at most PER_SLOT_CANDIDATE_CAP versions",
        );
        // The oldest versions were evicted; the newest survive.
        let mut hash_last = [0u8; 16];
        hash_last[0] = (versions - 1) as u8;
        assert!(
            db.get(&key, hash_last).is_some(),
            "the newest version must still be cached",
        );
        let mut hash_first = [0u8; 16];
        hash_first[0] = 0;
        assert!(
            db.get(&key, hash_first).is_none(),
            "the oldest version must have been evicted by the bounded cap",
        );
    }

    #[test]
    fn cap_constants_match_plan() {
        assert_eq!(ComponentMetaResultDb::<u32>::PER_SLOT_CANDIDATE_CAP, 4);
        assert_eq!(ComponentMetaResultDb::<u32>::GLOBAL_BUDGET, 512);
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        assert_eq!(db.per_slot_candidate_cap(), 4);
        assert_eq!(db.global_budget(), 512);
    }

    /// `invalidate_owner` drops every candidate whose owner canonical
    /// matches, regardless of owner whole-hash / options. Unrelated
    /// owners stay warm.
    #[test]
    fn invalidate_owner_removes_all_keys_for_one_canonical() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let mk_key = |owner: &str| mk_result_key(owner, [0u8; 16]);
        let mk_entry = || ComponentMetaResultEntry {
            payload: Arc::new(1u32),
            read_set_signature: empty_sig(),
            validated_at_generation: 0,
        };

        // Two versions for /w/a.vue, one for /w/b.vue.
        db.insert(mk_key("/w/a.vue"), [1u8; 16], mk_entry());
        db.insert(mk_key("/w/a.vue"), [2u8; 16], mk_entry());
        db.insert(mk_key("/w/b.vue"), [1u8; 16], mk_entry());

        let removed = db.invalidate_owner("/w/a.vue");
        assert_eq!(removed, 2);
        // /w/b.vue stays.
        assert!(db.get(&mk_key("/w/b.vue"), [1u8; 16]).is_some());
        // /w/a.vue is fully gone.
        assert!(db.get(&mk_key("/w/a.vue"), [1u8; 16]).is_none());
    }

    /// The external live counter is exact after a non-trivial
    /// admit / evict / re-admit / invalidate sequence — it tracks live
    /// occupancy, never lifetime inserts.
    ///
    /// DISCRIMINATES against a gross net-delta error: a missing
    /// `fetch_sub` on `remove`/`invalidate_owner`, a doubled `fetch_add`,
    /// a wrong sign, an `insert` that net-adds on a same-version
    /// replace, or a per-slot/global eviction whose victim removal
    /// skips the decrement would all leave the counter diverged from
    /// `inner.live_count()` after this mixed sequence — every assertion
    /// below would catch it.
    ///
    /// ## Why this is the discriminating form for the snapshot→delta fix
    ///
    /// The pre-fix bug is an unsynchronised `store(live_count())`
    /// that loses a concurrent update. A *deterministic* reproduction
    /// would need to pin one writer between its `live_count()` read and
    /// its `store` — and the only place that read-then-write window
    /// exists is inside `sync_live_counter`, which the net-delta fix
    /// DELETES. No `cfg(test)` injection point can therefore survive
    /// onto the post-fix tree to make the race deterministic there, so a
    /// fully-deterministic FAIL-pre / PASS-post race test is genuinely
    /// infeasible: the fix closes the window rather than guarding it. A
    /// non-deterministic stress test that passes against both trees
    /// would be non-discriminating, so none is committed. This
    /// deterministic exactness test is the committed discriminator — it
    /// fails against any net-delta accounting error on the post-fix
    /// tree, which is the form of regression a future edit can
    /// reintroduce.
    #[test]
    fn live_counter_exact_after_mixed_admit_evict_sequence() {
        let counter = Arc::new(AtomicU64::new(0));
        let db: ComponentMetaResultDb<u32> =
            ComponentMetaResultDb::with_counters(Arc::clone(&counter), Arc::new(AtomicU64::new(0)));
        let mk_key = |owner: &str| mk_result_key(owner, [0u8; 16]);
        let mk_entry = |v: u32| ComponentMetaResultEntry {
            payload: Arc::new(v),
            read_set_signature: empty_sig(),
            validated_at_generation: 0,
        };

        // Three fresh admissions under distinct owners → counter 3.
        db.insert(mk_key("/w/a.vue"), [1u8; 16], mk_entry(1));
        db.insert(mk_key("/w/b.vue"), [1u8; 16], mk_entry(2));
        db.insert(mk_key("/w/c.vue"), [1u8; 16], mk_entry(3));
        assert_eq!(counter.load(Ordering::Relaxed), 3, "three fresh admits");

        // Re-admitting the SAME (key, owner_whole_hash) refreshes in
        // place — it must NOT change the live count.
        db.insert(mk_key("/w/a.vue"), [1u8; 16], mk_entry(11));
        assert_eq!(
            counter.load(Ordering::Relaxed),
            3,
            "same-version re-admit must not change the live count",
        );

        // A second content version of /w/a.vue is a distinct candidate
        // → counter 4.
        db.insert(mk_key("/w/a.vue"), [2u8; 16], mk_entry(12));
        assert_eq!(counter.load(Ordering::Relaxed), 4, "second a.vue version");

        // Remove one candidate → counter 3.
        assert!(db.remove(&mk_key("/w/b.vue"), [1u8; 16]).is_some());
        assert_eq!(counter.load(Ordering::Relaxed), 3, "one candidate removed");

        // Invalidate /w/a.vue (both versions) → counter 1 (only c.vue).
        assert_eq!(db.invalidate_owner("/w/a.vue"), 2);
        assert_eq!(
            counter.load(Ordering::Relaxed),
            1,
            "invalidate_owner must net-subtract every removed candidate",
        );
        assert_eq!(
            counter.load(Ordering::Relaxed) as usize,
            db.len(),
            "the external counter must equal the substrate's live_count",
        );

        // Project-generation clear → counter 0.
        db.invalidate_all();
        assert_eq!(counter.load(Ordering::Relaxed), 0, "invalidate_all zeroes");
    }

    #[test]
    fn eligible_refusals_replay_once_and_are_not_successful_candidates() {
        use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
        use verter_type_engine::semantic_query::{PartialReasonSet, ResultCompleteness};

        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let canonical = "/w/owner.vue";
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let owner_hash = host.ensure_indexed_ready(canonical).unwrap().whole_hash;
        let dep = "/w/dep.ts";
        let dep_source = "export type Value = string;";
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(dep.into()),
                input_id: dep.into(),
                source: Arc::from(dep_source),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let dep_hash = host.ensure_indexed_ready(dep).unwrap().whole_hash;
        let owner_fact = verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: owner_hash,
        };
        let dep_fact = verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: dep.to_string(),
            hash: dep_hash,
        };
        let key = mk_result_key(canonical, [0u8; 16]);
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let dispatch =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
        let publish =
            crate::component_meta_result_admission::ComponentMetaResultPublish::new(&dispatch, &db);
        let read = crate::component_meta_result_admission::ComponentMetaResultRead::new(
            &dispatch,
            &db,
            host.provenance().as_ref(),
        );
        let generation = dispatch.current_project_generation();
        let install = |budget: usize, id: u64| {
            RequestContextGuard::install(RequestContext::with_kind_timing_and_projection_budget(
                id,
                Arc::from(canonical),
                verter_audit::RequestKind::ComponentMeta,
                false,
                false,
                None,
                budget,
            ))
        };
        let _low = install(4, 1);
        let query = dispatch.component_meta_refusal_query(canonical);
        assert_eq!(query.profile.request_operations, 4);
        assert_eq!(query.entry_allowance, 0);
        assert!(query.entered_isolated);

        let budget_partial = ResultCompleteness::partial(PartialReasonSet::BUDGET_EXCEEDED);
        let operation = match classify_final_component_retention(
            budget_partial,
            FinalComponentAdmissionProof::isolated(true),
        ) {
            FinalComponentRetention::Retain(class) => class,
            other => panic!("pure budget refusal must be retained, got {other:?}"),
        };
        assert_eq!(operation, ComponentMetaRefusalClass::OperationProfile);
        let mut producers = 0u32;
        let served = publish.compute_and_admit(
            canonical,
            "operation-refusal",
            || {
                producers += 1;
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                7u32
            },
            |_| {
                ComponentMetaPublishDecision::retain_refusal(
                    key.clone(),
                    owner_hash,
                    refusal_identity_for(operation, &query),
                    final_component_refusal_status(budget_partial.reasons()).unwrap(),
                    budget_partial,
                    Arc::new(7u32),
                    generation,
                )
            },
        );
        assert_eq!(served, 7);
        assert_eq!(producers, 1);
        assert_eq!(db.len(), 0, "a refusal is not a successful candidate");
        let replayed = read
            .replay_refusal(&key, owner_hash, &query)
            .expect("the same profile replays");
        assert_eq!(*replayed.recovery, 7);
        assert_eq!(producers, 1, "replay does not run the producer");
        let mut other_allowance = query.clone();
        other_allowance.entry_allowance = 1;
        assert!(
            read.replay_refusal(&key, owner_hash, &other_allowance)
                .is_none(),
            "a different entry allowance does not reuse an operation-profile refusal"
        );

        drop(_low);
        let _high = install(8, 2);
        let higher = dispatch.component_meta_refusal_query(canonical);
        assert!(
            read.replay_refusal(&key, owner_hash, &higher).is_none(),
            "a higher operation profile misses"
        );
        let healed = publish.compute_and_admit(
            canonical,
            "heal",
            || {
                producers += 1;
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                9u32
            },
            |_| {
                ComponentMetaPublishDecision::publish(
                    key.clone(),
                    owner_hash,
                    Arc::new(9u32),
                    generation,
                )
            },
        );
        assert_eq!(healed, 9);
        assert_eq!(producers, 2);
        assert_eq!(
            *dispatch
                .read_component_meta_result(&db, &key, owner_hash)
                .expect("the healed fact is a successful candidate")
                .payload,
            9
        );
        drop(_high);
        let _low_again = install(4, 3);
        assert!(
            dispatch
                .read_component_meta_result(&db, &key, owner_hash)
                .is_some(),
            "after heal the complete fact hits for the lower profile too"
        );

        let divergence = ResultCompleteness::partial(PartialReasonSet::SAME_PATH_RECURSION);
        assert!(
            matches!(
                classify_final_component_retention(
                    divergence,
                    FinalComponentAdmissionProof::isolated(true)
                ),
                FinalComponentRetention::NotAdmitted
            ),
            "a recursion bit without a certificate is not retained"
        );
        let certified = match classify_final_component_retention(
            divergence,
            FinalComponentAdmissionProof::certified(),
        ) {
            FinalComponentRetention::Retain(class) => class,
            other => panic!("a certified recurrence must be retained, got {other:?}"),
        };
        let certified_key = mk_result_key(canonical, [2u8; 16]);
        publish.compute_and_admit(
            canonical,
            "certified",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                8u32
            },
            |_| {
                ComponentMetaPublishDecision::retain_refusal(
                    certified_key.clone(),
                    owner_hash,
                    refusal_identity_for(certified, &query),
                    final_component_refusal_status(divergence.reasons()).unwrap(),
                    divergence,
                    Arc::new(8u32),
                    generation,
                )
            },
        );
        drop(_low_again);
        let _high_again = install(80, 4);
        let high_query = dispatch.component_meta_refusal_query(canonical);
        assert_eq!(
            *read
                .replay_refusal(&certified_key, owner_hash, &high_query)
                .expect("certified divergence ignores the profile")
                .recovery,
            8
        );
        let mut certified_other_allowance = high_query.clone();
        certified_other_allowance.entry_allowance = 3;
        assert!(
            read.replay_refusal(&certified_key, owner_hash, &certified_other_allowance)
                .is_none(),
            "certified divergence keeps the entry allowance"
        );

        let depth = ResultCompleteness::partial(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT);
        let isolated = match classify_final_component_retention(
            depth,
            FinalComponentAdmissionProof::isolated(true),
        ) {
            FinalComponentRetention::Retain(class) => class,
            other => panic!("isolated depth must be retained, got {other:?}"),
        };
        assert!(matches!(
            classify_final_component_retention(
                depth,
                FinalComponentAdmissionProof::isolated(false)
            ),
            FinalComponentRetention::NotAdmitted
        ));
        let isolated_key = mk_result_key(canonical, [3u8; 16]);
        publish.compute_and_admit(
            canonical,
            "isolated",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    dep_fact.clone(),
                );
                5u32
            },
            |_| {
                ComponentMetaPublishDecision::retain_refusal(
                    isolated_key.clone(),
                    owner_hash,
                    refusal_identity_for(isolated, &query),
                    final_component_refusal_status(depth.reasons()).unwrap(),
                    depth,
                    Arc::new(5u32),
                    generation,
                )
            },
        );
        assert!(read
            .replay_refusal(&isolated_key, owner_hash, &query)
            .is_some());
        let mut other_allowance = query.clone();
        other_allowance.entry_allowance = 1;
        assert!(read
            .replay_refusal(&isolated_key, owner_hash, &other_allowance)
            .is_none());
        let mut other_root = query.clone();
        other_root.root = Arc::from("/w/other.vue");
        assert!(read
            .replay_refusal(&isolated_key, owner_hash, &other_root)
            .is_none());
        let mut other_context = query.clone();
        other_context.context = 9;
        assert!(read
            .replay_refusal(&isolated_key, owner_hash, &other_context)
            .is_none());

        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(dep.into()),
                input_id: dep.into(),
                source: Arc::from("export type Value = number;"),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let _ = host.ensure_indexed_ready(dep).unwrap();
        assert!(
            read.replay_refusal(&isolated_key, owner_hash, &query)
                .is_none(),
            "a dependency edit misses"
        );
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(dep.into()),
                input_id: dep.into(),
                source: Arc::from(dep_source),
                file_language: crate::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        let _ = host.ensure_indexed_ready(dep).unwrap();
        assert!(
            read.replay_refusal(&isolated_key, owner_hash, &query)
                .is_some(),
            "restoring the dependency restores the retained refusal"
        );

        let missing = ResultCompleteness::partial(PartialReasonSet::MISSING_DEPENDENCY);
        assert!(matches!(
            classify_final_component_retention(
                missing,
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted
        ));
        let missing_key = mk_result_key(canonical, [4u8; 16]);
        publish.compute_and_admit(
            canonical,
            "missing",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                1u32
            },
            |_| {
                ComponentMetaPublishDecision::return_only(
                    verter_audit::NonAdmissionReason::PartialResult,
                )
            },
        );
        assert!(read
            .replay_refusal(&missing_key, owner_hash, &query)
            .is_none());
        publish.compute_and_admit(
            canonical,
            "missing-healed",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                2u32
            },
            |_| {
                ComponentMetaPublishDecision::publish(
                    missing_key.clone(),
                    owner_hash,
                    Arc::new(2u32),
                    generation,
                )
            },
        );
        assert_eq!(
            *dispatch
                .read_component_meta_result(&db, &missing_key, owner_hash)
                .unwrap()
                .payload,
            2
        );

        assert!(matches!(
            classify_final_component_retention(
                ResultCompleteness::partial(PartialReasonSet::CANCELLED),
                FinalComponentAdmissionProof::isolated(true)
            ),
            FinalComponentRetention::NotAdmitted
        ));
        let before = db.len();
        let sibling = mk_result_key(canonical, [5u8; 16]);
        publish.compute_and_admit(
            canonical,
            "sibling",
            || {
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                3u32
            },
            |_| {
                ComponentMetaPublishDecision::publish(
                    sibling.clone(),
                    owner_hash,
                    Arc::new(3u32),
                    generation,
                )
            },
        );
        assert_eq!(db.len(), before + 1);
        assert!(read
            .replay_refusal(&isolated_key, owner_hash, &query)
            .is_some());
        assert!(dispatch
            .read_component_meta_result(&db, &sibling, owner_hash)
            .is_some());
    }

    #[test]
    fn refusal_identity_keeps_spent_ops_and_replay_charges_them() {
        use verter_type_engine::project_semantic_dispatch::connected_demand::WorkBudgetForTests;
        use verter_type_engine::request_context::{
            current_request_budget, current_request_context, RequestContext, RequestContextGuard,
        };
        use verter_type_engine::semantic_query::{PartialReasonSet, ResultCompleteness};

        let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
        let canonical = "/w/spent-ops.vue";
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let owner_hash = host.ensure_indexed_ready(canonical).unwrap().whole_hash;
        let owner_fact = verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
            canonical_id: canonical.to_string(),
            hash: owner_hash,
        };
        let key = mk_result_key(canonical, [7u8; 16]);
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let dispatch =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
        let publish =
            crate::component_meta_result_admission::ComponentMetaResultPublish::new(&dispatch, &db);
        let read = crate::component_meta_result_admission::ComponentMetaResultRead::new(
            &dispatch,
            &db,
            host.provenance().as_ref(),
        );
        let generation = dispatch.current_project_generation();
        let install = |budget: usize, id: u64| {
            RequestContextGuard::install(RequestContext::with_kind_timing_and_projection_budget(
                id,
                Arc::from(canonical),
                verter_audit::RequestKind::ComponentMeta,
                false,
                false,
                None,
                budget,
            ))
        };

        let _fresh = install(8, 1);
        let entered = dispatch.component_meta_refusal_query(canonical);
        assert_eq!(entered.entry_allowance, 0);
        assert_eq!(entered.profile.request_operations, 8);
        current_request_context()
            .expect("the request is installed")
            .projection_budget
            .check_projection_op_count();
        let spent_query = dispatch.component_meta_refusal_query(canonical);
        assert_eq!(spent_query.entry_allowance, 1);
        assert_eq!(spent_query.profile, entered.profile);
        assert_ne!(
            refusal_identity_for(ComponentMetaRefusalClass::OperationProfile, &entered),
            refusal_identity_for(ComponentMetaRefusalClass::OperationProfile, &spent_query),
            "spent operations are part of the refusal identity the query derives"
        );
        drop(_fresh);

        let _cold = install(20, 2);
        let query = dispatch.component_meta_refusal_query(canonical);
        let budget_partial = ResultCompleteness::partial(PartialReasonSet::BUDGET_EXCEEDED);
        let operation = match classify_final_component_retention(
            budget_partial,
            FinalComponentAdmissionProof::isolated(true),
        ) {
            FinalComponentRetention::Retain(class) => class,
            other => panic!("budget refusal must be retained, got {other:?}"),
        };
        publish.compute_and_admit(
            canonical,
            "charge",
            || {
                let budget = current_request_budget().expect("request");
                budget.check_projection_op_count();
                budget.check_projection_op_count();
                verter_type_engine::resolver_core::resolver_context::observe_fan_out(
                    owner_fact.clone(),
                );
                7u32
            },
            |_| {
                ComponentMetaPublishDecision::retain_refusal(
                    key.clone(),
                    owner_hash,
                    refusal_identity_for(operation, &query),
                    final_component_refusal_status(budget_partial.reasons()).unwrap(),
                    budget_partial,
                    Arc::new(7u32),
                    generation,
                )
            },
        );
        assert_eq!(
            current_request_budget()
                .expect("request")
                .projection_ops_executed_count(),
            2
        );
        drop(_cold);

        let _replay = install(20, 3);
        assert_eq!(
            current_request_budget()
                .expect("fresh request")
                .projection_ops_executed_count(),
            0
        );
        let replayed = read
            .replay_refusal(&key, owner_hash, &query)
            .expect("an equal fresh request replays");
        assert_eq!(*replayed.recovery, 7);
        assert_eq!(
            current_request_budget()
                .expect("fresh request")
                .projection_ops_executed_count(),
            2,
            "replay charges the operations the refused compute spent"
        );
        assert!(
            read.replay_refusal(&key, owner_hash, &query).is_none(),
            "the same request is no longer at the entered state"
        );
        drop(_replay);

        let _low_work = WorkBudgetForTests::install(3);
        let low_dispatch =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
        let _low_request = install(20, 4);
        let low_query = low_dispatch.component_meta_refusal_query(canonical);
        assert_eq!(low_query.profile.work, 3);
        drop(_low_request);
        drop(_low_work);
        let _high_work = WorkBudgetForTests::install(9);
        let high_dispatch =
            verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch::new(&host);
        let _high_request = install(20, 5);
        let high_query = high_dispatch.component_meta_refusal_query(canonical);
        assert_eq!(high_query.profile.work, 9);
        assert_ne!(low_query.profile, high_query.profile);
        let high_read = crate::component_meta_result_admission::ComponentMetaResultRead::new(
            &high_dispatch,
            &db,
            host.provenance().as_ref(),
        );
        assert!(
            high_read
                .replay_refusal(&key, owner_hash, &high_query)
                .is_none(),
            "a different connected-work allowance does not reuse the refusal"
        );
    }

    #[test]
    fn retained_operation_refusal_does_not_rerun_component_meta() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use verter_type_engine::request_context::{RequestContext, RequestContextGuard};

        let host = crate::VerterHost::new_standalone(crate::types::HostConfig {
            projection_op_budget: 1,
            ..crate::types::HostConfig::default()
        });
        let canonical = "/w/budget-refusal.vue";
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();

        let first = host
            .get_component_meta(canonical)
            .expect("the refused demand still returns the recovery");
        assert!(
            !first.is_complete(),
            "projection budget 1 is an operation-profile refusal, got {:?}",
            first.status()
        );

        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let second = host
            .get_component_meta(canonical)
            .expect("the retained refusal is delivered again");
        assert!(
            !ran.load(Ordering::SeqCst),
            "replaying the refusal must not enter the cold body, status {:?}",
            second.status()
        );
        assert_eq!(second.status(), first.status());
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK
            .with(|slot| *slot.borrow_mut() = None);

        let ran_resolution = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran_resolution);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let resolved = host.get_component_meta_output_with_resolution(canonical);
        assert!(
            resolved.is_ok(),
            "resolution replay returns the retained refusal, got {resolved:?}"
        );
        assert!(
            !ran_resolution.load(Ordering::SeqCst),
            "the resolution demand must not resolve again"
        );
        let (_analysis, resolution) = host
            .get_component_meta_with_resolution(canonical)
            .expect("the resolution consumer replays the retained refusal");
        assert!(
            !resolution.fact_status().is_complete(),
            "a replayed resolution is the refusal, not a successful hit: {:?}",
            resolution.fact_status()
        );
        assert!(
            resolution.synthesis_should_suppress,
            "a refusal replay must not present the recovery as cacheable"
        );
        assert_eq!(
            host.project_type_store().component_meta_results().len(),
            0,
            "the refusal is not a successful candidate"
        );
        assert!(
            !ran_resolution.load(Ordering::SeqCst),
            "reading the resolution must not enter the cold body"
        );
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK
            .with(|slot| *slot.borrow_mut() = None);

        let _higher =
            RequestContextGuard::install(RequestContext::with_kind_timing_and_projection_budget(
                99,
                Arc::from(canonical),
                verter_audit::RequestKind::ComponentMeta,
                false,
                false,
                None,
                2000,
            ));
        let ran_higher = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran_higher);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let healed = host
            .get_component_meta(canonical)
            .expect("a higher profile recomputes");
        assert!(
            ran_higher.load(Ordering::SeqCst),
            "a higher operation profile misses the retained refusal and recomputes, status {:?}",
            healed.status()
        );
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK
            .with(|slot| *slot.borrow_mut() = None);
    }

    #[test]
    fn session_resolution_and_payload_replay_retained_operation_refusal() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let host = crate::component_meta_host::ComponentMetaHost::new_standalone(
            crate::types::HostConfig {
                projection_op_budget: 1,
                ..crate::types::HostConfig::default()
            },
        );
        let canonical = "/w/budget-refusal-session.vue";
        let _ = host
            .host()
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();

        let first = host
            .host()
            .get_component_meta(canonical)
            .expect("the refused demand still returns the recovery");
        assert!(
            !first.is_complete(),
            "projection budget 1 is an operation-profile refusal, got {:?}",
            first.status()
        );

        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let misses_before = host
            .host()
            .provenance()
            .resolver_node_cache_misses
            .load(Ordering::Relaxed);

        let session = host.open_session_batch().expect("session");
        let (_analysis, resolution) = session
            .get_component_meta_with_resolution(canonical)
            .expect("session resolution replays the retained refusal")
            .expect("the refusal still has a recovery");
        assert!(
            !resolution.fact_status().is_complete(),
            "session resolution replay is the refusal, not a successful hit: {:?}",
            resolution.fact_status()
        );
        assert!(
            resolution.synthesis_should_suppress,
            "a refusal replay must not present the recovery as cacheable"
        );

        let payload = session
            .get_component_meta_payload(canonical, |_| vec![7])
            .expect("session payload replays the retained refusal")
            .expect("the refusal still has a recovery payload");
        assert_eq!(payload, vec![7]);
        assert_eq!(
            host.host()
                .provenance()
                .resolver_node_cache_misses
                .load(Ordering::Relaxed),
            misses_before,
            "session resolution and payload must not run the semantic producer"
        );
        assert!(
            !ran.load(Ordering::SeqCst),
            "session consumers must not enter the cold body"
        );
        assert_eq!(
            host.host()
                .project_type_store()
                .component_meta_results()
                .len(),
            0,
            "the refusal is not a successful candidate"
        );
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK
            .with(|slot| *slot.borrow_mut() = None);
    }

    fn budget_refusal_host(canonical: &str) -> crate::component_meta_host::ComponentMetaHost {
        let host = crate::component_meta_host::ComponentMetaHost::new_standalone(
            crate::types::HostConfig {
                projection_op_budget: 1,
                ..crate::types::HostConfig::default()
            },
        );
        let _ = host
            .host()
            .upsert(crate::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: std::sync::Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ x: string }>()</script>",
                ),
                file_language: crate::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        host
    }

    #[test]
    fn session_resolution_first_caller_seals_operation_refusal() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let host = budget_refusal_host("/w/budget-refusal-resolution-first.vue");
        let canonical = "/w/budget-refusal-resolution-first.vue";
        let session = host.open_session_batch().expect("session");
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let misses_before = host
            .host()
            .provenance()
            .resolver_node_cache_misses
            .load(Ordering::Relaxed);
        let (_analysis, resolution) = session
            .get_component_meta_with_resolution(canonical)
            .expect("first resolution")
            .expect("the refusal still has a recovery");
        assert!(
            ran.load(Ordering::SeqCst),
            "the first session resolution enters the producer"
        );
        assert!(!resolution.fact_status().is_complete());
        assert!(resolution.synthesis_should_suppress);
        assert_eq!(
            host.host()
                .project_type_store()
                .component_meta_results()
                .len(),
            0
        );
        let misses_after = host
            .host()
            .provenance()
            .resolver_node_cache_misses
            .load(Ordering::Relaxed);
        assert!(misses_after > misses_before);

        let ran_again = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran_again);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let (_analysis, again) = session
            .get_component_meta_with_resolution(canonical)
            .expect("replayed resolution")
            .expect("the refusal still has a recovery");
        assert!(
            !ran_again.load(Ordering::SeqCst),
            "the repeat must not enter the producer"
        );
        assert_eq!(again.fact_status(), resolution.fact_status());
        assert!(again.synthesis_should_suppress);
        assert_eq!(
            host.host()
                .provenance()
                .resolver_node_cache_misses
                .load(Ordering::Relaxed),
            misses_after
        );
        assert_eq!(
            host.host()
                .project_type_store()
                .component_meta_results()
                .len(),
            0
        );
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK
            .with(|slot| *slot.borrow_mut() = None);
    }

    #[test]
    fn session_payload_first_caller_seals_operation_refusal() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let host = budget_refusal_host("/w/budget-refusal-payload-first.vue");
        let canonical = "/w/budget-refusal-payload-first.vue";
        let session = host.open_session_batch().expect("session");
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let misses_before = host
            .host()
            .provenance()
            .resolver_node_cache_misses
            .load(Ordering::Relaxed);
        let payload = session
            .get_component_meta_payload(canonical, |_| vec![7])
            .expect("first payload")
            .expect("the refusal still has a recovery");
        assert_eq!(payload, vec![7]);
        assert!(
            ran.load(Ordering::SeqCst),
            "the first session payload enters the producer"
        );
        assert_eq!(
            host.host()
                .project_type_store()
                .component_meta_results()
                .len(),
            0
        );
        let misses_after = host
            .host()
            .provenance()
            .resolver_node_cache_misses
            .load(Ordering::Relaxed);
        assert!(misses_after > misses_before);

        let ran_again = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran_again);
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));
        });
        let again = session
            .get_component_meta_payload(canonical, |_| vec![9])
            .expect("replayed payload")
            .expect("the refusal still has a recovery");
        assert_eq!(again, vec![9]);
        assert!(
            !ran_again.load(Ordering::SeqCst),
            "the repeat must not enter the producer"
        );
        assert_eq!(
            host.host()
                .provenance()
                .resolver_node_cache_misses
                .load(Ordering::Relaxed),
            misses_after
        );
        assert_eq!(
            host.host()
                .project_type_store()
                .component_meta_results()
                .len(),
            0
        );
        crate::host_manage::component_meta_entry::COLD_BODY_PRE_RESOLVE_HOOK
            .with(|slot| *slot.borrow_mut() = None);
    }
}
