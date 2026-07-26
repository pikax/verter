//! Parse-time fact emission performs **no repeated inventory traversal**.
//!
//! # What this guard claims, exactly
//!
//! The emitter reads its shallow inventory through a sealed
//! `FactEmissionView`. Whole-inventory operations (iterating the type /
//! value / enum symbol tables, the export map, the import map, the
//! wildcard list, the augmentation tables) are counted as TRAVERSALS;
//! per-symbol O(1) map probes are counted separately as POINT LOOKUPS.
//!
//! The invariant: **the traversal count does not grow with the
//! declaration count.** The emitter takes a fixed number of passes over
//! the inventory — one per emitter section — regardless of how many
//! declarations the file has. A walk that re-scans the inventory once per
//! declaration turns that fixed number into `N`, and this guard fails.
//!
//! Point lookups are asserted to scale linearly in the same measurement,
//! which pins the other half of the shape: the emitter visits each
//! declaration a constant number of times.
//!
//! # What this guard does NOT claim
//!
//! It is **not** a total-work or asymptotic-complexity guarantee, and it
//! must not be described as one. It bounds how many times the emitter
//! walks its inventory; it does not bound arbitrary computation over data
//! already collected. A quadratic loop over an already-materialised
//! `Vec` would not move either counter. That residue is covered — as a
//! reported, non-asserted wall-clock measurement — by
//! `crates/verter_bench/benches/fact_emission_scaling.rs`.
//!
//! The three deterministic rails and their separate, narrower claims:
//!
//! | rail | claims |
//! |---|---|
//! | `fact_emission_output_cardinality.rs` | emitted fact CARDINALITY is affine in the declaration count |
//! | `tests/allocator_canaries.rs` | ALLOCATION volume stays in the linear class |
//! | this file | no repeated inventory TRAVERSAL |
//!
//! None of them measures elapsed time, so none of them can flake under
//! machine load.
//!
//! # Why the seal is structural, not a convention
//!
//! `FactEmissionView` keeps its `ShallowFileState` reference in a field
//! that is private to its own child module. No walk function in
//! `fact_emission` can read it — the compiler rejects the attempt with
//! `error[E0616]: field 'shallow' of struct 'FactEmissionView' is
//! private` — and no walk function is handed a `ShallowFileState` value.
//! So a walk cannot reach the raw inventory by accident; doing so
//! requires changing a function signature, which is a visible act in
//! review rather than a one-line slip.
//!
//! Bounded residue, stated rather than glossed: `emit_parse_facts` itself
//! holds the `&IndexedReady` in order to BUILD the view, so the seal
//! binds the walk functions, not that one entry point.
//!
//! # Mutation that must turn this RED
//!
//! Replace the constant-time header lookup in `emit_type_symbol_headers`
//! (`crates/verter_session/src/fact_emission.rs`) with a linear scan of
//! the symbol inventory inside the per-declaration loop:
//!
//! ```ignore
//! for name in sorted {
//!     let exporter = InternedName::from(name);
//!     // MUTATION: re-scan the whole inventory per declaration.
//!     let headers = if view.type_symbol_names().any(|other| other == name) {
//!         view.type_member_headers(name).unwrap_or(&[])
//!     } else {
//!         &[]
//!     };
//!     emit_member_shape_facts(registry, name, &exporter, SymbolSpace::Type, headers);
//! }
//! ```
//!
//! That mutation allocates nothing and emits exactly the same facts, so
//! the cardinality guard and the allocator canary both stay green. This
//! guard fails: traversals go from a constant to `N + constant`.

use std::sync::Arc;

use verter_session::fact_emission::emit_parse_facts_with_inventory_counts_for_test;
use verter_session::project_type_store::IndexedReady;
use verter_session::resolver_core::shallow_file_state::ShallowFileState;

fn build_large_indexed(decl_count: usize) -> Arc<IndexedReady> {
    let mut source = String::with_capacity(decl_count * 48);
    for i in 0..decl_count {
        source.push_str(&format!("export interface Decl{i} {{ a: string }}\n"));
    }
    let shallow =
        ShallowFileState::service_backed_for_test_with_hash("/large.ts", &source, [0u8; 16]);
    Arc::new(IndexedReady::new_for_test_with_state(
        [0u8; 16],
        shallow,
        Arc::from(source.as_str()),
        Arc::from(source.as_str()),
    ))
}

/// `(inventory_traversals, point_lookups)` for exactly one
/// `emit_parse_facts` call on a file of `decl_count` declarations.
fn access_counts(decl_count: usize) -> (u32, u32) {
    let indexed = build_large_indexed(decl_count);
    // Warm first: settle any one-time lazy initialisation so the recorded
    // call is the steady state. The tally is per-call, so the warm call's
    // counts are discarded with it.
    let _ = emit_parse_facts_with_inventory_counts_for_test(&indexed);
    let (_, counts) = emit_parse_facts_with_inventory_counts_for_test(&indexed);
    (counts.inventory_traversals, counts.point_lookups)
}

#[test]
fn fact_emission_performs_no_repeated_inventory_traversal() {
    const N: usize = 2_000;

    let (traversals_n, points_n) = access_counts(N);
    let (traversals_2n, points_2n) = access_counts(2 * N);

    eprintln!(
        "fact-emission inventory access — emit({N}): {traversals_n} traversals / {points_n} point \
         lookups; emit({}): {traversals_2n} traversals / {points_2n} point lookups",
        2 * N,
    );

    // ANTI-VACUITY. Equality between two zeros would prove nothing: the
    // emitter must actually be reading its inventory through the counted
    // view, and must actually be visiting each declaration.
    assert!(
        traversals_n > 0,
        "anti-vacuity: emitting facts for {N} declarations recorded ZERO inventory traversals. \
         The emitter is not reading through the counted view, so the traversal invariant below \
         would pass vacuously.",
    );
    assert!(
        points_n >= N as u32,
        "anti-vacuity: emitting facts for {N} declarations recorded only {points_n} point \
         lookups (expected at least one per declaration). Either the walk is not visiting each \
         declaration or it is not going through the counted view.",
    );

    // THE INVARIANT. A fixed number of passes over the inventory,
    // independent of file size. Doubling the declaration count must not
    // add a single traversal — a walk that re-scans the inventory per
    // declaration reports `N` of them here.
    assert_eq!(
        traversals_2n,
        traversals_n,
        "emit_parse_facts must perform NO REPEATED INVENTORY TRAVERSAL: the number of passes \
         over the shallow inventory must not depend on the declaration count, but doubling it \
         from {N} to {} changed the traversal count from {traversals_n} to {traversals_2n}. A \
         walk that re-scans the whole inventory once per declaration produces exactly this \
         signature. (Point lookups, which are O(1) probes and SHOULD scale, went {points_n} -> \
         {points_2n}.)",
        2 * N,
    );

    // The complementary half of the shape: per-declaration visits stay
    // constant, so point lookups scale linearly rather than super-linearly.
    // Stated as a band rather than an equality because the emitter probes
    // several tables per declaration and a legitimate change may add one.
    assert!(
        points_2n <= points_n.saturating_mul(3),
        "per-declaration point lookups must scale LINEARLY: doubling the declaration count from \
         {N} to {} should roughly double the probe count ({points_n} -> {points_2n}), but the \
         growth crossed the 3x linear/quadratic class boundary.",
        2 * N,
    );
}

#[test]
fn inventory_access_counts_are_repeatable() {
    // The property that makes this a correctness-suite citizen: the
    // measurement is a function of the input alone. No wall-clock
    // measurement can pass this.
    const N: usize = 1_000;
    let indexed = build_large_indexed(N);
    let _ = emit_parse_facts_with_inventory_counts_for_test(&indexed);

    let (_, first) = emit_parse_facts_with_inventory_counts_for_test(&indexed);
    let (_, second) = emit_parse_facts_with_inventory_counts_for_test(&indexed);
    let (_, third) = emit_parse_facts_with_inventory_counts_for_test(&indexed);

    assert!(
        first.inventory_traversals > 0 && first.point_lookups > 0,
        "anti-vacuity: recorded no inventory access for {N} declarations ({first:?}) — equality \
         across three empty measurements would prove nothing",
    );
    assert_eq!(
        second, first,
        "inventory access for one emit_parse_facts call must be a function of the input alone: \
         run 1 was {first:?}, run 2 was {second:?}",
    );
    assert_eq!(
        third, first,
        "inventory access for one emit_parse_facts call must be a function of the input alone: \
         run 1 was {first:?}, run 3 was {third:?}",
    );
}
