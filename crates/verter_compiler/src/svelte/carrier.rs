//! Svelte carrier bridge: the frontend half of the Svelte carrier.
//!
//! [`SvelteParseCarrier`] wraps [`ParsedSvelte`]. [`build_svelte_parse_artifact`]
//! produces the unregistered artifact; the projector owns inventory
//! geometry. [`SvelteCarrierFrontend`]: `parse` → neutral artifact. IDE
//! rendering and runtime-bundle orchestration live in
//! [`super::carrier_bundle`].

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    sort_language_diagnostics, CarrierParse, FrameworkAdapterId, JsModuleKind, LanguageDiagnostic,
    LanguageDiagnosticSeverity, LanguageId, ParseKey, ScriptSourceType, SyntaxProfileId,
    UnregisteredFrameworkParseArtifact,
};
use verter_span::Span;

use crate::framework_common::FrameworkParseArtifact;

use super::attribute_expressions::SvelteAttributeExpressions;
use super::parser::{CloseTagViolationKind, ParsedSvelte, SvelteScript};

/// The concrete Svelte carrier: the parsed component behind the erasure seam,
/// plus the typed lowering of its plain-attribute `{expr}` values.
#[derive(Debug, verter_retention::heap_size::HeapSize)]
pub struct SvelteParseCarrier {
    parsed: Arc<ParsedSvelte>,
    attribute_expressions: SvelteAttributeExpressions,
}

impl SvelteParseCarrier {
    /// Wrap a parsed Svelte component, lowering its plain-attribute values
    /// against the carrier `source` they were parsed from.
    #[must_use]
    pub fn new(parsed: Arc<ParsedSvelte>, source: &str) -> Self {
        let attribute_expressions = SvelteAttributeExpressions::lower(&parsed, source);
        Self {
            parsed,
            attribute_expressions,
        }
    }

    /// The wrapped parse result.
    #[must_use]
    pub fn parsed(&self) -> &ParsedSvelte {
        &self.parsed
    }

    /// The wrapped parse result, as the shared handle.
    #[must_use]
    pub fn parsed_arc(&self) -> &Arc<ParsedSvelte> {
        &self.parsed
    }

    /// The retained typed lowering of this component's plain-attribute values.
    #[must_use]
    pub fn attribute_expressions(&self) -> &SvelteAttributeExpressions {
        &self.attribute_expressions
    }
}

impl CarrierParse for SvelteParseCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Resolve a Svelte `<script lang>` value to a neutral [`ScriptSourceType`].
///
/// Svelte components are TypeScript-or-JavaScript; only exact `lang="ts"` is
/// TypeScript. No-lang and other script languages use JavaScript grammar. A
/// `.svelte` script is module-grammar (top-level `import`/`export` allowed), so
/// JS dialects resolve the module module-kind.
pub(crate) fn svelte_script_source_type(script: Option<&SvelteScript>) -> ScriptSourceType {
    match script.and_then(|s| s.lang.as_deref()) {
        Some("ts") => ScriptSourceType::Ts,
        Some("jsx") => ScriptSourceType::Jsx(JsModuleKind::Module),
        _ => ScriptSourceType::Js(JsModuleKind::Module),
    }
}

/// The Svelte carrier parser version stamped on produced artifacts.
/// Map parser diagnostic rails onto the framework-neutral mapped channel.
/// Close-tag / parse-reject facts come from typed rails (no code-string
/// classification); every entry has a retained span.
///
/// Strict-parse facts (`ParsedSvelte::strict_parse_errors`) are mapped too —
/// official recovers a usable tree while still recording the diagnostic.
/// Unlike other rails they carry `blocks_compile: false`: IDE-visible at
/// `Error` severity, but `compile_entry` must not refuse the whole file over
/// one recoverable defect. Client-runtime "emits `Main` ⇔ official ACCEPTS"
/// still sees every strict fact at `official_reject_gate` (typed non-fatal
/// runtime refusal, never an `Err` that blocks IDE).
fn svelte_parse_diagnostics(
    source: &str,
    parsed: &ParsedSvelte,
    parse_key: &ParseKey,
) -> Vec<LanguageDiagnostic> {
    let mut diagnostics = parsed
        .diagnostics
        .iter()
        .map(|diagnostic| LanguageDiagnostic {
            span: diagnostic.span,
            severity: LanguageDiagnosticSeverity::Error,
            code: diagnostic.official_code.unwrap_or(diagnostic.code),
            arguments: Vec::new(),
            message: diagnostic.message.clone(),
            blocks_compile: true,
        })
        .collect::<Vec<_>>();
    diagnostics.extend(parsed.close_tag_violations.iter().map(|violation| {
        let code = match violation.kind {
            CloseTagViolationKind::Unclosed => "element_unclosed",
            CloseTagViolationKind::InvalidClosingTag => "element_invalid_closing_tag",
            CloseTagViolationKind::VoidElementInvalidContent => "void_element_invalid_content",
        };
        recovered_svelte_diagnostic(violation.span, code, true)
    }));
    // Strict-parse facts: `blocks_compile: false` (usable recovered tree).
    // Other rails stay `blocks_compile: true`.
    diagnostics.extend(
        parsed
            .strict_parse_errors
            .iter()
            .map(|fact| recovered_svelte_diagnostic(fact.span, fact.official_code, false)),
    );
    diagnostics.extend(
        parsed
            .parse_reject_facts
            .iter()
            .map(|fact| recovered_svelte_diagnostic(fact.span, fact.official_code, true)),
    );
    // Non-CSS deferred parse defects only. CSS style-body rejection is
    // `official_reject::deferred_parse_defects`, not this mapped channel.
    diagnostics.extend(
        super::runtime::deferred_parse_defects_excluding_css(source, parsed)
            .into_iter()
            .map(|defect| {
                recovered_svelte_diagnostic(defect.span, defect.rejection.official_code, true)
            }),
    );
    sort_language_diagnostics(parse_key, &mut diagnostics);
    diagnostics
}

fn recovered_svelte_diagnostic(
    span: Span,
    code: &'static str,
    blocks_compile: bool,
) -> LanguageDiagnostic {
    LanguageDiagnostic {
        span,
        severity: LanguageDiagnosticSeverity::Error,
        code,
        arguments: Vec::new(),
        message: format!("Svelte recovered from `{code}`"),
        blocks_compile,
    }
}

/// Wrap a parsed Svelte component for the registered projector.
#[must_use]
pub fn build_svelte_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSvelte>,
    parse_key: Arc<ParseKey>,
    syntax_profile: Arc<SyntaxProfileId>,
) -> Arc<UnregisteredFrameworkParseArtifact> {
    let diagnostics = svelte_parse_diagnostics(source, &parsed, &parse_key);
    Arc::new(UnregisteredFrameworkParseArtifact::new(
        FrameworkAdapterId::svelte(),
        LanguageId::new("svelte"),
        parse_key,
        syntax_profile,
        diagnostics,
        Arc::new(SvelteParseCarrier::new(parsed, source)),
    ))
}

/// The registered-projector opener installed on the Svelte framework leg
/// (`CarrierLeg::open`). Returns the artifact's erased carrier ONLY for a
/// Svelte-adapter artifact — the sole cross-crate entry point for reaching
/// a Svelte registered artifact's typed carrier.
#[doc(hidden)]
pub fn open_svelte_carrier(artifact: &FrameworkParseArtifact) -> Option<Arc<dyn CarrierParse>> {
    artifact.erased_carrier_for_adapter(&FrameworkAdapterId::svelte())
}
