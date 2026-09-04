//! Typed framework style rewrite stages over the shared style syntax IR.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use oxc_allocator::Allocator;
use sha2::{Digest, Sha256};
use verter_css_syntax::{
    css_identifier_eq_ignore_ascii_case, parse_style_ir, CombinatorKind, ComplexSelector,
    ComplexSelectorPart, ComponentValue, ComponentValueTree, CssDialect, CssParseMode, CssSource,
    QualifiedStyleResult, SelectorComponent, SelectorComponentKind, SelectorList, SelectorPseudo,
    StyleCompleteness, StyleDeclaration, StyleDiagnostic, StyleDirective, StyleStage,
    StyleStatement, StyleSyntaxIr, TokenKind, UnknownStatement, UnknownStatementKind,
};

/// The witness a caller-preprocessed style block enters the compiler through,
/// and the producer vocabulary the witness carries.
///
/// Re-exported so the admitting host names the compiler's own entry vocabulary
/// instead of taking a direct dependency on the syntax crate for these types.
pub use verter_css_syntax::{
    ExternalStyleProducer, PreprocessedStyle, PreprocessorIdentity, StyleProducer,
};
use verter_span::Span;

use crate::code_transform::{advance_generated_position, CodeTransform, SourceMapOptions};
use crate::framework_common::{RuntimeOutputDescriptor, SourceMapFidelity};
use oxc_sourcemap::{SourceMap, Token};

/// Generate a Vue CSS variable name from a scope ID and authored expression.
#[must_use]
pub fn generate_var_name(scope_id: &str, expression: &str) -> String {
    let mut out = String::with_capacity(2 + scope_id.len() + 1 + expression.len());
    out.push_str("--");
    out.push_str(scope_id);
    out.push('-');
    for character in expression.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
            out.push(character);
        } else {
            out.push('_');
        }
    }
    out
}

fn css_var_reference(var_name: &str) -> String {
    let mut out = String::with_capacity(5 + var_name.len());
    out.push_str("var(");
    out.push_str(var_name);
    out.push(')');
    out
}

/// A `v-bind()` expression replaced with a CSS variable.
#[derive(Debug, Clone)]
pub struct VBindVar {
    /// The original expression text (e.g. "color" or "theme.color").
    pub expression: String,
    /// The generated CSS variable name (e.g. "--a4f2eed6-color").
    pub var_name: String,
    /// Byte offset of the quote-stripped expression start within the style content.
    pub expr_start: u32,
    /// Byte offset of the quote-stripped expression end within the style content.
    pub expr_end: u32,
}

/// Generate a content-based hash for a CSS module class name.
fn hashed_class_name(component_id: &str, class_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(component_id.as_bytes());
    hasher.update(class_name.as_bytes());
    let result = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(class_name.len() + 1 + 8);
    out.push_str(class_name);
    out.push('_');
    for &byte in &result[..4] {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

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
    fn new(
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
struct StyleSourceIdentity<'a> {
    source_name: &'a str,
    source_space_token: &'a str,
    content_artifact_token: &'a str,
}

#[derive(Clone, Copy)]
pub struct AuthoredStyleInput<'a> {
    code: &'a str,
    dialect: CssDialect,
    source: StyleSourceIdentity<'a>,
    prepared: Option<&'a StyleSyntaxIr>,
    want_source_map: bool,
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
    code: &'a str,
    /// Threaded into the crate-test isolated materialize entries. Production
    /// uses this type only as the plain-CSS gate (`try_new`).
    #[cfg_attr(not(test), allow(dead_code))]
    source: StyleSourceIdentity<'a>,
    #[cfg_attr(not(test), allow(dead_code))]
    want_source_map: bool,
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
#[derive(Clone)]
pub struct PreparedStyleIr {
    ir: Arc<StyleSyntaxIr>,
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
    ir: &'a StyleSyntaxIr,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VueStyleRewriteMask {
    pub v_bind: bool,
    pub css_modules: bool,
    pub scoped_selector: bool,
    pub keyframes: bool,
    pub deep: bool,
    pub slotted: bool,
    pub global: bool,
}

#[derive(Debug, Clone, Default)]
pub struct VueStyleFacts {
    pub v_bind_vars: Vec<VBindVar>,
    pub module_classes: Vec<(String, String)>,
    pub refusals: Vec<StyleRewriteFailure>,
    pub rewrites: VueStyleRewriteMask,
    /// A parse of the cascade's INPUT surveyed its inclusions and answered
    /// whether they reach bytes nothing here parsed — or `None` when no such
    /// parse has run.
    ///
    /// Private, and three-state on purpose. The two states a `bool` conflates
    /// are the ones that matter: "surveyed, nothing foreign" and "never
    /// surveyed" are opposite answers to the question consumers ask, and only
    /// the tri-state distinguishes them. It is also what memoizes the
    /// recorder — shared planning parses the cascade input once, and a sheet
    /// with no inclusions must not read as "not recorded yet" and let a later
    /// observation publish a different space's answer as the input's.
    ///
    /// Read through [`Self::pulls_in_unparsed_bytes`], which fails closed.
    input_pulls_in_unparsed_bytes: Option<bool>,
}

impl VueStyleFacts {
    /// Whether this block pulls in stylesheet bytes nothing here parsed.
    ///
    /// `true` means the block's declared surface is incomplete: classes,
    /// custom properties and `v-bind()` calls may exist outside anything
    /// Verter saw, so a consumer publishing a complete-looking inventory has
    /// to fail open on it.
    ///
    /// **An unsurveyed block answers `true`.** `false` is the STRONG claim —
    /// "nothing outside these bytes can contribute to this block's surface" —
    /// and no parse has earned it until one has run. The reachable state is a
    /// cascade whose parse of the input never completed, with neither CSS
    /// Modules nor scoping requested to clear the output: nothing surveys the
    /// input, and answering "exhaustive" there is exactly the wrong-complete
    /// direction.
    ///
    /// A parse that COMPLETED has surveyed the block, and the answer is
    /// recorded from it before any stage plans an edit — so a stage that then
    /// refuses (an untrusted `v-bind()` rewrite target, an indented-layout
    /// mutation) does not un-survey it. A parse that merely RECOVERED still
    /// answers `true`, through the owner's own `discarded_input` check rather
    /// than through the absence of a recording.
    ///
    /// It is deliberately neither `!dependencies.is_empty()` nor a fold over
    /// an inclusion list — the style-syntax owner answers it for the whole
    /// parse, because a parse that recovered records FEWER inclusions than the
    /// sheet has, not more.
    #[must_use]
    pub const fn pulls_in_unparsed_bytes(&self) -> bool {
        match self.input_pulls_in_unparsed_bytes {
            Some(pulls) => pulls,
            None => true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum StyleRewriteOutcome {
    Unchanged {
        facts: VueStyleFacts,
    },
    Rewritten {
        code: String,
        source_map: String,
        facts: VueStyleFacts,
        output_descriptor: Box<RuntimeOutputDescriptor>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StyleEdit {
    Overwrite { span: Span, content: String },
    Insert { at: u32, content: String },
}

enum PreparedEdit<'a> {
    Overwrite {
        start: u32,
        end: u32,
        content: &'a str,
    },
    Insert {
        at: u32,
        content: &'a str,
    },
}

fn intern_edit_content<'a>(
    allocator: &'a Allocator,
    interned: &mut Vec<&'a str>,
    content: &str,
) -> &'a str {
    if let Some(existing) = interned.iter().copied().find(|s| *s == content) {
        existing
    } else {
        let interned_content = allocator.alloc_str(content);
        interned.push(interned_content);
        interned_content
    }
}

impl StyleEdit {
    const fn start(&self) -> u32 {
        match self {
            Self::Overwrite { span, .. } => span.start,
            Self::Insert { at, .. } => *at,
        }
    }

    const fn end(&self) -> u32 {
        match self {
            Self::Overwrite { span, .. } => span.end,
            Self::Insert { at, .. } => *at,
        }
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

struct ParsedStyleIr {
    inner: StyleSyntaxIr,
    #[cfg(test)]
    identity: usize,
}

impl ParsedStyleIr {
    fn from_existing(inner: StyleSyntaxIr) -> Self {
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
fn observe_style_ir(stage: StyleRewriteStage, ir: &ParsedStyleIr) {
    STYLE_IR_STAGE_OBSERVATIONS.with(|observations| {
        observations.borrow_mut().push((stage, ir.identity));
    });
}

#[cfg(not(test))]
fn observe_style_ir(_stage: StyleRewriteStage, _ir: &ParsedStyleIr) {}

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
fn record_input_dependencies(facts: &mut VueStyleFacts, ir: &ParsedStyleIr) {
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

fn relocate_edits(
    edits: Vec<StyleEdit>,
    origin: u32,
    stage: StyleRewriteStage,
    dialect: CssDialect,
) -> Result<Vec<StyleEdit>, StyleRewriteFailure> {
    if origin == 0 {
        return Ok(edits);
    }
    edits
        .into_iter()
        .map(|edit| match edit {
            StyleEdit::Overwrite { span, content } => {
                let start = span.start.checked_sub(origin).ok_or_else(|| {
                    StyleRewriteFailure::new(
                        StyleRewriteFailureClass::ParseFailure,
                        stage,
                        dialect,
                        Some(span),
                    )
                })?;
                let end = span.end.checked_sub(origin).ok_or_else(|| {
                    StyleRewriteFailure::new(
                        StyleRewriteFailureClass::ParseFailure,
                        stage,
                        dialect,
                        Some(span),
                    )
                })?;
                Ok(StyleEdit::Overwrite {
                    span: Span::new(start, end),
                    content,
                })
            }
            StyleEdit::Insert { at, content } => {
                let at = at.checked_sub(origin).ok_or_else(|| {
                    StyleRewriteFailure::new(
                        StyleRewriteFailureClass::ParseFailure,
                        stage,
                        dialect,
                        Some(Span::new(at, at)),
                    )
                })?;
                Ok(StyleEdit::Insert { at, content })
            }
        })
        .collect()
}

fn relocate_v_bind_vars(vars: Vec<VBindVar>, origin: u32) -> Vec<VBindVar> {
    if origin == 0 {
        return vars;
    }
    vars.into_iter()
        .map(|mut var| {
            var.expr_start = var.expr_start.saturating_sub(origin);
            var.expr_end = var.expr_end.saturating_sub(origin);
            var
        })
        .collect()
}

/// Plan the authored-`v-bind()` stage's edits and variable inventory from one
/// parsed IR, in real source coordinates.
///
/// Every route that plans v-bind edits calls this — the public authored-only
/// transform, the cascade's authored-only stage, and the shared multi-stage
/// plan — rather than each computing them independently, so a fix to v-bind
/// planning cannot land on one path and silently miss the others.
fn plan_authored_v_bind_edits(
    ir: &ParsedStyleIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, Vec<VBindVar>), StyleRewriteFailure> {
    let origin = ir.source().origin();
    let (edits, vars) = v_bind_edits_from_ir(ir, dialect, scope_id)?;
    let edits = relocate_edits(edits, origin, StyleRewriteStage::AuthoredVBind, dialect)?;
    let vars = relocate_v_bind_vars(vars, origin);
    Ok((edits, vars))
}

fn authored_or_parsed_ir(
    input: AuthoredStyleInput<'_>,
    stage: StyleRewriteStage,
) -> Result<ParsedStyleIr, StyleRewriteFailure> {
    if let Some(prepared) = input.prepared {
        return Ok(ParsedStyleIr::from_existing(prepared.clone()));
    }
    parse_ir(input.code, input.dialect, stage)
}

fn parse_ir(
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

fn build_transform_output(
    code: &str,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    mut edits: Vec<StyleEdit>,
    source_name: Option<&str>,
    want_source_map: bool,
) -> Result<Option<(String, Option<SourceMap<'static>>)>, StyleRewriteFailure> {
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
    let mut transform = CodeTransform::new(code, &allocator);
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
        transform.batch_overwrite(&ops);
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
}

/// `want_source_map` mirrors the caller's `RuntimeCompileOptions::source_map`
/// intent. `CodeTransform::generate_map` + `to_json_string()` are not
/// free — a caller that does not want a source map must not pay for building
/// and stringifying one. When `false`, `emit` skips that machinery and
/// returns an empty `source_map` / `raw_map: None`, matching the
/// `Unchanged`/no-map shape callers already handle.
fn emit(
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

pub fn transform_vue_v_bind(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = authored_or_parsed_ir(input, StyleRewriteStage::AuthoredVBind)?;
    let (edits, vars) = plan_authored_v_bind_edits(&ir, input.dialect, scope_id)?;
    let mut facts = VueStyleFacts {
        rewrites: VueStyleRewriteMask {
            v_bind: !edits.is_empty(),
            ..VueStyleRewriteMask::default()
        },
        v_bind_vars: vars,
        ..VueStyleFacts::default()
    };
    record_input_dependencies(&mut facts, &ir);
    emit(
        input.code,
        input.source,
        input.dialect,
        StyleRewriteStage::AuthoredVBind,
        edits,
        facts,
        input.want_source_map,
    )
}

/// Fact-only v-bind extraction over an already-parsed IR. Does not itself
/// call `parse_style_ir`. `Err` means at least one `v-bind()` target in this
/// block was too ambiguous to trust — callers must fail OPEN (never treat
/// this as "no v-binds present").
pub fn v_bind_vars_from_parsed_ir(
    ir: &StyleSyntaxIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<Vec<VBindVar>, StyleRewriteFailure> {
    v_bind_edits_from_ir(ir, dialect, scope_id).map(|(_edits, vars)| vars)
}

/// Collects the authored v-bind edits/vars from an already-parsed IR, without
/// itself calling `parse_ir` — the shared building block `transform_vue_v_bind`
/// (parses then delegates here) and `run_vue_style_cascade` (reuses a
/// retained IR across stages) both route through.
fn v_bind_edits_from_ir(
    ir: &StyleSyntaxIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, Vec<VBindVar>), StyleRewriteFailure> {
    let mut edits = Vec::new();
    let mut vars = Vec::new();
    collect_v_bind_statements(
        ir.statements(),
        ir.source(),
        dialect,
        scope_id,
        true,
        &mut edits,
        &mut vars,
    )?;
    Ok((edits, vars))
}

fn collect_v_bind_statements(
    statements: &[StyleStatement],
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    trusted_ancestor: bool,
    edits: &mut Vec<StyleEdit>,
    vars: &mut Vec<VBindVar>,
) -> Result<(), StyleRewriteFailure> {
    let mut trusted_context = trusted_ancestor;
    for statement in statements {
        match statement {
            StyleStatement::Declaration(declaration) => {
                let trusted = trusted_context
                    && declaration.completeness() == StyleCompleteness::Complete
                    && declaration.value().completeness() == StyleCompleteness::Complete;
                collect_v_bind_values(
                    declaration.value(),
                    source,
                    dialect,
                    scope_id,
                    trusted,
                    edits,
                    vars,
                )?;
                if let Some(body) = declaration.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        trusted && body.completeness() == StyleCompleteness::Complete,
                        edits,
                        vars,
                    )?;
                }
            }
            StyleStatement::Rule(rule) => {
                let trusted = trusted_context
                    && rule.completeness() == StyleCompleteness::Complete
                    && rule.body().completeness() == StyleCompleteness::Complete;
                collect_v_bind_statements(
                    rule.body().statements(),
                    source,
                    dialect,
                    scope_id,
                    trusted,
                    edits,
                    vars,
                )?;
            }
            StyleStatement::AtRule(rule) => {
                if let Some(body) = rule.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        trusted_context
                            && rule.completeness() == StyleCompleteness::Complete
                            && body.completeness() == StyleCompleteness::Complete,
                        edits,
                        vars,
                    )?;
                }
            }
            StyleStatement::MixinOrFunction(rule) => {
                if let Some(body) = rule.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        trusted_context
                            && rule.completeness() == StyleCompleteness::Complete
                            && body.completeness() == StyleCompleteness::Complete,
                        edits,
                        vars,
                    )?;
                }
            }
            StyleStatement::Unknown(unknown) => {
                if let Some(values) = unknown.opaque_values() {
                    let trusted = trusted_context
                        && dialect == CssDialect::Stylus
                        && unknown.kind() == UnknownStatementKind::Ambiguous
                        && values.completeness() == StyleCompleteness::Complete;
                    collect_v_bind_values(values, source, dialect, scope_id, trusted, edits, vars)?;
                }
                if let Some(body) = unknown.body() {
                    collect_v_bind_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        false,
                        edits,
                        vars,
                    )?;
                }
                if unknown.kind() == UnknownStatementKind::Recovery {
                    trusted_context = false;
                }
            }
        }
    }
    Ok(())
}

fn collect_v_bind_values(
    tree: &ComponentValueTree,
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    trusted: bool,
    edits: &mut Vec<StyleEdit>,
    vars: &mut Vec<VBindVar>,
) -> Result<(), StyleRewriteFailure> {
    for value in tree.values() {
        match value {
            ComponentValue::Function(function) => {
                let name = source.slice(function.name_span());
                if css_identifier_eq_ignore_ascii_case(name, "v-bind") {
                    if !trusted || !function.is_complete() {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if matches!(dialect, CssDialect::Sass | CssDialect::Stylus)
                        && source.slice(function.full_span()).contains(['\r', '\n'])
                    {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::IndentedLayoutMutation,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if values_contain_interpolation(function.values(), source, dialect) {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    let (expression, expression_span) = v_bind_expression(source, function)?;
                    let var_name = generate_var_name(scope_id, expression);
                    edits.push(StyleEdit::Overwrite {
                        span: function.full_span(),
                        content: css_var_reference(&var_name),
                    });
                    vars.push(VBindVar {
                        expression: expression.to_string(),
                        var_name,
                        expr_start: expression_span.start,
                        expr_end: expression_span.end,
                    });
                } else {
                    let nested = ComponentValueTreeRef {
                        values: function.values(),
                    };
                    collect_v_bind_value_slice(
                        nested.values,
                        source,
                        dialect,
                        scope_id,
                        trusted && function.is_complete(),
                        edits,
                        vars,
                    )?;
                }
            }
            ComponentValue::Block(block) => collect_v_bind_value_slice(
                block.values(),
                source,
                dialect,
                scope_id,
                trusted && block.is_complete(),
                edits,
                vars,
            )?,
            ComponentValue::Interpolation(_) => {}
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
    Ok(())
}

struct ComponentValueTreeRef<'a> {
    values: &'a [ComponentValue],
}

fn collect_v_bind_value_slice(
    values: &[ComponentValue],
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    trusted: bool,
    edits: &mut Vec<StyleEdit>,
    vars: &mut Vec<VBindVar>,
) -> Result<(), StyleRewriteFailure> {
    let tree = ComponentValueTreeRef { values };
    for value in tree.values {
        match value {
            ComponentValue::Function(function) => {
                let name = source.slice(function.name_span());
                if css_identifier_eq_ignore_ascii_case(name, "v-bind") {
                    if !trusted || !function.is_complete() {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if matches!(dialect, CssDialect::Sass | CssDialect::Stylus)
                        && source.slice(function.full_span()).contains(['\r', '\n'])
                    {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::IndentedLayoutMutation,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    if values_contain_interpolation(function.values(), source, dialect) {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            Some(function.full_span()),
                        ));
                    }
                    let (expression, expression_span) = v_bind_expression(source, function)?;
                    let var_name = generate_var_name(scope_id, expression);
                    edits.push(StyleEdit::Overwrite {
                        span: function.full_span(),
                        content: css_var_reference(&var_name),
                    });
                    vars.push(VBindVar {
                        expression: expression.to_string(),
                        var_name,
                        expr_start: expression_span.start,
                        expr_end: expression_span.end,
                    });
                } else {
                    collect_v_bind_value_slice(
                        function.values(),
                        source,
                        dialect,
                        scope_id,
                        trusted && function.is_complete(),
                        edits,
                        vars,
                    )?;
                }
            }
            ComponentValue::Block(block) => collect_v_bind_value_slice(
                block.values(),
                source,
                dialect,
                scope_id,
                trusted && block.is_complete(),
                edits,
                vars,
            )?,
            ComponentValue::Interpolation(_) => {}
            ComponentValue::Token(_) | ComponentValue::String(_) | ComponentValue::Comment(_) => {}
        }
    }
    Ok(())
}

fn values_contain_interpolation(
    values: &[ComponentValue],
    source: &CssSource,
    dialect: CssDialect,
) -> bool {
    values.iter().any(|value| match value {
        ComponentValue::Interpolation(_) => true,
        ComponentValue::Function(function) => {
            values_contain_interpolation(function.values(), source, dialect)
        }
        ComponentValue::Block(block) => {
            values_contain_interpolation(block.values(), source, dialect)
        }
        ComponentValue::String(token) => {
            let text = source.slice(token.span());
            match dialect {
                CssDialect::Scss | CssDialect::Sass => text.contains("#{"),
                CssDialect::Less => text.contains("@{"),
                CssDialect::Css | CssDialect::Stylus => false,
            }
        }
        ComponentValue::Token(_) | ComponentValue::Comment(_) => false,
    })
}

fn v_bind_expression<'a>(
    source: &'a CssSource,
    function: &verter_css_syntax::ComponentFunction,
) -> Result<(&'a str, Span), StyleRewriteFailure> {
    let full = function.full_span();
    let raw_span = Span::new(function.name_span().end + 1, full.end.saturating_sub(1));
    let raw = source.slice(raw_span);
    let trimmed = raw.trim();
    let leading = u32::try_from(trimmed.as_ptr() as usize - raw.as_ptr() as usize).unwrap_or(0);
    let mut span = Span::new(
        raw_span.start + leading,
        raw_span.start + leading + u32::try_from(trimmed.len()).unwrap_or(u32::MAX),
    );
    let expression = if trimmed.len() >= 2
        && ((trimmed.starts_with('\'') && trimmed.ends_with('\''))
            || (trimmed.starts_with('"') && trimmed.ends_with('"')))
    {
        span = Span::new(span.start + 1, span.end - 1);
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    Ok((expression, span))
}

/// Collects the CSS-Modules class edits/hashed-name map from an already-parsed
/// IR, without itself calling `parse_ir`. Dialect-agnostic: the selector walk
/// (`collect_module_statements`) never depends on `CssDialect::Css` — that
/// requirement lives entirely in the RUNTIME rewrite entry points
/// (`PlainCssInput`'s construction gate), not in the walk itself. This is what
/// lets `analyze_css_module_classes` (A10a/A10b, class *analysis* only) reuse
/// the exact same walk for all five native dialects without touching runtime
/// class-name rewriting's plain-CSS-only ownership (row 19, untouched).
fn module_classes_and_edits_from_ir(
    ir: &StyleSyntaxIr,
    dialect: CssDialect,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, BTreeMap<String, String>), StyleRewriteFailure> {
    let mut edits = Vec::new();
    let mut classes = BTreeMap::new();
    collect_module_statements(
        ir.statements(),
        ir.source(),
        dialect,
        scope_id,
        false,
        &mut edits,
        &mut classes,
    )?;
    Ok((edits, classes))
}

fn dedup_static_class_names(ir: &StyleSyntaxIr) -> Vec<String> {
    let source = ir.source();
    let mut seen = HashSet::new();
    ir.complete_static_classes()
        .filter_map(|class| {
            let name = source.slice(class.name_span());
            seen.insert(name).then(|| name.to_string())
        })
        .collect()
}

/// Enumerate complete static class selectors in `code` for IDE `$style` completions.
///
/// Parse failure degrades to an empty list (never panics): completions are advisory.
pub fn complete_static_class_names(code: &str, dialect: CssDialect) -> Vec<String> {
    let Ok(ir) = parse_ir(code, dialect, StyleRewriteStage::PostPreprocessModules) else {
        return Vec::new();
    };
    dedup_static_class_names(&ir)
}

/// Native CSS-Modules class *analysis*: enumerates every class selector an
/// authored style block declares, plus its would-be hashed name, for any of
/// the five native dialects (A10a/A10b) — analysis only, never a rewrite.
/// Runtime class-name rewriting stays the cascade's CSS-Modules stage
/// (plain-CSS-only, post-preprocess); row 19's ownership question is
/// untouched. Class selectors are syntactically identical across all five
/// dialects (no dialect-specific interpolation form is a bare `.class`), so
/// the walk that already backs that cascade stage needs no dialect gate to
/// run here.
pub fn analyze_css_module_classes(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<Vec<(String, String)>, StyleRewriteFailure> {
    let ir = authored_or_parsed_ir(input, StyleRewriteStage::PostPreprocessModules)?;
    let (_edits, classes) = module_classes_and_edits_from_ir(&ir, input.dialect, scope_id)?;
    Ok(classes.into_iter().collect())
}

/// Read-only style facts: complete static class names plus the hashed
/// CSS-Modules names those classes would receive. Never rewrites bytes and
/// never inherits rewrite-oriented refusal for an untrusted sibling selector.
#[derive(Debug, Clone, Default)]
pub struct StyleAnalysis {
    pub static_classes: Vec<String>,
    pub module_classes: Vec<(String, String)>,
}

pub fn analyze_style(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
) -> Result<StyleAnalysis, StyleRewriteFailure> {
    let ir = authored_or_parsed_ir(input, StyleRewriteStage::PostPreprocessModules)?;
    let static_classes = dedup_static_class_names(&ir);
    let module_classes = static_classes
        .iter()
        .map(|name| (name.clone(), hashed_class_name(scope_id, name)))
        .collect();
    Ok(StyleAnalysis {
        static_classes,
        module_classes,
    })
}

/// Crate-test instrument: parse a `PlainCssInput` and materialize only the
/// CSS-Modules stage's edits.
///
/// Not a production route. Shipped `<style module>` requests plan this stage
/// through `run_vue_style_cascade` over the one `StyleSyntaxIr` the run already
/// parsed. This entry exists so crate tests can compare a merged plan against
/// applying stages one after another. It is available only under `#[cfg(test)]`,
/// never through the `test-support` Cargo feature. Compiling it into any
/// non-test library build, or giving it a production caller, reintroduces the
/// second CSS parse and the staged coordinate spaces the shared plan removed.
#[cfg(test)]
pub(crate) fn transform_vue_css_modules(
    input: PlainCssInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(
        input.code,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessModules,
    )?;
    let (edits, classes) = module_classes_and_edits_from_ir(&ir, CssDialect::Css, scope_id)?;
    let mut facts = VueStyleFacts {
        module_classes: classes.into_iter().collect(),
        rewrites: VueStyleRewriteMask {
            css_modules: !edits.is_empty(),
            ..VueStyleRewriteMask::default()
        },
        ..VueStyleFacts::default()
    };
    record_input_dependencies(&mut facts, &ir);
    emit(
        input.code,
        input.source,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessModules,
        edits,
        facts,
        input.want_source_map,
    )
}

fn collect_module_statements(
    statements: &[StyleStatement],
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    inside_keyframes: bool,
    edits: &mut Vec<StyleEdit>,
    classes: &mut BTreeMap<String, String>,
) -> Result<(), StyleRewriteFailure> {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                if !inside_keyframes {
                    if rule.completeness() != StyleCompleteness::Complete
                        || !rule.selector_list().facts().is_complete_static()
                    {
                        return Err(StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::PostPreprocessModules,
                            dialect,
                            Some(rule.span()),
                        ));
                    }
                    collect_module_selector_list(
                        rule.selector_list(),
                        source,
                        dialect,
                        scope_id,
                        edits,
                        classes,
                    )?;
                }
                collect_module_statements(
                    rule.body().statements(),
                    source,
                    dialect,
                    scope_id,
                    inside_keyframes,
                    edits,
                    classes,
                )?;
            }
            StyleStatement::AtRule(rule) => {
                if let Some(body) = rule.body() {
                    let head = source.slice(rule.head_span());
                    let keyframes = css_identifier_eq_ignore_ascii_case(head, "@keyframes")
                        || css_identifier_eq_ignore_ascii_case(head, "@-webkit-keyframes");
                    collect_module_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        inside_keyframes || keyframes,
                        edits,
                        classes,
                    )?;
                }
            }
            StyleStatement::Declaration(declaration) => {
                if let Some(body) = declaration.body() {
                    collect_module_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        inside_keyframes,
                        edits,
                        classes,
                    )?;
                }
            }
            StyleStatement::MixinOrFunction(rule) => {
                if let Some(body) = rule.body() {
                    collect_module_statements(
                        body.statements(),
                        source,
                        dialect,
                        scope_id,
                        inside_keyframes,
                        edits,
                        classes,
                    )?;
                }
            }
            StyleStatement::Unknown(unknown) => {
                if !inside_keyframes {
                    return Err(StyleRewriteFailure::new(
                        StyleRewriteFailureClass::UntrustedRewriteTarget,
                        StyleRewriteStage::PostPreprocessModules,
                        dialect,
                        Some(unknown.span()),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn collect_module_selector_list(
    list: &SelectorList,
    source: &CssSource,
    dialect: CssDialect,
    scope_id: &str,
    edits: &mut Vec<StyleEdit>,
    classes: &mut BTreeMap<String, String>,
) -> Result<(), StyleRewriteFailure> {
    if !list.facts().is_complete_static() {
        return Err(StyleRewriteFailure::new(
            StyleRewriteFailureClass::UntrustedRewriteTarget,
            StyleRewriteStage::PostPreprocessModules,
            dialect,
            Some(list.span()),
        ));
    }
    for selector in list.selectors() {
        for compound in selector.compounds() {
            for component in compound.components() {
                if component.kind() == SelectorComponentKind::Class {
                    let name_span = component.name_span().ok_or_else(|| {
                        StyleRewriteFailure::new(
                            StyleRewriteFailureClass::UntrustedRewriteTarget,
                            StyleRewriteStage::PostPreprocessModules,
                            dialect,
                            Some(component.span()),
                        )
                    })?;
                    let name = source.slice(name_span);
                    let hashed = classes
                        .entry(name.to_string())
                        .or_insert_with(|| hashed_class_name(scope_id, name))
                        .clone();
                    edits.push(StyleEdit::Overwrite {
                        span: name_span,
                        content: hashed,
                    });
                }
                if let Some(nested) = component
                    .pseudo()
                    .and_then(verter_css_syntax::SelectorPseudo::selector_list)
                {
                    collect_module_selector_list(
                        nested, source, dialect, scope_id, edits, classes,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Collects the scoped-selector/keyframes edits and facts from an
/// already-parsed IR, without itself calling `parse_ir` — the shared building
/// block `run_vue_style_cascade` (reuses a retained IR across stages) and the
/// test-only isolated scoped instrument (parses then delegates here) both
/// route through.
fn scoped_edits_and_facts_from_ir(
    ir: &StyleSyntaxIr,
    scope_id: &str,
) -> Result<(Vec<StyleEdit>, VueStyleFacts), StyleRewriteFailure> {
    let mut planner = VueScopePlanner {
        source: ir.source(),
        scope_attr: format!("[data-v-{scope_id}]"),
        slotted_attr: format!("[data-v-{scope_id}-s]"),
        scope_id,
        edits: Vec::new(),
        facts: VueStyleFacts::default(),
        keyframes: Vec::new(),
    };
    planner.collect_keyframes(ir.statements())?;
    planner.plan_statements(ir.statements(), false)?;
    Ok((planner.edits, planner.facts))
}

/// Crate-test instrument: parse a `PlainCssInput` and materialize only the
/// scoped-selector stage's edits.
///
/// Not a production route. Shipped `<style scoped>` requests plan this stage
/// through `run_vue_style_cascade` over the one `StyleSyntaxIr` the run already
/// parsed. This entry exists so crate tests can compare a merged plan against
/// applying stages one after another. It is available only under `#[cfg(test)]`,
/// never through the `test-support` Cargo feature. Compiling it into any
/// non-test library build, or giving it a production caller, reintroduces the
/// second CSS parse and the staged coordinate spaces the shared plan removed.
#[cfg(test)]
pub(crate) fn transform_vue_scoped_css(
    input: PlainCssInput<'_>,
    scope_id: &str,
) -> Result<StyleRewriteOutcome, StyleRewriteFailure> {
    let ir = parse_ir(
        input.code,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessScoping,
    )?;
    let (edits, mut facts) = scoped_edits_and_facts_from_ir(&ir, scope_id)?;
    record_input_dependencies(&mut facts, &ir);
    emit(
        input.code,
        input.source,
        CssDialect::Css,
        StyleRewriteStage::PostPreprocessScoping,
        edits,
        facts,
        input.want_source_map,
    )
}

/// Result of running Vue's style cascade (`run_vue_style_cascade` /
/// `run_vue_style_cascade_verified`) end to end. The cascade never
/// hard-fails: a stage that cannot safely run is recorded in
/// `stage_failures` and the run continues on a best-effort basis instead of
/// aborting, so callers get one code path for both the fully-successful and
/// the degraded case.
#[derive(Debug, Clone)]
pub struct VueStyleCascadeOutcome {
    /// Final code after every requested stage's edits, qualified by the stage
    /// it now belongs to, the dialect it is written in, what produced it, and
    /// the inclusions and refusals observed along the way.
    ///
    /// The bytes are reachable only through this carrier: a caller that took a
    /// bare `String` here would have to re-derive which byte space it is
    /// holding, and every caller would answer that separately.
    /// The SOLE publication route for this run's diagnostics: `facts.refusals`
    /// and `stage_failures` below are each authority's own record of what it
    /// reported, and the carrier is where those records are projected into the
    /// shared diagnostic vocabulary exactly once. A consumer that reads both
    /// and re-projects reports every refusal twice, which is why projecting a
    /// failure into a diagnostic is not something a consumer can spell.
    pub result: QualifiedStyleResult,
    /// Authored-source-to-final-code map composed across every rewrite. Empty
    /// when no rewrite occurred or composition could not be completed.
    pub source_map: String,
    /// This run's accumulated style facts, including
    /// `facts.pulls_in_unparsed_bytes()` — the answer every entry point
    /// publishes for "does this block declare its whole surface".
    pub facts: VueStyleFacts,
    /// Stage-level failures (as opposed to `facts.refusals`' soft,
    /// individually-tolerated per-selector refusals): the authored-v-bind,
    /// CSS-Modules, or scoped-selector stage could not run at all. The
    /// authored-v-bind stage keeps whatever output preceded it on failure;
    /// the CSS-Modules and scoped-selector stages clear the output to empty
    /// and skip any stage after them, since their output is unsafe to use.
    pub stage_failures: Vec<StyleRewriteFailure>,
}

impl VueStyleCascadeOutcome {
    /// Final code after every requested stage's edits. Byte-identical to the
    /// cascade input when no stage produced any edit.
    #[must_use]
    pub fn code(&self) -> &str {
        self.result.code()
    }
}

/// Reports whether the cascade result can be published without exposing
/// output a stage wiped. The recorded refusal carrier is the authority:
/// [`QualifiedStyleResult::is_refused`] is what the runner wrote when it
/// cleared. An authored-`v-bind()` rewrite failure that left the input
/// intact is not a refusal and stays publishable. A parse miss that never
/// entered a later stage is still unpublished once the runner records
/// [`CascadeOutput::ClearedByRefusal`].
#[must_use]
pub fn cascade_output_is_publishable(
    outcome: &VueStyleCascadeOutcome,
    authored_code: &str,
) -> bool {
    // Emptiness is a shape two outcomes share — a wiped output and an
    // authored `<style></style>` — so publication reads the refusal flag,
    // never `code().is_empty()` and never the failing stage's identity.
    !(outcome.result.is_refused() && !authored_code.is_empty())
}

/// Returns an honest map for a requested cascade result: the accumulated map
/// for rewritten content, an identity map for byte-identical passthrough, or
/// no map when rewritten bytes could not be composed through every rewrite.
///
/// [`build_identity_source_map`] emits a token per authored character, so
/// every generated position resolves to the true authored position — not
/// just chunk/line boundaries.
#[must_use]
pub fn cascade_requested_source_map(
    outcome: &VueStyleCascadeOutcome,
    authored_code: &str,
    source_name: &str,
) -> Option<String> {
    if !outcome.source_map.is_empty() {
        return Some(outcome.source_map.clone());
    }
    if outcome.code() != authored_code {
        return None;
    }
    Some(build_identity_source_map(authored_code, source_name))
}

/// A genuinely byte-accurate identity map: one token per authored
/// character, generated position == authored position at every one.
/// Deliberately not `CodeTransform::generate_map` on a zero-edit transform —
/// that emits tokens only at chunk/line boundaries.
fn build_identity_source_map(source: &str, source_name: &str) -> String {
    let mut tokens: Vec<Token> = Vec::with_capacity(source.chars().count().max(1));
    let mut line = 0u32;
    let mut column = 0u32;
    let mut char_buf = [0u8; 4];
    for ch in source.chars() {
        tokens.push(Token::new(line, column, line, column, Some(0), None));
        advance_generated_position(ch.encode_utf8(&mut char_buf), &mut line, &mut column);
    }
    tokens.push(Token::new(line, column, line, column, Some(0), None));
    SourceMap::new(
        None,
        Vec::new(),
        None,
        vec![std::borrow::Cow::Owned(source_name.to_owned())],
        vec![Some(std::borrow::Cow::Owned(source.to_owned()))],
        tokens.into_boxed_slice(),
        None,
    )
    .to_json_string()
}

/// The map a materialized rewrite produced, if one was requested and built.
///
/// A single terminal `CodeTransform` per cascade run means there is never an
/// accumulated map to compose a later stage onto: every plan carries authored
/// coordinates right up to that one transform, so the map it generates is
/// already authored-source-to-final-code.
type MaterializedMap = Option<Box<SourceMap<'static>>>;

/// Applies a run's collected edits against `code`, returning the new
/// `(code, map)` pair when there was anything to rewrite, or `None` when there
/// was not (in which case the caller keeps the input bytes as its output).
fn apply_cascade_stage(
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    edits: Vec<StyleEdit>,
    want_source_map: bool,
) -> Result<Option<(String, MaterializedMap)>, StyleRewriteFailure> {
    if edits.is_empty() {
        return Ok(None);
    }
    let Some((code, source_map)) = build_transform_output(
        code,
        dialect,
        stage,
        edits,
        want_source_map.then_some(source.source_name),
        want_source_map,
    )?
    else {
        unreachable!("non-empty edits always produce a rewrite")
    };
    Ok(Some((code, source_map.map(Box::new))))
}

/// The stage the bytes a cascade run was handed belong to, with the provenance
/// that stage carries.
///
/// The preprocessed arm names its producer because the cascade did not make
/// those bytes and must not claim to know who did: recording an unrewritten
/// pass-through as [`StyleProducer::ExternalAnonymous`] would say "the tool
/// supplied no identity" about a tool that may well have supplied one further
/// upstream. Only the entry point handed the bytes knows, so it says.
#[derive(Debug, Clone)]
pub enum CascadeInput {
    /// The carrier's own `<style>` bytes, in the authored dialect.
    Authored,
    /// Plain CSS an external preprocessor already produced, and the identity
    /// the caller has for that tool.
    Preprocessed(PreprocessorIdentity),
}

impl CascadeInput {
    const fn stage(&self) -> StyleStage {
        match self {
            Self::Authored => StyleStage::Authored,
            Self::Preprocessed(_) => StyleStage::Preprocessed,
        }
    }
}

/// What a finished cascade run left as its output.
///
/// The three states are genuinely different answers to "who produced these
/// bytes", and the pair `(Option<bytes>, cleared: bool)` they replace could
/// spell a fourth that means nothing. `ClearedByRefusal` in particular is NOT
/// "rewritten to empty": a stage that cannot run safely wipes the output so a
/// half-applied rewrite is never exposed, and nothing produced what is left.
/// Emptiness alone cannot tell the two apart, since an authored
/// `<style></style>` is empty as well.
enum CascadeOutput {
    /// No stage produced bytes; the cascade's input still stands.
    Passthrough,
    /// A stage rewrote the input into these bytes, with the map the one
    /// terminal transform generated.
    Rewritten {
        code: String,
        source_map: MaterializedMap,
    },
    /// A stage refused and wiped the output.
    ClearedByRefusal,
}

/// What recording a refusal does to the run's output.
///
/// Named at every site that records one, so "was the output cleared" is
/// decided where the refusal is raised rather than recovered downstream.
/// Neither half of a recorded failure determines it: the same `AuthoredVBind`
/// stage raising the same `ParseFailure` class clears a `<style module>`
/// request and leaves a v-bind-only request's bytes published, because what
/// differs is what the request needed. A publication read keyed on the class
/// (or on the stage) would therefore be guessing, which is why
/// [`cascade_output_is_publishable`] asks what the runner recorded instead.
#[derive(Clone, Copy)]
enum RefusalEffect {
    /// The cascade's input still stands; only this stage's rewrite was lost.
    KeepsOutput,
    /// The block's meaning depended on a rewrite that could not be planned,
    /// so publishing the un-rewritten bytes would be actively wrong.
    ClearsOutput,
}

/// The refusals one cascade run recorded, with the run's clearing state
/// derived from their effects.
///
/// The clearing state is not a field a caller sets: the only way to reach it
/// is to record a refusal and name its effect, so a run cannot be cleared
/// without the refusal that cleared it, and a clearing refusal cannot be
/// recorded without clearing.
struct StageFailures {
    failures: Vec<StyleRewriteFailure>,
    cleared_by_refusal: bool,
}

impl StageFailures {
    const fn new() -> Self {
        Self {
            failures: Vec::new(),
            cleared_by_refusal: false,
        }
    }

    fn record(&mut self, failure: StyleRewriteFailure, effect: RefusalEffect) {
        self.failures.push(failure);
        self.cleared_by_refusal |= matches!(effect, RefusalEffect::ClearsOutput);
    }

    fn into_vec(self) -> Vec<StyleRewriteFailure> {
        self.failures
    }
}

/// The CSS-only stages one cascade run may plan, after the plain-CSS gate has
/// answered.
///
/// `refusal` is `Some` exactly when the gate refused, and the gate refusing is
/// exactly why `module`/`scoped` are cleared. Holding both in one value is
/// what keeps "which stages were planned" and "was a refusal recorded" from
/// becoming two flags a second entry point could set inconsistently.
struct CssStageRequest {
    module: bool,
    scoped: bool,
    refusal: Option<StyleRewriteFailure>,
}

impl CssStageRequest {
    /// Admit `<style module>` / `<style scoped>` only for bytes that are
    /// already plain CSS.
    ///
    /// [`PlainCssInput::try_new`] is the SOLE predicate. Spelling a second one
    /// here (`dialect == CssDialect::Css`) would make the admitted and the
    /// refused sets complements only by review: a dialect that is neither
    /// plain CSS nor externally preprocessed would be admitted here and
    /// rejected there, skipping the rewrite with no diagnostic.
    fn gated(input: AuthoredStyleInput<'_>, module: bool, scoped: bool) -> Self {
        if !(module || scoped) {
            return Self::admitted(false, false);
        }
        match PlainCssInput::try_new(
            input.code,
            input.dialect,
            input.source.source_name,
            input.source.source_space_token,
            input.source.content_artifact_token,
        ) {
            Ok(_) => Self::admitted(module, scoped),
            Err(refusal) => Self {
                module: false,
                scoped: false,
                refusal: Some(refusal),
            },
        }
    }

    /// Stages over bytes whose plain-CSS grammar the caller already proved —
    /// there is nothing left for the gate to decide.
    const fn admitted(module: bool, scoped: bool) -> Self {
        Self {
            module,
            scoped,
            refusal: None,
        }
    }
}

struct SharedVueStylePlan {
    edits: Vec<StyleEdit>,
    facts: VueStyleFacts,
    terminal_stage: StyleRewriteStage,
    failures: StageFailures,
}

impl SharedVueStylePlan {
    fn refused(
        facts: VueStyleFacts,
        terminal_stage: StyleRewriteStage,
        mut failures: StageFailures,
        failure: StyleRewriteFailure,
    ) -> Self {
        failures.record(failure, RefusalEffect::ClearsOutput);
        Self {
            edits: Vec::new(),
            facts,
            terminal_stage,
            failures,
        }
    }
}

/// The one order a shared plan keeps its edits in, from the moment the first
/// stage's edits are planned to the terminal transform: ascending by start,
/// then by end.
fn edit_order_key(edit: &StyleEdit) -> (u32, u32) {
    (edit.start(), edit.end())
}

/// Edits held in [`edit_order_key`] order AND pairwise disjoint.
///
/// Both halves are ONE type invariant rather than a documented precondition or
/// a debug assertion. Order alone would be the weaker claim: the merge's
/// `Insert` arm inspects only the nearest earlier edit by start offset, and
/// that single candidate is the unique possible container exactly when the
/// stream it scans is disjoint — with disjointness, `prior[i - 2].end <=
/// prior[i - 1].start < at`, so no earlier edit can reach the offset. Carrying
/// disjointness in the type is what stops a future caller from handing that arm
/// an overlapping stream it would silently mis-read; stating the precondition
/// on the arm left it checked by review.
///
/// The only routes in are [`Self::from_planned`] (sorts, then checks) and
/// [`Self::from_ordered`] (checks edits already in plan order), and the only
/// route on is [`merge_shared_stage_edits`], which re-mints through
/// `from_ordered` because a merge composes two disjoint streams and the
/// composition need not be disjoint. A three-stage plan therefore sorts each
/// stage's own edits exactly once and never re-sorts what a previous merge
/// already ordered.
///
/// A debug assertion would not have held it: the workspace's sanctioned
/// assertion macro force-evaluates its condition in every profile, so
/// asserting sortedness here would run the scan in shipped builds too.
///
/// `build_transform_output` keeps its own independent sort-and-check as the
/// backstop for every route, including the direct per-stage transforms that
/// never build a shared plan.
struct PlanDisjointEdits(Vec<StyleEdit>);

impl PlanDisjointEdits {
    /// The plan a stage that refused to run contributes.
    const fn empty() -> Self {
        Self(Vec::new())
    }

    /// Establish the invariant over one stage's freshly planned edits.
    fn from_planned(mut edits: Vec<StyleEdit>) -> Option<Self> {
        edits.sort_by_key(edit_order_key);
        Self::from_ordered(edits)
    }

    /// Establish the invariant over edits already in [`edit_order_key`] order.
    /// Refuses when two edits touch the same bytes; zero-width inserts on an
    /// overwrite boundary are disjoint. Reads the maintained order rather than
    /// restoring it.
    fn from_ordered(edits: Vec<StyleEdit>) -> Option<Self> {
        let mut previous_end = 0;
        for edit in &edits {
            if edit.start() < previous_end {
                return None;
            }
            previous_end = previous_end.max(edit.end());
        }
        Some(Self(edits))
    }

    fn into_vec(self) -> Vec<StyleEdit> {
        self.0
    }
}

fn edit_overwrite_span(edit: &StyleEdit) -> Option<Span> {
    match edit {
        StyleEdit::Overwrite { span, .. } => Some(*span),
        StyleEdit::Insert { .. } => None,
    }
}

fn edit_overlaps_span(edit: &StyleEdit, span: Span) -> bool {
    match edit {
        StyleEdit::Overwrite {
            span: candidate, ..
        } => candidate.start < span.end && span.start < candidate.end,
        StyleEdit::Insert { at, .. } => span.start < *at && *at < span.end,
    }
}

fn span_contains_edit(span: Span, edit: &StyleEdit) -> bool {
    match edit {
        StyleEdit::Overwrite {
            span: candidate, ..
        } => span.start <= candidate.start && candidate.end <= span.end,
        StyleEdit::Insert { at, .. } => span.start <= *at && *at < span.end,
    }
}

/// Merge a later stage into authored-coordinate edits. A later edit wholly
/// inside an earlier overwrite targets source bytes that no longer reach that
/// stage and is discarded. A later deletion may subsume earlier work in bytes
/// it removes. Every other intersection is refused because retaining either
/// edit would misrepresent stage order.
fn merge_shared_stage_edits(
    prior: PlanDisjointEdits,
    later: Vec<StyleEdit>,
) -> Option<PlanDisjointEdits> {
    let PlanDisjointEdits(prior) = prior;
    let mut later = later;
    later.sort_by_key(edit_order_key);
    let mut keep_prior = vec![true; prior.len()];
    let mut keep_later = vec![true; later.len()];
    for (later_index, later_edit) in later.iter().enumerate() {
        let later_span = match later_edit {
            // Only the nearest earlier edit by start offset is inspected. That
            // single candidate is the unique possible container because
            // [`PlanDisjointEdits`] carries `prior`'s disjointness in its type:
            // there is no way to reach this arm with a stream whose earlier
            // edits overlap, so the arm needs no scan of its own.
            StyleEdit::Insert { at, .. } => {
                let prior_index = prior.partition_point(|edit| edit.start() < *at);
                if let Some(candidate) = prior_index.checked_sub(1).and_then(|i| prior.get(i)) {
                    if let Some(span) = edit_overwrite_span(candidate) {
                        if span.start < *at && *at < span.end {
                            keep_later[later_index] = false;
                        }
                    }
                }
                continue;
            }
            StyleEdit::Overwrite { span, .. } => *span,
        };
        let mut prior_index = prior.partition_point(|edit| edit.end() <= later_span.start);
        while let Some(prior_edit) = prior.get(prior_index) {
            if prior_edit.start() >= later_span.end {
                break;
            }
            if !edit_overlaps_span(prior_edit, later_span) {
                prior_index += 1;
            } else if edit_overwrite_span(prior_edit).is_some_and(|prior_span| {
                prior_span.start < later_span.start && later_span.end < prior_span.end
            }) {
                keep_later[later_index] = false;
                break;
            } else if span_contains_edit(later_span, prior_edit)
                && matches!(later_edit, StyleEdit::Overwrite { content, .. } if content.is_empty())
            {
                keep_prior[prior_index] = false;
                prior_index += 1;
            } else {
                return None;
            }
        }
    }

    // Both retained streams are in plan order, so interleaving them yields
    // plan order — this is what makes the returned invariant hold without a
    // second sort.
    let capacity = prior.len() + later.len();
    let mut retained_prior = prior
        .into_iter()
        .zip(keep_prior)
        .filter_map(|(edit, keep)| keep.then_some(edit))
        .peekable();
    let mut retained_later = later
        .into_iter()
        .zip(keep_later)
        .filter_map(|(edit, keep)| keep.then_some(edit))
        .peekable();
    let mut merged = Vec::with_capacity(capacity);
    loop {
        let take_prior = match (retained_prior.peek(), retained_later.peek()) {
            (Some(prior_edit), Some(later_edit)) => {
                edit_order_key(prior_edit) <= edit_order_key(later_edit)
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let next = if take_prior {
            retained_prior.next()
        } else {
            retained_later.next()
        };
        match next {
            Some(edit) => merged.push(edit),
            None => break,
        }
    }
    // `later` carries no disjointness of its own, and a composition of two
    // disjoint streams need not be disjoint, so the invariant is re-established
    // here rather than assumed.
    PlanDisjointEdits::from_ordered(merged)
}

#[cfg(test)]
mod merge_shared_stage_edits_tests {
    use super::{merge_shared_stage_edits, span_contains_edit, PlanDisjointEdits, StyleEdit};
    use verter_span::Span;

    /// Establishes the plan invariant over `prior` the same way
    /// `shared_vue_style_plan` does, then unwraps the merged result so each case
    /// reads as edits in, edits out. `prior` is expected to satisfy the
    /// invariant: a stream that does not cannot reach a merge at all, which is
    /// what `overlapping_prior_edits_cannot_be_minted_into_a_plan` pins.
    fn merge(prior: Vec<StyleEdit>, later: Vec<StyleEdit>) -> Option<Vec<StyleEdit>> {
        let prior = PlanDisjointEdits::from_planned(prior)
            .expect("a merge's prior stream is disjoint by construction");
        merge_shared_stage_edits(prior, later).map(PlanDisjointEdits::into_vec)
    }

    fn overwrite(start: u32, end: u32, content: &str) -> StyleEdit {
        StyleEdit::Overwrite {
            span: Span::new(start, end),
            content: content.to_string(),
        }
    }

    fn insert(at: u32, content: &str) -> StyleEdit {
        StyleEdit::Insert {
            at,
            content: content.to_string(),
        }
    }

    /// Path 1: an earlier overwrite strictly containing a later edit discards
    /// the later edit — the later stage targeted bytes the earlier stage
    /// already replaced.
    ///
    /// Mutation recipe: drop the `keep_later[later_index] = false` in the
    /// strictly-contained overwrite arm — the merge then keeps both edits and
    /// `PlanDisjointEdits::from_ordered` refuses, so `expect` panics.
    #[test]
    fn earlier_overwrite_strictly_containing_later_edit_discards_the_later_edit() {
        let prior = vec![overwrite(0, 20, "var(--sc1-tone)")];
        let later = vec![overwrite(5, 10, ".hashed")];

        let merged = merge(prior.clone(), later.clone())
            .expect("a later edit inside an earlier overwrite must not refuse the merge");

        assert_eq!(merged, prior, "only the earlier overwrite survives");
    }

    /// Path 2: a later deletion (an empty-content overwrite) strictly
    /// containing one or more earlier edits discards those earlier edits
    /// instead — the later stage rewrote the whole region away, so the
    /// earlier work inside it never reaches the source.
    ///
    /// Mutation recipe: drop `keep_prior[prior_index] = false` from that arm —
    /// the earlier edits survive inside the deletion, the merged vector is no
    /// longer disjoint, and `expect` panics.
    #[test]
    fn later_empty_overwrite_strictly_containing_earlier_edits_discards_the_earlier_edits() {
        let prior = vec![overwrite(5, 10, "one"), overwrite(12, 15, "two")];
        let later = vec![overwrite(0, 20, "")];

        let merged = merge(prior.clone(), later.clone())
            .expect("a later deletion subsuming earlier edits must not refuse the merge");

        assert_eq!(
            merged, later,
            "only the later deletion survives; both earlier edits are discarded"
        );
    }

    /// Path 2's boundary complement: a deletion `[start, end)` removes no byte
    /// at `end`, so an earlier insert positioned exactly at the deletion's end
    /// addresses the gap AFTER the removed bytes and must survive the merge.
    /// This is the shape a later deletion would see the moment any stage
    /// deletion follows a scope-attribute insert.
    ///
    /// Mutation recipe: the boundary insert is shielded by two independent
    /// rails, and the recipe has to break both. Widen `edit_overlaps_span`'s
    /// insert arm to the closed `span.start <= *at && *at <= span.end` AND
    /// relax the scan's break to `prior_edit.start() > later_span.end` — the
    /// boundary insert is then read as overlapping the deletion, the half-open
    /// containment arm no longer subsumes it, and the merge refuses instead of
    /// keeping it. Widening the overlap arm alone cannot bite: the scan breaks
    /// on `insert.start() == later_span.end` before overlap is consulted.
    #[test]
    fn earlier_insert_at_a_later_deletions_end_survives() {
        let prior = vec![insert(20, "[data-v-sc1]")];
        let later = vec![overwrite(0, 20, "")];

        let merged = merge(prior.clone(), later.clone())
            .expect("an insert at a deletion's end offset must not refuse the merge");

        assert_eq!(
            merged,
            vec![later[0].clone(), prior[0].clone()],
            "the deletion survives and the boundary insert is kept, in plan order"
        );
    }

    /// A later deletion that only partially overlaps an earlier edit — never
    /// strictly containing it — is neither path 1 nor path 2, so the merge
    /// refuses rather than silently dropping or keeping either edit.
    ///
    /// Two independent rails refuse it, and the recipe has to break both: the
    /// classifier's trailing `return None`, and the re-mint of the merged
    /// vector through `PlanDisjointEdits::from_ordered`.
    ///
    /// Mutation recipe: replace that `return None` with `prior_index += 1` AND
    /// make `PlanDisjointEdits::from_ordered` return `Some(Self(edits))`
    /// unconditionally — the merge then hands the terminal transform two edits
    /// whose spans overlap. Breaking either rail alone still refuses.
    #[test]
    fn partially_overlapping_edits_refuse_the_merge() {
        let prior = vec![overwrite(0, 10, "one")];
        let later = vec![overwrite(5, 15, "")];

        assert!(
            merge(prior, later).is_none(),
            "a partial overlap must refuse rather than guess an ordering"
        );
    }

    /// The classifier only ever compares a LATER edit against the prior ones,
    /// so two prior edits that overlap each other would pass it untouched — and
    /// the `Insert` arm reads only the nearest prior edit by start offset, so it
    /// would mis-read them rather than refuse. Nothing downstream can catch that,
    /// which is why the mint is where it is refused: a stream whose own edits
    /// overlap can never become a [`PlanDisjointEdits`], so it can never be
    /// handed to a merge at all.
    ///
    /// Mutation recipe: make `PlanDisjointEdits::from_ordered` return
    /// `Some(Self(edits))` unconditionally — the overlapping pair mints a plan
    /// and this case resolves to a `Some` it must not.
    #[test]
    fn overlapping_prior_edits_cannot_be_minted_into_a_plan() {
        let overlapping = vec![overwrite(0, 10, "one"), overwrite(5, 15, "two")];

        assert!(
            PlanDisjointEdits::from_planned(overlapping).is_none(),
            "overlapping edits must never become a plan a merge can read"
        );
    }

    /// The re-mint over the merged vector is not redundant with the classifier
    /// either: `later` carries no invariant of its own, so two LATER edits that
    /// overlap each other reach the merged vector untouched. Only the re-mint
    /// sees them, and it must refuse — handing the terminal transform
    /// overlapping spans is the half-ordered rewrite the whole merge exists to
    /// prevent.
    ///
    /// Mutation recipe: make `PlanDisjointEdits::from_ordered` return
    /// `Some(Self(edits))` unconditionally — every other merge case still
    /// refuses through the classifier, and only this one resolves to a `Some`
    /// it must not.
    #[test]
    fn overlapping_later_edits_refuse_even_when_the_prior_plan_is_disjoint() {
        // Neither rail below inspects this pair: the classifier walks `later`
        // against `prior`, never `later` against itself.
        let prior = vec![insert(40, "[data-v-sc1]")];
        let later = vec![overwrite(0, 10, "one"), overwrite(5, 15, "two")];

        assert!(
            merge(prior, later).is_none(),
            "overlapping edits must never reach the terminal transform"
        );
    }

    /// The containment scan reads the NEAREST earlier edit by start offset, so
    /// it has to find the right container when several prior edits precede the
    /// insertion point. That is exactly what [`PlanDisjointEdits`] buys the arm:
    /// with `prior` disjoint, the nearest-by-start edit is the only one that can
    /// contain the offset, so one candidate is enough.
    /// A scan that read a fixed prior edit instead would splice a scope
    /// attribute into the middle of a `v-bind()` replacement whenever any
    /// unrelated rewrite came first.
    ///
    /// Mutation recipe: replace the `partition_point` lookup with a fixed
    /// index — the interior insert is no longer recognised as contained, and
    /// this merge hands back three edits instead of two.
    #[test]
    fn later_insert_finds_its_container_past_an_unrelated_earlier_overwrite() {
        let prior = vec![
            overwrite(0, 10, "var(--sc1-tone)"),
            overwrite(20, 40, ".hashed"),
        ];
        let later = vec![insert(30, "[data-v-sc1]")];

        let merged = merge(prior.clone(), later)
            .expect("an insert inside the later overwrite must not refuse the merge");

        assert_eq!(
            merged, prior,
            "the interior insert is dropped and both overwrites survive"
        );
    }

    /// Only a DELETION may subsume earlier work. A later overwrite that
    /// contains an earlier edit but writes bytes of its own has no defined
    /// composition — keeping the earlier edit would apply it to bytes the
    /// later stage replaced, dropping it would silently lose that stage's
    /// rewrite — so the merge refuses instead of choosing.
    ///
    /// Mutation recipe: weaken the arm's
    /// `matches!(later_edit, StyleEdit::Overwrite { content, .. } if content.is_empty())`
    /// guard to `matches!(later_edit, StyleEdit::Overwrite { .. })` — the
    /// earlier edit is silently discarded and this merge resolves.
    #[test]
    fn later_non_empty_overwrite_containing_an_earlier_edit_refuses_the_merge() {
        let prior = vec![overwrite(5, 10, "var(--sc1-tone)")];
        let later = vec![overwrite(0, 20, ".hashed { color: red; }")];

        assert!(
            merge(prior, later).is_none(),
            "only an empty later overwrite (a deletion) may subsume earlier edits"
        );
    }

    /// Path 1 for the insert shape: a later insertion point strictly inside an
    /// earlier overwrite addresses bytes that overwrite already replaced, so
    /// it is discarded. A scope-attribute insert landing inside a `v-bind()`
    /// replacement is the live shape — keeping it would splice the attribute
    /// into the middle of `var(--sc1-tone)`.
    ///
    /// Mutation recipe: delete the `keep_later[later_index] = false` in the
    /// `StyleEdit::Insert` arm — the insert survives and the merge hands back
    /// two edits instead of one.
    #[test]
    fn later_insert_strictly_inside_an_earlier_overwrite_is_discarded() {
        let prior = vec![overwrite(0, 20, "var(--sc1-tone)")];
        let later = vec![insert(10, "[data-v-sc1]")];

        let merged = merge(prior.clone(), later)
            .expect("an insert inside an earlier overwrite must not refuse the merge");

        assert_eq!(
            merged, prior,
            "only the earlier overwrite survives; the interior insert is dropped"
        );
    }

    /// The boundaries are not interior. An insert exactly at an overwrite's
    /// start or end still addresses live authored bytes, so both survive —
    /// which is what keeps the discard rule from eating the common
    /// scope-attribute insert that abuts a rewritten selector.
    ///
    /// Mutation recipe: widen the interior test to `span.start <= *at && *at <=
    /// span.end` and both boundary inserts vanish from the merge.
    #[test]
    fn later_insert_on_an_earlier_overwrite_boundary_survives() {
        for at in [0, 20] {
            let prior = vec![overwrite(0, 20, "var(--sc1-tone)")];
            let later = vec![insert(at, "[data-v-sc1]")];

            let merged =
                merge(prior, later.clone()).expect("a boundary insert must not refuse the merge");

            assert!(
                merged.contains(&later[0]),
                "an insert at offset {at} abuts the overwrite rather than \
                 sitting inside it: {merged:?}"
            );
            assert_eq!(merged.len(), 2, "{merged:?}");
        }
    }

    /// The insert scan reads the nearest earlier edit by start offset, so an
    /// insert must survive an unrelated overwrite that merely precedes it.
    ///
    /// Mutation recipe: drop the `edit_overwrite_span` containment test in the
    /// insert arm and every insert following any overwrite is discarded.
    #[test]
    fn later_insert_after_a_disjoint_earlier_overwrite_survives() {
        let prior = vec![overwrite(0, 10, "var(--sc1-tone)")];
        let later = vec![insert(30, "[data-v-sc1]")];

        let merged = merge(prior.clone(), later.clone())
            .expect("a disjoint insert must not refuse the merge");

        assert_eq!(merged, vec![prior[0].clone(), later[0].clone()]);
    }

    /// Deletion subsumption reads inserts against the same half-open span the
    /// rest of the module does: an insert is contained only strictly interior,
    /// because `[start, end)` owns no position at `end`. The predicate is
    /// reachable only behind `edit_overlaps_span`'s strictly-interior insert
    /// guard, so no merge case can distinguish the end edge — it is pinned
    /// here, against the predicate itself. The left edge stays closed to
    /// mirror the overwrite arm's `span.start <= candidate.start`.
    ///
    /// Mutation recipe: revert the arm to `*at <= span.end` — the
    /// end-boundary assertion fails.
    #[test]
    fn span_containment_for_inserts_ends_at_the_half_open_edge() {
        let deletion = Span::new(0, 20);

        assert!(
            span_contains_edit(deletion, &insert(10, "[data-v-sc1]")),
            "an insert strictly inside the deleted bytes is subsumed"
        );
        assert!(
            !span_contains_edit(deletion, &insert(20, "[data-v-sc1]")),
            "position 20 is the gap after the deleted bytes, not a deleted byte"
        );
    }
}

/// Build every compatible Vue rewrite from one syntax IR. The plans retain
/// authored coordinates until the terminal `CodeTransform`, so compatible
/// stages need neither an intermediate stylesheet nor a second parse.
fn shared_vue_style_plan(
    ir: &ParsedStyleIr,
    dialect: CssDialect,
    scope_id: &str,
    module: bool,
    scoped: bool,
) -> SharedVueStylePlan {
    let origin = ir.source().origin();
    let terminal_stage = if scoped {
        StyleRewriteStage::PostPreprocessScoping
    } else if module {
        StyleRewriteStage::PostPreprocessModules
    } else {
        StyleRewriteStage::AuthoredVBind
    };
    let mut facts = VueStyleFacts::default();
    // Recorded from the parse, before any stage plans an edit. Every route
    // into a plan lands here, so the derived "does this block declare its
    // whole surface" answer cannot depend on which entry point the same
    // request took, and a stage that refuses to plan does not un-survey a
    // parse that already read the block's inclusions.
    record_input_dependencies(&mut facts, ir);
    let mut failures = StageFailures::new();

    observe_style_ir(StyleRewriteStage::AuthoredVBind, ir);
    let mut edits = match plan_authored_v_bind_edits(ir, dialect, scope_id) {
        Ok((edits, v_bind_vars)) => {
            facts.v_bind_vars = v_bind_vars;
            facts.rewrites.v_bind = !edits.is_empty();
            // The one sort of this stage's own edits, and the one place the
            // plan's order-and-disjointness invariant is established. Every
            // merge below re-establishes it over its own composition, so no
            // later gate has to re-derive it.
            match PlanDisjointEdits::from_planned(edits) {
                Some(edits) => edits,
                None => {
                    return SharedVueStylePlan::refused(
                        facts,
                        terminal_stage,
                        failures,
                        StyleRewriteFailure::new(
                            StyleRewriteFailureClass::OverlappingEdits,
                            StyleRewriteStage::AuthoredVBind,
                            dialect,
                            None,
                        ),
                    );
                }
            }
        }
        Err(failure) => {
            // The cascade's input bytes still stand, and every later stage
            // plans those same bytes, so a refusal here costs the `v-bind()`
            // lowering and nothing else.
            failures.record(failure, RefusalEffect::KeepsOutput);
            PlanDisjointEdits::empty()
        }
    };

    if module {
        observe_style_ir(StyleRewriteStage::PostPreprocessModules, ir);
        let (module_edits, classes) =
            match module_classes_and_edits_from_ir(ir, CssDialect::Css, scope_id) {
                Ok(planned) => planned,
                Err(failure) => {
                    return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
                }
            };
        facts.rewrites.css_modules |= !module_edits.is_empty();
        facts.module_classes.extend(classes);
        // Relocated here, with the stage that actually produced these edits,
        // so a `checked_sub` underflow on this group is reported against
        // `PostPreprocessModules` rather than borrowing whichever stage
        // happens to be terminal.
        let module_edits = match relocate_edits(
            module_edits,
            origin,
            StyleRewriteStage::PostPreprocessModules,
            dialect,
        ) {
            Ok(edits) => edits,
            Err(failure) => {
                return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
            }
        };
        let Some(merged) = merge_shared_stage_edits(edits, module_edits) else {
            return SharedVueStylePlan::refused(
                facts,
                terminal_stage,
                failures,
                StyleRewriteFailure::new(
                    StyleRewriteFailureClass::OverlappingEdits,
                    StyleRewriteStage::PostPreprocessModules,
                    dialect,
                    None,
                ),
            );
        };
        edits = merged;
    }
    if scoped {
        observe_style_ir(StyleRewriteStage::PostPreprocessScoping, ir);
        let (scope_edits, scope_facts) = match scoped_edits_and_facts_from_ir(ir, scope_id) {
            Ok(planned) => planned,
            Err(failure) => {
                return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
            }
        };
        // Accumulated, never assigned: a plan folds every stage's observations
        // into one fact set, so a stage that arrives later must not be able to
        // clear what an earlier one recorded. Today no earlier stage sets these
        // fields, which is exactly why an assignment would look correct right
        // up until one does.
        facts.rewrites.deep |= scope_facts.rewrites.deep;
        facts.rewrites.slotted |= scope_facts.rewrites.slotted;
        facts.rewrites.global |= scope_facts.rewrites.global;
        facts.rewrites.keyframes |= scope_facts.rewrites.keyframes;
        facts.rewrites.scoped_selector |= scope_facts.rewrites.scoped_selector;
        facts.refusals.extend(scope_facts.refusals);
        // Relocated here, with the stage that actually produced these edits —
        // see the module-edit relocation above.
        let scope_edits = match relocate_edits(
            scope_edits,
            origin,
            StyleRewriteStage::PostPreprocessScoping,
            dialect,
        ) {
            Ok(edits) => edits,
            Err(failure) => {
                return SharedVueStylePlan::refused(facts, terminal_stage, failures, failure);
            }
        };
        let Some(merged) = merge_shared_stage_edits(edits, scope_edits) else {
            return SharedVueStylePlan::refused(
                facts,
                terminal_stage,
                failures,
                StyleRewriteFailure::new(
                    StyleRewriteFailureClass::OverlappingEdits,
                    StyleRewriteStage::PostPreprocessScoping,
                    dialect,
                    None,
                ),
            );
        };
        edits = merged;
    }

    // No terminal disjointness gate: `edits` is a [`PlanDisjointEdits`], so
    // every route that produced it already refused rather than composed
    // overlapping spans.
    SharedVueStylePlan {
        edits: edits.into_vec(),
        facts,
        terminal_stage,
        failures,
    }
}

/// The ONE route from a parsed IR to a finished cascade outcome. Every entry
/// point — authored-only, the full authored cascade, and the verified plain-CSS
/// one — reaches materialization through here, so a request that names the same
/// stages is answered the same way whatever spelled it.
#[allow(clippy::too_many_arguments)]
fn run_shared_vue_style_plan(
    ir: &ParsedStyleIr,
    input: CascadeInput,
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    scope_id: &str,
    css: CssStageRequest,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    let SharedVueStylePlan {
        edits,
        facts,
        terminal_stage,
        mut failures,
    } = shared_vue_style_plan(ir, dialect, scope_id, css.module, css.scoped);
    // Recorded before materialization, not after: a gate refusal already
    // dropped the stages the request's meaning depended on, so building a
    // transform whose output is about to be wiped would be work nobody reads.
    if let Some(refusal) = css.refusal {
        failures.record(refusal, RefusalEffect::ClearsOutput);
    }
    let output = if failures.cleared_by_refusal {
        CascadeOutput::ClearedByRefusal
    } else {
        match apply_cascade_stage(
            code,
            source,
            dialect,
            terminal_stage,
            edits,
            want_source_map,
        ) {
            Ok(Some((code, source_map))) => CascadeOutput::Rewritten { code, source_map },
            Ok(None) => CascadeOutput::Passthrough,
            Err(failure) => {
                // The terminal transform materializes the WHOLE plan, so a
                // failure here loses every stage's rewrite at once and leaves
                // nothing safe to publish.
                failures.record(failure, RefusalEffect::ClearsOutput);
                CascadeOutput::ClearedByRefusal
            }
        }
    };
    finish_vue_style_cascade(input, dialect, code, output, facts, failures.into_vec())
}

/// Assemble the one cascade outcome shape from a finished run.
///
/// Every cascade entry point lands here, so the rule that decides which byte
/// space the result belongs to — and therefore which space each refusal's span
/// addresses — is written once rather than re-derived per entry point.
///
/// `input` is the stage the cascade was handed: authored bytes for a carrier's
/// own `<style>` content, preprocessed bytes when an external tool produced
/// them. A run that changed nothing stays at that stage; a run that produced
/// output is a framework-rewritten result.
///
/// Shared planning hands every compatible stage the same cascade-input IR, so
/// a later-stage refusal's span always addresses the cascade's own input
/// space — authored carrier bytes or admitted preprocessed CSS — never a
/// later rewrite that has not been materialized yet. Clearing the output
/// after a refusal does not move the bytes the refusal was reported against.
fn finish_vue_style_cascade(
    input: CascadeInput,
    dialect: CssDialect,
    input_code: &str,
    output: CascadeOutput,
    facts: VueStyleFacts,
    stage_failures: Vec<StyleRewriteFailure>,
) -> VueStyleCascadeOutcome {
    let input_stage = input.stage();
    // Projected once, here, for every entry point. This is the SOLE
    // publication route (see `VueStyleCascadeOutcome.result`), so doing it
    // per-consumer instead would re-derive the same strings once per reader
    // and lose the single-route property that keeps a refusal from being
    // reported twice. A run with no refusals collects an empty `Vec`, which
    // allocates nothing — the cost is exactly one projection per refusal, and
    // only sheets that actually refused pay it.
    let diagnostics: Vec<StyleDiagnostic> = facts
        .refusals
        .iter()
        .chain(stage_failures.iter())
        .map(|failure| failure.to_diagnostic(input_stage))
        .collect();
    // The output state carries the bytes that state owns, so each arm reads
    // its own — a rewrite cannot reach for bytes a passthrough never minted.
    let (result, source_map) = match output {
        // Nothing produced these (absent) bytes, so nothing claims them. The
        // stage still names the space the refusals' own coordinates belong
        // to, which is what makes them placeable.
        CascadeOutput::ClearedByRefusal => (
            QualifiedStyleResult::refused(input_stage, dialect, diagnostics),
            String::new(),
        ),
        CascadeOutput::Rewritten { code, source_map } => (
            QualifiedStyleResult::framework_rewritten(dialect, code, diagnostics),
            source_map
                .map(|map| map.to_json_string())
                .unwrap_or_default(),
        ),
        CascadeOutput::Passthrough => {
            let result = match input {
                CascadeInput::Authored => {
                    QualifiedStyleResult::authored(dialect, input_code, diagnostics)
                }
                CascadeInput::Preprocessed(producer) => {
                    QualifiedStyleResult::preprocessed(producer, input_code, diagnostics)
                }
            };
            (result, String::new())
        }
    };
    VueStyleCascadeOutcome {
        result,
        source_map,
        facts,
        stage_failures,
    }
}

/// Runs only Vue's authored-dialect `v-bind()` stage.
///
/// This is the explicit entry point for a bundler Main render whose separate
/// style-module pipeline owns preprocessing and every plain-CSS-only stage.
/// It never requests CSS Modules or selector scoping, so authored SCSS/Sass/
/// Less/Stylus cannot be mistaken for completed CSS. The returned facts still
/// drive `_useCssVars` generation in the runtime module.
///
/// Literally [`run_vue_style_cascade`] with neither attribute set: those are
/// the same request — `v-bind()` lowering and nothing else — and delegating
/// rather than re-deriving it is what keeps the two entry points from
/// answering it differently. A second route here diverged on exactly one
/// recorded fact (whether a parse that survived but whose `v-bind()` planning
/// refused had surveyed the block's inclusions), which no signature could have
/// caught.
pub fn run_vue_style_authored_only(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    run_vue_style_cascade(input, scope_id, false, false, want_source_map)
}

/// Runs Vue's authored-v-bind → CSS-Modules → scoped-selector cascade.
/// Compatible stages derive authored-coordinate edits from one
/// `StyleSyntaxIr` and materialize them once. A plan whose edits cannot be
/// composed returns a terminal refusal without materializing partial output
/// or entering a staged fallback. Every CSS input — including v-bind-only —
/// takes that shared plan, with a terminal stage naming the last stage that
/// actually ran. `module`/`scoped` mirror the SFC's
/// `<style module>`/`<style scoped>` attributes; a non-CSS dialect with
/// either attribute set refuses at the [`PlainCssInput`] gate and never
/// parses those bytes as CSS.
///
/// A stage that cannot safely run does not abort the whole cascade — see
/// [`VueStyleCascadeOutcome::stage_failures`]. The authored-v-bind stage
/// runs against the authored bytes regardless of whether it itself
/// succeeds, so a v-bind rewrite failure still lets CSS-Modules/scoped-selector
/// plan those same authored bytes.
///
/// A parse miss is recorded exactly once, by the parse that ran. What it does
/// to the output depends on what was asked for, and the answer is the same one
/// [`run_vue_style_authored_only`] gives, because they are the same request:
/// with neither `module` nor `scoped` the only work was `v-bind()` lowering,
/// nothing was rewritten, and the authored bytes are published beside the
/// diagnostic rather than deleted. With either attribute set the block's
/// meaning depends on a rewrite that could not be planned — unhashed class
/// names, or selectors that would apply to the whole document instead of this
/// component — so publishing the authored bytes would be actively wrong and
/// the output is cleared.
pub fn run_vue_style_cascade(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    verter_audit::attribute_scope!(CssTransform);
    // One gate for every dialect, answered before anything is parsed. A
    // non-CSS `<style module>`/`<style scoped>` loses those stages here and
    // records the refusal that lost them; a v-bind-only request never consults
    // it. There is no second branch to keep in step with this one.
    let css = CssStageRequest::gated(input, module, scoped);
    match authored_or_parsed_ir(input, StyleRewriteStage::AuthoredVBind) {
        Ok(parsed) => run_shared_vue_style_plan(
            &parsed,
            CascadeInput::Authored,
            input.code,
            input.source,
            input.dialect,
            scope_id,
            css,
            want_source_map,
        ),
        Err(failure) => {
            // Nothing parsed, so nothing planned. See this function's doc
            // comment: only a request whose meaning depends on a CSS rewrite
            // clears on a parse miss.
            let mut failures = StageFailures::new();
            failures.record(
                failure,
                if module || scoped {
                    RefusalEffect::ClearsOutput
                } else {
                    RefusalEffect::KeepsOutput
                },
            );
            if let Some(refusal) = css.refusal {
                failures.record(refusal, RefusalEffect::ClearsOutput);
            }
            finish_vue_style_cascade(
                CascadeInput::Authored,
                input.dialect,
                input.code,
                if failures.cleared_by_refusal {
                    CascadeOutput::ClearedByRefusal
                } else {
                    CascadeOutput::Passthrough
                },
                VueStyleFacts::default(),
                failures.into_vec(),
            )
        }
    }
}

/// Runs the full Vue style cascade from native-CSS grammar provenance.
///
/// The supplied parse is the cascade's only CSS parse. Compatible stages
/// plan over it and materialize once. `input_stage` names where these bytes
/// came from. Plain CSS reaching this entry is either a carrier's own
/// authored CSS or an external preprocessor's output, and only the caller
/// knows which — the bytes look the same either way. It is a parameter
/// rather than an inference because guessing writes the wrong provenance
/// onto every result and, worse, the wrong coordinate space onto every
/// refusal.
///
/// The authored-`v-bind()` stage runs on both: a preprocessor leaves
/// `v-bind()` untouched in its output, so lowering it is still this cascade's
/// job. It simply runs against preprocessed bytes, and the recorded stage says
/// so.
#[allow(clippy::too_many_arguments)]
pub fn run_vue_style_cascade_verified(
    verified: VerifiedPlainCss<'_>,
    input_stage: CascadeInput,
    source_name: &str,
    source_space_token: &str,
    content_artifact_token: &str,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    verter_audit::attribute_scope!(CssTransform);
    let code = verified.code();
    let source = StyleSourceIdentity {
        source_name,
        source_space_token,
        content_artifact_token,
    };
    let parsed = ParsedStyleIr::from_existing(verified.ir.clone());
    run_shared_vue_style_plan(
        &parsed,
        input_stage,
        code,
        source,
        CssDialect::Css,
        scope_id,
        // `VerifiedPlainCss` already carries the plain-CSS grammar proof the
        // authored gate exists to establish, so there is nothing left to gate.
        CssStageRequest::admitted(module, scoped),
        want_source_map,
    )
}

/// Type-state-gated entry point for Vue transforms over CSS-grammar-proven
/// bytes.
#[allow(clippy::too_many_arguments)]
pub fn transform_vue_style(
    verified: VerifiedPlainCss<'_>,
    input_stage: CascadeInput,
    source_name: &str,
    source_space_token: &str,
    content_artifact_token: &str,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    run_vue_style_cascade_verified(
        verified,
        input_stage,
        source_name,
        source_space_token,
        content_artifact_token,
        scope_id,
        module,
        scoped,
        want_source_map,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VueSpecialPseudo {
    Global,
    Deep,
    Slotted,
}

fn component_ident_eq(source: &CssSource, component: &SelectorComponent, name: &str) -> bool {
    component.name_span().is_some_and(|span| {
        css_identifier_eq_ignore_ascii_case(source.slice(span).trim_end_matches('('), name)
    })
}

struct VueScopePlanner<'a> {
    source: &'a CssSource,
    scope_attr: String,
    slotted_attr: String,
    scope_id: &'a str,
    edits: Vec<StyleEdit>,
    facts: VueStyleFacts,
    keyframes: Vec<(String, String)>,
}

impl VueScopePlanner<'_> {
    fn collect_keyframes(
        &mut self,
        statements: &[StyleStatement],
    ) -> Result<(), StyleRewriteFailure> {
        for statement in statements {
            match statement {
                StyleStatement::AtRule(rule) => {
                    if self.is_keyframes(rule) {
                        if rule.completeness() != StyleCompleteness::Complete
                            || rule.opaque_args().completeness() != StyleCompleteness::Complete
                        {
                            return Err(self.untrusted(rule.span()));
                        }
                        let name = rule
                            .opaque_args()
                            .values()
                            .iter()
                            .find_map(|value| match value {
                                ComponentValue::Token(token)
                                    if token.kind() == TokenKind::Ident =>
                                {
                                    Some((self.source.slice(token.span()), token.span()))
                                }
                                _ => None,
                            })
                            .ok_or_else(|| self.untrusted(rule.head_span()))?;
                        let renamed = format!("{}-{}", name.0, self.scope_id);
                        self.edits.push(StyleEdit::Overwrite {
                            span: name.1,
                            content: renamed.clone(),
                        });
                        self.keyframes.push((name.0.to_string(), renamed));
                        self.facts.rewrites.keyframes = true;
                    }
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
                StyleStatement::Rule(rule) => self.collect_keyframes(rule.body().statements())?,
                StyleStatement::Declaration(rule) => {
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
                StyleStatement::MixinOrFunction(rule) => {
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
                StyleStatement::Unknown(rule) => {
                    if let Some(body) = rule.body() {
                        self.collect_keyframes(body.statements())?;
                    }
                }
            }
        }
        Ok(())
    }

    fn plan_statements(
        &mut self,
        statements: &[StyleStatement],
        inside_keyframes: bool,
    ) -> Result<(), StyleRewriteFailure> {
        for statement in statements {
            match statement {
                StyleStatement::Rule(rule) => {
                    let edits_checkpoint = self.edits.len();
                    let facts_checkpoint = self.facts.clone();
                    let refusal_checkpoint = facts_checkpoint.refusals.len();
                    let planned = (|| {
                        if !inside_keyframes {
                            if rule.completeness() != StyleCompleteness::Complete
                                || !selector_list_is_trusted_for_scoping(rule.selector_list())
                            {
                                return Err(self.untrusted(rule.selector_list().span()));
                            }
                            for selector in rule.selector_list().selectors() {
                                self.plan_selector(selector)?;
                            }
                        }
                        self.plan_statements(rule.body().statements(), inside_keyframes)?;
                        if self.facts.refusals.len() > refusal_checkpoint {
                            return Err(self
                                .facts
                                .refusals
                                .last()
                                .cloned()
                                .unwrap_or_else(|| self.untrusted(rule.span())));
                        }
                        Ok(())
                    })();
                    if let Err(refusal) = planned {
                        self.edits.truncate(edits_checkpoint);
                        self.facts = facts_checkpoint;
                        self.edits.push(StyleEdit::Overwrite {
                            span: rule.span(),
                            content: String::new(),
                        });
                        self.facts.refusals.push(refusal);
                    }
                }
                StyleStatement::Declaration(declaration) => {
                    let edits_checkpoint = self.edits.len();
                    let facts_checkpoint = self.facts.clone();
                    let planned = (|| {
                        self.plan_animation_declaration(declaration)?;
                        if let Some(body) = declaration.body() {
                            self.plan_statements(body.statements(), inside_keyframes)?;
                        }
                        Ok(())
                    })();
                    if let Err(refusal) = planned {
                        self.edits.truncate(edits_checkpoint);
                        self.facts = facts_checkpoint;
                        self.edits.push(StyleEdit::Overwrite {
                            span: declaration.span(),
                            content: String::new(),
                        });
                        self.facts.refusals.push(refusal);
                    }
                }
                StyleStatement::AtRule(rule) => {
                    if let Some(body) = rule.body() {
                        let edits_checkpoint = self.edits.len();
                        let facts_checkpoint = self.facts.clone();
                        if let Err(refusal) = self.plan_statements(
                            body.statements(),
                            inside_keyframes || self.is_keyframes(rule),
                        ) {
                            self.edits.truncate(edits_checkpoint);
                            self.facts = facts_checkpoint;
                            self.edits.push(StyleEdit::Overwrite {
                                span: rule.span(),
                                content: String::new(),
                            });
                            self.facts.refusals.push(refusal);
                        }
                    }
                }
                StyleStatement::MixinOrFunction(rule) => {
                    if let Some(body) = rule.body() {
                        self.plan_statements(body.statements(), inside_keyframes)?;
                    }
                }
                StyleStatement::Unknown(unknown) => {
                    if !inside_keyframes && unknown_may_contain_selector(unknown) {
                        return Err(self.untrusted(unknown.span()));
                    }
                    if let Some(body) = unknown.body() {
                        self.plan_statements(body.statements(), inside_keyframes)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn plan_selector(&mut self, selector: &ComplexSelector) -> Result<(), StyleRewriteFailure> {
        if !selector_is_trusted_for_scoping(selector) {
            return Err(self.untrusted(selector.span()));
        }
        let checkpoint = self.edits.len();
        let mut edits = std::mem::take(&mut self.edits);
        let has_special = self.collect_special_selector_edits(selector, &mut edits)?;
        if has_special {
            self.edits = edits;
            self.facts.rewrites.scoped_selector = true;
            return Ok(());
        }
        edits.truncate(checkpoint);
        let scope_attr = self.scope_attr.clone();
        let planned = self.collect_selector_scope_edits(selector, &scope_attr, &mut edits);
        self.edits = edits;
        planned?;
        self.facts.rewrites.scoped_selector = true;
        Ok(())
    }

    fn collect_special_selector_edits(
        &mut self,
        selector: &ComplexSelector,
        edits: &mut Vec<StyleEdit>,
    ) -> Result<bool, StyleRewriteFailure> {
        let mut found = false;
        let edits_checkpoint = edits.len();
        let mut previous_compound = None;
        let parts = selector.parts();
        let mut index = 0;
        while index < parts.len() {
            let part = &parts[index];
            match part {
                ComplexSelectorPart::Compound(compound) => {
                    for (component_index, component) in compound.components().iter().enumerate() {
                        let Some(pseudo) = component.pseudo() else {
                            continue;
                        };
                        let special_name = self.vue_special_pseudo_name(component);
                        if special_name == Some(VueSpecialPseudo::Global) {
                            // `:global()` with nothing to unwrap is a refusal,
                            // not an erasure: dropping the selector would leave
                            // the declarations attached to whatever followed.
                            let argument = self.trusted_first_selector_argument(pseudo)?;
                            edits.truncate(edits_checkpoint);
                            self.collect_unwrapped_argument_edits(
                                selector_content_span(selector),
                                argument.span(),
                                String::new(),
                                edits,
                            );
                            self.facts.rewrites.global = true;
                            return Ok(true);
                        }
                        if special_name == Some(VueSpecialPseudo::Deep) {
                            let argument = self.trusted_selector_argument_allowing_empty(pseudo)?;
                            let same_compound_anchor = component_index > 0;
                            let anchor = same_compound_anchor
                                .then_some(compound)
                                .or(previous_compound);
                            let prefix = if let Some(anchor) = anchor {
                                edits.push(StyleEdit::Insert {
                                    at: self.scope_insertion(anchor),
                                    content: self.scope_attr.clone(),
                                });
                                if same_compound_anchor {
                                    " ".to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                format!("{} ", self.scope_attr)
                            };
                            if let Some(argument) = argument {
                                self.collect_unwrapped_argument_edits(
                                    component.span(),
                                    argument.span(),
                                    prefix,
                                    edits,
                                );
                            } else {
                                edits.push(StyleEdit::Overwrite {
                                    span: component.span(),
                                    content: prefix,
                                });
                            }
                            self.facts.rewrites.deep = true;
                            return Ok(true);
                        }
                        if special_name == Some(VueSpecialPseudo::Slotted) {
                            self.collect_slotted_component_edits(component, pseudo, edits)?;
                            self.facts.rewrites.slotted = true;
                            return Ok(true);
                        }

                        let Some(selector_list) = pseudo.selector_list() else {
                            continue;
                        };
                        if !selector_list_is_trusted_for_scoping(selector_list) {
                            return Err(self.untrusted(selector_list.span()));
                        }
                        for nested in selector_list.selectors() {
                            found |= self.collect_special_selector_edits(nested, edits)?;
                        }
                    }
                    previous_compound = Some(compound);
                }
                ComplexSelectorPart::Combinator(combinator) => {
                    let mut deep_span = None;
                    if combinator.kind() == CombinatorKind::Child && index + 2 < parts.len() {
                        if let (
                            ComplexSelectorPart::Combinator(second),
                            ComplexSelectorPart::Combinator(third),
                        ) = (&parts[index + 1], &parts[index + 2])
                        {
                            if second.kind() == CombinatorKind::Child
                                && third.kind() == CombinatorKind::Child
                            {
                                let before = previous_compound
                                    .ok_or_else(|| self.untrusted(selector.span()))?;
                                let after = parts.get(index + 3).and_then(|part| match part {
                                    ComplexSelectorPart::Compound(compound) => Some(compound),
                                    ComplexSelectorPart::Combinator(_) => None,
                                });
                                let after = after.ok_or_else(|| self.untrusted(selector.span()))?;
                                deep_span =
                                    Some((Span::new(before.span().end, after.span().start), 3));
                            }
                        }
                    }
                    if let Some((span, consumed)) = deep_span {
                        if !found {
                            let compound =
                                previous_compound.ok_or_else(|| self.untrusted(selector.span()))?;
                            edits.push(StyleEdit::Insert {
                                at: self.scope_insertion(compound),
                                content: self.scope_attr.clone(),
                            });
                        }
                        edits.push(StyleEdit::Overwrite {
                            span,
                            content: " ".to_string(),
                        });
                        self.facts.rewrites.deep = true;
                        found = true;
                        index += consumed;
                        continue;
                    }
                }
            }
            index += 1;
        }
        Ok(found)
    }

    fn collect_unwrapped_argument_edits(
        &self,
        outer: Span,
        argument: Span,
        prefix: String,
        edits: &mut Vec<StyleEdit>,
    ) {
        if outer.start < argument.start || !prefix.is_empty() {
            edits.push(StyleEdit::Overwrite {
                span: Span::new(outer.start, argument.start),
                content: prefix,
            });
        }
        if argument.end < outer.end {
            edits.push(StyleEdit::Overwrite {
                span: Span::new(argument.end, outer.end),
                content: String::new(),
            });
        }
    }

    /// Rewrites `:slotted(<arg>)` to `<arg>` with the slotted scope attribute
    /// inserted, expressed entirely as edits against the outer source: the
    /// `:slotted(` prefix and the `)` suffix are deleted in place, and the
    /// argument's scope-attribute inserts (already carrying absolute source
    /// offsets) go into the same outer edit vector verbatim. The argument is
    /// never rendered to an intermediate string, so no occurrence-local
    /// allocator, transform, or splice exists — the outer `emit` remains the
    /// sole edit applier and source-map producer.
    ///
    /// The three edit groups cannot overlap: the prefix overwrite ends at
    /// `argument_span.start`, every argument insert lands inside
    /// `[argument_span.start, argument_span.end]`, and the suffix overwrite
    /// starts at `argument_span.end`; inserts are zero-width, so `emit`'s
    /// `start < previous_end` guard accepts the boundary-touching cases.
    fn collect_slotted_component_edits(
        &self,
        component: &SelectorComponent,
        pseudo: &SelectorPseudo,
        edits: &mut Vec<StyleEdit>,
    ) -> Result<(), StyleRewriteFailure> {
        // `:slotted()` with nothing to unwrap refuses for the same reason
        // `:global()` does — there is no selector to carry the declarations.
        let selector = self.trusted_first_selector_argument(pseudo)?;
        let component_span = component.span();
        let argument_span = selector.span();
        edits.push(StyleEdit::Overwrite {
            span: Span::new(component_span.start, argument_span.start),
            content: String::new(),
        });
        self.collect_selector_scope_edits(selector, &self.slotted_attr, edits)?;
        edits.push(StyleEdit::Overwrite {
            span: Span::new(argument_span.end, component_span.end),
            content: String::new(),
        });
        Ok(())
    }

    fn collect_selector_scope_edits(
        &self,
        selector: &ComplexSelector,
        scope_attr: &str,
        edits: &mut Vec<StyleEdit>,
    ) -> Result<(), StyleRewriteFailure> {
        if !selector_is_trusted_for_scoping(selector) {
            return Err(self.untrusted(selector.span()));
        }
        let compound = selector
            .parts()
            .iter()
            .rev()
            .find_map(|part| match part {
                ComplexSelectorPart::Compound(value) => Some(value),
                ComplexSelectorPart::Combinator(_) => None,
            })
            .ok_or_else(|| self.untrusted(selector.span()))?;
        let components = compound.components();
        if components.len() == 1 {
            let component = &components[0];
            if component_ident_eq(self.source, component, "is")
                || component_ident_eq(self.source, component, "where")
            {
                let nested = component
                    .pseudo()
                    .and_then(SelectorPseudo::selector_list)
                    .ok_or_else(|| self.untrusted(component.span()))?;
                for selector in nested.selectors() {
                    self.collect_selector_scope_edits(selector, scope_attr, edits)?;
                }
                return Ok(());
            }
        }
        edits.push(StyleEdit::Insert {
            at: self.scope_insertion(compound),
            content: scope_attr.to_string(),
        });
        Ok(())
    }

    fn scope_insertion(&self, compound: &verter_css_syntax::SelectorCompound) -> u32 {
        compound
            .components()
            .iter()
            .find(|component| {
                matches!(
                    component.kind(),
                    SelectorComponentKind::PseudoClass
                        | SelectorComponentKind::PseudoElement
                        | SelectorComponentKind::FunctionalPseudo
                )
            })
            .map_or(compound.span().end, |component| component.span().start)
    }

    fn plan_animation_declaration(
        &mut self,
        declaration: &StyleDeclaration,
    ) -> Result<(), StyleRewriteFailure> {
        if self.keyframes.is_empty() {
            return Ok(());
        }
        let property = self.source.slice(declaration.name_span());
        if !css_identifier_eq_ignore_ascii_case(property, "animation")
            && !css_identifier_eq_ignore_ascii_case(property, "animation-name")
            && !css_identifier_eq_ignore_ascii_case(property, "-webkit-animation")
            && !css_identifier_eq_ignore_ascii_case(property, "-webkit-animation-name")
        {
            return Ok(());
        }
        if declaration.completeness() != StyleCompleteness::Complete
            || declaration.value().completeness() != StyleCompleteness::Complete
        {
            return Err(self.untrusted(declaration.span()));
        }
        collect_animation_edits(
            declaration.value().values(),
            self.source,
            &self.keyframes,
            &mut self.edits,
        );
        Ok(())
    }

    fn is_keyframes(&self, rule: &StyleDirective) -> bool {
        let head = self.source.slice(rule.head_span());
        css_identifier_eq_ignore_ascii_case(head, "@keyframes")
            || css_identifier_eq_ignore_ascii_case(head, "@-webkit-keyframes")
    }

    fn vue_special_pseudo_name(&self, component: &SelectorComponent) -> Option<VueSpecialPseudo> {
        component.pseudo()?.selector_list()?;
        if component_ident_eq(self.source, component, "global")
            || component_ident_eq(self.source, component, "v-global")
        {
            Some(VueSpecialPseudo::Global)
        } else if component_ident_eq(self.source, component, "deep")
            || component_ident_eq(self.source, component, "v-deep")
        {
            Some(VueSpecialPseudo::Deep)
        } else if component_ident_eq(self.source, component, "slotted")
            || component_ident_eq(self.source, component, "v-slotted")
        {
            Some(VueSpecialPseudo::Slotted)
        } else {
            None
        }
    }

    /// The pseudo's first argument selector, for the pseudos whose rewrite has
    /// no meaning without one. An absent or untrusted argument is a refusal,
    /// so callers get a selector or an error and never a third case to spell.
    fn trusted_first_selector_argument<'a>(
        &self,
        pseudo: &'a SelectorPseudo,
    ) -> Result<&'a ComplexSelector, StyleRewriteFailure> {
        let argument_span = pseudo.argument_span();
        self.trusted_selector_argument_allowing_empty(pseudo)?
            .ok_or_else(|| self.untrusted(argument_span))
    }

    /// The `::v-deep`/`:deep()` variant: an argument-less `:deep` is the
    /// documented bare form (`:deep .child`), so an empty argument list over
    /// whitespace-only bytes is a real answer rather than a refusal. Anything
    /// else inside the parentheses that did not parse into a trusted selector
    /// still refuses.
    fn trusted_selector_argument_allowing_empty<'a>(
        &self,
        pseudo: &'a SelectorPseudo,
    ) -> Result<Option<&'a ComplexSelector>, StyleRewriteFailure> {
        let argument_span = pseudo.argument_span();
        let selector_list = pseudo
            .selector_list()
            .ok_or_else(|| self.untrusted(argument_span))?;
        if !selector_list_is_trusted_for_scoping(selector_list) {
            return Err(self.untrusted(argument_span));
        }
        match selector_list.selectors().first() {
            Some(selector) => Ok(Some(selector)),
            None if self.source.slice(argument_span).trim().is_empty() => Ok(None),
            None => Err(self.untrusted(argument_span)),
        }
    }

    fn untrusted(&self, span: Span) -> StyleRewriteFailure {
        StyleRewriteFailure::new(
            StyleRewriteFailureClass::UntrustedRewriteTarget,
            StyleRewriteStage::PostPreprocessScoping,
            CssDialect::Css,
            Some(span),
        )
    }
}

fn collect_animation_edits(
    values: &[ComponentValue],
    source: &CssSource,
    keyframes: &[(String, String)],
    edits: &mut Vec<StyleEdit>,
) {
    for value in values {
        match value {
            ComponentValue::Token(token) if token.kind() == TokenKind::Ident => {
                let text = source.slice(token.span());
                if let Some((_, renamed)) = keyframes.iter().find(|(name, _)| name == text) {
                    edits.push(StyleEdit::Overwrite {
                        span: token.span(),
                        content: renamed.clone(),
                    });
                }
            }
            ComponentValue::Function(function) => {
                collect_animation_edits(function.values(), source, keyframes, edits)
            }
            ComponentValue::Block(block) => {
                collect_animation_edits(block.values(), source, keyframes, edits)
            }
            ComponentValue::Token(_)
            | ComponentValue::String(_)
            | ComponentValue::Comment(_)
            | ComponentValue::Interpolation(_) => {}
        }
    }
}

fn unknown_may_contain_selector(unknown: &UnknownStatement) -> bool {
    unknown.span().start < unknown.span().end
}

fn selector_content_span(selector: &ComplexSelector) -> Span {
    let end = selector
        .parts()
        .last()
        .map_or(selector.span().end, |part| match part {
            ComplexSelectorPart::Compound(compound) => compound.span().end,
            ComplexSelectorPart::Combinator(combinator) => combinator.span().end,
        });
    Span::new(selector.span().start, end)
}

fn selector_list_is_trusted_for_scoping(selector_list: &SelectorList) -> bool {
    matches!(
        selector_list.facts().completeness(),
        verter_css_syntax::SelectorCompleteness::Complete
    ) && selector_list
        .selectors()
        .iter()
        .all(selector_is_trusted_for_scoping)
}

fn selector_is_trusted_for_scoping(selector: &ComplexSelector) -> bool {
    matches!(
        selector.facts().completeness(),
        verter_css_syntax::SelectorCompleteness::Complete
    ) && selector.parts().iter().all(|part| match part {
        ComplexSelectorPart::Combinator(_) => true,
        ComplexSelectorPart::Compound(compound) => compound.components().iter().all(|component| {
            !matches!(
                component.kind(),
                SelectorComponentKind::DynamicClass | SelectorComponentKind::Interpolation
            ) && component
                .pseudo()
                .and_then(SelectorPseudo::selector_list)
                .is_none_or(selector_list_is_trusted_for_scoping)
        }),
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
