//! D9 #3 — `Content` mode is content-addressed, never fact-validated.
//!
//! Positive: two compiles of a FACT-FREE SFC (no cross-file dep) with
//! identical `(content, env, Content mode)` reuse one
//! `CompileOutputNode_PureContent` entry — exactly one entry exists
//! after the first compile, and the second compile returns byte-identical
//! output without growing the store.
//!
//! Negative: a `Content` request on an SFC that DOES carry a cross-file
//! dependency (a macro type imported from a workspace `.ts`) downgrades
//! to `Stateless` per the matrix — it publishes NO content-addressed
//! entry and NO session slot.
//!
//! Discrimination against the pre-B5 tree (`204b5ef9`): no `Content`
//! mode, no content-addressed node, no entry-count accessor — does not
//! compile. Against a tree that fact-validated Content, the
//! cross-file-downgrade negative assertion (entry count stays 0) fails.

use verter_session::{
    CompileCacheMode, CompileErrorPolicy, CompileProfile, DowngradeReason, FileKind, HostConfig,
    UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};
use verter_workspace::{ExactResolution, ResolvePhase, ResolveRequestKind};

/// A production (non-dev) host config. The default `HostConfig` enables
/// `dev_mode` + `DevServeLastKnownGood`, which fires the
/// `HasDevLastGood` reason on EVERY compile and would downgrade every
/// `Content` request to `Stateless`. A `Content` request is only
/// reachable as `Content` when no reason fires, so these tests use a
/// production config to make the fact-free Content path actually run as
/// Content.
fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn content_profile() -> CompileProfile {
    CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    }
}

fn compile(host: &VerterHost, canonical: &str, profile: &CompileProfile) -> String {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    })
    .expect("compile")
    .code
    .to_string()
}

// A fact-free SFC: no imports, no cross-file deps → no reason fires →
// the Content request actually runs as Content.
const FACT_FREE: &str =
    "<script setup lang=\"ts\">const n = 1</script><template><div>{{ n }}</div></template>";

#[test]
fn content_mode_reuses_one_pure_content_entry() {
    let host = host();
    upsert_vue(&host, "/Plain.vue", FACT_FREE);
    let profile = content_profile();

    let code1 = compile(&host, "/Plain.vue", &profile);
    // Exactly one content-addressed entry after the first compile.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "first Content compile of a fact-free SFC must publish exactly one content entry"
    );

    let code2 = compile(&host, "/Plain.vue", &profile);
    // The second compile reuses the same entry — no growth, identical code.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "second Content compile must REUSE the existing entry, not add a new one"
    );
    assert_eq!(
        code1, code2,
        "Content warm hit must return byte-identical output"
    );

    // Content is NOT fact-validated: it never publishes a session slot.
    assert!(
        host.compile_slot_fact_dep_signature("/Plain.vue", &profile)
            .is_none(),
        "Content mode must NOT publish a fact-validated session slot"
    );
}

#[test]
fn content_request_with_cross_file_dep_downgrades_to_stateless() {
    // Negative: a cross-file macro type dep makes the pure key unsafe, so
    // a Content request floors to Stateless — NO content entry, NO
    // session slot.
    let host = host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    );
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    // The request asked for Content but ran as Stateless (a reason fired).
    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request on a cross-file-dependent SFC MUST downgrade to Stateless"
    );
    assert!(
        response.downgrade_reason.is_some(),
        "the downgrade must carry a reason"
    );

    // Stateless floor ⇒ NO content-addressed entry.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
    // And NO session slot (Stateless publishes nothing).
    assert!(
        host.compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_none(),
        "a downgraded Content request must NOT publish a session slot"
    );
}

#[test]
fn content_request_with_imported_module_augmentation_downgrades_to_stateless() {
    // An augmenter that augments a module the owner imports leaves NO
    // trace on the owner's OWN declared augmentations — it lives in a
    // separate file (`declare module 'vue' { ... }`). The owner imports
    // from 'vue' but declares no augmentation itself. A content-addressed
    // key carries no augmenter fingerprint, so editing the augmenter would
    // leave the key byte-identical and serve stale output; the classifier
    // MUST recognise the imported augmentation (via the augmentation
    // target index) and floor the Content request to Stateless.
    let host = host();
    // The augmenter both exports a value (so a relative import pulls it
    // into the program) AND augments the imported module 'vue'.
    upsert_ts(
        &host,
        "/src/augment.ts",
        "export const marker = 1;\n\
         declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $foo: string }\n\
         }\n",
    );
    // The owner imports the augmenter relatively (forcing it into the
    // program) and imports a value from 'vue' (the augmented module). It
    // uses neither in a macro, so the ONLY cross-file reason available is
    // the module augmentation — no HasMacroTypeDeps.
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import { marker } from './augment';\n\
         import { ref } from 'vue';\n\
         const n = ref(marker);\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    // The FIRST request already downgrades — no edit-replay needed.
    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request whose owner imports a module-augmented dependency MUST downgrade to Stateless"
    );
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasModuleAugmentation),
        "the highest-priority firing reason must be HasModuleAugmentation, got {:?}",
        response.downgrade_reason
    );
    // Stateless floor ⇒ NO content-addressed entry was published.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
}

#[test]
fn targeted_invalidation_evicts_content_entry_and_forces_recompile() {
    // A `Content` key carries no fact rail, so a targeted
    // `invalidate_compile_slots` MUST evict the content-addressed entry
    // for that canonical — otherwise a same-content recompile would warm-
    // hit and report `cache_hit = true`, breaking the force-recompute
    // contract. The per-canonical reverse index on the content node is the
    // eviction authority.
    let host = host();
    upsert_vue(&host, "/Plain.vue", FACT_FREE);
    let profile = content_profile();

    // First Content compile publishes exactly one content entry.
    let _ = compile(&host, "/Plain.vue", &profile);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "first Content compile of a fact-free SFC must publish exactly one content entry"
    );

    // Targeted invalidation must flush the content-addressed entry.
    host.invalidate_compile_slots("/Plain.vue");
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "invalidate_compile_slots MUST evict the content-addressed entry for the canonical"
    );

    // The next request recompiles (cold) instead of warm-hitting.
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/Plain.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("recompile");
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Content,
        "the fact-free SFC still classifies as Content after invalidation"
    );
    assert!(
        !response.cache_hit,
        "after a targeted invalidation the next Content request MUST recompile (cache_hit == false), \
         not warm-hit a stale content entry"
    );
    // The cold recompute re-publishes exactly one entry.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "the cold recompute must re-publish exactly one content entry"
    );
}

#[test]
fn content_request_with_side_effect_only_relative_augmenter_downgrades_to_stateless() {
    // A side-effect-only relative import (`import "./augment";` — no
    // named binding) of a file that carries a relative module
    // augmentation (`declare module "./local" { ... }`) MUST downgrade a
    // `Content` request to `Stateless`. The content-addressed key carries
    // no augmenter fingerprint, so editing `augment.ts` would leave the
    // key byte-identical and serve stale output unless the classifier
    // recognises the side-effect-driven augmentation dependency.
    //
    // Discriminator: the previous probe enumerated only
    // `ShallowFileState.import_targets`, which is keyed by local-binding
    // name and therefore SKIPS side-effect imports entirely (no binding ⇒
    // no entry). The pre-fix tree leaves the request in `Content` mode
    // with one published content entry; the post-fix tree picks up the
    // side-effect import via `IndexedReady.snapshot.imports` and floors
    // the request to `Stateless` with no content entry.
    let host = host();
    // `/src/augment.ts` carries a relative-target augmentation. The
    // exported value is unused (irrelevant to the discriminator) — only
    // the side-effect import in `Comp.vue` keeps the augmenter in the
    // owner's program.
    upsert_ts(
        &host,
        "/src/local.ts",
        "export interface Foo { a: number }\n",
    );
    upsert_ts(
        &host,
        "/src/augment.ts",
        "declare module './local' {\n\
         \x20 interface Foo { extension: string }\n\
         }\n\
         export {};\n",
    );
    // The owner SIDE-EFFECT-imports the augmenter (no binding) and uses
    // an import from the augmented module — so the augmentation is in
    // scope. The named import on the augmented module is unrelated to
    // the side-effect path; it exists only so the owner has at least one
    // cross-file reference (otherwise the SFC would have no cross-file
    // surface at all and the classifier would never reach the
    // augmentation probe).
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import './augment';\n\
         import type { Foo } from './local';\n\
         const f: Foo | null = null;\n\
         </script>\n\
         <template><div>{{ f }}</div></template>\n",
    );
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request whose owner SIDE-EFFECT-imports a relative augmenter MUST downgrade to Stateless"
    );
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasModuleAugmentation),
        "the firing reason must be HasModuleAugmentation, got {:?}",
        response.downgrade_reason
    );
    // Stateless floor ⇒ NO content-addressed entry was published.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
}

#[test]
fn upsert_evicts_prior_content_entries_for_canonical() {
    // The content-addressed compile node keys on
    // `(canonical, content_hash, env_*, profile, source_map_policy)`. An
    // edit to a canonical changes its `content_hash`, so the prior
    // version's Content entry becomes unreachable — but without an
    // explicit eviction on upsert it would still occupy a slot in the
    // store. Repeated edits would accumulate one entry per content
    // version per canonical, growing without bound.
    //
    // The upsert path's `whole_hash_changed` branch (the same place
    // `ProfileState.content_overrides` is cleared) MUST also evict the
    // canonical's prior content-keyed entries via
    // `compile_output_pure_content().remove_canonical(canonical)`.
    //
    // Discriminator: after N edits that each publish exactly one
    // Content entry, the live store size must equal 1 (the live
    // content's entry), not N. Pre-fix this assertion fails at N=2
    // (count grows monotonically). Post-fix the count stays at 1
    // regardless of N.
    let host = host();
    let profile = content_profile();
    const N: usize = 5;

    for i in 0..N {
        let n_lit = i + 1;
        // Each iteration upserts a SLIGHTLY different SFC content so
        // `whole_hash` changes. The SFC stays fact-free so the request
        // actually runs as Content (no downgrade reasons fire).
        let src = format!(
            "<script setup lang=\"ts\">const n = {n_lit}</script><template><div>{{{{ n }}}}</div></template>"
        );
        upsert_vue(&host, "/Edit.vue", &src);
        let _ = compile(&host, "/Edit.vue", &profile);
        // After every iteration, exactly one entry exists — the prior
        // version's entry was evicted by the upsert path.
        assert_eq!(
            host.compile_output_pure_content_entry_count(),
            1,
            "after edit #{n_lit} the content-addressed store must contain exactly one entry \
             (the live content); the prior version's entry must have been evicted on upsert"
        );
    }
}

#[test]
fn content_request_with_bare_side_effect_external_target_augmenter_downgrades_to_stateless() {
    // A side-effect-only BARE-specifier import (`import "pkg-augment";` —
    // no named binding, non-relative specifier) of a packaged augmenter
    // whose `.d.ts` declares an EXTERNAL-target augmentation
    // (`declare module "vue" { ... }`) MUST downgrade a `Content`
    // request to `Stateless`. The content-addressed key carries no
    // augmenter fingerprint, so editing the bare augmenter would leave
    // the key byte-identical and serve stale output unless the
    // classifier recognises the side-effect-driven augmentation
    // dependency on its EXTERNAL augmentation target (`"vue"`), not
    // just on the augmenter file's resolved canonical.
    //
    // Discriminator (vs the relative-only Step D probe): the prior
    // side-effect walk filtered specifiers to relative-only
    // (`./` / `../`), so a bare specifier never even reached
    // `resolve_type_dependency_canonical`. Even after dropping the
    // relative filter, pushing `ResolvedRelativeCanonical(<augmenter>)`
    // would not match a `declare module "vue"` fact (whose specifier
    // is bare, not relative). The structural fix walks the resolved
    // augmenter's own `ModuleAugmentationFact` entries and emits an
    // `ExternalSpecifier("vue")` probe per fact — the existing index
    // probe then finds the augmenter and downgrades the request.
    //
    // Fixture: a packaged augmenter resolved through an exact
    // resolution override. The owner SIDE-EFFECT-imports it; nothing
    // else in the owner's program references "vue" by name (so the
    // existing binding-driven probe path cannot catch it). The
    // discriminator passes only when the bare-specifier side-effect
    // branch walks the augmenter's facts and emits per-fact targets.
    let host = host();
    // Packaged augmenter: a `.d.ts` declaring `declare module "vue"`.
    // The owner has no relative path to it; resolution goes through
    // the exact-resolutions override.
    upsert_ts(
        &host,
        "/node_modules/pkg-augment/index.d.ts",
        "declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $foo: string }\n\
         }\n\
         export {};\n",
    );
    // The owner side-effect-imports the bare augmenter (no binding) and
    // declares no binding on "vue". With no relative augmenter and no
    // binding-driven probe target for "vue", the only way the
    // classifier can recognise the augmentation is by walking the
    // bare-specifier side-effect import's resolved augmenter facts.
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import 'pkg-augment';\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    // Override the workspace resolver so the owner's bare side-effect
    // import resolves to the packaged augmenter's `.d.ts`. Set AFTER
    // upsert because the upsert path's `record_parsed_edges` clears
    // any prior `exact_resolved` for the canonical (per
    // `integrate_scheduler_snapshot` in `host_lifecycle.rs`). The
    // wrapper handles invalidation of dependent caches.
    host.set_exact_resolutions(
        "/src/Comp.vue",
        vec![ExactResolution {
            specifier: "pkg-augment".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some("/node_modules/pkg-augment/index.d.ts".to_string()),
            possible_canonical_ids: vec!["/node_modules/pkg-augment/index.d.ts".to_string()],
        }],
    );
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request whose owner SIDE-EFFECT-imports a BARE-specifier augmenter \
         whose `.d.ts` declares `declare module \"vue\"` MUST downgrade to Stateless"
    );
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasModuleAugmentation),
        "the firing reason must be HasModuleAugmentation, got {:?}",
        response.downgrade_reason
    );
    // Stateless floor ⇒ NO content-addressed entry was published.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
}

#[test]
fn bare_side_effect_augmenter_invalidates_stale_empty_augmentation_index() {
    // The augmentation_index entry for an augmentation target is warmed
    // lazily on first probe via `ensure_augmentation_index_populated`'s
    // cold scan. If the cold scan runs BEFORE the augmenter file has
    // entered `FileArtifactStore`, the entry is warmed EMPTY. A later
    // bare side-effect import that materialises the augmenter must
    // invalidate the now-stale empty entry so the next probe rebuilds
    // it against the materialised state — otherwise a side-effect
    // augmenter loaded after a pre-warming compile leaves the owner
    // in `Content` mode despite carrying a module-augmentation
    // dependency.
    //
    // Discriminator: pre-warm the augmentation_index entry for the
    // bare specifier (`"vue"`) by compiling a fact-free SFC that
    // imports from `'vue'` — this seeds an empty `ExternalSpecifier
    // ("vue")` set because pkg-augment is NOT yet in the store. Then
    // upsert pkg-augment + a Content SFC that side-effect-imports it.
    // Pre-fix: the probe warm-hits the stale-empty entry → owner stays
    // Content. Post-fix: F1's walk invalidates the entry after
    // materialising pkg-augment → next probe cold-scans against the
    // now-fresh store → finds the augmenter → owner downgrades to
    // Stateless.
    let host = host();

    // Pre-warm the augmentation_index for `ExternalSpecifier("vue")`
    // by compiling a fact-free SFC that imports from 'vue'. This
    // triggers `owner_has_module_augmentation_dependency` which calls
    // `ensure_augmentation_index_populated` for `"vue"` — but no
    // augmenter is loaded yet, so the entry is warmed EMPTY.
    upsert_vue(
        &host,
        "/src/Prewarm.vue",
        "<script setup lang=\"ts\">\n\
         import { ref } from 'vue';\n\
         const n = ref(1);\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    let profile = content_profile();
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Prewarm.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("prewarm compile");

    // Now load the bare-specifier augmenter that declares
    // `declare module "vue"`. Its augmentations target `"vue"` — the
    // same key the prewarm seeded EMPTY.
    upsert_ts(
        &host,
        "/node_modules/pkg-augment/index.d.ts",
        "declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $foo: string }\n\
         }\n\
         export {};\n",
    );

    // Owner SIDE-EFFECT-imports the augmenter; nothing else
    // references "vue" by binding so only F1's side-effect walk can
    // discover the augmentation.
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import 'pkg-augment';\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    host.set_exact_resolutions(
        "/src/Comp.vue",
        vec![ExactResolution {
            specifier: "pkg-augment".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some("/node_modules/pkg-augment/index.d.ts".to_string()),
            possible_canonical_ids: vec!["/node_modules/pkg-augment/index.d.ts".to_string()],
        }],
    );

    // Snapshot the content-addressed entry count BEFORE compiling
    // Comp.vue. The pre-warm Prewarm.vue compile may have published
    // its own content entry (Prewarm is fact-free and runs as
    // Content) — this prior entry is unrelated to the discriminator.
    // The post-fix property is that the Comp.vue compile must NOT
    // grow the count: a downgraded `Stateless` request publishes
    // nothing.
    let pre_count = host.compile_output_pure_content_entry_count();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request whose bare side-effect augmenter loads AFTER a probe pre-warmed \
         the augmentation_index empty MUST still downgrade to Stateless; the stale-empty \
         entry must be invalidated when the augmenter materialises"
    );
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasModuleAugmentation),
        "the firing reason must be HasModuleAugmentation, got {:?}",
        response.downgrade_reason
    );
    let post_count = host.compile_output_pure_content_entry_count();
    assert_eq!(
        post_count, pre_count,
        "a downgraded Content request must NOT publish a content-addressed entry \
         (pre={pre_count}, post={post_count})"
    );
}

#[test]
fn bare_side_effect_barrel_with_reexported_augmenter_downgrades_to_stateless() {
    // A bare side-effect import (`import "pkg";`) may resolve to a
    // barrel `index.d.ts` that carries NO `ModuleAugmentationFact`
    // entries itself but re-exports the actual augmenter file. F1's
    // walk must follow the barrel's re-export edges and probe each
    // re-exported file's augmentation facts — otherwise a packaged
    // augmenter delivered through a barrel is missed and the owner
    // stays in Content mode despite carrying a module-augmentation
    // dependency.
    //
    // Discriminator: the barrel itself has empty `augmentations`; the
    // augmentation lives in `pkg/augment.d.ts` which the barrel
    // re-exports. The pre-fix walk stops at the barrel → owner stays
    // Content. The post-fix walk follows the re-export chain →
    // discovers the augmenter → owner downgrades to Stateless.
    let host = host();

    // The actual augmenter — declares `declare module "vue"`.
    upsert_ts(
        &host,
        "/node_modules/pkg/augment.d.ts",
        "declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $bar: string }\n\
         }\n\
         export {};\n",
    );
    // The barrel — empty augmentations, re-exports from ./augment.
    upsert_ts(
        &host,
        "/node_modules/pkg/index.d.ts",
        "export * from './augment';\n",
    );
    // Owner SIDE-EFFECT-imports the barrel only. With no relative
    // augmenter, no binding-driven probe target for "vue", and no
    // facts on the barrel itself, the only way the classifier can
    // recognise the augmentation is by walking the barrel's
    // re-export edges to discover the actual augmenter file.
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import 'pkg';\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    host.set_exact_resolutions(
        "/src/Comp.vue",
        vec![ExactResolution {
            specifier: "pkg".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some("/node_modules/pkg/index.d.ts".to_string()),
            possible_canonical_ids: vec!["/node_modules/pkg/index.d.ts".to_string()],
        }],
    );
    // The barrel `export * from "./augment"` must resolve to
    // pkg/augment.d.ts under the live resolver — set the override so
    // the F1 walk's re-export traversal can follow the edge.
    host.set_exact_resolutions(
        "/node_modules/pkg/index.d.ts",
        vec![ExactResolution {
            specifier: "./augment".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::TypeImport,
            resolved_canonical_id: Some("/node_modules/pkg/augment.d.ts".to_string()),
            possible_canonical_ids: vec!["/node_modules/pkg/augment.d.ts".to_string()],
        }],
    );
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request whose owner SIDE-EFFECT-imports a barrel that re-exports an \
         augmenter MUST downgrade to Stateless; the F1 walk must follow re-export edges"
    );
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasModuleAugmentation),
        "the firing reason must be HasModuleAugmentation, got {:?}",
        response.downgrade_reason
    );
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
}
