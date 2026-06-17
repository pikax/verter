//! `DeclBodyMemo` demand-scoping contract: a demand lowers exactly the
//! demanded symbol's contributing statements (counted on the
//! `decl_bodies_lowered` rail), sibling declarations of the same
//! statements backfill, repeated demands re-lower nothing, distinct
//! demands share ONE retained eval-program parse, and seeding from a
//! built env matches the lazy fold.

use std::sync::Arc;

use super::*;
use crate::decl_lowering::DeclLoweringService;

fn memo_for(source: &str) -> (DeclBodyMemo, Arc<MetaProvenance>) {
    let eval_source: Arc<str> = Arc::from(source);
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse");
    let header_index = Arc::new(
        verter_semantic::analysis::decl_headers::build_decl_header_index(&parsed.program, source),
    );
    let provenance = Arc::new(MetaProvenance::default());
    let memo = DeclBodyMemo::new(
        SnapshotKey {
            canonical: Arc::from("/ws/fixture.ts"),
            whole_hash: [7u8; 16],
            parse_env_hash: [0u8; 16],
        },
        Arc::clone(&eval_source),
        eval_source,
        None,
        oxc_span::SourceType::ts(),
        Arc::new(DeclLoweringService::new()),
        header_index,
        Arc::clone(&provenance),
        // No pre-acquired lease — the memo acquires its own lazily on the
        // first body demand (the cold-index pinning path is exercised by
        // the host integration tests).
        None,
    );
    (memo, provenance)
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
fn dependency_names_ride_on_the_lowered_entry() {
    let (memo, _) = memo_for(
        "import { Ext } from './dep';\ntype WithDeps = { a: Ext; b: Local };\ntype Local = { v: 1 };\n",
    );
    let decl = memo.type_decl("WithDeps").expect("WithDeps exists");
    assert!(decl.dep_names.contains("Ext"));
    assert!(decl.dep_names.contains("Local"));
    assert!(
        !decl.dep_names.contains("WithDeps"),
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
        declared.dep_names.contains("Imported"),
        "the default class's declared-name body must carry its heritage dep; got {:?}",
        declared.dep_names
    );
    let aliased = memo.type_decl("default").expect("default alias");
    assert!(
        aliased.dep_names.contains("Imported"),
        "the `default` alias body must carry its heritage dep; got {:?}",
        aliased.dep_names
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
        decl.dep_names.contains("Imported"),
        "namespaced `N.T` body must carry its dep `Imported`; got {:?}",
        decl.dep_names
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
        decl.dep_names.contains("Imported"),
        "nested `Outer.Inner.T` body must carry its dep `Imported`; got {:?}",
        decl.dep_names
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
        decl.dep_names.contains("Imported"),
        "the JSDoc typedef body must carry its dep `Imported`; got {:?}",
        decl.dep_names
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn concurrent_first_touch_lowers_once() {
    let (memo, provenance) = memo_for(FIVE_DECLS);
    let memo = Arc::new(memo);
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
        .augmentation_type_decl(&scope, "ComponentCustomProperties")
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
        .augmentation_type_decl(&AugmentationScopeKind::Global, "Missing")
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
    for name in header_index.type_headers.keys() {
        assert!(
            memo.type_decl(name).is_some(),
            "type header `{name}` must demand-resolve through type_decl"
        );
    }
    for name in header_index.value_headers.keys() {
        assert!(
            memo.value_decl(name).is_some(),
            "value header `{name}` must demand-resolve through value_decl"
        );
    }
    for (scope, names) in &header_index.augmentation_type_headers {
        for name in names.keys() {
            assert!(
                memo.augmentation_type_decl(scope, name).is_some(),
                "augmentation type header `{name}` in {scope:?} must demand-resolve"
            );
        }
    }
    for (scope, names) in &header_index.augmentation_value_headers {
        for name in names.keys() {
            assert!(
                memo.augmentation_value_decl(scope, name).is_some(),
                "augmentation value header `{name}` in {scope:?} must demand-resolve"
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
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["A", "B", "C", "D"],
        "merged enum value-space members must union all contributors in source order"
    );

    // Type space: the value-derived union, through `type_decl`.
    let ty = memo.type_decl("E").expect("enum type body resolves");
    match ty.body.primary() {
        TypeExpr::Union(types) => {
            assert_eq!(
                types.len(),
                4,
                "type union must carry all 4 member literals"
            );
            for literal in [1.0, 2.0, 3.0, 4.0] {
                assert!(
                    types.contains(&TypeExpr::number_literal(literal)),
                    "type union must contain literal {literal}; got {types:?}"
                );
            }
        }
        other => panic!("merged enum type body must be a 4-arm union, got {other:?}"),
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

#[test]
fn seeded_memo_matches_lazy_fold() {
    let source =
        "interface Merged { a: string }\ninterface Merged { b: number }\nconst v = { k: 1 };\n";
    let env = verter_semantic::analysis::type_eval_build::parse_and_build_env(source);
    let analysis =
        verter_compiler::utils::oxc::script::type_surface::AnalyzedExternalTypeSource::default();
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    let header_index = Arc::new(
        verter_semantic::analysis::decl_headers::build_decl_header_index(&parsed.program, source),
    );
    let seeded = DeclBodyMemo::seeded_from_env(
        SnapshotKey {
            canonical: Arc::from("/ws/seeded.ts"),
            whole_hash: [9u8; 16],
            parse_env_hash: [0u8; 16],
        },
        &env,
        &analysis,
        header_index,
    );

    let merged = seeded.type_decl("Merged").expect("seeded entry");
    assert!(merged.body.is_merged());
    assert_eq!(
        merged.body.merged_member_names(),
        vec!["a".to_string(), "b".to_string()]
    );
    let value = seeded.value_decl("v").expect("seeded value entry");
    assert!(value.object_shape.is_some());
    assert!(seeded.whole_env_materialized(), "seeding pre-sets the env");
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
    let memo = Arc::new(memo);
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
