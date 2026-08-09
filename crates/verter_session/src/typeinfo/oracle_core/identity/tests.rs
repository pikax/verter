//! Discriminating guards for the oracle snapshot-identity + hashing layer
//! (§Q1 / §Q4 / §4): `snapshot_id` uniqueness + row-ref inclusion + redrive
//! determinism, and the pinned canonical content-hash encoding.

use serde_json::json;

use super::super::normalize::ProjectionModeKind;
use super::{
    blake3_tagged, canonical_content, content_hash, derive_snapshot_id, env_corpus_dir_name,
    HostProject, HostSetupKind, OracleValueKind, PinnedEnv, ProbeRhsKind, QueryHelperKind,
    SnapshotIdentity, WorkspaceFileRef, ORACLE_SCHEMA_VERSION, PROBE_SYNTHESIS_VERSION,
    SNAPSHOT_ID_DOMAIN_TAG,
};

fn base_env() -> PinnedEnv {
    PinnedEnv {
        tsgo_version: "7.0.0-dev.20260526.1".to_string(),
        oracle_schema_version: 1,
        normalizer_version: 1,
        probe_synthesis_version: 1,
        compiler_options_hash: "sha256:deadbeef".to_string(),
        env_corpus_id: "blake3:cafef00d".to_string(),
    }
}

fn base_identity() -> SnapshotIdentity {
    SnapshotIdentity {
        row_file: "utility_composition.rs".to_string(),
        row_function: "composed_props_shallow".to_string(),
        query_ordinal: 0,
        query_helper_kind: QueryHelperKind::ResolveExpr,
        workspace_files: vec![WorkspaceFileRef {
            path: "/fixtures/utility-composition.ts".to_string(),
            content_hash: "sha256:4d2a".to_string(),
        }],
        primary_canonical: "/fixtures/utility-composition.ts".to_string(),
        symbol_or_expression: "ComposedProps".to_string(),
        type_arguments: vec![],
        projection_mode: ProjectionModeKind::Shallow,
        probe_rhs_kind: ProbeRhsKind::Bare,
        host_project: HostProject {
            project_root: "/".to_string(),
            workspace_root: "/".to_string(),
            tsconfig_path: "/oracle.tsconfig.json".to_string(),
            host_setup_kind: HostSetupKind::Standalone,
        },
        oracle_value_kind: OracleValueKind::StructuredTypeExpr,
    }
}

// =========================================================================
// snapshot_id_v2_includes_probe_rhs_kind
// =========================================================================

#[test]
fn snapshot_id_v2_includes_probe_rhs_kind() {
    let env = base_env();
    let base = base_identity();
    let id_bare = derive_snapshot_id(&base, &env);

    // probe_rhs_kind is a VALUE-AFFECTING identity axis: the harness never
    // leans on the identity theorem to claim two capture paths are the same
    // cache key — a scaffolded capture derives a DIFFERENT snapshot_id.
    let mut dist = base.clone();
    dist.probe_rhs_kind = ProbeRhsKind::DistributiveIdentity;
    assert_ne!(
        id_bare,
        derive_snapshot_id(&dist, &env),
        "probe_rhs_kind is a snapshot_id input (bare vs distributive_identity)"
    );

    // Closed tags + the strict-decoder redrive inverse.
    assert_eq!(ProbeRhsKind::Bare.tag(), "bare");
    assert_eq!(
        ProbeRhsKind::DistributiveIdentity.tag(),
        "distributive_identity"
    );
    assert_eq!(ProbeRhsKind::from_tag("bare"), Some(ProbeRhsKind::Bare));
    assert_eq!(
        ProbeRhsKind::from_tag("distributive_identity"),
        Some(ProbeRhsKind::DistributiveIdentity)
    );
    assert_eq!(
        ProbeRhsKind::from_tag("mapped_identity"),
        None,
        "an unknown capture-strategy tag is a closed-set decode failure"
    );

    // The `snapshot_id` HASH-INPUT field set is unchanged by v3/v4 (the migration
    // mirror is NOT a `snapshot_id` input, and the v4 relation family hashes
    // under its OWN domain tag), so the v3-family domain tag stays v2 — but the
    // file-shape version is v4 (the `relation_verdict` kind addition), and it
    // flows into `snapshot_id` through `PinnedEnv.oracle_schema_version`, so
    // every derived `snapshot_id` value still changes.
    assert_eq!(
        SNAPSHOT_ID_DOMAIN_TAG, b"verter.oracle.snapshot_id.v2",
        "the snapshot_id HASH-INPUT field set is unchanged by v4; the domain tag stays v2"
    );
    assert_eq!(
        ORACLE_SCHEMA_VERSION, 4,
        "the relation_verdict kind addition is the v4 schema-shape change"
    );
    assert_eq!(
        PROBE_SYNTHESIS_VERSION, 2,
        "the scaffold RHS kind is a probe-synthesis algorithm change"
    );
}

// =========================================================================
// snapshot_id_is_unique
// =========================================================================

#[test]
fn snapshot_id_is_unique() {
    let env = base_env();
    let base = base_identity();

    // The id is "u_" + the FULL 32-byte BLAKE3 digest (64 hex chars), not a
    // 12-byte truncation.
    let id = derive_snapshot_id(&base, &env);
    assert!(id.starts_with("u_"), "id carries the u_ prefix");
    assert_eq!(
        id.len(),
        2 + 64,
        "id must be the FULL 32-byte BLAKE3 digest (64 hex chars), not truncated"
    );
    assert!(
        id[2..].bytes().all(|b| b.is_ascii_hexdigit()),
        "digest is hex"
    );

    // A set of distinct identities — including two differing ONLY in
    // query_ordinal and two differing ONLY in symbol — all derive distinct ids.
    let mut q1 = base.clone();
    q1.query_ordinal = 1;
    let mut sym = base.clone();
    sym.symbol_or_expression = "OtherProps".to_string();
    let mut mode = base.clone();
    mode.projection_mode = ProjectionModeKind::Navigate;

    let ids = [
        derive_snapshot_id(&base, &env),
        derive_snapshot_id(&q1, &env),
        derive_snapshot_id(&sym, &env),
        derive_snapshot_id(&mode, &env),
    ];
    let mut sorted = ids.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ids.len(),
        "all distinct identities derive distinct ids"
    );
}

// =========================================================================
// snapshot_id_includes_row_ref
// =========================================================================

#[test]
fn snapshot_id_includes_row_ref() {
    let env = base_env();
    let base = base_identity();
    let base_id = derive_snapshot_id(&base, &env);

    // Each row-ref component is an input: changing it changes the id.
    let mut other_file = base.clone();
    other_file.row_file = "apparent_types.rs".to_string();
    assert_ne!(
        base_id,
        derive_snapshot_id(&other_file, &env),
        "row_file is an input"
    );

    let mut other_fn = base.clone();
    other_fn.row_function = "another_row".to_string();
    assert_ne!(
        base_id,
        derive_snapshot_id(&other_fn, &env),
        "row_function is an input"
    );

    let mut other_ord = base.clone();
    other_ord.query_ordinal = 3;
    assert_ne!(
        base_id,
        derive_snapshot_id(&other_ord, &env),
        "query_ordinal is an input"
    );

    // The STABLE env_corpus_id IS an input (a pinned-env constant): changing it
    // changes the id (regeneration re-keys the filename).
    let mut other_env = env.clone();
    other_env.env_corpus_id = "blake3:00000000".to_string();
    assert_ne!(
        base_id,
        derive_snapshot_id(&base, &other_env),
        "env_corpus_id is an input"
    );

    // compiler_options_hash + the version axes are inputs too.
    let mut other_copts = env.clone();
    other_copts.compiler_options_hash = "sha256:00000000".to_string();
    assert_ne!(
        base_id,
        derive_snapshot_id(&base, &other_copts),
        "compiler_options_hash is an input"
    );

    let mut other_norm = env.clone();
    other_norm.normalizer_version = 2;
    assert_ne!(
        base_id,
        derive_snapshot_id(&base, &other_norm),
        "normalizer_version is an input"
    );

    // NOTE: `SnapshotIdentity` carries NO `oracle_family` field by construction —
    // the family is a directory/presentation key only and is EXCLUDED from the
    // id, so a row re-categorised to a different family keeps its id. (Enforced
    // structurally: there is no family axis to vary here.)
}

// =========================================================================
// snapshot_id_redrives_from_identity
// =========================================================================

#[test]
fn snapshot_id_redrives_from_identity() {
    let env = base_env();
    let identity = base_identity();

    // Deterministic: re-deriving from the same identity + env yields the SAME id
    // (so the driver can re-derive the filename from the registry without a
    // stored map).
    let a = derive_snapshot_id(&identity, &env);
    let b = derive_snapshot_id(&identity.clone(), &env.clone());
    assert_eq!(
        a, b,
        "snapshot_id re-derives deterministically from identity + env"
    );

    // workspace-file UPSERT ORDER is not an input: the same final set in a
    // different vector order derives the SAME id (sorted-by-path canonicalized).
    let mut reordered = base_identity();
    reordered.workspace_files = vec![
        WorkspaceFileRef {
            path: "/fixtures/b.ts".to_string(),
            content_hash: "sha256:bb".to_string(),
        },
        WorkspaceFileRef {
            path: "/fixtures/a.ts".to_string(),
            content_hash: "sha256:aa".to_string(),
        },
    ];
    let mut sorted_order = base_identity();
    sorted_order.workspace_files = vec![
        WorkspaceFileRef {
            path: "/fixtures/a.ts".to_string(),
            content_hash: "sha256:aa".to_string(),
        },
        WorkspaceFileRef {
            path: "/fixtures/b.ts".to_string(),
            content_hash: "sha256:bb".to_string(),
        },
    ];
    assert_eq!(
        derive_snapshot_id(&reordered, &env),
        derive_snapshot_id(&sorted_order, &env),
        "workspace-file upsert order is NOT a snapshot_id input (sorted-by-path canonicalized)"
    );

    // but a CONTENT change to a workspace file IS an input.
    let mut edited = base_identity();
    edited.workspace_files[0].content_hash = "sha256:ffff".to_string();
    assert_ne!(
        derive_snapshot_id(&base_identity(), &env),
        derive_snapshot_id(&edited, &env),
        "a workspace-file content hash is an input"
    );
}

// =========================================================================
// canonical_encoding_is_pinned
// =========================================================================

#[test]
fn canonical_encoding_is_pinned() {
    // (1) line-ending normalization: CRLF, lone CR, and LF hash identically.
    let crlf = content_hash("a\r\nb\r\n");
    let lf = content_hash("a\nb\n");
    let cr = content_hash("a\rb\r");
    assert_eq!(crlf, lf, "CRLF normalizes to LF before hashing");
    assert_eq!(cr, lf, "lone CR normalizes to LF before hashing");

    // (2) trailing-newline → exactly one: no final newline, one, or several
    // blank trailing lines all hash identically (non-empty content).
    let none = content_hash("a\nb");
    let one = content_hash("a\nb\n");
    let many = content_hash("a\nb\n\n\n");
    assert_eq!(
        none, one,
        "missing trailing newline canonicalizes to exactly one"
    );
    assert_eq!(
        many, one,
        "multiple trailing newlines canonicalize to exactly one"
    );

    // an EMPTY file (and an all-newline file) canonicalizes to empty.
    assert_eq!(canonical_content(""), "");
    assert_eq!(
        canonical_content("\n\n"),
        "",
        "all-trailing-newline content is empty-canonical"
    );

    // distinct content → distinct hash (the encoding does not collapse real
    // differences).
    assert_ne!(
        content_hash("a\nb\n"),
        content_hash("a\nc\n"),
        "distinct content diverges"
    );

    // hash FAMILY prefixes are self-describing on disk.
    assert!(
        content_hash("x").starts_with("sha256:"),
        "content hash is the sha256 family"
    );
    assert!(
        blake3_tagged(b"x").starts_with("blake3:"),
        "harness identity hash is the blake3 family"
    );

    // canonical JSON: object keys sorted lexicographically, compact (no
    // insignificant whitespace), arrays in semantic order.
    let value = json!({ "b": 1, "a": [3, 2, 1], "c": { "z": 0, "y": 1 } });
    let canonical = super::super::normalize::canonical_json_string(&value);
    assert_eq!(
        canonical, r#"{"a":[3,2,1],"b":1,"c":{"y":1,"z":0}}"#,
        "canonical JSON sorts keys, stays compact, preserves array order"
    );
}

// -- env_corpus_dir_name (the on-disk path boundary) ------------------------

#[test]
fn env_corpus_dir_name_maps_tag_separator_to_hyphen() {
    assert_eq!(
        env_corpus_dir_name("blake3:cafef00d"),
        "blake3-cafef00d",
        "the logical tag separator `:` maps to `-` at the path boundary"
    );
    assert_eq!(env_corpus_dir_name("sha256:00ff"), "sha256-00ff");
}

#[test]
fn env_corpus_dir_name_of_current_corpus_is_ntfs_safe() {
    let dir = env_corpus_dir_name(super::super::query_specs::CURRENT_ENV_CORPUS_ID);
    let illegal = |b: u8| {
        matches!(
            b,
            b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'\\' | b'/'
        ) || b < 0x20
    };
    assert!(
        !dir.bytes().any(illegal),
        "on-disk corpus dir name must contain no NTFS-illegal byte: {dir}"
    );
    assert!(
        !dir.ends_with('.') && !dir.ends_with(' '),
        "no trailing dot/space: {dir}"
    );
}

// =========================================================================
// relation_snapshot_id_discriminates_every_axis
// =========================================================================

use super::{
    derive_relation_snapshot_id, BinderLayoutEntry, FreshnessTag, InferenceModeTag,
    OverloadSelectionTag, RelationKindTag, RelationPolicyRecord, RelationVerdictIdentity,
    VarianceTag, RELATION_SNAPSHOT_ID_DOMAIN_TAG,
};

fn base_relation_identity() -> RelationVerdictIdentity {
    RelationVerdictIdentity {
        row_file: "relation_verdict_oracle.rs".to_string(),
        row_function: "relation_infer_multi".to_string(),
        query_ordinal: 0,
        workspace_files: vec![WorkspaceFileRef {
            path: "/fixtures/relation_verdict/relation_infer_multi.ts".to_string(),
            content_hash: "sha256:4d2a".to_string(),
        }],
        source_operand: json!({ "kind": "tuple", "elements": [
            { "kind": "literal", "value": 1 },
            { "kind": "literal", "value": 2 }
        ] }),
        target_operand: json!({ "kind": "tuple", "elements": [
            { "kind": "ref", "name": "__oracle_binder__B" },
            { "kind": "ref", "name": "__oracle_binder__A" }
        ] }),
        binder_layout: vec![
            BinderLayoutEntry {
                ordinal: 0,
                name: "B".to_string(),
                constraint: None,
            },
            BinderLayoutEntry {
                ordinal: 1,
                name: "A".to_string(),
                constraint: None,
            },
        ],
        relation: RelationKindTag::Assignable,
        policy: RelationPolicyRecord::default_record(),
        freshness: FreshnessTag::Regular,
        inference_mode: InferenceModeTag::TargetPattern,
        host_project: HostProject {
            project_root: "/".to_string(),
            workspace_root: "/".to_string(),
            tsconfig_path: "/oracle.tsconfig.json".to_string(),
            host_setup_kind: HostSetupKind::Standalone,
        },
        oracle_value_kind: OracleValueKind::RelationVerdict,
    }
}

#[test]
fn relation_snapshot_id_discriminates_every_axis() {
    let env = base_env();
    let base = base_relation_identity();
    let base_id = derive_relation_snapshot_id(&base, &env);
    assert!(base_id.starts_with("u_"));
    assert_eq!(
        base_id.len(),
        2 + 64,
        "the FULL 32-byte digest, never truncated"
    );

    // Deterministic redrive.
    assert_eq!(
        base_id,
        derive_relation_snapshot_id(&base_relation_identity(), &env),
        "the relation id re-derives deterministically"
    );

    // EVERY value-affecting axis is an input.
    let mut source = base.clone();
    source.source_operand = json!({ "kind": "primitive", "name": "string" });
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&source, &env),
        "source_operand is an input"
    );

    let mut target = base.clone();
    target.target_operand = json!({ "kind": "tuple", "elements": [
        { "kind": "ref", "name": "__oracle_binder__A" },
        { "kind": "ref", "name": "__oracle_binder__B" }
    ] });
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&target, &env),
        "target_operand is an input"
    );

    // The binder layout's ORDER is an identity input (preorder, NOT name sort):
    // swapping the two binders' layout order changes the id.
    let mut swapped = base.clone();
    swapped.binder_layout = vec![
        BinderLayoutEntry {
            ordinal: 0,
            name: "A".to_string(),
            constraint: None,
        },
        BinderLayoutEntry {
            ordinal: 1,
            name: "B".to_string(),
            constraint: None,
        },
    ];
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&swapped, &env),
        "binder layout order is an input (preorder, not name sort)"
    );

    let mut relation = base.clone();
    relation.relation = RelationKindTag::Subtype;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&relation, &env),
        "relation kind is an input"
    );

    let mut policy = base.clone();
    policy.policy.overload_selection = OverloadSelectionTag::FirstApplicable;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&policy, &env),
        "policy.overload_selection is an input"
    );
    let mut policy2 = base.clone();
    policy2.policy.excess_property_check = true;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&policy2, &env),
        "policy.excess_property_check is an input"
    );
    let mut policy3 = base.clone();
    policy3.policy.variance = VarianceTag::StrictContravariance;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&policy3, &env),
        "policy.variance is an input"
    );

    let mut freshness = base.clone();
    freshness.freshness = FreshnessTag::Fresh;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&freshness, &env),
        "freshness is an input"
    );

    let mut mode = base.clone();
    mode.inference_mode = InferenceModeTag::None;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&mode, &env),
        "inference_mode is an input"
    );

    let mut host = base.clone();
    host.host_project.tsconfig_path = "/other.tsconfig.json".to_string();
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&host, &env),
        "host/config identity is an input"
    );

    let mut files = base.clone();
    files.workspace_files[0].content_hash = "sha256:ffff".to_string();
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&files, &env),
        "workspace identity is an input"
    );

    // The config/env axes (the pinned env) are inputs.
    let mut other_env = env.clone();
    other_env.compiler_options_hash = "sha256:00000000".to_string();
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&base, &other_env),
        "compiler_options_hash is an input"
    );
    let mut other_env2 = env.clone();
    other_env2.oracle_schema_version = 99;
    assert_ne!(
        base_id,
        derive_relation_snapshot_id(&base, &other_env2),
        "oracle_schema_version is an input"
    );

    // DOMAIN SEPARATION: the relation family hashes under its own tag — the
    // same row-ref + env under the v3 TypeExpr derivation yields a DIFFERENT id
    // (no cross-family collision).
    assert_eq!(
        RELATION_SNAPSHOT_ID_DOMAIN_TAG, b"verter.oracle.snapshot_id.relation.v1",
        "the v4 family hashes under its own domain tag"
    );
    let v3_identity = SnapshotIdentity {
        row_file: base.row_file.clone(),
        row_function: base.row_function.clone(),
        query_ordinal: base.query_ordinal,
        query_helper_kind: QueryHelperKind::ResolveExpr,
        workspace_files: base.workspace_files.clone(),
        primary_canonical: "/fixtures/relation_verdict/relation_infer_multi.ts".to_string(),
        symbol_or_expression: "X".to_string(),
        type_arguments: vec![],
        projection_mode: ProjectionModeKind::Expanded,
        probe_rhs_kind: ProbeRhsKind::Bare,
        host_project: base.host_project.clone(),
        oracle_value_kind: OracleValueKind::StructuredTypeExpr,
    };
    assert_ne!(
        base_id,
        derive_snapshot_id(&v3_identity, &env),
        "the v4 relation id is domain-separated from the v3 TypeExpr id"
    );

    // The closed tag sets round-trip; unknown tags are closed-set failures.
    assert_eq!(
        RelationKindTag::from_tag("assignable"),
        Some(RelationKindTag::Assignable)
    );
    assert_eq!(RelationKindTag::from_tag("extends"), None);
    assert_eq!(
        OverloadSelectionTag::from_tag("all"),
        Some(OverloadSelectionTag::All)
    );
    assert_eq!(OverloadSelectionTag::from_tag("best"), None);
    assert_eq!(
        VarianceTag::from_tag("method_parameter_bivariance"),
        Some(VarianceTag::MethodParameterBivariance)
    );
    assert_eq!(VarianceTag::from_tag("covariance"), None);
    assert_eq!(
        FreshnessTag::from_tag("regular"),
        Some(FreshnessTag::Regular)
    );
    assert_eq!(FreshnessTag::from_tag("stale"), None);
    assert_eq!(
        InferenceModeTag::from_tag("target_pattern"),
        Some(InferenceModeTag::TargetPattern)
    );
    assert_eq!(InferenceModeTag::from_tag("infer"), None);
    assert_eq!(
        RelationPolicyRecord::default_record(),
        RelationPolicyRecord {
            overload_selection: OverloadSelectionTag::All,
            excess_property_check: false,
            variance: VarianceTag::MethodParameterBivariance,
        },
        "the default policy record is the only admissible capture policy"
    );
}
