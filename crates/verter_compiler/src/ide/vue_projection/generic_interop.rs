//! Advanced generic binders and foreign component interoperability.
//!
//! A component use is checked through one construction of the used
//! component (see [`super::component_use`]). This module owns what that
//! construction applies and where it is placed. It preserves published
//! signature relationships within the reflection limits described below:
//!
//! - [`ForeignComponentContractAdapter`] — the construction's callee. It
//!   hands TypeScript the component's own contract first and an
//!   attribute-tolerant signature second, as one intersection, so ordinary
//!   overload resolution picks the first signature that accepts the use:
//!   - a typed constructor (a generated SFC default, a hand-written
//!     declaration, `defineComponent` over a setup function) is passed
//!     through unchanged, so every construct overload and every binder —
//!     constraints, defaults, `const`, variadic and dependent parameters —
//!     stays TypeScript's to resolve, with no fixed overload count;
//!   - a constructor whose last construct signature is Vue's open
//!     `...args: any[]` (the Options API `defineComponent`) is rebuilt
//!     signature by signature, in declaration order: every open signature is
//!     constructed from the props its instance publishes (`$props`, or no
//!     props when the instance publishes none), never widened to accept
//!     anything, and every earlier overload the walk reads stays selectable
//!     ahead of it;
//!   - a callable functional component is consumed through its own call
//!     signatures: each props parameter becomes a construct parameter and
//!     its context's `slots` / `emit` the instance's `$slots` / `$emit`.
//!     A single call signature goes through higher-order inference, so a
//!     generic functional component keeps its binder; an overload set is
//!     rebuilt signature by signature, in declaration order, each overload
//!     taking exactly its declared props, so every overload the walk reads
//!     stays selectable. Generated SFC defaults are never made callable.
//!
//!   Rebuilding walks the signature list from its end, one signature per
//!   step, with no signature count: each step intersects the signatures
//!   already read ahead of the component, TypeScript drops the component's
//!   identical copy, and conditional inference reads the next one. A
//!   per-step marker signature ends the walk once the component's list is
//!   exhausted. A signature with its own type parameters has no identical
//!   non-generic copy, so the walk cannot pass it: TypeScript reads it with
//!   its type parameters at their constraints and cannot enumerate the
//!   signatures declared ahead of it. The walk then keeps what it read —
//!   every signature from that one to the end, never the component itself,
//!   so an open `...args: any[]` signature is never reopened. A lone generic
//!   call signature is left to higher-order inference, which keeps its
//!   binder.
//!
//!   The attribute-tolerant signature (an `unknown` attribute index on the
//!   props of the component's last signature, the published `$props` or no
//!   props behind an open-argument one) is the fallback only, never
//!   part of a rebuilt overload: an undeclared fallthrough attribute is not
//!   an excess-key error, while an exact overload always wins over it.
//! - [`AdvancedGenericUseProjection`] — every use of a parent carrier,
//!   rendered through the adapter inside one scope over the parent's
//!   authored `generic` binder, so a parent parameter forwarded to a child
//!   (`Parent<T>` → `Child<T>`) stays that parameter instead of collapsing to
//!   its constraint, and each use records whether its component contract is
//!   available at all.
//!
//! What the adapter never does: it names no signature count, recovers no
//! generic information a dependency already erased, and replaces no
//! published type with `any`. An authored `any` (or an unresolved component
//! TypeScript types as `any`) stays exactly that; a use whose component
//! cannot be named has no witness rather than a fabricated one. Nothing here
//! answers types: TypeScript owns overload selection, inference and
//! constraint checking, and reports a violated explicit argument
//! (`Comp<string>`) at the authored argument.

use crate::framework_common::projection_plan::{ComponentUseId, PlanSnapshotId, ProjectionPlan};
use crate::ide::vue_projection::component_use::{
    ComponentUseProjection, USE_CONSTRUCTOR, USE_PRELUDE,
};
use crate::ide::vue_projection::public_constructor::{
    render_binder_list, BinderSite, PublicBinderParam,
};

/// Adapter entry: the component's exact contract intersected with its
/// attribute-tolerant signature.
pub const USE_COMPONENT: &str = "__VerterUseComponent";
/// The component's exact construct contract: a typed constructor itself, a
/// constructor ending in an open-argument signature rebuilt signature by
/// signature, an overloaded callable rebuilt signature by signature, and no
/// construct signature for anything else.
pub const USE_CONTRACT: &str = "__VerterUseContract";
/// Readable construct signatures of a constructor ending in an open-argument
/// signature, rebuilt in declaration order; see the module's generic limit.
pub const USE_CONSTRUCTS: &str = "__VerterUseConstructs";
/// Readable call signatures of an overloaded callable, rebuilt in declaration
/// order as construct signatures; see the module's generic limit.
pub const USE_CALLS: &str = "__VerterUseCalls";
/// Props of the attribute-tolerant signature: the declared props (the
/// published `$props`, or no props, behind an open-argument constructor)
/// plus an `unknown` attribute index.
pub const USE_TOLERANT: &str = "__VerterUseTolerant";
/// Instance of a functional component: its props and its context's slots
/// and emit.
pub const USE_FUNCTIONAL: &str = "__VerterUseFunctional";
/// Whether a construct parameter list is Vue's open `...args: any[]`.
pub const USE_OPEN_ARGS: &str = "__VerterUseOpenArgs";
/// The generic function every use of a parent is placed in.
pub const USE_SCOPE: &str = "__VerterGenericUseScope";

/// Module-scope declarations of the foreign component contract adapter, as a
/// literal so the component-use prelude can embed it.
macro_rules! foreign_contract_declarations {
    () => {
        concat!(
            "type __VerterUseOpenArgs<A> = A extends readonly any[] ? (number extends A[\"length\"] ? (0 extends 1 & A[number] ? true : false) : false) : false;\n",
            "type __VerterUseSame<X, Y> = (<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;\n",
            "type __VerterUsePeeled<N> = { readonly __verterUsePeeled: N };\n",
            "type __VerterUseOrdered<T, Acc> = T extends readonly [infer H, ...infer R] ? __VerterUseOrdered<R, Acc & H> : Acc;\n",
            "type __VerterUseConstruct<A extends readonly unknown[], I> = __VerterUseOpenArgs<A> extends true ? (I extends { readonly $props: infer P } ? new (props: P) => I : new (props: Record<string, never>) => I) : new (...args: A) => I;\n",
            "type __VerterUseConstructs<C, Seen, Prev, Out extends readonly unknown[]> = (Seen & C) extends abstract new (...args: infer A) => infer I ? (A extends readonly [__VerterUsePeeled<number>] ? __VerterUseOrdered<Out, unknown> : __VerterUseSame<[A, I], Prev> extends true ? __VerterUseOrdered<Out, unknown> : __VerterUseConstructs<C, Seen & { new (...args: A): I; new (...args: [__VerterUsePeeled<Out[\"length\"]>]): never }, [A, I], [__VerterUseConstruct<A, I>, ...Out]>) : C;\n",
            "type __VerterUseCall<A> = A extends readonly [infer P, ...infer X] ? new (props: P) => __VerterUseFunctional<P, X extends readonly [infer Y, ...unknown[]] ? Y : unknown> : new (props: Record<string, never>) => __VerterUseFunctional<unknown, unknown>;\n",
            "type __VerterUseCalls<C, Seen, Prev, Out extends readonly unknown[]> = (Seen & C) extends (...args: infer A) => infer R ? (A extends readonly [__VerterUsePeeled<number>] ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) : __VerterUseSame<[A, R], Prev> extends true ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) : __VerterUseCalls<C, Seen & { (...args: A): R; (...args: [__VerterUsePeeled<Out[\"length\"]>]): never }, [A, R], [__VerterUseCall<A>, ...Out]>) : unknown;\n",
            "type __VerterUseContract<C> = C extends abstract new (...args: infer A) => unknown ? (__VerterUseOpenArgs<A> extends true ? __VerterUseConstructs<C, unknown, never, []> : C) : __VerterUseCalls<C, unknown, never, []>;\n",
            "type __VerterUseTolerant<P, I> = (0 extends 1 & P ? (I extends { readonly $props: infer Q } ? Q : (0 extends 1 & I ? P : {})) : P) & Record<string, unknown>;\n",
            "type __VerterUseFunctional<P, X> = { readonly $props: P; readonly $slots: X extends { slots: infer S } ? S : {}; $emit: X extends { emit: infer E } ? E : never };\n",
            "declare function __VerterUseComponent<C, A>(component: C, tolerant: A): __VerterUseContract<C> & A;\n",
            "declare function __VerterUseConstructor<P, I>(component: abstract new (props: P) => I): new (props: __VerterUseTolerant<P, I>) => I;\n",
            "declare function __VerterUseConstructor<P, X, R>(component: (props: P, ctx: X) => R): new (props: P & Record<string, unknown>) => __VerterUseFunctional<P, X>;\n",
        )
    };
}
pub(super) use foreign_contract_declarations;

/// The foreign component contract adapter: what a use's one construction
/// applies (see the module docs for the per-shape rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForeignComponentContractAdapter;

impl ForeignComponentContractAdapter {
    /// Module-scope declarations the rendered callee reads, rendered once
    /// per checking module (inside the component-use prelude).
    pub const DECLARATIONS: &'static str = foreign_contract_declarations!();

    /// The constructor a use of `component` applies: its exact contract,
    /// then its attribute-tolerant signature. `component` is the rendered
    /// constructor expression (a binding, a member path or a parenthesized
    /// `:is` expression), repeated so each helper infers from the
    /// component's own declared type.
    #[must_use]
    pub fn construction_callee(&self, component: &str) -> String {
        format!("{USE_COMPONENT}({component}, {USE_CONSTRUCTOR}({component}))")
    }
}

/// Whether a use's component contract reached the construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseContractAvailability {
    /// The use is constructed through the adapter; its witness binding.
    Witnessed {
        /// Witness binding the construction initializes.
        binding: String,
    },
    /// The component cannot be named or a contributing expression was not
    /// admitted: no witness exists and none is fabricated. Distinct from a
    /// component whose declared type is `any` or unresolved, which is
    /// constructed and keeps that type.
    Unavailable,
}

/// One use of the parent carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedGenericUse {
    /// Logical component use.
    pub use_id: ComponentUseId,
    /// Whether its contract reached a construction.
    pub availability: UseContractAvailability,
}

/// Every use of one parent carrier, placed in one scope over the parent's
/// authored binder and constructed through the foreign contract adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvancedGenericUseProjection {
    /// Plan snapshot the uses were derived from.
    pub snapshot: PlanSnapshotId,
    /// False when the component-use product is incomplete; an incomplete
    /// product never warms caches.
    pub complete: bool,
    /// The parent's authored `generic` binder, in authored order; empty for
    /// a non-generic parent.
    pub binder: Vec<PublicBinderParam>,
    /// Every use in plan order.
    pub uses: Vec<AdvancedGenericUse>,
    /// The witnesses of the available uses.
    pub witnesses: ComponentUseProjection,
}

impl AdvancedGenericUseProjection {
    /// The scope's type parameter list: the authored binder with its
    /// constraints, defaults and `const` modifiers verbatim.
    #[must_use]
    pub fn scope_binder(&self) -> String {
        render_binder_list(&self.binder, BinderSite::Construct)
    }

    /// The component-use prelude, then one generic function over the
    /// parent's binder holding `setup_body` (the parent's setup statements,
    /// supplied by the checking composer) followed by every witness, so the
    /// witnesses read setup bindings and forwarded binder parameters in
    /// their own scope.
    #[must_use]
    pub fn render(&self, setup_body: &str) -> String {
        let mut out = String::from(USE_PRELUDE);
        out.push_str(&format!(
            "function {USE_SCOPE}{}() {{\n",
            self.scope_binder()
        ));
        out.push_str(setup_body);
        if !setup_body.is_empty() && !setup_body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&self.witnesses.render_witnesses());
        out.push_str("}\n");
        out
    }

    /// Use record of `use_id`.
    #[must_use]
    pub fn use_record(&self, use_id: &ComponentUseId) -> Option<&AdvancedGenericUse> {
        self.uses.iter().find(|u| u.use_id == *use_id)
    }
}

/// Place every use of `plan` in the scope of the parent's authored `binder`.
/// `witnesses` is the plan's component-use product.
#[must_use]
pub fn project_advanced_generic_uses(
    plan: &ProjectionPlan,
    witnesses: ComponentUseProjection,
    binder: Vec<PublicBinderParam>,
) -> AdvancedGenericUseProjection {
    let uses = plan
        .uses
        .iter()
        .map(|use_| AdvancedGenericUse {
            use_id: use_.id.clone(),
            availability: match witnesses.witness(&use_.id) {
                Some(witness) => UseContractAvailability::Witnessed {
                    binding: witness.binding.clone(),
                },
                None => UseContractAvailability::Unavailable,
            },
        })
        .collect();
    AdvancedGenericUseProjection {
        snapshot: witnesses.snapshot.clone(),
        complete: witnesses.complete,
        binder,
        uses,
        witnesses,
    }
}
