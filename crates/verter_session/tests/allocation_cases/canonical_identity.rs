//! What a canonical identity costs the allocator: an identity is cloned
//! into every record that carries it, so a clone must share the encoded
//! bytes rather than copy them, and encoding a descriptor must cost one
//! growing buffer, not one allocation per field.

use verter_identity::canonical::Canonical;
use verter_identity::encoding::CanonicalEncoder;

use super::{alloc_count, reset_alloc_counter};

/// An encoder holding `fields` fields of twelve payload bytes each.
fn encoder_with(fields: u16) -> CanonicalEncoder {
    let mut encoder = CanonicalEncoder::new("allocation-canary.identity.v1");
    for tag in 0..fields {
        encoder.field_bytes(tag, &[tag as u8; 12]);
    }
    encoder
}

/// Cloning an identity allocates nothing, however many clones are taken
/// and however long its encoding is.
#[test]
fn canonical_identity_clones_allocate_nothing() {
    for fields in [1u16, 16, 256] {
        let identity = Canonical::from_encoder(&encoder_with(fields));
        let mut clones = Vec::with_capacity(1_000);
        reset_alloc_counter();
        for _ in 0..1_000 {
            clones.push(identity.clone());
        }
        let allocations = alloc_count();
        assert_eq!(
            allocations, 0,
            "1000 clones of a {fields}-field identity allocated {allocations} times"
        );
        assert!(clones.iter().all(|clone| *clone == identity));
    }
}

/// Finishing an identity allocates its shared bytes once, whatever the
/// field count; appending fields grows one buffer geometrically, so the
/// allocations per field fall as the descriptor grows instead of staying
/// one per field.
#[test]
fn canonical_encoding_allocates_one_buffer_and_one_identity_block() {
    let mut appended = Vec::new();
    for fields in [16u16, 128, 1_024] {
        reset_alloc_counter();
        let encoder = encoder_with(fields);
        let appends = alloc_count();
        reset_alloc_counter();
        let identity = Canonical::from_encoder(&encoder);
        let finish = alloc_count();
        assert_eq!(
            finish, 1,
            "a {fields}-field identity is one allocation, took {finish}"
        );
        assert_eq!(identity.bytes(), &encoder.finish()[..]);
        // A doubling buffer reaches `n` bytes in at most log2(n) + 1 growths.
        let body = usize::from(fields) * (2 + 8 + 12);
        let bound = u64::from(usize::BITS - body.leading_zeros()) + 1;
        assert!(
            appends <= bound,
            "{fields} fields took {appends} allocations to append, more than {bound}"
        );
        appended.push(appends);
    }
    assert!(
        appended[2] < 1_024 / 16,
        "appending fields stays far below one allocation per field: {appended:?}"
    );
}
