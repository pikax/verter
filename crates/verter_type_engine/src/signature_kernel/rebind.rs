//! Copying signature records from one epoch into another.
//!
//! A computation works in one epoch for its whole life
//! ([`EpochBinding`]). Where a handle it holds must be named in another
//! epoch — a result read keyed in the current one, a set published after
//! its walk's epoch retired, the foreign side of a comparison — the records
//! that handle reaches are copied. Every table is content-interned, so the
//! copy is the record a fresh publication of the same input interns there:
//! nothing is discovered, matched, substituted or forced again.

use rustc_hash::FxHashMap;

use super::lifetime::{EpochBinding, SignatureEpochLease, StoreError};
use super::provenance::{
    ArmIdentity, ConstituentSequence, MappedConstituent, OriginRelation, SignatureProvenance,
};
use super::read_view::{ReadError, SemanticReadView};
use super::records::{
    BinderDeclaration, BinderSpace, BinderSpaceId, ConstituentSequenceId,
    DeclarationInstantiationId, ParameterLayout, ParameterSlot, PredicateEffect, RestSlot,
    ReturnObligationKey, SignatureCandidate, SignatureDescriptor, SignatureDescriptorId,
    SignatureInputShape, SignatureInputShapeId, SignatureProvenanceId, SignatureResultRecipe,
    SignatureResultRecipeId, SignatureTemplate, SpellingId, TypeToken,
};

/// One pass copying signature records from one epoch into another.
///
/// The pass reads one epoch and writes one epoch, so whatever the store
/// does meanwhile it never mixes handles of two epochs in a record. Each
/// descriptor reachable from the handles it is given is copied once; the
/// composite nesting is walked on an owned stack, never on the native one.
pub struct EpochRebind<'b> {
    from: SemanticReadView,
    into: &'b EpochBinding,
    /// Source descriptor index to its copy: the pass's materialization map.
    descriptors: FxHashMap<u32, SignatureDescriptorId>,
    /// Descriptors waiting for their copy, a composite after its
    /// constituents.
    pending: Vec<SignatureDescriptorId>,
}

/// A read of the source epoch that failed, as the store reports it.
fn read(error: ReadError) -> StoreError {
    match error {
        ReadError::StaleHandle => StoreError::StaleHandle,
        ReadError::Missing => StoreError::Missing,
    }
}

/// The constituent sequence a composite recipe names; `None` for a leaf.
fn composite_sequence(recipe: &SignatureResultRecipe) -> Option<ConstituentSequenceId> {
    match recipe {
        SignatureResultRecipe::UnionCommon { constituents, .. }
        | SignatureResultRecipe::UnionSynthesized { constituents, .. }
        | SignatureResultRecipe::IntersectionConstruct {
            mixins: constituents,
            ..
        } => Some(*constituents),
        SignatureResultRecipe::Declared { .. } | SignatureResultRecipe::Body { .. } => None,
    }
}

impl<'b> EpochRebind<'b> {
    /// A pass copying from `from`'s epoch into `into`'s.
    #[must_use]
    pub fn new(from: &SignatureEpochLease, into: &'b EpochBinding) -> Self {
        Self {
            from: from.read_view(),
            into,
            descriptors: FxHashMap::default(),
            pending: Vec::new(),
        }
    }

    /// `id`'s descriptor in the target epoch, with everything it reaches;
    /// `id` itself when both epochs are one. A token whose node the graph
    /// has released fails `Missing`, as a fresh intern of it would.
    pub fn descriptor(
        &mut self,
        id: SignatureDescriptorId,
    ) -> Result<SignatureDescriptorId, StoreError> {
        if self.from.epoch() == self.into.epoch() {
            return Ok(id);
        }
        self.pending.push(id);
        while let Some(&top) = self.pending.last() {
            if self.descriptors.contains_key(&top.index()) {
                self.pending.pop();
                continue;
            }
            let descriptor = *self.from.descriptor(top).map_err(read)?;
            let template = *self.from.template(descriptor.template).map_err(read)?;
            let recipe = *self.from.recipe(template.result_recipe).map_err(read)?;
            if let Some(sequence) = composite_sequence(&recipe) {
                let before = self.pending.len();
                for edge in self.from.sequence(sequence).map_err(read)?.edges.iter() {
                    for named in [edge.arm.contributor, edge.declaration, edge.residual] {
                        if !self.descriptors.contains_key(&named.index()) {
                            self.pending.push(named);
                        }
                    }
                }
                if self.pending.len() > before {
                    // Revisited once every edge descriptor has its copy.
                    continue;
                }
            }
            let copy = self.copy_descriptor(top, descriptor, template.input_shape, recipe)?;
            self.descriptors.insert(top.index(), copy);
            self.pending.pop();
        }
        self.descriptors
            .get(&id.index())
            .copied()
            .ok_or(StoreError::Missing)
    }

    /// `candidate` in the target epoch: its descriptor, and its provenance
    /// chain interned root first, each record naming its copied parent.
    pub fn candidate(
        &mut self,
        candidate: SignatureCandidate,
    ) -> Result<SignatureCandidate, StoreError> {
        if self.from.epoch() == self.into.epoch() {
            return Ok(candidate);
        }
        let signature = self.descriptor(candidate.signature)?;
        let mut chain: Vec<SignatureProvenance> =
            vec![*self.from.provenance(candidate.provenance).map_err(read)?];
        while let Some(
            OriginRelation::Synthesized { from: parent }
            | OriginRelation::Instantiated { from: parent },
        ) = chain.last().map(|record| record.origin)
        {
            chain.push(*self.from.provenance(parent).map_err(read)?);
        }
        let mut copied: Option<SignatureProvenanceId> = None;
        for record in chain.into_iter().rev() {
            let origin = match (record.origin, copied) {
                (OriginRelation::Authored, _) => OriginRelation::Authored,
                (OriginRelation::Synthesized { .. }, Some(from)) => {
                    OriginRelation::Synthesized { from }
                }
                (OriginRelation::Instantiated { .. }, Some(from)) => {
                    OriginRelation::Instantiated { from }
                }
                // The chain ends at its authored root.
                (_, None) => return Err(StoreError::Missing),
            };
            copied = Some(
                self.into
                    .intern_provenance(SignatureProvenance { origin, ..record }, None)?,
            );
        }
        let provenance = copied.ok_or(StoreError::Missing)?;
        self.into.candidate(signature, provenance)
    }

    /// Descriptors this pass copied.
    #[must_use]
    pub fn descriptors_copied(&self) -> u32 {
        self.descriptors.len() as u32
    }

    /// One descriptor's records, leaf to root, its edge descriptors already
    /// copied.
    fn copy_descriptor(
        &self,
        id: SignatureDescriptorId,
        descriptor: SignatureDescriptor,
        input_shape: SignatureInputShapeId,
        recipe: SignatureResultRecipe,
    ) -> Result<SignatureDescriptorId, StoreError> {
        let input_shape = self.shape(input_shape)?;
        let result_recipe = self.recipe(recipe)?;
        let template = self.into.intern_template(
            SignatureTemplate {
                input_shape,
                result_recipe,
            },
            None,
        )?;
        let declaration_environment = self.environment(descriptor.declaration_environment)?;
        let residual_binders = self.space(descriptor.residual_binders)?;
        let copy = self.into.intern_descriptor(
            SignatureDescriptor {
                template,
                declaration_environment,
                residual_binders,
            },
            None,
        )?;
        if let Some(source) = self.from.epoch_inner().descriptor_source(id)? {
            self.into
                .record_descriptor_source(copy, self.token(source)?)?;
        }
        Ok(copy)
    }

    /// A type token naming the same graph node in the target epoch, which
    /// roots the node there.
    fn token(&self, token: TypeToken) -> Result<TypeToken, StoreError> {
        let node = self.from.type_token_node(token).map_err(read)?;
        self.into.intern_type_token(node, None)
    }

    fn spelling(&self, id: SpellingId) -> Result<SpellingId, StoreError> {
        let spelling = self.from.spelling(id).map_err(read)?;
        self.into.intern_spelling(spelling, None)
    }

    fn slot(&self, slot: &ParameterSlot) -> Result<ParameterSlot, StoreError> {
        Ok(ParameterSlot {
            ty: self.token(slot.ty)?,
            optionality: slot.optionality,
            name: slot.name.map(|name| self.spelling(name)).transpose()?,
        })
    }

    fn layout(&self, layout: &ParameterLayout) -> Result<ParameterLayout, StoreError> {
        let mut parameters = Vec::with_capacity(layout.parameters.len());
        for slot in layout.parameters.iter() {
            parameters.push(self.slot(slot)?);
        }
        let rest = match &layout.rest {
            Some(rest) => {
                let mut tail = Vec::with_capacity(rest.tail.len());
                for slot in rest.tail.iter() {
                    tail.push(self.slot(slot)?);
                }
                Some(RestSlot {
                    slot: self.slot(&rest.slot)?,
                    kind: rest.kind,
                    tail: tail.into_boxed_slice(),
                })
            }
            None => None,
        };
        Ok(ParameterLayout {
            parameters: parameters.into_boxed_slice(),
            rest,
        })
    }

    /// A binder space; one with binders first claims its key for the
    /// identity that claimed it in the source epoch.
    fn space(&self, id: BinderSpaceId) -> Result<BinderSpaceId, StoreError> {
        let space = self.from.space(id).map_err(read)?;
        let mut binders = Vec::with_capacity(space.binders.len());
        for binder in space.binders.iter() {
            binders.push(BinderDeclaration {
                spelling: self.spelling(binder.spelling)?,
                constraint: binder.constraint.map(|ty| self.token(ty)).transpose()?,
                default: binder.default.map(|ty| self.token(ty)).transpose()?,
            });
        }
        if !binders.is_empty() {
            let identity = self
                .from
                .space_key_owner(space.key)
                .ok_or(StoreError::Missing)?;
            self.into.claim_space_key(space.key, identity)?;
        }
        self.into.intern_binder_space(
            BinderSpace {
                key: space.key,
                binders: binders.into_boxed_slice(),
            },
            None,
        )
    }

    fn environment(
        &self,
        id: DeclarationInstantiationId,
    ) -> Result<DeclarationInstantiationId, StoreError> {
        let environment = self.from.environment(id).map_err(read)?.clone();
        self.into.intern_environment(environment, None)
    }

    fn shape(&self, id: SignatureInputShapeId) -> Result<SignatureInputShapeId, StoreError> {
        let shape = *self.from.shape(id).map_err(read)?;
        let this_parameter = match shape.this_parameter {
            Some(receiver) => {
                let receiver = *self.from.slot(receiver).map_err(read)?;
                Some(self.into.intern_slot(self.slot(&receiver)?, None)?)
            }
            None => None,
        };
        let layout = self.layout(self.from.layout(shape.parameter_layout).map_err(read)?)?;
        let parameter_layout = self.into.intern_layout(layout, None)?;
        let binder_declarations = self.space(shape.binder_declarations)?;
        self.into.intern_shape(
            SignatureInputShape {
                binder_declarations,
                this_parameter,
                parameter_layout,
                ..shape
            },
            None,
        )
    }

    fn recipe(&self, recipe: SignatureResultRecipe) -> Result<SignatureResultRecipeId, StoreError> {
        let copied = match recipe {
            SignatureResultRecipe::Declared {
                return_type,
                predicate_or_assertion,
            } => SignatureResultRecipe::Declared {
                return_type: self.token(return_type)?,
                predicate_or_assertion: match predicate_or_assertion {
                    Some(effect) => Some(PredicateEffect {
                        ty: effect.ty.map(|ty| self.token(ty)).transpose()?,
                        ..effect
                    }),
                    None => None,
                },
            },
            SignatureResultRecipe::Body {
                return_obligation_key,
            } => {
                let locator = self
                    .from
                    .body_locator(return_obligation_key.body_locator)
                    .map_err(read)?;
                SignatureResultRecipe::Body {
                    return_obligation_key: ReturnObligationKey {
                        body_locator: self.into.intern_body_locator(locator, None)?,
                        ..return_obligation_key
                    },
                }
            }
            SignatureResultRecipe::UnionCommon {
                representative,
                constituents,
            } => SignatureResultRecipe::UnionCommon {
                representative: self.token(representative)?,
                constituents: self.sequence(constituents)?,
            },
            SignatureResultRecipe::UnionSynthesized {
                master,
                constituents,
            } => SignatureResultRecipe::UnionSynthesized {
                master: self.token(master)?,
                constituents: self.sequence(constituents)?,
            },
            SignatureResultRecipe::IntersectionConstruct {
                base_constructor,
                mixins,
            } => SignatureResultRecipe::IntersectionConstruct {
                base_constructor: self.token(base_constructor)?,
                mixins: self.sequence(mixins)?,
            },
        };
        self.into.intern_recipe(copied, None)
    }

    /// A composite's constituent sequence, each edge naming the copies of
    /// its descriptors.
    fn sequence(&self, id: ConstituentSequenceId) -> Result<ConstituentSequenceId, StoreError> {
        let copied = |descriptor: SignatureDescriptorId| {
            self.descriptors
                .get(&descriptor.index())
                .copied()
                .ok_or(StoreError::Missing)
        };
        let sequence = self.from.sequence(id).map_err(read)?;
        let mut edges = Vec::with_capacity(sequence.edges.len());
        for edge in sequence.edges.iter() {
            edges.push(MappedConstituent {
                arm: ArmIdentity {
                    ordinal: edge.arm.ordinal,
                    contributor: copied(edge.arm.contributor)?,
                },
                declaration: copied(edge.declaration)?,
                residual: copied(edge.residual)?,
            });
        }
        self.into.intern_sequence(
            ConstituentSequence {
                edges: edges.into_boxed_slice(),
            },
            None,
        )
    }
}
