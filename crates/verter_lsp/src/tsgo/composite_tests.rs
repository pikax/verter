//! Discriminating unit tests for the composite's diagnostics COMPOSITION — pure over
//! the `TypeDiagnostic` carrier (no engine, no transport).
//!
//! The defect these guard: the SHARED overlay used to REPLACE OWNED's `--lsp`
//! diagnostics wholesale, dropping the syntactic/suggestion/tag/related surface OWNED
//! provides. The composite now UNIONS SHARED's authoritative semantic diagnostics with
//! OWNED's, deduplicated — no OWNED class is silently dropped, and an identical
//! diagnostic reported by both engines is not double-reported.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use verter_session::external_ts::{AmbiguityCause, ProjectResolution};
use verter_session::{HostConfig, VerterHost};
use verter_type_runtime::protocol::{
    DiagnosticRelatedInfo, TypeDiagnostic, TypeDiagnosticSeverity, TypeDiagnosticTag,
};
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};
use verter_workspace::{FilesystemOptions, FilesystemWorkspace, WorkspaceAccess};

use super::{
    carrier_source_of, compose_diagnostics, compose_establishment_discriminant,
    compose_owned_with_bounded_shared, injection_shadow_safe, real_file_occupies_injected_path,
    SharedRendezvous, SharedTsgoOverlay,
};

/// A minimal carrier diagnostic at `[start, end)` with `code` + `message`.
fn diag(start: u32, end: u32, code: &str, message: &str) -> TypeDiagnostic {
    TypeDiagnostic {
        message: message.to_string(),
        severity: TypeDiagnosticSeverity::Error,
        start,
        end,
        code: Some(code.to_string()),
        tags: Vec::new(),
        related_information: Vec::new(),
    }
}

fn codes(diags: &[TypeDiagnostic]) -> Vec<String> {
    diags.iter().filter_map(|d| d.code.clone()).collect()
}

/// The core discriminator: for a bound carrier the merged result contains BOTH a
/// SHARED-only semantic diagnostic AND an OWNED-only non-semantic diagnostic, and an
/// identical diagnostic reported by both engines is reported exactly once. A
/// wholesale-replace composite would drop the OWNED-only syntactic diagnostic.
#[test]
fn compose_unions_shared_semantic_and_owned_nonsemantic_deduped() {
    // SHARED: an authoritative cross-file SEMANTIC diagnostic (TS2322).
    let shared = vec![diag(
        10,
        20,
        "2322",
        "Type 'string' is not assignable to type 'number'.",
    )];
    // OWNED (`--lsp`): the SAME semantic diagnostic (a duplicate) PLUS an OWNED-only
    // SYNTACTIC diagnostic (TS1005) that `--api getSemanticDiagnostics` never produces.
    let owned = vec![
        diag(
            10,
            20,
            "2322",
            "Type 'string' is not assignable to type 'number'.",
        ),
        diag(30, 31, "1005", "';' expected."),
    ];

    let merged = compose_diagnostics(shared, owned);
    let merged_codes = codes(&merged);

    assert!(
        merged_codes.iter().any(|c| c == "2322"),
        "the SHARED semantic diagnostic must survive; got {merged_codes:?}"
    );
    assert!(
        merged_codes.iter().any(|c| c == "1005"),
        "the OWNED-only syntactic diagnostic must NOT be dropped by the overlay; got {merged_codes:?}"
    );
    assert_eq!(
        merged
            .iter()
            .filter(|d| d.code.as_deref() == Some("2322"))
            .count(),
        1,
        "an identical diagnostic reported by BOTH engines is not double-reported"
    );
    assert_eq!(
        merged.len(),
        2,
        "union-with-dedup: 1 SHARED semantic + 1 OWNED-only syntactic (the duplicate collapsed)"
    );
}

/// The metadata-merge discriminator. When a SHARED and an OWNED diagnostic
/// collide on `(span, code, message)`, the OWNED copy's `tags` +
/// `relatedInformation` must be MERGED into the retained SHARED copy — never
/// dropped. RED before the fix: dedup kept only SHARED's copy, so OWNED's
/// Unnecessary tag + relatedInformation span vanished.
#[test]
fn compose_collision_merges_owned_tags_and_related_into_shared() {
    // SHARED (`--api getSemanticDiagnostics`) reports the diagnostic WITHOUT the
    // tag/related surface.
    let shared = vec![diag(10, 20, "6133", "'x' is declared but never used.")];
    // OWNED (`--lsp`) reports the IDENTICAL diagnostic WITH the Unnecessary tag (the
    // unused-fade) AND a relatedInformation span.
    let mut owned_diag = diag(10, 20, "6133", "'x' is declared but never used.");
    owned_diag.tags = vec![TypeDiagnosticTag::Unnecessary];
    owned_diag.related_information = vec![DiagnosticRelatedInfo {
        path: "/w/App.vue.tsx".to_string(),
        start: 5,
        end: 6,
        message: "'x' was also declared here.".to_string(),
    }];

    let merged = compose_diagnostics(shared, vec![owned_diag]);
    assert_eq!(
        merged.len(),
        1,
        "the identical diagnostic still collapses to one"
    );
    let d = &merged[0];
    assert_eq!(
        d.tags,
        vec![TypeDiagnosticTag::Unnecessary],
        "OWNED's Unnecessary tag must survive the dedup — never silently dropped"
    );
    assert_eq!(
        d.related_information.len(),
        1,
        "OWNED's relatedInformation must survive the dedup"
    );
    assert_eq!(
        d.related_information[0].message, "'x' was also declared here.",
        "the merged related span is OWNED's"
    );
}

/// The union is deduplicated: when BOTH engines carry a tag, it appears ONCE,
/// and an OWNED-only tag is added — SHARED's own metadata is preserved and never
/// doubled.
#[test]
fn compose_collision_unions_tags_without_duplication() {
    let mut shared_d = diag(0, 5, "6385", "'foo' is deprecated.");
    shared_d.tags = vec![TypeDiagnosticTag::Deprecated];
    let mut owned_d = diag(0, 5, "6385", "'foo' is deprecated.");
    owned_d.tags = vec![
        TypeDiagnosticTag::Deprecated,
        TypeDiagnosticTag::Unnecessary,
    ];

    let merged = compose_diagnostics(vec![shared_d], vec![owned_d]);
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].tags,
        vec![
            TypeDiagnosticTag::Deprecated,
            TypeDiagnosticTag::Unnecessary
        ],
        "the shared Deprecated tag is not doubled; the owned-only Unnecessary is added"
    );
}

/// A same-span/code diagnostic with a DIFFERENT message is NOT a duplicate (both are
/// kept), so dedup never silently merges genuinely distinct diagnostics.
#[test]
fn compose_dedup_is_span_code_and_message_exact() {
    let shared = vec![diag(0, 5, "2345", "Argument of type 'A' ...")];
    let owned = vec![diag(0, 5, "2345", "Argument of type 'B' ...")];
    let merged = compose_diagnostics(shared, owned);
    assert_eq!(
        merged.len(),
        2,
        "same span+code but a DIFFERENT message are distinct diagnostics — both kept"
    );
}

/// The SHARED-establishment re-arm discriminant depends on BOTH the
/// shim advertisement nonce AND the workspace/config generation — so a failed
/// establishment re-arms on a reconnect (fresh nonce) OR a fresh published snapshot
/// (fresh generation), never nonce-only. The pre-fix nonce-only discriminant could
/// not retry establishment under a later valid snapshot generation.
#[test]
fn establishment_discriminant_covers_nonce_and_generation() {
    let base = compose_establishment_discriminant("abc123", 5);
    // Same nonce, ADVANCED generation ⇒ a distinct discriminant (re-arms a prior
    // transient miss even under the SAME shim nonce — the poisoning-fix rail).
    assert_ne!(
        base,
        compose_establishment_discriminant("abc123", 6),
        "a fresh config generation must change the discriminant (re-arm under the same nonce)"
    );
    // Fresh nonce (reconnect), same generation ⇒ also a distinct discriminant.
    assert_ne!(
        base,
        compose_establishment_discriminant("def456", 5),
        "a reconnect (fresh nonce) must change the discriminant"
    );
    // Identical (nonce, generation) ⇒ the SAME discriminant (no per-query re-attempt).
    assert_eq!(
        base,
        compose_establishment_discriminant("abc123", 5),
        "an unchanged (nonce, generation) is a stable discriminant (no retry-storm)"
    );
}

/// The carrier SOURCE of a provider companion path — the shape classification the
/// shadow-safety gate resolves the source from. A `.vue.tsx` / `.vue.jsx` companion maps
/// to its `.vue` source; a plain `.ts` file is not a companion; a Windows backslash path
/// normalizes to the same forward-slashed source (cross-platform).
#[test]
fn carrier_source_of_maps_companion_to_source_cross_platform() {
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.vue.tsx").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    assert_eq!(
        carrier_source_of("d:/ws/src/Foo.vue.jsx").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    // Cross-platform: a backslash path normalizes to the same forward-slashed source.
    assert_eq!(
        carrier_source_of(r"d:\ws\src\Foo.vue.tsx").as_deref(),
        Some("d:/ws/src/Foo.vue")
    );
    // A plain `.ts` file (no carrier stem) is NOT a carrier companion — OWNED serves it.
    assert_eq!(carrier_source_of("d:/ws/src/plain.ts"), None);
}

/// The shadow-safety decision over a resolved source. A real user file at a
/// carrier-companion path surfaces — through the resolver's UNCONDITIONAL carrier-path
/// conflict pass — as `Ambiguous(CarrierPathOccupiedByRealFile)` in EVERY
/// owner-resolution state (owned `Unique`, unowned `NoProject`, or multiply-owned
/// `MultipleOwners`), and is NEVER injected / overlay-shadowed (`false`)
/// (`carrier_never_shadows_real_user_file`); a same-stem rune module is likewise
/// rejected. A GENUINE generated companion — one with NO real file at its path, whose
/// source resolves to a clean binding, `NoProject`, `SyntheticScratch`, or a
/// `MultipleOwners` overlap — IS safe to inject (`true`). Discriminates the exact
/// shadow-cause match from a blanket `Ambiguous` reject or an unconditional allow.
///
/// The END-TO-END guarantee that a real file at the companion path is NOT injected in
/// the `NoProject` / `MultipleOwners` / `Unique` states is enforced by the resolver's
/// unconditional conflict pass and guarded at the resolver level by
/// `real_file_at_carrier_path_downgrades_unowned_source_to_ambiguous`,
/// `real_file_at_carrier_path_downgrades_multiply_owned_source_to_ambiguous`, and
/// `real_file_at_carrier_path_downgrades_to_ambiguous` — this unit asserts the pure
/// mapping over the resolutions those states produce.
#[test]
fn injection_shadow_safe_rejects_only_real_file_shadow_causes() {
    // A real user file at the companion path — which the resolver's UNCONDITIONAL
    // conflict pass surfaces as `Ambiguous(CarrierPathOccupiedByRealFile)` REGARDLESS of
    // whether the source is owned, unowned (`NoProject`), or multiply-owned
    // (`MultipleOwners`) — is NEVER injected: fail closed to OWNED, never shadow the real
    // file. This is the E2 correction: pre-fix, `NoProject`/`MultipleOwners` short-
    // circuited before the conflict pass, so a real file there resolved to
    // `NoProject`/`MultipleOwners` (admitted below) and WAS overlay-shadowed.
    assert!(
        !injection_shadow_safe(&ProjectResolution::Ambiguous(
            AmbiguityCause::CarrierPathOccupiedByRealFile
        )),
        "a real user file at the companion path must never be overlay-shadowed, in ANY \
         owner state (the resolver's unconditional pass makes it this cause)"
    );
    assert!(
        !injection_shadow_safe(&ProjectResolution::Ambiguous(
            AmbiguityCause::SameStemRuneModule
        )),
        "a same-stem rune module beside the source must never be overlay-shadowed"
    );
    // A GENUINE generated companion — NO real file at its path — is injectable. A
    // `MultipleOwners` overlap and a `NoProject` source are these no-real-file
    // resolutions: a real file at the companion path is instead
    // `CarrierPathOccupiedByRealFile` (rejected above), NEVER these states — so admitting
    // them can no longer overlay-shadow a real user file.
    assert!(
        injection_shadow_safe(&ProjectResolution::Ambiguous(
            AmbiguityCause::MultipleOwners
        )),
        "a MultipleOwners overlap with NO real file at the companion path is a genuine \
         virtual companion — injectable (a real file there is CarrierPathOccupiedByRealFile, \
         rejected above)"
    );
    assert!(
        injection_shadow_safe(&ProjectResolution::NoProject),
        "a NoProject genuine companion with NO real file at its path is injectable as a \
         supporting import member (a real file there is CarrierPathOccupiedByRealFile, \
         rejected above)"
    );
}

/// E3: the ENTIRE SHARED overlay contribution is bounded by ONE outer deadline and
/// FAILS CLOSED to the already-computed OWNED result. A never-answering SHARED path
/// (a stuck relay / control / `--api` peer) must NOT stall the diagnostics response
/// past the bound even though OWNED is ready.
///
/// RED before the fix: `get_diagnostics` awaited the SHARED contribution with NO outer
/// deadline, so a hanging inject/control/`--api` turned opt-in SHARED into an unbounded
/// LSP diagnostics stall. The test guards the production helper with a generous OUTER
/// timeout so a regression (a removed production bound) fails cleanly rather than hanging
/// the suite — the production helper must return within its OWN (short) bound first.
#[tokio::test]
async fn bounded_shared_contribution_falls_back_to_owned_on_stall() {
    let owned = vec![diag(0, 1, "1005", "';' expected.")];
    // A never-answering SHARED path (models a stuck relay/control/`--api` peer).
    let hanging = async { std::future::pending::<Option<Vec<TypeDiagnostic>>>().await };

    let result = tokio::time::timeout(
        Duration::from_secs(3),
        compose_owned_with_bounded_shared(owned.clone(), hanging, Duration::from_millis(150)),
    )
    .await
    .expect(
        "the bounded SHARED contribution must return within its OWN deadline, never stall \
         (a stuck relay/control/--api peer must not hang diagnostics)",
    );
    assert_eq!(
        codes(&result),
        vec!["1005".to_string()],
        "a stalled SHARED path falls back to the already-computed OWNED result (fail-closed)"
    );
}

/// The bounded contribution still OVERLAYS a promptly-answering SHARED result over OWNED
/// (the bound is a fail-closed ceiling, not a suppressor): a SHARED result that arrives
/// within the deadline is unioned with OWNED.
#[tokio::test]
async fn bounded_shared_contribution_overlays_a_prompt_shared_result() {
    let owned = vec![diag(30, 31, "1005", "';' expected.")];
    let shared = async {
        Some(vec![diag(
            10,
            20,
            "2322",
            "Type 'string' is not assignable to type 'number'.",
        )])
    };
    let result = compose_owned_with_bounded_shared(owned, shared, Duration::from_secs(5)).await;
    let mut merged = codes(&result);
    merged.sort();
    assert_eq!(
        merged,
        vec!["1005".to_string(), "2322".to_string()],
        "a prompt SHARED result is overlaid (unioned) over OWNED within the bound"
    );
}

/// A `None` SHARED contribution (SHARED did not engage — no binding, unestablished attach,
/// not-SHARED decision) leaves OWNED unchanged WITHOUT waiting out the deadline.
#[tokio::test]
async fn bounded_shared_none_leaves_owned_unchanged() {
    let owned = vec![diag(0, 1, "6133", "'x' is declared but never used.")];
    let shared = async { None };
    let result = compose_owned_with_bounded_shared(owned, shared, Duration::from_secs(5)).await;
    assert_eq!(
        codes(&result),
        vec!["6133".to_string()],
        "a None SHARED contribution (fail-closed) leaves the OWNED result unchanged"
    );
}

/// Fail-closed shape: an empty SHARED set leaves OWNED unchanged (the composite's
/// diagnostics fallback when SHARED does not engage passes OWNED through verbatim).
#[test]
fn compose_empty_shared_leaves_owned_unchanged() {
    let owned = vec![
        diag(0, 1, "1005", "';' expected."),
        diag(2, 3, "6133", "unused"),
    ];
    let merged = compose_diagnostics(Vec::new(), owned);
    assert_eq!(
        codes(&merged),
        vec!["1005".to_string(), "6133".to_string()],
        "empty SHARED ⇒ OWNED stands unchanged, in order"
    );
}

// ── Shadow-safety at the injected path (`carrier_never_shadows_real_user_file`) ──
//
// These tests drive the PRODUCTION `injection_is_shadow_safe` gate — the predicate
// `inject_all_dirty` consults before injecting a recorded companion — over a REAL
// `VerterHost` whose live published snapshot OWNS `src/**/*` and whose VFS holds the
// given real user files. This exercises the full disk-occupancy + resolver decision
// end-to-end (the seam a pure `compose_*` test cannot reach).

const SHADOW_WS_ROOT: &str = "d:/ws";
const SHADOW_TSCONFIG: &str = "d:/ws/tsconfig.json";

/// The ownership snapshot: ONE configured project whose `include: ["src/**/*"]` OWNS
/// every carrier / `.ts` under `src/` (pattern-based membership), built through the SAME
/// production membership parse/expansion chain the resolver's own tests use. Ownership is
/// glob-pattern-based (empty `materialized_files` ⇒ bridge mode ⇒ `spec.matches`), so it
/// owns any `src/**/*.vue` / `.svelte` / `.ts` path whether or not a file sits there.
fn shadow_fixture_snapshot() -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![SHADOW_WS_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        SHADOW_TSCONFIG.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    let root = CanonicalPath::new(SHADOW_WS_ROOT);
    let raw_membership = load_project_membership(&ws, SHADOW_TSCONFIG);
    let compiler_options = load_compiler_options(&ws, SHADOW_TSCONFIG);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, SHADOW_TSCONFIG)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(SHADOW_WS_ROOT),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(SHADOW_TSCONFIG),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1))
}

/// A `SharedTsgoOverlay` over a real host whose VFS holds `real_files` (injected into the
/// same workspace `Arc` the host holds, so the resolver's `file_exists` probe and the
/// disk-occupancy gate both see them) and whose published snapshot owns `src/**/*`.
fn shadow_overlay_with(real_files: &[(&str, &str)]) -> SharedTsgoOverlay {
    let ws = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    for (path, content) in real_files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        shadow_fixture_snapshot(),
    )));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&ws) as Arc<dyn WorkspaceAccess>);
    SharedTsgoOverlay::new(
        host,
        SharedRendezvous {
            control_dir: PathBuf::from("d:/ws/.verter"),
            session_key: "shadow-test".to_string(),
            workspace_root: SHADOW_WS_ROOT.to_string(),
        },
    )
}

/// G1 (`carrier_never_shadows_real_user_file`): the SHARED overlay must NEVER inject a
/// generated carrier over a REAL user file at the exact injected path — for the
/// DECLARATION carrier (`Foo.d.vue.ts` / `Foo.d.svelte.ts`) as well as the IDE carrier.
/// The declaration carrier is a SEPARATE naming authority the resolver's IDE-companion
/// conflict pass never probes (it probes `Foo.d.vue.tsx`/`.jsx`), and `carrier_source_of`
/// MIS-derives its source as `Foo.d.vue` (not the real `Foo.vue`) — so the source-
/// resolution pass alone binds it cleanly and would inject it. The disk-occupancy gate at
/// the EXACT injected path closes it uniformly.
///
/// RED before the fix: `injection_is_shadow_safe(Foo.d.vue.ts)` returned `true` (the real
/// user file WAS admitted for injection / overlay-shadowed). GREEN after: `false`
/// (skipped, fail-closed to OWNED/native). Asserted for `.vue` AND `.svelte`.
#[tokio::test]
async fn real_declaration_carrier_is_never_overlay_shadowed_vue_and_svelte() {
    for (source, decl) in [
        ("d:/ws/src/Foo.vue", "d:/ws/src/Foo.d.vue.ts"),
        ("d:/ws/src/Foo.svelte", "d:/ws/src/Foo.d.svelte.ts"),
    ] {
        let overlay = shadow_overlay_with(&[
            (source, "<template></template>"),
            (decl, "export const realUserDeclaration = 1;\n"),
        ]);
        assert!(
            !overlay.injection_is_shadow_safe(decl),
            "a REAL user file at the declaration-carrier path `{decl}` must NEVER be \
             overlay-shadowed by the SHARED overlay (carrier_never_shadows_real_user_file)"
        );
    }
}

/// A GENUINE generated declaration carrier — NO real file at `Foo.d.vue.ts` (only the
/// `.vue` / `.svelte` source exists) — is STILL injectable (the disk-occupancy gate never
/// over-suppresses a real virtual companion). Guards against a blanket declaration-carrier
/// reject. Asserted for `.vue` AND `.svelte`.
#[tokio::test]
async fn genuine_declaration_carrier_is_still_injectable_vue_and_svelte() {
    for (source, decl) in [
        ("d:/ws/src/Foo.vue", "d:/ws/src/Foo.d.vue.ts"),
        ("d:/ws/src/Foo.svelte", "d:/ws/src/Foo.d.svelte.ts"),
    ] {
        // Only the SOURCE is a real file; the declaration carrier is Verter-generated.
        let overlay = shadow_overlay_with(&[(source, "<template></template>")]);
        assert!(
            overlay.injection_is_shadow_safe(decl),
            "a GENUINE generated declaration carrier `{decl}` (no real file at its path) \
             must stay injectable as a supporting Program member"
        );
    }
}

/// The existing IDE-carrier shadow behavior is preserved under the disk-occupancy gate: a
/// REAL `Foo.vue.tsx` is never injected; a GENUINE one (no real file) still is.
#[tokio::test]
async fn ide_carrier_shadow_behavior_preserved() {
    // A real user file at the IDE-carrier path ⇒ never injected.
    let overlay_real = shadow_overlay_with(&[
        ("d:/ws/src/Foo.vue", "<template></template>"),
        ("d:/ws/src/Foo.vue.tsx", "export const realUserFile = 1;\n"),
    ]);
    assert!(
        !overlay_real.injection_is_shadow_safe("d:/ws/src/Foo.vue.tsx"),
        "a real user file at the IDE-carrier path must never be overlay-shadowed"
    );
    // A genuine generated IDE carrier (no real file) ⇒ still injected.
    let overlay_genuine = shadow_overlay_with(&[("d:/ws/src/Foo.vue", "<template></template>")]);
    assert!(
        overlay_genuine.injection_is_shadow_safe("d:/ws/src/Foo.vue.tsx"),
        "a genuine generated IDE carrier (no real file) stays injectable"
    );
}

/// A lightweight in-memory workspace holding the given real user files (canonical ids) —
/// exercises the disk-occupancy gate directly without a full host.
fn memory_ws_with(files: &[&str]) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![SHADOW_WS_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    for f in files {
        ws.inject_file((*f).to_string(), Arc::<str>::from("real user file"));
    }
    ws
}

/// The comprehensive disk-occupancy gate distinguishes a REAL user file at the exact
/// injected path from a genuine (absent) generated companion — for EVERY companion type:
/// declaration (`.d.vue.ts` / `.d.svelte.ts`), IDE (`.vue.tsx`), and API (`.vue.verter.ts`).
/// This is the class the fix closes uniformly at the injected path.
#[test]
fn real_file_occupies_injected_path_covers_every_companion_type() {
    for injected in [
        "d:/ws/src/Foo.d.vue.ts",      // declaration carrier (.vue)
        "d:/ws/src/Foo.d.svelte.ts",   // declaration carrier (.svelte)
        "d:/ws/src/Foo.vue.tsx",       // IDE carrier
        "d:/ws/src/Foo.vue.verter.ts", // API companion
    ] {
        // A real user file at the exact injected path ⇒ occupied (never overlay-shadowed).
        let occupied = memory_ws_with(&["d:/ws/src/Foo.vue", "d:/ws/src/Foo.svelte", injected]);
        assert!(
            real_file_occupies_injected_path(&occupied, injected),
            "a real user file at `{injected}` must be detected as occupied (never overlay-shadowed)"
        );
        // Genuine generated companion: only the sources exist, not the companion path.
        let genuine = memory_ws_with(&["d:/ws/src/Foo.vue", "d:/ws/src/Foo.svelte"]);
        assert!(
            !real_file_occupies_injected_path(&genuine, injected),
            "a genuine generated companion `{injected}` (no real file at its path) stays injectable"
        );
    }
}

/// The occupancy probe NORMALIZES the injected path (backslash → slash, drive
/// lowercased on every platform), so a non-canonical injected path cannot evade the
/// fail-closed gate on a case-insensitive FS.
#[test]
fn real_file_occupies_injected_path_is_normalized_cross_platform() {
    let ws = memory_ws_with(&["d:/ws/src/Foo.d.vue.ts"]);
    assert!(
        real_file_occupies_injected_path(&ws, r"d:\ws\src\Foo.d.vue.ts"),
        "a backslash path must normalize to the canonical id and detect the real file"
    );
    assert!(
        real_file_occupies_injected_path(&ws, "D:/ws/src/Foo.d.vue.ts"),
        "an uppercase-drive path must normalize to the canonical id and detect the real file"
    );
    assert!(
        !real_file_occupies_injected_path(&ws, "d:/ws/src/Other.d.vue.ts"),
        "a different path is not occupied"
    );
}
