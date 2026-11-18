//! The complete artifacts of one Vue component compile: one module per
//! requested runtime product, its style blocks and its metadata, in one
//! [`CompileArtifactSet`] under one source identity and revision.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use verter_language::LanguageId;

use crate::assembly::artifact_graph::{ArtifactRole, StyleArtifactFacts};
use crate::assembly::custom_block::{CustomBlockDescriptor, CustomBlockDescriptorError};
use crate::assembly::emit_plan::{DecodedMap, EmittedArtifact, MapSegmentKind, MapSource};
use crate::assembly::fragment::{
    ArtifactContent, ArtifactId, ArtifactProvenance, ArtifactRelation, ArtifactRelationKind,
    CompileArtifact, FragmentDialect,
};
use crate::assembly::source_space::{ArtifactMapFamily, ArtifactMapSegment, QualifiedArtifactMap};
use crate::assembly::source_unit::{ArtifactSourceUnit, ContentId, SourceUnit};
use crate::assembly::{ArtifactSchemaError, CompileArtifactSet, StagedCompileArtifacts};
use crate::compile::VerterCustomBlock;
use crate::compile_request::ProductKind;
use crate::compile_transaction::{
    mint_admission_identity, mint_direct_source_id, DirectSetAdmission, DirectSetTag,
};
use crate::framework::vue::style::{VueStyleBlockInput, VueStyleBlockPlan, VueStyleOutcome};
use crate::framework_common::vue_bundle::custom_block_descriptor;
use crate::framework_common::RuntimeDiagnostic;
use crate::standalone::hash_len_prefixed_str;

/// Metadata of one style block, as published in the metadata artifact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VueStyleMetadata {
    pub index: u32,
    pub scoped: bool,
    pub module: Option<String>,
    pub lang: String,
}

/// The JSON content of a component's metadata artifact.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VueComponentMetadata {
    /// Eight hex digits.
    pub scope_id: String,
    pub has_scoped_style: bool,
    /// The render function is inside `setup`.
    pub inline_template: bool,
    pub has_script: bool,
    pub has_template: bool,
    pub styles: Vec<VueStyleMetadata>,
    /// Custom block type names in source order.
    pub custom_blocks: Vec<String>,
}

/// One source a module's mappings name: its unit in the set, and the row
/// (name and text) its V3 map carries. The unit's span starts where the
/// text starts, so a mapping offset into the text lands at the same
/// offset past the span start.
#[derive(Debug, Clone)]
pub(crate) struct VueModuleSource {
    pub unit: ArtifactSourceUnit,
    pub name: String,
    /// Present only when the module requested mappings.
    pub text: Option<Arc<str>>,
}

/// The written module of one requested runtime product.
pub(crate) struct VueModuleOutput {
    pub product: ProductKind,
    pub dialect: FragmentDialect,
    pub module: EmittedArtifact,
    /// One entry per key of `module.sources()`, in the same order.
    pub sources: Vec<VueModuleSource>,
}

/// Why a component's artifacts could not be assembled into one set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueArtifactSetError {
    Schema(ArtifactSchemaError),
    /// An unwitnessed module did not parse; no sibling artifact publishes.
    InvalidModule {
        product: ProductKind,
        reason: String,
    },
    /// A module's source rows do not name the sources, at the contents, it
    /// was written from.
    ModuleSourcesMismatch {
        product: ProductKind,
    },
    /// A module's map carries an external piece's admitted map, which the
    /// qualified map cannot state over the module's own sources.
    ExternalMapUnsupported {
        product: ProductKind,
    },
    /// A module was supplied for a product that is not a runtime module.
    NotARuntimeModule {
        product: ProductKind,
    },
    /// Two modules were supplied for one runtime product.
    DuplicateModule {
        product: ProductKind,
    },
    /// A style block was refused; nothing publishes.
    StyleRefused {
        index: u32,
    },
    /// The plan names a style block with no supplied input or metadata row.
    MissingStyleBlock {
        index: u32,
    },
    /// The metadata could not be written as JSON.
    Metadata(String),
    /// A custom block's facts could not become an attached descriptor.
    CustomBlock(CustomBlockDescriptorError),
}

/// The component's custom blocks, as the carrier parse read them, and the
/// length of the bytes they were read from.
#[derive(Debug, Default)]
pub(crate) struct VueCustomBlocks {
    pub source_len: u32,
    pub blocks: Vec<VerterCustomBlock>,
}

impl From<ArtifactSchemaError> for VueArtifactSetError {
    fn from(error: ArtifactSchemaError) -> Self {
        Self::Schema(error)
    }
}

/// What `stage` needs to hand one module over: its dialect, and its map
/// with the rows the map's sources are written under.
#[derive(Debug, Clone)]
struct ModuleHandoff {
    dialect: FragmentDialect,
    map: Option<DecodedMap>,
    rows: Vec<(String, Arc<str>)>,
    units: Vec<ArtifactSourceUnit>,
}

/// The complete artifacts of one Vue component compile.
#[derive(Debug, Clone)]
pub struct VueArtifactSet {
    set: CompileArtifactSet,
    client: Option<ArtifactId>,
    server: Option<ArtifactId>,
    diagnostics: Vec<RuntimeDiagnostic>,
    handoff: BTreeMap<ArtifactId, ModuleHandoff>,
    host_maps: Vec<(crate::assembly::SourceUnitId, Arc<str>)>,
}

/// One style block's published bytes and facts.
struct StyleArtifactInput {
    unit: SourceUnit,
    language: LanguageId,
    facts: StyleArtifactFacts,
    code: Arc<str>,
}

impl VueArtifactSet {
    /// Assemble the written modules, the style plan's blocks (with the
    /// supplied bytes of each block) and the metadata of one component into
    /// one set.
    ///
    /// Every module is named `main`. Each style block publishes once per
    /// module product, named `style/<index>` and related to its module by
    /// `StyleOf`; its language is `css`, or the authored language while an
    /// external stage is still needed. One metadata artifact per module,
    /// named `meta`, holds the metadata JSON with sorted keys and is related
    /// by `MetadataOf`. All of them share the request's one source identity
    /// and revision, minted from the admission and the published bytes.
    ///
    /// Each custom block becomes one descriptor attached, through
    /// [`Self::attach_custom_blocks`], to the first module. Its region
    /// addresses an `sfc` unit of the set's source over the component's
    /// bytes; its content is carried verbatim and is not part of any module.
    pub(crate) fn build(
        admission: DirectSetAdmission<'_>,
        modules: Vec<VueModuleOutput>,
        style_blocks: &[VueStyleBlockPlan],
        style_inputs: &[VueStyleBlockInput<'_>],
        metadata: &VueComponentMetadata,
        custom_blocks: VueCustomBlocks,
        diagnostics: Vec<RuntimeDiagnostic>,
    ) -> Result<Self, VueArtifactSetError> {
        let metadata_json = serde_json::to_value(metadata)
            .map(|value| value.to_string())
            .map_err(|error| VueArtifactSetError::Metadata(error.to_string()))?;
        let style_rows = style_rows(style_blocks, style_inputs, metadata)?;

        let mut products_hasher = blake3::Hasher::new();
        for output in &modules {
            if let Some(reason) = crate::assembly::fragment::module_parse_errors(
                output.module.text(),
                output.dialect,
                output.module.grammar(),
            ) {
                return Err(VueArtifactSetError::InvalidModule {
                    product: output.product,
                    reason,
                });
            }
            hash_len_prefixed_str(&mut products_hasher, output.product.wire_tag());
            hash_len_prefixed_str(&mut products_hasher, output.dialect.lang_id());
            hash_len_prefixed_str(&mut products_hasher, output.module.text());
        }
        for (index, _, _, code) in &style_rows {
            hash_len_prefixed_str(&mut products_hasher, &index.to_string());
            hash_len_prefixed_str(&mut products_hasher, code);
        }
        hash_len_prefixed_str(&mut products_hasher, &metadata_json);
        let mut products_digest = [0u8; 32];
        products_hasher.finalize_xof().fill(&mut products_digest);

        let canonical_id = admission.canonical_id;
        let framework = admission.framework;
        let project = admission.project;
        let minted = mint_admission_identity(&admission, products_digest);
        let source_id = mint_direct_source_id(canonical_id, framework, project);
        let mint = |role: String, bytes: &[u8]| {
            SourceUnit::mint(
                source_id.clone(),
                minted.revision.clone(),
                role,
                ContentId::from_content_bytes(bytes),
            )
        };
        let producer = |name: &'static str| {
            verter_identity::identity::ResultContractId::from_canonical(&DirectSetTag {
                canonical_id: name,
                framework,
                project,
            })
        };
        let provenance = |producer, inputs: BTreeSet<_>| ArtifactProvenance {
            input_basis: minted.input_basis.clone(),
            producer,
            inputs,
        };

        let mut source_units: Vec<ArtifactSourceUnit> = Vec::new();
        // A unit two modules share is listed once. One unit at two contents
        // is listed twice, and the schema refuses the inconsistent revision.
        let push_unit = |unit: ArtifactSourceUnit, units: &mut Vec<ArtifactSourceUnit>| {
            let listed = units.iter().any(|listed| {
                listed.unit.id() == unit.unit.id() && listed.unit.content() == unit.unit.content()
            });
            if !listed {
                units.push(unit);
            }
        };

        let styles: Vec<StyleArtifactInput> = style_rows
            .into_iter()
            .map(|(index, language, facts, code)| StyleArtifactInput {
                unit: mint(format!("style/{index}"), code.as_bytes()),
                language,
                facts,
                code,
            })
            .collect();
        let metadata_code: Arc<str> = Arc::from(metadata_json);
        let metadata_unit = mint("meta".to_owned(), metadata_code.as_bytes());

        let mut artifacts = Vec::new();
        let mut handoff = BTreeMap::new();
        let mut client = None;
        let mut server = None;
        for output in modules {
            let VueModuleOutput {
                product,
                dialect,
                module,
                sources,
            } = output;
            let keys = module.sources();
            if keys.len() != sources.len()
                || keys.iter().zip(&sources).any(|(key, source)| {
                    &key.unit != source.unit.unit.id() || &key.content != source.unit.unit.content()
                })
            {
                return Err(VueArtifactSetError::ModuleSourcesMismatch { product });
            }
            let (text, map) = module.into_parts();
            check_module_map(map.as_ref(), product)?;
            let code: Arc<str> = Arc::from(text);
            let module_unit = mint(product.wire_tag().to_owned(), code.as_bytes());
            let mut inputs = BTreeSet::from([module_unit.id().clone()]);
            inputs.extend(sources.iter().map(|source| source.unit.unit.id().clone()));
            let mut main = CompileArtifact::new(
                module_unit.id().clone(),
                product,
                dialect.schema_language(),
                "main",
                provenance(producer("vue-module"), inputs),
                ArtifactContent::Available(Arc::clone(&code)),
            )
            .with_dialect(dialect);
            if let Some(map) = map.as_ref().filter(|_| !sources.is_empty()) {
                main.maps.push(QualifiedArtifactMap {
                    family: ArtifactMapFamily::RuntimeSourceMap,
                    generated: main.id().clone(),
                    generated_content: module_unit.content().clone(),
                    input_basis: minted.input_basis.clone(),
                    sources: sources
                        .iter()
                        .map(|source| source.unit.unit.id().clone())
                        .collect(),
                    segments: module_segments(map, &sources),
                    authored_json: None,
                });
            }
            let module_id = main.id().clone();
            let slot = match product {
                ProductKind::RuntimeClient => &mut client,
                ProductKind::RuntimeServer => &mut server,
                _ => return Err(VueArtifactSetError::NotARuntimeModule { product }),
            };
            if slot.replace(module_id.clone()).is_some() {
                return Err(VueArtifactSetError::DuplicateModule { product });
            }

            for style in &styles {
                let mut artifact = CompileArtifact::new(
                    style.unit.id().clone(),
                    product,
                    style.language.clone(),
                    format!("style/{}", style.facts.index),
                    provenance(
                        producer("vue-style"),
                        BTreeSet::from([style.unit.id().clone()]),
                    ),
                    ArtifactContent::Available(Arc::clone(&style.code)),
                )
                .with_role(ArtifactRole::Style)
                .with_style(style.facts.clone());
                artifact.relations.insert(ArtifactRelation {
                    kind: ArtifactRelationKind::StyleOf,
                    target: module_id.clone(),
                });
                artifacts.push(artifact);
            }
            let mut meta = CompileArtifact::new(
                metadata_unit.id().clone(),
                product,
                LanguageId::new("json"),
                "meta",
                provenance(
                    producer("vue-metadata"),
                    BTreeSet::from([metadata_unit.id().clone()]),
                ),
                ArtifactContent::Available(Arc::clone(&metadata_code)),
            )
            .with_role(ArtifactRole::Metadata);
            meta.relations.insert(ArtifactRelation {
                kind: ArtifactRelationKind::MetadataOf,
                target: module_id.clone(),
            });
            artifacts.push(meta);

            let rows = if map.is_some() {
                sources
                    .iter()
                    .map(|source| {
                        source
                            .text
                            .as_ref()
                            .map(|text| (source.name.clone(), Arc::clone(text)))
                            .ok_or(ArtifactSchemaError::MapSourceWithoutContent)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };
            let units = if map.is_some() {
                sources.iter().map(|source| source.unit.clone()).collect()
            } else {
                Vec::new()
            };
            push_unit(
                ArtifactSourceUnit {
                    source_span: verter_span::Span::new(0, code.len() as u32),
                    unit: module_unit,
                },
                &mut source_units,
            );
            for source in sources {
                push_unit(source.unit, &mut source_units);
            }
            handoff.insert(
                module_id,
                ModuleHandoff {
                    dialect,
                    map,
                    rows,
                    units,
                },
            );
            artifacts.push(main);
        }
        for style in styles {
            let span = verter_span::Span::new(0, style.code.len() as u32);
            push_unit(
                ArtifactSourceUnit {
                    source_span: span,
                    unit: style.unit,
                },
                &mut source_units,
            );
        }
        push_unit(
            ArtifactSourceUnit {
                source_span: verter_span::Span::new(0, metadata_code.len() as u32),
                unit: metadata_unit,
            },
            &mut source_units,
        );

        // The custom blocks' facts were read from one carrier's bytes; their
        // regions address an `sfc` unit of this set over those bytes.
        let sfc_unit = match custom_blocks.blocks.first() {
            Some(first) => {
                let content = first.source_content.clone();
                if custom_blocks
                    .blocks
                    .iter()
                    .any(|block| block.source_content != content)
                {
                    return Err(VueArtifactSetError::CustomBlock(
                        CustomBlockDescriptorError::SourceMismatch,
                    ));
                }
                let unit =
                    SourceUnit::mint(source_id.clone(), minted.revision.clone(), "sfc", content);
                push_unit(
                    ArtifactSourceUnit {
                        source_span: verter_span::Span::new(0, custom_blocks.source_len),
                        unit: unit.clone(),
                    },
                    &mut source_units,
                );
                Some(unit)
            }
            None => None,
        };
        let attached_to = client.clone().or_else(|| server.clone());

        let set = CompileArtifactSet::new(source_units, artifacts)?;
        let built = Self {
            set,
            client,
            server,
            diagnostics,
            handoff,
            host_maps: Vec::new(),
        };
        let (Some(unit), Some(attached_to)) = (sfc_unit, attached_to) else {
            return Ok(built);
        };
        let provenance = provenance(
            producer("vue-custom-blocks"),
            BTreeSet::from([unit.id().clone()]),
        );
        let descriptors = custom_blocks
            .blocks
            .into_iter()
            .map(|block| custom_block_descriptor(block, &unit, &provenance, &attached_to))
            .collect::<Result<Vec<_>, _>>()
            .map_err(VueArtifactSetError::CustomBlock)?;
        built
            .attach_custom_blocks(descriptors)
            .map_err(VueArtifactSetError::CustomBlock)
    }

    pub fn set(&self) -> &CompileArtifactSet {
        &self.set
    }

    pub fn into_set(self) -> CompileArtifactSet {
        self.set
    }

    /// The module artifact of a runtime product, when it was requested.
    pub fn module(&self, product: ProductKind) -> Option<&ArtifactId> {
        match product {
            ProductKind::RuntimeClient => self.client.as_ref(),
            ProductKind::RuntimeServer => self.server.as_ref(),
            _ => None,
        }
    }

    pub fn diagnostics(&self) -> &[RuntimeDiagnostic] {
        &self.diagnostics
    }

    /// Host maps remain decoded only at the terminal handoff, and only for
    /// a product that requested mappings.
    pub(crate) fn with_host_maps(
        mut self,
        maps: Vec<(crate::assembly::SourceUnitId, Arc<str>)>,
    ) -> Self {
        self.host_maps = maps;
        self
    }

    /// Attach the component's custom-block descriptors; each names the
    /// module it is attached to.
    pub fn attach_custom_blocks(
        mut self,
        descriptors: Vec<CustomBlockDescriptor>,
    ) -> Result<Self, CustomBlockDescriptorError> {
        self.set = self.set.attach_custom_blocks(descriptors)?;
        Ok(self)
    }

    /// The staged handoff of one runtime product's module: the whole set,
    /// rooted at that module, with the module's V3 map written here, once.
    pub fn stage(
        &self,
        product: ProductKind,
    ) -> Result<StagedCompileArtifacts, ArtifactSchemaError> {
        let root = self
            .module(product)
            .ok_or(ArtifactSchemaError::UnknownRootArtifact)?;
        let handoff = self
            .handoff
            .get(root)
            .ok_or(ArtifactSchemaError::UnknownRootArtifact)?;
        if handoff.map.is_some()
            && handoff
                .units
                .iter()
                .any(|unit| self.host_maps.iter().any(|(id, _)| id == unit.unit.id()))
        {
            return self.stage_host_maps(root, product, handoff);
        }
        let source_map = match (&handoff.map, self.set.artifact(root).map(|a| &a.content)) {
            (Some(map), Some(ArtifactContent::Available(code))) => {
                let rows: Vec<MapSource<'_>> = handoff
                    .rows
                    .iter()
                    .map(|(name, text)| MapSource {
                        name,
                        content: Some(text.as_ref()),
                    })
                    .collect();
                Some(map.to_source_map(code, &rows, None).to_json_string())
            }
            _ => None,
        };
        StagedCompileArtifacts::stage(self.set.clone(), root.clone(), handoff.dialect, source_map)
    }

    fn stage_host_maps(
        &self,
        root: &ArtifactId,
        product: ProductKind,
        handoff: &ModuleHandoff,
    ) -> Result<StagedCompileArtifacts, ArtifactSchemaError> {
        use crate::assembly::map_input::validate_and_decode;
        let mut map = handoff.map.clone().expect("a mapped handoff");
        let mut rows = handoff.rows.clone();
        let mut units = handoff.units.clone();
        let code = match &self
            .set
            .artifact(root)
            .ok_or(ArtifactSchemaError::UnknownRootArtifact)?
            .content
        {
            ArtifactContent::Available(code) => code,
            _ => return Err(ArtifactSchemaError::UnavailableRootArtifact),
        };
        // Only the original source rows are candidates. A host source added
        // by one remap is never fed through another supplied unit's map.
        for (index, unit) in handoff.units.iter().enumerate() {
            let Some((_, raw)) = self.host_maps.iter().find(|(id, _)| id == unit.unit.id()) else {
                continue;
            };
            let host = validate_and_decode(raw, &handoff.rows[index].1)
                .map_err(|_| ArtifactSchemaError::UnqualifiedMap)?;
            let contents = host.sources_content();
            // Positional-only host maps cannot replace the content-backed
            // compiler map. Keep the supplied source, as on the block route.
            if host
                .segments()
                .iter()
                .filter_map(|segment| segment.payload)
                .any(|payload| {
                    contents
                        .and_then(|contents| contents.get(payload.source_index as usize))
                        .and_then(Option::as_deref)
                        .is_none_or(str::is_empty)
                })
            {
                continue;
            }
            for (host_index, name) in host.sources().iter().enumerate() {
                let text: Arc<str> = Arc::from(
                    contents
                        .and_then(|contents| contents.get(host_index))
                        .and_then(Option::as_deref)
                        .unwrap_or_default(),
                );
                let name = match host.source_root().filter(|root| !root.is_empty()) {
                    Some(root) => format!(
                        "{}{name}",
                        if root.ends_with('/') {
                            root.to_owned()
                        } else {
                            format!("{root}/")
                        }
                    ),
                    None => name.clone(),
                };
                units.push(ArtifactSourceUnit {
                    unit: SourceUnit::mint(
                        unit.unit.source_id().clone(),
                        unit.unit.revision().clone(),
                        format!("{}/host/{host_index}", unit.unit.logical_role()),
                        ContentId::from_content_bytes(text.as_bytes()),
                    ),
                    source_span: verter_span::Span::new(0, text.len() as u32),
                });
                rows.push((name, text));
            }
            let sources: Vec<_> = rows
                .iter()
                .map(|(name, text)| MapSource {
                    name,
                    content: Some(text),
                })
                .collect();
            map.remap_source(code, index as u32, &host, &sources)?;
        }
        // Remapping own sources never admits arbitrary external pieces.
        check_module_map(Some(&map), product).map_err(|_| ArtifactSchemaError::UnqualifiedMap)?;
        let mut artifacts = self.set.artifacts().to_vec();
        let artifact = artifacts
            .iter_mut()
            .find(|artifact| artifact.id() == root)
            .ok_or(ArtifactSchemaError::UnknownRootArtifact)?;
        let sources: Vec<_> = units
            .iter()
            .zip(&rows)
            .map(|(unit, (name, text))| VueModuleSource {
                unit: unit.clone(),
                name: name.clone(),
                text: Some(text.clone()),
            })
            .collect();
        artifact
            .provenance
            .inputs
            .extend(units.iter().map(|unit| unit.unit.id().clone()));
        if let Some(qualified) = artifact
            .maps
            .iter_mut()
            .find(|map| map.family == ArtifactMapFamily::RuntimeSourceMap)
        {
            qualified.sources = units.iter().map(|unit| unit.unit.id().clone()).collect();
            qualified.segments = module_segments(&map, &sources);
        }
        let mut source_units: BTreeMap<_, _> = self
            .set
            .source_units()
            .map(|unit| (unit.unit.id().clone(), unit.clone()))
            .collect();
        for unit in units {
            if let Some(existing) = source_units.get(unit.unit.id()) {
                if existing.unit.content() != unit.unit.content() {
                    return Err(ArtifactSchemaError::ConflictingSourceRevision);
                }
            } else {
                source_units.insert(unit.unit.id().clone(), unit);
            }
        }
        let set = CompileArtifactSet::new(source_units.into_values().collect(), artifacts)?
            .attach_custom_blocks(self.set.custom_blocks().to_vec())
            .map_err(|_| ArtifactSchemaError::InvalidIdentity)?;
        let sources: Vec<_> = rows
            .iter()
            .map(|(name, text)| MapSource {
                name,
                content: Some(text),
            })
            .collect();
        let source_map = map.to_source_map(code, &sources, None).to_json_string();
        StagedCompileArtifacts::stage(set, root.clone(), handoff.dialect, Some(source_map))
    }
}

fn check_module_map(
    map: Option<&DecodedMap>,
    product: ProductKind,
) -> Result<(), VueArtifactSetError> {
    if map.is_some_and(DecodedMap::has_external) {
        Err(VueArtifactSetError::ExternalMapUnsupported { product })
    } else {
        Ok(())
    }
}

/// The published bytes, language and facts of every style block, in block
/// order. A block whose rewrite left it unchanged, or that still needs an
/// external stage, publishes its supplied bytes.
type StyleRow = (u32, LanguageId, StyleArtifactFacts, Arc<str>);

fn style_rows(
    blocks: &[VueStyleBlockPlan],
    inputs: &[VueStyleBlockInput<'_>],
    metadata: &VueComponentMetadata,
) -> Result<Vec<StyleRow>, VueArtifactSetError> {
    let mut rows = Vec::with_capacity(blocks.len());
    for block in blocks {
        let index = block.index;
        let input = inputs
            .iter()
            .find(|input| input.index == index)
            .ok_or(VueArtifactSetError::MissingStyleBlock { index })?;
        let lang = metadata
            .styles
            .iter()
            .find(|style| style.index == index)
            .map(|style| style.lang.clone())
            .ok_or(VueArtifactSetError::MissingStyleBlock { index })?;
        let (code, stage, needs_external_stage): (Arc<str>, _, _) = match &block.outcome {
            VueStyleOutcome::Rewritten { code, .. } => (
                Arc::from(code.as_str()),
                verter_css_syntax::StyleStage::FrameworkRewritten,
                false,
            ),
            VueStyleOutcome::Unchanged => (Arc::from(input.authored.code), input.stage, false),
            VueStyleOutcome::NeedInput(_) => (Arc::from(input.authored.code), input.stage, true),
            VueStyleOutcome::Refused(_) => return Err(VueArtifactSetError::StyleRefused { index }),
        };
        let language = LanguageId::new(if needs_external_stage { &lang } else { "css" });
        rows.push((
            index,
            language,
            StyleArtifactFacts {
                index,
                scoped: block.scoped,
                module: block.module.clone(),
                lang,
                stage: Some(stage),
                needs_external_stage,
            },
            code,
        ));
    }
    Ok(rows)
}

/// The module's mappings as artifact segments, read from its decoded map:
/// an exact run maps byte for byte, a rewritten or anchored run maps to
/// its source point.
fn module_segments(map: &DecodedMap, sources: &[VueModuleSource]) -> Vec<ArtifactMapSegment> {
    map.segments()
        .iter()
        .filter_map(|segment| {
            let source = sources.get(segment.source as usize)?;
            let start = source.unit.source_span.start + segment.source_start;
            let end = match segment.kind {
                MapSegmentKind::Identity => {
                    start + (segment.generated.end - segment.generated.start)
                }
                MapSegmentKind::Rewritten | MapSegmentKind::Anchor => start,
            };
            Some(ArtifactMapSegment {
                generated: segment.generated.clone(),
                source_unit: source.unit.unit.id().clone(),
                source_span: verter_span::Span::new(start, end),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "artifact_set_tests.rs"]
mod tests;
