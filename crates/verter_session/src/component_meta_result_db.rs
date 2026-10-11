//! Session publication policy for the engine-owned final component store.

use std::sync::Arc;
use verter_session_query::analysis::types::Hash16;
pub use verter_type_engine::component_meta_result_db::{
    footprint, ComponentMetaOptionsFingerprint, ComponentMetaResultDb, ComponentMetaResultEntry,
    ComponentMetaResultKey,
};

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
}
