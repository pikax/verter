//! Portable platform-services boundary.
//!
//! The session host consumes exactly five platform service classes —
//! I/O, time, scheduling, persistence and process — and each class has
//! exactly one route per execution host. This module is the single
//! inventory that names those routes, the construction-time check that
//! binds the built host to the route set its target actually provides,
//! and the dependency guard that refuses a native-only route inside the
//! browser closure.
//!
//! The inventory mirrors `tests/browser-host/BWH0/products/
//! platform-host-services.v1.json` (the roadmap-side constitution) but
//! is owned here: a route change at a host boundary must change this
//! module and fail its guard, not pass silently because the JSON and
//! the code drifted.
//!
//! Native callers retain every native service. The browser closure
//! (wasm32) routes the same classes through the host-provided virtual
//! workspace, `web_time`, the inline-drive scheduler and in-memory
//! caches, and has NO process capability — the explicitly selected
//! authenticated local companion is the only sanctioned browser-to-
//! native route and lives outside this closure.

use std::sync::Arc;

/// Which execution host a [`PortableHostServices`] profile describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionHostKind {
    /// Threaded native runtime: driver thread, worker pools, disk.
    Native,
    /// Browser runtime (wasm32): no threads, no disk, no processes.
    Browser,
}

impl ExecutionHostKind {
    /// Stable wire spelling (`"native"` / `"browser"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Browser => "browser",
        }
    }
}

/// One platform service class the session host consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformServiceClass {
    /// File reads and workspace backing.
    Io,
    /// Monotonic wall/elapsed time.
    Time,
    /// Stage execution transport for scheduler work.
    Scheduling,
    /// Durable artifact and cache roots.
    Persistence,
    /// Child-process capability (provider spawn).
    Process,
}

impl PlatformServiceClass {
    /// All classes, in inventory order.
    pub const ALL: [Self; 5] = [
        Self::Io,
        Self::Time,
        Self::Scheduling,
        Self::Persistence,
        Self::Process,
    ];
}

/// The concrete route one service class takes on one execution host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceRoute {
    /// Native: filesystem-backed workspace plus ambient-library reads.
    NativeFilesystemWorkspace,
    /// Browser: host-provided in-memory virtual workspace.
    HostProvidedVirtualWorkspace,
    /// Native: `std::time::Instant`.
    StdInstant,
    /// Browser: `web_time::Instant`.
    WebInstant,
    /// Native: scheduler driver thread plus CPU/IO worker pools.
    ThreadedSchedulerPools,
    /// Browser: sync inline drive — the calling thread pumps stages.
    InlineDriveScheduler,
    /// Native: env-hash durable artifact stores.
    DurableArtifactStore,
    /// Browser: in-memory caches only; nothing durable.
    InMemoryCachesOnly,
    /// Native: child-process provider spawn (feature-gated).
    NativeProcessSpawn,
    /// Browser: no process capability; companion route only (external).
    Unavailable,
}

impl ServiceRoute {
    /// Whether this route may exist inside the browser closure.
    #[must_use]
    pub fn browser_admissible(self) -> bool {
        !matches!(
            self,
            Self::NativeFilesystemWorkspace
                | Self::StdInstant
                | Self::ThreadedSchedulerPools
                | Self::DurableArtifactStore
                | Self::NativeProcessSpawn
        )
    }
}

/// A native-only route was offered to the browser closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeServiceInBrowserClosure {
    /// The offending class.
    pub class: PlatformServiceClass,
    /// The native-only route that must not reach the browser closure.
    pub route: ServiceRoute,
}

impl std::fmt::Display for NativeServiceInBrowserClosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "native-only platform route {:?} for service class {:?} inside the browser closure",
            self.route, self.class
        )
    }
}

impl std::error::Error for NativeServiceInBrowserClosure {}

/// The bound platform-services profile of one host construction.
///
/// Owns no service itself: it is the named record of which routes the
/// constructed host actually received, the engine version they execute
/// under, and the guard surface for the browser closure. Bind it once
/// at construction via [`Self::current`] and read it through
/// [`crate::VerterHost::platform_services`].
#[derive(Debug, Clone)]
pub struct PortableHostServices {
    host: ExecutionHostKind,
    engine_version: Arc<str>,
}

impl PortableHostServices {
    /// Native profile: every service class keeps its native route.
    #[must_use]
    pub fn native() -> Self {
        Self {
            host: ExecutionHostKind::Native,
            engine_version: Arc::from(env!("CARGO_PKG_VERSION")),
        }
    }

    /// Browser profile: the wasm32 route set. Construction on wasm32
    /// must verify this profile (see [`Self::verify_browser_closure`])
    /// before the host is published.
    #[must_use]
    pub fn browser() -> Self {
        Self {
            host: ExecutionHostKind::Browser,
            engine_version: Arc::from(env!("CARGO_PKG_VERSION")),
        }
    }

    /// The profile the current compilation target requires. This is the
    /// compile-time boundary decision; the scheduler/workspace seams
    /// check their own `cfg` selection against it at construction so
    /// the two cannot drift apart silently.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_arch = "wasm32") {
            Self::browser()
        } else {
            Self::native()
        }
    }

    /// Execution host this profile was bound for.
    #[must_use]
    pub fn execution_host(&self) -> ExecutionHostKind {
        self.host
    }

    /// Semantic-host engine version bound at construction. Every
    /// exposed product of this host carries this version.
    #[must_use]
    pub fn engine_version(&self) -> &str {
        &self.engine_version
    }

    /// The route `class` takes under this profile.
    #[must_use]
    pub fn route(&self, class: PlatformServiceClass) -> ServiceRoute {
        match (self.host, class) {
            (ExecutionHostKind::Native, PlatformServiceClass::Io) => {
                ServiceRoute::NativeFilesystemWorkspace
            }
            (ExecutionHostKind::Native, PlatformServiceClass::Time) => ServiceRoute::StdInstant,
            (ExecutionHostKind::Native, PlatformServiceClass::Scheduling) => {
                ServiceRoute::ThreadedSchedulerPools
            }
            (ExecutionHostKind::Native, PlatformServiceClass::Persistence) => {
                ServiceRoute::DurableArtifactStore
            }
            (ExecutionHostKind::Native, PlatformServiceClass::Process) => {
                ServiceRoute::NativeProcessSpawn
            }
            (ExecutionHostKind::Browser, PlatformServiceClass::Io) => {
                ServiceRoute::HostProvidedVirtualWorkspace
            }
            (ExecutionHostKind::Browser, PlatformServiceClass::Time) => ServiceRoute::WebInstant,
            (ExecutionHostKind::Browser, PlatformServiceClass::Scheduling) => {
                ServiceRoute::InlineDriveScheduler
            }
            (ExecutionHostKind::Browser, PlatformServiceClass::Persistence) => {
                ServiceRoute::InMemoryCachesOnly
            }
            (ExecutionHostKind::Browser, PlatformServiceClass::Process) => {
                ServiceRoute::Unavailable
            }
        }
    }

    /// Dependency guard: refuse any native-only route inside the
    /// browser closure. A browser profile carrying a native route is a
    /// construction bug, not a degraded mode.
    pub fn verify_browser_closure(&self) -> Result<(), NativeServiceInBrowserClosure> {
        if self.host != ExecutionHostKind::Browser {
            return Ok(());
        }
        for class in PlatformServiceClass::ALL {
            let route = self.route(class);
            if !route.browser_admissible() {
                return Err(NativeServiceInBrowserClosure { class, route });
            }
        }
        Ok(())
    }

    /// Check the scheduling route a host seam actually selected against
    /// the route this profile records. The scheduler construction site
    /// compiles its choice under `cfg`; this check makes the inventory
    /// the authority the compiled decision is verified against, so a
    /// future seam edit that diverges from the profile fails host
    /// construction loudly on the affected target.
    pub fn verify_scheduling_route(&self, selected: ServiceRoute) -> Result<(), &'static str> {
        let expected = self.route(PlatformServiceClass::Scheduling);
        if expected == selected {
            Ok(())
        } else {
            Err("scheduler transport seam diverges from the bound platform-services profile")
        }
    }
}

#[cfg(test)]
#[path = "platform_services_tests.rs"]
mod platform_services_tests;
