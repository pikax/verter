//! Typed vocabulary shared by the two expression-rewrite passes.

use super::super::expr::{BindingTable, ScopeGraph, ScopeId};
use super::{PropReads, ProxyInitMap};

/// One mapped or unmapped edit over the wrapped authored expression.
///
/// Edits are disjoint by construction: a structural rewrite never also carries
/// a leaf edit inside the span it fully overwrites.
pub(super) enum Edit {
    /// Replace a source range with synthesized text plus exact retained-token mappings.
    Overwrite {
        start: u32,
        end: u32,
        text: String,
        mappings: Vec<ReplacementMapping>,
    },
    /// Insert synthesized text immediately before `at`.
    Insert { at: u32, text: String },
    /// Append synthesized text immediately after `at`.
    Append { at: u32, text: String },
    /// Remove a production-elided statement or relocated comment.
    Remove { start: u32, end: u32 },
}

/// One exact authored token retained inside replacement text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReplacementMapping {
    pub(super) generated_start: u32,
    pub(super) generated_end: u32,
    pub(super) source_start: u32,
}

/// An authored token that a replacement may reproduce more than once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MappedToken {
    pub(super) span: oxc_span::Span,
    pub(super) text: String,
}

/// The complete resolution context shared by occurrence collection and planning.
#[derive(Clone, Copy)]
pub(super) struct RewriteResolveCtx<'s> {
    pub(super) bindings: &'s BindingTable,
    pub(super) scopes: &'s ScopeGraph,
    pub(super) outer_scope: ScopeId,
    pub(super) prop_reads: &'s PropReads,
    pub(super) proxy_inits: &'s ProxyInitMap,
    /// Whether TypeScript-only runtime wrappers are valid on authored lvalues.
    pub(super) typescript: bool,
}

/// One binding-bearing decision recorded by the scope-aware collector.
pub(super) enum Occurrence {
    /// A signal or prop read that must be rewritten.
    ReadRewrite {
        span: oxc_span::Span,
        text: String,
        mapped_token: Option<MappedToken>,
    },
    /// A signal reassignment lowered to a setter head and trailing close.
    SignalReassign {
        head_span: oxc_span::Span,
        head_text: String,
        append_at: u32,
        append_text: String,
        mapped_token: MappedToken,
    },
    /// A signal update lowered as one replacement.
    SignalUpdate {
        span: oxc_span::Span,
        text: String,
        mapped_token: MappedToken,
    },
    /// A bindable-prop member mutation wrapped in its setter.
    WrapCall {
        insert_at: u32,
        head_text: String,
        append_at: u32,
        append_text: String,
    },
    /// A production-elided `$inspect.trace(...)` statement.
    DropStatement { span: oxc_span::Span },
    /// A transparent-wrapper comment moved inside a rewritten invocation head.
    RelocatedWrapperComment { span: oxc_span::Span },
}
