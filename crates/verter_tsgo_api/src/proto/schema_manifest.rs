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
    /// against (e.g. `7.0.2`). The gate accepts a version CHANNEL
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
    /// Per-op REQUEST field-name schedule: for each op the codec SENDS, its
    /// SORTED request-payload field-name set (the normalized wire AFTER the JS
    /// client shims — e.g. `updateSnapshot` carries `openProjects`, never the
    /// deprecated pre-shim `openProject`). Entries are kept sorted by op name so
    /// the fingerprint is order-stable.
    ///
    /// This is the dimension the op-NAME-only fingerprint was BLIND to: the op
    /// name `updateSnapshot` was unchanged while its param schema flipped
    /// `openProject`→`openProjects`, so the pre-widening fingerprint accepted the
    /// broken codec. Only fields the codec SENDS are scheduled — additive
    /// response fields the codec tolerates-by-ignoring are deliberately excluded
    /// (hashing them would manufacture false breakages on benign upstream drift).
    pub request_fields: &'static [(&'static str, &'static [&'static str])],
    /// The tsgo `--api` binary `PROTOCOL_VERSION` (`node/protocol.d.ts:1`). A
    /// protocol-version bump is a wire event the op/callback inventory cannot
    /// express, so it enters the fingerprint as its own scalar dimension.
    pub protocol_version: u32,
}

impl SchemaManifest {
    /// A deterministic 64-bit fingerprint of the wire shape this manifest
    /// describes. Derived purely from the manifest contents via a stable FNV-1a
    /// hash, so any covered field change flips the fingerprint. It folds the
    /// framing constants, the op-name inventory, the callback-name inventory, the
    /// per-op REQUEST field-name schedule, and the `PROTOCOL_VERSION` scalar.
    ///
    /// It deliberately does NOT depend on the engine version (the gate checks
    /// version and fingerprint as two separate dimensions, so a pure version bump
    /// that keeps the wire identical still matches the fingerprint) — the widened
    /// payload schedule is precisely what makes the broad version-channel accept
    /// safe: a patch that changes the request-payload schema trips the fingerprint
    /// even though its version is accepted.
    pub const fn wire_fingerprint(&self) -> u64 {
        // FNV-1a over a canonical byte stream of the framing + op + callback
        // inventory + request-field schedule + protocol version. `const fn` so the
        // pinned fingerprint is a compile-time constant available to tests without
        // runtime work. Each section is delimited by a distinct high separator
        // byte so section boundaries cannot collide.
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
        // Per-op REQUEST field-name schedule — the payload shapes the codec SENDS.
        // Blindness to this shipped the openProject fork (the op name was
        // unchanged; only its param schema flipped).
        h = fnv1a_byte(h, 0xfd);
        h = fnv1a_request_fields(h, self.request_fields);
        // PROTOCOL_VERSION scalar (little-endian bytes).
        h = fnv1a_byte(h, 0xfa);
        let pv = self.protocol_version.to_le_bytes();
        h = fnv1a_byte(h, pv[0]);
        h = fnv1a_byte(h, pv[1]);
        h = fnv1a_byte(h, pv[2]);
        h = fnv1a_byte(h, pv[3]);
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

/// FNV-1a over a per-op request field-name schedule. Each entry hashes the op
/// name, an op-name/field-list boundary byte, the sorted field-name set, and a
/// per-entry boundary byte — so a field-name change, an op-name change, or an
/// item-boundary shift all flip the fingerprint. The boundary bytes (`0xfb`,
/// `0xfc`) are distinct from every other section separator in
/// [`SchemaManifest::wire_fingerprint`].
const fn fnv1a_request_fields(mut h: u64, schedule: &[(&str, &[&str])]) -> u64 {
    let mut i = 0;
    while i < schedule.len() {
        let entry = schedule[i];
        let op_bytes = entry.0.as_bytes();
        let mut k = 0;
        while k < op_bytes.len() {
            h = fnv1a_byte(h, op_bytes[k]);
            k += 1;
        }
        // Op-name / field-list boundary.
        h = fnv1a_byte(h, 0xfb);
        h = fnv1a_str_list(h, entry.1);
        // Per-entry boundary.
        h = fnv1a_byte(h, 0xfc);
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

/// The per-op REQUEST field-name schedule the codec SENDS — the normalized wire
/// shape AFTER the JS client shims. Entries are sorted by op name, and each
/// field-name set is sorted, so the fingerprint is order-stable. Only ops Verter
/// actually sends appear (the never-sent `echo`/`getDefaultProjectForFile`/
/// `getSourceFile`/`parseConfigFile` ops carry no request-field row).
pub const PINNED_REQUEST_FIELDS: &[(&str, &[&str])] = &[
    ("getConfigFileParsingDiagnostics", &["project", "snapshot"]),
    ("getSemanticDiagnostics", &["file", "project", "snapshot"]),
    (
        "getSymbolAtPosition",
        &["file", "position", "project", "snapshot"],
    ),
    ("getSyntacticDiagnostics", &["file", "project", "snapshot"]),
    (
        "getTypeAtPosition",
        &["file", "position", "project", "snapshot"],
    ),
    ("initialize", &[]),
    ("release", &["snapshot"]),
    ("typeToString", &["project", "snapshot", "type"]),
    (
        "updateSnapshot",
        &[
            "closeFiles",
            "closeProjects",
            "fileChanges",
            "openFiles",
            "openProjects",
        ],
    ),
];

/// The maintained wire pin for the currently supported tsgo `--api` wire.
///
/// On a version bump the maintainer re-verifies the hand-written codec, then
/// updates `engine_version` (and the op/callback inventory if the surface
/// changed). `engine_version` is `typescript@7.0.2` — the REFERENCE build
/// within the accepted version channel
/// ([`crate::gate::classify_engine_version`]), not the sole accepted version:
/// the gate admits every build in the channel, all of which share the
/// bare-integer opaque-handle wire the codec
/// ([`crate::proto::types::OpaqueHandle`]) targets. The op inventory
/// carries `getConfigFileParsingDiagnostics` (the project config-parse /
/// compiler-options diagnostic surface the codec now hand-writes); the
/// fingerprint reflects that op set.
pub const PINNED: SchemaManifest = SchemaManifest {
    engine_version: "7.0.2",
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
    request_fields: PINNED_REQUEST_FIELDS,
    protocol_version: 5,
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

    /// The PRE-widening fingerprint algorithm — framing + op-NAMES + callback-
    /// NAMES ONLY, exactly what shipped the `openProject` fork. Reconstructed here
    /// to PROVE the old scheme was structurally blind to request-payload field
    /// names (it never hashed them), so the widened scheme's gain is demonstrated,
    /// not asserted.
    fn legacy_framing_ops_callbacks_hash(m: &SchemaManifest) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv1a_byte(h, m.framing.fixarray3);
        h = fnv1a_byte(h, m.framing.bin8);
        h = fnv1a_byte(h, m.framing.bin16);
        h = fnv1a_byte(h, m.framing.bin32);
        h = fnv1a_byte(h, m.framing.uint8);
        h = fnv1a_byte(h, m.framing.msg_request);
        h = fnv1a_byte(h, m.framing.msg_response);
        h = fnv1a_byte(h, m.framing.msg_error);
        h = fnv1a_byte(h, m.framing.msg_call);
        h = fnv1a_byte(h, 0xff);
        h = fnv1a_str_list(h, m.ops);
        h = fnv1a_byte(h, 0xfe);
        h = fnv1a_str_list(h, m.callbacks);
        h
    }

    // ── HEADLINE §1a: the widened fingerprint CATCHES the exact request-field
    //    miss the old scheme shipped. Two manifests identical in framing / ops /
    //    callbacks / protocol, differing ONLY in `updateSnapshot`'s request field
    //    name (`openProject` fork vs `openProjects` GA wire). ────────────────────
    #[test]
    fn fingerprint_catches_request_field_flip_that_old_scheme_missed() {
        const BROKEN_SCHEDULE: &[(&str, &[&str])] = &[("updateSnapshot", &["openProject"])];
        const GA_SCHEDULE: &[(&str, &[&str])] = &[("updateSnapshot", &["openProjects"])];

        let mut broken = PINNED;
        broken.request_fields = BROKEN_SCHEDULE;
        let mut ga = PINNED;
        ga.request_fields = GA_SCHEDULE;

        // 1. OLD-SCHEME BLINDNESS: framing + op-names + callbacks are identical, so
        //    the pre-widening fingerprint is EQUAL — exactly why the broken codec
        //    passed the version-witness gate.
        assert_eq!(
            legacy_framing_ops_callbacks_hash(&broken),
            legacy_framing_ops_callbacks_hash(&ga),
            "the pre-widening framing+op-name+callback-only scheme is BLIND to the \
             request field flip — the coverage gap that shipped the openProject fork"
        );

        // 2. WIDENED FINGERPRINT CATCHES IT: folding the request-field schedule in
        //    makes `openProject` vs `openProjects` flip the fingerprint.
        assert_ne!(
            broken.wire_fingerprint(),
            ga.wire_fingerprint(),
            "the widened fingerprint MUST catch an `updateSnapshot` request-payload \
             field-name change the op name cannot express"
        );
    }

    #[test]
    fn fingerprint_changes_when_protocol_version_changes() {
        let mut m = PINNED;
        m.protocol_version = PINNED.protocol_version + 1;
        assert_ne!(
            m.wire_fingerprint(),
            PINNED.wire_fingerprint(),
            "a PROTOCOL_VERSION change must flip the fingerprint"
        );
    }

    #[test]
    fn fingerprint_distinguishes_request_field_item_boundaries() {
        // Inside a request schedule, ["a","bc"] must not hash-collide with
        // ["ab","c"] — the per-item separator keeps field boundaries distinct.
        const SCHED_A: &[(&str, &[&str])] = &[("updateSnapshot", &["a", "bc"])];
        const SCHED_B: &[(&str, &[&str])] = &[("updateSnapshot", &["ab", "c"])];
        let mut ma = PINNED;
        ma.request_fields = SCHED_A;
        let mut mb = PINNED;
        mb.request_fields = SCHED_B;
        assert_ne!(
            ma.wire_fingerprint(),
            mb.wire_fingerprint(),
            "request-field item boundaries must not collide"
        );
    }

    #[test]
    fn pinned_op_inventory_is_sorted() {
        let mut sorted = PINNED_OPS.to_vec();
        sorted.sort_unstable();
        assert_eq!(PINNED_OPS, sorted.as_slice(), "ops must stay sorted");
    }

    #[test]
    fn pinned_request_fields_are_sorted() {
        // Ops sorted by name (order-stable fingerprint), each field set sorted.
        let ops: Vec<&str> = PINNED_REQUEST_FIELDS.iter().map(|(op, _)| *op).collect();
        let mut sorted_ops = ops.clone();
        sorted_ops.sort_unstable();
        assert_eq!(
            ops, sorted_ops,
            "request_fields ops must stay sorted by name"
        );
        for (op, fields) in PINNED_REQUEST_FIELDS {
            let mut sorted_fields = fields.to_vec();
            sorted_fields.sort_unstable();
            assert_eq!(
                *fields, sorted_fields,
                "request field set for `{op}` must stay sorted"
            );
        }
    }
}
