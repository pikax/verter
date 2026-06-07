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
//! Lifts ZERO rows: this is the storage-schema foundation the per-block
//! row-lifts ride on.

use serde::Deserialize;
use serde_json::Value;
use verter_type_expr::type_expr_from_json;

use super::identity::{
    projection_mode_from_tag, HostProject, HostSetupKind, OracleValueKind, PinnedEnv,
    QueryHelperKind, SnapshotIdentity, WorkspaceFileRef,
};
use super::normalize::canonical_json_string;

/// The closed set of `oracle_value_kind` discriminants this schema version
/// understands. Adding a kind is a CLOSED-tagged schema change that MUST bump
/// [`super::identity::ORACLE_SCHEMA_VERSION`] (a new kind carries a different
/// required `identity` shape). The `identity_is_kind_specific_schema_bumped`
/// guard ties this set's size to the schema version so the two cannot drift.
#[allow(dead_code)]
pub(crate) const KNOWN_VALUE_KINDS: &[&str] = &["structured_type_expr"];

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
    /// `host_setup_kind`) was not a recognized discriminant.
    BadTag(String),
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
    pub(crate) row_ref: RowRef,
    pub(crate) identity: Value,
    pub(crate) oracle_value: Value,
    pub(crate) raw_capture: RawCapture,
    pub(crate) source_admission_digest: SourceAdmissionDigest,
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
// Strict decode
// ---------------------------------------------------------------------------

/// Strictly decode a snapshot JSON document (§Q1):
/// 1. envelope decode under `deny_unknown_fields`;
/// 2. `oracle_value_kind` ∈ the closed [`KNOWN_VALUE_KINDS`];
/// 3. `identity` strictly decodes to the kind-specific shape;
/// 4. `oracle_value` decodes to a `TypeExpr` AND round-trips byte-equal under
///    re-encode (no silent `filter_map` member loss).
#[allow(dead_code)]
pub(crate) fn decode_strict(json: &Value) -> Result<OracleSnapshot, SnapshotDecodeError> {
    let snapshot: OracleSnapshot = serde_json::from_value(json.clone())
        .map_err(|e| SnapshotDecodeError::Envelope(e.to_string()))?;

    if !KNOWN_VALUE_KINDS.contains(&snapshot.oracle_value_kind.as_str()) {
        return Err(SnapshotDecodeError::UnknownValueKind(
            snapshot.oracle_value_kind.clone(),
        ));
    }

    // Kind-specific identity strict decode + tag validation.
    let _identity = decode_identity(&snapshot.oracle_value_kind, &snapshot.identity)?;

    // oracle_value strict TypeExpr decode + lossless round-trip.
    decode_oracle_value_strict(&snapshot.oracle_value)?;

    Ok(snapshot)
}

/// Strictly decode the kind-specific `identity` shape. Only
/// `structured_type_expr` is understood; the embedded enum tags
/// (`query_helper_kind` / `projection_mode` / `host_setup_kind`) must parse to a
/// known discriminant.
#[allow(dead_code)]
pub(crate) fn decode_identity(
    oracle_value_kind: &str,
    identity: &Value,
) -> Result<StructuredTypeExprIdentity, SnapshotDecodeError> {
    if oracle_value_kind != OracleValueKind::StructuredTypeExpr.tag() {
        return Err(SnapshotDecodeError::UnknownValueKind(
            oracle_value_kind.to_string(),
        ));
    }
    let dto: StructuredTypeExprIdentity = serde_json::from_value(identity.clone())
        .map_err(|e| SnapshotDecodeError::Identity(e.to_string()))?;

    // Validate the embedded closed-enum tags so a stored garbage tag is caught
    // at decode, not at redrive.
    QueryHelperKind::from_tag(&dto.query_helper_kind)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.query_helper_kind.clone()))?;
    projection_mode_from_tag(&dto.projection_mode)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.projection_mode.clone()))?;
    HostSetupKind::from_tag(&dto.host_project.host_setup_kind)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.host_project.host_setup_kind.clone()))?;

    Ok(dto)
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

// ---------------------------------------------------------------------------
// snapshot_id redrive (snapshot-backed §Q4 derivation)
// ---------------------------------------------------------------------------

/// Re-derive a snapshot's `snapshot_id` from its STORED `identity` + `row_ref` +
/// stored pinned env, and assert it equals the stored `snapshot_id`. The redrive
/// uses REGISTRY-ONLY inputs (the stable `env_corpus_id`, NOT the per-snapshot
/// `oracle_env_hash`), so a guard can compute the expected filename from the
/// snapshot's own identity alone. A mismatch means the snapshot's stored id does
/// not match its declared value-affecting axes.
#[allow(dead_code)]
pub(crate) fn redrive_snapshot_id(
    snapshot: &OracleSnapshot,
) -> Result<String, SnapshotDecodeError> {
    let dto = decode_identity(&snapshot.oracle_value_kind, &snapshot.identity)?;

    let helper = QueryHelperKind::from_tag(&dto.query_helper_kind)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.query_helper_kind.clone()))?;
    let mode = projection_mode_from_tag(&dto.projection_mode)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.projection_mode.clone()))?;
    let host_kind = HostSetupKind::from_tag(&dto.host_project.host_setup_kind)
        .ok_or_else(|| SnapshotDecodeError::BadTag(dto.host_project.host_setup_kind.clone()))?;
    let value_kind = OracleValueKind::from_tag(&snapshot.oracle_value_kind)
        .ok_or_else(|| SnapshotDecodeError::UnknownValueKind(snapshot.oracle_value_kind.clone()))?;

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
        host_project: HostProject {
            project_root: dto.host_project.project_root.clone(),
            workspace_root: dto.host_project.workspace_root.clone(),
            tsconfig_path: dto.host_project.tsconfig_path.clone(),
            host_setup_kind: host_kind,
        },
        oracle_value_kind: value_kind,
    };

    let env = PinnedEnv {
        tsgo_version: snapshot.tsgo_version.clone(),
        oracle_schema_version: snapshot.oracle_schema_version,
        normalizer_version: snapshot.normalizer_version,
        probe_synthesis_version: snapshot.probe_synthesis_version,
        compiler_options_hash: snapshot.compiler_options_hash.clone(),
        env_corpus_id: snapshot.env_corpus_id.clone(),
    };

    Ok(super::identity::derive_snapshot_id(&identity, &env))
}

#[cfg(test)]
mod tests;
