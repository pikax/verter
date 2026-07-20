//! Discriminating tests for the generation-stamped provider-surface store — the
//! fail-closed authority behind cross-file rename mapping.

use std::sync::Arc;

use super::*;

const VPATH: &str = "/src/Child.vue.ts";
const CANONICAL: &str = "/src/Child.vue";

fn record_surface(provider_content: &str, carrier_source: &str) -> RecordSurface {
    RecordSurface::carrier_api_legacy(
        VPATH.to_string(),
        CANONICAL.to_string(),
        Arc::from(provider_content),
        None,
        Arc::from(carrier_source),
    )
}

#[test]
fn unsynced_path_has_no_current_snapshot() {
    // A path that was never recorded resolves to None (fail closed) — so a real
    // on-disk file, or an as-yet-unsynced surface, never vouches a virtual map.
    let store = ProviderSurfaceStore::new();
    assert!(store.current_snapshot(VPATH).is_none());
    assert!(!store.is_tracked(VPATH));
}

#[test]
fn record_makes_current_and_returns_stamped_snapshot() {
    let store = ProviderSurfaceStore::new();
    let snap = store.record(record_surface("api v1\n", "<script setup>...</script>\n"));

    assert_eq!(&*snap.stamp.provider_path, VPATH);
    assert_eq!(&*snap.source_canonical, CANONICAL);
    assert_eq!(snap.kind, ProviderSurfaceKind::CarrierApi);
    assert!(store.is_tracked(VPATH));

    let current = store.current_snapshot(VPATH).expect("current snapshot");
    assert_eq!(current.stamp, snap.stamp);
}

#[test]
fn each_record_advances_generation() {
    let store = ProviderSurfaceStore::new();
    let a = store.record(record_surface("api v1\n", "carrier v1\n"));
    let b = store.record(record_surface("api v2\n", "carrier v2\n"));
    assert_ne!(
        a.stamp.generation, b.stamp.generation,
        "each record must mint a fresh generation"
    );
    assert!(b.stamp.generation > a.stamp.generation);
}

/// DESIGN TEST (a): a generation-A capture still maps through A after generation
/// B is synced. The historical snapshot for A's exact stamp is preserved and
/// distinct from B; `snapshot_at(A)` returns A's content, not B's.
#[test]
fn generation_a_capture_survives_generation_b_sync() {
    let store = ProviderSurfaceStore::new();

    let a = store.record(record_surface("API GEN A\n", "carrier A\n"));
    let gen_a = a.stamp.generation;
    // Capture A as an in-flight request would.
    let captured_a = store.current_snapshot(VPATH).expect("A is current");
    assert_eq!(captured_a.stamp.generation, gen_a);

    // Generation B is synced under the SAME path.
    let b = store.record(record_surface("API GEN B\n", "carrier B\n"));
    assert_ne!(b.stamp.generation, gen_a);

    // The pinned request looks up A by its exact stamp → still A's content.
    let mapped_a = store
        .snapshot_at(VPATH, gen_a)
        .expect("generation A must still be retrievable after B");
    assert_eq!(&*mapped_a.provider_content, "API GEN A\n");
    assert_eq!(&*mapped_a.carrier_source, "carrier A\n");
    assert_eq!(mapped_a.stamp, captured_a.stamp);

    // And the current snapshot is now B (not A).
    assert_eq!(
        store.current_snapshot(VPATH).unwrap().stamp.generation,
        b.stamp.generation
    );
}

/// H5 (byte-identical re-sync must not false-drop): a background re-sync mints a
/// FRESH generation even when the content is byte-IDENTICAL. A raw
/// generation-equality re-check would then DROP a legitimate in-flight rename
/// (the captured generation no longer equals the current one) even though the
/// current content — and therefore its source map — is identical to what the
/// captured offsets were produced against. `captured_snapshot_still_honored`
/// honors the captured snapshot when the current content hash matches, so the
/// rename still maps.
///
/// Discriminating: it captures generation A, records a byte-identical generation
/// B (advancing the generation but NOT the content hash), and asserts the captured
/// A is STILL honored. A raw-generation re-check returns false here (generations
/// differ) → the assertion fails; the content-hash-aware check returns true.
#[test]
fn byte_identical_resync_still_honors_captured_snapshot() {
    let store = ProviderSurfaceStore::new();

    let provider = "API IDENTICAL\n";
    let carrier = "carrier identical\n";
    let a = store.record(record_surface(provider, carrier));
    // Capture A as an in-flight rename would.
    let captured_a = store.current_snapshot(VPATH).expect("A is current");
    assert_eq!(captured_a.stamp.generation, a.stamp.generation);

    // A background re-sync records BYTE-IDENTICAL content → fresh generation, SAME
    // content hash.
    let b = store.record(record_surface(provider, carrier));
    assert_ne!(
        b.stamp.generation, a.stamp.generation,
        "the re-sync must mint a fresh generation even for identical content"
    );
    assert_eq!(
        b.stamp.content_hash, a.stamp.content_hash,
        "byte-identical content must hash identically"
    );

    // The captured A is STILL honored — the content (and its source map) is the
    // same, so mapping through A is correct despite the generation bump.
    assert!(
        store.captured_snapshot_still_honored(&captured_a),
        "a byte-identical background re-sync must NOT false-drop a captured snapshot — \
         the content hash matches, so the captured source map is still valid"
    );
}

/// H5 fail-closed guard: a re-sync that changes the content (fresh generation AND
/// a DIFFERENT content hash) is NOT honored — the captured offsets may index
/// content the new source map does not describe, so the rename must DROP. This
/// pins that the content-hash relaxation does NOT weaken the fail-closed
/// invariant: only byte-identical re-syncs are honored across a generation bump.
#[test]
fn content_changing_resync_is_not_honored_fail_closed() {
    let store = ProviderSurfaceStore::new();

    let a = store.record(record_surface("API GEN A\n", "carrier A\n"));
    let captured_a = store.current_snapshot(VPATH).expect("A is current");

    // A re-sync with DIFFERENT content → fresh generation AND different content hash.
    let b = store.record(record_surface("API GEN B (changed)\n", "carrier B\n"));
    assert_ne!(b.stamp.generation, a.stamp.generation);
    assert_ne!(
        b.stamp.content_hash, a.stamp.content_hash,
        "changed content must hash differently"
    );

    assert!(
        !store.captured_snapshot_still_honored(&captured_a),
        "a content-CHANGING re-sync must NOT honor the captured snapshot — fail closed (drop) \
         because the captured offsets may not match the new content's source map"
    );

    // And after a forget (close), the captured snapshot is also no longer honored.
    let _token = store.forget(VPATH);
    assert!(
        !store.captured_snapshot_still_honored(&captured_a),
        "a closed (forgotten) surface has no current snapshot → not honored (fail closed)"
    );
}

/// Map-identity gate on the byte-match honored arm: a re-sync recording
/// byte-IDENTICAL provider content over a byte-IDENTICAL carrier source but a
/// DIFFERENT source map (map-only regeneration) must NOT honor the captured
/// snapshot. The captured offsets would map through the OLD mapper while the
/// provider answered against the NEW mapping — same bytes, different
/// correlation — so the mapped result would be wrong, not stale. The
/// generation-match arm is inherently exact and needs no map compare.
#[test]
fn byte_identical_resync_with_changed_map_identity_is_not_honored() {
    let store = ProviderSurfaceStore::new();

    let provider = "API IDENTICAL\n";
    let carrier = "carrier identical\n";
    let mut surface_a = record_surface(provider, carrier);
    surface_a.map_hash = [1u8; 16];
    let a = store.record(surface_a);
    let captured_a = store.current_snapshot(VPATH).expect("A is current");
    assert_eq!(captured_a.stamp.generation, a.stamp.generation);

    // A map-only re-sync: same provider bytes, same carrier source, CHANGED map.
    let mut surface_b = record_surface(provider, carrier);
    surface_b.map_hash = [2u8; 16];
    let b = store.record(surface_b);
    assert_eq!(
        b.stamp.content_hash, a.stamp.content_hash,
        "byte-identical content must hash identically"
    );
    assert_eq!(
        b.stamp.source_hash, a.stamp.source_hash,
        "byte-identical carrier source must hash identically"
    );
    assert_ne!(
        b.stamp.map_hash, a.stamp.map_hash,
        "the changed source map must stamp a different map identity"
    );

    assert!(
        !store.captured_snapshot_still_honored(&captured_a),
        "a map-only re-sync must NOT honor the captured snapshot — mapping the \
         provider's response through the superseded mapper would be WRONG"
    );

    // Positive control: an identical-map byte-identical re-sync IS still honored.
    let mut surface_c = record_surface(provider, carrier);
    surface_c.map_hash = [2u8; 16];
    let _c = store.record(surface_c);
    let captured_b = store
        .snapshot_at(VPATH, b.stamp.generation)
        .expect("B retrievable");
    assert!(
        store.captured_snapshot_still_honored(&captured_b),
        "an identical-map byte-identical re-sync must stay honored (no over-drop)"
    );
}

/// DESIGN TEST (b): a generation-A result with only B available → DROP. If the
/// pinned request held a stamp whose generation the store does not have, the
/// lookup returns None. (Here A was never recorded; only B is.)
#[test]
fn unknown_generation_resolves_to_none_drop() {
    let store = ProviderSurfaceStore::new();
    let b = store.record(record_surface("API GEN B\n", "carrier B\n"));

    // A generation that was never recorded for this path → None (fail closed).
    let missing_generation = b.stamp.generation.wrapping_add(999);
    assert!(
        store.snapshot_at(VPATH, missing_generation).is_none(),
        "a generation the store never recorded must resolve to None (drop)"
    );
}

/// DESIGN TEST (c): a CLOSE after request capture does not break the captured
/// snapshot. `forget` retires the active generation but preserves history, so a
/// previously captured stamp still maps.
#[test]
fn close_after_capture_preserves_captured_snapshot() {
    let store = ProviderSurfaceStore::new();
    let a = store.record(record_surface("API GEN A\n", "carrier A\n"));
    let gen_a = a.stamp.generation;
    let _captured = store.current_snapshot(VPATH).expect("A current");

    // Close / drop the surface.
    let _token = store.forget(VPATH);
    assert!(
        !store.is_tracked(VPATH),
        "forget retires the active generation"
    );
    assert!(
        store.current_snapshot(VPATH).is_none(),
        "no current snapshot after forget"
    );

    // The in-flight request's captured generation still resolves.
    let mapped = store
        .snapshot_at(VPATH, gen_a)
        .expect("the captured generation must survive a close");
    assert_eq!(&*mapped.provider_content, "API GEN A\n");
}

#[test]
fn forget_then_record_uses_a_fresh_generation_not_the_retired_one() {
    // After a close the next record must not reuse the retired generation number
    // — otherwise a stale captured stamp could collide with new content.
    let store = ProviderSurfaceStore::new();
    let a = store.record(record_surface("api A\n", "carrier A\n"));
    let _token = store.forget(VPATH);
    let c = store.record(record_surface("api C\n", "carrier C\n"));

    assert!(
        c.stamp.generation > a.stamp.generation,
        "a record after forget must mint a strictly newer generation"
    );
    // The retired generation still points at A's content, never C's.
    assert_eq!(
        &*store
            .snapshot_at(VPATH, a.stamp.generation)
            .unwrap()
            .provider_content,
        "api A\n"
    );
}

#[test]
fn snapshot_captures_content_hashes_and_utf16_indexes() {
    let store = ProviderSurfaceStore::new();
    let provider = "declare const Child: {}\n";
    let carrier = "<script setup lang=\"ts\">\nconst foo = 1;\n</script>\n";
    let snap = store.record(record_surface(provider, carrier));

    assert_eq!(snap.stamp.content_hash, ContentHash::of(provider));
    assert_eq!(snap.source_hash, ContentHash::of(carrier));
    // The UTF-16 carrier index resolves the line:col of a known offset.
    let off = carrier.find("foo").unwrap() as u32;
    let pos = snap
        .carrier_utf16_line_index
        .offset_to_position(off)
        .unwrap();
    assert_eq!(pos.line, 1);
}

/// DESIGN TEST (d) — closed-carrier resolution: the carrier `.vue` source is
/// resolved from the host/VFS WITHOUT a `DocumentRegistry` open document. A
/// snapshot recorded for a CLOSED carrier therefore captures the on-host source,
/// so the cross-file rename maps onto it without any live document.
#[test]
fn record_for_closed_carrier_captures_host_source_without_documents() {
    use std::sync::Arc as StdArc;
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());
    let carrier_src = "<script setup lang=\"ts\">\ndefineProps<{ foo: string }>();\n</script>\n";
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: StdArc::from(carrier_src),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    // CLOSED carrier: pass `documents = None` so resolution can ONLY come from
    // host/VFS — the exact closed-child path the design requires.
    let store = ProviderSurfaceStore::new();
    record_carrier_api_surface(
        &store,
        None,
        &host,
        CANONICAL,
        VPATH,
        "declare const Child: { new(props?: { foo: string }): {} }\n",
        None,
    );

    let snap = store
        .current_snapshot(VPATH)
        .expect("a closed-carrier API surface must record a snapshot from host/VFS");
    assert_eq!(
        &*snap.carrier_source, carrier_src,
        "the snapshot must capture the host/VFS carrier source for the CLOSED carrier"
    );
    assert_eq!(snap.source_hash, ContentHash::of(carrier_src));
}

/// The publish-time choke records EVERY carrier role (IDE + API) so each
/// companion's `version` — the plugin's `getScriptVersion` — STRICTLY ADVANCES on
/// content change. The defect this guards: the IDE companion's surface was never
/// recorded (only the API was), so its version was pinned at `1` and tsserver
/// retained a stale `SourceFile` across `.vue` edits (split-brain with the
/// advancing API version). RED before the fix: the IDE version stays `1` across a
/// content change; the API version advances independently.
#[test]
fn carrier_companions_record_every_role_and_advance_version_on_content_change() {
    use std::sync::Arc as StdArc;
    use verter_session::external_ts::{ScriptKind, SnapshotRole};
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

    use crate::external_ts::CarrierCompanion;

    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: StdArc::from("<script setup lang=\"ts\">\nconst n: number = 1;\n</script>\n"),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });
    let store = ProviderSurfaceStore::new();

    let ide_path = "/src/Child.vue.tsx";
    let api_path = "/src/Child.vue.verter.ts";
    let make = |ide_code: &str, api_code: &str| {
        vec![
            CarrierCompanion {
                provider_uri: StdArc::from(ide_path),
                content: StdArc::from(ide_code),
                map_json: None,
                role: SnapshotRole::CarrierIde,
                script_kind: ScriptKind::Tsx,
                version: 0,
            },
            CarrierCompanion {
                provider_uri: StdArc::from(api_path),
                content: StdArc::from(api_code),
                map_json: None,
                role: SnapshotRole::CarrierApi,
                script_kind: ScriptKind::Ts,
                version: 0,
            },
        ]
    };

    let mut v1 = make(
        "export default {}; /* ide v1 */\n",
        "declare const C: {}; /* api v1 */\n",
    );
    record_and_version_carrier_companions(&store, None, &host, CANONICAL, &mut v1);
    let (ide_v1, api_v1) = (v1[0].version, v1[1].version);

    let mut v2 = make(
        "export default {}; /* ide v2 CHANGED */\n",
        "declare const C: {}; /* api v2 CHANGED */\n",
    );
    record_and_version_carrier_companions(&store, None, &host, CANONICAL, &mut v2);
    let (ide_v2, api_v2) = (v2[0].version, v2[1].version);

    assert!(
        ide_v2 > ide_v1,
        "the IDE carrier companion version (getScriptVersion) MUST strictly advance on a \
         content change — it was pinned at 1 because the IDE surface was never recorded; \
         got ide_v1={ide_v1} ide_v2={ide_v2}"
    );
    assert!(
        api_v2 > api_v1,
        "the API carrier companion version must STILL advance independently (no \
         split-brain / regression); got api_v1={api_v1} api_v2={api_v2}"
    );
    assert_ne!(
        ide_v1, 1,
        "the IDE companion must carry a real recorded generation, not the stale `1` fallback"
    );
    assert_ne!(
        ide_v1, api_v1,
        "within ONE record_and_version call the IDE and API companions are distinct \
         captures and MUST receive distinct generations (no cross-stamping); \
         ide_v1={ide_v1} api_v1={api_v1}"
    );
}

/// M6: `record_carrier_companion_surface` returns the EXACT generation
/// [`ProviderSurfaceStore::record`] linearized for the capture, so the batch
/// versioner stamps from that value rather than a second `current_snapshot` read
/// (which a concurrent close could race to `None`/stale). RED before M6: the
/// function returned `()` (no generation to source the version from — callers
/// re-read `current_snapshot`, the close-race / cross-stamp hazard).
#[test]
fn record_carrier_companion_surface_returns_linearized_record_generation() {
    use std::sync::Arc as StdArc;
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: StdArc::from("<script setup lang=\"ts\">\nconst n: number = 1;\n</script>\n"),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });
    let store = ProviderSurfaceStore::new();
    let ide_path = "/src/Child.vue.tsx";

    let g1 = record_carrier_companion_surface(
        &store,
        None,
        &host,
        CANONICAL,
        ide_path,
        ProviderSurfaceKind::CarrierIde,
        "export default {}; /* v1 */\n",
        None,
    );
    let g2 = record_carrier_companion_surface(
        &store,
        None,
        &host,
        CANONICAL,
        ide_path,
        ProviderSurfaceKind::CarrierIde,
        "export default {}; /* v2 CHANGED */\n",
        None,
    );

    assert!(
        g1.is_some() && g2.is_some(),
        "a recorded companion returns its linearized generation; got g1={g1:?} g2={g2:?}"
    );
    assert!(
        g2 > g1,
        "each record returns the generation it linearized — strictly monotonic across \
         a content change; got g1={g1:?} g2={g2:?}"
    );
    assert_eq!(
        g2,
        store
            .current_snapshot(ide_path)
            .map(|snapshot| snapshot.stamp.generation),
        "the returned generation MUST equal the generation `record` linearized (the \
         version is sourced from record(), not a second current_snapshot lookup)"
    );

    // A carrier with no host/VFS source records nothing and returns None — the
    // ONLY case the batch versioner falls to its `1` fail-safe.
    let missing = record_carrier_companion_surface(
        &store,
        None,
        &host,
        "/src/Never.vue",
        "/src/Never.vue.tsx",
        ProviderSurfaceKind::CarrierIde,
        "export default {}\n",
        None,
    );
    assert_eq!(
        missing, None,
        "no carrier source ⇒ no record ⇒ None (fail-closed), so only this case uses \
         the `1` fallback"
    );
}

/// A carrier with NO host/VFS source (deleted / never loaded) records nothing —
/// the path has no current generation, so a returned offset fails closed.
#[test]
fn record_for_missing_carrier_source_records_nothing() {
    use verter_session::{HostConfig, VerterHost};
    let host = VerterHost::new_standalone(HostConfig::default());
    let store = ProviderSurfaceStore::new();
    record_carrier_api_surface(
        &store,
        None,
        &host,
        CANONICAL,
        VPATH,
        "declare const X: {}\n",
        None,
    );
    assert!(
        store.current_snapshot(VPATH).is_none(),
        "no carrier source ⇒ no snapshot recorded (fail closed)"
    );
}

/// A failed public-API projection is not an absent map. Recording the supplied
/// code after that failure would mint a new provider generation for a surface
/// the host explicitly refused to publish.
#[test]
fn code_only_projection_failure_does_not_advance_surface_generation() {
    use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

    let host = VerterHost::new_standalone(HostConfig::default());
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(
                r#"<script setup lang="ts">
enum Unsafe { Value = Math.random() }
defineProps<{ value: Unsafe }>()
</script>"#,
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert unsafe enum fixture");

    let store = ProviderSurfaceStore::new();
    let before = store.record(record_surface("last known good\n", "unsafe carrier\n"));
    record_carrier_api_surface_code_only(
        &store,
        None,
        &host,
        CANONICAL,
        VPATH,
        "must not publish\n",
    );

    let after = store
        .current_snapshot(VPATH)
        .expect("prior snapshot survives");
    assert_eq!(after.stamp.generation, before.stamp.generation);
    assert_eq!(&*after.provider_content, "last known good\n");
}

/// Build a snapshot whose source map maps the API `foo` to the carrier `foo`,
/// then assert `external_ide_context_from_snapshot` produces a context that maps
/// correctly under the given negotiated encoding. Covers DESIGN TEST (d) — the
/// snapshot (carrier source captured, NO live document) drives the mapping — and
/// (f) — UTF-16 lookup re-emitted across UTF-8 / UTF-16 / UTF-32.
fn assert_snapshot_context_maps_for_encoding(
    negotiated: tower_lsp_server::ls_types::PositionEncodingKind,
) {
    use crate::type_provider::merge::api_surface_range_to_carrier_range;
    use crate::type_provider::protocol::RenameLocation;

    // Carrier line 1 begins with a multibyte identifier so the UTF-8/UTF-16/UTF-32
    // columns differ — the encoding boundary is exercised, not incidental.
    let carrier =
        "<script setup lang=\"ts\">\nconst café = defineProps<{ foo: string }>();\n</script>\n";
    let line1 = carrier.lines().nth(1).unwrap();
    let foo_byte_in_line = line1.find("foo").unwrap() as u32;
    let want_col = if negotiated == tower_lsp_server::ls_types::PositionEncodingKind::UTF8 {
        foo_byte_in_line
    } else if negotiated == tower_lsp_server::ls_types::PositionEncodingKind::UTF32 {
        line1[..foo_byte_in_line as usize].chars().count() as u32
    } else {
        line1[..foo_byte_in_line as usize]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum()
    };
    let want_utf16_col: u32 = line1[..foo_byte_in_line as usize]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();

    let api = "declare const Child: { new(props?: { foo: string }): {} }\n";
    let api_foo = api.find("foo").unwrap() as u32;
    let (api_line, api_col) = {
        let before = &api[..api_foo as usize];
        let line = before.matches('\n').count() as u32;
        let col = api_foo - before.rfind('\n').map(|i| i as u32 + 1).unwrap_or(0);
        (line, col)
    };
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Child.vue", carrier);
    builder.add_token(api_line, api_col, 1, want_utf16_col, Some(source_id), None);
    let source_map_json = builder.into_sourcemap().to_json_string();

    let store = ProviderSurfaceStore::new();
    let snap = store.record(RecordSurface::carrier_api_legacy(
        VPATH.to_string(),
        CANONICAL.to_string(),
        Arc::from(api),
        Some(
            crate::documents::provider_projection::ProviderPositionMapper::source_map(
                crate::documents::position_map::PositionMapper::from_json(&source_map_json)
                    .unwrap(),
            ),
        ),
        Arc::from(carrier),
    ));

    let ctx = external_ide_context_from_snapshot(&snap, negotiated.clone())
        .expect("a snapshot with a source map must build a context");

    // Map the API `foo` location through the snapshot-derived context.
    let loc = RenameLocation {
        path: VPATH.to_string(),
        start: api_foo,
        end: api_foo + 3,
    };
    let range = api_surface_range_to_carrier_range(
        loc.start,
        loc.end,
        &ctx.tsx_line_index,
        &ctx.mapper,
        &ctx.carrier_line_index,
        ctx.carrier_negotiated_line_index.as_ref().unwrap(),
    )
    .expect("the snapshot context must map the API offset onto the carrier");

    assert_eq!(
        range.start.line, 1,
        "mapped to carrier prop line 1 ({negotiated:?})"
    );
    assert_eq!(
        range.start.character, want_col,
        "edit column must match the {negotiated:?} column ({want_col})"
    );
}

#[test]
fn snapshot_context_maps_closed_carrier_utf16() {
    assert_snapshot_context_maps_for_encoding(
        tower_lsp_server::ls_types::PositionEncodingKind::UTF16,
    );
}

#[test]
fn snapshot_context_maps_closed_carrier_utf8() {
    assert_snapshot_context_maps_for_encoding(
        tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
    );
}

#[test]
fn snapshot_context_maps_closed_carrier_utf32() {
    assert_snapshot_context_maps_for_encoding(
        tower_lsp_server::ls_types::PositionEncodingKind::UTF32,
    );
}

#[test]
fn snapshot_without_source_map_builds_no_context_fail_closed() {
    let store = ProviderSurfaceStore::new();
    let snap = store.record(record_surface("api\n", "carrier\n")); // source_map: None
    assert!(
        external_ide_context_from_snapshot(
            &snap,
            tower_lsp_server::ls_types::PositionEncodingKind::UTF16
        )
        .is_none(),
        "a snapshot with no source map must fail closed (no context)"
    );
}

/// A1 (carrier-source identity in the honor oracle): the provider `{child}.vue.ts`
/// text can be byte-IDENTICAL across two generations while the child `.vue` carrier
/// source CHANGED (e.g. a comment inserted before `<script setup>`, or template text
/// edited — shifts `.vue` byte offsets while leaving the lifted `$props` public-API
/// text identical). The captured snapshot's source map describes the OLD `.vue`; if
/// the honor oracle accepted on provider `content_hash` ALONE it would map a returned
/// range through the OLD map and apply it to the NEW live `.vue` → wrong range →
/// corruption. The honor oracle must ALSO require the carrier `source_hash` to match.
///
/// Discriminating: provider content is byte-identical across gen A and gen B
/// (`content_hash` matches), but the carrier source differs (`source_hash` differs).
/// The pre-fix oracle (content_hash-only) returns `true` here → the assertion
/// `!honored` FAILS. The carrier-source-aware oracle returns `false` → fail closed.
#[test]
fn byte_identical_provider_but_changed_carrier_is_not_honored_fail_closed() {
    let store = ProviderSurfaceStore::new();

    // Gen A: a given provider API surface over carrier A.
    let a = store.record(record_surface("API IDENTICAL\n", "carrier A\n"));
    let captured_a = store.current_snapshot(VPATH).expect("A is current");
    assert_eq!(captured_a.stamp.generation, a.stamp.generation);

    // Gen B: the SAME provider content (the lifted public API is byte-identical) but a
    // DIFFERENT carrier source (a comment/template edit shifted the `.vue` bytes).
    let b = store.record(record_surface("API IDENTICAL\n", "carrier B (changed)\n"));
    assert_ne!(
        b.stamp.generation, a.stamp.generation,
        "the re-sync mints a fresh generation"
    );
    assert_eq!(
        a.stamp.content_hash, b.stamp.content_hash,
        "the provider API content is byte-identical across the two generations"
    );
    assert_ne!(
        a.stamp.source_hash, b.stamp.source_hash,
        "the carrier `.vue` source CHANGED across the two generations"
    );

    // FAIL CLOSED: the captured A is NOT honored — the current carrier source differs,
    // so A's source map (source side = OLD carrier) would mis-map onto the NEW live `.vue`.
    assert!(
        !store.captured_snapshot_still_honored(&captured_a),
        "a byte-identical provider surface over a CHANGED carrier source must NOT honor the \
         captured snapshot — the carrier-source identity differs, so mapping through A's old \
         source map would corrupt the new `.vue` (fail closed)"
    );
}

/// A2 (tombstone retire): retiring a `CarrierApi` path keeps the store positively aware
/// that the path IS/WAS a virtual API surface (a tombstone) until a SUCCESSFUL provider
/// close finalizes it. This is the absent-but-known-virtual signal the rename resolver
/// consults: a path the store knows as virtual but is absent from the in-flight capture
/// must classify `VirtualDrop`, never `NotVirtual` (which would edit a same-named real
/// file with virtual offsets when a provider close failed to retire tsserver).
///
/// Discriminating: the new `is_known_virtual_surface` / `finalize_close` APIs do not exist
/// pre-fix (compile-fail = fails pre-fix); post-fix the lifecycle holds: recorded ⇒ known +
/// current Some; retired ⇒ current None BUT still known (tombstone); finalized ⇒ not known.
#[test]
fn tombstone_retire_marks_path_known_virtual_until_finalize() {
    let store = ProviderSurfaceStore::new();

    // Recorded: a live virtual surface — known + current.
    store.record(record_surface("declare const Child: {}\n", "carrier A\n"));
    assert!(
        store.is_known_virtual_surface(VPATH),
        "a recorded CarrierApi surface is a known virtual surface"
    );
    assert!(store.current_snapshot(VPATH).is_some());

    // Retired (close started): no current snapshot, but the Closing state keeps it KNOWN as
    // a virtual surface so a captured-miss classifies VirtualDrop while the close is unsafe.
    let close_token = store.forget(VPATH);
    assert!(
        store.current_snapshot(VPATH).is_none(),
        "retire removes the current snapshot"
    );
    assert!(
        store.is_known_virtual_surface(VPATH),
        "a retired-but-not-finalized path stays a KNOWN virtual surface (Closing) — the \
         provider close has not been confirmed, so the path must keep classifying VirtualDrop"
    );

    // Finalized (provider close confirmed Ok): finalizing with THIS close's token clears the
    // Closing state — the path is no longer known as a virtual surface, so a genuinely real
    // same-named file classifies NotVirtual.
    assert!(
        store.finalize_close(close_token),
        "finalizing the matching-epoch close must clear the Closing state"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "after a confirmed close the Closing state clears: the path is no longer a known \
         virtual surface"
    );
}

/// A2 companion: a path that was NEVER recorded (no snapshot, no tombstone) is NOT a known
/// virtual surface — so a genuinely real on-disk `Child.vue.ts` next to `Child.vue` keeps
/// classifying `NotVirtual` (edit in place), never spuriously `VirtualDrop`.
#[test]
fn unknown_path_is_not_known_virtual_surface() {
    let store = ProviderSurfaceStore::new();
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "a never-recorded path is not a known virtual surface (it is a real file)"
    );
}

/// A2 routing discriminator (capture-state-driven): the 3-state classification policy
/// [`classify_captured_api_surface`] routes by the CAPTURED per-path state — never a live
/// store read. A path that was `Closing` AT CAPTURE is captured as `KnownNonMappable` →
/// `VirtualDrop` (fail closed); a path never recorded is ABSENT from the capture →
/// `NotVirtual` (edit in place). This is the exact decision the production rename closure
/// makes — pinned here without a live tsserver, AND without consulting the live store.
///
/// Discriminating: pre-fix the classifier did not exist AND the resolver returned
/// `NotVirtual` for every captured-miss (it could not tell a tombstoned virtual surface from
/// a real file). Post-fix the Closing-at-capture path routes `VirtualDrop` (via the captured
/// `KnownNonMappable` state), the never-recorded path `NotVirtual` — for the SAME
/// captured-miss shape, with classify reading ONLY the captured snapshot.
#[test]
fn classify_captured_miss_routes_known_virtual_to_drop_and_unknown_to_not_virtual() {
    use crate::type_provider::merge::ApiSurfaceResolution;
    use tower_lsp_server::ls_types::PositionEncodingKind;

    let store = ProviderSurfaceStore::new();
    // Record then retire VPATH so it is Closing at capture; the capture records it as
    // KnownNonMappable (no MAPPABLE snapshot, so `snapshot_for` is None).
    store.record(record_surface("declare const Child: {}\n", "carrier A\n"));
    let _token = store.forget(VPATH);
    let captured = store.capture_current_carrier_api_set();
    assert!(
        captured.snapshot_for(VPATH).is_none(),
        "VPATH has no MAPPABLE snapshot at capture (Closing → KnownNonMappable)"
    );

    // A Closing-at-capture virtual surface → captured KnownNonMappable → VirtualDrop (NEVER
    // edit a real same-named file). Classify reads ONLY the captured snapshot now (no `store`).
    let known = classify_captured_api_surface(&captured, VPATH, PositionEncodingKind::UTF16);
    assert!(
        matches!(known, ApiSurfaceResolution::VirtualDrop),
        "a captured-miss path the store KNOWS as a virtual surface (tombstone) must route \
         VirtualDrop, not NotVirtual"
    );

    // A genuinely unknown path (never recorded) → absent from the capture → NotVirtual (edit
    // its own real file).
    let unknown_path = "/src/Unknown.vue.ts";
    let unknown =
        classify_captured_api_surface(&captured, unknown_path, PositionEncodingKind::UTF16);
    assert!(
        matches!(unknown, ApiSurfaceResolution::NotVirtual),
        "a captured-miss path the store does NOT know as virtual must route NotVirtual"
    );
}

/// A2 NIT (record clears a prior tombstone): a re-synced surface is LIVE, not
/// tombstoned. After a `forget` (which tombstones the path) a subsequent `record`
/// must clear the tombstone AND make the path current — so the path is honored as a
/// current virtual surface, not merely known-via-tombstone. The re-review noted this
/// was only covered transitively; this pins it directly.
///
/// Discriminating, on the tombstone SET TRANSITION ITSELF (independent of
/// `finalize_close`): after `forget` the path is tombstoned (`is_tombstoned` true,
/// `is_tracked` false); after the re-sync `record` the tombstone is CLEARED
/// (`is_tombstoned` false) while the path is current (`is_tracked` true,
/// `current_snapshot` Some). A `record` that did NOT clear the tombstone leaves
/// `is_tombstoned(VPATH)` true after the re-sync → the `!store.is_tombstoned(VPATH)`
/// assertion FAILS. This is the load-bearing discriminator: it does NOT rely on
/// `finalize_close` (which clears BOTH maps) to drive the path unknown, so it would
/// catch a regression that stops `record` clearing the tombstone — the prior version,
/// which only checked `!is_known_virtual_surface` after `finalize_close`, would not.
#[test]
fn record_clears_prior_tombstone() {
    let store = ProviderSurfaceStore::new();

    // Record then forget → the path is Closing (known via Closing, not current).
    store.record(record_surface("api A\n", "carrier A\n"));
    let close_token = store.forget(VPATH);
    assert!(store.is_tombstoned(VPATH), "forget marks the path Closing");
    assert!(!store.is_tracked(VPATH), "forget retires the current entry");
    assert!(
        store.current_snapshot(VPATH).is_none(),
        "forget retires current"
    );

    // Re-sync: record must CLEAR the tombstone and make the path current again. This
    // is the discriminating assertion — it observes the tombstone SET directly and
    // does NOT depend on `finalize_close` to drive the path unknown.
    store.record(record_surface("api B\n", "carrier B\n"));
    assert!(
        !store.is_tombstoned(VPATH),
        "record must CLEAR the prior tombstone — a re-synced surface is current, not \
         tombstoned (a record that left the tombstone would fail here)"
    );
    assert!(
        store.is_tracked(VPATH),
        "a re-synced surface is CURRENT (tracked), not merely tombstoned"
    );
    assert!(
        store.current_snapshot(VPATH).is_some(),
        "a re-synced surface has a current snapshot"
    );

    // Additional (non-load-bearing) sanity: the re-sync (record → Current) cleared the prior
    // Closing, so the OLD close token no longer matches the path's state — finalizing with it
    // is an EPOCH-SCOPED no-op that must NOT erase the reopened Current surface.
    assert!(
        !store.finalize_close(close_token),
        "the stale (pre-reopen) close token must no-op against the reopened Current state"
    );
    assert!(
        store.is_known_virtual_surface(VPATH),
        "the stale finalize did not erase the reopened surface — it is still a known virtual \
         surface"
    );
    assert!(
        store.is_tracked(VPATH),
        "the reopened surface stays Current after the stale finalize no-op"
    );
    // A PROPER close (fresh forget → fresh token → finalize) then drives the reopened path
    // unknown, confirming the lifecycle still terminates correctly.
    let fresh_token = store.forget(VPATH);
    assert!(store.finalize_close(fresh_token));
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "a fresh-token close after the reopen leaves the path unknown"
    );
}

/// CONCURRENCY (known→known transitions): `is_known_virtual_surface` must observe `true`
/// at EVERY instant for a path that is VIRTUAL throughout — `forget` (Current→Closing)
/// and `record` (Closing→Current) each replace ONE present lifecycle state with another
/// under the single lifecycle lock, and the reader must never catch a transient where the
/// path is absent from the map.
///
/// WHY THIS MATTERS: the production rename resolver routes a captured-MISS path to
/// `VirtualDrop` ONLY if `is_known_virtual_surface(path)` is `true`; a transient `false`
/// would route it to `NotVirtual` → the merge edits a same-named REAL `{carrier}.ts` on
/// disk with VIRTUAL offsets → corruption. Under the single lifecycle map this is closed
/// STRUCTURALLY: a known→known transition is one `paths.insert` overwriting an existing
/// entry under the write lock, so a concurrent reader holding the read lock sees the
/// path present before and after — never absent.
///
/// Discriminating: N reader threads hammer `is_known_virtual_surface(VPATH)` in a tight
/// loop while the main thread alternates `forget(VPATH)`/`record(VPATH, ...)` M times.
/// The path is virtual at EVERY instant of every transition (Current→Closing→Current).
/// Under the single lifecycle lock the read is atomic, so the false-sighting count must
/// be 0; a non-zero count would mean a logic error in the state transitions (a path
/// transiently removed instead of overwritten).
#[test]
fn concurrent_reader_never_sees_unknown_during_known_to_known_transitions() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc as StdArc;

    let store = ProviderSurfaceStore::new();
    // Seed the path as a known virtual surface so it is virtual at the START.
    store.record(record_surface("api seed\n", "carrier seed\n"));
    assert!(store.is_known_virtual_surface(VPATH));

    const READERS: usize = 4;
    const TRANSITIONS: usize = 50_000;

    let stop = StdArc::new(AtomicBool::new(false));
    let false_sightings = StdArc::new(AtomicUsize::new(0));

    let mut readers = Vec::with_capacity(READERS);
    for _ in 0..READERS {
        let store = store.clone();
        let stop = StdArc::clone(&stop);
        let false_sightings = StdArc::clone(&false_sightings);
        readers.push(std::thread::spawn(move || {
            // Tight read loop: the path is KNOWN at every instant, so any `false`
            // sighting is the in-neither-set transient (the TOCTOU defect).
            while !stop.load(Ordering::Relaxed) {
                if !store.is_known_virtual_surface(VPATH) {
                    false_sightings.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    // Drive known→known transitions: forget (Current→Closing) then record
    // (Closing→Current). The path is a known virtual surface throughout.
    for i in 0..TRANSITIONS {
        let _t = store.forget(VPATH);
        store.record(record_surface("api hot\n", "carrier hot\n"));
        // Re-seed assurance every so often is unnecessary — record already makes it
        // current — but keep the loop variable used to avoid an unused warning path.
        let _ = i;
    }

    stop.store(true, Ordering::Relaxed);
    for r in readers {
        r.join().expect("reader thread panicked");
    }

    assert_eq!(
        false_sightings.load(Ordering::Relaxed),
        0,
        "a path that is a known virtual surface through every Current<->Closing \
         transition must NEVER be observed unknown — under the single lifecycle lock the \
         read is atomic, so a non-zero count would mean a logic error in state transitions \
         (a path transiently removed instead of overwritten), which would route a \
         captured-miss rename to NotVirtual and corrupt a real same-named file"
    );

    // Post-state sanity: the path is current after the final record.
    assert!(store.is_known_virtual_surface(VPATH));
    assert!(store.current_snapshot(VPATH).is_some());
}

#[test]
fn distinct_paths_are_independent() {
    let store = ProviderSurfaceStore::new();
    let other = "/src/Other.vue.ts";
    store.record(record_surface("api child\n", "carrier child\n"));
    store.record(RecordSurface::carrier_api_legacy(
        other.to_string(),
        "/src/Other.vue".to_string(),
        Arc::from("api other\n"),
        None,
        Arc::from("carrier other\n"),
    ));

    assert!(store.is_tracked(VPATH));
    assert!(store.is_tracked(other));
    let _token = store.forget(VPATH);
    assert!(!store.is_tracked(VPATH));
    assert!(
        store.is_tracked(other),
        "forgetting one path must not affect another"
    );
}

/// THE named-bug regression (epoch-scoped close finalization — the load-bearing
/// discriminator). A `record(P)` REOPEN that lands in the await window of an OLDER
/// close must NOT have its fresh snapshot erased by that older close's stale
/// `finalize_close`.
///
/// Bug timeline (pre-fix):
/// ```text
/// record(P)@genA   -> Current{genA}
/// forget(P)        -> Closing{epoch1}   [token1 captured; close driver 1 begins await]
/// record(P)@genB   -> Current{genB}     [REOPEN during the await]
/// finalize_close(token1) -> pre-fix UNCONDITIONALLY removed current[P] => ERASES genB
/// => is_known_virtual_surface(P) == false => captured-miss routes NotVirtual => corruption.
/// ```
/// The pre-fix `finalize_close(path)` did `current.remove(path) + tombstones.remove(path)`
/// with NO generation/epoch check, so genB would be erased → `current_snapshot` None →
/// `is_known_virtual_surface` false. Post-fix `finalize_close(token1)` sees the path is
/// `Current{genB}` (not `Closing{epoch1}`), so it is an EPOCH-SCOPED no-op and genB
/// SURVIVES.
///
/// MECHANICAL red→green proof (recorded in the implementer report): TEMPORARILY revert
/// the `finalize_close` body to the pre-fix unconditional `paths.remove(&token.provider_path)`
/// (ignoring the epoch) → THIS test FAILS (genB erased). Restore the epoch check → THIS
/// test PASSES (genB survives).
#[test]
fn reopen_during_older_close_await_survives_stale_finalize() {
    let store = ProviderSurfaceStore::new();

    // genA recorded, then a close begins (token1 owns epoch1).
    let a = store.record(record_surface("API GEN A\n", "carrier A\n"));
    let gen_a = a.stamp.generation;
    let token1 = store.forget(VPATH);
    assert!(
        store.is_tombstoned(VPATH),
        "after forget the path is Closing (close in flight)"
    );

    // REOPEN during the older close's await window: genB mints a fresh Current.
    let b = store.record(record_surface("API GEN B\n", "carrier B\n"));
    let gen_b = b.stamp.generation;
    assert!(
        gen_b > gen_a,
        "the reopen mints a strictly newer generation"
    );
    assert!(
        store.is_tracked(VPATH),
        "the reopen makes the path Current again (overwrites the Closing state)"
    );

    // The OLDER close confirms and finalizes with its stale token — this MUST be a
    // no-op against the reopened Current{genB} (the core fix).
    assert!(
        !store.finalize_close(token1),
        "the stale finalize (epoch1) must NOT clear the reopened Current(genB) — it returns \
         false (no-op)"
    );

    // genB SURVIVES: still current, still known, still tracked.
    let current = store
        .current_snapshot(VPATH)
        .expect("the reopened genB snapshot must survive the stale finalize");
    assert_eq!(
        current.stamp.generation, gen_b,
        "the surviving current snapshot is genB (the reopen), not erased"
    );
    assert_eq!(
        &*current.provider_content, "API GEN B\n",
        "the surviving snapshot carries genB's content"
    );
    assert!(
        store.is_known_virtual_surface(VPATH),
        "the path is NOT erased to unknown/NotVirtual — it stays a known virtual surface"
    );
    assert!(
        store.is_tracked(VPATH),
        "the path is Current (the reopened genB), not Closing"
    );
}

/// Normal close still clears (no reopen) — the epoch check does not break the
/// happy path. `record(P) → forget(P)[token] → finalize_close(token)` ⇒ fully
/// unknown.
#[test]
fn normal_close_without_reopen_still_clears() {
    let store = ProviderSurfaceStore::new();
    store.record(record_surface("api\n", "carrier\n"));
    let token = store.forget(VPATH);
    // No reopen: the path is still Closing under the token's epoch, so the matching
    // finalize clears it.
    assert!(
        store.finalize_close(token),
        "a matching-epoch finalize on an un-reopened Closing path clears it"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "after a normal confirmed close the path is fully unknown"
    );
    assert!(store.current_snapshot(VPATH).is_none());
    assert!(!store.is_tracked(VPATH));
    assert!(!store.is_tombstoned(VPATH));
}

/// THE duplicate-close leak regression (idempotent `forget` — the load-bearing
/// discriminator). A DUPLICATE close of an already-retired surface (no intervening
/// `record`) where the NEWER duplicate-close ERRORS (its token is dropped) and the
/// OLDER duplicate-close SUCCEEDS must still TERMINATE the lifecycle — the path
/// must end fully unknown, never stuck `Closing` forever.
///
/// Leak timeline (PRE-fix, when `forget` always minted a fresh epoch):
/// ```text
/// record(P)        -> Current{gen0}
/// forget(P) [A]    -> Closing{epoch1}, token_a   [driver A retires]
/// forget(P) [B]    -> OVERWRITES Closing{epoch2}, token_b   [driver B retires the SAME
///                     already-retired surface; NO intervening record]
/// drop(token_b)    -> driver B's close ERRORS: token dropped, state stays Closing{epoch2}
/// finalize_close(token_a=epoch1) -> state is Closing{epoch2}, epoch1 != epoch2 -> NO-OP
/// => no owner remains => P stuck Closing (known-virtual) FOREVER
/// => a genuinely real same-named file routes VirtualDrop forever (lifecycle-termination leak).
/// ```
/// POST-fix `forget` is IDEMPOTENT for an already-`Closing` path: driver B's
/// `forget` sees `Closing{epoch1}` and REUSES epoch1 (no fresh mint, no overwrite),
/// so `token_a` and `token_b` carry the SAME epoch. When B errors and A confirms,
/// `finalize_close(token_a=epoch1)` matches the live `Closing{epoch1}` and CLEARS it
/// → the path terminates unknown, no orphan.
///
/// MECHANICAL red→green proof (recorded in the implementer report): against the
/// CURRENT `forget` (unconditional mint), the second `forget` mints epoch2, so
/// `finalize_close(token_a=epoch1)` no-ops (returns false) and the path stays stuck
/// known-virtual → the `assert!(store.finalize_close(token_a))` FAILS. After the
/// idempotent fix, the second `forget` reuses epoch1, the finalize clears, and the
/// path is unknown → PASS.
#[test]
fn duplicate_close_newer_errors_older_finalizes_terminates_no_leak() {
    let store = ProviderSurfaceStore::new();
    store.record(record_surface("api\n", "carrier\n"));

    // Driver A retires the surface.
    let token_a = store.forget(VPATH);
    // Driver B retires the SAME already-retired surface — NO intervening `record`.
    // With the idempotent fix this REUSES A's epoch instead of overwriting it.
    let token_b = store.forget(VPATH);
    assert!(
        store.is_tombstoned(VPATH),
        "the path is Closing after the duplicate retires"
    );

    // The NEWER close (driver B) ERRORS: its token is simply DROPPED (never finalized).
    drop(token_b);

    // The OLDER close (driver A) confirms Ok and finalizes. With idempotent `forget`,
    // token_a's epoch still matches the live Closing state, so this CLEARS the path.
    // (Pre-fix the second forget minted a distinct epoch2 → this was a NO-OP (false)
    // and left the path stuck Closing forever — the leak.)
    assert!(
        store.finalize_close(token_a),
        "the older close's finalize MUST clear the path — idempotent forget means the \
         duplicate close shares one epoch, so the surviving token finalizes (pre-fix this \
         no-oped and stranded the path in Closing forever)"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "after the duplicate-close terminates, the path is fully unknown — NOT stuck Closing \
         (which would route a genuinely real same-named file VirtualDrop forever)"
    );
    assert!(
        !store.is_tracked(VPATH),
        "the terminated path is not Current"
    );
    assert!(
        !store.is_tombstoned(VPATH),
        "the terminated path is not Closing — the lifecycle reached fully-unknown"
    );
}

/// Directional companion to the duplicate-close leak regression: the SYMMETRIC
/// sub-case where the OLDER duplicate-close errors and the NEWER one succeeds must
/// ALSO terminate. Because idempotent `forget` makes both duplicate closers share
/// ONE epoch, the finalize is order-insensitive — whichever close confirms `Ok`
/// first clears the path.
#[test]
fn duplicate_close_older_errors_newer_finalizes_terminates_no_leak() {
    let store = ProviderSurfaceStore::new();
    store.record(record_surface("api\n", "carrier\n"));

    let token_a = store.forget(VPATH);
    let token_b = store.forget(VPATH); // reuses token_a's epoch (idempotent)
    assert!(store.is_tombstoned(VPATH), "the path is Closing");

    // The OLDER close ERRORS this time: its token is dropped.
    drop(token_a);

    // The NEWER close confirms Ok — its token shares the same epoch, so it clears.
    assert!(
        store.finalize_close(token_b),
        "the newer close's finalize clears the path — idempotent forget makes the duplicate \
         close share one epoch, so finalization is order-insensitive"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "the symmetric duplicate-close case also terminates fully unknown"
    );
    assert!(!store.is_tracked(VPATH));
    assert!(!store.is_tombstoned(VPATH));
}

/// Overlapping (duplicate) closes share ONE epoch (idempotent `forget`): two close
/// drivers retire the SAME already-retired path (no intervening `record`) before
/// either finalizes. Because `forget` is idempotent for an already-`Closing` path,
/// the second `forget` REUSES the first's epoch, so BOTH tokens carry the same
/// epoch. The FIRST confirmed close therefore CLEARS the path; a subsequent
/// finalize with the other (same-epoch) token is a harmless no-op (the path is
/// already absent).
/// `record(P) → forget(P)[t1] → forget(P)[t2] → finalize_close(t1)` ⇒ unknown;
/// `finalize_close(t2)` ⇒ no-op (already cleared).
///
/// Discriminating on the leak fix: it encodes that duplicate closes share ONE epoch.
/// The PRE-fix code (the second forget minting a DISTINCT epoch2) would leave
/// `finalize_close(t1=epoch1)` a no-op against `Closing{epoch2}` → the
/// `assert!(store.finalize_close(t1))` here FAILS. Only the idempotent fix (shared
/// epoch) makes the first finalize clear.
#[test]
fn overlapping_closes_share_one_epoch_first_finalize_clears() {
    let store = ProviderSurfaceStore::new();
    store.record(record_surface("api\n", "carrier\n"));

    let t1 = store.forget(VPATH); // close driver 1 retires (epoch1)
    let t2 = store.forget(VPATH); // close driver 2 retires the SAME path → REUSES epoch1
    assert!(
        store.is_tombstoned(VPATH),
        "the path is Closing after the overlapping retires"
    );

    // Idempotent forget: both tokens carry epoch1, so the FIRST confirmed close
    // clears the path (NOT a no-op — that was the pre-fix distinct-epoch behavior).
    assert!(
        store.finalize_close(t1),
        "with idempotent forget the duplicate close shares one epoch, so the first \
         finalize CLEARS the Closing state (pre-fix, distinct epochs, this no-oped)"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "after the first (shared-epoch) finalize the path is fully unknown"
    );
    assert!(
        !store.is_tombstoned(VPATH),
        "the path is no longer Closing — it terminated"
    );

    // The OTHER token (same epoch) now finds the path absent → harmless no-op.
    assert!(
        !store.finalize_close(t2),
        "the second same-epoch finalize is a harmless no-op — the path was already cleared"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "the path stays fully unknown after the redundant second finalize"
    );
}

/// Reopen then a NEWER close: a reopen lands during t1's await, then a fresh close
/// (t2) retires the reopened surface. t1 must no-op (the path is Closing@epoch2 by
/// then), and t2 finalizes.
/// `record@genA → forget[t1] → record@genB → forget[t2] → finalize_close(t1)` ⇒
/// no-op (Closing@epoch2); then `finalize_close(t2)` ⇒ unknown.
#[test]
fn reopen_then_newer_close_only_newest_token_finalizes() {
    let store = ProviderSurfaceStore::new();

    let a = store.record(record_surface("API GEN A\n", "carrier A\n"));
    let t1 = store.forget(VPATH); // older close begins (epoch1)
    let b = store.record(record_surface("API GEN B\n", "carrier B\n")); // reopen during await
    assert!(b.stamp.generation > a.stamp.generation);
    assert!(store.is_tracked(VPATH), "the reopen makes the path Current");

    let t2 = store.forget(VPATH); // a NEWER close retires the reopened surface (epoch2)
    assert!(
        store.is_tombstoned(VPATH),
        "the newer close marks the path Closing again (epoch2)"
    );

    // The OLDER close's finalize must no-op — the path is Closing@epoch2 now, not
    // Closing@epoch1 (and it was Current in between).
    assert!(
        !store.finalize_close(t1),
        "the stale (epoch1) finalize must no-op against Closing@epoch2"
    );
    assert!(
        store.is_known_virtual_surface(VPATH),
        "the path is still a known virtual surface (Closing@epoch2) after the stale finalize"
    );
    assert!(
        store.is_tombstoned(VPATH),
        "the path is Closing (the newer close), not Current, not erased"
    );

    // The NEWER close's finalize (matching epoch2) clears it.
    assert!(
        store.finalize_close(t2),
        "the matching (epoch2) finalize clears the Closing state"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "after the owning (newer) close finalizes, the path is fully unknown"
    );
}

/// THE third-TOCTOU regression (captured-miss-during-Closing → finalize race — the
/// load-bearing discriminator for THIS fix). A path that is `Closing` AT CAPTURE
/// (so the OLD capture skipped it → absent from the set) and is then
/// `finalize_close`d by a background close driver BEFORE classify runs MUST still
/// classify `VirtualDrop`, never `NotVirtual`. The fence (`rename_provider_fence`)
/// is held ONLY by `handle_rename`, NOT by the background close drivers, so this
/// interleave is reachable in production: classify a captured-miss path whose
/// virtual lifecycle the live store has since cleared, and the merge would edit a
/// same-named REAL on-disk `{carrier}.ts` with VIRTUAL offsets → SOURCE CORRUPTION.
///
/// Race timeline (the interleave the fence does NOT cover):
/// ```text
/// record(VPATH)                 -> Current{genA} + CarrierApi snapshot
/// forget(VPATH)                 -> Closing{epoch1}   [token captured; bg close begins]
/// capture_current_carrier_api_set()  -> VPATH is Closing at capture
///       PRE-fix: capture SKIPS Closing -> VPATH ABSENT from the set
///       POST-fix: capture records VPATH as KnownNonMappable
/// finalize_close(token)         -> bg close driver confirms Ok -> live store CLEARS VPATH
///                                  (is_known_virtual_surface(VPATH) now == false)
/// classify_captured_api_surface(VPATH)
///       PRE-fix: captured-MISS -> consults LIVE store -> not known virtual -> NotVirtual  ❌
///       POST-fix: reads ONLY the captured snapshot -> KnownNonMappable -> VirtualDrop      ✅
/// ```
///
/// MECHANICAL red→green proof (recorded in the implementer report): against the
/// PRE-fix tree (capture skips `Closing`; classify consults `is_known_virtual_surface`),
/// after `finalize_close` the live store returns `is_known_virtual_surface == false`,
/// so the captured-miss classifies `NotVirtual` → `assert!(matches!(res, VirtualDrop))`
/// FAILS. Post-fix (Closing captured as KnownNonMappable; classify reads only the
/// snapshot) it classifies `VirtualDrop` → PASS.
#[test]
fn captured_miss_during_closing_then_finalize_still_drops_not_not_virtual() {
    use crate::type_provider::merge::ApiSurfaceResolution;
    use tower_lsp_server::ls_types::PositionEncodingKind;

    let store = ProviderSurfaceStore::new();

    // VPATH is a live virtual surface with a CarrierApi snapshot.
    store.record(record_surface("declare const Child: {}\n", "carrier A\n"));
    assert!(store.is_known_virtual_surface(VPATH));

    // A background close begins: VPATH goes Closing (the bg driver owns this token).
    let close_token = store.forget(VPATH);
    assert!(
        store.is_tombstoned(VPATH),
        "after forget VPATH is Closing (close in flight, not yet confirmed)"
    );

    // CAPTURE while VPATH is Closing. Under the FIX this records VPATH as a
    // known-but-not-mappable captured state; under the OLD code it was skipped
    // (absent), forcing classify to consult the live store.
    let captured = store.capture_current_carrier_api_set();
    assert!(
        captured.snapshot_for(VPATH).is_none(),
        "a Closing path has NO mappable snapshot at capture (snapshot_for is None either way)"
    );

    // The background close driver confirms Ok and FINALIZES — AFTER capture, BEFORE
    // classify. This is the interleave the rename fence does not cover (close drivers
    // do not hold it). The live store now no longer knows VPATH as a virtual surface.
    assert!(
        store.finalize_close(close_token),
        "the matching-epoch finalize clears the Closing state"
    );
    assert!(
        !store.is_known_virtual_surface(VPATH),
        "post-finalize the LIVE store no longer knows VPATH as a virtual surface — the exact \
         state that mis-routed a captured-miss to NotVirtual pre-fix"
    );

    // CLASSIFY from the captured snapshot. The captured KnownNonMappable state must
    // drive VirtualDrop (fail closed) WITHOUT consulting the now-cleared live store.
    let res = classify_captured_api_surface(&captured, VPATH, PositionEncodingKind::UTF16);
    assert!(
        matches!(res, ApiSurfaceResolution::VirtualDrop),
        "a path that was Closing at capture and finalized before classify MUST classify \
         VirtualDrop (drop) — NOT NotVirtual, which would edit a same-named REAL {{carrier}}.ts \
         with virtual offsets and corrupt it. Got: not VirtualDrop"
    );
}

/// No-live-read structural discriminator: a path captured `Current` (mappable) must
/// classify identically REGARDLESS of any live-store mutation between capture and
/// classify. Here a DIFFERENT-content generation B is recorded for the SAME path
/// after capture (advancing the live current generation AND content hash); classify
/// of the captured set must STILL map through the captured generation-A snapshot
/// (`Vouched`), proving classification ignores live state.
///
/// Discriminating: the PRE-fix classify called `captured_snapshot_still_honored`,
/// which consulted the LIVE current snapshot — a content-CHANGING re-sync (gen B,
/// different content hash) made it return `false` → `VirtualDrop`, so the assertion
/// `matches!(res, Vouched)` FAILS. POST-fix classify reads only the captured snapshot
/// (which has a source map) → `Vouched`, unaffected by the live gen-B re-sync.
#[test]
fn classify_ignores_live_mutation_after_capture_for_current_path() {
    use crate::type_provider::merge::ApiSurfaceResolution;
    use tower_lsp_server::ls_types::PositionEncodingKind;

    // Build a snapshot WITH a source map so a captured `Current` path can Vouch.
    let carrier =
        "<script setup lang=\"ts\">\nconst foo = defineProps<{ foo: string }>();\n</script>\n";
    let api = "declare const Child: { new(props?: { foo: string }): {} }\n";
    let api_foo = api.find("foo").unwrap() as u32;
    let (api_line, api_col) = {
        let before = &api[..api_foo as usize];
        let line = before.matches('\n').count() as u32;
        let col = api_foo - before.rfind('\n').map(|i| i as u32 + 1).unwrap_or(0);
        (line, col)
    };
    let line1 = carrier.lines().nth(1).unwrap();
    let want_utf16_col: u32 = {
        let foo_byte = line1.find("foo").unwrap();
        line1[..foo_byte]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum()
    };
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Child.vue", carrier);
    builder.add_token(api_line, api_col, 1, want_utf16_col, Some(source_id), None);
    let source_map_json = builder.into_sourcemap().to_json_string();

    let store = ProviderSurfaceStore::new();
    store.record(RecordSurface::carrier_api_legacy(
        VPATH.to_string(),
        CANONICAL.to_string(),
        Arc::from(api),
        Some(
            crate::documents::provider_projection::ProviderPositionMapper::source_map(
                crate::documents::position_map::PositionMapper::from_json(&source_map_json)
                    .unwrap(),
            ),
        ),
        Arc::from(carrier),
    ));

    // Capture generation A (mappable, Current).
    let captured = store.capture_current_carrier_api_set();
    assert!(
        captured.snapshot_for(VPATH).is_some(),
        "VPATH is a Current CarrierApi surface at capture → mappable"
    );

    // LIVE MUTATION after capture: a content-CHANGING re-sync (gen B, different
    // content). Under the PRE-fix honor oracle this would make the captured snapshot
    // NOT honored → VirtualDrop. Classification must IGNORE it.
    store.record(record_surface(
        "declare const Child: { CHANGED: true }\n",
        "carrier B\n",
    ));

    let res = classify_captured_api_surface(&captured, VPATH, PositionEncodingKind::UTF16);
    assert!(
        matches!(res, ApiSurfaceResolution::Vouched(_)),
        "classify must map through the CAPTURED generation-A snapshot regardless of a live \
         content-changing re-sync after capture — it reads ONLY the captured snapshot, never \
         live state. A live re-consultation would have dropped this (VirtualDrop)."
    );
}

// ── locate_prop_decl_range_in_carrier_api ───────────────────────────────────
//
// The ONE missing piece for provider-agnostic cross-file Vue-prop rename: given a
// captured `{carrier}.ts` API snapshot, the prop's `.vue` decl span, and the prop
// name, locate the prop identifier's BYTE RANGE in the API content — keyed by the
// typed `.vue` decl identity through the snapshot's own source map, NOT a text scan.

/// Build a captured API snapshot whose source map maps the API `foo` token back to
/// the `.vue` `foo` declaration — mirroring how the public-API generator emits the
/// prop name via `push_mapped(name, vue_decl_span)`. Returns `(snapshot, carrier,
/// api, vue_foo_decl_span)`.
fn build_foo_prop_snapshot() -> (
    Arc<ProviderSurfaceSnapshot>,
    &'static str,
    &'static str,
    verter_span::Span,
) {
    let carrier =
        "<script setup lang=\"ts\">\ndefineProps<{ foo: string; bar: number }>();\n</script>\n";
    let api = "declare const Child: { new(props?: { foo: string; bar: number }): {} }\n";

    // The `.vue` decl span of `foo` (file-absolute bytes) — the typed identity the
    // analysis layer hands to `location_from_span`.
    let vue_foo_start = carrier.find("foo").unwrap() as u32;
    let vue_foo_span = verter_span::Span {
        start: vue_foo_start,
        end: vue_foo_start + 3,
    };

    // The map token: API `foo` start ↔ `.vue` `foo` start (UTF-16 columns), exactly
    // the `push_mapped(name, span)` shape.
    let api_foo = api.find("foo").unwrap() as u32;
    let (api_line, api_col) = {
        let before = &api[..api_foo as usize];
        let line = before.matches('\n').count() as u32;
        let col = api_foo - before.rfind('\n').map(|i| i as u32 + 1).unwrap_or(0);
        (line, col)
    };
    // `.vue` foo is on line 1 (0-indexed); col is its UTF-16 column on that line.
    let line1 = carrier.lines().nth(1).unwrap();
    let vue_foo_in_line = line1.find("foo").unwrap() as u32;
    let vue_foo_utf16_col: u32 = line1[..vue_foo_in_line as usize]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();

    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Child.vue", carrier);
    builder.add_token(
        api_line,
        api_col,
        1,
        vue_foo_utf16_col,
        Some(source_id),
        None,
    );
    let source_map_json = builder.into_sourcemap().to_json_string();

    let store = ProviderSurfaceStore::new();
    let snap = store.record(RecordSurface::carrier_api_legacy(
        VPATH.to_string(),
        CANONICAL.to_string(),
        Arc::from(api),
        Some(
            crate::documents::provider_projection::ProviderPositionMapper::source_map(
                crate::documents::position_map::PositionMapper::from_json(&source_map_json)
                    .unwrap(),
            ),
        ),
        Arc::from(carrier),
    ));
    (snap, carrier, api, vue_foo_span)
}

#[test]
fn locate_prop_decl_range_returns_exact_api_range_and_maps_back_to_vue() {
    use crate::type_provider::merge::api_surface_range_to_carrier_range;
    use tower_lsp_server::ls_types::PositionEncodingKind;

    let (snap, carrier, api, vue_foo_span) = build_foo_prop_snapshot();

    let (start, end) = locate_prop_decl_range_in_carrier_api(&snap, vue_foo_span, "foo")
        .expect("the locator must find `foo` in the API content via the typed `.vue` span");

    // EXACT: the located byte range must spell `foo` in the API content — discriminating,
    // a wrong range would slice a different substring.
    let expected_start = api.find("foo").unwrap() as u32;
    assert_eq!(
        start, expected_start,
        "located API start must be the prop-name start"
    );
    assert_eq!(
        end,
        expected_start + 3,
        "located API end must be start + name length"
    );
    assert_eq!(&api[start as usize..end as usize], "foo");

    // ROUND-TRIP: feeding the located range back through the SAME merge mapping
    // (the path a provider's real carrier location takes) lands on the `.vue` `foo`
    // declaration. This is exactly why the synthesized + real locations dedup.
    let ctx = external_ide_context_from_snapshot(&snap, PositionEncodingKind::UTF16)
        .expect("snapshot has a source map");
    let range = api_surface_range_to_carrier_range(
        start,
        end,
        &ctx.tsx_line_index,
        &ctx.mapper,
        &ctx.carrier_line_index,
        ctx.carrier_negotiated_line_index.as_ref().unwrap(),
    )
    .expect("the located API range must map back onto the `.vue`");

    // The carrier `.vue` `foo` is on line 1 at its UTF-16 column.
    let line1 = carrier.lines().nth(1).unwrap();
    let vue_foo_in_line = line1.find("foo").unwrap() as u32;
    let vue_foo_col: u32 = line1[..vue_foo_in_line as usize]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();
    assert_eq!(
        range.start.line, 1,
        "round-trip lands on the `.vue` prop line"
    );
    assert_eq!(
        range.start.character, vue_foo_col,
        "round-trip lands on the `.vue` prop column (a wrong API range would mis-land)"
    );
}

#[test]
fn locate_prop_decl_range_fails_closed_without_source_map() {
    // A snapshot with NO source map cannot identity-key the API position → the
    // locator must fail closed (None), never guess a range.
    let store = ProviderSurfaceStore::new();
    let snap = store.record(record_surface(
        "declare const Child: { new(props?: { foo: string }): {} }\n",
        "<script setup lang=\"ts\">\ndefineProps<{ foo: string }>();\n</script>\n",
    ));
    let span = verter_span::Span { start: 40, end: 43 };
    assert!(
        locate_prop_decl_range_in_carrier_api(&snap, span, "foo").is_none(),
        "no source map → fail closed (no synthesized child edit, no usage-only partial)"
    );
}

#[test]
fn locate_prop_decl_range_fails_closed_on_unmapped_or_mismatched_span() {
    let (snap, _carrier, _api, _vue_foo_span) = build_foo_prop_snapshot();

    // A `.vue` span the map does NOT cover (e.g. pointing at unmapped script text)
    // must fail closed — the merge maps only `foo`, so a far-off span resolves to
    // nothing.
    let unmapped = verter_span::Span { start: 0, end: 3 };
    assert!(
        locate_prop_decl_range_in_carrier_api(&snap, unmapped, "foo").is_none(),
        "an unmapped `.vue` span must fail closed (no fabricated API range)"
    );

    // The typed span resolves, but the claimed NAME does not match what the API
    // content spells there → fail closed (the correctness tripwire), so a
    // wrong-identity resolution never emits a corrupting edit.
    let (snap2, _c, _a, vue_foo_span) = build_foo_prop_snapshot();
    assert!(
        locate_prop_decl_range_in_carrier_api(&snap2, vue_foo_span, "barbaz").is_none(),
        "a name mismatch at the resolved range must fail closed"
    );
}

// ──────────────────── extended cache columns + split cache wiring ────────────────────

use crate::carrier_cache::{EngineRecheckState, RegenKey};

fn block2_regen_key(source: u8) -> RegenKey {
    RegenKey {
        source_content_hash: [source; 16],
        parse_env_hash: [0x10; 16],
        compile_profile_hash: 7,
        file_language_row_hash: [0x20; 16],
        helper_runtime_version: 1,
    }
}

fn block2_recheck(import_sig: u8, closure_gen: u64) -> EngineRecheckState {
    block2_recheck_proj(import_sig, closure_gen, 1)
}

fn block2_recheck_proj(import_sig: u8, closure_gen: u64, project_gen: u64) -> EngineRecheckState {
    EngineRecheckState {
        import_signature_hash: [import_sig; 16],
        closure_generation: closure_gen,
        project_recheck_generation: project_gen,
    }
}

fn block2_published(
    provider_path: &str,
    kind: ProviderSurfaceKind,
    content: &str,
    regen: RegenKey,
    recheck: EngineRecheckState,
) -> RecordSurface {
    RecordSurface {
        provider_path: provider_path.to_string(),
        kind,
        source_canonical: CANONICAL.to_string(),
        provider_content: Arc::from(content),
        source_map: None,
        carrier_source: Arc::from("<source>\n"),
        map_hash: [0x42; 16],
        project_owner: Some(Arc::from("/tsconfig.json")),
        regen_key: Some(regen),
        engine_recheck: Some(recheck),
    }
}

#[test]
fn project_owner_column_is_recorded_and_readable() {
    let store = ProviderSurfaceStore::new();
    store.record(block2_published(
        VPATH,
        ProviderSurfaceKind::CarrierIde,
        "ide\n",
        block2_regen_key(0xAA),
        block2_recheck(0x55, 5),
    ));
    assert_eq!(
        store.project_owner_of(VPATH).as_deref(),
        Some("/tsconfig.json"),
        "the project-owner column is recorded and readable from the store"
    );
}

#[test]
fn legacy_carrier_api_record_has_no_project_owner() {
    // The legacy rename-mapping CarrierApi record path leaves project owner unset
    // (the live project-bound publish path sets it). It must not fabricate one.
    let store = ProviderSurfaceStore::new();
    store.record(record_surface("api v1\n", "carrier v1\n"));
    assert!(
        store.project_owner_of(VPATH).is_none(),
        "a legacy CarrierApi record carries no project owner"
    );
}

#[test]
fn reserved_roles_are_recordable_and_round_trip_kind() {
    // CarrierIde / Shadow / Real are now wired into the single store (no second
    // store).
    for (path, kind) in [
        ("/src/A.vue.tsx", ProviderSurfaceKind::CarrierIde),
        ("/src/A.svelte.ts", ProviderSurfaceKind::Shadow),
        ("/src/real.ts", ProviderSurfaceKind::Real),
    ] {
        let store = ProviderSurfaceStore::new();
        store.record(block2_published(
            path,
            kind,
            "content\n",
            block2_regen_key(0xAA),
            block2_recheck(0x55, 5),
        ));
        let snap = store.current_snapshot(path).expect("recorded");
        assert_eq!(
            snap.kind, kind,
            "the reserved role round-trips through the store"
        );
    }
}

#[test]
fn map_hash_is_stamped_and_drives_mapped_result_validity() {
    // A surface WITH a usable source map exposes its stamped map_hash and drives
    // mapped-result validity. (current_map_hash fails closed for a no-map surface;
    // that is covered separately by
    // map_hash_is_none_for_a_surface_without_a_source_map_fail_closed.)
    let store = ProviderSurfaceStore::new();
    let map_json = r#"{"version":3,"sources":["Child.vue"],"names":[],"mappings":"AAAA"}"#;
    let mapper = crate::documents::provider_projection::ProviderPositionMapper::source_map(
        crate::documents::position_map::PositionMapper::from_json(map_json).unwrap(),
    );
    store.record(RecordSurface {
        provider_path: VPATH.to_string(),
        kind: ProviderSurfaceKind::CarrierIde,
        source_canonical: CANONICAL.to_string(),
        provider_content: Arc::from("ide\n"),
        source_map: Some(mapper),
        carrier_source: Arc::from("<source>\n"),
        map_hash: [0x42; 16],
        project_owner: Some(Arc::from("/tsconfig.json")),
        regen_key: Some(block2_regen_key(0xAA)),
        engine_recheck: Some(block2_recheck(0x55, 5)),
    });
    assert_eq!(
        store.current_map_hash(VPATH),
        Some([0x42; 16]),
        "a surface with a usable mapper exposes its stamped map_hash"
    );
    assert!(
        store.mapped_results_valid(VPATH, [0x42; 16]),
        "matching map_hash keeps mapped results"
    );
    assert!(
        !store.mapped_results_valid(VPATH, [0x43; 16]),
        "a map_hash mismatch invalidates mapped results"
    );
}

#[test]
fn store_carrier_regeneration_skip_reuses_byte_stable_carrier() {
    let store = ProviderSurfaceStore::new();
    let regen = block2_regen_key(0xAA);
    store.record(block2_published(
        VPATH,
        ProviderSurfaceKind::CarrierIde,
        "ide\n",
        regen,
        block2_recheck(0x55, 5),
    ));
    // Same self-content env dims ⇒ regeneration-fresh (reuse cached carrier).
    assert!(store.carrier_regeneration_is_fresh(VPATH, &regen));
    // A source-content change ⇒ not fresh (must regenerate).
    let changed = block2_regen_key(0xBB);
    assert!(!store.carrier_regeneration_is_fresh(VPATH, &changed));
}

#[test]
fn store_engine_recheck_fires_on_dependency_change_with_stable_carrier() {
    // The store-level wiring of the dependency-change discriminator: a byte-stable carrier
    // (regen fresh) whose dependency closure generation advanced STILL needs an
    // engine re-check.
    let store = ProviderSurfaceStore::new();
    let regen = block2_regen_key(0xAA);
    store.record(block2_published(
        VPATH,
        ProviderSurfaceKind::CarrierIde,
        "ide\n",
        regen,
        block2_recheck(0x55, 10),
    ));

    // Carrier text is byte-stable.
    assert!(store.carrier_regeneration_is_fresh(VPATH, &regen));

    // Dependency .d.ts changed: same import signature, closure generation +1.
    let live = block2_recheck(0x55, 11);
    assert!(
        store.carrier_needs_engine_recheck(VPATH, &live),
        "a dependency change MUST re-check even with a byte-stable carrier "
    );

    // No change ⇒ no spurious re-check.
    let same = block2_recheck(0x55, 10);
    assert!(!store.carrier_needs_engine_recheck(VPATH, &same));
}

#[test]
fn store_engine_recheck_is_conservative_without_recorded_state() {
    // A legacy record (no recheck state) conservatively re-checks rather than
    // risk a stale result — never suppress an engine re-check the design requires.
    let store = ProviderSurfaceStore::new();
    store.record(record_surface("api v1\n", "carrier v1\n"));
    let live = block2_recheck(0x55, 10);
    assert!(
        store.carrier_needs_engine_recheck(VPATH, &live),
        "no recorded recheck state ⇒ conservatively re-check"
    );
    // An unknown path also conservatively re-checks.
    assert!(store.carrier_needs_engine_recheck("/unknown.vue.tsx", &live));
}

#[test]
fn store_engine_recheck_fires_on_project_config_change_with_stable_carrier_and_deps() {
    // The store-level project/env rail: a byte-stable carrier whose imports and
    // dependency content closure are unchanged, but whose tsconfig/lib/paths env
    // changed (project_recheck_generation advanced), STILL needs an engine
    // re-check. A closure-generation-only state would miss this.
    let store = ProviderSurfaceStore::new();
    let regen = block2_regen_key(0xAA);
    store.record(block2_published(
        VPATH,
        ProviderSurfaceKind::CarrierIde,
        "ide\n",
        regen,
        block2_recheck_proj(0x55, 10, 1),
    ));
    // Same imports, same dependency closure, only the project config rail moved.
    let live = block2_recheck_proj(0x55, 10, 2);
    assert!(
        store.carrier_needs_engine_recheck(VPATH, &live),
        "a tsconfig/lib/paths change MUST re-check even with a byte-stable carrier \
         and unchanged dependency closure"
    );
    // Truly nothing changed ⇒ no re-check.
    assert!(!store.carrier_needs_engine_recheck(VPATH, &block2_recheck_proj(0x55, 10, 1)));
}

#[test]
fn map_hash_is_none_for_a_surface_without_a_source_map_fail_closed() {
    // F5: a surface recorded with NO source map has no usable mapper, so
    // current_map_hash returns None and mapped_results_valid fails closed — never
    // validate a cached mapped result against a missing map.
    let store = ProviderSurfaceStore::new();
    // block2_published records source_map: None.
    store.record(block2_published(
        VPATH,
        ProviderSurfaceKind::CarrierIde,
        "ide\n",
        block2_regen_key(0xAA),
        block2_recheck(0x55, 5),
    ));
    assert!(
        store.current_map_hash(VPATH).is_none(),
        "a surface with no parsed source map has no usable map identity (fail closed)"
    );
    assert!(
        !store.mapped_results_valid(VPATH, [0x42; 16]),
        "mapped_results_valid must fail closed when the current surface has no usable mapper"
    );
}

/// FORK-1 capture wiring: `capture_committed_carrier_ide_surface` must drop a surface
/// whose CURRENT identity no longer matches the receipt-attested committed stamp on the
/// OWNED state — the record-before-publish window that PERSISTS after a failed reconcile.
///
/// Reproduces the bug end-to-end: a committed IDE surface (V1) is capturable; a NEW sync
/// then records a different IDE surface (V2) at the same path but its publish FAILED, so
/// the committed state still attests V1. The store's CURRENT surface is V2, so mapping a
/// provider offset (produced against V1) through it would be wrong — the capture MUST
/// drop.
///
/// DISCRIMINATING: without the stamp gate the second capture reaches
/// `surface_matches_open_document_source` (which passes — the open doc still matches the
/// carrier source) and returns `Some`, so the `is_none()` assertion fails.
#[test]
fn committed_ide_capture_drops_a_newly_recorded_but_uncommitted_surface() {
    use crate::provider_sync::{
        CommittedCarrierIdeSurface, ProviderOwnerBinding, ProviderSyncState,
    };
    use dashmap::DashMap;
    use tower_lsp_server::ls_types::{TextDocumentItem, Uri};

    // A real open carrier document so `surface_matches_open_document_source` passes —
    // isolating the committed-surface stamp gate as the sole reason a capture drops.
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let documents = crate::documents::DocumentRegistry::new(host);
    let uri: Uri = "file:///ws/src/App.vue".parse().unwrap();
    let carrier_src = "<template><div/></template>\n";
    let _ = documents.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "vue".to_string(),
        version: 1,
        text: carrier_src.to_string(),
    });
    let canonical = documents
        .get_canonical_id(&uri)
        .expect("the open carrier has a canonical id");

    let store = ProviderSurfaceStore::new();
    let ide_path = format!("{canonical}.tsx");
    let states: DashMap<String, ProviderSyncState> = DashMap::new();

    // 1. A COMMITTED publish: record the IDE surface (V1) and stamp the OWNED state with
    //    that receipt-attested identity (exactly as `commit_carrier_provider_state` does).
    let v1 = store.record(RecordSurface::carrier_legacy(
        ProviderSurfaceKind::CarrierIde,
        ide_path.clone(),
        canonical.clone(),
        Arc::from("export const __IDE_V1 = 1;\n"),
        None,
        Arc::from(carrier_src),
    ));
    states.insert(
        canonical.clone(),
        ProviderSyncState {
            owner_binding: ProviderOwnerBinding::Owned("/ws/tsconfig.json".to_string()),
            ide_path: Some(ide_path.clone()),
            ide_background_loaded: true,
            committed_ide_surface: Some(CommittedCarrierIdeSurface {
                content_hash: v1.stamp.content_hash.to_hash16(),
                map_hash: v1.stamp.map_hash,
            }),
            ..Default::default()
        },
    );

    // The committed surface (current == committed) IS capturable.
    assert!(
        capture_committed_carrier_ide_surface(&store, &states, &documents, &canonical).is_some(),
        "the committed (published) IDE surface must be capturable"
    );

    // 2. A NEW sync records a DIFFERENT IDE surface (V2) at the same path — the
    //    record-before-publish. Its publish FAILED, so NO commit advanced the state's
    //    stamp (it still attests V1). The store's CURRENT surface is now V2.
    store.record(RecordSurface::carrier_legacy(
        ProviderSurfaceKind::CarrierIde,
        ide_path.clone(),
        canonical.clone(),
        Arc::from("export const __IDE_V2_UNCOMMITTED = 2;\n"),
        None,
        Arc::from(carrier_src),
    ));

    // The current surface (V2) no longer matches the committed stamp (V1) ⇒ fail closed.
    assert!(
        capture_committed_carrier_ide_surface(&store, &states, &documents, &canonical).is_none(),
        "a newly-recorded-but-uncommitted IDE surface must NOT be capturable (fail closed)"
    );
}
