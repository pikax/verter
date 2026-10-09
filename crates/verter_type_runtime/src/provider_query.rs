//! Query-bound provider coordinates.
//!
//! An external engine answers a query against the bytes it holds when it
//! dequeues that query, and every position in the answer names a line and
//! column in those bytes. Converting the request offset against one read of a
//! live content cache and decoding the answer against a second read taken after
//! the await mixes two documents whenever an edit lands in between.
//!
//! A [`DeliveryLedger`] records, for one engine incarnation, the bytes each
//! file carries at the engine in the order the engine receives them. A query
//! binds at the exact wire position of its request frame and receives a
//! [`ProviderQuery`]: the bytes its request position was converted against,
//! plus an immutable snapshot of every other delivered file at that same
//! position. Every range in the answer decodes through that capability;
//! nothing reads the live ledger or a content cache for coordinates after the
//! await.
//!
//! Bytes an engine reads out of band (a store its plugin reads while it
//! evaluates) have no wire position. They are recorded as
//! [`DeliveryOrder::OutOfBand`], and a query that decoded through one re-checks
//! that entry's stamp once the answer is in ([`DeliveryLedger::settle`]): a
//! publication that landed in between is a [`ProviderQueryConflict`], never a
//! decode against bytes the engine may not have evaluated.
//!
//! A file the engine was never handed (it reads the file from disk itself) has
//! no ledger entry. The caller converts against the file's disk bytes, which
//! the query retains for its own decode, and every other undelivered target
//! falls back to its own disk bytes exactly as the engine reads them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// How an engine receives one file's bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryOrder {
    /// A protocol frame carries the bytes, so the engine applies them at that
    /// frame's position in its input stream.
    Wire,
    /// The engine reads the bytes out of band while it evaluates, so no wire
    /// position orders them against a query.
    OutOfBand,
}

/// One file's bytes as one engine incarnation holds them.
#[derive(Clone, Debug)]
struct DeliveredBytes {
    bytes: Arc<str>,
    /// Minted from the ledger's own counter on every record, so a re-record of
    /// identical bytes is still a different delivery.
    stamp: u64,
    order: DeliveryOrder,
}

/// One change to what an engine holds, recorded when it takes effect.
#[derive(Clone, Debug)]
pub enum SurfaceEffect {
    /// The engine now holds `bytes` for `path`.
    Deliver { path: String, bytes: Arc<str> },
    /// The engine no longer holds an overlay for `path`.
    Withdraw { path: String },
}

impl SurfaceEffect {
    #[must_use]
    pub fn deliver(path: impl Into<String>, bytes: Arc<str>) -> Self {
        Self::Deliver {
            path: path.into(),
            bytes,
        }
    }

    #[must_use]
    pub fn withdraw(path: impl Into<String>) -> Self {
        Self::Withdraw { path: path.into() }
    }
}

/// The typed outcome of a query whose coordinates cannot be bound to the bytes
/// the engine evaluated: the requested file was replaced between the request
/// conversion and the request's wire position, or an out-of-band file the
/// answer decodes through was republished before the answer was decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderQueryConflict {
    path: String,
}

impl ProviderQueryConflict {
    /// The file whose delivered bytes moved under the query.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl std::fmt::Display for ProviderQueryConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "provider query coordinates conflict: the delivered bytes of {} moved under the query",
            self.path
        )
    }
}

#[derive(Default)]
struct LedgerState {
    files: imbl::HashMap<String, DeliveredBytes>,
    next_stamp: u64,
}

impl LedgerState {
    fn apply(&mut self, effect: SurfaceEffect, order: DeliveryOrder) {
        match effect {
            SurfaceEffect::Deliver { path, bytes } => {
                if order == DeliveryOrder::OutOfBand {
                    match self.files.get(&path) {
                        // A buffer the engine holds through its own protocol
                        // outranks bytes it would otherwise read out of band.
                        Some(held) if held.order == DeliveryOrder::Wire => return,
                        // Identical out-of-band bytes place every position the
                        // same, so they are the same delivery.
                        Some(held) if held.bytes == bytes => return,
                        _ => {}
                    }
                }
                self.next_stamp += 1;
                self.files.insert(
                    path,
                    DeliveredBytes {
                        bytes,
                        stamp: self.next_stamp,
                        order,
                    },
                );
            }
            SurfaceEffect::Withdraw { path } => {
                if order == DeliveryOrder::OutOfBand
                    && self
                        .files
                        .get(&path)
                        .is_some_and(|held| held.order == DeliveryOrder::Wire)
                {
                    return;
                }
                self.files.remove(&path);
            }
        }
    }

    fn query(&self, path: &str, requested: RequestedBytes) -> ProviderQuery {
        ProviderQuery {
            path: path.to_string(),
            requested,
            surface: self.files.clone(),
        }
    }
}

/// The bytes one query's request position was converted against.
#[derive(Clone, Debug)]
pub struct RequestedBytes {
    bytes: Option<Arc<str>>,
    /// The ledger entry those bytes came from; `None` when the engine was never
    /// handed the file and the bytes are its disk content.
    entry: Option<(u64, DeliveryOrder)>,
}

impl RequestedBytes {
    /// The bytes the request position converts against.
    #[must_use]
    pub fn bytes(&self) -> Option<&Arc<str>> {
        self.bytes.as_ref()
    }

    fn from_entry(entry: Option<&DeliveredBytes>, fallback: Option<Arc<str>>) -> Self {
        match entry {
            Some(entry) => Self {
                bytes: Some(Arc::clone(&entry.bytes)),
                entry: Some((entry.stamp, entry.order)),
            },
            None => Self {
                bytes: fallback,
                entry: None,
            },
        }
    }

    fn stamp(&self) -> Option<u64> {
        self.entry.map(|(stamp, _)| stamp)
    }
}

/// The delivered bytes of one engine incarnation, in the order the engine
/// receives them. Owned by that incarnation's transport and dropped with it, so
/// a replacement engine starts from an empty surface its replay refills.
#[derive(Default)]
pub struct DeliveryLedger {
    state: parking_lot::Mutex<LedgerState>,
}

impl DeliveryLedger {
    /// Record `effects` that take effect at the engine at the current wire
    /// position, running `put_on_wire` (the synchronous enqueue of the frame
    /// that carries them) under the same lock, so no query can bind between
    /// the frame's position and the record.
    pub fn deliver_with<R>(
        &self,
        effects: impl IntoIterator<Item = SurfaceEffect>,
        put_on_wire: impl FnOnce() -> R,
    ) -> R {
        let mut state = self.state.lock();
        let placed = put_on_wire();
        for effect in effects {
            state.apply(effect, DeliveryOrder::Wire);
        }
        placed
    }

    /// Record effects whose frame position is the caller's current position in
    /// a writer that alone orders the wire.
    pub fn record_wire(&self, effect: SurfaceEffect) {
        self.state.lock().apply(effect, DeliveryOrder::Wire);
    }

    /// Record bytes the engine reads out of band. They carry no wire position;
    /// a query decoding through them settles against their stamp. An
    /// out-of-band record never replaces or withdraws a buffer the engine holds
    /// through its protocol, and re-recording identical bytes keeps the stamp.
    pub fn record_out_of_band(&self, effects: impl IntoIterator<Item = SurfaceEffect>) {
        let mut state = self.state.lock();
        for effect in effects {
            state.apply(effect, DeliveryOrder::OutOfBand);
        }
    }

    /// The bytes a request on `path` converts against right now: the engine's
    /// delivered bytes, else `fallback` (the file's disk content).
    #[must_use]
    pub fn requested(&self, path: &str, fallback: Option<Arc<str>>) -> RequestedBytes {
        RequestedBytes::from_entry(self.state.lock().files.get(path), fallback)
    }

    /// Whether the engine holds delivered bytes for `path`.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.state.lock().files.contains_key(path)
    }

    /// Bind one query at its wire position: `put_on_wire` converts the request
    /// against the bytes the engine holds for `path` (else `fallback`) and
    /// synchronously enqueues the frame, all under the ledger lock, so the
    /// returned capability names exactly the surface that frame meets.
    ///
    /// # Errors
    /// Whatever `put_on_wire` returns; no capability is minted for a frame that
    /// never reached the wire.
    pub fn dispatch_with<R, E>(
        &self,
        path: &str,
        fallback: Option<Arc<str>>,
        put_on_wire: impl FnOnce(Option<&str>) -> Result<R, E>,
    ) -> Result<(ProviderQuery, R), E> {
        let state = self.state.lock();
        let requested = RequestedBytes::from_entry(state.files.get(path), fallback);
        let placed = put_on_wire(requested.bytes.as_deref())?;
        Ok((state.query(path, requested), placed))
    }

    /// Bind one query whose request was converted against `requested` before a
    /// writer that alone orders the wire reached its frame. Called by that
    /// writer at the frame's position.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] when the requested file's delivered bytes were
    /// replaced (or delivered for the first time) after the conversion: the
    /// frame must not be written, because the engine would evaluate a position
    /// computed against bytes it no longer holds.
    pub fn bind(
        &self,
        path: &str,
        requested: RequestedBytes,
    ) -> Result<ProviderQuery, ProviderQueryConflict> {
        let state = self.state.lock();
        if state.files.get(path).map(|entry| entry.stamp) != requested.stamp() {
            return Err(ProviderQueryConflict {
                path: path.to_string(),
            });
        }
        Ok(state.query(path, requested))
    }

    /// Settle an answer decoded through `query`: every out-of-band file it
    /// decoded through (the requested file and each of `decoded`) must still
    /// carry the stamp it had at dispatch. Wire-ordered bytes need no check —
    /// the engine evaluated exactly the bytes the capability retained.
    ///
    /// Reads only stamps, never content.
    ///
    /// # Errors
    /// [`ProviderQueryConflict`] naming the first out-of-band file that moved.
    pub fn settle<'a>(
        &self,
        query: &ProviderQuery,
        decoded: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ProviderQueryConflict> {
        let mut out_of_band: Vec<(&str, u64)> = Vec::new();
        if let Some((stamp, DeliveryOrder::OutOfBand)) = query.requested.entry {
            out_of_band.push((&query.path, stamp));
        }
        for path in decoded {
            if path == query.path {
                continue;
            }
            if let Some(entry) = query.surface.get(path) {
                if entry.order == DeliveryOrder::OutOfBand {
                    out_of_band.push((path, entry.stamp));
                }
            }
        }
        if out_of_band.is_empty() {
            return Ok(());
        }
        let state = self.state.lock();
        for (path, stamp) in out_of_band {
            if state.files.get(path).map(|entry| entry.stamp) != Some(stamp) {
                return Err(ProviderQueryConflict {
                    path: path.to_string(),
                });
            }
        }
        Ok(())
    }
}

/// The coordinate capability of one provider query: the bytes its request
/// position was converted against and the immutable delivered surface its
/// request frame met. Every response range decodes through it.
#[derive(Clone, Debug)]
pub struct ProviderQuery {
    path: String,
    requested: RequestedBytes,
    surface: imbl::HashMap<String, DeliveredBytes>,
}

impl ProviderQuery {
    /// The file the request names.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The bytes the request position was converted against, retained for the
    /// decode of every range in the same file.
    #[must_use]
    pub fn requested(&self) -> Option<&Arc<str>> {
        self.requested.bytes()
    }

    /// The bytes `path` carried at the engine when the request reached it, or
    /// `None` for a file the engine reads from disk itself.
    #[must_use]
    pub fn delivered(&self, path: &str) -> Option<&Arc<str>> {
        if path == self.path {
            return self.requested();
        }
        self.surface.get(path).map(|entry| &entry.bytes)
    }

    /// The delivered bytes of exactly `paths`, for decoders that take a
    /// path-keyed map. A path the engine reads from disk is omitted, so the
    /// decoder's own disk fallback reads it exactly as the engine does.
    #[must_use]
    pub fn targeted(&self, paths: &HashSet<String>) -> HashMap<String, Arc<str>> {
        paths
            .iter()
            .filter_map(|path| {
                self.delivered(path)
                    .map(|bytes| (path.clone(), Arc::clone(bytes)))
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "provider_query_tests.rs"]
mod tests;
