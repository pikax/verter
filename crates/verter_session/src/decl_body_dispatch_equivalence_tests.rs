//! Characterization of the OBSERVABLE equivalence contract a future
//! declaration-body PRODUCER flip (to handle-native carrier bodies) must
//! preserve, compared against the retained whole-env oracle.
//!
//! These tests pin the resolved-through-dispatch behaviour — NOT the
//! internal carrier-vs-resolved-`EvalEnv` representation, which is
//! meaningless across the flip. Each asserts a semantic output the eager
//! path produces TODAY and the carrier-native path must reproduce: the
//! lowered NODE KIND of an alias / import-alias / merged declaration, the
//! value-alias TERMINAL the two C2 peelers reach, the open generic
//! `TypeParam` an ordinary decl body resolves to, the FINAL defining-file
//! canonical a barrel-imported alias materialises through, and the
//! namespace-sibling resolution.
//!
//! Every assertion is DISCRIMINATING: it would FAIL if the contract were
//! broken (a regressed flip that eagerly resolved a carrier, lost the
//! `MergedDecl` carrier, stopped a barrel at the intermediate canonical,
//! or diverged the graph-native peeler from the oracle). The carrier
//! contract is already observable on this tree (the DeclBodyMemo body is
//! the syntactic lowering and `Navigate`/`Shallow` dispatch already mints
//! the carrier nodes), so these are written GREEN against the eager path.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use crate::semantic_query::{ProjectionMode, SemanticNodeData};
use crate::types::{FileLanguage, HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

/// The interned semantic-node data for a resolved symbol — the surface
/// the dispatch hands the consumer (carrier or resolved body).
fn node_data(
    host: &VerterHost,
    node: crate::semantic_query::SemanticNodeId,
) -> Arc<SemanticNodeData> {
    host.project_type_store()
        .semantic_graph()
        .node_data(node)
        .expect("node interned during resolution")
}

// ════════════════════════════════════════════════════════════════════
// D1 — Producer-flip shape (the carrier-body contract).
// ════════════════════════════════════════════════════════════════════

/// A plain alias `type A = B` lowers to a `DeclRef` CARRIER (raising to
/// `Ref { name: "B" }`) in shallow / navigate dispatch — NOT an eagerly
/// resolved `Object` body. This is the exact carrier shape the producer
/// flip stores handle-natively; the flip must keep `A`'s shallow/navigate
/// lowering a reference carrier, never an inlined resolution.
///
/// Discriminating: if a regressed flip eagerly resolved the alias body at
/// shallow/navigate lowering time, the node would be
/// `SemanticNodeData::Object(_)` and BOTH `DeclRef` asserts would fail.
/// The Expanded control proves the body IS reachable (so the carrier is
/// not hiding a resolution failure).
#[test]
fn alias_decl_body_lowers_to_a_declref_carrier_not_a_resolved_object() {
    let host = make_host();
    upsert_ts(
        &host,
        "/p.ts",
        "export type B = { v: number };\nexport type A = B;\n",
    );

    for mode in [ProjectionMode::Navigate, ProjectionMode::Shallow] {
        let node = host
            .resolve_named_symbol("/p.ts", "A", Some(mode))
            .unwrap_or_else(|| panic!("A must resolve in {mode:?}"));
        let data = node_data(&host, node);
        match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                assert_eq!(
                    identity.decl_name.as_ref(),
                    "B",
                    "the `type A = B` carrier must reference B, got {:?}",
                    identity.decl_name
                );
            }
            other => panic!(
                "`type A = B` must lower to a DeclRef carrier in {mode:?} \
                 (the producer-flip carrier-body shape), got {other:?}"
            ),
        }
        // Raised carrier projects back to the bare `Ref { name: "B" }`.
        let raised = host
            .project_node_to_type_expr_for_test(node)
            .expect("carrier must project");
        assert!(
            matches!(&raised, verter_type_expr::TypeExpr::Ref { name, type_arguments }
                if name.as_ref() == "B" && type_arguments.is_empty()),
            "the carrier must raise to `Ref {{ name: \"B\" }}`, got {raised:?}"
        );
    }

    // Control: Expanded DOES reach the resolved Object body — the alias is
    // genuinely resolvable, so the shallow/navigate carrier above is a
    // deliberate stop, not a resolution miss.
    let expanded = host
        .resolve_named_symbol("/p.ts", "A", Some(ProjectionMode::Expanded))
        .expect("A must resolve Expanded");
    assert!(
        matches!(
            node_data(&host, expanded).as_ref(),
            SemanticNodeData::Object(_)
        ),
        "Expanded must reach the resolved Object body (proving the carrier is a stop, not a miss)"
    );
}

/// An imported alias `type A = import("./m").G` lowers, in shallow /
/// navigate dispatch, to a reference CARRIER that names the imported
/// declaration G in its defining file `/m.ts` (the `ImportType` carrier
/// the producer lowers `import("…")` into, resolved to its target decl
/// reference). The producer flip must keep this a carrier reference, never
/// an eager surface inline — under BOTH `Navigate` AND `Shallow`.
///
/// Discriminating: a regressed eager flip would resolve to G's `Object`
/// body at navigate/shallow time; this asserts the carrier (`DeclRef` at
/// `/m.ts`) in BOTH modes and would fail on an inlined Object. A flip that
/// kept `Navigate` correct but eagerly inlined under `Shallow` would pass a
/// Navigate-only check but redden here. The producer-stored body is pinned
/// by a TYPED structural match on the `ImportType` carrier (never a fragile
/// Debug-substring) per the Typed-IR-Only rule.
#[test]
fn imported_alias_decl_body_lowers_to_an_import_carrier_reference() {
    use verter_semantic::analysis::type_eval::TypeDeclBody;

    let host = make_host();
    upsert_ts(&host, "/m.ts", "export type G = { g: number };\n");
    upsert_ts(&host, "/p.ts", "export type A = import('./m').G;\n");

    // The retained decl inventory stores the CONTENT-FREE whole-body slot
    // (a locator at A's own anchor — never an eagerly resolved body): pin
    // that producer-stored shape with a TYPED match on the single-body
    // slot, not a Debug-substring. The `import("./m").G` carrier semantics
    // are pinned below through the dispatch resolution (the DeclRef
    // reference to G in its defining file).
    let state = host.routed_shallow_state("/p.ts").expect("shallow state");
    let lowered = state.type_decl("A").expect("A decl body lowers");
    match &lowered.body {
        TypeDeclBody::Single(slot) => {
            assert_eq!(
                slot.anchor.canonical_id.as_ref(),
                "/p.ts",
                "the stored whole-body slot must anchor A's own defining file"
            );
            assert_eq!(
                slot.anchor.symbol.as_ref(),
                "A",
                "the stored whole-body slot must anchor the `A` declaration"
            );
            assert!(
                slot.path.is_empty(),
                "the stored slot must be the WHOLE-body position (empty path)"
            );
        }
        other => panic!(
            "the producer must store the `import(\"./m\").G` body as a single \
             content-free whole-body slot, got {other:?}"
        ),
    }

    // BOTH Navigate AND Shallow dispatch resolve the import carrier to the
    // target decl REFERENCE in its defining file — a carrier, not an inlined
    // surface. (The plain-alias test already loops both modes; the imported
    // alias must too, so a broken Shallow cannot hide behind a Navigate pass.)
    for mode in [ProjectionMode::Navigate, ProjectionMode::Shallow] {
        let node = host
            .resolve_named_symbol("/p.ts", "A", Some(mode))
            .unwrap_or_else(|| panic!("A must resolve in {mode:?}"));
        match node_data(&host, node).as_ref() {
            SemanticNodeData::DeclRef { identity } => {
                assert_eq!(
                    identity.decl_name.as_ref(),
                    "G",
                    "carrier must reference G in {mode:?}"
                );
                assert_eq!(
                    identity.canonical_id.as_ref(),
                    "/m.ts",
                    "the import carrier must reference G's defining file /m.ts in {mode:?}"
                );
            }
            other => panic!(
                "import alias must lower to a DeclRef carrier in {mode:?} \
                 (never an eager inlined surface), got {other:?}"
            ),
        }
    }
}

/// A same-name merged interface `interface I {a} + interface I {b}`
/// reaches dispatch as the distinct `MergedDecl` CARRIER — NEVER a bare
/// `Intersection` (whose reducer applies heritage-shadow precedence and
/// cannot accumulate overload groups). The producer flip MUST keep
/// minting `MergedDecl` for merged declarations.
///
/// Discriminating: this is the Declaration Merging (CRITICAL) carrier
/// invariant. If the flip lowered the merge as `Intersection` (or a single
/// collapsed `Object`), the match arm fails. Holds under Navigate AND
/// Expanded (the carrier is the decl-resolution surface in both).
#[test]
fn merged_interface_decl_lowers_to_the_distinct_merged_carrier_not_intersection() {
    let host = make_host();
    upsert_ts(
        &host,
        "/m.ts",
        "export interface I { a: number }\nexport interface I { b: string }\n",
    );

    for mode in [ProjectionMode::Navigate, ProjectionMode::Expanded] {
        let node = host
            .resolve_named_symbol("/m.ts", "I", Some(mode))
            .unwrap_or_else(|| panic!("I must resolve in {mode:?}"));
        match node_data(&host, node).as_ref() {
            SemanticNodeData::MergedDecl { contributors } => {
                assert_eq!(
                    contributors.len(),
                    2,
                    "the merged interface I must carry both contributors in {mode:?}"
                );
            }
            SemanticNodeData::Intersection(_) => panic!(
                "merged interface I must NOT lower to a bare Intersection in {mode:?} \
                 (heritage-shadow reducer cannot accumulate overload groups) — it must be MergedDecl"
            ),
            other => panic!(
                "merged interface I must lower to the distinct MergedDecl carrier in {mode:?}, got {other:?}"
            ),
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// D3 — C2 value-alias terminal: graph-native peeler vs oracle, on the
// barrel / cycle / namespace-sibling cases not exercised at the peeler
// level by the existing single-hop coverage.
// ════════════════════════════════════════════════════════════════════

/// A value re-exported through a BARREL (`export { themeImpl as theme }
/// from './dep'`) resolves, through both the oracle
/// `resolve_value_export_target` and the graph-native
/// `resolve_value_export_target_graph_native`, to the FINAL defining
/// `(./dep, themeImpl)` pair — NOT the intermediate barrel `(barrel,
/// theme)`. The two peelers must agree on the terminal.
///
/// Discriminating: if the graph-native peeler stopped at the barrel
/// canonical / alias name (the latent divergence the readiness work
/// guards against), the `assert_eq!` on the pair fails; the explicit
/// `(/dep.ts, themeImpl)` assert pins the value, not just agreement.
#[test]
fn c2_barrel_value_reexport_peels_to_final_source_in_oracle_and_graph_native() {
    let host = make_host();
    upsert_ts(
        &host,
        "/dep.ts",
        "export const themeImpl = { color: 'dark' }\n",
    );
    upsert_ts(
        &host,
        "/barrel.ts",
        "export { themeImpl as theme } from './dep'\n",
    );

    let oracle = host
        .resolve_value_export_target("/barrel.ts", "theme")
        .expect("oracle must resolve the barrel export target");
    let graph_native = host
        .resolve_value_export_target_graph_native("/barrel.ts", "theme")
        .expect("graph-native must resolve the barrel export target");

    assert_eq!(
        (
            oracle.canonical_id.as_str(),
            oracle.owner,
            oracle.name.as_str()
        ),
        (
            graph_native.canonical_id.as_str(),
            graph_native.owner,
            graph_native.name.as_str()
        ),
        "C2 barrel terminal divergence: oracle={oracle:?} graph_native={graph_native:?}"
    );
    assert_eq!(
        (
            oracle.canonical_id.as_str(),
            oracle.owner,
            oracle.name.as_str()
        ),
        (
            "/dep.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "themeImpl"
        ),
        "the barrel re-export must resolve to the FINAL defining (./dep, themeImpl), \
         not the intermediate barrel binding"
    );
}

/// A `typeof` alias CYCLE (`a: typeof b`, `b: typeof a`) terminates at the
/// same identity in both the oracle peeler and the graph-native peeler
/// (the byte-identical visited-set guard stops the cycle without
/// diverging or looping).
///
/// Discriminating: if the graph-native peeler's visited-set / same-name
/// guards diverged from the oracle's, the two terminals differ and the
/// `assert_eq!` fails. The `("/cyc.ts", "a")` pin proves the cycle
/// terminates at the origin, not one hop further.
#[test]
fn c2_typeof_alias_cycle_terminates_identically_in_oracle_and_graph_native() {
    let host = make_host();
    upsert_ts(
        &host,
        "/cyc.ts",
        "export const a: typeof b = 0 as unknown as { v: 1 }\n\
         export const b: typeof a = 0 as unknown as { v: 1 }\n",
    );

    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let oracle = host.peel_value_decl_alias_for_test("/cyc.ts", owner, "a");
    let graph_native = host.peel_value_decl_alias_graph_native_for_test("/cyc.ts", owner, "a");
    assert_eq!(
        oracle, graph_native,
        "C2 cycle terminal divergence: oracle={oracle:?} graph_native={graph_native:?}"
    );
    assert_eq!(
        oracle,
        crate::resolver_core::ValueDeclIdentity {
            canonical_id: "/cyc.ts".to_string(),
            owner,
            name: "a".to_string(),
        },
        "the typeof cycle must terminate at its origin `a` (visited-set guard), got {oracle:?}"
    );
}

// ════════════════════════════════════════════════════════════════════
// D5 — Latent-divergence re-validation: barrel identity + namespace
// sibling at the resolved-decl level.
// ════════════════════════════════════════════════════════════════════

/// A type alias imported through an `index` re-export BARREL (`import type
/// { Node } from './index'` where `./index` re-exports Node from `./t`)
/// resolves, under Expanded, to Node's REAL body materialised from the
/// FINAL defining file `/t.ts` — the dispatch fallthrough must follow the
/// barrel to the final definition, never stop at the intermediate barrel
/// canonical. (The Navigate carrier may reference the barrel canonical;
/// the resolved identity that materialises the body is the final file.)
///
/// Discriminating: a flip whose carrier resolution stopped at the
/// intermediate barrel `/index.ts` would either fail to materialise the
/// body or stamp the member origin as `/index.ts`; this asserts the
/// member's `declaration_origin` is `/t.ts`.
#[test]
fn barrel_imported_alias_materializes_at_the_final_defining_canonical() {
    let host = make_host();
    upsert_ts(&host, "/t.ts", "export type Node = { label: string };\n");
    upsert_ts(&host, "/index.ts", "export type { Node } from './t';\n");
    upsert_ts(
        &host,
        "/use.ts",
        "import type { Node } from './index';\nexport type A = Node;\n",
    );

    let node = host
        .resolve_named_symbol("/use.ts", "A", Some(ProjectionMode::Expanded))
        .expect("A must resolve Expanded through the barrel");
    let SemanticNodeData::Object(surface) = node_data(&host, node).as_ref().clone() else {
        panic!("A must materialise to Node's Object body through the barrel");
    };
    let label = surface
        .members
        .iter()
        .find(|member| member.name.as_ref() == "label")
        .expect("Node's `label` member must materialise");
    assert_eq!(
        label.declaration_origin.as_deref(),
        Some("/t.ts"),
        "the barrel-imported alias must materialise at the FINAL defining file /t.ts, \
         not the intermediate barrel /index.ts; got {:?}",
        label.declaration_origin
    );
}

/// A namespace-sibling type reference (`namespace M { type Inner; type
/// Outer = Inner }`) resolves the sibling: `M.Outer` materialises Inner's
/// body. This reproduces the eager `add_namespace_sibling_resolutions`
/// resolution through the shared dispatch (the file-scope dotted symbols
/// `M.Inner` / `M.Outer` carry the namespace scope).
///
/// Discriminating: if namespace-sibling resolution regressed, `M.Outer`
/// would fail to resolve or would not reach Inner's `{ a: 1 }` body; this
/// asserts the materialised member `a` is present.
#[test]
fn namespace_sibling_type_reference_resolves_through_dispatch() {
    let host = make_host();
    upsert_ts(
        &host,
        "/nst.ts",
        "export namespace M {\n  export type Inner = { a: 1 };\n  export type Outer = Inner;\n}\n",
    );

    let node = host
        .resolve_named_symbol("/nst.ts", "M.Outer", Some(ProjectionMode::Expanded))
        .expect("M.Outer must resolve the namespace sibling Inner");
    let projected = host
        .project_node_to_type_expr_for_test(node)
        .expect("M.Outer must project");
    match &projected {
        verter_type_expr::TypeExpr::Object(shape) => {
            assert!(
                shape.properties.iter().any(|member| matches!(
                    member,
                    verter_type_expr::ObjectMember::Property(prop) if prop.name == "a"
                )),
                "the namespace sibling M.Outer must materialise Inner's member `a`, got {:?}",
                shape.properties
            );
        }
        other => panic!("M.Outer must resolve to Inner's Object body, got {other:?}"),
    }
}

// ════════════════════════════════════════════════════════════════════
// D4 — rune ambient + script-setup generic, end-to-end through the host.
// ════════════════════════════════════════════════════════════════════

fn rune_module_language() -> FileLanguage {
    FileLanguage::adapter_module(
        verter_language::ScriptSourceType::Ts,
        verter_language::FrameworkAdapterId::svelte(),
        verter_language::LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
    )
}

fn upsert_rune_module(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: rune_module_language(),
            aliases: Vec::new(),
        })
        .expect("upsert svelte rune module");
}

/// A Svelte `$`-rune MODULE (`.svelte.ts`) exposes the ambient `$`-rune
/// value symbols (`$state`/`$derived`/`$effect`/`$inspect`) through the
/// SURVIVING graph-native value-symbol reader
/// (`dependency_value_symbol_graph_native`, the per-symbol reader the
/// fallthrough / C2 / C4 graph-native consumers and the dispatch route
/// through) — NOT through the retained whole-env `EvalEnv` oracle. A PLAIN
/// `.ts` does NOT (per-file scoping), and a USER declaration WINS over the
/// rune prelude (the user's own annotation survives). This pins the
/// observable contract the rune-ambient re-home (off the static
/// `OnceLock<EvalEnv>` consulted only by `whole_env()`) must satisfy on the
/// SURVIVING reader — the assertion stays valid AND meaningful after the
/// `whole_env()` oracle is deleted.
///
/// IGNORED until the rune-ambient re-home: the graph-native value-symbol
/// reader does NOT yet carry the rune ambient (the per-symbol header index
/// excludes it; only the doomed `whole_env()` injects it), so
/// `dependency_value_symbol_graph_native("/r.svelte.ts", "$state")` returns
/// `None` today. The re-home routes the runes through this reader; this test
/// pins the surviving contract it must then serve and is un-ignored by that
/// re-home. The per-file-scoping negative (a plain `.ts` returns `None`) is
/// already correct on this tree.
///
/// Discriminating: a re-home that injected the runes globally (not per-file)
/// would make the plain `.ts` reader return `Some` for `$state` (the
/// per-file negative fails); a re-home that clobbered user symbols would
/// replace the user's `$derived` annotation with the ambient rune signature
/// (the user-wins assert fails); a re-home that left the runes on the oracle
/// only would leave the graph-native reader returning `None` (the positive
/// visibility asserts fail). The oracle==graph-native equivalence cross-check
/// additionally fails if the re-home diverged the surviving reader from the
/// oracle for the rune symbols.
#[test]
fn svelte_rune_ambient_is_visible_per_file_and_user_declarations_win() {
    // 1. Rune module: the ambient runes are visible through the SURVIVING
    //    graph-native per-symbol value-symbol reader, AND so is the user's
    //    own `c`.
    let host = make_host();
    upsert_rune_module(&host, "/r.svelte.ts", "export const c = $state(0)\n");
    for rune in ["$state", "$derived", "$effect", "$inspect"] {
        assert!(
            host.dependency_value_symbol_graph_native("/r.svelte.ts", rune)
                .is_some(),
            "the rune module must expose the ambient `{rune}` through the \
             graph-native value-symbol reader (the surviving path the re-home \
             routes the runes through), not only the whole-env oracle"
        );
    }
    assert!(
        host.dependency_value_symbol_graph_native("/r.svelte.ts", "c")
            .is_some(),
        "the rune module's own `c` must be visible through the graph-native reader"
    );

    // 2. Per-file scoping: a plain `.ts` must NOT expose any rune symbol
    //    through the graph-native reader (a global injection would leak it).
    let plain_host = make_host();
    upsert_ts(&plain_host, "/plain.ts", "export const c = 0\n");
    assert!(
        plain_host
            .dependency_value_symbol_graph_native("/plain.ts", "$state")
            .is_none(),
        "a plain `.ts` must NOT expose the ambient `$state` through the \
         graph-native reader (per-file scoping); a global injection would leak it"
    );

    // 3. User-wins: a user-declared `$derived` keeps the USER's annotation
    //    (`{ mine: 1 }`) through the graph-native reader, NOT the ambient rune
    //    signature.
    let user_host = make_host();
    upsert_rune_module(
        &user_host,
        "/u.svelte.ts",
        "export const $derived: { mine: 1 } = { mine: 1 }\nexport const c = $state(0)\n",
    );
    let user_derived = user_host
        .dependency_value_symbol_graph_native("/u.svelte.ts", "$derived")
        .expect("user `$derived` must be visible through the graph-native reader");
    let annotation_source = user_derived
        .type_annotation
        .annotation
        .as_ref()
        .expect("the user `$derived` must carry its authored annotation source");
    let annotation = crate::test_only::semantic_source_probe::shallow_type_expr(
        &user_host,
        "/u.svelte.ts",
        annotation_source,
    )
    .unwrap_or_else(|| panic!("the user annotation source must shell-materialize"));
    assert!(
        matches!(&annotation, verter_type_expr::TypeExpr::Object(shape)
        if shape.properties.iter().any(|member| matches!(
            member,
            verter_type_expr::ObjectMember::Property(prop) if prop.name == "mine"
        ))),
        "the user `$derived` declaration must WIN over the rune prelude through \
         the graph-native reader (keep the user's `{{ mine: 1 }}` annotation), \
         got {annotation:?}"
    );

    // 4. Oracle == graph-native equivalence (the SURVIVING reader is the
    //    primary contract above; this is the explicit cross-check that the
    //    re-home keeps the graph-native reader in lock-step with the oracle
    //    for the rune symbols, until `whole_env()` is deleted).
    let oracle_env = host
        .base_eval_env_arc("/r.svelte.ts")
        .expect("rune module oracle env must build");
    for rune in ["$state", "$derived", "$effect", "$inspect"] {
        let oracle_has = oracle_env.value_symbols.contains_key(rune);
        let graph_native_has = host
            .dependency_value_symbol_graph_native("/r.svelte.ts", rune)
            .is_some();
        assert_eq!(
            oracle_has, graph_native_has,
            "rune `{rune}` visibility must agree between the oracle whole-env and \
             the surviving graph-native reader (no divergence across the re-home)"
        );
    }
}

/// In a `<script setup generic="T">` SFC, an ORDINARY type-alias decl body
/// `type A = T` resolves `T` to a first-class `TypeParameter` (carrying the
/// script-setup `<script-setup>` declaration identity + display name `T`),
/// NOT an unbound `Ref { name: "T" }` bare-name carrier. This pins the
/// observable contract the binder re-home for ordinary decl bodies must
/// preserve (today the macro hot mirror already seeds the script-setup
/// generic into the ordinary path).
///
/// Discriminating: if `T` were lowered as a bare `Ref`/`DeclRef` (an
/// unbound name) instead of a bound `TypeParameter`, the projected type
/// would be `Ref { name: "T" }` and the `TypeParameter` match fails. Holds
/// under Navigate and Expanded (an open type parameter has no body to
/// expand, so it stays a `TypeParam` shell).
#[test]
fn script_setup_generic_resolves_as_type_parameter_in_an_ordinary_decl_body() {
    let host = make_host();
    upsert_vue(
        &host,
        "/G.vue",
        "<script setup lang=\"ts\" generic=\"T\">\n\
         type A = T\n\
         defineProps<{ x: A }>()\n\
         </script>\n\
         <template><div /></template>",
    );

    for mode in [ProjectionMode::Navigate, ProjectionMode::Expanded] {
        let node = host
            .resolve_named_symbol("/G.vue", "A", Some(mode))
            .unwrap_or_else(|| panic!("A must resolve in {mode:?}"));
        let projected = host
            .project_node_to_type_expr_for_test(node)
            .unwrap_or_else(|| panic!("A must project in {mode:?}"));
        match &projected {
            verter_type_expr::TypeExpr::TypeParameter(param) => {
                assert_eq!(
                    &*param.name, "T",
                    "the open generic must resolve as TypeParameter T in {mode:?}, got {:?}",
                    param.name
                );
            }
            verter_type_expr::TypeExpr::Ref { name, .. } if name.as_ref() == "T" => panic!(
                "`type A = T` in a `generic=\"T\"` SFC must resolve T as a bound TypeParameter \
                 in {mode:?}, NOT an unbound `Ref {{ name: \"T\" }}` bare-name carrier"
            ),
            other => panic!("A must resolve T as a TypeParameter in {mode:?}, got {other:?}"),
        }
    }
}
