use std::collections::HashMap;

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

#[derive(Debug)]
pub struct CorrelationRegistry {
    channel_binding_hash: [u8; 32],
    entries: HashMap<DependencyRequestIdV1, CorrelationState>,
}

#[derive(Clone, Copy, Debug)]
enum CorrelationState {
    Pending {
        context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
    },
    Consumed,
}

impl CorrelationRegistry {
    #[must_use]
    pub fn new(channel_binding_hash: [u8; 32]) -> Self {
        Self {
            channel_binding_hash,
            entries: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        id: DependencyRequestIdV1,
        context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
    ) -> Result<(), CorrelationError> {
        match self.entries.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(CorrelationState::Pending { context, work });
                Ok(())
            }
            std::collections::hash_map::Entry::Occupied(entry) => match entry.get() {
                CorrelationState::Pending { .. } => Err(CorrelationError::DuplicatePending),
                CorrelationState::Consumed => Err(CorrelationError::ReplayConsumed),
            },
        }
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
            CorrelationState::Consumed => Err(CorrelationError::ReplayConsumed),
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
                *state = CorrelationState::Consumed;
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
