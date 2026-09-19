//! Platform-services inventory discriminators.

use super::*;

/// Native callers retain every native service: the native profile maps
/// each class to its native route and never trips the browser guard.
#[test]
fn native_profile_retains_native_services() {
    let profile = PortableHostServices::native();
    assert_eq!(profile.execution_host(), ExecutionHostKind::Native);
    assert_eq!(
        profile.route(PlatformServiceClass::Io),
        ServiceRoute::NativeFilesystemWorkspace
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Time),
        ServiceRoute::StdInstant
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Scheduling),
        ServiceRoute::ThreadedSchedulerPools
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Persistence),
        ServiceRoute::DurableArtifactStore
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Process),
        ServiceRoute::NativeProcessSpawn
    );
    // The native profile is outside the browser closure; the guard is
    // vacuously clean.
    assert!(profile.verify_browser_closure().is_ok());
}

/// The browser profile routes the same five classes through the
/// browser seams and passes the browser-closure dependency guard.
#[test]
fn browser_profile_routes_classes_through_browser_seams() {
    let profile = PortableHostServices::browser();
    assert_eq!(profile.execution_host(), ExecutionHostKind::Browser);
    assert_eq!(
        profile.route(PlatformServiceClass::Io),
        ServiceRoute::HostProvidedVirtualWorkspace
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Time),
        ServiceRoute::WebInstant
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Scheduling),
        ServiceRoute::InlineDriveScheduler
    );
    assert_eq!(
        profile.route(PlatformServiceClass::Persistence),
        ServiceRoute::InMemoryCachesOnly
    );
    // The browser closure has no process capability.
    assert_eq!(
        profile.route(PlatformServiceClass::Process),
        ServiceRoute::Unavailable
    );
    assert!(profile.verify_browser_closure().is_ok());
}

/// Every native-only route is refused inside the browser closure: a
/// plausible wrong-complete failure is a browser profile that silently
/// inherits a native service.
#[test]
fn native_only_routes_are_refused_in_browser_closure() {
    for route in [
        ServiceRoute::NativeFilesystemWorkspace,
        ServiceRoute::StdInstant,
        ServiceRoute::ThreadedSchedulerPools,
        ServiceRoute::DurableArtifactStore,
        ServiceRoute::NativeProcessSpawn,
    ] {
        assert!(
            !route.browser_admissible(),
            "native-only route {route:?} must not be browser-admissible"
        );
    }
    // Browser routes stay admissible so the guard is a boundary, not a
    // blanket refusal.
    for route in [
        ServiceRoute::HostProvidedVirtualWorkspace,
        ServiceRoute::WebInstant,
        ServiceRoute::InlineDriveScheduler,
        ServiceRoute::InMemoryCachesOnly,
        ServiceRoute::Unavailable,
    ] {
        assert!(route.browser_admissible());
    }
}

/// `current()` answers the profile the compiled target requires, and
/// both profiles carry the same engine version binding.
#[test]
fn current_profile_matches_compiled_target_and_binds_engine_version() {
    let profile = PortableHostServices::current();
    if cfg!(target_arch = "wasm32") {
        assert_eq!(profile.execution_host(), ExecutionHostKind::Browser);
    } else {
        assert_eq!(profile.execution_host(), ExecutionHostKind::Native);
    }
    assert_eq!(profile.engine_version(), env!("CARGO_PKG_VERSION"));
    assert_eq!(
        PortableHostServices::browser().engine_version(),
        PortableHostServices::native().engine_version()
    );
}

/// The scheduling seam is verified against the profile it was compiled
/// for: the matching route passes, the opposite target's route is a
/// construction refusal.
#[test]
fn scheduling_route_verification_separates_transports() {
    let native = PortableHostServices::native();
    assert!(native
        .verify_scheduling_route(ServiceRoute::ThreadedSchedulerPools)
        .is_ok());
    assert!(native
        .verify_scheduling_route(ServiceRoute::InlineDriveScheduler)
        .is_err());

    let browser = PortableHostServices::browser();
    assert!(browser
        .verify_scheduling_route(ServiceRoute::InlineDriveScheduler)
        .is_ok());
    assert!(browser
        .verify_scheduling_route(ServiceRoute::ThreadedSchedulerPools)
        .is_err());
}

/// A host binds the profile its target requires and the accessor
/// exposes it (the seam a browser export binds execution host and
/// engine version through).
#[test]
fn constructed_host_binds_current_platform_profile() {
    let host = crate::VerterHost::new_standalone(crate::types::HostConfig::default());
    let bound = host.platform_services();
    assert_eq!(
        bound.execution_host(),
        PortableHostServices::current().execution_host()
    );
    assert_eq!(
        bound.engine_version(),
        PortableHostServices::current().engine_version()
    );
    if cfg!(target_arch = "wasm32") {
        bound
            .verify_browser_closure()
            .expect("wasm32 host must bind a browser-verifiable route set");
    }
}
