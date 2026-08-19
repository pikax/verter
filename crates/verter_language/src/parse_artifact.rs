//! Framework-neutral parse vocabulary and the unregistered carrier payload.
//!
//! [`UnregisteredFrameworkParseArtifact`] is the frontend result before the
//! compiler-owned registered projector proves carrier geometry. It carries
//! parse identity, mapped diagnostics, and a private type-erased carrier, but
//! deliberately exposes no inventory or block lookup surface.
//!
//! Carrier privacy is public-hidden + statically guarded, not
//! capability-token-gated: the raw downcast helpers
//! ([`__carrier_downcast_ref`] / [`__carrier_downcast_arc`]) are
//! `#[doc(hidden)]` and stay confined to each adapter's own bridge module
//! (a foreign artifact's erased payload is a DIFFERENT concrete
//! `CarrierParse` type, so the `Any` downcast already fails structurally
//! for it — no separate adapter-identity gate is needed). The
//! `carrier_downcast_confined_to_owning_adapter` static guard (in
//! `verter_session`'s architecture-guard suite) is the enforcement
//! authority across the crate seam, where a literal `pub(crate)` cannot
//! compile.

use std::any::Any;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use verter_identity::identity::{ParseKey, SyntaxProfileId};
use verter_span::Span;

use crate::ids::{FrameworkAdapterId, LanguageId};
use crate::language::{JsModuleKind, ScriptSourceType};
use carrier_inventory::{
    AttributeValue, CarrierAttribute, CarrierBlock, CarrierBlockInventory, ScriptRole,
    ScriptSourceType as InventoryScriptSourceType, SectionRole,
};

pub mod carrier_inventory;
pub mod carrier_structure_hash;

/// Role of an embedded carrier script region (closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptRegionKind {
    /// Per-component-instance script (Vue `<script setup>`, Svelte
    /// instance `<script>`).
    Instance,
    /// Module-scope script (Vue plain `<script>`, Svelte
    /// `<script module>`).
    Module,
    /// Frontmatter script (Astro `---` fence).
    Frontmatter,
}

/// One embedded script region of a carrier file.
///
/// `source_type` is the neutral answer to "is this carrier's script TS
/// or TSX or JS?" — populated by the owning adapter's producer from its
/// own parse data (Vue: `<script lang>`; Svelte: `<script lang="ts">`;
/// Astro: frontmatter is always TS), so session-side source-type
/// computation reads the common surface uniformly and never needs a
/// carrier downcast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptRegion {
    /// Content byte range of the region, carrier-file-absolute.
    pub span: Span,
    /// Script dialect of the region's content.
    pub source_type: ScriptSourceType,
    /// Role of the region within the carrier.
    pub kind: ScriptRegionKind,
}

/// One template region of a carrier file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TemplateRegion {
    /// Content byte range of the region, carrier-file-absolute.
    pub span: Span,
}

/// One style region of a carrier file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleRegion {
    /// Content byte range of the region, carrier-file-absolute.
    pub span: Span,
}

/// Block kind an external link substitutes for (closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalLinkKind {
    /// External script source (`<script src="…">`).
    Script,
    /// External template source (`<template src="…">`).
    Template,
    /// External style source (`<style src="…">`).
    Style,
}

/// An external-source reference declared by a carrier block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalLink {
    /// Which block kind the link substitutes for.
    pub kind: ExternalLinkKind,
    /// The raw specifier as written in source.
    pub specifier: String,
    /// Byte range of the specifier in the carrier file, when known.
    pub span: Option<Span>,
}

/// Severity of a neutral carrier-parse diagnostic (closed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageDiagnosticSeverity {
    /// A parse problem.
    Error,
    /// A potential issue that does not prevent parsing.
    Warning,
    /// An informational note.
    Info,
}

/// Canonically comparable semantic argument carried by a diagnostic.
///
/// Display text is intentionally excluded from diagnostic ordering. Frontends
/// encode the values used to render that text here so equivalent diagnostics
/// retain a deterministic order across runs and insertion paths.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiagnosticArg {
    /// Boolean argument.
    Bool(bool),
    /// Unsigned integer argument.
    Unsigned(u64),
    /// Signed integer argument.
    Signed(i64),
    /// Authored text argument.
    Text(String),
    /// Authored source range argument.
    Span { start: u32, end: u32 },
}

/// A framework-neutral carrier-parse diagnostic.
///
/// Defined HERE (not borrowed from a compiler crate) so the leaf
/// crate's `verter_span`-only dependency claim holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageDiagnostic {
    /// Carrier-file-absolute byte range the diagnostic refers to.
    pub span: Span,
    /// Severity of the diagnostic.
    pub severity: LanguageDiagnosticSeverity,
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Typed semantic values used to render the diagnostic.
    pub arguments: Vec<DiagnosticArg>,
    /// Human-readable message.
    pub message: String,
    /// Whether an `Error`-severity instance of this diagnostic should gate
    /// downstream compilation (refuse to produce IDE/runtime output for the
    /// whole file) — `true` for almost every diagnostic on this channel,
    /// including every genuinely fatal parse-time defect (a carrier with no
    /// real template/script entry, an unsupported construct the compiler
    /// cannot safely lower). `false` marks a diagnostic that is accurate and
    /// IDE-visible (the editor still shows it at full severity) but
    /// describes a RECOVERABLE parser defect the carrier already produced a
    /// faithful, usable tree for — the compiler can and should still emit
    /// output for the rest of the file. Consumers that gate compilation on
    /// "does this file have an error" must consult this flag rather than
    /// severity alone; consumers that only DISPLAY diagnostics (hover,
    /// `textDocument/publishDiagnostics`) read severity as before.
    pub blocks_compile: bool,
}

/// Source-local fields in the normative diagnostic ordering key.
#[derive(Debug, Clone, Copy)]
pub struct LanguageDiagnosticOrderKey<'a> {
    span: Span,
    severity: LanguageDiagnosticSeverity,
    code: &'a str,
    arguments: &'a [DiagnosticArg],
}

impl<'a> LanguageDiagnosticOrderKey<'a> {
    /// Build a source-local ordering key from mapped diagnostic facts.
    #[must_use]
    pub const fn new(
        span: Span,
        severity: LanguageDiagnosticSeverity,
        code: &'a str,
        arguments: &'a [DiagnosticArg],
    ) -> Self {
        Self {
            span,
            severity,
            code,
            arguments,
        }
    }
}

/// Compare two mapped diagnostics by the normative cross-frontend key.
///
/// The display message is deliberately absent. Callers sorting diagnostics
/// from one parse pass the same `ParseKey` on both sides; the two-source form
/// also supports deterministic merges without weakening source identity.
pub fn compare_language_diagnostics(
    left_source: &ParseKey,
    left: &LanguageDiagnostic,
    right_source: &ParseKey,
    right: &LanguageDiagnostic,
) -> Ordering {
    left_source.cmp(right_source).then_with(|| {
        compare_language_diagnostic_fields(
            LanguageDiagnosticOrderKey::new(left.span, left.severity, left.code, &left.arguments),
            LanguageDiagnosticOrderKey::new(
                right.span,
                right.severity,
                right.code,
                &right.arguments,
            ),
        )
    })
}

/// Compare the source-local portion of the normative diagnostic key.
///
/// Strict-rejection arbitration uses this after its stronger semantic
/// discovery-order key. Ordinary diagnostics use it after source identity.
pub fn compare_language_diagnostic_fields(
    left: LanguageDiagnosticOrderKey<'_>,
    right: LanguageDiagnosticOrderKey<'_>,
) -> Ordering {
    left.span
        .start
        .cmp(&right.span.start)
        .then_with(|| left.span.end.cmp(&right.span.end))
        .then_with(|| {
            diagnostic_severity_rank(left.severity).cmp(&diagnostic_severity_rank(right.severity))
        })
        .then_with(|| left.code.cmp(right.code))
        .then_with(|| left.arguments.cmp(right.arguments))
}

/// Sort one parse's mapped diagnostics by the normative key.
pub fn sort_language_diagnostics(parse_key: &ParseKey, diagnostics: &mut [LanguageDiagnostic]) {
    diagnostics
        .sort_by(|left, right| compare_language_diagnostics(parse_key, left, parse_key, right));
}

const fn diagnostic_severity_rank(severity: LanguageDiagnosticSeverity) -> u8 {
    match severity {
        LanguageDiagnosticSeverity::Error => 0,
        LanguageDiagnosticSeverity::Warning => 1,
        LanguageDiagnosticSeverity::Info => 2,
    }
}

/// A parse-affecting profile condition a registered frontend cannot honor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedSyntaxProfileReason {
    /// The registered frontend does not implement the requested syntax option.
    UnsupportedOption,
    /// The requested language/profile pair does not belong to this frontend.
    FrontendMismatch,
}

/// Why a parser diagnostic cannot be mapped to authored source geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSpanRejectReason {
    /// The parser supplied no authored-source anchor.
    MissingSourceAnchor,
    /// The parser supplied a range outside the exact source byte extent.
    OutsideSourceBounds,
    /// The parser supplied an offset that is not a UTF-8 boundary.
    InvalidUtf8Boundary,
}

/// A closed, inspectable refusal produced before a parsed artifact can be published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxReject {
    /// The requested syntax profile is not supported by the selected frontend.
    UnsupportedProfile {
        /// Exact parse construction identity.
        parse_key: Arc<ParseKey>,
        /// Normalized parse-option identity.
        syntax_profile: Arc<SyntaxProfileId>,
        /// Closed unsupported-profile classification.
        reason: UnsupportedSyntaxProfileReason,
    },
    /// The source contains a syntax defect whose strict grammar outcome is rejection.
    RejectedSyntax {
        /// Exact parse construction identity.
        parse_key: Arc<ParseKey>,
        /// Normalized parse-option identity.
        syntax_profile: Arc<SyntaxProfileId>,
        /// First diagnostic by the frontend's normative rejection arbitration.
        primary: Arc<LanguageDiagnostic>,
        /// Remaining mapped diagnostics participating in the same rejection.
        related: Arc<[LanguageDiagnostic]>,
    },
    /// A frontend reported a diagnostic without usable authored-source geometry.
    UnmappedDiagnostic {
        /// Exact parse construction identity.
        parse_key: Arc<ParseKey>,
        /// Normalized parse-option identity.
        syntax_profile: Arc<SyntaxProfileId>,
        /// Stable diagnostic code.
        code: &'static str,
        /// Closed mapping-failure classification.
        reason: DiagnosticSpanRejectReason,
    },
    /// The registered projection could not prove its inventory geometry.
    InvalidCarrierGeometry {
        /// Exact parse construction identity.
        parse_key: Arc<ParseKey>,
        /// Normalized parse-option identity.
        syntax_profile: Arc<SyntaxProfileId>,
        /// Structural inventory validation failure.
        error: Arc<carrier_inventory::InventoryValidationError>,
    },
}

/// Typed framework-neutral metadata shared by every carrier parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameworkParseCommon {
    /// Sole framework-neutral geometry authority.
    pub inventory: Arc<CarrierBlockInventory>,
    /// Neutral carrier-parse diagnostics. Adapters whose parse
    /// diagnostics already flow through a dedicated host channel (Vue's
    /// `ParseSnapshot` diagnostics) leave this empty rather than
    /// duplicate the same data on a second channel.
    pub diagnostics: Vec<LanguageDiagnostic>,
}

impl FrameworkParseCommon {
    /// Compatibility projection derived only from the inventory.
    pub fn script_regions(&self) -> Vec<ScriptRegion> {
        self.script_regions_for_adapter(None)
    }

    #[doc(hidden)]
    pub fn script_regions_for_adapter(
        &self,
        adapter_id: Option<&FrameworkAdapterId>,
    ) -> Vec<ScriptRegion> {
        self.inventory
            .blocks()
            .iter()
            .filter_map(|block| match block {
                CarrierBlock::Section {
                    role: SectionRole::Script { role, dialect },
                    syntax,
                    ..
                } => Some(ScriptRegion {
                    span: source_span(syntax.content_span),
                    source_type: match dialect {
                        InventoryScriptSourceType::TypeScript => ScriptSourceType::Ts,
                        InventoryScriptSourceType::Tsx => ScriptSourceType::Tsx,
                        InventoryScriptSourceType::Jsx => {
                            ScriptSourceType::Jsx(if adapter_id.is_some_and(|id| id.is_vue()) {
                                JsModuleKind::Module
                            } else {
                                JsModuleKind::Unambiguous
                            })
                        }
                        InventoryScriptSourceType::JavaScript
                        | InventoryScriptSourceType::Custom { .. }
                        | InventoryScriptSourceType::Missing => {
                            ScriptSourceType::Js(if adapter_id.is_some_and(|id| id.is_vue()) {
                                JsModuleKind::Script
                            } else {
                                JsModuleKind::Unambiguous
                            })
                        }
                    },
                    kind: match role {
                        ScriptRole::Module => ScriptRegionKind::Module,
                        ScriptRole::Instance | ScriptRole::Setup => ScriptRegionKind::Instance,
                    },
                }),
                _ => None,
            })
            .collect()
    }

    /// Compatibility projection derived only from the inventory.
    pub fn template_regions(&self) -> Vec<TemplateRegion> {
        self.inventory
            .blocks()
            .iter()
            .filter_map(|block| match block {
                CarrierBlock::Section {
                    role: SectionRole::TemplateHost,
                    syntax,
                    ..
                } => Some(TemplateRegion {
                    span: source_span(syntax.content_span),
                }),
                _ => None,
            })
            .collect()
    }

    /// Compatibility projection derived only from the inventory.
    pub fn style_regions(&self) -> Vec<StyleRegion> {
        self.inventory
            .blocks()
            .iter()
            .filter_map(|block| match block {
                CarrierBlock::Section {
                    role: SectionRole::Style { .. },
                    syntax,
                    ..
                } => Some(StyleRegion {
                    span: source_span(syntax.content_span),
                }),
                _ => None,
            })
            .collect()
    }

    /// Compatibility projection derived only from inventory attributes.
    pub fn external_links(&self) -> Vec<ExternalLink> {
        self.inventory
            .blocks()
            .iter()
            .filter_map(|block| {
                let CarrierBlock::Section { role, syntax, .. } = block else {
                    return None;
                };
                let kind = match role {
                    SectionRole::Script { .. } => ExternalLinkKind::Script,
                    SectionRole::TemplateHost => ExternalLinkKind::Template,
                    SectionRole::Style { .. } => ExternalLinkKind::Style,
                    SectionRole::Custom { .. } => return None,
                };
                let attribute = syntax
                    .attributes
                    .iter()
                    .find_map(|attribute| match attribute {
                        CarrierAttribute::Named { name, value, .. }
                            if self.inventory.normalized_name(name.normalized).ok()
                                == Some("src") =>
                        {
                            Some(value)
                        }
                        _ => None,
                    })?;
                let AttributeValue::Static { raw, .. } = attribute else {
                    return None;
                };
                Some(ExternalLink {
                    kind,
                    specifier: self.inventory.slice(*raw).ok()?.to_owned(),
                    span: Some(source_span(raw.span)),
                })
            })
            .collect()
    }
}

const fn source_span(span: carrier_inventory::SourceSpan) -> Span {
    Span::new(span.start, span.end)
}

/// The type-erased adapter-owned carrier parse payload.
///
/// Implemented by each adapter's concrete carrier (e.g. the
/// compiler-owned `VueParseCarrier`). The trait is an erasure seam, not
/// an API: the only member is the hidden `Any` bridge the doc-hidden
/// downcast helpers use.
pub trait CarrierParse: Any + Send + Sync {
    /// Hidden `Any` bridge for the doc-hidden downcast helpers.
    #[doc(hidden)]
    fn __verter_as_any(&self) -> &dyn Any;

    /// Hidden `Arc`-preserving `Any` bridge for the doc-hidden `Arc`
    /// downcast helper.
    #[doc(hidden)]
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

/// A frontend parse that has not yet received registered carrier geometry.
///
/// This value deliberately has no inventory or block lookup surface. The
/// compiler-owned registered projector consumes it together with registered
/// source facts before any geometry-sensitive downstream API can observe it.
pub struct UnregisteredFrameworkParseArtifact {
    /// Owning adapter (open set).
    pub adapter_id: FrameworkAdapterId,
    /// Concrete language within the adapter (open set).
    pub language_id: LanguageId,
    /// Exact syntax construction identity.
    pub parse_key: Arc<ParseKey>,
    /// Normalized parse-option identity.
    pub syntax_profile: Arc<SyntaxProfileId>,
    /// Mapped frontend diagnostics retained across registered projection.
    pub diagnostics: Vec<LanguageDiagnostic>,
    /// Private type-erased adapter parse payload.
    carrier: Arc<dyn CarrierParse>,
}

impl UnregisteredFrameworkParseArtifact {
    /// Construct a frontend parse without registered geometry.
    pub fn new(
        adapter_id: FrameworkAdapterId,
        language_id: LanguageId,
        parse_key: Arc<ParseKey>,
        syntax_profile: Arc<SyntaxProfileId>,
        diagnostics: Vec<LanguageDiagnostic>,
        carrier: Arc<dyn CarrierParse>,
    ) -> Self {
        Self {
            adapter_id,
            language_id,
            parse_key,
            syntax_profile,
            diagnostics,
            carrier,
        }
    }
}

impl fmt::Debug for UnregisteredFrameworkParseArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnregisteredFrameworkParseArtifact")
            .field("adapter_id", &self.adapter_id)
            .field("language_id", &self.language_id)
            .field("parse_key", &self.parse_key)
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

/// Raw carrier downcast (reference form).
///
/// Returns the erased payload as a `T` when the concrete type matches — a
/// foreign adapter's artifact carries a DIFFERENT concrete `CarrierParse`
/// type, so the `Any` downcast already fails structurally for it; no
/// separate adapter-identity gate is needed on top of the type witness.
/// Doc-hidden: production code routes through each adapter's own typed
/// accessor (see the `carrier_downcast_confined_to_owning_adapter` guard).
#[doc(hidden)]
pub fn __carrier_downcast_ref<T: CarrierParse>(
    artifact: &UnregisteredFrameworkParseArtifact,
) -> Option<&T> {
    artifact.carrier.__verter_as_any().downcast_ref::<T>()
}

/// Raw carrier downcast (`Arc` form).
#[doc(hidden)]
pub fn __carrier_downcast_arc<T: CarrierParse>(
    artifact: &UnregisteredFrameworkParseArtifact,
) -> Option<Arc<T>> {
    Arc::clone(&artifact.carrier)
        .__verter_as_any_arc()
        .downcast::<T>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileLanguage;

    #[derive(Debug)]
    struct TestCarrier {
        value: u32,
    }

    impl CarrierParse for TestCarrier {
        fn __verter_as_any(&self) -> &dyn Any {
            self
        }
        fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    #[derive(Debug)]
    struct OtherCarrier;

    impl CarrierParse for OtherCarrier {
        fn __verter_as_any(&self) -> &dyn Any {
            self
        }
        fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    fn vue_artifact() -> UnregisteredFrameworkParseArtifact {
        let language = FileLanguage::vue();
        let syntax_profile =
            crate::syntax_profile_id_for(&language, &crate::ParseOptions::default())
                .expect("Vue syntax profile");
        let parse_key = crate::parse_key_for(
            "",
            &language,
            crate::VUE_SYNTAX_COMPATIBILITY_DOMAIN,
            crate::VUE_SYNTAX_COMPATIBILITY_EPOCH,
            &syntax_profile,
        )
        .expect("Vue parse key");
        UnregisteredFrameworkParseArtifact::new(
            FrameworkAdapterId::vue(),
            LanguageId::new("vue"),
            Arc::new(parse_key),
            Arc::new(syntax_profile),
            Vec::new(),
            Arc::new(TestCarrier { value: 7 }),
        )
    }

    #[test]
    fn owning_type_downcasts_to_typed_carrier() {
        let artifact = vue_artifact();
        let carrier = __carrier_downcast_ref::<TestCarrier>(&artifact).expect("owning downcast");
        assert_eq!(carrier.value, 7);

        let arc = __carrier_downcast_arc::<TestCarrier>(&artifact).expect("owning Arc downcast");
        assert_eq!(arc.value, 7);
    }

    #[test]
    fn wrong_carrier_type_downcast_returns_none() {
        let artifact = vue_artifact();
        assert!(
            __carrier_downcast_ref::<OtherCarrier>(&artifact).is_none(),
            "a mismatched concrete carrier type must downcast to None"
        );
    }

    #[test]
    fn debug_does_not_leak_the_carrier() {
        let rendered = format!("{:?}", vue_artifact());
        assert!(rendered.contains("UnregisteredFrameworkParseArtifact"));
        assert!(
            !rendered.contains("TestCarrier"),
            "Debug must not expose the erased carrier payload"
        );
    }
}
