//! Flat, single-owner document algebra. Nodes may be attached once, so neither
//! traversal nor destruction recurses and a shared subtree cannot expand work.
use std::sync::atomic::{AtomicU64, Ordering};

use crate::provenance::{AuthoredProvenance, FormatProvenanceId, Provenanced};

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::FormatProvenanceId {}
}

/// Connects the document's identity parameter to the sole provenance binding.
/// Sealed: another token cannot stand in for a formatter identity.
pub trait DocProvenance: sealed::Sealed {
    type Binding: Copy + std::fmt::Debug;
}
impl DocProvenance for FormatProvenanceId {
    type Binding = AuthoredProvenance;
}

/// Handle scoped to its owning document. Group and line handles also identify
/// their group and break respectively; handles have no serialization meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocId {
    owner: u64,
    pub(crate) index: usize,
}

/// In a flat group, Space emits a space and Soft emits nothing. Both break in
/// broken mode. Hard always emits a newline and forces enclosing groups broken.
#[derive(Debug, Clone, Copy)]
pub enum Line {
    Space,
    Soft,
    Hard,
}

#[derive(Debug)]
pub(crate) enum Node<P: DocProvenance> {
    Text(String, Option<P::Binding>),
    Concat(Vec<usize>),
    Group(usize),
    Indent(usize, usize),
    Align(usize, usize),
    IfBreak { broken: usize, flat: usize },
    Line(Line),
    Suffix(String, Option<P::Binding>),
}

/// An append-only tree stored in postorder. The identity parameter is fixed to
/// the foundation's sealed provenance contract, not a second identity minter.
#[derive(Debug)]
pub struct Doc<P: DocProvenance = FormatProvenanceId> {
    pub(crate) nodes: Vec<Node<P>>,
    attached: Vec<bool>,
    owner: u64,
}

impl Default for Doc {
    fn default() -> Self {
        Self::new()
    }
}

impl Doc {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("document identity space exhausted");
        Self {
            nodes: Vec::new(),
            attached: Vec::new(),
            owner,
        }
    }

    fn push(&mut self, node: Node<FormatProvenanceId>) -> DocId {
        let id = DocId {
            owner: self.owner,
            index: self.nodes.len(),
        };
        self.nodes.push(node);
        self.attached.push(false);
        id
    }

    fn attach(&mut self, id: DocId) -> usize {
        assert!(self.contains(id), "foreign document handle");
        assert!(!self.attached[id.index], "document node already attached");
        self.attached[id.index] = true;
        id.index
    }

    pub(crate) fn contains(&self, id: DocId) -> bool {
        id.owner == self.owner && id.index < self.nodes.len()
    }

    /// Generated text. Text is preserved byte for byte, including raw newlines.
    pub fn text(&mut self, text: impl Into<String>) -> DocId {
        self.push(Node::Text(text.into(), None))
    }

    /// Source text carries its original binding unchanged to the output.
    pub fn source(&mut self, text: Provenanced<String>) -> DocId {
        let (text, binding) = text.into_parts();
        self.push(Node::Text(text, Some(binding)))
    }

    pub fn concat(&mut self, children: impl IntoIterator<Item = DocId>) -> DocId {
        let children = children.into_iter().map(|id| self.attach(id)).collect();
        self.push(Node::Concat(children))
    }

    pub fn group(&mut self, body: DocId) -> DocId {
        let body = self.attach(body);
        self.push(Node::Group(body))
    }

    /// Add columns to the indentation used after breaks.
    pub fn indent(&mut self, columns: usize, body: DocId) -> DocId {
        let body = self.attach(body);
        self.push(Node::Indent(columns, body))
    }

    /// Set indentation to the current output column plus `columns`.
    pub fn align(&mut self, columns: usize, body: DocId) -> DocId {
        let body = self.attach(body);
        self.push(Node::Align(columns, body))
    }

    /// Select using the nearest enclosing group's mode (broken at the root).
    pub fn if_break(&mut self, broken: DocId, flat: DocId) -> DocId {
        let broken = self.attach(broken);
        let flat = self.attach(flat);
        self.push(Node::IfBreak { broken, flat })
    }

    pub fn line(&mut self, line: Line) -> DocId {
        self.push(Node::Line(line))
    }

    /// Queue single-line text, flushed FIFO before the next newline or EOF.
    /// A suffix is atomic: it cannot itself contain document control nodes.
    pub fn line_suffix(&mut self, text: impl Into<String>) -> DocId {
        let text = text.into();
        assert!(
            !text.contains(['\r', '\n']),
            "line suffix must be single-line"
        );
        self.push(Node::Suffix(text, None))
    }

    pub fn source_line_suffix(&mut self, text: Provenanced<String>) -> DocId {
        let (text, binding) = text.into_parts();
        assert!(
            !text.contains(['\r', '\n']),
            "line suffix must be single-line"
        );
        self.push(Node::Suffix(text, Some(binding)))
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}
