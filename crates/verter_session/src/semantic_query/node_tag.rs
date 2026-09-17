//! The one vocabulary of [`SemanticNodeData`] variants.
//!
//! Every consumer that needs a per-variant number — arena push buckets, the
//! content fingerprint of the footprint miner, the cycle-guard shape hash, the
//! interning `Hash` — reads it from [`SemanticNodeTag`] instead of keeping its
//! own table. Separate tables drift: before this vocabulary the same
//! intrinsic application was bucket 25, footprint tag 26 and cycle byte 21.
//!
//! The numbers are STABLE: they are folded into persisted footprint
//! fingerprints, so a value is never reused once assigned. `0` is never
//! assigned and `18` is retired.
//!
//! Topology-only walkers enumerate a payload's child ids through
//! [`SemanticNodeData::for_each_child`] rather than restating the per-variant
//! field set.

use super::{
    authored_property_key_child, SemanticNodeData, SemanticNodeId, SignatureReturnCarrier,
    SurfaceEntry,
};

/// Exclusive upper bound of every [`SemanticNodeTag::stable_id`] — the width of
/// per-variant bucket arrays.
pub const SEMANTIC_NODE_TAG_BOUND: usize = 32;

/// Fieldless identity of one [`SemanticNodeData`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum SemanticNodeTag {
    Alias = 1,
    Object = 2,
    Union = 3,
    Intersection = 4,
    Primitive = 5,
    Literal = 6,
    Opaque = 7,
    Array = 8,
    Tuple = 9,
    TemplateLiteral = 10,
    KeyOf = 11,
    IndexedAccess = 12,
    Mapped = 13,
    TypeOf = 14,
    TypeParam = 15,
    Infer = 16,
    Conditional = 17,
    // 18 is retired and stays unassigned.
    TypeOfNominal = 19,
    DeclRef = 20,
    InstantiationRef = 21,
    MergedDecl = 22,
    BareRef = 23,
    ImportType = 24,
    RawFallback = 25,
    IntrinsicApplication = 26,
    SyntheticBinding = 27,
    InferRef = 28,
    Signature = 29,
    ObjectSpreadProgram = 30,
    DeferredCallable = 31,
}

impl SemanticNodeTag {
    /// Every tag, in stable-id order.
    pub const ALL: [Self; 30] = [
        Self::Alias,
        Self::Object,
        Self::Union,
        Self::Intersection,
        Self::Primitive,
        Self::Literal,
        Self::Opaque,
        Self::Array,
        Self::Tuple,
        Self::TemplateLiteral,
        Self::KeyOf,
        Self::IndexedAccess,
        Self::Mapped,
        Self::TypeOf,
        Self::TypeParam,
        Self::Infer,
        Self::Conditional,
        Self::TypeOfNominal,
        Self::DeclRef,
        Self::InstantiationRef,
        Self::MergedDecl,
        Self::BareRef,
        Self::ImportType,
        Self::RawFallback,
        Self::IntrinsicApplication,
        Self::SyntheticBinding,
        Self::InferRef,
        Self::Signature,
        Self::ObjectSpreadProgram,
        Self::DeferredCallable,
    ];

    /// The stable one-byte identity. Always in `1..SEMANTIC_NODE_TAG_BOUND`.
    #[must_use]
    pub const fn stable_id(self) -> u8 {
        self as u8
    }

    /// Index into a `[_; SEMANTIC_NODE_TAG_BOUND]` per-variant bucket array.
    #[must_use]
    pub const fn bucket_index(self) -> usize {
        self as usize
    }
}

/// Whether [`SemanticNodeData::for_each_child`] enumerated the payload's
/// children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a `Sealed` walk did NOT enumerate this payload's children — a \
              reachability reader must treat its walk as incomplete"]
pub enum ChildWalk {
    /// Every child id of the payload was visited.
    Enumerated,
    /// The payload is a sealed composition whose parts are readable only by
    /// its sanctioned consumers; no child was visited.
    Sealed,
}

impl SemanticNodeData {
    /// The variant's identity in the shared vocabulary.
    #[must_use]
    pub fn node_tag(&self) -> SemanticNodeTag {
        match self {
            Self::Alias(_) => SemanticNodeTag::Alias,
            Self::Object(_) => SemanticNodeTag::Object,
            Self::ObjectSpreadProgram(_) => SemanticNodeTag::ObjectSpreadProgram,
            Self::Union(_) => SemanticNodeTag::Union,
            Self::Intersection(_) => SemanticNodeTag::Intersection,
            Self::Primitive(_) => SemanticNodeTag::Primitive,
            Self::Literal(_) => SemanticNodeTag::Literal,
            Self::Opaque(_) => SemanticNodeTag::Opaque,
            Self::Array { .. } => SemanticNodeTag::Array,
            Self::Tuple { .. } => SemanticNodeTag::Tuple,
            Self::TemplateLiteral { .. } => SemanticNodeTag::TemplateLiteral,
            Self::KeyOf { .. } => SemanticNodeTag::KeyOf,
            Self::IndexedAccess { .. } => SemanticNodeTag::IndexedAccess,
            Self::Mapped { .. } => SemanticNodeTag::Mapped,
            Self::TypeOf(_) => SemanticNodeTag::TypeOf,
            Self::TypeOfNominal(_) => SemanticNodeTag::TypeOfNominal,
            Self::TypeParam { .. } => SemanticNodeTag::TypeParam,
            Self::Infer { .. } => SemanticNodeTag::Infer,
            Self::InferRef { .. } => SemanticNodeTag::InferRef,
            Self::MergedDecl { .. } => SemanticNodeTag::MergedDecl,
            Self::Conditional { .. } => SemanticNodeTag::Conditional,
            Self::Signature { .. } => SemanticNodeTag::Signature,
            Self::DeferredCallable(_) => SemanticNodeTag::DeferredCallable,
            Self::DeclRef { .. } => SemanticNodeTag::DeclRef,
            Self::InstantiationRef { .. } => SemanticNodeTag::InstantiationRef,
            Self::BareRef(_) => SemanticNodeTag::BareRef,
            Self::ImportType(_) => SemanticNodeTag::ImportType,
            Self::RawFallback { .. } => SemanticNodeTag::RawFallback,
            Self::IntrinsicApplication { .. } => SemanticNodeTag::IntrinsicApplication,
            Self::SyntheticBinding { .. } => SemanticNodeTag::SyntheticBinding,
        }
    }

    /// Visit every child id stored in the payload, without allocating.
    ///
    /// Topology only: no semantic stop applies (a `TypeParam`'s constraint and
    /// default, a mapper's parameter node, a pending conditional frame's
    /// arguments and an `Object`'s derived kind-specific indexes are all
    /// visited). An id can therefore be visited more than once; the order is
    /// stable. Readers that stop at shallow carriers or binders keep their own
    /// walk.
    ///
    /// Returns [`ChildWalk::Sealed`] for the sealed callable carrier, whose
    /// children are not enumerable here.
    pub fn for_each_child(&self, mut visit: impl FnMut(SemanticNodeId)) -> ChildWalk {
        match self {
            Self::Primitive(_)
            | Self::Literal(_)
            | Self::Opaque(_)
            | Self::RawFallback { .. }
            | Self::Infer { .. }
            | Self::InferRef { .. }
            | Self::DeclRef { .. }
            | Self::TypeOfNominal(_) => {}
            Self::IntrinsicApplication { args, .. } | Self::InstantiationRef { args, .. } => {
                args.iter().copied().for_each(visit);
            }
            Self::Alias(inner) | Self::KeyOf { base: inner } => visit(*inner),
            Self::Object(view) => {
                for entry in view.entries.iter() {
                    match entry {
                        SurfaceEntry::Member(member) => {
                            authored_property_key_child(&member.key)
                                .into_iter()
                                .for_each(&mut visit);
                            visit(member.value);
                        }
                        SurfaceEntry::CallSignature(node)
                        | SurfaceEntry::ConstructSignature(node) => {
                            visit(*node);
                        }
                        SurfaceEntry::IndexSignature(signature) => {
                            visit(signature.key_type);
                            visit(signature.value_type);
                        }
                    }
                }
                for member in view.positive_members() {
                    authored_property_key_child(&member.key)
                        .into_iter()
                        .for_each(&mut visit);
                    visit(member.value);
                }
                view.call_signatures.iter().copied().for_each(&mut visit);
                view.construct_signatures
                    .iter()
                    .copied()
                    .for_each(&mut visit);
                for signature in view.index_signatures.iter() {
                    visit(signature.key_type);
                    visit(signature.value_type);
                }
                view.keyspace.into_iter().for_each(visit);
            }
            Self::ObjectSpreadProgram(program) => program.for_each_child_node(visit),
            Self::Union(members) => members.iter().copied().for_each(visit),
            Self::Intersection(members) => members.iter().copied().for_each(visit),
            Self::MergedDecl { contributors } => contributors.iter().copied().for_each(visit),
            Self::Array { element, .. } => visit(*element),
            Self::Tuple { elements, .. } => {
                elements.iter().map(|element| element.value).for_each(visit);
            }
            Self::TemplateLiteral { expressions, .. } => {
                expressions.iter().copied().for_each(visit);
            }
            Self::IndexedAccess { object, index } => {
                visit(*object);
                authored_property_key_child(index)
                    .into_iter()
                    .for_each(visit);
            }
            Self::Mapped { source, mapper } => {
                visit(*source);
                visit(mapper.parameter_node);
                visit(mapper.key_space);
                visit(mapper.value_expr);
                mapper.name_remap.into_iter().for_each(visit);
            }
            Self::TypeOf(_) | Self::BareRef(_) | Self::ImportType(_) => {
                self.carrier_type_args().iter().copied().for_each(visit);
            }
            Self::TypeParam {
                constraint,
                default,
                ..
            } => {
                constraint
                    .iter()
                    .chain(default.iter())
                    .copied()
                    .for_each(visit);
            }
            Self::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                pending,
                ..
            } => {
                if let Some(pending) = pending {
                    pending.argument_nodes().for_each(&mut visit);
                }
                [*check, *extends, *true_branch_ref, *false_branch_ref]
                    .into_iter()
                    .for_each(visit);
            }
            Self::Signature {
                params,
                return_type,
                type_parameters,
                return_carrier,
                ..
            } => {
                params.iter().map(|param| param.ty).for_each(&mut visit);
                visit(*return_type);
                if let SignatureReturnCarrier::Declared(node) = return_carrier {
                    visit(*node);
                }
                for decl in type_parameters.iter() {
                    visit(decl.param);
                    decl.constraint.into_iter().for_each(&mut visit);
                    decl.default.into_iter().for_each(&mut visit);
                }
            }
            Self::DeferredCallable(_) => return ChildWalk::Sealed,
            Self::SyntheticBinding { value_node, .. } => visit(SemanticNodeId(*value_node)),
        }
        ChildWalk::Enumerated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_are_unique_bounded_and_skip_the_unassigned_values() {
        let mut seen = [false; SEMANTIC_NODE_TAG_BOUND];
        for tag in SemanticNodeTag::ALL {
            let id = tag.stable_id();
            assert!(
                (1..SEMANTIC_NODE_TAG_BOUND as u8).contains(&id),
                "{tag:?} = {id}"
            );
            assert_ne!(id, 18, "18 is retired");
            assert!(!seen[tag.bucket_index()], "{tag:?} reuses {id}");
            seen[tag.bucket_index()] = true;
        }
        assert!(
            SemanticNodeTag::ALL
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "ALL is in stable-id order"
        );
        assert_eq!(
            SEMANTIC_NODE_TAG_BOUND,
            crate::types::SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT
        );
    }

    /// The values are folded into persisted footprint fingerprints: pin them
    /// so a renumbering is a visible test change, not a silent golden drift.
    /// A compiler-native node exists only at the operation's exact arity.
    #[test]
    fn intrinsic_application_is_minted_only_at_the_operations_arity() {
        use crate::semantic_query::CompilerIntrinsicTypeOp;
        use std::sync::Arc;

        let op = CompilerIntrinsicTypeOp::Awaited;
        let operands = |count: u64| -> Arc<[SemanticNodeId]> {
            (0..count).map(SemanticNodeId).collect::<Vec<_>>().into()
        };
        assert!(SemanticNodeData::intrinsic_application(op, operands(0)).is_none());
        assert!(matches!(
            SemanticNodeData::intrinsic_application(op, operands(1)),
            Some(SemanticNodeData::IntrinsicApplication { args, .. }) if args.len() == 1
        ));
        assert!(
            SemanticNodeData::intrinsic_application(op, operands(2)).is_none(),
            "Awaited<A, B> must never become a compiler-native node"
        );
    }

    #[test]
    fn stable_ids_are_pinned() {
        use SemanticNodeTag as T;
        let pinned: [(T, u8); 30] = [
            (T::Alias, 1),
            (T::Object, 2),
            (T::Union, 3),
            (T::Intersection, 4),
            (T::Primitive, 5),
            (T::Literal, 6),
            (T::Opaque, 7),
            (T::Array, 8),
            (T::Tuple, 9),
            (T::TemplateLiteral, 10),
            (T::KeyOf, 11),
            (T::IndexedAccess, 12),
            (T::Mapped, 13),
            (T::TypeOf, 14),
            (T::TypeParam, 15),
            (T::Infer, 16),
            (T::Conditional, 17),
            (T::TypeOfNominal, 19),
            (T::DeclRef, 20),
            (T::InstantiationRef, 21),
            (T::MergedDecl, 22),
            (T::BareRef, 23),
            (T::ImportType, 24),
            (T::RawFallback, 25),
            (T::IntrinsicApplication, 26),
            (T::SyntheticBinding, 27),
            (T::InferRef, 28),
            (T::Signature, 29),
            (T::ObjectSpreadProgram, 30),
            (T::DeferredCallable, 31),
        ];
        for (tag, id) in pinned {
            assert_eq!(tag.stable_id(), id, "{tag:?}");
        }
    }
}
