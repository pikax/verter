//! Tri-state assignability and unification engine.
//!
//! Replaces scattered boolean assignability helpers with one relation engine
//! that produces `Assignable | NotAssignable | Unknown`.
//!
//! Initial variance model:
//! - readonly arrays/tuples: covariant
//! - function return types: covariant
//! - mutable containers: invariant (unless builtin rule says otherwise)
//! - method/function parameters: bivariant (initial pass, matches TS declaration files)

use super::arena::{
    CallSignatureNode, FunctionNode, Node, NodeId, ObjectNode, PrimitiveKind, QueryArena,
    SolverCaches, SolverLiteral, TupleNodeElement,
};
use super::result::{RelationMode, RelationResult};
use super::substitution::InferBindings;

// ---------------------------------------------------------------------------
// Operational guards
// ---------------------------------------------------------------------------

/// Operational limits for the relation engine (generous TypeScript-like ceilings).
#[derive(Debug, Clone)]
pub struct RelationLimits {
    /// Maximum recursion depth for nested relation checks.
    pub max_depth: u32,
    /// Maximum total relation steps (comparisons) per query.
    pub max_steps: u64,
}

impl Default for RelationLimits {
    fn default() -> Self {
        Self {
            max_depth: 100,
            max_steps: 1_000_000,
        }
    }
}

/// Mutable state during a relation check.
pub struct RelationState {
    pub depth: u32,
    pub steps: u64,
    pub limits: RelationLimits,
    /// Active `infer` bindings (populated during conditional type resolution).
    pub infer_bindings: Option<InferBindings>,
}

impl RelationState {
    pub fn new(limits: RelationLimits) -> Self {
        Self {
            depth: 0,
            steps: 0,
            limits,
            infer_bindings: None,
        }
    }

    pub fn is_exceeded(&self) -> bool {
        self.depth > self.limits.max_depth || self.steps > self.limits.max_steps
    }

    pub fn step(&mut self) -> Option<RelationResult> {
        self.steps += 1;
        if self.is_exceeded() {
            Some(RelationResult::Unknown)
        } else {
            None
        }
    }

    pub fn enter_depth(&mut self) -> Option<RelationResult> {
        self.depth += 1;
        if self.is_exceeded() {
            Some(RelationResult::Unknown)
        } else {
            None
        }
    }

    pub fn exit_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    pub fn begin_infer(&mut self) {
        self.infer_bindings = Some(InferBindings::new());
    }

    pub fn take_infer_bindings(&mut self) -> Option<InferBindings> {
        self.infer_bindings.take()
    }
}

// ---------------------------------------------------------------------------
// Core relation entry point
// ---------------------------------------------------------------------------

/// Check whether `source` is assignable to `target`.
///
/// Takes `&QueryArena` (immutable node store) and `&mut SolverCaches`
/// (mutable memo tables) separately so nodes can be read by reference
/// without cloning.
pub fn relate(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: NodeId,
    target: NodeId,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    if let Some(cached) = caches.get_relation(source, target, mode) {
        return cached;
    }

    if let Some(bail) = state.step() {
        return bail;
    }

    let result = relate_inner(arena, caches, source, target, mode, state);
    caches.set_relation(source, target, mode, result);
    result
}

/// Inner relation logic — reads nodes by `&` reference, never clones.
fn relate_inner(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: NodeId,
    target: NodeId,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    if source == target {
        return RelationResult::Assignable;
    }

    // Top/bottom — read by reference, zero allocation.
    match (arena.get(source), arena.get(target)) {
        (Node::Primitive(PrimitiveKind::Never), _) => return RelationResult::Assignable,
        (_, Node::Primitive(PrimitiveKind::Unknown)) => return RelationResult::Assignable,
        (Node::Primitive(PrimitiveKind::Any), _) => return RelationResult::Assignable,
        (_, Node::Primitive(PrimitiveKind::Any)) => return RelationResult::Assignable,
        (_, Node::Primitive(PrimitiveKind::Never)) => return RelationResult::NotAssignable,
        _ => {}
    }

    // Structural dispatch.
    //
    // For compound types (union, intersection, object, function, tuple) we
    // need child NodeIds/data to recurse. We extract the necessary Copy/cheap
    // data here, *then* drop the borrow on `arena` before calling `relate`
    // recursively (which only needs `&QueryArena`, so this is actually fine
    // since `relate` takes a shared reference).
    //
    // Because `arena` is `&QueryArena` (shared), multiple concurrent reads
    // are fine.  Only `caches` is `&mut`, and we never hold a reference into
    // it across a recursive call.

    match (arena.get(source), arena.get(target)) {
        // -- Primitives / literals (Copy data) --
        (Node::Primitive(s), Node::Primitive(t)) => relate_primitives(*s, *t),

        (Node::Literal(lit), Node::Primitive(prim)) => relate_literal_to_primitive(lit, *prim),

        (Node::Literal(s), Node::Literal(t)) => {
            if s == t {
                RelationResult::Assignable
            } else {
                RelationResult::NotAssignable
            }
        }

        // -- Union source: every member must be assignable to target --
        (Node::Union(members), _) => {
            let members = members.clone(); // Vec<NodeId> = Vec<u32>, cheap
            if let Some(bail) = state.enter_depth() {
                return bail;
            }
            let result = members
                .iter()
                .map(|&m| relate(arena, caches, m, target, mode, state))
                .fold(RelationResult::Assignable, RelationResult::and);
            state.exit_depth();
            result
        }

        // -- Union target: source assignable to at least one member --
        (_, Node::Union(members)) => {
            let members = members.clone();
            if let Some(bail) = state.enter_depth() {
                return bail;
            }
            let result = members
                .iter()
                .map(|&m| relate(arena, caches, source, m, mode, state))
                .fold(RelationResult::NotAssignable, RelationResult::or);
            state.exit_depth();
            result
        }

        // -- Intersection source --
        (Node::Intersection(members), _) => {
            let members = members.clone();
            if let Some(bail) = state.enter_depth() {
                return bail;
            }
            let result = members
                .iter()
                .map(|&m| relate(arena, caches, m, target, mode, state))
                .fold(RelationResult::NotAssignable, RelationResult::or);
            state.exit_depth();
            result
        }

        // -- Intersection target --
        (_, Node::Intersection(members)) => {
            let members = members.clone();
            if let Some(bail) = state.enter_depth() {
                return bail;
            }
            let result = members
                .iter()
                .map(|&m| relate(arena, caches, source, m, mode, state))
                .fold(RelationResult::Assignable, RelationResult::and);
            state.exit_depth();
            result
        }

        // -- Object structural comparison --
        (Node::Object(s_obj), Node::Object(t_obj)) => {
            relate_objects(arena, caches, s_obj, t_obj, mode, state)
        }

        // -- Function comparison --
        (Node::Function(s_fn), Node::Function(t_fn)) => {
            relate_functions(arena, caches, s_fn, t_fn, mode, state)
        }

        // -- Array --
        (
            Node::Array {
                element: s_el,
                readonly: s_ro,
            },
            Node::Array {
                element: t_el,
                readonly: t_ro,
            },
        ) => {
            let (s_el, s_ro, t_el, t_ro) = (*s_el, *s_ro, *t_el, *t_ro);
            if !t_ro && s_ro {
                return RelationResult::NotAssignable;
            }
            if let Some(bail) = state.enter_depth() {
                return bail;
            }
            let result = if t_ro || s_ro {
                relate(arena, caches, s_el, t_el, mode, state)
            } else {
                relate(arena, caches, s_el, t_el, mode, state)
                    .and(relate(arena, caches, t_el, s_el, mode, state))
            };
            state.exit_depth();
            result
        }

        // -- Tuple --
        (
            Node::Tuple {
                elements: s_els,
                readonly: s_ro,
            },
            Node::Tuple {
                elements: t_els,
                readonly: t_ro,
            },
        ) => {
            let s_ro = *s_ro;
            let t_ro = *t_ro;
            relate_tuples(arena, caches, s_els, t_els, s_ro, t_ro, mode, state)
        }

        // -- Infer --
        (_, Node::Infer { name }) => {
            if let Some(ref mut bindings) = state.infer_bindings {
                bindings.add_candidate(name, source);
                RelationResult::Assignable
            } else {
                RelationResult::Unknown
            }
        }

        // Error nodes
        (Node::Error { .. }, _) | (_, Node::Error { .. }) => RelationResult::Unknown,

        // Unresolved references
        (Node::Ref { .. }, _)
        | (_, Node::Ref { .. })
        | (Node::Applied { .. }, _)
        | (_, Node::Applied { .. }) => RelationResult::Unknown,

        // Unresolved operators
        (Node::KeyOf(_), _)
        | (_, Node::KeyOf(_))
        | (Node::IndexedAccess { .. }, _)
        | (_, Node::IndexedAccess { .. })
        | (Node::Conditional { .. }, _)
        | (_, Node::Conditional { .. })
        | (Node::Mapped { .. }, _)
        | (_, Node::Mapped { .. })
        | (Node::TemplateLiteral { .. }, _)
        | (_, Node::TemplateLiteral { .. }) => RelationResult::Unknown,

        _ => RelationResult::NotAssignable,
    }
}

// ---------------------------------------------------------------------------
// Primitive-to-primitive
// ---------------------------------------------------------------------------

fn relate_primitives(source: PrimitiveKind, target: PrimitiveKind) -> RelationResult {
    if source == target || (source == PrimitiveKind::Undefined && target == PrimitiveKind::Void) {
        RelationResult::Assignable
    } else {
        RelationResult::NotAssignable
    }
}

// ---------------------------------------------------------------------------
// Literal-to-primitive widening
// ---------------------------------------------------------------------------

fn relate_literal_to_primitive(literal: &SolverLiteral, target: PrimitiveKind) -> RelationResult {
    let widens = matches!(
        (literal, target),
        (SolverLiteral::String(_), PrimitiveKind::String)
            | (SolverLiteral::Number(_), PrimitiveKind::Number)
            | (SolverLiteral::Boolean(_), PrimitiveKind::Boolean)
            | (SolverLiteral::BigInt(_), PrimitiveKind::BigInt)
    );
    if widens {
        RelationResult::Assignable
    } else {
        RelationResult::NotAssignable
    }
}

// ---------------------------------------------------------------------------
// Object structural comparison
// ---------------------------------------------------------------------------

fn relate_objects(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: &ObjectNode,
    target: &ObjectNode,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    if let Some(bail) = state.enter_depth() {
        return bail;
    }

    let mut result = RelationResult::Assignable;
    for t_prop in &target.properties {
        if let Some(s_prop) = source.properties.iter().find(|p| p.name == t_prop.name) {
            let prop_result = relate(arena, caches, s_prop.ty, t_prop.ty, mode, state);
            result = result.and(prop_result);
        } else if !t_prop.optional {
            result = result.and(RelationResult::NotAssignable);
        }
    }

    for t_sig in &target.call_signatures {
        let sig_ok = source.call_signatures.iter().any(|s_sig| {
            relate_call_signatures(arena, caches, s_sig, t_sig, mode, state)
                == RelationResult::Assignable
        });
        if !sig_ok {
            result = result.and(RelationResult::NotAssignable);
        }
    }

    state.exit_depth();
    result
}

// ---------------------------------------------------------------------------
// Function comparison
// ---------------------------------------------------------------------------

fn relate_functions(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: &FunctionNode,
    target: &FunctionNode,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    if target.signatures.is_empty() {
        return RelationResult::Assignable;
    }

    if let Some(bail) = state.enter_depth() {
        return bail;
    }

    let mut result = RelationResult::Assignable;
    for t_sig in &target.signatures {
        let sig_ok = source.signatures.iter().any(|s_sig| {
            relate_call_signatures(arena, caches, s_sig, t_sig, mode, state)
                == RelationResult::Assignable
        });
        if !sig_ok {
            result = result.and(RelationResult::NotAssignable);
        }
    }

    state.exit_depth();
    result
}

fn relate_call_signatures(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: &CallSignatureNode,
    target: &CallSignatureNode,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    if target.parameters.len() > source.parameters.len() {
        return RelationResult::NotAssignable;
    }

    let mut result = RelationResult::Assignable;

    // Parameters: intentionally bivariant for the initial implementation to match
    // common declaration-file behavior and reduce false negatives in library types.
    // TODO(Milestone 4): implement proper contravariance for strictFunctionTypes.
    for (s_param, t_param) in source.parameters.iter().zip(target.parameters.iter()) {
        let fwd = relate(arena, caches, s_param.ty, t_param.ty, mode, state);
        let bwd = relate(arena, caches, t_param.ty, s_param.ty, mode, state);
        result = result.and(fwd.or(bwd));
    }

    // Return type: covariant
    result = result.and(relate(
        arena,
        caches,
        source.return_type,
        target.return_type,
        mode,
        state,
    ));

    result
}

// ---------------------------------------------------------------------------
// Tuple comparison
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn relate_tuples(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: &[TupleNodeElement],
    target: &[TupleNodeElement],
    source_readonly: bool,
    target_readonly: bool,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    if !target_readonly && source_readonly {
        return RelationResult::NotAssignable;
    }

    if let Some(bail) = state.enter_depth() {
        return bail;
    }

    let required_target_len = target.iter().filter(|e| !e.optional && !e.rest).count();
    if source.len() < required_target_len {
        state.exit_depth();
        return RelationResult::NotAssignable;
    }

    let mut result = RelationResult::Assignable;
    for (s_el, t_el) in source.iter().zip(target.iter()) {
        let el_result = if target_readonly || source_readonly {
            relate(arena, caches, s_el.ty, t_el.ty, mode, state)
        } else {
            relate(arena, caches, s_el.ty, t_el.ty, mode, state)
                .and(relate(arena, caches, t_el.ty, s_el.ty, mode, state))
        };
        result = result.and(el_result);
    }

    state.exit_depth();
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::arena::*;
    use super::*;

    fn fresh() -> (QueryArena, SolverCaches, RelationState) {
        (
            QueryArena::new(),
            SolverCaches::new(),
            RelationState::new(RelationLimits::default()),
        )
    }

    #[test]
    fn same_node_is_assignable() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, s, s, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn never_assignable_to_everything() {
        let (mut a, mut c, mut st) = fresh();
        let never = a.primitive(PrimitiveKind::Never);
        let s = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, never, s, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn everything_assignable_to_unknown() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        let unk = a.primitive(PrimitiveKind::Unknown);
        assert_eq!(
            relate(&a, &mut c, s, unk, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn nothing_assignable_to_never() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        let never = a.primitive(PrimitiveKind::Never);
        assert_eq!(
            relate(&a, &mut c, s, never, RelationMode::Assignable, &mut st),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn any_is_special() {
        let (mut a, mut c, mut st) = fresh();
        let any = a.primitive(PrimitiveKind::Any);
        let s = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, any, s, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
        assert_eq!(
            relate(&a, &mut c, s, any, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn same_primitive_assignable() {
        let (mut a, mut c, mut st) = fresh();
        let s1 = a.primitive(PrimitiveKind::String);
        let s2 = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, s1, s2, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn different_primitives_not_assignable() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        let n = a.primitive(PrimitiveKind::Number);
        assert_eq!(
            relate(&a, &mut c, s, n, RelationMode::Assignable, &mut st),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn undefined_assignable_to_void() {
        let (mut a, mut c, mut st) = fresh();
        let undef = a.primitive(PrimitiveKind::Undefined);
        let void = a.primitive(PrimitiveKind::Void);
        assert_eq!(
            relate(&a, &mut c, undef, void, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn string_literal_assignable_to_string() {
        let (mut a, mut c, mut st) = fresh();
        let lit = a.string_literal("hello");
        let s = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, lit, s, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn string_literal_not_assignable_to_number() {
        let (mut a, mut c, mut st) = fresh();
        let lit = a.string_literal("hello");
        let n = a.primitive(PrimitiveKind::Number);
        assert_eq!(
            relate(&a, &mut c, lit, n, RelationMode::Assignable, &mut st),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn same_literal_assignable() {
        let (mut a, mut c, mut st) = fresh();
        let x = a.string_literal("hello");
        let y = a.string_literal("hello");
        assert_eq!(
            relate(&a, &mut c, x, y, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn different_literals_not_assignable() {
        let (mut a, mut c, mut st) = fresh();
        let x = a.string_literal("hello");
        let y = a.string_literal("world");
        assert_eq!(
            relate(&a, &mut c, x, y, RelationMode::Assignable, &mut st),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn union_target_succeeds_if_any_member_matches() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        let n = a.primitive(PrimitiveKind::Number);
        let union = a.union(vec![s, n]);
        let src = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, src, union, RelationMode::Assignable, &mut st),
            RelationResult::Assignable
        );
    }

    #[test]
    fn union_source_requires_all_members() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        let n = a.primitive(PrimitiveKind::Number);
        let union = a.union(vec![s, n]);
        let target = a.primitive(PrimitiveKind::String);
        assert_eq!(
            relate(&a, &mut c, union, target, RelationMode::Assignable, &mut st),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn object_structural_match() {
        let (mut a, mut c, mut st) = fresh();
        let str_ty = a.primitive(PrimitiveKind::String);
        let num_ty = a.primitive(PrimitiveKind::Number);

        let source = a.object(ObjectNode {
            properties: vec![
                PropertyNode {
                    name: "x".into(),
                    ty: str_ty,
                    optional: false,
                    readonly: false,
                    is_method: false,
                },
                PropertyNode {
                    name: "y".into(),
                    ty: num_ty,
                    optional: false,
                    readonly: false,
                    is_method: false,
                },
            ],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let target = a.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "x".into(),
                ty: str_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        assert_eq!(
            relate(
                &a,
                &mut c,
                source,
                target,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::Assignable
        );
    }

    #[test]
    fn object_missing_required_property() {
        let (mut a, mut c, mut st) = fresh();
        let str_ty = a.primitive(PrimitiveKind::String);

        let source = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let target = a.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "x".into(),
                ty: str_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        assert_eq!(
            relate(
                &a,
                &mut c,
                source,
                target,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn object_optional_property_ok_when_missing() {
        let (mut a, mut c, mut st) = fresh();
        let str_ty = a.primitive(PrimitiveKind::String);

        let source = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let target = a.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "x".into(),
                ty: str_ty,
                optional: true,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        assert_eq!(
            relate(
                &a,
                &mut c,
                source,
                target,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::Assignable
        );
    }

    #[test]
    fn readonly_array_not_assignable_to_mutable() {
        let (mut a, mut c, mut st) = fresh();
        let el = a.primitive(PrimitiveKind::String);
        let ro_arr = a.array(el, true);
        let mut_arr = a.array(el, false);
        assert_eq!(
            relate(
                &a,
                &mut c,
                ro_arr,
                mut_arr,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::NotAssignable
        );
    }

    #[test]
    fn mutable_array_assignable_to_readonly() {
        let (mut a, mut c, mut st) = fresh();
        let el = a.primitive(PrimitiveKind::String);
        let mut_arr = a.array(el, false);
        let ro_arr = a.array(el, true);
        assert_eq!(
            relate(
                &a,
                &mut c,
                mut_arr,
                ro_arr,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::Assignable
        );
    }

    #[test]
    fn relation_caching_works() {
        let (mut a, mut c, mut st) = fresh();
        let s = a.primitive(PrimitiveKind::String);
        let n = a.primitive(PrimitiveKind::Number);

        let r1 = relate(&a, &mut c, s, n, RelationMode::Assignable, &mut st);
        assert_eq!(r1, RelationResult::NotAssignable);

        let r2 = relate(&a, &mut c, s, n, RelationMode::Assignable, &mut st);
        assert_eq!(r2, RelationResult::NotAssignable);
    }
}
