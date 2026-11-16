//! Membership work and allocator traffic for one signature with many roots.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_session_query::facts::{
    fact_cache::{FactVersionRef, ResultReceipt},
    policy::FactPolicy,
};

use super::{alloc_bytes, alloc_count, reset_alloc_counter};

#[test]
fn strict_root_validation_work_and_allocations_grow_linearly() {
    for size in [16usize, 32, 48] {
        let host = VerterHost::new_standalone(HostConfig::default());
        let roots: Vec<_> = (0..size).map(|i| format!("/roots/{i:03}.ts")).collect();
        let mut facts = Vec::new();
        for root in &roots {
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: None,
                    input_id: root.clone(),
                    source: Arc::from("export const value = 1;"),
                    file_language: FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("root source");
            facts.push(FactVersionRef::FileWholeHash {
                canonical_id: root.as_str().into(),
                hash: verter_session::for_tests::validation_whole_hash_for_tests(&host, root)
                    .expect("tracked root hash"),
            });
        }
        // Every receipt has one dependency outside the strict roots. The
        // entire root population must not be scanned for each receipt.
        for i in 0..size {
            facts.push(FactVersionRef::Receipt(ResultReceipt::new(vec![
                FactVersionRef::FileWholeHash {
                    canonical_id: format!("/dependencies/{i:03}.ts").into(),
                    hash: [7; 16],
                },
            ])));
        }
        let facts = FactPolicy::production().set_of(facts);
        let roots: Vec<_> = roots.iter().map(String::as_str).collect();
        verter_session::for_tests::with_request_validation_for_tests(&host, |validate, work| {
            let before = work();
            let indexes_before = verter_session::for_tests::root_index_builds_for_tests();
            reset_alloc_counter();
            let valid = validate(&facts, &roots);
            let allocations = alloc_count();
            let bytes = alloc_bytes();
            let comparisons = work() - before;
            assert_eq!(
                verter_session::for_tests::root_index_builds_for_tests() - indexes_before,
                1
            );
            assert!(valid, "unchanged roots and lazy dependencies validate");
            assert!(comparisons >= size, "membership work was measured");
            assert!(
                comparisons <= 8 * size + 16,
                "{size} roots used {comparisons} membership operations"
            );
            assert!(
                allocations > 0 && allocations <= 12 * size as u64 + 32,
                "{size} roots allocated {allocations} times"
            );
            assert!(
                bytes > 0 && bytes <= 2048 * size as u64 + 2048,
                "{size} roots allocated {bytes} bytes"
            );
            assert!(validate(&facts, &roots), "retained verdict agrees");
            eprintln!(
                "roots={size} membership={comparisons} allocations={allocations} bytes={bytes}"
            );
        });
    }
}

#[test]
fn strict_root_verdicts_do_not_copy_disjoint_root_lists() {
    for size in [1usize, 7, 16, 32, 48] {
        let host = VerterHost::new_standalone(HostConfig::default());
        let facts = FactPolicy::production().set_of(vec![FactVersionRef::Receipt(
            ResultReceipt::new(vec![FactVersionRef::FileWholeHash {
                canonical_id: "/untracked/dependency.ts".into(),
                hash: [7; 16],
            }]),
        )]);
        let roots: Vec<_> = (0..size).map(|i| format!("/irrelevant/{i}.ts")).collect();
        let other_roots: Vec<_> = (0..size).map(|i| format!("/other/{i}.ts")).collect();
        let roots: Vec<_> = roots.iter().map(String::as_str).collect();
        let other_roots: Vec<_> = other_roots.iter().map(String::as_str).collect();
        verter_session::for_tests::with_request_validation_for_tests(&host, |validate, _| {
            reset_alloc_counter();
            let valid = validate(&facts, &roots);
            let allocations = alloc_count();
            let bytes = alloc_bytes();
            assert!(valid);
            let max_allocations = if size <= 7 { 8 } else { 12 };
            assert!(
                allocations <= max_allocations,
                "{size} disjoint roots allocated {allocations} times ({bytes} bytes)"
            );
            assert!(
                bytes <= 128 * size as u64 + 4096,
                "{size} disjoint roots allocated {bytes} bytes"
            );
            assert!(validate(&facts, &other_roots));
            let reused_allocations = alloc_count() - allocations;
            let reused_bytes = alloc_bytes() - bytes;
            // Small contexts use the ordinary receipt walk, whose visited
            // set costs one fixed block even when its receipt is settled.
            // Neither count nor bytes may grow with the disjoint root list.
            assert!(
                reused_allocations <= 1 && reused_bytes <= 128,
                "{size} disjoint roots allocated {reused_allocations} times ({reused_bytes} bytes) on an empty-context hit"
            );
            assert!(
                !validate(&facts, &["/untracked/dependency.ts", "/unrelated.ts"]),
                "a root inside the receipt must reject the lazy verdict"
            );
            eprintln!(
                "disjoint_roots={size} allocations={allocations} bytes={bytes} reused_allocations={reused_allocations} reused_bytes={reused_bytes}"
            );
        });
    }
}
