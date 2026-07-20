//! `parse_stable_hash` member/type-param header-shape coverage.
//!
//! These tests build the canonical post-parse `IndexedReady` through the
//! REAL declaration-header walk (`build_decl_header_index`) — NOT the
//! env-seeded `from_eval_env` mirror that the `tests/g_misc1`
//! invariance suite uses — so the header inventory carries the actual
//! `MemberHeader` flags (kind / optional / readonly), the
//! `TypeParamHeader` constraint/default-clause presence, and the
//! source-order contributor list. They pin that `parse_stable_hash`
//! folds that full header shape (so a member kind/optional/readonly flip,
//! a type-param arity / constraint change, a contributor split, or an
//! object-literal value-member edit MOVES the hash) while staying
//! invariant under a type-param IDENTIFIER rename and a member's
//! VALUE-type edit (bodies never lower at publish).

use std::any::Any;
use std::sync::Arc;

use oxc_span::SourceType;

use super::compute_parse_stable_hash;
use crate::decl_body_memo::DeclBodyMemo;
use crate::decl_lowering::{DeclLoweringService, SnapshotKey};
use crate::project_type_store::IndexedReady;
use crate::resolver_core::shallow_file_state::{ShallowFileState, ShallowImportResolver};
use crate::types::MetaProvenance;

/// A no-op import resolver — these fixtures declare no cross-file edges.
struct NoopResolver;
impl ShallowImportResolver for NoopResolver {
    fn resolve_canonical(&self, _specifier: &str) -> Option<String> {
        None
    }
}

/// Build the canonical `IndexedReady` for `source` through the REAL
/// header walk, so the shallow inventory carries real member/type-param
/// header flags + contributor lists (unlike the env-seeded mirror).
fn indexed_for(source: &str) -> Arc<IndexedReady> {
    let eval_source: Arc<str> = Arc::from(source);
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!parsed.panicked, "fixture must parse: {source}");
    let shallow_index = verter_semantic::analysis::script_shallow_index::build_script_shallow_index(
        &parsed.program,
        source,
    );
    let header_index = Arc::new(shallow_index.declaration_headers);
    let route_inventory = Arc::new(shallow_index.routes);
    let memo = DeclBodyMemo::new(
        SnapshotKey {
            canonical: Arc::from("/ws/fixture.ts"),
            whole_hash: [7u8; 16],
            parse_env_hash: [0u8; 16],
        },
        Arc::clone(&eval_source),
        None,
        SourceType::ts(),
        Arc::new(
            verter_semantic::analysis::TopLevelOwnerTable::ordinary_file(parsed.program.body.len()),
        ),
        false,
        Arc::new(DeclLoweringService::new()),
        header_index,
        Arc::new(MetaProvenance::default()),
        None,
    );
    let shallow = ShallowFileState::from_route_inventory_with_resolver(
        [7u8; 16],
        route_inventory,
        Arc::new(memo),
        &NoopResolver,
    );
    Arc::new(IndexedReady::new_for_test_with_state(
        [7u8; 16],
        Arc::new(shallow),
        Arc::clone(&eval_source),
        eval_source,
    ))
}

fn h(source: &str) -> [u8; 16] {
    compute_parse_stable_hash(&indexed_for(source))
}

#[derive(Debug)]
struct FixtureCarrier;

impl verter_language::CarrierParse for FixtureCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }

    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

fn h_with_script_region(
    source: &str,
    kind: verter_language::ScriptRegionKind,
    source_type: verter_language::ScriptSourceType,
) -> [u8; 16] {
    let mut indexed = Arc::try_unwrap(indexed_for(source)).expect("fixture has one owner");
    indexed.framework_parse = Some(Arc::new(verter_language::FrameworkParseArtifact::new(
        verter_language::FrameworkAdapterId::vue(),
        verter_language::LanguageId::new("vue"),
        1,
        verter_language::FrameworkParseCommon {
            script_regions: vec![verter_language::ScriptRegion {
                span: verter_span::Span::new(0, source.len() as u32),
                source_type,
                kind,
            }],
            ..Default::default()
        },
        Arc::new(FixtureCarrier),
    )));
    compute_parse_stable_hash(&indexed)
}

#[test]
fn setup_attribute_only_owner_change_moves_hash() {
    let source = "const value = 1;\n";
    let companion = h_with_script_region(
        source,
        verter_language::ScriptRegionKind::Module,
        verter_language::ScriptSourceType::Ts,
    );
    let setup = h_with_script_region(
        source,
        verter_language::ScriptRegionKind::Instance,
        verter_language::ScriptSourceType::Ts,
    );

    assert_ne!(
        companion, setup,
        "changing only the typed script owner (<script> <-> <script setup>) must move the hash"
    );
}

#[test]
fn carrier_script_span_offsets_do_not_move_hash() {
    let source = "const value = 1;\n";
    let mut first = Arc::try_unwrap(indexed_for(source)).expect("fixture has one owner");
    first.framework_parse = Some(Arc::new(verter_language::FrameworkParseArtifact::new(
        verter_language::FrameworkAdapterId::vue(),
        verter_language::LanguageId::new("vue"),
        1,
        verter_language::FrameworkParseCommon {
            script_regions: vec![verter_language::ScriptRegion {
                span: verter_span::Span::new(0, 1),
                source_type: verter_language::ScriptSourceType::Ts,
                kind: verter_language::ScriptRegionKind::Instance,
            }],
            ..Default::default()
        },
        Arc::new(FixtureCarrier),
    )));
    let mut second = first.clone();
    let parse = second
        .framework_parse
        .take()
        .expect("fixture has framework parse");
    let mut common = parse.common.clone();
    common.script_regions[0].span = verter_span::Span::new(8, 9);
    second.framework_parse = Some(Arc::new(verter_language::FrameworkParseArtifact::new(
        verter_language::FrameworkAdapterId::vue(),
        verter_language::LanguageId::new("vue"),
        1,
        common,
        Arc::new(FixtureCarrier),
    )));

    assert_eq!(
        compute_parse_stable_hash(&first),
        compute_parse_stable_hash(&second),
        "carrier byte-offset movement is cosmetic"
    );
}

// ── Member header shape (kind / optional / readonly) ──────────────────

#[test]
fn member_property_to_method_kind_flip_moves_hash() {
    // `foo: () => void` (property) vs `foo(): void` (method): same member
    // NAME, different MemberHeaderKind. The old name-only hash could not
    // tell them apart.
    let prop = h("export interface Foo { foo: () => void }\n");
    let method = h("export interface Foo { foo(): void }\n");
    assert_ne!(
        prop, method,
        "a member kind flip (property <-> method) MUST move parse_stable_hash"
    );
}

#[test]
fn member_optional_flip_moves_hash() {
    let required = h("export interface Foo { x: string }\n");
    let optional = h("export interface Foo { x?: string }\n");
    assert_ne!(
        required, optional,
        "a member optional flip (x: T <-> x?: T) MUST move parse_stable_hash"
    );
}

#[test]
fn member_readonly_flip_moves_hash() {
    let mutable = h("export interface Foo { x: string }\n");
    let readonly = h("export interface Foo { readonly x: string }\n");
    assert_ne!(
        mutable, readonly,
        "a member readonly flip MUST move parse_stable_hash"
    );
}

// ── Type-parameter shape (arity / constraint / default presence) ──────

#[test]
fn type_param_arity_change_moves_hash() {
    let one = h("export type F<A> = A[]\n");
    let two = h("export type F<A, B> = [A, B]\n");
    assert_ne!(
        one, two,
        "a type-param arity change (F<A> <-> F<A,B>) MUST move parse_stable_hash"
    );
}

#[test]
fn type_param_constraint_add_moves_hash() {
    let unconstrained = h("export type F<A> = A[]\n");
    let constrained = h("export type F<A extends string> = A[]\n");
    assert_ne!(
        unconstrained, constrained,
        "adding a type-param constraint MUST move parse_stable_hash"
    );
}

#[test]
fn type_param_default_add_moves_hash() {
    let no_default = h("export type F<A> = A[]\n");
    let with_default = h("export type F<A = string> = A[]\n");
    assert_ne!(
        no_default, with_default,
        "adding a type-param default MUST move parse_stable_hash"
    );
}

// ── Contributor count (same-name decl split / merge) ──────────────────

#[test]
fn type_contributor_count_change_moves_hash() {
    // One `interface I { a }` vs two same-name `interface I { a }` +
    // `interface I {}`. The unioned member set is identical ({a}); only
    // the CONTRIBUTOR COUNT differs (1 vs 2) — the old member-name-only
    // hash could not see the split.
    let single = h("export interface I { a: string }\n");
    let split = h("export interface I { a: string }\nexport interface I {}\n");
    assert_ne!(
        single, split,
        "a same-name decl split (1 -> 2 contributors, identical member set) MUST move \
         parse_stable_hash"
    );
}

// ── Value object-literal member shape ─────────────────────────────────

#[test]
fn value_object_member_rename_moves_hash() {
    // The old hash folded only `(kind, name)` for value symbols, ignoring
    // object-literal members entirely. A member rename leaves the value
    // symbol `(const, x)` identical, so the old hash did not move.
    let a = h("export const x = { foo: 1 }\n");
    let b = h("export const x = { bar: 1 }\n");
    assert_ne!(
        a, b,
        "an object-literal value member rename MUST move parse_stable_hash"
    );
}

#[test]
fn value_object_member_add_moves_hash() {
    let one = h("export const x = { foo: 1 }\n");
    let two = h("export const x = { foo: 1, bar: 2 }\n");
    assert_ne!(
        one, two,
        "adding an object-literal value member MUST move parse_stable_hash"
    );
}

// ── Negative: alpha-invariance + body-invariance ──────────────────────

#[test]
fn type_param_identifier_rename_is_invariant() {
    // `T` <-> `U` is a cosmetic alpha-rename: arity, constraint/default
    // presence, member shape all identical. The hash MUST NOT move (we
    // fold arity + clause presence, never the parameter IDENTIFIER).
    let t = h("export type F<T> = T[]\n");
    let u = h("export type F<U> = U[]\n");
    assert_eq!(
        t, u,
        "a type-param IDENTIFIER rename (T <-> U) MUST stay invariant"
    );
}

#[test]
fn member_value_type_edit_is_invariant() {
    // `x: string` vs `x: number`: the member's VALUE type is body data
    // (lowered on demand), not header shape. The skeleton hash MUST stay
    // invariant — body sensitivity rides the `FileWholeHash` rail, not
    // this hash.
    let s = h("export interface Foo { x: string }\n");
    let n = h("export interface Foo { x: number }\n");
    assert_eq!(
        s, n,
        "a member VALUE-type edit (x: string <-> x: number) MUST stay invariant — \
         parse_stable_hash never folds member bodies"
    );
}

#[test]
fn whitespace_and_comment_edits_are_invariant() {
    let plain = h("export interface Foo { x: string }\n");
    let spaced = h("export   interface   Foo   {   x:   string   }\n");
    let commented = h("// leading\nexport interface Foo {\n  /* mid */ x: string\n}\n");
    assert_eq!(plain, spaced, "whitespace edits MUST stay invariant");
    assert_eq!(plain, commented, "comment edits MUST stay invariant");
}
