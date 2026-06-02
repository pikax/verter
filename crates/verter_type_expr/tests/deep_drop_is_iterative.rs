//! Depth-safety regression for [`verter_type_expr::TypeExpr`]'s drop.
//!
//! # Why this exists
//!
//! `TypeExpr` is a recursively-`Arc`-linked tree. Real TypeScript lowers
//! to deeply-nested `TypeExpr` chains (`Array<Array<...>>`, long
//! `extends ? : ...` chains, deeply-parenthesised unions). The
//! compiler-generated drop glue is recursive: dropping the outer node
//! drops its `Arc<TypeExpr>` child, which drops *its* child, ... — so a
//! sufficiently deep tree overflows the thread stack during DROP ALONE,
//! before any hash / clone / resolve touches it.
//!
//! This file pins the manual iterative [`Drop`] impl. It builds a tree
//! far deeper than any default thread stack (~2–8 MiB) could survive a
//! recursive drop of, then drops it.
//!
//! # Pre-fix vs post-fix discrimination
//!
//! - **Pre-fix** (derived/auto recursive drop): the `drop(deep)` at the
//!   end of each test overflows the stack (`STATUS_STACK_OVERFLOW` /
//!   `SIGSEGV`) — the process aborts and the test does NOT complete.
//! - **Post-fix** (iterative `Drop`): the drop runs in O(1) stack and
//!   the test returns normally.
//!
//! Discrimination is empirically real: with the manual `Drop` removed
//! from `src/lib.rs`, `cargo test -p verter_type_expr --test
//! deep_drop_is_iterative` aborts on a default stack; with it present it
//! passes. (See the upstream g_misc3
//! `fingerprint_handles_deeply_nested_value_without_stack_overflow`,
//! which overflowed for exactly this reason even though the mapper hash
//! walker was already iterative — the overflow was the deep value's
//! drop, not the hash.)

use std::sync::Arc;

use verter_type_expr::{
    FunctionExpr, FunctionParam, MappedModifier, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, RecursiveConditionalBranch, RecursiveConditionalFrame, TupleElement, TypeExpr,
    TypeParam,
};

/// Deep enough that a recursive drop blows even a generous thread stack,
/// while staying fast to build/drop iteratively.
const DEEP: usize = 200_000;

fn leaf() -> Arc<TypeExpr> {
    Arc::new(TypeExpr::Primitive(PrimitiveName::String))
}

/// A direct `Array<Array<...<string>>>` chain `DEEP` levels deep. This is
/// the exact shape that overflowed g_misc3's depth test on drop.
#[test]
fn deeply_nested_array_chain_drops_without_stack_overflow() {
    let mut current = leaf();
    for _ in 0..DEEP {
        current = Arc::new(TypeExpr::Array {
            element: current,
            readonly: false,
        });
    }
    // The drop at end of scope MUST NOT overflow. A recursive drop of a
    // 200k-deep chain blows any default stack long before returning.
    drop(current);
}

/// `Parenthesized` is a different single-child arm — exercise it too so a
/// regression that only fixed `Array` would still be caught.
#[test]
fn deeply_nested_parenthesized_chain_drops_without_stack_overflow() {
    let mut current = leaf();
    for _ in 0..DEEP {
        current = Arc::new(TypeExpr::Parenthesized(current));
    }
    drop(current);
}

/// A deep chain threaded through MANY different recursive arms — proves
/// the iterative drop drains every child-bearing variant, not just the
/// `Arc<TypeExpr>` ones (slices, `Option`, inline `ty` in members /
/// params / tuple elements, conditional-frame `check`/`extends`).
///
/// The accumulated deep chain is always MOVED into the next node (never
/// cloned — cloning the chain would itself recurse and is O(DEEP²)). For
/// the inline-`TypeExpr` arms the chain is unwrapped from its sole-owner
/// `Arc` into an owned value first.
#[test]
fn deeply_nested_mixed_variant_chain_drops_without_stack_overflow() {
    // Unwrap the sole-owner `Arc<TypeExpr>` accumulator into an owned
    // `TypeExpr` (needed by the arms that embed the child by value).
    fn own(arc: Arc<TypeExpr>) -> TypeExpr {
        Arc::into_inner(arc).expect("accumulator is uniquely owned")
    }

    let mut current = leaf();
    for i in 0..DEEP {
        current = match i % 11 {
            0 => Arc::new(TypeExpr::Array {
                element: current,
                readonly: i % 2 == 0,
            }),
            1 => Arc::new(TypeExpr::Union(Arc::from(vec![own(current)]))),
            2 => Arc::new(TypeExpr::IndexedAccess {
                object: current,
                index: leaf(),
            }),
            3 => Arc::new(TypeExpr::Conditional {
                check: current,
                extends: leaf(),
                true_type: leaf(),
                false_type: leaf(),
            }),
            4 => Arc::new(TypeExpr::Mapped {
                parameter: "K".to_string(),
                source: leaf(),
                value: current,
                optional: MappedModifier::None,
                readonly: MappedModifier::None,
                name_type: None,
            }),
            5 => Arc::new(TypeExpr::Tuple {
                elements: Arc::from(vec![TupleElement {
                    label: None,
                    ty: own(current),
                    optional: false,
                    rest: false,
                }]),
                readonly: false,
            }),
            6 => Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                    "p".to_string(),
                    own(current),
                    false,
                    false,
                ))],
            }))),
            7 => Arc::new(TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
                vec![FunctionParam::synthetic(
                    Some("a".to_string()),
                    own(current),
                    false,
                    false,
                )],
                Some(leaf()),
                Vec::new(),
            )))),
            8 => Arc::new(TypeExpr::TypeParameter(TypeParam {
                name: "T".to_string(),
                constraint: Some(current),
                default: None,
            })),
            9 => Arc::new(TypeExpr::RecursiveRef {
                name: Arc::from("R"),
                type_arguments: Arc::from(vec![own(current)]),
                conditional_context: Arc::from(vec![RecursiveConditionalFrame {
                    branch: RecursiveConditionalBranch::True,
                    decided: false,
                    check: leaf(),
                    extends: leaf(),
                }]),
            }),
            _ => Arc::new(TypeExpr::KeyOf(current)),
        };
    }
    drop(current);
}

/// A deep subtree SHARED between two owners (`Arc` strong count > 1) must
/// drop safely: the first owner to drop only decrements the shared
/// child's strong count (it does NOT flatten or free the shared subtree),
/// and the LAST owner flattens it iteratively. Neither drop overflows,
/// and the shared subtree is freed exactly once (a double-free would
/// abort the process).
///
/// This pins the `Arc::strong_count == 1` / `Arc::into_inner` sole-owner
/// guard in the iterative `Drop`.
#[test]
fn shared_deep_subtree_drops_exactly_once_without_overflow() {
    // Build a deep chain owned by a single `Arc` (strong count 1).
    let mut deep = leaf();
    for _ in 0..DEEP {
        deep = Arc::new(TypeExpr::Array {
            element: deep,
            readonly: false,
        });
    }
    assert_eq!(Arc::strong_count(&deep), 1, "premise: deep starts unique");

    // Two distinct outer owners, each holding a CLONE of the same deep
    // `Arc` (so the shared chain now has strong count 2).
    let owner_a = TypeExpr::Parenthesized(Arc::clone(&deep));
    let owner_b = TypeExpr::KeyOf(Arc::clone(&deep));
    assert_eq!(
        Arc::strong_count(&deep),
        3,
        "premise: deep is shared by `deep`, owner_a, owner_b",
    );

    // Drop our own handle first → strong count 2 (still shared by the
    // two owners). No flatten happens here for the shared chain.
    drop(deep);

    // Drop the first owner → its clone decrements the shared chain to
    // count 1 WITHOUT flattening (the guard sees count > 1 at steal time
    // and leaves it intact). Must not overflow.
    drop(owner_a);

    // Drop the last owner → now the sole owner of the deep chain, which
    // it flattens iteratively. Must not overflow, must not double-free.
    drop(owner_b);
}
