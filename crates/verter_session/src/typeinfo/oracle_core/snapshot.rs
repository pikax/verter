//! Snapshot JSON schema + STRICT decode for the TS7 oracle harness
//! (`docs/arch/u0-oracle-harness-design.md` §Q1).
//!
//! A snapshot is the checked-in, tsgo-free proof artifact a lifted
//! `TypeExpr`-projection row compares against. This module owns the serde DTOs
//! for the full snapshot file shape (§Q1 field table) and the STRICT decode
//! path that the consumption driver + the coverage guards run.
//!
//! Strictness has two independent rails, both required by §Q1:
//!
//! 1. **Closed envelope.** Every struct is `#[serde(deny_unknown_fields)]`, so
//!    an unrecognized / mistyped field FAILS decode rather than being silently
//!    ignored — a snapshot whose shape drifted from the schema is rejected, not
//!    half-read.
//! 2. **No silent `TypeExpr` member loss.** `oracle_value` is a
//!    `TypeExpr::to_json_value()` document (the internally-tagged `kind` codec,
//!    fact 2). The shared `type_expr_from_json` decoder uses `filter_map` at the
//!    object-`properties` (`type_expr_json.rs:72`) and function-`parameters`
//!    (`:336`) sites, so a malformed member is SILENTLY DROPPED and the value
//!    decodes to a SMALLER `TypeExpr`. The strict decoder closes that by
//!    re-encoding the decoded value and asserting it round-trips BYTE-EQUAL to
//!    the (canonicalized) stored `oracle_value`: a dropped member reduces the
//!    re-encoded shape and breaks byte-equality, so a malformed-member snapshot
//!    FAILS rather than warm-validating against a smaller type.
//!
//! `identity` is a CLOSED TAGGED shape keyed by `oracle_value_kind`: for
//! `structured_type_expr` it strictly decodes to [`StructuredTypeExprIdentity`]
//! (also `deny_unknown_fields`); an unknown `oracle_value_kind` is rejected (a
//! future kind is a closed-tagged addition that bumps `ORACLE_SCHEMA_VERSION`).
//!
//! Lifts ZERO rows: this is the storage-schema foundation the
//! `TypeExpr`-projection oracle rows build on.

use serde::Deserialize;
use serde_json::{json, Value};
use verter_type_expr::type_expr_from_json;

use super::identity::{
    derive_relation_snapshot_id, derive_snapshot_id, projection_mode_from_tag, projection_mode_tag,
    BinderLayoutEntry, FreshnessTag, HostProject, HostSetupKind, InferenceModeTag, OracleValueKind,
    OverloadSelectionTag, PinnedEnv, ProbeRhsKind, QueryHelperKind, RelationKindTag,
    RelationPolicyRecord, RelationVerdictIdentity, SnapshotIdentity, VarianceTag, WorkspaceFileRef,
};
use super::normalize::canonical_json_string;
use super::probe;
use super::relation_probe::{self, RelationVerdict};

/// The closed set of `oracle_value_kind` discriminants this schema version
/// understands. Adding a kind is a CLOSED-tagged schema change that MUST bump
/// [`super::identity::ORACLE_SCHEMA_VERSION`] (a new kind carries a different
/// required `identity` shape). The `identity_is_kind_specific_schema_bumped`
/// guard ties this set's size to the schema version so the two cannot drift.
/// v4 seats `relation_verdict` (the relation-tuple-wire capture family).
#[allow(dead_code)]
pub(crate) const KNOWN_VALUE_KINDS: &[&str] = &["structured_type_expr", "relation_verdict"];

/// Why a snapshot could not be strictly decoded.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SnapshotDecodeError {
    /// The envelope or a nested struct failed serde decode (an unknown field, a
    /// missing required field, or a type mismatch).
    Envelope(String),
    /// `oracle_value_kind` is not in the closed [`KNOWN_VALUE_KINDS`] set.
    UnknownValueKind(String),
    /// `oracle_value` did not decode through `type_expr_from_json`.
    OracleValueNotTypeExpr,
    /// `oracle_value` decoded but did NOT round-trip byte-equal under re-encode —
    /// a silently-dropped object member / function parameter (the
    /// `type_expr_json.rs:72,336` `filter_map` loss).
    OracleValueLossyDecode,
    /// `identity` did not strictly decode to the kind-specific shape.
    Identity(String),
    /// A stored enum tag (`query_helper_kind` / `projection_mode` /
    /// `host_setup_kind` / `probe_rhs_kind` / a v4 relation axis) was not a
    /// recognized discriminant.
    BadTag(String),
    /// The `identity.probe_rhs_kind` / `raw_capture.probe_scaffold` /
    /// `raw_capture.probe_header` triple is inconsistent: a
    /// `distributive_identity` snapshot must record EXACTLY the versioned
    /// scaffold synthesis (helper decl + wrapped header RHS, a pure function of
    /// the query ordinal + symbol), and a `bare` snapshot must record none.
    ScaffoldInconsistent(String),
    /// A field meaningful only under the OTHER value kind was present (the
    /// closed kind-keyed schema rejects cross-kind fields): a
    /// `relation_verdict` snapshot carrying the v3 migration-fidelity mirror or
    /// a `source_admission_digest`, or a `structured_type_expr` snapshot
    /// carrying a v4 relation axis.
    CrossKindField(String),
    /// The v4 `relation_verdict` `oracle_value` / `raw_capture` was
    /// inconsistent: the verdict tag unknown, a false verdict with bindings, an
    /// ordinal/name not matching the identity's `binder_layout`, a duplicate
    /// binder name, a bound that fails the strict TypeExpr round-trip, or the
    /// recorded probe header / hover not re-deriving to the stored value.
    RelationValue(String),
}

// ---------------------------------------------------------------------------
// Envelope DTOs (§Q1 field table)
// ---------------------------------------------------------------------------

/// The full snapshot file shape. `identity` + `oracle_value` stay as raw
/// `Value` at the envelope (their shape is kind-specific / `TypeExpr`-shaped);
/// they are strictly decoded in a second pass keyed by `oracle_value_kind`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OracleSnapshot {
    pub(crate) oracle_schema_version: u32,
    pub(crate) normalizer_version: u32,
    pub(crate) probe_synthesis_version: u32,
    pub(crate) tsgo_version: String,
    pub(crate) compiler_options_hash: String,
    pub(crate) env_corpus_id: String,
    pub(crate) oracle_env_files: OracleEnvFiles,
    pub(crate) oracle_env_hash: String,
    pub(crate) oracle_family: String,
    pub(crate) oracle_value_kind: String,
    pub(crate) snapshot_id: String,
    /// The migration-fidelity mirror (v3, §Q4): the row's retained
    /// `migration_fingerprint` (+ algorithm version) computed at lift time from
    /// the ORIGINAL pre-`#[oracle_row]` body. NOT a `snapshot_id` input — bound
    /// to the retained-lift metadata by
    /// `snapshot_migration_fingerprint_matches_retained_lift_metadata`.
    /// KIND-KEYED (v4): REQUIRED for `structured_type_expr` (a lifted
    /// TypeExpr row), FORBIDDEN on `relation_verdict` (a capture-only relation
    /// row is never a lift and carries no retained-lift provenance — the fields
    /// are ABSENT there, never a sentinel). `Option` at the serde layer so the
    /// strict decoder can enforce the per-kind presence rule itself.
    pub(crate) migration_fingerprint_version: Option<u32>,
    pub(crate) migration_fingerprint: Option<String>,
    pub(crate) row_ref: RowRef,
    pub(crate) identity: Value,
    pub(crate) oracle_value: Value,
    pub(crate) raw_capture: RawCapture,
    /// The recorded source-side admission (v3, §Q1). KIND-KEYED (v4): REQUIRED
    /// for `structured_type_expr` (a lifted TypeExpr row records its source
    /// walk), FORBIDDEN on `relation_verdict` — a capture-only relation row has
    /// NO source-admission walk (its workspace file is the synthesized probe,
    /// which would REJECT under admission), so recording one would be
    /// fabricated data. `Option` at the serde layer so the strict decoder can
    /// enforce the per-kind presence rule itself.
    pub(crate) source_admission_digest: Option<SourceAdmissionDigest>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RowRef {
    pub(crate) row_file: String,
    pub(crate) row_function: String,
    pub(crate) query_ordinal: u16,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OracleEnvFiles {
    pub(crate) manifest: Vec<String>,
    pub(crate) files: Vec<EnvFileEntry>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvFileEntry {
    pub(crate) path: String,
    pub(crate) content_hash: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCapture {
    pub(crate) probe_name: String,
    pub(crate) probe_header: String,
    /// The scaffold helper declaration the probe RHS was wrapped in
    /// (`distributive_identity` captures only; `null` for `bare`). Re-derivable
    /// offline from `probe_synthesis_version` + the query ordinal — the strict
    /// decoder re-checks it.
    pub(crate) probe_scaffold: Option<String>,
    pub(crate) hover_contents: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceAdmissionDigest {
    pub(crate) source_locator: SourceLocatorDto,
    pub(crate) observed_source_files: Vec<EnvFileEntry>,
    pub(crate) contributors: Vec<SourceContributorDto>,
    pub(crate) final_verdict: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceLocatorDto {
    pub(crate) reference_canonical: String,
    pub(crate) reference_name: String,
    pub(crate) symbol_space: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceContributorDto {
    pub(crate) contributor_ordinal: u16,
    pub(crate) decl_span: DeclSpanDto,
    pub(crate) decl_canonical: String,
    pub(crate) name: String,
    pub(crate) symbol_space: String,
    pub(crate) decl_kind: String,
    pub(crate) raw_surface: Value,
    pub(crate) lowered_body: Value,
    pub(crate) verdict: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeclSpanDto {
    pub(crate) file: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

// ---------------------------------------------------------------------------
// Kind-specific identity DTO (closed-tagged by oracle_value_kind)
// ---------------------------------------------------------------------------

/// The `structured_type_expr` `identity` shape (§Q1 axis table). A different
/// `oracle_value_kind` would carry a DIFFERENT required axis set, so kinds are a
/// closed tagged schema — adding one bumps `ORACLE_SCHEMA_VERSION`.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StructuredTypeExprIdentity {
    pub(crate) query_helper_kind: String,
    pub(crate) workspace_files: Vec<WorkspaceFileDto>,
    pub(crate) primary_canonical: String,
    pub(crate) symbol_or_expression: String,
    pub(crate) type_arguments: Vec<Value>,
    pub(crate) projection_mode: String,
    pub(crate) probe_rhs_kind: String,
    pub(crate) host_project: HostProjectDto,
    pub(crate) probe_locator: ProbeLocatorDto,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkspaceFileDto {
    pub(crate) path: String,
    pub(crate) content_hash: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostProjectDto {
    pub(crate) project_root: String,
    pub(crate) workspace_root: String,
    pub(crate) tsconfig_path: String,
    pub(crate) host_setup_kind: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProbeLocatorDto {
    pub(crate) probe_name: String,
    pub(crate) offset: u64,
}

// ---------------------------------------------------------------------------
// The v4 `relation_verdict` kind-specific identity DTO (a DISTINCT closed shape)
// ---------------------------------------------------------------------------

/// The `relation_verdict` `identity` shape (v4). A DISTINCT closed shape from
/// [`StructuredTypeExprIdentity`] — `deny_unknown_fields` rejects every v3-only
/// axis (`query_helper_kind` / `primary_canonical` / `symbol_or_expression` /
/// `type_arguments` / `projection_mode` / `probe_rhs_kind`) as an unknown field,
/// and the v3 DTO likewise rejects every v4-only axis, so cross-kind fields
/// fail decode. NO graph-local node IDs: the operands are canonical AST JSON.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelationVerdictIdentityDto {
    pub(crate) workspace_files: Vec<WorkspaceFileDto>,
    /// Canonical normalized AST JSON of the SOURCE operand.
    pub(crate) source_operand: Value,
    /// Canonical normalized AST JSON of the TARGET operand (`infer X` positions
    /// encoded as reserved `__oracle_binder__X` refs).
    pub(crate) target_operand: Value,
    /// The target-pattern binder layout in binder preorder.
    pub(crate) binder_layout: Vec<BinderLayoutDto>,
    pub(crate) relation: String,
    pub(crate) policy: RelationPolicyDto,
    pub(crate) freshness: String,
    pub(crate) inference_mode: String,
    pub(crate) host_project: HostProjectDto,
    pub(crate) probe_locator: ProbeLocatorDto,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BinderLayoutDto {
    pub(crate) ordinal: u16,
    pub(crate) name: String,
    /// The binder's canonical constraint AST JSON (`infer X extends
    /// <constraint>`); ABSENT when unconstrained, so pre-constraint snapshots
    /// stay schema-valid and their stored ids redrive unchanged.
    #[serde(default)]
    pub(crate) constraint: Option<Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelationPolicyDto {
    pub(crate) overload_selection: String,
    pub(crate) excess_property_check: bool,
    pub(crate) variance: String,
}

/// The `relation_verdict` `oracle_value` DTO: the verdict plus the ordered
/// inference bindings.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelationVerdictValueDto {
    pub(crate) verdict: String,
    pub(crate) bindings: Vec<RelationBindingDto>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelationBindingDto {
    pub(crate) ordinal: u16,
    pub(crate) name: String,
    /// Canonical normalized TypeExpr JSON of the bound type (under the ONE
    /// relation-binding projection). NEVER a bare node id / integer.
    pub(crate) bound: Value,
}

// ---------------------------------------------------------------------------
// Strict decode
// ---------------------------------------------------------------------------

/// Strictly decode a snapshot JSON document (§Q1):
/// 1. envelope decode under `deny_unknown_fields`;
/// 2. `oracle_value_kind` ∈ the closed [`KNOWN_VALUE_KINDS`];
/// 3. the kind-keyed migration-mirror presence rule (required for
///    `structured_type_expr`, forbidden as a cross-kind field on
///    `relation_verdict`);
/// 4. `identity` strictly decodes to the KIND-SPECIFIC shape (cross-kind fields
///    rejected);
/// 5. the kind-specific value + capture rails (v3: scaffold triple + TypeExpr
///    round-trip; v4: relation raw-capture re-derivation + verdict/binding
///    strictness).
#[allow(dead_code)]
pub(crate) fn decode_strict(json: &Value) -> Result<OracleSnapshot, SnapshotDecodeError> {
    let snapshot: OracleSnapshot = serde_json::from_value(json.clone())
        .map_err(|e| SnapshotDecodeError::Envelope(e.to_string()))?;

    if !KNOWN_VALUE_KINDS.contains(&snapshot.oracle_value_kind.as_str()) {
        return Err(SnapshotDecodeError::UnknownValueKind(
            snapshot.oracle_value_kind.clone(),
        ));
    }
    let value_kind = OracleValueKind::from_tag(&snapshot.oracle_value_kind)
        .ok_or_else(|| SnapshotDecodeError::UnknownValueKind(snapshot.oracle_value_kind.clone()))?;

    // The kind-keyed migration-fidelity mirror: REQUIRED on `structured_type_expr`
    // (an absent v3 field is an Envelope-shaped failure), FORBIDDEN on
    // `relation_verdict` (a present one is a cross-kind field).
    match value_kind {
        OracleValueKind::StructuredTypeExpr => {
            if snapshot.migration_fingerprint.is_none()
                || snapshot.migration_fingerprint_version.is_none()
            {
                return Err(SnapshotDecodeError::Envelope(
                    "structured_type_expr snapshot missing the required migration-fidelity \
                     mirror (migration_fingerprint / migration_fingerprint_version)"
                        .to_string(),
                ));
            }
            // The source-admission digest is likewise REQUIRED for a lifted
            // TypeExpr row (its source walk is recorded evidence).
            if snapshot.source_admission_digest.is_none() {
                return Err(SnapshotDecodeError::Envelope(
                    "structured_type_expr snapshot missing the required \
                     source_admission_digest"
                        .to_string(),
                ));
            }
        }
        OracleValueKind::RelationVerdict => {
            if snapshot.migration_fingerprint.is_some()
                || snapshot.migration_fingerprint_version.is_some()
            {
                return Err(SnapshotDecodeError::CrossKindField(
                    "relation_verdict snapshot carries the v3 migration-fidelity mirror"
                        .to_string(),
                ));
            }
            // A capture-only relation row has NO source-admission walk — a
            // recorded digest would be fabricated data (cross-kind field).
            if snapshot.source_admission_digest.is_some() {
                return Err(SnapshotDecodeError::CrossKindField(
                    "relation_verdict snapshot carries a source_admission_digest".to_string(),
                ));
            }
        }
    }

    // Kind-specific identity strict decode + tag validation.
    let identity = decode_identity(&snapshot.oracle_value_kind, &snapshot.identity)?;

    match identity {
        DecodedIdentity::StructuredTypeExpr(v3_identity) => {
            // Capture-strategy consistency: the recorded scaffold + probe header
            // must be EXACTLY the versioned synthesis for the declared
            // `probe_rhs_kind`.
            validate_probe_scaffold(&snapshot, &v3_identity)?;
            // oracle_value strict TypeExpr decode + lossless round-trip.
            decode_oracle_value_strict(&snapshot.oracle_value)?;
        }
        DecodedIdentity::RelationVerdict(v4_identity) => {
            // The v4 capture rail: the recorded probe header must be EXACTLY the
            // versioned tuple-wire synthesis re-derived from the identity, and
            // the recorded hover must re-decode through the strict tuple-wire
            // decoder to the stored verdict + bindings.
            validate_relation_raw_capture(&snapshot, &v4_identity)?;
            // oracle_value strict relation-verdict decode (verdict tag, binding
            // ordinals/names vs the identity binder layout, per-bound TypeExpr
            // round-trip).
            decode_relation_value_strict(&snapshot.oracle_value, &v4_identity)?;
        }
    }

    Ok(snapshot)
}

/// The cross-field capture-strategy rail: a `distributive_identity` snapshot
/// records EXACTLY the versioned scaffold (helper decl re-derived from the
/// query ordinal) and the WRAPPED probe header (re-derived from ordinal +
/// symbol); a `bare` snapshot records NO scaffold. Both directions reject —
/// so a stale, tampered, or strategy-mislabelled capture fails strict decode
/// instead of half-validating.
fn validate_probe_scaffold(
    snapshot: &OracleSnapshot,
    identity: &StructuredTypeExprIdentity,
) -> Result<(), SnapshotDecodeError> {
    let kind = ProbeRhsKind::from_tag(&identity.probe_rhs_kind)
        .ok_or_else(|| SnapshotDecodeError::BadTag(identity.probe_rhs_kind.clone()))?;
    let ordinal = snapshot.row_ref.query_ordinal;
    match kind {
        ProbeRhsKind::Bare => {
            if let Some(stray) = &snapshot.raw_capture.probe_scaffold {
                return Err(SnapshotDecodeError::ScaffoldInconsistent(format!(
                    "bare capture carries a stray probe_scaffold: {stray}"
                )));
            }
        }
        ProbeRhsKind::DistributiveIdentity => {
            let expected =
                probe::distributive_identity_scaffold(ordinal, &identity.symbol_or_expression);
            match &snapshot.raw_capture.probe_scaffold {
                None => {
                    return Err(SnapshotDecodeError::ScaffoldInconsistent(
                        "distributive_identity capture records no probe_scaffold".to_string(),
                    ));
                }
                Some(stored) if *stored != expected.helper_decl => {
                    return Err(SnapshotDecodeError::ScaffoldInconsistent(format!(
                        "stored probe_scaffold `{stored}` != versioned synthesis \
                         `{}`",
                        expected.helper_decl
                    )));
                }
                Some(_) => {}
            }
            let expected_header = probe::probe_header(ordinal, &expected.rhs);
            if snapshot.raw_capture.probe_header != expected_header {
                return Err(SnapshotDecodeError::ScaffoldInconsistent(format!(
                    "stored probe_header `{}` != wrapped synthesis `{expected_header}`",
                    snapshot.raw_capture.probe_header
                )));
            }
        }
    }
    Ok(())
}

/// The v4 raw-capture rail: (a) NO `probe_scaffold` (the relation tuple-wire
/// probe has no scaffold — a recorded one is a cross-kind artefact); (b) the
/// recorded `probe_header` is EXACTLY the versioned tuple-wire synthesis
/// re-derived from the identity's operands + binder layout (a pure function —
/// the operands' canonical ASTs are re-canonicalized from the recorded
/// `raw_capture` texts… NO: the identity stores the canonical ASTs, so the
/// header is re-derived from the same operand TEXTS the identity was built
/// from — see `relation_probe::relation_probe_header`); (c) the recorded hover
/// re-decodes through the STRICT tuple-wire decoder, and the re-decoded
/// verdict + bindings equal the stored `oracle_value` under the relation-value
/// canonical form. A stale, tampered, or hand-edited capture fails here.
fn validate_relation_raw_capture(
    snapshot: &OracleSnapshot,
    identity: &RelationVerdictIdentityDto,
) -> Result<(), SnapshotDecodeError> {
    if let Some(stray) = &snapshot.raw_capture.probe_scaffold {
        return Err(SnapshotDecodeError::CrossKindField(format!(
            "relation_verdict capture carries a stray probe_scaffold: {stray}"
        )));
    }
    // (a2) The probe identity is BOUND, never self-anchored: the expected
    // probe name is the VERSIONED synthesis of the row's query ordinal
    // (`probe::probe_name`), and the stored `identity.probe_locator.probe_name`
    // AND `raw_capture.probe_name` must BOTH equal it (a rename of every probe
    // in the file — consistent across header, hover, and both stored names —
    // fails here; pre-F2 the raw_capture name was its own anchor and such a
    // rename passed).
    let expected_probe_name = super::probe::probe_name(snapshot.row_ref.query_ordinal);
    if identity.probe_locator.probe_name != expected_probe_name {
        return Err(SnapshotDecodeError::RelationValue(format!(
            "identity.probe_locator.probe_name `{}` != the versioned probe name              `{expected_probe_name}` for query ordinal {}",
            identity.probe_locator.probe_name, snapshot.row_ref.query_ordinal
        )));
    }
    if snapshot.raw_capture.probe_name != expected_probe_name {
        return Err(SnapshotDecodeError::RelationValue(format!(
            "raw_capture.probe_name `{}` != the versioned probe name `{expected_probe_name}`",
            snapshot.raw_capture.probe_name
        )));
    }
    // (b) probe header re-derivation. The operand TEXTS live in the recorded
    // probe header's own synthesis inputs — the header is validated by
    // re-DECODING it against the BOUND probe name (the grammar is injective)
    // and checking the decoded operands canonicalize to the stored identity
    // operands (canonical ASTs erase display text, so the header is the only
    // place the texts are recorded).
    let (source_text, target_text, binder_names) = relation_probe::parse_probe_header(
        &snapshot.raw_capture.probe_header,
        &expected_probe_name,
    )
    .map_err(|e| SnapshotDecodeError::RelationValue(format!("probe header: {e:?}")))?;
    // The header's binder triples must carry EXACTLY the identity's layout
    // (names in preorder).
    let layout_names: Vec<&str> = identity
        .binder_layout
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    if binder_names != layout_names {
        return Err(SnapshotDecodeError::RelationValue(format!(
            "probe header binders {binder_names:?} != identity binder layout {layout_names:?}"
        )));
    }
    // The header's operand texts must canonicalize to the stored identity
    // operands — a hand-edited operand (or a canonical-AST edit) breaks this.
    let source_ast =
        relation_probe::canonical_operand_ast(&source_text, relation_probe::OperandRole::Source)
            .map_err(|e| {
                SnapshotDecodeError::RelationValue(format!("header source operand: {e:?}"))
            })?;
    if canonical_json_string(&source_ast) != canonical_json_string(&identity.source_operand) {
        return Err(SnapshotDecodeError::RelationValue(
            "probe header source operand does not canonicalize to the stored identity".to_string(),
        ));
    }
    let target_ast =
        relation_probe::canonical_operand_ast(&target_text, relation_probe::OperandRole::Target)
            .map_err(|e| {
                SnapshotDecodeError::RelationValue(format!("header target operand: {e:?}"))
            })?;
    if canonical_json_string(&target_ast) != canonical_json_string(&identity.target_operand) {
        return Err(SnapshotDecodeError::RelationValue(
            "probe header target operand does not canonicalize to the stored identity".to_string(),
        ));
    }
    // (b2) The probe FILE is bound to the identity: re-derive the versioned
    // probe source (a pure function of the row function, query ordinal, the
    // header-decoded operand texts, and the binder layout) and require the
    // identity's workspace-file axis to be EXACTLY the one probe file with the
    // re-derived content hash — a hand-edited probe file (or a workspace_files
    // hash corrupted to match nothing) fails. The stored
    // `identity.probe_locator.offset` must equal the probe name's byte offset
    // in the re-derived source (a corrupted locator offset fails).
    let layout: Vec<super::identity::BinderLayoutEntry> = identity
        .binder_layout
        .iter()
        .map(|b| super::identity::BinderLayoutEntry {
            ordinal: b.ordinal,
            name: b.name.clone(),
            constraint: b.constraint.clone(),
        })
        .collect();
    let probe_source = relation_probe::relation_probe_source(
        &snapshot.row_ref.row_function,
        snapshot.row_ref.query_ordinal,
        &source_text,
        &target_text,
        &layout,
    );
    let expected_files: Vec<(String, String)> = vec![(
        relation_probe::relation_probe_canonical_path(&snapshot.row_ref.row_function),
        super::identity::content_hash(&probe_source),
    )];
    let actual_files: Vec<(String, String)> = identity
        .workspace_files
        .iter()
        .map(|f| (f.path.clone(), f.content_hash.clone()))
        .collect();
    if actual_files != expected_files {
        return Err(SnapshotDecodeError::RelationValue(format!(
            "identity.workspace_files {actual_files:?} != the re-derived probe file              {expected_files:?} (probe content binding)"
        )));
    }
    let expected_offset = probe_source.find(&expected_probe_name).ok_or_else(|| {
        SnapshotDecodeError::RelationValue(
            "re-derived probe source does not contain the probe name".to_string(),
        )
    })? as u64;
    if identity.probe_locator.offset != expected_offset {
        return Err(SnapshotDecodeError::RelationValue(format!(
            "identity.probe_locator.offset {} != the probe name's byte offset              {expected_offset} in the re-derived probe source",
            identity.probe_locator.offset
        )));
    }
    // (c) the recorded hover re-decodes to the stored verdict + bindings.
    let rhs = super::hover_extract::extract_probe_rhs(
        &snapshot.raw_capture.hover_contents,
        &expected_probe_name,
    )
    .map_err(|e| SnapshotDecodeError::RelationValue(format!("hover extract: {e:?}")))?;
    let decoded = relation_probe::decode_tuple_wire(&rhs)
        .map_err(|e| SnapshotDecodeError::RelationValue(format!("tuple wire: {e:?}")))?;
    let stored = relation_value_canonical_form(&snapshot.oracle_value)?;
    let redecoded = relation_probe::relation_value_canonical_form(&decoded);
    if redecoded != stored {
        return Err(SnapshotDecodeError::RelationValue(format!(
            "recorded hover re-decodes to {redecoded}, not the stored {stored}"
        )));
    }
    Ok(())
}

/// The kind-specific decoded identity (closed-tagged by `oracle_value_kind`).
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum DecodedIdentity {
    StructuredTypeExpr(StructuredTypeExprIdentity),
    RelationVerdict(RelationVerdictIdentityDto),
}

/// Strictly decode the kind-specific `identity` shape, keyed by
/// `oracle_value_kind`: `structured_type_expr` decodes to the v3 shape,
/// `relation_verdict` to the DISTINCT v4 shape — each `deny_unknown_fields`,
/// so cross-kind fields fail decode. An unknown kind is rejected (a future
/// kind is a closed-tagged addition that bumps the schema version). The
/// embedded enum tags must parse to a known discriminant, and the v4 binder
/// layout must be internally consistent (ordinals exactly `0..n-1` in layout
/// order, unique names, `inference_mode` consistent with the layout, binder
/// names set-matching the reserved binder refs in `target_operand`).
#[allow(dead_code)]
pub(crate) fn decode_identity(
    oracle_value_kind: &str,
    identity: &Value,
) -> Result<DecodedIdentity, SnapshotDecodeError> {
    let kind = OracleValueKind::from_tag(oracle_value_kind)
        .ok_or_else(|| SnapshotDecodeError::UnknownValueKind(oracle_value_kind.to_string()))?;
    match kind {
        OracleValueKind::StructuredTypeExpr => {
            let dto: StructuredTypeExprIdentity = serde_json::from_value(identity.clone())
                .map_err(|e| SnapshotDecodeError::Identity(e.to_string()))?;

            // Validate the embedded closed-enum tags so a stored garbage tag is
            // caught at decode, not at redrive.
            QueryHelperKind::from_tag(&dto.query_helper_kind)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.query_helper_kind.clone()))?;
            projection_mode_from_tag(&dto.projection_mode)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.projection_mode.clone()))?;
            ProbeRhsKind::from_tag(&dto.probe_rhs_kind)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.probe_rhs_kind.clone()))?;
            HostSetupKind::from_tag(&dto.host_project.host_setup_kind).ok_or_else(|| {
                SnapshotDecodeError::BadTag(dto.host_project.host_setup_kind.clone())
            })?;

            Ok(DecodedIdentity::StructuredTypeExpr(dto))
        }
        OracleValueKind::RelationVerdict => {
            let dto: RelationVerdictIdentityDto = serde_json::from_value(identity.clone())
                .map_err(|e| SnapshotDecodeError::Identity(e.to_string()))?;
            validate_relation_identity_axes(&dto)?;
            Ok(DecodedIdentity::RelationVerdict(dto))
        }
    }
}

/// The v4 identity's internal-consistency rail: every embedded tag parses, the
/// binder layout is a well-formed preorder layout, `inference_mode` agrees
/// with it, and the layout's names set-match the reserved binder refs carried
/// by `target_operand`.
fn validate_relation_identity_axes(
    dto: &RelationVerdictIdentityDto,
) -> Result<(), SnapshotDecodeError> {
    RelationKindTag::from_tag(&dto.relation)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.relation.clone()))?;
    OverloadSelectionTag::from_tag(&dto.policy.overload_selection)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.policy.overload_selection.clone()))?;
    VarianceTag::from_tag(&dto.policy.variance)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.policy.variance.clone()))?;
    FreshnessTag::from_tag(&dto.freshness)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.freshness.clone()))?;
    let inference_mode = InferenceModeTag::from_tag(&dto.inference_mode)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.inference_mode.clone()))?;
    HostSetupKind::from_tag(&dto.host_project.host_setup_kind)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.host_project.host_setup_kind.clone()))?;

    // Binder layout: ordinals exactly 0..n-1 in layout (preorder) position,
    // names unique.
    let mut names: Vec<&str> = Vec::with_capacity(dto.binder_layout.len());
    for (index, binder) in dto.binder_layout.iter().enumerate() {
        if binder.ordinal as usize != index {
            return Err(SnapshotDecodeError::Identity(format!(
                "binder layout ordinal {} at position {index} breaks the 0..n-1 preorder",
                binder.ordinal
            )));
        }
        if names.contains(&binder.name.as_str()) {
            return Err(SnapshotDecodeError::Identity(format!(
                "duplicate binder name `{}` in the layout",
                binder.name
            )));
        }
        names.push(binder.name.as_str());
    }

    // inference_mode ⇔ binder count.
    let expected_mode = if dto.binder_layout.is_empty() {
        InferenceModeTag::None
    } else {
        InferenceModeTag::TargetPattern
    };
    if inference_mode != expected_mode {
        return Err(SnapshotDecodeError::Identity(format!(
            "inference_mode `{}` inconsistent with a {}-binder layout",
            dto.inference_mode,
            dto.binder_layout.len()
        )));
    }

    // Each declared constraint is a canonical TypeExpr document (decode +
    // byte-equal round-trip — a bare node id / malformed member rejects).
    for binder in &dto.binder_layout {
        if let Some(constraint) = &binder.constraint {
            let decoded = type_expr_from_json(constraint).ok_or_else(|| {
                SnapshotDecodeError::Identity(format!(
                    "binder `{}` constraint is not a TypeExpr document",
                    binder.name
                ))
            })?;
            if canonical_json_string(constraint) != canonical_json_string(&decoded.to_json_value())
            {
                return Err(SnapshotDecodeError::Identity(format!(
                    "binder `{}` constraint does not round-trip byte-equal (lossy member)",
                    binder.name
                )));
            }
        }
    }

    // The layout's names set-match the reserved binder refs in target_operand.
    let mut refs: Vec<String> = Vec::new();
    collect_binder_refs(&dto.target_operand, &mut refs);
    refs.sort();
    let mut layout_names: Vec<&str> = names.clone();
    layout_names.sort();
    if refs.iter().map(String::as_str).collect::<Vec<_>>() != layout_names {
        return Err(SnapshotDecodeError::Identity(format!(
            "binder layout names {layout_names:?} do not set-match the target operand's \
             reserved binder refs {refs:?}"
        )));
    }
    Ok(())
}

/// Collect the binder names named by reserved `__oracle_binder__X` refs inside
/// a canonical operand AST (deduplicated) — the shared collector lives in
/// `relation_probe` (the derivation + this decode rail share it).
fn collect_binder_refs(value: &Value, out: &mut Vec<String>) {
    relation_probe::collect_binder_refs(value, out)
}

/// Strictly decode `oracle_value` as a `TypeExpr` and assert it round-trips
/// BYTE-EQUAL under re-encode. The round-trip is the strictness lever: a member
/// the shared `type_expr_from_json` `filter_map` drops (`type_expr_json.rs:72`
/// object properties, `:336` function parameters) reduces the re-encoded shape,
/// breaking byte-equality — so a malformed-member value FAILS rather than
/// decoding to a smaller `TypeExpr`.
#[allow(dead_code)]
pub(crate) fn decode_oracle_value_strict(oracle_value: &Value) -> Result<(), SnapshotDecodeError> {
    let decoded =
        type_expr_from_json(oracle_value).ok_or(SnapshotDecodeError::OracleValueNotTypeExpr)?;
    let reencoded = decoded.to_json_value();
    if canonical_json_string(oracle_value) != canonical_json_string(&reencoded) {
        return Err(SnapshotDecodeError::OracleValueLossyDecode);
    }
    Ok(())
}

/// Strictly decode a `relation_verdict` `oracle_value` against its identity's
/// binder layout:
/// 1. the DTO is closed (`deny_unknown_fields`);
/// 2. `verdict` is a known closed tag; a `not_assignable` verdict carries NO
///    bindings (a failed-infer row — non-empty layout, false verdict — records
///    ZERO matched bindings by definition, and is representable);
/// 3. for an ASSIGNABLE verdict the bindings are EXACTLY the identity's
///    `binder_layout` in order — ordinal AND name must match (no name-sort, no
///    worklist order, no dropped/extra binding);
/// 4. every `bound` decodes through `type_expr_from_json` AND round-trips
///    byte-equal (the same no-silent-member-loss rail as the v3 value) — a
///    bare integer / node-id where a TypeExpr document belongs FAILS.
#[allow(dead_code)]
pub(crate) fn decode_relation_value_strict(
    oracle_value: &Value,
    identity: &RelationVerdictIdentityDto,
) -> Result<(), SnapshotDecodeError> {
    let dto: RelationVerdictValueDto = serde_json::from_value(oracle_value.clone())
        .map_err(|e| SnapshotDecodeError::RelationValue(format!("value DTO: {e}")))?;
    let verdict = RelationVerdict::from_tag(&dto.verdict)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.verdict.clone()))?;
    if verdict == RelationVerdict::NotAssignable && !dto.bindings.is_empty() {
        return Err(SnapshotDecodeError::RelationValue(
            "a not_assignable verdict carries bindings".to_string(),
        ));
    }
    // The layout-length + per-position preorder match applies to an ASSIGNABLE
    // verdict (every declared binder bound). A FAILED-infer row
    // (`not_assignable` with a non-empty layout) records ZERO matched bindings
    // by definition — the wire's false branch is always the empty tuple — so
    // no length/preorder check applies to it.
    if verdict == RelationVerdict::Assignable {
        if dto.bindings.len() != identity.binder_layout.len() {
            return Err(SnapshotDecodeError::RelationValue(format!(
                "binding count {} != identity binder layout count {}",
                dto.bindings.len(),
                identity.binder_layout.len()
            )));
        }
        for (index, binding) in dto.bindings.iter().enumerate() {
            let layout = &identity.binder_layout[index];
            if binding.ordinal != layout.ordinal || binding.name != layout.name {
                return Err(SnapshotDecodeError::RelationValue(format!(
                    "binding {index} ({}, `{}`) does not match the identity binder layout \
                     ({}, `{}`) — ordinal AND name must match in binder preorder",
                    binding.ordinal, binding.name, layout.ordinal, layout.name
                )));
            }
        }
    }
    for (index, binding) in dto.bindings.iter().enumerate() {
        // The bound must decode AND round-trip byte-equal (a bare graph-node
        // integer fails `type_expr_from_json`; a malformed member breaks the
        // byte-equality).
        let decoded = type_expr_from_json(&binding.bound).ok_or_else(|| {
            SnapshotDecodeError::RelationValue(format!(
                "binding {index} bound is not a TypeExpr document"
            ))
        })?;
        let reencoded = decoded.to_json_value();
        if canonical_json_string(&binding.bound) != canonical_json_string(&reencoded) {
            return Err(SnapshotDecodeError::RelationValue(format!(
                "binding {index} bound does not round-trip byte-equal (lossy member)"
            )));
        }
    }
    Ok(())
}

/// The canonical comparison form of the STORED relation-verdict value
/// (verdict tag + ordered bindings with their stored bound JSON), matched
/// against the wire-redecoded form by the v4 raw-capture rail. The stored
/// bounds are used verbatim (the strict value rail independently proves they
/// round-trip through the TypeExpr codec).
fn relation_value_canonical_form(oracle_value: &Value) -> Result<String, SnapshotDecodeError> {
    let dto: RelationVerdictValueDto = serde_json::from_value(oracle_value.clone())
        .map_err(|e| SnapshotDecodeError::RelationValue(format!("value DTO: {e}")))?;
    RelationVerdict::from_tag(&dto.verdict)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.verdict.clone()))?;
    let bindings: Vec<Value> = dto
        .bindings
        .iter()
        .map(|b| {
            json!({
                "ordinal": b.ordinal,
                "name": b.name,
                "bound": b.bound,
            })
        })
        .collect();
    Ok(canonical_json_string(&json!({
        "verdict": dto.verdict,
        "bindings": bindings,
    })))
}

// ---------------------------------------------------------------------------
// Snapshot ENCODE / assembly (the generation-side write path)
// ---------------------------------------------------------------------------
//
// The strict DECODE path above is the consumption authority. This ENCODE path is
// its inverse — the generator (the `oracle-gen` binary, design §4 generator-side
// table) ASSEMBLES the canonical snapshot document from the structured query
// identity + pinned env + the captured oracle value + the per-stage capture /
// env-corpus / source-admission sub-objects, deriving the `snapshot_id` from the
// identity + env so the written filename is registry-derivable. Producing a
// snapshot is the half a lifted row cannot run without; it is built here as a
// pure function so it is fully exercised by a round-trip guard (encode of a
// fixed identity == the hand-authored canonical fixture, AND the assembled
// document strictly decodes) with NO tsgo and NO on-disk snapshot.

/// The probe-locator audit axis stored on `identity` (the synthesized probe's
/// name + its byte offset in the probe-bearing source). It is NOT a
/// `snapshot_id` input (the probe form is versioned by `probe_synthesis_version`)
/// — it is stored so `probe_header_names_target` can audit the wrong-hover fence
/// offline.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbeLocator {
    pub(crate) probe_name: String,
    pub(crate) offset: u64,
}

/// Render the kind-specific `structured_type_expr` `identity` object exactly as
/// [`StructuredTypeExprIdentity`] decodes it. `workspace_files` is emitted
/// PATH-SORTED (the canonical order the schema mandates and `snapshot_id`
/// hashes), so the stored identity object is order-independent of upsert order.
#[allow(dead_code)]
pub(crate) fn render_identity_json(identity: &SnapshotIdentity, probe: &ProbeLocator) -> Value {
    let mut files: Vec<&WorkspaceFileRef> = identity.workspace_files.iter().collect();
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let workspace_files: Vec<Value> = files
        .iter()
        .map(|f| json!({ "path": f.path, "content_hash": f.content_hash }))
        .collect();

    json!({
        "query_helper_kind": identity.query_helper_kind.tag(),
        "workspace_files": workspace_files,
        "primary_canonical": identity.primary_canonical,
        "symbol_or_expression": identity.symbol_or_expression,
        "type_arguments": identity.type_arguments,
        "projection_mode": projection_mode_tag(identity.projection_mode),
        "probe_rhs_kind": identity.probe_rhs_kind.tag(),
        "host_project": {
            "project_root": identity.host_project.project_root,
            "workspace_root": identity.host_project.workspace_root,
            "tsconfig_path": identity.host_project.tsconfig_path,
            "host_setup_kind": identity.host_project.host_setup_kind.tag(),
        },
        "probe_locator": {
            "probe_name": probe.probe_name,
            "offset": probe.offset,
        },
    })
}

/// Assemble the full canonical snapshot document (§Q1 field table) from the
/// structured query identity + pinned env + the captured / computed sub-objects.
///
/// The `snapshot_id` is DERIVED here from `identity` + `env` (the same
/// registry-derivable derivation a coverage guard runs), never passed in — so an
/// assembled document is self-consistent by construction. The
/// `raw_capture` / `oracle_env_files` / `source_admission_digest` sub-objects are
/// produced by their own generation stages (the hover capture, the corpus
/// vendoring, the source-side walk) and handed in as already-canonical `Value`s;
/// this function owns ONLY the envelope assembly + the identity rendering +
/// the `snapshot_id` wiring.
#[allow(dead_code, clippy::too_many_arguments)]
pub(crate) fn assemble_snapshot_document(
    oracle_family: &str,
    identity: &SnapshotIdentity,
    env: &PinnedEnv,
    oracle_value: &Value,
    probe: &ProbeLocator,
    raw_capture: &Value,
    oracle_env_files: &Value,
    oracle_env_hash: &str,
    source_admission_digest: &Value,
    migration_fingerprint_version: u32,
    migration_fingerprint: &str,
) -> Value {
    let snapshot_id = derive_snapshot_id(identity, env);
    json!({
        "oracle_schema_version": env.oracle_schema_version,
        "normalizer_version": env.normalizer_version,
        "probe_synthesis_version": env.probe_synthesis_version,
        "tsgo_version": env.tsgo_version,
        "compiler_options_hash": env.compiler_options_hash,
        "env_corpus_id": env.env_corpus_id,
        "oracle_env_files": oracle_env_files,
        "oracle_env_hash": oracle_env_hash,
        "oracle_family": oracle_family,
        "oracle_value_kind": identity.oracle_value_kind.tag(),
        "snapshot_id": snapshot_id,
        "migration_fingerprint_version": migration_fingerprint_version,
        "migration_fingerprint": migration_fingerprint,
        "row_ref": {
            "row_file": identity.row_file,
            "row_function": identity.row_function,
            "query_ordinal": identity.query_ordinal,
        },
        "identity": render_identity_json(identity, probe),
        "oracle_value": oracle_value,
        "raw_capture": raw_capture,
        "source_admission_digest": source_admission_digest,
    })
}

/// Render the kind-specific `relation_verdict` `identity` object exactly as
/// [`RelationVerdictIdentityDto`] decodes it. `workspace_files` is emitted
/// PATH-SORTED (the canonical order the schema mandates and `snapshot_id`
/// hashes). The binder layout is emitted in binder preorder (the order IS an
/// identity input).
#[allow(dead_code)]
pub(crate) fn render_relation_identity_json(
    identity: &RelationVerdictIdentity,
    probe: &ProbeLocator,
) -> Value {
    let mut files: Vec<&WorkspaceFileRef> = identity.workspace_files.iter().collect();
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let workspace_files: Vec<Value> = files
        .iter()
        .map(|f| json!({ "path": f.path, "content_hash": f.content_hash }))
        .collect();
    let binder_layout: Vec<Value> = identity
        .binder_layout
        .iter()
        .map(|b| match &b.constraint {
            Some(constraint) => {
                json!({ "ordinal": b.ordinal, "name": b.name, "constraint": constraint })
            }
            None => json!({ "ordinal": b.ordinal, "name": b.name }),
        })
        .collect();

    json!({
        "workspace_files": workspace_files,
        "source_operand": identity.source_operand,
        "target_operand": identity.target_operand,
        "binder_layout": binder_layout,
        "relation": identity.relation.tag(),
        "policy": {
            "overload_selection": identity.policy.overload_selection.tag(),
            "excess_property_check": identity.policy.excess_property_check,
            "variance": identity.policy.variance.tag(),
        },
        "freshness": identity.freshness.tag(),
        "inference_mode": identity.inference_mode.tag(),
        "host_project": {
            "project_root": identity.host_project.project_root,
            "workspace_root": identity.host_project.workspace_root,
            "tsconfig_path": identity.host_project.tsconfig_path,
            "host_setup_kind": identity.host_project.host_setup_kind.tag(),
        },
        "probe_locator": {
            "probe_name": probe.probe_name,
            "offset": probe.offset,
        },
    })
}

/// Assemble the full canonical `relation_verdict` snapshot document (v4 field
/// table) from the structured relation identity + pinned env + the captured
/// relation value + the per-stage sub-objects. The v4 analog of
/// [`assemble_snapshot_document`]: the `snapshot_id` is DERIVED here (never
/// passed in); the migration-fidelity mirror AND the `source_admission_digest`
/// are ABSENT (a capture-only relation row is never a lift and has no
/// source-admission walk — no retained-lift provenance, no fabricated digest,
/// no sentinels).
#[allow(dead_code, clippy::too_many_arguments)]
pub(crate) fn assemble_relation_snapshot_document(
    oracle_family: &str,
    identity: &RelationVerdictIdentity,
    env: &PinnedEnv,
    oracle_value: &Value,
    probe: &ProbeLocator,
    raw_capture: &Value,
    oracle_env_files: &Value,
    oracle_env_hash: &str,
) -> Value {
    let snapshot_id = derive_relation_snapshot_id(identity, env);
    json!({
        "oracle_schema_version": env.oracle_schema_version,
        "normalizer_version": env.normalizer_version,
        "probe_synthesis_version": env.probe_synthesis_version,
        "tsgo_version": env.tsgo_version,
        "compiler_options_hash": env.compiler_options_hash,
        "env_corpus_id": env.env_corpus_id,
        "oracle_env_files": oracle_env_files,
        "oracle_env_hash": oracle_env_hash,
        "oracle_family": oracle_family,
        "oracle_value_kind": identity.oracle_value_kind.tag(),
        "snapshot_id": snapshot_id,
        "row_ref": {
            "row_file": identity.row_file,
            "row_function": identity.row_function,
            "query_ordinal": identity.query_ordinal,
        },
        "identity": render_relation_identity_json(identity, probe),
        "oracle_value": oracle_value,
        "raw_capture": raw_capture,
    })
}

/// Materialize a strictly-decoded `relation_verdict` snapshot's stored
/// `oracle_value` into the shared normalized boundary
/// ([`relation_probe::RelationVerdictValue`]): runs the kind-specific identity
/// decode + the strict value rail, then lowers each stored `bound` through the
/// real TypeExpr codec (the strict rail already proved the byte-equal
/// round-trip, so the final `expect` cannot fire). The comparison driver
/// consumes THIS form — never the raw JSON.
#[allow(dead_code)]
pub(crate) fn materialize_relation_value(
    snapshot: &OracleSnapshot,
) -> Result<relation_probe::RelationVerdictValue, SnapshotDecodeError> {
    let dto = match decode_identity(&snapshot.oracle_value_kind, &snapshot.identity)? {
        DecodedIdentity::RelationVerdict(dto) => dto,
        DecodedIdentity::StructuredTypeExpr(_) => {
            return Err(SnapshotDecodeError::UnknownValueKind(
                snapshot.oracle_value_kind.clone(),
            ));
        }
    };
    decode_relation_value_strict(&snapshot.oracle_value, &dto)?;
    let value: RelationVerdictValueDto = serde_json::from_value(snapshot.oracle_value.clone())
        .map_err(|e| SnapshotDecodeError::RelationValue(format!("value DTO: {e}")))?;
    let verdict = RelationVerdict::from_tag(&value.verdict)
        .ok_or_else(|| SnapshotDecodeError::BadTag(value.verdict.clone()))?;
    let mut bindings = Vec::with_capacity(value.bindings.len());
    for binding in value.bindings {
        let bound = type_expr_from_json(&binding.bound).ok_or_else(|| {
            SnapshotDecodeError::RelationValue(format!(
                "binding `{}` bound is not a TypeExpr document",
                binding.name
            ))
        })?;
        bindings.push(relation_probe::RelationBinding {
            ordinal: binding.ordinal,
            name: binding.name,
            bound,
            // The wire's bound slice is capture-time evidence — the persisted
            // record carries only the normalized bound.
            bound_text: None,
        });
    }
    Ok(relation_probe::RelationVerdictValue { verdict, bindings })
}

// ---------------------------------------------------------------------------
// snapshot_id redrive (snapshot-backed §Q4 derivation)
// ---------------------------------------------------------------------------

/// Re-derive a snapshot's `snapshot_id` from its STORED `identity` + `row_ref` +
/// stored pinned env, and assert it equals the stored `snapshot_id`. The redrive
/// uses REGISTRY-ONLY inputs (the stable `env_corpus_id`, NOT the per-snapshot
/// `oracle_env_hash`), so a guard can compute the expected filename from the
/// snapshot's own identity alone. A mismatch means the snapshot's stored id does
/// not match its declared value-affecting axes. Dispatches on the value kind —
/// the v4 `relation_verdict` identity hashes under its own domain tag.
#[allow(dead_code)]
pub(crate) fn redrive_snapshot_id(
    snapshot: &OracleSnapshot,
) -> Result<String, SnapshotDecodeError> {
    let dto = decode_identity(&snapshot.oracle_value_kind, &snapshot.identity)?;

    let env = PinnedEnv {
        tsgo_version: snapshot.tsgo_version.clone(),
        oracle_schema_version: snapshot.oracle_schema_version,
        normalizer_version: snapshot.normalizer_version,
        probe_synthesis_version: snapshot.probe_synthesis_version,
        compiler_options_hash: snapshot.compiler_options_hash.clone(),
        env_corpus_id: snapshot.env_corpus_id.clone(),
    };

    match dto {
        DecodedIdentity::StructuredTypeExpr(dto) => {
            let helper = QueryHelperKind::from_tag(&dto.query_helper_kind)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.query_helper_kind.clone()))?;
            let mode = projection_mode_from_tag(&dto.projection_mode)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.projection_mode.clone()))?;
            let probe_rhs_kind = ProbeRhsKind::from_tag(&dto.probe_rhs_kind)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.probe_rhs_kind.clone()))?;
            let host_kind =
                HostSetupKind::from_tag(&dto.host_project.host_setup_kind).ok_or_else(|| {
                    SnapshotDecodeError::BadTag(dto.host_project.host_setup_kind.clone())
                })?;

            let identity = SnapshotIdentity {
                row_file: snapshot.row_ref.row_file.clone(),
                row_function: snapshot.row_ref.row_function.clone(),
                query_ordinal: snapshot.row_ref.query_ordinal,
                query_helper_kind: helper,
                workspace_files: dto
                    .workspace_files
                    .iter()
                    .map(|f| WorkspaceFileRef {
                        path: f.path.clone(),
                        content_hash: f.content_hash.clone(),
                    })
                    .collect(),
                primary_canonical: dto.primary_canonical.clone(),
                symbol_or_expression: dto.symbol_or_expression.clone(),
                type_arguments: dto.type_arguments.clone(),
                projection_mode: mode,
                probe_rhs_kind,
                host_project: HostProject {
                    project_root: dto.host_project.project_root.clone(),
                    workspace_root: dto.host_project.workspace_root.clone(),
                    tsconfig_path: dto.host_project.tsconfig_path.clone(),
                    host_setup_kind: host_kind,
                },
                oracle_value_kind: OracleValueKind::StructuredTypeExpr,
            };

            Ok(super::identity::derive_snapshot_id(&identity, &env))
        }
        DecodedIdentity::RelationVerdict(dto) => {
            let relation = RelationKindTag::from_tag(&dto.relation)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.relation.clone()))?;
            let overload_selection = OverloadSelectionTag::from_tag(&dto.policy.overload_selection)
                .ok_or_else(|| {
                    SnapshotDecodeError::BadTag(dto.policy.overload_selection.clone())
                })?;
            let variance = VarianceTag::from_tag(&dto.policy.variance)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.policy.variance.clone()))?;
            let freshness = FreshnessTag::from_tag(&dto.freshness)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.freshness.clone()))?;
            let inference_mode = InferenceModeTag::from_tag(&dto.inference_mode)
                .ok_or_else(|| SnapshotDecodeError::BadTag(dto.inference_mode.clone()))?;
            let host_kind =
                HostSetupKind::from_tag(&dto.host_project.host_setup_kind).ok_or_else(|| {
                    SnapshotDecodeError::BadTag(dto.host_project.host_setup_kind.clone())
                })?;

            let identity = RelationVerdictIdentity {
                row_file: snapshot.row_ref.row_file.clone(),
                row_function: snapshot.row_ref.row_function.clone(),
                query_ordinal: snapshot.row_ref.query_ordinal,
                workspace_files: dto
                    .workspace_files
                    .iter()
                    .map(|f| WorkspaceFileRef {
                        path: f.path.clone(),
                        content_hash: f.content_hash.clone(),
                    })
                    .collect(),
                source_operand: dto.source_operand.clone(),
                target_operand: dto.target_operand.clone(),
                binder_layout: dto
                    .binder_layout
                    .iter()
                    .map(|b| BinderLayoutEntry {
                        ordinal: b.ordinal,
                        name: b.name.clone(),
                        constraint: b.constraint.clone(),
                    })
                    .collect(),
                relation,
                policy: RelationPolicyRecord {
                    overload_selection,
                    excess_property_check: dto.policy.excess_property_check,
                    variance,
                },
                freshness,
                inference_mode,
                host_project: HostProject {
                    project_root: dto.host_project.project_root.clone(),
                    workspace_root: dto.host_project.workspace_root.clone(),
                    tsconfig_path: dto.host_project.tsconfig_path.clone(),
                    host_setup_kind: host_kind,
                },
                oracle_value_kind: OracleValueKind::RelationVerdict,
            };

            Ok(super::identity::derive_relation_snapshot_id(
                &identity, &env,
            ))
        }
    }
}

#[cfg(test)]
mod tests;
