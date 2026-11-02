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
//!    borrowed inputs, a set of retained keys projected through their
//!    digests included, costs exactly one allocating call (`alloc`,
//!    `alloc_zeroed` or `realloc`) and exactly one canonical-byte hash,
//!    with a barrier-fenced sibling construction landing inside the window
//!    proving the counters are scoped to the measured thread, and a
//!    discrimination companion proving the counters observe added work.
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
/// Only the exact description is written: the streaming `CanonicalEncode`
/// comes from the blanket implementation that drives this same
/// `exact_fields` through the encoder's field-sink implementation, which is
/// exactly the sharing the byte-parity tests hold to load-bearing.
struct StageLike<'a> {
    epoch: u32,
    predecessor: &'a CanonicalDigest,
    options: Option<&'a [u8]>,
    raw: &'a [u8],
    digests: &'a [CanonicalDigest],
}

impl ExactCanonicalEncode for StageLike<'_> {
    const DOMAIN_TAG: &'static str = "exact-test.stage-like.v1";

    fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
        sink.field_u32(1, self.epoch);
        sink.field_digest(2, self.predecessor);
        sink.field_option(3, self.options);
        sink.field_raw(4, self.raw);
        sink.field_digest_set(5, self.digests.iter().copied());
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

// A keyed-set source: a `digest_identity!` newtype whose retained digest a
// composition projects into a set field without copying it anywhere first.
digest_identity!(SourceProbeKey);

/// A neutral composition shaped like a semantic-stage identity: a set of
/// predecessor keys projected through their retained digests (field 1), a
/// profile digest, optional option bytes, a `u32` epoch, and a contiguous
/// digest set such as an admission basis (field 5) — every input borrowed,
/// nothing re-collected before or during construction.
struct StageComposition<'a> {
    source_units: &'a [SourceProbeKey],
    profile: &'a CanonicalDigest,
    options: Option<&'a [u8]>,
    semantic_epoch: u32,
    admitted: &'a [CanonicalDigest],
}

impl ExactCanonicalEncode for StageComposition<'_> {
    const DOMAIN_TAG: &'static str = "exact-test.stage-composition.v1";

    fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
        sink.field_digest_set(1, self.source_units.iter().map(SourceProbeKey::digest));
        sink.field_digest(2, self.profile);
        sink.field_option(3, self.options);
        sink.field_u32(4, self.semantic_epoch);
        sink.field_digest_set(5, self.admitted.iter().copied());
    }
}

/// Prepared source-unit keys — two distinct identities and a repeat of the
/// first — built before any measured window, so a composition over them
/// reads retained digests rather than hashing anything.
fn source_unit_keys() -> [SourceProbeKey; 3] {
    let first_digests = [seeded_digest(41)];
    let second_digests = [seeded_digest(40)];
    let first = SourceProbeKey::from_exact(&populated_stage(&first_digests));
    let second = SourceProbeKey::from_exact(&populated_stage(&second_digests));
    [first.clone(), second, first]
}

mod wire_parity {
    //! Bytes and digests from the exact path must equal the streaming
    //! grammar's. Both paths frame through the same routines, so a defect in
    //! the common framing moves both alike; the hand-assembled layout is the
    //! comparison that catches it.

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
        impl ExactCanonicalEncode for OtherDomain<'_> {
            const DOMAIN_TAG: &'static str = "exact-test.other.v1";

            fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
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
        impl ExactCanonicalEncode for Swapped<'_> {
            const DOMAIN_TAG: &'static str = "exact-test.stage-like.v1";

            fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
                let inner = self.0;
                // raw before epoch: same fields, different schema order.
                sink.field_raw(4, inner.raw);
                sink.field_u32(1, inner.epoch);
                sink.field_digest(2, inner.predecessor);
                sink.field_option(3, inner.options);
                sink.field_digest_set(5, inner.digests.iter().copied());
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
        exact.field_digest_set(1, digests.iter().copied());

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
        let expected = a
            .digest()
            .cmp(&b.digest())
            .then_with(|| a.bytes().cmp(b.bytes()));
        assert_eq!(a.cmp(&b), expected, "ordering is digest, then bytes");
        assert_eq!(b.cmp(&a), expected.reverse(), "ordering is antisymmetric");
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

mod descriptor_determinism {
    //! A descriptor is driven twice, once to size and once to write. One
    //! whose passes describe different fields must fail rather than yield
    //! bytes that are not its canonical encoding.

    use std::cell::Cell;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::*;

    type Fields<'a> = &'a [&'a [u8]];

    /// Describes `sized` on its first pass and `written` on every later one.
    struct TwoFaced<'a> {
        passes: Cell<u32>,
        sized: Fields<'a>,
        written: Fields<'a>,
    }

    impl ExactCanonicalEncode for TwoFaced<'_> {
        const DOMAIN_TAG: &'static str = "exact-test.two-faced.v1";

        fn exact_fields<S: CanonicalFieldSink>(&self, sink: &mut S) {
            let pass = self.passes.get();
            self.passes.set(pass + 1);
            let fields = if pass == 0 { self.sized } else { self.written };
            for (tag, payload) in (1u16..).zip(fields) {
                sink.field_raw(tag, payload);
            }
        }
    }

    #[test]
    fn passes_that_disagree_fail_instead_of_yielding_bytes() {
        let cases: [(&str, Fields, Fields); 3] = [
            ("writes less", &[&[0; 12]], &[&[0; 4]]),
            ("writes more", &[&[0; 4]], &[&[0; 12]]),
            // One 12-byte record against two 1-byte records: the same body
            // length (22 bytes), but a header counting the wrong fields.
            (
                "same length, different field count",
                &[&[0; 12]],
                &[&[0; 1], &[0; 1]],
            ),
        ];
        for (name, sized, written) in cases {
            let descriptor = TwoFaced {
                passes: Cell::new(0),
                sized,
                written,
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| Canonical::from_exact(&descriptor)));
            assert!(outcome.is_err(), "{name}: construction must fail");
            assert_eq!(descriptor.passes.get(), 2, "{name}: both passes ran");
        }
    }

    #[test]
    fn passes_that_agree_construct_normally() {
        let fields: Fields = &[&[1; 12], &[2; 1]];
        let descriptor = TwoFaced {
            passes: Cell::new(0),
            sized: fields,
            written: fields,
        };
        let payload = Canonical::from_exact(&descriptor);
        assert_eq!(
            descriptor.passes.get(),
            2,
            "one sizing and one writing pass"
        );
        let mut expected = CanonicalEncoder::new("exact-test.two-faced.v1");
        expected.field_bytes(1, &[1; 12]).field_bytes(2, &[2; 1]);
        assert_eq!(payload.bytes(), &expected.finish()[..]);
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
/// borrowed inputs only: predecessor keys as a borrowed slice of identity
/// newtypes projected through their digests, a profile digest, option bytes
/// as a borrowed slice, and the admission set as a borrowed digest slice. No
/// compiler vocabulary is needed to use the construction path, and no input
/// is re-collected into encoding scratch on the way in.
mod neutral_consumer {
    use super::*;

    #[test]
    fn composes_a_retained_identity_from_borrowed_fields() {
        let keys = source_unit_keys();
        let profile = seeded_digest(11);
        let admitted = [seeded_digest(21), seeded_digest(20), seeded_digest(21)];
        let composition = StageComposition {
            source_units: &keys,
            profile: &profile,
            options: Some(b"strict"),
            semantic_epoch: 4,
            admitted: &admitted,
        };

        let payload = Canonical::from_exact(&composition);
        assert_eq!(
            payload.bytes(),
            Canonical::from_encodable(&composition).bytes()
        );

        // The projected key set encodes exactly as the general set grammar
        // over the keys' digests — two distinct keys from three, sorted —
        // and the contiguous digest set likewise, so a consumer may supply
        // either shape and mint one identity.
        let mut expected = CanonicalEncoder::new("exact-test.stage-composition.v1");
        expected
            .field_sorted_set(1, keys.iter().map(|key| *key.digest().as_bytes()))
            .field_bytes(2, profile.as_bytes())
            .field_option(3, Some(b"strict"))
            .field_u32(4, 4)
            .field_sorted_set(5, admitted.iter().map(|digest| *digest.as_bytes()));
        assert_eq!(payload.bytes(), &expected.finish()[..]);
    }
}

mod construction_oracle {
    //! Bounded-work oracle: the whole exact construction — sizing pass,
    //! set canonicalization, storage finalization and the digest — costs
    //! exactly one allocating call and one canonical-byte hash.
    //!
    //! The counting allocator is thread-local, so the window is the
    //! measured thread's work only; the hash counter is equally
    //! thread-scoped. A barrier-fenced sibling performs the same
    //! construction on another thread strictly inside the window to prove
    //! sibling activity cannot enter the counts, and a discrimination
    //! companion adds one deliberate scratch allocation and one deliberate
    //! extra hash through the same window shape to prove a zero (or one)
    //! the oracle reports is a measured number, not a dead counter.

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::hint::black_box;
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

    /// The consumer's keyed-set shape: predecessor keys projected through
    /// their retained digests, nothing re-collected. The keys are prepared
    /// before the window, so their digests are reads, not hashes, and the
    /// composition over them stays within the bound.
    #[test]
    fn keyed_set_projection_constructs_in_one_allocation_and_one_hash() {
        let keys = source_unit_keys();
        let profile = seeded_digest(11);
        let admitted = [seeded_digest(21), seeded_digest(20)];
        let composition = StageComposition {
            source_units: &keys,
            profile: &profile,
            options: Some(b"strict"),
            semantic_epoch: 4,
            admitted: &admitted,
        };

        let (payload, allocs, hashes) = measured(|| Canonical::from_exact(&composition));
        assert_eq!(
            allocs, 1,
            "a composition over projected keys made {allocs} allocating calls"
        );
        assert_eq!(
            hashes, 1,
            "a composition over projected keys made {hashes} canonical-byte hashes"
        );
        assert_eq!(
            payload.bytes(),
            Canonical::from_encodable(&composition).bytes()
        );
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

    /// A sibling thread's construction landing inside the measured window
    /// must not move either counter: the oracle is scoped to the measured
    /// thread's execution. The window is fenced by barriers rather than
    /// sleeps — the sibling constructs strictly between the window opening
    /// and the window closing — so the interval between the counter reset
    /// and the counter reads contains one sibling allocation and one
    /// sibling hash on every run, and a process-global counter fails here
    /// deterministically rather than when scheduling happens to overlap.
    /// Every assertion runs after both threads have left the window and
    /// the sibling has been joined, so a failing assertion leaves no
    /// sibling running.
    #[test]
    fn sibling_construction_inside_the_window_stays_out_of_the_counters() {
        // One barrier, three rendezvous. The first, before the window, lets
        // any lazy initialisation of the barrier's primitives happen outside
        // the measurement; the next two fence the window.
        let fence = Arc::new(Barrier::new(2));
        let sibling_fence = Arc::clone(&fence);
        let sibling = std::thread::spawn(move || {
            let sibling_digests = [seeded_digest(30), seeded_digest(29)];
            let sibling_stage = populated_stage(&sibling_digests);
            sibling_fence.wait();
            sibling_fence.wait();
            let built = black_box(Canonical::from_exact(&sibling_stage));
            sibling_fence.wait();
            built
        });

        let digests = [seeded_digest(12), seeded_digest(11)];
        let stage = populated_stage(&digests);
        fence.wait();
        let (payload, allocs, hashes) = measured(|| {
            // Window open: the sibling constructs from here on.
            fence.wait();
            let payload = Canonical::from_exact(&stage);
            // The sibling's construction has completed before this returns.
            fence.wait();
            payload
        });
        let sibling_payload = sibling.join().expect("sibling thread");

        assert_eq!(
            allocs, 1,
            "window counted {allocs} allocating calls with a sibling construction inside it"
        );
        assert_eq!(
            hashes, 1,
            "window counted {hashes} canonical-byte hashes with a sibling construction inside it"
        );
        assert_eq!(payload.bytes(), Canonical::from_encodable(&stage).bytes());
        assert_ne!(
            sibling_payload.bytes(),
            payload.bytes(),
            "the sibling built its own identity inside the window"
        );
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
