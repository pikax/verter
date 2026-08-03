use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::attestation::AttestationFields;
use crate::channel::test_support::{establish_pair, HandshakeMutation};
use crate::lifecycle::test_support::{worker_executable, WorkerProbe};

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
    let launch = DeniedWorkerLaunch::from_worker(executable, b"{}".to_vec(), [])?;
    DeniedWorkerBroker::new()?.launch(launch, Duration::from_secs(10))
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
    let mut session = match launch() {
        Ok(session) => session,
        Err(BrokerError::SandboxUnavailable(evidence)) => {
            assert!(evidence.is_typed_and_fail_closed());
            #[cfg(windows)]
            panic!("Windows AppContainer must run: {evidence:?}");
            #[cfg(not(windows))]
            {
                eprintln!("typed sandbox unavailable: {evidence:?}");
                return;
            }
        }
        Err(error) => panic!("launch failed: {error:?}"),
    };

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
    let mut session = match launch() {
        Ok(session) => session,
        Err(BrokerError::SandboxUnavailable(evidence)) => {
            assert!(evidence.is_typed_and_fail_closed());
            #[cfg(windows)]
            panic!("Windows AppContainer must run: {evidence:?}");
            #[cfg(not(windows))]
            {
                eprintln!("typed sandbox unavailable: {evidence:?}");
                return;
            }
        }
        Err(error) => panic!("launch failed: {error:?}"),
    };
    let pid = session.worker().pid();
    assert!(matches!(
        session.probe_for_test(WorkerProbe::Hang, Duration::from_millis(100)),
        Err(BrokerError::WorkerTimeout)
    ));
    assert!(crate::platform::wait_pid_gone_for_test(
        pid,
        Duration::from_secs(5)
    ));

    let mut crashed = match launch() {
        Ok(session) => session,
        Err(BrokerError::SandboxUnavailable(_)) => return,
        Err(error) => panic!("launch failed: {error:?}"),
    };
    let crash_result = crashed.probe_for_test(WorkerProbe::Crash, Duration::from_secs(5));
    assert!(
        matches!(crash_result, Err(BrokerError::WorkerCrashed(_))),
        "crash must be typed: {crash_result:?}"
    );

    let dropped = match launch() {
        Ok(session) => session,
        Err(BrokerError::SandboxUnavailable(_)) => return,
        Err(error) => panic!("launch failed: {error:?}"),
    };
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
