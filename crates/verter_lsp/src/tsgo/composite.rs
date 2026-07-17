//! Host-aware tsgo admission and shared-first serving order.
//!
//! [`TsgoCompositeProvider`] is the production tsgo [`TypeProvider`] shape. It holds
//! a managed provider (eager for an explicit managed mode, statefully lazy for an editor
//! integration), the live host/project authority, and an optional [`SharedTsgoOverlay`].
//!
//! **Project-bound gate.** A carrier companion resolves its owning configured project
//! once through the shared
//! [`project_binding`](crate::tsgo::project_binding) helper — published snapshot →
//! [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
//! → [`ProjectBinding`](verter_session::external_ts::ProjectBinding) →
//! [`BoundProject`](verter_session::external_ts::BoundProject) witness. Non-bound states
//! fail closed to the feature's empty external answer; they never reach an engine's
//! inferred-project self-discovery. Feature admissions are generation-scoped through
//! [`CarrierAdmissionCache`].
//!
//! **Serving order.** For a bound carrier, an armed editor rendezvous is established,
//! synchronized, and live-revalidated first. Diagnostics and every read-only feature use
//! that exact editor-owned Program. Only an observed attach/sync/decision failure or the
//! bounded shared deadline admits the managed provider. With
//! [`crate::type_provider::lazy_managed::LazyManagedTypeProvider`] this means a successful
//! shared session never creates or queries a duplicate semantic engine. Diagnostics union
//! the attached `--api` semantic channel with the strict LSP pull channel from that same
//! process ([`compose_diagnostics`]); the managed provider is not part of that union.
//!
//! Lifecycle/configuration calls still flow to the managed slot so a lazy fallback can
//! cache the latest desired state without spawning; the shared overlay records carrier
//! content independently and injects it only when a bound demand engages.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use verter_session::external_ts::{
    AmbiguityCause, CarrierOwnershipResolution, ProjectBinding, ServeMode,
};
use verter_session::framework::descriptor::classify_carrier_companion;
use verter_session::VerterHost;
use verter_workspace::resolver::normalize_canonical_id;
use verter_workspace::traits::WorkspaceRead;

use verter_tsgo_api::control::Advertisement;
use verter_type_runtime::protocol::{
    Completion, CompletionResolveData, CompletionResolveResult, CompletionResult, HoverInfo,
    InlayHint, ProviderDiagnosticContext, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight, TypeLocation, TypeProviderError,
};
use verter_type_runtime::traits::{ProviderFuture, TypeProvider};

use crate::tsgo::overlay_core::{LazyOverlayCore, OverlayTransport};
use crate::tsgo::project_binding::{self, BoundCarrier, CarrierAdmissionCache};
use crate::tsgo::shared::{EstablishSharedParams, TsgoSharedProvider};
use crate::tsgo::transport_cell::EstablishedTransport;

/// The bound on the lazy SHARED-attach establishment: a slow or never-initializing
/// editor tsgo cannot stall a carrier diagnostics query beyond this — on elapse the
/// overlay yields no SHARED result and the composite admits managed fallback
/// (fail-closed). Concurrent queries during establishment reuse the one bounded
/// attempt (singleflight); a failed attempt re-arms on a fresh advertisement/editor
/// generation OR a fresh workspace/config generation (see
/// [`LazyTransport`](crate::tsgo::transport_cell::LazyTransport)). Establishment is
/// reached only from a query path, never the managed lifecycle path — so opting into
/// SHARED never trips the managed
/// foreground-sync budget.
const SHARED_ESTABLISH_TIMEOUT: Duration = Duration::from_secs(15);

/// The OUTER production deadline bounding the ENTIRE SHARED overlay contribution to a
/// single diagnostics query — establishment + whole-dirty-set injection + the per-query
/// control re-decision + both diagnostic channels, as one unit. On elapse the composite
/// admits the managed fallback. Every shared sub-operation is awaited inside this bound,
/// so no relay/control/`--api` stall can escape it. It exceeds
/// [`SHARED_ESTABLISH_TIMEOUT`] so a legitimate first establishment reaches its own
/// singleflight decision.
const SHARED_OVERLAY_TIMEOUT: Duration = Duration::from_secs(20);

/// The bound on a SHARED carrier retract issued from `close_file` lifecycle:
/// a slow or never-answering relay close cannot hang or delay the composite close beyond
/// this — on elapse the retract is abandoned (fail-closed) and the composite close
/// returns promptly. The retract is best-effort and the transport is torn down / evicted
/// on a broken connection anyway, so a dropped retract only leaves a soon-cleaned
/// lingering document, never a wrong result. Symmetric with the open/change lifecycle,
/// which only records content off the managed critical path.
const SHARED_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// The client label the SHARED overlay presents on the control hello.
const SHARED_CLIENT_LABEL: &str = "verter_lsp";

/// The rendezvous evidence a SHARED editor-attach is established from: the control
/// directory the editor's `verter-relay-shim` advertised into, the session key it
/// published under, and the workspace root (the editor-binding witness base).
#[derive(Debug, Clone)]
pub struct SharedRendezvous {
    /// The rendezvous control directory the editor's shim advertised into.
    pub control_dir: PathBuf,
    /// The `--session-key` the shim published under.
    pub session_key: String,
    /// The workspace root the editor bound the carrier to.
    pub workspace_root: String,
}

/// The SHARED carrier-diagnostics overlay: resolves the queried carrier's owning
/// project per query over the host's live published snapshot and, only for a
/// resolved binding, serves the SHARED `--api` carrier diagnostics through the
/// lazily-established relay-attach transport.
///
/// Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct SharedTsgoOverlay {
    inner: Arc<OverlayInner>,
}

struct OverlayInner {
    /// The host — the live published-snapshot + per-project R21 env-dims authority
    /// the per-query binding resolution reads from.
    host: Arc<VerterHost>,
    /// The rendezvous evidence the transport is lazily established from.
    rendezvous: SharedRendezvous,
    /// The lazy overlay core: the per-carrier content cache lifecycle records
    /// into OFF the critical path, plus the lazily-established relay-attach transport
    /// cell the QUERY path establishes + injects into. The transport is a singleflight,
    /// bounded, re-arming, liveness-evicting cell (established once on the first bound
    /// carrier DIAGNOSTICS query; reused after). The STATE lock is never held across the
    /// establishment I/O, a slow/broken attach is bounded by [`SHARED_ESTABLISH_TIMEOUT`]
    /// (fail-closed to managed), and a failed attach re-arms on a fresh advertisement/editor
    /// OR workspace/config generation (never poisoned by a carrier's transient
    /// non-binding). The observed engine version — the witness the per-query
    /// `BoundProject` mint and the `--api` snapshot rail key on — is read FROM the
    /// established transport (the attach version gate), never a hardcoded literal.
    core: LazyOverlayCore<TsgoSharedProvider>,
}

impl SharedTsgoOverlay {
    /// Build the overlay over the host and the rendezvous evidence. The transport is
    /// established lazily on the first bound carrier DIAGNOSTICS query (never the
    /// lifecycle path); the observed engine version is taken from the attach gate at
    /// that point.
    #[must_use]
    pub fn new(host: Arc<VerterHost>, rendezvous: SharedRendezvous) -> Self {
        Self {
            inner: Arc::new(OverlayInner {
                host,
                rendezvous,
                core: LazyOverlayCore::new(),
            }),
        }
    }

    /// Record the carrier's current content for the SHARED overlay — a cheap in-memory
    /// insert off the managed lifecycle critical path. It never establishes the SHARED
    /// transport, so opting into SHARED cannot trip the managed file-lifecycle timing
    /// (the foreground TSX sync is budgeted far below the SHARED establishment bound).
    /// The query path ([`Self::engage_diagnostics`]) establishes the transport and
    /// injects the recorded content lazily. A non-carrier path is ignored.
    fn record_content(&self, provider_path: &str, content: &str) {
        if carrier_source_of(provider_path).is_none() {
            return;
        }
        self.inner.core.record_content(provider_path, content);
    }

    /// Retract a carrier overlay off the managed `close_file` critical path — drop its
    /// recorded content and, if the SHARED transport is already established, issue the
    /// retract bounded + fail-closed (a slow/dead relay cannot hang or delay the managed
    /// close; the transport is torn down / evicted anyway). The retract never triggers —
    /// or head-of-line-blocks on — an establishment (the non-establishing `current`
    /// accessor), and routes through the transport's ordered per-carrier gate so it is
    /// correctly ordered w.r.t. any in-flight injection.
    async fn feed_close(&self, provider_path: &str) {
        if carrier_source_of(provider_path).is_none() {
            return;
        }
        self.inner
            .core
            .retract_bounded(provider_path, SHARED_CLOSE_TIMEOUT)
            .await;
    }

    /// Establish, synchronize, and revalidate the exact editor-owned provider for a
    /// carrier already resolved to a configured project. `None` is an observed attach,
    /// synchronization, or live-decision failure and is the only condition that admits
    /// the managed fallback tier.
    ///
    /// The carrier binding is passed in PRE-RESOLVED (the composite gate resolved it
    /// ONCE via the shared [`project_binding`] helper): SHARED reuses the SAME binding
    /// (for its per-query re-decision + transport), the SAME generation (for the
    /// transport re-arm), and the SAME already-minted `BoundProject`
    /// (`carrier.bound().project()` — the version-independent owning tsconfig) for the
    /// `--api` overlay target. There is NO second resolution and NO witness re-mint.
    async fn engage_provider(
        &self,
        provider_path: &str,
        carrier: &BoundCarrier,
    ) -> Option<Arc<TsgoSharedProvider>> {
        // Lazily establish (once) the SHARED relay-attach transport for the
        // ALREADY-resolved binding — at QUERY time, off the managed lifecycle critical
        // path (SHARED is never fabricated; the binding is the gate's resolved one). The
        // identity-bound object is retained so injection is attributed to THIS transport
        // instance's epoch (never a re-read of the overlay's current active epoch).
        let established = self
            .ensure_transport(carrier.binding().clone(), carrier.generation())
            .await?;

        // Inject the recorded content of EVERY open carrier into the established
        // transport (dirty-tracked — only what changed since the last injection) so the
        // queried carrier's `--api` diagnostics see the current text AND its companion
        // family / imported carriers are members of the SHARED Program (else its imports
        // spuriously fail with TS2307) — the normal open→diagnostics flow, now that the
        // lifecycle only RECORDS content off-path. GATED on the shadow/conflict
        // authority: a recorded path that is NOT a genuine generated carrier surface
        // (e.g. a real user file occupying a carrier-companion path) is NEVER injected /
        // overlay-shadowed (`carrier_never_shadows_real_user_file`). Best-effort: a
        // failed inject admits the managed fallback.
        // The workspace content generation keys the per-carrier shadow-safety cache: a
        // content-clean carrier re-checks shadow-safety only when this advances (any
        // file-set/overlay transition bumps it), so a real user file appearing at a
        // companion path — or a same-stem rune module — is never overlay-shadowed by a
        // stale "safe" cache (`carrier_never_shadows_real_user_file`). It advances on the
        // file-existence surface the shadow-safety `file_exists` probes read, which the
        // snapshot/config generation (`carrier.generation()`) does NOT.
        let shadow_generation = self.inner.host.workspace_read().content_generation();
        self.inner
            .core
            .inject_all_dirty(&established, shadow_generation, |companion| {
                self.injection_is_shadow_safe(companion)
            })
            .await;

        // Admit managed when the queried carrier's current content is not
        // confirmed synced into the shared Program (its dirty injection failed) — never
        // serve SHARED diagnostics computed against stale/absent content (a prior synced
        // slot). Only a carrier whose current content is confirmed synced is served.
        if !self.inner.core.is_synced(provider_path) {
            return None;
        }

        // Re-decide the serve mode through the live controller at the resolved
        // snapshot/config generation, reusing the SAME binding — a not-SHARED decision
        // admits managed.
        if established
            .transport
            .redecide_for_binding(carrier.binding(), carrier.generation())
            .mode()
            != ServeMode::Shared
        {
            return None;
        }

        Some(established.transport)
    }

    /// Full user-facing diagnostics from the exact editor-owned Program. The `--api`
    /// query is the configured-project membership proof; after it succeeds, the relay's
    /// strict pull-diagnostic request supplies the complete LSP surface (syntactic,
    /// suggestion, tags/related information, and semantic diagnostics). Either channel
    /// failing returns `None`, admitting the managed fallback instead of presenting a
    /// fabricated empty result.
    async fn engage_diagnostics(
        &self,
        provider_path: &str,
        carrier: &BoundCarrier,
    ) -> Option<Vec<TypeDiagnostic>> {
        let provider = self.engage_provider(provider_path, carrier).await?;
        let semantic = provider
            .overlay_diagnostics_in_project(provider_path, carrier.bound().project())
            .await
            .ok()??;
        let full = provider
            .full_diagnostics_for_carrier(provider_path)
            .await
            .ok()?;
        Some(compose_diagnostics(semantic, full))
    }

    /// Whether injecting the recorded `companion_path` overlay is shadow-safe — i.e. no
    /// REAL user file is displaced. TWO independent gates, either of which fails closed:
    ///
    /// 1. **Disk-occupancy at the EXACT injected path (defense-in-depth).** The injected
    ///    companion paths — IDE (`Foo.vue.tsx` / `.jsx` / `Foo.svelte.tsx`), DECLARATION
    ///    (`Foo.d.vue.ts` / `Foo.d.svelte.ts`), API (`Foo.vue.verter.ts`), testing-API,
    ///    sidecar, and any other companion [`carrier_source_of`] admits — all live in the
    ///    USER namespace. A REAL user file at the exact path SHARED is about to inject
    ///    generated content at is a collision Verter must NEVER overlay-shadow
    ///    (`carrier_never_shadows_real_user_file`), for EVERY companion type. This exact-
    ///    path occupancy gate closes the whole class uniformly, independent of what source
    ///    [`carrier_source_of`] derives AND of what gate (2)'s conflict pass enumerated, so
    ///    a stale VFS snapshot, a future companion form, or a direct overlay call can never
    ///    slip a shadow past ([`real_file_occupies_injected_path`]).
    /// 2. **Source-resolution shadow-safety.** For a genuine virtual companion (no real
    ///    file at its path), the source is resolved and its shadow-cause honoured. The
    ///    resolver's UNCONDITIONAL carrier-path-conflict pass (`carrier_path_conflict` over
    ///    `carrier_companion_identities_for_source`) enumerates EVERY descriptor-owned
    ///    companion family — IDE, declaration, import-surface API, testing-API, sidecar —
    ///    so a REAL user file at ANY of those companion paths (not just the IDE companion)
    ///    downgrades the source to `Ambiguous(CarrierPathOccupiedByRealFile)`; a same-stem
    ///    rune module beside the source downgrades it to `Ambiguous(SameStemRuneModule)`.
    ///    Either downgrade means skipped and managed serves. A genuine generated companion (a
    ///    clean binding, `NoProject`, `NotReady`, or a MultipleOwners ambiguity,
    ///    none of which sit a REAL file at a companion path) is safe to inject as a
    ///    supporting Program member.
    ///
    /// A not-a-companion path or a not-yet-ready snapshot is conservatively NOT injected.
    fn injection_is_shadow_safe(&self, companion_path: &str) -> bool {
        // (1) Disk-occupancy at the EXACT injected path — the defense-in-depth gate that
        //     covers every companion type (IDE / declaration / API / testing / sidecar)
        //     uniformly at the injected path.
        let ws_read = self.inner.host.workspace_read();
        if real_file_occupies_injected_path(ws_read.as_ref(), companion_path) {
            return false;
        }
        // (2) Source-resolution shadow-safety: the source's descriptor carrier-companion
        //     conflict (across EVERY companion family) or a same-stem rune module beside
        //     it, for a genuine virtual companion. The empty `ts_version` is safe — the
        //     shadow-safety decision is version-independent (it reads the resolution KIND,
        //     not the binding).
        let Some(source) = carrier_source_of(companion_path) else {
            return false;
        };
        match project_binding::resolve_carrier(
            self.inner.host.as_ref(),
            &source,
            Arc::from(""),
            project_binding::OwnershipReadinessMode::PresentSnapshotAuthoritative,
        ) {
            Some((resolution, _)) => injection_shadow_safe(&resolution),
            None => false,
        }
    }

    /// Lazily establish (once) the SHARED relay-attach transport for the carrier's
    /// ALREADY-resolved `binding` (resolved once by the composite gate at
    /// `generation`), through the singleflight + bounded + re-arming [`LazyTransport`]
    /// cell. Only a bound carrier reaches here (the gate resolved the binding before
    /// calling [`Self::engage_diagnostics`]), so the cell is never entered — nor its
    /// `Unavailable` slot poisoned — by a transient non-binding
    /// ([`LazyTransport::get_or_establish_bound`]). Concurrent queries reuse the ONE
    /// in-flight establishment; a slow/broken attach is bounded by
    /// [`SHARED_ESTABLISH_TIMEOUT`] (then managed is admitted, never a stall); a failed
    /// ATTACH re-arms on a fresh advertisement/editor generation OR a fresh
    /// workspace/config generation.
    ///
    /// The re-arm discriminant is BOTH the shim advertisement nonce (a cheap FS read
    /// — a reconnect republishes a fresh advertisement with a new nonce) AND the
    /// workspace/config generation the binding resolved at (a fresh published snapshot
    /// advances it): a prior failed establishment re-attempts on the new nonce OR the
    /// new generation, while within one (nonce, generation) a failure does not retry
    /// per query (no handshake retry-storm).
    async fn ensure_transport(
        &self,
        binding: ProjectBinding,
        generation: u64,
    ) -> Option<EstablishedTransport<TsgoSharedProvider>> {
        // The binding is pre-resolved (bound) — pass it straight to the cell. The core
        // supplies the live-death eviction predicate; a no-binding carrier never
        // reaches here, so the cell is never poisoned by a transient non-binding. The
        // identity-bound object is returned so the injection path attributes work to the
        // exact transport instance's epoch.
        self.inner
            .core
            .ensure(
                Some((binding, generation)),
                |generation| self.probe_establishment_discriminant(generation),
                |binding, generation| self.establish_transport(binding, generation),
                SHARED_ESTABLISH_TIMEOUT,
            )
            .await
    }

    /// The re-arm discriminant for a failed SHARED establishment at config
    /// `generation`: BOTH the shim advertisement nonce (a cheap FS read) AND the
    /// workspace/config generation, composed by [`compose_establishment_discriminant`].
    /// `None` when no advertisement is observable (a flapping / absent shim never
    /// storms establishment — [`LazyTransport::get_or_establish`]'s missing-generation
    /// rule then holds the fail-closed state).
    fn probe_establishment_discriminant(&self, generation: u64) -> Option<String> {
        let nonce = Advertisement::find_for_session_key(
            &self.inner.rendezvous.control_dir,
            &self.inner.rendezvous.session_key,
        )
        .ok()
        .map(|(_, adv)| adv.nonce)?;
        Some(compose_establishment_discriminant(&nonce, generation))
    }

    /// Run the SHARED attach establishment ONCE for the PRE-RESOLVED carrier
    /// `binding` (resolved at config `generation` BEFORE the cell was entered) — the
    /// body the [`LazyTransport`] cell drives under its singleflight + bounded
    /// timeout. The bootstrap `ts_version` the binding was resolved with is used ONLY
    /// to gate establishment (the witness + `--api` op key on the transport's
    /// gate-observed version downstream). Returns `None` for a failed / not-SHARED
    /// establishment (managed serves); a `None` here is an actual attach
    /// attempt outcome, so recording `Unavailable` is correct — the no-binding case
    /// is gated out before the cell and never reaches here.
    async fn establish_transport(
        &self,
        binding: ProjectBinding,
        generation: u64,
    ) -> Option<Arc<TsgoSharedProvider>> {
        let tsconfig_path = binding.tsconfig_uri().to_string();
        let params = EstablishSharedParams {
            control_dir: &self.inner.rendezvous.control_dir,
            session_key: &self.inner.rendezvous.session_key,
            workspace_root: &self.inner.rendezvous.workspace_root,
            tsconfig_path: &tsconfig_path,
            resolution: CarrierOwnershipResolution::Bound(binding),
            config_generation: generation,
            client_label: SHARED_CLIENT_LABEL,
        };
        match TsgoSharedProvider::establish_shared(params).await {
            Ok(transport) => Some(Arc::new(transport)),
            Err(e) => {
                tracing::info!(
                    "SHARED editor route not established ({e}); managed fallback is eligible"
                );
                None
            }
        }
    }
}

/// The carrier SOURCE (`Foo.vue`) a provider companion path projects from, or `None`
/// when `provider_path` is not a framework-carrier companion.
///
/// Routes through the descriptor companion-classification authority
/// ([`classify_carrier_companion`]): every companion family reverse-maps to its TRUE
/// carrier source — the IDE `Foo.vue.tsx` / `Foo.vue.jsx`, the extension-middle
/// declaration `Foo.d.vue.ts`, the `.verter.ts` import-surface API, and the
/// testing-API / sidecar surfaces. A declaration companion maps to `Foo.vue` (the
/// descriptor inverts the `.d.` infix), never the intermediate `Foo.d.vue` stem. A
/// plain `.ts`/`.tsx` file (no carrier stem) yields `None` (the managed provider serves
/// it). Backslash paths normalize to the same forward-slashed source.
fn carrier_source_of(provider_path: &str) -> Option<String> {
    classify_carrier_companion(provider_path).map(|companion| companion.source)
}

/// Whether a carrier companion whose SOURCE resolved to `resolution` is shadow-safe to
/// inject into the SHARED Program. A real user file occupying a descriptor
/// carrier-companion path (or a same-stem rune module beside the source) downgrades the
/// source to an `Ambiguous` real-file-shadow cause — Verter must NEVER overlay-shadow it
/// (`carrier_never_shadows_real_user_file`). Every other resolution (a clean binding,
/// `NoProject`, `NotReady`, or a MultipleOwners ambiguity — none of which sit a
/// REAL file at the companion path) leaves a GENUINE virtual companion safe to inject as
/// a supporting Program member. Typed over [`CarrierOwnershipResolution`] — never a path-shape or
/// substring check.
fn injection_shadow_safe(resolution: &CarrierOwnershipResolution) -> bool {
    !matches!(
        resolution,
        CarrierOwnershipResolution::Ambiguous {
            cause: AmbiguityCause::CarrierPathOccupiedByRealFile
                | AmbiguityCause::SameStemRuneModule,
            ..
        }
    )
}

/// Whether a REAL user file already occupies the EXACT path the SHARED overlay is about
/// to inject generated carrier content at. The injected companion paths — IDE
/// (`Foo.vue.tsx` / `.jsx` / `Foo.svelte.tsx`), DECLARATION (`Foo.d.vue.ts` /
/// `Foo.d.svelte.ts`), API (`Foo.vue.verter.ts`), and any other companion
/// [`carrier_source_of`] admits — all live in the USER namespace, so a real user file at
/// that exact path is a shadow collision Verter must NEVER overlay-shadow
/// (`carrier_never_shadows_real_user_file`).
///
/// This exact-path occupancy probe is an INDEPENDENT injection-boundary guard, NOT an
/// ownership classifier: it maps nothing back to a source (that is [`carrier_source_of`]'s
/// role, which reverse-maps every companion — including the extension-middle declaration
/// `Foo.d.vue.ts` -> the real `Foo.vue` — through the descriptor authority). It fails the
/// injection closed the instant a real file sits at the injected path, uniformly across
/// every companion type and independent of what the source-resolution conflict pass
/// enumerated — defense-in-depth so a stale VFS snapshot, a future companion form, or a
/// direct overlay call can never overlay-shadow a user file even if the source-side
/// conflict pass did not flag it. Probes the shared workspace/VFS authority
/// ([`WorkspaceRead::file_exists`] — the same disk-occupancy machinery the resolver's
/// carrier-path-conflict pass uses, never a private disk reimplementation) over the
/// NORMALIZED path, so a non-canonical (backslash / uppercase-drive) injected path cannot
/// evade the probe on a case-insensitive FS.
fn real_file_occupies_injected_path(ws: &dyn WorkspaceRead, injected_path: &str) -> bool {
    ws.file_exists(&normalize_canonical_id(injected_path))
}

/// Compose the SHARED-establishment re-arm discriminant from the shim advertisement
/// `nonce` and the workspace/config `generation`. A change to EITHER field yields a
/// distinct discriminant, so a failed establishment re-arms on a reconnect (fresh
/// nonce) OR a fresh published snapshot (fresh generation) — never nonce-only (the
/// transport-cell-poisoning fix's re-arm rail). The `\u{1f}` unit separator can never
/// appear in the hex nonce or the decimal generation, so the composition is injective.
fn compose_establishment_discriminant(nonce: &str, generation: u64) -> String {
    format!("{nonce}\u{1f}{generation}")
}

/// The dedup identity of a carrier diagnostic: its carrier byte span, code, and
/// message. Two diagnostics with the same `(start, end, code, message)` are the SAME
/// diagnostic — e.g. an identical carrier type error reported by both diagnostic
/// channels of the SAME editor-owned session.
type DiagnosticIdentity = (u32, u32, Option<String>, String);

fn diagnostic_identity(d: &TypeDiagnostic) -> DiagnosticIdentity {
    (d.start, d.end, d.code.clone(), d.message.clone())
}

/// Compose the two diagnostic channels of one editor-owned tsgo session.
///
/// The attached `--api` view proves configured-project membership and supplies semantic
/// diagnostics. The relayed LSP pull channel can additionally supply syntactic,
/// suggestion, tag, and related-information data. Both are views of the exact same
/// editor process and Program. The result is their deduplicated union; this never queries
/// or activates the managed fallback.
///
/// An identical diagnostic appears once. The `--api` copy's authoritative carrier span
/// is retained, while tags and related information found only on the LSP copy are merged
/// into it.
fn compose_diagnostics(
    semantic: Vec<TypeDiagnostic>,
    full: Vec<TypeDiagnostic>,
) -> Vec<TypeDiagnostic> {
    let mut merged = semantic;
    let mut index: HashMap<DiagnosticIdentity, usize> = merged
        .iter()
        .enumerate()
        .map(|(i, d)| (diagnostic_identity(d), i))
        .collect();
    for diag in full {
        match index.get(&diagnostic_identity(&diag)) {
            // Collision: preserve the `--api` span and union LSP metadata.
            Some(&i) => merge_diagnostic_metadata(&mut merged[i], diag),
            // LSP-only: append it (and index it so a later duplicate merges).
            None => {
                index.insert(diagnostic_identity(&diag), merged.len());
                merged.push(diag);
            }
        }
    }
    merged
}

/// Merge an LSP duplicate's metadata into the retained `--api` diagnostic on a
/// `(span, code, message)` collision: UNION the `tags` and `related_information`
/// (append each LSP entry the semantic copy does not already carry). The `--api` span,
/// severity, and message win (its authoritative mapping is retained); only the
/// metadata is unioned — never a silent LSP-metadata drop.
fn merge_diagnostic_metadata(into: &mut TypeDiagnostic, from: TypeDiagnostic) {
    for tag in from.tags {
        if !into.tags.contains(&tag) {
            into.tags.push(tag);
        }
    }
    for info in from.related_information {
        if !into.related_information.contains(&info) {
            into.related_information.push(info);
        }
    }
}

/// Every carrier TS FEATURE provider call the composite GATES on a resolved
/// `BoundProject` admission. Each variant maps 1:1 to exactly one gated `TypeProvider`
/// feature method on [`TsgoCompositeProvider`]; the enum is the EXHAUSTIVE registry of
/// gated features (no wildcard arm).
///
/// TWO distinct enforcement layers — do not conflate them:
/// * COMPILE-TIME: [`Self::name`]'s wildcard-free `match` makes only the
///   VARIANT→`name()` mapping exhaustive — a new variant must be named there or the
///   crate fails to compile. It does NOT tie variants to methods.
/// * BEHAVIOR-ENFORCED: the typed `owned_binding_gate` integration suite invokes every
///   feature group through [`TypeProvider`]. It proves that a denied carrier never calls
///   the managed provider, while bound carriers and plain TypeScript inputs delegate.
///   Runtime routing enters through [`TsgoCompositeProvider::feature_provider`], which
///   resolves the bound-carrier witness once and applies shared-first/fallback ordering.
///
/// Provenance class (informs the DENIED shape the HANDLER layer composes, NOT a
/// composite-runtime branch — every denied feature serves its own type's empty/none
/// external default):
/// * EXTERNAL-ONLY — no native sub-answer to merge; denied ⇒ empty/none.
/// * MIXED — the LSP handler merges a native sub-answer; denied ⇒ the external default
///   (empty/none) so the handler merge preserves the native side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderFeature {
    // ── EXTERNAL-ONLY (denied ⇒ empty/none; no owned call, no native sub-answer) ──
    /// `get_type_definition`.
    TypeDefinition,
    /// `get_signature_help`.
    SignatureHelp,
    /// `get_semantic_tokens`.
    SemanticTokens,
    // ── MIXED (denied ⇒ external default so the handler merge preserves the native) ──
    /// `get_hover`.
    Hover,
    /// `get_definition`.
    Definition,
    /// `get_references`.
    References,
    /// `get_document_highlights`.
    DocumentHighlights,
    /// `get_inlay_hints`.
    InlayHints,
    /// `get_completions` (MIXED).
    Completions,
    /// `get_completion_details` (MIXED).
    CompletionDetails,
    /// `resolve_completion` (EXTERNAL-ONLY — denied ⇒ no provider enrichment).
    ResolveCompletion,
    /// `get_rename_locations` (MIXED — the LSP handler's incomplete-rename safety gates
    /// stay a separate layer this admission does not touch).
    RenameLocations,
    /// `get_code_actions` (MIXED — the LSP `handle_code_action` handler contributes
    /// native Verter carrier code-actions (organize-imports, extract-component,
    /// macro/component/event actions, action-engine fixes) and MERGES the provider's
    /// `getCodeFixes` quickfixes over them; denied ⇒ the empty external default so the
    /// handler merge preserves the native side).
    CodeActions,
}

impl ProviderFeature {
    /// The stable feature name, for admission observability. EXHAUSTIVE match (no
    /// wildcard): the ONLY compile-time-enforced property is that every variant is named
    /// here (a new variant fails to compile until it is). The public feature behavior is
    /// pinned by the typed `owned_binding_gate` integration suite (see the type doc).
    fn name(self) -> &'static str {
        match self {
            ProviderFeature::TypeDefinition => "type_definition",
            ProviderFeature::SignatureHelp => "signature_help",
            ProviderFeature::SemanticTokens => "semantic_tokens",
            ProviderFeature::Hover => "hover",
            ProviderFeature::Definition => "definition",
            ProviderFeature::References => "references",
            ProviderFeature::DocumentHighlights => "document_highlights",
            ProviderFeature::InlayHints => "inlay_hints",
            ProviderFeature::Completions => "completions",
            ProviderFeature::CompletionDetails => "completion_details",
            ProviderFeature::ResolveCompletion => "resolve_completion",
            ProviderFeature::RenameLocations => "rename_locations",
            ProviderFeature::CodeActions => "code_actions",
        }
    }
}

/// The always-present host-aware admission and serving-order layer.
pub struct TsgoCompositeProvider {
    /// Managed fallback. It is eager in an explicit managed mode and statefully lazy
    /// when the editor-owned route is armed.
    managed: Arc<dyn TypeProvider>,
    /// Live published-snapshot + per-project R21 env-dims authority.
    host: Arc<VerterHost>,
    /// Exact editor-session route, present only with rendezvous evidence.
    shared: Option<SharedTsgoOverlay>,
    /// Generation-scoped carrier feature admission cache. It memoizes the one shared
    /// resolver per `(source, generation)`; it is not a second binding engine.
    admission: CarrierAdmissionCache,
}

impl TsgoCompositeProvider {
    /// Build the layer over a managed provider slot, the binding authority, and an
    /// optional exact-editor route.
    #[must_use]
    pub fn new(
        managed: Arc<dyn TypeProvider>,
        host: Arc<VerterHost>,
        shared: Option<SharedTsgoOverlay>,
    ) -> Self {
        Self {
            managed,
            host,
            shared,
            admission: CarrierAdmissionCache::new(),
        }
    }

    /// Select the provider for a feature query while preserving the serving order.
    ///
    /// A NON-carrier path (plain `.ts`/`.tsx`, `carrier_source_of == None`) is UNGATED:
    /// it delegates to managed unchanged. A carrier companion admits through the
    /// generation-scoped [`CarrierAdmissionCache`] (the ONE shared `resolve_carrier_bound`
    /// resolver, memoized): only a resolved `BoundProject` admits. Every non-bound state —
    /// and, by the cache's construction, any never-produced state — FAILS CLOSED: the
    /// caller serves its type's empty/none external default, NEVER a `tsgo --lsp`
    /// self-discovery fall-through. `feature` ties each method to its [`ProviderFeature`]
    /// variant (the method↔variant registry) and labels the fail-closed trace.
    async fn feature_provider(
        &self,
        feature: ProviderFeature,
        path: &str,
    ) -> Option<Arc<dyn TypeProvider>> {
        // NON-carrier path (plain `.ts`/`.tsx`): not gated. In a carrier-only LSP
        // client this is not normally queried; an explicit request uses managed.
        let Some(source) = carrier_source_of(path) else {
            return Some(Arc::clone(&self.managed));
        };

        let admission = self.admission.admit(&self.host, &source);
        let Some(carrier) = admission.bound_carrier() else {
            tracing::trace!(
                feature = feature.name(),
                source = %source,
                "carrier feature denied — no BoundProject; serving the external default \
                 (fail-closed, never a `--lsp` self-discovery fall-through)"
            );
            return None;
        };

        if let Some(shared) = &self.shared {
            match tokio::time::timeout(
                SHARED_OVERLAY_TIMEOUT,
                shared.engage_provider(path, carrier),
            )
            .await
            {
                Ok(Some(provider)) => return Some(provider),
                Ok(None) => tracing::info!(
                    feature = feature.name(),
                    source = %source,
                    "editor-owned tsgo attach did not engage; activating managed fallback"
                ),
                Err(_) => tracing::warn!(
                    feature = feature.name(),
                    source = %source,
                    "editor-owned tsgo attach timed out; activating managed fallback"
                ),
            }
        }

        Some(Arc::clone(&self.managed))
    }

    /// Shared-first diagnostics entry used by both foreground and background queries.
    ///
    /// A NON-carrier path (plain `.ts`/`.tsx`) is NOT gated — it delegates to managed
    /// unchanged. A carrier companion resolves its owning project ONCE via the shared
    /// [`project_binding`] helper: a NON-bound state yields NO external-TS diagnostics
    /// (fail closed — never a `tsgo --lsp` inferred / own-discovery fall-through), and a
    /// BOUND carrier first attempts the editor route and activates managed only after an
    /// observed shared failure.
    async fn diagnostics_gated(
        &self,
        path: &str,
        background: bool,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        // NON-carrier path: not gated — delegate to managed unchanged.
        let Some(source) = carrier_source_of(path) else {
            return self.managed_diagnostics(path, background).await;
        };

        // Carrier companion: resolve the owning project ONCE. A non-bound state yields
        // NO external-TS diagnostics for the carrier (fail closed — NEVER a `tsgo --lsp`
        // inferred / own-discovery fall-through for the carrier).
        let Some(carrier) =
            project_binding::resolve_carrier_bound(&self.host, &source).into_bound()
        else {
            return Ok(Vec::new());
        };

        // SHARED is authoritative and attempted FIRST. Only an observed failure or
        // timeout admits the managed provider. This is deliberately not a union: a
        // successful editor-owned route must not start or query a duplicate engine.
        if let Some(shared) = &self.shared {
            match tokio::time::timeout(
                SHARED_OVERLAY_TIMEOUT,
                shared.engage_diagnostics(path, &carrier),
            )
            .await
            {
                Ok(Some(diagnostics)) => return Ok(diagnostics),
                Ok(None) => tracing::info!(
                    source = %source,
                    "editor-owned tsgo diagnostics did not engage; activating managed fallback"
                ),
                Err(_) => tracing::warn!(
                    source = %source,
                    "editor-owned tsgo diagnostics timed out; activating managed fallback"
                ),
            }
        }

        self.managed_diagnostics(path, background).await
    }

    /// Managed diagnostics for `path` on the requested lane.
    async fn managed_diagnostics(
        &self,
        path: &str,
        background: bool,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        if background {
            self.managed.get_diagnostics_background(path).await
        } else {
            self.managed.get_diagnostics(path).await
        }
    }

    /// Record the carrier's content into the SHARED overlay (a cheap in-memory insert
    /// off the managed lifecycle critical path) — a no-op when SHARED is not opted in.
    fn shared_record(&self, path: &str, content: &str) {
        if let Some(shared) = &self.shared {
            shared.record_content(path, content);
        }
    }

    /// Retract a carrier from the SHARED overlay off the managed close critical path — a
    /// no-op when SHARED is not opted in.
    async fn shared_feed_close(&self, path: &str) {
        if let Some(shared) = &self.shared {
            shared.feed_close(path).await;
        }
    }
}

impl std::fmt::Debug for TsgoCompositeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoCompositeProvider")
            .field("managed", &self.managed.provider_id())
            .field("shared", &self.shared.is_some())
            .finish_non_exhaustive()
    }
}

impl TypeProvider for TsgoCompositeProvider {
    fn provider_id(&self) -> &'static str {
        // The composite IS the tsgo provider — the SHARED overlay is an internal
        // implementation detail of the ONE provider; every engine-identifying branch
        // treats it as tsgo.
        self.managed.provider_id()
    }

    fn supports_completion_resolve(&self) -> bool {
        self.managed.supports_completion_resolve()
    }

    // ── Carrier lifecycle: record desired state in the managed slot, then record the
    //    carrier for SHARED. A lazy managed slot does not spawn here. This path never
    //    awaits SHARED establishment.
    //    The query path (`get_diagnostics`) establishes the transport and injects the
    //    recorded content lazily; a bound carrier's `--api` diagnostics then see it. ──

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.open_file(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.load_file(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.update_file(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.managed.close_file(&path).await?;
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.open_file_background(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.load_file_background(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.update_file_background(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.managed.close_file_background(&path).await?;
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.open_file_normal(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.load_file_normal(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.managed.update_file_normal(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.managed.close_file_normal(&path).await?;
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    // ── Diagnostics: gate on a resolved BoundProject, serve SHARED first, and admit
    //    managed only after observed shared failure. The two SHARED diagnostic channels
    //    are composed inside one editor-owned session. ──

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.diagnostics_gated(&path, false).await })
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.diagnostics_gated(&path, true).await })
    }

    // ── Features: `feature_provider` gates each carrier on BoundProject and preserves
    //    shared-first ordering. A non-bound carrier serves its empty external default. ──

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        // MIXED: a denied carrier serves the empty external default (native completions
        // preserved by the handler merge); a non-carrier path is ungated.
        let path = path.to_string();
        let trigger_character = trigger_character.map(str::to_string);
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::Completions, &path)
                .await
            {
                provider
                    .get_completions(&path, offset, trigger_character.as_deref())
                    .await
            } else {
                Ok(CompletionResult {
                    items: Vec::new(),
                    is_incomplete: false,
                })
            }
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        // MIXED: a denied carrier serves the empty external default.
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::CompletionDetails, path)
                .await
            {
                provider.get_completion_details(path, offset, items).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        // EXTERNAL-ONLY: a denied carrier suppresses provider enrichment (None) — no
        // owned resolve call.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::ResolveCompletion, &path)
                .await
            {
                provider.resolve_completion(&path, data).await
            } else {
                Ok(None)
            }
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        // MIXED: a denied carrier serves the None external default (the handler merge
        // preserves any native sub-answer); a non-carrier path is ungated.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self.feature_provider(ProviderFeature::Hover, &path).await {
                provider.get_hover(&path, offset).await
            } else {
                Ok(None)
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::Definition, &path)
                .await
            {
                provider.get_definition(&path, offset).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        // EXTERNAL-ONLY: a denied carrier serves the empty external default with NO owned
        // delegation (never a `--lsp` self-discovery fall-through); a non-carrier path is
        // ungated.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::TypeDefinition, &path)
                .await
            {
                provider.get_type_definition(&path, offset).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::References, &path)
                .await
            {
                provider.get_references(&path, offset).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        // MIXED: a denied carrier serves the empty external default (native rename only);
        // the LSP handler's existing incomplete-rename safety gates — a SEPARATE layer
        // this admission does not touch — still block unsafe partial edits. Never a
        // `--lsp` self-discovery fall-through after admission failure.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::RenameLocations, &path)
                .await
            {
                provider.get_rename_locations(&path, offset).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        // EXTERNAL-ONLY: a denied carrier serves `None` with NO owned delegation.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::SignatureHelp, &path)
                .await
            {
                provider.get_signature_help(&path, offset).await
            } else {
                Ok(None)
            }
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        // MIXED: a denied carrier serves the empty external default (the LSP
        // `handle_code_action` handler's native Verter carrier code-actions —
        // organize-imports, extract-component, macro/component/event actions,
        // action-engine fixes — are preserved by its merge); a non-carrier path is
        // ungated. Never a `--lsp` self-discovery fall-through after admission failure.
        let path = path.to_string();
        let diagnostics = diagnostics.to_vec();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::CodeActions, &path)
                .await
            {
                provider
                    .get_code_actions(&path, start_offset, end_offset, &diagnostics)
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        // EXTERNAL-ONLY: a denied carrier serves the empty external default with NO owned
        // delegation.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::SemanticTokens, &path)
                .await
            {
                provider.get_semantic_tokens(&path).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::DocumentHighlights, &path)
                .await
            {
                provider.get_document_highlights(&path, offset).await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(provider) = self
                .feature_provider(ProviderFeature::InlayHints, &path)
                .await
            {
                provider
                    .get_inlay_hints(&path, start_offset, end_offset)
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    // ── Config / workspace lifecycle: record/delegate through the managed slot. ──

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        self.managed.configure_paths(base_url, paths)
    }

    fn configure_paths_background(
        &self,
        base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        self.managed.configure_paths_background(base_url, paths)
    }

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        self.managed.notify_carrier_changed(companion_path)
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        self.managed.register_carrier_member(
            source_path,
            companion_path,
            content,
            project_file_name,
        )
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        self.managed.resync_open_files()
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.managed.update_workspace_folders(added, removed)
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.managed
            .update_workspace_folders_background(added, removed)
    }

    fn child_pid(&self) -> Option<u32> {
        self.managed.child_pid()
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            if let Some(shared) = &self.shared {
                shared.shutdown().await;
            }
            self.managed.shutdown().await
        })
    }
}

impl SharedTsgoOverlay {
    /// Tear the SHARED transport down (best-effort), then let managed shutdown remain the
    /// composite authority. Bounded: a slow/dead SHARED teardown must never block past
    /// this bound; on elapse the
    /// teardown is abandoned (the transport is dropped/evicted anyway). Uses the
    /// non-establishing `current` accessor, so shutdown never triggers an establishment.
    async fn shutdown(&self) {
        if let Some(transport) = self.inner.core.current().await {
            let _ = tokio::time::timeout(SHARED_CLOSE_TIMEOUT, transport.teardown()).await;
        }
    }
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod composite_tests;
