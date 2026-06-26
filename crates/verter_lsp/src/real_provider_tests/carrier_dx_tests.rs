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
//! Each assertion genuinely depends on the carrier resolving: a definition lands
//! in the `.vue`/`.svelte` SOURCE (not the companion), references reach the
//! component, the Svelte public surface flows, and a non-symbol position fails
//! closed. The tsserver variant RUNS under `VERTER_REQUIRE_TSSERVER=1`; the tgo
//! variant rides the gate-proven `--api` FS-overlay + carrier-extension
//! redirection and is `#[ignore]`d with reason until the tgo backend is migrated
//! onto the project-bound contract (consistent with the external-TS baseline
//! `*_tsgo` split).

use crate::test_harness::{RealProviderTestSession, TestProviderKind, TestSessionBuilder};

const FIXTURE: &str = "external-ts-dx";

/// Byte offset of `needle` (+`delta`) in `content`.
fn offset_of(content: &str, needle: &str, delta: usize) -> u32 {
    (content.find(needle).expect("needle present in fixture") + delta) as u32
}

/// Does `diags` carry a diagnostic with the given numeric TS code?
fn has_ts_code(diags: &[verter_type_runtime::protocol::TypeDiagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code.as_deref() == Some(code))
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
    // module, not an unresolved one). Svelte: the public instance surface flows —
    // hover on the imported symbol surfaces the synthesized public component type
    // (`__VerterPublicInstance`), which only exists when the Svelte carrier
    // resolved. Both are resolution-dependent: a missing carrier yields TS2307
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
        widget_hover.contains("__VerterPublicInstance")
            || widget_hover.contains("__VerterPublicComponent")
            || widget_type_defs
                .iter()
                .any(|d| d.path.ends_with("Widget.svelte")),
        "(1) Svelte: the component's public surface must flow into the `.ts` — hover on the \
         imported `Widget` should surface the synthesized public component/instance type, or \
         type-definition land in Widget.svelte; got hover={widget_hover:?}, type_defs={:?}",
        widget_type_defs.iter().map(|d| &d.path).collect::<Vec<_>>()
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

/// §2.9 contract on the tgo backend (TS≥7, the gate-proven `--api` FS-overlay +
/// carrier-extension redirection). `#[ignore]`d until the tgo engine is migrated
/// onto the project-bound contract — the same split as the external-TS baseline
/// `*_tsgo` lanes. The tgo redirection + types-flow of item (1) is already proven
/// by the committed `tools/tsgo-api-gate/` GATE 4; the remaining contract items
/// follow once the tgo backend serves them. A REAL fixture + REAL assertions (the
/// same contract body), so it goes green by deleting the `#[ignore]` once tgo is
/// live.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "tgo half of carrier_dx_enhanced_both_engines_both_frameworks: enabled once the tgo --api overlay backend serves the project-bound contract (tsserver half is live)"]
async fn carrier_dx_enhanced_both_engines_both_frameworks_tsgo() {
    crate::test_harness::materialize_external_ts_dx_deps();
    let Some(session) = TestSessionBuilder::new(TestProviderKind::Tsgo)
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
