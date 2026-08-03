//! Unit tests for the fallthrough → props-type projection.
//!
//! Most cases here are NEGATIVE controls: a resolver answer that must widen
//! NOTHING. The positive direction is ALSO covered end-to-end against a real
//! `VerterHost` in `host_resolve_tests.rs`, where the resolver actually runs,
//! and against a real TypeScript engine in `crates/verter_tsc/tests/cases/
//! fallthrough.rs`.

use super::*;
use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, FallthroughBranch, FallthroughPropEntry, NoFallthroughReason,
    PartialBranchReason, UnresolvedBranchReason, UnresolvedRootTargetReason,
};
use verter_type_expr::facts::SourcePosition;

fn resolution(surface: FallthroughSurface) -> FallthroughResolution {
    FallthroughResolution {
        accepted_props: Vec::new(),
        accepted_events: Vec::new(),
        accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
        fallthrough_surface: surface,
        fact_versions: Vec::new(),
    }
}

fn branch(
    branch_key: &str,
    root_chain: Vec<ResolvedRootStep>,
    status: BranchStatus,
) -> FallthroughBranch {
    FallthroughBranch {
        branch_key: branch_key.to_string(),
        condition_text: None,
        props: Vec::new(),
        events: Vec::new(),
        root_chain,
        status,
    }
}

fn native(tag: &str) -> ResolvedRootStep {
    ResolvedRootStep::NativeTag {
        tag: tag.to_string(),
    }
}

fn component(canonical_id: &str, component_name: &str) -> ResolvedRootStep {
    ResolvedRootStep::Component {
        canonical_id: canonical_id.to_string(),
        component_name: component_name.to_string(),
    }
}

/// An inherited prop entry attributed to a CHILD COMPONENT — the resolver's
/// answer for "the root component declares this, so a forwarded attribute of
/// that name is consumed as a prop".
fn component_sourced_prop(name: &str, child_canonical_id: &str) -> FallthroughPropEntry {
    FallthroughPropEntry {
        name: name.to_string(),
        publication: crate::test_only::type_publication_fixture(
            SourcePosition::unannotated(),
            verter_type_expr::ResolutionExactness::ExactConcrete,
            None,
            None,
        ),
        type_source_scope: None,
        sources: vec![InheritedSource::Component {
            canonical_id: child_canonical_id.to_string(),
        }],
    }
}

/// An inherited prop entry attributed to the terminal NATIVE element. The
/// `root_tag` channel already carries the whole element props type, so these
/// must NOT be re-emitted as an explicit name list.
fn native_sourced_prop(name: &str, tag: &str) -> FallthroughPropEntry {
    FallthroughPropEntry {
        name: name.to_string(),
        publication: crate::test_only::type_publication_fixture(
            SourcePosition::unannotated(),
            verter_type_expr::ResolutionExactness::ExactConcrete,
            None,
            None,
        ),
        type_source_scope: None,
        sources: vec![InheritedSource::NativeTag {
            tag: tag.to_string(),
        }],
    }
}

/// Every child canonical maps to `("./<Stem>.vue", "default")`, mirroring an
/// owner that default-imports its root child relatively.
fn stem_specifier(child_canonical_id: &str) -> Option<(String, String)> {
    child_canonical_id
        .rsplit('/')
        .next()
        .map(|stem| (format!("./{stem}"), "default".to_string()))
}

fn project(surface: FallthroughSurface) -> FallthroughPropsProjection {
    project_fallthrough_props(Some(&resolution(surface)), &|child| stem_specifier(child))
}

/// The baseline the negatives are measured against: a single resolved native
/// root DOES widen, and names that exact tag.
#[test]
fn single_resolved_native_root_projects_that_tag() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![branch("0", vec![native("div")], BranchStatus::Resolved)],
    });

    assert_eq!(
        projection.arms,
        vec![FallthroughArm {
            root_tag: Some("div".to_string()),
            root_component_props: None,
        }],
        "a component whose template is a single native <div> root inherits that \
         element's attribute surface"
    );
}

/// `inheritAttrs: false` — the resolver reports no surface at all, and the
/// parent-facing props type must NOT be widened. This is the case the
/// Verter-owned lint had backwards.
#[test]
fn inherit_attrs_false_widens_nothing() {
    let projection = project(FallthroughSurface::None {
        reason: NoFallthroughReason::InheritAttrsFalse,
    });

    assert!(
        projection.arms.is_empty(),
        "`inheritAttrs: false` means NO inherited surface — the parent may pass \
         nothing beyond the declared props; got {:?}",
        projection.arms
    );
}

/// A root cycle resolves to an unresolved branch. Unresolved must fail toward
/// NOT widening: an unresolved branch that silently widened would be an
/// unbounded false negative.
#[test]
fn cycle_branch_widens_nothing() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![branch(
            "0",
            vec![ResolvedRootStep::Unresolved {
                tag: "component".to_string(),
                reason: UnresolvedBranchReason::Cycle {
                    canonical_id: "/src/Loop.vue".to_string(),
                },
            }],
            BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::Cycle {
                    canonical_id: "/src/Loop.vue".to_string(),
                },
            },
        )],
    });

    assert!(
        projection.arms.is_empty(),
        "a cycle is an unresolved branch and must not widen; got {:?}",
        projection.arms
    );
}

/// ONE unresolved branch poisons the WHOLE projection, not just its own arm.
/// The sibling `<div>` branch is perfectly resolvable, and widening from it
/// alone would accept attributes for a render path we cannot account for.
#[test]
fn one_unresolved_branch_zeroes_the_whole_projection() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![
            branch("0", vec![native("div")], BranchStatus::Resolved),
            branch(
                "1",
                vec![ResolvedRootStep::Unresolved {
                    tag: "component".to_string(),
                    reason: UnresolvedBranchReason::DynamicComponentIs,
                }],
                BranchStatus::Unresolved {
                    reason: UnresolvedBranchReason::DynamicComponentIs,
                },
            ),
        ],
    });

    assert!(
        projection.arms.is_empty(),
        "a resolved sibling branch must not license widening while another \
         branch is unresolved; got {:?}",
        projection.arms
    );
}

/// A chain that terminates at a COMPONENT which itself inherits nothing is a
/// RESOLVED answer, not an unresolved one: the resolver still computed that
/// component's declared props, and Vue consumes a forwarded attribute of such a
/// name as a prop before any element is involved. Dropping it made the carrier
/// reject `<Wrap tone="red"/>` while the Verter-owned lint — reading the SAME
/// resolver — accepted it.
#[test]
fn component_terminal_chain_projects_the_root_component_declared_props() {
    let mut leaf_branch = branch(
        "0",
        vec![component("/src/Leaf.vue", "Leaf")],
        BranchStatus::Resolved,
    );
    leaf_branch.props = vec![
        component_sourced_prop("tone", "/src/Leaf.vue"),
        component_sourced_prop("size", "/src/Leaf.vue"),
    ];

    let projection = project(FallthroughSurface::Branches {
        branches: vec![leaf_branch],
    });

    assert_eq!(
        projection.arms,
        vec![FallthroughArm {
            // The leaf inherits nothing of its own, so NO element attribute
            // enters through this branch…
            root_tag: None,
            // …but its declared props still consume forwarded attributes.
            root_component_props: Some(InheritedComponentProps {
                module_specifier: "./Leaf.vue".to_string(),
                export_name: "default".to_string(),
                prop_names: vec!["size".to_string(), "tone".to_string()],
            }),
        }],
        "a `BranchStatus::Resolved` component terminal carries the child's \
         declared props; discarding them is not fail-closed, it drops a \
         resolved fact"
    );
}

/// A component-terminal branch whose child declares nothing the owner has not
/// already taken is a resolved "nothing" — it widens nothing, and it does so
/// without an arm that would falsely claim a surface.
#[test]
fn component_terminal_chain_with_no_inheritable_props_widens_nothing() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![branch(
            "0",
            vec![component("/src/Leaf.vue", "Leaf")],
            BranchStatus::Resolved,
        )],
    });

    assert!(
        projection.arms.is_empty(),
        "nothing reaches anything through this branch; got {:?}",
        projection.arms
    );
}

/// A chain that traverses a component root and TERMINATES at a native element
/// projects BOTH channels: the terminal element (recursive propagation) AND the
/// intermediate component's own declared props, which consume a forwarded
/// attribute before it ever reaches that element.
#[test]
fn component_chain_terminating_at_native_projects_both_channels() {
    let mut chain_branch = branch(
        "0",
        vec![
            component("/src/Grandchild.vue", "Grandchild"),
            native("div"),
        ],
        BranchStatus::Resolved,
    );
    chain_branch.props = vec![
        component_sourced_prop("tone", "/src/Grandchild.vue"),
        // Native-sourced entries are the terminal element's OWN members; the
        // `root_tag` channel supplies them as one type reference.
        native_sourced_prop("title", "div"),
        native_sourced_prop("id", "div"),
    ];

    let projection = project(FallthroughSurface::Branches {
        branches: vec![chain_branch],
    });

    assert_eq!(
        projection.arms,
        vec![FallthroughArm {
            root_tag: Some("div".to_string()),
            root_component_props: Some(InheritedComponentProps {
                module_specifier: "./Grandchild.vue".to_string(),
                export_name: "default".to_string(),
                prop_names: vec!["tone".to_string()],
            }),
        }],
        "attributes forwarded through a component root reach BOTH the root \
         component's declared props and the terminal native element; and a \
         native-sourced name must NOT be re-listed, because the element's own \
         props type already carries it"
    );
}

/// A multi-hop chain names only the DIRECT root child. Every carrier on the
/// chain is widened by this same mechanism, so the direct child's own
/// parent-facing props already carry the deeper contributions — TypeScript
/// follows the chain so this projection does not have to reproduce it.
#[test]
fn multi_hop_chain_names_only_the_direct_root_child() {
    let mut deep = branch(
        "0",
        vec![
            component("/src/Mid.vue", "Mid"),
            component("/src/Leaf.vue", "Leaf"),
            native("div"),
        ],
        BranchStatus::Resolved,
    );
    deep.props = vec![
        component_sourced_prop("midProp", "/src/Mid.vue"),
        component_sourced_prop("leafProp", "/src/Leaf.vue"),
    ];

    let projection = project(FallthroughSurface::Branches {
        branches: vec![deep],
    });

    let arm = projection.arms.first().expect("one arm");
    let component_props = arm
        .root_component_props
        .as_ref()
        .expect("the root component channel");
    assert_eq!(
        component_props.module_specifier, "./Mid.vue",
        "only the DIRECT root child is named — the owner has no import \
         specifier for a grandchild"
    );
    assert_eq!(
        component_props.prop_names,
        vec!["leafProp".to_string(), "midProp".to_string()],
        "both hops' names are looked up THROUGH the direct child, whose own \
         carrier was widened with the deeper hop"
    );
}

/// No import specifier for the root child ⇒ the carrier cannot NAME it, so
/// that channel fails closed rather than inventing a module path.
#[test]
fn root_component_without_an_import_specifier_contributes_nothing() {
    let mut leaf_branch = branch(
        "0",
        vec![component("/src/Leaf.vue", "Leaf")],
        BranchStatus::Resolved,
    );
    leaf_branch.props = vec![component_sourced_prop("tone", "/src/Leaf.vue")];

    let projection = project_fallthrough_props(
        Some(&resolution(FallthroughSurface::Branches {
            branches: vec![leaf_branch],
        })),
        &|_| None,
    );

    assert!(
        projection.arms.is_empty(),
        "an unnameable root component must not widen; got {:?}",
        projection.arms
    );
}

/// Conditional branches contribute an arm each — the exact union of what the
/// two render paths accept.
#[test]
fn conditional_branches_project_every_terminal_tag() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![
            branch("0", vec![native("div")], BranchStatus::Resolved),
            branch("1", vec![native("a")], BranchStatus::Resolved),
        ],
    });

    assert_eq!(
        projection
            .arms
            .iter()
            .map(|arm| arm.root_tag.clone())
            .collect::<Vec<_>>(),
        vec![Some("div".to_string()), Some("a".to_string())]
    );
}

/// A PARTIALLY resolved branch still names its terminal element exactly (only
/// its member LIST is a lower bound), and the element's own props type is what
/// is widened in — so it projects normally. Distinguishes "partial" from
/// "unresolved", which do NOT behave the same here.
#[test]
fn partially_unresolved_branch_still_projects_its_terminal_tag() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![branch(
            "0",
            vec![native("div")],
            BranchStatus::PartiallyUnresolved {
                reasons: vec![PartialBranchReason::DynamicAttrName],
            },
        )],
    });

    assert_eq!(
        projection.arms,
        vec![FallthroughArm {
            root_tag: Some("div".to_string()),
            root_component_props: None,
        }]
    );
}

/// An unresolved ROOT TARGET (the resolver could not classify the root at all)
/// fails closed.
#[test]
fn unresolved_root_target_widens_nothing() {
    let projection = project(FallthroughSurface::Branches {
        branches: vec![branch(
            "0",
            vec![ResolvedRootStep::Unresolved {
                tag: "unknown".to_string(),
                reason: UnresolvedBranchReason::RootTarget {
                    reason: UnresolvedRootTargetReason::SlotOutlet,
                },
            }],
            BranchStatus::Unresolved {
                reason: UnresolvedBranchReason::RootTarget {
                    reason: UnresolvedRootTargetReason::SlotOutlet,
                },
            },
        )],
    });

    assert!(projection.arms.is_empty());
}

/// No resolver answer at all (the owner could not be resolved) widens nothing.
#[test]
fn absent_resolution_widens_nothing() {
    assert!(
        project_fallthrough_props(None, &|child| stem_specifier(child))
            .arms
            .is_empty()
    );
}

/// A `Branches` surface with no branches carries no root to inherit into.
#[test]
fn empty_branch_list_widens_nothing() {
    let projection = project(FallthroughSurface::Branches {
        branches: Vec::new(),
    });
    assert!(projection.arms.is_empty());
}
