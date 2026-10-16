//! Compile-fail: a graph payload's nested arrays and member records are
//! borrowed through the read that owns the payload. None of them — tuple
//! elements, signature parameters and type parameters, composite arms,
//! spread-program effects, a recursive reference's arguments, a one-level
//! or shallow surface adapter over a borrowed view, or a shallow member —
//! can be copied or shared away from that owner.

use std::sync::Arc;
use verter_type_engine::project_semantic_dispatch::one_level_surface::OneLevelSurface;
use verter_type_engine::project_semantic_dispatch::walk::{ShallowSurface, ShallowSurfaceMember};
use verter_type_engine::semantic_query::composite::{CompositeList, CompositeMembers, UnionKind};
use verter_type_engine::semantic_query::{
    FunctionParam, ObjectConstructionEffect, ObjectSpreadProgram, PayloadArray, QueryError,
    SemanticNodeId, SurfaceView, TupleElement, TypeParamDecl,
};
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

fn share_an_array(elements: &PayloadArray<TupleElement>) -> PayloadArray<TupleElement> {
    elements.share()
}

fn clone_an_array(args: &PayloadArray<SemanticNodeId>) -> PayloadArray<SemanticNodeId> {
    Clone::clone(args)
}

fn copy_an_element(elements: &PayloadArray<TupleElement>) -> TupleElement {
    Clone::clone(&elements[0])
}

fn copy_a_param(params: &[FunctionParam]) -> FunctionParam {
    Clone::clone(&params[0])
}

fn copy_type_parameter(parameter: &TypeParamDecl) -> TypeParamDecl {
    Clone::clone(parameter)
}

fn copy_arms(arms: &CompositeList<UnionKind>) -> CompositeList<UnionKind> {
    Clone::clone(arms)
}

fn share_arms(arms: &CompositeMembers) -> Arc<[SemanticNodeId]> {
    arms.members_arc()
}

fn copy_program(program: &ObjectSpreadProgram) -> ObjectSpreadProgram {
    Clone::clone(program)
}

fn copy_effect(effect: &ObjectConstructionEffect) -> ObjectConstructionEffect {
    Clone::clone(effect)
}

fn recursive_arguments(error: &QueryError) -> Vec<SemanticNodeId> {
    match error {
        QueryError::RecursiveRef { args, .. } => args.nodes().to_vec(),
        _ => Vec::new(),
    }
}

fn adapt_a_borrowed_view(graph: &SemanticGraphStore, view: &SurfaceView) -> OneLevelSurface {
    OneLevelSurface::from_view(graph, view)
}

fn shallow_copy_of_a_borrowed_view(view: &SurfaceView) -> ShallowSurface {
    ShallowSurface::from_object(view)
}

fn copy_a_shallow_member(member: &ShallowSurfaceMember) -> ShallowSurfaceMember {
    Clone::clone(member)
}

fn main() {}
