use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::attestation::AttestationFields;
use crate::channel::test_support::{establish_pair, HandshakeMutation};
use crate::lifecycle::test_support::{worker_executable, WorkerProbe};
use crate::work::{
    MAX_DEPENDENCY_BYTES_PER_WORK, MAX_DEPENDENCY_READS_PER_WORK,
    MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES,
};

fn hash(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn manifest() -> TrustedProcessorCapabilityManifest {
    TrustedProcessorCapabilityManifest::denied(hash(1), hash(2), [DependencyReadKind::Source])
}

#[test]
fn manifest_is_a_closed_denied_policy() {
    let manifest = manifest();
    assert!(manifest.ambient_filesystem_denied());
    assert!(manifest.ambient_network_denied());
    assert!(manifest.child_process_denied());
    assert!(manifest.native_addon_loading_denied());
    assert!(manifest.environment_access_denied());
    assert!(manifest.ambient_package_resolution_denied());
    assert!(manifest.dynamic_module_loading_is_dependency_only());
}

#[test]
fn module_graph_is_canonical_and_complete() {
    let ordered = CanonicalModuleGraph::new([
        ModuleGraphEntry::new("a", hash(3), ["b"]),
        ModuleGraphEntry::new("b", hash(4), std::iter::empty::<&str>()),
    ])
    .expect("ordered graph");
    assert_ne!(ordered.hash(), hash(0));

    let err = CanonicalModuleGraph::new([
        ModuleGraphEntry::new("b", hash(4), std::iter::empty::<&str>()),
        ModuleGraphEntry::new("a", hash(3), ["b"]),
    ])
    .expect_err("non-canonical order must reject");
    assert_eq!(err, LaunchEvidenceError::NonCanonicalModuleOrder);
}

#[test]
fn attestation_hash_covers_every_canonical_field() {
    let base = TrustedProcessorAttestation::new_for_test(AttestationFields {
        broker_instance: ProcessorBrokerInstanceId::from_bytes([1; 16]),
        launch_nonce: [2; 16],
        executable_hash: hash(3),
        config_hash: hash(4),
        module_graph_hash: hash(5),
        os_sandbox_kind: ProcessorSandboxKindV1::current(),
        sandbox_profile_hash: hash(6),
        manifest_hash: hash(7),
    });
    let base_hash = base.canonical_hash();
    for changed in base.single_field_mutations_for_test() {
        assert_ne!(changed.canonical_hash(), base_hash);
    }
}

#[test]
fn handshake_rejects_wrong_nonce_key_and_transcript() {
    for mutation in [
        HandshakeMutation::Nonce,
        HandshakeMutation::Secret,
        HandshakeMutation::Transcript,
    ] {
        let err = establish_pair(Some(mutation)).expect_err("mutation must reject");
        assert!(matches!(err, ChannelError::HandshakeAuthenticationFailed));
    }
    establish_pair(None).expect("control handshake");
}

#[test]
fn authenticated_frames_reject_replay_reorder_and_truncation() {
    let (mut broker, mut worker) = establish_pair(None).expect("handshake");
    let first = broker.encode_for_test(b"one").expect("first");
    assert_eq!(
        worker.decode_for_test(&first).expect("first decode"),
        b"one"
    );

    let replay = worker.decode_for_test(&first).expect_err("replay");
    assert!(matches!(replay, ChannelError::ReplayOrReorder { .. }));

    let (mut broker, mut worker) = establish_pair(None).expect("handshake");
    let first = broker.encode_for_test(b"one").expect("first");
    let second = broker.encode_for_test(b"two").expect("second");
    let reorder = worker.decode_for_test(&second).expect_err("reorder");
    assert!(matches!(reorder, ChannelError::ReplayOrReorder { .. }));
    assert_eq!(worker.decode_for_test(&first).expect("control"), b"one");

    let (mut broker, mut worker) = establish_pair(None).expect("handshake");
    let mut truncated = broker.encode_for_test(b"one").expect("frame");
    truncated.pop();
    assert!(matches!(
        worker.decode_for_test(&truncated),
        Err(ChannelError::TruncatedFrame)
    ));
}

#[test]
fn correlation_registry_allows_exactly_pending_to_consumed() {
    let mut registry = CorrelationRegistry::new(hash(9));
    let id = DependencyRequestIdV1::from_bytes([1; 16]).expect("id");
    let context = BlockContentResolveContextTokenV1::from_bytes([2; 16]);
    let work = BlockContentWorkTokenV1::from_bytes([3; 16]);
    registry.register(id, context, work).expect("pending");
    assert_eq!(
        registry.register(id, context, work),
        Err(CorrelationError::DuplicatePending)
    );
    registry
        .consume(id, context, work, hash(9))
        .expect("consume");
    assert_eq!(
        registry.consume(id, context, work, hash(9)),
        Err(CorrelationError::ReplayConsumed)
    );
}

#[test]
fn correlation_rejects_cross_context_work_channel_and_malformed_ids() {
    assert_eq!(
        DependencyRequestIdV1::from_bytes([0; 16]),
        Err(CorrelationError::AllZeroRequestId)
    );
    assert!(matches!(
        DependencyRequestIdV1::from_base64url("short"),
        Err(CorrelationError::MalformedRequestId)
    ));

    let mut registry = CorrelationRegistry::new(hash(9));
    let id = DependencyRequestIdV1::from_bytes([1; 16]).expect("id");
    let context = BlockContentResolveContextTokenV1::from_bytes([2; 16]);
    let work = BlockContentWorkTokenV1::from_bytes([3; 16]);
    registry.register(id, context, work).expect("pending");
    assert_eq!(
        registry.consume(
            id,
            BlockContentResolveContextTokenV1::from_bytes([4; 16]),
            work,
            hash(9),
        ),
        Err(CorrelationError::ContextMismatch)
    );
    assert_eq!(
        registry.consume(
            id,
            context,
            BlockContentWorkTokenV1::from_bytes([4; 16]),
            hash(9),
        ),
        Err(CorrelationError::WorkMismatch)
    );
    assert_eq!(
        registry.consume(id, context, work, hash(8)),
        Err(CorrelationError::ChannelMismatch)
    );
}

fn launch() -> Result<DeniedWorkerSession, BrokerError> {
    let executable = worker_executable();
    let launch = DeniedWorkerLaunch::new(
        &executable,
        b"{}".to_vec(),
        CanonicalModuleGraph::empty(),
        TrustedProcessorCapabilityManifest::denied(
            crate::attestation::executable_hash(&executable)?,
            crate::platform::sandbox_profile_hash(),
            [DependencyReadKind::Source],
        ),
    )?;
    DeniedWorkerBroker::new()?.launch(launch, Duration::from_secs(10))
}

/// The complete set of outcomes a sandbox-dependent test may take from a launch.
///
/// There is deliberately no `Skip` variant: `verter_processor_broker` compiles only
/// on Windows, Linux and macOS (`ProcessorSandboxKindV1::current`), all three of
/// which are supported sandbox platforms, so a launch that does not produce a live
/// denied worker is a test FAILURE. Platforms without a sandbox implementation are
/// excluded at compile time, never skipped at run time.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum LaunchGate {
    Run,
    Fail,
}

pub(crate) fn launch_gate(outcome: &Result<DeniedWorkerSession, BrokerError>) -> LaunchGate {
    match outcome {
        Ok(_) => LaunchGate::Run,
        Err(_) => LaunchGate::Fail,
    }
}

/// Launches a real denied worker on this platform, failing the test if it cannot.
pub(crate) fn launch_required() -> DeniedWorkerSession {
    let outcome = launch();
    match (launch_gate(&outcome), outcome) {
        (LaunchGate::Run, Ok(session)) => session,
        (_, Err(BrokerError::SandboxUnavailable(evidence))) => {
            assert!(evidence.is_typed_and_fail_closed());
            panic!(
                "{:?} sandbox must run on this supported platform, never skip: {evidence:?}",
                ProcessorSandboxKindV1::current()
            );
        }
        (_, Err(error)) => panic!("launch failed: {error:?}"),
        (LaunchGate::Fail, Ok(_)) => unreachable!("gate is total over the outcome"),
    }
}

#[test]
fn sandbox_launch_outcomes_have_no_passing_skip_on_a_supported_platform() {
    assert!(matches!(
        ProcessorSandboxKindV1::current(),
        ProcessorSandboxKindV1::LinuxNamespaceSeccomp
            | ProcessorSandboxKindV1::MacSandbox
            | ProcessorSandboxKindV1::WindowsAppContainer
    ));
    let unavailable = BrokerError::SandboxUnavailable(SandboxUnavailableEvidence::new(
        ProcessorSandboxKindV1::current(),
        "synthetic unavailable outcome",
        None,
    ));
    assert_eq!(launch_gate(&Err(unavailable)), LaunchGate::Fail);
    assert_eq!(launch_gate(&launch()), LaunchGate::Run);
}

pub(crate) fn execution_descriptor(
    initial_output: &[u8],
    dependencies: &[(DependencyReadKind, &[u8])],
) -> Vec<u8> {
    let mut descriptor = Vec::new();
    descriptor.extend_from_slice(b"VERTER-EXECUTION-1\0");
    descriptor.extend_from_slice(&(initial_output.len() as u32).to_be_bytes());
    descriptor.extend_from_slice(initial_output);
    descriptor.extend_from_slice(&(dependencies.len() as u32).to_be_bytes());
    for (kind, request) in dependencies {
        descriptor.push(*kind as u8);
        descriptor.extend_from_slice(&(request.len() as u32).to_be_bytes());
        descriptor.extend_from_slice(request);
    }
    descriptor
}

#[test]
fn work_round_trip_over_real_denied_worker() {
    let mut session = launch_required();
    let output = vec![0x5a; 96 * 1024];
    let work = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([11; 16]),
        BlockContentWorkTokenV1::from_bytes([12; 16]),
        execution_descriptor(&output, &[]),
    )
    .expect("bounded work");
    let mut authority =
        dependency_read_authority(|_| panic!("echo work must not request a dependency"));

    let result = session
        .submit_work(work, &mut authority, Duration::from_secs(10))
        .expect("work transport");
    assert_eq!(
        result,
        TrustedBrokerWorkResult::Success(TrustedBrokerWorkOutput::new(output))
    );
}

#[test]
fn dependency_read_suspends_and_resumes_with_worker_minted_correlation() {
    let mut session = launch_required();
    let context = BlockContentResolveContextTokenV1::from_bytes([21; 16]);
    let work_token = BlockContentWorkTokenV1::from_bytes([22; 16]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_authority = Arc::clone(&seen);
    let mut authority = dependency_read_authority(move |request: &DependencyReadRequest| {
        seen_for_authority.lock().expect("seen lock").push((
            request.resolve_context(),
            request.work(),
            request.id(),
            request.kind(),
            request.descriptor().to_vec(),
        ));
        DependencyReadDecision::resolved(b"authority-scoped bytes".to_vec())
    });
    let work = TrustedBrokerWork::new(
        context,
        work_token,
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"./theme.css")]),
    )
    .expect("work");

    let result = session
        .submit_work(work, &mut authority, Duration::from_secs(10))
        .expect("dependency work");
    assert_eq!(
        result,
        TrustedBrokerWorkResult::Success(TrustedBrokerWorkOutput::new(
            b"authority-scoped bytes".to_vec()
        ))
    );
    let seen_request = seen.lock().expect("seen lock");
    assert_eq!(seen_request.len(), 1);
    assert_eq!(seen_request[0].0, context);
    assert_eq!(seen_request[0].1, work_token);
    assert_ne!(seen_request[0].2.as_bytes(), [0; 16]);
    assert_eq!(seen_request[0].3, DependencyReadKind::Source);
    assert_eq!(seen_request[0].4, b"./theme.css");
    drop(seen_request);

    let denied = TrustedBrokerWork::new(
        context,
        BlockContentWorkTokenV1::from_bytes([24; 16]),
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"./denied.css")]),
    )
    .expect("denied work");
    let mut denying_authority = dependency_read_authority(|_| {
        DependencyReadDecision::denied(DependencyReadDenial::ScopeDenied)
    });
    assert_eq!(
        session
            .submit_work(denied, &mut denying_authority, Duration::from_secs(10))
            .expect("typed denial result"),
        TrustedBrokerWorkResult::Failed(TrustedBrokerProcessingFailure::DependencyDenied(
            DependencyReadDenial::ScopeDenied
        ))
    );
}

#[test]
fn dependency_ids_are_worker_minted_nonzero_and_not_broker_selected() {
    let mut session = launch_required();
    let broker_selected_collision = [33; 16];
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_for_authority = Arc::clone(&seen);
    let mut authority = dependency_read_authority(move |request: &DependencyReadRequest| {
        seen_for_authority
            .lock()
            .expect("seen lock")
            .push((request.id(), request.descriptor().to_vec()));
        DependencyReadDecision::resolved(b"once".to_vec())
    });
    let first = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([31; 16]),
        BlockContentWorkTokenV1::from_bytes([32; 16]),
        execution_descriptor(
            b"",
            &[(DependencyReadKind::Source, &broker_selected_collision)],
        ),
    )
    .expect("first work");
    assert!(matches!(
        session.submit_work(first, &mut authority, Duration::from_secs(10)),
        Ok(TrustedBrokerWorkResult::Success(_))
    ));

    let second = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([34; 16]),
        BlockContentWorkTokenV1::from_bytes([35; 16]),
        execution_descriptor(
            b"",
            &[(DependencyReadKind::Source, &broker_selected_collision)],
        ),
    )
    .expect("second work");
    assert!(matches!(
        session.submit_work(second, &mut authority, Duration::from_secs(10)),
        Ok(TrustedBrokerWorkResult::Success(_))
    ));
    let seen = seen.lock().expect("seen lock");
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].1, broker_selected_collision);
    assert_eq!(seen[1].1, broker_selected_collision);
    assert_ne!(seen[0].0.as_bytes(), [0; 16]);
    assert_ne!(seen[1].0.as_bytes(), [0; 16]);
    assert_ne!(seen[0].0.as_bytes(), broker_selected_collision);
    assert_ne!(seen[1].0.as_bytes(), broker_selected_collision);
    assert_ne!(seen[0].0, seen[1].0);
    assert_eq!(
        session.replay_worker_dependency_id_for_test(
            seen[0].0,
            BlockContentResolveContextTokenV1::from_bytes([31; 16]),
            BlockContentWorkTokenV1::from_bytes([32; 16]),
        ),
        Err(CorrelationError::ReplayConsumed)
    );
}

#[test]
fn evidence_is_rechecked_before_dispatch() {
    assert_evidence_mutation_rejected(EvidenceMutationPoint::Dispatch, b"dispatch", false);
}

#[test]
fn evidence_is_rechecked_before_success_admission() {
    assert_evidence_mutation_rejected(EvidenceMutationPoint::Success, b"success", false);
}

#[test]
fn evidence_is_rechecked_before_failure_admission() {
    assert_evidence_mutation_rejected(EvidenceMutationPoint::Failure, b"failure", false);
}

#[test]
fn evidence_is_rechecked_before_frame_rejected_admission() {
    assert_evidence_mutation_rejected(
        EvidenceMutationPoint::FrameRejected,
        b"frame-rejected",
        true,
    );
}

fn assert_evidence_mutation_rejected(
    point: EvidenceMutationPoint,
    label: &[u8],
    force_frame_rejection: bool,
) {
    let mut session = launch_required();
    let pid = session.worker().pid();
    session.mutate_evidence_for_test(point);
    if force_frame_rejection {
        session.force_worker_frame_rejection_for_test();
    }
    let descriptor = if point == EvidenceMutationPoint::Failure {
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"denied")])
    } else {
        execution_descriptor(label, &[])
    };
    let work = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([41; 16]),
        BlockContentWorkTokenV1::from_bytes([42; 16]),
        descriptor,
    )
    .expect("work");
    let mut authority = dependency_read_authority(|_| {
        DependencyReadDecision::denied(DependencyReadDenial::ScopeDenied)
    });
    assert!(matches!(
        session.submit_work(work, &mut authority, Duration::from_secs(10)),
        Err(BrokerError::LaunchEvidence(
            LaunchEvidenceError::ConfigHashMismatch
        ))
    ));
    assert!(crate::platform::wait_pid_gone_for_test(
        pid,
        Duration::from_secs(5)
    ));
}

#[test]
fn work_frame_bounds_and_unknown_descriptors_fail_typed() {
    assert!(TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([49; 16]),
        BlockContentWorkTokenV1::from_bytes([50; 16]),
        vec![0; MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES],
    )
    .is_ok());
    assert_eq!(
        TrustedBrokerWork::new(
            BlockContentResolveContextTokenV1::from_bytes([51; 16]),
            BlockContentWorkTokenV1::from_bytes([52; 16]),
            vec![0; MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES + 1],
        ),
        Err(TrustedBrokerWorkError::DescriptorTooLarge)
    );

    let mut session = launch_required();
    let mut authority =
        dependency_read_authority(|_| panic!("unknown work must not request a dependency"));
    let malformed = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([57; 16]),
        BlockContentWorkTokenV1::from_bytes([58; 16]),
        Vec::new(),
    )
    .expect("bounded malformed descriptor");
    assert_eq!(
        session
            .submit_work(malformed, &mut authority, Duration::from_secs(10))
            .expect("typed malformed failure"),
        TrustedBrokerWorkResult::Failed(TrustedBrokerProcessingFailure::MalformedDescriptor)
    );
    let unknown = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([53; 16]),
        BlockContentWorkTokenV1::from_bytes([54; 16]),
        vec![0xff],
    )
    .expect("bounded unknown descriptor");
    assert_eq!(
        session
            .submit_work(unknown, &mut authority, Duration::from_secs(10))
            .expect("typed failure"),
        TrustedBrokerWorkResult::Failed(TrustedBrokerProcessingFailure::UnknownDescriptor)
    );

    let control = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([55; 16]),
        BlockContentWorkTokenV1::from_bytes([56; 16]),
        execution_descriptor(b"worker survived typed rejection", &[]),
    )
    .expect("control work");
    assert_eq!(
        session
            .submit_work(control, &mut authority, Duration::from_secs(10))
            .expect("control result"),
        TrustedBrokerWorkResult::Success(TrustedBrokerWorkOutput::new(
            b"worker survived typed rejection".to_vec()
        ))
    );
}

#[test]
fn worker_rejects_malformed_unknown_and_out_of_window_frames_without_teardown() {
    use crate::protocol::{BrokerToWorkerFrame, WorkScope};

    let mut session = launch_required();
    assert_eq!(
        session.raw_application_frame_for_test(vec![2], Duration::from_secs(5)),
        Ok(WorkerFrameRejection::TruncatedPayload)
    );
    assert_eq!(
        session.raw_application_frame_for_test(vec![0xff], Duration::from_secs(5)),
        Ok(WorkerFrameRejection::UnknownFrame)
    );
    let scope = WorkScope {
        context: BlockContentResolveContextTokenV1::from_bytes([61; 16]),
        work: BlockContentWorkTokenV1::from_bytes([62; 16]),
    };
    assert_eq!(
        session.raw_application_frame_for_test(
            BrokerToWorkerFrame::WorkEnd { scope }.encode(),
            Duration::from_secs(5),
        ),
        Ok(WorkerFrameRejection::OutOfWindow)
    );

    let mut authority =
        dependency_read_authority(|_| panic!("control must not request a dependency"));
    let control = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([63; 16]),
        BlockContentWorkTokenV1::from_bytes([64; 16]),
        execution_descriptor(b"still alive", &[]),
    )
    .expect("control work");
    assert!(matches!(
        session.submit_work(control, &mut authority, Duration::from_secs(10)),
        Ok(TrustedBrokerWorkResult::Success(_))
    ));
}

#[test]
fn broker_typed_rejects_malformed_unknown_and_out_of_window_worker_frames_with_teardown() {
    use crate::protocol::{WorkScope, WorkerToBrokerFrame};

    let injections = [
        (vec![3], WorkerFrameRejection::TruncatedPayload),
        (vec![0xff], WorkerFrameRejection::UnknownFrame),
        (
            WorkerToBrokerFrame::WorkSuccessEnd {
                scope: WorkScope {
                    context: BlockContentResolveContextTokenV1::from_bytes([71; 16]),
                    work: BlockContentWorkTokenV1::from_bytes([72; 16]),
                },
            }
            .encode(),
            WorkerFrameRejection::OutOfWindow,
        ),
    ];
    for (payload, expected) in injections {
        let mut session = launch_required();
        let pid = session.worker().pid();
        assert_eq!(
            session.inject_worker_frame_for_test(payload),
            Err(BrokerError::WorkerFrameRejected(expected))
        );
        assert!(crate::platform::wait_pid_gone_for_test(
            pid,
            Duration::from_secs(5)
        ));
    }

    let mut control = launch_required();
    let mut authority = dependency_read_authority(|_| panic!("control has no dependencies"));
    let work = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([73; 16]),
        BlockContentWorkTokenV1::from_bytes([74; 16]),
        execution_descriptor(b"typed teardown control", &[]),
    )
    .expect("control work");
    assert!(matches!(
        control.submit_work(work, &mut authority, Duration::from_secs(10)),
        Ok(TrustedBrokerWorkResult::Success(_))
    ));
}

#[test]
fn execution_envelope_supports_256_reads_and_denies_257_before_authority() {
    let mut session = launch_required();
    let request = b"same-request".as_slice();
    let exact_dependencies =
        vec![(DependencyReadKind::Source, request); MAX_DEPENDENCY_READS_PER_WORK];
    let exact_calls = Arc::new(Mutex::new(0_usize));
    let exact_calls_for_authority = Arc::clone(&exact_calls);
    let mut exact_authority = dependency_read_authority(move |_| {
        *exact_calls_for_authority.lock().expect("calls lock") += 1;
        DependencyReadDecision::resolved(vec![0x5a])
    });
    let exact = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([81; 16]),
        BlockContentWorkTokenV1::from_bytes([82; 16]),
        execution_descriptor(b"", &exact_dependencies),
    )
    .expect("256-read work");
    let exact_result = session
        .submit_work(exact, &mut exact_authority, Duration::from_secs(30))
        .expect("256 reads transport");
    assert_eq!(*exact_calls.lock().expect("calls lock"), 256);
    assert_eq!(
        exact_result,
        TrustedBrokerWorkResult::Success(TrustedBrokerWorkOutput::new(vec![0x5a; 256]))
    );

    let over_dependencies =
        vec![(DependencyReadKind::Source, request); MAX_DEPENDENCY_READS_PER_WORK + 1];
    let over_calls = Arc::new(Mutex::new(0_usize));
    let over_calls_for_authority = Arc::clone(&over_calls);
    let mut over_authority = dependency_read_authority(move |_| {
        *over_calls_for_authority.lock().expect("calls lock") += 1;
        DependencyReadDecision::resolved(vec![0x5a])
    });
    let over = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([83; 16]),
        BlockContentWorkTokenV1::from_bytes([84; 16]),
        execution_descriptor(b"", &over_dependencies),
    )
    .expect("257-read work");
    assert_eq!(
        session
            .submit_work(over, &mut over_authority, Duration::from_secs(30))
            .expect("typed 257th-read result"),
        TrustedBrokerWorkResult::Failed(TrustedBrokerProcessingFailure::DependencyDenied(
            DependencyReadDenial::BudgetExceeded
        ))
    );
    assert_eq!(*over_calls.lock().expect("calls lock"), 256);
}

#[test]
fn real_worker_enforces_exact_and_plus_one_dependency_and_output_bounds() {
    let mut session = launch_required();

    let exact = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([91; 16]),
        BlockContentWorkTokenV1::from_bytes([92; 16]),
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"exact")]),
    )
    .expect("exact-bound work");
    let mut exact_authority = dependency_read_authority(|_| {
        DependencyReadDecision::resolved(vec![0x6b; MAX_DEPENDENCY_BYTES_PER_WORK])
    });
    let exact_output = session
        .submit_work(exact, &mut exact_authority, Duration::from_secs(60))
        .expect("64 MiB dependency and output transport");
    let TrustedBrokerWorkResult::Success(exact_output) = exact_output else {
        panic!("exact dependency/output boundary must succeed");
    };
    assert_eq!(
        exact_output.bytes().len(),
        MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES
    );
    assert_eq!(exact_output.bytes().first(), Some(&0x6b));
    assert_eq!(exact_output.bytes().last(), Some(&0x6b));
    drop(exact_output);

    let dependency_over = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([93; 16]),
        BlockContentWorkTokenV1::from_bytes([94; 16]),
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"dependency-over")]),
    )
    .expect("dependency +1 work");
    let mut dependency_over_authority = dependency_read_authority(|_| {
        DependencyReadDecision::resolved(vec![0x6c; MAX_DEPENDENCY_BYTES_PER_WORK + 1])
    });
    assert_eq!(
        session
            .submit_work(
                dependency_over,
                &mut dependency_over_authority,
                Duration::from_secs(60),
            )
            .expect("typed dependency +1 result"),
        TrustedBrokerWorkResult::Failed(TrustedBrokerProcessingFailure::DependencyDenied(
            DependencyReadDenial::BudgetExceeded
        ))
    );

    let output_over = TrustedBrokerWork::new(
        BlockContentResolveContextTokenV1::from_bytes([95; 16]),
        BlockContentWorkTokenV1::from_bytes([96; 16]),
        execution_descriptor(b"x", &[(DependencyReadKind::Source, b"output-over")]),
    )
    .expect("output +1 work");
    let mut output_over_authority = dependency_read_authority(|_| {
        DependencyReadDecision::resolved(vec![0x6d; MAX_DEPENDENCY_BYTES_PER_WORK])
    });
    assert_eq!(
        session
            .submit_work(
                output_over,
                &mut output_over_authority,
                Duration::from_secs(60),
            )
            .expect("typed output +1 result"),
        TrustedBrokerWorkResult::Failed(TrustedBrokerProcessingFailure::ProtocolRejected(
            WorkerFrameRejection::PayloadTooLarge
        ))
    );
}

#[cfg(windows)]
#[test]
fn windows_worker_starts_with_empty_environment_before_admission() {
    const CANARY: &str = "VERTER_PROCESSOR_BROKER_AMBIENT_CANARY";

    struct RestoreEnvironment(Option<std::ffi::OsString>);

    impl Drop for RestoreEnvironment {
        fn drop(&mut self) {
            if let Some(value) = self.0.take() {
                std::env::set_var(CANARY, value);
            } else {
                std::env::remove_var(CANARY);
            }
        }
    }

    let restore = RestoreEnvironment(std::env::var_os(CANARY));
    std::env::set_var(CANARY, "must-not-cross-create-process");
    let session = launch().expect("ambient parent environment must be absent before admission");
    assert_eq!(
        session.worker().attestation().os_sandbox_kind(),
        ProcessorSandboxKindV1::WindowsAppContainer
    );
    drop(session);
    drop(restore);
}

#[test]
fn sandbox_denies_filesystem_network_child_and_environment() {
    let mut session = launch_required();

    let outside_grant = PathBuf::from_platform_outside_grant();
    assert!(
        std::fs::read(&outside_grant).is_ok(),
        "control path must exist"
    );
    let probes = vec![
        WorkerProbe::ReadOutsideGrant(outside_grant.clone()),
        WorkerProbe::Network,
        WorkerProbe::ChildProcess,
        WorkerProbe::Environment,
    ];
    #[cfg(target_os = "linux")]
    let probes = {
        let mut probes = probes;
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        probes.push(WorkerProbe::DirectOpen);
        probes.push(WorkerProbe::OpenAt2);
        probes
    };
    for probe in probes {
        let label = format!("{probe:?}");
        assert_eq!(
            session.probe_for_test(probe, Duration::from_secs(5)),
            Ok(false),
            "sandbox escape must be denied: {label}"
        );
    }
    #[cfg(windows)]
    std::fs::remove_file(outside_grant).expect("remove denial fixture");
}

#[test]
fn worker_timeout_crash_and_drop_are_bounded() {
    let mut session = launch_required();
    let pid = session.worker().pid();
    assert!(matches!(
        session.probe_for_test(WorkerProbe::Hang, Duration::from_millis(100)),
        Err(BrokerError::WorkerTimeout)
    ));
    assert!(crate::platform::wait_pid_gone_for_test(
        pid,
        Duration::from_secs(5)
    ));

    let mut crashed = launch_required();
    let crash_result = crashed.probe_for_test(WorkerProbe::Crash, Duration::from_secs(5));
    assert!(
        matches!(crash_result, Err(BrokerError::WorkerCrashed(_))),
        "crash must be typed: {crash_result:?}"
    );

    let dropped = launch_required();
    let pid = dropped.worker().pid();
    drop(dropped);
    assert!(crate::platform::wait_pid_gone_for_test(
        pid,
        Duration::from_secs(5)
    ));
}

trait PlatformOutsideGrant {
    fn from_platform_outside_grant() -> Self;
}

impl PlatformOutsideGrant for PathBuf {
    fn from_platform_outside_grant() -> Self {
        #[cfg(windows)]
        {
            let path =
                std::env::temp_dir().join(format!("verter-broker-denied-{}", std::process::id()));
            std::fs::write(&path, b"denied").expect("create denial fixture");
            path
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/etc/passwd")
        }
    }
}
