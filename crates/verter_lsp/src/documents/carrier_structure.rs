//! Read-only LSP views over the registered carrier inventory.
//!
//! Geometry in this module is copied from `CarrierBlockInventory`; source text
//! is sliced only through validated spans. No carrier delimiter is searched.

use verter_language::parse_artifact::carrier_inventory::{
    AttributeValue, CarrierAttribute, CarrierBlock, MarkupElementSyntax, MarkupNodeKind,
    TaggedSyntax,
};
use verter_session::carrier_publication_store::{
    ArtifactAttributeRef, FrameworkBlockRef, RegisteredFileStructure,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttr {
    pub attribute_ref: ArtifactAttributeRef,
    pub name: String,
    pub value: Option<String>,
    pub name_start: u32,
    pub name_end: u32,
    pub value_start: Option<u32>,
    pub value_end: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierBlockView {
    pub block_ref: FrameworkBlockRef,
    pub tag_name: String,
    pub open_tag_start: u32,
    pub open_tag_end: u32,
    pub close_tag_start: u32,
    pub close_tag_end: u32,
    pub opening_name_start: u32,
    pub opening_name_end: u32,
    pub attribute_insertion_anchor: u32,
    pub attrs_raw: String,
    pub attributes: Vec<ParsedAttr>,
}

impl CarrierBlockView {
    pub fn content_range(&self) -> (u32, u32) {
        (self.open_tag_end, self.close_tag_start)
    }

    pub fn attr(&self, name: &str) -> Option<Option<&str>> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_deref())
    }

    pub fn is_setup(&self) -> bool {
        self.tag_name == "script" && self.attr("setup").is_some()
    }

    pub fn lang(&self) -> Option<&str> {
        self.attr("lang").flatten()
    }

    pub fn is_scoped(&self) -> bool {
        self.attr("scoped").is_some()
    }

    pub fn is_module(&self) -> bool {
        self.attr("module").is_some()
    }

    pub fn attrs(&self) -> Option<&str> {
        self.attr("attributes")
            .flatten()
            .or_else(|| self.attr("attrs").flatten())
    }
}

/// One markup open-tag fact projected from the registered inventory's markup
/// arena: the parser-identified opening span (+ authored name) of an
/// element-like node — `Element`, `Recovered`, or `Unknown` — plus the nearest
/// element-like ancestor for typed parent walks. Geometry is copied from the
/// arena; no carrier delimiter is searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupOpenTagFact {
    /// Authored tag name, when the parser retained one.
    pub name: Option<String>,
    /// Parser-identified opening-tag span (`<tag …>` inclusive of delimiters).
    pub opening_start: u32,
    pub opening_end: u32,
    /// End of the tag-name token inside the opening span.
    pub name_end: u32,
    /// Full node span (opening + content + closing).
    pub full_start: u32,
    pub full_end: u32,
    /// Nearest element-like ancestor, as an index into the projected vec.
    pub parent: Option<usize>,
}

/// Project every element-like markup node (element / recovered / unknown with
/// a retained opening span) into [`MarkupOpenTagFact`]s, preserving arena
/// parent relations collapsed to the nearest element-like ancestor.
pub fn project_markup_open_tags(structure: &RegisteredFileStructure) -> Vec<MarkupOpenTagFact> {
    let inventory = structure.inventory();
    let nodes = inventory.markup().nodes();
    let mut arena_to_fact: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut facts = Vec::new();
    for (arena_index, node) in nodes.iter().enumerate() {
        let projected = match node.kind() {
            MarkupNodeKind::Element(element) => {
                let full = element.full_span;
                Some((
                    inventory
                        .slice(element.authored_name)
                        .ok()
                        .map(str::to_string),
                    element.opening_span.start,
                    element.opening_span.end,
                    element.opening_name_span.end,
                    full.start,
                    full.end,
                ))
            }
            MarkupNodeKind::Recovered {
                opening_span,
                opening_name_span,
                full_span,
                ..
            }
            | MarkupNodeKind::Unknown {
                opening_span,
                opening_name_span,
                full_span,
                ..
            } => opening_span.map(|opening| {
                (
                    opening_name_span.and_then(|span| {
                        inventory
                            .source_spaces()
                            .first()
                            .and_then(|space| {
                                space.bytes().get(span.start as usize..span.end as usize)
                            })
                            .map(str::to_string)
                    }),
                    opening.start,
                    opening.end,
                    opening_name_span.map_or(opening.start, |span| span.end),
                    full_span.start,
                    full_span.end,
                )
            }),
            _ => None,
        };
        let Some((name, opening_start, opening_end, name_end, full_start, full_end)) = projected
        else {
            continue;
        };
        let parent = {
            let mut ancestor = node.parent;
            loop {
                match ancestor {
                    Some(id) => match arena_to_fact.get(id.get() as usize).copied().flatten() {
                        Some(fact_index) => break Some(fact_index),
                        None => {
                            ancestor = nodes.get(id.get() as usize).and_then(|node| node.parent)
                        }
                    },
                    None => break None,
                }
            }
        };
        arena_to_fact[arena_index] = Some(facts.len());
        facts.push(MarkupOpenTagFact {
            name,
            opening_start,
            opening_end,
            name_end,
            full_start,
            full_end,
            parent,
        });
    }
    facts
}

/// Innermost fact whose parser-identified OPENING span contains `offset`.
pub fn markup_open_tag_at(facts: &[MarkupOpenTagFact], offset: u32) -> Option<usize> {
    facts
        .iter()
        .enumerate()
        .filter(|(_, fact)| offset >= fact.opening_start && offset < fact.opening_end)
        .max_by_key(|(_, fact)| fact.opening_start)
        .map(|(index, _)| index)
}

/// Innermost fact whose FULL node span contains `offset` (for ancestor walks
/// from content positions).
pub fn markup_element_at(facts: &[MarkupOpenTagFact], offset: u32) -> Option<usize> {
    facts
        .iter()
        .enumerate()
        .filter(|(_, fact)| offset >= fact.full_start && offset < fact.full_end)
        .min_by_key(|(_, fact)| fact.full_end - fact.full_start)
        .map(|(index, _)| index)
}

#[derive(Debug, Clone)]
pub struct OpeningTagContext {
    pub tag_name: String,
    pub tag_name_start: u32,
    pub tag_name_end: u32,
    pub attrs: Vec<ParsedAttr>,
}

/// Return the already-parsed opening-tag facts. `source` remains in the
/// signature while feature APIs are cut over, but it is never inspected.
pub fn parse_opening_tag(_source: &str, block: &CarrierBlockView) -> OpeningTagContext {
    OpeningTagContext {
        tag_name: block.tag_name.clone(),
        tag_name_start: block.opening_name_start,
        tag_name_end: block.opening_name_end,
        attrs: block.attributes.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierCursorContext {
    BlockContent { block: CarrierBlockView },
    OpeningTag { block: CarrierBlockView },
    ClosingTag { block: CarrierBlockView },
    RootLevel,
}

pub fn classify_cursor(offset: u32, blocks: &[CarrierBlockView]) -> CarrierCursorContext {
    for block in blocks {
        if offset >= block.open_tag_start && offset < block.open_tag_end {
            return CarrierCursorContext::OpeningTag {
                block: block.clone(),
            };
        }
        if offset >= block.close_tag_start && offset < block.close_tag_end {
            return CarrierCursorContext::ClosingTag {
                block: block.clone(),
            };
        }
        let (content_start, content_end) = block.content_range();
        if offset >= content_start && offset < content_end {
            return CarrierCursorContext::BlockContent {
                block: block.clone(),
            };
        }
    }
    CarrierCursorContext::RootLevel
}

pub fn project_carrier_blocks(structure: &RegisteredFileStructure) -> Vec<CarrierBlockView> {
    let inventory = structure.inventory();
    let mut blocks = inventory
        .blocks()
        .iter()
        .filter_map(|block| match block {
            CarrierBlock::Section { id, syntax, .. } => {
                project_tagged(structure, structure.block_ref(*id)?, syntax)
            }
            CarrierBlock::MarkupRoot { id, node } => {
                let node = inventory.markup().nodes().get(node.get() as usize)?;
                let MarkupNodeKind::Element(element) = node.kind() else {
                    return None;
                };
                project_element(structure, structure.block_ref(*id)?, element)
            }
        })
        .collect::<Vec<_>>();

    // Error recovery may leave a template host without a closing/content span
    // while still publishing the following top-level section. Keep ownership
    // structural: the next inventory block (or EOF), never a delimiter search,
    // bounds the incomplete template region.
    let source = inventory.source_spaces().first().map(|space| space.bytes());
    let starts = blocks
        .iter()
        .map(|block| block.open_tag_start)
        .collect::<Vec<_>>();
    for (index, block) in blocks.iter_mut().enumerate() {
        let opening_is_self_closing = source.is_some_and(|source| {
            source
                .get(block.open_tag_start as usize..block.open_tag_end as usize)
                .is_some_and(|opening| opening.ends_with("/>"))
        });
        if block.tag_name == "template"
            && block.close_tag_start == block.open_tag_end
            && block.close_tag_end == block.open_tag_end
            && !opening_is_self_closing
        {
            let boundary = starts
                .get(index + 1)
                .copied()
                .or_else(|| source.map(|source| source.len() as u32))
                .unwrap_or(block.open_tag_end);
            block.close_tag_start = boundary;
            block.close_tag_end = boundary;
        }
    }
    blocks
}

pub fn project_carrier_blocks_for_document(
    document: &super::DocumentState,
) -> Vec<CarrierBlockView> {
    document
        .feature_snapshot
        .as_ref()
        .map(|snapshot| project_carrier_blocks(snapshot.structure()))
        .unwrap_or_default()
}

fn project_tagged(
    structure: &RegisteredFileStructure,
    block_ref: FrameworkBlockRef,
    syntax: &TaggedSyntax,
) -> Option<CarrierBlockView> {
    let inventory = structure.inventory();
    let tag_name = inventory.normalized_name(syntax.normalized_name).ok()?;
    project_common(
        structure,
        block_ref,
        tag_name,
        syntax.opening_span.start,
        syntax.opening_span.end,
        syntax.opening_name_span.start,
        syntax.opening_name_span.end,
        syntax.attribute_insertion_anchor.start,
        syntax.content_span.end,
        syntax.closing_span.map(|span| span.start),
        syntax.closing_span.map(|span| span.end),
        &syntax.attributes,
    )
}

fn project_element(
    structure: &RegisteredFileStructure,
    block_ref: FrameworkBlockRef,
    syntax: &MarkupElementSyntax,
) -> Option<CarrierBlockView> {
    let inventory = structure.inventory();
    let tag_name = inventory.normalized_name(syntax.normalized_name).ok()?;
    project_common(
        structure,
        block_ref,
        tag_name,
        syntax.opening_span.start,
        syntax.opening_span.end,
        syntax.opening_name_span.start,
        syntax.opening_name_span.end,
        syntax.attribute_insertion_anchor.start,
        syntax.content_span.end,
        syntax.closing_span.map(|span| span.start),
        syntax.closing_span.map(|span| span.end),
        &syntax.attributes,
    )
}

#[allow(clippy::too_many_arguments)]
fn project_common(
    structure: &RegisteredFileStructure,
    block_ref: FrameworkBlockRef,
    tag_name: &str,
    open_tag_start: u32,
    open_tag_end: u32,
    opening_name_start: u32,
    opening_name_end: u32,
    attribute_insertion_anchor: u32,
    content_end: u32,
    close_start: Option<u32>,
    close_end: Option<u32>,
    attributes: &[CarrierAttribute],
) -> Option<CarrierBlockView> {
    let inventory = structure.inventory();
    let attrs_raw = inventory
        .source_spaces()
        .first()?
        .bytes()
        .get(opening_name_end as usize..attribute_insertion_anchor as usize)?
        .to_string();
    let close_tag_start = close_start.unwrap_or(content_end);
    let close_tag_end = close_end.unwrap_or(content_end);
    let attributes = attributes
        .iter()
        .filter_map(|attribute| project_attribute(structure, attribute))
        .collect();
    Some(CarrierBlockView {
        block_ref,
        tag_name: tag_name.to_string(),
        open_tag_start,
        open_tag_end,
        close_tag_start,
        close_tag_end,
        opening_name_start,
        opening_name_end,
        attribute_insertion_anchor,
        attrs_raw,
        attributes,
    })
}

fn project_attribute(
    structure: &RegisteredFileStructure,
    attribute: &CarrierAttribute,
) -> Option<ParsedAttr> {
    let CarrierAttribute::Named {
        id, name, value, ..
    } = attribute
    else {
        return None;
    };
    let inventory = structure.inventory();
    let value_slice = match value {
        AttributeValue::Static { raw, .. } => Some(*raw),
        _ => None,
    };
    Some(ParsedAttr {
        attribute_ref: structure.attribute_ref(*id)?,
        name: inventory.slice(name.authored).ok()?.to_string(),
        value: value_slice
            .and_then(|slice| inventory.slice(slice).ok())
            .map(str::to_string),
        name_start: name.name_span.start,
        name_end: name.name_span.end,
        value_start: value_slice.map(|slice| slice.span.start),
        value_end: value_slice.map(|slice| slice.span.end),
    })
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_structure(source: &str, svelte: bool) -> RegisteredFileStructure {
    use std::sync::Arc;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = verter_session::VerterHost::new(verter_session::HostConfig::default(), workspace);
    let registry = verter_session::LanguageRegistry::global();
    let extension = registry
        .carrier_extensions()
        .iter()
        .copied()
        .find(|extension| {
            registry
                .classify_static(&format!("fixture.{extension}"))
                .static_resolution()
                .is_svelte()
                == svelte
        })
        .expect("registered carrier extension");
    let canonical = format!("/fixture.{extension}");
    let file_language = registry.classify_static(&canonical).static_resolution();
    let _ = host
        .upsert(verter_session::UpsertRequest {
            canonical_id: Some(canonical.clone()),
            input_id: canonical.clone(),
            source: Arc::from(source),
            file_language,
            aliases: vec![],
        })
        .expect("registered test carrier");
    host.registered_file_structure_snapshot(&canonical)
        .expect("registered test structure")
        .0
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_carrier_blocks(source: &str) -> Vec<CarrierBlockView> {
    project_carrier_blocks(&test_structure(source, false))
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestCarrierKind {
    RawText,
    Markup,
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_carrier_blocks_with(source: &str, kind: TestCarrierKind) -> Vec<CarrierBlockView> {
    match kind {
        TestCarrierKind::RawText => test_carrier_blocks(source),
        TestCarrierKind::Markup => test_svelte_blocks(source),
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn custom_block_content_kind(language_id: Option<&str>, canonical_id: &str) -> TestCarrierKind {
    if language_id == Some("svelte")
        || verter_session::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution()
            .is_svelte()
    {
        TestCarrierKind::Markup
    } else {
        TestCarrierKind::RawText
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn test_svelte_blocks(source: &str) -> Vec<CarrierBlockView> {
    project_carrier_blocks(&test_structure(source, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_projection_preserves_duplicate_attributes_and_sealed_refs() {
        let structure = test_structure(
            "<template><div/></template><script setup lang='ts' lang=js>ok</script>",
            false,
        );
        let blocks = project_carrier_blocks(&structure);
        let script = blocks
            .iter()
            .find(|block| block.tag_name == "script")
            .expect("script");
        assert!(script.is_setup());
        assert_eq!(script.lang(), Some("ts"));
        assert_eq!(script.attributes.len(), 3, "duplicates remain ordered");
        assert_eq!(script.block_ref.artifact_id(), structure.artifact_id());
        assert!(script
            .attributes
            .iter()
            .all(|attribute| attribute.attribute_ref.artifact_id() == structure.artifact_id()));
    }

    #[test]
    fn svelte_multi_root_projects_each_inventory_owner_once() {
        let structure = test_structure("<main/><aside/><script>let x = 1</script>", true);
        let blocks = project_carrier_blocks(&structure);
        assert_eq!(
            blocks
                .iter()
                .map(|block| block.tag_name.as_str())
                .collect::<Vec<_>>(),
            ["main", "aside", "script"]
        );
        let unique = blocks
            .iter()
            .map(|block| block.block_ref.block_id())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), blocks.len());
    }

    #[test]
    fn incomplete_template_is_bounded_by_the_next_inventory_block() {
        let source = "<template>\n  <div >\n</template>\n<script setup>\n</script>";
        let blocks = test_carrier_blocks(source);
        let template = blocks
            .iter()
            .find(|block| block.tag_name == "template")
            .expect("template");
        let boundary = blocks
            .iter()
            .find(|block| block.open_tag_start > template.open_tag_start)
            .map_or(source.len() as u32, |block| block.open_tag_start);
        assert_eq!(template.close_tag_start, boundary);
        assert!(source.find("<div >").unwrap() as u32 >= template.open_tag_end);
        assert!((source.find("<div >").unwrap() as u32) < template.close_tag_start);
    }
}
