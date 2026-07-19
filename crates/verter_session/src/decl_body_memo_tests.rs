//! `DeclBodyMemo` demand-scoping contract: a demand lowers exactly the
//! demanded symbol's contributing statements (counted on the
//! `decl_bodies_lowered` rail), sibling declarations of the same
//! statements backfill, repeated demands re-lower nothing, distinct
//! demands share ONE retained eval-program parse, and seeding from a
//! built env matches the lazy fold.

use std::sync::Arc;

use super::*;
use verter_type_expr::locators::{
    AugmentationBodyLocator, AuthoredAnchor, AuthoredAugmentationScope, AuthoredBodyLocator,
    LocatorSymbolSpace, TypeBodyPathStep, TypeBodySlot, TypeParamBoundPosition,
    TypeParamVisibility,
};

/// The canonical id every fixture memo in this module is keyed on.
const FIXTURE_CANONICAL: &str = "/ws/fixture.ts";

fn has_dependency(decl: &LoweredTypeDecl, name: &str) -> bool {
    decl.dependency_paths
        .iter()
        .any(|dependency| dependency.legacy_dotted_name() == name)
}

/// Build a TYPE-space decl-body locator anchored on this fixture's canonical.
fn type_body_locator(symbol: &str, path: Vec<TypeBodyPathStep>) -> AuthoredBodyLocator {
    type_body_locator_on(FIXTURE_CANONICAL, symbol, path)
}

/// Build a TYPE-space decl-body locator anchored on an explicit canonical.
fn type_body_locator_on(
    canonical: &str,
    symbol: &str,
    path: Vec<TypeBodyPathStep>,
) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from(path.into_boxed_slice()),
    })
}

/// Build a VALUE-space decl-body locator anchored on this fixture's canonical.
fn value_body_locator(symbol: &str, path: Vec<TypeBodyPathStep>) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(FIXTURE_CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        path: Arc::from(path.into_boxed_slice()),
    })
}

/// Build a NAMESPACE-space decl-body locator anchored on this fixture's canonical.
fn namespace_body_locator(symbol: &str, path: Vec<TypeBodyPathStep>) -> AuthoredBodyLocator {
    AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(FIXTURE_CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Namespace,
        },
        path: Arc::from(path.into_boxed_slice()),
    })
}

/// Build a TYPE-space augmentation-scoped locator anchored on this fixture's
/// canonical, in the given ambient augmentation scope.
fn aug_type_locator(
    symbol: &str,
    scope: AuthoredAugmentationScope,
    path: Vec<TypeBodyPathStep>,
) -> AuthoredBodyLocator {
    AuthoredBodyLocator::AugmentationBody(AugmentationBodyLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(FIXTURE_CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Type,
        },
        scope,
        path: Arc::from(path.into_boxed_slice()),
    })
}

/// The single derefed expression, or a panic naming the unexpected shape.
fn single(body: &locator_deref::DerefedAuthoredBody) -> &TypeExpr {
    match &body.shape {
        DerefedBodyShape::Single(expr) => expr,
        other => panic!("expected a single derefed body, got {other:?}"),
    }
}

fn memo_for(source: &str) -> (Arc<DeclBodyMemo>, Arc<MetaProvenance>) {
    memo_for_canonical(FIXTURE_CANONICAL, source)
}

/// SERVICE-backed fixture memo — the production lazy shape: the memo comes
/// out of a full [`ShallowFileState`] construction (which installs the ONE
/// shared lens pair from the finished state), a live lowering service
/// retains the parse, and nothing lowers until first demand.
fn memo_for_canonical(canonical: &str, source: &str) -> (Arc<DeclBodyMemo>, Arc<MetaProvenance>) {
    let (state, provenance) =
        crate::resolver_core::ShallowFileState::service_backed_with_provenance_for_test(
            canonical, source,
        );
    (Arc::clone(state.decl_bodies()), provenance)
}

fn bodies(p: &MetaProvenance) -> u64 {
    p.decl_bodies_lowered
        .load(std::sync::atomic::Ordering::Relaxed)
}

fn parses(p: &MetaProvenance) -> u64 {
    p.eval_program_parses
        .load(std::sync::atomic::Ordering::Relaxed)
}

const FIVE_DECLS: &str = "export type Unrelated = { a: 1 };\n\
     type Var0 = { v: 0 };\n\
     type Var1 = { v: 1 };\n\
     type Var2 = { v: 2 };\n\
     type Var3 = { v: 3 };\n";

#[test]
fn demand_lowers_only_the_demanded_statement() {
    let (memo, provenance) = memo_for(FIVE_DECLS);
    assert_eq!(bodies(&provenance), 0, "construction lowers nothing");

    let decl = memo.type_decl("Unrelated").expect("Unrelated exists");
    assert!(matches!(decl.kind, TypeDeclKind::Alias));
    assert_eq!(
        bodies(&provenance),
        1,
        "ONE demanded statement ⇒ one body lowered — never the other four"
    );

    // Re-demand: served from the entry, no re-lowering.
    let again = memo.type_decl("Unrelated").expect("still exists");
    assert!(Arc::ptr_eq(&decl, &again), "the cached entry is returned");
    assert_eq!(bodies(&provenance), 1, "a warm demand lowers nothing");
}

#[test]
fn distinct_demands_share_one_retained_parse() {
    let (memo, provenance) = memo_for(FIVE_DECLS);
    for name in ["Unrelated", "Var0", "Var1"] {
        assert!(memo.type_decl(name).is_some(), "{name} must lower");
    }
    assert_eq!(
        parses(&provenance),
        1,
        "three demands on one file must share ONE retained eval-program parse"
    );
    assert_eq!(bodies(&provenance), 3);
}

#[test]
fn merged_interface_demand_folds_all_contributors() {
    let (memo, provenance) = memo_for(
        "interface Merged { a: string }\ntype Unrelated = { u: 1 };\ninterface Merged { b: number }\n",
    );
    let decl = memo.type_decl("Merged").expect("Merged exists");
    assert!(
        decl.body.is_merged(),
        "two same-name interface contributors fold into the Merged carrier"
    );
    assert_eq!(decl.body.contributors().len(), 2);
    assert_eq!(
        bodies(&provenance),
        2,
        "both contributors lower (they ARE the demanded group) — the \
         unrelated statement does not"
    );
    assert!(
        memo.type_decl("Unrelated").is_some(),
        "the unrelated symbol still lowers on ITS OWN demand"
    );
    assert_eq!(bodies(&provenance), 3);
}

#[test]
fn class_statement_backfills_its_value_sibling() {
    let (memo, provenance) = memo_for("class K { a: number }\ntype Other = { o: 1 };\n");
    let type_side = memo.type_decl("K").expect("class type side");
    assert!(matches!(type_side.kind, TypeDeclKind::Class));
    // One class statement lowers BOTH its type and value declarations.
    assert_eq!(bodies(&provenance), 2);

    // The value side was backfilled from the same job — no re-lowering.
    let value_side = memo.value_decl("K").expect("class value side");
    assert!(matches!(value_side.kind, ValueDeclKind::Class));
    assert!(
        value_side.object_shape.is_some(),
        "the constructor shape rides on the backfilled value side"
    );
    assert_eq!(
        bodies(&provenance),
        2,
        "the backfilled sibling must NOT lower again"
    );
}

#[test]
fn dependency_paths_ride_on_the_lowered_entry() {
    let (memo, _) = memo_for(
        "import { Ext } from './dep';\ntype WithDeps = { a: Ext; b: Local };\ntype Local = { v: 1 };\n",
    );
    let decl = memo.type_decl("WithDeps").expect("WithDeps exists");
    assert!(has_dependency(&decl, "Ext"));
    assert!(has_dependency(&decl, "Local"));
    assert!(
        !has_dependency(&decl, "WithDeps"),
        "self is not a dependency"
    );
}

/// A `export default class Props extends Imported {}` lowers under BOTH
/// its declared name `Props` AND the `default` alias. The dependency-edge
/// collector must mirror that aliasing: BOTH entries must carry the
/// heritage dep `Imported`. Pre-fix the collector emits deps ONLY under
/// `default`, so the declared-name `Props` body loses `Imported`
/// (under-resolution + under-invalidation).
#[test]
fn default_class_declared_name_and_alias_both_carry_heritage_dep() {
    let (memo, _) = memo_for(
        "import { Imported } from './dep';\nexport default class Props extends Imported {}\n",
    );
    let declared = memo.type_decl("Props").expect("declared-name type side");
    assert!(
        has_dependency(&declared, "Imported"),
        "the default class's declared-name body must carry its heritage dep; got {:?}",
        declared.dependency_paths
    );
    let aliased = memo.type_decl("default").expect("default alias");
    assert!(
        has_dependency(&aliased, "Imported"),
        "the `default` alias body must carry its heritage dep; got {:?}",
        aliased.dependency_paths
    );
}

/// A namespaced type alias lowers under its qualified name `N.T`. The
/// dependency-edge collector must emit deps under the SAME qualified key
/// (mirroring the header index), else `N.T`'s body is cached with EMPTY
/// deps. Pre-fix the `TSModuleDeclaration` statement yields no dep records
/// at all.
#[test]
fn namespaced_type_alias_carries_its_dep_under_qualified_name() {
    let (memo, _) = memo_for(
        "import { Imported } from './dep';\nexport namespace N { export type T = Imported }\n",
    );
    let decl = memo.type_decl("N.T").expect("namespaced type symbol N.T");
    assert!(
        has_dependency(&decl, "Imported"),
        "namespaced `N.T` body must carry its dep `Imported`; got {:?}",
        decl.dependency_paths
    );
}

/// A nested namespace lowers under the doubly-qualified name
/// `Outer.Inner`. The collector must mirror the nested qualification.
#[test]
fn nested_namespace_type_alias_carries_dep_under_double_qualified_name() {
    let (memo, _) = memo_for(
        "import { Imported } from './dep';\nnamespace Outer { export namespace Inner { export type T = Imported } }\n",
    );
    let decl = memo
        .type_decl("Outer.Inner.T")
        .expect("nested namespaced type symbol");
    assert!(
        has_dependency(&decl, "Imported"),
        "nested `Outer.Inner.T` body must carry its dep `Imported`; got {:?}",
        decl.dependency_paths
    );
}

/// A JSDoc `@typedef {Imported} Alias` lowers a body referencing
/// `Imported`. The lazy lowering path must compute its dependency roots
/// from the lowered JSDoc `TypeExpr` and store them on the entry — JSDoc
/// is not a statement, so the statement dep-collector never sees it.
/// Pre-fix the typedef entry is cached with EMPTY deps.
#[test]
fn jsdoc_typedef_carries_its_dep() {
    let (memo, _) = memo_for(
        "import { Imported } from './dep';\n/** @typedef {Imported} Alias */\ntype Real = { r: 1 };\n",
    );
    let decl = memo.type_decl("Alias").expect("typedef must lower");
    assert!(
        has_dependency(&decl, "Imported"),
        "the JSDoc typedef body must carry its dep `Imported`; got {:?}",
        decl.dependency_paths
    );
}

#[test]
fn jsdoc_typedef_preserves_qualified_nullable_dependency_paths() {
    let (memo, _) = memo_for(
        "import * as NS from './dep';\n\
         /** @typedef {NS.Value} Plain */\n\
         /** @typedef {?NS.Value} Nullable */\n\
         /** @typedef {!NS.Value} NonNullable */\n",
    );

    for name in ["Plain", "Nullable", "NonNullable"] {
        let declaration = memo.type_decl(name).unwrap();
        assert!(
            has_dependency(&declaration, "NS.Value"),
            "{name} lost its qualified JSDoc dependency: {:?}",
            declaration.dependency_paths,
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn concurrent_first_touch_lowers_once() {
    let (memo, provenance) = memo_for(FIVE_DECLS);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let memo = Arc::clone(&memo);
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            memo.type_decl("Unrelated").is_some()
        }));
    }
    for handle in handles {
        assert!(handle.join().expect("no panics"));
    }
    assert_eq!(
        bodies(&provenance),
        1,
        "8 concurrent first-touches of one symbol must lower it exactly once"
    );
}

/// The shared per-symbol demand cell carries the LeaseMiss outcome ITSELF,
/// so EVERY concurrent waiter on one broken-lease demand observes the DISTINCT
/// `LeaseMiss` — never a false `Ready(None)`. Pre-fix the cell committed a bare
/// `None` (Ready-shaped) and the lease-miss lived in a THREAD-LOCAL flag: a
/// joiner that blocked on the initializer's `get_or_init` read the committed
/// `None` with its own flag unset and returned `Ready(None)` — the false
/// genuine-miss a concurrent cache-admitting consumer could warm-admit as
/// declaration absence. Post-fix the committed cell is `DemandCell::LeaseMiss`,
/// visible to every waiter, and the poisoned cell is evicted so a retry
/// recomputes under a live lease.
///
/// Discrimination (RED against the pre-change tree, GREEN after): under a
/// broken lease, N threads demand ONE not-yet-lowered symbol simultaneously.
/// The barrier makes them grab the same demand cell before the initializer
/// evicts it, so most park as joiners. Pre-fix the joiners return `Ready(None)`
/// (the false-miss count is nonzero); post-fix all N return `LeaseMiss`.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn concurrent_broken_lease_demand_every_waiter_sees_lease_miss() {
    for _round in 0..4 {
        let (memo, _provenance) = memo_for(FIVE_DECLS);

        // Pin the lease with one successful demand, then break the retained
        // snapshot out-of-band so every subsequent demand lease-misses.
        assert!(memo.type_decl("Var0").is_some());
        memo.release_retained_snapshot_for_test();

        const THREADS: usize = 32;
        let barrier = Arc::new(std::sync::Barrier::new(THREADS));
        let mut handles = Vec::new();
        for _ in 0..THREADS {
            let memo = Arc::clone(&memo);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                // Demand a DIFFERENT, not-yet-lowered symbol so the demand
                // actually runs and lease-misses (rather than a warm hit).
                match memo.type_decl_outcome_in(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    "Var1",
                ) {
                    DemandOutcome::LeaseMiss => 0u8,
                    DemandOutcome::Ready(None) => 1u8,
                    DemandOutcome::Ready(Some(_)) => 2u8,
                }
            }));
        }
        let tags: Vec<u8> = handles
            .into_iter()
            .map(|h| h.join().expect("no waiter thread may panic"))
            .collect();

        let false_some = tags.iter().filter(|&&t| t == 2).count();
        let false_miss = tags.iter().filter(|&&t| t == 1).count();
        assert_eq!(
            false_some, 0,
            "no waiter may observe a resolved body under a broken lease"
        );
        assert_eq!(
            false_miss, 0,
            "every waiter on a broken-lease demand must observe the DISTINCT \
             LeaseMiss; a `Ready(None)` here is the false genuine-miss a joiner \
             would warm-admit as declaration absence (got {false_miss} of {THREADS})"
        );
        // No false-warm cell survives: the poisoned cell is evicted.
        assert!(
            !memo.type_entry_materialized("Var1"),
            "a broken-lease demand must leave no committed cell for the real symbol"
        );
    }
}

#[test]
fn whole_env_is_a_memoized_whole_file_demand() {
    let (memo, provenance) = memo_for(FIVE_DECLS);
    assert!(!memo.whole_env_materialized());
    let env = memo.whole_env();
    assert_eq!(env.type_symbols.len(), 5);
    assert_eq!(
        bodies(&provenance),
        5,
        "the whole-file env lowers the full declaration set once"
    );
    let again = memo.whole_env();
    assert!(Arc::ptr_eq(&env, &again));
    assert_eq!(bodies(&provenance), 5, "the env is memoized — no rebuild");
}

#[test]
fn jsdoc_typedef_lowers_on_demand() {
    let (memo, provenance) =
        memo_for("/** @typedef {{a: number}} FromDoc */\ntype Real = { r: 1 };\n");
    let decl = memo.type_decl("FromDoc").expect("typedef must lower");
    assert!(matches!(decl.kind, TypeDeclKind::Alias));
    assert_eq!(
        bodies(&provenance),
        1,
        "the typedef body lowers alone — the TS statement does not"
    );
}

#[test]
fn augmentation_scoped_demand_lowers_only_the_block() {
    let source = r#"declare module "vue" {
  interface ComponentCustomProperties { $x: string }
  const injected: number;
}
type FileScope = { f: 1 };
"#;
    let (memo, provenance) = memo_for(source);
    let scope = AugmentationScopeKind::Module("vue".to_string());
    let decl = memo
        .augmentation_type_decl_in(
            &scope,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "ComponentCustomProperties",
        )
        .expect("augmentation entry exists");
    assert!(matches!(decl.kind, TypeDeclKind::Interface));
    // The block statement lowers both its inner declarations (type +
    // value contributor), never the file-scope statement.
    //
    // NOTE: `bodies == 2` CHARACTERIZES the CURRENT statement-granular
    // lowering — the `declare module` block is a single statement, and
    // its one lowering job materialises ALL of that block's inner
    // declarations together, so demanding the interface also lowers the
    // sibling `const`. This is NOT a claim that augmentation members
    // lower per-declaration; tightening the locator granularity to
    // per-declaration / per-declarator is tracked as deferred debt in
    // `docs/arch/semantic-db-overhaul-unified-remaining-plan.md` (the
    // decl-body locator granularity item).
    assert_eq!(bodies(&provenance), 2);
    assert!(
        memo.augmentation_value_decl(&scope, "injected").is_some(),
        "the value sibling was backfilled from the same block job"
    );
    assert_eq!(
        bodies(&provenance),
        2,
        "backfilled sibling does not re-lower"
    );
}

#[test]
fn unknown_names_are_none_without_lowering() {
    let (memo, provenance) = memo_for(FIVE_DECLS);
    assert!(memo.type_decl("Missing").is_none());
    assert!(memo.value_decl("Missing").is_none());
    assert!(memo
        .augmentation_type_decl_in(
            &AugmentationScopeKind::Global,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "Missing",
        )
        .is_none());
    assert_eq!(bodies(&provenance), 0, "a miss lowers nothing");
    assert_eq!(parses(&provenance), 0, "a miss parses nothing");
}

/// Header↔lowerer arm parity, proven EXHAUSTIVELY rather than by
/// per-kind spot-checks: build an all-decl-kinds fixture, enumerate
/// EVERY symbol the shallow header walk indexed, and assert each one
/// demand-resolves to `Some` through its matching lazy accessor. A
/// header kind the lowerer cannot resolve (an arm the header walk
/// registers but `lower_demanded` never produces) would fail here.
#[test]
fn every_header_symbol_demand_resolves_through_matching_accessor() {
    const ALL_DECL_KINDS: &str = r#"type AliasT = { a: 1 };
interface IFace { b: number }
class Klass { m(): void {} }
enum Color { Red, Green }
function fn1(x: number): string { return ""; }
const konst = { k: 1 };
let lett: number = 2;
var varr: string = "x";
namespace Ns {
  export type Inner = { i: 1 };
  export interface NInner { n: 2 }
}
declare module "ext" {
  interface AugT { at: string }
  const augVal: number;
}
/** @typedef {{d: number}} FromDoc */
"#;

    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        oxc_parser::Parser::new(&allocator, ALL_DECL_KINDS, oxc_span::SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse");
    let header_index = verter_semantic::analysis::decl_headers::build_decl_header_index(
        &parsed.program,
        ALL_DECL_KINDS,
    );
    let (memo, _) = memo_for(ALL_DECL_KINDS);

    // Non-vacuity: every kind actually produced a header entry, so the
    // parity loops below are not trivially empty. (Class registers BOTH
    // a type side and a value side; a namespace's type members register
    // under their qualified `Ns.<name>` key.)
    for name in [
        "AliasT",
        "IFace",
        "Klass",
        "Ns.Inner",
        "Ns.NInner",
        "FromDoc",
    ] {
        assert!(
            header_index.type_headers.contains_key(name),
            "type-side header `{name}` must be indexed"
        );
    }
    for name in ["Klass", "fn1", "konst", "lett", "varr"] {
        assert!(
            header_index.value_headers.contains_key(name),
            "value-side header `{name}` must be indexed"
        );
    }
    assert!(
        header_index.enum_headers.contains_key("Color"),
        "the enum must be header-indexed (header-only kind)"
    );
    let module_scope = AugmentationScopeKind::Module("ext".to_string());
    assert!(
        header_index
            .augmentation_type_headers
            .get(&module_scope)
            .is_some_and(|m| m.contains_key("AugT")),
        "augmentation type-member header must be indexed"
    );
    assert!(
        header_index
            .augmentation_value_headers
            .get(&module_scope)
            .is_some_and(|m| m.contains_key("augVal")),
        "augmentation value-member header must be indexed"
    );

    // PARITY — every indexed header symbol demand-resolves through its
    // matching accessor.
    for key in header_index.type_headers.keys() {
        assert!(
            memo.type_decl_in(key.owner, key.name.as_ref()).is_some(),
            "type header `{}` must demand-resolve through type_decl",
            key.name,
        );
    }
    for key in header_index.value_headers.keys() {
        assert!(
            memo.value_decl_in(key.owner, key.name.as_ref()).is_some(),
            "value header `{}` must demand-resolve through value_decl",
            key.name,
        );
    }
    for (scope, names) in &header_index.augmentation_type_headers {
        for key in names.keys() {
            assert!(
                memo.augmentation_type_decl_in(scope, key.owner, key.name.as_ref())
                    .is_some(),
                "augmentation type header `{}` in {scope:?} must demand-resolve",
                key.name,
            );
        }
    }
    for (scope, names) in &header_index.augmentation_value_headers {
        for key in names.keys() {
            assert!(
                memo.augmentation_value_decl_in(scope, key.owner, key.name.as_ref())
                    .is_some(),
                "augmentation value header `{}` in {scope:?} must demand-resolve",
                key.name,
            );
        }
    }

    // An enum is a dual-space symbol: it ALSO registers a type header
    // (the projected-type union) and a value header (the `typeof` object),
    // so `Color` demand-resolves through BOTH the `type_decl` and
    // `value_decl` loops above. The dedicated `enum_headers` table
    // additionally carries the member NAMES for the member-presence facts
    // rail — that name authority has no separate lazy-body accessor.
}

#[test]
fn merged_same_name_enum_resolves_all_members_in_both_spaces_through_the_memo() {
    // TS declaration merging: two same-name `enum E` bodies contribute to one
    // enum. The lazily-served value body (`typeof E` / `E.member`) and type
    // body (the projected-type union) must each carry the UNION of ALL
    // contributors' members. A `primary()`-only value fold or a last-wins
    // `merged_body()` type fold would drop the first declaration's `A`/`B`; the
    // memo serves the merged set for both spaces instead.
    let source = "enum E { A = 1, B = 2 }\nenum E { C = 3, D = 4 }\n";
    let (memo, _) = memo_for(source);

    // Value space: merged member set in source order, through `value_decl`.
    let value = memo.value_decl("E").expect("enum value body resolves");
    let names: Vec<&str> = value
        .enum_members
        .as_ref()
        .expect("value body carries enum_members")
        .members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["A", "B", "C", "D"],
        "merged enum value-space members must union all contributors in source order"
    );

    // Type space, through `type_decl`: the merge-aware body must retain
    // BOTH contributor body slots (a last-wins `merged_body()` fold would
    // collapse to one slot and drop the first declaration's `A`/`B` arms
    // from the value-derived projected union).
    let ty = memo.type_decl("E").expect("enum type body resolves");
    assert_eq!(
        ty.body.contributors().len(),
        2,
        "merged enum TYPE body must retain BOTH same-name contributor slots \
         (a last-wins fold drops the first declaration); got {:?}",
        ty.body
    );
    // The projected-type union derives from the merged VALUE rail: the
    // folded member scalars must carry all 4 member literals in source
    // order — the exact arms the value-derived type union projects.
    let scalar_values: Vec<&verter_type_expr::facts::EnumScalar> = value
        .enum_members
        .as_ref()
        .expect("value body carries enum_members")
        .members
        .iter()
        .map(|member| &member.value)
        .collect();
    for (index, literal) in ["1", "2", "3", "4"].iter().enumerate() {
        assert_eq!(
            scalar_values[index],
            &verter_type_expr::facts::EnumScalar::Number((*literal).to_string()),
            "merged enum member {index} must fold to the numeric literal {literal}; \
             got {scalar_values:?}"
        );
    }
}

#[test]
fn raw_surfaces_merge_overload_groups_for_the_demanded_name() {
    let source = "function f(a: number): string;\nfunction f(b: string): string;\nfunction f(x: unknown): string { return '' }\ntype Unrelated = { u: 1 };\n";
    let (memo, _) = memo_for(source);
    let surfaces = memo.raw_surfaces_for("f", SymbolSpace::Value);
    assert_eq!(surfaces.len(), 1, "one merged overload-group surface");
    assert!(
        surfaces[0].overload_signatures.len() >= 2,
        "the overload SET arity must survive the merge"
    );
    assert_eq!(surfaces[0].decl_canonical, "/ws/fixture.ts");
    let none = memo.raw_surfaces_for("Missing", SymbolSpace::Type);
    assert!(none.is_empty());
}

/// Seed a memo from an already-built env for `source`. Only ENUM type cells
/// (whose fingerprint derives from scalar-union arms — lens-independent) and
/// value cells may be seeded; a non-enum type cell fails loudly inside
/// `lowered_type_decl_from_group` (see `seeded_non_enum_merged_type_cell_fails_loudly`).
fn seeded_memo_for(source: &str) -> DeclBodyMemo {
    let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
    let analysis =
        verter_parser::utils::oxc::script::type_inventory::AnalyzedExternalTypeSource::default();
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    let header_index = Arc::new(
        verter_semantic::analysis::decl_headers::build_decl_header_index(&parsed.program, source),
    );
    DeclBodyMemo::seeded_from_env(
        SnapshotKey {
            canonical: Arc::from("/ws/seeded.ts"),
            whole_hash: [9u8; 16],
            parse_env_hash: [0u8; 16],
        },
        &env,
        &analysis,
        header_index,
    )
}

#[test]
fn seeded_memo_matches_lazy_fold() {
    // An enum is the ONE type-space shape a seeded (locator-only,
    // transient-less) env may fingerprint: its `body_hash` derives from the
    // scalar-union arms, which carry no `Ref` sites — the cross-decl lens is
    // never consulted, so the seed-time `UnresolvedLens` fingerprint must be
    // BYTE-IDENTICAL to the lazy fold's `ShallowLens` fingerprint.
    let source = "enum E { A = 1, B = 2 }\nconst v = { k: 1 };\n";
    let seeded = seeded_memo_for(source);
    let (lazy, _) = memo_for(source);

    let seeded_decl = seeded.type_decl("E").expect("seeded enum type entry");
    let lazy_decl = lazy.type_decl("E").expect("lazy enum type entry");
    assert_eq!(
        seeded_decl.body_hash, lazy_decl.body_hash,
        "enum seed-time fingerprint (UnresolvedLens) must equal the lazy \
         fold's (ShallowLens) — the arms are lens-independent"
    );
    let value = seeded.value_decl("v").expect("seeded value entry");
    assert!(value.object_shape.is_some());
    assert!(seeded.whole_env_materialized(), "seeding pre-sets the env");
}

#[test]
fn lowered_value_decl_copies_narrowed_facts_and_fingerprints_at_lowering_time() {
    // The lazily-served VALUE record carries narrowed FACTS copied off the
    // registered group (never re-derived from a raw body): the single-hop
    // `typeof base` peel target rides `type_annotation.typeof_alias_target`
    // (the Seam-1 consumer read), and the value-body fingerprint is computed
    // AT LOWERING TIME from the TRANSIENT lowered annotation. Dropping the
    // per-name value-transient retention would produce the DEGRADED
    // (`budget_exceeded = true`) fallback instead — this test fails on that
    // regression. (Two distinct producer mechanisms, no confinement claim:
    // the DEGRADED fallback is forced by the session fold
    // (`lowered_value_decl_from_group`), VALUE-only; the shared encoder
    // separately sets `budget_exceeded` on a REAL deep annotation via its
    // `MAX_HASH_DEPTH` cap, type and value alike; the parse-domain
    // admission's bit-drop is pre-existing — see
    // `ValueBodyHashFact::budget_exceeded`.)
    let source = "const base: { a: number } = { a: 1 };\nconst alias: typeof base = base;\n";
    let (memo, _) = memo_for(source);

    let alias = memo.value_decl("alias").expect("alias value body resolves");
    assert!(matches!(
        alias.type_annotation.classification,
        ValueAnnotationClass::TypeOfAlias
    ));
    let target = alias
        .type_annotation
        .typeof_alias_target
        .as_ref()
        .expect("single-hop non-self `typeof base` mints the peel-target fact");
    assert_eq!(target.symbol.as_ref(), "base");

    let base = memo.value_decl("base").expect("base value body resolves");
    assert!(
        !alias.body_hash.budget_exceeded && !base.body_hash.budget_exceeded,
        "the lazy path fingerprints the TRANSIENT annotation — never the \
         degraded transient-less fallback"
    );
    assert_ne!(
        alias.body_hash.hash, base.body_hash.hash,
        "the fingerprint observes the annotation BODY, so distinct \
         annotations must fingerprint distinctly"
    );
}

#[test]
fn seeded_annotated_value_cell_degrades_its_fingerprint_honestly() {
    // A seeded (transient-less) env cannot fingerprint an annotated value's
    // body — the record must carry the DEGRADED `budget_exceeded` outcome
    // (an honest bit, never a fabricated fingerprint that would collide two
    // different annotations), forced by the session fold
    // (`lowered_value_decl_from_group`) — a VALUE-only mechanism, distinct
    // from the shared encoder's `MAX_HASH_DEPTH` depth-cap; the type-side
    // transient-less non-enum fold fails loudly instead (see
    // `seeded_non_enum_merged_type_cell_fails_loudly`). Pre-existing
    // admission semantics: see `ValueBodyHashFact::budget_exceeded`. An
    // enum stays FULL-fidelity: its fingerprint derives entirely from the
    // folded member facts.
    let source = "const c: { a: 1 } = { a: 1 };\nenum E { A = 1 }\n";
    let seeded = seeded_memo_for(source);

    let annotated = seeded.value_decl("c").expect("seeded annotated value");
    assert!(
        annotated.body_hash.budget_exceeded,
        "a transient-less annotated value must degrade, not fabricate"
    );
    let enum_value = seeded.value_decl("E").expect("seeded enum value");
    assert!(
        !enum_value.body_hash.budget_exceeded,
        "an enum fingerprints fully from its folded member facts"
    );

    let (lazy, _) = memo_for(source);
    let lazy_enum = lazy.value_decl("E").expect("lazy enum value");
    assert_eq!(
        enum_value.body_hash, lazy_enum.body_hash,
        "the enum value-body fingerprint is fact-derived, so the seeded and \
         lazy folds must agree byte-for-byte"
    );
}

/// CHARACTERIZATION of the degraded (transient-less/seeded) value fold — the
/// ACTUAL behavior, not an admission fence (none exists at the shared
/// parse-domain body-fact admission; the bit-drop there is the pre-existing
/// carry-forward recorded in the deferral doc): the degraded fingerprint is
/// DETERMINISTIC with `budget_exceeded` stored honestly on the memo fact
/// (forced by the session fold `lowered_value_decl_from_group`, VALUE-only),
/// and FOR THIS SHALLOW FIXTURE the parse-domain fact rail's reader
/// (`compat_value_body_hash_input` over the demand-lowered FILE memo, the
/// one memo `LazyBodyFactSource` is wired to) serves the NON-degraded
/// transient-fingerprinted record. (No confinement claim — the OTHER,
/// distinct producer mechanism reaches the rail too: a REAL deep annotation
/// on the file memo sets `budget_exceeded` honestly via the shared encoder's
/// `MAX_HASH_DEPTH` cap, type and value alike, and reaches the rail with the
/// bit set — dropped at the pre-existing admission.)
#[test]
fn degraded_seeded_value_fingerprint_is_deterministic_and_honest() {
    let source = "const c: { a: 1 } = { a: 1 };\n";

    // DETERMINISTIC + honest: two independent transient-less folds of the
    // same source mint the SAME degraded fingerprint, bit set.
    let seeded_a = seeded_memo_for(source);
    let seeded_b = seeded_memo_for(source);
    let a = seeded_a.value_decl("c").expect("seeded annotated value");
    let b = seeded_b.value_decl("c").expect("seeded annotated value");
    assert!(
        a.body_hash.budget_exceeded,
        "a transient-less annotated value fold degrades (bit stored honestly)"
    );
    assert_eq!(
        a.body_hash, b.body_hash,
        "the degraded fingerprint is deterministic — content-addressed \
         invalidation stays sound for seeded records"
    );

    // FIXTURE-SCOPE: the fact-rail reader reads the demand-lowered FILE
    // memo, whose fold retained the annotation transient — non-degraded for
    // this shallow source, and a DIFFERENT fingerprint from the degraded
    // transient-less one (so a degraded record standing in for the real one
    // would be observable).
    let (lazy, _) = memo_for(source);
    let lowered = lazy.value_decl("c").expect("lazy annotated value");
    let rail = crate::fact_emission::compat_value_body_hash_input(&lowered);
    assert!(
        !rail.budget_exceeded,
        "this fixture's file-memo record is non-degraded (a shallow \
         annotation fingerprints without hitting the shared encoder's \
         depth cap)"
    );
    assert_ne!(
        rail.hash, a.body_hash.hash,
        "the degraded fingerprint differs from the transient-fingerprinted \
         one (the annotation body is observed only by the real fold)"
    );
}

/// A NON-ENUM seeded type cell has NO retained transient lowered bodies to
/// fingerprint — the fold must fail loudly (fail-lowering) instead of
/// hashing a fabricated fingerprint. The MERGED branch is the discriminating
/// case: before the loud guard, `Merged(&[])` silently minted an
/// empty-object fingerprint (the single-body branch already panicked via its
/// `expect`).
#[test]
#[should_panic(expected = "never a fabricated fingerprint")]
fn seeded_non_enum_merged_type_cell_fails_loudly() {
    let source = "interface Merged { a: string }\ninterface Merged { b: number }\n";
    let _ = seeded_memo_for(source);
}

/// Concurrent TYPE+VALUE demand of one merged name (`class K {}` — or
/// an `interface K {} + class K {}` merge — occupies BOTH the type and
/// value spaces) must NOT deadlock. Backfill publishes the sibling
/// space, so a `type_decl("K")` job sets the value cell and a
/// `value_decl("K")` job sets the type cell. If backfill ran INSIDE the
/// demanded cell's `get_or_init` closure, the two threads would each
/// hold their own cell's init-lock while `OnceLock::set` blocks on the
/// sibling cell the other thread is mid-initialising — a lock cycle.
/// Backfill runs AFTER `get_or_init` returns (the init-lock is already
/// released), so the cycle cannot form. Pre-fix: this hangs (the
/// watchdog `recv_timeout` fires and the test FAILS). Post-fix: both
/// demands complete in milliseconds.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn concurrent_type_and_value_demand_of_merged_name_does_not_deadlock() {
    use std::sync::mpsc;
    use std::time::Duration;

    // MANY merged names (`class Ki {}` occupies both the type and value
    // space) on ONE memo, each demanded concurrently from its TYPE side
    // and its VALUE side. The lowering jobs all serialize on the memo's
    // single retained-snapshot worker, so flooding the queue guarantees
    // that an EARLY-completing thread reaches its backfill `set()` of the
    // sibling cell while the sibling thread is STILL queued, holding that
    // cell's init-lock. If backfill ran INSIDE the demanded cell's
    // `get_or_init` closure, that is a lock cycle (A holds type[Ki], waits
    // on value[Ki]; B holds value[Ki], waits on type[Ki]) and the run
    // hangs. Backfill must run AFTER `get_or_init` returns. Pre-fix: the
    // watchdog `recv_timeout` fires and the test FAILS. Post-fix: every
    // demand completes promptly.
    const NAMES: usize = 48;
    let mut src = String::new();
    for i in 0..NAMES {
        // A non-trivial body so each lowering job takes real time and the
        // worker queue stays deep while early threads drain.
        src.push_str(&format!(
            "export class K{i} {{ a{i}: {{ p: string; q: number }} = {{ p: '', q: 0 }}; \
             b{i}: {{ r: boolean }} = {{ r: false }}; }}\n"
        ));
    }

    let (memo, _) = memo_for(&src);
    let barrier = Arc::new(std::sync::Barrier::new(NAMES * 2));
    let (tx, rx) = mpsc::channel::<bool>();

    let mut handles = Vec::new();
    for i in 0..NAMES {
        let name = format!("K{i}");
        for type_side in [true, false] {
            let memo = Arc::clone(&memo);
            let barrier = Arc::clone(&barrier);
            let tx = tx.clone();
            let name = name.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                let ok = if type_side {
                    memo.type_decl(&name).is_some()
                } else {
                    memo.value_decl(&name).is_some()
                };
                let _ = tx.send(ok);
            }));
        }
    }
    drop(tx);

    for _ in 0..(NAMES * 2) {
        let ok = rx.recv_timeout(Duration::from_secs(30)).expect(
            "concurrent type+value demand of merged names must not deadlock \
             (a backfill-inside-get_or_init lock cycle would hang here)",
        );
        assert!(ok, "every merged-name demand must resolve to Some");
    }
    for handle in handles {
        handle.join().expect("no worker thread may panic");
    }
}

/// The memo's demanded-lowering path is LEASE-ONLY: with the lease pin
/// broken out-of-band (the invariant-violation scenario), a body demand
/// fails CLOSED via ReturnOnly in DEBUG *and* RELEASE — it returns the
/// empty/None fallback to the caller but NEVER memoizes it as a wrong-empty
/// warm entry, and it lowers nothing / never transiently re-parses. A retry
/// under a live lease recovers.
///
/// This supersedes the prior debug-only-loudness contract (a `debug_assert!`
/// that PANICKED in debug and left the release build silently memoizing an
/// empty env / body-less decl / empty capture). Per "fail lowering, not
/// silent-skip", the release build must ALSO refuse to admit the
/// silent-wrong-empty warm entry — so all three lease-only arms now route
/// ReturnOnly uniformly.
///
/// Discrimination (RED against the pre-change tree, GREEN after): the
/// pre-change `type_decl` / `whole_env` / `raw_surfaces_for` lease-miss arms
/// PANICKED in this (debug_assertions) build, so the direct
/// `memo.type_decl("Var1")` call below would abort the test — RED. Post-change
/// each arm returns cleanly AND leaves its cell UNCOMMITTED (the
/// `*_materialized` probes), while the work / parse counters stay flat: a
/// silently-memoized wrong-empty entry (the release defect) would flip a
/// `*_materialized` probe to `true` — GREEN only when NOTHING is admitted.
#[test]
fn broken_lease_body_demand_fails_closed_return_only_without_caching() {
    let (memo, provenance) = memo_for(FIVE_DECLS);

    // First demand pins the lease and lowers the demanded body.
    assert!(memo.type_decl("Var0").is_some());
    assert_eq!(parses(&provenance), 1, "the lease acquisition parses once");
    let lowered_before = bodies(&provenance);

    // Break the lease pin out-of-band: the memo still HOLDS its
    // `SnapshotLease` (so `ensure_lease` will not re-acquire), but the
    // worker-side retained snapshot is released — every subsequent demand
    // lease-misses.
    let service = memo
        .service
        .as_ref()
        .expect("a production memo has a service");
    service.release_retained_snapshot_for_test(&memo.key);

    // 1. Per-symbol body demand (`lower_demanded`): ReturnOnly — a CLEAN None
    //    (no panic), and NO body-less warm entry is admitted for `Var1`.
    assert!(
        memo.type_decl("Var1").is_none(),
        "a body demand with a broken lease must fail CLOSED to None via ReturnOnly \
         (never a panic, never a transient re-parse)"
    );
    assert!(
        !memo.type_entry_materialized("Var1"),
        "the broken-lease per-symbol demand must NOT memoize a body-less warm entry"
    );

    // 2. Whole-file env demand: ReturnOnly — an empty env is returned but NOT
    //    memoized (the release-build silent-wrong-empty defect).
    let env = memo.whole_env();
    assert_eq!(
        env.total_decl_count(),
        0,
        "the broken-lease whole_env must fail closed to an empty env"
    );
    assert!(
        !memo.whole_env_materialized(),
        "the broken-lease whole_env must NOT memoize a wrong-empty env"
    );

    // 3. Raw-surface capture demand: ReturnOnly — an empty capture is returned
    //    but NOT inserted into the capture map.
    let surfaces = memo.raw_surfaces_for("Var1", SymbolSpace::Type);
    assert!(
        surfaces.is_empty(),
        "the broken-lease raw_surfaces_for must fail closed to an empty capture"
    );
    assert!(
        !memo.raw_surfaces_materialized("Var1", SymbolSpace::Type),
        "the broken-lease raw_surfaces_for must NOT memoize a wrong-empty capture"
    );

    // All three ReturnOnly arms lowered NOTHING and re-parsed NOTHING.
    assert_eq!(
        bodies(&provenance),
        lowered_before,
        "the broken-lease demands must lower NOTHING"
    );
    assert_eq!(parses(&provenance), 1, "no re-parse is accounted");
}

/// A broken-lease locator deref surfaces the DISTINCT `LeaseMiss` outcome —
/// NOT a cacheable `UnknownSymbol`. The prior fix proved the local
/// `DeclBodyMemo` cell is left uncommitted; this pins the TYPED no-warm signal
/// that the enclosing `LowerLocator` / `Instantiate` build folds into
/// `cache_suppress`, so a transient ReturnOnly can never be cached as a real
/// resolution result.
///
/// Discrimination (RED against the pre-change tree, GREEN after): the
/// pre-change deref collapsed the lease-miss `None` into
/// `LocatorBodyDerefError::UnknownSymbol` (a genuine, cacheable resolution
/// result) — the enclosing memo would then warm-publish the derived
/// `Opaque(Miss)` as a false body. Post-change the deref returns the distinct
/// `LeaseMiss`.
#[test]
fn broken_lease_locator_deref_returns_lease_miss_not_unknown_symbol() {
    let (memo, _) = memo_for(FIVE_DECLS);
    // Pin the lease with one successful demand, then break the retained
    // snapshot out-of-band so every subsequent demand lease-misses.
    assert!(memo.type_decl("Var0").is_some());
    memo.release_retained_snapshot_for_test();

    // Deref a DIFFERENT, not-yet-lowered TYPE symbol so the demand actually
    // runs and lease-misses (rather than hitting an already-committed cell).
    let locator = type_body_locator("Var1", Vec::new());
    let err = memo
        .deref_locator_body(&locator)
        .expect_err("a broken-lease deref must fail typed, never fabricate a body");
    assert_eq!(
        err,
        LocatorBodyDerefError::LeaseMiss,
        "a broken-lease locator deref must surface the DISTINCT ReturnOnly \
         LeaseMiss, NOT a cacheable UnknownSymbol — collapsing them lets the \
         enclosing memo warm-publish the derived Opaque(Miss) as a false body"
    );
    // The lease-miss deref committed NOTHING (fail-closed no-warm rail).
    assert!(
        !memo.type_entry_materialized("Var1"),
        "the broken-lease deref must not memoize a body-less cell"
    );
}

/// Backfill is coverage-gated: a statement batch that lowered only a
/// SUBSET of a sibling symbol's contributors must NOT pre-fill that
/// sibling's entry — a narrower result pretending broader coverage
/// would mask the unlowered contributors (the interface half of an
/// interface+class merge, when the class's VALUE side was demanded
/// first).
#[test]
fn partial_contributor_batch_does_not_backfill_merged_sibling() {
    let (memo, _) =
        memo_for("export interface Foo { a: string }\nexport class Foo { b: number = 1 }\n");

    // Demand the VALUE side first: only the class statement lowers.
    let value = memo.value_decl("Foo").expect("class value side");
    assert!(matches!(value.kind, ValueDeclKind::Class));

    // The TYPE side has TWO contributors (interface + class). The
    // class statement alone must not have pre-filled it: the type
    // demand must still fold the full Merged carrier.
    let ty = memo.type_decl("Foo").expect("type side");
    assert!(
        ty.body.is_merged(),
        "interface+class must fold into the Merged carrier even when \
         the value side was demanded first; got {:?}",
        ty.body
    );
    assert_eq!(ty.body.contributors().len(), 2);
}

/// A locator into a type parameter's constraint slot derefs to the EXACT
/// authored constraint body — not the decl body, not the default slot.
#[test]
fn locator_deref_reaches_type_param_constraint_body() {
    let (memo, _) = memo_for("type Foo<T extends string> = { x: T };\n");
    let derefed = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("the ordinal-0 constraint body must deref");
    assert!(
        matches!(
            single(&derefed),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the ordinal-0 constraint must deref to the `string` primitive (not the \
         body, not the default); got {:?}",
        derefed.shape
    );
}

/// The constraint and default slots of one parameter are DISTINCT authored
/// bodies; `position` selects between them.
#[test]
fn locator_deref_distinguishes_constraint_from_default_body() {
    let (memo, _) = memo_for("type Foo<T extends string = number> = { x: T };\n");
    let constraint = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("constraint derefs");
    let default = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Default,
            }],
        ))
        .expect("default derefs");
    assert!(
        matches!(
            single(&constraint),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "constraint is `string`; got {:?}",
        constraint.shape
    );
    assert!(
        matches!(
            single(&default),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "default is `number`; got {:?}",
        default.shape
    );
    assert_ne!(
        single(&constraint),
        single(&default),
        "constraint and default must be distinct authored bodies — an impl that \
         ignores `position` collapses them"
    );
}

/// A constraint bound's deref env is the FULL sibling list plus the
/// position-exact visibility — TS constraints may reference later siblings
/// and self, so a pre-truncated prior-sibling prefix cannot express the
/// lexical view. `U extends keyof T` (U at ordinal 1) carries `[T, U]`
/// with `Constraint { ordinal: 1 }`.
#[test]
fn locator_deref_constraint_env_is_full_sibling_list() {
    let (memo, _) = memo_for("type Foo<T, U extends keyof T> = { x: T; y: U };\n");
    let derefed = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 1,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("U's constraint `keyof T` derefs");
    assert!(
        matches!(single(&derefed), TypeExpr::KeyOf(_)),
        "U's constraint is `keyof T`; got {:?}",
        derefed.shape
    );
    let names: Vec<&str> = derefed
        .type_parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["T", "U"],
        "the bound env carries the FULL sibling list — a prefix-truncated env \
         cannot bind a constraint's later-sibling / self references"
    );
    assert_eq!(
        derefed.visibility,
        TypeParamVisibility::Constraint { ordinal: 1 },
        "the deref carries the bound's position-exact visibility so the \
         binder-frame constructor reconstructs the TS lexical view"
    );
}

/// An ordinal beyond the parameter count fails closed with the typed
/// out-of-range error — never a fabricated body.
#[test]
fn locator_deref_type_param_ordinal_out_of_range_fails_closed() {
    let (memo, _) = memo_for("type Foo<T extends string> = { x: T };\n");
    let err = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 5,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect_err("an out-of-range ordinal must fail closed");
    assert_eq!(
        err,
        LocatorBodyDerefError::TypeParamOrdinalOutOfRange { ordinal: 5 }
    );
}

/// A referenced parameter with no constraint fails closed with the typed
/// absent-bound error (analogous to a missing value annotation).
#[test]
fn locator_deref_absent_type_param_bound_fails_closed() {
    let (memo, _) = memo_for("type Foo<T> = { x: T };\n");
    let err = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect_err("a param with no constraint must fail closed");
    assert_eq!(
        err,
        LocatorBodyDerefError::TypeParamBoundAbsent {
            ordinal: 0,
            position: TypeParamBoundPosition::Constraint,
        }
    );
}

/// A bound step anywhere other than `path[0]` is misplaced — type parameters
/// live on the header, not mid-body — and merged group-level type params are
/// unioned (there is no per-contributor header param axis).
#[test]
fn locator_deref_misplaced_type_param_bound_step_fails_closed() {
    // A bound step following a member step is misplaced.
    let (memo, _) = memo_for("type Foo<T> = { a: string };\n");
    let member_err = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
            ],
        ))
        .expect_err("a mid-path bound step must fail closed");
    assert_eq!(
        member_err,
        LocatorBodyDerefError::TypeParamBoundStepMisplaced
    );

    // A bound step following a merged-contributor step is misplaced: the
    // contributor header carries no per-contributor type-param axis.
    let (merged_memo, _) = memo_for("interface Bar { a: string }\ninterface Bar { b: number }\n");
    let contributor_err = merged_memo
        .deref_locator_body(&type_body_locator(
            "Bar",
            vec![
                TypeBodyPathStep::MergedContributor { ordinal: 0 },
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
            ],
        ))
        .expect_err("a contributor-prefixed bound step must fail closed");
    assert_eq!(
        contributor_err,
        LocatorBodyDerefError::TypeParamBoundStepMisplaced
    );
}

/// A bound step on a VALUE-space anchor is misplaced: function / value-decl
/// type parameters live on signatures, not on a value annotation position.
#[test]
fn locator_deref_value_space_type_param_bound_fails_closed() {
    let (memo, _) = memo_for("const val: number = 1;\ntype Other = { o: 1 };\n");
    let err = memo
        .deref_locator_body(&value_body_locator(
            "val",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect_err("a value-space bound step must fail closed");
    assert_eq!(err, LocatorBodyDerefError::TypeParamBoundStepMisplaced);
}

/// After selecting a bound slot, the REMAINING path navigates over the selected
/// bound expression — an object constraint's member value is reachable.
#[test]
fn locator_deref_descends_into_selected_type_param_bound() {
    let (memo, _) = memo_for("type Foo<T extends { a: string }> = { x: T };\n");
    let derefed = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        ))
        .expect("descent into the constraint object's member value must resolve");
    assert!(
        matches!(
            single(&derefed),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the constraint object's member `a` value is `string`; got {:?}",
        derefed.shape
    );
    // The env is the FULL sibling list even at ordinal 0 — an ordinal-0
    // CONSTRAINT may reference the parameter itself (F-bounded), so the
    // list is `[T]` with the position-exact visibility, never an empty
    // prior-sibling prefix.
    assert_eq!(derefed.type_parameters.len(), 1);
    assert_eq!(derefed.type_parameters[0].name, "T");
    assert_eq!(
        derefed.visibility,
        TypeParamVisibility::Constraint { ordinal: 0 }
    );
}

// ===========================================================================
// Up-front structural misplacement validation.
//
// `TypeParamBound` placement is a SYNTACTIC invariant, validated over the WHOLE
// path BEFORE any body demand — so the distinct `TypeParamBoundStepMisplaced`
// error holds even when an earlier path step would ITSELF fail to resolve.
// ===========================================================================

/// A non-leading `TypeParamBound` behind an earlier step that ALSO does not
/// resolve must still fail closed with `TypeParamBoundStepMisplaced` — the
/// syntactic misplacement is decided up front, never swallowed as the generic
/// `PathUnresolved` a mid-body navigation would return.
///
/// Discrimination: `[Member{99}, TypeParamBound{..}]` on a body with no member
/// 99. WITHOUT the up-front pass the deref demands the body and navigates
/// `Member{99}` first, returning `PathUnresolved` before the misplaced bound is
/// ever reached; WITH it the misplaced bound is rejected first.
#[test]
fn deref_up_front_rejects_misplaced_bound_before_earlier_step_resolves() {
    let (memo, _) = memo_for("type Foo<T extends string> = { a: string };\n");
    let err = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![
                TypeBodyPathStep::Member { ordinal: 99 },
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
            ],
        ))
        .expect_err("a misplaced bound behind an unresolvable step must fail closed");
    assert_eq!(
        err,
        LocatorBodyDerefError::TypeParamBoundStepMisplaced,
        "the misplaced bound must be rejected UP FRONT — never swallowed as \
         PathUnresolved because the earlier `Member{{99}}` step also misses"
    );
}

/// A `TypeParamBound` anywhere in a VALUE-space path is misplaced — value /
/// function type parameters live on the signature, not on a value annotation
/// position — and the whole path is validated up front.
#[test]
fn deref_value_space_non_leading_bound_is_misplaced() {
    let (memo, _) = memo_for("const val: { a: string } = { a: '' };\ntype Other = { o: 1 };\n");
    let err = memo
        .deref_locator_body(&value_body_locator(
            "val",
            vec![
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
            ],
        ))
        .expect_err("a value-space bound step must fail closed");
    assert_eq!(err, LocatorBodyDerefError::TypeParamBoundStepMisplaced);
}

/// A leading `TypeParamBound` on a NAMESPACE-space anchor is misplaced — a
/// namespace has no header type-parameter axis — decided up front before the
/// namespace-unrouted path.
#[test]
fn deref_namespace_space_leading_bound_is_misplaced() {
    let (memo, _) = memo_for("namespace N { export type T = { a: 1 } }\n");
    let err = memo
        .deref_locator_body(&namespace_body_locator(
            "N",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect_err("a namespace-space bound step must fail closed");
    assert_eq!(err, LocatorBodyDerefError::TypeParamBoundStepMisplaced);
}

// ===========================================================================
// Anchor-canonical coherence: a locator must deref through the memo of its OWN
// producing canonical.
// ===========================================================================

/// Derefing a locator anchored at one canonical THROUGH a different memo fails
/// closed with the typed `CanonicalMismatch` — and materialises no body cell.
///
/// Discrimination: both memos declare the same-named symbol. WITHOUT the
/// top-level anchor check the wrong-memo deref demands and returns the OTHER
/// memo's body (a wrong result); WITH it the mismatch is rejected before any
/// demand.
#[test]
fn wrong_memo_deref_returns_canonical_mismatch() {
    let (memo_a, _) = memo_for_canonical("/ws/a.ts", "type Shared = { a: 1 };\n");
    let (memo_b, _) = memo_for_canonical("/ws/b.ts", "type Shared = { b: 2 };\n");
    // Control: A resolves its own `Shared`.
    assert!(
        memo_a.type_decl("Shared").is_some(),
        "memo A must declare the shared symbol"
    );

    // A locator anchored at A's canonical, derefed through B.
    let locator = type_body_locator_on("/ws/a.ts", "Shared", Vec::new());
    let err = memo_b
        .deref_locator_body(&locator)
        .expect_err("a cross-memo deref must fail typed, never return B's body");
    assert_eq!(
        err,
        LocatorBodyDerefError::CanonicalMismatch,
        "a locator anchored at A's canonical must not deref through B"
    );
    assert!(
        !memo_b.type_entry_materialized("Shared"),
        "the mismatched deref must materialise no body cell"
    );
}

// ===========================================================================
// Augmentation-scoped type-param bounds — addressable through the SAME path
// vocabulary as top-level type decls (`declare module` + `declare global`).
// ===========================================================================

fn module_scope(specifier: &str) -> AuthoredAugmentationScope {
    AuthoredAugmentationScope::Module {
        specifier: Arc::from(specifier),
    }
}

/// An augmentation-scoped `interface`'s type-param constraint derefs to the
/// EXACT authored constraint body through the shared type-space navigator.
#[test]
fn aug_module_type_param_constraint_reached() {
    let source = "declare module \"vue\" {\n  interface Foo<T extends string> { x: T }\n}\n";
    let (memo, _) = memo_for(source);
    let derefed = memo
        .deref_locator_body(&aug_type_locator(
            "Foo",
            module_scope("vue"),
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("the augmentation interface's ordinal-0 constraint must deref");
    assert!(
        matches!(
            single(&derefed),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the augmentation type-param constraint is `string`; got {:?}",
        derefed.shape
    );
}

/// The constraint and default slots of an augmentation-scoped parameter are
/// DISTINCT authored bodies; `position` selects between them.
#[test]
fn aug_module_type_param_default_distinct_from_constraint() {
    let source =
        "declare module \"vue\" {\n  interface Foo<T extends string = number> { x: T }\n}\n";
    let (memo, _) = memo_for(source);
    let constraint = memo
        .deref_locator_body(&aug_type_locator(
            "Foo",
            module_scope("vue"),
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("constraint derefs");
    let default = memo
        .deref_locator_body(&aug_type_locator(
            "Foo",
            module_scope("vue"),
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Default,
            }],
        ))
        .expect("default derefs");
    assert!(
        matches!(
            single(&constraint),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "constraint is `string`; got {:?}",
        constraint.shape
    );
    assert!(
        matches!(
            single(&default),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "default is `number`; got {:?}",
        default.shape
    );
    assert_ne!(single(&constraint), single(&default));
}

/// An augmentation-scoped constraint bound's deref env is the FULL sibling
/// list plus the position-exact visibility — the same TS lexical view as a
/// top-level decl header (one shared navigator, no augmentation fork).
#[test]
fn aug_module_type_param_constraint_env_is_full_sibling_list() {
    let source =
        "declare module \"vue\" {\n  interface Foo<T, U extends keyof T> { x: T; y: U }\n}\n";
    let (memo, _) = memo_for(source);
    let derefed = memo
        .deref_locator_body(&aug_type_locator(
            "Foo",
            module_scope("vue"),
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 1,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("U's constraint `keyof T` derefs");
    assert!(
        matches!(single(&derefed), TypeExpr::KeyOf(_)),
        "U's constraint is `keyof T`; got {:?}",
        derefed.shape
    );
    let names: Vec<&str> = derefed
        .type_parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["T", "U"],
        "the augmentation bound env carries the FULL sibling list, exactly as \
         a top-level decl header's bound does"
    );
    assert_eq!(
        derefed.visibility,
        TypeParamVisibility::Constraint { ordinal: 1 }
    );
}

/// A DEFAULT bound's deref env carries the same full sibling list but the
/// `Default` visibility — the session-phase frame constructor renders prior
/// siblings usable and self / later siblings shadow-forbidden from it. A
/// deref that collapsed the two positions onto one visibility could not
/// express TS's asymmetric constraint/default scoping.
#[test]
fn locator_deref_default_env_carries_default_visibility() {
    let (memo, _) = memo_for("type Foo<T, U = T> = { x: T; y: U };\n");
    let derefed = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 1,
                position: TypeParamBoundPosition::Default,
            }],
        ))
        .expect("U's default `T` derefs");
    assert!(
        matches!(single(&derefed), TypeExpr::Ref { name, .. } if name.as_ref() == "T"),
        "U's default is the authored `T` reference; got {:?}",
        derefed.shape
    );
    let names: Vec<&str> = derefed
        .type_parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["T", "U"]);
    assert_eq!(
        derefed.visibility,
        TypeParamVisibility::Default { ordinal: 1 },
        "a default bound derefs with the DISTINCT Default visibility — never \
         the constraint view"
    );
}

/// After selecting an augmentation-scoped bound, the REMAINING path navigates
/// over the selected bound expression.
#[test]
fn aug_module_type_param_post_bound_descent() {
    let source = "declare module \"vue\" {\n  interface Foo<T extends { a: string }> { x: T }\n}\n";
    let (memo, _) = memo_for(source);
    let derefed = memo
        .deref_locator_body(&aug_type_locator(
            "Foo",
            module_scope("vue"),
            vec![
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        ))
        .expect("descent into the augmentation constraint object's member value must resolve");
    assert!(
        matches!(
            single(&derefed),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the constraint object's member `a` value is `string`; got {:?}",
        derefed.shape
    );
}

/// An EMPTY-path augmentation locator still derefs the WHOLE contribution body
/// unchanged — for `declare module` AND `declare global`.
#[test]
fn aug_empty_path_derefs_whole_body_compat() {
    let source = "declare module \"vue\" {\n  interface Mod { m: string }\n}\n\
                  declare global {\n  interface Glob { g: number }\n}\n";
    let (memo, _) = memo_for(source);

    let module_body = memo
        .deref_locator_body(&aug_type_locator("Mod", module_scope("vue"), Vec::new()))
        .expect("the whole module-augmentation body derefs");
    assert!(
        matches!(
            module_body.shape,
            DerefedBodyShape::Single(TypeExpr::Object(_))
        ),
        "the whole module-augmentation body is the authored object; got {:?}",
        module_body.shape
    );

    let global_body = memo
        .deref_locator_body(&aug_type_locator(
            "Glob",
            AuthoredAugmentationScope::Global,
            Vec::new(),
        ))
        .expect("the whole global-augmentation body derefs");
    assert!(
        matches!(
            global_body.shape,
            DerefedBodyShape::Single(TypeExpr::Object(_))
        ),
        "the whole global-augmentation body is the authored object; got {:?}",
        global_body.shape
    );
}

/// A `declare global` augmentation-scoped `interface`'s type-param constraint
/// derefs through the SAME shared navigator.
#[test]
fn aug_global_type_param_constraint_reached() {
    let source = "declare global {\n  interface G<T extends number> { x: T }\n}\n";
    let (memo, _) = memo_for(source);
    let derefed = memo
        .deref_locator_body(&aug_type_locator(
            "G",
            AuthoredAugmentationScope::Global,
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("the global augmentation interface's ordinal-0 constraint must deref");
    assert!(
        matches!(
            single(&derefed),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the global augmentation type-param constraint is `number`; got {:?}",
        derefed.shape
    );
}

/// A mid-path `TypeParamBound` on an augmentation locator is misplaced —
/// caught by the SAME up-front pass as the top-level decl path.
#[test]
fn aug_misplaced_non_leading_bound_fails_closed() {
    let source = "declare module \"vue\" {\n  interface Foo<T> { a: string }\n}\n";
    let (memo, _) = memo_for(source);
    let err = memo
        .deref_locator_body(&aug_type_locator(
            "Foo",
            module_scope("vue"),
            vec![
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::TypeParamBound {
                    ordinal: 0,
                    position: TypeParamBoundPosition::Constraint,
                },
            ],
        ))
        .expect_err("a mid-path augmentation bound step must fail closed");
    assert_eq!(err, LocatorBodyDerefError::TypeParamBoundStepMisplaced);
}

// ===========================================================================
// Symmetric bound-slot coverage.
// ===========================================================================

/// The DEFAULT slot of a parameter with a constraint but no default fails
/// closed with the typed absent-bound error (the Default-position analogue of
/// the constraint-absent case).
#[test]
fn deref_type_param_default_absent_fails_closed() {
    let (memo, _) = memo_for("type Foo<T extends string> = { x: T };\n");
    let err = memo
        .deref_locator_body(&type_body_locator(
            "Foo",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Default,
            }],
        ))
        .expect_err("a param with no default must fail closed on the Default slot");
    assert_eq!(
        err,
        LocatorBodyDerefError::TypeParamBoundAbsent {
            ordinal: 0,
            position: TypeParamBoundPosition::Default,
        }
    );
}

/// A merged same-name `interface` with type parameters exposes the ordinal-0
/// constraint over the merged group through the leading-bound path.
#[test]
fn merged_interface_with_type_params_leading_bound() {
    let (memo, _) =
        memo_for("interface Bar<T extends string> { a: T }\ninterface Bar<T> { b: number }\n");
    // Control: `Bar` is a merged decl.
    let decl = memo.type_decl("Bar").expect("Bar exists");
    assert!(decl.body.is_merged(), "two same-name interfaces merge");

    let derefed = memo
        .deref_locator_body(&type_body_locator(
            "Bar",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect("the merged group's ordinal-0 constraint must deref");
    assert!(
        matches!(
            single(&derefed),
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the merged group's ordinal-0 constraint is `string`; got {:?}",
        derefed.shape
    );
}

/// A leading `TypeParamBound` on a NON-annotated value fails closed with
/// `TypeParamBoundStepMisplaced` — the up-front placement check fires BEFORE
/// the annotation-absent path, so a bound on a value never surfaces as
/// `ValueAnnotationAbsent`.
#[test]
fn non_annotated_value_leading_bound_is_misplaced() {
    let (memo, _) = memo_for("declare const val;\ntype Other = { o: 1 };\n");
    let err = memo
        .deref_locator_body(&value_body_locator(
            "val",
            vec![TypeBodyPathStep::TypeParamBound {
                ordinal: 0,
                position: TypeParamBoundPosition::Constraint,
            }],
        ))
        .expect_err("a leading bound on a non-annotated value must fail closed");
    assert_eq!(
        err,
        LocatorBodyDerefError::TypeParamBoundStepMisplaced,
        "the up-front placement check must fire before the annotation-absent path"
    );
}
