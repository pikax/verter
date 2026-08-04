//! Broker substrate hardening: bounded frame reads, teardown on authentication or
//! replay failure, enforced-policy attestation hashes, live recheck comparisons, and
//! a bounded correlation-registry lifecycle.

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::correlation::CorrelationAuditEvent;
use crate::lifecycle::test_support::WorkerProbe;
use crate::lifecycle::LaunchFactMutation;
use crate::platform::wait_pid_gone_for_test;
use crate::tests::{execution_descriptor, launch_required};

fn context(byte: u8) -> BlockContentResolveContextTokenV1 {
    BlockContentResolveContextTokenV1::from_bytes([byte; 16])
}

fn work_token(byte: u8) -> BlockContentWorkTokenV1 {
    BlockContentWorkTokenV1::from_bytes([byte; 16])
}

fn echo_work(context_byte: u8, work_byte: u8, payload: &[u8]) -> TrustedBrokerWork {
    TrustedBrokerWork::new(
        context(context_byte),
        work_token(work_byte),
        execution_descriptor(payload, &[]),
    )
    .expect("bounded work")
}

// (a) Deadline-bounded frame reads.

#[test]
fn a_worker_stalling_mid_frame_is_bounded_by_a_read_deadline_and_torn_down() {
    let (pid_sender, pid_receiver) = mpsc::channel();
    let (outcome_sender, outcome_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut session = launch_required();
        pid_sender.send(session.worker().pid()).expect("pid");
        let outcome =
            session.probe_for_test(WorkerProbe::StallMidFrame, Duration::from_millis(500));
        outcome_sender.send(outcome).expect("outcome");
    });

    let pid = pid_receiver
        .recv_timeout(Duration::from_secs(60))
        .expect("denied worker launches");
    let outcome = outcome_receiver
        .recv_timeout(Duration::from_secs(30))
        .expect(
            "broker parked on an unbounded frame read: the worker sent a length header and never \
         sent the body, so the read must expire instead of blocking the broker thread forever",
        );
    assert!(
        matches!(outcome, Err(BrokerError::WorkerTimeout)),
        "a stalled mid-frame read must be a typed timeout: {outcome:?}"
    );
    assert!(
        wait_pid_gone_for_test(pid, Duration::from_secs(10)),
        "the stalled worker must be torn down, not left running"
    );
}

#[test]
fn a_pre_elapsed_work_budget_is_a_terminal_timeout_that_tears_the_worker_down() {
    let mut session = launch_required();
    let pid = session.worker().pid();
    let mut authority =
        dependency_read_authority(|_| panic!("an expired budget resolves no dependency reads"));
    let outcome = session.submit_work(
        echo_work(141, 142, b"zero budget"),
        &mut authority,
        Duration::ZERO,
    );
    assert!(
        matches!(outcome, Err(BrokerError::WorkerTimeout)),
        "a work budget that elapsed before the first frame read must be a typed timeout: \
         {outcome:?}"
    );
    assert!(
        wait_pid_gone_for_test(pid, Duration::from_secs(10)),
        "a pre-read budget expiry must tear the worker down, not leave it running"
    );
    assert_eq!(
        session.probe_for_test(WorkerProbe::Environment, Duration::from_secs(5)),
        Err(BrokerError::SessionTerminated),
        "the session must be terminal after any timeout, exactly like a mid-frame expiry"
    );
    let mut refused_authority =
        dependency_read_authority(|_| panic!("terminated session runs no work"));
    assert_eq!(
        session.submit_work(
            echo_work(143, 144, b"after expiry"),
            &mut refused_authority,
            Duration::from_secs(5),
        ),
        Err(BrokerError::SessionTerminated)
    );
}

#[test]
fn a_bounded_read_deadline_does_not_disturb_a_responsive_worker() {
    let mut session = launch_required();
    let mut authority = dependency_read_authority(|_| panic!("control has no dependencies"));
    assert_eq!(
        session
            .submit_work(
                echo_work(101, 102, b"responsive worker"),
                &mut authority,
                Duration::from_secs(10),
            )
            .expect("control work"),
        TrustedBrokerWorkResult::Success(TrustedBrokerWorkOutput::new(
            b"responsive worker".to_vec()
        ))
    );
}

// (b) Teardown on authentication / replay failure.

fn assert_channel_failure_tears_down(probe: WorkerProbe, expect_replay: bool) {
    let mut session = launch_required();
    let pid = session.worker().pid();
    let failure = session.probe_for_test(probe, Duration::from_secs(5));
    match (&failure, expect_replay) {
        (Err(BrokerError::Channel(ChannelError::ReplayOrReorder { .. })), true) => {}
        (Err(BrokerError::Channel(ChannelError::AuthenticationFailed)), false) => {}
        _ => panic!("expected a typed channel failure, got {failure:?}"),
    }
    assert!(
        wait_pid_gone_for_test(pid, Duration::from_secs(10)),
        "an authentication or replay failure must tear the worker down"
    );

    assert_eq!(
        session.probe_for_test(WorkerProbe::Environment, Duration::from_secs(5)),
        Err(BrokerError::SessionTerminated),
        "the channel must never be reusable after a channel authentication failure"
    );
    let mut authority = dependency_read_authority(|_| panic!("terminated session runs no work"));
    assert_eq!(
        session.submit_work(
            echo_work(111, 112, b"after teardown"),
            &mut authority,
            Duration::from_secs(5),
        ),
        Err(BrokerError::SessionTerminated)
    );
}

#[test]
fn an_authentication_failure_terminates_the_session_and_tears_down_the_worker() {
    assert_channel_failure_tears_down(WorkerProbe::CorruptAuthFrame, false);
}

#[test]
fn a_replay_or_reorder_failure_terminates_the_session_and_tears_down_the_worker() {
    assert_channel_failure_tears_down(WorkerProbe::ReplaySequenceFrame, true);
}

// (c) Honest sandbox_profile_hash over the actually-enforced policy.

#[cfg(windows)]
#[test]
fn windows_sandbox_profile_hash_digests_the_enforced_app_container_policy() {
    use crate::platform::{
        hash_app_container_policy, AppContainerPolicyMaterial, ENFORCED_APP_CONTAINER_POLICY,
    };

    assert_eq!(
        crate::platform::sandbox_profile_hash(),
        hash_app_container_policy(&ENFORCED_APP_CONTAINER_POLICY),
        "the attested profile hash must digest the AppContainer policy actually applied at \
         launch, not a descriptive string literal"
    );

    let mutations: Vec<(&str, AppContainerPolicyMaterial)> = vec![
        ("capability_count", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.capability_count = 1;
            policy
        }),
        ("process_mitigation_policy", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.process_mitigation_policy = 0;
            policy
        }),
        ("job_limit_flags", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.job_limit_flags = 0;
            policy
        }),
        ("job_active_process_limit", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.job_active_process_limit = 2;
            policy
        }),
        ("inherited_handle_count", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.inherited_handle_count = 2;
            policy
        }),
        ("environment_block_u16s", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.environment_block_u16s = 64;
            policy
        }),
        ("lowbox_handle_count", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.lowbox_handle_count = 1;
            policy
        }),
        ("profile_access_mask", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.profile_access_mask = u32::MAX;
            policy
        }),
        ("profile_ace_inheritance", {
            let mut policy = ENFORCED_APP_CONTAINER_POLICY;
            policy.profile_ace_inheritance = 0;
            policy
        }),
    ];
    for (label, mutated) in mutations {
        assert_ne!(
            hash_app_container_policy(&mutated),
            crate::platform::sandbox_profile_hash(),
            "relaxing {label} must change the attested sandbox profile hash"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_sandbox_profile_hash_digests_the_enforced_namespace_and_seccomp_policy() {
    use crate::platform::{enforced_linux_sandbox_policy, hash_linux_sandbox_policy};

    let enforced = enforced_linux_sandbox_policy();
    assert_eq!(
        crate::platform::sandbox_profile_hash(),
        hash_linux_sandbox_policy(&enforced),
        "the attested profile hash must digest the namespace/seccomp configuration actually \
         installed at launch, not a descriptive string literal"
    );
    assert!(
        !enforced.launch_filter.is_empty() && !enforced.worker_filter.is_empty(),
        "both seccomp stages must contribute real filter bytes"
    );

    let mutations: Vec<(&str, _)> = vec![
        ("unshare_flags", {
            let mut policy = enforced.clone();
            policy.unshare_flags = 0;
            policy
        }),
        ("root_mount_flags", {
            let mut policy = enforced.clone();
            policy.root_mount_flags = 0;
            policy
        }),
        ("root_mount_data", {
            let mut policy = enforced.clone();
            policy.root_mount_data = "size=1g,mode=0777\0";
            policy
        }),
        ("bind_remount_flags", {
            let mut policy = enforced.clone();
            policy.bind_remount_flags = 0;
            policy
        }),
        ("no_new_privileges", {
            let mut policy = enforced.clone();
            policy.no_new_privileges = 0;
            policy
        }),
        ("setgroups_denied", {
            let mut policy = enforced.clone();
            policy.setgroups_denied = false;
            policy
        }),
        ("close_range_first_swept_fd", {
            let mut policy = enforced.clone();
            policy.close_range_first_swept_fd = 1024;
            policy
        }),
        ("close_range_flags", {
            let mut policy = enforced.clone();
            policy.close_range_flags = 0;
            policy
        }),
        ("launch_filter", {
            let mut policy = enforced.clone();
            policy.launch_filter.truncate(8);
            policy
        }),
        ("worker_filter", {
            let mut policy = enforced.clone();
            policy.worker_filter.truncate(8);
            policy
        }),
        ("audit_arch", {
            let mut policy = enforced.clone();
            policy.audit_arch ^= 1;
            policy
        }),
    ];
    for (label, mutated) in mutations {
        assert_ne!(
            hash_linux_sandbox_policy(&mutated),
            crate::platform::sandbox_profile_hash(),
            "relaxing {label} must change the attested sandbox profile hash"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_launch_enforcement_applies_the_exact_policy_material_the_attested_hash_digests() {
    use crate::platform::{
        hash_app_container_policy, take_applied_app_container_policy_for_test,
        ENFORCED_APP_CONTAINER_POLICY,
    };

    let session = launch_required();
    drop(session);
    let applied = take_applied_app_container_policy_for_test();
    assert_eq!(
        applied, ENFORCED_APP_CONTAINER_POLICY,
        "launch enforcement must consume exactly the policy material the attested hash digests \
         — a diverging enforcement literal is the false-stability defect this guards against"
    );
    assert_eq!(
        hash_app_container_policy(&applied),
        crate::platform::sandbox_profile_hash(),
        "the attested sandbox profile hash must digest the values enforcement actually applied"
    );
}

// (d) Live recheck comparisons against independently recorded launch facts.

#[test]
fn recheck_compares_launch_facts_that_are_recorded_independently_of_the_attestation() {
    for (mutation, expected) in [
        (
            LaunchFactMutation::BrokerInstance,
            LaunchEvidenceError::BrokerInstanceMismatch,
        ),
        (
            LaunchFactMutation::LaunchNonce,
            LaunchEvidenceError::LaunchNonceMismatch,
        ),
        (
            LaunchFactMutation::SandboxKind,
            LaunchEvidenceError::SandboxKindMismatch,
        ),
    ] {
        let mut session = launch_required();
        let pid = session.worker().pid();
        session.mutate_launch_fact_for_test(mutation);
        let mut authority =
            dependency_read_authority(|_| panic!("a refused recheck dispatches no work"));
        assert_eq!(
            session.submit_work(
                echo_work(121, 122, b"planted mismatch"),
                &mut authority,
                Duration::from_secs(10),
            ),
            Err(BrokerError::LaunchEvidence(expected.clone())),
            "{mutation:?} must be caught by a live recheck comparison"
        );
        assert!(
            wait_pid_gone_for_test(pid, Duration::from_secs(10)),
            "{mutation:?} must tear the worker down"
        );
    }

    let mut control = launch_required();
    let mut authority = dependency_read_authority(|_| panic!("control has no dependencies"));
    assert_eq!(
        control
            .submit_work(
                echo_work(123, 124, b"unmutated control"),
                &mut authority,
                Duration::from_secs(10),
            )
            .expect("control work"),
        TrustedBrokerWorkResult::Success(TrustedBrokerWorkOutput::new(
            b"unmutated control".to_vec()
        ))
    );
}

// (e) Bounded correlation-registry lifecycle.

fn registry_id(byte: u8) -> DependencyRequestIdV1 {
    DependencyRequestIdV1::from_bytes([byte; 16]).expect("nonzero id")
}

#[test]
fn correlation_registry_evicts_the_oldest_consumed_entry_at_capacity_with_audit() {
    let binding = [9_u8; 32];
    let mut registry =
        CorrelationRegistry::with_limits_for_test(binding, 2, Duration::from_secs(300));
    let context = context(2);
    let work = work_token(3);
    for byte in [1_u8, 2] {
        let id = registry_id(byte);
        registry.register(id, context, work).expect("pending");
        registry
            .consume(id, context, work, binding)
            .expect("consume");
    }
    let _ = registry.drain_audit_events();

    registry
        .register(registry_id(3), context, work)
        .expect("capacity is reclaimed from consumed entries");
    assert_eq!(registry.state_counts_for_test(), (1, 1));
    let events = registry.drain_audit_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            CorrelationAuditEvent::ConsumedEvictedByCapacity { id } if *id == registry_id(1)
        )),
        "capacity eviction must publish a typed audit event for the evicted id: {events:?}"
    );
}

#[test]
fn correlation_registry_evicts_consumed_entries_past_their_ttl_with_audit() {
    let binding = [9_u8; 32];
    let mut registry = CorrelationRegistry::with_limits_for_test(binding, 64, Duration::ZERO);
    let context = context(2);
    let work = work_token(3);
    let expired = registry_id(1);
    registry.register(expired, context, work).expect("pending");
    registry
        .consume(expired, context, work, binding)
        .expect("consume");
    let _ = registry.drain_audit_events();

    registry
        .register(registry_id(2), context, work)
        .expect("second registration");
    assert_eq!(
        registry.state_counts_for_test(),
        (1, 0),
        "the expired consumed entry must not be retained"
    );
    let events = registry.drain_audit_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            CorrelationAuditEvent::ConsumedEvictedByTtl { id } if *id == expired
        )),
        "ttl eviction must publish a typed audit event: {events:?}"
    );
}

#[test]
fn correlation_registry_refuses_registration_when_pending_entries_fill_capacity() {
    let binding = [9_u8; 32];
    let mut registry =
        CorrelationRegistry::with_limits_for_test(binding, 2, Duration::from_secs(300));
    let context = context(2);
    let work = work_token(3);
    registry
        .register(registry_id(1), context, work)
        .expect("first pending");
    registry
        .register(registry_id(2), context, work)
        .expect("second pending");
    let _ = registry.drain_audit_events();

    let refused = registry_id(3);
    assert_eq!(
        registry.register(refused, context, work),
        Err(CorrelationError::CapacityExhausted),
        "a registry full of pending entries must refuse growth, not grow without bound"
    );
    assert_eq!(registry.state_counts_for_test(), (2, 0));
    let events = registry.drain_audit_events();
    assert!(
        events.iter().any(|event| matches!(
            event,
            CorrelationAuditEvent::RegistrationRefusedAtCapacity { id } if *id == refused
        )),
        "a refused registration must publish a typed audit event: {events:?}"
    );
}

#[test]
fn correlation_registry_audit_buffer_is_bounded_and_reports_its_drops() {
    let binding = [9_u8; 32];
    let mut registry = CorrelationRegistry::with_limits_for_test(binding, 1, Duration::ZERO);
    let context = context(2);
    let work = work_token(3);
    for step in 0..600_u32 {
        let id = DependencyRequestIdV1::from_bytes([
            (step >> 8) as u8,
            step as u8,
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ])
        .expect("nonzero id");
        registry.register(id, context, work).expect("registration");
        registry
            .consume(id, context, work, binding)
            .expect("consume");
    }
    assert!(
        registry.audit_events_dropped() > 0,
        "an eviction storm must be reported as dropped audit events, not buffered without bound"
    );
    assert!(
        registry.drain_audit_events().len() <= 256,
        "the audit buffer itself must stay bounded"
    );
    assert_eq!(
        registry.state_counts_for_test(),
        (0, 1),
        "capacity one must retain exactly the last consumed entry, never 600"
    );
}

#[test]
fn correlation_registry_destruction_clears_every_entry_with_audit() {
    let binding = [9_u8; 32];
    let mut registry =
        CorrelationRegistry::with_limits_for_test(binding, 64, Duration::from_secs(300));
    let context = context(2);
    let work = work_token(3);
    let consumed = registry_id(1);
    registry.register(consumed, context, work).expect("pending");
    registry
        .consume(consumed, context, work, binding)
        .expect("consume");
    registry
        .register(registry_id(2), context, work)
        .expect("pending");
    assert_eq!(registry.state_counts_for_test(), (1, 1));
    let _ = registry.drain_audit_events();

    registry.destroy_for_teardown();
    assert_eq!(registry.state_counts_for_test(), (0, 0));
    assert_eq!(
        registry.drain_audit_events(),
        vec![CorrelationAuditEvent::DestroyedOnTeardown {
            pending: 1,
            consumed: 1,
        }]
    );
}

#[test]
fn correlation_registry_delivers_eviction_audit_through_the_installed_production_sink() {
    let binding = [9_u8; 32];
    let mut registry = CorrelationRegistry::with_limits_for_test(binding, 1, Duration::ZERO);
    let (sender, receiver) = mpsc::channel();
    registry.install_audit_sink(Box::new(move |event| {
        let _ = sender.send(event);
    }));
    let context = context(2);
    let work = work_token(3);
    let expired = registry_id(1);
    registry.register(expired, context, work).expect("pending");
    registry
        .consume(expired, context, work, binding)
        .expect("consume");
    registry
        .register(registry_id(2), context, work)
        .expect("second registration");

    let delivered: Vec<_> = receiver.try_iter().collect();
    assert!(
        delivered.iter().any(|event| matches!(
            event,
            CorrelationAuditEvent::ConsumedEvictedByTtl { id } if *id == expired
        )),
        "eviction audit must be observable through the production sink, not only through \
         the test-only drain: {delivered:?}"
    );
}

#[test]
fn normal_session_drop_destroys_the_correlation_registry_through_the_production_audit_sink() {
    let mut session = launch_required();
    let pid = session.worker().pid();
    let (sender, receiver) = mpsc::channel();
    session.install_correlation_audit_sink(Box::new(move |event| {
        let _ = sender.send(event);
    }));
    let mut authority =
        dependency_read_authority(|_| DependencyReadDecision::resolved(b"dependency".to_vec()));
    let work = TrustedBrokerWork::new(
        context(151),
        work_token(152),
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"./theme.css")]),
    )
    .expect("dependency work");
    assert!(matches!(
        session.submit_work(work, &mut authority, Duration::from_secs(10)),
        Ok(TrustedBrokerWorkResult::Success(_))
    ));
    drop(session);

    let delivered: Vec<_> = receiver.try_iter().collect();
    assert!(
        delivered.iter().any(|event| matches!(
            event,
            CorrelationAuditEvent::DestroyedOnTeardown {
                pending: 0,
                consumed: 1,
            }
        )),
        "normal session teardown on Drop must destroy the correlation registry and deliver \
         the typed destruction audit event through the production sink: {delivered:?}"
    );
    assert!(
        wait_pid_gone_for_test(pid, Duration::from_secs(10)),
        "normal session drop must still tear the worker tree down"
    );
}

#[test]
fn session_teardown_destroys_its_correlation_registry() {
    let mut session = launch_required();
    let pid = session.worker().pid();
    let mut authority =
        dependency_read_authority(|_| DependencyReadDecision::resolved(b"dependency".to_vec()));
    let work = TrustedBrokerWork::new(
        context(131),
        work_token(132),
        execution_descriptor(b"", &[(DependencyReadKind::Source, b"./theme.css")]),
    )
    .expect("dependency work");
    assert!(matches!(
        session.submit_work(work, &mut authority, Duration::from_secs(10)),
        Ok(TrustedBrokerWorkResult::Success(_))
    ));
    assert_eq!(
        session.correlation_counts_for_test(),
        (0, 1),
        "one consumed correlation must be retained for replay rejection"
    );

    assert_eq!(
        session.inject_worker_frame_for_test(vec![3]),
        Err(BrokerError::WorkerFrameRejected(
            WorkerFrameRejection::TruncatedPayload
        ))
    );
    assert!(wait_pid_gone_for_test(pid, Duration::from_secs(10)));
    assert_eq!(
        session.correlation_counts_for_test(),
        (0, 0),
        "session teardown must destroy every correlation entry"
    );
    assert!(
        session
            .drain_correlation_audit_for_test()
            .iter()
            .any(|event| matches!(
                event,
                CorrelationAuditEvent::DestroyedOnTeardown { consumed: 1, .. }
            )),
        "session teardown must publish the typed destruction audit event"
    );
}
