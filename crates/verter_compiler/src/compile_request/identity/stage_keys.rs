//! The stage identities declared by this module. See [`super`] for the
//! composition law; [`ParseKey`] is re-exported there, never re-minted
//! here.

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::ParseKey;

/// Per-stage contract epoch of the default contract registry row a key
/// composes: a revision that changes one stage advances only that stage's
/// epoch. `0` is a valid first epoch, not an uninitialized sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StageEpoch(pub u32);

verter_identity::digest_identity!(
    /// What semantic admission committed to answer: the demanded regions,
    /// the host-resolved facts it admitted, and the external style stages
    /// actually supplied. Field 5 of [`SemanticKey`].
    SemanticAdmissionBasis
);

/// Descriptor behind [`SemanticAdmissionBasis`]: three sorted sets under
/// the fixed domain tag.
struct SemanticAdmissionBasisDescriptor<'a> {
    demanded_regions: &'a [CanonicalDigest],
    admitted_facts: &'a [CanonicalDigest],
    supplied_style_stages: &'a [CanonicalDigest],
}

impl CanonicalEncode for SemanticAdmissionBasisDescriptor<'_> {
    const DOMAIN_TAG: &'static str = SemanticAdmissionBasis::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_sorted_set(1, self.demanded_regions.iter().map(|d| *d.as_bytes()))
            .field_sorted_set(2, self.admitted_facts.iter().map(|d| *d.as_bytes()))
            .field_sorted_set(3, self.supplied_style_stages.iter().map(|d| *d.as_bytes()));
    }
}

impl SemanticAdmissionBasis {
    /// The fixed compatibility domain of the admission-basis encoding.
    pub const DOMAIN_TAG: &'static str = "verter.compiler.stage.semantic-admission-basis.v1";

    /// Compose the basis admission mints once the product's demand closure
    /// is known and every semantic `NeedInputs` has been supplied. Each
    /// element is the canonical digest of one region identity, one
    /// admitted host-resolved fact, or one supplied external style stage;
    /// element order and repeats are not part of the basis.
    pub fn from_parts(
        demanded_regions: &[CanonicalDigest],
        admitted_facts: &[CanonicalDigest],
        supplied_style_stages: &[CanonicalDigest],
    ) -> Self {
        Self::from_canonical(&SemanticAdmissionBasisDescriptor {
            demanded_regions,
            admitted_facts,
            supplied_style_stages,
        })
    }
}

verter_identity::digest_identity!(
    /// Semantic-stage identity: over which source units, under which
    /// framework profile, options, contract epoch and admission basis the
    /// semantic stage resolves.
    SemanticKey
);

/// Descriptor behind [`SemanticKey`]. Field numbers are the public
/// compatibility contract; do not renumber.
struct SemanticKeyDescriptor<'a> {
    source_units: &'a [ParseKey],
    framework_profile: CanonicalDigest,
    semantic_options: &'a [u8],
    semantic_epoch: StageEpoch,
    admission_basis: &'a SemanticAdmissionBasis,
}

impl CanonicalEncode for SemanticKeyDescriptor<'_> {
    const DOMAIN_TAG: &'static str = SemanticKey::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_sorted_set(1, self.source_units.iter().map(|k| *k.digest().as_bytes()))
            .field_bytes(2, self.framework_profile.as_bytes())
            .field_bytes(3, self.semantic_options)
            .field_u32(4, self.semantic_epoch.0)
            .field_bytes(5, self.admission_basis.digest().as_bytes());
    }
}

impl SemanticKey {
    /// The fixed compatibility domain of the semantic-stage encoding.
    pub const DOMAIN_TAG: &'static str = "verter.compiler.stage.semantic.v1";

    /// Compose the semantic-stage identity.
    ///
    /// `source_units` holds the parse identity of every source unit that
    /// contains a demanded region — order and repeats are not part of the
    /// identity. `framework_profile` is the canonical digest of the
    /// workspace's activated framework profile, the only profile identity
    /// this key names. `semantic_options` is the canonical bytes of the
    /// semantic-class option set, which already carries the TypeScript
    /// semantic profile identity rather than a second field.
    /// `admission_basis` is what admission committed.
    pub fn compose(
        source_units: &[ParseKey],
        framework_profile: CanonicalDigest,
        semantic_options: &[u8],
        semantic_epoch: StageEpoch,
        admission_basis: &SemanticAdmissionBasis,
    ) -> Self {
        Self::from_canonical(&SemanticKeyDescriptor {
            source_units,
            framework_profile,
            semantic_options,
            semantic_epoch,
            admission_basis,
        })
    }
}

verter_identity::digest_identity!(
    /// Compile-structure-stage identity: which structure the compile
    /// builds, over the semantic identity plus the product's demand
    /// closure, structure-class options and contract epoch.
    CompileStructureKey
);

/// Descriptor behind [`CompileStructureKey`]; field numbers are the public
/// compatibility contract.
struct CompileStructureKeyDescriptor<'a> {
    semantic: &'a SemanticKey,
    demand_closure: CanonicalDigest,
    structure_options: &'a [u8],
    structure_epoch: StageEpoch,
}

impl CanonicalEncode for CompileStructureKeyDescriptor<'_> {
    const DOMAIN_TAG: &'static str = CompileStructureKey::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.semantic.digest().as_bytes())
            .field_bytes(2, self.demand_closure.as_bytes())
            .field_bytes(3, self.structure_options)
            .field_u32(4, self.structure_epoch.0);
    }
}

impl CompileStructureKey {
    /// The fixed compatibility domain of the compile-structure-stage
    /// encoding.
    pub const DOMAIN_TAG: &'static str = "verter.compiler.stage.compile-structure.v1";

    /// Compose the compile-structure-stage identity: the semantic stage's
    /// identity, the canonical digest of the product's closed demand
    /// closure, the canonical bytes of the structure-class option set,
    /// and the structure-stage contract epoch.
    pub fn compose(
        semantic: &SemanticKey,
        demand_closure: CanonicalDigest,
        structure_options: &[u8],
        structure_epoch: StageEpoch,
    ) -> Self {
        Self::from_canonical(&CompileStructureKeyDescriptor {
            semantic,
            demand_closure,
            structure_options,
            structure_epoch,
        })
    }
}

verter_identity::digest_identity!(
    /// Target-plan-stage identity: which per-target plan the compile
    /// executes, over the structure identity plus the target,
    /// target-plan-class options and contract epoch.
    TargetPlanKey
);

/// Descriptor behind [`TargetPlanKey`]; field numbers are the public
/// compatibility contract.
struct TargetPlanKeyDescriptor<'a> {
    structure: &'a CompileStructureKey,
    target: CanonicalDigest,
    target_plan_options: &'a [u8],
    target_epoch: StageEpoch,
}

impl CanonicalEncode for TargetPlanKeyDescriptor<'_> {
    const DOMAIN_TAG: &'static str = TargetPlanKey::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.structure.digest().as_bytes())
            .field_bytes(2, self.target.as_bytes())
            .field_bytes(3, self.target_plan_options)
            .field_u32(4, self.target_epoch.0);
    }
}

impl TargetPlanKey {
    /// The fixed compatibility domain of the target-plan-stage encoding.
    pub const DOMAIN_TAG: &'static str = "verter.compiler.stage.target-plan.v1";

    /// Compose the target-plan-stage identity: the structure stage's
    /// identity, the canonical digest of the target this plan executes
    /// for, the canonical bytes of the target-plan-class option set, and
    /// the target-stage contract epoch.
    pub fn compose(
        structure: &CompileStructureKey,
        target: CanonicalDigest,
        target_plan_options: &[u8],
        target_epoch: StageEpoch,
    ) -> Self {
        Self::from_canonical(&TargetPlanKeyDescriptor {
            structure,
            target,
            target_plan_options,
            target_epoch,
        })
    }
}

verter_identity::digest_identity!(
    /// Emit-stage identity: which emission the compile produces, over the
    /// target-plan identity plus the emit-class options, the map mode and
    /// the contract epoch. The emit planner keys its plans by this
    /// identity.
    EmitKey
);

/// Descriptor behind [`EmitKey`]; field numbers are the public
/// compatibility contract.
struct EmitKeyDescriptor<'a> {
    target_plan: &'a TargetPlanKey,
    emit_options: &'a [u8],
    map_mode: CanonicalDigest,
    emit_epoch: StageEpoch,
}

impl CanonicalEncode for EmitKeyDescriptor<'_> {
    const DOMAIN_TAG: &'static str = EmitKey::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.target_plan.digest().as_bytes())
            .field_bytes(2, self.emit_options)
            .field_bytes(3, self.map_mode.as_bytes())
            .field_u32(4, self.emit_epoch.0);
    }
}

impl EmitKey {
    /// The fixed compatibility domain of the emit-stage encoding.
    pub const DOMAIN_TAG: &'static str = "verter.compiler.stage.emit.v1";

    /// Compose the emit-stage identity: the target-plan stage's identity,
    /// the canonical bytes of the emit-class option set, the canonical
    /// digest of the map mode (map demand is emit-class; map encoding is
    /// terminal-class and enters the terminal key only), and the
    /// emit-stage contract epoch.
    pub fn compose(
        target_plan: &TargetPlanKey,
        emit_options: &[u8],
        map_mode: CanonicalDigest,
        emit_epoch: StageEpoch,
    ) -> Self {
        Self::from_canonical(&EmitKeyDescriptor {
            target_plan,
            emit_options,
            map_mode,
            emit_epoch,
        })
    }
}

verter_identity::digest_identity!(
    /// Terminal-stage identity: which terminal form the compile publishes,
    /// over the emit identity plus the terminal-class option bytes and the
    /// contract epoch. Output, presentation and serialization profile
    /// identities are terminal-class options and enter here only.
    TerminalKey
);

/// Descriptor behind [`TerminalKey`]; field numbers are the public
/// compatibility contract.
struct TerminalKeyDescriptor<'a> {
    emit: &'a EmitKey,
    terminal_options: &'a [u8],
    terminal_epoch: StageEpoch,
}

impl CanonicalEncode for TerminalKeyDescriptor<'_> {
    const DOMAIN_TAG: &'static str = TerminalKey::DOMAIN_TAG;

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder
            .field_bytes(1, self.emit.digest().as_bytes())
            .field_bytes(2, self.terminal_options)
            .field_u32(3, self.terminal_epoch.0);
    }
}

impl TerminalKey {
    /// The fixed compatibility domain of the terminal-stage encoding.
    pub const DOMAIN_TAG: &'static str = "verter.compiler.stage.terminal.v1";

    /// Compose the terminal-stage identity: the emit stage's identity, the
    /// canonical bytes of the terminal-class option set, and the
    /// terminal-stage contract epoch.
    pub fn compose(emit: &EmitKey, terminal_options: &[u8], terminal_epoch: StageEpoch) -> Self {
        Self::from_canonical(&TerminalKeyDescriptor {
            emit,
            terminal_options,
            terminal_epoch,
        })
    }
}
