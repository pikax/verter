//! Ordered SFC block facts for file-level lint rules.
//!
//! The facts are a read-only projection of the registered carrier inventory:
//! parsed roles, parser-identified opening spans, and parsed attributes in
//! source order. Rules consume these facts instead of scanning raw source for
//! block delimiters, so decoy `<script`/`<style`/`<template` literals inside
//! strings or comments can never fabricate a block.

use verter_language::parse_artifact::carrier_inventory::{
    AttributeValue, CarrierAttribute, CarrierBlock, CarrierBlockInventory, SectionRole,
};
use verter_span::Span;

/// Parsed role of one top-level SFC section, in inventory order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfcBlockRole {
    Template,
    Script,
    Style,
    Custom,
}

/// One parsed attribute on a section's opening tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfcBlockAttribute {
    pub name: String,
    /// Static attribute value, when authored (`lang="ts"` → `Some("ts")`;
    /// bare `scoped` → `None`).
    pub value: Option<String>,
    /// Parser-identified span of the attribute NAME token.
    pub name_span: Span,
}

/// One top-level SFC section: parsed role + parser-identified opening span +
/// parsed attributes. Ordered by source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfcBlockFact {
    pub role: SfcBlockRole,
    /// The whole opening tag (`<script setup lang="ts">`).
    pub opening_span: Span,
    /// The parser-identified content span between the opening and closing
    /// tags — the sole legal anchor for content-relative edits.
    pub content_span: Span,
    /// Parser-identified position where a new attribute is inserted (just
    /// before the opening tag's `>`).
    pub attribute_insertion_anchor: u32,
    pub attributes: Vec<SfcBlockAttribute>,
}

/// Project the ordered section facts out of a registered carrier inventory.
/// Markup roots (Svelte root elements) are not SFC sections and are skipped.
pub fn project_block_facts(inventory: &CarrierBlockInventory) -> Vec<SfcBlockFact> {
    inventory
        .blocks()
        .iter()
        .filter_map(|block| {
            let CarrierBlock::Section { role, syntax, .. } = block else {
                return None;
            };
            let role = match role {
                SectionRole::TemplateHost => SfcBlockRole::Template,
                SectionRole::Script { .. } => SfcBlockRole::Script,
                SectionRole::Style { .. } => SfcBlockRole::Style,
                SectionRole::Custom { .. } => SfcBlockRole::Custom,
            };
            let attributes = syntax
                .attributes
                .iter()
                .filter_map(|attribute| {
                    let CarrierAttribute::Named { name, value, .. } = attribute else {
                        return None;
                    };
                    Some(SfcBlockAttribute {
                        name: inventory.slice(name.authored).ok()?.to_string(),
                        value: match value {
                            AttributeValue::Static { raw, .. } => {
                                inventory.slice(*raw).ok().map(str::to_string)
                            }
                            _ => None,
                        },
                        name_span: Span::new(name.name_span.start, name.name_span.end),
                    })
                })
                .collect();
            Some(SfcBlockFact {
                role,
                opening_span: Span::new(syntax.opening_span.start, syntax.opening_span.end),
                content_span: Span::new(syntax.content_span.start, syntax.content_span.end),
                attribute_insertion_anchor: syntax.attribute_insertion_anchor.start,
                attributes,
            })
        })
        .collect()
}
