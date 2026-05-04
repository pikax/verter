//! Discriminating coverage for the `IncidentalFields` trait.
//!
//! The trait replaces the previously-hand-maintained
//! `INCIDENTAL_FIELD_NAMES` constant + `mask_incidental_spans` match
//! statement on `RustSemanticFootprintAudit`. The trait contract is
//! that:
//!
//! 1. `incidental_fields()` returns a `'static` slice of field names
//!    whose payload is incidental (cleared during snapshot
//!    masking).
//! 2. `mask_incidental(&mut self)` clears every payload
//!    corresponding to those names.
//!
//! These tests use *test-local* implementors so they pass on any
//! tree that has the trait defined — they are independent of which
//! audit record types currently have incidental fields. They fail
//! to compile against any tree where the trait does not yet exist
//! (which is the discriminator for Slice 0.3 vs the pre-change
//! codebase).
//!
//! See plan §2 Slice 0.3 ("Hand-written `IncidentalFields` trait").

use verter_session::component_meta_audit::IncidentalFields;

/// Test-local implementor with two incidental fields. Models a
/// future audit record with multiple flaky payloads (Wave 2 will
/// add ~6).
#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct TwoIncidentalFields {
    flaky_a: Vec<u32>,
    flaky_b: Vec<u32>,
    stable_c: Vec<u32>,
}

impl IncidentalFields for TwoIncidentalFields {
    fn incidental_fields() -> &'static [&'static str] {
        &["flaky_a", "flaky_b"]
    }

    fn mask_incidental(&mut self) {
        for field in Self::incidental_fields() {
            match *field {
                "flaky_a" => self.flaky_a.clear(),
                "flaky_b" => self.flaky_b.clear(),
                unknown => {
                    panic!("TwoIncidentalFields::mask_incidental: unknown field `{unknown}`")
                }
            }
        }
    }
}

/// Test-local implementor with no incidental fields. Models an
/// audit record type that opts into the trait but has nothing to
/// mask (e.g., a record where every field is semantically
/// load-bearing).
#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct NoIncidentalFields {
    stable_a: Vec<u32>,
    stable_b: Vec<u32>,
}

impl IncidentalFields for NoIncidentalFields {
    fn incidental_fields() -> &'static [&'static str] {
        &[]
    }

    // Empty incidental list, so the loop body is unreachable in the
    // current contract. The structure deliberately mirrors the real
    // implementation on `RustSemanticFootprintAudit` so a reviewer
    // can see the same shape; clippy correctly flags it as
    // `never_loop` / `match_single_binding` for this empty-list
    // case, which is exactly the contract this test pins.
    #[allow(clippy::never_loop, clippy::match_single_binding)]
    fn mask_incidental(&mut self) {
        for field in Self::incidental_fields() {
            match *field {
                unknown => panic!("NoIncidentalFields::mask_incidental: unknown field `{unknown}`"),
            }
        }
    }
}

/// Positive test: a struct that lists 2 incidental fields exposes a
/// 2-element slice with the expected names, AND `mask_incidental`
/// actually clears those fields' payloads while leaving stable
/// fields intact.
///
/// Discrimination: this test FAILS if the trait method returns the
/// wrong slice length, the wrong names, or fails to clear a listed
/// field. The stable-field assertion catches an over-eager
/// implementation that nukes everything.
#[test]
fn trait_lists_and_masks_two_incidental_fields() {
    let names = <TwoIncidentalFields as IncidentalFields>::incidental_fields();
    assert_eq!(
        names.len(),
        2,
        "incidental_fields() must report exactly 2 entries; got {names:?}",
    );
    assert!(
        names.contains(&"flaky_a"),
        "incidental_fields() should contain `flaky_a`; got {names:?}",
    );
    assert!(
        names.contains(&"flaky_b"),
        "incidental_fields() should contain `flaky_b`; got {names:?}",
    );

    let mut record = TwoIncidentalFields {
        flaky_a: vec![1, 2, 3],
        flaky_b: vec![10, 20],
        stable_c: vec![100, 200, 300],
    };
    record.mask_incidental();

    assert!(
        record.flaky_a.is_empty(),
        "mask_incidental must clear `flaky_a`; got {:?}",
        record.flaky_a,
    );
    assert!(
        record.flaky_b.is_empty(),
        "mask_incidental must clear `flaky_b`; got {:?}",
        record.flaky_b,
    );
    assert_eq!(
        record.stable_c,
        vec![100, 200, 300],
        "mask_incidental must NOT touch fields outside `incidental_fields()`",
    );
}

/// Negative test: a struct that lists 0 incidental fields exposes
/// an empty slice, AND `mask_incidental` is a no-op — it must not
/// panic and must not modify the struct.
///
/// Discrimination: this test FAILS if `incidental_fields()` returns
/// a non-empty slice (contract violation) or if `mask_incidental`
/// mutates the data on a record that has no fields to mask. It
/// catches a regression where the trait grows an implicit "default
/// behaviour" that touches state regardless of declared incidental
/// fields.
#[test]
fn trait_empty_list_is_no_op() {
    let names = <NoIncidentalFields as IncidentalFields>::incidental_fields();
    assert!(
        names.is_empty(),
        "incidental_fields() on a no-mask type must be empty; got {names:?}",
    );

    let original = NoIncidentalFields {
        stable_a: vec![1, 2, 3],
        stable_b: vec![4, 5, 6],
    };
    let mut record = original.clone();
    record.mask_incidental();

    assert_eq!(
        record, original,
        "mask_incidental on a no-mask type must leave the record unchanged",
    );
}
