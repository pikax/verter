//! Frozen contracts and barriers for the registered-carrier structural
//! authority.
//!
//! The always-on tests validate the ratified schema and capability ledger.
//! The feature-gated inverse tests remain RED until each final-state invariant
//! is implemented.

use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn run_workspace_command(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|error| panic!("failed to run {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("command output must be UTF-8")
}

fn independently_discovered_candidates() -> BTreeSet<(String, String)> {
    let tracked = run_workspace_command("git", &["ls-files"]);
    let metadata = run_workspace_command(
        "cargo",
        &[
            "metadata",
            "--format-version",
            "1",
            "--all-features",
            "--no-deps",
        ],
    );
    let metadata: Value = serde_json::from_str(&metadata).expect("cargo metadata JSON");
    assert!(metadata["packages"].as_array().is_some_and(|packages| {
        packages
            .iter()
            .any(|package| package["name"] == "verter_session")
    }));

    // These are the documented B0 rg families. Running the real probe proves
    // the selector is live; the same alternatives below classify its matches
    // at the agreed path/function aggregation granularity.
    let broad_probe = run_workspace_command(
        "rg",
        &[
            "--files-with-matches",
            "--glob",
            "*.{rs,ts,js,proto,json}",
            "scan_sfc_blocks|SfcBlock|customBlocks|applyBlockOverrides|findHtmlTagEnd|find_opening_tag_end|<style\\b|</script>|build_synthetic_source",
            "crates",
            "packages",
            "extensions",
        ],
    );
    assert!(
        !broad_probe.trim().is_empty(),
        "discovery rg selected no work"
    );

    let mut files = BTreeMap::new();
    for path in tracked.lines().map(|path| path.replace('\\', "/")) {
        if !(path.starts_with("crates/")
            || path.starts_with("packages/")
            || path.starts_with("extensions/")
            || path == "package.json")
        {
            continue;
        }
        if !path.ends_with(".rs")
            && !path.ends_with(".ts")
            && !path.ends_with(".js")
            && !path.ends_with(".proto")
            && !path.ends_with(".json")
        {
            continue;
        }
        if let Ok(source) = fs::read_to_string(workspace_root().join(&path)) {
            files.insert(path, source);
        }
    }

    let mut discovered = BTreeSet::new();
    // The named-symbol LSP probe is aggregated to one row per matching path;
    // test/support paths have a separate stable label. Independently named
    // manual-geometry helpers add function-level rows on the same path.
    let lsp_named = Regex::new(
        r"scan_sfc_blocks|SfcBlock|CustomBlockContentKind|find_balanced_close_tag|outer_template_content_span|find_close_tag_name",
    )
    .unwrap();
    for (path, source) in &files {
        if path.starts_with("crates/verter_lsp/src/") && lsp_named.is_match(source) {
            let symbol = if path.contains("_tests.rs")
                || path.ends_with("/tests.rs")
                || path.ends_with("/macro_fixture.rs")
                || path.ends_with("/integration_tests.rs")
                || path.contains("/real_provider_tests/")
            {
                "legacy_sfc_geometry_test_support"
            } else if path.ends_with("/documents/sfc_scanner.rs") {
                "scan_sfc_blocks_frontend"
            } else {
                "legacy_sfc_geometry_consumer"
            };
            discovered.insert((path.clone(), symbol.to_owned()));
        }
    }
    let vue_vscode_manifest = files
        .get("packages/vue-vscode/package.json")
        .expect("vue-vscode manifest");
    for target in [
        "darwin_arm64",
        "darwin_x64",
        "linux_arm64",
        "linux_x64",
        "win32_x64",
    ] {
        let manifest_spelling = target.replace('_', "-");
        assert!(vue_vscode_manifest.contains(&manifest_spelling));
        discovered.insert((
            "packages/vue-vscode/package.json".to_owned(),
            format!("vsix_bundle_{target}"),
        ));
    }
    let mut add_matches = |roots: &[&str], pattern: &str, symbol: &str| {
        let pattern = Regex::new(pattern).unwrap();
        for (path, source) in &files {
            if roots.iter().any(|root| path.starts_with(root)) && pattern.is_match(source) {
                discovered.insert((path.clone(), symbol.to_owned()));
            }
        }
    };
    add_matches(
        &["crates/verter_lsp/src/features/auto_close_tag.rs"],
        r"outer_template_content_span",
        "outer_template_content_span",
    );
    add_matches(
        &["crates/verter_lsp/src/features/linked_editing.rs"],
        r"fn find_close_tag_name",
        "find_close_tag_name",
    );
    add_matches(
        &["crates/verter_lsp/src/features/component_actions.rs"],
        r"fn find_opening_tag_end",
        "find_opening_tag_end",
    );
    add_matches(
        &["crates/verter_lsp/src/features/cursor_context.rs"],
        r"svelte_braced_attribute_from_tag_candidate",
        "complete_prefix_rfind_geometry",
    );
    add_matches(
        &["crates/verter_lsp/src/features/document_link.rs"],
        r"fn find_attribute_value_span",
        "raw_attribute_tokenizer",
    );
    add_matches(
        &["crates/verter_lsp/src/documents/analysis.rs"],
        r"\.upsert\(UpsertRequest",
        "semantic_host_raw_upsert",
    );

    // Direct definitions and path-level capability probes outside the LSP.
    add_matches(
        &["crates/verter_compiler/src/compile/"],
        r"pub fn compile\(",
        "compile",
    );
    add_matches(
        &["crates/verter_ffi/src/convert/component_meta.rs"],
        r"sfc_blocks",
        "sfc_blocks projection",
    );
    add_matches(
        &[
            "crates/verter_mcp/src/scanner.rs",
            "packages/unplugin/src/core/scanner.ts",
        ],
        r"WalkDir|read_dir|readdir",
        "filesystem_candidate_scanner",
    );
    add_matches(
        &["crates/verter_parser/src/cursor/"],
        r"pub struct ScriptDetector",
        "ScriptDetector",
    );
    add_matches(
        &["crates/verter_protocol/proto/verter/v1/"],
        r"SfcBlocksMeta",
        "SfcBlocksMeta/sfc_blocks",
    );
    add_matches(
        &["crates/verter_protocol/src/component_meta.rs"],
        r"fn sfc_blocks_to_proto",
        "sfc_blocks_to_proto",
    );
    add_matches(
        &["crates/verter_protocol/src/types.rs"],
        r"struct FfiSfcBlocksMeta",
        "FfiSfcBlocksMeta",
    );
    add_matches(
        &["crates/verter_semantic/src/analysis/component_meta.rs"],
        r"struct SfcBlocksAnalysis",
        "SfcBlocksAnalysis",
    );
    add_matches(
        &["crates/verter_semantic/src/analysis/types.rs"],
        r"TopLevelOwnerId::.*ordinal",
        "ordinal_block_dtos",
    );

    add_matches(
        &["crates/verter_session/src/compile.rs"],
        r"merge_external_sources|assemble_vue_main_module",
        "registered_raw_compile",
    );
    add_matches(
        &["crates/verter_session/src/host_compile_audit.rs"],
        r"compile as compile_sfc",
        "direct_audited_compile_sfc",
    );
    add_matches(
        &["crates/verter_session/src/host_manage/analysis_io.rs"],
        r"build_snapshot_from_scheduler",
        "source_only_analysis_artifact_builder",
    );
    add_matches(
        &["crates/verter_session/src/host_manage/eval_program.rs"],
        r"NO parse artifact|no parse artifact",
        "no_artifact_raw_fallback",
    );
    add_matches(
        &["crates/verter_session/src/host_manage/overlay_materialize.rs"],
        r"build_carrier_parse_artifact_from_source",
        "source_only_overlay_artifact_builder",
    );
    add_matches(
        &["crates/verter_session/src/host_manage/prepared_decl.rs"],
        r"ensure_indexed_ready",
        "source_only_prepared_artifact_builder",
    );
    add_matches(
        &[
            "crates/verter_session/src/host_resolve/virtual_file_pipeline.rs",
            "crates/verter_session/src/parse.rs",
        ],
        r"external_source|src_block|synthetic",
        "external_source_synthetic_parse",
    );
    add_matches(
        &["crates/verter_session/src/host_resolve/vue_script_extract.rs"],
        r"fn find_script_close_outside_js_context",
        "find_script_close_outside_js_context",
    );
    add_matches(
        &["crates/verter_session/src/host_resolve/vue_script_extract.rs"],
        r"fn populate_sfc_blocks_sidecar",
        "populate_sfc_blocks_sidecar",
    );
    add_matches(
        &["crates/verter_session/src/host_resolve/vue_script_extract.rs"],
        r"fn script_content_spans_from_source",
        "script_content_spans_from_source",
    );
    add_matches(
        &["crates/verter_session/src/host_upsert.rs"],
        r"applyBlockOverrides",
        "applyBlockOverrides",
    );
    add_matches(
        &["crates/verter_session/src/host_upsert.rs"],
        r"apply_style_overrides|</style>",
        "manual_style_close_search",
    );
    add_matches(
        &["crates/verter_session/src/host_upsert/block_splice.rs"],
        r"fn build_synthetic_source",
        "build_synthetic_source",
    );
    add_matches(
        &["crates/verter_session/src/types.rs"],
        r"struct ContentOverrideWithParse",
        "ContentOverrideWithParse",
    );
    add_matches(
        &[
            "crates/verter_wasm/src/lib.rs",
            "packages/native/index.js",
            "packages/native/index.ts",
            "packages/wasm/src/index.ts",
        ],
        r"applyBlockOverrides",
        "applyBlockOverrides",
    );

    add_matches(
        &["extensions/typescript-plugin/index.js"],
        r"descriptor\.customBlocks",
        "legacy_bundled_typescript_plugin",
    );
    add_matches(
        &["extensions/vscode/package.json"],
        r"typescript-plugin",
        "legacy_extension_packaging_root",
    );
    add_matches(
        &["package.json"],
        r"copy.*typescript-plugin|typescript-plugin.*copy",
        "copy-plugin/bare-package",
    );
    add_matches(
        &["packages/component-meta/src/native-component-meta.ts"],
        r"NativeSfcBlocksMeta",
        "NativeSfcBlocksMeta",
    );
    add_matches(
        &["packages/component-meta/src/type-graph-proto-decode.ts"],
        r"decodeOptionalSfcBlocks",
        "decodeOptionalSfcBlocks",
    );
    add_matches(
        &["packages/component-meta/src/types.ts"],
        r"interface SfcBlocksMeta|type SfcBlocksMeta",
        "SfcBlocksMeta",
    );
    add_matches(
        &["packages/proto/src/gen/verter/v1/"],
        r"SfcBlocksMeta",
        "SfcBlocksMeta generated binding",
    );

    add_matches(
        &["packages/playground/src/core/compiler.ts"],
        r"parse\(|template|customBlocks",
        "carrier_root_scanner",
    );
    add_matches(
        &["packages/playground/src/core/sourcemap.ts"],
        r#"indexOf\("<template"#,
        "template_offset_scan",
    );
    add_matches(
        &["packages/playground/src/core/types.ts"],
        r"interface FileAnalysis",
        "ordinal_block_projection",
    );
    add_matches(
        &["packages/playground/src/editor/analysisHelpers.ts"],
        r#"indexOf\("</script>"#,
        "script_close_geometry",
    );
    add_matches(
        &["packages/playground/src/editor/decorations.ts"],
        r"<template\\b|<style\\b",
        "template_style_geometry",
    );
    add_matches(
        &["packages/playground/src/editor/templateIde.ts"],
        r"<template|</script>",
        "template_script_geometry",
    );

    for symbol in [
        "asciiEqualsAt",
        "findHtmlTagEnd",
        "isFrameworkAttributeNamePosition",
        "isHtmlTagBoundary",
        "isInsideSfcScript",
        "sfcScriptImportAnchor",
    ] {
        add_matches(
            &["packages/typescript-plugin/src/index.ts"],
            &format!(r"(?:function|const)\s+{symbol}\b"),
            symbol,
        );
    }
    add_matches(
        &["packages/unplugin/src/core/preprocessor.ts"],
        r"BlockPreprocessor|processor\(",
        "arbitrary_callback_executor",
    );
    add_matches(
        &["packages/unplugin/src/core/types.ts"],
        r"BlockPreprocessor",
        "BlockPreprocessor",
    );
    add_matches(
        &["packages/unplugin/src/core/types.ts"],
        r"customBlocks",
        "customBlocks",
    );
    add_matches(
        &["packages/unplugin/src/index.ts"],
        r"compiler\.parse|\.parse\(",
        "compiler.parse_after_host_upsert",
    );
    add_matches(
        &["packages/unplugin/src/index.ts"],
        r"/<style\\b|styleRe",
        "style_regex_scan",
    );
    add_matches(
        &["packages/vue-vscode/src/css/styleBlockScanner.ts"],
        r"function scanStyleBlocks",
        "scanStyleBlocks",
    );
    add_matches(
        &[
            "packages/vue-vscode/src/css/cssService.ts",
            "packages/vue-vscode/src/extension.ts",
        ],
        r"scanStyleBlocks|styleBlockScanner",
        "styleBlockScanner consumer",
    );
    add_matches(
        &["packages/wasm/src/index.ts"],
        r"export async function compile",
        "raw_source_compile",
    );

    discovered
}

fn validate_ledger_discovery_equality(
    ledger: &Value,
    discovered: &BTreeSet<(String, String)>,
) -> Result<(), String> {
    let rows = ledger["rows"].as_array().ok_or("ledger rows")?;
    if ledger["set_equality"]["independently_discovered_candidates"] != rows.len()
        || ledger["set_equality"]["classified_ledger_rows"] != rows.len()
    {
        return Err("ledger count attestation drifted from rows".into());
    }
    let identities = rows
        .iter()
        .map(|row| {
            Ok((
                row["path"].as_str().ok_or("row path")?.to_owned(),
                row["symbol"].as_str().ok_or("row symbol")?.to_owned(),
            ))
        })
        .collect::<Result<BTreeSet<_>, &str>>()
        .map_err(str::to_owned)?;
    if &identities != discovered {
        return Err("independent candidate set drifted".into());
    }
    Ok(())
}

#[test]
fn scanners_replacement_capability_ledger_deleted_row_is_rejected() {
    let mut ledger = read_json("docs/arch/scanners-replacement-capability-ledger.json");
    let remaining = {
        let rows = ledger["rows"].as_array_mut().expect("ledger rows");
        let position = rows
            .iter()
            .position(|row| row["symbol"] == "sfc_blocks projection")
            .expect("non-seed discrimination row");
        rows.remove(position);
        rows.len()
    };
    ledger["set_equality"]["independently_discovered_candidates"] = remaining.into();
    ledger["set_equality"]["classified_ledger_rows"] = remaining.into();
    assert_eq!(
        validate_ledger_discovery_equality(&ledger, &independently_discovered_candidates()),
        Err("independent candidate set drifted".into())
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
        ledger["set_equality"]["independently_discovered_candidates"],
        rows.len()
    );
    assert_eq!(ledger["set_equality"]["classified_ledger_rows"], rows.len());
    let discovered = independently_discovered_candidates();
    let ledger_identities = identities
        .into_iter()
        .map(|(path, symbol)| (path.to_owned(), symbol.to_owned()))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        discovered, ledger_identities,
        "independent candidate set drifted"
    );
    validate_ledger_discovery_equality(&ledger, &discovered).unwrap();
    assert_eq!(
        ledger["consumer_matrix"].as_array().map(Vec::len),
        Some(
            rows.iter()
                .filter(|row| row["runtime_role"] == "production_runtime")
                .count()
        )
    );
}
