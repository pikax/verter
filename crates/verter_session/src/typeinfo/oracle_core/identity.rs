//! Snapshot identity model + `snapshot_id` derivation + canonical content
//! hashing for the TS7 oracle harness (`docs/arch/u0-oracle-harness-design.md`
//! §Q1 / §Q4).
//!
//! `snapshot_id` is the deterministic, REGISTRY-DERIVABLE filename stem of an
//! oracle snapshot. It is derived from REGISTRY-ONLY, tsgo-free inputs — the
//! row-ref + the value-affecting query identity + the pinned env/algorithm
//! versions including the STABLE `env_corpus_id` — so a coverage guard can
//! compute the expected filename set from the registry ALONE, without opening a
//! snapshot or running tsgo. The per-snapshot `oracle_env_hash` (the full
//! resolved-file-set hash) is DELIBERATELY NOT an input (folding it in would be
//! circular) — it is validated as a VALUE on read.
//!
//! Two hash FAMILIES by role, each self-described by its prefix on disk:
//! BLAKE3 (`blake3:`) for the harness's own identity/content ids
//! (`snapshot_id`, `env_corpus_id`, `oracle_env_hash`); SHA-256 (`sha256:`) for
//! one-shot content digests (a file's normalized bytes, the effective
//! compiler-options blob). The id is the FULL ≥256-bit BLAKE3 digest, never a
//! truncation.
//!
//! Lifts ZERO rows: this is the storage/identity foundation the
//! `TypeExpr`-projection oracle rows build on.

use serde_json::{json, Value};

use super::normalize::{canonical_json_string, ProjectionModeKind};

// ---------------------------------------------------------------------------
// Pinned-env constants (the registry-known, tsgo-free `snapshot_id` inputs)
// ---------------------------------------------------------------------------

/// The pinned tsgo version that produces every oracle snapshot value.
#[allow(dead_code)]
pub(crate) const TSGO_VERSION: &str = "7.0.0-dev.20260526.1";

/// Version of THIS snapshot FILE SHAPE (field set + per-kind `identity` shape).
/// Bumped on any schema-field change AND whenever a new `oracle_value_kind` is
/// added (a new kind carries a different required `identity` shape). v2 added
/// `identity.probe_rhs_kind` + `raw_capture.probe_scaffold` (the capture-
/// strategy axis); v3 added the REQUIRED top-level `migration_fingerprint_version` +
/// `migration_fingerprint` migration-fidelity mirror (§Q4); v4 added the CLOSED
/// `relation_verdict` value kind (the relation-tuple-wire capture family — the
/// migration-fidelity mirror becomes kind-keyed: required for
/// `structured_type_expr`, forbidden as a cross-kind field on
/// `relation_verdict`). Because it flows into `snapshot_id` through
/// `PinnedEnv`, the bump changes every `snapshot_id` (hence every checked-in
/// snapshot filename).
#[allow(dead_code)]
pub(crate) const ORACLE_SCHEMA_VERSION: u32 = 4;

/// Version of the PROBE-SYNTHESIS + hover-driver + hover-extraction +
/// admissibility algorithm. Distinct from `normalizer_version`. Enters
/// `snapshot_id`. v2 added the distributive-identity probe-RHS kind (the
/// keyof-expansion scaffold).
#[allow(dead_code)]
pub(crate) const PROBE_SYNTHESIS_VERSION: u32 = 2;

// NOTE — `compiler_options_hash` and `CURRENT_ENV_CORPUS_ID` are NOT pinned here.
// They are GENERATION-derived: `compiler_options_hash` hashes the EFFECTIVE
// committed `oracle.tsconfig.json`, and `env_corpus_id` is the content id of the
// vendored corpus at `oracle_env/<env_corpus_dir_name(env_corpus_id)>/`. Both
// are computed and committed by the snapshot generator whenever a corpus is
// (re-)vendored, and live in the query-spec registry. The derivation below is
// a pure function over an explicit `PinnedEnv` so it is fully exercised by
// the guards with synthetic env values.

/// The pinned env + algorithm versions that enter every `snapshot_id`.
///
/// `compiler_options_hash` and `env_corpus_id` are `sha256:` / `blake3:`
/// family-prefixed strings the snapshot generator fills from the vendored
/// corpus.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PinnedEnv {
    pub(crate) tsgo_version: String,
    pub(crate) oracle_schema_version: u32,
    pub(crate) normalizer_version: u32,
    pub(crate) probe_synthesis_version: u32,
    pub(crate) compiler_options_hash: String,
    pub(crate) env_corpus_id: String,
}

// ---------------------------------------------------------------------------
// On-disk corpus path boundary (shared by the consumption driver + generator)
// ---------------------------------------------------------------------------

/// The corpus-root infix from `CARGO_MANIFEST_DIR` (= `crates/verter_session/`)
/// to the vendored env corpus (`oracle_env/<dir>/`). Single owner — the
/// consumption driver and the `oracle-gen` generator both join through this
/// constant (this module is visible to both cfg contexts; `driver` is
/// `#[cfg(test)]`, `gen` is `#[cfg(feature = "oracle-gen")]`).
#[allow(dead_code)]
pub(crate) const ORACLE_ENV_INFIX: &str = "src/typeinfo/typeinfo_tests/oracle_env";

/// Map a LOGICAL tagged-digest env-corpus id (`blake3:<hex>` / `sha256:<hex>`)
/// to its NTFS-safe ON-DISK directory name by replacing `:` with `-`
/// (`blake3:c6c4… → blake3-c6c4…`). `:` is illegal in an NTFS path component,
/// so the raw id cannot be a tracked directory name on Windows; the logical
/// id encoding itself is pinned and unchanged — it stays `blake3:<hex>` in
/// snapshot JSON, `env_corpus_id` pins, and `snapshot_id` derivation. The
/// mapping is injective (the algo tag is `[a-z0-9]+` and hex digits contain
/// no `-`, so the FIRST `-` in a dir name is always the mapped separator);
/// the reverse mapping — replace the first `-` with `:` — is documented here
/// but deliberately unimplemented, since nothing reads ids back off disk.
#[allow(dead_code)]
pub(crate) fn env_corpus_dir_name(id: &str) -> String {
    id.replace(':', "-")
}

// ---------------------------------------------------------------------------
// Registry-derivable query identity (the value-affecting `snapshot_id` axes)
// ---------------------------------------------------------------------------

/// Which `support.rs` helper produces the in-process `TypeExpr`. Only the
/// discriminant enters `snapshot_id` — the helper's payload (symbol /
/// type-args / mode) enters through the dedicated identity axes below.
// The variant names are the design-mandated `support.rs` helper names
// (ResolveExpr / ShallowSurfaceExpr / EvaluateExpr); the shared `*Expr` suffix
// is intentional, not a naming smell.
#[allow(dead_code, clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryHelperKind {
    ResolveExpr,
    ShallowSurfaceExpr,
    EvaluateExpr,
}

impl QueryHelperKind {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::ResolveExpr => "ResolveExpr",
            Self::ShallowSurfaceExpr => "ShallowSurfaceExpr",
            Self::EvaluateExpr => "EvaluateExpr",
        }
    }

    /// Inverse of [`tag`]: parse a stored snapshot `query_helper_kind` string back
    /// to the closed enum (the strict snapshot decoder uses this to redrive
    /// `snapshot_id` from a snapshot's stored identity). An unknown tag is `None`.
    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "ResolveExpr" => Some(Self::ResolveExpr),
            "ShallowSurfaceExpr" => Some(Self::ShallowSurfaceExpr),
            "EvaluateExpr" => Some(Self::EvaluateExpr),
            _ => None,
        }
    }
}

/// The probe-RHS capture strategy: HOW the generator synthesized the probe RHS
/// the snapshot's hover was captured over. A VALUE-AFFECTING `snapshot_id` axis
/// — the harness never leans on the distributive-identity theorem to claim two
/// capture paths are the same cache key.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeRhsKind {
    /// The bare-symbol RHS (`type __oracle_probe__N = Symbol;`).
    Bare,
    /// The distributive-identity scaffold: an inlined per-query helper
    /// `type __oracle_probe_dist__N<T> = T extends never ? never : T;` plus
    /// the wrapped RHS `__oracle_probe_dist__N<Symbol>` — forces tsgo to print
    /// the expanded member union of its own keyof enumeration instead of
    /// echoing the written `keyof <operand>` display origin.
    DistributiveIdentity,
}

impl ProbeRhsKind {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Bare => "bare",
            Self::DistributiveIdentity => "distributive_identity",
        }
    }

    /// Inverse of [`tag`] for the strict snapshot decoder's redrive path. An
    /// unknown capture-strategy tag is `None` (closed set).
    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "bare" => Some(Self::Bare),
            "distributive_identity" => Some(Self::DistributiveIdentity),
            _ => None,
        }
    }
}

/// The host/project setup axis. `standalone` is the only currently-admissible
/// first-class kind; the others are carried for schema totality (their rows
/// stay deferred to the named env-pin spike).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostSetupKind {
    Standalone,
    WorkspaceFootprint,
    PackageBacked,
}

impl HostSetupKind {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::WorkspaceFootprint => "workspace_footprint",
            Self::PackageBacked => "package_backed",
        }
    }

    /// Inverse of [`tag`] for the strict snapshot decoder's redrive path.
    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "standalone" => Some(Self::Standalone),
            "workspace_footprint" => Some(Self::WorkspaceFootprint),
            "package_backed" => Some(Self::PackageBacked),
            _ => None,
        }
    }
}

/// The host/project setup axes that enter `identity` + `snapshot_id`.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostProject {
    pub(crate) project_root: String,
    pub(crate) workspace_root: String,
    pub(crate) tsconfig_path: String,
    pub(crate) host_setup_kind: HostSetupKind,
}

impl HostProject {
    fn to_canonical_json(&self) -> Value {
        json!({
            "project_root": self.project_root,
            "workspace_root": self.workspace_root,
            "tsconfig_path": self.tsconfig_path,
            "host_setup_kind": self.host_setup_kind.tag(),
        })
    }
}

/// The closed `oracle_value_kind` taxonomy. `StructuredTypeExpr` (v1–v3) is the
/// TypeExpr-projection family; `RelationVerdict` (v4) is the relation-tuple-wire
/// capture family. A future kind is an additive closed-tagged discriminant that
/// bumps `oracle_schema_version`.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OracleValueKind {
    StructuredTypeExpr,
    RelationVerdict,
}

impl OracleValueKind {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::StructuredTypeExpr => "structured_type_expr",
            Self::RelationVerdict => "relation_verdict",
        }
    }

    /// Inverse of [`tag`] for the strict snapshot decoder. An unknown
    /// `oracle_value_kind` string is `None` — a future kind is a CLOSED-tagged
    /// addition that bumps `ORACLE_SCHEMA_VERSION`, never a silently-accepted
    /// open string.
    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "structured_type_expr" => Some(Self::StructuredTypeExpr),
            "relation_verdict" => Some(Self::RelationVerdict),
            _ => None,
        }
    }
}

/// One workspace file's identity: the canonical leading-slash path + the
/// `sha256:`-prefixed content hash (the snapshot stores PATH + HASH only, never
/// the source bytes — the registry is the source-byte authority).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceFileRef {
    pub(crate) path: String,
    pub(crate) content_hash: String,
}

/// The full value-affecting query identity hashed into `snapshot_id`. Every
/// field is registry-derivable + tsgo-free. `oracle_family` is DELIBERATELY
/// ABSENT (a directory/presentation key only, excluded from the id).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotIdentity {
    pub(crate) row_file: String,
    pub(crate) row_function: String,
    pub(crate) query_ordinal: u16,
    pub(crate) query_helper_kind: QueryHelperKind,
    /// Canonicalized: sorted by path, duplicate paths forbidden (the SCHEMA
    /// rejects a path appearing twice — there is exactly one final content per
    /// path). Upsert order is NOT an input.
    pub(crate) workspace_files: Vec<WorkspaceFileRef>,
    pub(crate) primary_canonical: String,
    pub(crate) symbol_or_expression: String,
    /// Canonical `TypeExpr` JSON of each type argument (`ResolveExpr` only;
    /// distinguishes `Box<string>` from `Box<number>`).
    pub(crate) type_arguments: Vec<Value>,
    pub(crate) projection_mode: ProjectionModeKind,
    /// The capture strategy the probe RHS was synthesized under (§Q2 keyof-
    /// expansion scaffold) — a value-affecting axis since `snapshot_id` v2.
    pub(crate) probe_rhs_kind: ProbeRhsKind,
    pub(crate) host_project: HostProject,
    pub(crate) oracle_value_kind: OracleValueKind,
}

pub(crate) fn projection_mode_tag(mode: ProjectionModeKind) -> &'static str {
    match mode {
        ProjectionModeKind::Shallow => "Shallow",
        ProjectionModeKind::Navigate => "Navigate",
        ProjectionModeKind::Expanded => "Expanded",
        ProjectionModeKind::Skeleton => "Skeleton",
    }
}

/// Inverse of [`projection_mode_tag`] for the strict snapshot decoder's redrive
/// path. An unknown mode string is `None`.
#[allow(dead_code)]
pub(crate) fn projection_mode_from_tag(tag: &str) -> Option<ProjectionModeKind> {
    match tag {
        "Shallow" => Some(ProjectionModeKind::Shallow),
        "Navigate" => Some(ProjectionModeKind::Navigate),
        "Expanded" => Some(ProjectionModeKind::Expanded),
        "Skeleton" => Some(ProjectionModeKind::Skeleton),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The v4 `relation_verdict` identity (a DISTINCT closed shape)
// ---------------------------------------------------------------------------

/// Which relation the tuple-wire capture asked (v4 identity axis). Mirrors the
/// engine's `RelationKind` taxonomy as a CLOSED tag set for decode totality;
/// only `Assignable` is an admissible capture this block (the registry emits
/// nothing else, and the consumption adapter rejects broader keys).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationKindTag {
    Assignable,
    Subtype,
    StrictSubtype,
    Identity,
    Comparable,
}

impl RelationKindTag {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Assignable => "assignable",
            Self::Subtype => "subtype",
            Self::StrictSubtype => "strict_subtype",
            Self::Identity => "identity",
            Self::Comparable => "comparable",
        }
    }

    /// Inverse of [`tag`] for the strict snapshot decoder. An unknown tag is
    /// `None` (closed set).
    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "assignable" => Some(Self::Assignable),
            "subtype" => Some(Self::Subtype),
            "strict_subtype" => Some(Self::StrictSubtype),
            "identity" => Some(Self::Identity),
            "comparable" => Some(Self::Comparable),
            _ => None,
        }
    }
}

/// Overload-selection policy axis (v4 identity). Closed set; only `All` (relate
/// against the WHOLE overload set) is admissible this block.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverloadSelectionTag {
    All,
    FirstApplicable,
}

impl OverloadSelectionTag {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::FirstApplicable => "first_applicable",
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "all" => Some(Self::All),
            "first_applicable" => Some(Self::FirstApplicable),
            _ => None,
        }
    }
}

/// Variance-regime policy axis (v4 identity). Closed set; only
/// `MethodParameterBivariance` is admissible this block (the live oracle
/// tsconfig's `strict` regime relates function-typed parameters contravariantly
/// and method-shaped parameters bivariantly — exactly this record).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VarianceTag {
    MethodParameterBivariance,
    StrictContravariance,
}

impl VarianceTag {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::MethodParameterBivariance => "method_parameter_bivariance",
            Self::StrictContravariance => "strict_contravariance",
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "method_parameter_bivariance" => Some(Self::MethodParameterBivariance),
            "strict_contravariance" => Some(Self::StrictContravariance),
            _ => None,
        }
    }
}

/// The relation-policy record (v4 identity axis): the three policy axes a
/// relation judgement depends on. The registry emits only the DEFAULT record
/// this block (`all` / `false` / `method_parameter_bivariance`).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RelationPolicyRecord {
    pub(crate) overload_selection: OverloadSelectionTag,
    pub(crate) excess_property_check: bool,
    pub(crate) variance: VarianceTag,
}

impl RelationPolicyRecord {
    /// The default record — the only admissible capture policy this block.
    #[allow(dead_code)]
    pub(crate) fn default_record() -> Self {
        Self {
            overload_selection: OverloadSelectionTag::All,
            excess_property_check: false,
            variance: VarianceTag::MethodParameterBivariance,
        }
    }

    fn to_canonical_json(&self) -> Value {
        json!({
            "overload_selection": self.overload_selection.tag(),
            "excess_property_check": self.excess_property_check,
            "variance": self.variance.tag(),
        })
    }
}

/// Whether the relation source carries freshness (v4 identity axis). Closed
/// set; only `Regular` (a widened, non-fresh source — type-position operands
/// are never fresh literals) is admissible this block.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshnessTag {
    Fresh,
    Regular,
}

impl FreshnessTag {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Regular => "regular",
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "fresh" => Some(Self::Fresh),
            "regular" => Some(Self::Regular),
            _ => None,
        }
    }
}

/// The normalized inference mode the capture ran under (v4 identity axis):
/// `None` for binder-free relations, `TargetPattern` when the target pattern
/// carries `infer` binders captured through the tuple wire. Validated for
/// consistency with `binder_layout` (empty layout ⇔ `None`).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InferenceModeTag {
    None,
    TargetPattern,
}

impl InferenceModeTag {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::TargetPattern => "target_pattern",
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "none" => Some(Self::None),
            "target_pattern" => Some(Self::TargetPattern),
            _ => None,
        }
    }
}

/// One binder of the target-pattern binder layout (v4 identity axis): the
/// binder's target-pattern PREORDER ordinal (NOT a name sort) and its declared
/// name. Ordinals must be exactly `0..n-1` in layout order; names must be
/// unique and set-match the reserved binder refs in `target_operand`.
/// `constraint` is the canonical normalized AST JSON of the binder's
/// `infer X extends <constraint>` constraint when the target pattern declares
/// one (`None` for a bare `infer X`) — an `infer V extends string` and a bare
/// `infer V` MUST derive DISTINCT identities, so the constraint is an
/// identity-carrying part of the layout entry, never erased by the binder-ref
/// substitution.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BinderLayoutEntry {
    pub(crate) ordinal: u16,
    pub(crate) name: String,
    pub(crate) constraint: Option<Value>,
}

/// The full value-affecting query identity hashed into a `relation_verdict`
/// `snapshot_id` (v4). A DISTINCT closed shape from [`SnapshotIdentity`] — no
/// relation fields are bolted onto the v3 TypeExpr identity, and no graph-local
/// node IDs appear anywhere: the operands are canonical AST JSON lowered from
/// the registry's TS texts (`infer X` positions encoded as reserved binder refs
/// `__oracle_binder__X` inside `target_operand`, a closed encoding used ONLY
/// inside this axis). Every field is registry-derivable + tsgo-free.
/// `oracle_family` is DELIBERATELY ABSENT (a directory/presentation key only).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelationVerdictIdentity {
    pub(crate) row_file: String,
    pub(crate) row_function: String,
    pub(crate) query_ordinal: u16,
    /// Canonicalized: sorted by path, duplicate paths forbidden (same rule as
    /// the v3 identity — exactly one final content per path).
    pub(crate) workspace_files: Vec<WorkspaceFileRef>,
    /// Canonical normalized AST JSON of the SOURCE operand.
    pub(crate) source_operand: Value,
    /// Canonical normalized AST JSON of the TARGET operand, with each `infer X`
    /// position encoded as a reserved `__oracle_binder__X` ref.
    pub(crate) target_operand: Value,
    /// The target-pattern binder layout in binder preorder.
    pub(crate) binder_layout: Vec<BinderLayoutEntry>,
    pub(crate) relation: RelationKindTag,
    pub(crate) policy: RelationPolicyRecord,
    pub(crate) freshness: FreshnessTag,
    pub(crate) inference_mode: InferenceModeTag,
    pub(crate) host_project: HostProject,
    pub(crate) oracle_value_kind: OracleValueKind,
}

/// Domain-separation tag for the v4 `relation_verdict` `snapshot_id` hash
/// input. The v4 identity is a DISTINCT field set from the v3 TypeExpr
/// identity, so it hashes under its own tag — the v3 family's
/// [`SNAPSHOT_ID_DOMAIN_TAG`] field set is UNCHANGED (v3 snapshots re-key under
/// v4 ONLY through the `PinnedEnv.oracle_schema_version` bump).
pub(crate) const RELATION_SNAPSHOT_ID_DOMAIN_TAG: &[u8] = b"verter.oracle.snapshot_id.relation.v1";

/// Derive the deterministic `snapshot_id` (the filename stem) for a
/// `relation_verdict` snapshot from the registry-derivable v4 identity + the
/// pinned env. Same length-prefixed, domain-separated BLAKE3 recipe as the v3
/// derivation; the id is `"u_"` + the FULL 32-byte digest hex (never
/// truncated).
#[allow(dead_code)]
pub(crate) fn derive_relation_snapshot_id(
    identity: &RelationVerdictIdentity,
    env: &PinnedEnv,
) -> String {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(RELATION_SNAPSHOT_ID_DOMAIN_TAG);

    // The row-ref — one file per (row, query).
    lp_str(&mut buf, &identity.row_file);
    lp_str(&mut buf, &identity.row_function);
    lp_u16(&mut buf, identity.query_ordinal);
    // The relation identity.
    lp_str(
        &mut buf,
        &workspace_file_set_json(&identity.workspace_files),
    );
    lp_str(&mut buf, &canonical_json_string(&identity.source_operand));
    lp_str(&mut buf, &canonical_json_string(&identity.target_operand));
    lp_str(&mut buf, &binder_layout_json(&identity.binder_layout));
    lp_str(&mut buf, identity.relation.tag());
    lp_str(
        &mut buf,
        &canonical_json_string(&identity.policy.to_canonical_json()),
    );
    lp_str(&mut buf, identity.freshness.tag());
    lp_str(&mut buf, identity.inference_mode.tag());
    lp_str(
        &mut buf,
        &canonical_json_string(&identity.host_project.to_canonical_json()),
    );
    lp_str(&mut buf, identity.oracle_value_kind.tag());
    // The pinned env / algorithm versions (incl. the STABLE env_corpus_id).
    lp_u32(&mut buf, env.normalizer_version);
    lp_u32(&mut buf, env.probe_synthesis_version);
    lp_str(&mut buf, &env.compiler_options_hash);
    lp_str(&mut buf, &env.env_corpus_id);
    lp_str(&mut buf, &env.tsgo_version);
    lp_u32(&mut buf, env.oracle_schema_version);

    let digest = blake3::hash(&buf);
    format!("u_{}", digest.to_hex())
}

/// Canonical JSON of the binder layout: the ordered array in target-pattern
/// binder preorder (the order IS an identity input). Each entry carries
/// `{ name, ordinal }` plus `"constraint": <canonical AST JSON>` when the
/// binder declares one — OMITTED when unconstrained, so a pre-constraint
/// snapshot's stored layout stays byte-identical (its id redrives unchanged).
fn binder_layout_json(layout: &[BinderLayoutEntry]) -> String {
    let arr = Value::Array(
        layout
            .iter()
            .map(|b| match &b.constraint {
                Some(constraint) => {
                    json!({ "ordinal": b.ordinal, "name": b.name, "constraint": constraint })
                }
                None => json!({ "ordinal": b.ordinal, "name": b.name }),
            })
            .collect(),
    );
    canonical_json_string(&arr)
}

/// The canonical host/config identity block of a relation verdict identity —
/// re-rendered for the strict decoder's redrive path.
#[allow(dead_code)]
pub(crate) fn relation_policy_canonical_json(policy: &RelationPolicyRecord) -> String {
    canonical_json_string(&policy.to_canonical_json())
}

// ---------------------------------------------------------------------------
// snapshot_id derivation (§Q4): length-prefixed, domain-separated BLAKE3
// ---------------------------------------------------------------------------

/// Domain-separation tag for the `snapshot_id` hash input. A change to the field
/// set or ordering is a schema change and bumps this tag (v2 added the
/// `probe_rhs_kind` field).
pub(crate) const SNAPSHOT_ID_DOMAIN_TAG: &[u8] = b"verter.oracle.snapshot_id.v2";

/// Derive the deterministic `snapshot_id` (the filename stem) from the
/// registry-derivable `identity` + the pinned env. The id is `"u_"` + the FULL
/// 32-byte BLAKE3 digest hex-encoded (never truncated).
///
/// The hash input is a canonical, domain-separated, LENGTH-PREFIXED byte stream:
/// each field is `u32-LE byte-length || field-bytes`, concatenated in the FIXED
/// order below under the leading `SNAPSHOT_ID_DOMAIN_TAG`. Length-prefixing
/// makes any two distinct field tuples produce distinct streams (loose
/// concatenation is ambiguous when a field can contain the separator).
#[allow(dead_code)]
pub(crate) fn derive_snapshot_id(identity: &SnapshotIdentity, env: &PinnedEnv) -> String {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(SNAPSHOT_ID_DOMAIN_TAG);

    // The row-ref — one file per (row, query).
    lp_str(&mut buf, &identity.row_file);
    lp_str(&mut buf, &identity.row_function);
    lp_u16(&mut buf, identity.query_ordinal);
    // The query identity.
    lp_str(&mut buf, identity.query_helper_kind.tag());
    lp_str(
        &mut buf,
        &workspace_file_set_json(&identity.workspace_files),
    );
    lp_str(&mut buf, &identity.primary_canonical);
    lp_str(&mut buf, &identity.symbol_or_expression);
    lp_str(&mut buf, &type_args_json(&identity.type_arguments));
    lp_str(&mut buf, projection_mode_tag(identity.projection_mode));
    lp_str(&mut buf, identity.probe_rhs_kind.tag());
    lp_str(
        &mut buf,
        &canonical_json_string(&identity.host_project.to_canonical_json()),
    );
    lp_str(&mut buf, identity.oracle_value_kind.tag());
    // The pinned env / algorithm versions (incl. the STABLE env_corpus_id).
    lp_u32(&mut buf, env.normalizer_version);
    lp_u32(&mut buf, env.probe_synthesis_version);
    lp_str(&mut buf, &env.compiler_options_hash);
    lp_str(&mut buf, &env.env_corpus_id);
    lp_str(&mut buf, &env.tsgo_version);
    lp_u32(&mut buf, env.oracle_schema_version);

    let digest = blake3::hash(&buf);
    format!("u_{}", digest.to_hex())
}

/// Canonical JSON of the workspace-file set: sorted by path, each
/// `{ content_hash, path }` (keys sorted by the canonical encoder). Upsert order
/// is not an input.
fn workspace_file_set_json(files: &[WorkspaceFileRef]) -> String {
    let mut sorted: Vec<&WorkspaceFileRef> = files.iter().collect();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let arr = Value::Array(
        sorted
            .iter()
            .map(|f| json!({ "path": f.path, "content_hash": f.content_hash }))
            .collect(),
    );
    canonical_json_string(&arr)
}

fn type_args_json(args: &[Value]) -> String {
    canonical_json_string(&Value::Array(args.to_vec()))
}

fn lp_str(buf: &mut Vec<u8>, s: &str) {
    lp_bytes(buf, s.as_bytes());
}

fn lp_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("oracle snapshot_id field exceeds u32 length");
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
}

fn lp_u16(buf: &mut Vec<u8>, v: u16) {
    lp_bytes(buf, &v.to_le_bytes());
}

fn lp_u32(buf: &mut Vec<u8>, v: u32) {
    lp_bytes(buf, &v.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Canonical content hashing (§Q1 content-hash normalization + hash families)
// ---------------------------------------------------------------------------

/// Normalize file content to its canonical hashable form (pinned exactly):
/// (1) every CRLF (`\r\n`) and lone CR (`\r`) → a single LF (`\n`); (2) all
/// trailing `\n`s collapsed to EXACTLY ONE for non-empty content (an empty file
/// stays empty — no newline appended). So a file with no final newline, one, or
/// several blank trailing lines all hash identically.
#[allow(dead_code)]
pub(crate) fn canonical_content(text: &str) -> String {
    // (1) line endings → \n
    let mut lf = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            // Collapse \r\n and lone \r to a single \n.
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            lf.push('\n');
        } else {
            lf.push(c);
        }
    }
    // (2) trailing newline → exactly one (non-empty only).
    let trimmed = lf.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

/// `sha256:`-prefixed content hash over the canonicalized content.
#[allow(dead_code)]
pub(crate) fn content_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let canonical = canonical_content(text);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

/// `blake3:`-prefixed digest over arbitrary bytes (the harness's own identity
/// family).
#[allow(dead_code)]
pub(crate) fn blake3_tagged(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests;
