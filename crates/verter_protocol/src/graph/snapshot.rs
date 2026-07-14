//! Sealed post-resolution type-graph snapshot DTOs — the `NoTypeExpr`
//! OUTPUT representation of a resolved JSDoc `{Type}` tag payload.
//!
//! A producer resolves the tag's transient symbolic IR ONCE, walks it into a
//! private [`GraphBuilder`] (the same wire-node builder every proto graph
//! rides), and captures the resulting mini-arena as a
//! [`ResolvedTypeGraphSnapshot`] — strings + PERSISTED wire nodes + root id.
//! The transient `TypeExpr` is discarded at that boundary; every later
//! consumer (the session component-meta cache value, the FFI JSON DTO, the
//! proto conversion) carries ONLY this sealed snapshot. The proto conversion
//! re-interns the snapshot into the live output graph via
//! [`GraphBuilder::append_snapshot`], which is wire-identical to the retired
//! direct `node_id(&TypeExpr)` walk.
//!
//! Capture is VALIDATED and fail-closed: the node vocabulary is the closed
//! crate-private [`PersistedGraphNode`] enum (the live-wire-only
//! [`GraphNode::SyntheticSlotBinding`] carrier is structurally excluded),
//! and the module-private `ResolvedTypeGraphSnapshot::try_new` — the sole
//! construction funnel — verifies whole-snapshot well-formedness (non-zero
//! in-range root, in-range children-before-parents node refs, in-range
//! string refs) up front. The snapshot fields are module-private too, so
//! the seal is COMPILER-ENFORCED: the sole constructor reachable outside
//! this module — in-crate included — is
//! [`ResolvedTypeGraphSnapshot::from_builder`], and every persisted node
//! any other module or crate can obtain is `GraphBuilder`-sourced: scalar
//! enum tags (primitive / member kind / mapped modifier / conditional
//! branch) are valid by construction, and there is no hand-built node
//! injection surface. A snapshot that exists is valid by construction.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::graph::builder::{
    GraphBuilder, GraphConditionalFrame, GraphFunctionParam, GraphNode, GraphObjectMember,
    GraphTupleElement,
};

/// Content-derived stable identity of one captured snapshot — a structural
/// hash over the snapshot's `(strings, nodes, root_node_id)` tables (their
/// `Hash` impls, never a debug rendering). Two snapshots of the same resolved
/// graph carry the same key; perturbing any node, string, or the root flips
/// it. In-memory identity only (the owning caches are in-memory warm maps) —
/// not a persisted/serde-parity contract.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr,
)]
pub(crate) struct TypeGraphSnapshotKey(pub [u8; 16]);

/// The CLOSED persisted-snapshot node vocabulary: every [`GraphNode`] variant
/// EXCEPT the live-wire-only `SyntheticSlotBinding` carrier.
///
/// A captured [`ResolvedTypeGraphSnapshot`] outlives the semantic-graph
/// generation that produced it (it persists at the session component-meta
/// cache value and serialises through the FFI JSON DTO), while a
/// `SyntheticSlotBinding` carries a generation-LOCAL session semantic id
/// (`value_node`) whose meaning does not survive that boundary. Excluding
/// the carrier from this enum makes the exclusion type-level: a persisted
/// snapshot structurally CANNOT hold one, and [`try_persist`] fails closed
/// at capture instead of silently persisting a dangling id.
///
/// Variant names, field names, field types, and the serde container
/// attributes MIRROR [`GraphNode`] exactly, so the serialized JSON of every
/// shared variant is byte-identical to the live wire node's. Node / string
/// child ids stay `u32`, 1-based, with 0 = absent — the field-by-field
/// node-ref vs string-ref spaces are the same as [`GraphNode`]'s.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub(crate) enum PersistedGraphNode {
    Primitive {
        primitive: u32,
    },
    LiteralString {
        value: u32,
    },
    LiteralNumber {
        bits: u64,
    },
    LiteralBoolean {
        value: bool,
    },
    LiteralBigInt {
        value: u32,
    },
    Union {
        types: Vec<u32>,
    },
    Intersection {
        types: Vec<u32>,
    },
    Array {
        element: u32,
        readonly: bool,
    },
    Tuple {
        readonly: bool,
        elements: Vec<GraphTupleElement>,
    },
    Object {
        members: Vec<GraphObjectMember>,
    },
    Function {
        parameters: Vec<GraphFunctionParam>,
        return_type: u32,
        type_parameters: Vec<u32>,
    },
    Ref {
        name: u32,
        type_arguments: Vec<u32>,
    },
    TypeParameter {
        name: u32,
        constraint: u32,
        default: u32,
    },
    KeyOf {
        operand: u32,
    },
    TypeOf {
        path: Vec<u32>,
    },
    IndexedAccess {
        object: u32,
        index: u32,
    },
    Conditional {
        check: u32,
        extends: u32,
        true_type: u32,
        false_type: u32,
    },
    Mapped {
        parameter: u32,
        source: u32,
        value: u32,
        optional: u32,
        readonly: u32,
        name_type: u32,
    },
    TemplateLiteral {
        quasis: Vec<u32>,
        expressions: Vec<u32>,
    },
    Parenthesized {
        inner: u32,
    },
    Unknown {
        raw: u32,
    },
    Infer {
        name: u32,
    },
    Rest {
        inner: u32,
    },
    RecursiveRef {
        name: u32,
        type_arguments: Vec<u32>,
        conditional_context: Vec<GraphConditionalFrame>,
    },
}

/// Why a snapshot could not be captured / constructed. Fail-closed and
/// typed: capture NEVER silently drops, coerces, or clamps an offending
/// node or reference — the whole snapshot is refused with the exact reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCaptureError {
    /// The captured graph contains a node outside the closed persisted
    /// vocabulary (the live-wire-only `SyntheticSlotBinding` carrier, whose
    /// `value_node` is a generation-local session semantic id that must not
    /// outlive its generation).
    NonPersistableNode,
    /// `root_node_id` is 0 (absent) — a captured snapshot always has a root.
    RootNodeIdZero,
    /// `root_node_id` points past the node table.
    RootNodeIdOutOfRange,
    /// A node child-reference points past the node table.
    NodeRefOutOfRange,
    /// A node child-reference is not strictly children-before-parents: it
    /// names its own node or a later one (a forward / self reference a real
    /// `GraphBuilder` capture can never produce).
    NodeRefNotChildBeforeParent,
    /// A string reference points past the string table.
    StringRefOutOfRange,
}

/// Map one live wire node into the closed persisted vocabulary — a TOTAL
/// exhaustive 1:1 mapping for every persistable variant; the live-wire-only
/// `SyntheticSlotBinding` carrier fails closed with
/// [`SnapshotCaptureError::NonPersistableNode`] (never silently dropped or
/// coerced).
///
/// CONSUMES the wire node: every payload (`Vec`s, sub-structs) MOVES into
/// the persisted node, so capture at the output boundary
/// ([`ResolvedTypeGraphSnapshot::from_builder`]) clones nothing.
fn try_persist(node: GraphNode) -> Result<PersistedGraphNode, SnapshotCaptureError> {
    Ok(match node {
        GraphNode::Primitive { primitive } => PersistedGraphNode::Primitive { primitive },
        GraphNode::LiteralString { value } => PersistedGraphNode::LiteralString { value },
        GraphNode::LiteralNumber { bits } => PersistedGraphNode::LiteralNumber { bits },
        GraphNode::LiteralBoolean { value } => PersistedGraphNode::LiteralBoolean { value },
        GraphNode::LiteralBigInt { value } => PersistedGraphNode::LiteralBigInt { value },
        GraphNode::Union { types } => PersistedGraphNode::Union { types },
        GraphNode::Intersection { types } => PersistedGraphNode::Intersection { types },
        GraphNode::Array { element, readonly } => PersistedGraphNode::Array { element, readonly },
        GraphNode::Tuple { readonly, elements } => PersistedGraphNode::Tuple { readonly, elements },
        GraphNode::Object { members } => PersistedGraphNode::Object { members },
        GraphNode::Function {
            parameters,
            return_type,
            type_parameters,
        } => PersistedGraphNode::Function {
            parameters,
            return_type,
            type_parameters,
        },
        GraphNode::Ref {
            name,
            type_arguments,
        } => PersistedGraphNode::Ref {
            name,
            type_arguments,
        },
        GraphNode::TypeParameter {
            name,
            constraint,
            default,
        } => PersistedGraphNode::TypeParameter {
            name,
            constraint,
            default,
        },
        GraphNode::KeyOf { operand } => PersistedGraphNode::KeyOf { operand },
        GraphNode::TypeOf { path } => PersistedGraphNode::TypeOf { path },
        GraphNode::IndexedAccess { object, index } => {
            PersistedGraphNode::IndexedAccess { object, index }
        }
        GraphNode::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => PersistedGraphNode::Conditional {
            check,
            extends,
            true_type,
            false_type,
        },
        GraphNode::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => PersistedGraphNode::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        },
        GraphNode::TemplateLiteral {
            quasis,
            expressions,
        } => PersistedGraphNode::TemplateLiteral {
            quasis,
            expressions,
        },
        GraphNode::Parenthesized { inner } => PersistedGraphNode::Parenthesized { inner },
        GraphNode::Unknown { raw } => PersistedGraphNode::Unknown { raw },
        GraphNode::Infer { name } => PersistedGraphNode::Infer { name },
        GraphNode::Rest { inner } => PersistedGraphNode::Rest { inner },
        GraphNode::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => PersistedGraphNode::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        },
        GraphNode::SyntheticSlotBinding { .. } => {
            return Err(SnapshotCaptureError::NonPersistableNode)
        }
    })
}

/// A self-contained wire-node mini-arena captured from a [`GraphBuilder`]:
/// the string table, the PERSISTED node table (both 1-based; id 0 = absent),
/// and the root node id of the captured type. `NoTypeExpr` — the snapshot
/// owns persisted wire nodes, never symbolic IR.
///
/// Construction is SEALED and COMPILER-ENFORCED: the fields and the
/// validating `Self::try_new` funnel (whole-snapshot validation) are
/// module-private, so [`Self::from_builder`] (capture + validation) is the
/// sole constructor reachable outside this module — in-crate included.
/// Every snapshot that exists is well-formed by construction — non-zero
/// in-range root, children-before-parents in-range node refs, in-range
/// string refs — and `GraphBuilder`-sourced (scalar enum tags valid by
/// construction; no hand-built node injection surface).
#[derive(Debug, Clone, PartialEq, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTypeGraphSnapshot {
    /// The captured 1-based string table (`strings[0]` is string id 1).
    strings: Arc<[String]>,
    /// The captured 1-based persisted-node table (`nodes[0]` is node id 1),
    /// in the builder's children-before-parents intern order.
    nodes: Arc<[PersistedGraphNode]>,
    /// The captured type's root node id (1-based; validated non-zero and
    /// in-range).
    root_node_id: u32,
    /// The content-derived stable identity of this snapshot. In-memory
    /// identity ONLY (see [`TypeGraphSnapshotKey`]) — `#[serde(skip)]` keeps
    /// it out of the serialized snapshot bytes (the FFI JSON DTO).
    #[serde(skip)]
    stable_key: TypeGraphSnapshotKey,
}

impl ResolvedTypeGraphSnapshot {
    /// The SOLE validated constructor: verify whole-snapshot
    /// well-formedness UP FRONT, then compute the content-derived
    /// [`TypeGraphSnapshotKey`] and construct.
    ///
    /// Module-private ON PURPOSE: construction outside this module —
    /// in-crate included — enters only through [`Self::from_builder`], so
    /// the persisted tables any other module or crate can obtain are always
    /// `GraphBuilder`-sourced — the scalar enum TAG domains (primitive /
    /// member kind / mapped modifier / conditional branch) are valid by
    /// construction and need no re-validation here.
    ///
    /// Validation predicates (each failure is the exact typed
    /// [`SnapshotCaptureError`] variant, with NO per-reference coercion):
    ///
    /// - `root_node_id != 0` ([`SnapshotCaptureError::RootNodeIdZero`]);
    /// - `root_node_id <= nodes.len()`
    ///   ([`SnapshotCaptureError::RootNodeIdOutOfRange`]);
    /// - for the node at 1-based index `i`, every NODE child-ref `r` is 0
    ///   (absent) or in `1..=nodes.len()` with `r < i`
    ///   (children-before-parents): `r > nodes.len()` is
    ///   [`SnapshotCaptureError::NodeRefOutOfRange`], an in-range `r >= i`
    ///   is [`SnapshotCaptureError::NodeRefNotChildBeforeParent`];
    /// - every STRING child-ref is 0 or `<= strings.len()`
    ///   ([`SnapshotCaptureError::StringRefOutOfRange`]).
    fn try_new(
        strings: Vec<String>,
        nodes: Vec<PersistedGraphNode>,
        root_node_id: u32,
    ) -> Result<Self, SnapshotCaptureError> {
        if root_node_id == 0 {
            return Err(SnapshotCaptureError::RootNodeIdZero);
        }
        if root_node_id as usize > nodes.len() {
            return Err(SnapshotCaptureError::RootNodeIdOutOfRange);
        }
        for (index0, node) in nodes.iter().enumerate() {
            validate_node_refs(node, index0 + 1, nodes.len(), strings.len())?;
        }
        let strings: Arc<[String]> = strings.into();
        let nodes: Arc<[PersistedGraphNode]> = nodes.into();
        let stable_key = snapshot_stable_key(&strings, &nodes, root_node_id);
        Ok(Self {
            strings,
            nodes,
            root_node_id,
            stable_key,
        })
    }

    /// Capture `builder`'s tables as a validated snapshot rooted at
    /// `root_node_id`: persist every wire node through the module-private
    /// `try_persist` (fail-closed on the first non-persistable node — never
    /// a partial snapshot; the owned nodes MOVE into their persisted
    /// mirrors, no clone), then construct through the module-private
    /// `Self::try_new`.
    pub fn from_builder(
        builder: GraphBuilder,
        root_node_id: u32,
    ) -> Result<Self, SnapshotCaptureError> {
        let (strings, nodes) = builder.into_tables();
        let nodes = nodes
            .into_iter()
            .map(try_persist)
            .collect::<Result<Vec<_>, _>>()?;
        Self::try_new(strings, nodes, root_node_id)
    }

    /// The captured 1-based string table — read-only, for the widening
    /// remap ([`GraphBuilder::append_snapshot`]).
    pub(crate) fn strings(&self) -> &[String] {
        &self.strings
    }

    /// The captured 1-based persisted-node table in children-before-parents
    /// intern order — read-only, for the widening remap.
    pub(crate) fn nodes(&self) -> &[PersistedGraphNode] {
        &self.nodes
    }

    /// The captured type's root node id (1-based; validated non-zero and
    /// in-range at construction).
    pub(crate) fn root_node_id(&self) -> u32 {
        self.root_node_id
    }
}

/// The sealed OUTPUT value of a resolved JSDoc `{Type}` tag payload: an
/// optional rendered display string plus the captured wire-node graph
/// snapshot. This is the value the session cache retains, the FFI JSON
/// serialises, and the proto conversion re-interns — no `TypeExpr` survives
/// past the producer boundary, and there is no unwrap back to one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, verter_no_typeexpr::NoTypeExpr)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedJsdocTypeOutput {
    /// A rendered display string of the resolved type, when one renders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    /// The captured wire-node graph snapshot of the resolved type.
    pub graph: ResolvedTypeGraphSnapshot,
}

/// Validate one node child-reference against the node table under the
/// children-before-parents ordering: 0 (absent) is allowed; range is checked
/// FIRST (a wild id past the table reports
/// [`SnapshotCaptureError::NodeRefOutOfRange`]); an in-range reference to the
/// node itself or a later one reports
/// [`SnapshotCaptureError::NodeRefNotChildBeforeParent`].
fn check_node_ref(
    reference: u32,
    parent_index_1based: usize,
    nodes_len: usize,
) -> Result<(), SnapshotCaptureError> {
    if reference == 0 {
        return Ok(());
    }
    if reference as usize > nodes_len {
        return Err(SnapshotCaptureError::NodeRefOutOfRange);
    }
    if reference as usize >= parent_index_1based {
        return Err(SnapshotCaptureError::NodeRefNotChildBeforeParent);
    }
    Ok(())
}

/// Validate one string reference against the string table: 0 (absent) or an
/// in-range 1-based id; anything past the table reports
/// [`SnapshotCaptureError::StringRefOutOfRange`]. Strings are a flat table,
/// so there is no ordering constraint.
fn check_string_ref(reference: u32, strings_len: usize) -> Result<(), SnapshotCaptureError> {
    if reference == 0 || reference as usize <= strings_len {
        Ok(())
    } else {
        Err(SnapshotCaptureError::StringRefOutOfRange)
    }
}

/// Validate every child reference of one persisted node — the field-by-field
/// node-ref vs string-ref classification mirrors the widening remap
/// (`remap_snapshot_node`)'s `nid` / `sid` split exactly.
///
/// EXHAUSTIVE over [`PersistedGraphNode`] (no wildcard): adding a persisted
/// node variant fails compilation here until its reference spaces are stated
/// explicitly.
fn validate_node_refs(
    node: &PersistedGraphNode,
    parent_index_1based: usize,
    nodes_len: usize,
    strings_len: usize,
) -> Result<(), SnapshotCaptureError> {
    let nid = |reference: u32| check_node_ref(reference, parent_index_1based, nodes_len);
    let sid = |reference: u32| check_string_ref(reference, strings_len);
    match node {
        // Tag / raw-payload-only variants carry no table references.
        PersistedGraphNode::Primitive { .. }
        | PersistedGraphNode::LiteralNumber { .. }
        | PersistedGraphNode::LiteralBoolean { .. } => Ok(()),
        PersistedGraphNode::LiteralString { value }
        | PersistedGraphNode::LiteralBigInt { value } => sid(*value),
        PersistedGraphNode::Union { types } | PersistedGraphNode::Intersection { types } => {
            types.iter().try_for_each(|ty| nid(*ty))
        }
        PersistedGraphNode::Array { element, .. } => nid(*element),
        PersistedGraphNode::Tuple { elements, .. } => elements.iter().try_for_each(|element| {
            sid(element.label)?;
            nid(element.ty)
        }),
        PersistedGraphNode::Object { members } => members.iter().try_for_each(|member| {
            sid(member.name)?;
            nid(member.ty)?;
            sid(member.key_name)?;
            nid(member.key_type)?;
            nid(member.value_type)?;
            nid(member.function)
        }),
        PersistedGraphNode::Function {
            parameters,
            return_type,
            type_parameters,
        } => {
            parameters.iter().try_for_each(|param| {
                sid(param.name)?;
                nid(param.ty)
            })?;
            nid(*return_type)?;
            type_parameters.iter().try_for_each(|param| nid(*param))
        }
        PersistedGraphNode::Ref {
            name,
            type_arguments,
        } => {
            sid(*name)?;
            type_arguments.iter().try_for_each(|arg| nid(*arg))
        }
        PersistedGraphNode::TypeParameter {
            name,
            constraint,
            default,
        } => {
            sid(*name)?;
            nid(*constraint)?;
            nid(*default)
        }
        PersistedGraphNode::KeyOf { operand } => nid(*operand),
        PersistedGraphNode::TypeOf { path } => path.iter().try_for_each(|segment| sid(*segment)),
        PersistedGraphNode::IndexedAccess { object, index } => {
            nid(*object)?;
            nid(*index)
        }
        PersistedGraphNode::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            nid(*check)?;
            nid(*extends)?;
            nid(*true_type)?;
            nid(*false_type)
        }
        PersistedGraphNode::Mapped {
            parameter,
            source,
            value,
            name_type,
            ..
        } => {
            sid(*parameter)?;
            nid(*source)?;
            nid(*value)?;
            nid(*name_type)
        }
        PersistedGraphNode::TemplateLiteral {
            quasis,
            expressions,
        } => {
            quasis.iter().try_for_each(|quasi| sid(*quasi))?;
            expressions.iter().try_for_each(|expr| nid(*expr))
        }
        PersistedGraphNode::Parenthesized { inner } | PersistedGraphNode::Rest { inner } => {
            nid(*inner)
        }
        PersistedGraphNode::Unknown { raw } => sid(*raw),
        PersistedGraphNode::Infer { name } => sid(*name),
        PersistedGraphNode::RecursiveRef {
            name,
            type_arguments,
            conditional_context,
        } => {
            sid(*name)?;
            type_arguments.iter().try_for_each(|arg| nid(*arg))?;
            conditional_context.iter().try_for_each(|frame| {
                nid(frame.check)?;
                nid(frame.extends)
            })
        }
    }
}

/// Structural content hash over the snapshot tables (their `Hash` impls; the
/// deterministic `FxHasher`, matching the workspace's stable content-key
/// convention).
fn snapshot_stable_key(
    strings: &[String],
    nodes: &[PersistedGraphNode],
    root_node_id: u32,
) -> TypeGraphSnapshotKey {
    let mut hasher = rustc_hash::FxHasher::default();
    strings.hash(&mut hasher);
    nodes.hash(&mut hasher);
    root_node_id.hash(&mut hasher);
    let h = hasher.finish();
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&h.to_le_bytes());
    out[8..].copy_from_slice(&h.rotate_left(17).to_le_bytes());
    TypeGraphSnapshotKey(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use static_assertions::assert_impl_all;
    use verter_no_typeexpr::NoTypeExpr;
    use verter_type_expr::{PrimitiveName, TypeExpr};

    /// Compile-time witnesses: the sealed snapshot family owns NO transitive
    /// `TypeExpr` (fires NOW — `verter_protocol` compiles on this tree).
    #[test]
    fn snapshot_output_family_is_no_type_expr() {
        assert_impl_all!(TypeGraphSnapshotKey: NoTypeExpr);
        assert_impl_all!(ResolvedTypeGraphSnapshot: NoTypeExpr);
        assert_impl_all!(ResolvedJsdocTypeOutput: NoTypeExpr);
        assert_impl_all!(GraphNode: NoTypeExpr);
        assert_impl_all!(PersistedGraphNode: NoTypeExpr);
    }

    fn snapshot_of(expr: &TypeExpr) -> ResolvedTypeGraphSnapshot {
        let mut builder = GraphBuilder::new();
        let root = builder.node_id(expr);
        ResolvedTypeGraphSnapshot::from_builder(builder, root)
            .expect("valid non-synthetic snapshot")
    }

    /// The stable key is deterministic for equal content and DISCRIMINATES on
    /// every content axis (nodes, strings, root).
    #[test]
    fn stable_key_is_deterministic_and_discriminating() {
        let a = snapshot_of(&TypeExpr::Ref {
            name: "Props".into(),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        });
        let b = snapshot_of(&TypeExpr::Ref {
            name: "Props".into(),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        });
        assert_eq!(a.stable_key, b.stable_key, "equal content ⇒ equal key");
        assert_eq!(a, b, "equal content ⇒ equal snapshot");

        // A different referenced NAME (string table) flips the key.
        let c = snapshot_of(&TypeExpr::Ref {
            name: "Other".into(),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        });
        assert_ne!(a.stable_key, c.stable_key, "string content folds in");

        // A different node SHAPE flips the key.
        let d = snapshot_of(&TypeExpr::Primitive(PrimitiveName::String));
        assert_ne!(a.stable_key, d.stable_key, "node content folds in");
    }

    /// `stable_key` is IN-MEMORY identity only (its doc: "not a
    /// persisted/serde-parity contract") — it must never leak into the
    /// snapshot's serialized bytes (the FFI JSON DTO). Discriminating:
    /// removing the field's `#[serde(skip)]` puts `"stableKey"` back into
    /// the JSON and fails this test.
    #[test]
    fn stable_key_never_serializes_into_snapshot_bytes() {
        let snapshot = snapshot_of(&TypeExpr::Ref {
            name: "Props".into(),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        });
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        assert!(
            !json.contains("stableKey"),
            "the in-memory stable key must not serialize (camelCase spelling): {json}"
        );
        assert!(
            !json.contains("stable_key"),
            "the in-memory stable key must not serialize (snake_case spelling): {json}"
        );
        // The genuinely persisted fields still serialize.
        assert!(
            json.contains("\"strings\"")
                && json.contains("\"nodes\"")
                && json.contains("\"rootNodeId\""),
            "the persisted table fields must keep serializing: {json}"
        );
    }

    /// One representative live wire node per PERSISTABLE variant, paired
    /// with its expected persisted mirror. Ids are arbitrary but well-formed
    /// in-table values; the pair table is the 1:1 mapping contract.
    fn persistable_pairs() -> Vec<(GraphNode, PersistedGraphNode)> {
        let tuple_element = GraphTupleElement {
            label: 1,
            ty: 2,
            optional: true,
            rest: false,
        };
        let object_member = GraphObjectMember {
            kind: 1,
            name: 2,
            ty: 3,
            optional: false,
            readonly: true,
            key_name: 4,
            key_type: 5,
            value_type: 6,
            function: 7,
        };
        let function_param = GraphFunctionParam {
            name: 1,
            ty: 2,
            optional: false,
            rest: true,
        };
        let conditional_frame = GraphConditionalFrame {
            branch: 1,
            decided: true,
            check: 2,
            extends: 3,
        };
        vec![
            (
                GraphNode::Primitive { primitive: 3 },
                PersistedGraphNode::Primitive { primitive: 3 },
            ),
            (
                GraphNode::LiteralString { value: 1 },
                PersistedGraphNode::LiteralString { value: 1 },
            ),
            (
                GraphNode::LiteralNumber { bits: 42 },
                PersistedGraphNode::LiteralNumber { bits: 42 },
            ),
            (
                GraphNode::LiteralBoolean { value: true },
                PersistedGraphNode::LiteralBoolean { value: true },
            ),
            (
                GraphNode::LiteralBigInt { value: 2 },
                PersistedGraphNode::LiteralBigInt { value: 2 },
            ),
            (
                GraphNode::Union { types: vec![1, 2] },
                PersistedGraphNode::Union { types: vec![1, 2] },
            ),
            (
                GraphNode::Intersection { types: vec![3, 4] },
                PersistedGraphNode::Intersection { types: vec![3, 4] },
            ),
            (
                GraphNode::Array {
                    element: 1,
                    readonly: true,
                },
                PersistedGraphNode::Array {
                    element: 1,
                    readonly: true,
                },
            ),
            (
                GraphNode::Tuple {
                    readonly: false,
                    elements: vec![tuple_element.clone()],
                },
                PersistedGraphNode::Tuple {
                    readonly: false,
                    elements: vec![tuple_element],
                },
            ),
            (
                GraphNode::Object {
                    members: vec![object_member.clone()],
                },
                PersistedGraphNode::Object {
                    members: vec![object_member],
                },
            ),
            (
                GraphNode::Function {
                    parameters: vec![function_param.clone()],
                    return_type: 3,
                    type_parameters: vec![4],
                },
                PersistedGraphNode::Function {
                    parameters: vec![function_param],
                    return_type: 3,
                    type_parameters: vec![4],
                },
            ),
            (
                GraphNode::Ref {
                    name: 1,
                    type_arguments: vec![2],
                },
                PersistedGraphNode::Ref {
                    name: 1,
                    type_arguments: vec![2],
                },
            ),
            (
                GraphNode::TypeParameter {
                    name: 1,
                    constraint: 2,
                    default: 0,
                },
                PersistedGraphNode::TypeParameter {
                    name: 1,
                    constraint: 2,
                    default: 0,
                },
            ),
            (
                GraphNode::KeyOf { operand: 1 },
                PersistedGraphNode::KeyOf { operand: 1 },
            ),
            (
                GraphNode::TypeOf { path: vec![1, 2] },
                PersistedGraphNode::TypeOf { path: vec![1, 2] },
            ),
            (
                GraphNode::IndexedAccess {
                    object: 1,
                    index: 2,
                },
                PersistedGraphNode::IndexedAccess {
                    object: 1,
                    index: 2,
                },
            ),
            (
                GraphNode::Conditional {
                    check: 1,
                    extends: 2,
                    true_type: 3,
                    false_type: 4,
                },
                PersistedGraphNode::Conditional {
                    check: 1,
                    extends: 2,
                    true_type: 3,
                    false_type: 4,
                },
            ),
            (
                GraphNode::Mapped {
                    parameter: 1,
                    source: 2,
                    value: 3,
                    optional: 1,
                    readonly: 2,
                    name_type: 0,
                },
                PersistedGraphNode::Mapped {
                    parameter: 1,
                    source: 2,
                    value: 3,
                    optional: 1,
                    readonly: 2,
                    name_type: 0,
                },
            ),
            (
                GraphNode::TemplateLiteral {
                    quasis: vec![1, 2],
                    expressions: vec![3],
                },
                PersistedGraphNode::TemplateLiteral {
                    quasis: vec![1, 2],
                    expressions: vec![3],
                },
            ),
            (
                GraphNode::Parenthesized { inner: 1 },
                PersistedGraphNode::Parenthesized { inner: 1 },
            ),
            (
                GraphNode::Unknown { raw: 1 },
                PersistedGraphNode::Unknown { raw: 1 },
            ),
            (
                GraphNode::Infer { name: 1 },
                PersistedGraphNode::Infer { name: 1 },
            ),
            (
                GraphNode::Rest { inner: 1 },
                PersistedGraphNode::Rest { inner: 1 },
            ),
            (
                GraphNode::RecursiveRef {
                    name: 1,
                    type_arguments: vec![2],
                    conditional_context: vec![conditional_frame.clone()],
                },
                PersistedGraphNode::RecursiveRef {
                    name: 1,
                    type_arguments: vec![2],
                    conditional_context: vec![conditional_frame],
                },
            ),
        ]
    }

    /// [`try_persist`] is a TOTAL 1:1 mapping over every persistable
    /// variant — and the persisted mirror's serde JSON is BYTE-IDENTICAL to
    /// the live wire node's (same variant names, field names, and container
    /// attrs), so the FFI JSON of a snapshot is unchanged for every shared
    /// variant. The `SyntheticSlotBinding` carrier is the SOLE rejection:
    /// exactly `Err(NonPersistableNode)`.
    #[test]
    fn try_persist_maps_every_variant_and_rejects_only_the_synthetic_carrier() {
        let pairs = persistable_pairs();
        assert_eq!(
            pairs.len(),
            24,
            "the persisted vocabulary mirrors 24 of the 25 wire variants"
        );
        for (live, expected) in pairs {
            // Clone at the call site only: `try_persist` consumes its node,
            // and `live` is still needed for the serde byte-parity assertion.
            assert_eq!(
                try_persist(live.clone()),
                Ok(expected.clone()),
                "persistable variant must map 1:1: {live:?}"
            );
            assert_eq!(
                serde_json::to_string(&live).expect("wire node serializes"),
                serde_json::to_string(&expected).expect("persisted node serializes"),
                "persisted serde JSON must be byte-identical to the wire node's: {live:?}"
            );
        }

        let carrier = GraphNode::SyntheticSlotBinding {
            value_node: 424242,
            scope_canonical_id_id: 1,
            surface_kind: 0,
            slot_name_id: 2,
            binding_name_id: 3,
        };
        assert_eq!(
            try_persist(carrier),
            Err(SnapshotCaptureError::NonPersistableNode),
            "the generation-local carrier must fail closed, never persist"
        );
    }

    /// Capturing a builder whose table contains a `SyntheticSlotBinding`
    /// fails CLOSED — `Err(NonPersistableNode)`, never a partial snapshot
    /// with the carrier dropped.
    #[test]
    fn from_builder_fails_closed_on_synthetic_slot_binding_carrier() {
        use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};

        let mut builder = GraphBuilder::new();
        let root = builder.node_id(&TypeExpr::synthetic_slot_binding(SyntheticCarrierKey {
            scope_canonical_id: Arc::from("/abs/Foo.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("controls"),
            value_node: 424242,
        }));
        assert_eq!(
            ResolvedTypeGraphSnapshot::from_builder(builder, root).expect_err(
                "a slot-binding-bearing capture must fail closed, not produce a snapshot"
            ),
            SnapshotCaptureError::NonPersistableNode,
            "the exact typed capture error must surface"
        );
    }

    /// `try_new` rejects a zero root with the exact typed error.
    #[test]
    fn try_new_rejects_zero_root() {
        assert_eq!(
            ResolvedTypeGraphSnapshot::try_new(
                Vec::new(),
                vec![PersistedGraphNode::Primitive { primitive: 1 }],
                0,
            )
            .expect_err("a zero root must be rejected"),
            SnapshotCaptureError::RootNodeIdZero,
        );
    }

    /// `try_new` rejects a root past the node table with the exact typed
    /// error.
    #[test]
    fn try_new_rejects_root_out_of_range() {
        assert_eq!(
            ResolvedTypeGraphSnapshot::try_new(
                Vec::new(),
                vec![PersistedGraphNode::Primitive { primitive: 1 }],
                2,
            )
            .expect_err("an out-of-range root must be rejected"),
            SnapshotCaptureError::RootNodeIdOutOfRange,
        );
    }

    /// `try_new` rejects a node child-reference past the node table with the
    /// exact typed error (range is checked before the children-before-parents
    /// ordering, so a wild id reports as out-of-range).
    #[test]
    fn try_new_rejects_node_ref_out_of_range() {
        assert_eq!(
            ResolvedTypeGraphSnapshot::try_new(
                Vec::new(),
                vec![PersistedGraphNode::Array {
                    element: 5,
                    readonly: false,
                }],
                1,
            )
            .expect_err("an out-of-range node ref must be rejected"),
            SnapshotCaptureError::NodeRefOutOfRange,
        );
    }

    /// `try_new` rejects in-range FORWARD and SELF node references with the
    /// exact children-before-parents typed error.
    #[test]
    fn try_new_rejects_forward_and_self_node_refs() {
        // FORWARD: node 1 references node 2 (in-range — the table has 2
        // nodes — but not children-before-parents).
        assert_eq!(
            ResolvedTypeGraphSnapshot::try_new(
                Vec::new(),
                vec![
                    PersistedGraphNode::Array {
                        element: 2,
                        readonly: false,
                    },
                    PersistedGraphNode::Primitive { primitive: 1 },
                ],
                2,
            )
            .expect_err("a forward node ref must be rejected"),
            SnapshotCaptureError::NodeRefNotChildBeforeParent,
        );
        // SELF: node 1 references itself.
        assert_eq!(
            ResolvedTypeGraphSnapshot::try_new(
                Vec::new(),
                vec![PersistedGraphNode::Parenthesized { inner: 1 }],
                1,
            )
            .expect_err("a self node ref must be rejected"),
            SnapshotCaptureError::NodeRefNotChildBeforeParent,
        );
    }

    /// `try_new` rejects a string reference past the string table with the
    /// exact typed error.
    #[test]
    fn try_new_rejects_string_ref_out_of_range() {
        assert_eq!(
            ResolvedTypeGraphSnapshot::try_new(
                vec!["X".to_string()],
                vec![PersistedGraphNode::LiteralString { value: 3 }],
                1,
            )
            .expect_err("an out-of-range string ref must be rejected"),
            SnapshotCaptureError::StringRefOutOfRange,
        );
    }

    /// A well-formed table constructs: children-before-parents node refs,
    /// in-range string refs, in-range non-zero root — and absent (0) refs
    /// are allowed everywhere.
    #[test]
    fn try_new_accepts_well_formed_tables() {
        let snapshot = ResolvedTypeGraphSnapshot::try_new(
            vec!["Foo".to_string()],
            vec![
                PersistedGraphNode::Ref {
                    name: 1,
                    type_arguments: Vec::new(),
                },
                PersistedGraphNode::Array {
                    element: 1,
                    readonly: false,
                },
                // Absent-id positions (0) round through validation untouched.
                PersistedGraphNode::TypeParameter {
                    name: 1,
                    constraint: 2,
                    default: 0,
                },
            ],
            3,
        )
        .expect("a well-formed table must construct");
        assert_eq!(snapshot.root_node_id, 3);
        assert_eq!(snapshot.nodes.len(), 3);
        assert_eq!(snapshot.strings.len(), 1);
        // The content key is computed on construction and discriminates.
        let other = ResolvedTypeGraphSnapshot::try_new(
            vec!["Foo".to_string()],
            vec![PersistedGraphNode::Ref {
                name: 1,
                type_arguments: Vec::new(),
            }],
            1,
        )
        .expect("the smaller well-formed table must construct");
        assert_ne!(
            snapshot.stable_key, other.stable_key,
            "different content ⇒ different key"
        );
    }
}
