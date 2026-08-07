use smallvec::SmallVec;
use verter_span::Span;

use crate::diagnostic::{
    CssDiagnostic, CssParseFailure, CssStructureTooLarge, StructureOverflowKind,
};
use crate::dialect::CssDialect;
use crate::event::{NodeFlags, ParseEvent, ParseEventSink, SyntaxKind};
use crate::parser::{parse_with_sink, CssEntryPoint, CssParseMode, CssSource};
use crate::token::SyntaxToken;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxNode {
    pub kind: u16,
    pub flags: u16,
    pub start: u32,
    pub end: u32,
    pub child_start: u32,
    pub child_len: u32,
}

const _: [(); 20] = [(); std::mem::size_of::<SyntaxNode>()];

impl SyntaxNode {
    #[inline]
    pub const fn kind(&self) -> SyntaxKind {
        SyntaxKind::from_raw(self.kind)
    }

    #[inline]
    pub const fn span(&self) -> Span {
        Span::new(self.start, self.end)
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxElement(u32);

const _: [(); 4] = [(); std::mem::size_of::<SyntaxElement>()];

impl SyntaxElement {
    const NODE_TAG: u32 = 1 << 31;
    const INDEX_MASK: u32 = Self::NODE_TAG - 1;

    pub fn try_token(index: u32) -> Result<Self, CssStructureTooLarge> {
        if index > Self::INDEX_MASK {
            return Err(CssStructureTooLarge {
                kind: StructureOverflowKind::TokenIndex,
                attempted: u64::from(index),
            });
        }
        Ok(Self(index))
    }

    pub fn try_node(index: u32) -> Result<Self, CssStructureTooLarge> {
        if index > Self::INDEX_MASK {
            return Err(CssStructureTooLarge {
                kind: StructureOverflowKind::NodeIndex,
                attempted: u64::from(index),
            });
        }
        Ok(Self(Self::NODE_TAG | index))
    }

    #[inline]
    pub const fn is_node(self) -> bool {
        self.0 & Self::NODE_TAG != 0
    }

    #[inline]
    pub const fn index(self) -> u32 {
        self.0 & Self::INDEX_MASK
    }
}

#[derive(Debug, Clone, Copy)]
struct OpenNode {
    kind: SyntaxKind,
    flags: NodeFlags,
    start: u32,
    pending_start: usize,
}

pub struct LosslessCstSink {
    source: CssSource,
    tokens: Vec<SyntaxToken>,
    nodes: Vec<SyntaxNode>,
    elements: Vec<SyntaxElement>,
    pending: Vec<SyntaxElement>,
    open: SmallVec<[OpenNode; 16]>,
    diagnostics: Vec<CssDiagnostic>,
    fingerprint: u64,
}

impl LosslessCstSink {
    pub fn new(source: CssSource) -> Self {
        let source_len = source.text().len();
        Self {
            source,
            tokens: Vec::with_capacity(source_len + 1),
            nodes: Vec::with_capacity(source_len / 2 + 1),
            elements: Vec::with_capacity(source_len.saturating_mul(2).saturating_add(1)),
            pending: Vec::with_capacity(source_len + 1),
            open: SmallVec::new(),
            diagnostics: Vec::new(),
            fingerprint: 0,
        }
    }

    pub fn finish(self) -> Result<LosslessCst, CssStructureTooLarge> {
        if !self.open.is_empty() {
            return Err(CssStructureTooLarge {
                kind: StructureOverflowKind::ChildRange,
                attempted: u64::try_from(self.open.len()).unwrap_or(u64::MAX),
            });
        }
        Ok(LosslessCst {
            source: self.source,
            tokens: self.tokens,
            nodes: self.nodes,
            elements: self.elements,
            diagnostics: self.diagnostics,
            event_fingerprint: self.fingerprint,
        })
    }
}

impl ParseEventSink for LosslessCstSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        self.fingerprint = event.fold_fingerprint(self.fingerprint);
        match event {
            ParseEvent::StartNode { kind, flags, start } => {
                self.open.push(OpenNode {
                    kind,
                    flags,
                    start,
                    pending_start: self.pending.len(),
                });
            }
            ParseEvent::Token(token) => {
                let index = u32::try_from(self.tokens.len()).map_err(|_| CssStructureTooLarge {
                    kind: StructureOverflowKind::TokenIndex,
                    attempted: u64::try_from(self.tokens.len()).unwrap_or(u64::MAX),
                })?;
                self.tokens.push(token);
                self.pending.push(SyntaxElement::try_token(index)?);
            }
            ParseEvent::FinishNode { kind, end } => {
                let open = self.open.pop().ok_or(CssStructureTooLarge {
                    kind: StructureOverflowKind::ChildRange,
                    attempted: 0,
                })?;
                if open.kind != kind || open.pending_start > self.pending.len() {
                    return Err(CssStructureTooLarge {
                        kind: StructureOverflowKind::ChildRange,
                        attempted: u64::try_from(self.pending.len()).unwrap_or(u64::MAX),
                    });
                }
                let child_start =
                    u32::try_from(self.elements.len()).map_err(|_| CssStructureTooLarge {
                        kind: StructureOverflowKind::ElementIndex,
                        attempted: u64::try_from(self.elements.len()).unwrap_or(u64::MAX),
                    })?;
                let child_len =
                    u32::try_from(self.pending.len() - open.pending_start).map_err(|_| {
                        CssStructureTooLarge {
                            kind: StructureOverflowKind::ChildRange,
                            attempted: u64::try_from(self.pending.len() - open.pending_start)
                                .unwrap_or(u64::MAX),
                        }
                    })?;
                self.elements
                    .extend_from_slice(&self.pending[open.pending_start..]);
                self.pending.truncate(open.pending_start);
                let node_index =
                    u32::try_from(self.nodes.len()).map_err(|_| CssStructureTooLarge {
                        kind: StructureOverflowKind::NodeIndex,
                        attempted: u64::try_from(self.nodes.len()).unwrap_or(u64::MAX),
                    })?;
                self.nodes.push(SyntaxNode {
                    kind: kind as u16,
                    flags: open.flags.0,
                    start: open.start,
                    end,
                    child_start,
                    child_len,
                });
                self.pending.push(SyntaxElement::try_node(node_index)?);
            }
            ParseEvent::Diagnostic(diagnostic) => self.diagnostics.push(diagnostic),
        }
        Ok(())
    }
}

pub struct LosslessCst {
    source: CssSource,
    tokens: Vec<SyntaxToken>,
    nodes: Vec<SyntaxNode>,
    elements: Vec<SyntaxElement>,
    diagnostics: Vec<CssDiagnostic>,
    event_fingerprint: u64,
}

impl LosslessCst {
    #[inline]
    pub fn source(&self) -> &CssSource {
        &self.source
    }

    #[inline]
    pub fn tokens(&self) -> &[SyntaxToken] {
        &self.tokens
    }

    #[inline]
    pub fn nodes(&self) -> &[SyntaxNode] {
        &self.nodes
    }

    #[inline]
    pub fn elements(&self) -> &[SyntaxElement] {
        &self.elements
    }

    #[inline]
    pub fn diagnostics(&self) -> &[CssDiagnostic] {
        &self.diagnostics
    }

    #[inline]
    pub const fn event_fingerprint(&self) -> u64 {
        self.event_fingerprint
    }

    pub fn reconstruct(&self) -> String {
        self.source.slice_tokens(self.tokens.iter().copied())
    }

    pub fn children(&self, node: SyntaxNode) -> &[SyntaxElement] {
        let start = usize::try_from(node.child_start).expect("u32 fits usize");
        let len = usize::try_from(node.child_len).expect("u32 fits usize");
        &self.elements[start..start + len]
    }
}

pub fn parse_lossless(
    source: CssSource,
    dialect: CssDialect,
    entry: CssEntryPoint,
    mode: CssParseMode,
) -> Result<LosslessCst, CssParseFailure> {
    let mut sink = LosslessCstSink::new(source.clone());
    parse_with_sink(&source, dialect, entry, mode, &mut sink)?;
    sink.finish().map_err(CssParseFailure::Structure)
}
