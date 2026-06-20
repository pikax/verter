//! Characterization of the OBSERVABLE cross-file VALUE-symbol depth contract a
//! future declaration-body PRODUCER flip (to handle-native carrier bodies) must
//! preserve, compared against the retained `EvalEnv` value-symbol oracle.
//!
//! The graph-native per-symbol reader
//! `dependency_value_symbol_graph_native(canonical, name)` produces a
//! `ValueDeclInfo` from the per-file header index + lazy value memo; the oracle
//! is `base_eval_env_arc(canonical).value_symbols.get(name).primary()`. The
//! existing same-file `theme`/`count` coverage exercises only trivial consts;
//! this fills the gap with a CROSS-FILE value alias re-exported through a barrel
//! and a body rich across the facets that EXIST — a `const` with a
//! `type_annotation` + `object_shape`, an `enum` (so `enum_members` is
//! exercised), and a single-signature `function` (so `signatures` is exercised)
//! — each read from its DEFINING file across the re-export route and compared
//! field-by-field to the oracle. The cross-file "dep-fact" facet is the C2
//! peeler-pair `(canonical, name)` terminal agreement through the barrel.
//!
//! SCOPED NON-EQUIVALENCE (characterized, NOT asserted): a MULTI-overload
//! function's `signatures` facet genuinely DIVERGES between the two rails on
//! this tree, so it is deliberately NOT compared. The graph-native reader
//! (`effective_value_decl(name).signatures`) carries the FULL ordered overload
//! group, while the oracle reads `value_symbols.get(name).primary()` — the
//! LAST-WINS contributor, whose `.signatures` is the implementation entry ONLY
//! (the oracle's full group lives on `ValueDeclGroup::merged_signatures()`, a
//! DIFFERENT method this `primary()`-based oracle does not call). Asserting
//! `signatures` equivalence over a multi-overload function would be a FALSE
//! assertion on the current tree. A SINGLE-signature function exercises the
//! `signatures` facet with `primary().signatures == merged_signatures()`, so the
//! equivalence is real.
//!
//! Each compared assertion DISCRIMINATES: a flip that diverged the graph-native
//! reader from the oracle on any compared facet (kind / type_annotation /
//! signatures / object_shape / enum_members), or diverged the two cross-file
//! peelers' value terminal, fails the corresponding `assert_eq!`. Written GREEN
//! against the current tree (both rails already agree on the compared facets).
//!
//! HONESTY FLAGS — characterized known-absences, NOT omissions:
//!
//! - **Value-symbol body spans are NOT carried on this surface.** `ValueDeclInfo`
//!   has no `spans` field, and the `LoweredValueDecl` it is built from carries no
//!   span (its only `Span`-typed field is the `oxc_span::SourceType` parse
//!   config, not a source location). `FunctionSignature` likewise carries no span
//!   field. No value-symbol-body spans-equivalence assertion is therefore
//!   possible at this surface, and none is made.
//!
//! - **There is NO per-value cross-file dep-facts accessor.** Only the TYPE-symbol
//!   edge reader (`ShallowFileState::type_deps(name)`) exists; there is no
//!   value-space `value_deps` / `ClassifiedValueDeps` accessor and no
//!   `ValueDeclInfo.external_deps` field. The value symbol's cross-file dep-fact
//!   is therefore expressed as the C2 peeler-pair `(canonical, name)` terminal
//!   agreement for a cross-file value re-export (oracle vs graph-native), which
//!   proves the value's cross-file terminal dep resolves identically on both
//!   rails — NOT a (nonexistent) per-value dep-fact field.

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

/// A cross-file value alias re-exported through a barrel exercises the
/// graph-native `ValueDeclInfo` reader on the FACETS THAT EXIST — a `const`
/// with a `type_annotation` + `object_shape`, an `enum` populating
/// `enum_members`, and a single-signature `function` populating `signatures` —
/// each read from its DEFINING file (`/dep.ts`) and compared field-by-field to
/// the `EvalEnv` oracle. The cross-file dep-fact is the C2 peeler-pair terminal
/// agreement through the barrel `/barrel.ts`.
///
/// Discriminating: the per-facet `assert_eq!`s red if a flip diverged the
/// graph-native reader from the oracle on `kind`, `type_annotation`,
/// `signatures` (compared via `{:?}`, like the existing same-file C4 test),
/// `object_shape`, or `enum_members`. The `enum`/`function`/`const` mix forces
/// EACH facet to be non-trivially populated (an enum has members but no
/// annotation; a function has one signature but no object_shape; the const has
/// both an annotation and an object_shape) — so a regression that dropped
/// exactly one facet is caught. The C2 peeler-pair `assert_eq!` reds if the
/// graph-native peeler diverged from the oracle peeler on the cross-file value
/// terminal, and the explicit `("/dep.ts", name)` pin proves the barrel
/// re-export peels to the FINAL defining file, not the intermediate barrel
/// binding. The miss case (`None`) guards the negative.
#[test]
fn cross_file_value_symbol_depth_matches_oracle_on_present_facets() {
    let host = make_host();
    // Defining file: three values spanning every populated `ValueDeclInfo`
    // facet. `cfg` (const) → type_annotation + object_shape; `Color` (enum) →
    // enum_members; `single` (single-signature function) → a 1-entry
    // `signatures` group (multi-overload is the scoped non-equivalence — see the
    // file-level note).
    upsert_ts(
        &host,
        "/dep.ts",
        "export const cfg: { a: number } = { a: 1 }\n\
         export enum Color { Red, Green }\n\
         export function single(x: string): number { return x.length }\n",
    );
    // Barrel re-export: exercises the cross-file route for the C2 dep-fact.
    upsert_ts(
        &host,
        "/barrel.ts",
        "export { cfg, Color, single } from './dep'\n",
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
    // const with BOTH an annotation and an object_shape; `Color` is an enum with
    // members and no annotation; `over` is a function with a 3-entry overload
    // group and no object_shape.
    let cfg = host
        .dependency_value_symbol_graph_native("/dep.ts", "cfg")
        .expect("cfg present");
    assert_eq!(
        cfg.kind,
        verter_semantic::analysis::type_eval::ValueDeclKind::Const,
        "control: `cfg` is a const"
    );
    assert!(
        cfg.type_annotation.is_some(),
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
        .iter()
        .map(|(member_name, _)| member_name.as_str())
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
         compared (and equals the oracle's `primary().signatures` — a multi-overload group is the \
         scoped non-equivalence), got {}",
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
    // dep-facts accessor, see the file-level honesty flag). The barrel
    // re-export `export { cfg } from './dep'` must peel to the FINAL defining
    // `(/dep.ts, cfg)` pair on BOTH the oracle peeler and the graph-native
    // peeler, and the two must AGREE.
    for name in ["cfg", "Color", "single"] {
        let oracle_pair = host
            .resolve_value_export_target("/barrel.ts", name)
            .unwrap_or_else(|| panic!("oracle peeler must resolve barrel export `{name}`"));
        let graph_pair = host
            .resolve_value_export_target_graph_native("/barrel.ts", name)
            .unwrap_or_else(|| panic!("graph-native peeler must resolve barrel export `{name}`"));

        assert_eq!(
            (oracle_pair.canonical_id.as_str(), oracle_pair.name.as_str()),
            (graph_pair.canonical_id.as_str(), graph_pair.name.as_str()),
            "C2 cross-file value terminal divergence for `{name}`: \
             oracle={oracle_pair:?} graph_native={graph_pair:?}"
        );
        assert_eq!(
            (oracle_pair.canonical_id.as_str(), oracle_pair.name.as_str()),
            ("/dep.ts", name),
            "the barrel re-export `{name}` must peel to the FINAL defining (/dep.ts, {name}), \
             not the intermediate barrel binding; got {oracle_pair:?}"
        );
    }
}
