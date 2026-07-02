//! Free-function producers and mapping-context helpers over the
//! [`ProviderSurfaceStore`]: the record choke points every provider-surface
//! sync site funnels through (`record_carrier_*`,
//! [`record_and_version_carrier_companions`], the reserved owner-bearing
//! [`record_carrier_surface`]), the captured-snapshot classification authority
//! ([`classify_captured_api_surface`]), and the merge-time context builders
//! ([`external_ide_context_from_snapshot`],
//! [`foreign_ide_context_from_captured`],
//! [`locate_prop_decl_range_in_carrier_api`]). Pure clients of the core
//! store's public API in the parent module — no store internals are touched.

use std::sync::Arc;

use dashmap::DashMap;

use verter_semantic::analysis::types::Hash16;
use verter_session::VerterHost;

use crate::carrier_cache::{EngineRecheckState, RegenKey};
use crate::documents::line_index::LineIndex;
use crate::documents::position_map::PositionMapper;
use crate::documents::provider_projection::ProviderPositionMapper;
use crate::documents::DocumentRegistry;

use super::{
    CapturedPathState, ContentHash, ProviderQuerySnapshot, ProviderSurfaceKind,
    ProviderSurfaceSnapshot, ProviderSurfaceStore, RecordSurface,
};

/// Build the merge-time [`ExternalIdeContext`](crate::type_provider::merge::ExternalIdeContext)
/// for a carrier PUBLIC-API surface from a PINNED, immutable snapshot — the
/// fail-closed bridge behind cross-file rename, anchored to the EXACT generation
/// the provider's offsets were produced against.
///
/// EVERYTHING comes from the snapshot, never a live `get_public_api()` / open
/// document: the provider (API) UTF-16 line index, the source map (parsed from
/// the SAME bytes), and the carrier `.vue` UTF-16 index. The carrier source —
/// captured at sync time (open buffer or host/VFS for a CLOSED carrier) — is
/// re-measured in the NEGOTIATED encoding so the merge re-emits the mapped
/// UTF-16 carrier range in that encoding. A snapshot with no source map fails
/// closed (`None`).
#[must_use]
pub fn external_ide_context_from_snapshot(
    snapshot: &ProviderSurfaceSnapshot,
    negotiated_encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> Option<crate::type_provider::merge::ExternalIdeContext> {
    let mapper = snapshot.source_map.as_ref()?;
    let carrier_negotiated_line_index =
        LineIndex::new(&snapshot.carrier_source, negotiated_encoding);
    Some(crate::type_provider::merge::ExternalIdeContext {
        tsx_line_index: snapshot.provider_utf16_line_index.clone(),
        mapper: (**mapper).clone(),
        carrier_line_index: snapshot.carrier_utf16_line_index.clone(),
        carrier_negotiated_line_index: Some(carrier_negotiated_line_index),
    })
}

/// Resolve the merge-time mapping context for a FOREIGN carrier IDE location
/// from the [`ProviderQuerySnapshot`] captured when the request BEGAN — never
/// the surface current at merge time.
///
/// The provider answered against the surfaces it held when the request was
/// issued; a foreign carrier re-synced mid-request would make a live-current
/// merge map the provider's generation-A offsets through a generation-B
/// mapper (torn, not stale). Fail-closed gates (any miss ⇒ `None`, the
/// location drops):
/// - the path was not captured as a `Current` `CarrierIde` surface when the
///   request began;
/// - the captured surface is no longer honored at merge time (a mid-request
///   re-sync with different content, or a close, invalidates — a
///   byte-identical same-map re-sync stays honored);
/// - the foreign carrier's OPEN document no longer byte-matches the captured
///   carrier source (a closed foreign document also drops — mapping targets
///   the open buffer, matching the live resolver this replaces);
/// - the captured surface has no usable source map.
#[must_use]
pub fn foreign_ide_context_from_captured(
    store: &ProviderSurfaceStore,
    documents: &DocumentRegistry,
    captured: &ProviderQuerySnapshot,
    ide_path: &str,
    negotiated_encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> Option<crate::type_provider::merge::ExternalIdeContext> {
    let snapshot = captured.snapshot_for(ide_path)?;
    if snapshot.kind != ProviderSurfaceKind::CarrierIde {
        return None;
    }
    if !store.captured_snapshot_still_honored(snapshot) {
        return None;
    }
    if !surface_matches_open_document_source(documents, &snapshot.source_canonical, snapshot) {
        return None;
    }
    external_ide_context_from_snapshot(snapshot, negotiated_encoding)
}

/// Locate the byte range of a child component's prop identifier IN the captured
/// `{carrier}.ts` PUBLIC-API content, keyed by the prop's TYPED `.vue` declaration
/// identity (its `.vue` decl span + name) — never a text scan of the API content.
///
/// This is the one piece a provider-agnostic cross-file Vue-prop rename needs that
/// a provider's `textDocument/rename` may not itself enumerate (tsgo does not): the
/// child-declaration rename leg, synthesized by Verter as a [`RenameLocation`] whose
/// `start..end` is the prop name's byte range in the SAME captured API content the
/// merge maps through. The merge then maps that range back onto the `.vue` via the
/// snapshot's own source map — byte-identically to how it maps a provider's real
/// carrier location — so the result dedups against the provider's location by the
/// final `.vue` range.
///
/// IDENTITY-DRIVEN, not text-scanned: the API generator emits the prop name through
/// the SAME `push_mapped(name, vue_decl_span)` token that seeds this snapshot's
/// source map, so querying the map for the prop's `.vue` decl-span START yields the
/// API position the generator wrote the name at. The byte range is then
/// `[start, start + name.len())` because the generator writes the name VERBATIM
/// (a position-preserving run), so the API slice equals the name exactly.
///
/// FAIL CLOSED (`None`) when any of:
/// - the snapshot carries no source map (an unmappable surface),
/// - the prop's `.vue` decl-span start does not resolve to a `.vue` position, or
///   the map does not map that `.vue` position into the API content (the prop name
///   was not emitted with a mapped token — e.g. a `defineProps<ImportedType>()`
///   surface whose props are a bare type ref, not inline members),
/// - the resolved API position does not convert to a byte offset, or
/// - the API slice at the resolved range is NOT byte-equal to the prop name (the
///   correctness tripwire: a wrong/mis-ranged mapping must never emit an edit that
///   could corrupt the `.vue`).
///
/// A `None` here means the caller must NOT synthesize the child-declaration leg
/// (and, per the fail-closed rename ruling, must not ship a usage-only partial).
#[must_use]
pub fn locate_prop_decl_range_in_carrier_api(
    snapshot: &ProviderSurfaceSnapshot,
    prop_decl_span: verter_span::Span,
    prop_name: &str,
) -> Option<(u32, u32)> {
    use tower_lsp_server::ls_types::Position;
    use verter_span::LspPosition;

    // The map is parsed from the SAME bytes as `provider_content`; no map ⇒ no
    // identity-keyed lookup is possible ⇒ fail closed.
    let mapper = snapshot.source_map.as_ref()?;

    // The prop's `.vue` decl span is a file-absolute `.vue` byte span (the same
    // span analysis hands to `location_from_span`). Convert its START to a `.vue`
    // UTF-16 position — the source map's source column space.
    let vue_pos = snapshot
        .carrier_utf16_line_index
        .offset_to_position(prop_decl_span.start)?;

    // Map the `.vue` decl-span start INTO the API content (source → generated).
    // This is the inverse of the merge's API→`.vue` hop and lands on the API
    // position the generator wrote the prop name at (strict in-run lookup; a `.vue`
    // position the map does not cover returns `None` ⇒ fail closed).
    let api_pos = mapper
        .carrier_to_tsx(LspPosition::new(vue_pos.line, vue_pos.character))?
        .pos;

    // The API UTF-16 position → API byte offset (the `RenameLocation` coordinate
    // space, encoding-neutral bytes the merge re-derives positions from).
    let start = snapshot
        .provider_utf16_line_index
        .position_to_offset(&Position {
            line: api_pos.line,
            character: api_pos.character,
        })?;
    // The generator writes the name verbatim, so the API byte length equals the
    // name's byte length. (`end` is exclusive.)
    let end = start + prop_name.len() as u32;

    // Correctness tripwire — fail closed, NOT the lookup mechanism. The lookup is
    // the structured-offset hop above; this only VALIDATES that the resolved range
    // actually spells the prop name in the API content. A mismatch (mis-ranged
    // mapping, or a caller name that does not match what the resolved range spells)
    // must never emit an edit that could corrupt the `.vue` → fail closed.
    let slice = snapshot
        .provider_content
        .get(start as usize..end as usize)?;
    if slice != prop_name {
        return None;
    }

    Some((start, end))
}

/// Classify a returned carrier PUBLIC-API path into the fail-closed 3-state
/// [`ApiSurfaceResolution`](crate::type_provider::merge::ApiSurfaceResolution) — the
/// SINGLE authority the cross-file rename merge routes on. The production rename
/// closure is a thin adapter over this; pinning the policy here keeps the decision
/// testable without a live provider and prevents a second, divergent classifier.
///
/// ZERO-LIVE-READ INVARIANT (the class-closing property): every decision is read
/// from the CAPTURED snapshot ([`ProviderQuerySnapshot`]) pinned at the rename
/// fence — NEVER the live store. Between [`ProviderSurfaceStore::capture_current_carrier_api_set`]
/// returning and the merge finishing there is ZERO read of mutable store state
/// (`is_known_virtual_surface`, `captured_snapshot_still_honored`, `current_snapshot`,
/// `snapshot_at`, any `lifecycle`/`snapshots` access). This closes the third TOCTOU:
/// a path `Closing` at capture, returned by the provider, then `finalize_close`d by a
/// background close driver (which does NOT hold the rename fence) BEFORE classify
/// could previously consult the now-cleared live store and mis-classify `NotVirtual`
/// → edit a same-named real file with virtual offsets → corruption. The captured
/// snapshot is the exact generation the provider's offsets were produced against and
/// IS the merge authority; a legitimate background re-sync between capture and
/// classify must NOT change how those pinned offsets map, so re-validating against
/// live state would be both a live read AND wrong.
///
/// (The merge's `carrier_source_exists` / `source_reader` host-VFS reads for the
/// `NotVirtual` real-file branch read a genuinely-real on-disk file AFTER this
/// decision — they are not reads of mutable store state and are out of scope.)
///
/// The decision, over the CAPTURED state for `api_path`:
///
/// 1. **Captured [`CapturedPathState::Current`] (a `CarrierApi` surface at capture),
///    context builds** → `Vouched(ctx)`: map the API-surface offsets onto the `.vue`
///    through THAT captured generation's own source map.
/// 2. **Captured [`CapturedPathState::Current`] but no context (no source map)** →
///    `VirtualDrop` (fail closed).
/// 3. **Captured [`CapturedPathState::KnownNonMappable`]** (was `Closing`, or a
///    non-`CarrierApi`/snapshot-less `Current` at capture) → `VirtualDrop`. The store
///    knew the path as virtual; its offsets index VIRTUAL content, so it must NEVER
///    fall through to the real-file branch.
/// 4. **ABSENT from the capture** → a genuinely real file (a hand-written
///    `Child.vue.ts` next to `Child.vue`) the store did not know as virtual at
///    capture: `NotVirtual` (edit it in place).
#[must_use]
pub fn classify_captured_api_surface(
    captured: &ProviderQuerySnapshot,
    api_path: &str,
    negotiated_encoding: tower_lsp_server::ls_types::PositionEncodingKind,
) -> crate::type_provider::merge::ApiSurfaceResolution {
    use crate::type_provider::merge::ApiSurfaceResolution;

    match captured.captured_state_for(api_path) {
        // Mappable captured surface: build the context from THE CAPTURED snapshot
        // (its own provider/carrier indexes + source map). A snapshot with no source
        // map fails closed.
        Some(CapturedPathState::Current(snapshot)) => {
            match external_ide_context_from_snapshot(snapshot, negotiated_encoding) {
                Some(ctx) => ApiSurfaceResolution::Vouched(ctx),
                None => ApiSurfaceResolution::VirtualDrop,
            }
        }
        // Known-virtual-but-not-mappable at capture (e.g. Closing): drop, never
        // edit a same-named real file with virtual offsets.
        Some(CapturedPathState::KnownNonMappable) => ApiSurfaceResolution::VirtualDrop,
        // Absent from the capture: the store did not know it as virtual → a real
        // on-disk file, edit in place.
        None => ApiSurfaceResolution::NotVirtual,
    }
}

/// Resolve the carrier `.vue` source for `canonical_id`, working for OPEN and
/// CLOSED carriers alike.
///
/// Prefers the open editor buffer (the authoritative in-memory edit state — an
/// unsaved edit differs from the on-disk file, and the rename maps INTO this
/// source) when a `DocumentRegistry` is supplied and the carrier is open; falls
/// back to the host/VFS source, which is the workspace authority for a CLOSED
/// carrier. This is the closed-carrier resolution the design mandates: the
/// carrier `.vue` source is captured WITHOUT requiring an open document.
#[must_use]
pub fn resolve_carrier_source(
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
) -> Option<Arc<str>> {
    if let Some(documents) = documents {
        if let Some(uri) = documents.canonical_id_to_uri(canonical_id) {
            if let Some(doc) = documents.get(&uri) {
                return Some(doc.source.clone());
            }
        }
    }
    host.get_source(canonical_id)
}

/// THE single record choke point every API-surface sync site funnels through.
///
/// Captures an immutable [`ProviderSurfaceSnapshot`] of the `{carrier}.ts` API
/// surface just synced to the provider — the EXACT `api_code` (the provider's
/// offsets index it), its source map (parsed from the SAME bytes), and the
/// carrier `.vue` source (open buffer or host/VFS — works closed) — under a
/// fresh generation. A cross-file rename later interprets the provider's offsets
/// against this precise generation.
///
/// Pass `documents = Some(..)` from the live/server/coordinator paths (so an
/// open carrier's unsaved buffer wins); pass `None` from the host-only
/// background workspace scanner. Routing every sync path through this one helper
/// makes completeness STRUCTURAL: a sync site that records is correct by calling
/// this; the only way to miss a generation is to not call it — auditable by a
/// single grep over call sites.
pub fn record_carrier_api_surface(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    provider_path: &str,
    api_code: &str,
    source_map_json: Option<&str>,
) {
    // The legacy `CarrierApi`-only record site does not stamp a companion version,
    // so the recorded generation is intentionally discarded here.
    let _ = record_carrier_companion_surface(
        store,
        documents,
        host,
        canonical_id,
        provider_path,
        ProviderSurfaceKind::CarrierApi,
        api_code,
        source_map_json,
    );
}

/// Whether the live source for `canonical_id` still byte-matches the captured
/// surface's carrier source — the by-canonical source-identity half of the
/// request-snapshot validation used by the background diagnostics paths (the
/// server-side handlers use the uri-keyed
/// `VerterLanguageServer::request_surface_matches_live_source`).
///
/// The OPEN document buffer is the authority: a closed document (no uri, or a
/// registry miss) does NOT match — the background diagnostics paths publish
/// for open files only, so a mid-flight close retires the context (fail
/// closed).
#[must_use]
pub fn surface_matches_open_document_source(
    documents: &DocumentRegistry,
    canonical_id: &str,
    snapshot: &ProviderSurfaceSnapshot,
) -> bool {
    let Some(uri) = documents.canonical_id_to_uri(canonical_id) else {
        return false;
    };
    let Some(doc) = documents.get(&uri) else {
        return false;
    };
    ContentHash::of(&doc.source) == snapshot.source_hash
}

/// Capture the recorded `CarrierIde` request surface for `canonical_id` — the
/// by-canonical capture the background diagnostics paths (debounced
/// coordinator, post-init/post-scan publishers) build their provider query
/// from, mirroring the server-side `capture_provider_request_surface`
/// fail-closed gates:
/// - the committed sync state must hold a LIVE (`ide_background_loaded`) IDE
///   path (the key lookup only — the snapshot is the content/mapper
///   authority);
/// - the store must hold a CURRENT `CarrierIde` snapshot at that path;
/// - the snapshot must belong to THIS canonical;
/// - the OPEN document source must byte-match the captured carrier source.
#[must_use]
pub fn capture_committed_carrier_ide_surface(
    store: &ProviderSurfaceStore,
    provider_sync_states: &DashMap<String, crate::provider_sync::ProviderSyncState>,
    documents: &DocumentRegistry,
    canonical_id: &str,
) -> Option<Arc<ProviderSurfaceSnapshot>> {
    let ide_path = provider_sync_states.get(canonical_id).and_then(|state| {
        state
            .ide_background_loaded
            .then(|| state.ide_path.clone())
            .flatten()
    })?;
    let snapshot = store.current_snapshot(&ide_path)?;
    if snapshot.kind != ProviderSurfaceKind::CarrierIde {
        return None;
    }
    if snapshot.source_canonical.as_ref() != canonical_id {
        return None;
    }
    surface_matches_open_document_source(documents, canonical_id, &snapshot).then_some(snapshot)
}

/// Capture the recorded `Shadow` request surface for a self-file rune module —
/// the by-canonical Shadow analogue of
/// [`capture_committed_carrier_ide_surface`]: the committed state must hold
/// the module's own path as a live shadow buffer, the store must hold a
/// CURRENT `Shadow` snapshot at that path for THIS canonical, and the OPEN
/// document source must byte-match the captured carrier source.
#[must_use]
pub fn capture_committed_shadow_surface(
    store: &ProviderSurfaceStore,
    provider_sync_states: &DashMap<String, crate::provider_sync::ProviderSyncState>,
    documents: &DocumentRegistry,
    canonical_id: &str,
) -> Option<Arc<ProviderSurfaceSnapshot>> {
    let committed = provider_sync_states.get(canonical_id).is_some_and(|state| {
        state.shadow_background_loaded && state.shadow_path.as_deref() == Some(canonical_id)
    });
    if !committed {
        return None;
    }
    let snapshot = store.current_snapshot(canonical_id)?;
    if snapshot.kind != ProviderSurfaceKind::Shadow {
        return None;
    }
    if snapshot.source_canonical.as_ref() != canonical_id {
        return None;
    }
    surface_matches_open_document_source(documents, canonical_id, &snapshot).then_some(snapshot)
}

/// Post-await validation for a by-canonical captured surface: still honored by
/// the store AND the open document still byte-matches. `false` ⇒ the provider
/// response was produced against a surface that no longer matches the live
/// state — the caller must DROP the provider contribution (fail closed).
#[must_use]
pub fn captured_surface_still_valid_for_canonical(
    store: &ProviderSurfaceStore,
    documents: &DocumentRegistry,
    canonical_id: &str,
    snapshot: &ProviderSurfaceSnapshot,
) -> bool {
    store.captured_snapshot_still_honored(snapshot)
        && surface_matches_open_document_source(documents, canonical_id, snapshot)
}

/// THE free-function record choke point for a DIRECT IDE-surface sync outside
/// the server (the coordinator / background-drain / workspace-scanner direct-
/// open paths; the tsserver publish path records through
/// [`record_and_version_carrier_companions`] inside the carrier-sync gateway,
/// and the server-side interactive paths through
/// `VerterLanguageServer::record_carrier_ide_snapshot`).
///
/// Records a fresh generation pinning the EXACT `ide_code` synced under
/// `provider_path`, with the source map parsed from the SAME bytes. Called ONLY
/// after a SUCCESSFUL provider sync (fail-closed: a failed sync records
/// nothing). Without this record the interactive request-surface capture has
/// no `CarrierIde` snapshot to serve, and every provider-backed feature drops
/// its provider contribution for the synced file.
pub fn record_carrier_ide_surface(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    provider_path: &str,
    ide_code: &str,
    source_map_json: Option<&str>,
) {
    // These direct-open sites do not stamp a companion version, so the recorded
    // generation is intentionally discarded here.
    let _ = record_carrier_companion_surface(
        store,
        documents,
        host,
        canonical_id,
        provider_path,
        ProviderSurfaceKind::CarrierIde,
        ide_code,
        source_map_json,
    );
}

/// Record a published carrier companion surface of ANY role (`CarrierIde` /
/// `CarrierApi` / …) under a fresh generation — the role-generalised core of
/// [`record_carrier_api_surface`]. Stamps the source-map identity (`map_hash`)
/// from the SAME JSON; the owner columns (project owner, regen key, engine-recheck
/// state) are left UNSET — the owner-bearing `record_carrier_surface` path that
/// would set them has no live producer and stays reserved until the §2.7
/// producer-wiring follow-on.
///
/// This is the publish-time choke the IDE role MUST flow through: recording only
/// the API role pins the IDE companion's `getScriptVersion` at the `unwrap_or(1)`
/// fallback, so tsserver retains a STALE `SourceFile` across `.vue` edits (the
/// engine uses `getScriptVersion` as its content-invalidation contract). A surface
/// whose carrier source is unavailable is NOT recorded (fail-closed) and returns
/// `None`; otherwise returns the EXACT generation [`ProviderSurfaceStore::record`]
/// linearized for THIS capture under its lifecycle lock. Callers stamp the
/// companion version from this returned value rather than a second
/// [`ProviderSurfaceStore::current_snapshot`] read — a concurrent close racing
/// between the record and the re-read could return `None` (pinning the version to a
/// stale `1` fallback) or a sibling capture's generation.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn record_carrier_companion_surface(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    provider_path: &str,
    kind: ProviderSurfaceKind,
    code: &str,
    source_map_json: Option<&str>,
) -> Option<u64> {
    let carrier_source = resolve_carrier_source(documents, host, canonical_id)?;
    let map_hash = source_map_json.map(hash16_of_str).unwrap_or([0u8; 16]);
    let source_map = source_map_json
        .and_then(|json| PositionMapper::from_json(json).ok())
        .map(ProviderPositionMapper::source_map);
    let mut surface = RecordSurface::carrier_legacy(
        kind,
        provider_path.to_string(),
        canonical_id.to_string(),
        Arc::from(code),
        source_map,
        carrier_source,
    );
    // Stamp the source-map identity (§2.7) so a map-only change is a distinct
    // capture. The remaining project-bound columns (project owner, regen key,
    // engine-recheck state) are left UNSET here: this live publish path records
    // carrier companions through the working `RecordSurface::carrier_legacy`
    // (`project_owner: None`, regen key and engine-recheck state `None`). The
    // owner-bearing `record_carrier_surface` producer and the owner-gated consumers
    // have no live producer yet, so nothing sets those columns downstream — wiring
    // them is a tracked cross-crate §2.7 producer-wiring follow-on (the surface-store
    // carrier-ownership deferral recorded in
    // docs/arch/external-ts-engine-architecture.md).
    surface.map_hash = map_hash;
    // `record` returns the immutable snapshot it linearized under the lifecycle
    // lock — `stamp.generation` is the authoritative version for THIS capture.
    Some(store.record(surface).stamp.generation)
}

/// Record EVERY published carrier companion's surface through the store at publish
/// time (so each role's generation — the plugin's `getScriptVersion` — advances on
/// content change) and stamp each companion's `version` from its freshly-recorded
/// generation. THE single publish-time carrier recording+versioning path: it
/// replaces the per-site "record the API surface only, then read
/// `generation-or-1`" logic that left the IDE companion's surface unrecorded and
/// its version pinned at `1` (a stale-diagnostics defect, since the live tsserver
/// backend invalidates on `getScriptVersion`). Records each companion exactly once
/// (no double-record) under its role's [`ProviderSurfaceKind`].
pub fn record_and_version_carrier_companions(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    companions: &mut [crate::external_ts::CarrierCompanion],
) {
    use verter_session::external_ts::SnapshotRole;
    for companion in companions.iter_mut() {
        let kind = match companion.role {
            SnapshotRole::CarrierIde => ProviderSurfaceKind::CarrierIde,
            SnapshotRole::CarrierApi => ProviderSurfaceKind::CarrierApi,
            SnapshotRole::Shadow => ProviderSurfaceKind::Shadow,
            SnapshotRole::Real => ProviderSurfaceKind::Real,
        };
        // Stamp the version from the generation `record` linearized for THIS
        // capture (returned directly), NOT a second `current_snapshot` read: the
        // re-read could race a concurrent close to `None` (pinning the IDE
        // companion at the `1` fail-safe) or to a sibling capture's generation. The
        // `1` fail-safe survives only when the carrier source was unavailable
        // (record skipped, returns `None`).
        companion.version = record_carrier_companion_surface(
            store,
            documents,
            host,
            canonical_id,
            &companion.provider_uri,
            kind,
            &companion.content,
            companion.map_json.as_deref(),
        )
        .unwrap_or(1);
    }
}

/// Hash a string into a [`Hash16`] (the env-hash representation the contract
/// uses). blake3 over the bytes, truncated to 16 — consistent with the store's
/// [`ContentHash`] identity. Used to stamp the source-map identity (`map_hash`)
/// from the map JSON.
#[must_use]
fn hash16_of_str(s: &str) -> Hash16 {
    let digest = blake3::hash(s.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest.as_bytes()[..16]);
    out
}

/// Record an API-surface snapshot when only the synced `api_code` is in scope
/// (no source map at hand). Fetches the live `get_public_api()` source map and
/// attaches it ONLY when the live code byte-matches `api_code`, so the snapshot
/// never pairs the synced offsets with a map produced against drifted content.
pub fn record_carrier_api_surface_code_only(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    canonical_id: &str,
    provider_path: &str,
    api_code: &str,
) {
    let owned_map: Option<Arc<str>> = host
        .get_public_api(canonical_id)
        .filter(|api| &*api.code == api_code)
        .and_then(|api| api.source_map.clone());
    record_carrier_api_surface(
        store,
        documents,
        host,
        canonical_id,
        provider_path,
        api_code,
        owned_map.as_deref(),
    );
}

/// Inputs to [`record_carrier_surface`] — a published carrier surface of ANY role
/// with its full owner columns. RESERVED: the owner-bearing producer that would
/// build one per file in a snapshot has no live producer (unwired); it lands with
/// the §2.7 producer-wiring follow-on (the surface-store carrier-ownership deferral).
pub struct PublishedCarrierSurface<'a> {
    pub provider_path: &'a str,
    pub kind: ProviderSurfaceKind,
    pub source_canonical: &'a str,
    pub content: Arc<str>,
    /// The `CodeTransform` source-map JSON. The recorded `map_hash` is DERIVED
    /// from this same JSON inside the record choke (it is NOT a caller-supplied
    /// field), so the stamped map identity and the stored mapper can never
    /// disagree: a surface whose map JSON is absent or fails to parse gets a
    /// `[0; 16]` map identity AND no stored mapper, and
    /// [`ProviderSurfaceStore::current_map_hash`] then fails closed for it.
    pub source_map_json: Option<&'a str>,
    /// The owning configured project (tsconfig URI).
    pub project_owner: Arc<str>,
    /// The self-content regeneration key (§2.7(a)).
    pub regen_key: RegenKey,
    /// The dependency-driven engine-recheck state (§2.7(b)).
    pub engine_recheck: EngineRecheckState,
}

/// Record a published carrier surface of ANY role (`CarrierIde`
/// / `Shadow` / `Real`, or `CarrierApi`) under a fresh generation, with the full
/// owner columns (project owner, `map_hash`, regen key, engine-recheck state).
/// RESERVED owner-bearing choke point: it has NO live producer yet — the env dims +
/// dependency/recheck state it needs are unwired, so nothing in production calls it;
/// it is wired by the §2.7 producer-wiring follow-on (the surface-store
/// carrier-ownership deferral). The working live path records carrier companions
/// through [`record_carrier_companion_surface`] / `RecordSurface::carrier_legacy`
/// with the owner columns unset.
///
/// The carrier source captured for the mapped-into target is the surface's own
/// canonical source (open buffer or host/VFS). A surface whose carrier source is
/// unavailable is NOT recorded (a returned offset then fails closed), mirroring
/// [`record_carrier_api_surface`].
pub fn record_carrier_surface(
    store: &ProviderSurfaceStore,
    documents: Option<&DocumentRegistry>,
    host: &VerterHost,
    surface: PublishedCarrierSurface<'_>,
) {
    let Some(carrier_source) = resolve_carrier_source(documents, host, surface.source_canonical)
    else {
        return;
    };
    // Derive the map identity and the stored mapper from the SAME JSON, so they
    // can never disagree. Only a map that actually PARSES contributes a non-zero
    // `map_hash` AND a stored mapper; an absent/unparseable map yields `[0; 16]`
    // and no mapper, and `current_map_hash` fails closed for it (§2.7 fail-closed
    // map identity).
    let parsed_mapper = surface
        .source_map_json
        .and_then(|json| PositionMapper::from_json(json).ok());
    let map_hash = match (surface.source_map_json, &parsed_mapper) {
        (Some(json), Some(_)) => hash16_of_str(json),
        // No JSON, or JSON that failed to parse ⇒ no usable map identity.
        _ => [0u8; 16],
    };
    let source_map = parsed_mapper.map(ProviderPositionMapper::source_map);
    store.record(RecordSurface {
        provider_path: surface.provider_path.to_string(),
        kind: surface.kind,
        source_canonical: surface.source_canonical.to_string(),
        provider_content: surface.content,
        source_map,
        carrier_source,
        map_hash,
        project_owner: Some(surface.project_owner),
        regen_key: Some(surface.regen_key),
        engine_recheck: Some(surface.engine_recheck),
    });
}
