//! Record-level signature discovery: publishing candidates from neutral
//! input, signature-equivalence comparison, union common-match then
//! restricted synthesis, and intersection dedup with constructor/mixin
//! composition.
//!
//! Everything here works over V2 records and is independent of the
//! semantic graph; the graph-facing subject walk lives with the dispatcher
//! and reaches the type graph only through [`DiscoveryTypes`]. Nothing in
//! this module reads a body: a candidate's result is a closed recipe.
//! Candidate precedence is authored order; comparison results are decided
//! in order, so an undecided earlier comparison never lets a later
//! candidate win.

use std::sync::Arc;

use crate::semantic_query::{
    CanonicalTypeSubstitution, IncompleteReason, SemanticNodeId, CONTEXT_FREE_EVALUATION,
};

use super::lifetime::{SignatureStore, StoreError};
use super::positional::{PositionalMode, PositionalShape, SlotTypeFacts, TypeAt};
use super::provenance::{
    ArmIdentity, ConstituentSequence, MappedConstituent, OriginRelation, OverloadOrder,
    SignatureProvenance,
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

type Res<T> = Result<T, DiscoveryError>;

fn undecided<T>() -> Res<T> {
    Err(DiscoveryError::Incomplete(IncompleteReason::UnsettledInput))
}

/// What discovery needs from the type graph.
pub trait DiscoveryTypes: SlotTypeFacts {
    /// Whether two types are identical under a binder correspondence
    /// (`(a_binder, b_binder)` pairs). `None`: the graph cannot decide yet.
    fn identical(
        &self,
        a: TypeToken,
        b: TypeToken,
        corr: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<bool>;
    /// The intersection of parameter types, in order.
    fn intersect(&self, members: &[TypeToken]) -> Option<TypeToken>;
    /// Rewrite binder occurrences (`from -> to`) inside a type.
    fn map_binders(
        &self,
        ty: TypeToken,
        map: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<TypeToken>;
    fn unknown(&self) -> TypeToken;
    fn any(&self) -> TypeToken;
    fn is_any(&self, ty: TypeToken) -> bool;
    /// The return type a candidate's recipe denotes. This FORCES a body
    /// recipe; discovery calls it only when comparison semantics need a
    /// return (exact matching that includes returns).
    fn forced_return(&self, candidate: &SignatureCandidate) -> Result<TypeToken, IncompleteReason>;
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
    Declared { return_type: SemanticNodeId },
    Body { locator: u64 },
}

/// Neutral description of one authored signature.
#[derive(Debug, Clone)]
pub struct SignatureInput {
    pub kind: SignatureKind,
    pub source: SemanticNodeId,
    pub space_key: u64,
    pub binders: Vec<BinderInput>,
    pub receiver: Option<ParamInput>,
    pub params: Vec<ParamInput>,
    pub rest: Option<RestInput>,
    pub flags: SignatureSemanticFlags,
    pub result: ResultInput,
    pub provenance: SignatureProvenance,
}

fn slot(store: &SignatureStore, p: &ParamInput) -> Res<ParameterSlot> {
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
pub fn publish_signature(
    store: &SignatureStore,
    input: &SignatureInput,
) -> Res<SignatureCandidate> {
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
    let environment = store.intern_environment(CanonicalTypeSubstitution::new(env), None)?;

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
            declared_minimum: u16::try_from(declared_minimum).unwrap_or(u16::MAX),
            signature_semantic_flags: input.flags,
        },
        None,
    )?;
    let recipe = match input.result {
        ResultInput::Declared { return_type } => SignatureResultRecipe::Declared {
            return_type: store.intern_type_token(return_type, None)?,
            predicate_or_assertion: None,
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
    store: &SignatureStore,
    candidates: Vec<SignatureCandidate>,
) -> Res<SignatureSetRef> {
    Ok(match candidates.len() {
        0 => SignatureSetRef::Empty,
        1 => SignatureSetRef::One(candidates[0]),
        _ => store.set_ref_many(candidates.into_boxed_slice(), None)?,
    })
}

/// A candidate with its records loaded from a pinned view.
struct Loaded<'v> {
    candidate: SignatureCandidate,
    shape: &'v SignatureInputShape,
    layout: &'v ParameterLayout,
    space: &'v BinderSpace,
    env: &'v CanonicalTypeSubstitution,
    recipe: &'v SignatureResultRecipe,
    receiver: Option<ParameterSlot>,
    descriptor: &'v SignatureDescriptor,
}

fn load<'v>(view: &'v SemanticReadView, candidate: SignatureCandidate) -> Res<Loaded<'v>> {
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
    fn positional<'a>(&'a self, facts: &'a dyn SlotTypeFacts) -> PositionalShape<'a> {
        PositionalShape::new(
            self.layout,
            self.receiver,
            self.shape.signature_semantic_flags,
            facts,
        )
    }

    /// Declared binder nodes in ordinal order (from the environment: each
    /// binder maps to `binder_token(space_key, ordinal)`).
    fn binder_nodes(&self) -> Vec<(u32, SemanticNodeId)> {
        let mut out: Vec<(u32, SemanticNodeId)> = self
            .env
            .bindings()
            .iter()
            .map(|(param, token)| (token.0 as u32, *param))
            .collect();
        out.sort_by_key(|(ordinal, _)| *ordinal);
        out
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

fn ident(
    types: &dyn DiscoveryTypes,
    a: TypeToken,
    b: TypeToken,
    corr: &[(SemanticNodeId, SemanticNodeId)],
) -> Res<bool> {
    match types.identical(a, b, corr) {
        Some(v) => Ok(v),
        None => undecided(),
    }
}

fn slot_type_identical(
    types: &dyn DiscoveryTypes,
    a: TypeAt<'_>,
    b: TypeAt<'_>,
    corr: &[(SemanticNodeId, SemanticNodeId)],
) -> Res<bool> {
    let any = types.any();
    let one = |ty: TypeAt<'_>| match ty {
        TypeAt::Absent => Some((any, false)),
        TypeAt::One(s) => Some((s.ty, s.optionality.includes_undefined)),
        _ => None,
    };
    match (a, b) {
        (
            TypeAt::Run {
                element: ea,
                tail: ta,
            },
            TypeAt::Run {
                element: eb,
                tail: tb,
            },
        ) => {
            if ta.len() != tb.len() || !ident(types, ea, eb, corr)? {
                return Ok(false);
            }
            for (x, y) in ta.iter().zip(tb) {
                if !ident(types, x.ty, y.ty, corr)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (
            TypeAt::GenericRest {
                rest: ra,
                index: ia,
            },
            TypeAt::GenericRest {
                rest: rb,
                index: ib,
            },
        ) => Ok(ia == ib && ident(types, ra, rb, corr)?),
        (a, b) => match (one(a), one(b)) {
            (Some((ta, ua)), Some((tb, ub))) => Ok(ua == ub && ident(types, ta, tb, corr)?),
            _ => Ok(false),
        },
    }
}

/// `compareSignaturesIdentical`: arity match (or partial), binder
/// constraint/default equality under a positional binder correspondence,
/// receiver, every target position, and (unless ignored) the result.
pub fn signatures_identical(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    source: SignatureCandidate,
    target: SignatureCandidate,
    options: MatchOptions,
) -> Res<bool> {
    if source.signature == target.signature {
        return Ok(true);
    }
    let view = SemanticReadView::pin(store);
    let s = load(&view, source)?;
    let t = load(&view, target)?;
    let sp = s.positional(types);
    let tp = t.positional(types);
    let mode = PositionalMode::Comparison;
    let arity_same = sp.parameter_count() == tp.parameter_count()
        && sp.effective_minimum(mode) == tp.effective_minimum(mode)
        && sp.has_rest() == tp.has_rest();
    if !arity_same
        && !(options.partial_match && sp.effective_minimum(mode) <= tp.effective_minimum(mode))
    {
        return Ok(false);
    }
    if s.space.binders.len() != t.space.binders.len() {
        return Ok(false);
    }
    let sb = s.binder_nodes();
    let tb = t.binder_nodes();
    let corr: Vec<(SemanticNodeId, SemanticNodeId)> = sb
        .iter()
        .zip(tb.iter())
        .map(|((_, a), (_, b))| (*a, *b))
        .collect();
    for (a, b) in s.space.binders.iter().zip(t.space.binders.iter()) {
        let unknown = types.unknown();
        let ac = a.constraint.unwrap_or(unknown);
        let bc = b.constraint.unwrap_or(unknown);
        let ad = a.default.unwrap_or(unknown);
        let bd = b.default.unwrap_or(unknown);
        if !ident(types, ac, bc, &corr)? || !ident(types, ad, bd, &corr)? {
            return Ok(false);
        }
    }
    if !options.ignore_this_types {
        if let (Some(a), Some(b)) = (s.receiver, t.receiver) {
            if !ident(types, a.ty, b.ty, &corr)? {
                return Ok(false);
            }
        }
    }
    for pos in 0..tp.parameter_count() {
        if !slot_type_identical(types, sp.type_at(pos), tp.type_at(pos), &corr)? {
            return Ok(false);
        }
    }
    if !options.ignore_return_types {
        let same_body = matches!(
            (s.recipe, t.recipe),
            (
                SignatureResultRecipe::Body { return_obligation_key: a },
                SignatureResultRecipe::Body { return_obligation_key: b },
            ) if a == b && corr.is_empty()
        );
        if !same_body {
            let a = types
                .forced_return(&source)
                .map_err(DiscoveryError::Incomplete)?;
            let b = types
                .forced_return(&target)
                .map_err(DiscoveryError::Incomplete)?;
            if !ident(types, a, b, &corr)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn find_matching(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    list: &[SignatureCandidate],
    sig: SignatureCandidate,
    options: MatchOptions,
) -> Res<Option<SignatureCandidate>> {
    // Ordered: an undecided earlier comparison surfaces as an error; a later
    // match is never taken past it.
    for &candidate in list {
        if signatures_identical(store, types, candidate, sig, options)? {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Append `new` signatures to `existing`, skipping any signature-equivalent
/// to one already present (the first stays the representative).
pub fn append_signatures(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    existing: &mut Vec<SignatureCandidate>,
    new: &[SignatureCandidate],
) -> Res<()> {
    for &sig in new {
        if find_matching(store, types, existing, sig, MatchOptions::EXACT)?.is_none() {
            existing.push(sig);
        }
    }
    Ok(())
}

fn source_token(store: &SignatureStore, c: SignatureCandidate) -> Res<TypeToken> {
    match store.descriptor_source(c.signature)? {
        Some(token) => Ok(token),
        None => undecided(),
    }
}

fn composite_provenance(
    store: &SignatureStore,
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
    store: &SignatureStore,
    view: &SemanticReadView,
    rep: &Loaded<'_>,
    c: &Loaded<'_>,
) -> Res<SignatureDescriptorIdOrSame> {
    if c.descriptor.residual_binders == rep.descriptor.residual_binders
        || c.space.binders.is_empty()
    {
        return Ok(SignatureDescriptorIdOrSame::Same);
    }
    let rb = rep.binder_nodes();
    let cb = c.binder_nodes();
    let _ = view;
    let mut to_rep = Vec::new();
    for ((ordinal, _), (rep_ordinal, _)) in cb.iter().zip(rb.iter()) {
        to_rep.push((
            SignatureStore::binder_token(space_key(c.space), *ordinal)?,
            SignatureStore::binder_token(space_key(rep.space), *rep_ordinal)?,
        ));
    }
    let env = compose_canonical(c.env, &CanonicalTypeSubstitution::new(to_rep));
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

fn edges_for(
    store: &SignatureStore,
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

fn composite_candidate(
    store: &SignatureStore,
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

/// Union call/construct signatures over the per-arm candidate lists (arms
/// in `VerterStableV1` order, each list in authored order).
///
/// Phase 1 — common matches: a signature from any arm that has a match in
/// every other arm (exact for generic signatures, exact-then-partial with
/// returns ignored otherwise) becomes one union candidate whose
/// constituents are the matches. Phase 2 — restricted synthesis: only when
/// phase 1 found nothing and at most one arm has several signatures, the
/// single signatures combine with each of that arm's signatures. There is
/// never a Cartesian product of overload choices.
pub fn union_signatures(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    lists: &[Vec<SignatureCandidate>],
) -> Res<Vec<SignatureCandidate>> {
    if lists.is_empty() || lists.iter().any(Vec::is_empty) {
        return Ok(Vec::new());
    }
    if lists.len() == 1 {
        return Ok(lists[0].clone());
    }
    let over_one: Vec<usize> = lists
        .iter()
        .enumerate()
        .filter(|(_, l)| l.len() > 1)
        .map(|(i, _)| i)
        .collect();
    let view = SemanticReadView::pin(store);
    let mut result: Vec<SignatureCandidate> = Vec::new();
    let mut representatives: Vec<SignatureCandidate> = Vec::new();
    for (list_index, list) in lists.iter().enumerate() {
        for &sig in list {
            if find_matching(
                store,
                types,
                &representatives,
                sig,
                MatchOptions::EXACT_IGNORING_RETURNS,
            )?
            .is_some()
            {
                continue;
            }
            let sig_loaded = load(&view, sig)?;
            let matches = if !sig_loaded.space.binders.is_empty() {
                // Generic signatures require an exact match, and only the
                // first list may start one.
                if list_index > 0 {
                    None
                } else {
                    let mut ms = vec![(0u32, sig)];
                    let mut ok = true;
                    for (i, other) in lists.iter().enumerate().skip(1) {
                        match find_matching(store, types, other, sig, MatchOptions::EXACT)? {
                            Some(m) => ms.push((i as u32, m)),
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    ok.then_some(ms)
                }
            } else {
                let mut ms: Vec<(u32, SignatureCandidate)> = Vec::new();
                let mut ok = true;
                for (i, other) in lists.iter().enumerate() {
                    let m = if i == list_index {
                        Some(sig)
                    } else {
                        match find_matching(
                            store,
                            types,
                            other,
                            sig,
                            MatchOptions::EXACT_IGNORING_RETURNS,
                        )? {
                            Some(m) => Some(m),
                            None => find_matching(
                                store,
                                types,
                                other,
                                sig,
                                MatchOptions::PARTIAL_IGNORING_RETURNS,
                            )?,
                        }
                    };
                    match m {
                        Some(m) => {
                            if !ms.iter().any(|(_, x)| x.signature == m.signature) {
                                ms.push((i as u32, m));
                            }
                        }
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                ok.then_some(ms)
            };
            let Some(matches) = matches else { continue };
            representatives.push(sig);
            if matches.len() == 1 {
                result.push(sig);
                continue;
            }
            let sequence = edges_for(store, &view, sig, &matches)?;
            let rep_source = source_token(store, sig)?;
            let candidate = composite_candidate(
                store,
                &view,
                sig,
                view.template(sig_loaded.descriptor.template)?.input_shape,
                SignatureResultRecipe::UnionCommon {
                    representative: rep_source,
                    constituents: sequence,
                },
                result.len() as u32,
            )?;
            result.push(candidate);
        }
    }
    if !result.is_empty() || over_one.len() > 1 {
        return Ok(result);
    }
    synthesize_union(store, types, lists, over_one.first().copied())
}

fn synthesize_union(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    lists: &[Vec<SignatureCandidate>],
    master_index: Option<usize>,
) -> Res<Vec<SignatureCandidate>> {
    let master = master_index.unwrap_or(0);
    let mut out = Vec::new();
    'masters: for &m in &lists[master] {
        let mut acc = m;
        let mut constituents: Vec<(u32, SignatureCandidate)> = vec![(master as u32, m)];
        for (i, list) in lists.iter().enumerate() {
            if i == master {
                continue;
            }
            let Some(other) = list.first().copied() else {
                continue 'masters;
            };
            match combine_union_signatures(store, types, acc, other)? {
                Some(next) => acc = next,
                None => continue 'masters,
            }
            constituents.push((i as u32, other));
        }
        if constituents.len() == 1 {
            out.push(m);
            continue;
        }
        let view = SemanticReadView::pin(store);
        let m_loaded = load(&view, m)?;
        let acc_loaded = load(&view, acc)?;
        let sequence = edges_for(store, &view, m, &constituents)?;
        let master_source = source_token(store, m)?;
        let candidate = composite_candidate(
            store,
            &view,
            m,
            view.template(acc_loaded.descriptor.template)?.input_shape,
            SignatureResultRecipe::UnionSynthesized {
                master: master_source,
                constituents: sequence,
            },
            out.len() as u32,
        )?;
        let _ = m_loaded;
        out.push(candidate);
    }
    Ok(out)
}

/// `combineSignaturesOfUnionMembers`: parameter types intersect position by
/// position, the minimum is the larger of the two, an extra rest element is
/// added when only the shorter side has a rest.
fn combine_union_signatures(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    left: SignatureCandidate,
    right: SignatureCandidate,
) -> Res<Option<SignatureCandidate>> {
    let view = SemanticReadView::pin(store);
    let l = load(&view, left)?;
    let r = load(&view, right)?;
    if !l.space.binders.is_empty() && !r.space.binders.is_empty() {
        if !signatures_identical(
            store,
            types,
            left,
            right,
            MatchOptions {
                partial_match: true,
                ignore_this_types: true,
                ignore_return_types: true,
            },
        )? && l.space.binders.len() != r.space.binders.len()
        {
            return Ok(None);
        }
        if l.space.binders.len() != r.space.binders.len() {
            return Ok(None);
        }
    }
    let map_right: Vec<(SemanticNodeId, SemanticNodeId)> =
        if !l.space.binders.is_empty() && !r.space.binders.is_empty() {
            r.binder_nodes()
                .iter()
                .zip(l.binder_nodes().iter())
                .map(|((_, b), (_, a))| (*b, *a))
                .collect()
        } else {
            Vec::new()
        };
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
            match types.map_binders(ty, &map_right) {
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
        let shorter_ty =
            type_of(shorter, pos, !right_is_longest)?.unwrap_or_else(|| types.unknown());
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
        (Some(a), None) | (None, Some(a)) => Some(a),
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
    declared_minimum = declared_minimum.max(usize::from(
        l.shape.declared_minimum.max(r.shape.declared_minimum),
    ));
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
            declared_minimum: u16::try_from(declared_minimum).unwrap_or(u16::MAX),
            signature_semantic_flags: flags,
        },
        None,
    )?;
    let base = if l.space.binders.is_empty() { &r } else { &l };
    let recipe = store.intern_recipe(l.recipe.clone(), None)?;
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
    Ok(Some(store.candidate(descriptor, left.provenance)?))
}

fn is_mixin_constructor(
    view: &SemanticReadView,
    types: &dyn DiscoveryTypes,
    list: &[SignatureCandidate],
) -> Res<bool> {
    let [only] = list else { return Ok(false) };
    let loaded = load(view, *only)?;
    Ok(loaded.layout.parameters.is_empty()
        && loaded.receiver.is_none()
        && loaded.layout.rest.as_ref().is_some_and(|r| {
            r.kind == RestKind::Array && r.tail.is_empty() && types.is_any(r.slot.ty)
        }))
}

/// Intersection signatures for one kind. `members` are the per-member
/// candidate lists in authored order.
///
/// Call signatures concatenate with signature-equivalence dedup. Construct
/// signatures do the same, except that when any member is a mixin
/// constructor (`new (...args: any[]) => X`) every other member's construct
/// signature is composed with the mixin returns (`IntersectionConstruct`).
pub fn intersection_signatures(
    store: &SignatureStore,
    types: &dyn DiscoveryTypes,
    kind: SignatureKind,
    members: &[Vec<SignatureCandidate>],
) -> Res<Vec<SignatureCandidate>> {
    let view = SemanticReadView::pin(store);
    let mut mixin_flags = vec![false; members.len()];
    if kind == SignatureKind::Construct {
        for (i, list) in members.iter().enumerate() {
            mixin_flags[i] = is_mixin_constructor(&view, types, list)?;
        }
    }
    let mixin_count = mixin_flags.iter().filter(|f| **f).count();
    let mut out: Vec<SignatureCandidate> = Vec::new();
    for (index, list) in members.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let mapped: Vec<SignatureCandidate> = if mixin_count > 0 {
            let mut composed = Vec::with_capacity(list.len());
            for &sig in list {
                let mut constituents: Vec<(u32, SignatureCandidate)> = Vec::new();
                for (i, other) in members.iter().enumerate() {
                    if i == index {
                        constituents.push((i as u32, sig));
                    } else if mixin_flags[i] {
                        constituents.push((i as u32, other[0]));
                    }
                }
                let loaded = load(&view, sig)?;
                let sequence = edges_for(store, &view, sig, &constituents)?;
                let base = source_token(store, sig)?;
                composed.push(composite_candidate(
                    store,
                    &view,
                    sig,
                    view.template(loaded.descriptor.template)?.input_shape,
                    SignatureResultRecipe::IntersectionConstruct {
                        base_constructor: base,
                        mixins: sequence,
                    },
                    composed.len() as u32,
                )?);
            }
            composed
        } else {
            list.clone()
        };
        append_signatures(store, types, &mut out, &mapped)?;
    }
    Ok(out)
}
