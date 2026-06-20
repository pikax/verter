//! Pins the OBSERVABLE cross-file ambient-augmentation merge contract: the
//! resolved-through-dispatch merged surface equals an independently-computed
//! retained `EvalEnv` oracle, and a divergence between them fails the assertion.
//!
//! `interface Foo { base }` in `/types.ts` + `declare module './types' {
//! interface Foo { fromAug } }` in a sibling `/aug.ts` reaches dispatch as ONE
//! `SemanticNodeData::MergedDecl { contributors }` carrier (the
//! `stitch_module_augmentations` fold of base ∪ augmenter — the ONE
//! declaration-merge path, NOT a bare `Intersection`). The resolved-through-
//! dispatch merged member SURFACE is compared, member-name-and-type, against an
//! oracle built INDEPENDENTLY from the typed retained inventory: the base's
//! file-scope `type_symbols["Foo"]` UNION the augmenter's
//! `augmentation_scopes[(Module("./types"), "Foo")]`. The augmenter member is
//! NOT in the base file's `type_symbols` (ambient augmentation never pollutes a
//! file's top-level surface — it lives in the separate `augmentation_scopes`
//! inventory), so the oracle here is a genuine SECOND source that walks the two
//! retained scopes directly, not the dispatch stitch re-run.
//!
//! Every assertion DISCRIMINATES: it would FAIL if a regression dropped an
//! augmenter contributor or the base contributor (the union member set would
//! shrink), diverged a merged member's type from the oracle, or lost the
//! `MergedDecl` carrier and collapsed the merge into a bare `Intersection`
//! (whose heritage-shadow reducer cannot accumulate the peer merge — the
//! Declaration Augmentation / Declaration Merging carrier invariant). The cold
//! stitch mints the `MergedDecl` and Expanded dispatch materialises its unioned
//! object surface, so the dispatch surface and the retained-inventory oracle are
//! two independent sources of the same merged shape.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_semantic::analysis::type_eval::AugmentationScopeKind;
use verter_type_expr::{ObjectMember, TypeExpr};

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

fn node_data(
    host: &VerterHost,
    node: crate::semantic_query::SemanticNodeId,
) -> Arc<SemanticNodeData> {
    host.project_type_store()
        .semantic_graph()
        .node_data(node)
        .expect("node interned during resolution")
}

/// The sorted `(member-name, debug-rendered member type)` pairs of an `Object`
/// projection's direct properties — the comparable surface both the dispatch
/// projection and the oracle reduce to.
fn object_member_surface(ty: &TypeExpr) -> Vec<(String, String)> {
    let TypeExpr::Object(shape) = ty else {
        panic!("expected an Object surface to read members from, got {ty:?}");
    };
    let mut members: Vec<(String, String)> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some((prop.name.clone(), format!("{:?}", prop.ty))),
            _ => None,
        })
        .collect();
    members.sort();
    members
}

/// The fixture: a base interface, a sibling ambient augmenter, and a consumer
/// that imports the base. Augmenter discovery (`ensure_augmentation_index_populated`
/// → `collect_augmenter_candidates`) COLD-SCANS every LOADED artifact for a
/// matching `declare module` fact — it does NOT use the consumer's import edges
/// to discover the augmenter. `/aug.ts` is found because it is UPSERTED (loaded)
/// and carries a `declare module './types'` fact whose specifier resolves to
/// `/types.ts`; the augmenter's OWN `import type { Foo } from './types'` is what
/// associates it with the augmentation target, NOT any side-effect import in the
/// consumer. The reverse-deps walk in `stitch_module_augmentations` only
/// PRE-LOADS lazily-unloaded augmenters before the cold scan, so it is a no-op
/// when every file is already upserted (as here). The consumer's
/// `import './aug'` is therefore harmless but immaterial to the merge: entry
/// point (a) below resolves `Foo` directly from `/types.ts` and never touches
/// `/use.ts`, yet still unions both contributors.
fn upsert_augmentation_fixture(host: &VerterHost) {
    upsert_ts(host, "/types.ts", "export interface Foo { base: string }\n");
    upsert_ts(
        host,
        "/aug.ts",
        "import type { Foo } from './types'\n\
         declare module './types' { interface Foo { fromAug: number } }\n",
    );
    upsert_ts(
        host,
        "/use.ts",
        "import type { Foo } from './types'\n\
         import './aug'\n\
         export type A = Foo\n",
    );
}

/// The INDEPENDENT oracle: walk the base file's file-scope `type_symbols["Foo"]`
/// (the `base` member) and UNION the augmenter file's
/// `augmentation_scopes[(Module("./types"), "Foo")]` (the `fromAug` member),
/// reading each contributor's RETAINED body straight from the typed inventory.
/// This is a different code path than the dispatch `stitch_module_augmentations`
/// fold, so the equivalence is a genuine two-source cross-check.
fn oracle_augmented_foo_surface(host: &VerterHost) -> Vec<(String, String)> {
    let mut members: Vec<(String, String)> = Vec::new();

    // Base contributor: the file-scope interface body in `/types.ts`.
    let types_env = host
        .base_eval_env_arc("/types.ts")
        .expect("base env for /types.ts must build");
    let base_group = types_env
        .type_symbols
        .get("Foo")
        .expect("the base file must carry the `Foo` interface in file-scope type_symbols");
    members.extend(object_member_surface(&base_group.primary().body));

    // Augmenter contributor: the RETAINED ambient body in `/aug.ts`, kept in
    // the SEPARATE augmentation-scope inventory (never in file-scope symbols).
    let aug_env = host
        .base_eval_env_arc("/aug.ts")
        .expect("base env for /aug.ts must build");
    let aug_key = (
        AugmentationScopeKind::Module("./types".to_string()),
        "Foo".to_string(),
    );
    let aug_group = aug_env
        .augmentation_scopes
        .get(&aug_key)
        .expect("the augmenter must retain its `declare module './types' { Foo }` body in augmentation_scopes");
    members.extend(object_member_surface(&aug_group.primary().body));

    members.sort();
    members
}

/// A cross-file ambient-augmented interface resolves THROUGH DISPATCH to the
/// distinct `MergedDecl` carrier whose unioned member surface equals the
/// independently-computed `EvalEnv` oracle (base file-scope ∪ augmenter
/// augmentation-scope). Asserted on TWO graph-native entry points: resolving
/// `Foo` directly from the base file, and resolving the importing alias `A =
/// Foo` (under Expanded, which follows the alias through to the augmented body).
///
/// Discriminating: (1) the `MergedDecl` arm + the explicit anti-`Intersection`
/// panic catch a flip that lost the merge carrier or collapsed the merge into a
/// heritage-shadow `Intersection`. (2) The `assert_eq!` of the dispatch member
/// surface against the oracle surface — `{base: Primitive(String), fromAug:
/// Primitive(Number)}` — reds if a flip dropped the augmenter contributor (the
/// set loses `fromAug`), dropped the base contributor (loses `base`), or
/// diverged a member type (the debug-rendered type differs). The oracle is the
/// SECOND source: it reads the two retained scopes directly, so the equality is
/// real cross-rail agreement, not a tautology.
#[test]
fn cross_file_module_augmentation_merge_surface_matches_oracle() {
    let host = make_host();
    upsert_augmentation_fixture(&host);

    // The independently-computed oracle merged surface (base file-scope ∪
    // augmenter augmentation-scope). Pin its exact content so the cross-check
    // below is anchored to a known, discriminating value — not just "the two
    // rails happen to agree on whatever they both produce".
    let oracle = oracle_augmented_foo_surface(&host);
    assert_eq!(
        oracle,
        vec![
            ("base".to_string(), "Primitive(String)".to_string()),
            ("fromAug".to_string(), "Primitive(Number)".to_string()),
        ],
        "the INDEPENDENT oracle (base /types.ts file-scope `Foo` ∪ augmenter \
         /aug.ts augmentation-scope `Foo`) must union exactly \
         {{base: string, fromAug: number}}; got {oracle:?}"
    );

    // Entry point (a): resolve `Foo` directly from the base file, under BOTH
    // Navigate AND Expanded — the augmentation stitch runs at decl-body
    // resolution time, so the carrier is a `MergedDecl` in both modes.
    for mode in [ProjectionMode::Navigate, ProjectionMode::Expanded] {
        let node = host
            .resolve_named_symbol("/types.ts", "Foo", &[], Some(mode))
            .unwrap_or_else(|| panic!("Foo must resolve in {mode:?}"));
        match node_data(&host, node).as_ref() {
            SemanticNodeData::MergedDecl { contributors } => {
                assert_eq!(
                    contributors.len(),
                    2,
                    "the augmented `Foo` must carry both the base and the augmenter \
                     contributor in {mode:?}, got {} contributors",
                    contributors.len()
                );
            }
            SemanticNodeData::Intersection(_) => panic!(
                "the cross-file augmented `Foo` must NOT collapse into a bare Intersection in \
                 {mode:?} (the heritage-shadow reducer cannot peer-merge the augmenter) — it must \
                 be the distinct MergedDecl carrier"
            ),
            other => panic!(
                "the cross-file augmented `Foo` must lower to the distinct MergedDecl carrier in \
                 {mode:?}, got {other:?}"
            ),
        }

        let projected = host
            .project_node_to_type_expr(node)
            .unwrap_or_else(|| panic!("the merged `Foo` must project in {mode:?}"));
        let dispatch_surface = object_member_surface(&projected);
        assert_eq!(
            dispatch_surface, oracle,
            "the resolved-through-dispatch merged `Foo` member surface in {mode:?} must equal the \
             independently-computed base∪augmenter oracle; dispatch={dispatch_surface:?} \
             oracle={oracle:?}"
        );
    }

    // Entry point (b): the importing alias `A = Foo`. Under Expanded the alias
    // follows through to the augmented `Foo` body — also a `MergedDecl` whose
    // unioned surface equals the oracle. (This is the consumer-facing path a
    // downstream file sees for an imported augmented type.)
    let alias_node = host
        .resolve_named_symbol("/use.ts", "A", &[], Some(ProjectionMode::Expanded))
        .expect("A must resolve Expanded through the imported augmented Foo");
    match node_data(&host, alias_node).as_ref() {
        SemanticNodeData::MergedDecl { contributors } => assert_eq!(
            contributors.len(),
            2,
            "the alias `A = Foo` must Expanded-resolve to the 2-contributor augmented MergedDecl"
        ),
        SemanticNodeData::Intersection(_) => panic!(
            "the alias to the augmented `Foo` must not collapse to a bare Intersection"
        ),
        other => panic!(
            "the alias `A = Foo` must Expanded-resolve to the augmented MergedDecl carrier, got {other:?}"
        ),
    }
    let alias_projected = host
        .project_node_to_type_expr(alias_node)
        .expect("the aliased merged surface must project");
    let alias_surface = object_member_surface(&alias_projected);
    assert_eq!(
        alias_surface, oracle,
        "the imported-alias `A = Foo` Expanded surface must also equal the base∪augmenter oracle; \
         alias={alias_surface:?} oracle={oracle:?}"
    );

    // Explicit negative: the augmenter member is NOT in the base file's
    // top-level surface (ambient augmentation lives only in augmentation_scopes)
    // — so the merge genuinely contributed `fromAug`, it was not already a
    // file-scope member of `/types.ts`. A flip that leaked augmenters into
    // file-scope `type_symbols` (a different defect) would surface `fromAug`
    // here and fail.
    let types_env = host
        .base_eval_env_arc("/types.ts")
        .expect("base env for /types.ts");
    let base_only = object_member_surface(
        &types_env
            .type_symbols
            .get("Foo")
            .expect("Foo in base file-scope")
            .primary()
            .body,
    );
    assert_eq!(
        base_only,
        vec![("base".to_string(), "Primitive(String)".to_string())],
        "the base file's file-scope `Foo` must carry ONLY `base` — the augmenter member `fromAug` \
         must NOT pollute file-scope type_symbols (it lives only in augmentation_scopes); \
         got {base_only:?}"
    );
}
