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

        // -- Type parameters --
        (Node::TypeParam { constraint, .. }, _) => {
            relate_source_type_param(arena, caches, *constraint, target, mode, state)
        }

        (_, Node::TypeParam { constraint, .. }) => {
            relate_target_type_param(arena, caches, source, *constraint, mode, state)
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
        let prop_result =
            if let Some(s_prop) = source.properties.iter().find(|p| p.name == t_prop.name) {
                relate(arena, caches, s_prop.ty, t_prop.ty, mode, state)
            } else if let Some(index_result) =
                relate_property_via_source_index(arena, caches, source, t_prop, mode, state)
            {
                index_result
            } else if t_prop.optional {
                RelationResult::Assignable
            } else {
                RelationResult::NotAssignable
            };
        result = result.and(prop_result);
    }

    for t_index in &target.index_signatures {
        result = result.and(relate_target_index_signature(
            arena, caches, source, t_index, mode, state,
        ));
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

    for t_sig in &target.construct_signatures {
        let sig_ok = source.construct_signatures.iter().any(|s_sig| {
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

    for (s_param, t_param) in source.parameters.iter().zip(target.parameters.iter()) {
        let param_result = if contains_infer(arena, t_param.ty) {
            relate(arena, caches, s_param.ty, t_param.ty, mode, state)
        } else {
            relate(arena, caches, t_param.ty, s_param.ty, mode, state)
        };
        result = result.and(param_result);
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

fn contains_infer(arena: &QueryArena, node: NodeId) -> bool {
    match arena.get(node) {
        Node::Infer { .. } => true,
        Node::Array { element, .. } | Node::KeyOf(element) | Node::Rest(element) => {
            contains_infer(arena, *element)
        }
        Node::Tuple { elements, .. } => elements
            .iter()
            .any(|element| contains_infer(arena, element.ty)),
        Node::Union(members) | Node::Intersection(members) => members
            .iter()
            .copied()
            .any(|member| contains_infer(arena, member)),
        Node::Object(obj) => {
            obj.properties
                .iter()
                .any(|prop| contains_infer(arena, prop.ty))
                || obj.index_signatures.iter().any(|sig| {
                    contains_infer(arena, sig.key_type) || contains_infer(arena, sig.value_type)
                })
                || obj.call_signatures.iter().any(|sig| {
                    sig.parameters
                        .iter()
                        .any(|param| contains_infer(arena, param.ty))
                        || contains_infer(arena, sig.return_type)
                })
                || obj.construct_signatures.iter().any(|sig| {
                    sig.parameters
                        .iter()
                        .any(|param| contains_infer(arena, param.ty))
                        || contains_infer(arena, sig.return_type)
                })
        }
        Node::Function(func) => func.signatures.iter().any(|sig| {
            sig.parameters
                .iter()
                .any(|param| contains_infer(arena, param.ty))
                || contains_infer(arena, sig.return_type)
        }),
        Node::Ref { type_arguments, .. } => type_arguments
            .iter()
            .copied()
            .any(|arg| contains_infer(arena, arg)),
        Node::Applied { args, .. } => args.iter().copied().any(|arg| contains_infer(arena, arg)),
        Node::IndexedAccess { object, index } => {
            contains_infer(arena, *object) || contains_infer(arena, *index)
        }
        Node::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            ..
        } => {
            contains_infer(arena, *check)
                || contains_infer(arena, *extends)
                || contains_infer(arena, *true_branch)
                || contains_infer(arena, *false_branch)
        }
        Node::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            contains_infer(arena, *source)
                || contains_infer(arena, *value)
                || name_type
                    .map(|node| contains_infer(arena, node))
                    .unwrap_or(false)
        }
        Node::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .copied()
            .any(|expr| contains_infer(arena, expr)),
        Node::Primitive(_)
        | Node::Literal(_)
        | Node::TypeParam { .. }
        | Node::RecursiveRef { .. }
        | Node::Error { .. }
        | Node::TypeOf { .. } => false,
    }
}

fn relate_source_type_param(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    constraint: Option<NodeId>,
    target: NodeId,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    let Some(constraint) = constraint else {
        return RelationResult::Unknown;
    };

    match relate(arena, caches, constraint, target, mode, state) {
        RelationResult::Assignable => RelationResult::Assignable,
        RelationResult::NotAssignable | RelationResult::Unknown => RelationResult::Unknown,
    }
}

fn relate_target_type_param(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: NodeId,
    constraint: Option<NodeId>,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    let Some(constraint) = constraint else {
        return RelationResult::Unknown;
    };

    match relate(arena, caches, source, constraint, mode, state) {
        RelationResult::NotAssignable => RelationResult::NotAssignable,
        RelationResult::Assignable | RelationResult::Unknown => RelationResult::Unknown,
    }
}

fn relate_property_via_source_index(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: &ObjectNode,
    target_prop: &super::arena::PropertyNode,
    mode: RelationMode,
    state: &mut RelationState,
) -> Option<RelationResult> {
    let mut matched = false;
    let mut result = RelationResult::Assignable;

    for s_index in &source.index_signatures {
        if !index_signature_applies_to_property(arena, s_index.key_type, &target_prop.name) {
            continue;
        }
        matched = true;
        result = result.and(relate(
            arena,
            caches,
            s_index.value_type,
            target_prop.ty,
            mode,
            state,
        ));
    }

    matched.then_some(result)
}

fn relate_target_index_signature(
    arena: &QueryArena,
    caches: &mut SolverCaches,
    source: &ObjectNode,
    target_index: &super::arena::IndexSignatureNode,
    mode: RelationMode,
    state: &mut RelationState,
) -> RelationResult {
    let mut result = RelationResult::Assignable;

    for s_index in &source.index_signatures {
        if !index_domains_overlap(arena, s_index.key_type, target_index.key_type) {
            continue;
        }
        result = result.and(relate(
            arena,
            caches,
            s_index.value_type,
            target_index.value_type,
            mode,
            state,
        ));
    }

    for prop in &source.properties {
        if !index_signature_applies_to_property(arena, target_index.key_type, &prop.name) {
            continue;
        }
        result = result.and(relate(
            arena,
            caches,
            prop.ty,
            target_index.value_type,
            mode,
            state,
        ));
    }

    result
}

fn index_domains_overlap(arena: &QueryArena, source_key: NodeId, target_key: NodeId) -> bool {
    match arena.get(target_key) {
        Node::Primitive(PrimitiveKind::String | PrimitiveKind::Any) => matches!(
            arena.get(source_key),
            Node::Primitive(
                PrimitiveKind::String
                    | PrimitiveKind::Number
                    | PrimitiveKind::Any
                    | PrimitiveKind::Unknown
            ) | Node::Literal(_)
                | Node::Union(_)
        ),
        Node::Primitive(PrimitiveKind::Number) => matches!(
            arena.get(source_key),
            Node::Primitive(PrimitiveKind::Number | PrimitiveKind::Any | PrimitiveKind::Unknown)
                | Node::Literal(super::arena::SolverLiteral::Number(_))
                | Node::Union(_)
        ),
        Node::Literal(super::arena::SolverLiteral::String(name)) => {
            index_signature_applies_to_property(arena, source_key, name)
        }
        Node::Literal(super::arena::SolverLiteral::Number(n)) => {
            index_signature_applies_to_property(arena, source_key, &format_numeric_property(*n))
        }
        Node::Union(members) => members
            .iter()
            .any(|&member| index_domains_overlap(arena, source_key, member)),
        _ => false,
    }
}

fn index_signature_applies_to_property(
    arena: &QueryArena,
    key_type: NodeId,
    property_name: &str,
) -> bool {
    match arena.get(key_type) {
        Node::Primitive(PrimitiveKind::String | PrimitiveKind::Any) => true,
        Node::Primitive(PrimitiveKind::Number) => property_name.parse::<u64>().is_ok(),
        Node::Literal(SolverLiteral::String(name)) => name == property_name,
        Node::Literal(SolverLiteral::Number(n)) => format_numeric_property(*n) == property_name,
        Node::Union(members) => members
            .iter()
            .any(|&member| index_signature_applies_to_property(arena, member, property_name)),
        _ => false,
    }
}

fn format_numeric_property(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
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

    #[test]
    fn object_properties_must_satisfy_target_string_index_signature() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);

        let source = a.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "title".into(),
                ty: string_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let target = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: string_ty,
                value_type: string_ty,
                readonly: false,
            }],
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
    fn object_properties_fail_target_string_index_signature_on_value_mismatch() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);
        let number_ty = a.primitive(PrimitiveKind::Number);

        let source = a.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "title".into(),
                ty: number_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let target = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: string_ty,
                value_type: string_ty,
                readonly: false,
            }],
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
    fn source_index_signature_satisfies_named_target_property() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);

        let source = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![IndexSignatureNode {
                key_type: string_ty,
                value_type: string_ty,
                readonly: false,
            }],
            call_signatures: vec![],
            construct_signatures: vec![],
        });

        let target = a.object(ObjectNode {
            properties: vec![PropertyNode {
                name: "title".into(),
                ty: string_ty,
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
    fn construct_signatures_participate_in_object_assignability() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);

        let ctor = CallSignatureNode {
            type_parameters: vec![],
            parameters: vec![],
            return_type: string_ty,
        };

        let source = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![ctor.clone()],
        });

        let target = a.object(ObjectNode {
            properties: vec![],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![ctor],
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
    fn function_parameters_are_contravariant() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);
        let number_ty = a.primitive(PrimitiveKind::Number);
        let string_or_number = a.union(vec![string_ty, number_ty]);
        let void_ty = a.primitive(PrimitiveKind::Void);

        let source = a.function(FunctionNode {
            signatures: vec![CallSignatureNode {
                type_parameters: vec![],
                parameters: vec![ParamNode {
                    name: Some("value".into()),
                    ty: string_ty,
                    optional: false,
                    rest: false,
                }],
                return_type: void_ty,
            }],
        });

        let target = a.function(FunctionNode {
            signatures: vec![CallSignatureNode {
                type_parameters: vec![],
                parameters: vec![ParamNode {
                    name: Some("value".into()),
                    ty: string_or_number,
                    optional: false,
                    rest: false,
                }],
                return_type: void_ty,
            }],
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
    fn constrained_type_parameter_uses_constraint_as_source_upper_bound() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);
        let type_param = a.alloc(Node::TypeParam {
            name: "T".into(),
            constraint: Some(string_ty),
            default: None,
        });

        assert_eq!(
            relate(
                &a,
                &mut c,
                type_param,
                string_ty,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::Assignable
        );
    }

    #[test]
    fn infer_target_binds_source_type_param_before_constraint_projection() {
        let (mut a, mut c, mut st) = fresh();
        let string_ty = a.primitive(PrimitiveKind::String);
        let type_param = a.alloc(Node::TypeParam {
            name: "T".into(),
            constraint: Some(string_ty),
            default: None,
        });
        let infer = a.alloc(Node::Infer { name: "U".into() });

        st.begin_infer();
        assert_eq!(
            relate(
                &a,
                &mut c,
                type_param,
                infer,
                RelationMode::Assignable,
                &mut st
            ),
            RelationResult::Assignable
        );

        let bindings = st
            .take_infer_bindings()
            .expect("infer bindings should be recorded");
        assert_eq!(bindings.candidates("U"), Some(&[type_param][..]));
    }
}
