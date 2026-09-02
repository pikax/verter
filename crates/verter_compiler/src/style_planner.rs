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
pub use verter_css_syntax::{ExternalStyleProducer, PreprocessedStyle, StyleProducer};
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
    /// `space` is the stage whose bytes the refused stage was handed, which
    /// only the cascade driving the stages knows: the authored-`v-bind()`
    /// stage always sees the cascade's input bytes, while a later stage sees
    /// whatever an earlier rewrite left behind. Carrying it is what lets a
    /// consumer decide which map a reported span needs.
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
    source: StyleSourceIdentity<'a>,
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
    /// recorder — a later stage parses whatever an earlier rewrite left
    /// behind, and a sheet with no inclusions must not read as "not recorded
    /// yet" and let that later parse publish its own space's answer as the
    /// input's.
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
    /// cascade whose only parsing stage failed (a `v-bind()` stage that hits
    /// an untrusted rewrite target or a parse error, with neither CSS Modules
    /// nor scoping requested): nothing surveys the input, and answering
    /// "exhaustive" there is exactly the wrong-complete direction.
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

#[derive(Debug, Clone)]
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
    /// shared `parse_ir` wrapper. Every Vue-owned style transform stage — direct
    /// (`transform_vue_v_bind`/`transform_vue_css_modules`/`transform_vue_scoped_css`)
    /// or cascaded (`run_vue_style_cascade`) — routes through `parse_ir`, so this
    /// count is the authoritative, directly-observable proof that an `Unchanged`
    /// stage hands its parsed `StyleSyntaxIr` forward instead of re-parsing
    /// (the one-parse-per-content-identity invariant). THREAD-LOCAL, not
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
/// bytes: a later stage parses whatever an earlier rewrite left behind, and
/// its answer describes that space instead. A cascade that never reaches this
/// recorder leaves the answer unrecorded, and
/// [`VueStyleFacts::pulls_in_unparsed_bytes`] fails closed on it.
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
    accumulated: Option<&SourceMap<'static>>,
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
    let source_map = if !want_source_map {
        None
    } else {
        match (source_name, accumulated) {
            (Some(source_name), None) => {
                Some(transform.generate_map(SourceMapOptions::new().with_source(source_name)))
            }
            (None, Some(accumulated)) => transform.chain_source_map(accumulated).ok(),
            (None, None) => None,
            (Some(_), Some(_)) => {
                unreachable!("a transform cannot build a fresh map and compose one simultaneously")
            }
        }
    };
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
        None,
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
    let origin = ir.source().origin();
    let (edits, vars) = v_bind_edits_from_ir(&ir, input.dialect, scope_id)?;
    let edits = relocate_edits(
        edits,
        origin,
        StyleRewriteStage::AuthoredVBind,
        input.dialect,
    )?;
    let vars = relocate_v_bind_vars(vars, origin);
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
/// Runtime class-name rewriting stays `transform_vue_css_modules`'s
/// plain-CSS-only, post-preprocess job; row 19's ownership question is
/// untouched. Class selectors are syntactically identical across all five
/// dialects (no dialect-specific interpolation form is a bare `.class`), so
/// the walk that already backs `transform_vue_css_modules` needs no dialect
/// gate to run here.
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

pub fn transform_vue_css_modules(
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
/// block `transform_vue_scoped_css` (parses then delegates here) and
/// `run_vue_style_cascade` (reuses a retained IR across stages) both route
/// through.
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

pub fn transform_vue_scoped_css(
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

/// Whether a `StyleRewriteFailureClass` participates in the publish gate at
/// all. Exhaustive over the enum — no wildcard arm — so adding a new class
/// fails to compile here until its disposition is decided explicitly.
#[must_use]
const fn failure_class_gates_publication(class: StyleRewriteFailureClass) -> bool {
    match class {
        StyleRewriteFailureClass::StageRequiresPlainCss
        | StyleRewriteFailureClass::ParseFailure
        | StyleRewriteFailureClass::UntrustedRewriteTarget
        | StyleRewriteFailureClass::OverlappingEdits
        | StyleRewriteFailureClass::IndentedLayoutMutation => true,
    }
}

/// Reports whether the cascade result can be published without exposing
/// output cleared by a failed plain-CSS rewrite. An authored-v-bind failure
/// does not gate publication because that rewrite precedes caller-supplied
/// preprocessed bytes and never clears the cascade output.
///
/// Every class ultimately reduces to the same rule: a failure whose `stage`
/// is `AuthoredVBind` never blocks publication; a failure on either
/// post-preprocess stage blocks publication only when it left the output
/// wiped to empty for a non-empty authored input.
#[must_use]
pub fn cascade_output_is_publishable(
    outcome: &VueStyleCascadeOutcome,
    authored_code: &str,
) -> bool {
    let post_preprocess_failure = outcome.stage_failures.iter().any(|failure| {
        failure_class_gates_publication(failure.class)
            && !matches!(failure.stage, StyleRewriteStage::AuthoredVBind)
    });
    if !post_preprocess_failure {
        return true;
    }
    // The recorded refusal, not `code().is_empty()`. Emptiness is a shape two
    // different outcomes share — a wiped output and an authored
    // `<style></style>` — and only the stage that wiped it knows which.
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

#[derive(Debug, Clone)]
enum MapComposition {
    NotStarted,
    Composing(Box<SourceMap<'static>>),
    Abandoned,
}

impl MapComposition {
    const fn accumulated(&self) -> Option<&SourceMap<'static>> {
        match self {
            Self::Composing(map) => Some(map),
            Self::NotStarted | Self::Abandoned => None,
        }
    }
}

/// Applies a stage's collected edits against `code`, returning the new
/// `(code, map composition)` pair when the stage rewrote anything, or `None`
/// when it did not (in which case the caller retains its already-parsed IR
/// for the next stage instead of re-parsing).
fn apply_cascade_stage(
    code: &str,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    stage: StyleRewriteStage,
    edits: Vec<StyleEdit>,
    composition: &MapComposition,
    want_source_map: bool,
) -> Result<Option<(String, MapComposition)>, StyleRewriteFailure> {
    if edits.is_empty() {
        return Ok(None);
    }
    let (source_name, accumulated) = if !want_source_map {
        (None, None)
    } else {
        match composition {
            MapComposition::NotStarted => (Some(source.source_name), None),
            MapComposition::Composing(map) => (None, Some(map.as_ref())),
            MapComposition::Abandoned => (None, None),
        }
    };
    let Some((code, source_map)) = build_transform_output(
        code,
        dialect,
        stage,
        edits,
        source_name,
        accumulated,
        want_source_map,
    )?
    else {
        unreachable!("non-empty edits always produce a rewrite")
    };
    let composition = match (composition, source_map) {
        (MapComposition::Abandoned, _) => MapComposition::Abandoned,
        (_, Some(source_map)) => MapComposition::Composing(Box::new(source_map)),
        (_, None) => MapComposition::Abandoned,
    };
    Ok(Some((code, composition)))
}

struct AuthoredVueStyleState {
    owned: Option<(String, MapComposition)>,
    facts: VueStyleFacts,
    stage_failures: Vec<StyleRewriteFailure>,
    retained_ir: Option<ParsedStyleIr>,
}

fn run_vue_authored_v_bind_stage(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    want_source_map: bool,
) -> AuthoredVueStyleState {
    let mut owned: Option<(String, MapComposition)> = None;
    let mut facts = VueStyleFacts::default();
    let mut stage_failures = Vec::new();
    let mut retained_ir: Option<ParsedStyleIr> = None;

    // Authored v-bind — always runs, on the authored dialect. A
    // stage failure is recorded without clearing the accumulated output:
    // the modules/scoped-selector stages below still run against the
    // original authored bytes.
    {
        let stage: Result<_, StyleRewriteFailure> = (|| {
            let ir = authored_or_parsed_ir(input, StyleRewriteStage::AuthoredVBind)?;
            observe_style_ir(StyleRewriteStage::AuthoredVBind, &ir);
            let origin = ir.source().origin();
            let (edits, vars) = v_bind_edits_from_ir(&ir, input.dialect, scope_id)?;
            let edits = relocate_edits(
                edits,
                origin,
                StyleRewriteStage::AuthoredVBind,
                input.dialect,
            )?;
            let vars = relocate_v_bind_vars(vars, origin);
            Ok((ir, edits, vars))
        })();
        match stage {
            Ok((ir, edits, vars)) => {
                facts.v_bind_vars = vars;
                // This stage parsed the cascade's input, so it publishes the
                // inventory through the shared recorder rather than assigning
                // the list alone: the derived "does this pull in bytes nothing
                // here parsed" answer is what consumers branch on, and a bare
                // assignment leaves it at its default while making every later
                // stage's recorder call a no-op.
                record_input_dependencies(&mut facts, &ir);
                facts.rewrites.v_bind = !edits.is_empty();
                match apply_cascade_stage(
                    input.code,
                    input.source,
                    input.dialect,
                    StyleRewriteStage::AuthoredVBind,
                    edits,
                    &MapComposition::NotStarted,
                    want_source_map,
                ) {
                    Ok(Some(rewritten)) => owned = Some(rewritten),
                    Ok(None) => retained_ir = Some(ir),
                    Err(failure) => stage_failures.push(failure),
                }
            }
            Err(failure) => stage_failures.push(failure),
        }
    }

    AuthoredVueStyleState {
        owned,
        facts,
        stage_failures,
        retained_ir,
    }
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
    Preprocessed(StyleProducer),
}

impl CascadeInput {
    const fn stage(&self) -> StyleStage {
        match self {
            Self::Authored => StyleStage::Authored,
            Self::Preprocessed(_) => StyleStage::Preprocessed,
        }
    }
}

/// Which byte space each stage of one cascade run was handed.
///
/// A refusal's span addresses the bytes the refusing stage parsed, so the only
/// party that can say which space that is, is the runner that handed those
/// bytes over. It records the answer per stage here instead of letting the
/// outcome assembler re-derive it from one earlier stage's rewrite flag: every
/// stage in the cascade can rewrite bytes on its own, so "did `v-bind()`
/// rewrite" answers for the CSS-Modules stage and is simply wrong for the
/// scoped-selector stage below it, which parses whatever CSS Modules left
/// behind.
///
/// `true` means the stage was handed the cascade's own input bytes. A stage
/// that never ran cannot have produced a refusal, so its value is never read.
#[derive(Debug, Clone, Copy)]
struct CascadeStageSpaces {
    modules_at_input: bool,
    scoping_at_input: bool,
}

impl CascadeStageSpaces {
    /// The state before any post-`v-bind()` stage runs: whatever runs next
    /// still sees the cascade input.
    const AT_INPUT: Self = Self {
        modules_at_input: true,
        scoping_at_input: true,
    };

    /// Resolve a refused stage to the stage its span's byte space belongs to.
    /// The authored-`v-bind()` stage always runs against the cascade input.
    const fn space_of(self, stage: StyleRewriteStage, input_stage: StyleStage) -> StyleStage {
        let at_input = match stage {
            StyleRewriteStage::AuthoredVBind => true,
            StyleRewriteStage::PostPreprocessModules => self.modules_at_input,
            StyleRewriteStage::PostPreprocessScoping => self.scoping_at_input,
        };
        if at_input {
            input_stage
        } else {
            StyleStage::FrameworkRewritten
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
    /// A stage rewrote the input into these bytes, with the map composed
    /// across every rewrite that ran.
    Rewritten {
        code: String,
        composition: MapComposition,
    },
    /// A stage refused and wiped the output.
    ClearedByRefusal,
}

impl CascadeOutput {
    /// Read the stage runner's state as one of the three answers above. The
    /// clearing flag wins: a refusal that wiped the output leaves owned bytes
    /// behind (empty ones), and those are the bytes nothing produced.
    fn from_stage_state(owned: Option<(String, MapComposition)>, cleared_by_refusal: bool) -> Self {
        if cleared_by_refusal {
            return Self::ClearedByRefusal;
        }
        match owned {
            Some((code, composition)) => Self::Rewritten { code, composition },
            None => Self::Passthrough,
        }
    }
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
/// `spaces` is the per-stage record of which bytes each stage was handed. It
/// is deliberately not derived from the output state: a plain-CSS-only stage
/// that refuses also CLEARS the output, and that clearing happens after the
/// refusal — it does not move the bytes the refusal was reported against.
fn finish_vue_style_cascade(
    input: CascadeInput,
    dialect: CssDialect,
    input_code: &str,
    spaces: CascadeStageSpaces,
    output: CascadeOutput,
    facts: VueStyleFacts,
    stage_failures: Vec<StyleRewriteFailure>,
) -> VueStyleCascadeOutcome {
    let input_stage = input.stage();
    let cleared_by_refusal = matches!(output, CascadeOutput::ClearedByRefusal);
    let rewritten = matches!(output, CascadeOutput::Rewritten { .. });
    let (code, source_map) = match output {
        CascadeOutput::Rewritten { code, composition } => (
            code,
            composition
                .accumulated()
                .map(SourceMap::to_json_string)
                .unwrap_or_default(),
        ),
        CascadeOutput::ClearedByRefusal => (String::new(), String::new()),
        CascadeOutput::Passthrough => (input_code.to_string(), String::new()),
    };
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
        .map(|failure| failure.to_diagnostic(spaces.space_of(failure.stage, input_stage)))
        .collect();
    let result = if cleared_by_refusal {
        // Nothing produced these (absent) bytes, so nothing claims them. The
        // stage still names the space the refusals' own coordinates belong
        // to, which is what makes them placeable.
        QualifiedStyleResult::refused(input_stage, dialect, diagnostics)
    } else if rewritten {
        QualifiedStyleResult::framework_rewritten(dialect, code, diagnostics)
    } else {
        match input {
            CascadeInput::Authored => QualifiedStyleResult::authored(dialect, code, diagnostics),
            CascadeInput::Preprocessed(producer) => {
                QualifiedStyleResult::preprocessed(producer, code, diagnostics)
            }
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
pub fn run_vue_style_authored_only(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    verter_audit::attribute_scope!(CssTransform);
    let state = run_vue_authored_v_bind_stage(input, scope_id, want_source_map);
    finish_vue_style_cascade(
        CascadeInput::Authored,
        input.dialect,
        input.code,
        // No post-`v-bind()` stage runs on this entry point.
        CascadeStageSpaces::AT_INPUT,
        // The authored-`v-bind()` stage keeps whatever preceded it on failure
        // and never wipes the output.
        CascadeOutput::from_stage_state(state.owned, false),
        state.facts,
        state.stage_failures,
    )
}

/// Runs Vue's authored-v-bind → CSS-Modules → scoped-selector cascade,
/// parsing each content identity at most once: a stage that produces
/// no edits hands its own already-parsed `StyleSyntaxIr` to the next stage
/// instead of causing a re-parse. Only a stage that DID change bytes forces
/// the following stage to parse fresh (the new bytes are a new content
/// identity `StyleSyntaxIr` never saw). `module`/`scoped` mirror the SFC's
/// `<style module>`/`<style scoped>` attributes; both require the AUTHORED
/// dialect to already be plain CSS (external preprocessing, JS/builder-owned,
/// is not modelled here — same `PlainCssInput` gate the CSS-Modules/
/// scoped-selector stages already enforce individually).
///
/// A stage that cannot safely run does not abort the whole cascade — see
/// [`VueStyleCascadeOutcome::stage_failures`]. The authored-v-bind stage
/// runs against the authored bytes regardless of whether it itself
/// succeeds, so a v-bind failure still lets CSS-Modules/scoped-selector
/// process those same authored bytes.
pub fn run_vue_style_cascade(
    input: AuthoredStyleInput<'_>,
    scope_id: &str,
    module: bool,
    scoped: bool,
    want_source_map: bool,
) -> VueStyleCascadeOutcome {
    verter_audit::attribute_scope!(CssTransform);
    let AuthoredVueStyleState {
        mut owned,
        mut facts,
        mut stage_failures,
        retained_ir,
    } = run_vue_authored_v_bind_stage(input, scope_id, want_source_map);
    let mut spaces = CascadeStageSpaces::AT_INPUT;
    let mut cleared_by_refusal = false;

    if module || scoped {
        let current_code = owned.as_ref().map_or(input.code, |(code, _)| code.as_str());
        let composition = owned
            .as_ref()
            .map_or(MapComposition::NotStarted, |(_, composition)| {
                composition.clone()
            });
        let post = run_post_v_bind_stages(
            current_code,
            owned.is_none(),
            input.source,
            input.dialect,
            retained_ir,
            scope_id,
            module,
            scoped,
            &mut facts,
            &mut stage_failures,
            composition,
            want_source_map,
        );
        spaces = post.spaces;
        cleared_by_refusal = post.output_cleared_by_refusal;
        if let Some(rewritten) = post.owned {
            owned = Some(rewritten);
        }
    }

    finish_vue_style_cascade(
        CascadeInput::Authored,
        input.dialect,
        input.code,
        spaces,
        CascadeOutput::from_stage_state(owned, cleared_by_refusal),
        facts,
        stage_failures,
    )
}

/// Runs the full Vue style cascade from native-CSS grammar provenance.
///
/// The supplied parse is reused for the first stage. If that stage leaves the
/// bytes unchanged, the same parsed structure continues into later stages.
///
/// `input_stage` names where these bytes came from. Plain CSS reaching this
/// entry is either a carrier's own authored CSS or an external preprocessor's
/// output, and only the caller knows which — the bytes look the same either
/// way. It is a parameter rather than an inference because guessing writes the
/// wrong provenance onto every result and, worse, the wrong coordinate space
/// onto every refusal.
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
    let mut owned: Option<(String, MapComposition)> = None;
    let mut facts = VueStyleFacts::default();
    let mut stage_failures = Vec::new();
    let mut retained_ir = None;

    observe_style_ir(StyleRewriteStage::AuthoredVBind, &parsed);
    record_input_dependencies(&mut facts, &parsed);
    let origin = parsed.source().origin();
    match v_bind_edits_from_ir(&parsed, CssDialect::Css, scope_id).and_then(|(edits, vars)| {
        let edits = relocate_edits(
            edits,
            origin,
            StyleRewriteStage::AuthoredVBind,
            CssDialect::Css,
        )?;
        Ok((edits, relocate_v_bind_vars(vars, origin)))
    }) {
        Ok((edits, vars)) => {
            facts.v_bind_vars = vars;
            facts.rewrites.v_bind = !edits.is_empty();
            match apply_cascade_stage(
                code,
                source,
                CssDialect::Css,
                StyleRewriteStage::AuthoredVBind,
                edits,
                &MapComposition::NotStarted,
                want_source_map,
            ) {
                Ok(Some(rewritten)) => owned = Some(rewritten),
                Ok(None) => retained_ir = Some(parsed),
                Err(failure) => stage_failures.push(failure),
            }
        }
        Err(failure) => stage_failures.push(failure),
    }
    let mut spaces = CascadeStageSpaces::AT_INPUT;
    let mut cleared_by_refusal = false;

    if module || scoped {
        let current_code = owned.as_ref().map_or(code, |(value, _)| value.as_str());
        let composition = owned
            .as_ref()
            .map_or(MapComposition::NotStarted, |(_, composition)| {
                composition.clone()
            });
        let post = run_post_v_bind_stages(
            current_code,
            owned.is_none(),
            source,
            CssDialect::Css,
            retained_ir,
            scope_id,
            module,
            scoped,
            &mut facts,
            &mut stage_failures,
            composition,
            want_source_map,
        );
        spaces = post.spaces;
        cleared_by_refusal = post.output_cleared_by_refusal;
        if let Some(rewritten) = post.owned {
            owned = Some(rewritten);
        }
    }

    finish_vue_style_cascade(
        input_stage,
        CssDialect::Css,
        code,
        spaces,
        CascadeOutput::from_stage_state(owned, cleared_by_refusal),
        facts,
        stage_failures,
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

/// Outcome of the post-`v-bind()` half of the cascade: the bytes it produced,
/// if any, and which byte space each of its stages was handed.
struct PostVBindStages {
    owned: Option<(String, MapComposition)>,
    spaces: CascadeStageSpaces,
    /// A stage refused and WIPED the output rather than producing it. Read
    /// as a recorded fact, never re-derived from `owned`'s bytes being
    /// empty: a rewrite can legitimately produce nothing, and only the stage
    /// that cleared the output knows the difference.
    output_cleared_by_refusal: bool,
}

/// Shared CSS-Modules → scoped-selector continuation of the style cascade
/// (stages 2 and 3), used by both cascade entry points so the module→scoped
/// IR hand-off applies identically to each. `owned` is
/// `Some((code, source_map))` when a stage rewrote bytes or hard-failed (in
/// which case `code` is empty); `None` when neither stage produced output.
///
/// A CSS-Modules or scoped-selector stage that cannot safely run pushes its
/// failure onto `stage_failures` and clears the output rather than leaving
/// unsafe partial bytes in place; a CSS-Modules failure also skips the
/// scoped-selector stage below it, since it would only ever see the
/// cleared, empty output.
///
/// `code_is_cascade_input` says whether `current_code` is still the bytes the
/// cascade was handed. Only the caller knows — an earlier stage may already
/// have rewritten them — and it decides whether a parse here may contribute
/// the input's inclusion inventory, whose spans must address the input space.
#[allow(clippy::too_many_arguments)]
fn run_post_v_bind_stages(
    current_code: &str,
    code_is_cascade_input: bool,
    source: StyleSourceIdentity<'_>,
    dialect: CssDialect,
    mut retained_ir: Option<ParsedStyleIr>,
    scope_id: &str,
    module: bool,
    scoped: bool,
    facts: &mut VueStyleFacts,
    stage_failures: &mut Vec<StyleRewriteFailure>,
    composition_in: MapComposition,
    want_source_map: bool,
) -> PostVBindStages {
    let mut owned: Option<(String, MapComposition)> = None;
    let mut spaces = CascadeStageSpaces {
        modules_at_input: code_is_cascade_input,
        scoping_at_input: code_is_cascade_input,
    };
    let mut output_cleared_by_failure = false;
    let composition_for = |owned: &Option<(String, MapComposition)>| {
        owned.as_ref().map_or_else(
            || composition_in.clone(),
            |(_, composition)| composition.clone(),
        )
    };

    // Stage 2: CSS Modules — plain-CSS only. Nothing has rewritten yet, so
    // this stage reads the cascade's input whenever the caller says the code
    // it was handed is that input. The scoped stage below cannot say the same:
    // this stage may have replaced the bytes underneath it.
    if module {
        let at_input = code_is_cascade_input;
        spaces.modules_at_input = at_input;
        let code_now = owned
            .as_ref()
            .map_or(current_code, |(code, _)| code.as_str());
        let stage: Result<_, StyleRewriteFailure> = (|| {
            let plain = PlainCssInput::try_new(
                code_now,
                dialect,
                source.source_name,
                source.source_space_token,
                source.content_artifact_token,
            )?;
            let ir = match retained_ir.take() {
                Some(ir) => ir,
                None => parse_ir(
                    plain.code,
                    CssDialect::Css,
                    StyleRewriteStage::PostPreprocessModules,
                )?,
            };
            observe_style_ir(StyleRewriteStage::PostPreprocessModules, &ir);
            if at_input {
                record_input_dependencies(facts, &ir);
            }
            let origin = ir.source().origin();
            let (edits, classes) =
                module_classes_and_edits_from_ir(&ir, CssDialect::Css, scope_id)?;
            let edits = relocate_edits(
                edits,
                origin,
                StyleRewriteStage::PostPreprocessModules,
                CssDialect::Css,
            )?;
            Ok((plain, ir, edits, classes))
        })();
        match stage {
            Ok((plain, ir, edits, classes)) => {
                facts.module_classes = classes.into_iter().collect();
                facts.rewrites.css_modules = !edits.is_empty();
                let composition = composition_for(&owned);
                match apply_cascade_stage(
                    plain.code,
                    source,
                    CssDialect::Css,
                    StyleRewriteStage::PostPreprocessModules,
                    edits,
                    &composition,
                    want_source_map,
                ) {
                    Ok(Some(rewritten)) => owned = Some(rewritten),
                    Ok(None) => retained_ir = Some(ir),
                    Err(failure) => {
                        stage_failures.push(failure);
                        owned = Some((String::new(), MapComposition::Abandoned));
                        retained_ir = None;
                        output_cleared_by_failure = true;
                    }
                }
            }
            Err(failure) => {
                stage_failures.push(failure);
                owned = Some((String::new(), MapComposition::Abandoned));
                retained_ir = None;
                output_cleared_by_failure = true;
            }
        }
    }

    // Stage 3: scoped selectors + keyframes — plain-CSS only. Skipped when
    // the modules stage above hard-failed and cleared the output.
    if scoped && !output_cleared_by_failure {
        let at_input = code_is_cascade_input && owned.is_none();
        spaces.scoping_at_input = at_input;
        let code_now = owned
            .as_ref()
            .map_or(current_code, |(code, _)| code.as_str());
        let stage: Result<_, StyleRewriteFailure> = (|| {
            let plain = PlainCssInput::try_new(
                code_now,
                dialect,
                source.source_name,
                source.source_space_token,
                source.content_artifact_token,
            )?;
            let ir = match retained_ir.take() {
                Some(ir) => ir,
                None => parse_ir(
                    plain.code,
                    CssDialect::Css,
                    StyleRewriteStage::PostPreprocessScoping,
                )?,
            };
            observe_style_ir(StyleRewriteStage::PostPreprocessScoping, &ir);
            if at_input {
                record_input_dependencies(facts, &ir);
            }
            let origin = ir.source().origin();
            let (edits, stage_facts) = scoped_edits_and_facts_from_ir(&ir, scope_id)?;
            let edits = relocate_edits(
                edits,
                origin,
                StyleRewriteStage::PostPreprocessScoping,
                CssDialect::Css,
            )?;
            Ok((plain, edits, stage_facts))
        })();
        match stage {
            Ok((plain, edits, stage_facts)) => {
                facts.rewrites.deep |= stage_facts.rewrites.deep;
                facts.rewrites.slotted |= stage_facts.rewrites.slotted;
                facts.rewrites.global |= stage_facts.rewrites.global;
                facts.rewrites.keyframes |= stage_facts.rewrites.keyframes;
                facts.rewrites.scoped_selector |= stage_facts.rewrites.scoped_selector;
                facts.refusals.extend(stage_facts.refusals);
                let composition = composition_for(&owned);
                match apply_cascade_stage(
                    plain.code,
                    source,
                    CssDialect::Css,
                    StyleRewriteStage::PostPreprocessScoping,
                    edits,
                    &composition,
                    want_source_map,
                ) {
                    Ok(Some(rewritten)) => owned = Some(rewritten),
                    Ok(None) => {}
                    Err(failure) => {
                        stage_failures.push(failure);
                        owned = Some((String::new(), MapComposition::Abandoned));
                        output_cleared_by_failure = true;
                    }
                }
            }
            Err(failure) => {
                stage_failures.push(failure);
                owned = Some((String::new(), MapComposition::Abandoned));
                output_cleared_by_failure = true;
            }
        }
    }

    PostVBindStages {
        owned,
        spaces,
        output_cleared_by_refusal: output_cleared_by_failure,
    }
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
                            let argument = self.render_special_argument(pseudo, false)?;
                            edits.truncate(edits_checkpoint);
                            edits.push(StyleEdit::Overwrite {
                                span: selector_content_span(selector),
                                content: argument,
                            });
                            self.facts.rewrites.global = true;
                            return Ok(true);
                        }
                        if special_name == Some(VueSpecialPseudo::Deep) {
                            let argument = self.render_special_argument(pseudo, true)?;
                            let same_compound_anchor = component_index > 0;
                            let anchor = same_compound_anchor
                                .then_some(compound)
                                .or(previous_compound);
                            let content = if let Some(anchor) = anchor {
                                edits.push(StyleEdit::Insert {
                                    at: self.scope_insertion(anchor),
                                    content: self.scope_attr.clone(),
                                });
                                if same_compound_anchor {
                                    format!(" {argument}")
                                } else {
                                    argument
                                }
                            } else {
                                format!("{} {argument}", self.scope_attr)
                            };
                            edits.push(StyleEdit::Overwrite {
                                span: component.span(),
                                content,
                            });
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

    fn render_special_argument(
        &self,
        pseudo: &SelectorPseudo,
        allow_empty: bool,
    ) -> Result<String, StyleRewriteFailure> {
        let Some(selector) = self.trusted_first_selector_argument(pseudo, allow_empty)? else {
            return Ok(String::new());
        };
        Ok(self.source.slice(selector.span()).to_string())
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
        let Some(selector) = self.trusted_first_selector_argument(pseudo, false)? else {
            // Unreachable with `allow_empty: false`; preserved as the exact
            // outcome the string-rendering path produced for a missing
            // argument: the whole `:slotted()` component is deleted.
            edits.push(StyleEdit::Overwrite {
                span: component.span(),
                content: String::new(),
            });
            return Ok(());
        };
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

    fn trusted_first_selector_argument<'a>(
        &self,
        pseudo: &'a SelectorPseudo,
        allow_empty: bool,
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
            None if allow_empty && self.source.slice(argument_span).trim().is_empty() => Ok(None),
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
            verter_css_syntax::StyleProducer::ExternalAnonymous,
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
