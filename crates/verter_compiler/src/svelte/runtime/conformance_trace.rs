//! The OPT-IN conformance-observability side channel (the `conformance-trace`
//! Cargo feature) — TYPED observations the Svelte conformance corpus tooling
//! consumes after lowering a fixture, WITHOUT rescanning source.
//!
//! Two observation halves:
//!
//! 1. **Static-attribute provenance** — the LEXICAL facts the semantic IR
//!    deliberately erases: how each [`AttrIr::Static`] value was QUOTED
//!    (quoted / unquoted / boolean-valueless) and which HTML entity SOURCE
//!    REPRESENTATION spelled it (literal / named / decimal / hex / mixed
//!    forms). Captured at the attribute-lowering PRODUCER BOUNDARY:
//!    quoting from the delimiter byte immediately before the value span (by
//!    tokenizer construction a QUOTED value's body span starts exactly one
//!    byte past its `"`/`'` delimiter, and an UNQUOTED value is preceded by
//!    `=` or whitespace — never a quote byte — so the delimiter byte fully
//!    determines the quoting), and the representation from the reference
//!    forms the producer's SINGLE decode pass EMITS while it produces the
//!    semantic value (the `DecodedAttrValue::decode` observer) — this module
//!    only FOLDS those emitted facts; it never runs a second scan over the
//!    raw value bytes.
//!
//! 2. **Matcher facts** — the CSS selector-to-template matcher's tri-state
//!    [`MatchCertainty`] per top-level complex selector (`No` rows included),
//!    the used/scoped selector spans, and the scoped element identities: the
//!    complete fact set the metamorphic executor compares across
//!    representation variants.
//!
//! Delivery is a THREAD-LOCAL collector: [`capture`] installs a fresh trace,
//! runs the closure, and returns whatever the feature-gated producer hooks
//! recorded; [`compile_client_with_conformance_trace`] wraps the production
//! [`compile_client`] entry in a capture. Recording outside a capture is
//! inert. The module (and every hook referencing it) exists ONLY under the
//! feature — the DEFAULT build carries no trace state, no extra fields on
//! production IR, and no runtime cost; an ungated production reference
//! cannot compile.
//!
//! [`AttrIr::Static`]: super::ir::AttrIr::Static

use std::cell::RefCell;

use oxc_allocator::Allocator;
use verter_span::Span;

use super::client::ClientModule;
use super::entity_decode::EntityRefForm;
use super::{compile_client, ClientCompileError, SvelteRuntimeOptions};
use crate::svelte::parser::ParsedSvelte;

pub use super::css::matcher::MatchCertainty;

/// How a static attribute's value was QUOTED in the source — lexical
/// provenance the decoded IR value erases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrQuoting {
    /// A `"…"` or `'…'` quoted value.
    Quoted,
    /// An unquoted value (`class=a&#32;b`).
    Unquoted,
    /// A valueless boolean-form attribute (`disabled`).
    BooleanValueless,
}

/// Which HTML entity SOURCE REPRESENTATION spelled a static attribute value.
/// Folded from the reference forms the producer boundary's SINGLE decode
/// pass emitted (one report per DECODED reference) — an undecodable
/// reference (`&bogus;`, the uppercase-`X` `&#X41;`) reports nothing and
/// stays literal text, exactly as the decode keeps it literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrSourceRepresentation {
    /// No decodable entity reference — plain literal text.
    Literal,
    /// Named reference(s) only (`&amp;` / the legacy no-`;` `&amp`).
    HtmlNamedEntity,
    /// Decimal reference(s) only (`&#32;`).
    HtmlDecimalEntity,
    /// Hex reference(s) only (`&#x20;` — the official pattern accepts a
    /// lowercase `x` only).
    HtmlHexEntity,
    /// At least two DISTINCT reference forms (`a&amp;&#65;b`). Literal text
    /// AROUND a single form does NOT make it mixed (`a&#32;b` is Decimal).
    Mixed,
}

/// The provenance of one lowered [`AttrIr::Static`] attribute, in encounter
/// (lowering) order.
///
/// [`AttrIr::Static`]: super::ir::AttrIr::Static
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrProvenance {
    /// The attribute name, verbatim.
    pub name: String,
    /// How the value was quoted.
    pub quoting: AttrQuoting,
    /// The entity source representation of the raw value text — `None` for a
    /// valueless boolean attribute (there is no value text).
    pub representation: Option<AttrSourceRepresentation>,
}

/// One top-level complex selector's tri-state verdict (prune visit order —
/// the ordinal position is the variant-stable identity; the span anchors the
/// selector in THIS variant's source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorCertaintyFact {
    /// The complex selector's source span.
    pub selector_span: Span,
    /// The matcher's certainty for this selector (OR-fold over the template
    /// elements; `No` = provably unused).
    pub certainty: MatchCertainty,
}

/// One scoped template element's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedElementFact {
    /// The runtime-IR node id (the IR arena ordinal — stable across
    /// representation variants of the same template structure).
    pub node: u32,
    /// The element tag (`div`; the literal `svelte:element` for a dynamic
    /// element).
    pub tag: String,
    /// The element's open-tag source span.
    pub span: Span,
}

/// The complete matcher-fact set of one `<style>` match run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleMatchTrace {
    /// Per TOP-LEVEL complex selector (prune visit order, `No` rows
    /// included): the tri-state certainty behind the production used/unused
    /// projection.
    pub selector_certainties: Vec<SelectorCertaintyFact>,
    /// The `used = true` selector spans (sorted; synthetic spans excluded).
    pub used_selector_spans: Vec<Span>,
    /// The `scoped = true` relative-selector spans (sorted; synthetic spans
    /// excluded).
    pub scoped_selector_spans: Vec<Span>,
    /// The scoped element identities (sorted by node id).
    pub scoped_elements: Vec<ScopedElementFact>,
}

/// The typed observations of one [`capture`] run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConformanceTrace {
    /// Every lowered static attribute's provenance, in encounter order.
    pub static_attrs: Vec<AttrProvenance>,
    /// One entry per `<style>` matcher run.
    pub style_matches: Vec<StyleMatchTrace>,
}

thread_local! {
    /// The active trace of the innermost [`capture`] on this thread — `None`
    /// outside a capture (recording is then inert).
    static ACTIVE: RefCell<Option<ConformanceTrace>> = const { RefCell::new(None) };
}

/// Run `f` with a fresh thread-local trace installed and return `f`'s result
/// together with everything the producer hooks recorded.
///
/// Captures NEST: an inner capture temporarily displaces the outer trace and
/// restores it afterwards (records go to the innermost capture only); the
/// restore also runs on unwind, so a panicking closure cannot leak an active
/// trace into later work on the thread.
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, ConformanceTrace) {
    /// Restores the displaced outer trace on scope exit (unwind included).
    struct RestoreOnDrop {
        prev: Option<Option<ConformanceTrace>>,
    }
    impl Drop for RestoreOnDrop {
        fn drop(&mut self) {
            if let Some(prev) = self.prev.take() {
                ACTIVE.with(|active| *active.borrow_mut() = prev);
            }
        }
    }

    let prev = ACTIVE.with(|active| {
        let mut slot = active.borrow_mut();
        let prev = slot.take();
        *slot = Some(ConformanceTrace::default());
        prev
    });
    let guard = RestoreOnDrop { prev: Some(prev) };
    let result = f();
    let trace = ACTIVE
        .with(|active| active.borrow_mut().take())
        .unwrap_or_default();
    drop(guard);
    (result, trace)
}

/// Invoke `f` on the active trace — a no-op (the closure never runs, so no
/// observation is even CONSTRUCTED) outside a capture.
pub(super) fn record(f: impl FnOnce(&mut ConformanceTrace)) {
    ACTIVE.with(|active| {
        if let Some(trace) = active.borrow_mut().as_mut() {
            f(trace);
        }
    });
}

/// The reference forms ONE producer decode pass reported — the accumulator
/// the attribute-lowering boundary folds while `DecodedAttrValue::decode`
/// performs its single scan. The fold is order-insensitive presence (which
/// DISTINCT forms appeared), exactly what [`AttrSourceRepresentation`]
/// classifies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct SeenEntityForms {
    named: bool,
    decimal: bool,
    hex: bool,
}

impl SeenEntityForms {
    /// Fold one decoded reference's spelled form (the producer's observer
    /// callback body).
    pub(super) fn observe(&mut self, form: EntityRefForm) {
        match form {
            EntityRefForm::Named => self.named = true,
            EntityRefForm::Decimal => self.decimal = true,
            EntityRefForm::Hex => self.hex = true,
        }
    }

    /// Classify the whole value from the folded forms: no decoded reference
    /// is literal text; exactly one distinct form names it; two or more
    /// distinct forms are mixed (literal text AROUND a single form does NOT
    /// make it mixed — `a&#32;b` is Decimal).
    fn representation(self) -> AttrSourceRepresentation {
        match (self.named, self.decimal, self.hex) {
            (false, false, false) => AttrSourceRepresentation::Literal,
            (true, false, false) => AttrSourceRepresentation::HtmlNamedEntity,
            (false, true, false) => AttrSourceRepresentation::HtmlDecimalEntity,
            (false, false, true) => AttrSourceRepresentation::HtmlHexEntity,
            _ => AttrSourceRepresentation::Mixed,
        }
    }
}

/// Record one VALUED static attribute's provenance — called by the attribute
/// lowering at the [`AttrIr::Static`] producer boundary with the RAW value
/// span and the [`SeenEntityForms`] its single decode pass emitted. The only
/// source read is the ONE delimiter byte immediately before the value span
/// (a parse fact the decode never touches); the representation is a pure
/// fold of the producer-emitted forms — never a re-scan of the value bytes.
///
/// [`AttrIr::Static`]: super::ir::AttrIr::Static
pub(super) fn record_static_attr(
    source: &str,
    name: &str,
    value_span: Span,
    forms: SeenEntityForms,
) {
    record(|trace| {
        trace.static_attrs.push(AttrProvenance {
            name: name.to_string(),
            quoting: quoting_of(source, value_span),
            representation: Some(forms.representation()),
        });
    });
}

/// Record a VALUELESS boolean-form attribute's provenance (`disabled`) —
/// there is no value text, so no quoting delimiter and no representation.
pub(super) fn record_boolean_attr(name: &str) {
    record(|trace| {
        trace.static_attrs.push(AttrProvenance {
            name: name.to_string(),
            quoting: AttrQuoting::BooleanValueless,
            representation: None,
        });
    });
}

/// The quoting of a value span, from its delimiter byte: by tokenizer
/// construction a QUOTED body span starts exactly one byte past its `"`/`'`
/// delimiter, and an UNQUOTED value is preceded by `=` or whitespace (never
/// a quote byte).
fn quoting_of(source: &str, value_span: Span) -> AttrQuoting {
    let preceding = (value_span.start as usize)
        .checked_sub(1)
        .and_then(|p| source.as_bytes().get(p));
    match preceding {
        Some(b'"' | b'\'') => AttrQuoting::Quoted,
        _ => AttrQuoting::Unquoted,
    }
}

/// The conformance-crate compile entry: run the production [`compile_client`]
/// pipeline (client backend, no SSR) under a [`capture`] and return the
/// compile outcome TOGETHER with the trace — a refused/rejected fixture still
/// returns whatever was observed up to the failure (the reject-unclassified
/// gate classifies refusals from typed observations too).
pub fn compile_client_with_conformance_trace<'a>(
    source: &'a str,
    parsed: &ParsedSvelte,
    opts: &SvelteRuntimeOptions,
    alloc: &'a Allocator,
    want_source_map: bool,
) -> (Result<ClientModule, ClientCompileError>, ConformanceTrace) {
    capture(|| compile_client(source, parsed, opts, alloc, false, want_source_map))
}

#[cfg(test)]
#[path = "conformance_trace_tests.rs"]
mod tests;
