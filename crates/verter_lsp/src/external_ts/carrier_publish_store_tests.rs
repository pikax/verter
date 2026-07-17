//! Discriminating unit tests for the on-disk content-addressed carrier-snapshot
//! store + atomic manifest (§2.2). Each test FAILS if the named invariant is
//! violated; the two-phase / portability / last-good properties are exercised
//! directly against the on-disk artifacts.

use std::sync::Arc;

use verter_session::external_ts::{
    OpenState, PublishSnapshot, ScriptKind, SnapshotFile, SnapshotRole,
};

use super::*;

const HOST_VERSION: &str = "test-host-1.2.3";

/// blake3-16 of a string — the content-addressed identity a `SnapshotFile` carries.
fn h16(s: &str) -> [u8; 16] {
    let d = blake3::hash(s.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&d.as_bytes()[..16]);
    out
}

/// Build a `SnapshotFile` with content-addressed hashes derived from `content`
/// (and `map` for the map hash; `None` ⇒ zero map hash = no source map).
fn file(
    provider_uri: &str,
    source_uri: &str,
    role: SnapshotRole,
    script_kind: ScriptKind,
    content: &str,
    map: Option<&str>,
    version: u64,
) -> SnapshotFile {
    SnapshotFile {
        source_uri: Arc::from(source_uri),
        provider_uri: Arc::from(provider_uri),
        role,
        script_kind,
        content: Arc::from(content),
        content_hash: h16(content),
        map_hash: map.map(h16).unwrap_or([0u8; 16]),
        map_json: map.map(Arc::from),
        version,
        open_state: OpenState::Closed,
    }
}

/// Build a one-file `PublishSnapshot` for a project.
fn snapshot(project: &str, files: Vec<SnapshotFile>) -> PublishSnapshot {
    PublishSnapshot {
        project: Arc::from(project),
        files,
        resolution_map_version: 1,
        fs_generation: 1,
    }
}

/// A store rooted at a UNIQUE temp workspace dir (so tests do not collide on the
/// shared content-addressed temp store). Returns the store and the "user tree"
/// root the workspace hash is computed from.
fn fresh_store() -> (CarrierPublishStore, tempfile::TempDir) {
    let user_tree = tempfile::tempdir().expect("user tree tempdir");
    let store = CarrierPublishStore::open(HOST_VERSION, &user_tree.path().to_string_lossy());
    (store, user_tree)
}

// ── two-phase ordering: every ready_files entry has a blob on disk ──────────

#[test]
fn every_ready_file_has_its_blob_on_disk_after_publish() {
    let (store, _ut) = fresh_store();
    let snap = snapshot(
        "d:/ws/tsconfig.json",
        vec![
            file(
                "d:/ws/src/A.vue.tsx",
                "d:/ws/src/A.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const A = 1;",
                Some("{\"version\":3}"),
                7,
            ),
            file(
                "d:/ws/src/B.vue.verter.ts",
                "d:/ws/src/B.vue",
                SnapshotRole::CarrierApi,
                ScriptKind::Ts,
                "export default class {}",
                None,
                7,
            ),
        ],
    );
    let batch = PublishBatch::from_snapshot(
        _ut.path().to_string_lossy().to_string(),
        snap,
        None,
        OwnedSetScope::ProjectAuthoritative,
    );
    store.publish_batch(&batch).expect("publish");

    let manifest = store.current_manifest();
    let project = manifest
        .projects
        .get("d:/ws/tsconfig.json")
        .expect("project entry");
    assert_eq!(project.ready_files.len(), 2, "both files ready");
    // THE two-phase invariant: every ready_files entry's blob EXISTS on disk, and
    // every advertised map_rel's blob EXISTS on disk too.
    for ready in project.ready_files.values() {
        let blob = store.workspace_dir().join(&ready.blob_rel);
        assert!(
            blob.exists(),
            "ready_files entry advertises a blob that does not exist on disk: {}",
            blob.display()
        );
        if let Some(map_rel) = &ready.map_rel {
            assert!(
                store.workspace_dir().join(map_rel).exists(),
                "ready_files entry advertises a map_rel with no blob on disk: {map_rel}"
            );
        }
    }
    // A.vue.tsx carried a source map → its map_rel must be present + on disk.
    let a = project
        .ready_files
        .get("d:/ws/src/A.vue.tsx")
        .expect("A ready");
    assert!(
        a.map_rel.is_some(),
        "the mapped carrier advertises a map_rel"
    );
    // B.vue.verter.ts had no map → no map_rel.
    let b = project
        .ready_files
        .get("d:/ws/src/B.vue.verter.ts")
        .expect("B ready");
    assert!(
        b.map_rel.is_none(),
        "the unmapped carrier advertises no map_rel"
    );
}

#[test]
fn manifest_ready_set_is_subset_of_on_disk_blobs() {
    // The plugin reads ready_files and expects every named blob present. Assert the
    // invariant directly: ready set ⊆ on-disk blobs.
    let (store, _ut) = fresh_store();
    let snap = snapshot(
        "p",
        vec![file(
            "x.vue.tsx",
            "x.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "const x = 1;",
            None,
            1,
        )],
    );
    let batch = PublishBatch::from_snapshot(
        _ut.path().to_string_lossy().to_string(),
        snap,
        None,
        OwnedSetScope::ProjectAuthoritative,
    );
    store.publish_batch(&batch).expect("publish");
    let manifest = store.current_manifest();
    for project in manifest.projects.values() {
        for ready in project.ready_files.values() {
            assert!(
                store.workspace_dir().join(&ready.blob_rel).exists(),
                "ready set must be a subset of on-disk blobs"
            );
        }
    }
}

// ── epoch monotonic across publishes ────────────────────────────────────────

#[test]
fn epoch_is_monotonic_across_publishes() {
    let (store, _ut) = fresh_store();
    let mk = |v: u64| {
        let snap = snapshot(
            "p",
            vec![file(
                "x.vue.tsx",
                "x.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                &format!("const x = {v};"),
                None,
                v,
            )],
        );
        PublishBatch::from_snapshot(
            _ut.path().to_string_lossy().to_string(),
            snap,
            None,
            OwnedSetScope::ProjectAuthoritative,
        )
    };
    let e1 = store.publish_batch(&mk(1)).expect("publish 1");
    let e2 = store.publish_batch(&mk(2)).expect("publish 2");
    let e3 = store.publish_batch(&mk(3)).expect("publish 3");
    assert!(
        e1 < e2 && e2 < e3,
        "epoch must advance every publish: {e1} {e2} {e3}"
    );
    assert_eq!(
        store.current_manifest().epoch,
        e3,
        "manifest epoch is the latest"
    );
}

// ── SourceDelta flip prune: a superseded companion identity is retracted ────

#[test]
fn source_delta_republish_with_flipped_companion_extension_retracts_stale_ready_entry() {
    // A per-source (SourceDelta) publish that changes a carrier's IDE companion
    // identity (`.tsx` → `.jsx` on a script-kind correction) must retract the
    // SUPERSEDED ready entry. Otherwise the stale companion stays resolvable
    // through `ready_files`, joins the tsserver Program, and tsserver's
    // output-file membership check then excludes the current same-stem
    // companion from the configured project ("Detected output file").
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let publish = |provider_uri: &str, kind: ScriptKind| {
        let snap = snapshot(
            "d:/ws/tsconfig.json",
            vec![
                file(
                    provider_uri,
                    "d:/ws/src/Comp.vue",
                    SnapshotRole::CarrierIde,
                    kind,
                    "export const c = 1;",
                    None,
                    1,
                ),
                // A SIBLING carrier's companion — must survive every delta.
                file(
                    "d:/ws/src/Other.vue.tsx",
                    "d:/ws/src/Other.vue",
                    SnapshotRole::CarrierIde,
                    ScriptKind::Tsx,
                    "export const o = 1;",
                    None,
                    1,
                ),
            ],
        );
        store
            .publish_batch(&PublishBatch::from_snapshot(
                ws.clone(),
                snap,
                None,
                OwnedSetScope::SourceDelta,
            ))
            .expect("publish");
    };
    publish("d:/ws/src/Comp.vue.tsx", ScriptKind::Tsx);
    publish("d:/ws/src/Comp.vue.jsx", ScriptKind::Jsx);

    let manifest = store.current_manifest();
    let project = manifest
        .projects
        .get("d:/ws/tsconfig.json")
        .expect("project");
    assert!(
        project.ready_files.contains_key("d:/ws/src/Comp.vue.jsx"),
        "the current companion is advertised"
    );
    assert!(
        !project.ready_files.contains_key("d:/ws/src/Comp.vue.tsx"),
        "the superseded companion identity must be retracted by the delta publish"
    );
    assert!(
        project.ready_files.contains_key("d:/ws/src/Other.vue.tsx"),
        "a sibling carrier's ready entry is preserved"
    );
}

// ── owned_sources vs ready_files split ──────────────────────────────────────

#[test]
fn owned_source_present_before_its_content_is_ready() {
    // Publish the OWNED set first (no content), THEN the content. A source is in
    // owned_sources before it is in ready_files — the architect's primary C10
    // defense (never advertise a companion before its content exists).
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();

    // Step one: register the owned set with NO ready files.
    let owned = vec![OwnedSource {
        source_uri: "d:/ws/src/A.vue".to_string(),
        provider_uri: "d:/ws/src/A.vue.tsx".to_string(),
        role: ManifestRole::CarrierIde,
        script_kind: ManifestScriptKind::Tsx,
    }];
    let empty = snapshot("d:/ws/tsconfig.json", vec![]);
    let batch_a = PublishBatch::from_snapshot(
        ws.clone(),
        empty,
        Some(owned),
        OwnedSetScope::ProjectAuthoritative,
    );
    store.publish_batch(&batch_a).expect("publish owned");

    let m1 = store.current_manifest();
    let p1 = m1.projects.get("d:/ws/tsconfig.json").expect("project");
    assert_eq!(p1.owned_sources.len(), 1, "owned set registered");
    assert!(
        p1.ready_files.is_empty(),
        "NOT ready yet — content not published"
    );
    assert!(
        !p1.ready_files.contains_key("d:/ws/src/A.vue.tsx"),
        "the owned source is NOT advertised through ready_files before its blob exists"
    );

    // Step two: publish the content → now ready.
    let snap_b = snapshot(
        "d:/ws/tsconfig.json",
        vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "export const A = 1;",
            None,
            5,
        )],
    );
    // Pass the same owned set so it is preserved.
    let batch_b = PublishBatch::from_snapshot(
        ws,
        snap_b,
        Some(vec![OwnedSource {
            source_uri: "d:/ws/src/A.vue".to_string(),
            provider_uri: "d:/ws/src/A.vue.tsx".to_string(),
            role: ManifestRole::CarrierIde,
            script_kind: ManifestScriptKind::Tsx,
        }]),
        OwnedSetScope::ProjectAuthoritative,
    );
    store.publish_batch(&batch_b).expect("publish content");

    let m2 = store.current_manifest();
    let p2 = m2.projects.get("d:/ws/tsconfig.json").expect("project");
    assert!(
        p2.ready_files.contains_key("d:/ws/src/A.vue.tsx"),
        "NOW ready — content published"
    );
    assert_eq!(p2.owned_sources.len(), 1, "owned set preserved");
}

// ── atomic replace: re-publish overwrites the manifest atomically ────────────

#[test]
fn republish_overwrites_manifest_with_valid_json() {
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let mk = |content: &str| {
        let snap = snapshot(
            "p",
            vec![file(
                "x.vue.tsx",
                "x.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                content,
                None,
                1,
            )],
        );
        PublishBatch::from_snapshot(ws.clone(), snap, None, OwnedSetScope::ProjectAuthoritative)
    };
    store.publish_batch(&mk("v1")).expect("publish v1");
    store.publish_batch(&mk("v2")).expect("publish v2");

    // The final manifest is valid JSON, never truncated.
    let bytes = std::fs::read(store.manifest_path()).expect("manifest readable");
    let parsed: Manifest = serde_json::from_slice(&bytes).expect("manifest is valid JSON");
    assert_eq!(parsed.epoch, 2, "second publish epoch");
    assert!(
        !store.manifest_path().with_extension("json.tmp").exists(),
        "the temp manifest must NOT remain after an atomic swap"
    );
}

#[test]
fn concurrent_publishes_never_leave_a_torn_manifest() {
    use std::sync::Arc as StdArc;
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let store = StdArc::new(CarrierPublishStore::open(HOST_VERSION, &ws));

    let mut handles = Vec::new();
    for t in 0..6u64 {
        let store = StdArc::clone(&store);
        let ws = ws.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..8u64 {
                let v = t * 100 + i;
                let snap = snapshot(
                    "p",
                    vec![file(
                        "x.vue.tsx",
                        "x.vue",
                        SnapshotRole::CarrierIde,
                        ScriptKind::Tsx,
                        &format!("const v = {v};"),
                        None,
                        v,
                    )],
                );
                let batch = PublishBatch::from_snapshot(
                    ws.clone(),
                    snap,
                    None,
                    OwnedSetScope::ProjectAuthoritative,
                );
                store.publish_batch(&batch).expect("publish");
                // Every observation of the manifest is valid JSON (never torn).
                if let Ok(bytes) = std::fs::read(store.manifest_path()) {
                    serde_json::from_slice::<Manifest>(&bytes)
                        .expect("manifest is always valid JSON, never torn");
                }
            }
        }));
    }
    for h in handles {
        h.join().expect("thread");
    }
    // The final manifest parses and every ready blob exists.
    let manifest = store.current_manifest();
    for project in manifest.projects.values() {
        for ready in project.ready_files.values() {
            assert!(store.workspace_dir().join(&ready.blob_rel).exists());
        }
    }
}

// ── content-addressed idempotency ────────────────────────────────────────────

#[test]
fn publishing_the_same_content_twice_writes_the_blob_once_at_a_stable_path() {
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let mk = || {
        let snap = snapshot(
            "p",
            vec![file(
                "x.vue.tsx",
                "x.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "stable content",
                None,
                1,
            )],
        );
        PublishBatch::from_snapshot(ws.clone(), snap, None, OwnedSetScope::ProjectAuthoritative)
    };
    store.publish_batch(&mk()).expect("publish 1");
    let blob_path = {
        let m = store.current_manifest();
        let r = m.projects["p"].ready_files["x.vue.tsx"].clone();
        store.workspace_dir().join(&r.blob_rel)
    };
    let mtime1 = std::fs::metadata(&blob_path)
        .and_then(|m| m.modified())
        .expect("mtime");

    store.publish_batch(&mk()).expect("publish 2");
    let m2 = store.current_manifest();
    let r2 = &m2.projects["p"].ready_files["x.vue.tsx"];
    let blob_path2 = store.workspace_dir().join(&r2.blob_rel);
    // The blob path is STABLE for a given content hash.
    assert_eq!(
        blob_path, blob_path2,
        "content-addressed blob path is stable"
    );
    // Idempotent: the blob was NOT re-written (mtime unchanged — the existing blob
    // is skipped).
    let mtime2 = std::fs::metadata(&blob_path2)
        .and_then(|m| m.modified())
        .expect("mtime");
    assert_eq!(
        mtime1, mtime2,
        "an existing content-addressed blob is not re-written"
    );
}

// ── portability: no NTFS-illegal chars, blake3- not blake3:, Path::join ──────

#[test]
fn generated_blob_names_are_portable() {
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let snap = snapshot(
        "p",
        vec![
            file(
                "a.vue.tsx",
                "a.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "a",
                None,
                1,
            ),
            file(
                "b.vue.verter.ts",
                "b.vue",
                SnapshotRole::CarrierApi,
                ScriptKind::Ts,
                "b",
                None,
                1,
            ),
            file(
                "c.svelte.jsx",
                "c.svelte",
                SnapshotRole::CarrierIde,
                ScriptKind::Jsx,
                "c",
                None,
                1,
            ),
        ],
    );
    let batch = PublishBatch::from_snapshot(ws, snap, None, OwnedSetScope::ProjectAuthoritative);
    store.publish_batch(&batch).expect("publish");

    let re = regex_lite_blob();
    let manifest = store.current_manifest();
    for project in manifest.projects.values() {
        for ready in project.ready_files.values() {
            // The blob_rel basename matches ^blake3-[0-9a-f]+\.(tsx|ts|jsx)$
            let base = ready.blob_rel.rsplit('/').next().expect("basename");
            assert!(
                re(base),
                "blob basename `{base}` must match ^blake3-[0-9a-f]+\\.(tsx|ts|jsx)$"
            );
            // NO NTFS-illegal characters anywhere in the relative path.
            for ch in ready.blob_rel.chars() {
                assert!(
                    !matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\'),
                    "blob_rel `{}` contains NTFS-illegal char {ch:?}",
                    ready.blob_rel
                );
            }
            // blake3- not blake3:
            assert!(
                base.starts_with("blake3-"),
                "uses the portable `blake3-` prefix"
            );
            assert!(
                !base.contains("blake3:"),
                "never the NTFS-illegal `blake3:` form"
            );
        }
    }
}

/// A tiny `^blake3-[0-9a-f]+\.(tsx|ts|jsx)$` matcher (no regex dep needed).
fn regex_lite_blob() -> impl Fn(&str) -> bool {
    |s: &str| {
        let Some(rest) = s.strip_prefix("blake3-") else {
            return false;
        };
        let Some(dot) = rest.find('.') else {
            return false;
        };
        let (hex, ext) = rest.split_at(dot);
        let ext = &ext[1..];
        !hex.is_empty()
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && matches!(ext, "tsx" | "ts" | "jsx")
    }
}

#[test]
fn map_blob_name_is_portable_json() {
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let snap = snapshot(
        "p",
        vec![file(
            "a.vue.tsx",
            "a.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "a",
            Some("{\"version\":3,\"mappings\":\"\"}"),
            1,
        )],
    );
    let batch = PublishBatch::from_snapshot(ws, snap, None, OwnedSetScope::ProjectAuthoritative);
    store.publish_batch(&batch).expect("publish");
    let m = store.current_manifest();
    let r = &m.projects["p"].ready_files["a.vue.tsx"];
    let map_rel = r
        .map_rel
        .as_ref()
        .expect("map_rel present for a mapped file");
    let base = map_rel.rsplit('/').next().expect("basename");
    assert!(base.starts_with("blake3-"), "map uses blake3- prefix");
    assert!(base.ends_with(".json"), "map blob is .json");
    assert!(!base.contains(':'), "no NTFS-illegal colon");
    // TWO-PHASE FOR MAPS: an advertised map_rel must have its blob on disk.
    assert!(
        store.workspace_dir().join(map_rel).exists(),
        "an advertised map_rel must have its blob on disk: {map_rel}"
    );
}

#[test]
fn map_rel_is_absent_when_no_map_json_is_carried() {
    // A file with a map_hash but NO map_json (the in-memory rename-mapping path)
    // advertises NO on-disk map blob — never a broken pointer. The map_hash
    // identity is still recorded.
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let mut f = file(
        "a.vue.tsx",
        "a.vue",
        SnapshotRole::CarrierIde,
        ScriptKind::Tsx,
        "a",
        None,
        1,
    );
    // Stamp a non-zero map_hash but leave map_json None (the identity-only path).
    f.map_hash = h16("some-map-identity");
    f.map_json = None;
    let snap = snapshot("p", vec![f]);
    let batch = PublishBatch::from_snapshot(ws, snap, None, OwnedSetScope::ProjectAuthoritative);
    store.publish_batch(&batch).expect("publish");
    let m = store.current_manifest();
    let r = &m.projects["p"].ready_files["a.vue.tsx"];
    assert!(
        r.map_rel.is_none(),
        "no map_rel advertised without a map blob on disk"
    );
    // The map_hash identity IS recorded (it is part of the carrier identity).
    assert_eq!(r.map_hash, hex_basename(&h16("some-map-identity")));
}

// ── store dir is under temp_dir, NOT under any workspace path ────────────────

#[test]
fn store_dir_is_under_temp_dir_and_not_under_the_workspace() {
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let store = CarrierPublishStore::open(HOST_VERSION, &ws);

    // Under the system temp dir.
    assert!(
        store.workspace_dir().starts_with(std::env::temp_dir()),
        "store dir {} must be under the system temp dir {}",
        store.workspace_dir().display(),
        std::env::temp_dir().display()
    );
    // NOT under the user's workspace tree.
    assert!(
        !store.workspace_dir().starts_with(user_tree.path()),
        "store dir must NOT be under the user workspace tree"
    );
    // The store-dir-name segment is present (a Verter-managed dir).
    assert!(
        store
            .workspace_dir()
            .components()
            .any(|c| c.as_os_str() == STORE_DIR_NAME),
        "store dir must be under the `{STORE_DIR_NAME}` Verter-managed segment"
    );
}

// ── last-good: a prior version's blob persists (no GC yet) ───────────────────

#[test]
fn prior_version_blob_is_not_deleted_after_a_new_version() {
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let mk = |content: &str, v: u64| {
        let snap = snapshot(
            "p",
            vec![file(
                "x.vue.tsx",
                "x.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                content,
                None,
                v,
            )],
        );
        PublishBatch::from_snapshot(ws.clone(), snap, None, OwnedSetScope::ProjectAuthoritative)
    };
    store
        .publish_batch(&mk("v1 content", 1))
        .expect("publish v1");
    let v1_blob = store
        .blobs_dir()
        .join(format!("blake3-{}.tsx", hex_basename(&h16("v1 content"))));
    assert!(v1_blob.exists(), "v1 blob exists after publish");

    store
        .publish_batch(&mk("v2 content", 2))
        .expect("publish v2");
    // GC is not done → v1 blob STILL readable (last-good persists).
    assert!(
        v1_blob.exists(),
        "v1 blob must NOT be deleted when v2 is published (no GC yet — last-good persists)"
    );
    let v2_blob = store
        .blobs_dir()
        .join(format!("blake3-{}.tsx", hex_basename(&h16("v2 content"))));
    assert!(v2_blob.exists(), "v2 blob exists");
    assert_ne!(
        v1_blob, v2_blob,
        "different content → different content-addressed path"
    );
}

/// Lowercase hex of a 16-byte hash (no prefix) — for building expected blob names.
fn hex_basename(h: &[u8; 16]) -> String {
    let mut s = String::new();
    for b in h {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// ── zero-working-tree-write: the store writes ONLY under its store root ──────

#[test]
fn store_writes_only_under_its_store_root_never_the_user_tree() {
    // Backs the `tsc_batch_writes_no_working_tree_files` guarantee: snapshot the
    // user tree, publish, and assert NOTHING was written under the user tree — every
    // write landed under the store's temp root.
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let store = CarrierPublishStore::open(HOST_VERSION, &ws);

    // The user tree starts empty.
    let before = list_files_recursive(user_tree.path());
    assert!(before.is_empty(), "user tree starts empty");

    let snap = snapshot(
        "d:/ws/tsconfig.json",
        vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "export const A = 1;",
            Some("{\"version\":3}"),
            1,
        )],
    );
    let batch = PublishBatch::from_snapshot(ws, snap, None, OwnedSetScope::ProjectAuthoritative);
    store.publish_batch(&batch).expect("publish");

    // The user tree is STILL empty — zero working-tree writes.
    let after = list_files_recursive(user_tree.path());
    assert!(
        after.is_empty(),
        "publish must write ZERO files under the user tree, found: {after:?}"
    );
    // And the store DID write under its own root.
    assert!(
        store.manifest_path().exists(),
        "manifest written under the store root"
    );
    assert!(
        !store.workspace_dir().starts_with(user_tree.path()),
        "the store root is not under the user tree"
    );
}

/// List every file (recursively) under `dir`, as relative path strings.
fn list_files_recursive(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push(
                    p.strip_prefix(base)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
    }
    walk(dir, dir, &mut out);
    out
}

// ── blake3_name / portability unit ──────────────────────────────────────────

#[test]
fn blake3_name_is_lowercase_hex_with_portable_prefix() {
    let name = blake3_name(&[
        0x0a, 0xbc, 0xde, 0xf0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    ]);
    assert!(name.starts_with("blake3-"));
    assert!(!name.contains(':'), "never the NTFS-illegal colon form");
    let hex = name.strip_prefix("blake3-").unwrap();
    assert_eq!(hex.len(), 32, "16 bytes → 32 hex chars");
    assert!(hex
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert!(hex.starts_with("0abcdef0"), "low nibble ordering");
}

// ── R2a: prune ready_files + full-owned-set vs per-source-delta contract ─────

/// An OwnedSource row for `(source, provider)` (CarrierIde/TSX — the only axis
/// these prune tests vary is membership, not kind).
fn owned(source: &str, provider: &str) -> OwnedSource {
    OwnedSource {
        source_uri: source.to_string(),
        provider_uri: provider.to_string(),
        role: ManifestRole::CarrierIde,
        script_kind: ManifestScriptKind::Tsx,
    }
}

#[test]
fn authoritative_publish_prunes_ready_file_no_longer_owned() {
    // Publish A + B as the AUTHORITATIVE owned set, then publish with B dropped from
    // the owned set. The prune must remove B from ready_files — `getExternalFiles`
    // stops advertising a carrier whose source left the project. DISCRIMINATING: a
    // pure-insert merge (no prune) would keep B forever.
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();

    let snap_ab = snapshot(
        "d:/ws/tsconfig.json",
        vec![
            file(
                "d:/ws/src/A.vue.tsx",
                "d:/ws/src/A.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const A = 1;",
                None,
                1,
            ),
            file(
                "d:/ws/src/B.vue.tsx",
                "d:/ws/src/B.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const B = 2;",
                None,
                1,
            ),
        ],
    );
    let batch_ab = PublishBatch::from_snapshot(
        ws.clone(),
        snap_ab,
        None,
        OwnedSetScope::ProjectAuthoritative,
    );
    store.publish_batch(&batch_ab).expect("publish A+B");

    let m1 = store.current_manifest();
    let p1 = m1.projects.get("d:/ws/tsconfig.json").expect("project");
    assert!(
        p1.ready_files.contains_key("d:/ws/src/A.vue.tsx"),
        "A ready"
    );
    assert!(
        p1.ready_files.contains_key("d:/ws/src/B.vue.tsx"),
        "B ready"
    );

    // Re-publish: only A is owned now (B was deleted / lost its owner). Authoritative
    // owned set = {A}. The content delta carries A only.
    let snap_a = snapshot(
        "d:/ws/tsconfig.json",
        vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "export const A = 1;",
            None,
            2,
        )],
    );
    let batch_a = PublishBatch::from_snapshot(
        ws,
        snap_a,
        Some(vec![owned("d:/ws/src/A.vue", "d:/ws/src/A.vue.tsx")]),
        OwnedSetScope::ProjectAuthoritative,
    );
    store.publish_batch(&batch_a).expect("publish A only");

    let m2 = store.current_manifest();
    let p2 = m2.projects.get("d:/ws/tsconfig.json").expect("project");
    assert!(
        p2.ready_files.contains_key("d:/ws/src/A.vue.tsx"),
        "A still ready"
    );
    assert!(
        !p2.ready_files.contains_key("d:/ws/src/B.vue.tsx"),
        "B PRUNED from ready_files — a no-longer-owned carrier is not advertised"
    );
    assert!(
        !p2.owned_sources
            .iter()
            .any(|o| o.source_uri == "d:/ws/src/B.vue"),
        "B dropped from owned_sources too"
    );
}

#[test]
fn source_delta_publish_does_not_prune_sibling_carriers() {
    // The live per-edit publish carries only the TOUCHED carrier. Under SourceDelta
    // it must UNION its own rows and NEVER prune a sibling it does not know about.
    // DISCRIMINATING: if a per-source publish pruned to its own owned set, editing A
    // would wipe sibling B from ready_files (the storm this contract prevents).
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();

    // Seed A + B authoritatively.
    let snap_ab = snapshot(
        "d:/ws/tsconfig.json",
        vec![
            file(
                "d:/ws/src/A.vue.tsx",
                "d:/ws/src/A.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const A = 1;",
                None,
                1,
            ),
            file(
                "d:/ws/src/B.vue.tsx",
                "d:/ws/src/B.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const B = 2;",
                None,
                1,
            ),
        ],
    );
    store
        .publish_batch(&PublishBatch::from_snapshot(
            ws.clone(),
            snap_ab,
            None,
            OwnedSetScope::ProjectAuthoritative,
        ))
        .expect("seed A+B");

    // Edit ONLY A — a per-source DELTA (owned rows = A's companions only).
    let snap_a = snapshot(
        "d:/ws/tsconfig.json",
        vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "export const A = 99;",
            None,
            2,
        )],
    );
    store
        .publish_batch(&PublishBatch::from_snapshot(
            ws,
            snap_a,
            None,
            OwnedSetScope::SourceDelta,
        ))
        .expect("delta edit A");

    let m = store.current_manifest();
    let p = m.projects.get("d:/ws/tsconfig.json").expect("project");
    assert!(
        p.ready_files.contains_key("d:/ws/src/B.vue.tsx"),
        "sibling B must SURVIVE a per-source delta publish of A (no prune)"
    );
    assert!(
        p.ready_files.contains_key("d:/ws/src/A.vue.tsx"),
        "A still ready"
    );
    // A's owned row is a UNION refresh — exactly one A row, not duplicated.
    assert_eq!(
        p.owned_sources
            .iter()
            .filter(|o| o.source_uri == "d:/ws/src/A.vue")
            .count(),
        1,
        "delta union refreshes A's owned row without duplicating it"
    );
}

#[test]
fn retract_sources_removes_owned_and_ready_for_named_source() {
    // The explicit delete / no-owner transition: retract B by source_uri removes its
    // owned rows + advertised companions, leaving A intact.
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    let snap_ab = snapshot(
        "d:/ws/tsconfig.json",
        vec![
            file(
                "d:/ws/src/A.vue.tsx",
                "d:/ws/src/A.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const A = 1;",
                None,
                1,
            ),
            file(
                "d:/ws/src/B.vue.tsx",
                "d:/ws/src/B.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const B = 2;",
                None,
                1,
            ),
        ],
    );
    store
        .publish_batch(&PublishBatch::from_snapshot(
            ws,
            snap_ab,
            None,
            OwnedSetScope::ProjectAuthoritative,
        ))
        .expect("seed A+B");
    let epoch_before = store.current_manifest().epoch;

    let new_epoch = store
        .retract_sources("d:/ws/tsconfig.json", &["d:/ws/src/B.vue"])
        .expect("retract B");
    assert!(new_epoch > epoch_before, "retraction advances the epoch");

    let p = store
        .current_manifest()
        .projects
        .get("d:/ws/tsconfig.json")
        .cloned()
        .expect("project");
    assert!(p.ready_files.contains_key("d:/ws/src/A.vue.tsx"), "A kept");
    assert!(
        !p.ready_files.contains_key("d:/ws/src/B.vue.tsx"),
        "B retracted from ready_files"
    );
    assert!(
        !p.owned_sources
            .iter()
            .any(|o| o.source_uri == "d:/ws/src/B.vue"),
        "B retracted from owned_sources"
    );
}

#[test]
fn retract_source_from_all_projects_clears_every_owner() {
    // A deleted carrier's owner can no longer be resolved, so retraction scans every
    // project. Seed the same source in two projects, then retract everywhere.
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();
    for proj in ["d:/ws/p1/tsconfig.json", "d:/ws/p2/tsconfig.json"] {
        let snap = snapshot(
            proj,
            vec![file(
                "d:/ws/src/Shared.vue.tsx",
                "d:/ws/src/Shared.vue",
                SnapshotRole::CarrierIde,
                ScriptKind::Tsx,
                "export const S = 1;",
                None,
                1,
            )],
        );
        store
            .publish_batch(&PublishBatch::from_snapshot(
                ws.clone(),
                snap,
                None,
                OwnedSetScope::ProjectAuthoritative,
            ))
            .expect("seed");
    }

    store
        .retract_source_from_all_projects("d:/ws/src/Shared.vue")
        .expect("retract everywhere");

    let m = store.current_manifest();
    for proj in ["d:/ws/p1/tsconfig.json", "d:/ws/p2/tsconfig.json"] {
        let p = m.projects.get(proj).expect("project entry");
        assert!(
            p.ready_files.is_empty() && p.owned_sources.is_empty(),
            "{proj}: the shared carrier is retracted from every project"
        );
    }
}

// ── R2b: read_manifest is fresh-only-on-NotFound (never clobber on corrupt) ──

#[test]
fn corrupt_manifest_does_not_erase_known_entries_on_next_publish() {
    // A present-but-corrupt manifest must FAIL the publish, not silently reset to
    // empty (which would clobber other projects' entries on the next atomic swap).
    // DISCRIMINATING: the old swallow-all `read_manifest` would publish a fresh
    // manifest carrying ONLY the new project, erasing the prior one — here we assert
    // the publish ERRORS and the on-disk manifest (with the prior project) is intact.
    let (store, _ut) = fresh_store();
    let ws = _ut.path().to_string_lossy().to_string();

    // Establish a valid manifest with project P1.
    let snap_p1 = snapshot(
        "d:/ws/p1/tsconfig.json",
        vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "export const A = 1;",
            None,
            1,
        )],
    );
    store
        .publish_batch(&PublishBatch::from_snapshot(
            ws.clone(),
            snap_p1,
            None,
            OwnedSetScope::ProjectAuthoritative,
        ))
        .expect("publish P1");
    assert!(
        store
            .current_manifest()
            .projects
            .contains_key("d:/ws/p1/tsconfig.json"),
        "P1 established"
    );

    // Corrupt the manifest on disk (a torn / partially-written / garbage file).
    std::fs::write(store.manifest_path(), b"{ this is not valid json").expect("corrupt write");

    // A publish for a DIFFERENT project P2 must FAIL (the corrupt manifest is not
    // silently reset to empty) rather than succeed by erasing P1.
    let snap_p2 = snapshot(
        "d:/ws/p2/tsconfig.json",
        vec![file(
            "d:/ws/src/B.vue.tsx",
            "d:/ws/src/B.vue",
            SnapshotRole::CarrierIde,
            ScriptKind::Tsx,
            "export const B = 2;",
            None,
            1,
        )],
    );
    let result = store.publish_batch(&PublishBatch::from_snapshot(
        ws,
        snap_p2,
        None,
        OwnedSetScope::ProjectAuthoritative,
    ));
    assert!(
        result.is_err(),
        "a publish over a corrupt manifest must FAIL closed (never reset to empty)"
    );
    assert_eq!(
        result.unwrap_err().kind(),
        std::io::ErrorKind::InvalidData,
        "the corrupt manifest surfaces as an InvalidData error, not a silent fresh"
    );
}

#[test]
fn missing_manifest_yields_fresh_default() {
    // The ONLY case that yields a fresh default: NotFound. A brand-new store has no
    // manifest; `current_manifest` returns a fresh (empty) one rather than erroring.
    let (store, _ut) = fresh_store();
    assert!(!store.manifest_path().exists(), "no manifest yet");
    let m = store.current_manifest();
    assert!(m.projects.is_empty(), "fresh manifest is empty");
    assert_eq!(m.epoch, 0, "fresh manifest seeds epoch 0");
}

// ── R2c: workspace-hash case-fold is platform-gated ─────────────────────────

#[test]
fn workspace_hash_case_fold_is_platform_gated() {
    // Two case-DISTINCT roots. On a case-INsensitive FS (Windows/macOS default) they
    // fold to ONE store dir (the same workspace reached via a different-case path).
    // On a case-SENSITIVE FS (Linux) they are DISTINCT workspaces and MUST get
    // DISTINCT store dirs — an unconditional lowercase would collide them.
    // DISCRIMINATING: the pre-fix code (always `to_lowercase()`) would make these
    // equal on EVERY platform, failing the Linux branch.
    let lower = workspace_hash_dir("/repo/app");
    let upper = workspace_hash_dir("/repo/App");
    if fs_is_case_insensitive() {
        assert_eq!(
            lower, upper,
            "case-insensitive FS: case-distinct roots fold to one store"
        );
    } else {
        assert_ne!(
            lower, upper,
            "case-sensitive FS (Linux): case-distinct roots get DISTINCT stores \
             (an unconditional lowercase would wrongly collide them)"
        );
    }
}

#[test]
fn distinct_case_roots_get_distinct_store_dirs_on_case_sensitive_fs() {
    // Whole-path level assertion of the same property through the public
    // `carrier_store_dir_for` (the path the spawn + publish both derive). On a
    // case-sensitive FS the two roots MUST resolve to different store dirs.
    let a = carrier_store_dir_for(HOST_VERSION, "/srv/Project");
    let b = carrier_store_dir_for(HOST_VERSION, "/srv/project");
    if fs_is_case_insensitive() {
        assert_eq!(a, b, "case-insensitive FS: one store dir for both");
    } else {
        assert_ne!(
            a, b,
            "case-sensitive FS: distinct store dirs for case-distinct roots"
        );
    }
}
