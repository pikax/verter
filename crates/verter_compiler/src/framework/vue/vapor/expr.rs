//! Record authored and generated expression parts through the shared rewriter.
use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;
use verter_span::{RelativeSpan, Span};

use super::ops::{Part, RunBuf};
use crate::framework::vue::semantic::{BindingAccess, VueBindingView};
use crate::template::binding::prefixed_expr::{
    write_prefixed_expr, write_simple_expr, ExprBindings, ExprSink,
};
use crate::template::oxc::types::{OxcParsedExpression, OxcParsedVFor};
use crate::types::BindingType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Resolution {
    Parsed,
    Simple,
    Verbatim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExprProvenance {
    Authored,
    Generated,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RecordOptions {
    pub(super) force_js: bool,
    pub(super) resolution: Resolution,
    pub(super) provenance: ExprProvenance,
}

impl From<bool> for RecordOptions {
    fn from(force_js: bool) -> Self {
        Self {
            force_js,
            resolution: Resolution::Parsed,
            provenance: ExprProvenance::Authored,
        }
    }
}

pub(super) struct VaporBindings<'a, 'c> {
    pub(super) view: &'a VueBindingView<'c>,
    pub(super) renames: &'a [FxHashMap<String, String>],
}

impl ExprBindings for VaporBindings<'_, '_> {
    fn get(&self, ident: &str) -> Option<BindingType> {
        self.view.get(ident)
    }
    fn resolve_prefix(&self, ident: &str, access: BindingAccess) -> &'static str {
        ExprBindings::resolve_prefix(self.view, ident, access)
    }
    fn resolve_suffix(&self, ident: &str, access: BindingAccess) -> &'static str {
        ExprBindings::resolve_suffix(self.view, ident, access)
    }
    fn resolve_local(&self, name: &str) -> Option<&str> {
        self.renames
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(String::as_str))
    }
}

struct PartSink<'a, 'c> {
    parts: &'a mut RunBuf<'c>,
    allocator: &'c Allocator,
    provenance: ExprProvenance,
}

impl ExprSink for PartSink<'_, '_> {
    fn push_source(&mut self, text: &str, source_start: u32) {
        if text.is_empty() {
            return;
        }
        self.parts.push(match self.provenance {
            ExprProvenance::Authored => {
                Part::Source(Span::new(source_start, source_start + text.len() as u32))
            }
            ExprProvenance::Generated => Part::Text(self.allocator.alloc_str(text)),
        });
    }
    fn push_synthetic(&mut self, text: &str) {
        if !text.is_empty() {
            self.parts.push(Part::Text(self.allocator.alloc_str(text)));
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Syntax<'a, 'c> {
    Parsed(Option<&'a OxcParsedExpression<'c>>),
    LoopSource(Option<&'a OxcParsedVFor<'c>>),
    Parameters(Option<(&'a oxc_ast::ast::FormalParameters<'c>, u32)>),
}

pub(super) fn record_expr<'c>(
    buf: &mut RunBuf<'c>,
    allocator: &'c Allocator,
    text: &str,
    start: u32,
    syntax: Syntax<'_, '_>,
    bindings: &VaporBindings<'_, 'c>,
    options: impl Into<RecordOptions>,
) {
    let options = options.into();
    let oxc = match syntax {
        Syntax::Parsed(parsed) => parsed,
        _ => None,
    };
    let origin = oxc.map_or(0, |oxc| oxc.offset);
    let mut sink = PartSink {
        parts: buf,
        allocator,
        provenance: options.provenance,
    };
    let mut skip = if options.force_js {
        if let Syntax::Parameters(Some((parameters, annotation_offset))) = syntax {
            crate::strip_types::typescript::collect_parameter_ts_removal_spans(
                parameters,
                annotation_offset,
            )
        } else if let Syntax::LoopSource(Some(vfor)) = syntax {
            vfor.parsed
                .right()
                .map(crate::strip_types::typescript::collect_ts_removal_spans)
                .unwrap_or_default()
        } else if let Some(oxc) = oxc {
            if let Some(expression) = &oxc.expression {
                crate::strip_types::typescript::collect_ts_removal_spans(expression)
            } else if let Some(program) = oxc.statements {
                crate::strip_types::typescript::collect_program_ts_removal_spans(program)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    if origin != start {
        let window = Span::new(start, start + text.len() as u32);
        skip.retain_mut(|(range_start, range_end)| {
            let absolute = RelativeSpan::new(*range_start, *range_end).to_absolute(origin);
            let overlap = Span::new(
                absolute.start.max(window.start),
                absolute.end.min(window.end),
            );
            if overlap.is_empty() {
                return false;
            }
            let relative = overlap.to_relative(window.start);
            *range_start = relative.start;
            *range_end = relative.end;
            true
        });
    }
    if options.resolution == Resolution::Verbatim {
        crate::template::binding::prefixed_expr::write_source_expr(&mut sink, text, start, &skip);
    } else if options.resolution == Resolution::Simple && skip.is_empty() {
        write_simple_expr(&mut sink, bindings, text, start);
    } else if let Some(oxc) = oxc {
        write_prefixed_expr(&mut sink, text, start, oxc, bindings, &skip);
    } else if let Syntax::LoopSource(Some(vfor)) = syntax {
        crate::template::binding::prefixed_expr::write_prefixed_bindings(
            &mut sink,
            text,
            start,
            vfor.source_bindings
                .as_ref()
                .map_or(&[], |inventory| inventory.bindings.as_slice()),
            bindings,
            &skip,
        );
    } else if !skip.is_empty() {
        crate::template::binding::prefixed_expr::write_prefixed_bindings(
            &mut sink,
            text,
            start,
            &[],
            bindings,
            &skip,
        );
    } else {
        write_simple_expr(&mut sink, bindings, text, start);
    }
}
