// The SINGLE definition of the fixture-freshness content hash, `include!`d by BOTH sides of the
// check: the `fake_tsgo_heartbeat` test-support bin (`tests/support/fake_tsgo_heartbeat.rs`) and the
// integration test that probes it (`tests/cases/shim_live.rs`).
//
// They are separate compilation units — the bin cannot import from the test and this crate exposes
// no lib — so `include!` is what keeps them ONE definition instead of two hand-copied loops that can
// silently drift apart and turn the freshness probe into a permanent false mismatch (or, worse, a
// permanent false match). Both sides recompile when this file changes, so they can never disagree
// about the algorithm.

/// A stable, dependency-free content hash (FNV-1a, 64-bit).
///
/// The constants are the STANDARD FNV-1a 64 ones — offset basis `0xcbf29ce484222325`, prime
/// `0x100000001b3` — and the order is the FNV-1a order (XOR the byte in, THEN multiply). The name
/// has to stay honest: a staleness oracle that advertises a published algorithm and quietly uses a
/// different constant invites the next reader to reason about it as FNV-1a and to cross-check it
/// against a reference implementation that will not agree.
///
/// Both sides of the freshness check hash the SAME checked-out file, so the bytes agree
/// byte-for-byte on every platform — no line-ending normalization is needed or wanted.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
