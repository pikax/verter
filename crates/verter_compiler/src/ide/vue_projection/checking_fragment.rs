//! Checking-text fragments: synthesized text interleaved with authored spans.
//!
//! Every product that contributes text to the checking module returns
//! fragments, never a flat string, so the composer can keep an exact source
//! correspondence for authored bytes and none for scaffolding.

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUseId, ElementUseId,
};
use crate::ide::vue_projection::script_setup::SourceRange;

/// One piece of checking text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckingPiece {
    /// Scaffolding with no authored preimage.
    Synth(String),
    /// The single mapped occurrence of an admitted expression, emitted
    /// verbatim with its context references qualified: each authored byte
    /// maps to the byte it copies. An expression may appear as `Expression`
    /// at most once per module.
    Expression(AdmittedExpressionId),
    /// A further occurrence of an admitted expression. Same text and the same
    /// correspondence as `Expression`: a diagnostic it repeats lands on the
    /// same authored bytes and collapses into the original's. It records no
    /// observation of its own.
    Replay(AdmittedExpressionId),
    /// Authored bytes outside any admitted expression, emitted verbatim.
    Verbatim(SourceRange),
    /// Generated spelling of an authored name that differs from the
    /// authored bytes (camelized prop key, handler key). Diagnostics map
    /// to `source`; it takes no edits.
    Alias { text: String, source: SourceRange },
}

/// Ordered pieces of one statement or statement part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckingFragment {
    pub pieces: Vec<CheckingPiece>,
}

impl CheckingFragment {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    #[must_use]
    pub fn synth(text: impl Into<String>) -> Self {
        let mut fragment = Self::new();
        fragment.push_synth(text);
        fragment
    }
    pub fn push_synth(&mut self, text: impl Into<String>) -> &mut Self {
        self.pieces.push(CheckingPiece::Synth(text.into()));
        self
    }
    pub fn push_expression(&mut self, id: AdmittedExpressionId) -> &mut Self {
        self.pieces.push(CheckingPiece::Expression(id));
        self
    }
    pub fn push_replay(&mut self, id: AdmittedExpressionId) -> &mut Self {
        self.pieces.push(CheckingPiece::Replay(id));
        self
    }
    pub fn push_verbatim(&mut self, range: SourceRange) -> &mut Self {
        self.pieces.push(CheckingPiece::Verbatim(range));
        self
    }
    pub fn push_alias(&mut self, text: impl Into<String>, source: SourceRange) -> &mut Self {
        self.pieces.push(CheckingPiece::Alias {
            text: text.into(),
            source,
        });
        self
    }
    pub fn append(&mut self, other: CheckingFragment) -> &mut Self {
        self.pieces.extend(other.pieces);
        self
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
    /// The fragment as plain text, each expression replaced by `spelling(id)`.
    /// Test and harness use only; the composer never flattens.
    #[must_use]
    pub fn flatten(
        &self,
        source: &str,
        spelling: &dyn Fn(&AdmittedExpressionId) -> String,
    ) -> String {
        let mut out = String::new();
        for piece in &self.pieces {
            match piece {
                CheckingPiece::Synth(text) | CheckingPiece::Alias { text, .. } => {
                    out.push_str(text);
                }
                CheckingPiece::Expression(id) | CheckingPiece::Replay(id) => {
                    out.push_str(&spelling(id));
                }
                CheckingPiece::Verbatim(range) => {
                    out.push_str(&source[range.start as usize..range.end as usize]);
                }
            }
        }
        out
    }
}

/// Dense id of one template block in a [`super::branches::TemplateBranchProjection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TemplateBlockId(pub u32);

/// Host of an operation: a component use or an element use.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UseHost {
    Component(ComponentUseId),
    Element(ElementUseId),
}

/// Which obligation family a statement checks. Diagnostic attribution reads
/// this; nothing is recovered from generated names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObligationFamily {
    Interpolation,
    Branch,
    Iteration,
    ComponentUse,
    Listener,
    Model,
    ProvidedSlot,
    SlotOutlet,
    NativeElement,
    NativeModel,
    Directive,
    Builtin,
    Fallthrough,
    TemplateRef,
    /// Context view, preludes and block scaffolding.
    Scaffold,
}

/// Where a statement goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementAnchor {
    /// At the position of item `item` of `block`, in item order.
    Item { block: TemplateBlockId, item: u32 },
    /// After the last item of `block`, before it closes.
    BlockTail(TemplateBlockId),
}

/// Order of statements that share one anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatementPhase {
    /// Declarations a construction reads.
    Before = 0,
    /// The construction or the main statement of the item.
    Main = 1,
    /// Checks that read the construction's binding.
    After = 2,
}

/// One statement with its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedStatement {
    pub anchor: StatementAnchor,
    pub phase: StatementPhase,
    pub family: ObligationFamily,
    pub host: Option<UseHost>,
    /// One complete statement, newline-terminated when flattened.
    pub fragment: CheckingFragment,
}

/// A module-scope declaration block emitted once when any product asks for it.
/// `key` orders and deduplicates; `text` is TypeScript declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreludeDeclaration {
    pub key: &'static str,
    pub text: &'static str,
}

/// The opening of a block that a product owns: an optional statement in the
/// parent scope, then the text that opens the block. The composer writes the
/// closing `}`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockHead {
    pub before: CheckingFragment,
    pub open: CheckingFragment,
}
