//! Frozen contracts and barriers for the registered-carrier structural
//! authority.
//!
//! The always-on tests validate the ratified schema.
//! Architecture enforcement lives in construction boundaries and compile-fail
//! tests, never source-name scanners.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// Closed type universe derived from ruling v2 §§4.1–4.9 plus the V1 images
// named by the binding plan's canonical projection table.
const RULING_REQUIRED_TYPES: &[&str] = &[
    "AnalysisSnapshotToken",
    "ArtifactBlockTokenV1",
    "ArtifactMarkupNodeTokenV1",
    "AttachAttribute",
    "AttributeDynamicSyntaxV1",
    "AttributeQuoteV1",
    "AttributeTokenV1",
    "AttributeValuePartV1",
    "AttributeValuePartV1Expression",
    "AttributeValuePartV1Static",
    "AttributeValueV1",
    "AttributeValueV1Expression",
    "AttributeValueV1Missing",
    "AttributeValueV1Mixed",
    "AttributeValueV1Static",
    "AuthoredNameV1",
    "AuthoredProvenance",
    "AuthoredProvenanceKind",
    "AuthoredSliceV1",
    "AuthoredTypeEvidence",
    "AuthoredTypeSource",
    "AwaitHead",
    "Base64UrlStringV1",
    "BlockContentArtifactSchemaVersion",
    "BlockContentAvailabilityV1",
    "BlockContentBasisTokenV1",
    "BlockContentCapturedEchoV1",
    "BlockContentCorrelationTokenV1",
    "BlockContentOriginFingerprintV1",
    "BlockContentOriginV1",
    "BlockContentOriginV1External",
    "BlockContentOriginV1Inline",
    "BlockContentPostCaptureStaleReasonV1",
    "BlockContentPreCaptureEchoV1",
    "BlockContentPreCaptureStaleReasonV1",
    "BlockContentPreCaptureUnavailableReasonV1",
    "BlockContentProvenanceV1",
    "BlockContentResolveContextTokenV1",
    "BlockContentResolveRequestV1",
    "BlockContentResolveResponseV1",
    "BlockContentResolveResponseV1PostCaptureCancelled",
    "BlockContentResolveResponseV1PostCaptureClosed",
    "BlockContentResolveResponseV1PostCaptureFailed",
    "BlockContentResolveResponseV1PostCaptureStaleNeedsRecapture",
    "BlockContentResolveResponseV1PostCaptureStaleWithReplacement",
    "BlockContentResolveResponseV1PostCaptureSuperseded",
    "BlockContentResolveResponseV1PreCaptureCancelled",
    "BlockContentResolveResponseV1PreCaptureClosed",
    "BlockContentResolveResponseV1PreCaptureFailed",
    "BlockContentResolveResponseV1PreCaptureStale",
    "BlockContentResolveResponseV1PreCaptureUnavailable",
    "BlockContentResolveResponseV1Resolved",
    "BlockRecoveryReasonV1",
    "CacheClusterSchemaVersion",
    "CanonicalRangeV1",
    "CanonicalRangeV1Lsp",
    "CanonicalRangeV1Offset",
    "CarrierAttributeV1",
    "CarrierBlockRoleV1",
    "CarrierBlockRoleV1Custom",
    "CarrierBlockRoleV1Script",
    "CarrierBlockRoleV1Style",
    "CarrierBlockRoleV1TemplateHost",
    "CarrierCacheSerializationVersion",
    "CarrierGrammarFingerprintSchemaVersion",
    "CarrierParserGrammarVersion",
    "CarrierParserVersion",
    "CarrierSourceMapSchemaVersion",
    "CarrierSourceSpaceSchemaVersion",
    "ClientOpenEpochTokenV1",
    "ClientRequestNonceV1",
    "ClientRequestTokenV1",
    "ClientVersionV1",
    "CommentSyntax",
    "ComponentContractAvailability",
    "ComponentContractProvenanceV1",
    "ComponentContractUnsupported",
    "ComponentContractUnsupportedReason",
    "ComponentMetaSchemaVersion",
    "ComponentPublicContract",
    "ConfigContextTokenV1",
    "ContentArtifactTokenV1",
    "ContentGenerationV1",
    "ContentHashV1",
    "ContractDegradation",
    "ContractDegradationCode",
    "ContractExactness",
    "DescriptorHandle",
    "DescriptorManifestToken",
    "DirectiveArgumentV1",
    "DirectiveArgumentV1Dynamic",
    "DirectiveArgumentV1None",
    "DirectiveArgumentV1Static",
    "DirectiveAttribute",
    "DirectiveFamilyV1",
    "DirectiveFamilyV1Svelte",
    "DirectiveFamilyV1Vue",
    "DirectiveModifierV1",
    "DocumentRevisionTokenV1",
    "DocumentStructureRequestV1",
    "DocumentStructureResponseV1",
    "DocumentStructureResponseV1Available",
    "DocumentStructureResponseV1Closed",
    "DocumentStructureResponseV1ReplacementDocument",
    "DocumentStructureResponseV1StaleClientDocument",
    "DocumentStructureResponseV1Superseded",
    "DocumentStructureResponseV1Unavailable",
    "DocumentStructureV1",
    "DocumentUnavailableReasonV1",
    "DocumentUriV1",
    "EachHead",
    "EntityDecodeRecipeV1",
    "EntityDecodeRecipeV1Html5Attribute",
    "EntityDecodeRecipeV1Html5Text",
    "EntityDecodeRecipeV1SvelteAttribute",
    "EntityDecodeRecipeV1SvelteText",
    "EntityDecodeRecipeV1XmlAttribute",
    "EntityDecodeRecipeV1XmlText",
    "FrameworkAdapterId",
    "FrameworkAdapterSemanticVersion",
    "FrameworkArtifactTokenV1",
    "FrameworkParseArtifactSchemaVersion",
    "IfHead",
    "InterpolationSyntax",
    "KeyHead",
    "LanguageIdV1",
    "LazyDecodedTextV1",
    "LazyDecodedTextV1EntityDecode",
    "LazyDecodedTextV1SameAsSource",
    "LineCharacterV1",
    "MarkupElementKindV1",
    "MarkupElementKindV1Component",
    "MarkupElementKindV1DynamicComponent",
    "MarkupElementKindV1DynamicElement",
    "MarkupElementKindV1Native",
    "MarkupElementKindV1SvelteNestedStyle",
    "MarkupElementKindV1SvelteSpecial",
    "MarkupElementKindV1Unknown",
    "MarkupElementSyntax",
    "MarkupElementSyntaxV1",
    "MarkupInterpolationFamily",
    "MarkupInterpolationFamilyV1",
    "MarkupNamespaceV1",
    "MarkupNodeSyntaxV1",
    "MarkupNodeV1",
    "MarkupRootBlock",
    "NamedAttributeSyntaxV1",
    "NamedAttribute",
    "NapiSchemaVersion",
    "NativeApiVersion",
    "NestedLanguageV1",
    "NestedLanguageV1CoffeeScript",
    "NestedLanguageV1Css",
    "NestedLanguageV1Custom",
    "NestedLanguageV1Html",
    "NestedLanguageV1JavaScript",
    "NestedLanguageV1Jsx",
    "NestedLanguageV1Less",
    "NestedLanguageV1PostCss",
    "NestedLanguageV1Pug",
    "NestedLanguageV1Sass",
    "NestedLanguageV1Scss",
    "NestedLanguageV1Stylus",
    "NestedLanguageV1Tsx",
    "NestedLanguageV1TypeScript",
    "NestedParserModeV1",
    "OpaqueCapabilityTokenV1",
    "PositionEncodingSessionTokenV1",
    "PostCaptureProcessingFailureV1",
    "PreCaptureValidationFailureV1",
    "ProviderProtocolVersion",
    "PublicCallSignature",
    "PublicDerivedHandlerShape",
    "PublicEvent",
    "PublicHashV1",
    "PublicParameter",
    "PublicPositionEncodingV1",
    "PublicPositionV1",
    "PublicPositionV1LineCharacter",
    "PublicPositionV1Offset",
    "PublicProp",
    "PublicRangeV1",
    "PublicSlot",
    "PublicSlotProp",
    "PublicTypeReference",
    "PublicTypeReferenceDescriptorHandle",
    "PublicTypeReferencePublishedSemanticSource",
    "PublicationPolicy",
    "PublicationPolicyExactOnly",
    "PublicationPolicyPermitAuthoredForIncomplete",
    "PublicationProvenance",
    "PublicationReason",
    "PublicationReasonAuthoredForIncomplete",
    "PublicationReasonAuthoredSymbolicRepresentation",
    "PublicationReasonResolvedExactConcrete",
    "PublicationReasonResolvedExactSymbolic",
    "PublicationResult",
    "PublicationResultAbsent",
    "PublicationResultFailed",
    "PublicationResultPublished",
    "PublicationSelection",
    "PublicationSemanticAuthority",
    "QualifiedMapFidelityV1",
    "QualifiedMapSegmentV1",
    "QualifiedSourceMapHashV1",
    "QualifiedSourceMapSchemaVersion",
    "QualifiedSourceMapV1",
    "QuotedAttributeQuoteV1",
    "RecoveredMarkupKindV1",
    "RecoveredSyntax",
    "RecoveredTermination",
    "RegisteredSourceTokenV1",
    "ResolutionDiagnostic",
    "ResolutionDiagnosticSeverity",
    "ResolutionProvenance",
    "ResolutionProvider",
    "ResolutionTokenV1",
    "ResolvedDialectV1",
    "ResolvedLanguageV1",
    "ResolvedTypeAuthority",
    "ResolvedTypeAuthorityAbsent",
    "ResolvedTypeAuthorityFailed",
    "ResolvedTypeAuthorityPresent",
    "ResolverContextTokenV1",
    "ScriptRoleV1",
    "ScriptSourceTypeV1",
    "ScriptSourceTypeV1Custom",
    "ScriptSourceTypeV1JavaScript",
    "ScriptSourceTypeV1Jsx",
    "ScriptSourceTypeV1Missing",
    "ScriptSourceTypeV1Tsx",
    "ScriptSourceTypeV1TypeScript",
    "SectionBlock",
    "SelectedBlockInputV1",
    "SemanticSourceToken",
    "SemanticTypeKind",
    "SemanticTypeSource",
    "SessionCurrentParserVersion",
    "SnippetHead",
    "SourceEncodingV1",
    "SourceSpaceDescriptorV1",
    "SourceSpaceKindV1",
    "SourceSpaceTokenV1",
    "SpreadAttribute",
    "StructureBlockV1",
    "StructureProtocolVersion",
    "StructureSectionV1",
    "StampedBlockContentRequestV1",
    "StampedBlockContentResultV1",
    "StyleDialectV1",
    "StyleDialectV1Css",
    "StyleDialectV1Custom",
    "StyleDialectV1Less",
    "StyleDialectV1Missing",
    "StyleDialectV1PostCss",
    "StyleDialectV1Sass",
    "StyleDialectV1Scss",
    "StyleDialectV1Stylus",
    "StyleModuleV1",
    "StyleModuleV1Default",
    "StyleModuleV1Named",
    "StyleModuleV1None",
    "SvelteAwaitInlineBranchV1",
    "SvelteAwaitInlineBranchV1Catch",
    "SvelteAwaitInlineBranchV1None",
    "SvelteAwaitInlineBranchV1Then",
    "SvelteClauseHeadV1",
    "SvelteClauseHeadV1Catch",
    "SvelteClauseHeadV1Else",
    "SvelteClauseHeadV1ElseIf",
    "SvelteClauseHeadV1Then",
    "SvelteClauseSyntax",
    "SvelteClauseSyntaxV1",
    "SvelteControlBlockHeadV1",
    "SvelteControlBlockSyntax",
    "SvelteControlBlockSyntaxV1",
    "SvelteDirectiveKindV1",
    "SvelteDirectiveKindV1Animate",
    "SvelteDirectiveKindV1Bind",
    "SvelteDirectiveKindV1Class",
    "SvelteDirectiveKindV1Custom",
    "SvelteDirectiveKindV1In",
    "SvelteDirectiveKindV1Let",
    "SvelteDirectiveKindV1On",
    "SvelteDirectiveKindV1Out",
    "SvelteDirectiveKindV1Style",
    "SvelteDirectiveKindV1Transition",
    "SvelteDirectiveKindV1Unknown",
    "SvelteDirectiveKindV1Use",
    "SvelteSpecialElementKindV1",
    "SvelteSpecialElementKindV1Body",
    "SvelteSpecialElementKindV1Boundary",
    "SvelteSpecialElementKindV1Component",
    "SvelteSpecialElementKindV1Document",
    "SvelteSpecialElementKindV1Element",
    "SvelteSpecialElementKindV1Fragment",
    "SvelteSpecialElementKindV1Head",
    "SvelteSpecialElementKindV1Options",
    "SvelteSpecialElementKindV1SelfRef",
    "SvelteSpecialElementKindV1Unknown",
    "SvelteSpecialElementKindV1Window",
    "SvelteStandaloneTagFamilyV1",
    "SvelteStandaloneTagFamilyV1Attach",
    "SvelteStandaloneTagFamilyV1Const",
    "SvelteStandaloneTagFamilyV1Debug",
    "SvelteStandaloneTagFamilyV1Html",
    "SvelteStandaloneTagFamilyV1LegacyConst",
    "SvelteStandaloneTagFamilyV1Let",
    "SvelteStandaloneTagFamilyV1Render",
    "SvelteStandaloneTagFamilyV1Unknown",
    "SvelteStandaloneTagSyntax",
    "SvelteStandaloneTagSyntaxV1",
    "SymbolicEquivalenceProof",
    "SymbolicEquivalenceProofLosslessProjection",
    "SymbolicEquivalenceProofSameResolvedSymbol",
    "SyntaxTerminationV1",
    "SyntaxTerminationV1Closed",
    "SyntaxTerminationV1SelfClosing",
    "SyntaxTerminationV1UnclosedEof",
    "SyntaxTerminationV1Void",
    "TerminalTypeDisplay",
    "TerminalTypeDisplayFormat",
    "TextDocumentIdentifierV1",
    "TextSyntax",
    "TypeExactness",
    "TypePublicationMeta",
    "TypeReferenceLookupFailure",
    "TypedResolutionFailure",
    "TypedResolutionFailureBudgetExceeded",
    "TypedResolutionFailureCycle",
    "TypedResolutionFailureInternalFailure",
    "TypedResolutionFailureInvalidDescriptor",
    "TypedResolutionFailureProviderUnavailable",
    "TypedResolutionFailureSourceStale",
    "TypedResolutionFailureSymbolMissing",
    "TypedResolutionFailureUnsupportedSyntax",
    "UnknownDirectiveReasonV1",
    "UnknownMarkupReasonV1",
    "UnknownSyntax",
    "UnpluginApiVersion",
    "Utf8ByteLengthV1",
    "Utf8TextV1",
    "VueDirectiveKindV1",
    "WasmSchemaVersion",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root above verter_session manifest")
        .to_path_buf()
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
    let required = schema["completeness"]["required_types"]
        .as_array()
        .ok_or("missing required_types")?;
    let declarations = schema["declarations"]
        .as_object()
        .ok_or("missing declarations")?;
    let ruling_required = RULING_REQUIRED_TYPES
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let artifact_required = required
        .iter()
        .map(|name| name.as_str().ok_or("non-string required type"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if artifact_required != ruling_required {
        return Err("required_types does not equal the ruling-derived closed set".into());
    }
    for name in RULING_REQUIRED_TYPES {
        if !declarations.contains_key(*name) {
            return Err(format!("ruling-required type {name} is undeclared"));
        }
    }
    for name in required {
        let name = name.as_str().ok_or("non-string required type")?;
        if !declarations.contains_key(name) {
            return Err(format!("unresolved type {name}"));
        }
    }
    let builtins = BTreeSet::from(["String", "bool", "i32", "u32", "u64"]);
    for (owner, declaration) in declarations {
        match declaration["kind"].as_str() {
            Some("record") if !declaration["fields"].is_array() => {
                return Err(format!("{owner} has no explicit field ledger"));
            }
            Some("sum") => {
                let variants = declaration["variants"]
                    .as_array()
                    .ok_or_else(|| format!("{owner} has no variant ledger"))?;
                for variant in variants {
                    let payload = variant["type"]
                        .as_str()
                        .ok_or_else(|| format!("{owner} arm has no payload declaration"))?;
                    let payload_declaration = declarations.get(payload).ok_or_else(|| {
                        format!("{owner} references unresolved payload {payload}")
                    })?;
                    if payload_declaration["kind"] != "record"
                        || !payload_declaration["fields"].is_array()
                    {
                        return Err(format!(
                            "{owner} payload {payload} has no complete field ledger"
                        ));
                    }
                }
            }
            Some("canonical_projection") => {
                return Err(format!("{owner} uses canonical_projection shorthand"));
            }
            _ => {}
        }
        if let Some(fields) = declaration["fields"].as_array() {
            let mut names = BTreeSet::new();
            let mut tags = BTreeSet::new();
            for member in fields {
                let name = member["name"]
                    .as_str()
                    .ok_or_else(|| format!("{owner} field has no name"))?;
                let tag = member["tag"]
                    .as_u64()
                    .ok_or_else(|| format!("{owner}.{name} field has no tag"))?;
                if !names.insert(name) || !tags.insert(tag) || tag == 0 {
                    return Err(format!("{owner} has duplicate/zero field identity"));
                }
                if !matches!(member["presence"].as_str(), Some("R" | "O" | "L")) {
                    return Err(format!("{owner}.{name} has invalid presence"));
                }
                if declaration["reserved_numbers"]
                    .as_array()
                    .is_some_and(|reserved| reserved.iter().any(|number| number == tag))
                {
                    return Err(format!("{owner}.{name} reuses reserved tag {tag}"));
                }
            }
        }
        if let Some(variants) = declaration["variants"].as_array() {
            let mut names = BTreeSet::new();
            let mut tags = BTreeSet::new();
            for variant in variants {
                let name = variant["name"]
                    .as_str()
                    .ok_or_else(|| format!("{owner} variant has no name"))?;
                let tag = variant["tag"]
                    .as_u64()
                    .ok_or_else(|| format!("{owner}.{name} variant has no tag"))?;
                if !names.insert(name) || !tags.insert(tag) || tag == 0 {
                    return Err(format!("{owner} has duplicate/zero variant identity"));
                }
            }
        }
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
    for (direction, messages) in schema["direction_specific_wire"]
        .as_object()
        .ok_or("missing direction wire table")?
    {
        if let Some(messages) = messages.as_array() {
            for message in messages {
                let message = message.as_str().ok_or("non-string wire type")?;
                if !declarations.contains_key(message) {
                    return Err(format!("{direction} references unresolved type {message}"));
                }
            }
        }
    }
    for row in schema["structure_projection"]["canonical_to_v1"]
        .as_array()
        .ok_or("missing canonical projection table")?
    {
        let projected = row[1].as_str().ok_or("invalid projection row")?;
        if !declarations.contains_key(projected) {
            return Err(format!("projection references unresolved type {projected}"));
        }
    }
    for block in [
        &schema["persisted_carrier_artifact_cohort"]["fields"],
        &schema["consumer_compatibility_manifest"]["fields"],
    ] {
        for referenced in block
            .as_object()
            .ok_or("missing cohort or manifest field table")?
            .values()
        {
            let referenced = referenced.as_str().ok_or("non-string manifest type")?;
            if !declarations.contains_key(referenced) {
                return Err(format!(
                    "cohort/manifest references unresolved type {referenced}"
                ));
            }
        }
    }
    for version_type in schema["version_types"]
        .as_object()
        .ok_or("missing version type table")?
        .keys()
    {
        if !declarations.contains_key(version_type) {
            return Err(format!(
                "version table references unresolved type {version_type}"
            ));
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
    if schema["declarations"]["PublicEvent"]["fields"]
        != serde_json::json!([
            {"name":"derived_handler","type":"PublicDerivedHandlerShape","tag":3,"presence":"R"},
            {"name":"name","type":"String","tag":7,"presence":"R"},
            {"name":"signatures","type":"PublicCallSignature","tag":8,"presence":"L","min_items":1},
            {"name":"publication","type":"TypePublicationMeta","tag":9,"presence":"R"}
        ])
    {
        return Err("PublicEvent final field ledger drifted".into());
    }
    Ok(())
}

#[test]
fn scanners_replacement_schema_is_closed_and_ratified() {
    let schema = read_json("schemas/scanners-replacement-v1.schema.json");
    validate_frozen_schema(&schema).unwrap();

    let source =
        fs::read_to_string(workspace_root().join("schemas/scanners-replacement-v1.schema.json"))
            .expect("schema source")
            .to_ascii_lowercase();
    for forbidden in [
        "tbd",
        "infer by name",
        "implementation chooses",
        "representation-only",
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden schema marker {forbidden}"
        );
    }

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

    let mut missing_payload = schema.clone();
    missing_payload["declarations"]
        .as_object_mut()
        .unwrap()
        .remove("PublicationPolicyExactOnly");
    assert!(validate_frozen_schema(&missing_payload)
        .unwrap_err()
        .contains("PublicationPolicyExactOnly"));

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
        .push(serde_json::json!({"name":"PostCaptureFailure","number":7}));
    assert_eq!(
        validate_frozen_schema(&cross_phase).unwrap_err(),
        "pre-capture phase algebra drifted"
    );
}

#[test]
fn scanners_replacement_schema_declaration_deletion_is_rejected() {
    let mut schema = read_json("schemas/scanners-replacement-v1.schema.json");
    schema["declarations"]
        .as_object_mut()
        .unwrap()
        .remove("DocumentStructureResponseV1Available");
    assert!(validate_frozen_schema(&schema)
        .unwrap_err()
        .contains("DocumentStructureResponseV1Available"));
}

fn validate_stamped_block_content_contract(schema: &Value) -> Result<(), String> {
    let declarations = schema["declarations"]
        .as_object()
        .ok_or("missing declarations")?;
    let require_exact_record = |name: &str, expected_fields: Value| {
        let declaration = declarations
            .get(name)
            .ok_or_else(|| format!("missing {name} declaration"))?;
        if declaration["kind"] != "record" {
            return Err(format!("{name} is not a record"));
        }
        if declaration["fields"] != expected_fields {
            return Err(format!("{name} field ledger drifted"));
        }
        Ok(())
    };

    if declarations["BlockContentAvailabilityV1"]["kind"] != "enum"
        || declarations["BlockContentAvailabilityV1"]["values"]
            != serde_json::json!([
                {"name":"NativeAvailable","number":1},
                {"name":"ProcessedContentRequired","number":2},
                {"name":"SuppliedAvailable","number":3},
                {"name":"Missing","number":4},
                {"name":"Conflict","number":5},
                {"name":"Stale","number":6}
            ])
    {
        return Err("BlockContentAvailabilityV1 closed algebra drifted".into());
    }

    require_exact_record(
        "StampedBlockContentRequestV1",
        serde_json::json!([
            {"name":"correlation_token","type":"BlockContentCorrelationTokenV1","tag":1,"presence":"R"},
            {"name":"block_token","type":"ArtifactBlockTokenV1","tag":2,"presence":"R"},
            {"name":"owner_revision","type":"DocumentRevisionTokenV1","tag":3,"presence":"R"},
            {"name":"artifact_token","type":"FrameworkArtifactTokenV1","tag":4,"presence":"R"},
            {"name":"basis_token","type":"BlockContentBasisTokenV1","tag":5,"presence":"R"},
            {"name":"source_space_token","type":"SourceSpaceTokenV1","tag":6,"presence":"R"},
            {"name":"availability","type":"BlockContentAvailabilityV1","tag":7,"presence":"R"},
            {"name":"language","type":"ResolvedLanguageV1","tag":8,"presence":"R"},
            {"name":"content","type":"Utf8TextV1","tag":9,"presence":"R"},
            {"name":"content_hash","type":"ContentHashV1","tag":10,"presence":"R"},
            {"name":"prior_basis_token","type":"BlockContentBasisTokenV1","tag":11,"presence":"O"}
        ]),
    )?;
    require_exact_record(
        "StampedBlockContentResultV1",
        serde_json::json!([
            {"name":"correlation_token","type":"BlockContentCorrelationTokenV1","tag":1,"presence":"R"},
            {"name":"block_token","type":"ArtifactBlockTokenV1","tag":2,"presence":"R"},
            {"name":"owner_revision","type":"DocumentRevisionTokenV1","tag":3,"presence":"R"},
            {"name":"artifact_token","type":"FrameworkArtifactTokenV1","tag":4,"presence":"R"},
            {"name":"basis_token","type":"BlockContentBasisTokenV1","tag":5,"presence":"R"},
            {"name":"source_space_token","type":"SourceSpaceTokenV1","tag":6,"presence":"R"},
            {"name":"code","type":"Utf8TextV1","tag":7,"presence":"R"},
            {"name":"code_hash","type":"ContentHashV1","tag":8,"presence":"R"},
            {"name":"source_map","type":"Utf8TextV1","tag":9,"presence":"O"},
            {"name":"source_map_hash","type":"ContentHashV1","tag":10,"presence":"O"},
            {"name":"supplied_provenance","type":"BlockContentOriginFingerprintV1","tag":11,"presence":"O"},
            {"name":"expected_language","type":"ResolvedLanguageV1","tag":12,"presence":"R"},
            {"name":"prior_basis_token","type":"BlockContentBasisTokenV1","tag":13,"presence":"O"}
        ]),
    )?;
    require_exact_record(
        "BlockContentCapturedEchoV1",
        serde_json::json!([
            {"name":"request","type":"BlockContentPreCaptureEchoV1","tag":1,"presence":"R"},
            {"name":"basis_token","type":"BlockContentBasisTokenV1","tag":2,"presence":"R"}
        ]),
    )?;

    let captured_echo_field = serde_json::json!({
        "name":"echo",
        "type":"BlockContentCapturedEchoV1",
        "tag":1,
        "presence":"R"
    });
    for terminal in [
        "BlockContentResolveResponseV1Resolved",
        "BlockContentResolveResponseV1PostCaptureFailed",
        "BlockContentResolveResponseV1PostCaptureStaleWithReplacement",
        "BlockContentResolveResponseV1PostCaptureStaleNeedsRecapture",
        "BlockContentResolveResponseV1PostCaptureSuperseded",
        "BlockContentResolveResponseV1PostCaptureClosed",
        "BlockContentResolveResponseV1PostCaptureCancelled",
    ] {
        let fields = declarations
            .get(terminal)
            .and_then(|declaration| declaration["fields"].as_array())
            .ok_or_else(|| format!("missing {terminal} fields"))?;
        if fields.first() != Some(&captured_echo_field) {
            return Err(format!("{terminal} lost the captured echo"));
        }
    }
    Ok(())
}

#[test]
fn stamped_block_content_schema_is_additive_and_capture_complete() {
    let schema = read_json("schemas/scanners-replacement-v1.schema.json");
    validate_stamped_block_content_contract(&schema).unwrap();

    let mut missing_prior = schema.clone();
    missing_prior["declarations"]["StampedBlockContentRequestV1"]["fields"]
        .as_array_mut()
        .unwrap()
        .retain(|field| field["name"] != "prior_basis_token");
    assert_eq!(
        validate_stamped_block_content_contract(&missing_prior).unwrap_err(),
        "StampedBlockContentRequestV1 field ledger drifted"
    );

    let mut open_availability = schema.clone();
    open_availability["declarations"]["BlockContentAvailabilityV1"]["values"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"Unknown","number":7}));
    assert_eq!(
        validate_stamped_block_content_contract(&open_availability).unwrap_err(),
        "BlockContentAvailabilityV1 closed algebra drifted"
    );

    let mut unsealed_hash = schema.clone();
    unsealed_hash["declarations"]["StampedBlockContentResultV1"]["fields"][7]["type"] =
        Value::String("Utf8TextV1".into());
    assert_eq!(
        validate_stamped_block_content_contract(&unsealed_hash).unwrap_err(),
        "StampedBlockContentResultV1 field ledger drifted"
    );

    let mut missing_capture_basis = schema.clone();
    missing_capture_basis["declarations"]["BlockContentCapturedEchoV1"]["fields"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(
        validate_stamped_block_content_contract(&missing_capture_basis).unwrap_err(),
        "BlockContentCapturedEchoV1 field ledger drifted"
    );

    let mut pre_capture_terminal = schema;
    pre_capture_terminal["declarations"]["BlockContentResolveResponseV1PostCaptureClosed"]
        ["fields"][0]["type"] = Value::String("BlockContentPreCaptureEchoV1".into());
    assert_eq!(
        validate_stamped_block_content_contract(&pre_capture_terminal).unwrap_err(),
        "BlockContentResolveResponseV1PostCaptureClosed lost the captured echo"
    );
}
