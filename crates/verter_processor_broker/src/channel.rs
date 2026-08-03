use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use snow::{Builder, HandshakeState, TransportState};

use crate::attestation::{domain_hash, ProcessorBrokerInstanceId};

const NOISE_PATTERN: &str = "Noise_KKpsk0_25519_ChaChaPoly_SHA256";
const MAX_FRAME_PAYLOAD: usize = 60 * 1024;
const MAX_NOISE_MESSAGE: usize = 65_535;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedBrokerChannelBindingV1 {
    broker_instance_token: ProcessorBrokerInstanceId,
    broker_ephemeral_public_key: [u8; 32],
    worker_ephemeral_public_key: [u8; 32],
    launch_nonce: [u8; 16],
    handshake_transcript_hash: [u8; 32],
    broker_attestation_hash: [u8; 32],
    worker_attestation_hash: [u8; 32],
}

impl TrustedBrokerChannelBindingV1 {
    #[must_use]
    pub const fn handshake_transcript_hash(&self) -> [u8; 32] {
        self.handshake_transcript_hash
    }

    #[must_use]
    pub const fn broker_ephemeral_public_key(&self) -> [u8; 32] {
        self.broker_ephemeral_public_key
    }

    #[must_use]
    pub const fn worker_ephemeral_public_key(&self) -> [u8; 32] {
        self.worker_ephemeral_public_key
    }

    pub(crate) fn canonical_transcript(inputs: &ChannelBindingInputs) -> Vec<u8> {
        let mut transcript = Vec::with_capacity(241);
        transcript.extend_from_slice(b"verter.trusted-broker-channel-binding.v1\0");
        transcript.extend_from_slice(inputs.broker_instance_token.as_bytes());
        transcript.extend_from_slice(&inputs.launch_nonce);
        transcript.extend_from_slice(&inputs.broker_attestation_hash);
        transcript.extend_from_slice(&inputs.worker_attestation_hash);
        transcript.extend_from_slice(&inputs.manifest_hash);
        transcript.extend_from_slice(&inputs.sandbox_profile_hash);
        transcript.extend_from_slice(&inputs.broker_ephemeral_public_key);
        transcript.extend_from_slice(&inputs.worker_ephemeral_public_key);
        transcript
    }

    pub(crate) fn from_transcript(inputs: ChannelBindingInputs) -> (Self, Vec<u8>) {
        let transcript = Self::canonical_transcript(&inputs);
        let handshake_transcript_hash = domain_hash(b"channel-transcript\0", &[&transcript]);
        (
            Self {
                broker_instance_token: inputs.broker_instance_token,
                broker_ephemeral_public_key: inputs.broker_ephemeral_public_key,
                worker_ephemeral_public_key: inputs.worker_ephemeral_public_key,
                launch_nonce: inputs.launch_nonce,
                handshake_transcript_hash,
                broker_attestation_hash: inputs.broker_attestation_hash,
                worker_attestation_hash: inputs.worker_attestation_hash,
            },
            transcript,
        )
    }
}

pub(crate) struct ChannelBindingInputs {
    pub broker_instance_token: ProcessorBrokerInstanceId,
    pub broker_ephemeral_public_key: [u8; 32],
    pub worker_ephemeral_public_key: [u8; 32],
    pub launch_nonce: [u8; 16],
    pub broker_attestation_hash: [u8; 32],
    pub worker_attestation_hash: [u8; 32],
    pub manifest_hash: [u8; 32],
    pub sandbox_profile_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChannelError {
    HandshakeAuthenticationFailed,
    InvalidKey,
    FrameTooLarge,
    TruncatedFrame,
    AuthenticationFailed,
    ReplayOrReorder { expected: u64, received: u64 },
    SequenceExhausted,
    Io(String),
    Poisoned,
}

impl From<std::io::Error> for ChannelError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

/// A channel state that can only be produced by the authenticated handshake.
pub struct ValidatedBrokerChannel {
    binding: TrustedBrokerChannelBindingV1,
    transport: Arc<Mutex<TransportState>>,
    send_sequence: u64,
    receive_sequence: u64,
}

impl std::fmt::Debug for ValidatedBrokerChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedBrokerChannel")
            .field("binding", &self.binding)
            .field("send_sequence", &self.send_sequence)
            .field("receive_sequence", &self.receive_sequence)
            .finish_non_exhaustive()
    }
}

impl ValidatedBrokerChannel {
    pub(crate) fn new(binding: TrustedBrokerChannelBindingV1, transport: TransportState) -> Self {
        Self {
            binding,
            transport: Arc::new(Mutex::new(transport)),
            send_sequence: 0,
            receive_sequence: 0,
        }
    }

    #[must_use]
    pub const fn binding(&self) -> &TrustedBrokerChannelBindingV1 {
        &self.binding
    }

    pub(crate) fn encode(&mut self, payload: &[u8]) -> Result<Vec<u8>, ChannelError> {
        if payload.len() > MAX_FRAME_PAYLOAD {
            return Err(ChannelError::FrameTooLarge);
        }
        let sequence = self.send_sequence;
        self.send_sequence = self
            .send_sequence
            .checked_add(1)
            .ok_or(ChannelError::SequenceExhausted)?;
        let mut plain = Vec::with_capacity(8 + payload.len());
        plain.extend_from_slice(&sequence.to_be_bytes());
        plain.extend_from_slice(payload);
        let mut cipher = vec![0_u8; plain.len() + 16];
        let written = self
            .transport
            .lock()
            .map_err(|_| ChannelError::Poisoned)?
            .write_message(&plain, &mut cipher)
            .map_err(|_| ChannelError::AuthenticationFailed)?;
        cipher.truncate(written);
        let wire_length = 8_usize
            .checked_add(cipher.len())
            .ok_or(ChannelError::FrameTooLarge)?;
        let wire_length = u32::try_from(wire_length).map_err(|_| ChannelError::FrameTooLarge)?;
        let mut frame = Vec::with_capacity(4 + wire_length as usize);
        frame.extend_from_slice(&wire_length.to_be_bytes());
        frame.extend_from_slice(&sequence.to_be_bytes());
        frame.extend_from_slice(&cipher);
        Ok(frame)
    }

    pub(crate) fn decode(&mut self, frame: &[u8]) -> Result<Vec<u8>, ChannelError> {
        if frame.len() < 12 {
            return Err(ChannelError::TruncatedFrame);
        }
        let declared = u32::from_be_bytes(frame[..4].try_into().expect("four bytes")) as usize;
        if declared != frame.len() - 4 {
            return Err(ChannelError::TruncatedFrame);
        }
        if declared > MAX_NOISE_MESSAGE + 8 {
            return Err(ChannelError::FrameTooLarge);
        }
        let received = u64::from_be_bytes(frame[4..12].try_into().expect("eight bytes"));
        if received != self.receive_sequence {
            return Err(ChannelError::ReplayOrReorder {
                expected: self.receive_sequence,
                received,
            });
        }
        let mut plain = vec![0_u8; frame.len()];
        let read = self
            .transport
            .lock()
            .map_err(|_| ChannelError::Poisoned)?
            .read_message(&frame[12..], &mut plain)
            .map_err(|_| ChannelError::AuthenticationFailed)?;
        plain.truncate(read);
        if plain.len() < 8 {
            return Err(ChannelError::TruncatedFrame);
        }
        let authenticated_sequence =
            u64::from_be_bytes(plain[..8].try_into().expect("eight bytes"));
        if authenticated_sequence != received {
            return Err(ChannelError::AuthenticationFailed);
        }
        self.receive_sequence = self
            .receive_sequence
            .checked_add(1)
            .ok_or(ChannelError::SequenceExhausted)?;
        Ok(plain.split_off(8))
    }

    pub(crate) fn write_frame(
        &mut self,
        writer: &mut impl Write,
        payload: &[u8],
    ) -> Result<(), ChannelError> {
        let frame = self.encode(payload)?;
        writer.write_all(&frame)?;
        writer.flush()?;
        Ok(())
    }

    pub(crate) fn read_frame(&mut self, reader: &mut impl Read) -> Result<Vec<u8>, ChannelError> {
        let mut length = [0_u8; 4];
        reader.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > MAX_NOISE_MESSAGE + 8 {
            return Err(ChannelError::FrameTooLarge);
        }
        let mut frame = vec![0_u8; 4 + length];
        frame[..4].copy_from_slice(&(length as u32).to_be_bytes());
        reader.read_exact(&mut frame[4..])?;
        self.decode(&frame)
    }

    #[cfg(test)]
    pub(crate) fn encode_for_test(&mut self, payload: &[u8]) -> Result<Vec<u8>, ChannelError> {
        self.encode(payload)
    }

    #[cfg(test)]
    pub(crate) fn decode_for_test(&mut self, frame: &[u8]) -> Result<Vec<u8>, ChannelError> {
        self.decode(frame)
    }
}

#[cfg(test)]
pub(crate) struct HandshakeInputs {
    pub broker_instance: ProcessorBrokerInstanceId,
    pub launch_nonce: [u8; 16],
    pub secret: [u8; 32],
    pub broker_attestation_hash: [u8; 32],
    pub worker_attestation_hash: [u8; 32],
    pub manifest_hash: [u8; 32],
    pub sandbox_profile_hash: [u8; 32],
}

pub(crate) fn generate_ephemeral_keypair() -> Result<snow::Keypair, ChannelError> {
    Builder::new(
        NOISE_PATTERN
            .parse()
            .map_err(|_| ChannelError::InvalidKey)?,
    )
    .generate_keypair()
    .map_err(|_| ChannelError::InvalidKey)
}

pub(crate) fn build_handshake(
    initiator: bool,
    local_private: &[u8],
    remote_public: &[u8],
    prologue: &[u8],
    secret: &[u8; 32],
) -> Result<HandshakeState, ChannelError> {
    let params = NOISE_PATTERN
        .parse()
        .map_err(|_| ChannelError::InvalidKey)?;
    let builder = Builder::new(params)
        .local_private_key(local_private)
        .map_err(|_| ChannelError::InvalidKey)?
        .remote_public_key(remote_public)
        .map_err(|_| ChannelError::InvalidKey)?
        .prologue(prologue)
        .map_err(|_| ChannelError::InvalidKey)?
        .psk(0, secret)
        .map_err(|_| ChannelError::InvalidKey)?;
    if initiator {
        builder.build_initiator()
    } else {
        builder.build_responder()
    }
    .map_err(|_| ChannelError::InvalidKey)
}

pub(crate) fn write_handshake_message(
    state: &mut HandshakeState,
    payload: &[u8],
) -> Result<Vec<u8>, ChannelError> {
    let mut output = vec![0_u8; MAX_NOISE_MESSAGE];
    let written = state
        .write_message(payload, &mut output)
        .map_err(|_| ChannelError::HandshakeAuthenticationFailed)?;
    output.truncate(written);
    Ok(output)
}

pub(crate) fn read_handshake_message(
    state: &mut HandshakeState,
    message: &[u8],
) -> Result<Vec<u8>, ChannelError> {
    let mut output = vec![0_u8; MAX_NOISE_MESSAGE];
    let read = state
        .read_message(message, &mut output)
        .map_err(|_| ChannelError::HandshakeAuthenticationFailed)?;
    output.truncate(read);
    Ok(output)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum HandshakeMutation {
        Nonce,
        Secret,
        Transcript,
    }

    pub(crate) fn establish_pair(
        mutation: Option<HandshakeMutation>,
    ) -> Result<(ValidatedBrokerChannel, ValidatedBrokerChannel), ChannelError> {
        let broker_key = generate_ephemeral_keypair()?;
        let worker_key = generate_ephemeral_keypair()?;
        let inputs = HandshakeInputs {
            broker_instance: ProcessorBrokerInstanceId::from_bytes([1; 16]),
            launch_nonce: [2; 16],
            secret: [3; 32],
            broker_attestation_hash: [4; 32],
            worker_attestation_hash: [5; 32],
            manifest_hash: [6; 32],
            sandbox_profile_hash: [7; 32],
        };
        let broker_public = broker_key
            .public
            .as_slice()
            .try_into()
            .map_err(|_| ChannelError::InvalidKey)?;
        let worker_public = worker_key
            .public
            .as_slice()
            .try_into()
            .map_err(|_| ChannelError::InvalidKey)?;
        let (binding, broker_prologue) =
            TrustedBrokerChannelBindingV1::from_transcript(ChannelBindingInputs {
                broker_instance_token: inputs.broker_instance,
                broker_ephemeral_public_key: broker_public,
                worker_ephemeral_public_key: worker_public,
                launch_nonce: inputs.launch_nonce,
                broker_attestation_hash: inputs.broker_attestation_hash,
                worker_attestation_hash: inputs.worker_attestation_hash,
                manifest_hash: inputs.manifest_hash,
                sandbox_profile_hash: inputs.sandbox_profile_hash,
            });
        let mut worker_inputs = HandshakeInputs { ..inputs };
        let mut worker_prologue = broker_prologue.clone();
        match mutation {
            Some(HandshakeMutation::Nonce) => worker_inputs.launch_nonce[0] ^= 1,
            Some(HandshakeMutation::Secret) => worker_inputs.secret[0] ^= 1,
            Some(HandshakeMutation::Transcript) => worker_prologue[0] ^= 1,
            None => {}
        }
        if matches!(mutation, Some(HandshakeMutation::Nonce)) {
            let (_, changed) =
                TrustedBrokerChannelBindingV1::from_transcript(ChannelBindingInputs {
                    broker_instance_token: worker_inputs.broker_instance,
                    broker_ephemeral_public_key: broker_public,
                    worker_ephemeral_public_key: worker_public,
                    launch_nonce: worker_inputs.launch_nonce,
                    broker_attestation_hash: worker_inputs.broker_attestation_hash,
                    worker_attestation_hash: worker_inputs.worker_attestation_hash,
                    manifest_hash: worker_inputs.manifest_hash,
                    sandbox_profile_hash: worker_inputs.sandbox_profile_hash,
                });
            worker_prologue = changed;
        }
        let mut broker = build_handshake(
            true,
            &broker_key.private,
            &worker_key.public,
            &broker_prologue,
            &inputs.secret,
        )?;
        let mut worker = build_handshake(
            false,
            &worker_key.private,
            &broker_key.public,
            &worker_prologue,
            &worker_inputs.secret,
        )?;
        let first = write_handshake_message(&mut broker, b"broker")?;
        read_handshake_message(&mut worker, &first)?;
        let second = write_handshake_message(&mut worker, b"worker")?;
        read_handshake_message(&mut broker, &second)?;
        let broker_transport = broker
            .into_transport_mode()
            .map_err(|_| ChannelError::HandshakeAuthenticationFailed)?;
        let worker_transport = worker
            .into_transport_mode()
            .map_err(|_| ChannelError::HandshakeAuthenticationFailed)?;
        Ok((
            ValidatedBrokerChannel::new(binding.clone(), broker_transport),
            ValidatedBrokerChannel::new(binding, worker_transport),
        ))
    }
}
