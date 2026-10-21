//! The heap a graph payload owns beyond its inline record.
//!
//! A [`SemanticNodeData`] payload is charged its `Arc` block by the node
//! arena; everything the record points at — member arrays, names, quasis,
//! carrier paths, error text, identities — lives in further allocations.
//! [`PayloadHeap`] walks those allocations and reports each one to a
//! [`PayloadHeapVisitor`], following the conventions of
//! [`verter_session_query::retention::allocation`]:
//!
//! * a `Box` / `Vec` / `String` backing belongs to whoever holds it and is
//!   reported through [`PayloadHeapVisitor::owned`] by capacity; its contents
//!   are always visited;
//! * an allocation a borrower could clone out of the payload — a name, a
//!   quasi list, a pending substitution frame — is an owned handle charged
//!   by its own cell and reported through [`PayloadHeapVisitor::handle`];
//!   there is no `PayloadHeap` for a bare `Arc`, so a public raw `Arc` field
//!   fails to compile here;
//! * a raw `Arc` a payload keeps only behind a borrow ([`PayloadArray`], a
//!   carrier's private name and arguments, a surface's private lists) may be
//!   shared between payloads, so it is reported through
//!   [`PayloadHeapVisitor::shared`] keyed by its address with its own block
//!   cost, and its contents are visited only when the visitor asks;
//! * `SemanticNodeId` operands are graph edges, not heap: the arena charges
//!   the node they name.
//!
//! Every payload type routes every field through [`PayloadHeap`]. Leaf types
//! without heap are declared through `inline_only!`, which requires them to
//! be `Copy` — a type that gains an owned allocation stops being `Copy` and
//! fails to compile there.

#[cfg(test)]
use rustc_hash::FxHashSet;
use verter_session_query::retention::allocation::{
    arc_block_bytes, box_bytes, string_bytes, vec_bytes, AliasSet,
};

use super::composite::{CompositeKind, CompositeList};
use super::deferred_callable::{DeferredCallable, DeferredCallableParts};
use super::{
    AuthoredAccessorEffect, AuthoredIndexEffect, AuthoredMethodEffect, AuthoredPropertyEffect,
    ClassExpressionIdentity, ConditionalPendingSubstitution, DeclIdentity, EnumLiteralType,
    FunctionParam, IndexSignature, InferBinderId, InferBinderIdentity, MapperKey, NodeScopeId,
    ObjectConstructionEffect, ObjectSpreadProgram, PayloadArray, PendingSubstitutionFrame,
    QueryError, RecursiveRefArgs, ScopeId, SemanticNodeData, SignatureNodeOccurrence,
    SignatureReturnCarrier, SurfaceEntry, SurfaceMember, SurfaceView, SyntheticBindingId,
    TupleElement, TypeParamDecl, ValueRootKey,
};
use crate::project_semantic_dispatch::flow_return_fact::DeferredFlowReturnSource;

/// Receives the heap a graph payload owns beyond its inline record.
pub(crate) trait PayloadHeapVisitor {
    /// An allocation only this payload (or the enclosing shared allocation
    /// being visited) owns — a `Box`, `Vec`, `String` backing — of `bytes`.
    fn owned(&mut self, bytes: usize);
    /// A shared `Arc` allocation, keyed by its address
    /// (`Arc::as_ptr(..).cast::<()>() as usize`), whose own block costs
    /// `bytes` (counters + inline elements, NOT the heap its elements point
    /// at). Returns whether to descend into the allocation's contents: the
    /// visitor decides (it descends only the first time an allocation gains
    /// an owner, and, when releasing, only when it loses its last owner), so
    /// nested allocations are offered once per enclosing allocation, never
    /// once per path to it.
    fn shared(&mut self, key: usize, bytes: usize) -> bool;
    /// An owned, self-charged allocation ([`OwnedStr`], [`OwnedSlice`]):
    /// charged by its own cell, never by the payload. `offer` offers it to
    /// the visitor's measurement.
    ///
    /// [`OwnedStr`]: verter_type_expr::OwnedStr
    /// [`OwnedSlice`]: verter_type_expr::OwnedSlice
    fn handle(&mut self, offer: &mut dyn FnMut(&mut AliasSet) -> usize);
}

/// Payload types whose owned heap can be visited.
pub(crate) trait PayloadHeap {
    /// Report every allocation this value points at beyond its inline bytes.
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor);
}

/// The heap `data` keeps reachable beyond its inline record, each distinct
/// `Arc` allocation counted once — the payload's heap when it shares nothing
/// with another payload.
#[cfg(test)]
pub(crate) fn payload_heap_bytes(data: &SemanticNodeData) -> usize {
    let mut visitor = SingleChargeVisitor::default();
    data.visit_payload_heap(&mut visitor);
    visitor.bytes
}

/// Sums owned and first-seen shared bytes for one measurement. An owned
/// handle counts nothing: its own cell charges it.
#[cfg(test)]
#[derive(Default)]
struct SingleChargeVisitor {
    seen: FxHashSet<usize>,
    bytes: usize,
}

#[cfg(test)]
impl PayloadHeapVisitor for SingleChargeVisitor {
    fn owned(&mut self, bytes: usize) {
        self.bytes += bytes;
    }

    fn shared(&mut self, key: usize, bytes: usize) -> bool {
        let first = self.seen.insert(key);
        if first {
            self.bytes += bytes;
        }
        first
    }

    fn handle(&mut self, _offer: &mut dyn FnMut(&mut AliasSet) -> usize) {}
}

/// The address an `Arc` allocation is keyed by: its data pointer, which is
/// what `Arc::as_ptr` returns and what a borrowed view of its contents
/// starts at.
fn allocation_key<T: ?Sized>(data: *const T) -> usize {
    data.cast::<()>() as usize
}

/// Report a slice that is the whole contents of one `Arc<[T]>` allocation,
/// and its elements when the visitor descends.
fn visit_arc_slice<T: PayloadHeap>(slice: &[T], visitor: &mut dyn PayloadHeapVisitor) {
    let block = arc_block_bytes(std::mem::size_of_val(slice));
    if visitor.shared(allocation_key(slice.as_ptr()), block) {
        for item in slice {
            item.visit_payload_heap(visitor);
        }
    }
}

/// Report text that is the whole contents of one `Arc<str>` allocation.
fn visit_arc_str(text: &str, visitor: &mut dyn PayloadHeapVisitor) {
    // Leaf text: nothing to descend into.
    let _ = visitor.shared(allocation_key(text.as_ptr()), arc_block_bytes(text.len()));
}

impl PayloadHeap for verter_type_expr::OwnedStr {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visitor.handle(&mut |aliases| self.offer(aliases));
    }
}

impl<T: verter_session_query::retention::owned::Measured> PayloadHeap
    for verter_type_expr::OwnedSlice<T>
{
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visitor.handle(&mut |aliases| self.offer(aliases));
    }
}

impl<T: verter_session_query::retention::owned::Measured> PayloadHeap
    for verter_session_query::retention::owned::Owned<T>
{
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visitor.handle(&mut |aliases| self.offer(aliases));
    }
}

impl<T: PayloadHeap> PayloadHeap for PayloadArray<T> {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visit_arc_slice(self, visitor);
    }
}

impl<T: PayloadHeap> PayloadHeap for Box<T> {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visitor.owned(box_bytes::<T>());
        T::visit_payload_heap(self, visitor);
    }
}

impl<T: PayloadHeap> PayloadHeap for Vec<T> {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visitor.owned(vec_bytes(self));
        for item in self {
            item.visit_payload_heap(visitor);
        }
    }
}

impl PayloadHeap for String {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        visitor.owned(string_bytes(self));
    }
}

impl<T: PayloadHeap> PayloadHeap for Option<T> {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        if let Some(value) = self {
            value.visit_payload_heap(visitor);
        }
    }
}

impl<A: PayloadHeap, B: PayloadHeap> PayloadHeap for (A, B) {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        self.0.visit_payload_heap(visitor);
        self.1.visit_payload_heap(visitor);
    }
}

/// A leaf with no owned heap. `Copy` proves it: no owned allocation is
/// `Copy`.
fn inline_leaf_is_copy<T: Copy>() {}

/// Values whose every byte is inline.
macro_rules! inline_only {
    ($($t:ty),* $(,)?) => {
        $(
            const _: fn() = inline_leaf_is_copy::<$t>;

            impl PayloadHeap for $t {
                #[inline]
                fn visit_payload_heap(&self, _visitor: &mut dyn PayloadHeapVisitor) {}
            }
        )*
    };
}

inline_only!(
    bool,
    u8,
    u16,
    u32,
    u64,
    usize,
    f64,
    [u8; 16],
    super::SemanticNodeId,
    super::PrimitiveKind,
    super::CompilerIntrinsicTypeOp,
    super::OptionalityMod,
    super::ReadonlyMod,
    super::MapperKind,
    super::SignatureKind,
    super::SignaturePredicate,
    super::MacroOwnBodyStamp,
    super::MergeRoleStamp,
    super::BinderScopeId,
    super::NodeLocalScopeId,
    super::PartialReasonSet,
    super::SemanticQueryValueTag,
    super::CheckerDiagnostic,
    super::RecoveryBasis,
    super::fact_result::ExecutionAbort,
    super::composite::CompositeOriginCategory,
    super::index_key::CanonicalIndexInt,
    verter_span::Span,
    verter_type_expr::TopLevelOwnerId,
    verter_type_expr::MemberVisibility,
    verter_type_expr::ObjectMethodKind,
    verter_type_expr::MemberSpans,
    verter_type_expr::IndexSignatureSpans,
    verter_type_expr::ExcessPropertyOrigin,
    verter_type_expr::SyntheticCarrierSurfaceKind,
    verter_type_expr::locators::LocatorSymbolSpace,
    verter_type_expr::locators::TypeBodyPathStep,
    verter_type_expr::locators::MacroPayloadPosition,
    verter_session_query::inputs::budget::BudgetDomain,
    verter_type_expr::UnknownProvenance,
);

impl PayloadHeap for SemanticNodeData {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::IntrinsicApplication { op, args } => {
                op.visit_payload_heap(visitor);
                args.visit_payload_heap(visitor);
            }
            Self::Alias(target) => target.visit_payload_heap(visitor),
            Self::Object(surface) => surface.visit_payload_heap(visitor),
            Self::ObjectSpreadProgram(program) => program.visit_payload_heap(visitor),
            Self::Union(list) => list.visit_payload_heap(visitor),
            Self::Intersection(list) => list.visit_payload_heap(visitor),
            Self::Primitive(kind) => kind.visit_payload_heap(visitor),
            Self::Literal(literal) => literal.visit_payload_heap(visitor),
            Self::EnumLiteral(literal) => literal.visit_payload_heap(visitor),
            Self::Opaque(error) => error.visit_payload_heap(visitor),
            Self::Array { element, readonly } => {
                element.visit_payload_heap(visitor);
                readonly.visit_payload_heap(visitor);
            }
            Self::Tuple { elements, readonly } => {
                elements.visit_payload_heap(visitor);
                readonly.visit_payload_heap(visitor);
            }
            Self::TemplateLiteral {
                quasis,
                expressions,
            } => {
                quasis.visit_payload_heap(visitor);
                expressions.visit_payload_heap(visitor);
            }
            Self::KeyOf { base } => base.visit_payload_heap(visitor),
            Self::IndexedAccess { object, index } => {
                object.visit_payload_heap(visitor);
                index.visit_payload_heap(visitor);
            }
            Self::Mapped { source, mapper } => {
                source.visit_payload_heap(visitor);
                mapper.visit_payload_heap(visitor);
            }
            // The opaque carriers expose their fields only through the
            // sanctioned head / type-argument accessors.
            Self::TypeOf(_) => {
                if let Some((root, path)) = self.typeof_head() {
                    root.visit_payload_heap(visitor);
                    path.visit_payload_heap(visitor);
                }
                visit_arc_slice(self.carrier_type_args(), visitor);
            }
            Self::TypeOfNominal(_) => {
                if let Some((root, path)) = self.typeof_head() {
                    root.visit_payload_heap(visitor);
                    path.visit_payload_heap(visitor);
                }
                self.typeof_nominal_identity()
                    .visit_payload_heap_ref(visitor);
            }
            Self::BareRef(_) => {
                if let Some((name, scope)) = self.bare_ref_head() {
                    // The head lends the whole text of the private name.
                    visit_arc_str(name, visitor);
                    scope.visit_payload_heap(visitor);
                }
                visit_arc_slice(self.carrier_type_args(), visitor);
            }
            Self::ImportType(_) => {
                if let Some((specifier, qualifier, _typeof_query)) = self.import_type_head() {
                    specifier.visit_payload_heap(visitor);
                    qualifier.visit_payload_heap(visitor);
                }
                visit_arc_slice(self.carrier_type_args(), visitor);
            }
            Self::TypeParam {
                decl,
                param_index,
                constraint,
                default,
                display_name,
            } => {
                decl.visit_payload_heap(visitor);
                param_index.visit_payload_heap(visitor);
                constraint.visit_payload_heap(visitor);
                default.visit_payload_heap(visitor);
                display_name.visit_payload_heap(visitor);
            }
            Self::Infer {
                name,
                binder,
                constraint,
            } => {
                name.visit_payload_heap(visitor);
                binder.visit_payload_heap(visitor);
                constraint.visit_payload_heap(visitor);
            }
            Self::InferRef { name, binder } => {
                name.visit_payload_heap(visitor);
                binder.visit_payload_heap(visitor);
            }
            Self::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
                pending,
            } => {
                check.visit_payload_heap(visitor);
                extends.visit_payload_heap(visitor);
                true_branch_ref.visit_payload_heap(visitor);
                false_branch_ref.visit_payload_heap(visitor);
                distributive.visit_payload_heap(visitor);
                pending.visit_payload_heap(visitor);
            }
            Self::Signature {
                kind,
                params,
                return_type,
                type_parameters,
                occurrence,
                return_carrier,
                signature_span,
                return_type_span,
                predicate,
                is_abstract,
            } => {
                kind.visit_payload_heap(visitor);
                params.visit_payload_heap(visitor);
                return_type.visit_payload_heap(visitor);
                type_parameters.visit_payload_heap(visitor);
                occurrence.visit_payload_heap(visitor);
                return_carrier.visit_payload_heap(visitor);
                signature_span.visit_payload_heap(visitor);
                return_type_span.visit_payload_heap(visitor);
                predicate.visit_payload_heap(visitor);
                is_abstract.visit_payload_heap(visitor);
            }
            Self::DeferredCallable(callable) => callable.visit_payload_heap(visitor),
            Self::DeclRef { identity } => identity.visit_payload_heap(visitor),
            Self::InstantiationRef { base, args } => {
                base.visit_payload_heap(visitor);
                args.visit_payload_heap(visitor);
            }
            Self::ClassExpressionInstance {
                identity,
                type_arguments,
                surface,
            } => {
                identity.visit_payload_heap(visitor);
                type_arguments.visit_payload_heap(visitor);
                surface.visit_payload_heap(visitor);
            }
            Self::MergedDecl { contributors } => contributors.visit_payload_heap(visitor),
            Self::RawFallback { value } => value.visit_payload_heap(visitor),
            Self::SyntheticBinding { id, value_node } => {
                id.visit_payload_heap(visitor);
                value_node.visit_payload_heap(visitor);
            }
        }
    }
}

/// Visiting through an optional borrow.
trait VisitRef {
    fn visit_payload_heap_ref(self, visitor: &mut dyn PayloadHeapVisitor);
}

impl<T: PayloadHeap> VisitRef for Option<&T> {
    fn visit_payload_heap_ref(self, visitor: &mut dyn PayloadHeapVisitor) {
        if let Some(value) = self {
            value.visit_payload_heap(visitor);
        }
    }
}

impl PayloadHeap for SurfaceView {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            entries,
            members,
            call_signatures,
            construct_signatures,
            index_signatures,
            keyspace,
            has_index_signature,
        } = self;
        // The lists are private to the surface and borrowed as slices.
        visit_arc_slice(entries, visitor);
        visit_arc_slice(members, visitor);
        visit_arc_slice(call_signatures, visitor);
        visit_arc_slice(construct_signatures, visitor);
        visit_arc_slice(index_signatures, visitor);
        keyspace.visit_payload_heap(visitor);
        has_index_signature.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for SurfaceEntry {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::Member(member) => member.visit_payload_heap(visitor),
            Self::CallSignature(signature) | Self::ConstructSignature(signature) => {
                signature.visit_payload_heap(visitor);
            }
            Self::IndexSignature(signature) => signature.visit_payload_heap(visitor),
        }
    }
}

impl PayloadHeap for SurfaceMember {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            key,
            value,
            optional,
            readonly,
            method_kind,
            has_implementation_body,
            visibility,
            spans,
            declaration_origin,
            declared_in_macro_type_arg,
            merge_role,
            excess_origin,
        } = self;
        key.visit_payload_heap(visitor);
        value.visit_payload_heap(visitor);
        optional.visit_payload_heap(visitor);
        readonly.visit_payload_heap(visitor);
        method_kind.visit_payload_heap(visitor);
        has_implementation_body.visit_payload_heap(visitor);
        visibility.visit_payload_heap(visitor);
        spans.visit_payload_heap(visitor);
        declaration_origin.visit_payload_heap(visitor);
        declared_in_macro_type_arg.visit_payload_heap(visitor);
        merge_role.visit_payload_heap(visitor);
        excess_origin.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for IndexSignature {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            key_type,
            value_type,
            readonly,
            spans,
            declaration_origin,
        } = self;
        key_type.visit_payload_heap(visitor);
        value_type.visit_payload_heap(visitor);
        readonly.visit_payload_heap(visitor);
        spans.visit_payload_heap(visitor);
        declaration_origin.visit_payload_heap(visitor);
    }
}

impl<C: PayloadHeap, I: PayloadHeap> PayloadHeap for verter_type_expr::AuthoredPropertyKey<C, I> {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::String(text) => text.visit_payload_heap(visitor),
            Self::Number(index) => index.visit_payload_heap(visitor),
            Self::UniqueSymbol(identity) => identity.visit_payload_heap(visitor),
            Self::Computed(node) => node.visit_payload_heap(visitor),
        }
    }
}

impl PayloadHeap for verter_type_expr::facts::ValueDeclIdentityPart {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            canonical_id,
            owner,
            symbol,
            member_path,
        } = self;
        canonical_id.visit_payload_heap(visitor);
        owner.visit_payload_heap(visitor);
        symbol.visit_payload_heap(visitor);
        member_path.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for ObjectSpreadProgram {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            effects,
            is_literal: _,
        } = self;
        effects.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for ObjectConstructionEffect {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::DirectProperty(effect) => effect.visit_payload_heap(visitor),
            Self::DirectMethod(effect) => effect.visit_payload_heap(visitor),
            Self::DirectGet(effect) | Self::DirectSet(effect) => {
                effect.visit_payload_heap(visitor);
            }
            Self::DirectIndex(effect) => effect.visit_payload_heap(visitor),
            Self::DirectCall(node) | Self::DirectConstruct(node) | Self::Spread(node) => {
                node.visit_payload_heap(visitor);
            }
        }
    }
}

impl PayloadHeap for AuthoredPropertyEffect {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            key,
            value,
            optional,
            readonly,
            visibility,
            spans,
            declaration_origin,
            declared_in_macro_type_arg,
            merge_role,
            excess_origin,
        } = self;
        key.visit_payload_heap(visitor);
        value.visit_payload_heap(visitor);
        optional.visit_payload_heap(visitor);
        readonly.visit_payload_heap(visitor);
        visibility.visit_payload_heap(visitor);
        spans.visit_payload_heap(visitor);
        declaration_origin.visit_payload_heap(visitor);
        declared_in_macro_type_arg.visit_payload_heap(visitor);
        merge_role.visit_payload_heap(visitor);
        excess_origin.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for AuthoredMethodEffect {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            key,
            signature,
            optional,
            has_implementation_body,
            visibility,
            spans,
            declaration_origin,
            declared_in_macro_type_arg,
            merge_role,
            excess_origin,
        } = self;
        key.visit_payload_heap(visitor);
        signature.visit_payload_heap(visitor);
        optional.visit_payload_heap(visitor);
        has_implementation_body.visit_payload_heap(visitor);
        visibility.visit_payload_heap(visitor);
        spans.visit_payload_heap(visitor);
        declaration_origin.visit_payload_heap(visitor);
        declared_in_macro_type_arg.visit_payload_heap(visitor);
        merge_role.visit_payload_heap(visitor);
        excess_origin.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for AuthoredAccessorEffect {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            key,
            signature,
            optional,
            has_implementation_body,
            visibility,
            spans,
            declaration_origin,
            declared_in_macro_type_arg,
            merge_role,
            excess_origin,
        } = self;
        key.visit_payload_heap(visitor);
        signature.visit_payload_heap(visitor);
        optional.visit_payload_heap(visitor);
        has_implementation_body.visit_payload_heap(visitor);
        visibility.visit_payload_heap(visitor);
        spans.visit_payload_heap(visitor);
        declaration_origin.visit_payload_heap(visitor);
        declared_in_macro_type_arg.visit_payload_heap(visitor);
        merge_role.visit_payload_heap(visitor);
        excess_origin.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for AuthoredIndexEffect {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            key_type,
            value_type,
            readonly,
            spans,
            declaration_origin,
        } = self;
        key_type.visit_payload_heap(visitor);
        value_type.visit_payload_heap(visitor);
        readonly.visit_payload_heap(visitor);
        spans.visit_payload_heap(visitor);
        declaration_origin.visit_payload_heap(visitor);
    }
}

impl<K: CompositeKind> PayloadHeap for CompositeList<K> {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        // The member list is reachable only by borrow; the borrow is the
        // whole contents of its shared allocation.
        self.origin_category().visit_payload_heap(visitor);
        let members: &[super::SemanticNodeId] = self;
        visit_arc_slice(members, visitor);
    }
}

impl PayloadHeap for verter_type_expr::LiteralValue {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::String(text) | Self::BigInt(text) => text.visit_payload_heap(visitor),
            Self::Number(value) => value.visit_payload_heap(visitor),
            Self::Boolean(value) => value.visit_payload_heap(visitor),
        }
    }
}

impl PayloadHeap for EnumLiteralType {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            enum_decl,
            module_specifier,
            member,
            base,
            member_count,
        } = self;
        enum_decl.visit_payload_heap(visitor);
        module_specifier.visit_payload_heap(visitor);
        member.visit_payload_heap(visitor);
        base.visit_payload_heap(visitor);
        member_count.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for DeclIdentity {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            canonical_id,
            owner,
            whole_hash,
            decl_name,
        } = self;
        canonical_id.visit_payload_heap(visitor);
        owner.visit_payload_heap(visitor);
        whole_hash.visit_payload_heap(visitor);
        decl_name.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for QueryError {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::ExecutionAborted(abort) => abort.visit_payload_heap(visitor),
            Self::UnsupportedIntrinsic { name } | Self::Other(name) => {
                name.visit_payload_heap(visitor);
            }
            Self::BudgetExceeded(failure) => {
                let verter_session_query::inputs::budget::BudgetExceededFailure {
                    domain,
                    limit,
                    actual,
                    context,
                } = failure;
                domain.visit_payload_heap(visitor);
                limit.visit_payload_heap(visitor);
                actual.visit_payload_heap(visitor);
                context.visit_payload_heap(visitor);
            }
            Self::UnstableState { attempts } => attempts.visit_payload_heap(visitor),
            Self::IncompleteSemanticOperand { reasons } => reasons.visit_payload_heap(visitor),
            Self::AliasCycle { chain } => chain.visit_payload_heap(visitor),
            Self::RecursiveRef { name, args } => {
                name.visit_payload_heap(visitor);
                args.visit_payload_heap(visitor);
            }
            Self::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            } => {
                canonical_id.visit_payload_heap(visitor);
                owner.visit_payload_heap(visitor);
                name.visit_payload_heap(visitor);
                whole_hash.visit_payload_heap(visitor);
            }
            Self::ValueDomainMismatch { expected, actual } => {
                expected.visit_payload_heap(visitor);
                actual.visit_payload_heap(visitor);
            }
            Self::CheckerRecovery {
                diagnostic,
                basis,
                origin,
            } => {
                diagnostic.visit_payload_heap(visitor);
                basis.visit_payload_heap(visitor);
                origin.visit_payload_heap(visitor);
            }
            Self::Miss
            | Self::Cancelled
            | Self::ForeignSemanticOperand
            | Self::StaleSemanticOperand
            | Self::RaiseAliasCycle
            | Self::TypeParamCycle
            | Self::RaiseMiss
            | Self::UnrepresentableSurface
            | Self::UnrepresentableSurfaceMember
            | Self::OpenSurface
            | Self::UnmodeledPosition
            | Self::PermissiveWildcard => {}
        }
    }
}

impl PayloadHeap for RecursiveRefArgs {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        self.operands_owned().visit_payload_heap(visitor);
    }
}

impl PayloadHeap for TupleElement {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            label,
            value,
            optional,
            rest,
        } = self;
        label.visit_payload_heap(visitor);
        value.visit_payload_heap(visitor);
        optional.visit_payload_heap(visitor);
        rest.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for MapperKey {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            parameter_node,
            key_space,
            value_expr,
            optionality,
            readonly,
            name_remap,
            kind,
            over_type_variable,
            modifiers_type,
        } = self;
        parameter_node.visit_payload_heap(visitor);
        key_space.visit_payload_heap(visitor);
        value_expr.visit_payload_heap(visitor);
        optionality.visit_payload_heap(visitor);
        readonly.visit_payload_heap(visitor);
        name_remap.visit_payload_heap(visitor);
        kind.visit_payload_heap(visitor);
        over_type_variable.visit_payload_heap(visitor);
        modifiers_type.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for ValueRootKey {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self { scope, name } = self;
        scope.visit_payload_heap(visitor);
        name.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for ScopeId {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            canonical_id,
            owner,
            local_scope,
            binder_scope_id,
        } = self;
        canonical_id.visit_payload_heap(visitor);
        owner.visit_payload_heap(visitor);
        local_scope.visit_payload_heap(visitor);
        binder_scope_id.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for NodeScopeId {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::Global => {}
            Self::File {
                canonical_id,
                owner,
                whole_hash,
                local_scope,
            } => {
                canonical_id.visit_payload_heap(visitor);
                owner.visit_payload_heap(visitor);
                whole_hash.visit_payload_heap(visitor);
                local_scope.visit_payload_heap(visitor);
            }
        }
    }
}

impl PayloadHeap for InferBinderId {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        self.0.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for InferBinderIdentity {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::Authored {
                scope,
                locator,
                declaration_path,
            } => {
                scope.visit_payload_heap(visitor);
                locator.visit_payload_heap(visitor);
                declaration_path.visit_payload_heap(visitor);
            }
            Self::Transient {
                scope,
                exact_typed_root,
                declaration_path,
            } => {
                scope.visit_payload_heap(visitor);
                // A private identity field, never lent out.
                visit_arc_slice(exact_typed_root, visitor);
                declaration_path.visit_payload_heap(visitor);
            }
            #[cfg(any(test, feature = "test-support"))]
            Self::Synthetic(ordinal) => ordinal.visit_payload_heap(visitor),
        }
    }
}

impl PayloadHeap for verter_type_expr::locators::AuthoredAnchor {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            canonical_id,
            owner,
            symbol,
            space,
        } = self;
        canonical_id.visit_payload_heap(visitor);
        owner.visit_payload_heap(visitor);
        symbol.visit_payload_heap(visitor);
        space.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for verter_type_expr::locators::TypeBodySlot {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self { anchor, path } = self;
        anchor.visit_payload_heap(visitor);
        path.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for verter_type_expr::locators::AuthoredBodyLocator {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        use verter_type_expr::locators::{
            AugmentationBodyLocator, AuthoredAugmentationScope, JsdocTypedefBodyLocator,
            MacroPayloadLocator,
        };
        match self {
            Self::DeclBody(slot) => slot.visit_payload_heap(visitor),
            Self::AugmentationBody(AugmentationBodyLocator {
                anchor,
                scope,
                path,
            }) => {
                anchor.visit_payload_heap(visitor);
                match scope {
                    AuthoredAugmentationScope::Global => {}
                    AuthoredAugmentationScope::Module { specifier } => {
                        specifier.visit_payload_heap(visitor);
                    }
                }
                path.visit_payload_heap(visitor);
            }
            Self::JsdocTypedefBody(JsdocTypedefBodyLocator { anchor, path }) => {
                anchor.visit_payload_heap(visitor);
                path.visit_payload_heap(visitor);
            }
            Self::MacroPayload(MacroPayloadLocator {
                anchor,
                macro_index,
                payload,
            }) => {
                anchor.visit_payload_heap(visitor);
                macro_index.visit_payload_heap(visitor);
                payload.visit_payload_heap(visitor);
            }
        }
    }
}

impl PayloadHeap for ConditionalPendingSubstitution {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            true_branch,
            false_branch,
            distribution,
        } = self;
        true_branch.visit_payload_heap(visitor);
        false_branch.visit_payload_heap(visitor);
        distribution.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for PendingSubstitutionFrame {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self { pairs } = self;
        pairs.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for FunctionParam {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            name,
            ty,
            optional,
            rest,
            span,
            declared_literal,
        } = self;
        name.visit_payload_heap(visitor);
        ty.visit_payload_heap(visitor);
        optional.visit_payload_heap(visitor);
        rest.visit_payload_heap(visitor);
        span.visit_payload_heap(visitor);
        declared_literal.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for TypeParamDecl {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            name,
            param,
            constraint,
            default,
            is_const,
        } = self;
        name.visit_payload_heap(visitor);
        param.visit_payload_heap(visitor);
        constraint.visit_payload_heap(visitor);
        default.visit_payload_heap(visitor);
        is_const.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for SignatureNodeOccurrence {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            function,
            signature_ordinal,
        } = self;
        function.visit_payload_heap(visitor);
        signature_ordinal.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for verter_type_expr::facts::FlowFunctionReturnIdentity {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            anchor,
            function_part,
            overload_ordinal,
        } = self;
        anchor.visit_payload_heap(visitor);
        function_part.visit_payload_heap(visitor);
        overload_ordinal.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for verter_type_expr::facts::FunctionPartIdentity {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::DeclarationBody | Self::Initializer => {}
            Self::Member { member_path } => member_path.visit_payload_heap(visitor),
            Self::Other { ordinal } => ordinal.visit_payload_heap(visitor),
        }
    }
}

impl PayloadHeap for verter_type_expr::facts::FunctionReturnSource {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        use verter_type_expr::locators::FunctionReturnLocator;
        match self {
            Self::Declared(
                FunctionReturnLocator::Authored(slot) | FunctionReturnLocator::Jsdoc(slot),
            ) => {
                slot.visit_payload_heap(visitor);
            }
            Self::Flow(identity) => identity.visit_payload_heap(visitor),
            Self::Absent => {}
        }
    }
}

impl PayloadHeap for SignatureReturnCarrier {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        match self {
            Self::Declared(node) => node.visit_payload_heap(visitor),
            Self::Function(source) => source.visit_payload_heap(visitor),
            Self::DeferredFlow(source) | Self::EvaluatedFlow(source) => {
                source.visit_payload_heap(visitor);
            }
        }
    }
}

impl PayloadHeap for DeferredFlowReturnSource {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        // Fields are private to their module; its accessors expose each one,
        // the bindings as the whole contents of their shared allocation.
        self.source().visit_payload_heap(visitor);
        // The bindings are engine-internal; their names are counted with
        // the list that holds them.
        let bindings = self.bindings();
        let block = arc_block_bytes(std::mem::size_of_val(bindings));
        if visitor.shared(allocation_key(bindings.as_ptr()), block) {
            for (name, image) in bindings {
                visit_arc_str(name, visitor);
                image.visit_payload_heap(visitor);
            }
        }
        self.receiver().visit_payload_heap(visitor);
    }
}

impl PayloadHeap for DeferredCallable {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let DeferredCallableParts {
            kind,
            params,
            type_parameters,
            occurrence,
            return_carrier,
            signature_span,
            return_type_span,
            predicate,
            is_abstract,
        } = self.stable_identity_parts();
        kind.visit_payload_heap(visitor);
        // The parts are borrowed inside the engine only.
        visit_arc_slice(params, visitor);
        visit_arc_slice(type_parameters, visitor);
        occurrence.visit_payload_heap_ref(visitor);
        return_carrier.visit_payload_heap(visitor);
        signature_span.visit_payload_heap(visitor);
        return_type_span.visit_payload_heap(visitor);
        predicate.visit_payload_heap(visitor);
        is_abstract.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for ClassExpressionIdentity {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            canonical_id,
            owner,
            offset,
            name,
            outer_clauses,
            own_arity,
            constructor_visibility,
            prototype,
            object_literal,
        } = self;
        canonical_id.visit_payload_heap(visitor);
        owner.visit_payload_heap(visitor);
        offset.visit_payload_heap(visitor);
        name.visit_payload_heap(visitor);
        outer_clauses.visit_payload_heap(visitor);
        own_arity.visit_payload_heap(visitor);
        constructor_visibility.visit_payload_heap(visitor);
        prototype.visit_payload_heap(visitor);
        object_literal.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for verter_session_query::flow::policy::ClassExpressionClause {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            container,
            parameters,
        } = self;
        container.visit_payload_heap(visitor);
        parameters.visit_payload_heap(visitor);
    }
}

impl PayloadHeap for verter_type_expr::UnknownValue {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        // `raw` borrows the whole contents of its shared text allocation.
        visit_arc_str(self.raw(), visitor);
        self.provenance().visit_payload_heap(visitor);
    }
}

impl PayloadHeap for SyntheticBindingId {
    fn visit_payload_heap(&self, visitor: &mut dyn PayloadHeapVisitor) {
        let Self {
            scope_canonical_id,
            surface_kind,
            slot_name,
            binding_name,
        } = self;
        scope_canonical_id.visit_payload_heap(visitor);
        surface_kind.visit_payload_heap(visitor);
        slot_name.visit_payload_heap(visitor);
        binding_name.visit_payload_heap(visitor);
    }
}

#[cfg(test)]
#[path = "node_payload_footprint_tests.rs"]
mod tests;
