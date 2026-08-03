//! Frozen contracts and barriers for the registered-carrier structural
//! authority.
//!
//! The always-on tests validate the ratified schema and capability ledger.
//! The feature-gated inverse tests remain RED until each final-state invariant
//! is implemented.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const LEDGER_KEYS: [&str; 14] = [
    "path",
    "symbol",
    "runtime_role",
    "provenance",
    "shipped_artifact",
    "shipping_target",
    "rust_target",
    "candidate_evidence",
    "processor_authority",
    "capability_class",
    "disposition",
    "acceptance_id",
    "test",
    "architecture_guard",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(relative: &str) -> Value {
    let path = workspace_root().join(relative);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn validate_frozen_schema(schema: &Value) -> Result<(), String> {
    if schema["schema"] != "verter.scanners-replacement.v1" {
        return Err("wrong schema identity".into());
    }
    if schema["completeness"]["closed"] != true {
        return Err("schema is not marked closed".into());
    }
    let required = schema["completeness"]["required_types"]
        .as_array()
        .ok_or("missing required_types")?;
    let declarations = schema["declarations"]
        .as_object()
        .ok_or("missing declarations")?;
    for name in required {
        let name = name.as_str().ok_or("non-string required type")?;
        if !declarations.contains_key(name) {
            return Err(format!("unresolved type {name}"));
        }
    }
    let builtins = BTreeSet::from(["String", "bool", "i32", "u32", "u64"]);
    for (owner, declaration) in declarations {
        for member in declaration["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(declaration["variants"].as_array().into_iter().flatten())
        {
            if let Some(referenced) = member["type"].as_str() {
                if !builtins.contains(referenced) && !declarations.contains_key(referenced) {
                    return Err(format!("{owner} references unresolved type {referenced}"));
                }
            }
        }
        if let Some(storage) = declaration["storage"].as_str() {
            if !builtins.contains(storage) && !declarations.contains_key(storage) {
                return Err(format!("{owner} references unresolved storage {storage}"));
            }
        }
    }
    let projection = schema["structure_projection"]["authority_materialized_fields"]
        .as_array()
        .ok_or("missing authority materialization table")?;
    for field in [
        "DocumentStructureV1.schema_version",
        "StructureSectionV1.block_content_basis_token",
        "CanonicalRangeV1.Lsp.encoding_session_token",
    ] {
        if !projection.iter().any(|row| row["field"] == field) {
            return Err(format!("unmapped authority field {field}"));
        }
    }
    if schema["declarations"]["ComponentPublicContract"]["fields"]
        .as_array()
        .is_some_and(|fields| fields.iter().any(|field| field["name"] == "schema_version"))
    {
        return Err("ComponentPublicContract owns a forbidden schema_version".into());
    }
    if schema["declarations"]["CanonicalRangeV1"]["representation"] != "required_oneof"
        || schema["declarations"]["CanonicalRangeV1"]["variants"]
            .as_array()
            .is_none_or(|variants| variants.len() != 2)
    {
        return Err("CanonicalRangeV1 is not the ratified two-arm oneof".into());
    }
    if schema["grammars"]["PublicHashV1"] != "^sha256:[0-9a-f]{64}$" {
        return Err("PublicHashV1 grammar drifted".into());
    }
    if schema["declarations"]["PreprocessorStepV1"]["fields"]
        != serde_json::json!([
            {"name":"identity","type":"PreprocessorIdentityV1","tag":1,"presence":"R"},
            {"name":"trusted_attestation_hash","type":"PublicHashV1","tag":2,"presence":"R"},
            {"name":"input_space_token","type":"SourceSpaceTokenV1","tag":3,"presence":"R"},
            {"name":"output_space_token","type":"SourceSpaceTokenV1","tag":4,"presence":"R"},
            {"name":"input_hash","type":"ContentHashV1","tag":5,"presence":"R"},
            {"name":"output_hash","type":"ContentHashV1","tag":6,"presence":"R"},
            {"name":"map_hash","type":"QualifiedSourceMapHashV1","tag":7,"presence":"R"}
        ])
    {
        return Err("PreprocessorStepV1 tag/presence ledger drifted".into());
    }
    if schema["declarations"]["DependencyResolutionProvenanceV1"]["fields"]
        .as_array()
        .is_none_or(|fields| {
            fields.len() != 6
                || fields[1]
                    != serde_json::json!({"name":"importer_space_token","type":"SourceSpaceTokenV1","tag":2,"presence":"O"})
        })
    {
        return Err("DependencyResolutionProvenanceV1 tag/presence ledger drifted".into());
    }
    if schema["declarations"]["PreCaptureValidationFailureV1"]["values"]
        != serde_json::json!([
            {"name":"MissingOwner","number":1},
            {"name":"DuplicateOwner","number":2},
            {"name":"ExternalInlineConflict","number":3},
            {"name":"LanguageMismatch","number":4},
            {"name":"OriginPolicyMismatch","number":5},
            {"name":"PriorBasisKindMismatch","number":6}
        ])
    {
        return Err("pre-capture phase algebra drifted".into());
    }
    Ok(())
}

#[test]
fn scanners_replacement_schema_is_closed_and_ratified() {
    let schema = read_json("schemas/scanners-replacement-v1.schema.json");
    validate_frozen_schema(&schema).unwrap();

    assert_eq!(
        schema["authority"]["precedence"],
        serde_json::json!(["T-B-schema-ratification-v2", "scanners-replacement-verter"])
    );
    assert_eq!(
        schema["declarations"]["SemanticTypeSource"]["fields"][0]["name"],
        "analysis_snapshot_token"
    );
    assert_eq!(
        schema["declarations"]["ComponentPublicContract"]["fields"][3]["type"],
        "ComponentContractProvenanceV1"
    );
    assert_eq!(
        schema["declarations"]["ComponentContractUnsupportedReason"]["values"],
        serde_json::json!([
            {"name":"UnsupportedCarrier","number":5},
            {"name":"SemanticProviderUnavailable","number":6},
            {"name":"InvalidArtifact","number":7}
        ])
    );
}

#[test]
fn scanners_replacement_schema_mutations_fail_completeness() {
    let schema = read_json("schemas/scanners-replacement-v1.schema.json");

    let mut dropped_attestation = schema.clone();
    dropped_attestation["declarations"]["PreprocessorStepV1"]["fields"]
        .as_array_mut()
        .unwrap()
        .remove(1);
    assert!(validate_frozen_schema(&dropped_attestation)
        .unwrap_err()
        .contains("PreprocessorStepV1 tag/presence"));

    let mut representation_only = schema.clone();
    representation_only["declarations"]["DependencyResolutionProvenanceV1"]["fields"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "resolved_display_url",
            "type": "String",
            "tag": 7,
            "presence": "O"
        }));
    assert_eq!(
        validate_frozen_schema(&representation_only).unwrap_err(),
        "DependencyResolutionProvenanceV1 tag/presence ledger drifted"
    );

    let mut flat_range = schema.clone();
    flat_range["declarations"]["CanonicalRangeV1"]["representation"] =
        Value::String("record".into());
    assert!(validate_frozen_schema(&flat_range)
        .unwrap_err()
        .contains("two-arm oneof"));

    let mut cross_phase = schema;
    cross_phase["declarations"]["PreCaptureValidationFailureV1"]["values"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"ProcessorExecutionFailed","number":7}));
    assert_eq!(
        validate_frozen_schema(&cross_phase).unwrap_err(),
        "pre-capture phase algebra drifted"
    );
}

#[test]
fn scanners_replacement_capability_ledger_is_total() {
    let ledger = read_json("docs/arch/scanners-replacement-capability-ledger.json");
    assert_eq!(
        ledger["schema"],
        "verter.scanners-replacement-capability-ledger.v1"
    );
    let rows = ledger["rows"].as_array().expect("ledger rows");
    assert!(!rows.is_empty(), "candidate universe must be non-empty");

    let expected_keys = BTreeSet::from(LEDGER_KEYS);
    let mut identities = BTreeSet::new();
    let mut symbols = BTreeSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows {
        let object = row.as_object().expect("ledger row object");
        let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        assert_eq!(keys, expected_keys, "row has non-canonical shape: {row}");
        let path = row["path"].as_str().expect("path");
        let symbol = row["symbol"].as_str().expect("symbol");
        assert!(
            identities.insert((path, symbol)),
            "duplicate ledger identity"
        );
        symbols.insert(symbol);
        let disposition = row["disposition"].as_str().expect("disposition");
        assert!(
            matches!(
                disposition,
                "migrate" | "delete" | "allowed_nested" | "allowed_standalone" | "test_only"
            ),
            "invalid disposition {disposition}"
        );
        if row["runtime_role"] == "production_runtime" {
            assert_ne!(disposition, "test_only");
        }
        for key in ["acceptance_id", "test", "architecture_guard"] {
            assert!(row[key].as_str().is_some_and(|value| !value.is_empty()));
        }
        *counts
            .entry(row["capability_class"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }

    for required in [
        "sfcScriptImportAnchor",
        "findHtmlTagEnd",
        "isFrameworkAttributeNamePosition",
        "find_opening_tag_end",
        "ScriptDetector",
    ] {
        assert!(
            symbols.contains(required),
            "missing required seed {required}"
        );
    }
    assert_eq!(ledger["statistics"]["rows_total"], rows.len());
    assert_eq!(
        ledger["statistics"]["by_capability_class"],
        serde_json::to_value(counts).unwrap()
    );
    assert_eq!(ledger["set_equality"]["unclassified_runtime_rows"], 0);
    assert_eq!(ledger["set_equality"]["deferred_runtime_rows"], 0);
    assert_eq!(
        ledger["consumer_matrix"].as_array().map(Vec::len),
        Some(
            rows.iter()
                .filter(|row| row["runtime_role"] == "production_runtime")
                .count()
        )
    );
}
