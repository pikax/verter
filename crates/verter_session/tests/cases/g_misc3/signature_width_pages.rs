//! A `ValidatedFactCache` candidate's signature is never refused for its
//! width: at, around and well beyond one evidence page the candidate is
//! admitted whole, warm reads validate every fact on every page, and an edit
//! to a fact on ANY page — the last one included — misses the warm read.

use rustc_hash::FxHashSet;
use std::sync::Arc;

use verter_session::resolver_core::ValidatedFactCache;
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::fact_read_set::FACT_PAGE_WIDTH;
use verter_session_query::facts::store_view::{StoreView, StoreViewCompatToken};

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
/// their indices do, so fact `i` sits on page `i / FACT_PAGE_WIDTH` of the
/// sealed signature and the last facts sit on its last, partial page.
fn wide_facts(prefix: &str, count: usize) -> Vec<FactVersionRef> {
    let facts: Vec<FactVersionRef> = (0..count)
        .map(|i| FactVersionRef::FileWholeHash {
            canonical_id: format!("/src/{prefix}_{i:07}.ts"),
            hash: [(i % 256) as u8; 16],
        })
        .collect();
    assert!(
        facts.windows(2).all(|pair| pair[0] < pair[1]),
        "fixture: input order is the canonical order the pages are cut in"
    );
    facts
}

#[test]
fn signatures_around_and_beyond_one_page_are_admitted_whole() {
    let cache = ValidatedFactCache::<String, usize>::default();
    for (value, width) in [
        FACT_PAGE_WIDTH - 1,
        FACT_PAGE_WIDTH,
        FACT_PAGE_WIDTH + 1,
        3 * FACT_PAGE_WIDTH + 5,
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
            Some(Arc::new(value)),
            "{width}: an unchanged world serves the candidate warm"
        );
    }
}

/// An edit to the first fact, the facts either side of every page boundary,
/// and the first and very last fact of the last, partial page each miss the
/// warm read — paging never lets a fact go unvalidated.
#[test]
fn an_edit_on_any_page_including_the_last_misses_the_warm_read() {
    let cache = ValidatedFactCache::<String, usize>::default();
    let width = 3 * FACT_PAGE_WIDTH + 5;
    let facts = wide_facts("edit", width);
    cache.insert("edit".to_string(), 7, facts.clone());
    for edited in [
        0,
        FACT_PAGE_WIDTH - 1,
        FACT_PAGE_WIDTH,
        2 * FACT_PAGE_WIDTH - 1,
        2 * FACT_PAGE_WIDTH,
        3 * FACT_PAGE_WIDTH - 1,
        3 * FACT_PAGE_WIDTH,
        width - 1,
    ] {
        let mut valid: FxHashSet<FactVersionRef> = facts.iter().cloned().collect();
        valid.remove(&facts[edited]);
        let view = TestView { valid_facts: valid };
        assert!(
            cache.get_if_valid(&"edit".to_string(), &view).is_none(),
            "an edit to fact {edited} (page {}) must miss the warm read",
            edited / FACT_PAGE_WIDTH
        );
    }
}
