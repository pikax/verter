//! Bound-proof + correctness-preserved tests for the bounded
//! query-identity retention substrate.
//!
//! ## Why these tests exist
//!
//! A class of durable query-identity caches stores entries whose
//! effective identity carries self-version state — `ComponentMetaResultDb`
//! carries the owner whole-hash, `RefCycleResultDb` keys on a
//! `DeclIdentity` that embeds the file whole-hash, `MaterializeStructureDb`
//! and the `SemanticGraphStore` memo key on content-derived
//! `SemanticNodeId`s. Each distinct content edit of an owner appends a
//! fresh, permanent entry to those caches. Without a routine reclamation
//! path the caches grow monotonically with the edit count in a
//! long-lived session.
//!
//! The bounded retention substrate gives every member of that class a
//! real total cap (a global insertion-ordered budget) plus, for
//! `ComponentMetaResultDb`, a per-slot bounded candidate list. Eviction
//! is stale-first then FIFO; evicting a still-valid entry only forces a
//! recompute, never an incorrect result.
//!
//! `retention_bounds_component_meta_result_growth` DISCRIMINATES: it
//! FAILS against the pre-substrate tree (the result cache grew +1 per
//! owner edit, unbounded) and PASSES once the substrate caps it.
//! `eviction_of_valid_entry_recomputes_correct_result` proves the
//! correctness half — after the substrate evicts a *valid* entry a
//! re-query recomputes the right answer.

#![cfg(test)]

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::component_meta_result_db::ComponentMetaResultDb;
use verter_session::{CompileErrorPolicy, HostConfig};
use verter_type_expr::{PrimitiveName, TypeExpr};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

/// The named prop's evaluated `TypeExpr` from a `get_component_meta`
/// result, demand-materialized from its published source through the
/// ONE shared dispatch.
fn prop_type(
    host: &verter_session::VerterHost,
    owner: &str,
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> TypeExpr {
    let source = meta
        .props
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("missing prop `{name}`"))
        .type_source
        .present()
        .unwrap_or_else(|| panic!("prop `{name}` must publish a typed source"));
    verter_session::test_only::semantic_source_probe::demand_type_expr(host, owner, source)
        .unwrap_or_else(|| panic!("prop `{name}`'s published source must demand-materialize"))
}

/// BOUND PROOF — performing many distinct content edits of one owner
/// must NOT grow the final `ComponentMetaResultDb` monotonically with
/// the edit count.
///
/// Pre-substrate: every owner edit shifted the `owner_whole_hash` cache
/// key, so each `get_component_meta` after an edit appended a permanent
/// new entry — `component_meta_live` (the DB's live counter) grew +1 per
/// edit and never came back down. With 60 edits the counter would read
/// ~61.
///
/// Post-substrate: the owner whole-hash leaves the slot key and becomes
/// a per-candidate discriminant; the slot `(owner, options, env)` holds
/// a bounded candidate list, and the cache also enforces a global
/// insertion-ordered budget. The live entry count is bounded by the
/// candidate cap, independent of how many edits were performed.
///
/// The assertion bound is the substrate's own published per-slot cap —
/// not a magic number — so the test stays correct if the cap is tuned.
#[test]
fn retention_bounds_component_meta_result_growth() {
    let mh = metahost();
    let owner = "/src/Comp.vue";

    // 60 distinct content edits of the SAME owner. Each edit changes the
    // local prop type, so each edit produces a distinct owner content
    // hash, and each post-edit `get_component_meta` is a cold compute
    // that publishes a result-cache entry.
    let edit_count = 60usize;
    for i in 0..edit_count {
        let member_ty = if i % 2 == 0 { "number" } else { "string" };
        let src = format!(
            "<script setup lang=\"ts\">\n\
             interface LocalProps {{ value: {member_ty}; tag: \"v{i}\" }}\n\
             defineProps<LocalProps>()\n\
             </script>\n\
             <template><div/></template>\n",
        );
        mh.upsert_base(owner, &src)
            .unwrap_or_else(|e| panic!("upsert edit {i}: {e:?}"));
        let meta = mh
            .host()
            .get_component_meta(owner)
            .unwrap_or_else(|| panic!("get_component_meta after edit {i}"));
        // Sanity — the recompute reflects the latest edit.
        let expect_number = i % 2 == 0;
        assert_eq!(
            matches!(
                prop_type(mh.host(), owner, &meta, "value"),
                TypeExpr::Primitive(PrimitiveName::Number)
            ),
            expect_number,
            "edit {i}: recomputed `value` prop must reflect the latest edit",
        );
    }

    let live = mh
        .host()
        .project_type_store()
        .counters
        .component_meta_live
        .load(std::sync::atomic::Ordering::Relaxed);

    // The bound is the substrate's own per-slot candidate cap. One owner
    // + one options fingerprint = one slot; the slot retains at most
    // `PER_SLOT_CANDIDATE_CAP` candidates regardless of the 60 edits.
    let cap = ComponentMetaResultDb::<()>::PER_SLOT_CANDIDATE_CAP as u64;
    assert!(
        live <= cap,
        "bounded retention proof: after {edit_count} distinct owner edits \
         the ComponentMetaResultDb live entry count must stay bounded by \
         the per-slot candidate cap ({cap}), not grow with the edit count. \
         Observed live={live}. Pre-substrate this counter grew +1 per edit \
         (would be ~{}).",
        edit_count + 1,
    );
    // Discrimination floor — the cache is still doing its job (it is not
    // empty); the latest result is retained.
    assert!(
        live >= 1,
        "the latest owner result must still be cached — observed live={live}",
    );
}

/// CORRECTNESS PRESERVED — after the substrate evicts a *valid* entry,
/// a re-query recomputes the correct fresh result.
///
/// The substrate evicts on a stale-first then FIFO policy; evicting a
/// still-valid candidate is allowed (it only triggers a recompute). This
/// test drives enough distinct edits to force the substrate to evict the
/// candidate for an EARLY content version, then re-queries that exact
/// early version and asserts the recomputed result is correct — never a
/// stale or torn payload from a different version.
#[test]
fn eviction_of_valid_entry_recomputes_correct_result() {
    let mh = metahost();
    let owner = "/src/Comp.vue";

    // Version A — `value: number`.
    let src_a = "<script setup lang=\"ts\">\n\
                 interface LocalProps { value: number }\n\
                 defineProps<LocalProps>()\n\
                 </script>\n\
                 <template><div/></template>\n";
    mh.upsert_base(owner, src_a).expect("upsert A");
    let meta_a = mh.host().get_component_meta(owner).expect("cold A");
    assert!(
        matches!(
            prop_type(mh.host(), owner, &meta_a, "value"),
            TypeExpr::Primitive(PrimitiveName::Number)
        ),
        "version A `value` prop must be `number`",
    );

    // Drive many MORE distinct versions so the substrate's per-slot
    // candidate cap forces version A's candidate out (stale-first /
    // FIFO eviction). Each version is byte-distinct.
    for i in 0..40usize {
        let src = format!(
            "<script setup lang=\"ts\">\n\
             interface LocalProps {{ value: boolean; n: \"x{i}\" }}\n\
             defineProps<LocalProps>()\n\
             </script>\n\
             <template><div/></template>\n",
        );
        mh.upsert_base(owner, &src)
            .unwrap_or_else(|e| panic!("upsert filler {i}: {e:?}"));
        let _ = mh.host().get_component_meta(owner);
    }

    // Re-upsert version A's exact bytes. Its candidate was evicted by the
    // bounded substrate, so this is a cold recompute — and the recompute
    // must produce the CORRECT result for version A (`value: number`),
    // never a stale `boolean` leaked from a filler version.
    mh.upsert_base(owner, src_a).expect("re-upsert A");
    let meta_a2 = mh
        .host()
        .get_component_meta(owner)
        .expect("recompute after eviction of version A");
    assert!(
        matches!(
            prop_type(mh.host(), owner, &meta_a2, "value"),
            TypeExpr::Primitive(PrimitiveName::Number)
        ),
        "after the bounded substrate evicted version A's candidate, the \
         re-query MUST recompute the correct `number` type for version A — \
         eviction triggers a recompute, never a stale or wrong result. \
         Got {:?}",
        prop_type(mh.host(), owner, &meta_a2, "value"),
    );
}
