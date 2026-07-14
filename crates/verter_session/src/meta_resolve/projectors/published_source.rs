//! Published-SOURCE upgrades for reduced publication nodes.
//!
//! The content-free `SemanticTypeSource` a publication terminal emits for a
//! reduced node: the closed leaf / leaf-union upgrades shared by every
//! published surface, plus the member-sink REF-IDENTITY upgrade. Pure
//! node-domain projections — no reduction, no dispatch execution, no
//! `TypeExpr` materialisation.

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::SemanticNodeId;

/// The publication SOURCE for a reduced node: the complete closed LEAF fact
/// when the node decided one, the closed LEAF-UNION fact when the node is a
/// union of complete leaves, otherwise the caller's `existing` source
/// unchanged (never a fabricated stand-in).
pub(super) fn published_source_for_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: Option<SemanticNodeId>,
    existing: verter_type_expr::facts::SemanticTypeSource,
) -> verter_type_expr::facts::SemanticTypeSource {
    published_source_upgrade_for_node(dispatch, node).unwrap_or(existing)
}

/// The closed-leaf half of [`published_source_for_node`] as an UPGRADE
/// decision: `Some(closed fact)` when the reduced node decided a complete
/// leaf / leaf-union, else `None` — the caller decides what its position
/// publishes when no upgrade exists (its authored source, a proven absence,
/// or a typed required-position failure; never a fabricated stand-in).
pub(super) fn published_source_upgrade_for_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: Option<SemanticNodeId>,
) -> Option<verter_type_expr::facts::SemanticTypeSource> {
    match node.and_then(|node| dispatch.node_leaf_fact(node)) {
        Some(leaf) => Some(verter_type_expr::facts::SemanticTypeSource::Closed(
            verter_type_expr::facts::ClosedTypeFact::Leaf(leaf),
        )),
        // A reduced UNION whose members are all complete leaves is a
        // decided closed result (`"a" | "c"` from a fully-closed
        // distributive utility) — publish the closed leaf-union fact
        // (the shared dispatch-owned node→closed-union projection).
        None => node
            .and_then(|node| dispatch.node_leaf_union_fact(node))
            .map(|leaves| {
                verter_type_expr::facts::SemanticTypeSource::Closed(
                    verter_type_expr::facts::ClosedTypeFact::LeafUnion(leaves),
                )
            }),
    }
}

/// The publication SOURCE for a reduced surface-member value at the macro
/// publication sink: extends [`published_source_for_node`]'s leaf upgrade with
/// the REF-IDENTITY upgrade — a member value the publication reduce resolved
/// to a declaration-identity carrier publishes the content-free shallow
/// symbol-reference source (`Synthesized(Ref(SymbolBodyLocator))`, the
/// shallow-by-default "published type stays a `Ref` carrier" shape — consumers
/// re-resolve the named root on demand through the one shared dispatch).
///
/// Losslessness gate: a plain `DeclRef` upgrade is LOSSLESS (name + declaring
/// canonical, no type arguments) and applies unconditionally — the
/// navigate-terminal contract ("the declaration reference survives to the
/// published surface"). An `InstantiationRef` upgrade DROPS the instantiation's
/// type arguments, so it applies only when `include_lossy_instantiation` is set
/// — the payload-less resolved-surface member case (an imported /
/// heritage-reached macro type argument has NO flat authored macro-payload
/// position; without the upgrade its publication degraded to the Unknown leaf
/// and DESTROYED the resolvable reference) whose arg-preserving authored
/// use-site slot was NOT recoverable (see
/// [`crate::meta_resolve::arg_preserving_member_use_site_slot`] — the
/// preferred, substitution-preserving publication). An authored position keeps
/// its authored source instead (the raise reproduces the full instantiation,
/// arguments included — e.g. the L1 `Pick<Source, Keys>` carrier keeps BOTH
/// type arguments). Any other shape (an L1 carrier-stop utility root, an open
/// mapped surface, an unresolved `BareRef`) keeps `existing` — never a
/// fabricated locator minted from a node.
/// The member-sink publication upgrade: `Some` when
/// the reduced member value decided a complete closed leaf / leaf-union or a
/// declaration-identity carrier, else `None` — the member sink decides what
/// the position publishes when no upgrade exists (its authored source, or —
/// for a REQUIRED payload position with no authored source — the typed
/// source-construction failure; never a fabricated stand-in).
pub(super) fn published_member_source_upgrade_for_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: Option<SemanticNodeId>,
    include_lossy_instantiation: bool,
) -> Option<verter_type_expr::facts::SemanticTypeSource> {
    use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace, SymbolBodyLocator};

    let node = node?;
    if let Some(leaf) = dispatch.node_leaf_fact(node) {
        return Some(verter_type_expr::facts::SemanticTypeSource::Closed(
            verter_type_expr::facts::ClosedTypeFact::Leaf(leaf),
        ));
    }
    if let Some(leaves) = dispatch.node_leaf_union_fact(node) {
        return Some(verter_type_expr::facts::SemanticTypeSource::Closed(
            verter_type_expr::facts::ClosedTypeFact::LeafUnion(leaves),
        ));
    }
    let identity =
        match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
            Some(crate::semantic_query::SemanticNodeData::DeclRef { identity }) => {
                Some(identity.clone())
            }
            Some(crate::semantic_query::SemanticNodeData::InstantiationRef { base, .. })
                if include_lossy_instantiation =>
            {
                Some(base.clone())
            }
            _ => None,
        };
    match identity {
        // Content-free: canonical + symbol only — the whole-hash is a version,
        // never part of a published source.
        Some(identity) if !identity.canonical_id.is_empty() => {
            Some(verter_type_expr::facts::SemanticTypeSource::Synthesized(
                verter_type_expr::facts::ResolvedLocalShape::Ref(SymbolBodyLocator {
                    anchor: AuthoredAnchor {
                        canonical_id: std::sync::Arc::clone(&identity.canonical_id),
                        symbol: std::sync::Arc::clone(&identity.decl_name),
                        space: LocatorSymbolSpace::Type,
                    },
                }),
            ))
        }
        _ => None,
    }
}

/// The REQUIREMENT class of a published surface member's VALUE position —
/// the classification a projector supplies so the member sink can type a
/// no-faithful-source residue correctly. A single class remains: the shallow
/// published member value, whose GENUINE-miss residue fails typed as the
/// required-member-value failure (a fabricated `unknown` success is
/// forbidden) — a KNOWN structural value publishes the projected
/// member-path replay route instead (see
/// [`structural_member_value_source`]). The former REQUIRED-payload class
/// is gone: emit payload sources are owned by the normalized
/// `ResolvedEmitField.payload_source` rows (closed tuple / member-path /
/// callable-params replay), so the flat emit member rows classify as
/// shallow members like every other surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberValuePosition {
    /// A shallow published member value (props / emits-metadata / exposed /
    /// options / slots members): a genuine no-faithful-source miss ⇒ the
    /// typed member-value failure.
    ShallowMember,
}

/// The faithful PRESENT structural source for a shallow published member
/// value with NO authored slot, NO use-site slot, and NO closed/ref upgrade
/// on its published node: demand-validate the value node through the shared
/// [`ProjectSemanticDispatch::demand_validated_structural_node`] (the same
/// per-root validation the callable-params replay applies to its payload
/// parameters), re-run the closed-leaf /
/// leaf-union / declaration-identity upgrades on the DEMANDED node (a
/// resolvable reference publishes its shallow symbol-reference carrier —
/// shallow-by-default), and publish the projected MEMBER-PATH replay route
/// ([`verter_type_expr::facts::ProjectedTypeFact::MemberPath`] — the macro's
/// STAMPED type-argument base + the member name, replayed through the one
/// dispatch's EXISTING `ProjectPath` query on demand) for every remaining
/// KNOWN structural shape (a function / inline object / rich tuple / array /
/// composite / instantiation). The published element stays SHALLOW — the
/// consumer re-resolves the replay address on demand; nothing is flattened
/// eagerly here.
///
/// `None` ONLY on a genuine miss (an unresolvable residual carrier, an
/// unknown-materializing failure, no live node data) or with NO stamped
/// type-argument base to replay off — the caller types the REQUIRED
/// position's source-construction failure; never a fabricated `unknown`
/// success and never a fabricated locator.
pub(crate) fn structural_member_value_source(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    member_name: &str,
    type_arg_base: Option<&verter_type_expr::locators::MacroPayloadLocator>,
) -> Option<verter_type_expr::facts::SemanticTypeSource> {
    // STRUCTURAL TRANSIT, not `Published`: validation is a carrier-preserving
    // classification, never consumer demand — `may_reduce_operator` must stay
    // `false` at every nested dispatch so a utility/mapped-produced member
    // value keeps its carrier deferred (a `Published` demand here enumerated
    // inherited library members into the audit derivation subgraph — the
    // Rule-5 leak the `block_6i` guards pin closed).
    let normalized = dispatch.demand_validated_structural_node(
        node,
        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        ),
    )?;
    // The demanded node may NOW carry a closed / declaration-identity
    // upgrade the published node did not (an unresolved `BareRef` that the
    // demand resolved to its `DeclRef` identity publishes the shallow
    // symbol-reference carrier, never a replay route). Lossy instantiation
    // is excluded: an argument-bearing instantiation publishes the
    // arg-preserving member-path replay below instead.
    if let Some(upgraded) =
        published_member_source_upgrade_for_node(dispatch, Some(normalized), false)
    {
        return Some(upgraded);
    }
    let base = type_arg_base?;
    Some(verter_type_expr::facts::SemanticTypeSource::Projected(
        verter_type_expr::facts::ProjectedTypeFact::MemberPath {
            base: verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(base.clone()),
            path: std::sync::Arc::from(vec![member_name.to_string()].into_boxed_slice()),
        },
    ))
}
