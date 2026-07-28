//! Projection of the resolved fallthrough surface onto the PARENT-FACING
//! props type of the generated `.tsc.tsx` carrier.
//!
//! Vue forwards every attribute a component does not declare onto its single
//! root through `$attrs`, so `<Child title="hi" />` renders correctly even
//! though `Child` declares no `title` prop. Both codegen paths already model
//! the READ half of that (`$attrs` / `___VERTER___Attrs`: "what may this
//! component pull out of `$attrs`?"). Neither modelled the WRITE half — "what
//! may a parent PASS?" — which is the surface a consumer's `<Child …/>` is
//! actually checked against, and which is what
//! <https://github.com/pikax/verter/issues/97> reports as a false positive.
//!
//! Per the Fallthrough / Root Inheritance CRITICAL rule, `verter_session` owns
//! the single inheritance resolver. This module does not re-derive any of it:
//! it reads the resolver's branch structure and hands the compiler a data DTO.
//! `verter_compiler` cannot compute the surface itself — it sits BELOW this
//! crate in the dependency graph and cannot see the resolver at all.
//!
//! # The two channels a forwarded attribute can land in
//!
//! The resolver already answers BOTH questions per branch, and this projection
//! carries both, because dropping either makes the carrier disagree with the
//! resolver — and therefore with the Verter-owned lint, which reads the same
//! resolver's `accepted_props`:
//!
//! * the terminal NATIVE element ([`ResolvedRootStep::NativeTag`]) — the
//!   attribute reaches the DOM and the element's own props type accepts it;
//! * the ROOT COMPONENT's DECLARED props — the attribute is consumed as a prop
//!   before it ever reaches an element. The resolver folds these into
//!   `FallthroughBranch::props` with an
//!   [`InheritedSource::Component`] provenance, for a component-terminal chain
//!   (`inheritAttrs: false` on the leaf, a fragment leaf) AND for a chain that
//!   continues on to a native element. Both are `BranchStatus::Resolved`
//!   answers, so discarding them is not fail-closed behaviour — it is dropping
//!   a resolved fact.
//!
//! Names from an [`InheritedSource::NativeTag`] provenance are deliberately NOT
//! carried: they are exactly the members of the terminal element's own props
//! type, which the `root_tag` channel already supplies as one type reference
//! rather than a list of hundreds of names.
//!
//! Only the branch's DIRECT root component is named, even for a multi-hop
//! chain. Every carrier on the chain is widened by this same mechanism, so the
//! direct child's own parent-facing props already carry the deeper
//! contributions — TypeScript follows the chain, this projection does not have
//! to reproduce it.
//!
//! # Fail-closed direction
//!
//! Widening trades a false positive for a false negative, and the false
//! negative is worse: an attribute accepted here but not actually reaching
//! anything at runtime is an unbounded hole in prop checking. Every
//! UNCERTAINTY therefore resolves to "widen nothing":
//!
//! * `inheritAttrs: false`, a fragment / multi-root template, an empty
//!   template, a text-only root — the resolver already reports these as
//!   [`FallthroughSurface::None`].
//! * a branch the resolver could not resolve at all, INCLUDING a root cycle
//!   (which the resolver reports as
//!   [`UnresolvedBranchReason::Cycle`](verter_semantic::analysis::component_meta::UnresolvedBranchReason::Cycle))
//!   — one such branch zeroes the WHOLE projection, not just its own arm.
//! * a root component the owner has no importable reference for: without one
//!   the carrier cannot NAME the child, so the WHOLE ARM contributes nothing —
//!   not merely its component channel. Dropping only that channel would leave a
//!   surface that LOOKS widened while rejecting exactly the props the resolver
//!   proved reach the child, and the element channel it left behind could no
//!   longer subtract the child's declared keys (it needs `keyof` of a type it
//!   cannot spell), which is the collision that makes a shared key `never`.
//!   Zeroing the arm is the honest answer; sibling arms are unaffected.
//!
//! A branch that is only PARTIALLY resolved (a dynamic attribute name, an
//! unknown spread) still names its terminal target exactly, and that target's
//! own props type is what gets widened in — so those branches project normally.

use verter_compiler::tsc::{FallthroughArm, FallthroughPropsProjection, InheritedComponentProps};
use verter_semantic::analysis::component_meta::{
    BranchStatus, FallthroughBranch, FallthroughSurface, InheritedSource, ResolvedRootStep,
};

use crate::types::FallthroughResolution;

/// Project a resolved fallthrough surface onto the parent-facing props type.
///
/// `owner_reference_for_child` maps a resolved child canonical id back to the
/// `(module specifier, namespace member)` pair the OWNER's own source reaches it
/// through — the inverse of the resolver's own binding-kind + imported-name
/// walk, so a barrel re-export recovers `("./barrel", "Child")` and not a
/// fabricated `default`. It is a callback rather than a host borrow so this
/// projection stays a pure function of the resolver's answer.
///
/// The returned projection names each resolvable branch's native root element
/// (rendered as that element's real, member-typed Vue props type,
/// `IntrinsicElementAttributes[tag]`) and its root component's declared props
/// (rendered as a restricted view of that component's own carrier type). It is
/// deliberately NOT a member list and never an index signature: a name no
/// element accepts and no root component declares — `notARealThing` — stays an
/// error for the structural reason that it is not a member of either type.
pub(crate) fn project_fallthrough_props(
    resolution: Option<&FallthroughResolution>,
    owner_reference_for_child: &dyn Fn(&str) -> Option<(String, String)>,
) -> FallthroughPropsProjection {
    let Some(resolution) = resolution else {
        // The resolver could not answer for this owner at all.
        return FallthroughPropsProjection::none();
    };

    let branches = match &resolution.fallthrough_surface {
        FallthroughSurface::None { .. } => return FallthroughPropsProjection::none(),
        FallthroughSurface::Branches { branches } => branches,
    };
    if branches.is_empty() {
        return FallthroughPropsProjection::none();
    }

    let mut arms: Vec<FallthroughArm> = Vec::with_capacity(branches.len());
    for branch in branches {
        if matches!(branch.status, BranchStatus::Unresolved { .. }) {
            return FallthroughPropsProjection::none();
        }
        let root_tag = match branch.root_chain.last() {
            Some(ResolvedRootStep::NativeTag { tag }) => Some(tag.clone()),
            // A chain terminating at a component whose OWN surface is empty
            // (`inheritAttrs: false`, a fragment) reaches no element — but its
            // declared props still consume forwarded attributes, so the branch
            // is not dropped; only the element channel is.
            Some(ResolvedRootStep::Component { .. }) => None,
            // An unresolved terminal, and an empty chain, fail closed.
            _ => return FallthroughPropsProjection::none(),
        };

        let root_component_props =
            match project_root_component_props(branch, owner_reference_for_child) {
                RootComponentChannel::None => None,
                RootComponentChannel::Named(props) => Some(props),
                // The resolver proved this branch's root component consumes named
                // props, but the owner reaches that component through no importable
                // binding, so the carrier cannot name it. The whole arm is dropped:
                // keeping only the element channel would publish a surface that
                // reads as widened while rejecting exactly those props, and that
                // element channel could not subtract the child's declared keys
                // either, so a shared key would collide to `never`.
                RootComponentChannel::UnnameableComponent => {
                    arms.push(FallthroughArm::default());
                    continue;
                }
            };

        if root_tag.is_none() && root_component_props.is_none() {
            // A resolved branch that genuinely contributes nothing. It must not
            // be dropped from `arms`, because an arm-less projection would read
            // as "widen nothing at all" for its resolved siblings too — but it
            // has no members of its own to intersect in.
            arms.push(FallthroughArm::default());
            continue;
        }

        arms.push(FallthroughArm {
            root_tag,
            root_component_props,
        });
    }

    // Every branch resolved to "nothing" — there is nothing to widen with.
    if arms
        .iter()
        .all(|arm| arm.root_tag.is_none() && arm.root_component_props.is_none())
    {
        return FallthroughPropsProjection::none();
    }

    FallthroughPropsProjection { arms }
}

/// The outcome of projecting one branch's declared-prop channel.
///
/// Three states, not two: "this branch has no component channel" and "this
/// branch HAS one that cannot be spelled" are different answers, and collapsing
/// them is what let an unnameable root component degrade silently.
enum RootComponentChannel {
    /// No component root, or no name the resolver attributed to a component.
    None,
    /// The channel, with the reference the carrier will name it by.
    Named(InheritedComponentProps),
    /// A component root DOES consume named props, but the owner reaches it
    /// through no importable binding — the arm must contribute nothing.
    UnnameableComponent,
}

/// The DIRECT root component of `branch`, plus the inherited prop names the
/// resolver attributed to a component (rather than to a native element).
fn project_root_component_props(
    branch: &FallthroughBranch,
    owner_reference_for_child: &dyn Fn(&str) -> Option<(String, String)>,
) -> RootComponentChannel {
    let Some(ResolvedRootStep::Component { canonical_id, .. }) = branch.root_chain.first() else {
        return RootComponentChannel::None;
    };

    let mut prop_names: Vec<String> = branch
        .props
        .iter()
        .filter(|prop| {
            prop.sources
                .iter()
                .any(|source| matches!(source, InheritedSource::Component { .. }))
        })
        .map(|prop| prop.name.clone())
        .collect();
    if prop_names.is_empty() {
        return RootComponentChannel::None;
    }
    prop_names.sort();
    prop_names.dedup();

    // No importable reference ⇒ the carrier cannot name the child. Report that
    // distinctly rather than inventing a path or quietly returning "no channel".
    let Some((module_specifier, export_name)) = owner_reference_for_child(canonical_id) else {
        return RootComponentChannel::UnnameableComponent;
    };

    RootComponentChannel::Named(InheritedComponentProps {
        module_specifier,
        export_name,
        prop_names,
    })
}

#[cfg(test)]
#[path = "fallthrough_props_tests.rs"]
mod fallthrough_props_tests;
