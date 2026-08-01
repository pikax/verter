//! Algebra 1 of the shared shape engine: `MaterializeTypeExprAlg` — the EXACT
//! historical `SemanticNodeId -> TypeExpr` materialization, reached only through
//! the sealed `OutputProjector` output seam and the `#[cfg(test)]` oracle. Split
//! from the parent for file-size; the algebra trait lives in the parent
//! `shape_engine` module, the shared fold in the sibling [`fold`](super::fold)
//! module.
//!
//! The algebra's `Out` is [`MaterializedTypeExpr`]: the compatibility
//! [`TypeExpr`] tree PLUS a private per-leaf degradation sidecar. Resolver
//! degradation (a dispatch miss, an exhausted budget, an unrepresentable
//! surface) reaches the fold as a TYPED [`QueryError`] and rides the sidecar
//! to the output carrier — it is NEVER encoded as a raw sentinel spelling
//! re-classified downstream. The tree leaf at a degraded position is the
//! terminal compatibility projection
//! (`UnknownValue::compatibility_projection(semantic_query_error_raw(..))`),
//! so wire/display/hash bytes are byte-identical to the legacy encoding while
//! the sidecar carries the only control meaning.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::{
    LiteralValue, MappedModifier, MemberVisibility, PrimitiveName, TypeExpr, UnknownValue,
};

use super::super::ProjectSemanticDispatch;
use super::fold::{fold_node, FoldedFunction, FoldedTupleElement};
use super::RaisedShapeAlgebra;
use crate::resolver_core::component_meta_query_engine::semantic_query_error_raw;
use crate::semantic_query::{QueryError, SemanticNodeId};

// ===========================================================================
// The degradation sidecar vocabulary.
// ===========================================================================

/// One step on the path from a materialized tree's root to a degraded leaf.
/// Purely positional — it names WHICH child slot the fold descended through,
/// so a consumer can locate the degraded leaf inside the compat tree without
/// re-parsing any spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum MaterializePathSegment {
    UnionArm(u32),
    IntersectionArm(u32),
    ArrayElement,
    TupleElement(u32),
    ObjectMember {
        index: u32,
        slot: ObjectMemberSlot,
    },
    FunctionParameter(u32),
    FunctionReturn,
    FunctionTypeParameter {
        index: u32,
        slot: TypeParameterSlot,
    },
    /// The top-level standalone `type_parameter` arm's constraint/default.
    TypeParameter {
        slot: TypeParameterSlot,
    },
    ReferenceArgument(u32),
    ImportArgument(u32),
    TypeOfArgument(u32),
    KeyOfOperand,
    IndexedObject,
    IndexedIndex,
    ConditionalCheck,
    ConditionalExtends,
    ConditionalTrue,
    ConditionalFalse,
    MappedSource,
    MappedValue,
    MappedName,
    TemplateExpression(u32),
    /// Re-anchor marker for a degradation absorbed from a structure the fold
    /// NORMALIZED AWAY (a dropped vacuous intersection arm, an invalid
    /// call/construct signature): the leaf is retained (fail-closed partial)
    /// but NEVER at the root path, so the collapsed result cannot read as a
    /// fresh root sentinel downstream.
    DroppedStructure,
}

/// Which slot of an object member carries the degraded leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ObjectMemberSlot {
    /// Computed property / method key expression.
    Key,
    /// Property / method / call / construct signature value.
    Value,
    /// Index-signature key type.
    IndexKey,
    /// Index-signature value type.
    IndexValue,
}

/// Which slot of a type parameter carries the degraded leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TypeParameterSlot {
    Constraint,
    Default,
}

/// One degradation event at a leaf position inside a materialized
/// [`TypeExpr`]: the TYPED reason plus the positional path from the tree root.
/// The reason is the ONLY control channel — the compat tree leaf at this
/// position is inert [`UnknownValue`] text.
#[derive(Debug, Clone)]
pub(crate) struct DegradedLeaf {
    path: Arc<[MaterializePathSegment]>,
    reason: QueryError,
}

impl DegradedLeaf {
    /// The positional path from the tree root (empty at the root itself).
    #[cfg(test)]
    pub(crate) fn path(&self) -> &[MaterializePathSegment] {
        &self.path
    }
    /// The typed degradation reason.
    #[cfg(test)]
    pub(crate) fn reason(&self) -> &QueryError {
        &self.reason
    }
    /// Prepend `segment` to this leaf's path (the fold descends one slot).
    fn prefixed(&self, segment: MaterializePathSegment) -> Self {
        let mut path = Vec::with_capacity(self.path.len() + 1);
        path.push(segment);
        path.extend_from_slice(&self.path);
        Self {
            path: Arc::from(path.into_boxed_slice()),
            reason: self.reason.clone(),
        }
    }
}

/// The materialize-fold value: the compatibility [`TypeExpr`] tree plus the
/// per-leaf degradation sidecar. Construction is sealed to this module's
/// constructors — the sidecar can only grow through [`Self::degraded`] (a
/// typed [`QueryError`] leaf) and the fold's prefix/merge plumbing.
#[derive(Debug, Clone)]
pub(crate) struct MaterializedTypeExpr {
    expr: TypeExpr,
    degraded_leaves: Vec<DegradedLeaf>,
}

impl MaterializedTypeExpr {
    /// A fully-exact tree: no degradation anywhere.
    pub(crate) fn exact(expr: TypeExpr) -> Self {
        Self {
            expr,
            degraded_leaves: Vec::new(),
        }
    }

    /// A ROOT degradation: the tree is ALWAYS the terminal compatibility
    /// projection of `reason` (byte-identical legacy spelling). The sidecar
    /// records the typed reason at the empty (root) path ONLY when `reason`
    /// is in the unmaterialised-sentinel class
    /// ([`query_error_is_unmaterialized_sentinel`]) — the partial channel
    /// agrees with the node-domain `materialized` fact by construction, so a
    /// deliberately-materialised placeholder (`RaiseMiss`, `TypeParamCycle`,
    /// `Other(..)`, `DeclPlaceholder`) degrades NOTHING.
    ///
    /// [`query_error_is_unmaterialized_sentinel`]: crate::project_semantic_dispatch::raise_sentinel::query_error_is_unmaterialized_sentinel
    pub(crate) fn degraded(reason: QueryError) -> Self {
        let is_unmaterialized = crate::project_semantic_dispatch::raise_sentinel::query_error_is_unmaterialized_sentinel(&reason);
        Self {
            expr: TypeExpr::Unknown(UnknownValue::compatibility_projection(
                semantic_query_error_raw(&reason),
            )),
            degraded_leaves: if is_unmaterialized {
                vec![DegradedLeaf {
                    path: Arc::from(Vec::new().into_boxed_slice()),
                    reason,
                }]
            } else {
                Vec::new()
            },
        }
    }

    /// Prepend `segment` to every degraded-leaf path (consuming fold step).
    fn prefix(self, segment: MaterializePathSegment) -> Self {
        Self {
            expr: self.expr,
            degraded_leaves: self
                .degraded_leaves
                .iter()
                .map(|leaf| leaf.prefixed(segment))
                .collect(),
        }
    }

    /// Assemble a compound value from its already-prefixed children: the
    /// given tree plus the concatenation of the children's sidecars.
    fn merge(expr: TypeExpr, children: Vec<MaterializedTypeExpr>) -> Self {
        let mut degraded_leaves = Vec::new();
        for child in children {
            degraded_leaves.extend(child.degraded_leaves);
        }
        Self {
            expr,
            degraded_leaves,
        }
    }

    /// The typed reason when the ROOT of this value is itself a degradation
    /// leaf (empty path) — the intersection arm-drop reads this. A nested
    /// degraded leaf (non-empty path) is NOT a root degradation.
    pub(crate) fn root_degradation(&self) -> Option<&QueryError> {
        self.degraded_leaves
            .iter()
            .find(|leaf| leaf.path.is_empty())
            .map(|leaf| &leaf.reason)
    }

    /// `true` when any leaf degraded — folded into the output carrier's
    /// `result_is_partial` at the `from_parts` choke point (which reads the
    /// sealed payload's own copy; this fold-value accessor is part of the
    /// sidecar API surface and is exercised by the materialize tests).
    #[allow(
        dead_code,
        reason = "sidecar API surface; production reads the sealed payload's has_degradation"
    )]
    pub(crate) fn has_degradation(&self) -> bool {
        !self.degraded_leaves.is_empty()
    }

    /// Split into the compat tree and the sidecar. Callers MUST carry both
    /// into the sealed output payload — re-sealing the bare tree would
    /// silently drop degradation (the plumbing rule).
    pub(crate) fn into_parts(self) -> (TypeExpr, Vec<DegradedLeaf>) {
        (self.expr, self.degraded_leaves)
    }

    /// Borrow the compat tree. A BORROW only — the sidecar stays attached to
    /// `self`; callers must not re-seal the bare tree (the plumbing rule).
    pub(crate) fn expr(&self) -> &TypeExpr {
        &self.expr
    }
}

/// The materialize-fold function intermediate: the built [`FunctionExpr`]
/// plus the sidecar leaves its parameter / return / type-parameter slots
/// contributed (already prefixed with their function-level segments).
pub(crate) struct MaterializedFunction {
    function: Arc<verter_type_expr::FunctionExpr>,
    degraded_leaves: Vec<DegradedLeaf>,
}

/// A member slot's not-yet-indexed degradation: the member constructors
/// record WHICH slot degraded; [`MaterializeTypeExprAlg::object_from_members`]
/// prefixes the member index.
pub(crate) struct PendingMemberDegradation {
    slot: ObjectMemberSlot,
    leaf: DegradedLeaf,
}

/// The materialize-fold object-member intermediate.
pub(crate) struct MaterializedObjectMember {
    member: verter_type_expr::ObjectMember,
    degraded_leaves: Vec<PendingMemberDegradation>,
}

// ===========================================================================
// Algebra 1 — `MaterializeTypeExprAlg` (Out = MaterializedTypeExpr).
//
// The EXACT historical materialization, reached ONLY through the sealed
// `OutputProjector` output seam and the `#[cfg(test)]` oracle. Each arm
// reproduces the former `raise_node_to_type_expr_core_impl` construction
// byte-for-byte (the byte-identity contract pinned by the raise /
// materialization suite + the 20 raised-shape parity tests); the sidecar
// rides alongside without touching the tree bytes.
// ===========================================================================

/// Stateless materialization algebra.
pub(in crate::project_semantic_dispatch) struct MaterializeTypeExprAlg;

/// Assemble a compound [`MaterializedTypeExpr`] from a tree constructor and
/// the (segment, child) pairs the tree consumes: each child is prefixed with
/// its slot segment, the tree is built from the inner exprs, and the sidecars
/// merge.
fn fold_compound(
    children: Vec<(MaterializePathSegment, MaterializedTypeExpr)>,
    build: impl FnOnce(Vec<TypeExpr>) -> TypeExpr,
) -> MaterializedTypeExpr {
    let mut exprs = Vec::with_capacity(children.len());
    let mut prefixed = Vec::with_capacity(children.len());
    for (segment, child) in children {
        let child = child.prefix(segment);
        exprs.push(child.expr.clone());
        prefixed.push(child);
    }
    MaterializedTypeExpr::merge(build(exprs), prefixed)
}

impl RaisedShapeAlgebra for MaterializeTypeExprAlg {
    type Out = MaterializedTypeExpr;
    type Fn = MaterializedFunction;
    type Member = MaterializedObjectMember;

    fn primitive(&mut self, kind: PrimitiveName) -> MaterializedTypeExpr {
        MaterializedTypeExpr::exact(TypeExpr::Primitive(kind))
    }
    fn literal(&mut self, value: LiteralValue) -> MaterializedTypeExpr {
        MaterializedTypeExpr::exact(TypeExpr::Literal(value))
    }
    fn infer(&mut self, name: Arc<str>) -> MaterializedTypeExpr {
        MaterializedTypeExpr::exact(TypeExpr::Infer {
            name: name.as_ref().to_string(),
        })
    }
    fn unknown(&mut self, value: UnknownValue) -> MaterializedTypeExpr {
        // A GENUINE unknown (unrepresentable authored/raw syntax): exact —
        // the sidecar is for typed resolver degradation only.
        MaterializedTypeExpr::exact(TypeExpr::Unknown(value))
    }
    fn opaque_sentinel(&mut self, err: &QueryError) -> MaterializedTypeExpr {
        // A TYPED resolver-control sentinel: the sidecar carries the typed
        // reason; the tree leaf is the terminal compatibility projection —
        // byte-for-byte the legacy `Unknown { raw }` string the old hardcoded
        // literal emitted (via the single `semantic_query_error_raw`
        // mapping). No raw-string classification reads it back.
        MaterializedTypeExpr::degraded(err.clone())
    }
    fn recursive_ref(&mut self, name: Arc<str>) -> MaterializedTypeExpr {
        MaterializedTypeExpr::exact(TypeExpr::recursive_ref(name.as_ref(), Vec::new()))
    }
    fn reference(
        &mut self,
        name: Arc<str>,
        type_arguments: Vec<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        let has_args = !type_arguments.is_empty();
        fold_compound(
            type_arguments
                .into_iter()
                .enumerate()
                .map(|(i, arg)| (MaterializePathSegment::ReferenceArgument(i as u32), arg))
                .collect(),
            |exprs| TypeExpr::Ref {
                name,
                type_arguments: if has_args {
                    Arc::from(exprs.into_boxed_slice())
                } else {
                    verter_type_expr::empty_type_args()
                },
            },
        )
    }
    fn synthetic_slot_binding(
        &mut self,
        carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> MaterializedTypeExpr {
        MaterializedTypeExpr::exact(TypeExpr::SyntheticSlotBinding(carrier))
    }
    fn import_type(
        &mut self,
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        typeof_query: bool,
        type_arguments: Vec<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        fold_compound(
            type_arguments
                .into_iter()
                .enumerate()
                .map(|(i, arg)| (MaterializePathSegment::ImportArgument(i as u32), arg))
                .collect(),
            |exprs| TypeExpr::ImportType {
                specifier,
                qualifier,
                typeof_query,
                type_arguments: Arc::from(exprs.into_boxed_slice()),
            },
        )
    }
    fn type_of(
        &mut self,
        path: Vec<String>,
        type_args: Vec<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        fold_compound(
            type_args
                .into_iter()
                .enumerate()
                .map(|(i, arg)| (MaterializePathSegment::TypeOfArgument(i as u32), arg))
                .collect(),
            |exprs| {
                TypeExpr::TypeOf(verter_type_expr::ValueRef {
                    path,
                    type_args: exprs,
                })
            },
        )
    }

    fn union(&mut self, members: Vec<MaterializedTypeExpr>) -> MaterializedTypeExpr {
        fold_compound(
            members
                .into_iter()
                .enumerate()
                .map(|(i, member)| (MaterializePathSegment::UnionArm(i as u32), member))
                .collect(),
            |exprs| TypeExpr::Union(Arc::from(exprs.into_boxed_slice())),
        )
    }
    fn intersection(&mut self, arms: Vec<MaterializedTypeExpr>) -> MaterializedTypeExpr {
        fold_compound(
            arms.into_iter()
                .enumerate()
                .map(|(i, arm)| (MaterializePathSegment::IntersectionArm(i as u32), arm))
                .collect(),
            |exprs| TypeExpr::Intersection(Arc::from(exprs.into_boxed_slice())),
        )
    }
    fn empty_object(&mut self) -> MaterializedTypeExpr {
        MaterializedTypeExpr::exact(TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
            properties: Vec::new(),
        })))
    }
    fn array(&mut self, element: MaterializedTypeExpr, readonly: bool) -> MaterializedTypeExpr {
        fold_compound(
            vec![(MaterializePathSegment::ArrayElement, element)],
            |mut exprs| TypeExpr::Array {
                element: Arc::new(exprs.pop().expect("array element")),
                readonly,
            },
        )
    }
    fn tuple(
        &mut self,
        elements: Vec<FoldedTupleElement<MaterializedTypeExpr>>,
        readonly: bool,
    ) -> MaterializedTypeExpr {
        let mut children = Vec::with_capacity(elements.len());
        let mut labels = Vec::with_capacity(elements.len());
        let mut flags = Vec::with_capacity(elements.len());
        for (i, e) in elements.into_iter().enumerate() {
            children.push((MaterializePathSegment::TupleElement(i as u32), e.ty));
            labels.push(e.label);
            flags.push((e.optional, e.rest));
        }
        fold_compound(children, |exprs| {
            let elements: Vec<verter_type_expr::TupleElement> = exprs
                .into_iter()
                .zip(labels)
                .zip(flags)
                .map(
                    |((ty, label), (optional, rest))| verter_type_expr::TupleElement {
                        label,
                        ty,
                        optional,
                        rest,
                    },
                )
                .collect();
            TypeExpr::Tuple {
                elements: Arc::from(elements.into_boxed_slice()),
                readonly,
            }
        })
    }
    fn key_of(&mut self, base: MaterializedTypeExpr) -> MaterializedTypeExpr {
        fold_compound(
            vec![(MaterializePathSegment::KeyOfOperand, base)],
            |mut exprs| TypeExpr::KeyOf(Arc::new(exprs.pop().expect("keyof operand"))),
        )
    }
    fn indexed_access(
        &mut self,
        object: MaterializedTypeExpr,
        index: MaterializedTypeExpr,
    ) -> MaterializedTypeExpr {
        fold_compound(
            vec![
                (MaterializePathSegment::IndexedObject, object),
                (MaterializePathSegment::IndexedIndex, index),
            ],
            |mut exprs| {
                let index = exprs.pop().expect("indexed index");
                let object = exprs.pop().expect("indexed object");
                TypeExpr::IndexedAccess {
                    object: Arc::new(object),
                    index: Arc::new(index),
                }
            },
        )
    }
    fn conditional(
        &mut self,
        check: MaterializedTypeExpr,
        extends: MaterializedTypeExpr,
        true_type: MaterializedTypeExpr,
        false_type: MaterializedTypeExpr,
    ) -> MaterializedTypeExpr {
        fold_compound(
            vec![
                (MaterializePathSegment::ConditionalCheck, check),
                (MaterializePathSegment::ConditionalExtends, extends),
                (MaterializePathSegment::ConditionalTrue, true_type),
                (MaterializePathSegment::ConditionalFalse, false_type),
            ],
            |mut exprs| {
                let false_type = exprs.pop().expect("conditional false");
                let true_type = exprs.pop().expect("conditional true");
                let extends = exprs.pop().expect("conditional extends");
                let check = exprs.pop().expect("conditional check");
                TypeExpr::Conditional {
                    check: Arc::new(check),
                    extends: Arc::new(extends),
                    true_type: Arc::new(true_type),
                    false_type: Arc::new(false_type),
                }
            },
        )
    }
    fn mapped(
        &mut self,
        parameter: String,
        source: MaterializedTypeExpr,
        value: MaterializedTypeExpr,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        let mut children = vec![
            (MaterializePathSegment::MappedSource, source),
            (MaterializePathSegment::MappedValue, value),
        ];
        if let Some(name_type) = name_type {
            children.push((MaterializePathSegment::MappedName, name_type));
        }
        fold_compound(children, |mut exprs| {
            let name_type = if exprs.len() == 3 {
                exprs.pop().map(Arc::new)
            } else {
                None
            };
            let value = exprs.pop().expect("mapped value");
            let source = exprs.pop().expect("mapped source");
            TypeExpr::Mapped {
                parameter,
                source: Arc::new(source),
                value: Arc::new(value),
                optional,
                readonly,
                name_type,
            }
        })
    }
    fn template_literal(
        &mut self,
        quasis: Vec<String>,
        expressions: Vec<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        fold_compound(
            expressions
                .into_iter()
                .enumerate()
                .map(|(i, expr)| (MaterializePathSegment::TemplateExpression(i as u32), expr))
                .collect(),
            |exprs| TypeExpr::TemplateLiteral {
                quasis,
                expressions: Arc::from(exprs.into_boxed_slice()),
            },
        )
    }
    fn type_parameter(
        &mut self,
        name: Arc<str>,
        constraint: Option<MaterializedTypeExpr>,
        default: Option<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        // The two slots are INDEPENDENT Options — carry them explicitly
        // (never rebuild positionally from a flat vec, which shifts a
        // lone default into the constraint slot).
        let mut degraded_leaves = Vec::new();
        let constraint = constraint.map(|c| {
            let c = c.prefix(MaterializePathSegment::TypeParameter {
                slot: TypeParameterSlot::Constraint,
            });
            degraded_leaves.extend(c.degraded_leaves.iter().cloned());
            Arc::new(c.expr)
        });
        let default = default.map(|d| {
            let d = d.prefix(MaterializePathSegment::TypeParameter {
                slot: TypeParameterSlot::Default,
            });
            degraded_leaves.extend(d.degraded_leaves.iter().cloned());
            Arc::new(d.expr)
        });
        MaterializedTypeExpr {
            expr: TypeExpr::TypeParameter(verter_type_expr::TypeParam {
                name: name.as_ref().to_string(),
                constraint,
                default,
            }),
            degraded_leaves,
        }
    }

    fn build_function(
        &mut self,
        function: FoldedFunction<MaterializedTypeExpr>,
    ) -> MaterializedFunction {
        use verter_type_expr::{FunctionExpr, FunctionParam, FunctionSpans, TypeParam};
        let mut degraded_leaves = Vec::new();
        let parameters: Vec<FunctionParam> = function
            .parameters
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                let ty =
                    p.ty.prefix(MaterializePathSegment::FunctionParameter(i as u32));
                degraded_leaves.extend(ty.degraded_leaves.iter().cloned());
                FunctionParam::with_span(
                    p.name.as_ref().map(|n| n.as_ref().to_string()),
                    ty.expr,
                    p.optional,
                    p.rest,
                    p.span,
                    false,
                )
            })
            .collect();
        let return_type = function.return_type.map(|r| {
            let r = r.prefix(MaterializePathSegment::FunctionReturn);
            degraded_leaves.extend(r.degraded_leaves.iter().cloned());
            Arc::new(r.expr)
        });
        let type_params: Vec<TypeParam> = function
            .type_parameters
            .into_iter()
            .enumerate()
            .map(|(i, tp)| {
                let constraint = tp.constraint.map(|c| {
                    let c = c.prefix(MaterializePathSegment::FunctionTypeParameter {
                        index: i as u32,
                        slot: TypeParameterSlot::Constraint,
                    });
                    degraded_leaves.extend(c.degraded_leaves.iter().cloned());
                    Arc::new(c.expr)
                });
                let default = tp.default.map(|d| {
                    let d = d.prefix(MaterializePathSegment::FunctionTypeParameter {
                        index: i as u32,
                        slot: TypeParameterSlot::Default,
                    });
                    degraded_leaves.extend(d.degraded_leaves.iter().cloned());
                    Arc::new(d.expr)
                });
                TypeParam {
                    name: tp.name.as_ref().to_string(),
                    constraint,
                    default,
                }
            })
            .collect();
        MaterializedFunction {
            function: Arc::new(FunctionExpr::with_spans(
                parameters,
                return_type,
                type_params,
                FunctionSpans {
                    signature: function.signature_span,
                    return_type: function.return_type_span,
                },
            )),
            degraded_leaves,
        }
    }
    fn function_to_out(&mut self, function: MaterializedFunction) -> MaterializedTypeExpr {
        MaterializedTypeExpr {
            expr: TypeExpr::Function(function.function),
            degraded_leaves: function.degraded_leaves,
        }
    }
    fn constructor_to_out(&mut self, function: MaterializedFunction) -> MaterializedTypeExpr {
        MaterializedTypeExpr {
            expr: TypeExpr::ConstructorType(function.function),
            degraded_leaves: function.degraded_leaves,
        }
    }
    fn out_as_function(&self, out: &MaterializedTypeExpr) -> Option<MaterializedFunction> {
        match out.expr() {
            TypeExpr::Function(function) => Some(MaterializedFunction {
                function: Arc::clone(function),
                degraded_leaves: out.degraded_leaves.clone(),
            }),
            _ => None,
        }
    }

    fn out_as_constructor(&self, out: &MaterializedTypeExpr) -> Option<MaterializedFunction> {
        match out.expr() {
            TypeExpr::ConstructorType(function) => Some(MaterializedFunction {
                function: Arc::clone(function),
                degraded_leaves: out.degraded_leaves.clone(),
            }),
            _ => None,
        }
    }

    fn member_property(
        &mut self,
        key: verter_type_expr::AuthoredPropertyKey<
            MaterializedTypeExpr,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        ty: MaterializedTypeExpr,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        excess_origin: verter_type_expr::ExcessPropertyOrigin,
        spans: verter_type_expr::MemberSpans,
    ) -> MaterializedObjectMember {
        let mut degraded_leaves: Vec<_> = ty
            .degraded_leaves
            .into_iter()
            .map(|leaf| PendingMemberDegradation {
                slot: ObjectMemberSlot::Value,
                leaf,
            })
            .collect();
        let key = key.map(
            |computed| {
                degraded_leaves.extend(computed.degraded_leaves.into_iter().map(|leaf| {
                    PendingMemberDegradation {
                        slot: ObjectMemberSlot::Key,
                        leaf,
                    }
                }));
                computed.expr
            },
            |identity| identity,
        );
        MaterializedObjectMember {
            member: verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::with_key_visibility(
                    key, ty.expr, optional, readonly, visibility, spans,
                )
                // Verbatim thread-through of the recorded provenance
                // (lossless raise round-trip).
                .with_excess_origin(excess_origin),
            ),
            degraded_leaves,
        }
    }
    fn member_method(
        &mut self,
        key: verter_type_expr::AuthoredPropertyKey<
            MaterializedTypeExpr,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
        function: MaterializedFunction,
        optional: bool,
        method_kind: verter_type_expr::ObjectMethodKind,
        has_implementation_body: bool,
        visibility: MemberVisibility,
        excess_origin: verter_type_expr::ExcessPropertyOrigin,
        spans: verter_type_expr::MemberSpans,
    ) -> MaterializedObjectMember {
        let mut degraded_leaves: Vec<_> = function
            .degraded_leaves
            .into_iter()
            .map(|leaf| PendingMemberDegradation {
                slot: ObjectMemberSlot::Value,
                leaf,
            })
            .collect();
        let key = key.map(
            |computed| {
                degraded_leaves.extend(computed.degraded_leaves.into_iter().map(|leaf| {
                    PendingMemberDegradation {
                        slot: ObjectMemberSlot::Key,
                        leaf,
                    }
                }));
                computed.expr
            },
            |identity| identity,
        );
        let mut signature = verter_type_expr::MethodSignature::with_key_visibility(
            key,
            (*function.function).clone(),
            optional,
            visibility,
            spans,
        );
        signature.method_kind = method_kind;
        signature.has_implementation_body = has_implementation_body;
        MaterializedObjectMember {
            member: verter_type_expr::ObjectMember::Method(
                signature.with_excess_origin(excess_origin),
            ),
            degraded_leaves,
        }
    }
    fn member_spread(&mut self, ty: MaterializedTypeExpr) -> MaterializedObjectMember {
        let degraded_leaves = ty
            .degraded_leaves
            .into_iter()
            .map(|leaf| PendingMemberDegradation {
                slot: ObjectMemberSlot::Value,
                leaf,
            })
            .collect();
        MaterializedObjectMember {
            member: verter_type_expr::ObjectMember::Spread(verter_type_expr::SpreadMember::new(
                ty.expr,
            )),
            degraded_leaves,
        }
    }
    fn member_call_signature(
        &mut self,
        function: MaterializedFunction,
    ) -> MaterializedObjectMember {
        let function_expr = verter_type_expr::FunctionExpr::with_spans(
            function.function.parameters.clone(),
            function.function.return_type.clone(),
            function.function.type_parameters.clone(),
            function.function.spans,
        );
        let degraded_leaves = function
            .degraded_leaves
            .into_iter()
            .map(|leaf| PendingMemberDegradation {
                slot: ObjectMemberSlot::Value,
                leaf,
            })
            .collect();
        MaterializedObjectMember {
            member: verter_type_expr::ObjectMember::CallSignature(function_expr),
            degraded_leaves,
        }
    }
    fn member_construct_signature(
        &mut self,
        function: MaterializedFunction,
    ) -> MaterializedObjectMember {
        let function_expr = verter_type_expr::FunctionExpr::with_spans(
            function.function.parameters.clone(),
            function.function.return_type.clone(),
            function.function.type_parameters.clone(),
            function.function.spans,
        );
        let degraded_leaves = function
            .degraded_leaves
            .into_iter()
            .map(|leaf| PendingMemberDegradation {
                slot: ObjectMemberSlot::Value,
                leaf,
            })
            .collect();
        MaterializedObjectMember {
            member: verter_type_expr::ObjectMember::ConstructSignature(function_expr),
            degraded_leaves,
        }
    }
    fn member_index_signature(
        &mut self,
        key_name: String,
        key_type: MaterializedTypeExpr,
        value_type: MaterializedTypeExpr,
        readonly: bool,
        spans: verter_type_expr::IndexSignatureSpans,
    ) -> MaterializedObjectMember {
        let mut degraded_leaves = Vec::new();
        degraded_leaves.extend(key_type.degraded_leaves.iter().cloned().map(|leaf| {
            PendingMemberDegradation {
                slot: ObjectMemberSlot::IndexKey,
                leaf,
            }
        }));
        degraded_leaves.extend(value_type.degraded_leaves.into_iter().map(|leaf| {
            PendingMemberDegradation {
                slot: ObjectMemberSlot::IndexValue,
                leaf,
            }
        }));
        MaterializedObjectMember {
            member: verter_type_expr::ObjectMember::IndexSignature(
                verter_type_expr::IndexSignature::with_spans(
                    key_name,
                    key_type.expr,
                    value_type.expr,
                    readonly,
                    spans,
                ),
            ),
            degraded_leaves,
        }
    }
    fn object_from_members(
        &mut self,
        members: Vec<MaterializedObjectMember>,
    ) -> MaterializedTypeExpr {
        let mut degraded_leaves = Vec::new();
        let mut raw_members = Vec::with_capacity(members.len());
        for (i, member) in members.into_iter().enumerate() {
            for pending in member.degraded_leaves {
                degraded_leaves.push(pending.leaf.prefixed(MaterializePathSegment::ObjectMember {
                    index: i as u32,
                    slot: pending.slot,
                }));
            }
            raw_members.push(member.member);
        }
        MaterializedTypeExpr {
            expr: TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                properties: raw_members,
            })),
            degraded_leaves,
        }
    }

    fn absorb_dropped(
        &mut self,
        mut out: MaterializedTypeExpr,
        dropped: Vec<MaterializedTypeExpr>,
    ) -> MaterializedTypeExpr {
        for arm in dropped {
            out.degraded_leaves.extend(
                arm.degraded_leaves
                    .iter()
                    .map(|leaf| leaf.prefixed(MaterializePathSegment::DroppedStructure)),
            );
        }
        out
    }

    fn is_object_surface_sentinel(&self, out: &MaterializedTypeExpr) -> bool {
        // TYPED root-level check only: a genuine `UnknownValue` — even one
        // spelled identically to the legacy sentinel — is NEVER dropped, and
        // `QueryError::Other("semanticObjectSurface")` never acts as the
        // sentinel.
        matches!(
            out.root_degradation(),
            Some(QueryError::UnrepresentableSurface)
        )
    }
    fn is_empty_object(&self, out: &MaterializedTypeExpr) -> bool {
        matches!(out.expr(), TypeExpr::Object(object) if object.properties.is_empty())
    }
}

/// Fold `node` to a [`MaterializedTypeExpr`] through the shared
/// `MaterializeTypeExprAlg` — the entry the raise-side shell primitive
/// ([`super::ProjectSemanticDispatch::raise_node_to_type_expr`]) delegates to.
/// `None` when the node — or a `?`-propagating required child — is unavailable /
/// unraisable. (Named `fold_to_type_expr`, NOT `materialize_type_expr`, so it
/// does not collide with the `#[cfg(test)]` `materialize_type_expr(HotTypeRef)`
/// boundary the G-A guard pins to exactly one definition.)
pub(in crate::project_semantic_dispatch) fn fold_to_type_expr(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<MaterializedTypeExpr> {
    let mut alg = MaterializeTypeExprAlg;
    fold_node(&mut alg, dispatch, node, active)
}

#[cfg(test)]
mod tests {
    use verter_type_expr::{TypeExpr, UnknownValue};

    use super::{
        MaterializePathSegment, MaterializeTypeExprAlg, MaterializedTypeExpr, ObjectMemberSlot,
        RaisedShapeAlgebra,
    };
    use crate::resolver_core::component_meta_query_engine::{
        semantic_query_error_raw, SEMANTIC_OBJECT_SURFACE, SEMANTIC_SURFACE_MEMBER,
    };
    use crate::semantic_query::QueryError;

    /// The typed `opaque_sentinel` algebra entry point on the materializer:
    /// (1) the sidecar carries the TYPED [`QueryError`] at the expected
    /// (root) path, and (2) the terminal tree projects byte-identically to
    /// the legacy raw spelling — the materialization byte-identity contract
    /// for the swap, now with the typed reason as the only control channel.
    #[test]
    fn opaque_sentinel_materializes_byte_identical_legacy_raw() {
        // (variant, the exact legacy raw string the old literal emitted)
        let cases: &[(QueryError, &str)] = &[
            (QueryError::RaiseAliasCycle, "semanticAliasCycle"),
            (QueryError::TypeParamCycle, "semanticTypeParamCycle"),
            (QueryError::RaiseMiss, "<raise miss>"),
            (QueryError::UnrepresentableSurface, SEMANTIC_OBJECT_SURFACE),
            (
                QueryError::UnrepresentableSurfaceMember,
                SEMANTIC_SURFACE_MEMBER,
            ),
        ];

        for (variant, expected_raw) in cases {
            let mut alg = MaterializeTypeExprAlg;
            let produced = alg.opaque_sentinel(variant);
            let unmaterialized = crate::project_semantic_dispatch::raise_sentinel::query_error_is_unmaterialized_sentinel(variant);
            let (expr, leaves) = produced.into_parts();
            if unmaterialized {
                // (1) The sidecar carries the TYPED reason at the ROOT path …
                assert_eq!(
                    leaves.len(),
                    1,
                    "opaque_sentinel({variant:?}) must record exactly one root degradation leaf"
                );
                assert!(
                    leaves[0].path().is_empty(),
                    "the root degradation sits at the empty path"
                );
                assert!(
                    std::mem::discriminant(leaves[0].reason())
                        == std::mem::discriminant(variant),
                    "opaque_sentinel({variant:?}) must carry the same typed variant at the root, got {:?}",
                    leaves[0].reason()
                );
            } else {
                // … while a deliberately-MATERIALISED placeholder records NO
                // leaf (the partial channel agrees with the typed
                // `materialized` fact) — the tree bytes are identical either
                // way.
                assert!(
                    leaves.is_empty(),
                    "opaque_sentinel({variant:?}) is materialised-class ⇒ no sidecar leaf"
                );
            }
            // (2) The terminal tree is byte-equal to the legacy raw:
            // the same `Unknown` spelling `semantic_query_error_raw` maps the
            // variant to (JSON/display/hash all read the tree, so the
            // equality below pins all three byte surfaces).
            assert_eq!(
                expr,
                TypeExpr::Unknown(UnknownValue::compatibility_projection(*expected_raw)),
                "opaque_sentinel({variant:?}) terminal tree must spell {expected_raw:?}"
            );
            assert_eq!(
                expr,
                TypeExpr::Unknown(UnknownValue::compatibility_projection(
                    semantic_query_error_raw(variant)
                )),
                "opaque_sentinel({variant:?}) must agree with semantic_query_error_raw"
            );
            // The tree is raw-only identical to a genuine unknown with the
            // same spelling.
            let genuine = MaterializedTypeExpr::exact(TypeExpr::Unknown(
                UnknownValue::unsupported_syntax(*expected_raw),
            ));
            assert_eq!(genuine.expr(), &expr, "raw-only tree identity holds");
            assert!(
                !genuine.has_degradation(),
                "a genuine UnknownValue NEVER carries degradation"
            );
        }
    }

    /// The sidecar plumbing: a nested degraded leaf keeps its exact path and
    /// typed reason while the surrounding tree survives intact.
    #[test]
    fn nested_degradation_carries_exact_path_and_reason() {
        let mut alg = MaterializeTypeExprAlg;
        let degraded = alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember);
        let exact_arm = alg.primitive(verter_type_expr::PrimitiveName::String);
        let member = alg.union(vec![exact_arm, degraded]);
        let (expr, leaves) = member.into_parts();
        assert_eq!(
            expr,
            TypeExpr::Union(
                vec![
                    TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                    TypeExpr::Unknown(UnknownValue::compatibility_projection(
                        "semanticSurfaceMember"
                    )),
                ]
                .into()
            ),
        );
        assert_eq!(leaves.len(), 1);
        assert_eq!(
            leaves[0].path(),
            &[MaterializePathSegment::UnionArm(1)],
            "the nested leaf sits at UnionArm(1)"
        );
        assert!(
            matches!(leaves[0].reason(), QueryError::UnrepresentableSurfaceMember),
            "the typed reason survives the fold"
        );
    }

    /// F1 regression: a `TypeParam` with `constraint: None, default: Some(_)`
    /// must keep the two slots independent — the compat tree is `U = string`,
    /// never `U extends string` (the fold formerly rebuilt the two Options
    /// positionally from a flat vec, shifting the default into the constraint
    /// slot).
    #[test]
    fn type_parameter_default_only_keeps_slots_independent() {
        let mut alg = MaterializeTypeExprAlg;
        let default_value = alg.primitive(verter_type_expr::PrimitiveName::String);
        let out = alg.type_parameter(std::sync::Arc::from("U"), None, Some(default_value));
        let (expr, leaves) = out.into_parts();
        assert!(leaves.is_empty(), "an exact default carries no sidecar");
        let TypeExpr::TypeParameter(param) = &expr else {
            panic!("expected TypeParameter, got {expr:?}");
        };
        assert_eq!(param.name, "U");
        assert!(
            param.constraint.is_none(),
            "a missing constraint must stay ABSENT, got {:?}",
            param.constraint
        );
        assert_eq!(
            param.default.as_deref(),
            Some(&TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String
            )),
            "the default must stay in the DEFAULT slot"
        );
        // Bytes: the JSON wire carries the default WITHOUT a constraint key —
        // never `{"constraint": string}` (the corrupted pre-fix shape).
        let json = expr.to_json_value();
        assert_eq!(
            json,
            serde_json::json!({
                "kind": "typeParameter",
                "name": "U",
                "default": { "kind": "primitive", "name": "string" },
            }),
            "a lone default must wire as default-only"
        );
        // The (Some, Some) shape keeps both slots too.
        let mut alg = MaterializeTypeExprAlg;
        let constraint_value = alg.primitive(verter_type_expr::PrimitiveName::Number);
        let default_value = alg.primitive(verter_type_expr::PrimitiveName::String);
        let out = alg.type_parameter(
            std::sync::Arc::from("U"),
            Some(constraint_value),
            Some(default_value),
        );
        let (expr, _) = out.into_parts();
        let TypeExpr::TypeParameter(param) = &expr else {
            panic!("expected TypeParameter");
        };
        assert_eq!(
            param.constraint.as_deref(),
            Some(&TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::Number
            ))
        );
        assert_eq!(
            param.default.as_deref(),
            Some(&TypeExpr::Primitive(
                verter_type_expr::PrimitiveName::String
            ))
        );
    }

    /// A degraded member value preserves the surrounding
    /// object tree (sibling members intact) and records the EXACT
    /// `ObjectMember { index, slot: Value }` path with the typed reason.
    #[test]
    fn object_member_degradation_preserves_tree_and_exact_path() {
        let mut alg = MaterializeTypeExprAlg;
        let kept_value = alg.primitive(verter_type_expr::PrimitiveName::Number);
        let kept = alg.member_property(
            "kept".to_string().into(),
            kept_value,
            false,
            false,
            verter_type_expr::MemberVisibility::Public,
            verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            verter_type_expr::MemberSpans::default(),
        );
        let degraded_value = alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember);
        let broken = alg.member_property(
            "broken".to_string().into(),
            degraded_value,
            false,
            false,
            verter_type_expr::MemberVisibility::Public,
            verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            verter_type_expr::MemberSpans::default(),
        );
        let object = alg.object_from_members(vec![kept, broken]);
        let (expr, leaves) = object.into_parts();

        // The surrounding tree survives intact.
        let TypeExpr::Object(object) = &expr else {
            panic!("expected an object, got {expr:?}");
        };
        assert_eq!(object.properties.len(), 2);
        let verter_type_expr::ObjectMember::Property(kept) = &object.properties[0] else {
            panic!("member 0 must stay a property");
        };
        assert_eq!(kept.string_name().expect("string-key fixture"), "kept");
        assert_eq!(
            kept.ty,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        );
        let verter_type_expr::ObjectMember::Property(broken) = &object.properties[1] else {
            panic!("member 1 must stay a property");
        };
        assert_eq!(
            broken.ty,
            TypeExpr::Unknown(UnknownValue::compatibility_projection(
                "semanticSurfaceMember"
            )),
            "the degraded member value projects the byte-identical legacy spelling"
        );

        // The exact path + typed reason.
        assert_eq!(leaves.len(), 1);
        assert_eq!(
            leaves[0].path(),
            &[MaterializePathSegment::ObjectMember {
                index: 1,
                slot: ObjectMemberSlot::Value,
            }],
            "the degraded leaf sits at ObjectMember(index 1, Value)"
        );
        assert!(
            matches!(leaves[0].reason(), QueryError::UnrepresentableSurfaceMember),
            "the typed reason survives the member fold"
        );
    }
}
