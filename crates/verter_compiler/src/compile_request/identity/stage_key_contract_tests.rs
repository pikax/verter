//! Table-driven contract of the stage identities declared in this module.
//!
//! Discriminated boundaries: canonical bytes are stable for equal inputs
//! through independent construction; every typed input is load-bearing (a
//! change in exactly one input re-keys the stage); set-like inputs are
//! order- and repeat-independent; the bytes carry this stage's one domain
//! tag; and construction retains one allocation per key, hashed once.
//!
//! The parse identity is re-exported, not declared, here; its own
//! encoding contract is pinned where it is owned (`verter_language`
//! `parse_identity` tests). This table covers the five keys declared here
//! and the admission basis, and covers the parse identity in its composed
//! role as a set input. Incremental equivalence does not apply: pure
//! value types, no cache or publication authority; option-change
//! invalidation is tested where the classifier derives keys.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::ParseKey;

use super::{
    CompileStructureKey, EmitKey, SemanticAdmissionBasis, SemanticKey, StageEpoch, TargetPlanKey,
    TerminalKey,
};

/// The contract every identity declared in this module satisfies.
trait StageKeyContract {
    type Key: Clone + PartialEq + Eq + Hash + std::fmt::Debug;

    /// This stage's fixed compatibility domain.
    const DOMAIN_TAG: &'static str;

    /// A key over the base inputs, built afresh on every call so two calls
    /// are two independent constructions.
    fn compose_base(&self) -> Self::Key;

    /// One key per typed input, each differing from the base in exactly
    /// that input.
    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)>;

    /// Keys whose set-like inputs were supplied in a different order or
    /// with repeats; each must compose the base identity.
    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)>;

    fn digest(key: &Self::Key) -> CanonicalDigest;
    fn canonical_bytes(key: &Self::Key) -> &[u8];
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]>;
}

fn hash_of<K: Hash>(key: &K) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// A parse-identity fixture: a distinct typed input value whose own
/// encoding contract is owned and pinned in `verter_language`.
struct FixtureParseDescriptor(u64);

impl CanonicalEncode for FixtureParseDescriptor {
    const DOMAIN_TAG: &'static str = "stage-key-contract-test.parse-fixture.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u64(1, self.0);
    }
}

fn parse_key(seed: u64) -> ParseKey {
    ParseKey::from_canonical(&FixtureParseDescriptor(seed))
}

fn fixture_digest(label: &[u8]) -> CanonicalDigest {
    CanonicalDigest::of_bytes(label)
}

/// Owned [`SemanticKey`] inputs, so each variant mutates exactly one.
#[derive(Clone)]
struct SemanticKeyParams {
    source_units: Vec<ParseKey>,
    framework_profile: CanonicalDigest,
    semantic_options: Vec<u8>,
    semantic_epoch: StageEpoch,
    admission_basis: SemanticAdmissionBasis,
}

impl SemanticKeyParams {
    fn base() -> Self {
        Self {
            source_units: vec![parse_key(1), parse_key(2)],
            framework_profile: fixture_digest(b"framework-profile"),
            semantic_options: b"semantic-class-options".to_vec(),
            semantic_epoch: StageEpoch(4),
            admission_basis: AdmissionBasisParams::base().basis(),
        }
    }

    fn key(&self) -> SemanticKey {
        SemanticKey::compose(
            &self.source_units,
            self.framework_profile,
            &self.semantic_options,
            self.semantic_epoch,
            &self.admission_basis,
        )
    }
}

/// Owned [`SemanticAdmissionBasis`] inputs.
#[derive(Clone)]
struct AdmissionBasisParams {
    demanded_regions: Vec<CanonicalDigest>,
    admitted_facts: Vec<CanonicalDigest>,
    supplied_style_stages: Vec<CanonicalDigest>,
}

impl AdmissionBasisParams {
    fn base() -> Self {
        Self {
            demanded_regions: vec![fixture_digest(b"region-1"), fixture_digest(b"region-2")],
            admitted_facts: vec![fixture_digest(b"fact-1")],
            supplied_style_stages: vec![fixture_digest(b"style-stage-1")],
        }
    }

    fn basis(&self) -> SemanticAdmissionBasis {
        SemanticAdmissionBasis::from_parts(
            &self.demanded_regions,
            &self.admitted_facts,
            &self.supplied_style_stages,
        )
    }
}

struct SemanticKeyRow;

impl StageKeyContract for SemanticKeyRow {
    type Key = SemanticKey;
    const DOMAIN_TAG: &'static str = SemanticKey::DOMAIN_TAG;

    fn compose_base(&self) -> Self::Key {
        SemanticKeyParams::base().key()
    }

    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = SemanticKeyParams::base();
        let mut regions = AdmissionBasisParams::base();
        regions.demanded_regions.push(fixture_digest(b"region-3"));

        let mut units = base.clone();
        units.source_units.push(parse_key(3));
        let mut profile = base.clone();
        profile.framework_profile = fixture_digest(b"framework-profile-b");
        let mut options = base.clone();
        options.semantic_options = b"semantic-class-options-b".to_vec();
        let mut epoch = base.clone();
        epoch.semantic_epoch = StageEpoch(5);
        let mut basis = base.clone();
        basis.admission_basis = regions.basis();

        vec![
            ("source units", units.key()),
            ("framework profile", profile.key()),
            ("semantic options", options.key()),
            ("semantic epoch", epoch.key()),
            ("admission basis", basis.key()),
        ]
    }

    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = SemanticKeyParams::base();
        let mut reordered = base.clone();
        reordered.source_units = vec![parse_key(2), parse_key(1)];
        let mut repeated = base.clone();
        repeated.source_units = vec![parse_key(1), parse_key(2), parse_key(1)];
        vec![
            ("reordered source units", reordered.key()),
            ("repeated source unit", repeated.key()),
        ]
    }

    fn digest(key: &Self::Key) -> CanonicalDigest {
        key.digest()
    }
    fn canonical_bytes(key: &Self::Key) -> &[u8] {
        key.canonical_bytes()
    }
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]> {
        key.canonical_shared_bytes()
    }
}

struct AdmissionBasisRow;

impl StageKeyContract for AdmissionBasisRow {
    type Key = SemanticAdmissionBasis;
    const DOMAIN_TAG: &'static str = SemanticAdmissionBasis::DOMAIN_TAG;

    fn compose_base(&self) -> Self::Key {
        AdmissionBasisParams::base().basis()
    }

    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = AdmissionBasisParams::base();
        let mut regions = base.clone();
        regions.demanded_regions.push(fixture_digest(b"region-3"));
        let mut facts = base.clone();
        facts.admitted_facts.push(fixture_digest(b"fact-2"));
        let mut styles = base.clone();
        styles
            .supplied_style_stages
            .push(fixture_digest(b"style-stage-2"));
        vec![
            ("demanded regions", regions.basis()),
            ("admitted facts", facts.basis()),
            ("supplied style stages", styles.basis()),
        ]
    }

    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = AdmissionBasisParams::base();
        let mut reordered = base.clone();
        reordered.demanded_regions.reverse();
        let mut repeated_regions = base.clone();
        repeated_regions.demanded_regions = vec![
            fixture_digest(b"region-1"),
            fixture_digest(b"region-2"),
            fixture_digest(b"region-1"),
        ];
        let mut repeated_fact = base.clone();
        repeated_fact.admitted_facts = vec![fixture_digest(b"fact-1"), fixture_digest(b"fact-1")];
        vec![
            ("reordered regions", reordered.basis()),
            ("repeated region", repeated_regions.basis()),
            ("repeated fact", repeated_fact.basis()),
        ]
    }

    fn digest(key: &Self::Key) -> CanonicalDigest {
        key.digest()
    }
    fn canonical_bytes(key: &Self::Key) -> &[u8] {
        key.canonical_bytes()
    }
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]> {
        key.canonical_shared_bytes()
    }
}

/// Owned [`CompileStructureKey`] inputs.
#[derive(Clone)]
struct CompileStructureKeyParams {
    semantic: SemanticKey,
    demand_closure: CanonicalDigest,
    structure_options: Vec<u8>,
    structure_epoch: StageEpoch,
}

impl CompileStructureKeyParams {
    fn base() -> Self {
        Self {
            semantic: SemanticKeyParams::base().key(),
            demand_closure: fixture_digest(b"demand-closure"),
            structure_options: b"structure-class-options".to_vec(),
            structure_epoch: StageEpoch(7),
        }
    }

    fn key(&self) -> CompileStructureKey {
        CompileStructureKey::compose(
            &self.semantic,
            self.demand_closure,
            &self.structure_options,
            self.structure_epoch,
        )
    }
}

struct CompileStructureKeyRow;

impl StageKeyContract for CompileStructureKeyRow {
    type Key = CompileStructureKey;
    const DOMAIN_TAG: &'static str = CompileStructureKey::DOMAIN_TAG;

    fn compose_base(&self) -> Self::Key {
        CompileStructureKeyParams::base().key()
    }

    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = CompileStructureKeyParams::base();
        let mut upstream = SemanticKeyParams::base();
        upstream.semantic_epoch = StageEpoch(5);
        let mut semantic = base.clone();
        semantic.semantic = upstream.key();
        let mut closure = base.clone();
        closure.demand_closure = fixture_digest(b"demand-closure-b");
        let mut options = base.clone();
        options.structure_options = b"structure-class-options-b".to_vec();
        let mut epoch = base.clone();
        epoch.structure_epoch = StageEpoch(8);
        vec![
            ("semantic key", semantic.key()),
            ("demand closure", closure.key()),
            ("structure options", options.key()),
            ("structure epoch", epoch.key()),
        ]
    }

    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)> {
        Vec::new()
    }

    fn digest(key: &Self::Key) -> CanonicalDigest {
        key.digest()
    }
    fn canonical_bytes(key: &Self::Key) -> &[u8] {
        key.canonical_bytes()
    }
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]> {
        key.canonical_shared_bytes()
    }
}

/// Owned [`TargetPlanKey`] inputs.
#[derive(Clone)]
struct TargetPlanKeyParams {
    structure: CompileStructureKey,
    target: CanonicalDigest,
    target_plan_options: Vec<u8>,
    target_epoch: StageEpoch,
}

impl TargetPlanKeyParams {
    fn base() -> Self {
        Self {
            structure: CompileStructureKeyParams::base().key(),
            target: fixture_digest(b"target"),
            target_plan_options: b"target-plan-class-options".to_vec(),
            target_epoch: StageEpoch(9),
        }
    }

    fn key(&self) -> TargetPlanKey {
        TargetPlanKey::compose(
            &self.structure,
            self.target,
            &self.target_plan_options,
            self.target_epoch,
        )
    }
}

struct TargetPlanKeyRow;

impl StageKeyContract for TargetPlanKeyRow {
    type Key = TargetPlanKey;
    const DOMAIN_TAG: &'static str = TargetPlanKey::DOMAIN_TAG;

    fn compose_base(&self) -> Self::Key {
        TargetPlanKeyParams::base().key()
    }

    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = TargetPlanKeyParams::base();
        let mut upstream = CompileStructureKeyParams::base();
        upstream.structure_epoch = StageEpoch(8);
        let mut structure = base.clone();
        structure.structure = upstream.key();
        let mut target = base.clone();
        target.target = fixture_digest(b"target-b");
        let mut options = base.clone();
        options.target_plan_options = b"target-plan-class-options-b".to_vec();
        let mut epoch = base.clone();
        epoch.target_epoch = StageEpoch(10);
        vec![
            ("structure key", structure.key()),
            ("target", target.key()),
            ("target-plan options", options.key()),
            ("target epoch", epoch.key()),
        ]
    }

    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)> {
        Vec::new()
    }

    fn digest(key: &Self::Key) -> CanonicalDigest {
        key.digest()
    }
    fn canonical_bytes(key: &Self::Key) -> &[u8] {
        key.canonical_bytes()
    }
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]> {
        key.canonical_shared_bytes()
    }
}

/// Owned [`EmitKey`] inputs.
#[derive(Clone)]
struct EmitKeyParams {
    target_plan: TargetPlanKey,
    emit_options: Vec<u8>,
    map_mode: CanonicalDigest,
    emit_epoch: StageEpoch,
}

impl EmitKeyParams {
    fn base() -> Self {
        Self {
            target_plan: TargetPlanKeyParams::base().key(),
            emit_options: b"emit-class-options".to_vec(),
            map_mode: fixture_digest(b"map-mode"),
            emit_epoch: StageEpoch(11),
        }
    }

    fn key(&self) -> EmitKey {
        EmitKey::compose(
            &self.target_plan,
            &self.emit_options,
            self.map_mode,
            self.emit_epoch,
        )
    }
}

struct EmitKeyRow;

impl StageKeyContract for EmitKeyRow {
    type Key = EmitKey;
    const DOMAIN_TAG: &'static str = EmitKey::DOMAIN_TAG;

    fn compose_base(&self) -> Self::Key {
        EmitKeyParams::base().key()
    }

    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = EmitKeyParams::base();
        let mut upstream = TargetPlanKeyParams::base();
        upstream.target_epoch = StageEpoch(10);
        let mut plan = base.clone();
        plan.target_plan = upstream.key();
        let mut options = base.clone();
        options.emit_options = b"emit-class-options-b".to_vec();
        let mut map_mode = base.clone();
        map_mode.map_mode = fixture_digest(b"map-mode-b");
        let mut epoch = base.clone();
        epoch.emit_epoch = StageEpoch(12);
        vec![
            ("target-plan key", plan.key()),
            ("emit options", options.key()),
            ("map mode", map_mode.key()),
            ("emit epoch", epoch.key()),
        ]
    }

    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)> {
        Vec::new()
    }

    fn digest(key: &Self::Key) -> CanonicalDigest {
        key.digest()
    }
    fn canonical_bytes(key: &Self::Key) -> &[u8] {
        key.canonical_bytes()
    }
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]> {
        key.canonical_shared_bytes()
    }
}

/// Owned [`TerminalKey`] inputs.
#[derive(Clone)]
struct TerminalKeyParams {
    emit: EmitKey,
    terminal_options: Vec<u8>,
    terminal_epoch: StageEpoch,
}

impl TerminalKeyParams {
    fn base() -> Self {
        Self {
            emit: EmitKeyParams::base().key(),
            terminal_options: b"terminal-class-options".to_vec(),
            terminal_epoch: StageEpoch(13),
        }
    }

    fn key(&self) -> TerminalKey {
        TerminalKey::compose(&self.emit, &self.terminal_options, self.terminal_epoch)
    }
}

struct TerminalKeyRow;

impl StageKeyContract for TerminalKeyRow {
    type Key = TerminalKey;
    const DOMAIN_TAG: &'static str = TerminalKey::DOMAIN_TAG;

    fn compose_base(&self) -> Self::Key {
        TerminalKeyParams::base().key()
    }

    fn one_field_variants(&self) -> Vec<(&'static str, Self::Key)> {
        let base = TerminalKeyParams::base();
        let mut upstream = EmitKeyParams::base();
        upstream.emit_epoch = StageEpoch(12);
        let mut emit = base.clone();
        emit.emit = upstream.key();
        let mut options = base.clone();
        options.terminal_options = b"terminal-class-options-b".to_vec();
        let mut epoch = base.clone();
        epoch.terminal_epoch = StageEpoch(14);
        vec![
            ("emit key", emit.key()),
            ("terminal options", options.key()),
            ("terminal epoch", epoch.key()),
        ]
    }

    fn set_order_variants(&self) -> Vec<(&'static str, Self::Key)> {
        Vec::new()
    }

    fn digest(key: &Self::Key) -> CanonicalDigest {
        key.digest()
    }
    fn canonical_bytes(key: &Self::Key) -> &[u8] {
        key.canonical_bytes()
    }
    fn shared_bytes(key: &Self::Key) -> &Arc<[u8]> {
        key.canonical_shared_bytes()
    }
}

/// Runs the shared contract over one row, labeling every assertion with
/// the stage and input it discriminates.
fn run_stage_contract<R: StageKeyContract>(label: &'static str, row: &R) {
    let base = row.compose_base();
    let twin = row.compose_base();

    assert_eq!(base, twin, "{label}: equal inputs compose one identity");
    assert_eq!(
        R::digest(&base),
        R::digest(&twin),
        "{label}: equal inputs compose one digest"
    );
    assert_eq!(
        R::canonical_bytes(&base),
        R::canonical_bytes(&twin),
        "{label}: equal inputs compose one canonical byte encoding"
    );
    assert_eq!(
        hash_of(&base),
        hash_of(&twin),
        "{label}: equal inputs hash equally"
    );

    for (field, variant) in row.one_field_variants() {
        assert_ne!(base, variant, "{label}: {field} is load-bearing");
        assert_ne!(
            R::digest(&base),
            R::digest(&variant),
            "{label}: {field} must change the digest"
        );
        assert_ne!(
            R::canonical_bytes(&base),
            R::canonical_bytes(&variant),
            "{label}: {field} must change the canonical bytes"
        );
    }

    for (field, variant) in row.set_order_variants() {
        assert_eq!(base, variant, "{label}: {field} must not re-key");
        assert_eq!(
            R::canonical_bytes(&base),
            R::canonical_bytes(&variant),
            "{label}: {field} must not change the canonical bytes"
        );
    }

    // The bytes carry this stage's one domain tag in their header, so an
    // equal payload under a different stage can never collide with it.
    let bytes = R::canonical_bytes(&base);
    let tag_len = u32::from_le_bytes(bytes[0..4].try_into().expect("u32 header")) as usize;
    assert_eq!(
        &bytes[4..4 + tag_len],
        R::DOMAIN_TAG.as_bytes(),
        "{label}: canonical bytes start with this stage's domain tag"
    );

    // One retained allocation per key, hashed once: a clone shares the
    // canonical bytes instead of copying them; an equal independently
    // composed key owns its own allocation with equal bytes; and the
    // stored digest is exactly one hash pass over the retained bytes, so
    // comparisons and hash-map lookups never re-hash them.
    let clone = base.clone();
    assert!(
        Arc::ptr_eq(R::shared_bytes(&base), R::shared_bytes(&clone)),
        "{label}: a clone shares the one retained allocation"
    );
    assert!(
        !Arc::ptr_eq(R::shared_bytes(&base), R::shared_bytes(&twin)),
        "{label}: an independent construction owns its own allocation"
    );
    assert_eq!(
        R::digest(&base),
        CanonicalDigest::of_bytes(R::canonical_bytes(&base)),
        "{label}: the digest is one hash pass over the canonical bytes"
    );
}

#[test]
fn stage_keys_satisfy_the_composition_contract() {
    run_stage_contract("SemanticAdmissionBasis", &AdmissionBasisRow);
    run_stage_contract("SemanticKey", &SemanticKeyRow);
    run_stage_contract("CompileStructureKey", &CompileStructureKeyRow);
    run_stage_contract("TargetPlanKey", &TargetPlanKeyRow);
    run_stage_contract("EmitKey", &EmitKeyRow);
    run_stage_contract("TerminalKey", &TerminalKeyRow);
}

/// A domain tag is a compatibility domain: changing one silently re-keys
/// every slot keyed by the stage. Pin all six so a change is deliberate.
#[test]
fn stage_domain_tags_are_pinned() {
    assert_eq!(
        SemanticAdmissionBasis::DOMAIN_TAG,
        "verter.compiler.stage.semantic-admission-basis.v1"
    );
    assert_eq!(SemanticKey::DOMAIN_TAG, "verter.compiler.stage.semantic.v1");
    assert_eq!(
        CompileStructureKey::DOMAIN_TAG,
        "verter.compiler.stage.compile-structure.v1"
    );
    assert_eq!(
        TargetPlanKey::DOMAIN_TAG,
        "verter.compiler.stage.target-plan.v1"
    );
    assert_eq!(EmitKey::DOMAIN_TAG, "verter.compiler.stage.emit.v1");
    assert_eq!(TerminalKey::DOMAIN_TAG, "verter.compiler.stage.terminal.v1");
}

/// The terminal key's exact canonical bytes, laid out by the shared
/// tagged length-delimited grammar (`identity-encoding.md` § 2): header
/// (`u32` tag length, tag, `u32` field count), then per field a `u16`
/// tag, a `u64` payload length and the payload. Because the whole vector
/// is pinned, this also proves no formatted rendering participates: the
/// epoch appears only as its four little-endian bytes, never as decimal
/// text, and no joined-set or `Debug` string occurs anywhere in the key.
#[test]
fn terminal_key_canonical_bytes_are_pinned() {
    let emit = EmitKeyParams::base().key();
    let key = TerminalKey::compose(&emit, b"terminal-class-options", StageEpoch(13));

    let mut expected = Vec::new();
    expected.extend_from_slice(&33u32.to_le_bytes());
    expected.extend_from_slice(b"verter.compiler.stage.terminal.v1");
    expected.extend_from_slice(&3u32.to_le_bytes());
    expected.extend_from_slice(&1u16.to_le_bytes());
    expected.extend_from_slice(&32u64.to_le_bytes());
    expected.extend_from_slice(emit.digest().as_bytes());
    expected.extend_from_slice(&2u16.to_le_bytes());
    expected.extend_from_slice(&22u64.to_le_bytes());
    expected.extend_from_slice(b"terminal-class-options");
    expected.extend_from_slice(&3u16.to_le_bytes());
    expected.extend_from_slice(&4u64.to_le_bytes());
    expected.extend_from_slice(&13u32.to_le_bytes());

    assert_eq!(key.canonical_bytes(), &expected[..]);
}
