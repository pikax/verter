//! @ai-generated - slice-plan lowering tests: only selected nodes lower
//! (sibling elision with a recorded count), effect obligations survive
//! for value-dead siblings, slot/def wiring, return accumulation, and
//! arena-freedom witnesses for every IR carrier.

use std::sync::Arc;

use super::*;
use crate::analysis::flow::flow_graph::build_function_flow_graph;
use crate::analysis::flow::flow_ir::{
    FlowCallee, FlowDef, FlowEffect, FlowEffectTarget, FlowExpr, FlowExprId, FlowExprRole,
    FlowExprShape, FlowObjectEntry, FlowObjectKey, FlowPathSegment, FlowRead, FlowReturnEntry,
    FlowSliceIR, FlowSlot, FlowSlotId, ReturnAccumulator,
};
use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
use crate::analysis::flow::{
    build_function_body_skeleton, FrameSpan, FunctionBodySkeleton, FunctionBodySource,
    SkeletonWriteCertainty,
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

fn lowered(source: &str, path: &[&str]) -> FlowSliceIR {
    let skeleton = skeleton_of(source);
    let graph = build_function_flow_graph(&skeleton);
    let names: Vec<Arc<str>> = path.iter().map(|name| Arc::from(*name)).collect();
    let demand = SliceDemand::for_return_projection(&skeleton, &names);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan");
    lower_slice_plan(&plan, &graph, &skeleton)
}

fn slot_named<'ir>(ir: &'ir FlowSliceIR, name: &str) -> Option<&'ir FlowSlot> {
    ir.slots.iter().find(|slot| slot.name.as_ref() == name)
}

/// The acceptance case lowers ONLY the demanded member: `b`'s entry and
/// its literal initializer lower; sibling `a` is elided (counted, never
/// lowered), `a` gets no slot, and `new Mytype()` produces no expression
/// record.
#[test]
fn lower_slice_plan_lowers_only_selected_nodes() {
    let ir = lowered(
        "function myType() { const a = new Mytype(); const b = 1; return { a, b } }",
        &["b"],
    );

    // The returned object lowers with exactly the selected `b` entry and
    // one elided sibling.
    let object = ir
        .exprs
        .iter()
        .find_map(|expr| match &expr.shape {
            FlowExprShape::ObjectLiteral {
                entries,
                elided_entries,
            } => Some((entries, *elided_entries)),
            FlowExprShape::Opaque { .. } => None,
        })
        .expect("the returned object literal lowers");
    let (entries, elided) = object;
    assert_eq!(entries.len(), 1, "only the demanded entry lowers");
    assert_eq!(elided, 1, "the unselected sibling is counted, not lowered");
    let FlowObjectEntry::Property { key, value } = &entries[0] else {
        panic!("b lowers as a property entry");
    };
    assert_eq!(key, &FlowObjectKey::Named(Arc::from("b")));
    assert_eq!(ir.expr(*value).role, FlowExprRole::Value);

    // Slots: `b` (with its literal def) — never `a`.
    let b_slot = slot_named(&ir, "b").expect("b lowers to a slot");
    assert!(b_slot.value_selected);
    assert_eq!(b_slot.defs.len(), 1, "b's initializer is its one def");
    assert!(slot_named(&ir, "a").is_none(), "a must not lower");

    // No expression record covers `new Mytype()` — nothing outside the
    // plan lowers.
    let source = "function myType() { const a = new Mytype(); const b = 1; return { a, b } }";
    // The function starts at offset 0 here, so the authored offset IS the
    // frame offset; `FrameSpan::rebase` states that rather than assuming it.
    let mytype_at = FrameSpan::rebase(
        0,
        verter_span::Span::new(
            source.find("new Mytype()").expect("fixture") as u32,
            (source.find("new Mytype()").expect("fixture") + "new Mytype()".len()) as u32,
        ),
    );
    assert!(
        ir.exprs.iter().all(|expr| !expr.span.contains(mytype_at)),
        "the unselected initializer must not produce an expression record"
    );

    // One return contributor carrying the object.
    assert_eq!(ir.returns.sites.len(), 1);
    assert!(ir.returns.sites[0].argument.is_some());
    assert_eq!(
        ir.demanded_path.as_ref(),
        &[FlowPathSegment::Named(Arc::from("b"))]
    );
}

/// The value-dead sibling's evaluation effect survives lowering: `a`'s
/// site lowers EffectOnly, its `x = "s"` write lowers as a Write
/// obligation targeting `x`'s slot with the selected right-hand side,
/// and the demanded `b` expression stays a Value record whose read
/// resolves to the same slot.
#[test]
fn lower_preserves_effect_obligations_for_value_dead_siblings() {
    let ir = lowered(
        r#"function f(x: string) { return { a: (x = "s"), b: x.toUpperCase() } }"#,
        &["b"],
    );

    let x_slot_id = ir
        .slots
        .iter()
        .position(|slot| slot.name.as_ref() == "x")
        .map(|index| FlowSlotId::from_index(index as u32))
        .expect("x lowers to a slot");

    // The write obligation: a selected effect targeting x's slot,
    // definite, with the selected RHS.
    let write = ir
        .effects
        .iter()
        .find_map(|effect| match effect {
            FlowEffect::Write {
                site,
                target,
                certainty,
                value,
                ..
            } => Some((site, target, certainty, value)),
            FlowEffect::Call { .. } => None,
        })
        .expect("the sibling write lowers as an effect obligation");
    let (site, target, certainty, value) = write;
    assert_eq!(target, &FlowEffectTarget::Slot(x_slot_id));
    assert_eq!(*certainty, SkeletonWriteCertainty::Definite);
    assert_eq!(
        ir.expr(*site).role,
        FlowExprRole::EffectOnly,
        "the write's carrier site is the value-dead sibling"
    );
    let rhs = value.expect("the RHS is value-selected through x's defs");
    assert_eq!(ir.expr(rhs).role, FlowExprRole::Value);

    // b's expression is a Value record reading x's slot.
    let b_read = ir
        .exprs
        .iter()
        .find_map(|expr| match &expr.shape {
            FlowExprShape::Opaque { reads } if expr.role == FlowExprRole::Value => reads
                .iter()
                .find(|read| read.name.as_ref() == "x")
                .map(|read| read.slot),
            _ => None,
        })
        .expect("b's expression reads x");
    assert_eq!(b_read, Some(x_slot_id));

    // The call obligation on the demanded expression also lowers.
    assert!(
        ir.effects.iter().any(|effect| matches!(
            effect,
            FlowEffect::Call {
                callee: FlowCallee::Path(_),
                ..
            }
        )),
        "b's `x.toUpperCase()` call lowers as a call obligation"
    );

    // x's slot carries the sibling write as a def (whole-slot definite).
    let x_slot = ir.slot(x_slot_id);
    assert!(x_slot.value_selected);
    assert_eq!(x_slot.defs.len(), 1);
    assert!(x_slot.defs[0].path.is_empty());
}

/// The effect kind sequence of one body, at anchor 0 and at a non-zero
/// anchor.
fn effect_kinds(source: &str) -> Vec<&'static str> {
    lowered(source, &["c"])
        .effects
        .iter()
        .map(|effect| match effect {
            FlowEffect::Call { .. } => "call",
            FlowEffect::Write { .. } => "write",
        })
        .collect()
}

const INTERLEAVED_BODY: &str = "function f(x: number) { return { a: g(x), b: (x = 1), c: x } }";
const PADDED_INTERLEAVED_BODY: &str =
    "const pad = 0;\nfunction f(x: number) { return { a: g(x), b: (x = 1), c: x } }";

/// Effects lower in AUTHORED order — interleaving CALL and WRITE
/// obligations by source position, not by obligation family and not by
/// coordinate system.
///
/// The expected order is derived from the fixture TEXT (`g(x)` is written
/// before `(x = 1)`), never from the spans the lowering sorted on. Its
/// predecessor read `span.start` back off the result and asserted the
/// vector ascended — the same key `lower_slice_plan` had sorted on one
/// line earlier, so it held by construction under any coordinate system,
/// and its single fixture sat at offset 0 where the two systems coincide.
/// It passed while the invariant was violated.
///
/// The discriminating half is the PADDED fixture: one statement above the
/// function, so its anchor is non-zero. A call span left ABSOLUTE inside
/// an otherwise anchor-relative artifact keys larger than every relative
/// write span, and every call sorts after every write regardless of what
/// was authored.
#[test]
fn lower_orders_effects_by_authored_position_at_any_anchor() {
    assert_eq!(
        effect_kinds(INTERLEAVED_BODY),
        ["call", "write"],
        "`g(x)` is authored before `(x = 1)`"
    );
    assert_eq!(
        effect_kinds(PADDED_INTERLEAVED_BODY),
        ["call", "write"],
        "the same body one statement lower is the same body"
    );
}

/// The lowered IR is a pure function of the FUNCTION's content — moving
/// the whole function through the file changes nothing in it.
///
/// This is the property the flow artifacts' cache key already assumes:
/// the IR is memoized per function content version and reused for any
/// file content that key admits, so an absolute offset stored anywhere
/// inside it makes the cached value depend on something the key cannot
/// see. Asserting whole-IR equality covers every span-bearing family at
/// once — expression locators, slot identities, return sites, and both
/// effect families — rather than the five a hand-written rebase pass
/// remembered.
#[test]
fn lowered_ir_is_invariant_under_the_function_position() {
    assert_eq!(
        lowered(INTERLEAVED_BODY, &["c"]),
        lowered(PADDED_INTERLEAVED_BODY, &["c"]),
        "the same function body lowers identically wherever it sits"
    );
}

/// A spread entry selected by the demand lowers as a Spread entry.
#[test]
fn lower_keeps_selected_spread_entries() {
    let ir = lowered(
        "function s(rest: object) { return { b: 1, ...rest } }",
        &["b"],
    );
    let (entries, _) = ir
        .exprs
        .iter()
        .find_map(|expr| match &expr.shape {
            FlowExprShape::ObjectLiteral {
                entries,
                elided_entries,
            } => Some((entries, *elided_entries)),
            FlowExprShape::Opaque { .. } => None,
        })
        .expect("object lowers");
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry, FlowObjectEntry::Spread { .. })),
        "the trailing spread stays a lowered candidate provider"
    );
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry, FlowObjectEntry::Property { .. })),
        "the demanded static entry lowers alongside it"
    );
}

/// Every IR carrier is arena-free and `TypeExpr`-free — instantiated
/// compile-time witnesses, matching the skeleton / graph discipline.
#[test]
fn flow_slice_ir_is_arena_free_send_sync_static() {
    fn assert_arena_free<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {}
    assert_arena_free::<FlowSliceIR>();
    assert_arena_free::<FlowSlot>();
    assert_arena_free::<FlowSlotId>();
    assert_arena_free::<FlowDef>();
    assert_arena_free::<FlowExpr>();
    assert_arena_free::<FlowExprId>();
    assert_arena_free::<FlowExprRole>();
    assert_arena_free::<FlowExprShape>();
    assert_arena_free::<FlowObjectEntry>();
    assert_arena_free::<FlowObjectKey>();
    assert_arena_free::<FlowRead>();
    assert_arena_free::<FlowEffect>();
    assert_arena_free::<FlowEffectTarget>();
    assert_arena_free::<FlowCallee>();
    assert_arena_free::<FlowPathSegment>();
    assert_arena_free::<FlowReturnEntry>();
    assert_arena_free::<ReturnAccumulator>();
}
