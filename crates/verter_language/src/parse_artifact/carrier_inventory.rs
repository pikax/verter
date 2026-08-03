use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use crate::registered_source_authority::{
    RegisteredSourceSnapshot, RegisteredSourceSnapshotId, WholeSourceHash,
};
use crate::ScriptSourceType as RegistryScriptSourceType;

macro_rules! local_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);
        impl $name {
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

local_id!(SourceSpaceId);
local_id!(BlockId);
local_id!(MarkupNodeId);
local_id!(AttributeId);
local_id!(InternedNameId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub source_space: SourceSpaceId,
    pub start: u32,
    pub end: u32,
}
impl SourceSpan {
    pub const fn new(source_space: SourceSpaceId, start: u32, end: u32) -> Self {
        Self {
            source_space,
            start,
            end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSlice {
    pub span: SourceSpan,
}
impl SourceSlice {
    pub const fn new(span: SourceSpan) -> Self {
        Self { span }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceEncoding {
    Utf8,
}

/// Sealed artifact-bound block reference.
///
/// Fields are private by design: a ref is minted ONLY by its owning
/// [`CarrierBlockInventory`] (see [`CarrierBlockInventory::block_ref`]), so a
/// local block id can never be spliced onto a different artifact and a naked
/// integer can never masquerade as block identity. Consumers associate data
/// through full-identity equality (artifact identity + block id) and
/// re-validate against a live inventory via [`ArtifactBlockRef::validate`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactBlockRef {
    artifact_identity: Arc<str>,
    block: BlockId,
}

impl ArtifactBlockRef {
    /// Content-addressed identity of the artifact that minted this ref.
    pub fn artifact_identity(&self) -> &Arc<str> {
        &self.artifact_identity
    }

    /// Artifact-local block id. Span metadata for the owning artifact only —
    /// never a cross-artifact join key on its own.
    pub fn block_id(&self) -> BlockId {
        self.block
    }

    /// True iff this ref was minted by an inventory with `owner`'s identity
    /// AND still names a block present in `owner`. Fails closed on any
    /// mismatch (stale artifact, foreign artifact, out-of-range block).
    pub fn validate(&self, owner: &CarrierBlockInventory) -> bool {
        (owner.blocks.len() > self.block.get() as usize)
            && *self.artifact_identity == **owner.artifact_identity_token()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockContentOriginFingerprint(pub [u8; 32]);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformChainFingerprint(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceSpaceIdentity {
    RegisteredSnapshot {
        snapshot: RegisteredSourceSnapshotId,
    },
    DerivedTransformOutput {
        owner: ArtifactBlockRef,
        origin_fingerprint: BlockContentOriginFingerprint,
        transform_chain_fingerprint: TransformChainFingerprint,
        step_index: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceSpaceDescriptor {
    pub id: SourceSpaceId,
    pub identity: SourceSpaceIdentity,
    pub content_hash: WholeSourceHash,
    pub byte_len: u32,
    pub encoding: SourceEncoding,
    pub bytes: Arc<str>,
}
impl SourceSpaceDescriptor {
    pub fn registered(id: SourceSpaceId, snapshot: &RegisteredSourceSnapshot) -> Self {
        Self {
            id,
            identity: SourceSpaceIdentity::RegisteredSnapshot {
                snapshot: snapshot.snapshot_id().clone(),
            },
            content_hash: snapshot.content_hash(),
            byte_len: snapshot.byte_len(),
            encoding: SourceEncoding::Utf8,
            bytes: Arc::clone(snapshot.source_arc()),
        }
    }
    pub fn bytes(&self) -> &Arc<str> {
        &self.bytes
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct NormalizedNameTable {
    pub values: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecodedValueKey {
    pub raw: SourceSlice,
    pub recipe: EntityDecodeRecipe,
}

#[derive(Debug, Clone, Default)]
pub struct CarrierBlockInventory {
    source_spaces: Arc<[SourceSpaceDescriptor]>,
    normalized_names: Arc<NormalizedNameTable>,
    blocks: Arc<[CarrierBlock]>,
    markup: Arc<MarkupSyntaxArena>,
    /// Lazily computed content-addressed artifact identity (memo only —
    /// derived entirely from the fields above, excluded from equality).
    artifact_identity: std::sync::OnceLock<Arc<str>>,
}

impl PartialEq for CarrierBlockInventory {
    fn eq(&self, other: &Self) -> bool {
        self.source_spaces == other.source_spaces
            && self.normalized_names == other.normalized_names
            && self.blocks == other.blocks
            && self.markup == other.markup
    }
}

impl Eq for CarrierBlockInventory {}

impl CarrierBlockInventory {
    pub fn new(
        source_spaces: Arc<[SourceSpaceDescriptor]>,
        normalized_names: Arc<NormalizedNameTable>,
        blocks: Arc<[CarrierBlock]>,
        markup: Arc<MarkupSyntaxArena>,
    ) -> Result<Self, InventoryValidationError> {
        let inventory = Self {
            source_spaces,
            normalized_names,
            blocks,
            markup,
            artifact_identity: std::sync::OnceLock::new(),
        };
        inventory.validate()?;
        Ok(inventory)
    }

    /// Content-addressed identity of this inventory: a digest over every
    /// source space's identity-bearing content facts plus the carrier
    /// structure hash. Generation/incarnation facts are deliberately
    /// EXCLUDED — the same bytes with the same block geometry yield the same
    /// identity (path-independent), while any content or geometry change
    /// yields a different one.
    pub fn artifact_identity_token(&self) -> &Arc<str> {
        self.artifact_identity.get_or_init(|| {
            let mut out = Vec::new();
            out.extend_from_slice(b"verter.sealed-artifact-block-identity.v1\0");
            out.extend_from_slice(&(self.source_spaces.len() as u32).to_le_bytes());
            for space in self.source_spaces.iter() {
                out.extend_from_slice(space.content_hash.as_bytes());
                out.extend_from_slice(&space.byte_len.to_le_bytes());
                out.push(match space.encoding {
                    SourceEncoding::Utf8 => 1,
                });
                match &space.identity {
                    SourceSpaceIdentity::RegisteredSnapshot { snapshot } => {
                        out.push(1);
                        // Language projection only — content-addressed, so
                        // generation/incarnation/authority stay excluded.
                        out.extend_from_slice(
                            format!("{:?}", snapshot.resolved_file_language()).as_bytes(),
                        );
                        out.push(0);
                    }
                    SourceSpaceIdentity::DerivedTransformOutput {
                        owner,
                        origin_fingerprint,
                        transform_chain_fingerprint,
                        step_index,
                    } => {
                        out.push(2);
                        out.extend_from_slice(owner.artifact_identity.as_bytes());
                        out.push(0);
                        out.extend_from_slice(&owner.block.get().to_le_bytes());
                        out.extend_from_slice(&origin_fingerprint.0);
                        out.extend_from_slice(&transform_chain_fingerprint.0);
                        out.extend_from_slice(&step_index.to_le_bytes());
                    }
                }
            }
            out.extend_from_slice(
                crate::parse_artifact::carrier_structure_hash::compute_carrier_structure_hash(self)
                    .as_bytes(),
            );
            let digest = crate::registered_source_authority::sha256(&[&out]);
            let mut token = String::with_capacity(64);
            for byte in digest {
                use std::fmt::Write;
                let _ = write!(token, "{byte:02x}");
            }
            Arc::from(token)
        })
    }

    /// The SOLE mint authority for sealed block refs: seals
    /// `(artifact identity, block)` only when `block` names a block present
    /// in this inventory.
    pub fn block_ref(&self, block: BlockId) -> Option<ArtifactBlockRef> {
        self.blocks.get(block.get() as usize)?;
        Some(ArtifactBlockRef {
            artifact_identity: Arc::clone(self.artifact_identity_token()),
            block,
        })
    }
    pub fn source_spaces(&self) -> &[SourceSpaceDescriptor] {
        &self.source_spaces
    }
    pub fn normalized_names(&self) -> &NormalizedNameTable {
        &self.normalized_names
    }
    pub fn blocks(&self) -> &[CarrierBlock] {
        &self.blocks
    }
    pub fn markup(&self) -> &MarkupSyntaxArena {
        &self.markup
    }
    pub fn slice(&self, slice: SourceSlice) -> Result<&str, InventoryValidationError> {
        self.slice_span(slice.span)
    }
    pub fn slice_span(&self, span: SourceSpan) -> Result<&str, InventoryValidationError> {
        let source = self.source_spaces.get(span.source_space.0 as usize).ok_or(
            InventoryValidationError::UnknownSourceSpace(span.source_space),
        )?;
        source
            .bytes
            .get(span.start as usize..span.end as usize)
            .ok_or(InventoryValidationError::InvalidSpan(span))
    }
    pub fn normalized_name(&self, id: InternedNameId) -> Result<&str, InventoryValidationError> {
        self.normalized_names
            .values
            .get(id.0 as usize)
            .map(AsRef::as_ref)
            .ok_or(InventoryValidationError::UnknownNormalizedName(id))
    }
    pub fn block_start(&self, block: &CarrierBlock) -> Result<u32, InventoryValidationError> {
        match block {
            CarrierBlock::Section { syntax, .. } => Ok(syntax.full_span.start),
            CarrierBlock::MarkupRoot { node, .. } => self
                .markup
                .nodes
                .get(node.0 as usize)
                .map(|node| node.kind.full_span().start)
                .ok_or(InventoryValidationError::UnknownNode(*node)),
        }
    }
    pub fn decode_attribute_value(
        &self,
        attribute: &CarrierAttribute,
    ) -> Result<Option<Cow<'_, str>>, InventoryValidationError> {
        let value = match attribute {
            CarrierAttribute::Named { value, .. } | CarrierAttribute::Directive { value, .. } => {
                value
            }
            CarrierAttribute::Spread { .. } | CarrierAttribute::Attach { .. } => return Ok(None),
        };
        match value {
            AttributeValue::Static { raw, decoded, .. } => match decoded {
                LazyDecodedText::SameAsSource => Ok(Some(Cow::Borrowed(self.slice(*raw)?))),
                LazyDecodedText::EntityDecode { key } => {
                    Ok(Some(decode_entities(self.slice(key.raw)?, key.recipe)))
                }
            },
            AttributeValue::Missing
            | AttributeValue::Expression { .. }
            | AttributeValue::Mixed { .. } => Ok(None),
        }
    }
    pub fn validate(&self) -> Result<(), InventoryValidationError> {
        for (index, name) in self.normalized_names.values.iter().enumerate() {
            if self.normalized_names.values[..index].contains(name) {
                return Err(InventoryValidationError::DuplicateNormalizedName(
                    InternedNameId(index as u32),
                ));
            }
        }
        for (index, source) in self.source_spaces.iter().enumerate() {
            if source.id.0 as usize != index {
                return Err(InventoryValidationError::SourceSpaceIdMismatch {
                    expected: SourceSpaceId(index as u32),
                    actual: source.id,
                });
            }
            if source.byte_len as usize != source.bytes.len() {
                return Err(InventoryValidationError::SourceLengthMismatch(source.id));
            }
            if source.content_hash != WholeSourceHash::digest(&source.bytes) {
                return Err(InventoryValidationError::SourceHashMismatch(source.id));
            }
            if let SourceSpaceIdentity::RegisteredSnapshot { snapshot } = &source.identity {
                if snapshot.content_hash() != source.content_hash
                    || !snapshot.resolved_file_language().is_framework_carrier()
                {
                    return Err(InventoryValidationError::RegisteredIdentityMismatch(
                        source.id,
                    ));
                }
            }
        }
        for (index, block) in self.blocks.iter().enumerate() {
            if block.id().0 as usize != index {
                return Err(InventoryValidationError::BlockIdMismatch {
                    expected: BlockId(index as u32),
                    actual: block.id(),
                });
            }
        }
        let mut child_slots_used = vec![false; self.markup.child_ids.len()];
        let mut node_inbound = vec![0u32; self.markup.nodes.len()];
        let mut attributes = Vec::new();
        for (index, node) in self.markup.nodes.iter().enumerate() {
            if node.id.0 as usize != index {
                return Err(InventoryValidationError::NodeIdMismatch {
                    expected: MarkupNodeId(index as u32),
                    actual: node.id,
                });
            }
            if node.root_block.0 as usize >= self.blocks.len() {
                return Err(InventoryValidationError::UnknownBlock(node.root_block));
            }
            let range = node.children.start as usize..node.children.end as usize;
            if range.start > range.end || range.end > self.markup.child_ids.len() {
                return Err(InventoryValidationError::InvalidChildRange(node.id));
            }
            for slot in range.clone() {
                if child_slots_used[slot] {
                    return Err(InventoryValidationError::OverlappingChildRange(node.id));
                }
                child_slots_used[slot] = true;
                let child = &self.markup.child_ids[slot];
                let child_node = self
                    .markup
                    .nodes
                    .get(child.0 as usize)
                    .ok_or(InventoryValidationError::UnknownNode(*child))?;
                if child_node.parent != Some(node.id) || child_node.root_block != node.root_block {
                    return Err(InventoryValidationError::InvalidChildOwnership(*child));
                }
                node_inbound[child.0 as usize] += 1;
            }
            for pair in self.markup.child_ids[range].windows(2) {
                let first = self.markup.nodes[pair[0].0 as usize].kind.full_span().start;
                let second = self.markup.nodes[pair[1].0 as usize].kind.full_span().start;
                if first > second {
                    return Err(InventoryValidationError::ChildrenOutOfOrder(node.id));
                }
            }
            for span in node.kind.spans() {
                self.slice_span(span)?;
            }
            for id in node.kind.normalized_names() {
                self.normalized_name(id)?;
            }
            for attribute in node.kind.attributes() {
                self.validate_attribute(attribute)?;
                attributes.push(attribute);
            }
            if node
                .kind
                .attributes()
                .windows(2)
                .any(|pair| pair[0].full_span().start > pair[1].full_span().start)
            {
                return Err(InventoryValidationError::AttributesOutOfOrder);
            }
        }
        if child_slots_used.iter().any(|used| !used) {
            return Err(InventoryValidationError::UnownedChildSlot);
        }
        for block in self.blocks.iter() {
            match block {
                CarrierBlock::Section { role, syntax, .. } => {
                    for span in syntax.spans() {
                        self.slice_span(span)?;
                    }
                    self.normalized_name(syntax.normalized_name)?;
                    self.validate_section_role(role)?;
                    for attribute in syntax.attributes.iter() {
                        self.validate_attribute(attribute)?;
                        attributes.push(attribute);
                    }
                    if syntax
                        .attributes
                        .windows(2)
                        .any(|pair| pair[0].full_span().start > pair[1].full_span().start)
                    {
                        return Err(InventoryValidationError::AttributesOutOfOrder);
                    }
                }
                CarrierBlock::MarkupRoot { id, node } => {
                    let root = self
                        .markup
                        .nodes
                        .get(node.0 as usize)
                        .ok_or(InventoryValidationError::UnknownNode(*node))?;
                    if root.parent.is_some() || root.root_block != *id {
                        return Err(InventoryValidationError::InvalidRootOwnership(*node));
                    }
                }
            }
        }
        let mut root_seen = vec![false; self.markup.nodes.len()];
        for root in self.markup.roots.iter() {
            let Some(node) = self.markup.nodes.get(root.0 as usize) else {
                return Err(InventoryValidationError::InvalidRootOwnership(*root));
            };
            let Some(block) = self.blocks.get(node.root_block.0 as usize) else {
                return Err(InventoryValidationError::InvalidRootOwnership(*root));
            };
            let owns_root = match block {
                CarrierBlock::Section {
                    role: SectionRole::TemplateHost,
                    ..
                } => true,
                CarrierBlock::MarkupRoot { id, node: owner } => {
                    *id == node.root_block && *owner == *root
                }
                _ => false,
            };
            if node.parent.is_some() || !owns_root || root_seen[root.0 as usize] {
                return Err(InventoryValidationError::InvalidRootOwnership(*root));
            }
            root_seen[root.0 as usize] = true;
        }
        for node in self
            .markup
            .nodes
            .iter()
            .filter(|node| node.parent.is_none())
        {
            if !root_seen[node.id.0 as usize] {
                return Err(InventoryValidationError::InvalidRootOwnership(node.id));
            }
        }
        for pair in self.markup.roots.windows(2) {
            let first = self.markup.nodes[pair[0].0 as usize].kind.full_span().start;
            let second = self.markup.nodes[pair[1].0 as usize].kind.full_span().start;
            if first > second {
                return Err(InventoryValidationError::RootsOutOfOrder);
            }
        }
        for node in self.markup.nodes.iter() {
            let expected = u32::from(node.parent.is_some());
            if node_inbound[node.id.0 as usize] != expected {
                return Err(InventoryValidationError::InvalidNodeCardinality(node.id));
            }
        }
        let mut attribute_seen = vec![false; attributes.len()];
        for attribute in attributes {
            let id = attribute.id();
            let Some(seen) = attribute_seen.get_mut(id.0 as usize) else {
                return Err(InventoryValidationError::InvalidAttributeCardinality(id));
            };
            if *seen {
                return Err(InventoryValidationError::InvalidAttributeCardinality(id));
            }
            *seen = true;
            if let Some(duplicate) = attribute.duplicate_of() {
                if duplicate.0 >= id.0 || duplicate.0 as usize >= attribute_seen.len() {
                    return Err(InventoryValidationError::InvalidDuplicateAttribute {
                        id,
                        duplicate,
                    });
                }
            }
        }
        if attribute_seen.iter().any(|seen| !seen) {
            return Err(InventoryValidationError::MissingAttributeId);
        }
        for pair in self.blocks.windows(2) {
            if self.block_start(&pair[0])? > self.block_start(&pair[1])? {
                return Err(InventoryValidationError::BlocksOutOfOrder);
            }
        }
        Ok(())
    }

    fn validate_section_role(&self, role: &SectionRole) -> Result<(), InventoryValidationError> {
        match role {
            SectionRole::Script {
                dialect:
                    ScriptSourceType::Custom {
                        authored,
                        normalized,
                    },
                ..
            }
            | SectionRole::Style {
                dialect:
                    StyleDialect::Custom {
                        authored,
                        normalized,
                    },
                ..
            } => {
                self.slice(*authored)?;
                self.normalized_name(*normalized)?;
            }
            SectionRole::Style {
                module: StyleModule::Named { name },
                ..
            } => {
                self.slice(*name)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_attribute(
        &self,
        attribute: &CarrierAttribute,
    ) -> Result<(), InventoryValidationError> {
        for span in attribute.spans() {
            self.slice_span(span)?;
        }
        let validate_name = |name: &AttributeName| -> Result<(), InventoryValidationError> {
            self.slice(name.authored)?;
            self.normalized_name(name.normalized)?;
            Ok(())
        };
        match attribute {
            CarrierAttribute::Named { name, value, .. } => {
                validate_name(name)?;
                self.validate_value(value)?;
            }
            CarrierAttribute::Directive {
                local_name,
                argument,
                modifiers,
                value,
                ..
            } => {
                if let Some(name) = local_name {
                    validate_name(name)?;
                }
                if let DirectiveArgument::Static { name } = argument {
                    validate_name(name)?;
                }
                for modifier in modifiers.iter() {
                    self.slice(modifier.authored)?;
                    self.normalized_name(modifier.normalized)?;
                }
                self.validate_value(value)?;
            }
            CarrierAttribute::Spread { .. } | CarrierAttribute::Attach { .. } => {}
        }
        Ok(())
    }

    fn validate_value(&self, value: &AttributeValue) -> Result<(), InventoryValidationError> {
        let validate_decoded = |raw: SourceSlice, decoded: &LazyDecodedText| {
            if let LazyDecodedText::EntityDecode { key } = decoded {
                if key.raw != raw {
                    return Err(InventoryValidationError::DecodedValueKeyMismatch);
                }
                self.slice(key.raw)?;
            }
            Ok(())
        };
        match value {
            AttributeValue::Static { raw, decoded, .. } => validate_decoded(*raw, decoded)?,
            AttributeValue::Mixed { parts, .. } => {
                for part in parts.iter() {
                    if let AttributeValuePart::Static { raw, decoded } = part {
                        validate_decoded(*raw, decoded)?;
                    }
                }
            }
            AttributeValue::Missing | AttributeValue::Expression { .. } => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryValidationError {
    SourceSpaceIdMismatch {
        expected: SourceSpaceId,
        actual: SourceSpaceId,
    },
    SourceLengthMismatch(SourceSpaceId),
    SourceHashMismatch(SourceSpaceId),
    RegisteredIdentityMismatch(SourceSpaceId),
    UnknownSourceSpace(SourceSpaceId),
    InvalidSpan(SourceSpan),
    UnknownNormalizedName(InternedNameId),
    BlockIdMismatch {
        expected: BlockId,
        actual: BlockId,
    },
    NodeIdMismatch {
        expected: MarkupNodeId,
        actual: MarkupNodeId,
    },
    UnknownBlock(BlockId),
    UnknownNode(MarkupNodeId),
    InvalidChildRange(MarkupNodeId),
    OverlappingChildRange(MarkupNodeId),
    UnownedChildSlot,
    InvalidChildOwnership(MarkupNodeId),
    ChildrenOutOfOrder(MarkupNodeId),
    InvalidRootOwnership(MarkupNodeId),
    RootsOutOfOrder,
    InvalidNodeCardinality(MarkupNodeId),
    DuplicateNormalizedName(InternedNameId),
    InvalidAttributeCardinality(AttributeId),
    InvalidDuplicateAttribute {
        id: AttributeId,
        duplicate: AttributeId,
    },
    MissingAttributeId,
    AttributesOutOfOrder,
    DecodedValueKeyMismatch,
    BlocksOutOfOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CarrierBlock {
    Section {
        id: BlockId,
        role: SectionRole,
        syntax: TaggedSyntax,
    },
    MarkupRoot {
        id: BlockId,
        node: MarkupNodeId,
    },
}
impl CarrierBlock {
    pub const fn id(&self) -> BlockId {
        match self {
            Self::Section { id, .. } | Self::MarkupRoot { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SectionRole {
    TemplateHost,
    Script {
        role: ScriptRole,
        dialect: ScriptSourceType,
    },
    Style {
        dialect: StyleDialect,
        scoped: bool,
        module: StyleModule,
    },
    Custom {
        normalized_name: Arc<str>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptRole {
    Instance,
    Setup,
    Module,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScriptSourceType {
    JavaScript,
    TypeScript,
    Jsx,
    Tsx,
    Custom {
        authored: SourceSlice,
        normalized: InternedNameId,
    },
    Missing,
}
impl From<RegistryScriptSourceType> for ScriptSourceType {
    fn from(value: RegistryScriptSourceType) -> Self {
        match value {
            RegistryScriptSourceType::Ts | RegistryScriptSourceType::Dts => Self::TypeScript,
            RegistryScriptSourceType::Tsx => Self::Tsx,
            RegistryScriptSourceType::Js(_) => Self::JavaScript,
            RegistryScriptSourceType::Jsx(_) => Self::Jsx,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StyleDialect {
    Css,
    Scss,
    Sass,
    Less,
    Stylus,
    PostCss,
    Custom {
        authored: SourceSlice,
        normalized: InternedNameId,
    },
    Missing,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StyleModule {
    None,
    Default,
    Named { name: SourceSlice },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkupFragmentKind {
    Element,
    Text,
    Comment,
    Interpolation,
    SvelteControlBlock,
    SvelteClause,
    SvelteStandaloneTag,
    Recovered,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveredMarkupKind {
    Element,
    Comment,
    Interpolation,
    SvelteControlBlock,
    SvelteClause,
    SvelteStandaloneTag,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnknownMarkupReason {
    ParserUnknownVariant,
    UnsupportedAuthoredHead,
    MalformedAuthoredHead,
    RecoveryBudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaggedSyntax {
    pub authored_name: SourceSlice,
    pub normalized_name: InternedNameId,
    pub opening_span: SourceSpan,
    pub opening_name_span: SourceSpan,
    pub attribute_insertion_anchor: SourceSpan,
    pub content_span: SourceSpan,
    pub closing_span: Option<SourceSpan>,
    pub closing_name_span: Option<SourceSpan>,
    pub full_span: SourceSpan,
    pub termination: SyntaxTermination,
    pub attributes: Arc<[CarrierAttribute]>,
}
impl TaggedSyntax {
    fn spans(&self) -> Vec<SourceSpan> {
        let mut spans = vec![
            self.authored_name.span,
            self.opening_span,
            self.opening_name_span,
            self.attribute_insertion_anchor,
            self.content_span,
            self.full_span,
        ];
        spans.extend(self.closing_span);
        spans.extend(self.closing_name_span);
        spans.extend(self.attributes.iter().flat_map(CarrierAttribute::spans));
        spans
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SyntaxTermination {
    Closed,
    SelfClosing,
    Void,
    UnclosedEof,
    Recovered {
        reason: BlockRecoveryReason,
        recovery_span: Option<SourceSpan>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockRecoveryReason {
    MissingCloseTag,
    MismatchedCloseTag,
    StrayCloseTag,
    UnterminatedOpenTag,
    UnterminatedAttribute,
    InvalidNesting,
    DuplicateSingletonRoot,
    InvalidSelfClosing,
    InvalidRawTextTermination,
    ParserRejectedSyntax,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CarrierAttribute {
    Named {
        id: AttributeId,
        name: AttributeName,
        syntax: NamedAttributeSyntax,
        value: AttributeValue,
        full_span: SourceSpan,
        duplicate_of: Option<AttributeId>,
    },
    Spread {
        id: AttributeId,
        full_span: SourceSpan,
        open_span: SourceSpan,
        expression_span: SourceSpan,
        close_span: Option<SourceSpan>,
        termination: SyntaxTermination,
    },
    Directive {
        id: AttributeId,
        family: DirectiveFamily,
        prefix_span: SourceSpan,
        local_name: Option<AttributeName>,
        argument: DirectiveArgument,
        modifiers: Arc<[DirectiveModifier]>,
        value: AttributeValue,
        full_span: SourceSpan,
        duplicate_of: Option<AttributeId>,
    },
    Attach {
        id: AttributeId,
        full_span: SourceSpan,
        keyword_span: SourceSpan,
        expression_span: SourceSpan,
        close_span: Option<SourceSpan>,
        termination: SyntaxTermination,
    },
}
impl CarrierAttribute {
    pub const fn id(&self) -> AttributeId {
        match self {
            Self::Named { id, .. }
            | Self::Spread { id, .. }
            | Self::Directive { id, .. }
            | Self::Attach { id, .. } => *id,
        }
    }
    pub const fn duplicate_of(&self) -> Option<AttributeId> {
        match self {
            Self::Named { duplicate_of, .. } | Self::Directive { duplicate_of, .. } => {
                *duplicate_of
            }
            _ => None,
        }
    }
    pub const fn full_span(&self) -> SourceSpan {
        match self {
            Self::Named { full_span, .. }
            | Self::Spread { full_span, .. }
            | Self::Directive { full_span, .. }
            | Self::Attach { full_span, .. } => *full_span,
        }
    }
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::Named {
                name,
                value,
                full_span,
                ..
            } => {
                let mut v = vec![name.authored.span, name.name_span, *full_span];
                v.extend(value.spans());
                v
            }
            Self::Spread {
                full_span,
                open_span,
                expression_span,
                close_span,
                ..
            } => {
                let mut v = vec![*full_span, *open_span, *expression_span];
                v.extend(*close_span);
                v
            }
            Self::Directive {
                prefix_span,
                local_name,
                argument,
                modifiers,
                value,
                full_span,
                ..
            } => {
                let mut v = vec![*prefix_span, *full_span];
                if let Some(n) = local_name {
                    v.extend([n.authored.span, n.name_span]);
                }
                v.extend(argument.spans());
                for m in modifiers.iter() {
                    v.extend([m.authored.span, m.separator_span, m.name_span, m.full_span]);
                }
                v.extend(value.spans());
                v
            }
            Self::Attach {
                full_span,
                keyword_span,
                expression_span,
                close_span,
                ..
            } => {
                let mut v = vec![*full_span, *keyword_span, *expression_span];
                v.extend(*close_span);
                v
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttributeName {
    pub authored: SourceSlice,
    pub normalized: InternedNameId,
    pub name_span: SourceSpan,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedAttributeSyntax {
    Explicit,
    SvelteShorthand,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AttributeValue {
    Missing,
    Static {
        raw: SourceSlice,
        decoded: LazyDecodedText,
        quote: AttributeQuote,
        value_span: SourceSpan,
        inner_span: SourceSpan,
    },
    Expression {
        syntax: AttributeDynamicSyntax,
        full_span: SourceSpan,
        open_span: Option<SourceSpan>,
        expression_span: SourceSpan,
        close_span: Option<SourceSpan>,
        termination: SyntaxTermination,
    },
    Mixed {
        full_span: SourceSpan,
        parts: Arc<[AttributeValuePart]>,
    },
}
impl AttributeValue {
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::Missing => vec![],
            Self::Static {
                raw,
                value_span,
                inner_span,
                ..
            } => vec![raw.span, *value_span, *inner_span],
            Self::Expression {
                full_span,
                open_span,
                expression_span,
                close_span,
                ..
            } => {
                let mut v = vec![*full_span, *expression_span];
                v.extend(*open_span);
                v.extend(*close_span);
                v
            }
            Self::Mixed { full_span, parts } => {
                let mut v = vec![*full_span];
                v.extend(parts.iter().flat_map(AttributeValuePart::spans));
                v
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AttributeValuePart {
    Static {
        raw: SourceSlice,
        decoded: LazyDecodedText,
    },
    Expression {
        syntax: AttributeDynamicSyntax,
        full_span: SourceSpan,
        open_span: Option<SourceSpan>,
        expression_span: SourceSpan,
        close_span: Option<SourceSpan>,
        termination: SyntaxTermination,
    },
}
impl AttributeValuePart {
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::Static { raw, .. } => vec![raw.span],
            Self::Expression {
                full_span,
                open_span,
                expression_span,
                close_span,
                ..
            } => {
                let mut v = vec![*full_span, *expression_span];
                v.extend(*open_span);
                v.extend(*close_span);
                v
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeDynamicSyntax {
    VueBracedExpression,
    VueDynamicArgument,
    VueShorthand,
    SvelteMustacheExpression,
    SvelteShorthand,
    SvelteExpressionTag,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttributeQuote {
    Unquoted,
    Single,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DirectiveFamily {
    Vue(VueDirectiveKind),
    Svelte(SvelteDirectiveKind),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VueDirectiveKind {
    Bind,
    On,
    Model,
    Show,
    If,
    ElseIf,
    Else,
    For,
    Slot,
    Pre,
    Cloak,
    Once,
    Memo,
    Html,
    Text,
    Custom,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SvelteDirectiveKind {
    Bind,
    On,
    Use,
    Class,
    Style,
    Let,
    Transition,
    In,
    Out,
    Animate,
    Custom,
    Unknown {
        authored_family: SourceSlice,
        reason: UnknownDirectiveReason,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnknownDirectiveReason {
    ParserUnknownVariant,
    UnsupportedAuthoredPrefix,
    MalformedAuthoredPrefix,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DirectiveArgument {
    None,
    Static {
        name: AttributeName,
    },
    Dynamic {
        full_span: SourceSpan,
        open_span: SourceSpan,
        expression_span: SourceSpan,
        close_span: Option<SourceSpan>,
        termination: SyntaxTermination,
    },
}
impl DirectiveArgument {
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::None => vec![],
            Self::Static { name } => vec![name.authored.span, name.name_span],
            Self::Dynamic {
                full_span,
                open_span,
                expression_span,
                close_span,
                ..
            } => {
                let mut v = vec![*full_span, *open_span, *expression_span];
                v.extend(*close_span);
                v
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DirectiveModifier {
    pub authored: SourceSlice,
    pub normalized: InternedNameId,
    pub separator_span: SourceSpan,
    pub name_span: SourceSpan,
    pub full_span: SourceSpan,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LazyDecodedText {
    SameAsSource,
    EntityDecode { key: DecodedValueKey },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityDecodeRecipe {
    Html5Text,
    Html5Attribute { quote: AttributeQuote },
    XmlText,
    XmlAttribute { quote: QuotedAttributeQuote },
    SvelteText,
    SvelteAttribute { quote: AttributeQuote },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuotedAttributeQuote {
    Single,
    Double,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct MarkupSyntaxArena {
    pub roots: Arc<[MarkupNodeId]>,
    pub nodes: Arc<[MarkupSyntaxNode]>,
    pub child_ids: Arc<[MarkupNodeId]>,
}
impl MarkupSyntaxArena {
    pub fn roots(&self) -> &[MarkupNodeId] {
        &self.roots
    }
    pub fn nodes(&self) -> &[MarkupSyntaxNode] {
        &self.nodes
    }
    pub fn child_ids(&self) -> &[MarkupNodeId] {
        &self.child_ids
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MarkupSyntaxNode {
    pub id: MarkupNodeId,
    pub root_block: BlockId,
    pub parent: Option<MarkupNodeId>,
    pub children: Range<u32>,
    pub kind: MarkupNodeKind,
}
impl MarkupSyntaxNode {
    pub const fn kind(&self) -> &MarkupNodeKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MarkupNodeKind {
    Element(MarkupElementSyntax),
    Text {
        content_span: SourceSpan,
    },
    Comment {
        opening_span: SourceSpan,
        content_span: SourceSpan,
        closing_span: Option<SourceSpan>,
        full_span: SourceSpan,
        termination: SyntaxTermination,
    },
    Interpolation {
        family: MarkupInterpolationFamily,
        opening_span: SourceSpan,
        expression_span: SourceSpan,
        closing_span: Option<SourceSpan>,
        full_span: SourceSpan,
        termination: SyntaxTermination,
    },
    SvelteControlBlock(SvelteControlBlockSyntax),
    SvelteClause(SvelteClauseSyntax),
    SvelteStandaloneTag(SvelteStandaloneTagSyntax),
    Recovered {
        opening_span: Option<SourceSpan>,
        opening_name_span: Option<SourceSpan>,
        content_span: Option<SourceSpan>,
        closing_span: Option<SourceSpan>,
        closing_name_span: Option<SourceSpan>,
        full_span: SourceSpan,
        termination: SyntaxTermination,
        expected: RecoveredMarkupKind,
        reason: BlockRecoveryReason,
    },
    Unknown {
        opening_span: Option<SourceSpan>,
        opening_name_span: Option<SourceSpan>,
        content_span: Option<SourceSpan>,
        closing_span: Option<SourceSpan>,
        closing_name_span: Option<SourceSpan>,
        full_span: SourceSpan,
        termination: SyntaxTermination,
        authored_head: Option<SourceSlice>,
        reason: UnknownMarkupReason,
    },
}
impl MarkupNodeKind {
    pub const fn fragment_kind(&self) -> MarkupFragmentKind {
        match self {
            Self::Element(_) => MarkupFragmentKind::Element,
            Self::Text { .. } => MarkupFragmentKind::Text,
            Self::Comment { .. } => MarkupFragmentKind::Comment,
            Self::Interpolation { .. } => MarkupFragmentKind::Interpolation,
            Self::SvelteControlBlock(_) => MarkupFragmentKind::SvelteControlBlock,
            Self::SvelteClause(_) => MarkupFragmentKind::SvelteClause,
            Self::SvelteStandaloneTag(_) => MarkupFragmentKind::SvelteStandaloneTag,
            Self::Recovered { .. } => MarkupFragmentKind::Recovered,
            Self::Unknown { .. } => MarkupFragmentKind::Unknown,
        }
    }
    pub fn attributes(&self) -> &[CarrierAttribute] {
        match self {
            Self::Element(v) => &v.attributes,
            _ => &[],
        }
    }
    pub fn full_span(&self) -> SourceSpan {
        match self {
            Self::Element(v) => v.full_span,
            Self::Text { content_span } => *content_span,
            Self::Comment { full_span, .. }
            | Self::Interpolation { full_span, .. }
            | Self::Recovered { full_span, .. }
            | Self::Unknown { full_span, .. } => *full_span,
            Self::SvelteControlBlock(v) => v.full_span,
            Self::SvelteClause(v) => v.full_span,
            Self::SvelteStandaloneTag(v) => v.full_span,
        }
    }
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::Element(v) => v.spans(),
            Self::Text { content_span } => vec![*content_span],
            Self::Comment {
                opening_span,
                content_span,
                closing_span,
                full_span,
                ..
            } => {
                let mut v = vec![*opening_span, *content_span, *full_span];
                v.extend(*closing_span);
                v
            }
            Self::Interpolation {
                opening_span,
                expression_span,
                closing_span,
                full_span,
                ..
            } => {
                let mut v = vec![*opening_span, *expression_span, *full_span];
                v.extend(*closing_span);
                v
            }
            Self::SvelteControlBlock(v) => v.spans(),
            Self::SvelteClause(v) => v.spans(),
            Self::SvelteStandaloneTag(v) => v.spans(),
            Self::Recovered {
                opening_span,
                opening_name_span,
                content_span,
                closing_span,
                closing_name_span,
                full_span,
                ..
            }
            | Self::Unknown {
                opening_span,
                opening_name_span,
                content_span,
                closing_span,
                closing_name_span,
                full_span,
                ..
            } => {
                let mut v = vec![*full_span];
                v.extend(*opening_span);
                v.extend(*opening_name_span);
                v.extend(*content_span);
                v.extend(*closing_span);
                v.extend(*closing_name_span);
                v
            }
        }
    }
    fn normalized_names(&self) -> Vec<InternedNameId> {
        match self {
            Self::Element(v) => {
                let mut out = vec![v.normalized_name];
                for a in v.attributes.iter() {
                    match a {
                        CarrierAttribute::Named { name, .. } => out.push(name.normalized),
                        CarrierAttribute::Directive {
                            local_name,
                            modifiers,
                            ..
                        } => {
                            if let Some(n) = local_name {
                                out.push(n.normalized)
                            }
                            out.extend(modifiers.iter().map(|m| m.normalized));
                        }
                        _ => {}
                    }
                }
                out
            }
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkupInterpolationFamily {
    VueInterpolation,
    SvelteInterpolation,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MarkupElementSyntax {
    pub authored_name: SourceSlice,
    pub normalized_name: InternedNameId,
    pub namespace: MarkupNamespace,
    pub kind: MarkupElementKind,
    pub opening_span: SourceSpan,
    pub opening_name_span: SourceSpan,
    pub attribute_insertion_anchor: SourceSpan,
    pub content_span: SourceSpan,
    pub closing_span: Option<SourceSpan>,
    pub closing_name_span: Option<SourceSpan>,
    pub full_span: SourceSpan,
    pub self_closing: bool,
    pub void_element: bool,
    pub raw_text: bool,
    pub termination: SyntaxTermination,
    pub attributes: Arc<[CarrierAttribute]>,
}
impl MarkupElementSyntax {
    pub const fn authored_name(&self) -> SourceSlice {
        self.authored_name
    }
    pub const fn normalized_name(&self) -> InternedNameId {
        self.normalized_name
    }
    pub fn attributes(&self) -> &[CarrierAttribute] {
        &self.attributes
    }
    fn spans(&self) -> Vec<SourceSpan> {
        let mut v = vec![
            self.authored_name.span,
            self.opening_span,
            self.opening_name_span,
            self.attribute_insertion_anchor,
            self.content_span,
            self.full_span,
        ];
        v.extend(self.closing_span);
        v.extend(self.closing_name_span);
        v.extend(self.attributes.iter().flat_map(CarrierAttribute::spans));
        v
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkupNamespace {
    Html,
    Svg,
    MathMl,
    Foreign,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MarkupElementKind {
    Native,
    Component,
    DynamicComponent,
    DynamicElement,
    SvelteNestedStyle,
    SvelteSpecial(SvelteSpecialElementKind),
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SvelteSpecialElementKind {
    Head,
    Window,
    Document,
    Body,
    Element,
    Boundary,
    Options,
    Component,
    SelfRef,
    Fragment,
    Unknown { authored_local: SourceSlice },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SvelteControlBlockSyntax {
    pub head: SvelteControlBlockHead,
    pub opening_span: SourceSpan,
    pub closing_span: Option<SourceSpan>,
    pub full_span: SourceSpan,
    pub termination: SyntaxTermination,
}
impl SvelteControlBlockSyntax {
    fn spans(&self) -> Vec<SourceSpan> {
        let mut v = vec![self.opening_span, self.full_span];
        v.extend(self.closing_span);
        v.extend(self.head.spans());
        v
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SvelteControlBlockHead {
    If {
        condition: SourceSpan,
    },
    Each {
        iterable: SourceSpan,
        item: Option<SourceSpan>,
        index: Option<SourceSpan>,
        key: Option<SourceSpan>,
    },
    Await {
        promise: SourceSpan,
        inline_branch: SvelteAwaitInlineBranch,
    },
    Key {
        expression: SourceSpan,
    },
    Snippet {
        authored_name: SourceSlice,
        name_span: SourceSpan,
        params_span: Option<SourceSpan>,
    },
}
impl SvelteControlBlockHead {
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::If { condition }
            | Self::Key {
                expression: condition,
            } => vec![*condition],
            Self::Each {
                iterable,
                item,
                index,
                key,
            } => {
                let mut v = vec![*iterable];
                v.extend(*item);
                v.extend(*index);
                v.extend(*key);
                v
            }
            Self::Await {
                promise,
                inline_branch,
            } => {
                let mut v = vec![*promise];
                v.extend(inline_branch.spans());
                v
            }
            Self::Snippet {
                authored_name,
                name_span,
                params_span,
            } => {
                let mut v = vec![authored_name.span, *name_span];
                v.extend(*params_span);
                v
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SvelteClauseSyntax {
    pub head: SvelteClauseHead,
    pub marker_span: SourceSpan,
    pub full_span: SourceSpan,
    pub termination: SyntaxTermination,
}
impl SvelteClauseSyntax {
    fn spans(&self) -> Vec<SourceSpan> {
        let mut v = vec![self.marker_span, self.full_span];
        v.extend(self.head.spans());
        v
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SvelteClauseHead {
    Else,
    ElseIf { condition: SourceSpan },
    Then { binding: Option<SourceSpan> },
    Catch { binding: Option<SourceSpan> },
}
impl SvelteClauseHead {
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::Else => vec![],
            Self::ElseIf { condition } => vec![*condition],
            Self::Then { binding } | Self::Catch { binding } => binding.iter().copied().collect(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SvelteAwaitInlineBranch {
    None,
    Then {
        marker_span: SourceSpan,
        head_span: SourceSpan,
        binding: Option<SourceSpan>,
    },
    Catch {
        marker_span: SourceSpan,
        head_span: SourceSpan,
        binding: Option<SourceSpan>,
    },
}
impl SvelteAwaitInlineBranch {
    fn spans(&self) -> Vec<SourceSpan> {
        match self {
            Self::None => vec![],
            Self::Then {
                marker_span,
                head_span,
                binding,
            }
            | Self::Catch {
                marker_span,
                head_span,
                binding,
            } => {
                let mut v = vec![*marker_span, *head_span];
                v.extend(*binding);
                v
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SvelteStandaloneTagSyntax {
    pub family: SvelteStandaloneTagFamily,
    pub opening_span: SourceSpan,
    pub expression_span: Option<SourceSpan>,
    pub closing_span: Option<SourceSpan>,
    pub full_span: SourceSpan,
    pub termination: SyntaxTermination,
}
impl SvelteStandaloneTagSyntax {
    fn spans(&self) -> Vec<SourceSpan> {
        let mut v = vec![self.opening_span, self.full_span];
        v.extend(self.expression_span);
        v.extend(self.closing_span);
        if let SvelteStandaloneTagFamily::Unknown { authored_name, .. } = &self.family {
            v.push(authored_name.span)
        }
        v
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SvelteStandaloneTagFamily {
    Render,
    Html,
    LegacyConst,
    Const,
    Let,
    Debug,
    Attach,
    Unknown {
        authored_name: SourceSlice,
        reason: UnknownMarkupReason,
    },
}

fn decode_entities(raw: &str, recipe: EntityDecodeRecipe) -> Cow<'_, str> {
    if !raw.as_bytes().contains(&b'&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    let mut changed = false;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(semi) = rest.get(1..).and_then(|tail| tail.find(';')) else {
            out.push_str(rest);
            rest = "";
            break;
        };
        let consumed = semi + 2;
        if consumed <= 34 {
            if let Some(value) = decode_entity(&rest[1..consumed - 1], recipe) {
                out.push(value);
                rest = &rest[consumed..];
                changed = true;
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    if changed {
        out.push_str(rest);
        Cow::Owned(out)
    } else {
        Cow::Borrowed(raw)
    }
}

fn decode_entity(entity: &str, recipe: EntityDecodeRecipe) -> Option<char> {
    if let Some(number) = entity.strip_prefix('#') {
        let code_point = if let Some(hex) = number
            .strip_prefix('x')
            .or_else(|| number.strip_prefix('X'))
        {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            number.parse::<u32>().ok()?
        };
        return char::from_u32(code_point);
    }

    let xml = matches!(
        recipe,
        EntityDecodeRecipe::XmlText | EntityDecodeRecipe::XmlAttribute { .. }
    );
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ if xml => None,
        "nbsp" => Some('\u{00a0}'),
        "copy" => Some('\u{00a9}'),
        "reg" => Some('\u{00ae}'),
        "trade" => Some('\u{2122}'),
        "mdash" => Some('\u{2014}'),
        "ndash" => Some('\u{2013}'),
        "hellip" => Some('\u{2026}'),
        "laquo" => Some('\u{00ab}'),
        "raquo" => Some('\u{00bb}'),
        "bull" => Some('\u{2022}'),
        "middot" => Some('\u{00b7}'),
        "cent" => Some('\u{00a2}'),
        "pound" => Some('\u{00a3}'),
        "yen" => Some('\u{00a5}'),
        "euro" => Some('\u{20ac}'),
        "eacute" => Some('\u{00e9}'),
        "Eacute" => Some('\u{00c9}'),
        _ => None,
    }
}

#[cfg(test)]
mod entity_decode_tests {
    use super::*;

    #[test]
    fn closed_entity_recipes_decode_numeric_and_keep_xml_html_only_names_literal() {
        assert_eq!(
            decode_entities("&#169; &#x1f642;", EntityDecodeRecipe::Html5Text),
            "© 🙂"
        );
        assert_eq!(
            decode_entities("&nbsp;&amp;", EntityDecodeRecipe::XmlText),
            "&nbsp;&"
        );
    }

    #[test]
    fn lazy_decode_borrows_when_no_bytes_change() {
        assert!(matches!(
            decode_entities("literal", EntityDecodeRecipe::Html5Text),
            Cow::Borrowed("literal")
        ));
        assert!(matches!(
            decode_entities("&unknown;", EntityDecodeRecipe::Html5Text),
            Cow::Borrowed("&unknown;")
        ));
    }
}
