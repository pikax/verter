//! Hand-written tsgo `--api` wire codec.
//!
//! The codec is hand-written (not generated). Each module mirrors a specific
//! shipped reference file from the rc `typescript` package and cites the
//! mirrored lines so a maintainer's version-update agent can re-verify against
//! the same source on a TypeScript bump:
//!
//! - [`msgpack`] mirrors `dist/api/node/msgpack.js` (the minimal MessagePack
//!   subset: 3-element fixarray, unsigned ints, strings, bools, binary).
//! - [`frame`] mirrors `dist/api/syncChannel.js` (the `[type, name, payload]`
//!   tuple framing and the `MessageType` constants).
//! - [`schema_manifest`] is the maintained wire pin (tsgo version + a
//!   normalized fingerprint of the wire shape the codec targets).

pub mod frame;
pub mod msgpack;
pub mod schema_manifest;
pub mod types;
