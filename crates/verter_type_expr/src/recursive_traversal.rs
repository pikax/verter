//! Depth-safe recursive traversal of [`TypeExpr`]: the iterative `Drop`
//! and the iterative, byte-identical-to-derive `Hash`.
//!
//! Both impls live here (rather than inline in `lib.rs`) purely to keep
//! the crate root under the production file-size budget. The orphan rule
//! permits `impl Drop`/`impl Hash for TypeExpr` in any module of this
//! crate because `TypeExpr` is crate-local; the helpers reach every node
//! type through `use crate::*`.
//!
//! Neither walker recurses on the call stack — both flatten the
//! `Arc<TypeExpr>` tree onto an explicit heap worklist — so arbitrarily
//! deep annotations drop and hash without overflowing the thread stack.

use crate::*;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};
use verter_span::Span;

// ---------------------------------------------------------------------------
// Iterative drop — depth-safe deconstruction
// ---------------------------------------------------------------------------
//
// `TypeExpr` is a recursively-`Arc`-linked tree. Real-world TypeScript
// produces deeply-nested annotations (`Array<Array<...>>`, long
// `extends ? : extends ? : ...` chains, deeply-parenthesised unions),
// which lower into `TypeExpr` chains thousands of levels deep. The
// compiler-generated drop glue is recursive — dropping the outermost
// node drops its `Arc<TypeExpr>` child, which drops *its* child, and so
// on — so a sufficiently deep tree overflows the thread stack during
// drop alone, before any consumer touches it.
//
// This manual `Drop` flattens the recursion onto the heap: it drains
// every directly-owned child `TypeExpr` into an explicit worklist,
// leaving each visited node SHALLOW (its recursive children replaced by
// cheap leaves) so the subsequent compiler drop glue has nothing deep to
// chase. The worklist is processed iteratively, so the call stack stays
// flat regardless of tree depth.
//
// `Arc` sharing is respected exactly: a child is only stolen (and thus
// flattened) when this node is its SOLE strong owner. When the child is
// shared, dropping this node merely decrements the strong count and the
// child's own storage is left intact for the final owner to flatten
// later — so no node is ever dropped twice and shared subtrees are never
// mutated out from under another owner.

/// Cheap terminal leaf used to replace stolen recursive children. It owns
/// no further `TypeExpr`, so the structure that retains it drops in O(1).
const fn drop_leaf() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Unknown)
}

/// Steal the inner `TypeExpr` out of a sized `Arc<TypeExpr>` field into
/// `worklist`, leaving a shared empty placeholder behind. Only flattens
/// when this is the sole strong owner; a shared child is left for its
/// final owner (the `Arc` swapped out here just decrements the count).
fn drain_arc(field: &mut Arc<TypeExpr>, worklist: &mut Vec<TypeExpr>) {
    let owned = std::mem::replace(field, drop_leaf_arc());
    if let Some(inner) = Arc::into_inner(owned) {
        worklist.push(inner);
    }
}

/// As [`drain_arc`] but for an `Option<Arc<TypeExpr>>` field.
fn drain_opt_arc(field: &mut Option<Arc<TypeExpr>>, worklist: &mut Vec<TypeExpr>) {
    if let Some(owned) = field.take() {
        if let Some(inner) = Arc::into_inner(owned) {
            worklist.push(inner);
        }
    }
}

/// Steal every element of an `Arc<[TypeExpr]>` field into `worklist`,
/// leaving a shared empty slice behind. Only flattens when this is the
/// sole strong owner (otherwise `make_mut` would deep-clone the shared
/// slice, which is exactly the recursion we are avoiding).
fn drain_slice(field: &mut Arc<[TypeExpr]>, worklist: &mut Vec<TypeExpr>) {
    let mut owned = std::mem::replace(field, empty_type_args());
    if Arc::strong_count(&owned) == 1 {
        for el in Arc::make_mut(&mut owned).iter_mut() {
            worklist.push(std::mem::replace(el, drop_leaf()));
        }
    }
}

/// Shared cheap `Arc<TypeExpr>` placeholder, allocated once.
fn drop_leaf_arc() -> Arc<TypeExpr> {
    static LEAF: LazyLock<Arc<TypeExpr>> = LazyLock::new(|| Arc::new(drop_leaf()));
    Arc::clone(&LEAF)
}

impl Drop for TypeExpr {
    fn drop(&mut self) {
        // Worklist of stolen children to flatten. `self`'s own (now
        // shallow) fields are dropped by the compiler glue when this
        // function returns; every node placed on the worklist is itself
        // drained to shallow before it drops, so the chain never recurses.
        let mut worklist: Vec<TypeExpr> = Vec::new();
        drain_children(self, &mut worklist);
        while let Some(mut node) = worklist.pop() {
            drain_children(&mut node, &mut worklist);
            // `node` drops here — shallow, O(1) — as it leaves scope.
        }
    }
}

/// Move every directly-owned recursive `TypeExpr` child of `node` onto
/// `worklist`, leaving `node` shallow. Leaf variants do nothing.
fn drain_children(node: &mut TypeExpr, worklist: &mut Vec<TypeExpr>) {
    match node {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => {}

        TypeExpr::Union(items) | TypeExpr::Intersection(items) => drain_slice(items, worklist),

        TypeExpr::Array { element, .. } => drain_arc(element, worklist),

        TypeExpr::Tuple { elements, .. } => {
            let mut owned = std::mem::replace(elements, Arc::from(Vec::<TupleElement>::new()));
            if Arc::strong_count(&owned) == 1 {
                for el in Arc::make_mut(&mut owned).iter_mut() {
                    worklist.push(std::mem::replace(&mut el.ty, drop_leaf()));
                }
            }
        }

        TypeExpr::Object(obj) => {
            if let Some(obj) = Arc::into_inner(std::mem::replace(
                obj,
                Arc::new(ObjectExpr {
                    properties: Vec::new(),
                }),
            )) {
                for member in obj.properties {
                    drain_object_member(member, worklist);
                }
            }
        }

        TypeExpr::Function(func) => {
            if let Some(func) = Arc::into_inner(std::mem::replace(
                func,
                Arc::new(FunctionExpr::synthetic(Vec::new(), None, Vec::new())),
            )) {
                drain_function_expr(func, worklist);
            }
        }

        TypeExpr::Ref { type_arguments, .. } => drain_slice(type_arguments, worklist),

        TypeExpr::TypeParameter(tp) => {
            drain_opt_arc(&mut tp.constraint, worklist);
            drain_opt_arc(&mut tp.default, worklist);
        }

        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            drain_arc(inner, worklist);
        }

        TypeExpr::IndexedAccess { object, index } => {
            drain_arc(object, worklist);
            drain_arc(index, worklist);
        }

        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            drain_arc(check, worklist);
            drain_arc(extends, worklist);
            drain_arc(true_type, worklist);
            drain_arc(false_type, worklist);
        }

        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            drain_arc(source, worklist);
            drain_arc(value, worklist);
            drain_opt_arc(name_type, worklist);
        }

        TypeExpr::TemplateLiteral { expressions, .. } => drain_slice(expressions, worklist),

        TypeExpr::RecursiveRef {
            type_arguments,
            conditional_context,
            ..
        } => {
            drain_slice(type_arguments, worklist);
            let mut owned = std::mem::replace(
                conditional_context,
                Arc::from(Vec::<RecursiveConditionalFrame>::new()),
            );
            if Arc::strong_count(&owned) == 1 {
                for frame in Arc::make_mut(&mut owned).iter_mut() {
                    drain_arc(&mut frame.check, worklist);
                    drain_arc(&mut frame.extends, worklist);
                }
            }
        }
    }
}

/// Steal the inline `TypeExpr` children of an owned `ObjectMember`.
fn drain_object_member(member: ObjectMember, worklist: &mut Vec<TypeExpr>) {
    match member {
        ObjectMember::Property(mut p) => worklist.push(std::mem::replace(&mut p.ty, drop_leaf())),
        ObjectMember::IndexSignature(mut s) => {
            worklist.push(std::mem::replace(&mut s.key_type, drop_leaf()));
            worklist.push(std::mem::replace(&mut s.value_type, drop_leaf()));
        }
        ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
            drain_function_expr(f, worklist);
        }
        ObjectMember::Method(m) => drain_function_expr(m.function, worklist),
    }
}

/// Steal the recursive `TypeExpr` children of an owned `FunctionExpr`.
fn drain_function_expr(func: FunctionExpr, worklist: &mut Vec<TypeExpr>) {
    for mut p in func.parameters {
        worklist.push(std::mem::replace(&mut p.ty, drop_leaf()));
    }
    if let Some(ret) = func.return_type {
        if let Some(inner) = Arc::into_inner(ret) {
            worklist.push(inner);
        }
    }
    for mut tp in func.type_parameters {
        drain_opt_arc(&mut tp.constraint, worklist);
        drain_opt_arc(&mut tp.default, worklist);
    }
}

// ---------------------------------------------------------------------------
// Iterative hash — depth-safe, byte-identical to the former derived Hash
// ---------------------------------------------------------------------------
//
// The std `#[derive(Hash)]` for `TypeExpr` is recursive over the
// `Arc<TypeExpr>` tree, so a deeply-nested type overflows the stack when
// hashed (`cycle_guard::hash_type_expr` routes a `TypeExpr` through
// `Hash`). This hand-written impl walks the tree iteratively with an
// explicit heap work-stack of CONTINUATION FRAMES, emitting the EXACT
// same `Hasher` call/byte stream as the derive:
//
//   discriminant (an `isize`, declaration order 0..) then each field in
//   declaration order; slices emit `len` (usize) then each element;
//   `Option<Arc<TypeExpr>>` emits its `isize` discriminant then the
//   inner subtree; the hand-written `Hash` impls on `LiteralValue` and
//   `FunctionParam` (the latter excluding `has_ts_annotation`) are
//   reused verbatim.
//
// A simple `Vec<&TypeExpr>` node-stack would NOT suffice: several
// variants emit leaf bytes AFTER a child subtree (e.g. `Array.readonly`
// after `element`; `Mapped`'s `optional`/`readonly` between `value` and
// `name_type`; every aggregate's trailing fields). Continuation frames
// preserve that exact interleaving — a trailing-leaf frame pushed BELOW
// a child's `Node` frame is popped only after the child subtree is fully
// hashed.
//
// Byte-identity is pinned by `tests/hash_byte_stream_contract.rs`, which
// asserts this stream equals a frozen mirror of the former derive across
// every variant.

/// One unit of hashing work on the iterative `Hash` work-stack.
///
/// `Node` (and the struct-decomposition frames) emit a leading
/// discriminant / leading leaves inline, then push their remaining
/// sub-steps in REVERSE emission order so the LIFO stack replays them
/// forward. Leaf frames carry `Copy` payloads emitted on pop AFTER the
/// child subtree(s) above them have drained.
enum HashStep<'a> {
    /// Hash a `TypeExpr` node (discriminant + decomposition).
    Node(&'a TypeExpr),
    /// `Option<Arc<TypeExpr>>`: emit the `isize` discriminant, then the
    /// inner subtree when `Some`.
    OptNode(Option<&'a TypeExpr>),
    /// Emit a `usize` (a slice/collection length that follows an earlier
    /// child block within the same node).
    Usize(usize),
    /// Emit a trailing `bool` field.
    Bool(bool),
    /// Emit a trailing `MappedModifier` field.
    Modifier(MappedModifier),
    /// Emit a trailing `MemberSpans` field.
    MemberSpans(MemberSpans),
    /// Emit a trailing `IndexSignatureSpans` field.
    IndexSpans(IndexSignatureSpans),
    /// Emit a trailing `FunctionSpans` field.
    FnSpans(FunctionSpans),
    /// Emit a trailing `Option<Span>` field (a function parameter's span).
    OptSpan(Option<Span>),
    /// Decompose one `ObjectMember`.
    Member(&'a ObjectMember),
    /// Decompose one `TupleElement`.
    TupleElem(&'a TupleElement),
    /// Decompose one `FunctionParam`.
    Param(&'a FunctionParam),
    /// Decompose one `TypeParam`.
    TyParam(&'a TypeParam),
    /// Decompose one `RecursiveConditionalFrame`.
    RecFrame(&'a RecursiveConditionalFrame),
    /// Decompose one `FunctionExpr` (call/construct signature, method
    /// body, or the `Function` variant payload).
    Func(&'a FunctionExpr),
}

impl Hash for TypeExpr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut stack: Vec<HashStep<'_>> = Vec::with_capacity(16);
        stack.push(HashStep::Node(self));
        while let Some(step) = stack.pop() {
            match step {
                HashStep::Node(node) => hash_node(node, state, &mut stack),
                HashStep::OptNode(opt) => match opt {
                    None => 0isize.hash(state),
                    Some(inner) => {
                        1isize.hash(state);
                        stack.push(HashStep::Node(inner));
                    }
                },
                HashStep::Usize(n) => n.hash(state),
                HashStep::Bool(b) => b.hash(state),
                HashStep::Modifier(m) => m.hash(state),
                HashStep::MemberSpans(s) => s.hash(state),
                HashStep::IndexSpans(s) => s.hash(state),
                HashStep::FnSpans(s) => s.hash(state),
                HashStep::OptSpan(s) => s.hash(state),
                HashStep::Member(m) => hash_object_member_step(m, state, &mut stack),
                HashStep::TupleElem(el) => hash_tuple_element_step(el, state, &mut stack),
                HashStep::Param(p) => hash_param_step(p, state, &mut stack),
                HashStep::TyParam(tp) => hash_type_param_step(tp, state, &mut stack),
                HashStep::RecFrame(f) => hash_recursive_frame_step(f, state, &mut stack),
                HashStep::Func(f) => hash_function_step(f, state, &mut stack),
            }
        }
    }
}

/// Discriminant index in declaration order (matches the derive's
/// `discriminant_value`, hashed as `isize`).
fn type_expr_discriminant(node: &TypeExpr) -> isize {
    match node {
        TypeExpr::Primitive(_) => 0,
        TypeExpr::Literal(_) => 1,
        TypeExpr::Union(_) => 2,
        TypeExpr::Intersection(_) => 3,
        TypeExpr::Array { .. } => 4,
        TypeExpr::Tuple { .. } => 5,
        TypeExpr::Object(_) => 6,
        TypeExpr::Function(_) => 7,
        TypeExpr::Ref { .. } => 8,
        TypeExpr::TypeParameter(_) => 9,
        TypeExpr::KeyOf(_) => 10,
        TypeExpr::TypeOf(_) => 11,
        TypeExpr::IndexedAccess { .. } => 12,
        TypeExpr::Conditional { .. } => 13,
        TypeExpr::Mapped { .. } => 14,
        TypeExpr::TemplateLiteral { .. } => 15,
        TypeExpr::Infer { .. } => 16,
        TypeExpr::Rest(_) => 17,
        TypeExpr::Parenthesized(_) => 18,
        TypeExpr::RecursiveRef { .. } => 19,
        TypeExpr::SyntheticSlotBinding(_) => 20,
        TypeExpr::Unknown { .. } => 21,
    }
}

/// Hash `node`'s discriminant + leading leaves inline, then push its
/// remaining sub-steps in REVERSE emission order.
fn hash_node<'a, H: Hasher>(node: &'a TypeExpr, state: &mut H, stack: &mut Vec<HashStep<'a>>) {
    type_expr_discriminant(node).hash(state);
    match node {
        // -- Leaves (no recursive children) --
        TypeExpr::Primitive(name) => name.hash(state),
        TypeExpr::Literal(lit) => lit.hash(state),
        TypeExpr::TypeOf(value_ref) => value_ref.hash(state),
        TypeExpr::Infer { name } => name.hash(state),
        TypeExpr::SyntheticSlotBinding(carrier) => carrier.hash(state),
        TypeExpr::Unknown { raw } => raw.hash(state),

        // -- `Arc<[TypeExpr]>`: len (usize) then each element --
        TypeExpr::Union(items) | TypeExpr::Intersection(items) => {
            items.len().hash(state);
            push_nodes_reverse(items, stack);
        }

        // -- Single child then trailing `readonly` --
        TypeExpr::Array { element, readonly } => {
            // Emit: element subtree, then readonly. Push readonly BELOW
            // the child so it pops after the child drains.
            stack.push(HashStep::Bool(*readonly));
            stack.push(HashStep::Node(element));
        }

        // -- Tuple: len, each element, then trailing `readonly` --
        TypeExpr::Tuple { elements, readonly } => {
            elements.len().hash(state);
            stack.push(HashStep::Bool(*readonly));
            for el in elements.iter().rev() {
                stack.push(HashStep::TupleElem(el));
            }
        }

        // -- Object: len, each member --
        TypeExpr::Object(obj) => {
            obj.properties.len().hash(state);
            for member in obj.properties.iter().rev() {
                stack.push(HashStep::Member(member));
            }
        }

        // -- Function payload --
        TypeExpr::Function(func) => stack.push(HashStep::Func(func)),

        // -- Ref: name (leaf) then type_arguments slice --
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            name.hash(state);
            type_arguments.len().hash(state);
            push_nodes_reverse(type_arguments, stack);
        }

        // -- TypeParameter --
        TypeExpr::TypeParameter(tp) => stack.push(HashStep::TyParam(tp)),

        // -- Single child, no trailing leaf --
        TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => {
            stack.push(HashStep::Node(inner));
        }

        // -- Two children, no trailing leaf --
        TypeExpr::IndexedAccess { object, index } => {
            stack.push(HashStep::Node(index));
            stack.push(HashStep::Node(object));
        }

        // -- Four children, no trailing leaf --
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            stack.push(HashStep::Node(false_type));
            stack.push(HashStep::Node(true_type));
            stack.push(HashStep::Node(extends));
            stack.push(HashStep::Node(check));
        }

        // -- Mapped: parameter (leaf), source, value, optional, readonly,
        //    name_type. The two modifiers are emitted BETWEEN `value` and
        //    `name_type`. --
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            parameter.hash(state);
            // Emission order after parameter: source, value, optional,
            // readonly, name_type. Push in reverse.
            stack.push(HashStep::OptNode(name_type.as_deref()));
            stack.push(HashStep::Modifier(*readonly));
            stack.push(HashStep::Modifier(*optional));
            stack.push(HashStep::Node(value));
            stack.push(HashStep::Node(source));
        }

        // -- TemplateLiteral: quasis (leaf Vec<String>) then expressions --
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            quasis.hash(state);
            expressions.len().hash(state);
            push_nodes_reverse(expressions, stack);
        }

        // -- RecursiveRef: name (leaf), type_arguments, conditional_context --
        TypeExpr::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => {
            name.hash(state);
            type_arguments.len().hash(state);
            // Emission order: ta-len (done), each ta, cc-len, each frame.
            // Push reverse: frames, cc-len, ta nodes.
            for frame in conditional_context.iter().rev() {
                stack.push(HashStep::RecFrame(frame));
            }
            stack.push(HashStep::Usize(conditional_context.len()));
            push_nodes_reverse(type_arguments, stack);
        }
    }
}

/// Push each element of a `TypeExpr` slice as a `Node` step in REVERSE so
/// they pop (and hash) in forward order.
fn push_nodes_reverse<'a>(items: &'a [TypeExpr], stack: &mut Vec<HashStep<'a>>) {
    for item in items.iter().rev() {
        stack.push(HashStep::Node(item));
    }
}

/// `ObjectMember` derive: discriminant (isize) then fields in order.
fn hash_object_member_step<'a, H: Hasher>(
    member: &'a ObjectMember,
    state: &mut H,
    stack: &mut Vec<HashStep<'a>>,
) {
    match member {
        ObjectMember::Property(p) => {
            0isize.hash(state);
            p.name.hash(state);
            // ty, optional, readonly, spans. Push reverse.
            stack.push(HashStep::MemberSpans(p.spans));
            stack.push(HashStep::Bool(p.readonly));
            stack.push(HashStep::Bool(p.optional));
            stack.push(HashStep::Node(&p.ty));
        }
        ObjectMember::IndexSignature(s) => {
            1isize.hash(state);
            s.key_name.hash(state);
            // key_type, value_type, readonly, spans. Push reverse.
            stack.push(HashStep::IndexSpans(s.spans));
            stack.push(HashStep::Bool(s.readonly));
            stack.push(HashStep::Node(&s.value_type));
            stack.push(HashStep::Node(&s.key_type));
        }
        ObjectMember::CallSignature(f) => {
            2isize.hash(state);
            stack.push(HashStep::Func(f));
        }
        ObjectMember::ConstructSignature(f) => {
            3isize.hash(state);
            stack.push(HashStep::Func(f));
        }
        ObjectMember::Method(m) => {
            4isize.hash(state);
            m.name.hash(state);
            // function, optional, spans. Push reverse.
            stack.push(HashStep::MemberSpans(m.spans));
            stack.push(HashStep::Bool(m.optional));
            stack.push(HashStep::Func(&m.function));
        }
    }
}

/// `TupleElement`: label (leaf), ty, optional, rest.
fn hash_tuple_element_step<'a, H: Hasher>(
    el: &'a TupleElement,
    state: &mut H,
    stack: &mut Vec<HashStep<'a>>,
) {
    el.label.hash(state);
    stack.push(HashStep::Bool(el.rest));
    stack.push(HashStep::Bool(el.optional));
    stack.push(HashStep::Node(&el.ty));
}

/// `FunctionParam` hand-written `Hash`: name, ty, optional, rest, span
/// (EXCLUDES `has_ts_annotation`, exactly like the hand-written
/// `Hash for FunctionParam`).
fn hash_param_step<'a, H: Hasher>(
    p: &'a FunctionParam,
    state: &mut H,
    stack: &mut Vec<HashStep<'a>>,
) {
    p.name.hash(state);
    // Emission order after `name`: ty subtree, optional, rest, span.
    // Push reverse so the LIFO stack replays them forward.
    stack.push(HashStep::OptSpan(p.span));
    stack.push(HashStep::Bool(p.rest));
    stack.push(HashStep::Bool(p.optional));
    stack.push(HashStep::Node(&p.ty));
}

/// `TypeParam` derive: name (leaf), constraint (Option), default (Option).
fn hash_type_param_step<'a, H: Hasher>(
    tp: &'a TypeParam,
    state: &mut H,
    stack: &mut Vec<HashStep<'a>>,
) {
    tp.name.hash(state);
    // constraint, default. Push reverse.
    stack.push(HashStep::OptNode(tp.default.as_deref()));
    stack.push(HashStep::OptNode(tp.constraint.as_deref()));
}

/// `RecursiveConditionalFrame` derive: branch (leaf), decided (leaf),
/// check, extends.
fn hash_recursive_frame_step<'a, H: Hasher>(
    frame: &'a RecursiveConditionalFrame,
    state: &mut H,
    stack: &mut Vec<HashStep<'a>>,
) {
    frame.branch.hash(state);
    frame.decided.hash(state);
    stack.push(HashStep::Node(&frame.extends));
    stack.push(HashStep::Node(&frame.check));
}

/// `FunctionExpr` derive: parameters (len + each), return_type (Option),
/// type_parameters (len + each), spans.
fn hash_function_step<'a, H: Hasher>(
    func: &'a FunctionExpr,
    state: &mut H,
    stack: &mut Vec<HashStep<'a>>,
) {
    func.parameters.len().hash(state);
    // Emission order: each param, return_type, tp-len, each tp, spans.
    // Push reverse: spans, tp frames, tp-len, OptNode(return), param frames.
    stack.push(HashStep::FnSpans(func.spans));
    for tp in func.type_parameters.iter().rev() {
        stack.push(HashStep::TyParam(tp));
    }
    stack.push(HashStep::Usize(func.type_parameters.len()));
    stack.push(HashStep::OptNode(func.return_type.as_deref()));
    for p in func.parameters.iter().rev() {
        stack.push(HashStep::Param(p));
    }
}
