//! The composite tsgo provider: the ALWAYS-present host-aware admission layer over a
//! live OWNED provider, with an OPTIONAL SHARED carrier-diagnostics overlay.
//!
//! [`TsgoCompositeProvider`] is the production tsgo [`TypeProvider`] shape. It holds
//! the complete OWNED dual-surface provider ([`crate::tsgo::ipc::TsgoOwnedProvider`],
//! the full feature + diagnostics surface), the host (the LIVE published-snapshot +
//! per-project R21 env-dims authority), and an OPTIONAL [`SharedTsgoOverlay`] (present
//! only under the SHARED editor-attach rendezvous). Every non-diagnostic
//! `TypeProvider` method delegates to OWNED; ONLY carrier `get_diagnostics` is gated +
//! composed.
//!
//! **The OWNED carrier-diagnostics gate.** A path that is a carrier companion resolves
//! its owning configured project ONCE through the shared
//! [`project_binding`](crate::tsgo::project_binding) helper — published snapshot →
//! [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
//! → [`ProjectBinding`](verter_session::external_ts::ProjectBinding) →
//! [`BoundProject`](verter_session::external_ts::BoundProject) witness. Only a bound
//! carrier delegates to OWNED's `--lsp` diagnostics; every non-bound state (`NoProject`
//! / `Ambiguous` / `SyntheticScratch`, a pre-published snapshot, an `ensure_project`
//! refusal) yields NO external-TS diagnostics for the carrier — fail closed, NEVER a
//! `tsgo --lsp` inferred / own-discovery fall-through. A NON-carrier path (plain
//! `.ts`/`.tsx`) is NOT gated — it delegates to OWNED unchanged. Non-diagnostic FEATURE
//! methods delegate to OWNED unchanged (carrier-feature admission is deferred).
//!
//! **The optional SHARED union.** For a bound carrier, when the SHARED overlay is opted
//! in, its `--api` semantic diagnostics — served through the SAME already-resolved
//! binding (no second resolution) — are UNIONED over OWNED's `--lsp` surface,
//! deduplicated ([`compose_diagnostics`]): OWNED's syntactic/suggestion/tag/related
//! diagnostics are preserved, never replaced wholesale. A not-SHARED / unestablished /
//! failed / unavailable SHARED result leaves the OWNED diagnostics unchanged. SHARED
//! never displaces OWNED and never regresses an editor feature to a SHARED stub, and the
//! SHARED opt-in path can NEVER bypass the OWNED gate (a non-bound carrier fails closed
//! for both bare-OWNED and SHARED-composite consistently).
//!
//! This preserves the project-bound external-TS contract: ownership resolves through
//! the shared [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
//! over the live [`PublishedRoot`](verter_workspace::published_state::PublishedRoot),
//! mints the `BoundProject` witness from the resolved binding, and fails closed — no
//! inferred fallback, no fabricated binding, no path-only bypass of the witness.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use verter_session::external_ts::{AmbiguityCause, ProjectBinding, ProjectResolution, ServeMode};
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
use crate::tsgo::project_binding::{self, BoundCarrier};
use crate::tsgo::shared::{EstablishSharedParams, TsgoSharedProvider};

/// The bound on the lazy SHARED-attach establishment: a slow or never-initializing
/// editor tsgo cannot stall a carrier diagnostics query beyond this — on elapse the
/// overlay yields no SHARED result and the composite serves the OWNED baseline
/// (fail-closed). Concurrent queries during establishment reuse the one bounded
/// attempt (singleflight); a failed attempt re-arms on a fresh advertisement/editor
/// generation OR a fresh workspace/config generation (see
/// [`LazyTransport`](crate::tsgo::transport_cell::LazyTransport)). Establishment is
/// reached ONLY from the query path ([`SharedTsgoOverlay::engage_diagnostics`]), never
/// the OWNED file-lifecycle path — so opting into SHARED never trips the OWNED
/// foreground-sync budget.
const SHARED_ESTABLISH_TIMEOUT: Duration = Duration::from_secs(15);

/// The OUTER production deadline bounding the ENTIRE SHARED overlay contribution to a
/// single diagnostics query — establishment + whole-dirty-set injection + the per-query
/// control re-decision + the `--api` semantic-diagnostics query, as ONE unit. OWNED has
/// already completed by the time this runs, so on elapse (a stuck relay / control /
/// `--api` peer) OR any SHARED error the composite returns the already-computed OWNED
/// result (fail-closed) — opt-in SHARED can NEVER turn a ready-OWNED diagnostics response
/// into an unbounded LSP stall. Every SHARED sub-op on the query path is awaited inside
/// this bound, so the single outer deadline cancels whichever sub-op is blocked; no
/// sub-op can escape it. It EXCEEDS [`SHARED_ESTABLISH_TIMEOUT`] so a legitimately slow
/// FIRST establishment is not cut short before its own singleflight bound (which itself
/// fails closed) can decide — a warm session skips establishment, so only inject + `--api`
/// (sub-second in practice) run under it.
const SHARED_OVERLAY_TIMEOUT: Duration = Duration::from_secs(20);

/// The bound on a SHARED carrier retract issued from the OWNED `close_file` lifecycle:
/// a slow or never-answering relay close cannot hang or delay the composite close beyond
/// this — on elapse the retract is abandoned (fail-closed) and the composite close
/// returns promptly. The retract is best-effort and the transport is torn down / evicted
/// on a broken connection anyway, so a dropped retract only leaves a soon-cleaned
/// lingering document, never a wrong result. Symmetric with the open/change lifecycle,
/// which only RECORDS content off the OWNED critical path.
const SHARED_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// The client label the SHARED overlay presents on the control hello (diagnostics
/// only).
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
    /// The lazy overlay core: the per-carrier content cache the OWNED lifecycle records
    /// into OFF the critical path, plus the lazily-established relay-attach transport
    /// cell the QUERY path establishes + injects into. The transport is a singleflight,
    /// bounded, re-arming, liveness-evicting cell (established once on the first bound
    /// carrier DIAGNOSTICS query; reused after). The STATE lock is never held across the
    /// establishment I/O, a slow/broken attach is bounded by [`SHARED_ESTABLISH_TIMEOUT`]
    /// (fail-closed to OWNED), and a failed attach re-arms on a fresh advertisement/editor
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
    /// insert OFF the OWNED lifecycle critical path. It NEVER establishes the SHARED
    /// transport, so opting into SHARED cannot trip the OWNED file-lifecycle timing
    /// (the foreground TSX sync is budgeted far below the SHARED establishment bound).
    /// The query path ([`Self::engage_diagnostics`]) establishes the transport and
    /// injects the recorded content lazily. A non-carrier path is ignored.
    fn record_content(&self, provider_path: &str, content: &str) {
        if carrier_source_of(provider_path).is_none() {
            return;
        }
        self.inner.core.record_content(provider_path, content);
    }

    /// Retract a carrier overlay OFF the OWNED `close_file` critical path — drop its
    /// recorded content and, if the SHARED transport is already established, issue the
    /// retract BOUNDED + fail-closed (a slow/dead relay cannot hang or delay the OWNED
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

    /// The SHARED carrier diagnostics for a carrier ALREADY resolved to a bound
    /// configured project by the composite's OWNED gate, or `None` when SHARED does
    /// not engage — an unestablished/failed attach, a not-SHARED live decision, a
    /// project the `--api` could not open, or a carrier that is not a Program root —
    /// in which case the caller keeps the OWNED baseline. A successfully-served result
    /// (even empty) is overlaid; a fail-closed/errored one is not (no forged span
    /// leaks).
    ///
    /// The carrier binding is passed in PRE-RESOLVED (the composite gate resolved it
    /// ONCE via the shared [`project_binding`] helper): SHARED reuses the SAME binding
    /// (for its per-query re-decision + transport), the SAME generation (for the
    /// transport re-arm), and the SAME already-minted `BoundProject`
    /// (`carrier.bound().project()` — the version-independent owning tsconfig) for the
    /// `--api` overlay target. There is NO second resolution and NO witness re-mint.
    async fn engage_diagnostics(
        &self,
        provider_path: &str,
        carrier: &BoundCarrier,
    ) -> Option<Vec<TypeDiagnostic>> {
        // Lazily establish (once) the SHARED relay-attach transport for the
        // ALREADY-resolved binding — at QUERY time, OFF the OWNED lifecycle critical
        // path (SHARED is never fabricated; the binding is the gate's resolved one).
        let transport = self
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
        // failed inject leaves the OWNED baseline as the fallback (fail-closed).
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
            .inject_all_dirty(&transport, Some(shadow_generation), |companion| {
                self.injection_is_shadow_safe(companion)
            })
            .await;

        // Fail closed to OWNED when the QUERIED carrier's CURRENT content is not
        // confirmed synced into the shared Program (its dirty injection failed) — never
        // serve SHARED diagnostics computed against stale/absent content (a prior synced
        // slot). Only a carrier whose current content is confirmed synced is served.
        if !self.inner.core.is_synced(provider_path) {
            return None;
        }

        // Re-decide the serve mode through the live controller at the resolved
        // snapshot/config generation, reusing the SAME binding — a not-SHARED decision
        // falls back to OWNED.
        if transport
            .redecide_for_binding(carrier.binding(), carrier.generation())
            .mode()
            != ServeMode::Shared
        {
            return None;
        }

        // The witnessed project — the owning tsconfig from the SAME already-minted
        // BoundProject (version-independent) — is the `--api` overlay target. No second
        // resolution, no witness re-mint (the `--api` snapshot rail keys on the
        // transport's own gate-observed version downstream).
        match transport
            .overlay_diagnostics_in_project(provider_path, carrier.bound().project())
            .await
        {
            Ok(Some(diags)) => Some(diags),
            Ok(None) | Err(_) => None,
        }
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
    ///    Either downgrade means SKIPPED, OWNED serves. A genuine generated companion (a
    ///    clean binding, `NoProject`, `SyntheticScratch`, or a MultipleOwners ambiguity,
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
        match project_binding::resolve_carrier(&self.inner.host, &source, Arc::from("")) {
            Some((resolution, _)) => injection_shadow_safe(&resolution),
            None => false,
        }
    }

    /// Lazily establish (once) the SHARED relay-attach transport for the carrier's
    /// ALREADY-resolved `binding` (resolved once by the composite's OWNED gate at
    /// `generation`), through the singleflight + bounded + re-arming [`LazyTransport`]
    /// cell. Only a bound carrier reaches here (the gate resolved the binding before
    /// calling [`Self::engage_diagnostics`]), so the cell is never entered — nor its
    /// `Unavailable` slot poisoned — by a transient non-binding
    /// ([`LazyTransport::get_or_establish_bound`]). Concurrent queries reuse the ONE
    /// in-flight establishment; a slow/broken attach is bounded by
    /// [`SHARED_ESTABLISH_TIMEOUT`] (fail-closed to OWNED, never a stall); a failed
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
    ) -> Option<Arc<TsgoSharedProvider>> {
        // The binding is pre-resolved (bound) — pass it straight to the cell. The core
        // supplies the live-death eviction predicate; a no-binding carrier never
        // reaches here, so the cell is never poisoned by a transient non-binding.
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
    /// establishment (the OWNED baseline serves); a `None` here is an ACTUAL attach
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
            resolution: ProjectResolution::ProjectBinding(binding),
            config_generation: generation,
            client_label: SHARED_CLIENT_LABEL,
        };
        match TsgoSharedProvider::establish_shared(params).await {
            Ok(transport) => Some(Arc::new(transport)),
            Err(e) => {
                tracing::info!("SHARED overlay not established ({e}); the OWNED baseline serves");
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
/// plain `.ts`/`.tsx` file (no carrier stem) yields `None` (the OWNED baseline serves
/// it). Backslash paths normalize to the same forward-slashed source.
fn carrier_source_of(provider_path: &str) -> Option<String> {
    classify_carrier_companion(provider_path).map(|companion| companion.source)
}

/// Whether a carrier companion whose SOURCE resolved to `resolution` is shadow-safe to
/// inject into the SHARED Program. A real user file occupying a descriptor
/// carrier-companion path (or a same-stem rune module beside the source) downgrades the
/// source to an `Ambiguous` real-file-shadow cause — Verter must NEVER overlay-shadow it
/// (`carrier_never_shadows_real_user_file`). Every other resolution (a clean binding,
/// `NoProject`, `SyntheticScratch`, or a MultipleOwners ambiguity — none of which sit a
/// REAL file at the companion path) leaves a GENUINE virtual companion safe to inject as
/// a supporting Program member. Typed over [`ProjectResolution`] — never a path-shape or
/// substring check.
fn injection_shadow_safe(resolution: &ProjectResolution) -> bool {
    !matches!(
        resolution,
        ProjectResolution::Ambiguous(
            AmbiguityCause::CarrierPathOccupiedByRealFile | AmbiguityCause::SameStemRuneModule
        )
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
/// diagnostic — e.g. an identical carrier type error reported by BOTH the OWNED
/// `--lsp` surface and the SHARED `--api getSemanticDiagnostics`.
type DiagnosticIdentity = (u32, u32, Option<String>, String);

fn diagnostic_identity(d: &TypeDiagnostic) -> DiagnosticIdentity {
    (d.start, d.end, d.code.clone(), d.message.clone())
}

/// Compose the SHARED and OWNED carrier diagnostics for a bound carrier.
///
/// SHARED serves the AUTHORITATIVE cross-file SEMANTIC diagnostics (the editor's own
/// Program via `--api getSemanticDiagnostics`). OWNED (the `--lsp` surface) additionally
/// serves the SYNTACTIC / suggestion / tag / related diagnostic surface that
/// `getSemanticDiagnostics` does NOT produce. The composite UNIONS the two, deduplicated
/// by [`diagnostic_identity`], so:
///
/// - OWNED's syntactic/suggestion/tag/related diagnostics are PRESERVED — no diagnostic
///   class the user received from OWNED is silently dropped (the wholesale-replace defect
///   this closes);
/// - SHARED's authoritative semantic diagnostics are overlaid;
/// - an IDENTICAL diagnostic reported by BOTH engines appears exactly ONCE — SHARED's
///   copy (listed first, with its authoritative span mapping) is retained, and OWNED's
///   duplicate's METADATA (`tags` + `relatedInformation`) is MERGED INTO it
///   ([`merge_diagnostic_metadata`]) rather than dropped, so nothing is double-reported
///   AND OWNED's richer tag/related surface (e.g. the `Unnecessary` unused-fade, a
///   "declared here" related span) is never lost when SHARED reports the same
///   `(span, code, message)` without it.
///
/// This is a UNION, not a semantic/non-semantic partition of OWNED: a semantic diagnostic
/// OWNED reports that SHARED does not (a rare Program divergence) survives — the
/// fail-safe direction (surfacing a diagnostic, never hiding one). The `TypeDiagnostic`
/// carrier has no semantic-vs-syntactic tag, so the honest merge is the deduplicated
/// union rather than a heuristic code-range partition.
fn compose_diagnostics(
    shared: Vec<TypeDiagnostic>,
    owned: Vec<TypeDiagnostic>,
) -> Vec<TypeDiagnostic> {
    // Index the retained SHARED diagnostics by identity so an OWNED duplicate MERGES
    // its metadata into the SHARED copy at that index (never dropping OWNED's tags /
    // relatedInformation) instead of being discarded wholesale.
    let mut merged = shared;
    let mut index: HashMap<DiagnosticIdentity, usize> = merged
        .iter()
        .enumerate()
        .map(|(i, d)| (diagnostic_identity(d), i))
        .collect();
    for diag in owned {
        match index.get(&diagnostic_identity(&diag)) {
            // Collision: SHARED already carries this `(span, code, message)` — union
            // OWNED's metadata into the retained SHARED copy.
            Some(&i) => merge_diagnostic_metadata(&mut merged[i], diag),
            // OWNED-only: append it (and index it so a later OWNED duplicate merges).
            None => {
                index.insert(diagnostic_identity(&diag), merged.len());
                merged.push(diag);
            }
        }
    }
    merged
}

/// Compose the OWNED baseline with the SHARED overlay contribution under ONE outer
/// bounded deadline. `owned` is the already-computed baseline; `shared` is the SHARED
/// overlay future (establish + inject-all-dirty + control re-decision + `--api` query,
/// as one unit). On elapse OR a `None`/errored SHARED result, the already-computed OWNED
/// result serves (fail-closed) — a stuck relay / control / `--api` peer never stalls
/// diagnostics past `timeout` even though OWNED is ready. On a prompt SHARED result the
/// two are unioned ([`compose_diagnostics`]). This is the sole place OWNED and the
/// bounded SHARED contribution are composed, so every diagnostics entry point shares the
/// one fail-closed deadline.
async fn compose_owned_with_bounded_shared<F>(
    owned: Vec<TypeDiagnostic>,
    shared: F,
    timeout: Duration,
) -> Vec<TypeDiagnostic>
where
    F: Future<Output = Option<Vec<TypeDiagnostic>>>,
{
    match tokio::time::timeout(timeout, shared).await {
        Ok(Some(shared)) => compose_diagnostics(shared, owned),
        // Timeout (Err) OR a fail-closed/errored SHARED result (Ok(None)) ⇒ the
        // already-computed OWNED result stands unchanged (fail-closed to OWNED).
        Ok(None) | Err(_) => owned,
    }
}

/// Merge an OWNED duplicate's metadata into the retained SHARED diagnostic on a
/// `(span, code, message)` collision: UNION the `tags` and `related_information`
/// (append each OWNED entry the SHARED copy does not already carry), so OWNED's
/// richer tag/related surface is preserved rather than dropped. SHARED's span,
/// severity, and message win (its authoritative mapping is retained); ONLY the
/// metadata is unioned — never a silent OWNED-metadata drop.
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

/// The ALWAYS-present host-aware admission layer over the OWNED tsgo baseline, with an
/// OPTIONAL SHARED carrier-diagnostics overlay.
pub struct TsgoCompositeProvider {
    /// The complete OWNED dual-surface provider — the universal baseline every
    /// non-diagnostic method delegates to, and the bound-carrier diagnostics surface.
    owned: Arc<dyn TypeProvider>,
    /// The host — the LIVE published-snapshot + per-project R21 env-dims authority the
    /// OWNED carrier-diagnostics gate resolves the carrier's owning project over.
    host: Arc<VerterHost>,
    /// The OPTIONAL SHARED carrier-diagnostics overlay — present ONLY under the SHARED
    /// editor-attach rendezvous. Absent, the composite is the bare host-aware OWNED
    /// admission layer (still gating carrier diagnostics on a resolved `BoundProject`).
    shared: Option<SharedTsgoOverlay>,
}

impl TsgoCompositeProvider {
    /// Build the always-present admission layer over a complete OWNED provider, the
    /// host (the binding-resolution authority), and an OPTIONAL SHARED overlay.
    #[must_use]
    pub fn new(
        owned: Arc<dyn TypeProvider>,
        host: Arc<VerterHost>,
        shared: Option<SharedTsgoOverlay>,
    ) -> Self {
        Self {
            owned,
            host,
            shared,
        }
    }

    /// The OWNED carrier-diagnostics gate + optional SHARED union — the sole gated
    /// diagnostics entry both `get_diagnostics` and `get_diagnostics_background` share.
    ///
    /// A NON-carrier path (plain `.ts`/`.tsx`) is NOT gated — it delegates to OWNED
    /// unchanged. A carrier companion resolves its owning project ONCE via the shared
    /// [`project_binding`] helper: a NON-bound state yields NO external-TS diagnostics
    /// (fail closed — never a `tsgo --lsp` inferred / own-discovery fall-through), and a
    /// BOUND carrier delegates to OWNED and, when SHARED is opted in, unions the SHARED
    /// `--api` diagnostics over OWNED through the SAME already-resolved binding.
    async fn diagnostics_gated(
        &self,
        path: &str,
        background: bool,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        // NON-carrier path: not gated — delegate to OWNED unchanged.
        let Some(source) = carrier_source_of(path) else {
            return self.owned_diagnostics(path, background).await;
        };

        // Carrier companion: resolve the owning project ONCE. A non-bound state yields
        // NO external-TS diagnostics for the carrier (fail closed — NEVER a `tsgo --lsp`
        // inferred / own-discovery fall-through for the carrier).
        let Some(carrier) =
            project_binding::resolve_carrier_bound(&self.host, &source).into_bound()
        else {
            return Ok(Vec::new());
        };

        // Bound: OWNED `--lsp` diagnostics for the carrier.
        let owned = self.owned_diagnostics(path, background).await?;

        // SHARED (opt-in) + bound: union the `--api` semantic diagnostics OVER OWNED
        // through the SAME already-resolved binding (no second resolution), under ONE
        // outer bounded deadline — a not-SHARED / stuck / errored SHARED result leaves
        // OWNED unchanged (fail-closed). Absent SHARED, OWNED stands.
        match &self.shared {
            Some(shared) => Ok(compose_owned_with_bounded_shared(
                owned,
                shared.engage_diagnostics(path, &carrier),
                SHARED_OVERLAY_TIMEOUT,
            )
            .await),
            None => Ok(owned),
        }
    }

    /// OWNED diagnostics for `path` on the requested lane (foreground vs background).
    async fn owned_diagnostics(
        &self,
        path: &str,
        background: bool,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        if background {
            self.owned.get_diagnostics_background(path).await
        } else {
            self.owned.get_diagnostics(path).await
        }
    }

    /// Record the carrier's content into the SHARED overlay (a cheap in-memory insert
    /// OFF the OWNED lifecycle critical path) — a no-op when SHARED is not opted in.
    fn shared_record(&self, path: &str, content: &str) {
        if let Some(shared) = &self.shared {
            shared.record_content(path, content);
        }
    }

    /// Retract a carrier from the SHARED overlay OFF the OWNED close critical path — a
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
            .field("owned", &self.owned.provider_id())
            .field("shared", &self.shared.is_some())
            .finish_non_exhaustive()
    }
}

impl TypeProvider for TsgoCompositeProvider {
    fn provider_id(&self) -> &'static str {
        // The composite IS the tsgo provider — the SHARED overlay is an internal
        // implementation detail of the ONE provider; every engine-identifying branch
        // treats it as tsgo.
        self.owned.provider_id()
    }

    fn supports_completion_resolve(&self) -> bool {
        self.owned.supports_completion_resolve()
    }

    // ── Carrier lifecycle: delegate to OWNED (the baseline), then RECORD the carrier
    //    content for the SHARED overlay — a cheap in-memory insert OFF the OWNED
    //    critical path. It NEVER awaits the (up-to-15s) SHARED establishment, so the
    //    OWNED file-lifecycle timing is unchanged whether or not SHARED is opted in.
    //    The query path (`get_diagnostics`) establishes the transport and injects the
    //    recorded content lazily; a bound carrier's `--api` diagnostics then see it. ──

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.open_file(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.load_file(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.update_file(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.owned.close_file(&path).await?;
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.open_file_background(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.load_file_background(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.update_file_background(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.owned.close_file_background(&path).await?;
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.open_file_normal(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.load_file_normal(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.owned.update_file_normal(&path, &content).await?;
            self.shared_record(&path, &content);
            Ok(())
        })
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.owned.close_file_normal(&path).await?;
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    // ── Diagnostics: GATE the carrier-diagnostics on a resolved `BoundProject` (a
    //    non-bound carrier fails closed to no external-TS diagnostics, never a
    //    `tsgo --lsp` inferred fall-through), then COMPOSE the optional SHARED `--api`
    //    semantic diagnostics OVER the OWNED `--lsp` surface for a bound carrier —
    //    OWNED's syntactic/suggestion/tag/related diagnostics preserved, SHARED's
    //    authoritative semantic ones overlaid, deduplicated. A non-carrier path is not
    //    gated. Fail-closed: when SHARED does not engage, OWNED stands unchanged. ──

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.diagnostics_gated(&path, false).await })
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.diagnostics_gated(&path, true).await })
    }

    // ── Features: delegate wholly to OWNED (the complete feature surface). SHARED
    //    never regresses an editor feature to a stub. ──

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        self.owned.get_completions(path, offset, trigger_character)
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        self.owned.get_completion_details(path, offset, items)
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        self.owned.resolve_completion(path, data)
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        self.owned.get_hover(path, offset)
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.owned.get_definition(path, offset)
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.owned.get_type_definition(path, offset)
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.owned.get_references(path, offset)
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        self.owned.get_rename_locations(path, offset)
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        self.owned.get_signature_help(path, offset)
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        self.owned
            .get_code_actions(path, start_offset, end_offset, diagnostics)
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        self.owned.get_semantic_tokens(path)
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        self.owned.get_document_highlights(path, offset)
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        self.owned.get_inlay_hints(path, start_offset, end_offset)
    }

    // ── Config / workspace / lifecycle: delegate to OWNED. ──

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        self.owned.configure_paths(base_url, paths)
    }

    fn configure_paths_background(
        &self,
        base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        self.owned.configure_paths_background(base_url, paths)
    }

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        self.owned.notify_carrier_changed(companion_path)
    }

    fn register_carrier_member(
        &self,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        self.owned
            .register_carrier_member(companion_path, content, project_file_name)
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        self.owned.resync_open_files()
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.owned.update_workspace_folders(added, removed)
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.owned
            .update_workspace_folders_background(added, removed)
    }

    fn child_pid(&self) -> Option<u32> {
        self.owned.child_pid()
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            if let Some(shared) = &self.shared {
                shared.shutdown().await;
            }
            self.owned.shutdown().await
        })
    }
}

impl SharedTsgoOverlay {
    /// Tear the SHARED transport down (best-effort) — the OWNED shutdown is the
    /// composite's authority. BOUNDED: a slow/dead SHARED teardown (a wedged relay /
    /// `--api` peer) must never block the OWNED shutdown past this bound; on elapse the
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
