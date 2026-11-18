//! Ordered operation records, before any output is written.
use std::ops::Range;

use verter_span::Span;

use super::effect::EffectGroupId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RunId(pub(super) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameKind {
    Node,
    Text,
    Path,
    Template,
    Number,
}

impl NameKind {
    pub(super) fn prefix(self) -> &'static str {
        match self {
            Self::Node => "n",
            Self::Text => "x",
            Self::Path => "p",
            Self::Template => "t",
            Self::Number => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part<'c> {
    Text(&'c str),
    Name(NameKind, u32),
    Source(Span),
    Anchored(&'c str, u32),
    Run(RunId),
    Indented(RunId, &'c str),
    LineBreak,
    Effects(EffectGroupId),
}

pub(super) type RunBuf<'c> = Vec<Part<'c>>;

#[derive(Debug, Default)]
pub(super) struct Runs<'c> {
    parts: Vec<Part<'c>>,
    ranges: Vec<Range<usize>>,
}

impl<'c> Runs<'c> {
    pub(super) fn commit(&mut self, buf: &mut RunBuf<'c>) -> RunId {
        let start = self.parts.len();
        self.parts.append(buf);
        let id = RunId(self.ranges.len() as u32);
        self.ranges.push(start..self.parts.len());
        id
    }

    pub(super) fn get(&self, id: RunId) -> &[Part<'c>] {
        &self.parts[self.ranges[id.0 as usize].clone()]
    }
}

/// One navigated child, declared once in the closure that uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Nav {
    Reserved,
    /// The child at DOM position `index` of `container`, declared under
    /// `target`: a node name when output names the node (effects, events,
    /// insertion points, its own children), a path name when only its text is
    /// read. `text` marks a text node, which hydration resolves as text.
    At {
        target: (NameKind, u32),
        container: u32,
        index: u32,
        text: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TextCreation {
    pub(super) text: u32,
    /// The navigated element holding the text: a node or a path name.
    pub(super) from: (NameKind, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InsertionAnchor {
    Append,
    Index(u32),
    NodeRef(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stmt {
    Reserved,
    InsertionState {
        container: u32,
        anchor: InsertionAnchor,
    },
    Run(RunId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockKind {
    Node,
    Component,
    Fragment,
    /// Several sibling roots, returned as an array.
    Roots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClosedBlock {
    pub(super) run: RunId,
    pub(super) node_ref: u32,
    pub(super) template_idx: Option<u32>,
    pub(super) is_static: bool,
    pub(super) kind: BlockKind,
}
