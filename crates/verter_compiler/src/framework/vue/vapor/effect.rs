//! Effects belong to one closed block; scopes carry their stable row ids.
use super::ops::RunId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EffectId(pub(super) u32);

pub(super) type EffectGroupId =
    crate::framework_common::structure::Id<super::graph::VaporEffectKey>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextPart<'c> {
    Static(&'c str),
    Dynamic(RunId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum VaporEffect<'c> {
    Text {
        text_ref: u32,
        parts: Vec<TextPart<'c>>,
        generated: bool,
    },
    Class {
        node_ref: u32,
        expr: RunId,
    },
    Style {
        node_ref: u32,
        expr: RunId,
    },
    Prop {
        node_ref: u32,
        attr: &'c str,
        expr: RunId,
    },
    Attr {
        node_ref: u32,
        attr: &'c str,
        expr: RunId,
    },
    DomProp {
        node_ref: u32,
        attr: &'c str,
        expr: RunId,
    },
    Html {
        node_ref: u32,
        expr: RunId,
    },
    DynamicProps {
        node_ref: u32,
        expr: RunId,
    },
}
