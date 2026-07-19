//! Pins the OBSERVABLE cross-file VALUE-symbol depth contract: the equivalence
//! between the graph-native per-symbol reader and the retained `EvalEnv`
//! value-symbol oracle on every facet where they agree, AND the CURRENT
//! divergence on the two facets where they do not — so a change that diverges an
//! agreeing facet OR aligns a diverging one is caught.
//!
//! The graph-native per-symbol reader
//! `dependency_value_symbol_graph_native(canonical, name)` produces a
//! `ValueDeclInfo` from the per-file header index + lazy value memo (its
//! `signatures`/`enum_members` come from `ValueDeclGroup::merged_signatures()`/
//! `merged_enum_unified()` — the FULL ordered group, unioned across same-name
//! merged contributors); the oracle is
//! `base_eval_env_arc(canonical).value_symbols.get(name).primary()` (the LAST
//! contributor, last-wins). For a SINGLE-contributor symbol the two rails are
//! identical by construction; for a MULTI-contributor symbol (a function
//! overload group, a declaration-merged enum) they diverge — the graph-native
//! group is a strict superset.
//!
//! Two coverage halves:
//!
//! - **Agreeing facets (equality pinned)** — a `const` carrying both a
//!   `type_annotation` and an `object_shape` (`cfg`), a single-body `enum`
//!   (`Color`), and a single-signature `function` (`single`), each a
//!   single-contributor symbol read from its DEFINING file and compared
//!   field-by-field (kind / type_annotation / signatures / object_shape /
//!   enum_members) to the oracle. Each facet is non-trivially populated by
//!   exactly one of the three (the const has an annotation and an object_shape,
//!   the enum has members, the function has one signature), so a regression
//!   dropping one facet is caught.
//!
//! - **Diverging facets (divergence pinned)** — a 3-entry function overload
//!   group (`over`) whose graph-native `signatures` carries all 3 declarations
//!   while the oracle `primary().signatures` carries the implementation entry
//!   ONLY, and a 2-body declaration-merged enum (`Merged`) whose graph-native
//!   `enum_members` unions both bodies while the oracle `primary().enum_members`
//!   carries the last body ONLY. These are pinned with `assert_ne!` + a
//!   `len()`-superset assertion, NOT asserted equal — equating them would be a
//!   false equivalence on the current tree, and a future change that aligned (or
//!   widened) either rail flips the divergence and fails the pin.
//!
//! The cross-file "dep-fact" facet is the C2 peeler-pair `(canonical,
//! source_name)` terminal agreement. The plain renamed exports
//! (`cfg`/`Color`/`single`) exercise the cross-file ROUTE (exported ≠ source)
//! plus the body-depth equivalence — the peeled terminal is fed into BOTH the
//! graph-native body reader and the oracle. They do NOT exercise the divergent
//! `typeof`-peel branch (a plain value's declaration carries no `typeof`
//! annotation, so `peel_value_decl_alias` breaks at the first hop). That branch
//! is exercised SEPARATELY by a `typeof`-aliased value (`export const aliased:
//! typeof base = base`) re-exported RENAMED through the barrel: both peelers
//! must HOP through `typeof base` and land on the FINAL underlying `base`, on
//! the direct `typeof`-alias peeler (`peel_value_decl_alias[_graph_native]`) AND
//! through the renamed-barrel export-target peeler — so a no-op peeler that
//! skipped the `typeof` hop is caught.
//!
//! HONESTY FLAGS — characterized known-absences, NOT omissions:
//!
//! - **Value-symbol body spans are NOT carried on this surface.** `ValueDeclInfo`
//!   has no `spans` field; the `LoweredValueDecl` it is built from has no span
//!   field at all (the `oxc_span::SourceType` parse-config carrier lives on a
//!   DIFFERENT type, `DeclBodyMemo`, never on the value surface).
//!   `FunctionSignature` likewise carries no span field. No value-symbol-body
//!   spans-equivalence assertion is therefore possible at this surface, and none
//!   is made.
//!
//! - **There is NO per-value cross-file dep-facts accessor.** Only the TYPE-symbol
//!   edge reader (`ShallowFileState::type_deps(name)`) exists; there is no
//!   value-space `value_deps` / `ClassifiedValueDeps` accessor and no
//!   `ValueDeclInfo.external_deps` field. The value symbol's cross-file dep-fact
//!   is therefore expressed as the C2 peeler-pair `(canonical, source_name)`
//!   terminal agreement for a cross-file value re-export (oracle vs
//!   graph-native), which proves the value's cross-file terminal dep resolves
//!   identically on both rails — NOT a (nonexistent) per-value dep-fact field.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

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

/// The AGREEING-facet half: three SINGLE-contributor cross-file values — a
/// `const` with a `type_annotation` + `object_shape` (`cfg`), a single-body
/// `enum` (`Color`), and a single-signature `function` (`single`) — read from
/// their DEFINING file (`/dep.ts`) and compared field-by-field to the `EvalEnv`
/// oracle, plus the cross-file dep-fact as the C2 peeler-pair terminal agreement
/// through a RENAMED barrel (`/barrel.ts`).
///
/// Discriminating: the per-facet `assert_eq!`s red if a flip diverged the
/// graph-native reader from the oracle on `kind`, `type_annotation`,
/// `signatures` (compared via `{:?}`, like the existing same-file C4 test),
/// `object_shape`, or `enum_members`. For these single-contributor symbols
/// `merged_signatures()`/`merged_enum_unified()` equal `primary().*`, so the
/// equality is real (the MULTI-contributor divergence is pinned separately by
/// `cross_file_value_symbol_depth_pins_multi_contributor_divergence`). The
/// `enum`/`function`/`const` mix forces EACH facet to be non-trivially populated
/// (an enum has members but no annotation; a function has one signature but no
/// object_shape; the const has both an annotation and an object_shape) — so a
/// regression that dropped exactly one facet is caught. The C2 peeler-pair
/// `assert_eq!` reds if the graph-native peeler diverged from the oracle peeler
/// on the cross-file value terminal; the RENAMED export (`cfg as cfgAlias`)
/// forces the exported name ≠ the source name so the peel genuinely traverses
/// the cross-file route, and feeding the peeled `(canonical, source_name)` into
/// BOTH body readers proves the body-depth equivalence THROUGH the re-export
/// route. The explicit `("/dep.ts", source_name)` pin proves the barrel
/// re-export peels to the FINAL defining file, not the intermediate barrel
/// binding. The miss case (`None`) guards the negative.
///
/// The plain renamed exports above exercise the cross-file ROUTE + body-depth,
/// but NOT the divergent `typeof`-peel branch (a plain const/enum/function
/// declaration carries no `typeof` annotation, so `peel_value_decl_alias`
/// breaks at the first hop — a no-op peeler that skipped the `typeof` hop would
/// still pass). That branch is exercised by the `typeof`-aliased value (`export
/// const aliased: typeof base = base`) re-exported RENAMED as `aliasedExport`:
/// the trailing block asserts BOTH peelers (oracle vs graph-native) HOP through
/// `typeof base` and land on the FINAL underlying `(/dep.ts, base)`, NOT the
/// intermediate `aliased` — on the direct `typeof`-alias peeler
/// (`peel_value_decl_alias[_graph_native]_for_test`, the API that genuinely
/// follows the `typeof` chain) AND through the renamed-barrel export-target
/// peeler (`resolve_value_export_target[_graph_native]`).
#[test]
fn cross_file_value_symbol_depth_matches_oracle_on_present_facets() {
    let host = make_host();
    // Defining file: three SINGLE-contributor values spanning every populated
    // `ValueDeclInfo` facet. `cfg` (const) → type_annotation + object_shape;
    // `Color` (single-body enum) → enum_members; `single` (single-signature
    // function) → a 1-entry `signatures` group. Each is a single-contributor
    // symbol, so the graph-native group (`merged_*`) equals the oracle
    // `primary()`. The multi-contributor divergence (overload group / merged
    // enum) lives in the sibling divergence test.
    upsert_ts(
        &host,
        "/dep.ts",
        "export const cfg: { a: number } = { a: 1 }\n\
         export enum Color { Red, Green }\n\
         export function single(x: string): number { return x.length }\n\
         export const base = { v: 1 }\n\
         export const aliased: typeof base = base\n",
    );
    // RENAMED barrel re-exports: the exported name differs from the source name,
    // so peeling genuinely traverses the cross-file route (exported ≠ source) —
    // not a same-file read. `aliased as aliasedExport` re-exports the
    // `typeof`-aliased value renamed, so the export-target peeler must FIRST
    // resolve `aliasedExport` → source `(/dep.ts, aliased)` and THEN follow the
    // `typeof base` hop to the FINAL underlying `base` — genuinely exercising the
    // divergent `typeof`-peel branch on BOTH rails.
    upsert_ts(
        &host,
        "/barrel.ts",
        "export { cfg as cfgAlias, Color as ColorAlias, single as singleAlias, \
         aliased as aliasedExport } from './dep'\n",
    );

    for name in ["cfg", "Color", "single"] {
        // Oracle: the dependency whole-env value_symbols read on the DEFINING
        // file (the barrel itself carries no value declaration for a pure
        // re-export — the value body lives in `/dep.ts`).
        let oracle_env = host
            .base_eval_env_arc("/dep.ts")
            .expect("defining-file env builds");
        let oracle = oracle_env
            .value_symbols
            .get(name)
            .map(|g| g.primary().clone())
            .unwrap_or_else(|| panic!("oracle must know `{name}` in /dep.ts"));

        // Graph-native per-symbol reader on the same defining file.
        let graph = host
            .dependency_value_symbol_graph_native("/dep.ts", name)
            .unwrap_or_else(|| panic!("graph-native reader must know `{name}` in /dep.ts"));

        assert_eq!(graph.name, oracle.name, "name must match for `{name}`");
        assert_eq!(graph.kind, oracle.kind, "kind must match for `{name}`");
        assert_eq!(
            graph.type_annotation, oracle.type_annotation,
            "type_annotation must match for `{name}`"
        );
        assert_eq!(
            format!("{:?}", graph.signatures),
            format!("{:?}", oracle.signatures),
            "signatures must match for `{name}`"
        );
        assert_eq!(
            graph.object_shape, oracle.object_shape,
            "object_shape must match for `{name}`"
        );
        assert_eq!(
            graph.enum_members, oracle.enum_members,
            "enum_members must match for `{name}`"
        );
        assert_eq!(
            graph.declaration_id, 0,
            "the alias-path declaration_id is the opaque 0 (matching the prepared route) for `{name}`"
        );
    }

    // Per-facet population guards — prove the fixture genuinely exercises EACH
    // facet, so the field-by-field equivalence above is non-trivial. `cfg` is a
    // const with BOTH an annotation and an object_shape; `Color` is a
    // single-body enum with members and no annotation; `single` is a
    // single-signature function with no object_shape.
    let cfg = host
        .dependency_value_symbol_graph_native("/dep.ts", "cfg")
        .expect("cfg present");
    assert_eq!(
        cfg.kind,
        verter_semantic::analysis::type_eval::ValueDeclKind::Const,
        "control: `cfg` is a const"
    );
    assert!(
        !matches!(
            cfg.type_annotation.classification,
            verter_type_expr::facts::ValueAnnotationClass::Absent
        ) && cfg.type_annotation.annotation.is_some(),
        "control: `cfg` must carry its `{{ a: number }}` annotation so the type_annotation \
         facet is non-trivially compared, got {:?}",
        cfg.type_annotation
    );
    assert!(
        cfg.object_shape.is_some(),
        "control: `cfg` must carry its object_shape so that facet is non-trivially compared"
    );

    let color = host
        .dependency_value_symbol_graph_native("/dep.ts", "Color")
        .expect("Color present");
    assert_eq!(
        color.kind,
        verter_semantic::analysis::type_eval::ValueDeclKind::Enum,
        "control: `Color` is an enum"
    );
    let color_member_names: Vec<&str> = color
        .enum_members
        .as_ref()
        .expect("control: `Color` must carry enum_members so that facet is non-trivially compared")
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(
        color_member_names,
        vec!["Red", "Green"],
        "control: the enum_members facet must carry the ordered `Red`, `Green` members"
    );

    let single = host
        .dependency_value_symbol_graph_native("/dep.ts", "single")
        .expect("single present");
    assert_eq!(
        single.kind,
        verter_semantic::analysis::type_eval::ValueDeclKind::Function,
        "control: `single` is a function"
    );
    assert_eq!(
        single.signatures.len(),
        1,
        "control: `single` must carry its one signature so the signatures facet is non-trivially \
         compared — a single-contributor group, so `merged_signatures()` == `primary().signatures`; \
         got {}",
        single.signatures.len()
    );
    assert!(
        single.object_shape.is_none(),
        "control: a function value carries no object_shape"
    );

    // Miss case: a non-existent name resolves to `None` on the graph-native
    // reader (the negative).
    assert!(
        host.dependency_value_symbol_graph_native("/dep.ts", "doesNotExist")
            .is_none(),
        "a non-existent value name must resolve to None on the graph-native reader"
    );

    // Cross-file dep-fact terminal (the value symbol's cross-file dep facet,
    // expressed as the C2 peeler-pair agreement — there is NO per-value
    // dep-facts accessor, see the file-level honesty flag). The RENAMED barrel
    // re-export `export { cfg as cfgAlias } from './dep'` must peel to the FINAL
    // defining `(/dep.ts, cfg)` SOURCE pair on BOTH the oracle peeler and the
    // graph-native peeler, and the two must AGREE — and the peeled
    // `(canonical, source_name)` must read the SAME body on both readers, so the
    // body-depth equivalence is proven THROUGH the re-export route.
    for (exported_name, source_name) in [
        ("cfgAlias", "cfg"),
        ("ColorAlias", "Color"),
        ("singleAlias", "single"),
    ] {
        let oracle_pair = host
            .resolve_value_export_target("/barrel.ts", exported_name)
            .unwrap_or_else(|| {
                panic!("oracle peeler must resolve barrel export `{exported_name}`")
            });
        let graph_pair = host
            .resolve_value_export_target_graph_native("/barrel.ts", exported_name)
            .unwrap_or_else(|| {
                panic!("graph-native peeler must resolve barrel export `{exported_name}`")
            });

        assert_eq!(
            &oracle_pair, &graph_pair,
            "C2 cross-file value terminal divergence for `{exported_name}`: \
             oracle={oracle_pair:?} graph_native={graph_pair:?}"
        );
        assert_eq!(
            (
                oracle_pair.canonical_id.as_str(),
                oracle_pair.owner,
                oracle_pair.name.as_str()
            ),
            (
                "/dep.ts",
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                source_name
            ),
            "the RENAMED barrel re-export `{exported_name}` must peel to the FINAL defining \
             (/dep.ts, {source_name}) SOURCE pair, not the intermediate barrel binding; \
             got {oracle_pair:?}"
        );

        // Body-depth equivalence THROUGH the re-export route: feed the PEELED
        // terminal `(canonical, source_name)` into BOTH the graph-native body
        // reader and the oracle. Because the export was renamed, this proves the
        // body reader works through the actual cross-file route — the peeled
        // SOURCE name (`cfg`), not the exported name (`cfgAlias`), keys the body.
        let peeled_canonical = oracle_pair.canonical_id.as_str();
        let peeled_name = oracle_pair.name.as_str();
        let routed_graph = host
            .dependency_value_symbol_graph_native(peeled_canonical, peeled_name)
            .unwrap_or_else(|| {
                panic!("graph-native body reader must know the peeled ({peeled_canonical}, {peeled_name})")
            });
        let routed_oracle = host
            .base_eval_env_arc(peeled_canonical)
            .expect("peeled defining-file env builds")
            .value_group_in(oracle_pair.owner, peeled_name)
            .map(|g| g.primary().clone())
            .unwrap_or_else(|| {
                panic!("oracle must know the peeled ({peeled_canonical}, {peeled_name})")
            });
        assert_eq!(
            routed_graph.kind, routed_oracle.kind,
            "routed-through-barrel kind divergence for peeled ({peeled_canonical}, {peeled_name})"
        );
        assert_eq!(
            routed_graph.type_annotation, routed_oracle.type_annotation,
            "routed-through-barrel type_annotation divergence for peeled ({peeled_canonical}, {peeled_name})"
        );
        assert_eq!(
            format!("{:?}", routed_graph.signatures),
            format!("{:?}", routed_oracle.signatures),
            "routed-through-barrel signatures divergence for peeled ({peeled_canonical}, {peeled_name})"
        );
        assert_eq!(
            routed_graph.object_shape, routed_oracle.object_shape,
            "routed-through-barrel object_shape divergence for peeled ({peeled_canonical}, {peeled_name})"
        );
        assert_eq!(
            routed_graph.enum_members, routed_oracle.enum_members,
            "routed-through-barrel enum_members divergence for peeled ({peeled_canonical}, {peeled_name})"
        );
    }

    // The `typeof`-peel branch (the peeler-SPECIFIC divergent logic). The
    // export-target peeler loop above resolves the cross-file ROUTE + the
    // body-depth equivalence, but `cfg`/`Color`/`single` are plain values whose
    // declarations carry no `typeof` annotation, so `peel_value_decl_alias`
    // breaks at the first hop — the `typeof` chain is never walked, and a no-op
    // peeler that skipped the `typeof` hop would still pass that loop. `aliased`
    // (`export const aliased: typeof base = base`) forces the divergent
    // `typeof`-peel branch: both peelers must HOP through `typeof base` and land
    // on the FINAL underlying `(/dep.ts, base)`, not the intermediate `aliased`.
    //
    // Asserted on BOTH peeler APIs:
    //  - `peel_value_decl_alias[_graph_native]_for_test` is the API that
    //    DIRECTLY follows the `typeof` chain (the same API the cycle test in
    //    `decl_body_dispatch_equivalence_tests.rs` uses). Run on the SOURCE
    //    `(/dep.ts, aliased)` it walks `typeof base` → `base`.
    //  - `resolve_value_export_target[_graph_native]` through the RENAMED
    //    barrel re-export (`aliased as aliasedExport`) proves the FULL route:
    //    resolve `aliasedExport` → source `(/dep.ts, aliased)`, THEN the same
    //    `typeof` peel to `(/dep.ts, base)`.
    // Both rails (oracle vs graph-native) must AGREE and land on `base`.

    // (i) The direct `typeof`-alias peeler on the source value. This is the
    // API that genuinely exercises the `typeof`-peel branch — it walks the
    // single-segment `typeof base` chain `aliased` → `base`.
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let oracle_peel = host.peel_value_decl_alias_for_test("/dep.ts", owner, "aliased");
    let graph_peel = host.peel_value_decl_alias_graph_native_for_test("/dep.ts", owner, "aliased");
    assert_eq!(
        oracle_peel, graph_peel,
        "the direct `typeof`-alias peeler must AGREE across rails for `(/dep.ts, aliased)`: \
         oracle={oracle_peel:?} graph_native={graph_peel:?}"
    );
    assert_eq!(
        oracle_peel,
        crate::resolver_core::ValueDeclIdentity {
            canonical_id: "/dep.ts".to_string(),
            owner,
            name: "base".to_string(),
        },
        "the direct `typeof`-alias peeler must HOP through `typeof base` and land on the FINAL \
         underlying `(/dep.ts, base)`, NOT the intermediate `aliased`; got {oracle_peel:?}"
    );

    // (ii) The export-target peeler through the RENAMED barrel re-export: the
    // full cross-file route THEN the `typeof` peel. `resolve_value_export_target`
    // resolves `aliasedExport` → source `(/dep.ts, aliased)` (the rename) and
    // then peels the `typeof base` chain to the FINAL `(/dep.ts, base)`.
    let oracle_route = host
        .resolve_value_export_target("/barrel.ts", "aliasedExport")
        .expect("oracle peeler must resolve the renamed `aliasedExport`");
    let graph_route = host
        .resolve_value_export_target_graph_native("/barrel.ts", "aliasedExport")
        .expect("graph-native peeler must resolve the renamed `aliasedExport`");
    assert_eq!(
        oracle_route, graph_route,
        "the renamed-barrel `typeof`-aliased export must AGREE across rails: \
         oracle={oracle_route:?} graph_native={graph_route:?}"
    );
    assert_eq!(
        (
            oracle_route.canonical_id.as_str(),
            oracle_route.owner,
            oracle_route.name.as_str()
        ),
        ("/dep.ts", owner, "base"),
        "the renamed re-export `aliasedExport` must resolve to the source `aliased` AND THEN peel \
         the `typeof base` chain to the FINAL underlying `(/dep.ts, base)`, NOT the intermediate \
         `aliased` and NOT the barrel binding; got {oracle_route:?}"
    );
}

/// The DIVERGING-facet half: pins the CURRENT cross-rail divergence on the two
/// MULTI-contributor facets so a future change that aligns (or further widens)
/// either rail is caught. The graph-native reader builds its `ValueDeclInfo`
/// from `ValueDeclGroup::merged_signatures()` / `merged_enum_unified()` (the
/// FULL ordered group, unioned across same-name merged contributors), while the
/// oracle reads `value_symbols.get(name).primary()` — the LAST contributor only.
/// For a single-contributor symbol these are identical; for a multi-contributor
/// symbol the graph-native group is a strict SUPERSET.
///
/// This is NOT a false equivalence: the two rails are NOT expected to agree on
/// these facets today, so equating them would be wrong. Pinning the divergence
/// with `assert_ne!` + a `len()`-superset assertion means a change that made the
/// `primary()`-based oracle see the full group (alignment), or that grew the
/// graph-native group differently, flips the observed relation and fails here.
#[test]
fn cross_file_value_symbol_depth_pins_multi_contributor_divergence() {
    let host = make_host();
    // `over`: a 3-entry function overload group — two bodiless overload
    // signatures + one implementation; each `function over` statement is a
    // separate same-name contributor appended to the value group.
    // `Merged`: a TS declaration-merged enum — two `enum Merged` bodies that
    // contribute to one enum, so the merged member inventory is `{A, B}` while
    // the last-wins `primary()` body carries `{B}` only.
    upsert_ts(
        &host,
        "/dep.ts",
        "export function over(a: string): number;\n\
         export function over(a: number): string;\n\
         export function over(a: any): any { return a }\n\
         export enum Merged { A }\n\
         export enum Merged { B }\n",
    );

    let oracle_env = host
        .base_eval_env_arc("/dep.ts")
        .expect("defining-file env builds");

    // --- Multi-overload `signatures` divergence ---
    let graph_over = host
        .dependency_value_symbol_graph_native("/dep.ts", "over")
        .expect("graph-native reader must know `over`");
    let oracle_over = oracle_env
        .value_symbols
        .get("over")
        .map(|g| g.primary().clone())
        .expect("oracle must know `over`");

    // CURRENT divergence: graph-native carries the FULL 3-entry overload group;
    // the `primary()`-based oracle carries the implementation entry ONLY.
    assert_eq!(
        graph_over.signatures.len(),
        3,
        "graph-native `over` must carry the FULL 3-entry overload group \
         (`merged_signatures()` concatenates every contributor); got {}",
        graph_over.signatures.len()
    );
    assert_eq!(
        oracle_over.signatures.len(),
        1,
        "the `primary()`-based oracle `over` must carry the LAST contributor's \
         signatures ONLY (the implementation entry); got {}",
        oracle_over.signatures.len()
    );
    assert!(
        graph_over.signatures.len() > oracle_over.signatures.len(),
        "the graph-native overload group must be a strict SUPERSET of the \
         `primary()`-based oracle on this facet; graph={} oracle={}",
        graph_over.signatures.len(),
        oracle_over.signatures.len()
    );
    assert_ne!(
        format!("{:?}", graph_over.signatures),
        format!("{:?}", oracle_over.signatures),
        "the multi-overload `signatures` facet must DIVERGE between the rails on \
         the current tree (graph-native full group vs oracle implementation-only); \
         equating them would be a false equivalence — this pin catches a future \
         flip that aligns or further diverges either rail"
    );

    // --- Declaration-merged-enum `enum_members` divergence ---
    let graph_merged = host
        .dependency_value_symbol_graph_native("/dep.ts", "Merged")
        .expect("graph-native reader must know `Merged`");
    let oracle_merged = oracle_env
        .value_symbols
        .get("Merged")
        .map(|g| g.primary().clone())
        .expect("oracle must know `Merged`");

    let graph_merged_names: Vec<&str> = graph_merged
        .enum_members
        .as_ref()
        .expect("graph-native `Merged` must carry enum_members")
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    let oracle_merged_names: Vec<&str> = oracle_merged
        .enum_members
        .as_ref()
        .expect("oracle `Merged` must carry enum_members")
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();

    // CURRENT divergence: graph-native UNIONS both merged bodies (`{A, B}`); the
    // `primary()`-based oracle carries the LAST body ONLY (`{B}`).
    assert_eq!(
        graph_merged_names,
        vec!["A", "B"],
        "graph-native `Merged` must UNION both declaration-merged enum bodies \
         (`merged_enum_unified()`); got {graph_merged_names:?}"
    );
    assert_eq!(
        oracle_merged_names,
        vec!["B"],
        "the `primary()`-based oracle `Merged` must carry the LAST body's members \
         ONLY; got {oracle_merged_names:?}"
    );
    assert!(
        graph_merged_names.len() > oracle_merged_names.len(),
        "the graph-native merged-enum inventory must be a strict SUPERSET of the \
         `primary()`-based oracle; graph={graph_merged_names:?} oracle={oracle_merged_names:?}"
    );
    assert_ne!(
        graph_merged.enum_members, oracle_merged.enum_members,
        "the declaration-merged-enum `enum_members` facet must DIVERGE between the \
         rails on the current tree (graph-native union vs oracle last-body-only); \
         equating them would be a false equivalence"
    );
}
