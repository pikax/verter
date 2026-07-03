//! The maintained tsgo `--api` wire pin.
//!
//! Because the codec is hand-written (not generated), this manifest is the
//! human-maintained record of the tsgo version and the normalized wire shape
//! the codec targets. The runtime fail-closed wire gate ([`crate::gate`])
//! validates an installed engine against this pin and refuses to start a
//! diverged tsgo.
//!
//! Maintainer process: on a TypeScript / tsgo version bump, run the
//! version-update agent to re-verify the hand-written codec against the shipped
//! reference (`dist/api/*`), then bump [`PINNED`] to the confirmed wire (the
//! version string, the op/callback inventory, and the framing constants). The
//! [`SchemaManifest::wire_fingerprint`] is derived deterministically from the
//! manifest contents, so bumping any field changes the fingerprint, and the
//! gate refuses any engine whose fingerprint does not match.

/// The framing constants the hand-written codec targets, mirrored from
/// `dist/api/node/msgpack.js` + `dist/api/syncChannel.js`. These enter the wire
/// fingerprint so a change to the framing (e.g. a new array marker, a different
/// message-type tag) flips the pin and trips the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramingConstants {
    /// `MSGPACK_FIXARRAY3` (msgpack.js:4).
    pub fixarray3: u8,
    /// `MSGPACK_BIN8` (msgpack.js:5).
    pub bin8: u8,
    /// `MSGPACK_BIN16` (msgpack.js:6).
    pub bin16: u8,
    /// `MSGPACK_BIN32` (msgpack.js:7).
    pub bin32: u8,
    /// `MSGPACK_UINT8` (msgpack.js:8).
    pub uint8: u8,
    /// `MSG_REQUEST` (syncChannel.js:17).
    pub msg_request: u8,
    /// `MSG_RESPONSE` (syncChannel.js:21).
    pub msg_response: u8,
    /// `MSG_ERROR` (syncChannel.js:22).
    pub msg_error: u8,
    /// `MSG_CALL` (syncChannel.js:23).
    pub msg_call: u8,
}

/// The maintained pin: the tsgo version and the wire shape the codec targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaManifest {
    /// The REFERENCE `typescript` distribution version this codec was verified
    /// against (e.g. `7.0.1-rc`). The gate accepts a version CHANNEL
    /// ([`crate::gate::classify_engine_version`]), not this one build; this
    /// field documents the build the hand-written codec was checked against
    /// and appears in gate refusal messages.
    pub engine_version: &'static str,
    /// The framing constants the codec emits/accepts.
    pub framing: FramingConstants,
    /// The high-level op method-name strings the codec hand-writes, sorted.
    /// Mirrors the `apiRequest(...)` call sites in `sync/api.js`.
    pub ops: &'static [&'static str],
    /// The host-callback names the codec services, in the exact wire order
    /// (`fs.js:3`).
    pub callbacks: &'static [&'static str],
}

impl SchemaManifest {
    /// A deterministic 64-bit fingerprint of the wire shape this manifest
    /// describes. Derived purely from the manifest contents via a stable
    /// FNV-1a hash, so any field change flips the fingerprint. The fingerprint
    /// deliberately does NOT depend on the engine version (the gate checks
    /// version and fingerprint as two separate dimensions, so a pure version
    /// bump that keeps the wire identical still matches the fingerprint).
    pub const fn wire_fingerprint(&self) -> u64 {
        // FNV-1a over a canonical byte stream of the framing + op + callback
        // inventory. `const fn` so the pinned fingerprint is a compile-time
        // constant available to tests without runtime work.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv1a_byte(h, self.framing.fixarray3);
        h = fnv1a_byte(h, self.framing.bin8);
        h = fnv1a_byte(h, self.framing.bin16);
        h = fnv1a_byte(h, self.framing.bin32);
        h = fnv1a_byte(h, self.framing.uint8);
        h = fnv1a_byte(h, self.framing.msg_request);
        h = fnv1a_byte(h, self.framing.msg_response);
        h = fnv1a_byte(h, self.framing.msg_error);
        h = fnv1a_byte(h, self.framing.msg_call);
        // Separator so op/callback boundaries cannot collide.
        h = fnv1a_byte(h, 0xff);
        h = fnv1a_str_list(h, self.ops);
        h = fnv1a_byte(h, 0xfe);
        h = fnv1a_str_list(h, self.callbacks);
        h
    }
}

const fn fnv1a_byte(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x0000_0100_0000_01b3)
}

const fn fnv1a_str_list(mut h: u64, items: &[&str]) -> u64 {
    let mut i = 0;
    while i < items.len() {
        let bytes = items[i].as_bytes();
        let mut j = 0;
        while j < bytes.len() {
            h = fnv1a_byte(h, bytes[j]);
            j += 1;
        }
        // Per-item separator keeps `["ab","c"]` distinct from `["a","bc"]`.
        h = fnv1a_byte(h, 0x00);
        i += 1;
    }
    h
}

/// The op method-name inventory the codec currently hand-writes. Kept sorted so
/// the fingerprint is order-stable. Mirrors `sync/api.js` (see
/// [`crate::proto::types::method`] for per-op line citations).
pub const PINNED_OPS: &[&str] = &[
    "echo",
    "getConfigFileParsingDiagnostics",
    "getDefaultProjectForFile",
    "getSemanticDiagnostics",
    "getSourceFile",
    "getSymbolAtPosition",
    "getSyntacticDiagnostics",
    "getTypeAtPosition",
    "initialize",
    "parseConfigFile",
    "release",
    "typeToString",
];

/// The host-callback inventory in wire order (`fs.js:3`).
pub const PINNED_CALLBACKS: &[&str] = &[
    "readFile",
    "fileExists",
    "directoryExists",
    "getAccessibleEntries",
    "realpath",
];

/// The maintained wire pin for the currently supported tsgo `--api` wire.
///
/// On a version bump the maintainer re-verifies the hand-written codec, then
/// updates `engine_version` (and the op/callback inventory if the surface
/// changed). `engine_version` is `typescript@7.0.1-rc` — the REFERENCE build
/// within the accepted version channel
/// ([`crate::gate::classify_engine_version`]), not the sole accepted version:
/// the gate admits every build in the channel, all of which share the
/// bare-integer opaque-handle wire the codec
/// ([`crate::proto::types::OpaqueHandle`]) targets. The op inventory
/// carries `getConfigFileParsingDiagnostics` (the project config-parse /
/// compiler-options diagnostic surface the codec now hand-writes); the
/// fingerprint reflects that op set.
pub const PINNED: SchemaManifest = SchemaManifest {
    engine_version: "7.0.1-rc",
    framing: FramingConstants {
        fixarray3: crate::proto::msgpack::MSGPACK_FIXARRAY3,
        bin8: crate::proto::msgpack::MSGPACK_BIN8,
        bin16: crate::proto::msgpack::MSGPACK_BIN16,
        bin32: crate::proto::msgpack::MSGPACK_BIN32,
        uint8: crate::proto::msgpack::MSGPACK_UINT8,
        msg_request: 1,
        msg_response: 4,
        msg_error: 5,
        msg_call: 6,
    },
    ops: PINNED_OPS,
    callbacks: PINNED_CALLBACKS,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_fingerprint_is_stable_and_nonzero() {
        let fp = PINNED.wire_fingerprint();
        assert_ne!(fp, 0);
        // Stable across calls (pure function of the const manifest).
        assert_eq!(fp, PINNED.wire_fingerprint());
    }

    #[test]
    fn fingerprint_changes_when_framing_changes() {
        let mut m = PINNED;
        m.framing.msg_response = 9; // a different message-type tag
        assert_ne!(
            m.wire_fingerprint(),
            PINNED.wire_fingerprint(),
            "a framing change must flip the fingerprint"
        );
    }

    #[test]
    fn fingerprint_changes_when_op_set_changes() {
        const FEWER: &[&str] = &["echo", "initialize"];
        let mut m = PINNED;
        m.ops = FEWER;
        assert_ne!(m.wire_fingerprint(), PINNED.wire_fingerprint());
    }

    #[test]
    fn fingerprint_independent_of_engine_version() {
        // Two manifests that differ ONLY in version share a fingerprint: the
        // gate treats version and wire shape as separate dimensions.
        let mut m = PINNED;
        m.engine_version = "7.0.0-dev.20260526.1";
        assert_ne!(
            m.engine_version, PINNED.engine_version,
            "the injected version must differ from the pin for this to discriminate"
        );
        assert_eq!(
            m.wire_fingerprint(),
            PINNED.wire_fingerprint(),
            "version is not part of the wire fingerprint"
        );
    }

    #[test]
    fn fingerprint_distinguishes_item_boundaries() {
        // ["a","bc"] must not hash equal to ["ab","c"].
        const A: &[&str] = &["a", "bc"];
        const B: &[&str] = &["ab", "c"];
        let mut ma = PINNED;
        ma.ops = A;
        let mut mb = PINNED;
        mb.ops = B;
        assert_ne!(ma.wire_fingerprint(), mb.wire_fingerprint());
    }

    #[test]
    fn pinned_op_inventory_is_sorted() {
        let mut sorted = PINNED_OPS.to_vec();
        sorted.sort_unstable();
        assert_eq!(PINNED_OPS, sorted.as_slice(), "ops must stay sorted");
    }
}
