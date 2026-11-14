//! Record-level signature discovery: publishing candidates from neutral
//! input, the record helpers union and intersection composition build
//! composites with, a union signature's parameter composition, and a
//! declaration's heritage and merged-declaration concatenation. Signature
//! comparison and the union and intersection compositions are owned
//! machines in [`super::matching`].
//!
//! Everything here works over V2 records and is independent of the
//! semantic graph; the graph-facing subject walk lives with the dispatcher
//! and reaches the type graph only through [`DiscoveryTypes`]. Nothing in
//! this module reads a body: a candidate's result is a closed recipe.

use std::sync::Arc;

use crate::semantic_query::{
    CanonicalTypeSubstitution, IncompleteReason, SemanticNodeId, CONTEXT_FREE_EVALUATION,
};

use super::lifetime::{EpochBinding, SignatureStore, StoreError};
use super::positional::{PositionalMode, PositionalShape, SlotTypeFacts, TypeAt};
use super::provenance::{
    ArmIdentity, ConstituentSequence, DeclarationGroupId, DeclarationParentId, MappedConstituent,
    OriginRelation, OverloadOrder, SignatureProvenance,
};
use super::read_view::{ReadError, SemanticReadView};
use super::records::{
    BinderDeclaration, BinderSpace, ParameterLayout, ParameterOptionality, ParameterSlot, RestKind,
    RestSlot, ReturnObligationKey, SignatureCandidate, SignatureDescriptor, SignatureInputShape,
    SignatureKind, SignatureProvenanceId, SignatureResultRecipe, SignatureSemanticFlags,
    SignatureSetRef, SignatureTemplate, TypeToken,
};
use super::substitution::compose_canonical;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryError {
    Store(StoreError),
    Read(ReadError),
    Incomplete(IncompleteReason),
}

impl From<StoreError> for DiscoveryError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

impl From<ReadError> for DiscoveryError {
    fn from(e: ReadError) -> Self {
        Self::Read(e)
    }
}

impl DiscoveryError {
    /// The closed incomplete reason this error maps to.
    #[must_use]
    pub fn incomplete_reason(self) -> IncompleteReason {
        match self {
            Self::Incomplete(reason) => reason,
            Self::Store(StoreError::Cancelled) => IncompleteReason::Cancelled,
            Self::Store(StoreError::Overflow) => IncompleteReason::Budget,
            Self::Store(_) | Self::Read(_) => IncompleteReason::UnsettledInput,
        }
    }
}

pub(super) type Res<T> = Result<T, DiscoveryError>;

pub(super) fn undecided<T>() -> Res<T> {
    Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))
}

/// What discovery reads from the type graph without a dependency read.
/// Matching asks its holder for anything else
/// ([`super::matching::TypeRequest`]).
pub trait DiscoveryTypes: SlotTypeFacts {
    /// Whether two types are identical under a binder correspondence
    /// (`(a_binder, b_binder)` pairs), where the graph can tell without a
    /// read. `None` leaves the question to the matching machine's holder
    /// ([`super::matching::TypeRequest::Identical`]).
    fn identical_at_rest(
        &self,
        a: TypeToken,
        b: TypeToken,
        corr: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool>;
    /// One signature pair was decided.
    fn comparison_decided(&self);
    /// The intersection of parameter types, in order.
    fn intersect(&self, members: &[TypeToken]) -> Option<TypeToken>;
    /// Rewrite binder occurrences (`from -> to`) inside a type.
    fn map_binders(
        &self,
        ty: TypeToken,
        map: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<TypeToken>;
    fn unknown(&self) -> Option<TypeToken>;
    fn any(&self) -> Option<TypeToken>;
    fn is_any(&self, ty: TypeToken) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamInput {
    pub name: Option<Arc<str>>,
    pub ty: SemanticNodeId,
    pub optional: bool,
    pub includes_undefined: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestInput {
    pub name: Option<Arc<str>>,
    pub ty: SemanticNodeId,
    pub generic: bool,
    pub tail: Vec<ParamInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinderInput {
    pub name: Arc<str>,
    pub param: SemanticNodeId,
    pub constraint: Option<SemanticNodeId>,
    pub default: Option<SemanticNodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultInput {
    Declared {
        return_type: SemanticNodeId,
        predicate_or_assertion: Option<crate::semantic_query::SignaturePredicate>,
    },
    Body {
        locator: u64,
    },
}

/// Neutral description of one authored signature.
#[derive(Debug, Clone)]
pub struct SignatureInput {
    pub kind: SignatureKind,
    pub source: SemanticNodeId,
    pub space_key: u64,
    /// Full logical identity behind `space_key`; the key is a truncation.
    pub space_identity: u128,
    pub binders: Vec<BinderInput>,
    pub receiver: Option<ParamInput>,
    pub params: Vec<ParamInput>,
    pub rest: Option<RestInput>,
    pub flags: SignatureSemanticFlags,
    pub result: ResultInput,
    pub provenance: SignatureProvenance,
}

fn slot(store: &EpochBinding, p: &ParamInput) -> Res<ParameterSlot> {
    let name = match &p.name {
        Some(n) => Some(store.intern_spelling(n, None)?),
        None => None,
    };
    Ok(ParameterSlot {
        ty: store.intern_type_token(p.ty, None)?,
        optionality: ParameterOptionality {
            declared_optional: p.optional,
            includes_undefined: p.includes_undefined,
        },
        name,
    })
}

struct NoFacts;
impl SlotTypeFacts for NoFacts {
    fn accepts_void(&self, _: TypeToken) -> bool {
        false
    }
}

/// Publish one authored signature's records and return its candidate.
pub fn publish_signature(store: &EpochBinding, input: &SignatureInput) -> Res<SignatureCandidate> {
    let mut binders = Vec::with_capacity(input.binders.len());
    for b in &input.binders {
        binders.push(BinderDeclaration {
            spelling: store.intern_spelling(&b.name, None)?,
            constraint: match b.constraint {
                Some(c) => Some(store.intern_type_token(c, None)?),
                None => None,
            },
            default: match b.default {
                Some(d) => Some(store.intern_type_token(d, None)?),
                None => None,
            },
        });
    }
    // The key names the space's binder tokens. A space with no binders
    // mints none, so another identity under its key can alias nothing:
    // only a space that will mint tokens claims its key.
    if !input.binders.is_empty() {
        store.claim_space_key(input.space_key, input.space_identity)?;
    }
    let space = store.intern_binder_space(
        BinderSpace {
            key: input.space_key,
            binders: binders.into_boxed_slice(),
        },
        None,
    )?;
    let mut env = Vec::with_capacity(input.binders.len());
    for (ordinal, b) in input.binders.iter().enumerate() {
        env.push((
            b.param,
            SignatureStore::binder_token(input.space_key, ordinal as u32)?,
        ));
    }
    let environment = store.intern_environment(
        CanonicalTypeSubstitution::new_in_arena(store.graph_arena().as_ref(), env),
        None,
    )?;

    let mut parameters = Vec::with_capacity(input.params.len());
    for p in &input.params {
        parameters.push(slot(store, p)?);
    }
    let rest = match &input.rest {
        None => None,
        Some(r) => {
            let name = match &r.name {
                Some(n) => Some(store.intern_spelling(n, None)?),
                None => None,
            };
            let mut tail = Vec::with_capacity(r.tail.len());
            for t in &r.tail {
                tail.push(slot(store, t)?);
            }
            Some(RestSlot {
                slot: ParameterSlot {
                    ty: store.intern_type_token(r.ty, None)?,
                    optionality: ParameterOptionality::required(),
                    name,
                },
                kind: if r.generic {
                    RestKind::GenericTuple
                } else {
                    RestKind::Array
                },
                tail: tail.into_boxed_slice(),
            })
        }
    };
    let layout_record = ParameterLayout {
        parameters: parameters.into_boxed_slice(),
        rest,
    };
    let receiver = match &input.receiver {
        Some(p) => Some(slot(store, p)?),
        None => None,
    };
    let declared_minimum =
        PositionalShape::new(&layout_record, receiver, input.flags, &NoFacts).declared_minimum();
    let layout = store.intern_layout(layout_record, None)?;
    let this_parameter = match receiver {
        Some(r) => Some(store.intern_slot(r, None)?),
        None => None,
    };
    let shape = store.intern_shape(
        SignatureInputShape {
            kind: input.kind,
            binder_declarations: space,
            this_parameter,
            parameter_layout: layout,
            declared_minimum,
            signature_semantic_flags: input.flags,
        },
        None,
    )?;
    let recipe = match input.result {
        ResultInput::Declared {
            return_type,
            predicate_or_assertion,
        } => SignatureResultRecipe::Declared {
            return_type: store.intern_type_token(return_type, None)?,
            predicate_or_assertion: match predicate_or_assertion {
                Some(predicate) => Some(super::records::PredicateEffect {
                    subject: predicate.subject,
                    asserts: predicate.asserts,
                    ty: predicate
                        .ty
                        .map(|node| store.intern_type_token(node, None))
                        .transpose()?,
                }),
                None => None,
            },
        },
        ResultInput::Body { locator } => SignatureResultRecipe::Body {
            return_obligation_key: ReturnObligationKey {
                body_locator: store.intern_body_locator(locator, None)?,
                evaluation: CONTEXT_FREE_EVALUATION,
            },
        },
    };
    let recipe = store.intern_recipe(recipe, None)?;
    let template = store.intern_template(
        SignatureTemplate {
            input_shape: shape,
            result_recipe: recipe,
        },
        None,
    )?;
    let descriptor = store.intern_descriptor(
        SignatureDescriptor {
            template,
            declaration_environment: environment,
            residual_binders: space,
        },
        None,
    )?;
    store.record_descriptor_source(descriptor, store.intern_type_token(input.source, None)?)?;
    let provenance = store.intern_provenance(input.provenance, None)?;
    Ok(store.candidate(descriptor, provenance)?)
}

/// `Empty | One | Many` from an ordered candidate list.
pub fn set_from_candidates(
    store: &EpochBinding,
    candidates: Vec<SignatureCandidate>,
) -> Res<SignatureSetRef> {
    Ok(match candidates.len() {
        0 => SignatureSetRef::Empty,
        1 => SignatureSetRef::One(candidates[0]),
        _ => store.set_ref_many(candidates.into_boxed_slice(), None)?,
    })
}

/// A candidate with its records loaded from a pinned view.
pub(super) struct Loaded<'v> {
    pub(super) candidate: SignatureCandidate,
    pub(super) shape: &'v SignatureInputShape,
    pub(super) layout: &'v ParameterLayout,
    pub(super) space: &'v BinderSpace,
    pub(super) env: &'v CanonicalTypeSubstitution,
    pub(super) recipe: &'v SignatureResultRecipe,
    pub(super) receiver: Option<ParameterSlot>,
    pub(super) descriptor: &'v SignatureDescriptor,
}

pub(super) fn load<'v>(
    view: &'v SemanticReadView,
    candidate: SignatureCandidate,
) -> Res<Loaded<'v>> {
    let descriptor = view.descriptor(candidate.signature)?;
    let template = view.template(descriptor.template)?;
    let shape = view.shape(template.input_shape)?;
    let receiver = match shape.this_parameter {
        Some(id) => Some(*view.slot(id)?),
        None => None,
    };
    Ok(Loaded {
        candidate,
        shape,
        layout: view.layout(shape.parameter_layout)?,
        space: view.space(descriptor.residual_binders)?,
        env: view.environment(descriptor.declaration_environment)?,
        recipe: view.recipe(template.result_recipe)?,
        receiver,
        descriptor,
    })
}

impl Loaded<'_> {
    pub(super) fn positional<'a>(&'a self, facts: &'a dyn SlotTypeFacts) -> PositionalShape<'a> {
        PositionalShape::new(
            self.layout,
            self.receiver,
            self.shape.signature_semantic_flags,
            facts,
        )
    }

    /// Declared binder nodes in ordinal order (from the environment: each
    /// binder maps to `binder_token(space_key, ordinal)`).
    pub(super) fn binder_nodes(&self) -> Option<Vec<(u32, SemanticNodeId)>> {
        let mut out: Vec<(u32, SemanticNodeId)> = self
            .env
            .bindings()
            .iter()
            .map(|(param, token)| {
                SignatureStore::binder_token_ordinal(*token).map(|ordinal| (ordinal, *param))
            })
            .collect::<Option<_>>()?;
        out.sort_by_key(|(ordinal, _)| *ordinal);
        Some(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchOptions {
    pub partial_match: bool,
    pub ignore_this_types: bool,
    pub ignore_return_types: bool,
}

impl MatchOptions {
    pub const EXACT: Self = Self {
        partial_match: false,
        ignore_this_types: false,
        ignore_return_types: false,
    };
    pub const EXACT_IGNORING_RETURNS: Self = Self {
        partial_match: false,
        ignore_this_types: false,
        ignore_return_types: true,
    };
    pub const PARTIAL_IGNORING_RETURNS: Self = Self {
        partial_match: true,
        ignore_this_types: false,
        ignore_return_types: true,
    };
}

pub(super) fn source_token(store: &EpochBinding, c: SignatureCandidate) -> Res<TypeToken> {
    match store.descriptor_source(c.signature)? {
        Some(token) => Ok(token),
        None => undecided(),
    }
}

fn composite_provenance(
    store: &EpochBinding,
    view: &SemanticReadView,
    rep: SignatureCandidate,
    ordinal: u32,
) -> Res<SignatureProvenanceId> {
    let base = *view.provenance(rep.provenance)?;
    Ok(store.intern_provenance(
        SignatureProvenance {
            origin: OriginRelation::Synthesized {
                from: rep.provenance,
            },
            overload_ordinal: ordinal,
            effective_overload_order: OverloadOrder {
                group: base.declaration_group,
                ordinal,
            },
            ..base
        },
        None,
    )?)
}

/// Constituent `c` expressed in the representative's residual binder space.
fn residual_of(
    store: &EpochBinding,
    view: &SemanticReadView,
    rep: &Loaded<'_>,
    c: &Loaded<'_>,
) -> Res<SignatureDescriptorIdOrSame> {
    if c.descriptor.residual_binders == rep.descriptor.residual_binders
        || c.space.binders.is_empty()
    {
        return Ok(SignatureDescriptorIdOrSame::Same);
    }
    let Some(rb) = rep.binder_nodes() else {
        return undecided();
    };
    let Some(cb) = c.binder_nodes() else {
        return undecided();
    };
    if cb.len() != rb.len() {
        return undecided();
    }
    let _ = view;
    let mut to_rep = Vec::new();
    for ((ordinal, _), (rep_ordinal, _)) in cb.iter().zip(rb.iter()) {
        to_rep.push((
            SignatureStore::binder_token(space_key(c.space), *ordinal)?,
            SignatureStore::binder_token(space_key(rep.space), *rep_ordinal)?,
        ));
    }
    let env = compose_canonical(
        c.env,
        &CanonicalTypeSubstitution::derived_from(&[c.env], to_rep),
    );
    let environment = store.intern_environment(env, None)?;
    let descriptor = store.intern_descriptor(
        SignatureDescriptor {
            template: c.descriptor.template,
            declaration_environment: environment,
            residual_binders: rep.descriptor.residual_binders,
        },
        None,
    )?;
    Ok(SignatureDescriptorIdOrSame::Other(descriptor))
}

fn space_key(space: &BinderSpace) -> u64 {
    space.key
}

enum SignatureDescriptorIdOrSame {
    Same,
    Other(super::records::SignatureDescriptorId),
}

pub(super) fn edges_for(
    store: &EpochBinding,
    view: &SemanticReadView,
    rep: SignatureCandidate,
    constituents: &[(u32, SignatureCandidate)],
) -> Res<super::records::ConstituentSequenceId> {
    let rep_loaded = load(view, rep)?;
    let mut edges = Vec::with_capacity(constituents.len());
    for &(arm, c) in constituents {
        let c_loaded = load(view, c)?;
        let residual = match residual_of(store, view, &rep_loaded, &c_loaded)? {
            SignatureDescriptorIdOrSame::Same => c.signature,
            SignatureDescriptorIdOrSame::Other(d) => d,
        };
        edges.push(MappedConstituent {
            arm: ArmIdentity {
                ordinal: arm,
                contributor: c.signature,
            },
            declaration: c.signature,
            residual,
        });
    }
    Ok(store.intern_sequence(
        ConstituentSequence {
            edges: edges.into_boxed_slice(),
        },
        None,
    )?)
}

pub(super) fn composite_candidate(
    store: &EpochBinding,
    view: &SemanticReadView,
    rep: SignatureCandidate,
    shape: super::records::SignatureInputShapeId,
    recipe: SignatureResultRecipe,
    ordinal: u32,
) -> Res<SignatureCandidate> {
    let rep_loaded = load(view, rep)?;
    let recipe = store.intern_recipe(recipe, None)?;
    let template = store.intern_template(
        SignatureTemplate {
            input_shape: shape,
            result_recipe: recipe,
        },
        None,
    )?;
    let descriptor = store.intern_descriptor(
        SignatureDescriptor {
            template,
            declaration_environment: rep_loaded.descriptor.declaration_environment,
            residual_binders: rep_loaded.descriptor.residual_binders,
        },
        None,
    )?;
    if let Some(source) = store.descriptor_source(rep.signature)? {
        store.record_descriptor_source(descriptor, source)?;
    }
    let provenance = composite_provenance(store, view, rep, ordinal)?;
    Ok(store.candidate(descriptor, provenance)?)
}

/// `combineSignaturesOfUnionMembers`, after the two signatures' binder
/// spaces matched: parameter types intersect position by position, the
/// minimum is the larger of the two, an extra rest element is added when
/// only the shorter side has a rest. `map_right` maps the right side's
/// binders onto the left's (empty unless both are generic). Binder mapping
/// and parameter intersection are read through `types`.
pub(super) fn combine_union_parameters(
    store: &EpochBinding,
    types: &dyn DiscoveryTypes,
    l: &Loaded<'_>,
    r: &Loaded<'_>,
    left: SignatureCandidate,
    map_right: &[(SemanticNodeId, SemanticNodeId)],
) -> Res<SignatureCandidate> {
    let lp = l.positional(types);
    let rp = r.positional(types);
    let (longest, shorter, right_is_longest) = if lp.parameter_count() >= rp.parameter_count() {
        (&lp, &rp, false)
    } else {
        (&rp, &lp, true)
    };
    let longest_count = longest.parameter_count();
    let either_rest = lp.has_rest() || rp.has_rest();
    let needs_extra_rest = either_rest && !longest.has_rest();
    let mapped = |ty: TypeToken, from_right: bool| -> Res<TypeToken> {
        if from_right && !map_right.is_empty() {
            match types.map_binders(ty, map_right) {
                Some(t) => Ok(t),
                None => undecided(),
            }
        } else {
            Ok(ty)
        }
    };
    let type_of =
        |shape: &PositionalShape<'_>, pos: usize, from_right: bool| -> Res<Option<TypeToken>> {
            Ok(match shape.type_at(pos) {
                TypeAt::One(s) => Some(mapped(s.ty, from_right)?),
                TypeAt::Run { element, tail } => {
                    let mut parts = vec![mapped(element, from_right)?];
                    for t in tail {
                        parts.push(mapped(t.ty, from_right)?);
                    }
                    match types.intersect(&parts) {
                        Some(t) => Some(t),
                        None => return undecided(),
                    }
                }
                TypeAt::GenericRest { rest, .. } => Some(mapped(rest, from_right)?),
                TypeAt::Absent => None,
            })
        };
    let min_l = lp.effective_minimum(PositionalMode::Comparison);
    let min_r = rp.effective_minimum(PositionalMode::Comparison);
    let mut parameters: Vec<ParameterSlot> = Vec::new();
    let mut rest_slot: Option<RestSlot> = None;
    for pos in 0..longest_count {
        let longest_ty = type_of(longest, pos, right_is_longest)?
            .ok_or(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))?;
        let shorter_ty = type_of(shorter, pos, !right_is_longest)?
            .or(types.unknown())
            .ok_or(DiscoveryError::Incomplete(IncompleteReason::Budget))?;
        let ty = match types.intersect(&[longest_ty, shorter_ty]) {
            Some(t) => t,
            None => return undecided(),
        };
        let is_rest = either_rest && !needs_extra_rest && pos + 1 == longest_count;
        let optional = pos >= min_l && pos >= min_r;
        let name = match longest.type_at(pos) {
            TypeAt::One(s) => s.name,
            _ => None,
        };
        let slot = ParameterSlot {
            ty,
            optionality: ParameterOptionality {
                declared_optional: optional && !is_rest,
                includes_undefined: optional && !is_rest,
            },
            name,
        };
        if is_rest {
            rest_slot = Some(RestSlot {
                slot,
                kind: RestKind::Array,
                tail: Box::from([]),
            });
        } else {
            parameters.push(slot);
        }
    }
    if needs_extra_rest {
        let ty = type_of(shorter, longest_count, !right_is_longest)?
            .ok_or(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))?;
        rest_slot = Some(RestSlot {
            slot: ParameterSlot::new(ty, ParameterOptionality::required()),
            kind: RestKind::Array,
            tail: Box::from([]),
        });
    }
    let receiver = match (l.receiver, r.receiver) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(a)) => Some(ParameterSlot {
            ty: mapped(a.ty, true)?,
            ..a
        }),
        (Some(a), Some(b)) => {
            let b_ty = mapped(b.ty, true)?;
            match types.intersect(&[a.ty, b_ty]) {
                Some(ty) => Some(ParameterSlot { ty, ..a }),
                None => return undecided(),
            }
        }
    };
    let layout_record = ParameterLayout {
        parameters: parameters.into_boxed_slice(),
        rest: rest_slot,
    };
    let flags = l
        .shape
        .signature_semantic_flags
        .union(r.shape.signature_semantic_flags);
    let mut declared_minimum =
        PositionalShape::new(&layout_record, receiver, flags, &NoFacts).declared_minimum();
    declared_minimum = declared_minimum.max(l.shape.declared_minimum.max(r.shape.declared_minimum));
    let layout = store.intern_layout(layout_record, None)?;
    let this_parameter = match receiver {
        Some(rc) => Some(store.intern_slot(rc, None)?),
        None => None,
    };
    let binder_space = if l.space.binders.is_empty() {
        r.shape.binder_declarations
    } else {
        l.shape.binder_declarations
    };
    let shape = store.intern_shape(
        SignatureInputShape {
            kind: l.shape.kind,
            binder_declarations: binder_space,
            this_parameter,
            parameter_layout: layout,
            declared_minimum,
            signature_semantic_flags: flags,
        },
        None,
    )?;
    let base = if l.space.binders.is_empty() { r } else { l };
    let recipe = store.intern_recipe(*l.recipe, None)?;
    let template = store.intern_template(
        SignatureTemplate {
            input_shape: shape,
            result_recipe: recipe,
        },
        None,
    )?;
    let descriptor = store.intern_descriptor(
        SignatureDescriptor {
            template,
            declaration_environment: base.descriptor.declaration_environment,
            residual_binders: base.descriptor.residual_binders,
        },
        None,
    )?;
    if let Some(source) = store.descriptor_source(left.signature)? {
        store.record_descriptor_source(descriptor, source)?;
    }
    Ok(store.candidate(descriptor, left.provenance)?)
}

/// Whether a member's construct list is a MIXIN constructor: exactly one
/// non-generic signature whose only parameter is an `any`-element array
/// rest (`new (...args: any[]) => X`). A `this` receiver is not a
/// parameter and does not take part (TypeScript's `isMixinConstructorType`).
pub(super) fn is_mixin_constructor(
    view: &SemanticReadView,
    types: &dyn DiscoveryTypes,
    list: &[SignatureCandidate],
) -> Res<bool> {
    let [only] = list else { return Ok(false) };
    let loaded = load(view, *only)?;
    Ok(loaded.space.binders.is_empty()
        && loaded.layout.parameters.is_empty()
        && loaded.layout.rest.as_ref().is_some_and(|r| {
            r.kind == RestKind::Array && r.tail.is_empty() && types.is_any(r.slot.ty)
        }))
}

/// The signatures of one kind an interface or class declaration with
/// `extends` heritage carries (TypeScript's `resolveObjectTypeMembers`).
/// `members` are the per-member candidate lists of the declaration's body
/// in body order — its bases in clause order, its own body LAST — and the
/// answer is the own body's signatures first, then each base's, in clause
/// order.
///
/// A declaration is not an intersection type, so nothing is dropped and
/// nothing is composed (an intersection's composition,
/// [`super::matching::SignatureComposition::intersection`], does both): a
/// signature identical to one already present stays, a base reached twice
/// through a diamond contributes twice, and a mixin constructor base keeps
/// its own construct signature. Call resolution therefore tries the own
/// signatures first, and conditional inference (`ReturnType`,
/// `InstanceType`), which reads the LAST signature, reads the last base's.
#[must_use]
pub fn heritage_signatures(members: &[Vec<SignatureCandidate>]) -> Vec<SignatureCandidate> {
    let Some((own, bases)) = members.split_last() else {
        return Vec::new();
    };
    own.iter().chain(bases.iter().flatten()).copied().collect()
}

/// The signatures of one symbol declared by several declarations —
/// interface declarations merged into one, a class and an interface of the
/// same name, a method overloaded across them. `declarations` are the
/// per-declaration candidate lists in declaration order, and the answer is
/// their concatenation (TypeScript's `getSignaturesOfSymbol` keeps every
/// declaration's signatures, identical ones included), each candidate
/// attributed to ONE declaration group — the merged symbol — and to its own
/// declaration inside it. Call resolution orders a merged symbol's
/// candidates by those identities ([`resolution_order`]).
pub fn merged_declaration_signatures(
    store: &EpochBinding,
    declarations: &[Vec<SignatureCandidate>],
) -> Res<Vec<SignatureCandidate>> {
    let view = store.view();
    let Some(first) = declarations.iter().flatten().next() else {
        return Ok(Vec::new());
    };
    // The symbol's group derives from its first signature's own group, so
    // it is as schedule-independent as that group, and never equal to it.
    let seed = view.provenance(first.provenance)?.declaration_group;
    let group = DeclarationGroupId::from_raw(merged_group_of(seed));
    let mut out = Vec::with_capacity(declarations.iter().map(Vec::len).sum());
    for (ordinal, list) in declarations.iter().enumerate() {
        let parent = DeclarationParentId::from_raw(ordinal as u64);
        for &candidate in list {
            let base = *view.provenance(candidate.provenance)?;
            let provenance = store.intern_provenance(
                SignatureProvenance {
                    declaration_group: group,
                    declaration_parent: parent,
                    effective_overload_order: OverloadOrder {
                        group,
                        ordinal: base.overload_ordinal,
                    },
                    ..base
                },
                None,
            )?;
            out.push(store.candidate(candidate.signature, provenance)?);
        }
    }
    Ok(out)
}

/// The declaration group of a merged symbol whose first signature's group
/// is `seed` (a fixed mix of it, distinct from it).
fn merged_group_of(seed: DeclarationGroupId) -> u64 {
    (seed.as_u64() ^ 0x6D65_7267_6564_5F73).rotate_left(29)
}

/// The order call resolution tries `candidates` in — TypeScript's
/// `reorderCandidates`, as a permutation of candidate positions.
///
/// Consecutive candidates of one declaration group (one symbol) whose
/// declaration changes start a new run at the front of that symbol's
/// candidates, so a later declaration of a merged symbol is tried first
/// while each declaration keeps its own order; a candidate of another
/// symbol starts after everything placed so far. A candidate that declares
/// a parameter of a literal type (`LITERAL_SPECIALIZATION`) is tried
/// before every non-specialized one, whatever its symbol. The candidate
/// list itself — the list conditional inference reads the LAST signature
/// of — keeps declaration order.
pub fn resolution_order(
    view: &SemanticReadView,
    candidates: &[SignatureCandidate],
) -> Res<Vec<usize>> {
    let mut order: Vec<usize> = Vec::with_capacity(candidates.len());
    let mut last_group: Option<DeclarationGroupId> = None;
    let mut last_parent: Option<DeclarationParentId> = None;
    let mut cutoff = 0usize;
    let mut index = 0usize;
    let mut specialized = 0usize;
    for (position, candidate) in candidates.iter().enumerate() {
        let provenance = view.provenance(candidate.provenance)?;
        let (group, parent) = (provenance.declaration_group, provenance.declaration_parent);
        if last_group.is_none() || last_group == Some(group) {
            if last_parent == Some(parent) {
                index += 1;
            } else {
                last_parent = Some(parent);
                index = cutoff;
            }
        } else {
            cutoff = order.len();
            index = cutoff;
            last_parent = Some(parent);
        }
        last_group = Some(group);
        let descriptor = view.descriptor(candidate.signature)?;
        let template = view.template(descriptor.template)?;
        let flags = view.shape(template.input_shape)?.signature_semantic_flags;
        let splice = if flags.contains(SignatureSemanticFlags::LITERAL_SPECIALIZATION) {
            specialized += 1;
            cutoff += 1;
            specialized - 1
        } else {
            index
        };
        order.insert(splice.min(order.len()), position);
    }
    Ok(order)
}
