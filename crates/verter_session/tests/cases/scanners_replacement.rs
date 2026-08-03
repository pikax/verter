//! Frozen contracts and barriers for the registered-carrier structural
//! authority.
//!
//! The always-on tests validate the ratified schema and capability ledger.
//! Architecture enforcement lives in construction boundaries and compile-fail
//! tests, never source-name scanners.

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
    "BlockContentBasisTokenV1",
    "BlockContentCapturedEchoV1",
    "BlockContentDependencyReadEchoV1",
    "BlockContentDependencyReadRequestV1",
    "BlockContentDependencyReadResponseV1",
    "BlockContentDependencyReadResponseV1BudgetExceeded",
    "BlockContentDependencyReadResponseV1Cancelled",
    "BlockContentDependencyReadResponseV1CorrelationRejected",
    "BlockContentDependencyReadResponseV1Cycle",
    "BlockContentDependencyReadResponseV1Failed",
    "BlockContentDependencyReadResponseV1NotFound",
    "BlockContentDependencyReadResponseV1Resolved",
    "BlockContentDependencyReadResponseV1ScopeDenied",
    "BlockContentDependencyReadResponseV1Stale",
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
    "BlockContentWorkTokenV1",
    "BlockRecoveryReasonV1",
    "CacheClusterSchemaVersion",
    "CanonicalConfigBytesV1",
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
    "DeniedCapabilityV1",
    "DependencyFingerprintV1",
    "DependencyReadBudgetKindV1",
    "DependencyReadFactOutcomeV1",
    "DependencyReadFactOutcomeV1Missing",
    "DependencyReadFactOutcomeV1Resolved",
    "DependencyReadFactV1",
    "DependencyReadFailureV1",
    "DependencyReadKindV1",
    "DependencyReadNegativeReasonV1",
    "DependencyReadOutcomeKindV1",
    "DependencyReadSetV1",
    "DependencyReadTerminalCauseV1",
    "DependencyRequestCorrelationAuditEventV1",
    "DependencyRequestCorrelationAuditEventV1Consumed",
    "DependencyRequestCorrelationAuditEventV1OutstandingDestroyed",
    "DependencyRequestCorrelationAuditEventV1Pending",
    "DependencyRequestCorrelationAuditEventV1Rejected",
    "DependencyRequestCorrelationRejectV1",
    "DependencyRequestIdV1",
    "DependencyResolutionProvenanceV1",
    "DependencyResolverKindV1",
    "DependencyTokenV1",
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
    "DocumentStructureResponseV1StaleClientDocument",
    "DocumentStructureResponseV1Unavailable",
    "DocumentStructureV1",
    "DocumentUnavailableReasonV1",
    "DocumentUriV1",
    "DynamicModuleAuthorityV1",
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
    "PreprocessorIdentityV1",
    "PreprocessorStepV1",
    "PreprocessorWorkSpecV1",
    "ProcessorBrokerInstanceTokenV1",
    "ProcessorSandboxKindV1",
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
    "TransformChainFingerprintV1",
    "TrustedBrokerChannelBindingV1",
    "TrustedBrokerProcessingFailureV1",
    "TrustedBrokerWorkEchoV1",
    "TrustedBrokerWorkRequestV1",
    "TrustedBrokerWorkResultV1",
    "TrustedBrokerWorkResultV1Cancelled",
    "TrustedBrokerWorkResultV1Failed",
    "TrustedBrokerWorkResultV1Success",
    "TrustedProcessorAttestationV1",
    "TrustedProcessorCapabilityManifestV1",
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
    "WorkerRequestNonceV1",
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

    assert_eq!(ledger["statistics"]["rows_total"], rows.len());
    assert_eq!(
        ledger["statistics"]["by_capability_class"],
        serde_json::to_value(counts).unwrap()
    );
    let mut dispositions = BTreeMap::<String, usize>::new();
    for row in rows {
        *dispositions
            .entry(row["disposition"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }
    assert_eq!(
        ledger["statistics"]["by_disposition"],
        serde_json::to_value(dispositions).unwrap()
    );
    assert_eq!(ledger["set_equality"]["unclassified_runtime_rows"], 0);
    assert_eq!(ledger["set_equality"]["deferred_runtime_rows"], 0);
    // RETRACTED self-attestation: `independently_discovered_candidates ==
    // rows.len()` asserted the ledger's own row count as the discovered count
    // — a receipt of nothing. The B-52/B-91 evidence is the EXTERNAL
    // input-bound discovery receipt this record must reference; this suite
    // checks only the RECORD's shape (fresh run + retraction + honest
    // reopened status), never a self-derived equality.
    let fresh_run = &ledger["discovery"]["fresh_run"];
    assert!(
        fresh_run["receipt"]
            .as_str()
            .is_some_and(|receipt| receipt.contains("DISCOVERY-RECEIPT.md")),
        "the discovery record must name its input-bound receipt"
    );
    assert!(
        fresh_run["fixed_tip"]
            .as_str()
            .is_some_and(|tip| tip.len() == 40 && tip.chars().all(|c| c.is_ascii_hexdigit())),
        "the discovery record must pin the fixed tip it ran against"
    );
    for input in ["git_ls_files", "cargo_metadata", "pnpm_workspace_graph"] {
        assert!(
            fresh_run["inputs"][input]["sha256"]
                .as_str()
                .is_some_and(|hash| !hash.is_empty()),
            "the discovery record must carry an input hash for {input}"
        );
    }
    assert!(
        ledger["discovery"]["retraction"]
            .as_str()
            .is_some_and(|text| text.contains("RETRACTED")),
        "the retraction of the prior self-attested closure must stay recorded"
    );
    let open_residuals = ledger["set_equality"]["open_residual_migrate_rows"]
        .as_u64()
        .expect("open_residual_migrate_rows must be a recorded count");
    let b52_status = ledger["set_equality"]["b52_b91_status"]
        .as_str()
        .expect("b52_b91_status must be recorded");
    if open_residuals == 0 {
        assert!(
            b52_status.contains("CLOSED")
                && !b52_status.contains("REOPENED")
                && b52_status.contains("DISCOVERY-RECEIPT.md"),
            "an empty residual set closes B-52/B-91 citing the input-bound receipt"
        );
    } else {
        assert!(
            b52_status.contains("REOPENED"),
            "B-52/B-91 must remain explicitly reopened while any named residual is open"
        );
    }
    assert_eq!(
        ledger["consumer_matrix"].as_array().map(Vec::len),
        Some(
            rows.iter()
                .filter(|row| row["runtime_role"] == "production_runtime")
                .count()
        )
    );
}

#[test]
fn scanners_replacement_b_78_names_exactly_five_inspection_rows() {
    let ledger = read_json("docs/arch/scanners-replacement-capability-ledger.json");
    let targets = ledger["rows"]
        .as_array()
        .expect("ledger rows")
        .iter()
        .filter(|row| row["acceptance_id"] == "B-78")
        .map(|row| {
            assert_eq!(row["test"], "fresh_vsix_scanner_inventory_inspection");
            row["symbol"].as_str().expect("B-78 symbol").to_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        targets,
        BTreeSet::from([
            "vsix_bundle_darwin_arm64".to_owned(),
            "vsix_bundle_darwin_x64".to_owned(),
            "vsix_bundle_linux_arm64".to_owned(),
            "vsix_bundle_linux_x64".to_owned(),
            "vsix_bundle_win32_x64".to_owned(),
        ])
    );

    let extension = read_json("packages/vue-vscode/package.json");
    let package_targets = extension["scripts"]
        .as_object()
        .expect("extension scripts")
        .keys()
        .filter_map(|script| script.strip_prefix("package:"))
        .filter(|target| *target != "dev:universal")
        .map(|target| format!("vsix_bundle_{}", target.replace('-', "_")))
        .collect::<BTreeSet<_>>();
    assert_eq!(targets, package_targets);
}

#[test]
fn scanners_replacement_b_70_has_single_extension_authority() {
    let workspace = workspace_root();

    // Single-extension authority (B-70): the ONLY live `extensions/*` trees
    // are the editor extensions below. Every retired authority must be gone
    // BOTH physically and from git tracking — the live VS Code extension is
    // `packages/vue-vscode` and the live TypeScript plugin is
    // `packages/typescript-plugin`; nothing under `extensions/` may shadow
    // them.
    const LIVE_EXTENSIONS: &[&str] = &["extensions/lapce", "extensions/zed"];
    const RETIRED_EXTENSIONS: &[&str] = &[
        "extensions/vscode",
        "extensions/typescript-plugin",
        "extensions/vue-vscode",
    ];

    // PHYSICAL arm: no retired extension tree may exist on disk.
    for retired in RETIRED_EXTENSIONS {
        assert!(
            !workspace.join(retired).exists(),
            "legacy {retired} authority must be physically deleted"
        );
    }

    // TRACKED arm: every tracked path under extensions/ must belong to a live
    // extension.
    let tracked = std::process::Command::new("git")
        .args(["ls-files", "-z", "--", "extensions"])
        .current_dir(&workspace)
        .output()
        .expect("git ls-files for the extensions tree");
    assert!(tracked.status.success(), "git ls-files failed");
    let tracked = String::from_utf8(tracked.stdout).expect("tracked paths are utf-8");
    for path in tracked.split('\0').filter(|path| !path.is_empty()) {
        let path = path.replace('\\', "/");
        assert!(
            LIVE_EXTENSIONS
                .iter()
                .any(|live| path.starts_with(&format!("{live}/"))),
            "tracked extensions residue outside the live allowlist: {path}"
        );
    }

    for entry in walkdir::WalkDir::new(&workspace)
        .into_iter()
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some("node_modules" | "target" | ".git" | ".integration-tests")
            )
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() == "package.json")
    {
        let source = fs::read_to_string(entry.path())
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", entry.path().display()));
        let normalized = source.replace("\\\\", "/");
        for retired in RETIRED_EXTENSIONS {
            assert!(
                !normalized.contains(retired),
                "{} still references the retired {retired} authority",
                entry.path().display()
            );
        }
    }

    for relative in [
        "packages/playground/scripts/generate-vue-language.ts",
        "packages/playground/src/editor/vueLanguage.ts",
    ] {
        let source =
            fs::read_to_string(workspace.join(relative)).expect("playground language source");
        assert!(
            source.contains("packages/vue-vscode"),
            "{relative} must name packages/vue-vscode as grammar authority"
        );
        for retired in RETIRED_EXTENSIONS {
            assert!(
                !source.contains(retired),
                "{relative} still names the retired {retired} authority"
            );
        }
    }
}
