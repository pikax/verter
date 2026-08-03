use std::collections::VecDeque;

use crate::policy::DependencyReadKind;
use crate::work::{TrustedBrokerProcessingFailure, MAX_DEPENDENCY_REQUEST_DESCRIPTOR_BYTES};

const EXECUTION_ENVELOPE_MAGIC: &[u8] = b"VERTER-EXECUTION-1\0";

pub(crate) struct WorkerExecutionEnvelope {
    initial_output: Vec<u8>,
    dependencies: VecDeque<WorkerDependencyRead>,
}

pub(crate) struct WorkerDependencyRead {
    pub kind: DependencyReadKind,
    pub descriptor: Vec<u8>,
}

pub(crate) enum WorkerExecutionEvent {
    Suspend(WorkerDependencyRead),
    Complete(Vec<u8>),
}

pub(crate) struct WorkerExecutionMachine {
    state: WorkerExecutionState,
}

enum WorkerExecutionState {
    Ready {
        output: Vec<u8>,
        dependencies: VecDeque<WorkerDependencyRead>,
    },
    Suspended {
        output: Vec<u8>,
        dependencies: VecDeque<WorkerDependencyRead>,
    },
    Terminal,
}

impl WorkerExecutionEnvelope {
    pub fn decode(input: &[u8]) -> Result<Self, TrustedBrokerProcessingFailure> {
        if input.is_empty() {
            return Err(TrustedBrokerProcessingFailure::MalformedDescriptor);
        }
        let Some(mut remaining) = input.strip_prefix(EXECUTION_ENVELOPE_MAGIC) else {
            return Err(TrustedBrokerProcessingFailure::UnknownDescriptor);
        };
        let initial_output = read_bytes(&mut remaining)?.to_vec();
        let dependency_count = read_u32(&mut remaining)? as usize;
        let mut dependencies = VecDeque::with_capacity(dependency_count);
        for _ in 0..dependency_count {
            let kind = DependencyReadKind::from_wire(read_byte(&mut remaining)?)
                .ok_or(TrustedBrokerProcessingFailure::MalformedDescriptor)?;
            let descriptor = read_bytes(&mut remaining)?.to_vec();
            if descriptor.len() > MAX_DEPENDENCY_REQUEST_DESCRIPTOR_BYTES {
                return Err(TrustedBrokerProcessingFailure::MalformedDescriptor);
            }
            dependencies.push_back(WorkerDependencyRead { kind, descriptor });
        }
        if !remaining.is_empty() {
            return Err(TrustedBrokerProcessingFailure::MalformedDescriptor);
        }
        Ok(Self {
            initial_output,
            dependencies,
        })
    }
}

impl WorkerExecutionMachine {
    pub fn new(envelope: WorkerExecutionEnvelope) -> Self {
        Self {
            state: WorkerExecutionState::Ready {
                output: envelope.initial_output,
                dependencies: envelope.dependencies,
            },
        }
    }

    pub fn next_event(&mut self) -> WorkerExecutionEvent {
        let WorkerExecutionState::Ready {
            output,
            mut dependencies,
        } = std::mem::replace(&mut self.state, WorkerExecutionState::Terminal)
        else {
            unreachable!("execution can advance only while ready");
        };
        if let Some(request) = dependencies.pop_front() {
            self.state = WorkerExecutionState::Suspended {
                output,
                dependencies,
            };
            WorkerExecutionEvent::Suspend(request)
        } else {
            WorkerExecutionEvent::Complete(output)
        }
    }

    pub fn resume(&mut self, bytes: Vec<u8>) {
        let WorkerExecutionState::Suspended {
            mut output,
            dependencies,
        } = std::mem::replace(&mut self.state, WorkerExecutionState::Terminal)
        else {
            unreachable!("execution can resume only while suspended");
        };
        output.extend_from_slice(&bytes);
        self.state = WorkerExecutionState::Ready {
            output,
            dependencies,
        };
    }
}

fn read_byte(input: &mut &[u8]) -> Result<u8, TrustedBrokerProcessingFailure> {
    let Some((&byte, remaining)) = input.split_first() else {
        return Err(TrustedBrokerProcessingFailure::MalformedDescriptor);
    };
    *input = remaining;
    Ok(byte)
}

fn read_u32(input: &mut &[u8]) -> Result<u32, TrustedBrokerProcessingFailure> {
    if input.len() < 4 {
        return Err(TrustedBrokerProcessingFailure::MalformedDescriptor);
    }
    let (value, remaining) = input.split_at(4);
    *input = remaining;
    Ok(u32::from_be_bytes(
        value.try_into().expect("length checked"),
    ))
}

fn read_bytes<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], TrustedBrokerProcessingFailure> {
    let length = read_u32(input)? as usize;
    if input.len() < length {
        return Err(TrustedBrokerProcessingFailure::MalformedDescriptor);
    }
    let (value, remaining) = input.split_at(length);
    *input = remaining;
    Ok(value)
}
