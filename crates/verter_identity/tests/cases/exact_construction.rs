//! Exact-size canonical construction: the shared framing contract and the
//! bounded-work oracle.
//!
//! The exact-size path is a *construction* contract, not a second encoder:
//! one borrowed descriptor drives the measuring pass and the writing pass
//! through the same field-sink surface the streaming `CanonicalEncoder`
//! implements, so the two must agree byte-for-byte. The tests here pin
//! three things:
//!
//! 1. **Wire parity** — bytes and digests from the exact path equal the
//!    streaming grammar's for empty and populated descriptors, including
//!    domain separation, field order, set permutations/repeats and
//!    absent-versus-present options, plus a hand-assembled golden layout.
//! 2. **Bounded work** — constructing a retained payload from prepared
//!    borrowed inputs costs exactly one allocating call (`alloc`,
//!    `alloc_zeroed` or `realloc`) and exactly one canonical-byte hash,
//!    with a barrier-coordinated sibling proving the counters are scoped
//!    to the measured thread, and a discrimination companion proving the
//!    counters observe added work.
//! 3. **Identity semantics** — equality, ordering, clone sharing and
//!    `digest_identity!` newtype construction through the same payload.
//!
//! The counting `#[global_allocator]` in [`construction_oracle`] is
//! thread-local and const-initialised, matching the allocator-canary
//! pattern used elsewhere in the workspace: sibling test threads never
//! enter this thread's counters, and the counters themselves allocate
//! nothing.

use std::sync::Arc;

use verter_identity::canonical::Canonical;
use verter_identity::digest_identity;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::exact::{CanonicalFieldSink, ExactCanonicalEncode};

/// A neutral borrowed descriptor shaped like a stage identity: a `u32`
/// compatibility epoch, a predecessor-key digest field, optional raw
/// option bytes, a raw byte field, and an unordered, possibly repeated
/// fixed-size digest set.
///
/// `encode_fields` is deliberately NOT implemented by hand: the streaming
/// encoding comes from the shared default that drives the same
/// `exact_fields` description through the encoder's field-sink
/// implementation, which is exactly the sharing the byte-parity tests
/// hold to load-bearing.
struct StageLike<'a> {
    epoch: u32,
    predecessor: &'a CanonicalDigest,
    options: Option<&'a [u8]>,
    raw: &'a [u8],
    digests: &'a [CanonicalDigest],
}

impl CanonicalEncode for StageLike<'_> {
    const DOMAIN_TAG: &'static str = "exact-test.stage-like.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        self.exact_fields(encoder);
    }
}

impl ExactCanonicalEncode for StageLike<'_> {
    fn exact_fields<S: verter_identity::exact::CanonicalFieldSink>(&self, sink: &mut S) {
        sink.field_u32(1, self.epoch);
        sink.field_digest(2, self.predecessor);
        sink.field_option(3, self.options);
        sink.field_raw(4, self.raw);
        sink.field_digest_set(5, self.digests);
    }
}

/// Digest test data: fixed 32-byte payloads derived from a seed so tests
/// read as data, not magic hex.
fn seeded_digest(seed: u8) -> CanonicalDigest {
    CanonicalDigest::of_bytes(&[seed; 48])
}

/// The shared predecessor digest every populated descriptor borrows,
/// computed once so descriptors stay freely borrowable.
fn anchor_digest() -> &'static CanonicalDigest {
    static ANCHOR: std::sync::OnceLock<CanonicalDigest> = std::sync::OnceLock::new();
    ANCHOR.get_or_init(|| seeded_digest(1))
}

fn populated_stage<'a>(digests: &'a [CanonicalDigest]) -> StageLike<'a> {
    StageLike {
        epoch: 7,
        predecessor: anchor_digest(),
        options: Some(&[0x5a, 0xa5]),
        raw: &[1, 2, 3, 4],
        digests,
    }
}

mod wire_parity {
    //! Bytes and digests from the exact path must equal the streaming
    //! grammar's. The streaming side here routes through the shared
    //! field-sink default, so a defect planted in the common framing
    //! fails both sides — while a defect planted only in the exact path
    //! fails the comparison against the hand-assembled layout.

    use super::*;

    /// The documented wire layout, assembled by hand for one populated
    /// descriptor: header (domain length, domain, field count) then each
    /// field record (`u16` tag, `u64` payload length, payload), the set
    /// payload carrying its distinct-element count and each element
    /// length-prefixed.
    #[test]
    fn exact_bytes_equal_the_documented_layout() {
        let digests = [seeded_digest(9), seeded_digest(4), seeded_digest(9)];
        let stage = populated_stage(&digests);

        let exact = Canonical::from_exact(&stage);

        // The set's distinct elements in ascending order, derived with the
        // standard library's sort rather than the encoder's own
        // selection routine.
        let mut sorted_distinct = vec![seeded_digest(9), seeded_digest(4)];
        sorted_distinct.sort();
        sorted_distinct.dedup();

        let mut expected = Vec::new();
        expected.extend_from_slice(&24u32.to_le_bytes());
        expected.extend_from_slice(b"exact-test.stage-like.v1");
        expected.extend_from_slice(&5u32.to_le_bytes());
        // field 1: epoch
        expected.extend_from_slice(&1u16.to_le_bytes());
        expected.extend_from_slice(&4u64.to_le_bytes());
        expected.extend_from_slice(&7u32.to_le_bytes());
        // field 2: predecessor digest
        expected.extend_from_slice(&2u16.to_le_bytes());
        expected.extend_from_slice(&32u64.to_le_bytes());
        expected.extend_from_slice(seeded_digest(1).as_bytes());
        // field 3: present option bytes
        expected.extend_from_slice(&3u16.to_le_bytes());
        expected.extend_from_slice(&3u64.to_le_bytes());
        expected.extend_from_slice(&[1u8, 0x5a, 0xa5]);
        // field 4: raw bytes
        expected.extend_from_slice(&4u16.to_le_bytes());
        expected.extend_from_slice(&4u64.to_le_bytes());
        expected.extend_from_slice(&[1u8, 2, 3, 4]);
        // field 5: digest set, sorted and deduplicated
        expected.extend_from_slice(&5u16.to_le_bytes());
        expected.extend_from_slice(&88u64.to_le_bytes());
        expected.extend_from_slice(&2u64.to_le_bytes());
        for digest in sorted_distinct {
            expected.extend_from_slice(&32u64.to_le_bytes());
            expected.extend_from_slice(digest.as_bytes());
        }

        assert_eq!(exact.bytes(), &expected[..]);
        assert_eq!(exact.digest(), CanonicalDigest::of_bytes(&expected));
    }

    #[test]
    fn exact_bytes_equal_the_streaming_bytes_for_empty_and_populated() {
        let empty_digests: [CanonicalDigest; 0] = [];
        let empty = StageLike {
            epoch: 0,
            predecessor: &seeded_digest(0),
            options: None,
            raw: &[],
            digests: &empty_digests,
        };
        let populated_digests = [seeded_digest(3), seeded_digest(1), seeded_digest(3)];
        let populated = populated_stage(&populated_digests);

        for stage in [&empty, &populated] {
            let exact = Canonical::from_exact(stage);
            let streamed = Canonical::from_encodable(stage);
            assert_eq!(
                exact.bytes(),
                streamed.bytes(),
                "exact construction must produce the streaming grammar's bytes"
            );
            assert_eq!(exact.digest(), streamed.digest());
            assert_eq!(
                exact.digest(),
                stage.canonical_digest(),
                "the exact digest equals the streaming encoder's in-place digest"
            );
        }
    }

    #[test]
    fn domain_tag_separates_exact_identities() {
        struct OtherDomain<'a>(&'a StageLike<'a>);
        impl CanonicalEncode for OtherDomain<'_> {
            const DOMAIN_TAG: &'static str = "exact-test.other.v1";
            fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
                self.0.exact_fields(encoder);
            }
        }
        impl ExactCanonicalEncode for OtherDomain<'_> {
            fn exact_fields<S: verter_identity::exact::CanonicalFieldSink>(&self, sink: &mut S) {
                self.0.exact_fields(sink);
            }
        }

        let digests = [seeded_digest(2)];
        let stage = populated_stage(&digests);
        let other = OtherDomain(&stage);

        assert_ne!(
            Canonical::from_exact(&stage).bytes(),
            Canonical::from_exact(&other).bytes(),
            "the domain tag is part of the exact-path bytes"
        );
        assert_ne!(
            Canonical::from_exact(&stage).digest(),
            Canonical::from_exact(&other).digest()
        );
    }

    #[test]
    fn field_order_is_significant_through_the_exact_path() {
        struct Swapped<'a>(&'a StageLike<'a>);
        impl CanonicalEncode for Swapped<'_> {
            const DOMAIN_TAG: &'static str = "exact-test.stage-like.v1";
            fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
                self.0.exact_fields(encoder);
            }
        }
        impl ExactCanonicalEncode for Swapped<'_> {
            fn exact_fields<S: verter_identity::exact::CanonicalFieldSink>(&self, sink: &mut S) {
                let inner = self.0;
                // raw before epoch: same fields, different schema order.
                sink.field_raw(4, inner.raw);
                sink.field_u32(1, inner.epoch);
                sink.field_digest(2, inner.predecessor);
                sink.field_option(3, inner.options);
                sink.field_digest_set(5, inner.digests);
            }
        }

        let digests = [seeded_digest(5)];
        let stage = populated_stage(&digests);
        let swapped = Swapped(&stage);

        assert_ne!(
            Canonical::from_exact(&stage).bytes(),
            Canonical::from_exact(&swapped).bytes()
        );
    }

    #[test]
    fn digest_sets_are_order_and_repeat_independent() {
        let ordered = [seeded_digest(1), seeded_digest(2), seeded_digest(3)];
        let permuted = [seeded_digest(3), seeded_digest(1), seeded_digest(2)];
        let repeated = [
            seeded_digest(2),
            seeded_digest(1),
            seeded_digest(2),
            seeded_digest(3),
            seeded_digest(1),
        ];

        let encode_set = |set: &[CanonicalDigest]| {
            let stage = StageLike {
                epoch: 9,
                predecessor: &seeded_digest(8),
                options: None,
                raw: b"r",
                digests: set,
            };
            Canonical::from_exact(&stage)
        };

        let baseline = encode_set(&ordered);
        assert_eq!(encode_set(&permuted).bytes(), baseline.bytes());
        assert_eq!(encode_set(&repeated).bytes(), baseline.bytes());

        let extra = [
            seeded_digest(1),
            seeded_digest(2),
            seeded_digest(3),
            seeded_digest(4),
        ];
        assert_ne!(
            encode_set(&extra).bytes(),
            baseline.bytes(),
            "a genuinely additional digest must change the identity"
        );
    }

    /// The exact path's digest-set payload is the general streaming set
    /// layout: distinct count, then each element length-prefixed.
    #[test]
    fn exact_digest_sets_use_the_streaming_set_layout() {
        let digests = [seeded_digest(6), seeded_digest(6), seeded_digest(2)];

        let mut streaming = CanonicalEncoder::new("set-layout-test");
        streaming.field_sorted_set(1, digests.iter().map(|digest| digest.as_bytes()));
        let mut exact = CanonicalEncoder::new("set-layout-test");
        exact.field_digest_set(1, &digests);

        assert_eq!(streaming.finish(), exact.finish());
    }

    #[test]
    fn absent_options_stay_distinct_from_present_ones() {
        let digests = [seeded_digest(1)];
        let absent = StageLike {
            options: None,
            ..populated_stage(&digests)
        };
        let present_empty = StageLike {
            options: Some(&[]),
            ..populated_stage(&digests)
        };
        let present = populated_stage(&digests);

        let absent_bytes = Canonical::from_exact(&absent).bytes().to_vec();
        let present_empty_bytes = Canonical::from_exact(&present_empty).bytes().to_vec();
        let present_bytes = Canonical::from_exact(&present).bytes().to_vec();
        assert_ne!(absent_bytes, present_empty_bytes);
        assert_ne!(present_empty_bytes, present_bytes);
        assert_ne!(absent_bytes, present_bytes);
    }
}

mod identity_semantics {
    //! Equality, ordering and clone sharing hold through the exact-path
    //! payload exactly as they do through the streaming one.

    use super::*;

    #[test]
    fn equal_inputs_construct_equal_identities() {
        let digests = [seeded_digest(1), seeded_digest(2)];
        let a = Canonical::from_exact(&populated_stage(&digests));
        let b = Canonical::from_exact(&populated_stage(&digests));
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.cmp(&b), core::cmp::Ordering::Equal);
    }

    #[test]
    fn different_inputs_order_by_digest_then_bytes() {
        let digests = [seeded_digest(1)];
        let a = Canonical::from_exact(&populated_stage(&digests));
        let other = StageLike {
            epoch: 8,
            ..populated_stage(&digests)
        };
        let b = Canonical::from_exact(&other);
        assert_ne!(a, b);
        let ordering = a.cmp(&b);
        assert_eq!(ordering, a.cmp(&b), "ordering is stable");
        assert_eq!(b.cmp(&a), ordering.reverse(), "ordering is antisymmetric");
    }

    #[test]
    fn clones_share_the_constructed_bytes() {
        let digests = [seeded_digest(1)];
        let original = Canonical::from_exact(&populated_stage(&digests));
        let clone = original.clone();
        assert!(
            Arc::ptr_eq(original.shared_bytes(), clone.shared_bytes()),
            "a clone shares the one constructed allocation"
        );
    }
}

// The identity-wrapper integration: a `digest_identity!` newtype
// constructs through the exact path and keeps the shared payload.
digest_identity!(ExactProbeKey);

mod newtype_integration {
    use super::*;

    #[test]
    fn digest_identity_newtype_constructs_from_an_exact_descriptor() {
        let digests = [seeded_digest(7), seeded_digest(3)];
        let stage = populated_stage(&digests);

        let key = ExactProbeKey::from_exact(&stage);
        let streamed = ExactProbeKey::from_canonical(&stage);

        assert_eq!(key.digest(), streamed.digest());
        assert_eq!(key.canonical_bytes(), streamed.canonical_bytes());
        let clone = key.clone();
        assert!(Arc::ptr_eq(
            key.canonical_shared_bytes(),
            clone.canonical_shared_bytes()
        ));
        assert_eq!(key, clone);
    }
}

/// A neutral consumer composing a stage-shaped identity end to end from
/// borrowed inputs only: predecessors arrive as digests, option bytes as
/// borrowed slices, and the admission set as a borrowed digest slice. No
/// compiler vocabulary is needed to use the construction path.
mod neutral_consumer {
    use super::*;

    struct DemandBasis<'a> {
        semantic_epoch: u32,
        profile: &'a CanonicalDigest,
        options: Option<&'a [u8]>,
        admitted: &'a [CanonicalDigest],
    }

    impl CanonicalEncode for DemandBasis<'_> {
        const DOMAIN_TAG: &'static str = "consumer-test.demand-basis.v1";
        fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
            self.exact_fields(encoder);
        }
    }

    impl ExactCanonicalEncode for DemandBasis<'_> {
        fn exact_fields<S: verter_identity::exact::CanonicalFieldSink>(&self, sink: &mut S) {
            sink.field_u32(1, self.semantic_epoch);
            sink.field_digest(2, self.profile);
            sink.field_option(3, self.options);
            sink.field_digest_set(4, self.admitted);
        }
    }

    #[test]
    fn composes_a_retained_identity_from_borrowed_fields() {
        let profile = seeded_digest(11);
        let admitted = [seeded_digest(21), seeded_digest(20), seeded_digest(21)];
        let basis = DemandBasis {
            semantic_epoch: 4,
            profile: &profile,
            options: Some(b"strict"),
            admitted: &admitted,
        };

        let payload = Canonical::from_exact(&basis);
        assert_eq!(payload.bytes(), Canonical::from_encodable(&basis).bytes());
        assert_eq!(
            payload.bytes().len(),
            4 + "consumer-test.demand-basis.v1".len()
                + 4
                + 14
                + 42
                + (10 + 1 + 6)
                + (10 + 8 + 2 * 40)
        );
    }
}

mod construction_oracle {
    //! Bounded-work oracle: the whole exact construction — sizing pass,
    //! set canonicalization, storage finalization and the digest — costs
    //! exactly one allocating call and one canonical-byte hash.
    //!
    //! The counting allocator is thread-local, so the window is the
    //! measured thread's work only; the hash counter is equally
    //! thread-scoped. A barrier-coordinated sibling hammers the same
    //! construction on another thread during the window to prove sibling
    //! activity cannot enter the counts, and a discrimination companion
    //! adds one deliberate scratch allocation and one deliberate extra
    //! hash through the same window shape to prove a zero (or one) the
    //! oracle reports is a measured number, not a dead counter.

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::hint::black_box;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};

    use verter_identity::encoding::{canonical_hash_calls, reset_canonical_hash_calls};

    use super::*;

    thread_local! {
        /// Allocating calls (`alloc`/`alloc_zeroed`/`realloc`) on this
        /// harness thread. A `const` initializer keeps allocator access
        /// allocation-free.
        static ALLOC_CALLS: Cell<u64> = const { Cell::new(0) };
    }

    struct CountingAllocator;

    fn count_alloc_call() {
        // Allocation can occur while a thread is tearing down TLS; never
        // turn a valid allocation into a panic there.
        let _ = ALLOC_CALLS.try_with(|calls| calls.set(calls.get().wrapping_add(1)));
    }

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count_alloc_call();
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            count_alloc_call();
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            count_alloc_call();
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

    fn reset_counters() {
        ALLOC_CALLS.with(|calls| calls.set(0));
        reset_canonical_hash_calls();
    }

    fn read_alloc_calls() -> u64 {
        ALLOC_CALLS.with(Cell::get)
    }

    /// Runs `window`, then reads both counters while the window's values
    /// are still live, so later assertions cannot be contaminated by
    /// cleanup work.
    fn measured<T>(window: impl FnOnce() -> T) -> (T, u64, u64) {
        reset_counters();
        let value = window();
        let allocs = read_alloc_calls();
        let hashes = canonical_hash_calls();
        (value, allocs, hashes)
    }

    #[test]
    fn empty_descriptor_constructs_in_one_allocation_and_one_hash() {
        let digests: [CanonicalDigest; 0] = [];
        let empty = StageLike {
            epoch: 0,
            predecessor: &seeded_digest(0),
            options: None,
            raw: &[],
            digests: &digests,
        };

        let (payload, allocs, hashes) = measured(|| Canonical::from_exact(&empty));
        assert_eq!(
            allocs, 1,
            "empty exact construction made {allocs} allocating calls"
        );
        assert_eq!(
            hashes, 1,
            "empty exact construction made {hashes} canonical-byte hashes"
        );
        assert_eq!(payload.bytes(), Canonical::from_encodable(&empty).bytes());
    }

    #[test]
    fn populated_descriptor_constructs_in_one_allocation_and_one_hash() {
        let digests = [
            seeded_digest(9),
            seeded_digest(4),
            seeded_digest(9),
            seeded_digest(1),
        ];
        let stage = populated_stage(&digests);

        let (payload, allocs, hashes) = measured(|| Canonical::from_exact(&stage));
        assert_eq!(
            allocs, 1,
            "populated exact construction (set canonicalization included) made {allocs} allocating calls"
        );
        assert_eq!(
            hashes, 1,
            "populated exact construction made {hashes} canonical-byte hashes"
        );
        assert_eq!(payload.bytes(), Canonical::from_encodable(&stage).bytes());
    }

    #[test]
    fn cloning_the_constructed_payload_allocates_nothing() {
        let digests = [seeded_digest(3), seeded_digest(2)];
        let stage = populated_stage(&digests);
        let payload = Canonical::from_exact(&stage);

        let (clone, allocs, hashes) = measured(|| payload.clone());
        assert_eq!(allocs, 0, "a clone allocated {allocs} times");
        assert_eq!(hashes, 0, "a clone hashed {hashes} times");
        assert!(Arc::ptr_eq(payload.shared_bytes(), clone.shared_bytes()));
    }

    /// A sibling thread constructing identities throughout the measured
    /// window must not move either counter: the oracle is scoped to the
    /// measured thread's execution, coordinated by barriers rather than
    /// sleeps.
    #[test]
    fn sibling_construction_stays_outside_the_measured_window() {
        let start = Arc::new(Barrier::new(2));
        let finished = Arc::new(AtomicBool::new(false));
        let sibling_start = Arc::clone(&start);
        let sibling_finished = Arc::clone(&finished);
        let sibling = std::thread::spawn(move || {
            let sibling_digests = [seeded_digest(30), seeded_digest(29)];
            let sibling_stage = populated_stage(&sibling_digests);
            sibling_start.wait();
            while !sibling_finished.load(Ordering::Relaxed) {
                black_box(Canonical::from_exact(&sibling_stage));
            }
        });

        let digests = [seeded_digest(12), seeded_digest(11)];
        let stage = populated_stage(&digests);
        start.wait();
        let (payload, allocs, hashes) = measured(|| Canonical::from_exact(&stage));
        finished.store(true, Ordering::Relaxed);
        sibling.join().expect("sibling thread");

        assert_eq!(
            allocs, 1,
            "window counted {allocs} allocating calls with a sibling constructing concurrently"
        );
        assert_eq!(hashes, 1, "window counted {hashes} canonical-byte hashes with a sibling constructing concurrently");
        assert_eq!(payload.bytes(), Canonical::from_encodable(&stage).bytes());
    }

    /// Discrimination companion: through the same window shape, one
    /// deliberate scratch allocation and one deliberate extra
    /// canonical-byte hash are both observed. The single-allocation and
    /// single-hash results above are measured zeros, not dead counters.
    #[test]
    fn the_oracle_observes_added_scratch_allocation_and_added_hashes() {
        let digests = [seeded_digest(5)];
        let stage = populated_stage(&digests);

        let (_, allocs, hashes) = measured(|| {
            let payload = Canonical::from_exact(&stage);
            // Exactly the regressions the oracle exists to catch: hidden
            // encoding scratch allocated before measurement, and a second
            // canonical-byte hash.
            let scratch: Vec<u8> = Vec::with_capacity(64);
            let rehashed = CanonicalDigest::of_bytes(b"deliberate-extra-hash");
            (black_box(payload), black_box(scratch), black_box(rehashed))
        });

        assert_eq!(
            allocs, 2,
            "window observed {allocs} allocating calls; the scratch allocation must be visible beside the one construction allocation"
        );
        assert_eq!(
            hashes, 2,
            "window observed {hashes} canonical-byte hashes; the deliberate extra hash must be visible beside the construction hash"
        );
    }
}
