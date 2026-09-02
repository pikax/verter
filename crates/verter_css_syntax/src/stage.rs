//! Stage-qualified style identity.
//!
//! Style bytes travel through up to three byte spaces — as authored, as an
//! external preprocessor left them, and as a framework rewrite left them. A
//! span, a diagnostic position, or a dependency specifier only means something
//! against the space it was minted in, so this module makes the space part of
//! every value that carries one.
//!
//! [`QualifiedStyleResult`] is the only shape in which this crate hands style
//! bytes to a consumer: it names the stage, the dialect and the producer, and
//! carries the diagnostics observed while producing them. There is no
//! representation for style bytes with none of that attached.

use std::sync::Arc;

use verter_span::Span;

use crate::dialect::CssDialect;

/// Where a set of style bytes sits in the authored → preprocessed →
/// framework-rewritten lineage.
///
/// Each variant names a distinct byte space. Two stages of the same block
/// share coordinates only when the earlier stage changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleStage {
    /// The bytes exactly as authored in the carrier, in the authored dialect.
    Authored,
    /// The bytes an external preprocessor produced from the authored ones.
    /// Always plain CSS: leaving the authored dialect behind is what a
    /// preprocessor is for.
    ///
    /// Note which side of the tool this names. A preprocessor reports its own
    /// diagnostics against the bytes it CONSUMED, which is [`Self::Authored`];
    /// this variant is the output space, and a diagnostic tagged with it is
    /// claiming a position inside the generated CSS.
    Preprocessed,
    /// The bytes a Verter style rewrite produced (`v-bind()` lowering, CSS
    /// Modules class hashing, selector scoping). The dialect is whatever the
    /// rewrite's input was — the authored-`v-bind()` rewrite runs on every
    /// dialect and hands back the same one.
    FrameworkRewritten,
}

impl StyleStage {
    /// Whether a tool outside Verter produced bytes at this stage.
    #[must_use]
    pub const fn is_external(self) -> bool {
        match self {
            Self::Preprocessed => true,
            Self::Authored | Self::FrameworkRewritten => false,
        }
    }
}

/// One style diagnostic, qualified by the stage whose byte space `span`
/// addresses.
///
/// A style diagnostic without its stage is not addressable. A rewrite refusal
/// reports positions in the bytes it refused, and those stop being the
/// authored bytes the moment an earlier stage changes anything. Carrying the
/// stage is what lets a consumer decide which map to run a position through
/// instead of guessing.
///
/// The field always names the space the position is IN, never the authority
/// that reported it. There is no authority or severity axis here because every
/// style diagnostic this crate's consumers can produce today is one thing: a
/// framework rewrite refusing, which is an error. A closed taxonomy whose only
/// other members name routes that do not exist would assert a generality the
/// pipeline does not have; the route that adds one adds it with its producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleDiagnostic {
    stage: StyleStage,
    message: Arc<str>,
    span: Option<Span>,
}

impl StyleDiagnostic {
    #[must_use]
    pub fn new(stage: StyleStage, message: impl Into<Arc<str>>, span: Option<Span>) -> Self {
        Self {
            stage,
            message: message.into(),
            span,
        }
    }

    #[must_use]
    pub const fn stage(&self) -> StyleStage {
        self.stage
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Byte span within [`Self::stage`]'s own space, or `None` when the
    /// reporting authority gave no position.
    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        self.span
    }
}

/// Identity of an external preprocessor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalStyleProducer {
    identity: Arc<str>,
    version: Option<Arc<str>>,
    config_fingerprint: Option<Arc<str>>,
}

impl ExternalStyleProducer {
    /// Mint a named external producer. Returns `None` for an empty identity:
    /// a nameless tool is [`StyleProducer::ExternalAnonymous`], never a named
    /// one with a blank name.
    #[must_use]
    pub fn new(
        identity: &str,
        version: Option<&str>,
        config_fingerprint: Option<&str>,
    ) -> Option<Self> {
        let identity = identity.trim();
        if identity.is_empty() {
            return None;
        }
        Some(Self {
            identity: Arc::from(identity),
            version: version
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(Arc::from),
            config_fingerprint: config_fingerprint
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(Arc::from),
        })
    }

    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Opaque fingerprint of the tool's configuration. Never interpreted here.
    #[must_use]
    pub fn config_fingerprint(&self) -> Option<&str> {
        self.config_fingerprint.as_deref()
    }
}

/// What produced a stage's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleProducer {
    /// Verter's own parser and framework rewrite pipeline.
    Verter,
    /// An external preprocessor that named itself.
    External(ExternalStyleProducer),
    /// An external preprocessor that supplied no identity.
    ///
    /// Distinct from [`Self::External`] on purpose. An unnamed tool is a real,
    /// recordable state; folding it into a fabricated name would make
    /// provenance read as exact when it is not, and refusing the bytes
    /// outright would discard a result that is otherwise complete.
    ExternalAnonymous,
}

impl StyleProducer {
    /// The named external tool, when there is one.
    #[must_use]
    pub const fn external(&self) -> Option<&ExternalStyleProducer> {
        match self {
            Self::External(producer) => Some(producer),
            Self::Verter | Self::ExternalAnonymous => None,
        }
    }

    /// Whether a tool outside Verter produced the bytes, named or not.
    #[must_use]
    pub const fn is_external(&self) -> bool {
        match self {
            Self::External(_) | Self::ExternalAnonymous => true,
            Self::Verter => false,
        }
    }
}

/// Sass's built-in modules that contribute no stylesheet bytes, by their exact
/// reserved specifier.
///
/// `@use "sass:math"` and its peers name a library the compiler itself
/// provides, not another stylesheet: they contribute functions, never rules,
/// classes or custom properties. The set is closed and exact rather than the
/// `sass:` prefix, because the namespace is NOT uniformly non-emitting:
/// `sass:meta` exposes `load-css($url)`, a mixin that loads another module and
/// emits ITS css into the current sheet. A prefix test answers "no foreign
/// bytes" for `@use "sass:meta"; .a { @include meta.load-css("theme"); }`,
/// where the loaded sheet's rules — and any `v-bind()` in them — are exactly
/// the bytes nothing here parsed. `sass:meta` is therefore absent, and so is
/// any future member: an unrecognised `sass:` name is treated as contributing,
/// which fails toward "the surface may be incomplete".
pub(crate) const SASS_NON_EMITTING_BUILTIN_MODULES: [&str; 6] = [
    "sass:color",
    "sass:list",
    "sass:map",
    "sass:math",
    "sass:selector",
    "sass:string",
];

/// Kind of stylesheet inclusion a directive declares.
///
/// Closed over the inclusion directives of this crate's five dialects — every
/// keyword that pulls another stylesheet into the current one. Anything else
/// is not a dependency.
///
/// A missing member is not a cosmetic gap. An unrecognised inclusion keyword
/// records no dependency at all, so
/// [`crate::StyleSyntaxIr::pulls_in_unparsed_bytes`] answers "this sheet
/// declares its whole surface" for a sheet that plainly pulls in another one —
/// the wrong-complete direction, which publishes an exhaustive `v-bind()`
/// liveness inventory for a block whose bindings live in a sheet nothing here
/// parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleDependencyKind {
    /// CSS/Sass/SCSS/Less/Stylus `@import`.
    Import,
    /// Sass/SCSS `@use`.
    Use,
    /// Sass/SCSS `@forward`.
    Forward,
    /// Less `@plugin`.
    Plugin,
    /// Stylus `@require`, the language's include-once form. It brings in
    /// exactly the bytes `@import` does; only the repeat-inclusion behaviour
    /// differs, and that is not a distinction any consumer of this inventory
    /// makes.
    Require,
}

impl StyleDependencyKind {
    /// Classify an at-rule name (without the `@`). ASCII-case-insensitive, as
    /// CSS at-keywords are.
    ///
    /// Stylus also spells `import`/`require` as a bare statement with no `@`.
    /// That form carries no at-keyword and never reaches an at-rule frame, so
    /// it is recognised separately — see [`Self::from_stylus_statement_keyword`].
    #[must_use]
    pub fn from_at_rule_name(name: &str) -> Option<Self> {
        use crate::token::css_identifier_eq_ignore_ascii_case as eq;
        if eq(name, "import") {
            Some(Self::Import)
        } else if eq(name, "use") {
            Some(Self::Use)
        } else if eq(name, "forward") {
            Some(Self::Forward)
        } else if eq(name, "plugin") {
            Some(Self::Plugin)
        } else if eq(name, "require") {
            Some(Self::Require)
        } else {
            None
        }
    }

    /// Classify a Stylus statement that opens with a bare identifier.
    ///
    /// Stylus accepts `require 'theme'` and `import 'theme'` without the `@`,
    /// and both mean exactly what their `@`-spelled forms mean. Such a
    /// statement has no at-keyword for the layout pass to classify, so it
    /// reaches the IR as an unclassified statement rather than an at-rule;
    /// left unrecognised it records no dependency, which is the wrong-complete
    /// direction this inventory exists to close.
    ///
    /// Only the two inclusion keywords are recognised, and only for Stylus.
    /// Every other unclassified statement stays exactly that.
    #[must_use]
    pub fn from_stylus_statement_keyword(name: &str) -> Option<Self> {
        use crate::token::css_identifier_eq_ignore_ascii_case as eq;
        if eq(name, "import") {
            Some(Self::Import)
        } else if eq(name, "require") {
            Some(Self::Require)
        } else {
            None
        }
    }
}

/// Syntactic form the specifier was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleSpecifierForm {
    /// A quoted string: `@import "a.css"`.
    Quoted,
    /// A `url()` token or function: `@import url(a.css)`.
    Url,
}

/// The target a [`StyleDependency`] names, as a span into the parsed source.
///
/// The span addresses the specifier text with its quotes and any `url(`/`)`
/// wrapper already removed, so a reader never re-trims it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleSpecifier {
    span: Span,
    form: StyleSpecifierForm,
}

impl StyleSpecifier {
    #[must_use]
    pub(crate) const fn new(span: Span, form: StyleSpecifierForm) -> Self {
        Self { span, form }
    }

    #[must_use]
    pub const fn span(self) -> Span {
        self.span
    }

    #[must_use]
    pub const fn form(self) -> StyleSpecifierForm {
        self.form
    }
}

/// One stylesheet this stylesheet pulls in, as the parse observed it.
///
/// Minted by the parse that already had to recognise the at-rule, so reading
/// this list is never a second scan over bytes the parser has seen. Spans
/// address the parsed source — [`crate::StyleSyntaxIr::specifier_text`] reads
/// them without the caller needing to know the origin offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleDependency {
    kind: StyleDependencyKind,
    head_span: Span,
    specifier: Option<StyleSpecifier>,
}

impl StyleDependency {
    #[must_use]
    pub(crate) const fn new(
        kind: StyleDependencyKind,
        head_span: Span,
        specifier: Option<StyleSpecifier>,
    ) -> Self {
        Self {
            kind,
            head_span,
            specifier,
        }
    }

    #[must_use]
    pub const fn kind(self) -> StyleDependencyKind {
        self.kind
    }

    /// Span of the inclusion keyword itself, `@` included when the form has
    /// one and excluded for Stylus's bare `require`/`import` statement.
    #[must_use]
    pub const fn head_span(self) -> Span {
        self.head_span
    }

    /// The named target, or `None` when the prelude did not start with a form
    /// this crate can address exactly — a dialect interpolation, a variable,
    /// or a recovered prelude. `None` is not "no dependency": the at-rule is
    /// still here and its target is still unresolved.
    #[must_use]
    pub const fn specifier(self) -> Option<StyleSpecifier> {
        self.specifier
    }
}

/// Style bytes together with the identity that makes them addressable: the
/// stage they belong to, the dialect they are written in, what produced them,
/// and the diagnostics observed while producing them.
///
/// Constructed only through the stage constructors, each of which fixes the
/// invariants its stage carries. There is no constructor that takes bytes
/// alone, so preprocessed output cannot travel as an unqualified string.
///
/// It carries no inclusion list. A [`StyleDependency`]'s spans address the
/// space of the PARSE that minted them, and reading a specifier's text needs
/// that same [`crate::StyleSyntaxIr`] — which this carrier does not hold — so a
/// list here would be kind-and-order only, would be empty both for a sheet with
/// no inclusions and for bytes nothing ever parsed, and would cost a per-block
/// copy for a question no consumer asks of it. The one question consumers do
/// ask is whether the surface is exhaustive, and its owner is
/// [`crate::StyleSyntaxIr::pulls_in_unparsed_bytes`]. A consumer that wants the
/// targets themselves obtains the IR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedStyleResult {
    stage: StyleStage,
    dialect: CssDialect,
    producer: StyleProducer,
    code: String,
    diagnostics: Vec<StyleDiagnostic>,
    /// See [`Self::refused`]. Set only by that constructor, and the one thing
    /// that makes "no bytes were produced" readable rather than inferred from
    /// `code` being empty — an authored `<style></style>` is empty too.
    refused: bool,
}

impl QualifiedStyleResult {
    /// The bytes as authored, in the authored dialect.
    #[must_use]
    pub fn authored(
        dialect: CssDialect,
        code: impl Into<String>,
        diagnostics: Vec<StyleDiagnostic>,
    ) -> Self {
        Self {
            stage: StyleStage::Authored,
            dialect,
            producer: StyleProducer::Verter,
            code: code.into(),
            diagnostics,
            refused: false,
        }
    }

    /// Bytes an external preprocessor produced.
    ///
    /// The dialect is fixed to [`CssDialect::Css`] — a preprocessor that has
    /// not left its dialect behind has not finished. The producer slot is
    /// mandatory, but [`StyleProducer::ExternalAnonymous`] is a legitimate
    /// value for it: a tool that named no identity is recorded as unnamed, not
    /// refused and not given a fabricated name. What keeps these bytes from
    /// travelling on as an unqualified string is [`PreprocessedStyle`], the
    /// only shape a plain-CSS-only consumer accepts them in.
    ///
    #[must_use]
    pub fn preprocessed(
        producer: StyleProducer,
        code: impl Into<String>,
        diagnostics: Vec<StyleDiagnostic>,
    ) -> Self {
        Self {
            stage: StyleStage::Preprocessed,
            dialect: CssDialect::Css,
            producer,
            code: code.into(),
            diagnostics,
            refused: false,
        }
    }

    /// Bytes a Verter style rewrite produced from `dialect` input.
    #[must_use]
    pub fn framework_rewritten(
        dialect: CssDialect,
        code: impl Into<String>,
        diagnostics: Vec<StyleDiagnostic>,
    ) -> Self {
        Self {
            stage: StyleStage::FrameworkRewritten,
            dialect,
            producer: StyleProducer::Verter,
            code: code.into(),
            diagnostics,
            refused: false,
        }
    }

    /// A result the pipeline REFUSED to produce.
    ///
    /// The code is empty by construction, and nothing produced it — that is
    /// the whole content of this shape. A stage that cannot run safely wipes
    /// the output rather than exposing a half-applied rewrite, and the wiped
    /// output is not a rewrite's work: labelling it [`StyleStage::
    /// FrameworkRewritten`] with [`StyleProducer::Verter`] said "Verter's
    /// rewrite made these bytes" about bytes no rewrite ever made, which is
    /// exactly the provenance claim this type exists to keep exact.
    ///
    /// `stage` is the space the carried diagnostics address — the bytes the
    /// refusing stage was handed — NOT a space these absent bytes occupy, and
    /// the producer slot names Verter because the refusal is Verter's
    /// pipeline's own act. [`Self::is_refused`] is what a consumer branches
    /// on; `code().is_empty()` is not the same question, since an authored
    /// `<style></style>` is legitimately empty.
    #[must_use]
    pub fn refused(
        stage: StyleStage,
        dialect: CssDialect,
        diagnostics: Vec<StyleDiagnostic>,
    ) -> Self {
        Self {
            stage,
            dialect,
            producer: StyleProducer::Verter,
            code: String::new(),
            diagnostics,
            refused: true,
        }
    }

    #[must_use]
    pub const fn stage(&self) -> StyleStage {
        self.stage
    }

    /// Whether the pipeline refused to produce a result — see [`Self::refused`].
    ///
    /// `false` on every result that carries produced bytes, including a
    /// legitimately empty one.
    #[must_use]
    pub const fn is_refused(&self) -> bool {
        self.refused
    }

    #[must_use]
    pub const fn dialect(&self) -> CssDialect {
        self.dialect
    }

    #[must_use]
    pub const fn producer(&self) -> &StyleProducer {
        &self.producer
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Take the bytes, consuming the qualification with them.
    ///
    /// For the terminal consumer that owns the output: reading through
    /// [`Self::code`] and copying would add a per-block allocation the
    /// pre-qualified pipeline did not have.
    #[must_use]
    pub fn into_code(self) -> String {
        self.code
    }

    /// Diagnostics observed while producing this result, in production order:
    /// each authority appends its own in the order it reported them, and the
    /// authorities themselves append in the order they ran.
    #[must_use]
    pub fn diagnostics(&self) -> &[StyleDiagnostic] {
        &self.diagnostics
    }

    /// Borrow this result as preprocessor output, or `None` when it is not
    /// one.
    ///
    /// The single gate on the [`PreprocessedStyle`] witness: a consumer that
    /// only accepts preprocessor output takes the witness, so authored or
    /// framework-rewritten bytes are not something a caller can spell at that
    /// signature, let alone pass by mistake.
    ///
    /// All three recorded facts gate it, because the witness asserts all
    /// three:
    ///
    /// - the stage is [`StyleStage::Preprocessed`] — the output space,
    /// - the producer is external — the witness names the tool that made the
    ///   bytes, and [`StyleProducer::Verter`] names Verter's own pipeline, and
    /// - the result is not a refusal — [`Self::refused`] is public, sets the
    ///   producer to `Verter` and leaves the stage where the refusing stage
    ///   was, so a refusal AT the preprocessed stage would otherwise mint a
    ///   witness over zero bytes and `prepare_supplied_style` would admit it
    ///   as a valid empty stylesheet.
    #[must_use]
    pub fn as_preprocessed(&self) -> Option<PreprocessedStyle<'_>> {
        (!self.refused && self.stage == StyleStage::Preprocessed && self.producer.is_external())
            .then(|| PreprocessedStyle {
                code: &self.code,
                producer: self.producer.clone(),
            })
    }
}

/// Preprocessor output, borrowed from whoever owns the bytes, with the
/// identity of the tool that produced it.
///
/// A plain-CSS-only consumer takes this instead of `&str`, so "these bytes
/// already left their authored dialect behind, and here is who says so" is a
/// fact the signature carries rather than one every call site restates in a
/// comment. There are exactly two ways to obtain one:
/// [`QualifiedStyleResult::as_preprocessed`], which gates on the recorded
/// stage and producer, and [`Self::admitted`], for the boundary that accepted
/// the tool's output before it entered the process at all.
///
/// What the type enforces is exact and worth stating plainly: bytes cannot
/// reach a plain-CSS-only signature without a stated space AND a stated
/// producer. It does not — and cannot — prove the bytes really left SCSS
/// behind: plain CSS is a subset of every dialect this crate parses, so no
/// grammar check separates "CSS" from "SCSS that happens to be CSS-shaped".
/// The assertion is the admitting boundary's, and requiring the producer
/// alongside it is what keeps the assertion attached to a party that actually
/// ran or accepted the tool.
///
/// It borrows the bytes rather than owning them because the admitting boundary
/// already holds them; copying them to qualify them would add a per-block
/// allocation to the admission path and buy nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessedStyle<'a> {
    code: &'a str,
    producer: StyleProducer,
}

impl<'a> PreprocessedStyle<'a> {
    /// Name bytes an admission boundary has accepted as an external
    /// preprocessor's output, together with the identity it holds for the tool
    /// that produced them.
    ///
    /// The caller asserts the provenance; it is the only party that can, since
    /// it is the one that ran or accepted the tool.
    ///
    /// Read the rejection this constructor's existence bounds exactly. What a
    /// plain-CSS-only signature structurally rejects is `&str` — bytes handed
    /// over with NO stated stage and NO stated producer. It does not reject a
    /// caller willing to assert one: this constructor is public, and so is
    /// [`StyleProducer::ExternalAnonymous`], so `admitted(bytes,
    /// ExternalAnonymous)` is a spelling any crate can write. That is the
    /// point — the boundary that accepted the tool's output before it entered
    /// the process has no [`QualifiedStyleResult`] to mint the witness from —
    /// and the cost is that the assertion, not the bytes, is what carries the
    /// weight.
    ///
    /// Provenance the boundary retains beyond this — the tool's reported
    /// diagnostics and dependency list — stays on that boundary's own record,
    /// where its consumers read it.
    #[must_use]
    pub const fn admitted(code: &'a str, producer: StyleProducer) -> Self {
        Self { code, producer }
    }

    /// The preprocessed bytes. Plain CSS by the admitter's assertion.
    #[must_use]
    pub const fn code(&self) -> &'a str {
        self.code
    }

    /// The tool the admitter names for these bytes.
    #[must_use]
    pub const fn producer(&self) -> &StyleProducer {
        &self.producer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessed_result_is_always_plain_css_and_externally_produced() {
        let producer = StyleProducer::External(
            ExternalStyleProducer::new("sass", Some("1.77.0"), None).expect("named producer"),
        );
        let result = QualifiedStyleResult::preprocessed(producer, ".a{color:red}", Vec::new());

        assert_eq!(result.stage(), StyleStage::Preprocessed);
        assert_eq!(result.dialect(), CssDialect::Css);
        assert!(result.producer().is_external());
        assert_eq!(
            result
                .producer()
                .external()
                .map(ExternalStyleProducer::identity),
            Some("sass")
        );
    }

    #[test]
    fn a_blank_processor_name_is_anonymous_not_a_named_producer() {
        assert!(ExternalStyleProducer::new("   ", Some("1.0"), None).is_none());
        assert!(ExternalStyleProducer::new("", None, None).is_none());

        let anonymous = QualifiedStyleResult::preprocessed(
            StyleProducer::ExternalAnonymous,
            ".a{}",
            Vec::new(),
        );
        assert!(anonymous.producer().is_external());
        assert!(anonymous.producer().external().is_none());
    }

    #[test]
    fn framework_rewritten_keeps_the_input_dialect_unlike_preprocessed() {
        let rewritten = QualifiedStyleResult::framework_rewritten(
            CssDialect::Scss,
            ".a{color:var(--x)}",
            Vec::new(),
        );
        assert_eq!(rewritten.dialect(), CssDialect::Scss);
        assert!(!rewritten.stage().is_external());
        assert_eq!(rewritten.producer(), &StyleProducer::Verter);
    }

    #[test]
    fn at_rule_names_classify_case_insensitively_and_closed() {
        assert_eq!(
            StyleDependencyKind::from_at_rule_name("IMPORT"),
            Some(StyleDependencyKind::Import)
        );
        assert_eq!(
            StyleDependencyKind::from_at_rule_name("forward"),
            Some(StyleDependencyKind::Forward)
        );
        assert_eq!(StyleDependencyKind::from_at_rule_name("media"), None);
        assert_eq!(StyleDependencyKind::from_at_rule_name("imports"), None);
    }
}
