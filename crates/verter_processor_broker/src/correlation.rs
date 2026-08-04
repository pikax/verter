use std::collections::HashMap;
use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

/// Maximum live correlation entries one registry retains.
pub const MAX_CORRELATION_ENTRIES: usize = 4096;

/// How long a consumed correlation entry is retained for replay rejection.
pub const CONSUMED_CORRELATION_TTL: Duration = Duration::from_secs(300);

const MAX_CORRELATION_AUDIT_EVENTS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DependencyRequestIdV1([u8; 16]);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockContentResolveContextTokenV1([u8; 16]);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockContentWorkTokenV1([u8; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrelationError {
    MalformedRequestId,
    AllZeroRequestId,
    DuplicatePending,
    ReplayConsumed,
    UnknownRequest,
    ContextMismatch,
    WorkMismatch,
    ChannelMismatch,
    /// The registry is at capacity with only pending entries; registration is refused.
    CapacityExhausted,
}

/// Production sink for correlation eviction and destruction audit events.
///
/// The session owner supplies it; every [`CorrelationAuditEvent`] is delivered to it
/// synchronously as it is recorded — including the destruction event recorded by
/// normal session teardown on `Drop` — so eviction and destruction stay observable
/// outside test builds.
pub type CorrelationAuditSink = Box<dyn FnMut(CorrelationAuditEvent) + Send>;

/// Typed audit record for every correlation-entry eviction or destruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrelationAuditEvent {
    ConsumedEvictedByTtl { id: DependencyRequestIdV1 },
    ConsumedEvictedByCapacity { id: DependencyRequestIdV1 },
    RegistrationRefusedAtCapacity { id: DependencyRequestIdV1 },
    DestroyedOnTeardown { pending: usize, consumed: usize },
}

impl DependencyRequestIdV1 {
    pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, CorrelationError> {
        if bytes == [0; 16] {
            return Err(CorrelationError::AllZeroRequestId);
        }
        Ok(Self(bytes))
    }

    pub fn from_base64url(encoded: &str) -> Result<Self, CorrelationError> {
        if encoded.len() != 22 || encoded.bytes().any(|byte| !is_base64url(byte)) {
            return Err(CorrelationError::MalformedRequestId);
        }
        let mut output = [0_u8; 16];
        let mut accumulator = 0_u32;
        let mut bits = 0_u8;
        let mut written = 0_usize;
        for byte in encoded.bytes() {
            accumulator = (accumulator << 6) | u32::from(base64url_value(byte));
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                if written < output.len() {
                    output[written] = (accumulator >> bits) as u8;
                    written += 1;
                }
            }
        }
        if written != 16 || bits != 4 || accumulator & 0x0f != 0 {
            return Err(CorrelationError::MalformedRequestId);
        }
        Self::from_bytes(output)
    }

    #[must_use]
    pub fn to_base64url(self) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut output = String::with_capacity(22);
        let mut accumulator = 0_u32;
        let mut bits = 0_u8;
        for byte in self.0 {
            accumulator = (accumulator << 8) | u32::from(byte);
            bits += 8;
            while bits >= 6 {
                bits -= 6;
                output.push(ALPHABET[((accumulator >> bits) & 0x3f) as usize] as char);
            }
        }
        if bits > 0 {
            output.push(ALPHABET[((accumulator << (6 - bits)) & 0x3f) as usize] as char);
        }
        output
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl BlockContentResolveContextTokenV1 {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl BlockContentWorkTokenV1 {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

pub struct CorrelationRegistry {
    channel_binding_hash: [u8; 32],
    entries: HashMap<DependencyRequestIdV1, CorrelationState>,
    capacity: usize,
    consumed_ttl: Duration,
    audit_events: VecDeque<CorrelationAuditEvent>,
    audit_events_dropped: u64,
    audit_sink: Option<CorrelationAuditSink>,
}

impl fmt::Debug for CorrelationRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CorrelationRegistry")
            .field("entries", &self.entries.len())
            .field("capacity", &self.capacity)
            .field("consumed_ttl", &self.consumed_ttl)
            .field("audit_events", &self.audit_events.len())
            .field("audit_events_dropped", &self.audit_events_dropped)
            .field("audit_sink_installed", &self.audit_sink.is_some())
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
enum CorrelationState {
    Pending {
        context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
    },
    Consumed {
        at: Instant,
    },
}

impl CorrelationRegistry {
    #[must_use]
    pub fn new(channel_binding_hash: [u8; 32]) -> Self {
        Self::with_limits(
            channel_binding_hash,
            MAX_CORRELATION_ENTRIES,
            CONSUMED_CORRELATION_TTL,
        )
    }

    fn with_limits(
        channel_binding_hash: [u8; 32],
        capacity: usize,
        consumed_ttl: Duration,
    ) -> Self {
        Self {
            channel_binding_hash,
            entries: HashMap::new(),
            capacity,
            consumed_ttl,
            audit_events: VecDeque::new(),
            audit_events_dropped: 0,
            audit_sink: None,
        }
    }

    /// Installs the production sink that observes every eviction and destruction
    /// audit event from now on, delivered synchronously as it is recorded.
    pub fn install_audit_sink(&mut self, sink: CorrelationAuditSink) {
        self.audit_sink = Some(sink);
    }

    #[cfg(test)]
    pub(crate) fn with_limits_for_test(
        channel_binding_hash: [u8; 32],
        capacity: usize,
        consumed_ttl: Duration,
    ) -> Self {
        Self::with_limits(channel_binding_hash, capacity, consumed_ttl)
    }

    /// Drains the typed eviction/destruction audit events recorded so far.
    pub fn drain_audit_events(&mut self) -> Vec<CorrelationAuditEvent> {
        self.audit_events.drain(..).collect()
    }

    /// Number of audit events dropped because the bounded audit buffer overflowed.
    #[must_use]
    pub const fn audit_events_dropped(&self) -> u64 {
        self.audit_events_dropped
    }

    /// Destroys every correlation entry on session teardown, recording one audit event.
    pub fn destroy_for_teardown(&mut self) {
        let (pending, consumed) = self.counts();
        self.entries.clear();
        self.record_audit(CorrelationAuditEvent::DestroyedOnTeardown { pending, consumed });
    }

    fn counts(&self) -> (usize, usize) {
        let pending = self
            .entries
            .values()
            .filter(|state| matches!(state, CorrelationState::Pending { .. }))
            .count();
        (pending, self.entries.len() - pending)
    }

    /// Drops consumed entries whose replay-rejection window has elapsed.
    fn evict_expired(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(id, state)| match state {
                CorrelationState::Consumed { at }
                    if now.saturating_duration_since(*at) >= self.consumed_ttl =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        for id in expired {
            self.entries.remove(&id);
            self.record_audit(CorrelationAuditEvent::ConsumedEvictedByTtl { id });
        }
    }

    /// Reclaims one slot by dropping the oldest consumed entry, if any exists.
    fn evict_oldest_consumed(&mut self) -> bool {
        let oldest = self
            .entries
            .iter()
            .filter_map(|(id, state)| match state {
                CorrelationState::Consumed { at } => Some((*at, *id)),
                CorrelationState::Pending { .. } => None,
            })
            .min_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| left.1 .0.cmp(&right.1 .0))
            });
        let Some((_, id)) = oldest else {
            return false;
        };
        self.entries.remove(&id);
        self.record_audit(CorrelationAuditEvent::ConsumedEvictedByCapacity { id });
        true
    }

    fn record_audit(&mut self, event: CorrelationAuditEvent) {
        if let Some(sink) = self.audit_sink.as_mut() {
            sink(event);
        }
        if self.audit_events.len() >= MAX_CORRELATION_AUDIT_EVENTS {
            self.audit_events.pop_front();
            self.audit_events_dropped = self.audit_events_dropped.saturating_add(1);
        }
        self.audit_events.push_back(event);
    }

    #[cfg(test)]
    pub(crate) fn state_counts_for_test(&self) -> (usize, usize) {
        self.counts()
    }

    pub fn register(
        &mut self,
        id: DependencyRequestIdV1,
        context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
    ) -> Result<(), CorrelationError> {
        self.evict_expired(Instant::now());
        match self.entries.get(&id) {
            Some(CorrelationState::Pending { .. }) => {
                return Err(CorrelationError::DuplicatePending);
            }
            Some(CorrelationState::Consumed { .. }) => {
                return Err(CorrelationError::ReplayConsumed);
            }
            None => {}
        }
        if self.entries.len() >= self.capacity && !self.evict_oldest_consumed() {
            self.record_audit(CorrelationAuditEvent::RegistrationRefusedAtCapacity { id });
            return Err(CorrelationError::CapacityExhausted);
        }
        self.entries
            .insert(id, CorrelationState::Pending { context, work });
        Ok(())
    }

    pub fn consume(
        &mut self,
        id: DependencyRequestIdV1,
        context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
        channel_binding_hash: [u8; 32],
    ) -> Result<(), CorrelationError> {
        if channel_binding_hash != self.channel_binding_hash {
            return Err(CorrelationError::ChannelMismatch);
        }
        let state = self
            .entries
            .get_mut(&id)
            .ok_or(CorrelationError::UnknownRequest)?;
        match *state {
            CorrelationState::Consumed { .. } => Err(CorrelationError::ReplayConsumed),
            CorrelationState::Pending {
                context: expected_context,
                work: expected_work,
            } => {
                if context != expected_context {
                    return Err(CorrelationError::ContextMismatch);
                }
                if work != expected_work {
                    return Err(CorrelationError::WorkMismatch);
                }
                *state = CorrelationState::Consumed { at: Instant::now() };
                Ok(())
            }
        }
    }
}

const fn is_base64url(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_')
}

const fn base64url_value(byte: u8) -> u8 {
    match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => 0,
    }
}
