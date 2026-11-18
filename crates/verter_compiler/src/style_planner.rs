//! Typed framework style rewrite stages over the shared style syntax IR.

use crate::code_transform::{CodeTransform, CodeTransformError, SourceMapOptions};
use crate::framework::vue::style::cascade::{intern_edit_content, PreparedEdit, StyleEdit};
use crate::framework_common::{RuntimeOutputDescriptor, SourceMapFidelity};
use oxc_allocator::Allocator;
use oxc_sourcemap::SourceMap;
use std::sync::Arc;
use verter_css_syntax::{
    parse_style_ir, CssDialect, CssParseMode, CssSource, QualifiedStyleResult, StyleSyntaxIr,
};
use verter_span::Span;

/// The witness a caller-preprocessed style block enters the compiler through,
/// and the producer vocabulary the witness carries.
///
/// Re-exported so the admitting host names the compiler's own entry vocabulary
/// instead of taking a direct dependency on the syntax crate for these types.
pub use verter_css_syntax::{
    ExternalStyleProducer, PreprocessedStyle, PreprocessorIdentity, StyleDiagnostic,
    StyleDiagnosticSeverity, StyleProducer, StyleStage,
};

pub use crate::framework::vue::style::cascade::{
    cascade_output_is_publishable, cascade_requested_source_map, run_vue_style_authored_only,
    run_vue_style_cascade, run_vue_style_cascade_verified, transform_vue_style, CascadeInput,
    StyleRewriteOutcome, VueStyleCascadeOutcome, VueStyleFacts, VueStyleRewriteMask,
};
pub use crate::framework::vue::style::css_vars::{
    generate_var_name, transform_vue_v_bind, v_bind_vars_from_parsed_ir, VBindVar,
};
#[cfg(test)]
pub(crate) use crate::framework::vue::style::modules::transform_vue_css_modules;
pub use crate::framework::vue::style::modules::{
    analyze_css_module_classes, analyze_style, complete_static_class_names, StyleAnalysis,
};
#[cfg(test)]
pub(crate) use crate::framework::vue::style::scoped::transform_vue_scoped_css;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleRewriteStage {
    AuthoredVBind,
    PostPreprocessModules,
    PostPreprocessScoping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleRewriteFailureClass {
    StageRequiresPlainCss,
    ParseFailure,
    UntrustedRewriteTarget,
    OverlappingEdits,
    IndentedLayoutMutation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRewriteFailure {
    pub class: StyleRewriteFailureClass,
    pub stage: StyleRewriteStage,
    pub dialect: CssDialect,
    pub span: Option<Span>,
}

impl StyleRewriteFailure {
    pub(crate) fn new(
        class: StyleRewriteFailureClass,
        stage: StyleRewriteStage,
        dialect: CssDialect,
        span: Option<Span>,
    ) -> Self {
        Self {
            class,
            stage,
            dialect,
            span,
        }
    }

    /// Project the refusal into the shared style-diagnostic vocabulary.
    ///
    /// `space` is the cascade input stage the refused span addresses. Shared
    /// planning hands every compatible stage the same input IR, so a later
    /// stage's refusal is still in that input space — never a rewritten-byte
    /// coordinate space. Carrying it is what lets a consumer decide which map
    /// a reported span needs.
    ///
    /// Crate-private on purpose. A failure is recorded in TWO shapes — as
    /// itself, on the facts and stage-failure lists, and as a diagnostic on
    /// the result carrier — and only one of them is the publication route.
    /// Keeping the projection private is what makes the carrier's
    /// `diagnostics()` the obvious way to obtain a diagnostic rather than one
    /// of two.
    ///
    /// Read what the privacy buys, exactly. It removes THIS projection, with
    /// its `space` argument, from the outside vocabulary — so no consumer can
    /// accidentally re-derive a correctly-spaced diagnostic and publish the
    /// same refusal twice. It is not a structural bar: `StyleDiagnostic::new`
    /// is public, `Display` is public, and `refusals`/`stage_failures` stay
    /// `pub` on the outcome, so a consumer that sets out to mint its own
    /// shape can. The bar against double-reporting is the convention this
    /// doc states plus the fact that the private helper is where the correct
    /// answer lives; anything stronger would need the failure lists
    /// themselves to stop being public.
    #[must_use]
    pub(crate) fn to_diagnostic(&self, space: StyleStage) -> StyleDiagnostic {
        StyleDiagnostic::new(space, self.to_string(), self.span)
    }
}

impl std::fmt::Display for StyleRewriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "style rewrite {:?} refused {:?} input with {:?} at {:?}",
            self.stage, self.dialect, self.class, self.span
        )
    }
}

impl std::error::Error for StyleRewriteFailure {}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StyleSourceIdentity<'a> {
    pub(crate) source_name: &'a str,
    pub(crate) source_space_token: &'a str,
    pub(crate) content_artifact_token: &'a str,
}

#[derive(Clone, Copy)]
pub struct AuthoredStyleInput<'a> {
    pub(crate) code: &'a str,
    pub(crate) dialect: CssDialect,
    pub(crate) source: StyleSourceIdentity<'a>,
    pub(crate) prepared: Option<&'a StyleSyntaxIr>,
    pub(crate) want_source_map: bool,
}

impl<'a> AuthoredStyleInput<'a> {
    #[must_use]
    pub const fn new(
        code: &'a str,
        dialect: CssDialect,
        source_name: &'a str,
        source_space_token: &'a str,
        content_artifact_token: &'a str,
    ) -> Self {
        Self {
            code,
            dialect,
            source: StyleSourceIdentity {
                source_name,
                source_space_token,
                content_artifact_token,
            },
            prepared: None,
            want_source_map: true,
        }
    }

    #[must_use]
    pub const fn with_prepared(mut self, ir: &'a StyleSyntaxIr) -> Self {
        self.prepared = Some(ir);
        self
    }

    /// Isolated rewrite stages skip `CodeTransform::generate_map` and descriptor
    /// hashing when the caller does not need a source map.
    #[must_use]
    pub const fn without_source_map(mut self) -> Self {
        self.want_source_map = false;
        self
    }

    /// Authored input whose dialect is native CSS — for fact-only callers
    /// that do not name `CssDialect` themselves.
    #[must_use]
    pub const fn new_css(
        code: &'a str,
        source_name: &'a str,
        source_space_token: &'a str,
        content_artifact_token: &'a str,
    ) -> Self {
        Self::new(
            code,
            CssDialect::Css,
            source_name,
            source_space_token,
            content_artifact_token,
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlainCssInput<'a> {
    pub(crate) code: &'a str,
    /// Threaded into the crate-test isolated materialize entries. Production
    /// uses this type only as the plain-CSS gate (`try_new`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) source: StyleSourceIdentity<'a>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) want_source_map: bool,
}

impl<'a> PlainCssInput<'a> {
    pub fn try_new(
        code: &'a str,
        dialect: CssDialect,
        source_name: &'a str,
        source_space_token: &'a str,
        content_artifact_token: &'a str,
    ) -> Result<Self, StyleRewriteFailure> {
        if dialect.requires_external_preprocessing() {
            return Err(StyleRewriteFailure::new(
                StyleRewriteFailureClass::StageRequiresPlainCss,
                StyleRewriteStage::PostPreprocessScoping,
                dialect,
                None,
            ));
        }
        Ok(Self {
            code,
            source: StyleSourceIdentity {
                source_name,
                source_space_token,
                content_artifact_token,
            },
            want_source_map: true,
        })
    }

    #[must_use]
    pub const fn code(&self) -> &'a str {
        self.code
    }

    /// Isolated rewrite stages skip `CodeTransform::generate_map` and descriptor
    /// hashing when the caller does not need a source map.
    #[must_use]
    pub const fn without_source_map(mut self) -> Self {
        self.want_source_map = false;
        self
    }
}

/// Host-retained parsed style IR, threaded into later compile/analysis.
///
/// Companion carrier — not a field on public `StyleBlockAnalysis`. Clone is
/// Arc-cheap. Excluded from request/cache identity.
#[derive(Clone, verter_retention::heap_size::HeapSize)]
pub struct PreparedStyleIr {
    #[heap_size(with = "style_ir_heap")]
    ir: Arc<StyleSyntaxIr>,
}

/// The framework-neutral CSS crate describes its parse's storage without
/// depending on retention; it is measured here, under the shared-allocation
/// rule: each shared block once, the source text as its own allocation.
fn style_ir_heap<'t>(
    ir: &'t Arc<StyleSyntaxIr>,
    meter: &mut verter_retention::heap_size::HeapMeter<'_, 't>,
) -> usize {
    use verter_retention::heap_size::HeapSize;
    meter.shared_arc(ir, |ir, meter| {
        meter.shared_arc(ir.storage(), |storage, meter| {
            storage.source_text().heap_size(meter)
                + storage
                    .exclusive_allocations()
                    .into_iter()
                    .map(verter_retention::allocation::heap_block_bytes)
                    .sum::<usize>()
        })
    })
}

impl PreparedStyleIr {
    #[must_use]
    pub fn new(ir: StyleSyntaxIr) -> Self {
        Self { ir: Arc::new(ir) }
    }

    #[must_use]
    pub fn from_arc(ir: Arc<StyleSyntaxIr>) -> Self {
        Self { ir }
    }

    #[must_use]
    pub fn ir(&self) -> &StyleSyntaxIr {
        &self.ir
    }

    #[must_use]
    pub fn clone_arc(&self) -> Arc<StyleSyntaxIr> {
        Arc::clone(&self.ir)
    }
}

impl std::fmt::Debug for PreparedStyleIr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedStyleIr")
            .field("dialect", &self.ir.dialect())
            .field("origin", &self.ir.source().origin())
            .finish_non_exhaustive()
    }
}

/// Bind a host-retained prepared IR to one style inventory slot.
///
/// Join is the sealed slot only: `slot_parsed` (host-resolved block content)
/// or `prepared_styles[index]`. Identical source bytes in another slot are
/// not a match. A present slot whose IR bytes do not equal `code` is a
/// mismatch and yields `None`.
#[must_use]
pub fn prepared_style_for_sealed_slot<'a>(
    slot_parsed: Option<&'a PreparedStyleIr>,
    prepared_styles: &'a [Option<PreparedStyleIr>],
    index: usize,
    code: &str,
) -> Option<&'a PreparedStyleIr> {
    let prepared = slot_parsed.or_else(|| prepared_styles.get(index).and_then(Option::as_ref))?;
    (prepared.ir().source().text() == code).then_some(prepared)
}

/// Proof that these bytes came from a `StyleSyntaxIr` parse tagged with the
/// native CSS dialect.
///
/// This witness establishes provenance only. The parser runs in recovery
/// mode, so it does not claim that the bytes contain only valid plain CSS.
pub struct VerifiedPlainCss<'a> {
    pub(crate) ir: &'a StyleSyntaxIr,
}

impl<'a> VerifiedPlainCss<'a> {
    /// Mints a witness only from an already-parsed native-CSS syntax IR.
    #[must_use]
    pub fn from_parsed_native_css(ir: &'a StyleSyntaxIr) -> Option<Self> {
        (ir.dialect() == CssDialect::Css).then_some(Self { ir })
    }

    /// Returns the exact bytes carried by the verified parse.
    #[must_use]
    pub fn code(&self) -> &'a str {
        self.ir.source().text()
    }
}

thread_local! {
    /// Counts every `parse_style_ir` invocation reached through this module's
    /// shared `parse_ir` wrapper. Production Vue style transforms
    /// (`transform_vue_v_bind` / `run_vue_style_cascade`) route through
    /// `parse_ir`, so this count is the authoritative, directly-observable
    /// proof that an `Unchanged` stage hands its parsed `StyleSyntaxIr`
    /// forward instead of re-parsing (the one-parse-per-content-identity
    /// invariant). THREAD-LOCAL, not
    /// a process-global static: the Rust test harness runs each `#[test]` on its
    /// own thread, and every counted call this module makes stays on the calling
    /// test's thread (no internal spawning) — a thread-local counter is exactly
    /// isolated per-test regardless of how many OTHER tests run concurrently in
    /// the same process, where a shared process-global counter would be
    /// contaminated by them. Always compiled (not `#[cfg(test)]`-gated) because
    /// the observing test lives in a separate integration-test binary that links
    /// the crate's normal (non-test-cfg) build.
    static PARSE_IR_INVOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LAST_PARSE_IR_DIALECT: std::cell::Cell<Option<CssDialect>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
thread_local! {
    static NEXT_STYLE_IR_IDENTITY: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
    static STYLE_IR_STAGE_OBSERVATIONS: std::cell::RefCell<Vec<(StyleRewriteStage, usize)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub(crate) struct ParsedStyleIr {
    inner: StyleSyntaxIr,
    #[cfg(test)]
    identity: usize,
}

impl ParsedStyleIr {
    pub(crate) fn from_existing(inner: StyleSyntaxIr) -> Self {
        Self {
            inner,
            #[cfg(test)]
            identity: NEXT_STYLE_IR_IDENTITY.with(|identity| {
                let current = identity.get();
                identity.set(current + 1);
                current
            }),
        }
    }
}

impl std::ops::Deref for ParsedStyleIr {
    type Target = StyleSyntaxIr;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[cfg(test)]
pub(crate) fn observe_style_ir(stage: StyleRewriteStage, ir: &ParsedStyleIr) {
    STYLE_IR_STAGE_OBSERVATIONS.with(|observations| {
        observations.borrow_mut().push((stage, ir.identity));
    });
}

#[cfg(not(test))]
pub(crate) fn observe_style_ir(_stage: StyleRewriteStage, _ir: &ParsedStyleIr) {}

/// Record what a parse of the cascade's INPUT observed about its inclusions.
///
/// Every entry point that parses those bytes calls this, so the published
/// answer does not depend on which entry point the same source was routed
/// through — the divergence a consumer would otherwise inherit as a
/// route-dependent yes/no about whether a block declares its whole surface.
///
/// It records at most once per cascade, and only from a parse of the input
/// bytes: shared planning never re-parses rewritten output, so a later
/// observation must not overwrite the input parse's answer with a different
/// space. A cascade that never reaches this recorder leaves the answer
/// unrecorded, and [`VueStyleFacts::pulls_in_unparsed_bytes`] fails closed on
/// it.
pub(crate) fn record_input_dependencies(facts: &mut VueStyleFacts, ir: &ParsedStyleIr) {
    if facts.input_pulls_in_unparsed_bytes.is_some() {
        return;
    }
    // Asked of the parse as a whole, never folded over the inclusion list it
    // recorded. A recovered parse hands back a list that is a lower bound —
    // an inclusion inside the range it skipped never reached the at-rule
    // frame — so a fold over what it did record answers "nothing foreign
    // here" for exactly the sheets where it saw least.
    facts.input_pulls_in_unparsed_bytes = Some(ir.pulls_in_unparsed_bytes());
}

#[cfg(test)]
pub(crate) fn reset_style_ir_stage_observations() {
    NEXT_STYLE_IR_IDENTITY.with(|identity| identity.set(1));
    STYLE_IR_STAGE_OBSERVATIONS.with(|observations| observations.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn style_ir_stage_observations() -> Vec<(StyleRewriteStage, usize)> {
    STYLE_IR_STAGE_OBSERVATIONS.with(|observations| observations.borrow().clone())
}

/// Current `parse_ir` invocation count on the calling thread. Test-only
/// observability hook.
#[must_use]
pub fn parse_ir_invocation_count() -> usize {
    PARSE_IR_INVOCATIONS.with(std::cell::Cell::get)
}

/// Resets the `parse_ir` invocation counter on the calling thread. Test-only
/// observability hook.
pub fn reset_parse_ir_invocation_count() {
    PARSE_IR_INVOCATIONS.with(|count| count.set(0));
}

/// Last dialect passed through the shared style-IR parse entry on this thread.
#[must_use]
pub fn last_parse_ir_dialect() -> Option<CssDialect> {
    LAST_PARSE_IR_DIALECT.with(std::cell::Cell::get)
}

/// Clears the calling thread's last observed style-IR dialect.
pub fn reset_last_parse_ir_dialect() {
    LAST_PARSE_IR_DIALECT.with(|dialect| dialect.set(None));
}

/// Current `build_string` invocation count on the calling thread. Test-only
/// observability hook.
#[cfg(test)]
#[must_use]
pub fn build_string_invocation_count() -> usize {
    crate::code_transform::code_transform_build_string_call_count()
}

/// Resets the `build_string` invocation counter on the calling thread.
/// Test-only observability hook.
#[cfg(test)]
pub fn reset_build_string_invocation_count() {
    crate::code_transform::reset_code_transform_build_string_call_count();
}

/// Parses plain CSS through this module's shared, COUNTED parse funnel and
/// returns the IR a [`VerifiedPlainCss`] witness can be minted from.
///
/// Callers outside this module must obtain their IR here rather than reaching
/// for the CSS grammar directly. The witness only requires a parsed IR, so a
/// direct parse satisfies the type while leaving the parse INVISIBLE to
/// `parse_ir_invocation_count` — and that counter is the observable proof that
/// a style block is parsed once per content identity. Bypassing the funnel does
/// not break the type-state; it blinds the instrument that watches it, which is
/// the harder defect to notice because every test still passes.
///
/// The IR is returned by value because the witness borrows it: the caller owns
/// the IR for as long as it holds the witness.
pub fn parse_plain_css_for_verification(
    code: &str,
    stage: StyleRewriteStage,
) -> Result<StyleSyntaxIr, StyleRewriteFailure> {
    parse_ir(code, CssDialect::Css, stage).map(|parsed| parsed.inner)
}

/// Parse supplied preprocessor output once at host admission.
///
/// Takes the [`PreprocessedStyle`] witness rather than bare bytes. A caller
/// holding a `&str` cannot state which byte space it is in or who produced
/// it, and this stage runs a plain-CSS grammar over its input, so admitting
/// one here would re-open the route this boundary exists to close: authored
/// SCSS entering the compiler as anonymous "CSS".
///
/// The signature is what closes it, and closes exactly it: an authored or
/// framework-rewritten value cannot be spelled here, and neither can bytes
/// whose producer nobody named. It is not a proof that the bytes really left
/// SCSS behind — plain CSS is a subset of every dialect this compiler parses,
/// so no grammar check could be. That assertion is the admitting boundary's,
/// made once, with the tool's identity attached.
#[must_use]
pub fn prepare_supplied_style(style: PreprocessedStyle<'_>) -> Option<PreparedStyleIr> {
    parse_plain_css_for_verification(style.code(), StyleRewriteStage::PostPreprocessModules)
        .ok()
        .map(PreparedStyleIr::new)
}

pub(crate) fn authored_or_parsed_ir(
    input: AuthoredStyleInput<'_>,
    stage: StyleRewriteStage,
) -> Result<ParsedStyleIr, StyleRewriteFailure> {
    if let Some(prepared) = input.prepared {
        return Ok(ParsedStyleIr::from_existing(prepared.clone()));
    }
    parse_ir(input.code, input.dialect, stage)
}

pub(crate) fn parse_ir(
    code: &str,
    dialect: CssDialect,
    stage: StyleRewriteStage,
) -> Result<ParsedStyleIr, StyleRewriteFailure> {
    PARSE_IR_INVOCATIONS.with(|count| count.set(count.get() + 1));
    LAST_PARSE_IR_DIALECT.with(|observed| observed.set(Some(dialect)));
    let source = CssSource::new(Arc::from(code), 0).map_err(|_| {
        StyleRewriteFailure::new(StyleRewriteFailureClass::ParseFailure, stage, dialect, None)
    })?;
    let inner = parse_style_ir(source, dialect, CssParseMode::Recover).map_err(|_| {
        StyleRewriteFailure::new(StyleRewriteFailureClass::ParseFailure, stage, dialect, None)
    })?;
    Ok(ParsedStyleIr {
        inner,
        #[cfg(test)]
        identity: NEXT_STYLE_IR_IDENTITY.with(|identity| {
            let current = identity.get();
            identity.set(current + 1);
            current
        }),
    })
}

pub(crate) fn build_transform_output(
    code: &str,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    mut edits: Vec<StyleEdit>,
    source_name: Option<&str>,
    want_source_map: bool,
) -> Result<Option<(String, Option<SourceMap<'static>>)>, StyleRewriteFailure> {
    verter_audit::specialise!(crate::code_transform::installed_capture_mode() => M, {
    if edits.is_empty() {
        return Ok(None);
    }
    edits.sort_by_key(|edit| (edit.start(), edit.end()));
    let mut previous_end = 0;
    for edit in &edits {
        if edit.start() < previous_end {
            return Err(StyleRewriteFailure::new(
                StyleRewriteFailureClass::OverlappingEdits,
                stage,
                dialect,
                Some(Span::new(edit.start(), edit.end())),
            ));
        }
        previous_end = previous_end.max(edit.end());
    }

    let allocator = Allocator::new();
    let mut transform = CodeTransform::<M>::new_mapped_in(code, &allocator);
    let mut interned = Vec::new();
    let mut prepared = Vec::with_capacity(edits.len());
    let mut overwrite_count = 0usize;
    let mut insert_count = 0usize;
    for edit in edits {
        match edit {
            StyleEdit::Overwrite { span, content } => {
                overwrite_count += 1;
                prepared.push(PreparedEdit::Overwrite {
                    start: span.start,
                    end: span.end,
                    content: intern_edit_content(&allocator, &mut interned, &content),
                });
            }
            StyleEdit::Insert { at, content } => {
                insert_count += 1;
                prepared.push(PreparedEdit::Insert {
                    at,
                    content: intern_edit_content(&allocator, &mut interned, &content),
                });
            }
        }
    }
    if overwrite_count == prepared.len() && overwrite_count >= 2 {
        let ops: Vec<(u32, u32, &str)> = prepared
            .iter()
            .map(|edit| match *edit {
                PreparedEdit::Overwrite {
                    start,
                    end,
                    content,
                } => (start, end, content),
                PreparedEdit::Insert { .. } => unreachable!("overwrite-only partition"),
            })
            .collect();
        transform.try_batch_overwrite(&ops).map_err(|error| {
            let class = match error {
                CodeTransformError::Conflict { .. } => StyleRewriteFailureClass::OverlappingEdits,
                _ => StyleRewriteFailureClass::UntrustedRewriteTarget,
            };
            let offset = error.offset();
            StyleRewriteFailure::new(class, stage, dialect, Some(Span::new(offset, offset)))
        })?;
    } else if insert_count == prepared.len() && insert_count >= 2 {
        let ops: Vec<(u32, &str)> = prepared
            .iter()
            .map(|edit| match *edit {
                PreparedEdit::Insert { at, content } => (at, content),
                PreparedEdit::Overwrite { .. } => unreachable!("insert-only partition"),
            })
            .collect();
        transform.batch_prepend_left_static(&ops);
    } else {
        for edit in prepared {
            match edit {
                PreparedEdit::Overwrite {
                    start,
                    end,
                    content,
                } => {
                    transform.overwrite(start, end, content);
                }
                PreparedEdit::Insert { at, content } => {
                    transform.prepend_left(at, content);
                }
            }
        }
    }
    let output = transform.build_string();
    let source_map = want_source_map
        .then_some(source_name)
        .flatten()
        .map(|source_name| {
            transform.generate_map(SourceMapOptions::new().with_source(source_name))
        });
    Ok(Some((output, source_map)))

    })
}

/// `want_source_map` mirrors the caller's `RuntimeCompileOptions::source_map`
/// intent. `CodeTransform::generate_map` + `to_json_string()` are not
/// free — a caller that does not want a source map must not pay for building
/// and stringifying one. When `false`, `emit` skips that machinery and
/// returns an empty `source_map` / `raw_map: None`, matching the
/// `Unchanged`/no-map shape callers already handle.
pub(crate) fn emit(
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    edits: Vec<StyleEdit>,
    facts: VueStyleFacts,
    want_source_map: bool,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let Some((output, source_map)) = build_transform_output(
        code,
        dialect,
        stage,
        edits,
        want_source_map.then_some(source.source_name),
        want_source_map,
    )?
    else {
        return Ok(StyleRewriteOutcome::Unchanged { facts });
    };
    let source_map = source_map
        .map(|map| map.to_json_string())
        .unwrap_or_default();
    let output_descriptor = if want_source_map {
        RuntimeOutputDescriptor::generated(
            &output,
            Some(source_map.as_str()).filter(|map| !map.is_empty()),
            &[(source.source_space_token, source.content_artifact_token)],
            SourceMapFidelity::Exact,
        )
    } else {
        RuntimeOutputDescriptor::generated_without_map(
            output.len() as u64,
            source.source_space_token,
            source.content_artifact_token,
        )
    };
    Ok(StyleRewriteOutcome::Rewritten {
        code: output,
        source_map,
        facts,
        output_descriptor: Box::new(output_descriptor),
    })
}

#[cfg(test)]
mod prepared_slot_join_tests {
    use super::{prepare_supplied_style, prepared_style_for_sealed_slot, PreparedStyleIr};
    use verter_css_syntax::PreprocessedStyle;

    fn supplied(css: &str) -> PreparedStyleIr {
        prepare_supplied_style(PreprocessedStyle::admitted(
            css,
            verter_css_syntax::PreprocessorIdentity::Anonymous,
        ))
        .expect("css parses")
    }

    #[test]
    fn sealed_slot_join_does_not_alias_same_bytes_in_another_slot() {
        let css = ".card { color: red; }";
        let prepared = supplied(css);
        let styles = vec![Some(prepared.clone()), None];

        assert!(
            prepared_style_for_sealed_slot(None, &styles, 0, css).is_some(),
            "index 0 is the sealed slot"
        );
        assert!(
            prepared_style_for_sealed_slot(None, &styles, 1, css).is_none(),
            "identical bytes at another index are not a join"
        );
        assert!(
            prepared_style_for_sealed_slot(Some(&prepared), &[], 0, css).is_some(),
            "host-resolved slot_parsed is the sealed join"
        );
        assert!(
            prepared_style_for_sealed_slot(None, &styles, 0, ".other { color: red; }").is_none(),
            "byte mismatch on a sealed slot fails closed"
        );
    }
}

// ---------------------------------------------------------------------------
// Stage-qualified external-style continuation boundary
// ---------------------------------------------------------------------------

/// Why a completed external preprocessing result was not admitted as a
/// continuation.
///
/// Every variant names a fact the boundary READ and found wrong. None of them
/// names work the boundary would have had to do itself: this authority never
/// parses, preprocesses or normalises CSS, so "the bytes are not valid CSS" is
/// not a refusal it can spell, and a caller cannot reach a state where it
/// would want to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleContinuationRefusal {
    /// The result is not completed external preprocessor output: it does not
    /// sit at [`StyleStage::Preprocessed`], or it records
    /// [`StyleProducer::Verter`] rather than the external tool that made the
    /// bytes. Authored and framework-rewritten bytes are different byte
    /// spaces, and continuing from them would attach this boundary's identity
    /// to coordinates it does not describe.
    ///
    /// Both halves are one variant because
    /// [`QualifiedStyleResult::as_preprocessed`] is one gate over both, and
    /// splitting it here would add an arm no caller can reach: every public
    /// constructor that reaches the preprocessed stage with bytes is
    /// externally produced by construction.
    WrongStage,
    /// The pipeline refused to produce the result — see
    /// [`QualifiedStyleResult::refused`]. There are no bytes to continue from,
    /// and an empty artifact would be indistinguishable from an authored
    /// `<style></style>`.
    RefusedResult,
    /// A carried diagnostic's position resolves to nothing this continuation
    /// carries: it addresses [`StyleStage::FrameworkRewritten`] — a space no
    /// continuation carries, since the rewrite has not run — or its span is
    /// inverted or leaves the space its stage names. An
    /// [`StyleStage::Authored`] span is block-relative (`0..` the authored
    /// extent's length, the arithmetic the compiler's diagnostic surface
    /// shifts by the block start); a [`StyleStage::Preprocessed`] span
    /// addresses the produced bytes.
    UnqualifiedDiagnostic,
    /// The recorded consumed basis is not the style unit's own content
    /// identity: the bytes the tool consumed are not the bytes this unit
    /// registered, so the result describes a different revision.
    StaleContentBasis,
    /// The producer's observed inputs do not include the style unit itself.
    ProvenanceMissingStyleUnit,
    /// The authored extent is inverted.
    InvalidAuthoredExtent,
    /// A map anchor names a source unit other than this continuation's style
    /// unit. The continuation maps exactly one authored space.
    MapSourceMismatch,
    /// A map anchor addresses authored bytes outside the style unit's
    /// registered extent (anchor source spans are carrier-absolute, like the
    /// extent itself), or generated bytes outside the produced code; or two
    /// anchors overlap in, or share the start of, a generated range.
    UnqualifiedMap,
    /// Two continuations claim the same style source unit.
    DuplicateContinuation,
    /// A companion source unit reuses a continuation's style-unit identity —
    /// two different declarations of one identity.
    SourceAliasing,
    /// A companion source unit that no admitted continuation's provenance
    /// observes. Companions exist so the schema can resolve the inputs the
    /// continuations declare; one that appears in no `provenance.inputs`
    /// contributes no artifact and is referenced by nothing, so admitting it
    /// would publish an unrelated source unit inside this set's declared
    /// source space.
    UnobservedCompanion,
    /// The artifact schema rejected the derived set. Carried through rather
    /// than restated, so the schema stays the single authority for the
    /// invariants it owns.
    Schema(crate::assembly::ArtifactSchemaError),
}

impl std::fmt::Display for StyleContinuationRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "external style continuation refused: {self:?}")
    }
}
impl std::error::Error for StyleContinuationRefusal {}

/// Everything a caller states about one completed external preprocessing run,
/// before the boundary has checked any of it.
///
/// Fields are public because this is the unvalidated input shape; the admitted
/// [`ExternalStyleContinuation`] is the immutable one. `product` is caller-
/// declared on purpose: which compile product a stylesheet belongs to is the
/// requesting capability's decision, and this boundary validates identity,
/// stage and basis — not product taxonomy.
#[derive(Debug, Clone)]
pub struct StyleContinuationInput {
    /// The dialect the block was AUTHORED in, retained because the produced
    /// bytes are always plain CSS and would otherwise lose that fact.
    pub authored_dialect: CssDialect,
    /// The style block's own logical unit: lineage, source revision, and the
    /// exact-bytes identity of its authored content.
    pub style_unit: crate::assembly::SourceUnit,
    /// The block's registered extent in its carrier's absolute byte space.
    pub authored_extent: Span,
    /// The exact authored bytes the external tool consumed, as the caller
    /// observed them. Checked against the style unit's own content identity.
    pub consumed_basis: crate::assembly::ContentId,
    /// Observed inputs and producer contract for the completed run.
    ///
    /// Only `inputs` is checked here, and only for the one relation this
    /// boundary can judge: that the producer observed the style unit it
    /// claims to have consumed. `producer` and `input_basis` are carried
    /// declarations — [`crate::assembly::ArtifactProvenance`] defines them as
    /// supplied by the owning compiler capability and never resolved, and
    /// this boundary owns no registry of producer contracts and no way to
    /// re-derive an observation basis, so it can neither confirm nor deny
    /// them. Taking a caller-supplied "expected" copy of each would compare
    /// one caller's two declarations of the same fact and prove nothing.
    /// Whoever mints these values remains their authority.
    pub provenance: crate::assembly::ArtifactProvenance,
    /// The completed, stage-qualified result the style owner produced.
    pub result: QualifiedStyleResult,
    /// Authored-to-produced anchors, in the caller's order. Empty means the
    /// tool supplied no map — an absent map, never an implicit identity one.
    pub anchors: Vec<crate::assembly::ArtifactMapSegment>,
    /// The compile product the produced stylesheet belongs to.
    pub product: crate::compile_request::ProductKind,
}

/// One admitted external-style continuation: a completed preprocessing result
/// together with the identity that makes its bytes addressable.
///
/// Immutable, and constructible only through [`Self::admit`], so a value of
/// this type is a proof of exactly these facts, and of no others: the result
/// completed at [`StyleStage::Preprocessed`] under an external producer; every
/// carried diagnostic resolves inside a space this value carries; the consumed
/// basis is the style unit's own content identity; the producer's observed
/// inputs include that style unit; the authored extent is well-formed; and
/// every map anchor names that unit, lands inside both the authored extent and
/// the produced bytes, and does not overlap or share a start with another
/// anchor. Provenance facts the compiler cannot re-derive — `producer` and
/// `input_basis` — are carried, not checked; see
/// [`StyleContinuationInput::provenance`]. It holds no production call site:
/// the route that consumes it arrives with its own owner.
///
/// Diagnostics stay HERE rather than being folded into the produced
/// [`crate::assembly::CompileArtifact`]. The artifact schema has no diagnostic
/// axis, and inventing one for this additive boundary would mutate a schema it
/// does not own; the continuation is the record its consumers read, and it
/// preserves the producing order exactly.
#[derive(Debug, Clone)]
pub struct ExternalStyleContinuation {
    input: StyleContinuationInput,
}

/// The artifact slot name a completed external preprocessing result publishes
/// under, within its style unit and product.
const PREPROCESSED_STYLE_SLOT: &str = "preprocessed";

/// The language a continuation's produced bytes are written in. Fixed, not
/// derived from the authored dialect: [`QualifiedStyleResult::preprocessed`]
/// already fixes the output dialect to plain CSS, which is what a preprocessor
/// finishing means.
const PREPROCESSED_STYLE_LANGUAGE: &str = "css";

impl ExternalStyleContinuation {
    /// Check a caller's stated facts and admit the result as a continuation.
    ///
    /// Bounded work: a constant number of identity comparisons, one pass over
    /// the diagnostics, and one pass plus one sort over the anchors' generated
    /// ranges. Nothing here reads, parses, transforms or copies the style
    /// bytes; only their length is consulted.
    pub fn admit(input: StyleContinuationInput) -> Result<Self, StyleContinuationRefusal> {
        use StyleContinuationRefusal as R;

        // A refusal is checked first so the caller learns that the run
        // produced nothing, rather than being told its stage was wrong — a
        // refusal AT the preprocessed stage records exactly that stage.
        if input.result.is_refused() {
            return Err(R::RefusedResult);
        }
        // `as_preprocessed` is the single gate on stage AND external producer;
        // this boundary does not restate either half.
        if input.result.as_preprocessed().is_none() {
            return Err(R::WrongStage);
        }
        if input.authored_extent.start > input.authored_extent.end {
            return Err(R::InvalidAuthoredExtent);
        }
        let produced = input.result.code();
        let authored_len = (input.authored_extent.end - input.authored_extent.start) as usize;
        let within = |span: Span, len: usize| span.start <= span.end && span.end as usize <= len;
        let diagnostics_resolve = input.result.diagnostics().iter().all(|diagnostic| {
            match (diagnostic.stage(), diagnostic.span()) {
                (StyleStage::FrameworkRewritten, _) => false,
                (_, None) => true,
                (StyleStage::Authored, Some(span)) => within(span, authored_len),
                (StyleStage::Preprocessed, Some(span)) => within(span, produced.len()),
            }
        });
        if !diagnostics_resolve {
            return Err(R::UnqualifiedDiagnostic);
        }
        if input.consumed_basis != *input.style_unit.content() {
            return Err(R::StaleContentBasis);
        }
        if !input.provenance.inputs.contains(input.style_unit.id()) {
            return Err(R::ProvenanceMissingStyleUnit);
        }
        for anchor in &input.anchors {
            if anchor.source_unit != *input.style_unit.id() {
                return Err(R::MapSourceMismatch);
            }
            if anchor.source_span.start > anchor.source_span.end
                || anchor.source_span.start < input.authored_extent.start
                || anchor.source_span.end > input.authored_extent.end
            {
                return Err(R::UnqualifiedMap);
            }
            if produced
                .get(anchor.generated.start as usize..anchor.generated.end as usize)
                .is_none()
            {
                return Err(R::UnqualifiedMap);
            }
        }
        // The same overlap rule the artifact schema applies, judged over
        // generated order on a copy so the anchors stay as supplied.
        let mut generated: Vec<_> = input
            .anchors
            .iter()
            .map(|anchor| (anchor.generated.start, anchor.generated.end))
            .collect();
        generated.sort_unstable();
        if generated
            .windows(2)
            .any(|pair| pair[0].1 > pair[1].0 || pair[0].0 == pair[1].0)
        {
            return Err(R::UnqualifiedMap);
        }
        Ok(Self { input })
    }

    /// The dialect the block was authored in.
    #[must_use]
    pub const fn authored_dialect(&self) -> CssDialect {
        self.input.authored_dialect
    }

    /// The style block's logical source unit.
    #[must_use]
    pub const fn style_unit(&self) -> &crate::assembly::SourceUnit {
        &self.input.style_unit
    }

    /// The block's registered extent in its carrier's byte space.
    #[must_use]
    pub const fn authored_extent(&self) -> Span {
        self.input.authored_extent
    }

    /// Observed inputs and producer contract for the completed run.
    #[must_use]
    pub const fn provenance(&self) -> &crate::assembly::ArtifactProvenance {
        &self.input.provenance
    }

    /// The admitted result: produced bytes, stage, dialect and producer.
    #[must_use]
    pub const fn result(&self) -> &QualifiedStyleResult {
        &self.input.result
    }

    /// Diagnostics the producing run reported, in production order.
    #[must_use]
    pub fn diagnostics(&self) -> &[StyleDiagnostic] {
        self.input.result.diagnostics()
    }

    /// The authored-to-produced anchors, exactly as supplied.
    #[must_use]
    pub fn anchors(&self) -> &[crate::assembly::ArtifactMapSegment] {
        &self.input.anchors
    }

    /// The source unit this continuation registers in an artifact set.
    fn source_unit(&self) -> crate::assembly::ArtifactSourceUnit {
        crate::assembly::ArtifactSourceUnit {
            unit: self.input.style_unit.clone(),
            source_span: self.input.authored_extent,
        }
    }

    /// Convert into the artifact this continuation describes, taking the
    /// produced bytes rather than borrowing and re-copying them. The single
    /// `String` → `Arc<str>` conversion the shared payload needs happens here,
    /// once, so no consumer downstream copies the stylesheet again.
    ///
    /// The map family is [`crate::assembly::ArtifactMapFamily::SourceProjection`]:
    /// the anchors relate authored carrier bytes to generated stylesheet bytes,
    /// which is a projection, not a runtime source map. No anchors means no map
    /// at all — this boundary never synthesizes an identity map.
    fn into_artifact(self) -> crate::assembly::CompileArtifact {
        use crate::assembly::{
            ArtifactContent, ArtifactMapFamily, CompileArtifact, ContentId, QualifiedArtifactMap,
        };
        let StyleContinuationInput {
            style_unit,
            provenance,
            result,
            anchors,
            product,
            ..
        } = self.input;
        let input_basis = provenance.input_basis.clone();
        let sources = std::collections::BTreeSet::from([style_unit.id().clone()]);
        let code = result.into_code();
        let generated_content = ContentId::from_content_bytes(code.as_bytes());
        let mut artifact = CompileArtifact::new(
            style_unit.id().clone(),
            product,
            verter_language::LanguageId::new(PREPROCESSED_STYLE_LANGUAGE),
            PREPROCESSED_STYLE_SLOT,
            provenance,
            ArtifactContent::Available(code.into()),
        );
        if !anchors.is_empty() {
            artifact.maps.push(QualifiedArtifactMap {
                family: ArtifactMapFamily::SourceProjection,
                generated: artifact.id().clone(),
                generated_content,
                input_basis,
                sources,
                authored_json: None,
                segments: anchors,
            });
        }
        artifact
    }
}

/// Describe admitted continuations as one immutable artifact set.
///
/// `companions` are source units the continuations' provenance already
/// observes (a shared theme sheet, the carrier's script unit) and which the
/// schema must therefore be able to resolve; they contribute no artifact.
/// That is a requirement, not a description of the expected caller: a
/// companion no continuation observes is refused rather than declared.
///
/// Two continuations of the same carrier source are recorded as
/// [`crate::assembly::ArtifactRelationKind::CompanionOf`] peers, which is what
/// sibling `<style>` blocks of one component are. Continuations of different
/// sources are unrelated and get no edge.
///
/// Deterministic: identical inputs in any order produce a byte-identical
/// serialization, because the schema sorts artifacts by identity and relations
/// and sources are set-valued.
pub fn describe_style_continuations(
    continuations: Vec<ExternalStyleContinuation>,
    companions: Vec<crate::assembly::ArtifactSourceUnit>,
) -> Result<crate::assembly::CompileArtifactSet, StyleContinuationRefusal> {
    use crate::assembly::{ArtifactRelation, ArtifactRelationKind, CompileArtifactSet};
    use StyleContinuationRefusal as R;

    let mut source_units: Vec<_> = Vec::with_capacity(continuations.len() + companions.len());
    let mut claimed = std::collections::BTreeSet::new();
    for continuation in &continuations {
        if !claimed.insert(continuation.style_unit().id().clone()) {
            return Err(R::DuplicateContinuation);
        }
        source_units.push(continuation.source_unit());
    }
    // A companion is only ever needed because some continuation's provenance
    // names it, so the set of names is exactly what the continuations already
    // declared. Aliasing is judged first: reusing a continuation's identity is
    // a wrong declaration of an observed unit, not an unobserved one.
    let observed: std::collections::BTreeSet<_> = continuations
        .iter()
        .flat_map(|continuation| continuation.provenance().inputs.iter())
        .collect();
    for companion in companions {
        if claimed.contains(companion.unit.id()) {
            return Err(R::SourceAliasing);
        }
        if !observed.contains(companion.unit.id()) {
            return Err(R::UnobservedCompanion);
        }
        source_units.push(companion);
    }

    // Every artifact is minted before any edge is recorded, so the peer set a
    // continuation sees does not depend on the order it was supplied in.
    let mut artifacts: Vec<_> = continuations
        .into_iter()
        .map(|continuation| {
            let source = continuation.style_unit().source_id().clone();
            (source, continuation.into_artifact())
        })
        .collect();
    let peers: Vec<_> = artifacts
        .iter()
        .map(|(source, artifact)| (source.clone(), artifact.id().clone()))
        .collect();
    for (source, artifact) in &mut artifacts {
        for (peer_source, peer_id) in &peers {
            if peer_id != artifact.id() && peer_source == source {
                artifact.relations.insert(ArtifactRelation {
                    kind: ArtifactRelationKind::CompanionOf,
                    target: peer_id.clone(),
                });
            }
        }
    }

    let artifacts = artifacts
        .into_iter()
        .map(|(_, artifact)| artifact)
        .collect();
    CompileArtifactSet::new(source_units, artifacts).map_err(R::Schema)
}
