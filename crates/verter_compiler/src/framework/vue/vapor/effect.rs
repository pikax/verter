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
    /// `svg` passes runtime-vapor's SVG flag: an SVG element's class is
    /// its `class` attribute.
    Class {
        node_ref: u32,
        expr: RunId,
        svg: bool,
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
    /// `svg` passes runtime-vapor's SVG flag.
    Attr {
        node_ref: u32,
        attr: &'c str,
        expr: RunId,
        svg: bool,
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
    /// `setValue`: an element's `value`, kept beside the DOM property.
    Value {
        node_ref: u32,
        expr: RunId,
    },
    /// `setElementText`: a bound `textContent`.
    ElementText {
        node_ref: u32,
        expr: RunId,
    },
    /// `svg` passes runtime-vapor's SVG flag, so each key is set as an SVG
    /// element's attribute rather than as its (read-only) DOM property.
    DynamicProps {
        node_ref: u32,
        expr: RunId,
        svg: bool,
    },
}
