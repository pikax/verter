//! Host-owned native and supplied carrier-block content admission.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use verter_language::parse_artifact::carrier_inventory::{
    AttributeValue, CarrierAttribute, CarrierBlock, SectionRole, SourceSlice, StyleDialect,
    TaggedSyntax,
};

use crate::hash::compile_profile_hash;
use crate::host_executor::HostSourceData;
use crate::shared::{default_shared, read_lock, write_lock, Shared};
use crate::types::*;
use crate::VerterHost;

/// Host-owned block-content lane: the admission state, the fence that
/// serializes validation and atomic admission after asynchronous provider
/// work (docs/arch/scanners-replacement-preprocessor-interim.md §Sealed
/// handoff), the correlation counter, and the test-only admission seam
/// hook, grouped so the root `VerterHost` struct stays thin. NOT a cache;
/// admitted artifacts live in [`BlockContentState`].
pub(crate) struct BlockContentHostLane {
    pub(crate) state: Shared<BlockContentState>,
    pub(crate) admission_fence: parking_lot::Mutex<()>,
    pub(crate) correlation_counter: std::sync::atomic::AtomicU64,
    /// Test-only seam fired after validation and before publication for
    /// deterministic owner-publication races. **Compiled out in
    /// production builds.**
    #[cfg(test)]
    pub(crate) admission_seam_hook:
        parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
}

impl Default for BlockContentHostLane {
    fn default() -> Self {
        Self {
            state: default_shared(BlockContentState::default()),
            admission_fence: parking_lot::Mutex::new(()),
            correlation_counter: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            admission_seam_hook: parking_lot::Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingCorrelation {
    canonical_id: String,
    block_token: crate::carrier_publication_store::ArtifactBlockToken,
    owner_revision: BlockContentOwnerRevisionToken,
    artifact_token: crate::carrier_publication_store::FrameworkArtifactToken,
    basis_token: BlockContentBasisToken,
    source_space_token: BlockContentSourceSpaceToken,
    content_hash: BlockContentHashToken,
    input_content: Arc<str>,
    captured_echo: BlockContentCapturedEcho,
}

#[derive(Debug, Clone)]
pub(crate) struct SuppliedContentArtifact {
    owner_revision: BlockContentOwnerRevisionToken,
    artifact_token: crate::carrier_publication_store::FrameworkArtifactToken,
    basis_token: BlockContentBasisToken,
    input_source_space_token: BlockContentSourceSpaceToken,
    output_source_space_token: BlockContentSourceSpaceToken,
    content_artifact_token: BlockContentArtifactToken,
    output_descriptor: BlockContentSourceSpaceDescriptor,
    qualified_source_map: QualifiedBlockContentSourceMap,
    code: Arc<str>,
    code_hash: BlockContentHashToken,
    source_map: Option<Arc<str>>,
    source_map_hash: Option<BlockContentHashToken>,
    dependencies: Vec<String>,
    diagnostics: Vec<PreprocessorDiagnostic>,
    processor_identity: String,
    processor_version: String,
    config_fingerprint: Option<BlockContentHashToken>,
}

impl SuppliedContentArtifact {
    fn retained_bytes(&self) -> usize {
        self.code
            .len()
            .saturating_add(self.source_map.as_deref().map_or(0, str::len))
            .saturating_add(self.processor_identity.len())
            .saturating_add(self.processor_version.len())
            .saturating_add(self.dependencies.iter().map(String::len).sum::<usize>())
            .saturating_add(
                self.diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.len())
                    .sum::<usize>(),
            )
    }
}

type SuppliedContentKey = (
    String,
    u64,
    crate::carrier_publication_store::ArtifactBlockToken,
);

#[derive(Debug, Default)]
pub(crate) struct BlockContentState {
    pending: HashMap<BlockContentCorrelationToken, PendingCorrelation>,
    pending_order: VecDeque<BlockContentCorrelationToken>,
    terminal: HashMap<BlockContentCorrelationToken, BlockContentResolveTerminal>,
    terminal_order: VecDeque<BlockContentCorrelationToken>,
    supplied: HashMap<SuppliedContentKey, SuppliedContentArtifact>,
    supplied_order: VecDeque<SuppliedContentKey>,
    supplied_bytes: usize,
}

const MAX_PENDING_CORRELATIONS: usize = 4_096;
const MAX_TERMINAL_CORRELATIONS: usize = 4_096;
const MAX_SEALED_TOKEN_BYTES: usize = 256;
const MAX_SUPPLIED_CODE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SUPPLIED_MAP_BYTES: usize = 8 * 1024 * 1024;
const MAX_SUPPLIED_PROVENANCE_BYTES: usize = 4 * 1024;
const MAX_SUPPLIED_ARTIFACTS: usize = 4_096;
const MAX_SUPPLIED_TOTAL_BYTES: usize = 64 * 1024 * 1024;

impl BlockContentState {
    fn mark_terminal(
        &mut self,
        token: BlockContentCorrelationToken,
        terminal: BlockContentResolveTerminal,
    ) {
        if self.terminal.insert(token.clone(), terminal).is_none() {
            self.terminal_order.push_back(token);
        }
        while self.terminal_order.len() > MAX_TERMINAL_CORRELATIONS {
            if let Some(expired) = self.terminal_order.pop_front() {
                self.terminal.remove(&expired);
            }
        }
    }

    fn remove_pending(
        &mut self,
        token: &BlockContentCorrelationToken,
    ) -> Option<PendingCorrelation> {
        let pending = self.pending.remove(token);
        if pending.is_some() {
            self.pending_order.retain(|queued| queued != token);
        }
        pending
    }

    fn insert_pending(&mut self, token: BlockContentCorrelationToken, pending: PendingCorrelation) {
        while self.pending.len() >= MAX_PENDING_CORRELATIONS {
            let Some(expired) = self.pending_order.pop_front() else {
                break;
            };
            if let Some(pending) = self.pending.remove(&expired) {
                self.mark_terminal(
                    expired,
                    BlockContentResolveTerminal::PostCapture {
                        echo: pending.captured_echo,
                        outcome: BlockContentPostCaptureTerminal::Cancelled,
                    },
                );
            }
        }
        self.pending_order.push_back(token.clone());
        self.pending.insert(token, pending);
    }

    fn insert_supplied(&mut self, key: SuppliedContentKey, artifact: SuppliedContentArtifact) {
        if let Some(previous) = self.supplied.remove(&key) {
            self.supplied_bytes = self
                .supplied_bytes
                .saturating_sub(previous.retained_bytes());
            self.supplied_order.retain(|queued| queued != &key);
        }
        self.supplied_bytes = self
            .supplied_bytes
            .saturating_add(artifact.retained_bytes());
        self.supplied_order.push_back(key.clone());
        self.supplied.insert(key, artifact);
        // High-water mark of the store's live retained footprint.
        verter_audit::attribute_max!(StoreRetainedBytes, self.supplied_bytes);

        while self.supplied.len() > MAX_SUPPLIED_ARTIFACTS
            || self.supplied_bytes > MAX_SUPPLIED_TOTAL_BYTES
        {
            let Some(expired) = self.supplied_order.pop_front() else {
                break;
            };
            if let Some(artifact) = self.supplied.remove(&expired) {
                self.supplied_bytes = self
                    .supplied_bytes
                    .saturating_sub(artifact.retained_bytes());
            }
        }
    }

    fn remove_supplied_owner(&mut self, canonical_id: &str) {
        self.supplied
            .retain(|(owner, _, _), _| owner != canonical_id);
        self.supplied_order
            .retain(|(owner, _, _)| owner != canonical_id);
        self.supplied_bytes = self
            .supplied
            .values()
            .map(SuppliedContentArtifact::retained_bytes)
            .sum();
    }

    pub(crate) fn supersede_owner(&mut self, canonical_id: &str) {
        let correlations = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.canonical_id == canonical_id)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in correlations {
            if let Some(pending) = self.remove_pending(&token) {
                self.mark_terminal(
                    token,
                    BlockContentResolveTerminal::PostCapture {
                        echo: pending.captured_echo,
                        outcome: BlockContentPostCaptureTerminal::Superseded,
                    },
                );
            }
        }
        self.remove_supplied_owner(canonical_id);
    }

    pub(crate) fn close_owner(&mut self, canonical_id: &str) {
        let correlations = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.canonical_id == canonical_id)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in correlations {
            if let Some(pending) = self.remove_pending(&token) {
                self.mark_terminal(
                    token,
                    BlockContentResolveTerminal::PostCapture {
                        echo: pending.captured_echo,
                        outcome: BlockContentPostCaptureTerminal::Closed,
                    },
                );
            }
        }
        self.remove_supplied_owner(canonical_id);
    }

    pub(crate) fn close_all(&mut self) {
        let pending = self.pending.drain().collect::<Vec<_>>();
        self.pending_order.clear();
        for (token, pending) in pending {
            self.mark_terminal(
                token,
                BlockContentResolveTerminal::PostCapture {
                    echo: pending.captured_echo,
                    outcome: BlockContentPostCaptureTerminal::Closed,
                },
            );
        }
        self.supplied.clear();
        self.supplied_order.clear();
        self.supplied_bytes = 0;
    }
}

#[derive(Clone)]
struct SelectedBlock {
    content_class: BlockContentClass,
    lang: String,
    block_token: crate::carrier_publication_store::ArtifactBlockToken,
    owner_revision: BlockContentOwnerRevisionToken,
    artifact_token: crate::carrier_publication_store::FrameworkArtifactToken,
    source_space_token: BlockContentSourceSpaceToken,
    content_artifact_token: BlockContentArtifactToken,
    source_descriptor: BlockContentSourceSpaceDescriptor,
    basis_token: BlockContentBasisToken,
    content_hash: Option<BlockContentHashToken>,
    authored_origin: Option<BlockContentOrigin>,
    authored_content: Option<Arc<str>>,
    availability: BlockContentAvailability,
}

#[derive(Debug, Clone)]
pub(crate) struct CompilerBlockContentCapture {
    pub(crate) has_supplied: bool,
    pub(crate) inputs: verter_compiler::framework_common::RuntimeBlockContentInputs,
    pub(crate) stamp: BlockContentHashToken,
}

pub(crate) struct CompilerStyleContentCapture {
    pub(crate) analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>,
    pub(crate) v_bind_vars: Vec<String>,
    pub(crate) usage_complete: bool,
    analyses_changed: bool,
}

pub fn hash_block_content(value: &str) -> BlockContentHashToken {
    BlockContentHashToken::mint(hash_parts(&[
        b"verter.block-content.bytes.v1\0",
        value.as_bytes(),
    ]))
}

fn hash_parts(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update(part);
    }
    let mut out = String::with_capacity(71);
    out.push_str("sha256:");
    for byte in digest.finalize() {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn push_stamp_part(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn push_runtime_block_input(
    bytes: &mut Vec<u8>,
    input: Option<&verter_compiler::framework_common::RuntimeBlockContentInput>,
) {
    let Some(input) = input else {
        bytes.push(0);
        return;
    };
    bytes.push(1);
    push_stamp_part(bytes, &input.content_artifact_token);
    push_stamp_part(bytes, &input.source_space_token);
    push_stamp_part(bytes, &input.lang);
}

fn compiler_block_content_stamp(
    has_supplied: bool,
    inputs: &verter_compiler::framework_common::RuntimeBlockContentInputs,
) -> BlockContentHashToken {
    let mut bytes = Vec::new();
    bytes.push(u8::from(has_supplied));
    push_runtime_block_input(&mut bytes, inputs.template.as_ref());
    push_runtime_block_input(&mut bytes, inputs.script.as_ref());
    push_runtime_block_input(&mut bytes, inputs.script_setup.as_ref());
    bytes.extend_from_slice(&(inputs.styles.len() as u64).to_le_bytes());
    for input in &inputs.styles {
        push_runtime_block_input(&mut bytes, input.as_ref());
    }
    bytes.extend_from_slice(&(inputs.custom_blocks.len() as u64).to_le_bytes());
    for input in &inputs.custom_blocks {
        push_runtime_block_input(&mut bytes, input.as_ref());
    }
    BlockContentHashToken::mint(hash_parts(&[
        b"verter.block-content.compile-capture.v1\0",
        &bytes,
    ]))
}

fn valid_sealed_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_SEALED_TOKEN_BYTES
        && !token.chars().any(char::is_control)
}

fn position_within_utf16_content(content: &str, line: u32, column: u32) -> bool {
    content
        .split('\n')
        .nth(line as usize)
        .is_some_and(|value| column as usize <= value.encode_utf16().count())
}

fn valid_source_map_v3(map: &str, input_content: &str, output_content: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(map) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(sources) = object.get("sources").and_then(serde_json::Value::as_array) else {
        return false;
    };
    if object.get("version").and_then(serde_json::Value::as_u64) != Some(3)
        || sources.len() != 1
        || !sources.iter().all(serde_json::Value::is_string)
        || !object
            .get("names")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| items.iter().all(serde_json::Value::is_string))
        || !object
            .get("mappings")
            .is_some_and(serde_json::Value::is_string)
        // A `sections` member marks an INDEXED source map (the V3 spec's
        // alternate top-level shape: a list of offset-anchored sub-maps
        // instead of a single flat `mappings` stream). `oxc_sourcemap`'s
        // decoder has no notion of `sections` and silently ignores it as an
        // unknown field, decoding whatever flat `sources`/`mappings` happen
        // to sit alongside it — so without this check a supplied indexed map
        // would decode as if the `sections` payload never existed instead of
        // being rejected. This function accepts only a flat regular map, so
        // `sections` presence (regardless of content, including `[]`) is
        // rejected up front.
        || object.contains_key("sections")
        // `rangeMappings` is a non-standard Sentry extension neither this
        // codebase nor `oxc_sourcemap` produces or reads. `oxc_sourcemap`
        // silently ignores it as an unknown field, but the prior validator
        // (built on `sourcemap` 9.3) decoded and VLQ-validated its content,
        // rejecting the whole map when that content was malformed. Without
        // this check a supplied map carrying a malformed `rangeMappings`
        // string would silently pass instead of being rejected — reject its
        // presence up front so this function's accept set never depends on
        // content it does not itself validate.
        || object.contains_key("rangeMappings")
    {
        return false;
    }
    if let Some(contents) = object.get("sourcesContent") {
        let Some(contents) = contents.as_array() else {
            return false;
        };
        if contents.len() != 1
            || !contents
                .iter()
                .all(|entry| entry.is_null() || entry.is_string())
            || contents[0]
                .as_str()
                .is_some_and(|declared| declared != input_content)
        {
            return false;
        }
    }
    // JSON shape alone is insufficient: decoding also validates the VLQ
    // mapping stream and rejects structurally shaped but unusable maps.
    let Ok(decoded) = oxc_sourcemap::SourceMap::from_json_string(map) else {
        return false;
    };
    decoded.get_sources().count() == 1
        && decoded.get_tokens().all(|token| {
            position_within_utf16_content(output_content, token.get_dst_line(), token.get_dst_col())
                && (token.get_source_id().is_none()
                    || (token.get_source_id() == Some(0)
                        && position_within_utf16_content(
                            input_content,
                            token.get_src_line(),
                            token.get_src_col(),
                        )))
        })
}

fn identity_qualified_map(
    source_space_token: &BlockContentSourceSpaceToken,
) -> QualifiedBlockContentSourceMap {
    QualifiedBlockContentSourceMap {
        map_hash: BlockContentHashToken::mint(hash_parts(&[
            b"verter.block-content.qualified-map.identity.v1\0",
            source_space_token.as_bytes(),
        ])),
        destination_space_token: source_space_token.clone(),
        declared_space_tokens: vec![source_space_token.clone()],
        raw_map: None,
    }
}

fn terminal_refusal(terminal: &BlockContentResolveTerminal) -> BlockContentRefusal {
    match terminal {
        BlockContentResolveTerminal::PreCapture(outcome) => match outcome {
            BlockContentPreCaptureTerminal::Failed => BlockContentRefusal::CorrelationMismatch,
            BlockContentPreCaptureTerminal::Stale => BlockContentRefusal::Stale,
            BlockContentPreCaptureTerminal::Unavailable => BlockContentRefusal::Missing,
            BlockContentPreCaptureTerminal::Closed => BlockContentRefusal::CorrelationClosed,
            BlockContentPreCaptureTerminal::Cancelled => BlockContentRefusal::CorrelationCancelled,
        },
        BlockContentResolveTerminal::PostCapture { outcome, .. } => match outcome {
            BlockContentPostCaptureTerminal::Failed(reason) => reason.clone(),
            BlockContentPostCaptureTerminal::StaleWithReplacement(_)
            | BlockContentPostCaptureTerminal::StaleNeedsRecapture => BlockContentRefusal::Stale,
            BlockContentPostCaptureTerminal::Superseded => {
                BlockContentRefusal::CorrelationSuperseded
            }
            BlockContentPostCaptureTerminal::Closed => BlockContentRefusal::CorrelationClosed,
            BlockContentPostCaptureTerminal::Cancelled => BlockContentRefusal::CorrelationCancelled,
            BlockContentPostCaptureTerminal::Admitted => BlockContentRefusal::CorrelationTerminal,
        },
    }
}

fn pre_capture_outcome(
    availability: BlockContentAvailability,
) -> Result<(), BlockContentPreCaptureTerminal> {
    match availability {
        BlockContentAvailability::ProcessedContentRequired => Ok(()),
        BlockContentAvailability::Stale => Err(BlockContentPreCaptureTerminal::Stale),
        BlockContentAvailability::Conflict => Err(BlockContentPreCaptureTerminal::Failed),
        BlockContentAvailability::NativeAvailable
        | BlockContentAvailability::SuppliedAvailable
        | BlockContentAvailability::Missing => Err(BlockContentPreCaptureTerminal::Unavailable),
    }
}

pub(crate) fn named_attr<'a>(
    inventory: &'a verter_language::CarrierBlockInventory,
    syntax: &TaggedSyntax,
    wanted: &str,
) -> Option<Option<&'a str>> {
    syntax.attributes.iter().find_map(|attribute| {
        let CarrierAttribute::Named { name, value, .. } = attribute else {
            return None;
        };
        let authored = inventory.slice(name.authored).ok()?;
        if !authored.eq_ignore_ascii_case(wanted) {
            return None;
        }
        Some(match value {
            AttributeValue::Static { raw, .. } => inventory.slice(*raw).ok(),
            AttributeValue::Missing
            | AttributeValue::Expression { .. }
            | AttributeValue::Mixed { .. } => None,
        })
    })
}

pub(crate) fn role_class(role: &SectionRole) -> BlockContentClass {
    match role {
        SectionRole::TemplateHost => BlockContentClass::Template,
        SectionRole::Script { .. } => BlockContentClass::Script,
        SectionRole::Style { .. } => BlockContentClass::Style,
        SectionRole::Custom { .. } => BlockContentClass::Custom,
    }
}

pub(crate) fn role_lang(
    inventory: &verter_language::CarrierBlockInventory,
    role: &SectionRole,
    syntax: &TaggedSyntax,
) -> String {
    if let Some(Some(lang)) = named_attr(inventory, syntax, "lang") {
        return lang.to_ascii_lowercase();
    }
    match role {
        SectionRole::TemplateHost => "html".to_string(),
        SectionRole::Script { dialect, .. } => match dialect {
            verter_language::parse_artifact::carrier_inventory::ScriptSourceType::JavaScript => {
                "js"
            }
            verter_language::parse_artifact::carrier_inventory::ScriptSourceType::TypeScript => {
                "ts"
            }
            verter_language::parse_artifact::carrier_inventory::ScriptSourceType::Jsx => "jsx",
            verter_language::parse_artifact::carrier_inventory::ScriptSourceType::Tsx => "tsx",
            verter_language::parse_artifact::carrier_inventory::ScriptSourceType::Custom {
                ..
            }
            | verter_language::parse_artifact::carrier_inventory::ScriptSourceType::Missing => {
                "unknown"
            }
        }
        .to_string(),
        SectionRole::Style { dialect, .. } => match dialect {
            StyleDialect::Css => "css",
            StyleDialect::Scss => "scss",
            StyleDialect::Sass => "sass",
            StyleDialect::Less => "less",
            StyleDialect::Stylus => "stylus",
            StyleDialect::PostCss => "postcss",
            StyleDialect::Custom { .. } | StyleDialect::Missing => "unknown",
        }
        .to_string(),
        SectionRole::Custom { normalized_name } => normalized_name.to_string(),
    }
}

pub(crate) fn native_language(content_class: BlockContentClass, lang: &str) -> bool {
    match content_class {
        BlockContentClass::Template => matches!(lang, "" | "html"),
        BlockContentClass::Script => {
            matches!(
                lang,
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
            )
        }
        BlockContentClass::Style => matches!(lang, "css" | "scss" | "sass" | "less" | "stylus"),
        BlockContentClass::Custom => false,
    }
}

/// Single authority for whether selected authored bytes can flow directly
/// into the compiler. A custom block without an explicit language is opaque
/// native payload; declaring `lang` opts it into the supplied-content handoff.
pub(crate) fn block_content_is_native(
    inventory: &verter_language::CarrierBlockInventory,
    role: &SectionRole,
    syntax: &TaggedSyntax,
    lang: &str,
) -> bool {
    native_language(role_class(role), lang)
        || (matches!(role, SectionRole::Custom { .. })
            && named_attr(inventory, syntax, "lang").is_none())
}

fn external_specifier_lang(content_class: BlockContentClass, specifier: &str) -> Option<String> {
    let path = specifier
        .split(['?', '#'])
        .next()
        .unwrap_or(specifier)
        .trim_end_matches('/');
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    let lang = match content_class {
        BlockContentClass::Template => match extension.as_str() {
            "html" | "htm" => "html",
            other => other,
        },
        BlockContentClass::Script => match extension.as_str() {
            "javascript" => "js",
            "typescript" => "ts",
            other => other,
        },
        BlockContentClass::Style => match extension.as_str() {
            "pcss" => "postcss",
            "styl" => "stylus",
            other => other,
        },
        BlockContentClass::Custom => extension.as_str(),
    };
    Some(lang.to_string())
}

impl VerterHost {
    pub(crate) fn has_current_supplied_block_content(
        &self,
        canonical_id: &str,
        profile_hash: u64,
    ) -> bool {
        let candidates = read_lock(&self.block_content.state)
            .supplied
            .iter()
            .filter(|((owner, profile, _), _)| owner == canonical_id && *profile == profile_hash)
            .map(|((_, _, block_token), artifact)| (block_token.clone(), artifact.clone()))
            .collect::<Vec<_>>();
        candidates.into_iter().any(|(block_token, artifact)| {
            self.selected_block(canonical_id, &block_token)
                .is_ok_and(|selected| {
                    selected.owner_revision == artifact.owner_revision
                        && selected.artifact_token == artifact.artifact_token
                        && selected.basis_token == artifact.basis_token
                        && selected.source_space_token == artifact.input_source_space_token
                })
        })
    }

    fn selected_block(
        &self,
        canonical_id: &str,
        wanted_block_token: &crate::carrier_publication_store::ArtifactBlockToken,
    ) -> Result<SelectedBlock, BlockContentRefusal> {
        let canonical_id = self.resolve_alias_or_canonical(canonical_id);
        let owner = self
            .scheduler
            .try_get_source(&canonical_id)
            .ok_or(BlockContentRefusal::Missing)?;
        let owner_data = owner
            .downcast_data::<HostSourceData>()
            .ok_or(BlockContentRefusal::Missing)?;
        let structure = owner_data
            .structure
            .as_ref()
            .ok_or(BlockContentRefusal::Missing)?;
        let inventory = structure.inventory();
        let owner_revision =
            BlockContentOwnerRevisionToken::mint(owner_data.revision_token.public_token());
        let artifact_token = structure.public_artifact_token();

        let mut selected = None;
        for block in inventory.blocks() {
            let Some(block_ref) = structure.block_ref(block.id()) else {
                continue;
            };
            let Some(token) = structure.public_block_token(&block_ref) else {
                continue;
            };
            if token != *wanted_block_token {
                continue;
            }
            let CarrierBlock::Section { role, syntax, .. } = block else {
                return Err(BlockContentRefusal::Missing);
            };
            selected = Some((role, syntax, token));
            break;
        }
        let (role, syntax, block_token) = selected.ok_or(BlockContentRefusal::Missing)?;
        let content_class = role_class(role);
        let mut lang = role_lang(inventory, role, syntax);
        let carrier_source_space_token = BlockContentSourceSpaceToken::mint(
            structure
                .public_source_space_token(syntax.content_span.source_space)
                .ok_or(BlockContentRefusal::SourceSpaceMismatch)?
                .as_str()
                .to_string(),
        );

        let inline = inventory
            .slice(SourceSlice::new(syntax.content_span))
            .map_err(|_| BlockContentRefusal::Missing)?;
        let src = named_attr(inventory, syntax, "src").flatten();
        let has_explicit_lang = named_attr(inventory, syntax, "lang").is_some();
        if !has_explicit_lang {
            if let Some(specifier) = src {
                if let Some(inferred) = external_specifier_lang(content_class, specifier) {
                    lang = inferred;
                }
            }
        }
        let inline_live = !inline.trim().is_empty();

        let (availability, authored_origin, authored_content, source_space_token, content_hash) =
            if let Some(specifier) = src {
                if inline_live {
                    (
                        BlockContentAvailability::Conflict,
                        None,
                        None,
                        carrier_source_space_token.clone(),
                        None,
                    )
                } else {
                    // Select external block bytes through the same VFS owner-edge
                    // authority used by compile-blocker hydration and the compile
                    // prefetch.  The parse-time path join is only a fallback for an
                    // admitted unresolved answer; it cannot represent workspace
                    // aliases, exact bundler routes, or extension probing.
                    let resolved = match self.resolve_for_persistent_state(
                        &canonical_id,
                        specifier,
                        verter_semantic::resolver_core::ResolutionContext {
                            phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                            kind: verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                        },
                    ) {
                        verter_workspace::ResolutionPublication::Admitted(admitted) => admitted
                            .into_result()
                            .map(|resolution| resolution.source_id)
                            .unwrap_or_else(|| {
                                crate::id::resolve_external(&canonical_id, specifier)
                            }),
                        verter_workspace::ResolutionPublication::Refused(_) => {
                            return Err(BlockContentRefusal::Stale);
                        }
                    };
                    // An extensionless or aliased authored specifier does not
                    // carry the selected dialect.  Infer from the admitted VFS
                    // canonical so `.js`/`.ts` retargets move both language and
                    // basis identity.  An explicit `lang` remains authoritative.
                    if !has_explicit_lang {
                        if let Some(inferred) = external_specifier_lang(content_class, &resolved) {
                            lang = inferred;
                        }
                    }
                    let content_is_native = block_content_is_native(inventory, role, syntax, &lang);
                    let Some(external) = self.scheduler.try_get_source(&resolved) else {
                        let missing_basis = BlockContentBasisToken::mint(hash_parts(&[
                            b"verter.block-content.missing.v1\0",
                            canonical_id.as_bytes(),
                            wanted_block_token.as_bytes(),
                        ]));
                        return Ok(SelectedBlock {
                            content_class,
                            lang,
                            block_token,
                            owner_revision: owner_revision.clone(),
                            artifact_token,
                            source_space_token: carrier_source_space_token.clone(),
                            content_artifact_token: BlockContentArtifactToken::mint(hash_parts(&[
                                b"verter.block-content.artifact.missing.v1\0",
                                missing_basis.as_bytes(),
                            ])),
                            source_descriptor: BlockContentSourceSpaceDescriptor {
                                token: carrier_source_space_token.clone(),
                                kind: BlockContentSourceSpaceKind::Owner,
                                source_token: BlockContentArtifactToken::mint(
                                    owner_revision.as_str().to_string(),
                                ),
                                content_hash: hash_block_content(""),
                                utf8_byte_len: 0,
                            },
                            basis_token: missing_basis,
                            content_hash: None,
                            authored_origin: None,
                            authored_content: None,
                            availability: BlockContentAvailability::Missing,
                        });
                    };
                    let external_data = external
                        .downcast_data::<HostSourceData>()
                        .ok_or(BlockContentRefusal::Missing)?;
                    let hash = hash_block_content(external.source.as_ref());
                    let external_revision = external_data.revision_token.public_token();
                    let source_space = BlockContentSourceSpaceToken::mint(hash_parts(&[
                        b"verter.block-content.source-space.v1\0",
                        resolved.as_bytes(),
                        hash.as_bytes(),
                        external_revision.as_bytes(),
                    ]));
                    (
                        if content_is_native {
                            BlockContentAvailability::NativeAvailable
                        } else {
                            BlockContentAvailability::ProcessedContentRequired
                        },
                        Some(BlockContentOrigin::NativeVfs {
                            canonical_id: resolved,
                            content_hash: hash.to_string(),
                        }),
                        Some(external.source.clone()),
                        source_space,
                        Some(hash),
                    )
                }
            } else {
                let content_is_native = block_content_is_native(inventory, role, syntax, &lang);
                let content = Arc::<str>::from(inline);
                let hash = hash_block_content(&content);
                (
                    if content_is_native {
                        BlockContentAvailability::NativeAvailable
                    } else {
                        BlockContentAvailability::ProcessedContentRequired
                    },
                    Some(BlockContentOrigin::InlineAuthored),
                    Some(content),
                    carrier_source_space_token.clone(),
                    Some(hash),
                )
            };

        let basis_token = BlockContentBasisToken::mint(hash_parts(&[
            b"verter.block-content.basis.v1\0",
            owner_revision.as_bytes(),
            artifact_token.as_bytes(),
            block_token.as_bytes(),
            source_space_token.as_bytes(),
            content_hash.as_deref().unwrap_or("").as_bytes(),
            lang.as_bytes(),
        ]));
        let content_artifact_token = BlockContentArtifactToken::mint(hash_parts(&[
            b"verter.block-content.artifact.native.v1\0",
            basis_token.as_bytes(),
            source_space_token.as_bytes(),
            content_hash.as_deref().unwrap_or("").as_bytes(),
        ]));
        let source_descriptor = BlockContentSourceSpaceDescriptor {
            token: source_space_token.clone(),
            kind: if matches!(authored_origin, Some(BlockContentOrigin::NativeVfs { .. })) {
                BlockContentSourceSpaceKind::External
            } else {
                BlockContentSourceSpaceKind::Owner
            },
            source_token: BlockContentArtifactToken::mint(match authored_origin.as_ref() {
                Some(BlockContentOrigin::NativeVfs { canonical_id, .. }) => canonical_id.clone(),
                _ => owner_revision.as_str().to_string(),
            }),
            content_hash: content_hash
                .clone()
                .unwrap_or_else(|| hash_block_content("")),
            utf8_byte_len: authored_content
                .as_deref()
                .map_or(0, |content| content.len() as u64),
        };

        Ok(SelectedBlock {
            content_class,
            lang,
            block_token,
            owner_revision,
            artifact_token,
            source_space_token,
            content_artifact_token,
            source_descriptor,
            basis_token,
            content_hash,
            authored_origin,
            authored_content,
            availability,
        })
    }

    pub fn get_block_content(
        &self,
        query: BlockContentQuery,
    ) -> Result<BlockContentSnapshot, HostError> {
        let canonical_id = self.resolve_alias_or_canonical(&query.canonical_id);
        let block_token = crate::carrier_publication_store::ArtifactBlockToken::parse_untrusted(
            query.block_token,
        )
        .ok_or(HostError::BlockContentRefused(
            BlockContentRefusal::MalformedToken,
        ))?;
        let selected = self
            .selected_block(&canonical_id, &block_token)
            .map_err(HostError::BlockContentRefused)?;
        if query
            .expected_basis_token
            .as_deref()
            .is_some_and(|expected| expected != selected.basis_token.as_str())
        {
            return Ok(BlockContentSnapshot {
                availability: BlockContentAvailability::Stale,
                origin: None,
                content: None,
                content_class: selected.content_class,
                lang: selected.lang,
                block_token: selected.block_token,
                owner_revision: selected.owner_revision,
                artifact_token: selected.artifact_token,
                content_artifact_token: selected.content_artifact_token,
                basis_token: selected.basis_token,
                source_space_token: selected.source_space_token.clone(),
                content_hash: None,
                source_map: None,
                source_map_hash: None,
                source_spaces: vec![selected.source_descriptor.clone()],
                final_output_space: selected.source_descriptor.clone(),
                immediate_maps: Vec::new(),
                composed_map: identity_qualified_map(&selected.source_space_token),
            });
        }

        let profile_hash = compile_profile_hash(&query.compile_profile);
        let supplied = read_lock(&self.block_content.state)
            .supplied
            .get(&(canonical_id, profile_hash, selected.block_token.clone()))
            .cloned();
        if let Some(supplied) = supplied {
            if supplied.owner_revision != selected.owner_revision
                || supplied.artifact_token != selected.artifact_token
                || supplied.basis_token != selected.basis_token
                || supplied.input_source_space_token != selected.source_space_token
            {
                // A stale supplied artifact is not a second live source. It
                // simply loses precedence and the current native/inline
                // selection remains authoritative.
            } else {
                return Ok(BlockContentSnapshot {
                    availability: BlockContentAvailability::SuppliedAvailable,
                    origin: Some(BlockContentOrigin::SuppliedValidated {
                        dependencies: supplied.dependencies,
                        diagnostics: supplied.diagnostics,
                        processor_identity: supplied.processor_identity,
                        processor_version: supplied.processor_version,
                        config_fingerprint: supplied.config_fingerprint,
                    }),
                    content: Some(supplied.code),
                    content_class: selected.content_class,
                    lang: selected.lang,
                    block_token: selected.block_token,
                    owner_revision: selected.owner_revision,
                    artifact_token: selected.artifact_token,
                    content_artifact_token: supplied.content_artifact_token,
                    basis_token: selected.basis_token,
                    source_space_token: supplied.output_source_space_token,
                    content_hash: Some(supplied.code_hash),
                    source_map: supplied.source_map,
                    source_map_hash: supplied.source_map_hash,
                    source_spaces: vec![
                        selected.source_descriptor,
                        supplied.output_descriptor.clone(),
                    ],
                    final_output_space: supplied.output_descriptor,
                    immediate_maps: supplied
                        .qualified_source_map
                        .raw_map
                        .is_some()
                        .then(|| supplied.qualified_source_map.clone())
                        .into_iter()
                        .collect(),
                    composed_map: supplied.qualified_source_map,
                });
            }
        }

        Ok(BlockContentSnapshot {
            availability: selected.availability,
            origin: selected.authored_origin,
            content: selected.authored_content,
            content_class: selected.content_class,
            lang: selected.lang,
            block_token: selected.block_token,
            owner_revision: selected.owner_revision,
            artifact_token: selected.artifact_token,
            content_artifact_token: selected.content_artifact_token,
            basis_token: selected.basis_token,
            source_space_token: selected.source_space_token.clone(),
            content_hash: selected.content_hash,
            source_map: None,
            source_map_hash: None,
            source_spaces: vec![selected.source_descriptor.clone()],
            final_output_space: selected.source_descriptor.clone(),
            immediate_maps: Vec::new(),
            composed_map: identity_qualified_map(&selected.source_space_token),
        })
    }

    /// Build the compiler's parser-local projection from sealed block
    /// selections. Public callers never provide these slots: every entry is
    /// derived from the current registered inventory and validated content
    /// state immediately before compilation.
    pub(crate) fn compiler_block_content_inputs(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<verter_compiler::framework_common::RuntimeBlockContentInputs, HostError> {
        use verter_compiler::framework_common::{
            RuntimeBlockContentInput, RuntimeBlockContentInputs,
        };
        use verter_language::parse_artifact::carrier_inventory::ScriptRole;

        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let owner =
            self.scheduler
                .try_get_source(&canonical)
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;
        let owner_data =
            owner
                .downcast_data::<HostSourceData>()
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;
        let structure = owner_data
            .structure
            .as_ref()
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical.clone(),
            })?;

        let mut projection = RuntimeBlockContentInputs::default();
        for block in structure.inventory().blocks() {
            let CarrierBlock::Section { role, syntax, .. } = block else {
                continue;
            };
            let slot = match role {
                SectionRole::Style { .. } => {
                    projection.styles.push(None);
                    Some((BlockContentClass::Style, projection.styles.len() - 1))
                }
                SectionRole::Custom { .. } => {
                    projection.custom_blocks.push(None);
                    Some((
                        BlockContentClass::Custom,
                        projection.custom_blocks.len() - 1,
                    ))
                }
                _ => None,
            };
            let block_ref = structure
                .block_ref(block.id())
                .ok_or(HostError::BlockContentRefused(BlockContentRefusal::Missing))?;
            let block_token = structure
                .public_block_token(&block_ref)
                .ok_or(HostError::BlockContentRefused(BlockContentRefusal::Missing))?;
            let snapshot = self.get_block_content(BlockContentQuery {
                canonical_id: canonical.clone(),
                block_token: block_token.to_string(),
                compile_profile: profile.clone(),
                expected_basis_token: None,
            })?;

            match snapshot.availability {
                BlockContentAvailability::NativeAvailable
                | BlockContentAvailability::SuppliedAvailable => {}
                BlockContentAvailability::ProcessedContentRequired
                | BlockContentAvailability::Missing
                | BlockContentAvailability::Conflict
                | BlockContentAvailability::Stale => {
                    return Err(HostError::BlockContentRefused(
                        BlockContentRefusal::Unavailable {
                            block_token: snapshot.block_token.to_string(),
                            availability: snapshot.availability,
                        },
                    ));
                }
            }

            let is_external = named_attr(structure.inventory(), syntax, "src")
                .flatten()
                .is_some();
            let is_supplied = snapshot.availability == BlockContentAvailability::SuppliedAvailable;
            if !is_external && !is_supplied {
                continue;
            }
            let code = snapshot
                .content
                .ok_or(HostError::BlockContentRefused(BlockContentRefusal::Missing))?;
            let output_lang = if is_supplied {
                match snapshot.content_class {
                    BlockContentClass::Template => "html".to_string(),
                    BlockContentClass::Script => "js".to_string(),
                    BlockContentClass::Style => "css".to_string(),
                    BlockContentClass::Custom => snapshot.lang.clone(),
                }
            } else {
                snapshot.lang.clone()
            };
            let input = RuntimeBlockContentInput {
                code,
                source_map: snapshot.source_map,
                lang: output_lang,
                content_artifact_token: snapshot.content_artifact_token.to_string(),
                source_space_token: snapshot.source_space_token.to_string(),
            };
            match role {
                SectionRole::TemplateHost => projection.template = Some(input),
                SectionRole::Script {
                    role: ScriptRole::Setup,
                    ..
                } => projection.script_setup = Some(input),
                SectionRole::Script { .. } => projection.script = Some(input),
                SectionRole::Style { .. } => {
                    let (_, index) = slot.expect("style slot was allocated");
                    projection.styles[index] = Some(input);
                }
                SectionRole::Custom { .. } => {
                    let (_, index) = slot.expect("custom slot was allocated");
                    projection.custom_blocks[index] = Some(input);
                }
            }
        }
        Ok(projection)
    }

    /// Capture the classifier bit and exact compiler projection as one
    /// stampable unit. Callers hold `block_content.admission_fence` while
    /// invoking this method, so owner/external publication and supplied
    /// admission cannot tear the two reads.
    pub(crate) fn capture_compiler_block_content(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<CompilerBlockContentCapture, HostError> {
        let profile_hash = compile_profile_hash(profile);
        let has_supplied = self.has_current_supplied_block_content(canonical_id, profile_hash);
        let inputs = self.compiler_block_content_inputs(canonical_id, profile)?;
        let stamp = compiler_block_content_stamp(has_supplied, &inputs);
        Ok(CompilerBlockContentCapture {
            has_supplied,
            inputs,
            stamp,
        })
    }

    /// Revalidate a cold compile's full owner + block-content capture before
    /// observable publication. The caller holds `block_content.admission_fence`
    /// through both this check and the corresponding cache/artifact writes.
    pub(crate) fn compiler_block_content_capture_is_current(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
        captured_whole_hash: verter_semantic::analysis::types::Hash16,
        captured_stamp: &BlockContentHashToken,
    ) -> bool {
        let owner_is_current = self
            .scheduler
            .try_get_source(canonical_id)
            .is_some_and(|source| {
                source
                    .downcast_data::<HostSourceData>()
                    .is_some_and(|data| data.parse.whole_hash == captured_whole_hash)
            });
        owner_is_current
            && self
                .capture_compiler_block_content(canonical_id, profile)
                .is_ok_and(|capture| capture.stamp == *captured_stamp)
    }

    pub(crate) fn hydrate_style_content(
        &self,
        canonical_id: &str,
        styles: &Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>,
    ) -> Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>> {
        let captured = self.capture_compiler_style_content_for_profile(
            canonical_id,
            styles,
            &CompileProfile::default(),
        );
        if captured.analyses_changed {
            captured.analyses
        } else {
            Arc::clone(styles)
        }
    }

    pub(crate) fn capture_compiler_style_content_for_profile(
        &self,
        canonical_id: &str,
        styles: &[verter_semantic::analysis::StyleBlockAnalysis],
        profile: &CompileProfile,
    ) -> CompilerStyleContentCapture {
        let mut hydrated = styles.to_vec();
        let mut v_bind_vars = Vec::new();
        let mut usage_complete = true;
        let mut analyses_changed = false;
        for style in &mut hydrated {
            let Some(block_token) = style.block_token.as_deref() else {
                continue;
            };
            let Some(block_token) =
                crate::carrier_publication_store::ArtifactBlockToken::parse_untrusted(block_token)
            else {
                continue;
            };
            let Ok(snapshot) = self.get_block_content(BlockContentQuery {
                canonical_id: canonical_id.to_string(),
                block_token: block_token.to_string(),
                compile_profile: profile.clone(),
                expected_basis_token: None,
            }) else {
                usage_complete = false;
                continue;
            };
            if matches!(
                snapshot.availability,
                BlockContentAvailability::NativeAvailable
                    | BlockContentAvailability::SuppliedAvailable
            ) {
                if let Some(content) = snapshot.content.as_deref() {
                    let usage_lang = if snapshot.availability
                        == BlockContentAvailability::SuppliedAvailable
                        && snapshot.content_class == BlockContentClass::Style
                    {
                        "css"
                    } else {
                        snapshot.lang.as_str()
                    };
                    let usage = verter_compiler::compile::style_usage::
                        extract_style_v_bind_usage_for_languages([(content, usage_lang)]);
                    usage_complete &= usage.complete;
                    v_bind_vars.extend(usage.used);
                } else {
                    usage_complete = false;
                }
            }
            if snapshot.availability == BlockContentAvailability::NativeAvailable
                && style.content_is_available()
                && style
                    .source_space_token
                    .as_deref()
                    .is_none_or(|token| token == snapshot.source_space_token.as_str())
            {
                continue;
            }
            if matches!(
                snapshot.availability,
                BlockContentAvailability::NativeAvailable
                    | BlockContentAvailability::SuppliedAvailable
            ) {
                // LSP consumers interpret these fields as carrier-absolute.
                // External/supplied bytes occupy another source space, so no
                // source-located facts may be published here.
                style.v_binds.clear();
                style.special_pseudos.clear();
                style.css = None;
                style.content_offset = 0;
                style.content_availability = snapshot.availability;
                style.source_space_token = Some(snapshot.source_space_token.to_string());
                analyses_changed = true;
            } else if style.content_availability != snapshot.availability {
                style.content_availability = snapshot.availability;
                style.source_space_token = Some(snapshot.source_space_token.to_string());
                analyses_changed = true;
            }
        }
        v_bind_vars.sort_unstable();
        v_bind_vars.dedup();
        CompilerStyleContentCapture {
            analyses: Arc::new(hydrated),
            v_bind_vars,
            usage_complete,
            analyses_changed,
        }
    }

    pub(crate) fn materialize_preprocessor_requests(
        &self,
        canonical_id: &str,
        pending: &[PendingPreprocessorRequest],
    ) -> Vec<PreprocessorRequest> {
        let Some(owner) = self.scheduler.try_get_source(canonical_id) else {
            return Vec::new();
        };
        let Some(owner_data) = owner.downcast_data::<HostSourceData>() else {
            return Vec::new();
        };
        let Some(structure) = owner_data.structure.as_ref() else {
            return Vec::new();
        };
        let mut output = Vec::with_capacity(pending.len());
        for seed in pending {
            if !seed.block_ref.validate(structure.inventory()) {
                continue;
            }
            let Some(block_ref) = structure.block_ref(seed.block_ref.block_id()) else {
                continue;
            };
            let Some(block_token) = structure.public_block_token(&block_ref) else {
                continue;
            };
            let Ok(snapshot) = self.get_block_content(BlockContentQuery {
                canonical_id: canonical_id.to_string(),
                block_token: block_token.to_string(),
                compile_profile: CompileProfile::default(),
                expected_basis_token: None,
            }) else {
                continue;
            };
            if pre_capture_outcome(snapshot.availability).is_err() {
                continue;
            }
            let content_hash = snapshot
                .content_hash
                .clone()
                .unwrap_or_else(|| hash_block_content(&seed.content));
            let input_content = snapshot
                .content
                .clone()
                .unwrap_or_else(|| Arc::from(seed.content.as_str()));
            // A byte-identical no-op upsert reuses the already-issued live
            // correlation. This keeps repeated transforms stable and avoids
            // growing pending state merely because a bundler asks again.
            let reusable = read_lock(&self.block_content.state)
                .pending
                .iter()
                .find(|(_, pending)| {
                    pending.canonical_id == canonical_id
                        && pending.block_token == snapshot.block_token
                        && pending.owner_revision == snapshot.owner_revision
                        && pending.artifact_token == snapshot.artifact_token
                        && pending.basis_token == snapshot.basis_token
                        && pending.source_space_token == snapshot.source_space_token
                        && pending.content_hash == content_hash
                })
                .map(|(token, _)| token.clone());
            let correlation_token = reusable.unwrap_or_else(|| loop {
                let sequence = self
                    .block_content
                    .correlation_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                let token = BlockContentCorrelationToken::mint(hash_parts(&[
                    b"verter.block-content.correlation.v1\0",
                    &self.instance_id.to_le_bytes(),
                    &sequence.to_le_bytes(),
                    snapshot.basis_token.as_bytes(),
                ]));
                let mut state = write_lock(&self.block_content.state);
                if state.pending.contains_key(&token) || state.terminal.contains_key(&token) {
                    continue;
                }
                state.insert_pending(
                    token.clone(),
                    PendingCorrelation {
                        canonical_id: canonical_id.to_string(),
                        block_token: snapshot.block_token.clone(),
                        owner_revision: snapshot.owner_revision.clone(),
                        artifact_token: snapshot.artifact_token.clone(),
                        basis_token: snapshot.basis_token.clone(),
                        source_space_token: snapshot.source_space_token.clone(),
                        content_hash: content_hash.clone(),
                        input_content: Arc::clone(&input_content),
                        captured_echo: BlockContentCapturedEcho {
                            request: BlockContentPreCaptureEcho {
                                correlation_token: token.clone(),
                                canonical_id: canonical_id.to_string(),
                                block_token: snapshot.block_token.clone(),
                                owner_revision: snapshot.owner_revision.clone(),
                                artifact_token: snapshot.artifact_token.clone(),
                                expected_language: snapshot.lang.clone(),
                                prior_basis_token: None,
                            },
                            basis_token: snapshot.basis_token.clone(),
                        },
                    },
                );
                break token;
            });
            let captured_echo = BlockContentCapturedEcho {
                request: BlockContentPreCaptureEcho {
                    correlation_token: correlation_token.clone(),
                    canonical_id: canonical_id.to_string(),
                    block_token: snapshot.block_token.clone(),
                    owner_revision: snapshot.owner_revision.clone(),
                    artifact_token: snapshot.artifact_token.clone(),
                    expected_language: snapshot.lang.clone(),
                    prior_basis_token: None,
                },
                basis_token: snapshot.basis_token.clone(),
            };
            output.push(PreprocessorRequest {
                content_class: seed.content_class,
                lang: seed.lang.clone(),
                content: snapshot
                    .content
                    .as_deref()
                    .unwrap_or(seed.content.as_str())
                    .to_string(),
                availability: snapshot.availability,
                correlation_token,
                block_token: snapshot.block_token,
                owner_revision: snapshot.owner_revision,
                artifact_token: snapshot.artifact_token,
                prior_basis_token: None,
                pre_capture_echo: captured_echo.request.clone(),
                basis_token: snapshot.basis_token,
                captured_echo,
                source_space_token: snapshot.source_space_token,
                content_hash,
                custom_type: seed.custom_type.clone(),
            });
        }
        output
    }

    pub fn apply_block_overrides(
        &self,
        req: BlockOverrideRequest,
    ) -> Result<HostUpdateResult, HostError> {
        let _fence = self.block_content.admission_fence.lock();
        let canonical_id = self.resolve_alias_or_canonical(&req.canonical_id);
        let profile_hash = compile_profile_hash(&req.compile_profile);
        let mut unique = HashSet::new();
        let mut admitted = Vec::with_capacity(req.overrides.len());
        for entry in req.overrides {
            // Once a correlation has been captured, every refusal must consume
            // that pending await and retain the host's exact echo in a
            // post-capture terminal. Do the minimal correlation lookup before
            // inspecting any other untrusted field.
            let pending = {
                let state = read_lock(&self.block_content.state);
                if let Some(terminal) = state.terminal.get(&entry.correlation_token) {
                    return Err(HostError::BlockContentRefused(terminal_refusal(terminal)));
                }
                state.pending.get(&entry.correlation_token).cloned()
            }
            .ok_or(HostError::BlockContentRefused(
                BlockContentRefusal::CorrelationMismatch,
            ))?;
            let terminal_on_error = |reason: BlockContentRefusal| {
                let mut state = write_lock(&self.block_content.state);
                state.remove_pending(&entry.correlation_token);
                let outcome = if reason == BlockContentRefusal::Stale {
                    BlockContentPostCaptureTerminal::StaleNeedsRecapture
                } else {
                    BlockContentPostCaptureTerminal::Failed(reason.clone())
                };
                state.mark_terminal(
                    entry.correlation_token.clone(),
                    BlockContentResolveTerminal::PostCapture {
                        echo: pending.captured_echo.clone(),
                        outcome,
                    },
                );
                HostError::BlockContentRefused(reason)
            };
            if ![
                entry.correlation_token.as_str(),
                entry.block_token.as_str(),
                entry.owner_revision.as_str(),
                entry.artifact_token.as_str(),
                entry.basis_token.as_str(),
                entry.source_space_token.as_str(),
                entry.code_hash.as_str(),
            ]
            .into_iter()
            .all(valid_sealed_token)
                || entry
                    .source_map_hash
                    .as_deref()
                    .is_some_and(|token| !valid_sealed_token(token))
            {
                return Err(terminal_on_error(BlockContentRefusal::MalformedToken));
            }
            let supplied_provenance_bytes = entry
                .processor_identity
                .len()
                .saturating_add(entry.processor_version.len())
                .saturating_add(entry.dependencies.iter().map(String::len).sum::<usize>())
                .saturating_add(
                    entry
                        .diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.len())
                        .sum::<usize>(),
                );
            if entry.code.len() > MAX_SUPPLIED_CODE_BYTES
                || entry
                    .source_map
                    .as_deref()
                    .is_some_and(|map| map.len() > MAX_SUPPLIED_MAP_BYTES)
                || supplied_provenance_bytes > MAX_SUPPLIED_PROVENANCE_BYTES
            {
                return Err(terminal_on_error(BlockContentRefusal::PayloadTooLarge));
            }
            if !unique.insert(entry.block_token.clone()) {
                return Err(terminal_on_error(BlockContentRefusal::DuplicateBlock));
            }
            if entry.captured_echo != pending.captured_echo
                || entry.correlation_token != entry.captured_echo.request.correlation_token
                || entry.block_token != entry.captured_echo.request.block_token
                || entry.owner_revision != entry.captured_echo.request.owner_revision
                || entry.artifact_token != entry.captured_echo.request.artifact_token
                || entry.basis_token != entry.captured_echo.basis_token
                || entry.captured_echo.request.canonical_id != canonical_id
            {
                return Err(terminal_on_error(BlockContentRefusal::CorrelationMismatch));
            }
            if pending.canonical_id != canonical_id || pending.block_token != entry.block_token {
                return Err(terminal_on_error(BlockContentRefusal::CorrelationMismatch));
            }
            if pending.owner_revision != entry.owner_revision {
                return Err(terminal_on_error(BlockContentRefusal::Stale));
            }
            if pending.artifact_token != entry.artifact_token {
                return Err(terminal_on_error(BlockContentRefusal::ArtifactMismatch));
            }
            if pending.basis_token != entry.basis_token {
                return Err(terminal_on_error(BlockContentRefusal::BasisMismatch));
            }
            if pending.source_space_token != entry.source_space_token {
                return Err(terminal_on_error(BlockContentRefusal::SourceSpaceMismatch));
            }
            let current = self
                .selected_block(&canonical_id, &entry.block_token)
                .map_err(&terminal_on_error)?;
            if current.owner_revision != entry.owner_revision
                || current.artifact_token != entry.artifact_token
                || current.basis_token != entry.basis_token
            {
                return Err(terminal_on_error(BlockContentRefusal::Stale));
            }
            match current.availability {
                BlockContentAvailability::ProcessedContentRequired => {}
                BlockContentAvailability::Conflict => {
                    return Err(terminal_on_error(BlockContentRefusal::Conflict));
                }
                availability @ (BlockContentAvailability::NativeAvailable
                | BlockContentAvailability::SuppliedAvailable
                | BlockContentAvailability::Missing
                | BlockContentAvailability::Stale) => {
                    return Err(terminal_on_error(BlockContentRefusal::Unavailable {
                        block_token: entry.block_token.to_string(),
                        availability,
                    }));
                }
            }
            if hash_block_content(&entry.code) != entry.code_hash {
                return Err(terminal_on_error(BlockContentRefusal::CodeHashMismatch));
            }
            match (&entry.source_map, &entry.source_map_hash) {
                (None, None) => {}
                (Some(map), Some(expected)) => {
                    if hash_block_content(map) != *expected {
                        return Err(terminal_on_error(
                            BlockContentRefusal::SourceMapHashMismatch,
                        ));
                    }
                    if !valid_source_map_v3(map, &pending.input_content, &entry.code) {
                        return Err(terminal_on_error(BlockContentRefusal::InvalidSourceMap));
                    }
                }
                _ => {
                    return Err(terminal_on_error(
                        BlockContentRefusal::SourceMapHashMismatch,
                    ));
                }
            }
            let output_source_space_token = BlockContentSourceSpaceToken::mint(hash_parts(&[
                b"verter.block-content.source-space.supplied.v1\0",
                entry.source_space_token.as_bytes(),
                entry.code_hash.as_bytes(),
                entry.source_map_hash.as_deref().unwrap_or("").as_bytes(),
                &profile_hash.to_le_bytes(),
            ]));
            let qualified_source_map = QualifiedBlockContentSourceMap {
                map_hash: BlockContentHashToken::mint(hash_parts(&[
                    b"verter.block-content.qualified-map.v1\0",
                    output_source_space_token.as_bytes(),
                    entry.source_space_token.as_bytes(),
                    entry
                        .source_map_hash
                        .as_deref()
                        .unwrap_or("unmapped")
                        .as_bytes(),
                ])),
                destination_space_token: output_source_space_token.clone(),
                declared_space_tokens: vec![entry.source_space_token.clone()],
                raw_map: entry.source_map.clone(),
            };
            let content_artifact_token = BlockContentArtifactToken::mint(hash_parts(&[
                b"verter.block-content.artifact.supplied.v1\0",
                entry.artifact_token.as_bytes(),
                entry.block_token.as_bytes(),
                entry.basis_token.as_bytes(),
                output_source_space_token.as_bytes(),
                entry.code_hash.as_bytes(),
                qualified_source_map.map_hash.as_bytes(),
            ]));
            let output_descriptor = BlockContentSourceSpaceDescriptor {
                token: output_source_space_token.clone(),
                kind: BlockContentSourceSpaceKind::DerivedTransform,
                source_token: BlockContentArtifactToken::mint(entry.basis_token.to_string()),
                content_hash: entry.code_hash.clone(),
                utf8_byte_len: entry.code.len() as u64,
            };
            #[cfg(test)]
            {
                let hook = self.block_content.admission_seam_hook.lock().clone();
                if let Some(hook) = hook {
                    hook();
                }
            }
            admitted.push((
                entry.correlation_token,
                entry.block_token,
                entry.captured_echo,
                SuppliedContentArtifact {
                    owner_revision: entry.owner_revision,
                    artifact_token: entry.artifact_token,
                    basis_token: entry.basis_token,
                    input_source_space_token: entry.source_space_token,
                    output_source_space_token,
                    content_artifact_token,
                    output_descriptor,
                    qualified_source_map,
                    code: entry.code,
                    code_hash: entry.code_hash,
                    source_map: entry.source_map,
                    source_map_hash: entry.source_map_hash,
                    dependencies: entry.dependencies,
                    diagnostics: entry.diagnostics,
                    processor_identity: entry.processor_identity,
                    processor_version: entry.processor_version,
                    config_fingerprint: entry.config_fingerprint,
                },
            ));
        }
        let mut state = write_lock(&self.block_content.state);
        for (correlation, block_token, captured_echo, artifact) in admitted {
            state.remove_pending(&correlation);
            state.mark_terminal(
                correlation,
                BlockContentResolveTerminal::PostCapture {
                    echo: captured_echo,
                    outcome: BlockContentPostCaptureTerminal::Admitted,
                },
            );
            state.insert_supplied((canonical_id.clone(), profile_hash, block_token), artifact);
        }
        drop(state);
        if let Some(mut compile_cache) = self.compile_cache().get_mut(&canonical_id) {
            let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
            session_node.clear_compile_outputs_for_file(&mut compile_cache);
        }
        self.compile_output_pure_content()
            .remove_canonical(&canonical_id);
        let mut result = HostUpdateResult::no_change(canonical_id);
        result.changed = true;
        self.bump_store_view_epoch();
        Ok(result)
    }
}

pub(crate) fn attach_external_request_tokens(
    structure: &crate::carrier_publication_store::RegisteredFileStructure,
    revision: crate::carrier_publication_store::HostSourceRevisionToken,
    requests: &mut [ExternalSourceRequest],
) {
    let inventory = structure.inventory();
    for request in requests {
        let content_class = match request.block_kind {
            ExternalBlockKind::Template => BlockContentClass::Template,
            ExternalBlockKind::Script => BlockContentClass::Script,
            ExternalBlockKind::Style => BlockContentClass::Style,
            ExternalBlockKind::Custom => BlockContentClass::Custom,
        };
        let block = inventory.blocks().iter().find(|block| {
            matches!(block, CarrierBlock::Section { role, syntax, .. }
                    if role_class(role) == content_class
                        && syntax.opening_span.start == request.opening_start)
        });
        let Some(CarrierBlock::Section { id, syntax, .. }) = block else {
            continue;
        };
        let Some(block_ref) = structure.block_ref(*id) else {
            continue;
        };
        request.block_token = structure
            .public_block_token(&block_ref)
            .map(|token| token.as_str().to_string())
            .unwrap_or_default();
        request.owner_revision = revision.public_token();
        request.artifact_token = structure.public_artifact_token().as_str().to_string();
        request.carrier_source_space_token = structure
            .public_source_space_token(syntax.content_span.source_space)
            .map(|token| token.as_str().to_string())
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Barrier};
    use std::time::Duration;

    fn pending_fixture(index: usize) -> (BlockContentCorrelationToken, PendingCorrelation) {
        let correlation = BlockContentCorrelationToken::mint(format!("correlation-{index}"));
        let block_token =
            crate::carrier_publication_store::ArtifactBlockToken::parse_untrusted("block").unwrap();
        let owner_revision = BlockContentOwnerRevisionToken::mint("owner".to_string());
        let artifact_token =
            crate::carrier_publication_store::FrameworkArtifactToken::parse_untrusted("artifact")
                .unwrap();
        let basis_token = BlockContentBasisToken::mint(format!("basis-{index}"));
        let captured_echo = BlockContentCapturedEcho {
            request: BlockContentPreCaptureEcho {
                correlation_token: correlation.clone(),
                canonical_id: "/bounded.vue".to_string(),
                block_token: block_token.clone(),
                owner_revision: owner_revision.clone(),
                artifact_token: artifact_token.clone(),
                expected_language: "pug".to_string(),
                prior_basis_token: None,
            },
            basis_token: basis_token.clone(),
        };
        (
            correlation,
            PendingCorrelation {
                canonical_id: "/bounded.vue".to_string(),
                block_token,
                owner_revision,
                artifact_token,
                basis_token,
                source_space_token: BlockContentSourceSpaceToken::mint("space".to_string()),
                content_hash: hash_block_content("p bounded"),
                input_content: Arc::from("p bounded"),
                captured_echo,
            },
        )
    }

    #[test]
    fn pending_and_terminal_correlation_state_is_strictly_bounded() {
        let mut state = BlockContentState::default();
        for index in 0..(MAX_PENDING_CORRELATIONS + MAX_TERMINAL_CORRELATIONS + 64) {
            let (token, pending) = pending_fixture(index);
            state.insert_pending(token, pending);
        }
        assert_eq!(state.pending.len(), MAX_PENDING_CORRELATIONS);
        assert_eq!(state.pending_order.len(), MAX_PENDING_CORRELATIONS);
        assert_eq!(state.terminal.len(), MAX_TERMINAL_CORRELATIONS);
        assert_eq!(state.terminal_order.len(), MAX_TERMINAL_CORRELATIONS);
        assert!(state.terminal.values().all(|terminal| matches!(
            terminal,
            BlockContentResolveTerminal::PostCapture {
                outcome: BlockContentPostCaptureTerminal::Cancelled,
                ..
            }
        )));

        let live_tokens = state.pending.keys().cloned().collect::<Vec<_>>();
        for token in live_tokens {
            assert!(state.remove_pending(&token).is_some());
        }
        assert!(state.pending.is_empty());
        assert!(state.pending_order.is_empty());
    }

    fn supplied_fixture(code: Arc<str>) -> SuppliedContentArtifact {
        let input_space = BlockContentSourceSpaceToken::mint("input-space".to_string());
        let output_space = BlockContentSourceSpaceToken::mint("output-space".to_string());
        let code_hash = hash_block_content(&code);
        SuppliedContentArtifact {
            owner_revision: BlockContentOwnerRevisionToken::mint("owner".to_string()),
            artifact_token:
                crate::carrier_publication_store::FrameworkArtifactToken::parse_untrusted(
                    "artifact",
                )
                .unwrap(),
            basis_token: BlockContentBasisToken::mint("basis".to_string()),
            input_source_space_token: input_space.clone(),
            output_source_space_token: output_space.clone(),
            content_artifact_token: BlockContentArtifactToken::mint("content-artifact".to_string()),
            output_descriptor: BlockContentSourceSpaceDescriptor {
                token: output_space.clone(),
                kind: BlockContentSourceSpaceKind::DerivedTransform,
                source_token: BlockContentArtifactToken::mint("basis".to_string()),
                content_hash: code_hash.clone(),
                utf8_byte_len: code.len() as u64,
            },
            qualified_source_map: QualifiedBlockContentSourceMap {
                map_hash: BlockContentHashToken::mint("map-hash".to_string()),
                destination_space_token: output_space,
                declared_space_tokens: vec![input_space],
                raw_map: None,
            },
            code,
            code_hash,
            source_map: None,
            source_map_hash: None,
            dependencies: Vec::new(),
            diagnostics: Vec::new(),
            processor_identity: String::new(),
            processor_version: String::new(),
            config_fingerprint: None,
        }
    }

    #[test]
    fn supplied_artifacts_are_bounded_across_compile_profiles_and_bytes() {
        let block_token =
            crate::carrier_publication_store::ArtifactBlockToken::parse_untrusted("block").unwrap();
        let fixture = supplied_fixture(Arc::from(""));
        let mut state = BlockContentState::default();
        for profile_hash in 0..=(MAX_SUPPLIED_ARTIFACTS as u64) {
            state.insert_supplied(
                (
                    "/bounded.vue".to_string(),
                    profile_hash,
                    block_token.clone(),
                ),
                fixture.clone(),
            );
        }
        assert_eq!(state.supplied.len(), MAX_SUPPLIED_ARTIFACTS);
        assert_eq!(state.supplied_order.len(), MAX_SUPPLIED_ARTIFACTS);
        assert!(!state.supplied.contains_key(&(
            "/bounded.vue".to_string(),
            0,
            block_token.clone()
        )));

        let shared_code: Arc<str> = Arc::from("x".repeat(MAX_SUPPLIED_CODE_BYTES));
        let large_fixture = supplied_fixture(shared_code);
        let mut byte_bounded = BlockContentState::default();
        for profile_hash in 0..5 {
            byte_bounded.insert_supplied(
                (
                    "/byte-bounded.vue".to_string(),
                    profile_hash,
                    block_token.clone(),
                ),
                large_fixture.clone(),
            );
        }
        assert!(byte_bounded.supplied_bytes <= MAX_SUPPLIED_TOTAL_BYTES);
        assert_eq!(byte_bounded.supplied.len(), 4);
        assert_eq!(byte_bounded.supplied_order.len(), 4);
    }

    #[test]
    fn post_capture_payload_validation_terminalizes_the_exact_captured_echo() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/MalformedAfterCapture.vue".to_string(),
                source: Arc::from("<template lang=\"pug\">p captured</template>"),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let request = update.preprocessor_requests[0].clone();
        let mut entry = BlockOverrideEntry::supplied_for_test(&request, "<p>supplied</p>");
        entry.code_hash = BlockContentHashToken::mint("invalid\ntoken".to_string());

        let error = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: update.canonical_id,
                compile_profile: CompileProfile::default(),
                overrides: vec![entry],
            })
            .unwrap_err();
        assert!(matches!(
            error,
            HostError::BlockContentRefused(BlockContentRefusal::MalformedToken)
        ));

        let state = read_lock(&host.block_content.state);
        assert!(!state.pending.contains_key(&request.correlation_token));
        assert_eq!(
            state.terminal.get(&request.correlation_token),
            Some(&BlockContentResolveTerminal::PostCapture {
                echo: request.captured_echo,
                outcome: BlockContentPostCaptureTerminal::Failed(
                    BlockContentRefusal::MalformedToken,
                ),
            })
        );

        drop(state);
        let provenance_host = VerterHost::new_standalone(HostConfig::default());
        let provenance_update = provenance_host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/OversizedProvenance.vue".to_string(),
                source: Arc::from("<template lang=\"pug\">p captured</template>"),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let provenance_request = provenance_update.preprocessor_requests[0].clone();
        let mut provenance_entry =
            BlockOverrideEntry::supplied_for_test(&provenance_request, "<p>supplied</p>");
        provenance_entry.processor_identity = "x".repeat(MAX_SUPPLIED_PROVENANCE_BYTES + 1);
        let error = provenance_host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: provenance_update.canonical_id,
                compile_profile: CompileProfile::default(),
                overrides: vec![provenance_entry],
            })
            .unwrap_err();
        assert!(matches!(
            error,
            HostError::BlockContentRefused(BlockContentRefusal::PayloadTooLarge)
        ));
        assert_eq!(
            read_lock(&provenance_host.block_content.state)
                .terminal
                .get(&provenance_request.correlation_token),
            Some(&BlockContentResolveTerminal::PostCapture {
                echo: provenance_request.captured_echo,
                outcome: BlockContentPostCaptureTerminal::Failed(
                    BlockContentRefusal::PayloadTooLarge,
                ),
            })
        );
    }

    fn pending_template(path: &str) -> (Arc<VerterHost>, HostUpdateResult, PreprocessorRequest) {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: path.to_string(),
                source: Arc::from("<template lang=\"pug\">p captured</template>"),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let request = update.preprocessor_requests[0].clone();
        (host, update, request)
    }

    fn assert_post_capture_terminal(
        host: &VerterHost,
        request: &PreprocessorRequest,
        outcome: BlockContentPostCaptureTerminal,
    ) {
        let state = read_lock(&host.block_content.state);
        assert!(!state.pending.contains_key(&request.correlation_token));
        assert_eq!(
            state.terminal.get(&request.correlation_token),
            Some(&BlockContentResolveTerminal::PostCapture {
                echo: request.captured_echo.clone(),
                outcome,
            })
        );
    }

    #[test]
    fn hash_map_duplicate_and_stale_refusals_are_exact_post_capture_terminals() {
        let (hash_host, hash_update, hash_request) =
            pending_template("/workspace/HashTerminal.vue");
        let mut bad_hash = BlockOverrideEntry::supplied_for_test(&hash_request, "<p>supplied</p>");
        bad_hash.code_hash = hash_block_content("different bytes");
        let error = hash_host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: hash_update.canonical_id,
                compile_profile: CompileProfile::default(),
                overrides: vec![bad_hash],
            })
            .unwrap_err();
        assert!(matches!(
            error,
            HostError::BlockContentRefused(BlockContentRefusal::CodeHashMismatch)
        ));
        assert_post_capture_terminal(
            &hash_host,
            &hash_request,
            BlockContentPostCaptureTerminal::Failed(BlockContentRefusal::CodeHashMismatch),
        );

        let (map_host, map_update, map_request) = pending_template("/workspace/MapTerminal.vue");
        let mut bad_map = BlockOverrideEntry::supplied_for_test(&map_request, "<p>supplied</p>");
        let invalid_map: Arc<str> = Arc::from(
            "{\"version\":3,\"sources\":[\"input.pug\"],\"names\":[],\"mappings\":\"!!!\"}",
        );
        bad_map.source_map_hash = Some(hash_block_content(&invalid_map));
        bad_map.source_map = Some(invalid_map);
        let error = map_host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: map_update.canonical_id,
                compile_profile: CompileProfile::default(),
                overrides: vec![bad_map],
            })
            .unwrap_err();
        assert!(matches!(
            error,
            HostError::BlockContentRefused(BlockContentRefusal::InvalidSourceMap)
        ));
        assert_post_capture_terminal(
            &map_host,
            &map_request,
            BlockContentPostCaptureTerminal::Failed(BlockContentRefusal::InvalidSourceMap),
        );

        let (duplicate_host, duplicate_update, duplicate_request) =
            pending_template("/workspace/DuplicateTerminal.vue");
        let duplicate =
            BlockOverrideEntry::supplied_for_test(&duplicate_request, "<p>supplied</p>");
        let error = duplicate_host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: duplicate_update.canonical_id,
                compile_profile: CompileProfile::default(),
                overrides: vec![duplicate.clone(), duplicate],
            })
            .unwrap_err();
        assert!(matches!(
            error,
            HostError::BlockContentRefused(BlockContentRefusal::DuplicateBlock)
        ));
        assert_post_capture_terminal(
            &duplicate_host,
            &duplicate_request,
            BlockContentPostCaptureTerminal::Failed(BlockContentRefusal::DuplicateBlock),
        );
        assert!(read_lock(&duplicate_host.block_content.state)
            .supplied
            .is_empty());

        let (stale_host, stale_update, stale_request) =
            pending_template("/workspace/StaleTerminal.vue");
        let _ = stale_host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: stale_update.canonical_id,
                source: Arc::from("<template lang=\"pug\">p replacement</template>"),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        assert_post_capture_terminal(
            &stale_host,
            &stale_request,
            BlockContentPostCaptureTerminal::Superseded,
        );
    }

    /// `valid_source_map_v3` decodes through `oxc_sourcemap`, which validates
    /// VLQ-stream well-formedness but NOT that a decoded token's position
    /// actually falls inside the content it claims to point into — that is
    /// the `position_within_utf16_content` bounds check this function layers
    /// on top. A syntactically valid, cleanly-decodable map whose one token
    /// points past the end of the destination content must still be
    /// rejected; the SAME map re-pointed at an in-bounds column must be
    /// accepted — proving the assertion discriminates on the bounds check
    /// specifically, not on decodability.
    ///
    /// This bounds check predates the `oxc_sourcemap` consolidation. The
    /// constructor/decoder calls were rewritten onto that API
    /// (compile-enforced); the rejection this test proves is not a
    /// consolidation discriminator.
    #[test]
    fn valid_source_map_v3_rejects_a_decodable_map_with_an_out_of_bounds_token() {
        let input_content = "let x = 1;";
        let output_content = "const x = 1;";

        let make_map = |dst_col: u32| {
            let token = oxc_sourcemap::Token::new(0, dst_col, 0, 0, Some(0), None);
            let sm = oxc_sourcemap::SourceMap::new(
                None,
                vec![],
                None,
                vec![std::borrow::Cow::Borrowed("input.ts")],
                vec![Some(std::borrow::Cow::Borrowed(input_content))],
                vec![token].into_boxed_slice(),
                None,
            );
            sm.to_json_string()
        };

        // `output_content` is 12 UTF-16 units long: column 12 is the exact
        // end-of-line boundary (valid, one-past-the-last-char), column 13
        // is one UTF-16 unit past it.
        let in_bounds = make_map(12);
        assert!(
            oxc_sourcemap::SourceMap::from_json_string(&in_bounds).is_ok(),
            "the constructed map must itself decode cleanly — this test isolates the \
             bounds check, not VLQ well-formedness"
        );
        assert!(
            valid_source_map_v3(&in_bounds, input_content, output_content),
            "a token at the exact end-of-content column is in bounds and must be accepted"
        );

        let out_of_bounds = make_map(13);
        assert!(
            oxc_sourcemap::SourceMap::from_json_string(&out_of_bounds).is_ok(),
            "the out-of-bounds variant must ALSO decode cleanly, so a rejection can only \
             come from the bounds check"
        );
        assert!(
            !valid_source_map_v3(&out_of_bounds, input_content, output_content),
            "a token one UTF-16 unit past the destination content's end must be rejected"
        );
    }

    /// `oxc_sourcemap`'s decoder has no `sections` concept and silently
    /// ignores an unrecognized `sections` field as ordinary unknown JSON —
    /// it does not itself reject an indexed source map the way the old
    /// `sourcemap` crate's `SourceMap::from_slice`/`from_reader` did (that
    /// crate detects `sections.is_some()` and returns
    /// `Error::IncompatibleSourceMap` for anything but a `Regular` map).
    /// Without an explicit `sections` check in `valid_source_map_v3`, a
    /// syntactically-valid-looking map carrying a `sections` array — the
    /// hallmark of an indexed map this validator is not equipped to
    /// interpret — would be silently accepted using only its top-level
    /// `sources`/`mappings`, discarding the `sections` payload instead of
    /// being refused. Proves the explicit `sections`-presence check, not
    /// just decodability, drives the rejection: an identical map with the
    /// `sections` key removed must be accepted.
    #[test]
    fn valid_source_map_v3_rejects_a_map_carrying_a_sections_member() {
        let input_content = "let x = 1;";
        let output_content = "const x = 1;";

        let with_sections = serde_json::json!({
            "version": 3,
            "sources": ["input.ts"],
            "sourcesContent": [input_content],
            "names": [],
            "mappings": "",
            "sections": [],
        })
        .to_string();
        assert!(
            oxc_sourcemap::SourceMap::from_json_string(&with_sections).is_ok(),
            "the map must itself decode cleanly through oxc_sourcemap — a rejection can \
             only come from the explicit sections check, not from decode failure"
        );
        assert!(
            !valid_source_map_v3(&with_sections, input_content, output_content),
            "a map carrying a `sections` member is an indexed map and must be rejected, \
             matching the old sourcemap crate's IncompatibleSourceMap behavior"
        );

        let without_sections = serde_json::json!({
            "version": 3,
            "sources": ["input.ts"],
            "sourcesContent": [input_content],
            "names": [],
            "mappings": "",
        })
        .to_string();
        assert!(
            valid_source_map_v3(&without_sections, input_content, output_content),
            "the identical map with the `sections` key removed must be accepted — proving \
             the rejection above is driven by `sections` presence, not some other field"
        );
    }

    /// `oxc_sourcemap` has no `rangeMappings` field at all — it is a
    /// non-standard Sentry extension, not part of the V3 spec `oxc_sourcemap`
    /// implements. serde silently drops it as an unknown field. So a map
    /// carrying `rangeMappings`, malformed or not, would — without an
    /// explicit check — now silently decode as valid, the same
    /// permissive-drift class as the `sections` gap above. Proves the
    /// rejection is driven by `rangeMappings` presence, not decodability: the
    /// map decodes cleanly through `oxc_sourcemap` (which never looks at the
    /// field), and the identical map with `rangeMappings` removed is
    /// accepted.
    ///
    /// The `mappings` line here must be non-empty and valid VLQ (`"AAAA"`):
    /// the old `sourcemap` 9.3 crate's `decode_regular` skips a line's
    /// segments entirely via `continue` when that line is empty, so an
    /// empty `mappings` string never reaches `decode_rmi` and the old
    /// validator would have accepted a malformed `rangeMappings` paired
    /// with it — the fixture must actually exercise the old decoder's
    /// `rangeMappings` validation path, not bypass it.
    #[test]
    fn valid_source_map_v3_rejects_a_map_carrying_a_range_mappings_member() {
        let input_content = "let x = 1;";
        let output_content = "const x = 1;";

        let with_range_mappings = serde_json::json!({
            "version": 3,
            "sources": ["input.ts"],
            "sourcesContent": [input_content],
            "names": [],
            "mappings": "AAAA",
            // A malformed range-mappings VLQ segment: the old `sourcemap`
            // crate's `decode_rmi` rejects "!" as invalid base64, but
            // `oxc_sourcemap` never reads this field, so decodability alone
            // cannot be what causes the rejection asserted below.
            "rangeMappings": "!",
        })
        .to_string();
        assert!(
            oxc_sourcemap::SourceMap::from_json_string(&with_range_mappings).is_ok(),
            "the map must itself decode cleanly through oxc_sourcemap — a rejection can \
             only come from the explicit rangeMappings check, not from decode failure"
        );
        assert!(
            !valid_source_map_v3(&with_range_mappings, input_content, output_content),
            "a map carrying a `rangeMappings` member is rejected, matching the old \
             sourcemap crate's malformed-VLQ decode-error behavior"
        );

        let without_range_mappings = serde_json::json!({
            "version": 3,
            "sources": ["input.ts"],
            "sourcesContent": [input_content],
            "names": [],
            "mappings": "AAAA",
        })
        .to_string();
        assert!(
            valid_source_map_v3(&without_range_mappings, input_content, output_content),
            "the identical map with the `rangeMappings` key removed must be accepted — \
             proving the rejection above is driven by `rangeMappings` presence, not some \
             other field"
        );
    }

    #[test]
    fn owner_publication_cannot_land_between_post_await_validation_and_admission() {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let first = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/Fenced.vue".to_string(),
                source: Arc::from("<template lang=\"pug\">p first</template>"),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let request = first.preprocessor_requests[0].clone();

        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            *host.block_content.admission_seam_hook.lock() = Some(Arc::new(move || {
                entered.wait();
                release.wait();
            }));
        }

        let apply_host = Arc::clone(&host);
        let canonical = first.canonical_id.clone();
        let apply = std::thread::spawn(move || {
            apply_host.apply_block_overrides(BlockOverrideRequest {
                canonical_id: canonical,
                compile_profile: CompileProfile::default(),
                overrides: vec![BlockOverrideEntry::supplied_for_test(
                    &request,
                    "<p>first supplied</p>",
                )],
            })
        });
        entered.wait();

        let upsert_host = Arc::clone(&host);
        let (done_tx, done_rx) = mpsc::channel();
        let upsert = std::thread::spawn(move || {
            let result = upsert_host.upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/Fenced.vue".to_string(),
                source: Arc::from("<template lang=\"pug\">p second</template>"),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            });
            done_tx.send(()).unwrap();
            result
        });
        let publication_was_blocked = matches!(
            done_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );

        release.wait();
        let _ = apply.join().unwrap().unwrap();
        let second = upsert.join().unwrap().unwrap();
        if publication_was_blocked {
            done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        }
        *host.block_content.admission_seam_hook.lock() = None;
        assert!(
            publication_was_blocked,
            "owner publication crossed the block-content admission fence"
        );

        let live = &second.preprocessor_requests[0];
        let selected = host
            .get_block_content(BlockContentQuery {
                canonical_id: second.canonical_id,
                block_token: live.block_token.to_string(),
                compile_profile: CompileProfile::default(),
                expected_basis_token: None,
            })
            .unwrap();
        assert_eq!(
            selected.availability,
            BlockContentAvailability::ProcessedContentRequired
        );
        assert_eq!(selected.content.as_deref(), Some("p second"));
    }

    #[test]
    fn supplied_change_during_compile_cannot_publish_stale_output() {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        // This test exercises the supplied-content publish fence, not Vue's
        // missing-entry diagnostic. Keep the carrier valid while retaining the
        // single preprocessed style block that drives the race below.
        let source =
            "<template></template><style lang=\"customcss\">.authored { color: red }</style>";
        let first = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: "/workspace/CompileFence.vue".to_string(),
                source: Arc::from(source),
                file_language: verter_language::FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .unwrap();
        let profile = CompileProfile::default();
        let first_request = first.preprocessor_requests[0].clone();
        let _ = host
            .apply_block_overrides(BlockOverrideRequest {
                canonical_id: first.canonical_id.clone(),
                compile_profile: profile.clone(),
                overrides: vec![BlockOverrideEntry::supplied_for_test(
                    &first_request,
                    ".first-supplied { color: red }",
                )],
            })
            .unwrap();

        // Model a second in-flight preprocessor await captured against the
        // exact same owner/basis. The public upsert response intentionally
        // emits one request per capture, so this unit-level seam installs the
        // second captured correlation explicitly without moving the owner.
        let second_correlation =
            BlockContentCorrelationToken::mint("compile-race-second".to_string());
        let mut second_echo = first_request.captured_echo.clone();
        second_echo.request.correlation_token = second_correlation.clone();
        write_lock(&host.block_content.state).insert_pending(
            second_correlation.clone(),
            PendingCorrelation {
                canonical_id: first.canonical_id.clone(),
                block_token: first_request.block_token.clone(),
                owner_revision: first_request.owner_revision.clone(),
                artifact_token: first_request.artifact_token.clone(),
                basis_token: first_request.basis_token.clone(),
                source_space_token: first_request.source_space_token.clone(),
                content_hash: first_request.content_hash.clone(),
                input_content: Arc::from(first_request.content.as_str()),
                captured_echo: second_echo.clone(),
            },
        );
        let mut second_entry = BlockOverrideEntry::supplied_for_test(
            &first_request,
            ".second-supplied { color: blue }",
        );
        second_entry.correlation_token = second_correlation;
        second_entry.captured_echo = second_echo;
        {
            let hook_host = Arc::clone(&host);
            let hook_profile = profile.clone();
            let fired = std::sync::atomic::AtomicBool::new(false);
            *host.compile_publish_seam_hook.lock() = Some(Arc::new(move || {
                if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    let _ = hook_host
                        .apply_block_overrides(BlockOverrideRequest {
                            canonical_id: "/workspace/CompileFence.vue".to_string(),
                            compile_profile: hook_profile.clone(),
                            overrides: vec![second_entry.clone()],
                        })
                        .unwrap();
                }
            }));
        }

        let style_query = || VirtualQuery {
            raw_id: None,
            canonical_id: Some("/workspace/CompileFence.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: profile.clone(),
        };
        let raced = host.get_virtual_file(style_query()).unwrap();
        *host.compile_publish_seam_hook.lock() = None;
        assert!(raced.code.contains("first-supplied"));

        let recovered = host.get_virtual_file(style_query()).unwrap();
        assert!(recovered.code.contains("second-supplied"));
        assert!(
            !recovered.cache_hit,
            "the stale first compile must not republish after the second apply cleared its slot"
        );
        let warm = host.get_virtual_file(style_query()).unwrap();
        assert!(warm.code.contains("second-supplied"));
        assert!(warm.cache_hit);
    }
}
