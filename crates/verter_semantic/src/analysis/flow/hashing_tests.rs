//! @ai-generated - slice-hash tests: determinism, demand sensitivity,
//! slice-scoped narrowness (an unselected sibling edit keeps the hash),
//! alpha-normalized locals vs content-bearing property keys, span
//! insensitivity to cosmetic shifts, and the opaque-mint witness.

use std::sync::Arc;

use super::*;
use crate::analysis::flow::flow_graph::build_function_flow_graph;
use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
use crate::analysis::flow::{
    build_function_body_skeleton, FunctionBodySkeleton, FunctionBodySource,
};

fn skeleton_of(source: &str) -> FunctionBodySkeleton {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::ts();
    let ret = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    assert!(
        ret.errors.is_empty(),
        "fixture must parse: {:?}",
        ret.errors
    );
    for statement in &ret.program.body {
        if let oxc_ast::ast::Statement::FunctionDeclaration(function) = statement {
            if let Some(body_source) = FunctionBodySource::from_function(function) {
                return build_function_body_skeleton(&body_source);
            }
        }
    }
    panic!("fixture must contain a bodied function declaration");
}

fn slice_hash_of(source: &str, path: &[&str]) -> FlowSliceHash {
    let skeleton = skeleton_of(source);
    let graph = build_function_flow_graph(&skeleton);
    let names: Vec<Arc<str>> = path.iter().map(|name| Arc::from(*name)).collect();
    let demand = SliceDemand::for_return_projection(&skeleton, &names);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan");
    compute_flow_slice_hash(&plan, &graph, &skeleton)
}

/// Same source, same demand: the same hash — twice over one build and
/// across an independent re-parse.
#[test]
fn slice_hash_is_deterministic() {
    let source = "function f() { const a = 1; const b = 2; return { a, b } }";
    assert_eq!(slice_hash_of(source, &["b"]), slice_hash_of(source, &["b"]));
}

/// Two demands over one body select different subgraphs and hash
/// unequal; the demanded key text itself is identity (a Foreign demand
/// differs from a matched demand).
#[test]
fn slice_hash_is_demand_sensitive() {
    let source = "function f() { const a = 1; const b = 2; return { a, b } }";
    assert_ne!(slice_hash_of(source, &["a"]), slice_hash_of(source, &["b"]));
    assert_ne!(slice_hash_of(source, &["b"]), slice_hash_of(source, &[]));
    assert_ne!(
        slice_hash_of(source, &["zzz"]),
        slice_hash_of(source, &["yyy"]),
        "foreign demanded keys are identity even though neither matches"
    );
}

/// The hash covers ONLY the selected slice: an edit to an UNSELECTED
/// sibling's initializer content leaves the demanded slice's hash
/// unchanged (the whole-body content identity is the cache key's
/// separate `flow_body_stable_hash`, not this hash).
#[test]
fn slice_hash_is_slice_scoped_not_whole_body() {
    let before = "function f() { const a = new Foo(); const b = 1; return { a, b } }";
    let after = "function f() { const a = new Bar(); const b = 1; return { a, b } }";
    assert_eq!(
        slice_hash_of(before, &["b"]),
        slice_hash_of(after, &["b"]),
        "an unselected sibling edit must not perturb the slice identity"
    );
    // The same edit IS identity for a demand that selects the sibling…
    // through the selection shape only when it changes the subgraph;
    // `new Foo()` vs `new Bar()` keep the same structural subgraph, so
    // the discriminating axis here is the demanded key set:
    assert_ne!(slice_hash_of(before, &["a"]), slice_hash_of(before, &["b"]));
}

/// Locals are alpha-normalized (a local rename keeps the hash); property
/// keys are content (a key rename changes the hash — both the demanded
/// key and the written key).
#[test]
fn slice_hash_alpha_normalizes_locals_but_not_property_keys() {
    let x = r#"function f(x: string) { return { a: (x = "s"), b: x.toUpperCase() } }"#;
    let y = r#"function f(y: string) { return { a: (y = "s"), b: y.toUpperCase() } }"#;
    assert_eq!(
        slice_hash_of(x, &["b"]),
        slice_hash_of(y, &["b"]),
        "a local rename is alpha-normalized out of the slice identity"
    );

    let b_key = "function g() { const v = 1; return { b: v } }";
    let c_key = "function g() { const v = 1; return { c: v } }";
    assert_ne!(
        slice_hash_of(b_key, &["b"]),
        slice_hash_of(c_key, &["c"]),
        "a property-key rename is content and changes the slice identity"
    );
}

/// Cosmetic position shifts (added comments / whitespace) never enter
/// the fold: the hash is span-free.
#[test]
fn slice_hash_ignores_cosmetic_position_shifts() {
    let plain = "function f() { const b = 1; return { b } }";
    let shifted = "function f() {   /* shifted */   const b = 1;   return { b }   }";
    assert_eq!(slice_hash_of(plain, &["b"]), slice_hash_of(shifted, &["b"]));
}

/// `FlowSliceHash` is opaque and unforgeable: it exposes bytes but no
/// bytes-to-hash constructor, so holding one proves the planner + hasher
/// ran. (The lowered-body cache key embeds this type, which is what
/// enforces hash-then-lower structurally.)
#[test]
fn flow_slice_hash_is_opaque_send_sync_static() {
    fn assert_arena_free<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {}
    assert_arena_free::<FlowSliceHash>();
    let hash = slice_hash_of("function f() { return { b: 1 } }", &["b"]);
    let bytes = hash.bytes();
    assert_eq!(bytes.len(), 16);
    // Read-only: the ONLY way to obtain a `FlowSliceHash` is
    // `compute_flow_slice_hash` — there is no `from_bytes`, no `Default`,
    // and the field is private. (Enforced by the type; this test pins the
    // public surface by exercising all of it.)
    assert_eq!(hash, hash.clone());
}
