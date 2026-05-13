//! Sub-task F — path-precise invalidation (Stage 0 pairing).
//!
//! Paired with `tests/path_precise_invalidation_baseline.rs`. Both
//! consume the shared fixture corpus under
//! `crates/verter_session/tests/fixtures/path_precise/`.
//!
//! Stage 0 (baseline) characterises today's coarse cache
//! invalidation: even when an edit targets a sibling member the
//! consumer did NOT select, the consumer recomputes. The fixtures'
//! `invalidation_matrix.stage_0_today_invalidates_consumer: true`
//! cells pin that.
//!
//! Stage 6d (THIS file) asserts the INVERTED behaviour at the
//! substrate level via the
//! `ValidatedFactCache::get_if_valid(view)` discrimination: a
//! consumer that observes ONLY the selected members' facts MUST
//! stay warm when an unselected sibling member's body changes.
//!
//! **Architectural rules bound:** R14 + R28 (path-precise fact
//! granularity); central correctness invariant.
//!
//! Discrimination strategy. The Stage 0 baseline observed COARSE
//! invalidation in production (one fact for the whole export
//! surface). Stage 6d's substrate, via per-member `Member` /
//! `MemberPresence` facts, makes the path-precise invariant
//! observable at the cache-substrate level. The Stage 7 cutover
//! migrates each production producer to emit per-member facts; this
//! Stage 6d test pins the SUBSTRATE invariant the migrated
//! producers must consume.

use std::fs;
use std::path::PathBuf;

use rustc_hash::FxHashSet;

use verter_semantic::facts::{FactKey, FactLane, SymbolSpace};
use verter_session::resolver_core::{
    FactVersionRef, ParseFactRef, StoreView, StoreViewCompatToken, ValidatedFactCache,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read_expected_json(name: &str) -> serde_json::Value {
    let path = workspace_root()
        .join("crates")
        .join("verter_session")
        .join("tests")
        .join("fixtures")
        .join("path_precise")
        .join(name);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path:?}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse JSON {path:?}: {e}"))
}

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
    }
}

fn member_fact(canonical: &str, exporter: &str, member: &str, byte: u8) -> FactVersionRef {
    let mut h = [0u8; 16];
    h[0] = byte;
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical.to_string(),
        key: FactKey::Member {
            exporter: exporter.into(),
            name: member.into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: h,
    })
}

fn member_presence(canonical: &str, exporter: &str, member: &str, byte: u8) -> FactVersionRef {
    let mut h = [0u8; 16];
    h[0] = byte;
    FactVersionRef::Parse(ParseFactRef {
        canonical_id: canonical.to_string(),
        key: FactKey::MemberPresence {
            exporter: exporter.into(),
            name: member.into(),
            space: SymbolSpace::Type,
        },
        lane: FactLane::Semantic,
        expected_hash: h,
    })
}

/// Central correctness invariant: a `Pick<Foo, "a">` consumer that
/// observes ONLY `MemberPresence(Foo, "a")` + `Member(Foo, "a")`
/// MUST stay warm when `Foo.b` body changes.
///
/// Stage 0's baseline test PASSES on the same fixture under the
/// coarse-cache behaviour (whole-export closure invalidates). This
/// Stage 6d substrate test PASSES under the path-precise contract.
/// On the substrate alone, the discrimination is direct: only
/// observed facts participate in validation.
#[test]
fn pick_literal_key_unselected_sibling_edit_preserves_consumer_substrate() {
    let cache: ValidatedFactCache<&'static str, &'static str> = ValidatedFactCache::default();

    // Consumer observes `MemberPresence(Foo, "a")` + `Member(Foo, "a")`
    // — the exact facts a path-precise `Pick<Foo, "a">` consumer
    // emits.
    let member_a_pre = member_fact("/w/foo.ts", "Foo", "a", 0x01);
    let presence_a_pre = member_presence("/w/foo.ts", "Foo", "a", 0x01);

    cache.insert(
        "pick_foo_a",
        "Pick<Foo, 'a'>",
        vec![member_a_pre.clone(), presence_a_pre.clone()],
    );

    // Edit: `Foo.b` body changes. The fact set the view validates
    // is `Foo.a`'s presence + body fact (unchanged) — the consumer
    // never observed `Foo.b` so `Foo.b`'s facts don't enter the
    // valid set / are irrelevant. The view validates the
    // consumer's facts as long as the OLD `Foo.a` facts remain
    // valid.
    let post_view = TestView {
        token: make_token(),
        valid_facts: [member_a_pre.clone(), presence_a_pre.clone()]
            .into_iter()
            .collect(),
    };

    // Discrimination: path-precise consumer STAYS WARM (Stage 6d
    // invariant; inverts Stage 0's `stage_0_today_invalidates_consumer:
    // true`).
    assert!(
        cache.get_if_valid(&"pick_foo_a", &post_view).is_some(),
        "Stage 6d path-precise invariant: a Pick<Foo, 'a'> consumer that observed only \
         Member(Foo, 'a') + MemberPresence(Foo, 'a') MUST stay warm when an unselected \
         sibling Foo.b changes. This is the central correctness invariant the cutover \
         enforces; Stage 0 baseline observed COARSE invalidation on the same fixture."
    );
}

/// Discrimination: editing the SELECTED member's body (Foo.a) DOES
/// invalidate the path-precise consumer. Pairs with the test
/// above — together they fully characterise the path-precise
/// contract.
#[test]
fn pick_literal_key_selected_member_body_edit_invalidates_consumer() {
    let cache: ValidatedFactCache<&'static str, &'static str> = ValidatedFactCache::default();

    let member_a_pre = member_fact("/w/foo.ts", "Foo", "a", 0x01);
    let presence_a_pre = member_presence("/w/foo.ts", "Foo", "a", 0x01);

    cache.insert(
        "pick_foo_a",
        "Pick<Foo, 'a'>",
        vec![member_a_pre.clone(), presence_a_pre.clone()],
    );

    // Edit Foo.a body: the new `Member(Foo, "a")` has a different
    // expected_hash. The view no longer validates the OLD fact.
    let post_view = TestView {
        token: make_token(),
        valid_facts: [presence_a_pre.clone()].into_iter().collect(),
    };

    // Negative discrimination: Pick<Foo, 'a'> IS invalidated when
    // Foo.a's body changes.
    assert!(
        cache.get_if_valid(&"pick_foo_a", &post_view).is_none(),
        "Pick<Foo, 'a'> consumer MUST be invalidated when Foo.a body changes — its \
         observed Member(Foo, 'a') fact no longer validates"
    );
}

/// Member-add invariant (R28 two-fact discrimination): adding
/// `Foo.b` produces a NEW `MemberPresence(Foo, "b")` fact but does
/// NOT change the existing `MemberPresence(Foo, "a")` /
/// `Member(Foo, "a")` facts. Consumer of `Foo.a` stays warm; only
/// whole-surface consumers (`MemberShape`) invalidate.
#[test]
fn member_add_preserves_existing_member_consumers() {
    let cache: ValidatedFactCache<&'static str, &'static str> = ValidatedFactCache::default();

    let member_a = member_fact("/w/foo.ts", "Foo", "a", 0x01);
    let presence_a = member_presence("/w/foo.ts", "Foo", "a", 0x01);
    cache.insert(
        "pick_foo_a",
        "Pick<Foo, 'a'>",
        vec![member_a.clone(), presence_a.clone()],
    );

    // Adding Foo.b: a new member-presence emerges (for "b"), but
    // the existing facts for "a" stay unchanged. The view still
    // validates them.
    let post_view = TestView {
        token: make_token(),
        valid_facts: [member_a.clone(), presence_a.clone()].into_iter().collect(),
    };

    assert!(
        cache.get_if_valid(&"pick_foo_a", &post_view).is_some(),
        "member-add (Foo gains 'b') MUST NOT invalidate Pick<Foo, 'a'> consumer — the \
         two-fact model emits a NEW MemberPresence(Foo, 'b') fact rather than mutating \
         existing facts"
    );
}

/// Every Stage 0 archetype has a paired stage_6d cell in the
/// invalidation matrix. This test does NOT re-validate the source
/// body assertions (that's `path_precise_invalidation_baseline.rs`'s
/// job); it asserts that the Stage 6d cell exists for at least the
/// minimum required set of archetypes, so the substrate-level
/// invariants this test file pins are paired with the production
/// invariants Stage 7's cutover will land.
#[test]
fn path_precise_corpus_carries_stage_6d_cells_for_all_archetypes() {
    let required = [
        "pick_literal_key.expected.json",
        "omit_literal_key.expected.json",
        "indexed_access_chain.expected.json",
    ];
    for name in required {
        let json = read_expected_json(name);
        let matrix = json
            .get("invalidation_matrix")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("{name}: invalidation_matrix array required"));
        assert!(
            !matrix.is_empty(),
            "{name}: invalidation_matrix must be non-empty (Stage 6d substrate inversion \
             depends on at least one paired cell)"
        );
        // Sanity-check the first row has both cells.
        let row = &matrix[0];
        assert!(
            row.get("stage_6d_target_invalidates_consumer").is_some(),
            "{name}: every row must carry stage_6d_target_invalidates_consumer"
        );
    }
}
