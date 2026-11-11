//! Canonical native-observation encoding of committed fact evidence.
//!
//! A typeinfo fact's native input basis commits the exact observations its
//! computation consumed. Each [`FactVersionRef`] leaf becomes one
//! [`NativeObservation`]: a typed KEY naming what was observed and the
//! REVISION the observing scope saw. The owning type writes both (see
//! `FactVersionRef::native_observation`, the resolution keys'
//! `encode_native` and `ResultReceipt::native_observations_into`), so the
//! private fields of a resolution query key reach the encoding without a
//! public accessor.
//!
//! The encoding is canonical: explicit one-byte tags assigned here, never
//! `Debug`, Rust `Hash`, receipt digests or declaration-order
//! discriminants; every variable-length field is length-prefixed so
//! neighbours cannot fuse. Counter-based revisions (resolution fact
//! versions, generations, aggregate stamps) are namespaced by the issuing
//! host's [`NativeAuthority`], so equal counters from two hosts never
//! alias.
//!
//! Receipts are EXPANDED: a consumed result's evidence contributes its
//! leaves, each distinct receipt walked once, so grouping the same facts
//! under different receipts yields the same observation set. Terminal
//! domain aggregates keep their complete population and stamp.

use std::sync::Arc;

use super::receipt::ReceiptWalk;
use super::version::FactVersionRef;

/// The host authority that minted the counter-based revisions an
/// observation carries. Process-unique per host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeAuthority(u64);

impl NativeAuthority {
    /// The authority with raw identity `raw`.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Raw identity.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One committed native observation: the canonical key bytes and the
/// canonical revision bytes observed for it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeObservation {
    key: Arc<[u8]>,
    revision: Arc<[u8]>,
}

impl NativeObservation {
    /// An observation from already-encoded key and revision bytes.
    #[must_use]
    pub fn new(key: impl Into<Arc<[u8]>>, revision: impl Into<Arc<[u8]>>) -> Self {
        Self {
            key: key.into(),
            revision: revision.into(),
        }
    }

    /// Canonical key bytes: what was observed.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Canonical revision bytes: the value observed for the key.
    #[must_use]
    pub fn revision(&self) -> &[u8] {
        &self.revision
    }

    /// Bytes this observation retains.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.key.len() + self.revision.len()
    }
}

/// Why a fact could not be encoded canonically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEncodingError(pub(crate) &'static str);

impl std::fmt::Display for NativeEncodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native observation encoding failed: {}", self.0)
    }
}

impl std::error::Error for NativeEncodingError {}

/// Tagged, length-prefixed canonical writer shared by the owning types.
pub struct NativeWriter {
    buf: Vec<u8>,
}

impl NativeWriter {
    /// A writer whose output starts with `domain`, so two encodings of
    /// different kinds never share bytes.
    #[must_use]
    pub fn new(domain: &'static str) -> Self {
        let mut writer = Self {
            buf: Vec::with_capacity(64),
        };
        writer.str(domain);
        writer
    }

    /// One explicit tag byte.
    pub fn tag(&mut self, tag: u8) -> &mut Self {
        self.buf.push(tag);
        self
    }

    /// A fixed-width little-endian `u64`.
    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// A fixed-width little-endian `u32`.
    pub fn u32(&mut self, value: u32) -> &mut Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// `0`/`1`.
    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.buf.push(u8::from(value));
        self
    }

    /// Length-prefixed bytes.
    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.u64(value.len() as u64);
        self.buf.extend_from_slice(value);
        self
    }

    /// Length-prefixed UTF-8, exact bytes.
    pub fn str(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    /// Presence byte, then the value when present.
    pub fn option<T>(&mut self, value: Option<T>, write: impl FnOnce(&mut Self, T)) -> &mut Self {
        match value {
            None => self.tag(0),
            Some(value) => {
                self.tag(1);
                write(self, value);
                self
            }
        }
    }

    /// A value whose derived `Serialize` is its canonical structural form:
    /// field and variant names are part of the bytes and the encoded types
    /// hold no maps, so the JSON text is a pure, injective function of the
    /// value. Length-prefixed like every other field.
    pub fn serde<T: serde::Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<&mut Self, NativeEncodingError> {
        let text = serde_json::to_vec(value)
            .map_err(|_| NativeEncodingError("a typed fact key did not serialise"))?;
        Ok(self.bytes(&text))
    }

    /// The finished bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Every native observation `facts` reaches, receipts expanded (each
/// distinct receipt once), sorted and deduplicated.
///
/// Cost is proportional to the evidence reachable from `facts`; nothing
/// is re-read or re-derived.
pub fn native_observations<'a>(
    facts: impl IntoIterator<Item = &'a FactVersionRef>,
    authority: NativeAuthority,
) -> Result<Vec<NativeObservation>, NativeEncodingError> {
    let mut walk = ReceiptWalk::default();
    let mut out = Vec::new();
    for fact in facts {
        match fact {
            FactVersionRef::Receipt(receipt) => {
                receipt.native_observations_into(&mut walk, authority, &mut out)?;
            }
            leaf => out.push(leaf.native_observation(authority)?),
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Canonical bytes of a validating view's population: base, a session
/// overlay set, or a request-completion overlay with its revision.
pub fn view_population_bytes(population: super::version::ViewPopulation) -> Vec<u8> {
    let mut writer = NativeWriter::new("verter.native_view_population.v1");
    super::version::encode_view_population(&mut writer, population);
    writer.finish()
}

#[cfg(test)]
#[path = "native_observation_tests.rs"]
mod tests;
