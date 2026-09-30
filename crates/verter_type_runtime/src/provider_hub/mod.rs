//! The provider lifecycle owner: establishment, recovery, desired-state replay
//! and the serving [`ProviderEpoch`].
//!
//! A [`ProviderHub`] is ONE provider instance (one tsgo engine, one
//! project-bound tsserver, one managed fallback chain). It is the only
//! component allowed to:
//!
//! * **establish** an engine ([`ProviderHub::establish`]) — singleflight per
//!   instance, bounded by [`HubPolicy::establish_timeout`], with a retry
//!   cooldown after a failure so a persistently failing backend cannot hot-loop;
//! * **recover** a crashed engine ([`ProviderHub::recover`]) — retire the
//!   crashed epoch, respawn within [`HubPolicy::max_restarts`] with bounded
//!   backoff, and replay the desired state;
//! * **replay** the desired editor state into a fresh engine before any caller
//!   can observe it;
//! * **mint** the serving [`ProviderEpoch`] and applied receipts.
//!
//! Adapters (the tsgo / tsserver transports and their [`ProviderEstablisher`]
//! strategies) only spawn processes, frame messages, bound their own I/O and
//! report death through the crash signal the hub hands them. They never decide
//! readiness, restart an engine, replay state or accept a retired engine's
//! results.
//!
//! # Single-writer actor
//!
//! The desired state and the serving cell are written by ONE actor task per
//! hub. Every state mutation is a typed command; the actor records it, then
//! forwards it to the serving engine. Because the actor owns the desired
//! state, a replay reads the set as it is at install time — a mutation issued
//! while an engine is being (re-)established is either folded into the replay
//! or applied after the engine goes live, never lost and never observed
//! half-applied.
//!
//! The actor stays receptive to control while it waits on an engine: a crash
//! retirement or a shutdown interrupts a wedged forward or replay; every other
//! command queues behind it and settles in submission order. Each hub has its
//! own actor, so a held or failed instance never blocks work admitted to an
//! independent instance.
//!
//! # Epoch fencing
//!
//! Every installed engine carries a freshly minted epoch. A crash signal names
//! the epoch it retires, so a late signal from an already-replaced engine is
//! inert. A query that was served by an engine that has since been retired
//! settles as an error, never as a result attributed to the engine that
//! replaced it; a mutation's receipt names the epoch that actually applied it.
//!
//! # Deadlines
//!
//! A caller's ambient request deadline ([`crate::deadline`]) is captured ONCE
//! at submission and travels with the command: the forward runs under the same
//! absolute deadline on the actor task, and the submitter's wait for the
//! settlement is bounded by it.
//!
//! # Lock discipline
//!
//! No synchronous guard is ever held across an `.await` or a channel send. The
//! crate denies `clippy::await_holding_lock` to keep this enforced.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock, Weak};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, watch, Notify};

use crate::protocol::TypeProviderError;
use crate::traits::TypeProvider;

mod admission;
mod desired;
mod epoch;
mod forwarding;
mod quarantine;

pub use admission::{
    AdmissionRefusal, AdmittedRequest, DroppedAdmittedCarrier, DroppedAdmittedState,
    OverlayFileKind, OverlayMutation, OverlayPriority, ProjectBasis, ProjectBindingInput,
    ProjectWitness,
};
use desired::{DesiredMutation, DesiredState, Disposition, Lane};
use epoch::EpochMint;
use quarantine::{InFlightGuard, QueryFingerprint, QueryWatch};

pub use verter_identity::identity::ProviderEpoch;

/// Notification severity levels for provider events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifySeverity {
    Info,
    Warning,
    Error,
}

/// Which kind of engine start a structural announcement describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStart {
    /// A fresh engine began serving this instance — its first installation, or
    /// a re-activation after a failed attempt or a settled cooldown.
    Initial,
    /// A replacement engine took over from a crashed one within recovery.
    Recovery,
}

/// Notification interface for provider lifecycle events.
///
/// The LSP implements this to call `client.show_message()`.
/// Component-meta can use a logging-only or no-op implementation.
pub trait ProviderNotifier: Send + Sync + 'static {
    fn notify(&self, severity: NotifySeverity, message: String);

    /// An engine child the HUB established (on demand or by recovery) went
    /// live, carrying its process id when the platform reports one.
    ///
    /// Structural, not prose: the editor tracks the provider by pid, and a
    /// benchmark receipt must be able to COUNT engine restarts. A run that tore
    /// its engine down and rebuilt it mid-flight is not a clean latency
    /// measurement, and nothing else on the wire says so.
    ///
    /// [`EngineStart`] lets an adapter distinguish a first serve from a crash
    /// replacement: the wire contract of a shared route attests its managed
    /// fallback stays cold, so its adapter may announce replacements only,
    /// while a route whose engine is announced at startup announces both.
    ///
    /// Deliberately has no default body: a silent default is how a provider
    /// silently inherits behaviour it was supposed to override.
    fn provider_started(&self, pid: Option<u32>, start: EngineStart);

    /// Admitted generated state was dropped because a replacement engine
    /// installed: the recorded carriers and overlays exist in NO engine now,
    /// and the desired state will not replay them — a replacement may only
    /// receive them through a FRESH admission against its own serving epoch.
    ///
    /// This is the recovery re-arm signal: the tier that minted the dropped
    /// admissions re-runs its publication (resolve → admit → apply) for the
    /// named units, so a recovered engine is not left without its companion
    /// registrations and overlays until the next ordinary publication. The hub
    /// emits it AFTER the replacement is installed (a re-arm can bind a
    /// witness to the new epoch). Defaulted: a tier without hub-issued
    /// admissions never drops any.
    fn admitted_state_dropped(&self, dropped: &admission::DroppedAdmittedState) {
        let _ = dropped;
    }
}

/// No-op notifier (logs via tracing only).
pub struct TracingNotifier;

impl ProviderNotifier for TracingNotifier {
    fn notify(&self, severity: NotifySeverity, message: String) {
        match severity {
            NotifySeverity::Info => tracing::info!("{}", message),
            NotifySeverity::Warning => tracing::warn!("{}", message),
            NotifySeverity::Error => tracing::error!("{}", message),
        }
    }

    fn provider_started(&self, pid: Option<u32>, start: EngineStart) {
        tracing::info!("type provider started (pid: {pid:?}, {start:?})");
    }
}

/// The future an establishment strategy returns.
pub type EstablishFuture<'a, P> =
    Pin<Box<dyn Future<Output = Result<Arc<P>, TypeProviderError>> + Send + 'a>>;

/// The adapter-side establishment strategy of one provider instance.
///
/// `establish` spawns and handshakes ONE engine and wires `crash_signal` into
/// its transport, so the engine's death (or a detected hang) reaches the hub.
/// It must not replay state, retry, or install anything: the hub does.
pub trait ProviderEstablisher<P: ?Sized>: Send + Sync + 'static {
    fn log_name(&self) -> &'static str;

    fn user_label(&self) -> &'static str;

    fn restarting_error(&self) -> &'static str;

    /// Whether the engine this strategy establishes answers
    /// `completionItem/resolve` — reported while no engine is serving.
    fn supports_completion_resolve(&self) -> bool;

    fn establish<'a>(&'a self, crash_signal: Arc<Notify>) -> EstablishFuture<'a, P>;
}

/// Default bound on one establishment step (engine spawn + handshake, then the
/// desired-state replay into it).
pub const DEFAULT_ESTABLISH_TIMEOUT: Duration = Duration::from_secs(60);

/// Lifecycle policy of one hub instance.
#[derive(Debug, Clone, Copy)]
pub struct HubPolicy {
    /// Recovery attempts per crash before the instance stays down for the
    /// rest of the session. Reset by every successful recovery.
    pub max_restarts: u32,
    /// Bound on each establishment step: the engine spawn and handshake, and
    /// separately the replay into it. An elapsed step is a failed attempt.
    pub establish_timeout: Duration,
    /// `Some(cooldown)`: a query with no serving engine establishes one on
    /// demand, and a failed attempt is not retried until `cooldown` elapsed.
    /// `None`: the hub establishes only through [`ProviderHub::establish`] and
    /// recovery, and queries without a serving engine fail closed.
    pub on_demand: Option<Duration>,
    /// `true` for an externally-attached transport (the shared editor
    /// attach): a death RETIRES the serving epoch fail-closed without the
    /// respawn loop. Re-establishment belongs to the re-arm door
    /// ([`ProviderHub::establish_rearming`]) — a fresh generation
    /// discriminant — never to a backoff storm against a dead attach, and
    /// the instance is never declared exhausted: a later fresh
    /// discriminant (a reconnect) still re-arms.
    pub retire_only_on_crash: bool,
}

impl HubPolicy {
    /// An instance the caller establishes explicitly and the hub recovers.
    #[must_use]
    pub const fn explicit(max_restarts: u32) -> Self {
        Self {
            max_restarts,
            establish_timeout: DEFAULT_ESTABLISH_TIMEOUT,
            on_demand: None,
            retire_only_on_crash: false,
        }
    }

    /// An instance established by its first query demand.
    #[must_use]
    pub const fn on_demand(max_restarts: u32, retry_cooldown: Duration) -> Self {
        Self {
            max_restarts,
            establish_timeout: DEFAULT_ESTABLISH_TIMEOUT,
            on_demand: Some(retry_cooldown),
            retire_only_on_crash: false,
        }
    }

    /// An externally-attached transport the re-arm door
    /// ([`ProviderHub::establish_rearming`]) establishes lazily: a death
    /// retires the epoch fail-closed (no respawn loop, no exhaustion), and a
    /// failed or evicted attach re-attempts only through a fresh generation
    /// discriminant.
    #[must_use]
    pub const fn lazy_attach(establish_timeout: Duration) -> Self {
        Self {
            max_restarts: 0,
            establish_timeout,
            on_demand: None,
            retire_only_on_crash: true,
        }
    }

    #[must_use]
    pub const fn with_establish_timeout(mut self, establish_timeout: Duration) -> Self {
        self.establish_timeout = establish_timeout;
        self
    }
}

/// Proof that the hub applied a desired-state mutation.
///
/// `epoch` names the serving incarnation the mutation was forwarded to and
/// accepted by; `None` means no engine was serving and the mutation is held
/// in the desired state for the next establishment's replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppliedReceipt {
    pub(crate) epoch: Option<ProviderEpoch>,
}

/// One serving incarnation.
struct Serving<P: ?Sized> {
    provider: Arc<P>,
    epoch: ProviderEpoch,
    crash_signal: Arc<Notify>,
}

impl<P: ?Sized> Clone for Serving<P> {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            epoch: self.epoch,
            crash_signal: Arc::clone(&self.crash_signal),
        }
    }
}

type EstablishOutcome = Option<Result<ProviderEpoch, String>>;

enum Phase {
    /// No lifecycle transition in flight.
    Idle,
    /// One establishment is in flight; demands join it.
    Establishing(watch::Receiver<EstablishOutcome>),
    /// A crash recovery owns the instance until it installs or gives up.
    Recovering,
    /// Recovery exhausted its budget: the instance stays down.
    Exhausted,
}

struct Lifecycle {
    phase: Phase,
    /// Advanced by every deliberate shutdown. Establishments, recoveries and
    /// crash monitors captured at an older value are abandoned.
    teardown_generation: u64,
    last_failure: Option<(Instant, String)>,
    /// The re-arm gate of a lazily-attached transport
    /// ([`HubPolicy::lazy_attach`]): the generation discriminant of the last
    /// FAILED attempt — or of the establishment a death evicted. `None` while
    /// an incarnation serves (or before the first attempt): every demand may
    /// establish. `Some(discriminant)` while gated: only a probe that ADVANCES
    /// past it re-arms; an unchanged discriminant — or an unobservable one
    /// (`None`, gated by a first failure at `None` itself) — fails closed
    /// without a new attempt.
    attach_gate: Option<Option<String>>,
    /// The discriminant the serving (or last evicted) incarnation
    /// established at — re-read AFTER a successful establishment, so an
    /// advertisement that advanced mid-handshake is carried as the
    /// establishment's own. A death arms [`Lifecycle::attach_gate`] with it.
    established_discriminant: Option<String>,
}

/// State shared between the hub handle and its actor.
struct Shared<P: ?Sized> {
    /// Read by queries; written ONLY by the actor.
    serving: StdRwLock<Option<Serving<P>>>,
    /// The engine tier of the most recent serving incarnation, recorded by
    /// the actor at every install; written ONLY by the actor.
    last_serving_id: StdRwLock<Option<&'static str>>,
    epochs: EpochMint,
    lifecycle: StdMutex<Lifecycle>,
    query_watch: Arc<StdMutex<QueryWatch>>,
    admission: StdMutex<admission::AdmissionState>,
}

impl<P: ?Sized> Shared<P> {
    fn serving(&self) -> Option<Serving<P>> {
        self.serving
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn last_serving_id(&self) -> Option<&'static str> {
        *self
            .last_serving_id
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn serving_epoch(&self) -> Option<ProviderEpoch> {
        self.serving
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|serving| serving.epoch)
    }

    fn lifecycle(&self) -> std::sync::MutexGuard<'_, Lifecycle> {
        self.lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn teardown_generation(&self) -> u64 {
        self.lifecycle().teardown_generation
    }
}

enum Command<P: ?Sized> {
    /// Record a desired-state mutation and forward it to the serving engine.
    Mutate {
        mutation: DesiredMutation,
        lane: Lane,
        deadline: Option<tokio::time::Instant>,
        ack: oneshot::Sender<Result<AppliedReceipt, TypeProviderError>>,
    },
    /// An admitted generated unit is forwarded only to the exact serving
    /// incarnation. It is never put in desired state for unproven replay.
    ApplyOverlay {
        mutation: DesiredMutation,
        admissions: Vec<AdmittedRequest>,
        lane: Lane,
        deadline: Option<tokio::time::Instant>,
        ack: oneshot::Sender<Result<AppliedReceipt, AdmissionRefusal>>,
    },
    /// The engine serving `epoch` died: retire it so queries fail closed.
    Retire {
        epoch: ProviderEpoch,
        ack: oneshot::Sender<()>,
    },
    /// A freshly established engine: replay the desired state into it, then
    /// install it under a newly minted epoch.
    Install {
        provider: Arc<P>,
        crash_signal: Arc<Notify>,
        teardown_generation: u64,
        replay_timeout: Duration,
        ack: oneshot::Sender<Result<ProviderEpoch, TypeProviderError>>,
    },
    /// Deliberate teardown of the serving engine.
    Shutdown { ack: oneshot::Sender<()> },
}

struct HubState<P: ?Sized> {
    shared: Arc<Shared<P>>,
    commands: mpsc::UnboundedSender<Command<P>>,
    notifier: Arc<dyn ProviderNotifier>,
    establisher: Box<dyn ProviderEstablisher<P>>,
    policy: HubPolicy,
}

impl<P: ?Sized + 'static> HubState<P> {
    /// The terminal-state error of an instance whose recovery exhausted its
    /// restart budget: it stays down for the rest of the session, and no
    /// caller may be told a restart is in progress.
    fn exhausted(&self) -> TypeProviderError {
        TypeProviderError::new(format!(
            "{} exhausted its restart budget and stays down for this session",
            self.establisher.log_name()
        ))
    }
}

impl<P: ?Sized> Drop for HubState<P> {
    fn drop(&mut self) {
        // Release the parked crash monitor of the serving incarnation: it holds
        // only a weak handle, finds the hub gone, and exits.
        if let Some(serving) = self.shared.serving() {
            serving.crash_signal.notify_one();
        }
    }
}

/// The lifecycle owner of one provider instance. See the module docs.
pub struct ProviderHub<P: ?Sized> {
    state: Arc<HubState<P>>,
}

impl<P> ProviderHub<P>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    /// A hub with no engine yet. Nothing is spawned until
    /// [`ProviderHub::establish`] (or, under [`HubPolicy::on_demand`], the
    /// first query) asks for one.
    pub fn new(
        establisher: impl ProviderEstablisher<P>,
        notifier: Arc<dyn ProviderNotifier>,
        policy: HubPolicy,
    ) -> Self {
        let shared = Arc::new(Shared {
            serving: StdRwLock::new(None),
            last_serving_id: StdRwLock::new(None),
            epochs: EpochMint::new(),
            lifecycle: StdMutex::new(Lifecycle {
                phase: Phase::Idle,
                teardown_generation: 0,
                last_failure: None,
                attach_gate: None,
                established_discriminant: None,
            }),
            query_watch: Arc::new(StdMutex::new(QueryWatch::default())),
            admission: StdMutex::new(admission::AdmissionState::default()),
        });
        let (commands, command_rx) = mpsc::unbounded_channel();
        let log_name = establisher.log_name();
        let demand_driven = policy.on_demand.is_some();
        tokio::spawn(run_actor(
            command_rx,
            Arc::clone(&shared),
            Arc::clone(&notifier),
            log_name,
            demand_driven,
        ));
        Self {
            state: Arc::new(HubState {
                shared,
                commands,
                notifier,
                establisher: Box::new(establisher),
                policy,
            }),
        }
    }

    /// The serving epoch, or `None` while no engine serves.
    #[must_use]
    pub fn serving_epoch(&self) -> Option<ProviderEpoch> {
        self.state.shared.serving_epoch()
    }

    /// Whether an engine is serving now.
    #[must_use]
    pub fn is_serving(&self) -> bool {
        self.serving_epoch().is_some()
    }

    /// Whether this instance has ever had an engine installed. Such an
    /// instance keeps accepting state updates while it recovers (or after it
    /// gave up): they are held for the next incarnation, and queries fail
    /// closed until one serves.
    #[must_use]
    pub fn has_served(&self) -> bool {
        self.state.shared.epochs.minted_any()
    }

    /// Establish an engine for this instance, or return the serving epoch.
    ///
    /// Singleflight: concurrent demands join the one in-flight establishment.
    /// The establishment runs on its own task, so a caller that gives up (its
    /// deadline elapsed, it was cancelled) neither cancels it nor causes a
    /// second one; the engine is replayed and installed or torn down, never
    /// orphaned. A failure is not retried within the policy's retry cooldown.
    pub async fn establish(&self) -> Result<ProviderEpoch, TypeProviderError> {
        establish(&self.state).await
    }

    /// Establish through the re-arm door of a lazily-attached transport
    /// ([`HubPolicy::lazy_attach`]) — the door that replaces a wall-clock
    /// retry cooldown with the transport's own re-arm authority: an external
    /// generation discriminant (a reconnect nonce, a workspace/config
    /// generation).
    ///
    /// A demand FAILS CLOSED — no attempt, no I/O — while the observed
    /// discriminant is UNCHANGED since the last failed attempt (or since the
    /// establishment a death evicted), so a dead or absent attach is never
    /// stormed; any ADVANCE (a fresh discriminant) re-arms and establishes
    /// through the same singleflight as [`ProviderHub::establish`]. A serving
    /// incarnation returns its epoch without probing.
    ///
    /// `probe` reads the CURRENT discriminant (`None` when none is
    /// observable — a first failure AT `None` gates exactly like one at an
    /// observable discriminant and never re-arms on another `None`). The
    /// discriminant is re-read AFTER a success and retained as the
    /// establishment's own, so a generation that advanced mid-establishment
    /// is what a later eviction compares against.
    pub async fn establish_rearming(
        &self,
        probe: impl Fn() -> Option<String>,
    ) -> Result<ProviderEpoch, TypeProviderError> {
        if let Some(epoch) = self.state.shared.serving_epoch() {
            return Ok(epoch);
        }
        let gate = self.state.shared.lifecycle().attach_gate.clone();
        let observed = probe();
        if let Some(failed_at) = gate {
            if !generation_advanced(&failed_at, &observed) {
                return Err(TypeProviderError::new(format!(
                    "{} attach failed at the current generation and re-arms only on a fresh one",
                    self.state.establisher.log_name()
                )));
            }
        }
        match establish(&self.state).await {
            Ok(epoch) => {
                let established_at = probe();
                let mut lifecycle = self.state.shared.lifecycle();
                lifecycle.established_discriminant = established_at;
                lifecycle.attach_gate = None;
                Ok(epoch)
            }
            Err(error) => {
                self.state.shared.lifecycle().attach_gate = Some(observed);
                Err(error)
            }
        }
    }

    /// The serving provider incarnation, if one serves: the exact instance
    /// and epoch every hub-issued witness and admitted request binds to. A
    /// hub with no serving engine yields `None` (the caller fails closed).
    #[must_use]
    pub fn serving(&self) -> Option<(Arc<P>, ProviderEpoch)> {
        self.state
            .shared
            .serving()
            .map(|serving| (Arc::clone(&serving.provider), serving.epoch))
    }

    /// Recover from the death of the engine serving `retired`.
    ///
    /// Inert unless `retired` is the serving epoch and no recovery or teardown
    /// is under way: a late signal from an already-replaced engine never
    /// disturbs its replacement. The crash signal an establishment hands the
    /// adapter drives this; it is public so an adapter-independent detector
    /// can report a wedged engine through the same authority.
    pub async fn recover(&self, retired: ProviderEpoch) {
        let teardown_generation = self.state.shared.teardown_generation();
        recover(Arc::clone(&self.state), retired, teardown_generation).await;
    }

    /// Record `mutation` and forward it to the serving engine.
    ///
    /// The mutation's effect on the crash quarantine (a content change lifts
    /// the touched paths' attribution) is applied by the actor when it records
    /// the mutation, so it holds even when the submitter's deadline elapses
    /// before the settlement — the mutation stays queued and applies in order.
    async fn submit_mutation(
        &self,
        mutation: DesiredMutation,
        lane: Lane,
    ) -> Result<AppliedReceipt, TypeProviderError> {
        let deadline = crate::deadline::current();
        let (ack, ack_rx) = oneshot::channel();
        self.state
            .commands
            .send(Command::Mutate {
                mutation,
                lane,
                deadline,
                ack,
            })
            .map_err(|_| self.restarting())?;
        let settled = match deadline {
            Some(at) => tokio::time::timeout_at(at, ack_rx).await.map_err(|_| {
                TypeProviderError::new(format!(
                    "{}: request deadline elapsed before the state update settled; \
                     it stays queued and applies in order",
                    self.state.establisher.log_name()
                ))
            })?,
            None => ack_rx.await,
        };
        settled.map_err(|_| self.restarting())?
    }

    async fn mutate(&self, mutation: DesiredMutation, lane: Lane) -> Result<(), TypeProviderError> {
        self.submit_mutation(mutation, lane).await.map(|_| ())
    }

    fn restarting(&self) -> TypeProviderError {
        TypeProviderError::new(self.state.establisher.restarting_error())
    }

    /// The serving incarnation for a query, establishing one on demand when
    /// the policy allows it.
    async fn serving_for_query(&self) -> Result<Serving<P>, TypeProviderError> {
        if let Some(serving) = self.state.shared.serving() {
            return Ok(serving);
        }
        if self.state.policy.on_demand.is_none() {
            // An explicit hub cannot establish on demand: fail closed with the
            // LIFECYCLE's truth — a hub that exhausted its restart budget is
            // terminal, not "restarting".
            let exhausted = matches!(self.state.shared.lifecycle().phase, Phase::Exhausted);
            return Err(if exhausted {
                self.state.exhausted()
            } else {
                self.restarting()
            });
        }
        self.establish().await?;
        self.state.shared.serving().ok_or_else(|| self.restarting())
    }

    /// Settle a query result against the epoch that produced it: a result from
    /// an engine that has since been retired never reaches the caller as a
    /// result of the current incarnation.
    fn settle<T>(
        &self,
        epoch: ProviderEpoch,
        result: Result<T, TypeProviderError>,
    ) -> Result<T, TypeProviderError> {
        let value = result?;
        if self.state.shared.serving_epoch() == Some(epoch) {
            Ok(value)
        } else {
            Err(TypeProviderError::new(format!(
                "{}: the answering engine was retired before its result settled",
                self.state.establisher.log_name()
            )))
        }
    }

    /// Run a read-only provider query under crash quarantine.
    ///
    /// A fingerprint attributed to an engine crash is NEVER replayed into the
    /// (recovered) engine: it fails closed with `fail_closed()` instead —
    /// preventing the identical killer request from re-crashing the fresh
    /// process in an infinite crash-restart loop.
    async fn run_guarded<T, F, Fut>(
        &self,
        fp: QueryFingerprint,
        fail_closed: impl FnOnce() -> T,
        run: F,
    ) -> Result<T, TypeProviderError>
    where
        F: FnOnce(Arc<P>) -> Fut,
        Fut: Future<Output = Result<T, TypeProviderError>>,
    {
        self.run_guarded_with_fallback(fp, || Ok(fail_closed()), run)
            .await
    }

    /// Diagnostics cannot equate quarantine with a completed, clean check.
    async fn run_guarded_with_fallback<T, F, Fut>(
        &self,
        fp: QueryFingerprint,
        fail_closed: impl FnOnce() -> Result<T, TypeProviderError>,
        run: F,
    ) -> Result<T, TypeProviderError>
    where
        F: FnOnce(Arc<P>) -> Fut,
        Fut: Future<Output = Result<T, TypeProviderError>>,
    {
        let serving = self.serving_for_query().await?;
        let quarantined = self
            .state
            .shared
            .query_watch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_quarantined(&fp);
        if quarantined {
            tracing::warn!(
                "{} query {} {}@{} is quarantined after an engine crash — failing closed \
                 (a content change to the file lifts the quarantine)",
                self.state.establisher.log_name(),
                fp.method,
                fp.path,
                fp.offset,
            );
            return fail_closed();
        }
        let guard = InFlightGuard::begin(Arc::clone(&self.state.shared.query_watch), fp);
        let result = run(serving.provider).await;
        // Settle FIRST, and only a settlement the serving epoch accepted counts
        // as a success: an answer the epoch discarded proves nothing about the
        // request and must not erase its crash strikes.
        let settled = self.settle(serving.epoch, result);
        guard.complete(settled.is_ok());
        settled
    }

    /// Deliberate teardown: retire the serving engine and abandon any
    /// establishment or recovery in flight. The torn-down child's exit is not
    /// a crash. A hub under [`HubPolicy::on_demand`] establishes afresh on its
    /// next demand; any other hub stays down.
    async fn shutdown_hub(&self) {
        {
            let mut lifecycle = self.state.shared.lifecycle();
            lifecycle.teardown_generation += 1;
        }
        let (ack, ack_rx) = oneshot::channel();
        if self.state.commands.send(Command::Shutdown { ack }).is_ok() {
            let _ = ack_rx.await;
        }
    }
}

/// Join or start the one establishment of `state`'s instance.
async fn establish<P>(state: &Arc<HubState<P>>) -> Result<ProviderEpoch, TypeProviderError>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    if let Some(epoch) = state.shared.serving_epoch() {
        return Ok(epoch);
    }
    let mut outcome = {
        let mut lifecycle = state.shared.lifecycle();
        // Re-check under the lifecycle lock: an establishment that installed
        // after the unlocked read has already returned the phase to idle.
        if let Some(epoch) = state.shared.serving_epoch() {
            return Ok(epoch);
        }
        match &lifecycle.phase {
            Phase::Establishing(outcome) => outcome.clone(),
            Phase::Recovering => {
                return Err(TypeProviderError::new(state.establisher.restarting_error()))
            }
            Phase::Exhausted => return Err(state.exhausted()),
            Phase::Idle => {
                if let (Some(cooldown), Some((failed_at, message))) =
                    (state.policy.on_demand, &lifecycle.last_failure)
                {
                    if failed_at.elapsed() < cooldown {
                        return Err(TypeProviderError::new(format!(
                            "{} establishment failed (retry pending): {message}",
                            state.establisher.log_name()
                        )));
                    }
                }
                let (publish, outcome) = watch::channel(None);
                lifecycle.phase = Phase::Establishing(outcome.clone());
                let teardown_generation = lifecycle.teardown_generation;
                let flight_state = Arc::clone(state);
                tokio::spawn(async move {
                    let mut abandoned = AbandonedFlight {
                        shared: &flight_state.shared,
                        armed: true,
                    };
                    let result = establish_once(&flight_state, teardown_generation).await;
                    abandoned.armed = false;
                    {
                        let mut lifecycle = flight_state.shared.lifecycle();
                        lifecycle.phase = Phase::Idle;
                        lifecycle.last_failure = match &result {
                            Ok(_) => None,
                            Err(error) => Some((Instant::now(), error.message.clone())),
                        };
                    }
                    match &result {
                        Ok(installed) => {
                            let (epoch, pid) = (installed.epoch, installed.pid);
                            installed.arm_crash_monitor(&flight_state, teardown_generation);
                            tracing::info!(
                                "{} established (epoch {})",
                                flight_state.establisher.log_name(),
                                epoch.0
                            );
                            flight_state
                                .notifier
                                .provider_started(pid, EngineStart::Initial);
                        }
                        Err(error) => tracing::warn!(
                            "{} establishment failed: {error}",
                            flight_state.establisher.log_name()
                        ),
                    }
                    let _ = publish.send(Some(
                        result
                            .map(|installed| installed.epoch)
                            .map_err(|error| error.message),
                    ));
                });
                outcome
            }
        }
    };
    let settled = outcome
        .wait_for(Option::is_some)
        .await
        .map_err(|_| TypeProviderError::new(state.establisher.restarting_error()))?
        .clone();
    match settled {
        Some(Ok(epoch)) => Ok(epoch),
        Some(Err(message)) => Err(TypeProviderError::new(message)),
        None => Err(TypeProviderError::new(state.establisher.restarting_error())),
    }
}

/// Returns the phase to idle if an establishment task ends without settling
/// (it panicked), so the instance is never left joined to a dead flight.
struct AbandonedFlight<'a, P: ?Sized> {
    shared: &'a Shared<P>,
    armed: bool,
}

impl<P: ?Sized> Drop for AbandonedFlight<'_, P> {
    fn drop(&mut self) {
        if self.armed {
            let mut lifecycle = self.shared.lifecycle();
            lifecycle.phase = Phase::Idle;
            lifecycle.last_failure =
                Some((Instant::now(), "establishment was abandoned".to_string()));
        }
    }
}

/// An incarnation the actor installed.
struct Installed {
    epoch: ProviderEpoch,
    pid: Option<u32>,
    crash_signal: Arc<Notify>,
}

impl Installed {
    /// Arm the incarnation's crash monitor. Called only once the lifecycle
    /// phase is idle again, so a death signalled the instant the engine went
    /// live is recovered rather than mistaken for a concurrent transition.
    fn arm_crash_monitor<P>(&self, state: &Arc<HubState<P>>, teardown_generation: u64)
    where
        P: TypeProvider + ?Sized + Send + Sync + 'static,
    {
        spawn_crash_monitor(
            Arc::downgrade(state),
            self.epoch,
            Arc::clone(&self.crash_signal),
            teardown_generation,
        );
    }
}

/// Spawn one engine and install it through the actor.
async fn establish_once<P>(
    state: &Arc<HubState<P>>,
    teardown_generation: u64,
) -> Result<Installed, TypeProviderError>
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    let crash_signal = Arc::new(Notify::new());
    let timeout = state.policy.establish_timeout;
    let provider = tokio::time::timeout(
        timeout,
        state.establisher.establish(Arc::clone(&crash_signal)),
    )
    .await
    .map_err(|_| {
        TypeProviderError::new(format!(
            "{} establishment exceeded its {timeout:?} bound",
            state.establisher.log_name()
        ))
    })??;
    let pid = provider.child_pid();
    let (ack, ack_rx) = oneshot::channel();
    if let Err(mpsc::error::SendError(command)) = state.commands.send(Command::Install {
        provider,
        crash_signal: Arc::clone(&crash_signal),
        teardown_generation,
        replay_timeout: timeout,
        ack,
    }) {
        if let Command::Install { provider, .. } = command {
            let _ = provider.shutdown().await;
        }
        return Err(TypeProviderError::new(state.establisher.restarting_error()));
    }
    let epoch = ack_rx
        .await
        .map_err(|_| TypeProviderError::new(state.establisher.restarting_error()))??;
    Ok(Installed {
        epoch,
        pid,
        crash_signal,
    })
}

fn spawn_crash_monitor<P>(
    state: Weak<HubState<P>>,
    epoch: ProviderEpoch,
    crash_signal: Arc<Notify>,
    teardown_generation: u64,
) where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    tokio::spawn(async move {
        crash_signal.notified().await;
        let Some(state) = state.upgrade() else {
            return;
        };
        recover(state, epoch, teardown_generation).await;
    });
}

/// Retire the crashed `retired` epoch and re-establish within the budget.
async fn recover<P>(state: Arc<HubState<P>>, retired: ProviderEpoch, teardown_generation: u64)
where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    let log_name = state.establisher.log_name();
    let retire_only = state.policy.retire_only_on_crash;
    {
        let mut lifecycle = state.shared.lifecycle();
        // TEARDOWN DISCRIMINATION: a deliberate shutdown produces the same
        // crash-signal wake as a real death (the torn-down child's EOF). That
        // is the requested teardown — never a crash to report or respawn.
        if lifecycle.teardown_generation != teardown_generation {
            tracing::debug!("{log_name} exit during deliberate teardown — not a crash");
            return;
        }
        if state.shared.serving_epoch() != Some(retired) {
            tracing::debug!(
                "{log_name} crash signal for retired epoch {} is inert",
                retired.0
            );
            return;
        }
        if !matches!(lifecycle.phase, Phase::Idle) {
            return;
        }
        if retire_only {
            // An externally-attached transport never respawns on a timer:
            // retire the epoch fail-closed and hold the re-arm at the
            // discriminant the dead incarnation established at. The next
            // demand re-establishes only through a FRESH discriminant (a
            // reconnect); within the same one every query fails closed to
            // its baseline — including an unobservable discriminant, which
            // gates exactly like an observable one. The instance is never
            // declared exhausted.
            lifecycle.attach_gate = Some(lifecycle.established_discriminant.clone());
            lifecycle.phase = Phase::Idle;
        } else {
            lifecycle.phase = Phase::Recovering;
        }
    }
    tracing::warn!("{log_name} crash detected - initiating restart sequence");

    // CRASH QUARANTINE: strike the requests in flight at (or erroring moments
    // before) the death; a repeat offender is never replayed again.
    state
        .shared
        .query_watch
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .record_crash_implications();

    let (ack, ack_rx) = oneshot::channel();
    if state
        .commands
        .send(Command::Retire {
            epoch: retired,
            ack,
        })
        .is_err()
    {
        return;
    }
    let _ = ack_rx.await;

    if retire_only {
        tracing::warn!(
            "{log_name} attach died — epoch {retired:?} retired; re-establishment \
             waits for a fresh generation discriminant"
        );
        return;
    }

    state.notifier.notify(
        NotifySeverity::Warning,
        format!(
            "TypeScript server ({}) crashed. Restarting...",
            state.establisher.user_label()
        ),
    );

    // A transient spawn/replay failure must not leave the instance dead for the
    // session: retry within the SAME budget and backoff as crash events, so a
    // persistently failing backend still fails closed — bounded, never a hot
    // respawn loop.
    let max_restarts = state.policy.max_restarts;
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        if state.shared.teardown_generation() != teardown_generation {
            tracing::debug!("{log_name} teardown during restart sequence — abandoning respawn");
            state.shared.lifecycle().phase = Phase::Idle;
            return;
        }
        if attempt > max_restarts {
            tracing::error!(
                "{log_name} restart limit reached ({max_restarts}) - staying in verter-only mode"
            );
            state.shared.lifecycle().phase = Phase::Exhausted;
            state.notifier.notify(
                NotifySeverity::Error,
                format!(
                    "TypeScript server ({}) crashed {max_restarts} times. Running in verter-only mode for the rest of this session.",
                    state.establisher.user_label()
                ),
            );
            return;
        }

        let delay_secs = (1u64 << (attempt - 1)).min(4);
        tracing::info!("{log_name} restart attempt {attempt}/{max_restarts} after {delay_secs}s");
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;

        // A shutdown that completed while the backoff slept (or raced the
        // spawn) abandons the respawn BEFORE any process is spawned: recovery
        // must never raise an engine — or a user-visible failure — after
        // `shutdown().await` has returned.
        if state.shared.teardown_generation() != teardown_generation {
            tracing::debug!("{log_name} teardown during restart backoff — abandoning respawn");
            state.shared.lifecycle().phase = Phase::Idle;
            return;
        }

        match establish_once(&state, teardown_generation).await {
            Ok(installed) => {
                state.shared.lifecycle().phase = Phase::Idle;
                installed.arm_crash_monitor(&state, teardown_generation);
                tracing::info!(
                    "{log_name} restarted successfully (attempt {attempt}, epoch {})",
                    installed.epoch.0
                );
                state.notifier.notify(
                    NotifySeverity::Info,
                    "TypeScript server restarted successfully.".to_string(),
                );
                state
                    .notifier
                    .provider_started(installed.pid, EngineStart::Recovery);
                return;
            }
            Err(error) => {
                tracing::error!("Failed to restart {log_name} (attempt {attempt}): {error}");
                if state.shared.teardown_generation() != teardown_generation {
                    // Teardown raced the attempt (its install was rejected and
                    // its engine torn down): report nothing, abandon quietly.
                    state.shared.lifecycle().phase = Phase::Idle;
                    return;
                }
                state.notifier.notify(
                    NotifySeverity::Error,
                    format!("Failed to restart TypeScript server: {error}"),
                );
            }
        }
    }
}

/// What interrupted an actor wait on an engine.
enum Interrupt {
    /// The awaited engine's epoch was retired (crash).
    Retired(oneshot::Sender<()>),
    /// Deliberate teardown.
    Shutdown(oneshot::Sender<()>),
}

/// Await `work` while staying receptive to control: a retirement of
/// `awaited` (when it names an installed epoch) or a shutdown interrupts it;
/// every other command is queued in order behind it.
async fn await_receptive<P, T>(
    work: impl Future<Output = T>,
    awaited: Option<ProviderEpoch>,
    command_rx: &mut mpsc::UnboundedReceiver<Command<P>>,
    queued: &mut std::collections::VecDeque<Command<P>>,
) -> Result<T, Interrupt>
where
    P: ?Sized,
{
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return Ok(output),
            Some(command) = command_rx.recv() => match command {
                Command::Retire { epoch, ack } if Some(epoch) == awaited => {
                    return Err(Interrupt::Retired(ack));
                }
                // A retirement naming any other epoch is inert.
                Command::Retire { ack, .. } => {
                    let _ = ack.send(());
                }
                Command::Shutdown { ack } => return Err(Interrupt::Shutdown(ack)),
                command => queued.push_back(command),
            },
        }
    }
}

/// The single-writer actor loop. Owns the desired state (task-local) and is
/// the sole writer of the serving cell.
async fn run_actor<P>(
    mut command_rx: mpsc::UnboundedReceiver<Command<P>>,
    shared: Arc<Shared<P>>,
    notifier: Arc<dyn ProviderNotifier>,
    log_name: &'static str,
    demand_driven: bool,
) where
    P: TypeProvider + ?Sized + Send + Sync + 'static,
{
    let mut desired = DesiredState::default();
    let mut queued = std::collections::VecDeque::new();

    loop {
        let command = match queued.pop_front() {
            Some(command) => command,
            None => match command_rx.recv().await {
                Some(command) => command,
                None => return,
            },
        };
        match command {
            Command::ApplyOverlay {
                mutation,
                admissions,
                lane,
                deadline,
                ack,
            } => {
                let current = || {
                    admissions
                        .iter()
                        .try_for_each(|admission| admission::check_current(&shared, admission))
                };
                let result = if deadline.is_some_and(|at| tokio::time::Instant::now() >= at) {
                    Err(AdmissionRefusal::DeadlineElapsed)
                } else {
                    match current() {
                        Err(reason) => Err(reason),
                        Ok(()) => {
                            let serving = shared
                                .serving()
                                .expect("admission checked serving provider");
                            let disposition = desired.disposition(&mutation);
                            if disposition == Disposition::Shadowed {
                                Err(AdmissionRefusal::ShadowedMutation)
                            } else {
                                let forwarding = async {
                                    let forwarded = desired::forward(
                                        serving.provider.as_ref(),
                                        &mutation,
                                        lane,
                                    );
                                    match deadline {
                                        Some(at) => {
                                            crate::deadline::with_deadline_at(at, forwarded).await
                                        }
                                        None => forwarded.await,
                                    }
                                };
                                match await_receptive(
                                    forwarding,
                                    Some(serving.epoch),
                                    &mut command_rx,
                                    &mut queued,
                                )
                                .await
                                {
                                    Ok(Ok(())) => {
                                        match current() {
                                            Ok(()) => {
                                                desired.apply(&mutation, lane);
                                                desired.record_admitted(&mutation, &admissions);
                                                let mut watch =
                                                    shared.query_watch.lock().unwrap_or_else(
                                                        |poisoned| poisoned.into_inner(),
                                                    );
                                                for path in mutation.touched_paths() {
                                                    watch.clear_path(&path);
                                                }
                                                Ok(AppliedReceipt {
                                                    epoch: Some(serving.epoch),
                                                })
                                            }
                                            Err(reason) => {
                                                // A publication raced the forward. The engine
                                                // may now contain an unadmitted unit, so it cannot
                                                // serve or replay that state into another epoch.
                                                if demand_driven {
                                                    retire(&shared, serving.epoch).await;
                                                } else {
                                                    serving.crash_signal.notify_one();
                                                }
                                                Err(reason)
                                            }
                                        }
                                    }
                                    Ok(Err(_)) => {
                                        // A partial provider write cannot be allowed to serve.
                                        if demand_driven {
                                            retire(&shared, serving.epoch).await;
                                        } else {
                                            serving.crash_signal.notify_one();
                                        }
                                        Err(AdmissionRefusal::ProviderWriteFailed)
                                    }
                                    Err(Interrupt::Retired(retired)) => {
                                        retire(&shared, serving.epoch).await;
                                        let _ = retired.send(());
                                        Err(AdmissionRefusal::StaleProvider)
                                    }
                                    Err(Interrupt::Shutdown(done)) => {
                                        shutdown_serving(&shared).await;
                                        let _ = done.send(());
                                        Err(AdmissionRefusal::StaleProvider)
                                    }
                                }
                            }
                        }
                    }
                };
                let _ = ack.send(result);
            }
            Command::Mutate {
                mutation,
                lane,
                deadline,
                ack,
            } => {
                let touched = mutation.touched_paths();
                let disposition = desired.apply(&mutation, lane);
                // The content change is RECORDED: lift the touched paths' crash
                // attribution here, on the actor — the mutation applies in
                // order regardless of whether its submitter was still waiting
                // for the settlement, so a submitter deadline can never leave
                // stale quarantine behind.
                {
                    let mut watch = shared
                        .query_watch
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    for path in &touched {
                        watch.clear_path(path);
                    }
                }
                let result = match (shared.serving(), disposition) {
                    (None, _) => Ok(AppliedReceipt { epoch: None }),
                    (Some(serving), Disposition::Shadowed) => Ok(AppliedReceipt {
                        epoch: Some(serving.epoch),
                    }),
                    (Some(serving), Disposition::Forward) => {
                        let forwarding = async {
                            let forwarded =
                                desired::forward(serving.provider.as_ref(), &mutation, lane);
                            match deadline {
                                Some(at) => crate::deadline::with_deadline_at(at, forwarded).await,
                                None => forwarded.await,
                            }
                        };
                        match await_receptive(
                            forwarding,
                            Some(serving.epoch),
                            &mut command_rx,
                            &mut queued,
                        )
                        .await
                        {
                            Ok(Ok(())) => Ok(AppliedReceipt {
                                epoch: Some(serving.epoch),
                            }),
                            Ok(Err(error)) => {
                                // A failed forward is DIVERGENCE: the mutation
                                // is recorded in the desired state but this
                                // engine never accepted it, so the epoch must
                                // not keep serving content the recorded state
                                // no longer describes. Reconcile before any
                                // query serves again — the desired state
                                // (rejected mutation included) replays into
                                // the next engine — while the submitter keeps
                                // the forward's own error.
                                if demand_driven {
                                    // An on-demand instance never respawns on
                                    // its own: retire fail-closed now; the next
                                    // query demand establishes a fresh engine
                                    // and replays the desired state into it.
                                    retire(&shared, serving.epoch).await;
                                } else {
                                    // An explicit instance reconciles through
                                    // its armed crash monitor — the same
                                    // bounded recover() path as a crash: retire
                                    // the epoch, respawn within the budget,
                                    // replay before install.
                                    serving.crash_signal.notify_one();
                                }
                                Err(error)
                            }
                            // The forward is dropped; its desired state and
                            // every queued mutation survive for the next engine.
                            Err(Interrupt::Retired(retired)) => {
                                retire(&shared, serving.epoch).await;
                                let _ = retired.send(());
                                Err(TypeProviderError::new(
                                    "provider crashed during state update",
                                ))
                            }
                            Err(Interrupt::Shutdown(done)) => {
                                shutdown_serving(&shared).await;
                                let _ = done.send(());
                                Err(TypeProviderError::new(
                                    "provider shut down during state update",
                                ))
                            }
                        }
                    }
                };
                let _ = ack.send(result);
            }
            Command::Retire { epoch, ack } => {
                retire(&shared, epoch).await;
                let _ = ack.send(());
            }
            Command::Shutdown { ack } => {
                shutdown_serving(&shared).await;
                let _ = ack.send(());
            }
            Command::Install {
                provider,
                crash_signal,
                teardown_generation,
                replay_timeout,
                ack,
            } => {
                if shared.teardown_generation() != teardown_generation {
                    let _ = provider.shutdown().await;
                    let _ = ack.send(Err(TypeProviderError::new(format!(
                        "{log_name} was shut down while it was being established"
                    ))));
                    continue;
                }
                // Replay the CURRENT desired state (every command processed
                // before this one), then install. No caller observes the engine
                // before its replay completes.
                let replay = tokio::time::timeout(replay_timeout, desired.replay_into(&*provider));
                let outcome =
                    match await_receptive(replay, None, &mut command_rx, &mut queued).await {
                        Ok(Ok(Ok(()))) => Ok(()),
                        Ok(Ok(Err(error))) => Err(error),
                        Ok(Err(_)) => Err(TypeProviderError::new(format!(
                            "{log_name} replay exceeded its {replay_timeout:?} bound"
                        ))),
                        Err(Interrupt::Shutdown(done)) => {
                            let _ = provider.shutdown().await;
                            shutdown_serving(&shared).await;
                            let _ = done.send(());
                            let _ = ack.send(Err(TypeProviderError::new(format!(
                                "{log_name} was shut down while it was being established"
                            ))));
                            continue;
                        }
                        Err(Interrupt::Retired(retired)) => {
                            // `await_receptive` interrupts on retirement only for
                            // the awaited epoch, and a replay awaits none.
                            let _ = retired.send(());
                            Err(TypeProviderError::new(format!(
                                "{log_name} replay was interrupted"
                            )))
                        }
                    };
                match outcome {
                    Ok(()) => {
                        let dropped = desired.discard_admitted();
                        let epoch = shared.epochs.mint();
                        // Record the installed engine's tier BEFORE releasing
                        // it into the serving cell: from that instant
                        // `provider_id()` must name it even after it retires —
                        // it is the tier that minted the completion envelopes
                        // still in flight.
                        let provider_id = provider.provider_id();
                        let previous = shared
                            .serving
                            .write()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .replace(Serving {
                                provider,
                                epoch,
                                crash_signal,
                            });
                        *shared
                            .last_serving_id
                            .write()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(provider_id);
                        if let Some(previous) = previous {
                            // Never two live incarnations: a replaced engine is
                            // torn down, not orphaned, and its monitor released.
                            let _ = previous.provider.shutdown().await;
                            previous.crash_signal.notify_one();
                        }
                        // A replacement dropped the OLD epoch's admitted state
                        // (a first install never has any). Announce it AFTER the
                        // serving cell is filled so a re-arm binds its fresh
                        // admission to the epoch that now serves.
                        if !dropped.carriers.is_empty() || !dropped.files.is_empty() {
                            notifier.admitted_state_dropped(&dropped);
                        }
                        let _ = ack.send(Ok(epoch));
                    }
                    Err(error) => {
                        // An engine that did not accept the complete desired
                        // state is never exposed; tear it down, never orphan it.
                        let _ = provider.shutdown().await;
                        let _ = ack.send(Err(TypeProviderError::new(format!(
                            "{log_name} desired-state replay failed: {error}"
                        ))));
                    }
                }
            }
        }
    }
}

/// Retire the engine serving `epoch`: fail queries closed first, then tear the
/// failed child down without holding the serving lock.
async fn retire<P>(shared: &Shared<P>, epoch: ProviderEpoch)
where
    P: TypeProvider + ?Sized,
{
    let retired = {
        let mut serving = shared
            .serving
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match serving.as_ref() {
            Some(current) if current.epoch == epoch => serving.take(),
            _ => None,
        }
    };
    if let Some(retired) = retired {
        let _ = retired.provider.shutdown().await;
    }
}

async fn shutdown_serving<P>(shared: &Shared<P>)
where
    P: TypeProvider + ?Sized,
{
    let serving = shared
        .serving
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(serving) = serving {
        let _ = serving.provider.shutdown().await;
        // Release the incarnation's crash monitor; it observes the teardown.
        serving.crash_signal.notify_one();
    }
}

/// Whether the observed generation discriminant ADVANCED past the gated one —
/// the re-arm signal of [`ProviderHub::establish_rearming`].
///
/// Re-arm ONLY when the current discriminant is `Some(new)` that differs from
/// the gate. A missing current discriminant (`None` — none observable) does
/// NOT re-arm, so a flapping / absent advertisement never storms
/// establishment.
fn generation_advanced(gated: &Option<String>, current: &Option<String>) -> bool {
    match current {
        Some(now) => Some(now) != gated.as_ref(),
        None => false,
    }
}

#[cfg(test)]
#[path = "hub_tests.rs"]
pub(crate) mod hub_tests;
