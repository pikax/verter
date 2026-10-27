//! A `ValidatedFactCache` candidate's signature is never refused for its
//! width: at, around and well beyond `WIDE_SIGNATURE_WIDTH` the candidate is
//! admitted whole, warm reads validate every fact, and an edit to ANY fact —
//! the last in canonical order included — misses the warm read.

use rustc_hash::FxHashSet;

use verter_session::resolver_core::ValidatedFactCache;
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::store_view::{StoreView, StoreViewCompatToken};

/// The production fresh-slice width these fixtures straddle.
const WIDE_SIGNATURE_WIDTH: usize =
    verter_session_query::facts::policy::FactPolicy::production().fresh_slice_max();

#[derive(Debug)]
struct TestView {
    valid_facts: FxHashSet<FactVersionRef>,
}

impl StoreView for TestView {
    fn compat_token(&self) -> StoreViewCompatToken {
        StoreViewCompatToken {
            epoch: 1,
            session: None,
            validity_fingerprint: 0,
        }
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        self.valid_facts.contains(fact)
    }
}

/// `count` facts in their canonical order: the zero-padded names sort as
/// their indices do, so fact `i` is the `i`-th fact of the sealed signature
/// and the last index is its last fact in canonical order.
fn wide_facts(prefix: &str, count: usize) -> Vec<FactVersionRef> {
    let facts: Vec<FactVersionRef> = (0..count)
        .map(|i| FactVersionRef::FileWholeHash {
            canonical_id: format!("/src/{prefix}_{i:07}.ts").into(),
            hash: [(i % 256) as u8; 16],
        })
        .collect();
    assert!(
        facts.windows(2).all(|pair| pair[0] < pair[1]),
        "fixture: input order is the canonical order"
    );
    facts
}

#[test]
fn signatures_around_and_beyond_the_wide_width_are_admitted_whole() {
    let cache = ValidatedFactCache::<String, usize>::default();
    for (value, width) in [
        WIDE_SIGNATURE_WIDTH - 1,
        WIDE_SIGNATURE_WIDTH,
        WIDE_SIGNATURE_WIDTH + 1,
        3 * WIDE_SIGNATURE_WIDTH + 5,
    ]
    .into_iter()
    .enumerate()
    {
        let key = format!("width_{width}");
        let facts = wide_facts(&key, width);
        cache.insert(key.clone(), value, facts.clone());
        let view = TestView {
            valid_facts: facts.iter().cloned().collect(),
        };
        assert_eq!(
            cache.get_if_valid(&key, &view),
            Some(verter_retention::heap_size::OwnedValue::new(value)),
            "{width}: an unchanged world serves the candidate warm"
        );
    }
}

/// An edit to the first fact, the facts either side of every multiple of
/// `WIDE_SIGNATURE_WIDTH` in canonical order, and the very last fact each
/// miss the warm read — width never lets a fact go unvalidated.
#[test]
fn an_edit_to_any_fact_including_the_last_misses_the_warm_read() {
    let cache = ValidatedFactCache::<String, usize>::default();
    let width = 3 * WIDE_SIGNATURE_WIDTH + 5;
    let facts = wide_facts("edit", width);
    cache.insert("edit".to_string(), 7, facts.clone());
    for edited in [
        0,
        WIDE_SIGNATURE_WIDTH - 1,
        WIDE_SIGNATURE_WIDTH,
        2 * WIDE_SIGNATURE_WIDTH - 1,
        2 * WIDE_SIGNATURE_WIDTH,
        3 * WIDE_SIGNATURE_WIDTH - 1,
        3 * WIDE_SIGNATURE_WIDTH,
        width - 1,
    ] {
        let mut valid: FxHashSet<FactVersionRef> = facts.iter().cloned().collect();
        valid.remove(&facts[edited]);
        let view = TestView { valid_facts: valid };
        assert!(
            cache.get_if_valid(&"edit".to_string(), &view).is_none(),
            "an edit to fact {edited} (run {} of the wide width) must miss the warm read",
            edited / WIDE_SIGNATURE_WIDTH
        );
    }
}
