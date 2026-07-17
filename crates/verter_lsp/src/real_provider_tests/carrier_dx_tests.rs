//! §2.9 plain-`.ts`-imports-`.vue`/`.svelte` enhanced-DX contract, verified on
//! the LIVE tsserver backend (the project-bound external-TS engine).
//!
//! For a carrier-source file (`.vue`/`.svelte`) imported from a PLAIN `.ts`/`.js`
//! file, the imported symbol gets the full enhanced type experience — the same
//! cross-engine/cross-framework contract `docs/arch/external-ts-engine-architecture.md`
//! §2.9 specifies — delivered on TS<7 by the in-process `@verter/typescript-plugin`
//! loaded into production tsserver. A plain script's enhanced `.vue`/`.svelte` DX
//! is served by the plugin making tsserver itself resolve the bare carrier import
//! to the component IDE carrier; Verter's LSP does NOT build a carrier
//! position-mapper for a plain `.ts` (that surface is owned by the editor's own
//! TS channel talking to tsserver). The contract is therefore verified at the
//! PROVIDER level — driving the real tsserver+plugin on the plain `.ts` opened as
//! a configured-project member — which is exactly the layer that delivers it.
//!
//! ## Which response mapper this test exercises (the DIRECT surface)
//!
//! The plain-`.ts`-imports-`.vue` DX is a property of the VS Code DIRECT surface:
//! the editor's OWN TS server loads the plugin and a plain `.ts` talks to the
//! plugin DIRECTLY, with NO verter_lsp in the response path. So the SOLE
//! companion→source response mapper for this surface is the PLUGIN — verter_lsp's
//! Rust merge layer is not reachable from a plain `.ts` (it builds no carrier
//! position-mapper for one, so its definition/references/rename handlers never
//! query the provider for a `.ts` document). This test therefore spawns its
//! provider with the plugin's response remap ENABLED
//! (`plugin_response_remap(true)`) — the DIRECT-surface configuration — and drives
//! the RAW provider, which is exactly the path VS Code's TS channel takes. (The
//! verter_lsp-INTERNAL backend, where the Rust merge layer is the sole mapper and
//! the plugin returns raw companion responses, is a DIFFERENT surface, covered by
//! the carrier-driven code-action / rename / completion lanes that go through
//! `session.server()`.)
//!
//! The fixture (`external-ts-dx`) is hermetic: a directory-`include` tsconfig
//! that OWNS both the `.vue`/`.svelte` carriers AND the plain `.ts` importers,
//! a `Comp.vue` and a `Widget.svelte` each with a uniquely-named public prop,
//! and two plain `.ts` importers. Framework runtime types come from a flat
//! dependency-free `vue` type stub plus the bundled `@verter/types` declaration,
//! materialised at test time (`node_modules` is gitignored repo-wide); so the DX
//! signals depend ONLY on the carrier-import resolution, never on
//! framework-runtime-vendoring noise — no spurious `vue` TS2307 contamination.
//!
//! Each tsserver assertion genuinely depends on the carrier resolving: a
//! definition lands in the `.vue`/`.svelte` SOURCE (not the companion), references
//! reach the component, the Svelte public surface flows, and a non-symbol position
//! fails closed. The tsserver variant RUNS under `VERTER_REQUIRE_TSSERVER=1`.
//!
//! ## The tsgo variant — the CARRIER-SOURCE surface (verter_lsp-owned)
//!
//! tsgo (TS≥7) has NO in-process plugin, so the companion→source response mapping
//! is the verter_lsp Rust merge layer's job — and that mapping is wired ONLY for a
//! document with a carrier PROJECTION (a `.vue`/`.svelte`), never a plain `.ts`. So
//! the tsgo §2.9 contract is asserted on the CARRIER-SOURCE surface through
//! `session.server()` (the real LSP handlers + the merge layer), exactly as the
//! established `*_tsgo` baseline tests assert carrier diagnostics. It runs against
//! the OWNED dual-surface provider landed in this block (`--lsp` features + the
//! attached `--api` typecheck oracle) under `VERTER_REQUIRE_TSGO=1`. The narrowed
//! carrier-surface guard asserts the routing + carrier-offset mapping S5 owns
//! (carrier resolves with no false `TS2307`; go-to-definition on the
//! template-projected prop maps back to the `.vue`/`.svelte`; fail-closed at a
//! non-symbol; no companion-path leak). The both-sides refs/rename + member
//! completion items, and the plain-`.ts`-importer remap, are deferred — see
//! `assert_carrier_dx_contract_carrier_surface` for the precise deferral rationale.

use tower_lsp_server::ls_types::{Diagnostic, NumberOrString};

use crate::test_harness::{RealProviderTestSession, TestProviderKind, TestSessionBuilder};

const FIXTURE: &str = "external-ts-dx";

/// Byte offset of `needle` (+`delta`) in `content`.
fn offset_of(content: &str, needle: &str, delta: usize) -> u32 {
    (content.find(needle).expect("needle present in fixture") + delta) as u32
}

/// Does `diags` carry a provider diagnostic with the given numeric TS code?
fn has_ts_code(diags: &[verter_type_runtime::protocol::TypeDiagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code.as_deref() == Some(code))
}

/// Does `diags` carry an LSP diagnostic (the server's merged set) with the given
/// numeric TS code?
fn has_lsp_code(diags: &[Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| match &d.code {
        Some(NumberOrString::Number(n)) => n.to_string() == code,
        Some(NumberOrString::String(s)) => s == code,
        None => false,
    })
}

/// The single shared §2.9 contract check, run against the live tsserver+plugin.
async fn assert_carrier_dx_contract_tsserver(session: &RealProviderTestSession) {
    // Open the components via the LSP `did_open` so their carriers are PUBLISHED
    // into the on-disk store the plugin reads (the production carrier-publish
    // path); opening the consumer also prewarms its imports' carrier APIs.
    let _comp = session.open_fixture_file("src/Comp.vue").await;
    let _widget = session.open_fixture_file("src/Widget.svelte").await;
    let _consumer_uri = session.open_fixture_file("src/Consumer.ts").await;
    let _second_uri = session.open_fixture_file("src/SecondConsumer.ts").await;

    // Open the plain `.ts` importers DIRECTLY in the provider as real on-disk
    // configured-project members, and query the real tsserver+plugin on them.
    let (consumer, csrc) = session.open_fixture_in_provider("src/Consumer.ts").await;
    let _ = session
        .open_fixture_in_provider("src/SecondConsumer.ts")
        .await;

    // Give tsserver's configured project + the plugin's getExternalFiles time to
    // make the carriers program members (retry on the marquee item below).
    let comp_specifier_off = offset_of(
        &csrc,
        "import Comp from \"./Comp.vue\"",
        "import Comp from \"".len(),
    );

    // ── (2) go-to-definition from the plain `.ts` import lands in the SOURCE ──
    // The marquee §2.9 item AND the readiness gate: a returned target whose path
    // is `Comp.vue` (not the `.vue.tsx`/`.verter.ts` companion) proves the
    // carrier import resolved and was mapped back to source. This genuinely
    // depends on resolution — an unresolved import yields no definition.
    let mut comp_defs = Vec::new();
    for _ in 0..16 {
        comp_defs = session
            .provider()
            .get_definition(&consumer, comp_specifier_off)
            .await
            .unwrap_or_default();
        if !comp_defs.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    if comp_defs.is_empty()
        && session.allow_empty_result_skip(
            "tsserver returned no definition for the `./Comp.vue` import specifier — \
         carrier did not become a program member",
        )
    {
        return;
    }
    assert!(
        comp_defs.iter().any(|d| d.path.ends_with("Comp.vue")),
        "(2) Vue: go-to-definition from the plain `.ts` `./Comp.vue` import must land in \
         the SFC SOURCE (Comp.vue), not a `.vue.tsx`/`.verter.ts` companion; got: {:?}",
        comp_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
    assert!(
        !comp_defs
            .iter()
            .any(|d| d.path.ends_with(".vue.tsx") || d.path.ends_with(".verter.ts")),
        "(2)/(6) Vue: definition must be mapped back to source, never left on a carrier \
         companion path; got: {:?}",
        comp_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );

    let widget_specifier_off = offset_of(
        &csrc,
        "import Widget from \"./Widget.svelte\"",
        "import Widget from \"".len(),
    );
    let widget_defs = session
        .provider()
        .get_definition(&consumer, widget_specifier_off)
        .await
        .unwrap_or_default();
    assert!(
        widget_defs
            .iter()
            .any(|d| d.path.ends_with("Widget.svelte")),
        "(2) Svelte: go-to-definition from the plain `.ts` `./Widget.svelte` import must land \
         in the Svelte SOURCE (Widget.svelte); got: {:?}",
        widget_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
    assert!(
        !widget_defs
            .iter()
            .any(|d| d.path.ends_with(".svelte.tsx") || d.path.ends_with(".verter.ts")),
        "(2)/(6) Svelte: definition must be mapped to source, never a carrier companion; got: {:?}",
        widget_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );

    // ── (1) the component's real public surface flows into the `.ts` ──
    // Vue: the bare import resolves with NO false TS2307 (the import is a typed
    // module, not an unresolved one). Svelte: the public Component surface flows —
    // hover on the imported symbol surfaces Svelte 5's native callable Component
    // type, which only exists when the Svelte carrier resolved. Both are
    // resolution-dependent: a missing carrier yields TS2307
    // (Vue) / a bare `any` import (Svelte).
    let consumer_diags = session
        .provider()
        .get_diagnostics(&consumer)
        .await
        .unwrap_or_default();
    assert!(
        !has_ts_code(&consumer_diags, "2307"),
        "(1) the plain `.ts`'s bare `.vue`/`.svelte` imports must resolve under the \
         configured project — no false TS2307; got: {:?}",
        consumer_diags
            .iter()
            .map(|d| (d.code.clone(), &d.message))
            .collect::<Vec<_>>()
    );

    let widget_use_off = offset_of(
        &csrc,
        "export const widget = Widget;",
        "export const widget = ".len(),
    );
    let widget_hover = session
        .provider()
        .get_hover(&consumer, widget_use_off)
        .await
        .ok()
        .flatten()
        .map(|h| h.contents)
        .unwrap_or_default();
    // Type-definition is the resolution-independent corroborator: it must land in
    // the Svelte source.
    let widget_type_defs = session
        .provider()
        .get_type_definition(&consumer, widget_use_off)
        .await
        .unwrap_or_default();
    assert!(
        widget_hover.contains("Component<")
            || widget_hover.contains("Component<Props")
            || widget_type_defs
                .iter()
                .any(|d| d.path.ends_with("Widget.svelte")),
        "(1) Svelte: the component's public surface must flow into the `.ts` — hover on the \
         imported `Widget` should surface the native Svelte Component type, or \
         type-definition land in Widget.svelte; got hover={widget_hover:?}, type_defs={:?}",
        widget_type_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
    assert!(
        !widget_hover.contains("__VerterPublicInstance")
            && !widget_hover.contains("new (...args")
            && !widget_hover.contains("new (options"),
        "(1) Svelte: hover must not regress to Verter's retired class/constructor shim; \
         got hover={widget_hover:?}"
    );

    // ── (3) find-all-references spans the `.ts` importer(s) AND reaches the component ──
    // From one importer's `Comp` binding, references span the OTHER importer
    // (SecondConsumer.ts) AND reach the component (its `.vue` source or the
    // `.vue.tsx` carrier the LSP maps back to source). This only holds because
    // the import resolved to the shared component carrier.
    let comp_binding_off = offset_of(&csrc, "import Comp from", "import ".len());
    let comp_refs = session
        .provider()
        .get_references(&consumer, comp_binding_off)
        .await
        .unwrap_or_default();
    let ref_paths: Vec<&String> = comp_refs.iter().map(|r| &r.path).collect();
    let distinct_files: std::collections::HashSet<&str> =
        comp_refs.iter().map(|r| r.path.as_str()).collect();
    assert!(
        distinct_files.len() >= 2,
        "(3) Vue: find-all-references from a plain `.ts` importer must span >= 2 files \
         (both importers / the component carrier); got: {ref_paths:?}"
    );
    assert!(
        comp_refs
            .iter()
            .any(|r| r.path.ends_with("SecondConsumer.ts")),
        "(3) Vue: references must include the SECOND plain `.ts` importer; got: {ref_paths:?}"
    );
    assert!(
        comp_refs
            .iter()
            .any(|r| r.path.ends_with("Comp.vue") || r.path.contains("Comp.vue.tsx")),
        "(3) Vue: references must REACH the component (its `.vue` source or the `.vue.tsx` \
         carrier the LSP maps back to source); got: {ref_paths:?}"
    );

    // ── (4) rename of the imported component edits the importer(s) ──
    // tsserver's rename-locations for the Svelte import binding span the importer.
    // (Renaming a default-imported binding is a local-binding rename; the §2.9
    // cross-file member-rename into the component declaration is exercised by the
    // dedicated cross-file Vue-prop rename lane in `rename.rs`.)
    let widget_binding_off = offset_of(&csrc, "import Widget from", "import ".len());
    let widget_rename = session
        .provider()
        .get_rename_locations(&consumer, widget_binding_off)
        .await
        .unwrap_or_default();
    assert!(
        widget_rename
            .iter()
            .any(|r| r.path.ends_with("Consumer.ts")),
        "(4) Svelte: rename of the imported `Widget` binding must edit the `.ts` importer; \
         got: {:?}",
        widget_rename.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
    assert!(
        widget_rename.len() >= 2,
        "(4) Svelte: rename of `Widget` (binding + use) must produce >= 2 edit locations; \
         got: {:?}",
        widget_rename.iter().map(|r| &r.path).collect::<Vec<_>>()
    );

    // ── (5) completion of the component members works ──
    // Auto-import of a component into a `.ts` relies on the eager `CarrierApi`
    // index (§2.7) and is covered by the completion/auto-import lanes; here we
    // assert the tractable provider signal: completion at the consumer's top
    // level surfaces the imported component binding (a resolution-dependent
    // identifier), confirming the import contributes a real value symbol.
    let top_off = offset_of(
        &csrc,
        "export const comp = Comp;",
        "export const comp = ".len(),
    );
    let completions = session
        .provider()
        .get_completions(&consumer, top_off, None)
        .await;
    if let Ok(list) = completions {
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"Comp") && labels.contains(&"Widget"),
            "(5) completion at the consumer must surface the imported component bindings \
             (Comp, Widget); got {} labels: {:?}",
            labels.len(),
            &labels[..labels.len().min(40)]
        );
    }

    // ── (6) unmapped / non-symbol regions fail closed (never a mis-mapped result) ──
    // A definition request at a pure-comment position must return nothing — never
    // a fabricated companion-path or source location.
    let comment_off = offset_of(&csrc, "// A PLAIN `.ts` file", 3);
    let comment_defs = session
        .provider()
        .get_definition(&consumer, comment_off)
        .await
        .unwrap_or_default();
    assert!(
        comment_defs.is_empty(),
        "(6) fail-closed: a definition request at a non-symbol (comment) position must return \
         no result, never a mis-mapped location; got: {:?}",
        comment_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
}

/// The §2.9 contract over the surface verter_lsp OWNS — the carrier-source
/// (`.vue`/`.svelte`) document — driven through `session.server()` (the real LSP
/// handlers + the Rust merge layer). This is the tsgo path: tsgo has NO in-process
/// plugin, so the companion→source mapping is the verter_lsp merge layer's job, and
/// that mapping is wired ONLY for a document with a carrier PROJECTION (a
/// `.vue`/`.svelte`), never a plain `.ts`. So the tsgo §2.9 contract is asserted on
/// the carrier surface, exactly as the established `*_tsgo` baseline diagnostics
/// tests assert through `session.merged_diagnostics(&{vue})`.
///
/// This guard asserts the routing + carrier-offset mapping S5 delivers and owns,
/// every item mapped back through `ProviderPositionMapper`:
/// 1. the carrier resolves under the configured project — NO false `TS2307`
///    (unresolved-module) on the `.vue`/`.svelte` source;
/// 2. go-to-definition on the template-projected prop use lands on the declaration
///    IN the carrier source (carrier→source mapped) — never a companion path;
/// 6. a definition request at a non-symbol (comment) position fails closed
///    (no mis-mapped result), and NO companion path (`.vue.tsx`/`.svelte.tsx`/
///    `.verter.ts`) ever appears in a mapped result.
///
/// DEFERRED (explicitly NOT asserted here — see the deferral note below):
/// * §2.9 items (3) find-all-references / (4) rename "both sides" (script decl ↔
///   template use), and (5) member completion. Empirically these require the
///   carrier to ANALYZE cleanly (verter-native binding extraction produces the
///   script↔template binding edges only on a clean carrier analysis; proven by the
///   hermetic `crate::features::rename::tests::test_rename_binding_across_blocks`).
///   The `external-ts-dx` fixture's minimal `vue` stub / `@verter/types` does NOT
///   give the carrier a clean analysis (the Vue compiler macros are not ambient →
///   `TS2304 defineProps`, the prop surface degrades to `any`, no bindings are
///   extracted), so on THIS fixture refs/rename return only the self-occurrence
///   side. Building a Vue-macro/JSX/type-runtime conformance fixture, and validating
///   against the authoritative `typescript@7.0.2` engine (not the dev channel),
///   is the tracked follow-up for the both-sides refs/rename + completion items.
/// * The plain-`.ts`-importer raw companion→source remap for def/refs/rename (the
///   editor's-own-TS-channel surface for a plain `.ts`) is the §2.10 SHARED-proxy
///   concern (a future SHARED-proxy workstream), NOT a guarantee of the current
///   external-TS engine work — verter_lsp builds no carrier mapper for a plain `.ts`.
async fn assert_carrier_dx_contract_carrier_surface(session: &RealProviderTestSession) {
    // Open every fixture document through the LSP server so each carrier is compiled
    // (its `ProviderPositionMapper` built) AND didOpen'd into the OWNED tsgo `--lsp`
    // session (project-bound membership). Opening the consumers prewarms imports.
    let comp_uri = session.open_fixture_file("src/Comp.vue").await;
    let comp_state = session
        .provider_sync_state(&comp_uri)
        .expect("opening an owned Vue carrier must commit provider state");
    assert!(
        comp_state.ide_background_loaded
            && comp_state
                .ide_path
                .as_deref()
                .is_some_and(|path| path.ends_with("Comp.vue.tsx"))
            && comp_state.committed_ide_surface.is_some(),
        "opening an owned Vue carrier must make its receipt-attested IDE TSX a live project \
         member before template navigation; got: {comp_state:#?}"
    );
    let widget_uri = session.open_fixture_file("src/Widget.svelte").await;
    // The plain `.ts` importers are opened so their bare `.vue`/`.svelte` imports
    // make the carriers project members (membership prewarm); their own definition/
    // rename surface is the deferred plain-`.ts` surface, not asserted here.
    let _consumer_uri = session.open_fixture_file("src/Consumer.ts").await;
    let _second_uri = session.open_fixture_file("src/SecondConsumer.ts").await;

    // Helper: does any location/edit URI carry a carrier-companion path? (the leak
    // the merge layer must never produce.)
    fn no_companion(uris: &[String]) -> bool {
        uris.iter().all(|u| {
            !u.ends_with(".vue.tsx")
                && !u.ends_with(".svelte.tsx")
                && !u.contains(".verter.ts")
                && !u.ends_with(".vue.jsx")
                && !u.ends_with(".svelte.jsx")
        })
    }

    // ── (1) Vue: the carrier resolves under the configured project — no false TS2307. ──
    // `merged_diagnostics` already retries internally for the async OWNED tsgo
    // membership (didOpen overlay + `--api` updateSnapshot). The carrier's own
    // imports resolve under the configured project; a false `TS2307` would mean the
    // carrier was placed in a config-less inferred project (the inferred-project bug
    // S3 fixed). The positive readiness gate is the definition in item (2).
    let comp_diags = session.merged_diagnostics(&comp_uri).await;
    assert!(
        !has_lsp_code(&comp_diags, "2307"),
        "(1) Vue: the carrier must resolve under the configured project — no false TS2307; \
         got: {:?}",
        comp_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // ── (2) Vue go-to-definition: marquee item + readiness gate. ──
    // From the TEMPLATE use `{{ verterDxHeadline }}`, definition must land on the
    // `defineProps<{ verterDxHeadline }>` declaration IN `Comp.vue` (carrier→source
    // mapped). This genuinely depends on the OWNED tsgo `--lsp` resolving the
    // template-projected prop and the merge layer mapping the TSX span back through
    // `ProviderPositionMapper` — the carrier-offset mapping S5 delivers.
    let tmpl_prop = session.find_nth_position(&comp_uri, "verterDxHeadline", 1, 0);
    let mut comp_defs = Vec::new();
    for _ in 0..16 {
        comp_defs = session.definition_locations(&comp_uri, tmpl_prop).await;
        if !comp_defs.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    if comp_defs.is_empty()
        && session.allow_empty_result_skip(
            "tsgo --lsp returned no definition for the Vue template prop — carrier did not \
             become a project member",
        )
    {
        return;
    }
    let comp_def_uris: Vec<String> = comp_defs
        .iter()
        .map(|l| l.uri.as_str().to_string())
        .collect();
    assert!(
        comp_def_uris.iter().any(|u| u.ends_with("Comp.vue")),
        "(2) Vue: definition on the template `verterDxHeadline` use must land in the `.vue` \
         SOURCE (carrier→source mapped); got: {comp_def_uris:?}"
    );
    assert!(
        no_companion(&comp_def_uris),
        "(2)/(6) Vue: definition must be mapped back to source, never a carrier companion path; \
         got: {comp_def_uris:?}"
    );

    // ── (6) Vue fail-closed: a definition request at a non-symbol (comment) position. ──
    let comment_pos = session.find_position(&comp_uri, "A Vue SFC whose public", 0);
    let comment_defs = session.definition_locations(&comp_uri, comment_pos).await;
    assert!(
        comment_defs.is_empty(),
        "(6) Vue: a definition request at a comment position must return nothing (fail closed); \
         got: {:?}",
        comment_defs
            .iter()
            .map(|l| l.uri.as_str())
            .collect::<Vec<_>>()
    );

    // ── Svelte: the same routing/mapping contract for the Svelte adapter. ──
    // (1) the carrier resolves (no false TS2307); (2) definition on the markup prop
    // use lands on the `export let verterDxCaption` declaration in `Widget.svelte`,
    // carrier→source mapped, never a companion path.
    let sv_diags = session.merged_diagnostics(&widget_uri).await;
    assert!(
        !has_lsp_code(&sv_diags, "2307"),
        "(1) Svelte: the carrier must resolve under the configured project — no false TS2307; \
         got: {sv_diags:?}"
    );
    let sv_markup_prop = session.find_nth_position(&widget_uri, "verterDxCaption", 1, 0);
    let mut sv_defs = Vec::new();
    for _ in 0..16 {
        sv_defs = session
            .definition_locations(&widget_uri, sv_markup_prop)
            .await;
        if !sv_defs.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let sv_def_uris: Vec<String> = sv_defs.iter().map(|l| l.uri.as_str().to_string()).collect();
    if sv_defs.is_empty()
        && session
            .allow_empty_result_skip("tsgo --lsp returned no definition for the Svelte markup prop")
    {
        return;
    }
    assert!(
        sv_def_uris.iter().any(|u| u.ends_with("Widget.svelte")),
        "(2) Svelte: definition on the markup `verterDxCaption` use must land in the `.svelte` \
         SOURCE; got: {sv_def_uris:?}"
    );
    assert!(
        no_companion(&sv_def_uris),
        "(2)/(6) Svelte: definition must be mapped to source, never a companion path; got: \
         {sv_def_uris:?}"
    );
}

/// §2.9 contract on the LIVE tsserver backend (TS<7, the in-process plugin).
/// Vue AND Svelte. RUNS under `VERTER_REQUIRE_TSSERVER=1` (a skip is a failure
/// there); degrades gracefully when the toolchain is absent locally.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_dx_enhanced_both_engines_both_frameworks_tsserver() {
    crate::test_harness::materialize_external_ts_dx_deps();
    // The DIRECT surface: the plugin is the sole companion→source response mapper
    // (no verter_lsp in a plain `.ts`'s response path), so spawn it with response
    // remap ENABLED and drive the RAW provider — exactly VS Code's TS channel.
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsserver)
        .fixture(FIXTURE)
        .plugin_response_remap(true)
        .build()
        .await
    else {
        return;
    };
    assert_carrier_dx_contract_tsserver(&session).await;
    session.shutdown().await;
}

/// §2.9 contract on the OWNED tsgo backend (TS≥7) — the project-bound dual-surface
/// provider landed in this block. ALL interactive features served by tsgo's `--lsp`
/// surface, mapped back through `ProviderPositionMapper` + the Rust merge layer (the
/// one-surface rule; `--api` is the typecheck/membership oracle, never the
/// interactive features). Asserted on the CARRIER-SOURCE surface verter_lsp owns
/// (`session.server()`), exactly as the established `*_tsgo` baseline tests assert
/// carrier diagnostics — NOT the plain-`.ts`-importer raw-provider surface (whose
/// companion→source remap for tsgo is the §2.10 SHARED-proxy concern, a future
/// SHARED-proxy workstream).
///
/// RUNS under `VERTER_REQUIRE_TSGO=1` (a skip is a failure there); degrades
/// gracefully when tsgo is absent locally. Vue AND Svelte.
#[tokio::test(flavor = "multi_thread")]
async fn carrier_dx_enhanced_both_engines_both_frameworks_tsgo() {
    crate::test_harness::materialize_external_ts_dx_deps();
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
        .fixture(FIXTURE)
        .build()
        .await
    else {
        return;
    };
    assert_carrier_dx_contract_carrier_surface(&session).await;
    session.shutdown().await;
}
