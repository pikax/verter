//! Sub-task H — closure-boundary invalidation.
//!
//! R14 + R28 contract: a consumer that observes a `Member` /
//! `MemberPresence` / `ImportRef` fact across a closure boundary
//! (e.g. `import type { X } from './x'; type Props = Pick<X, 'a'>`)
//! is invalidated ONLY when the corresponding observed fact
//! changes — NOT when an unrelated member of the imported file
//! changes.
//!
//! The substrate this test exercises: `ValidatedFactCache`'s
//! per-candidate `fact_dep_signature` revalidation against a
//! `StoreView`. We construct a synthetic view that toggles which
//! facts validate, then assert the cache reuse / invalidation
//! decision reflects exactly the observed-fact set.

use rustc_hash::FxHashSet;

use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};
use verter_session::resolver_core::{
    FactVersionRef, ParseFactRef, StoreView, StoreViewCompatToken, ValidatedFactCache,
};
use verter_session::semantic_query::HashValue;

/// Synthetic store view: a fact `validates` iff its
/// `FactVersionRef` is in `valid_facts`. The `compat_token` is a
/// fixed identifier so concurrency / lane behaviour stays
/// isolated.
#[derive(Debug)]
struct TestView {
    token: StoreViewCompatToken,
    valid_facts: FxHashSet<FactVersionRef>,
}

impl StoreView for TestView {
    fn compat_token(&self) -> StoreViewCompatToken {
        self.token
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        self.valid_facts.contains(fact)
    }
}

fn make_token() -> StoreViewCompatToken {
    StoreViewCompatToken {
        epoch: 1,
        session: None,
        validity_fingerprint: 0,
    }
}

fn fake_fact(canonical: &str, name: &str, byte: u8) -> FactVersionRef {
    let mut h = [0u8; 16];
    h[0] = byte;
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical.to_string(),
        key: FactKey::Member {
            exporter: "X".into(),
            name: name.into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: h,
    })
}

fn import_ref_fact(specifier: &str) -> FactVersionRef {
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/w/owner.ts".to_string(),
        key: FactKey::ImportRef {
            specifier: specifier.into(),
            binding: "X".into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: HashValue::default(),
    })
}

/// Editing an imported file's body invalidates ONLY consumers that
/// recorded the corresponding member fact through the import. A
/// consumer that observed `Member(X, "a")` is invalidated when the
/// imported `X.a` body changes; a consumer that observed only
/// `Member(X, "b")` is NOT invalidated.
#[test]
fn editing_imported_member_a_invalidates_only_consumers_of_member_a() {
    let cache: ValidatedFactCache<&'static str, &'static str> = ValidatedFactCache::default();

    // Two consumers, each observes a DIFFERENT member of the
    // imported file `/w/x.ts`.
    let member_a_pre = fake_fact("/w/x.ts", "a", 0x01);
    let member_b_pre = fake_fact("/w/x.ts", "b", 0x02);
    let import_x = import_ref_fact("./x");

    cache.insert(
        "consumer_a",
        "uses Pick<X, 'a'>",
        vec![member_a_pre.clone(), import_x.clone()],
    );
    cache.insert(
        "consumer_b",
        "uses Pick<X, 'b'>",
        vec![member_b_pre.clone(), import_x.clone()],
    );

    // Pre-edit: both consumers validate against the original
    // expected hashes.
    let pre_view = TestView {
        token: make_token(),
        valid_facts: [member_a_pre.clone(), member_b_pre.clone(), import_x.clone()]
            .into_iter()
            .collect(),
    };
    assert!(cache.get_if_valid(&"consumer_a", &pre_view).is_some());
    assert!(cache.get_if_valid(&"consumer_b", &pre_view).is_some());

    // Post-edit: `Member(X, "a")` body changed → its expected
    // hash differs. `Member(X, "b")` unchanged. The import ref
    // unchanged. The view no longer validates the OLD
    // `member_a_pre` (because the underlying file's content
    // changed); it validates `member_b_pre` and `import_x` still.
    let post_view = TestView {
        token: make_token(),
        valid_facts: [member_b_pre.clone(), import_x.clone()]
            .into_iter()
            .collect(),
    };

    // Discrimination: consumer_a INVALIDATED, consumer_b PRESERVED.
    assert!(
        cache.get_if_valid(&"consumer_a", &post_view).is_none(),
        "consumer_a (Pick<X, 'a'>) MUST be invalidated when the imported X.a body \
         changes — its observed Member(X, 'a') no longer validates"
    );
    assert!(
        cache.get_if_valid(&"consumer_b", &post_view).is_some(),
        "consumer_b (Pick<X, 'b'>) MUST be preserved when X.a (not X.b) changes — \
         the closure-boundary observation is path-precise"
    );
}

/// Editing the imported file's `ImportRef` (rename / reorder of the
/// import specifier on the consumer side) invalidates ALL
/// consumers that observed the import — but does NOT touch
/// consumers in OTHER files that have their own imports of the
/// same target.
#[test]
fn editing_import_ref_does_not_invalidate_unrelated_consumers() {
    let cache: ValidatedFactCache<&'static str, &'static str> = ValidatedFactCache::default();

    // Two consumers, each in a DIFFERENT owner file with its own
    // import of `./x`. They share the *target* but their own
    // `ImportRef` facts are distinct (different canonical id).
    let member_a = fake_fact("/w/x.ts", "a", 0x01);
    let import_x_from_owner_1 = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/w/owner1.ts".to_string(),
        key: FactKey::ImportRef {
            specifier: "./x".into(),
            binding: "X".into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: HashValue::default(),
    });
    let import_x_from_owner_2 = FactVersionRef::Parse(ParseFactRef {
        canonical_id: "/w/owner2.ts".to_string(),
        key: FactKey::ImportRef {
            specifier: "./x".into(),
            binding: "X".into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: HashValue::default(),
    });

    cache.insert(
        "consumer_owner_1",
        "uses Pick<X, 'a'>",
        vec![member_a.clone(), import_x_from_owner_1.clone()],
    );
    cache.insert(
        "consumer_owner_2",
        "uses Pick<X, 'a'>",
        vec![member_a.clone(), import_x_from_owner_2.clone()],
    );

    // Edit only owner1's import (e.g. they renamed the binding).
    let post_view = TestView {
        token: make_token(),
        valid_facts: [member_a.clone(), import_x_from_owner_2.clone()]
            .into_iter()
            .collect(),
    };

    // Discrimination: owner1 invalidated, owner2 preserved.
    assert!(
        cache
            .get_if_valid(&"consumer_owner_1", &post_view)
            .is_none(),
        "consumer_owner_1 MUST be invalidated when its OWN ImportRef changes"
    );
    assert!(
        cache
            .get_if_valid(&"consumer_owner_2", &post_view)
            .is_some(),
        "consumer_owner_2 MUST be preserved when owner1's ImportRef (not its own) \
         changes — closure-boundary observations are per-importer"
    );
}
