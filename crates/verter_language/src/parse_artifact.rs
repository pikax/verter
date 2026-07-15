//! The open framework parse artifact.
//!
//! [`FrameworkParseArtifact`] is the single framework-neutral post-parse
//! payload every host carrier slot stores (`framework_parse:
//! Option<Arc<FrameworkParseArtifact>>`): typed common metadata
//! ([`FrameworkParseCommon`] — script/template/style regions, external
//! links, diagnostics) plus a PRIVATE type-erased carrier
//! (`Arc<dyn CarrierParse>`) holding the owning adapter's full parse
//! result (e.g. Vue's `ParsedSfc` behind the compiler-owned
//! `VueParseCarrier`).
//!
//! Carrier privacy is public-hidden + token-gated + statically guarded:
//!
//! * the raw downcast helpers ([`__carrier_downcast_ref`] /
//!   [`__carrier_downcast_arc`]) are `#[doc(hidden)]` and verify the
//!   caller's [`CarrierAccessToken`] names the artifact's own adapter;
//! * [`CarrierAccessToken`] is minted ONLY inside this crate, during
//!   `LanguageRegistry` carrier-row construction
//!   (`LanguageRow::carrier`), and returned exactly once to the
//!   registry-construction caller as the carrier row's registration
//!   proof — the non-public `_private: ()` field keeps out-of-crate
//!   struct literals uncompilable, and NO public arbitrary-id
//!   constructor and NO public by-id token lookup exist;
//! * the `carrier_downcast_confined_to_owning_adapter` and
//!   `carrier_access_token_minted_only_in_verter_language` static
//!   guards (in `verter_session`'s architecture-guard suite) are the
//!   enforcement authority across crate seams, where a literal
//!   `pub(crate)` cannot compile.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use verter_span::Span;

use crate::ids::{FrameworkAdapterId, LanguageId};
use crate::language::ScriptSourceType;

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
    /// Human-readable message.
    pub message: String,
}

/// Typed framework-neutral metadata shared by every carrier parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameworkParseCommon {
    /// Embedded script regions, in source order.
    pub script_regions: Vec<ScriptRegion>,
    /// Template regions, in source order.
    pub template_regions: Vec<TemplateRegion>,
    /// Style regions, in source order.
    pub style_regions: Vec<StyleRegion>,
    /// External-source references declared by carrier blocks.
    pub external_links: Vec<ExternalLink>,
    /// Neutral carrier-parse diagnostics. Adapters whose parse
    /// diagnostics already flow through a dedicated host channel (Vue's
    /// `ParseSnapshot` diagnostics) leave this empty rather than
    /// duplicate the same data on a second channel.
    pub diagnostics: Vec<LanguageDiagnostic>,
}

/// The type-erased adapter-owned carrier parse payload.
///
/// Implemented by each adapter's concrete carrier (e.g. the
/// compiler-owned `VueParseCarrier`). The trait is an erasure seam, not
/// an API: the only member is the hidden `Any` bridge the token-gated
/// downcast helpers use.
pub trait CarrierParse: Any + Send + Sync {
    /// Hidden `Any` bridge for the token-gated downcast helpers.
    #[doc(hidden)]
    fn __verter_as_any(&self) -> &dyn Any;

    /// Hidden `Arc`-preserving `Any` bridge for the token-gated `Arc`
    /// downcast helper.
    #[doc(hidden)]
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

/// The single framework-neutral post-parse artifact.
///
/// `carrier` is private by design: consumers read the typed
/// [`FrameworkParseCommon`] surface; ONLY the owning adapter reaches
/// its concrete parse through the blessed token-gated `carrier_for`
/// wrappers.
pub struct FrameworkParseArtifact {
    /// Owning adapter (open set).
    pub adapter_id: FrameworkAdapterId,
    /// Concrete language within the adapter (open set).
    pub language_id: LanguageId,
    /// Version of the adapter's parser that produced this artifact.
    pub parser_version: u32,
    /// Typed framework-neutral metadata.
    pub common: FrameworkParseCommon,
    /// Private type-erased adapter parse payload.
    carrier: Arc<dyn CarrierParse>,
}

impl FrameworkParseArtifact {
    /// Construct an artifact. Construction is open (producers live in
    /// adapter crates); only DOWNCAST of the erased carrier is
    /// token-gated.
    pub fn new(
        adapter_id: FrameworkAdapterId,
        language_id: LanguageId,
        parser_version: u32,
        common: FrameworkParseCommon,
        carrier: Arc<dyn CarrierParse>,
    ) -> Self {
        Self {
            adapter_id,
            language_id,
            parser_version,
            common,
            carrier,
        }
    }
}

impl fmt::Debug for FrameworkParseArtifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrameworkParseArtifact")
            .field("adapter_id", &self.adapter_id)
            .field("language_id", &self.language_id)
            .field("parser_version", &self.parser_version)
            .field("common", &self.common)
            .finish_non_exhaustive()
    }
}

/// Capability token proving the holder is the registered owner of an
/// adapter's carrier payload.
///
/// Minted ONLY inside `verter_language`, during `LanguageRegistry`
/// carrier-row construction, and returned exactly once to the
/// registry-construction caller as the carrier row's registration
/// proof. The non-public `_private: ()` field keeps out-of-crate
/// struct literals uncompilable; there is deliberately NO public
/// arbitrary-id constructor (`new(adapter_id)` / `From` / `Default`)
/// and NO public by-id token lookup — consumers RECEIVE the token,
/// never construct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierAccessToken {
    adapter_id: FrameworkAdapterId,
    _private: (),
}

impl CarrierAccessToken {
    /// The adapter this token grants carrier access for.
    pub fn adapter_id(&self) -> &FrameworkAdapterId {
        &self.adapter_id
    }
}

/// The crate-private token factory — the SOLE minting authority's
/// named construction point. Called only from `LanguageRow::carrier`
/// during registry carrier-row construction.
pub(crate) fn mint_carrier_access_token(adapter_id: FrameworkAdapterId) -> CarrierAccessToken {
    CarrierAccessToken {
        adapter_id,
        _private: (),
    }
}

/// Token-gated raw carrier downcast (reference form).
///
/// Returns the typed carrier ONLY when `token` names the artifact's own
/// adapter AND the erased payload is a `T`. Hidden: production code
/// routes through the blessed `carrier_for::<T>` wrappers (see the
/// `carrier_downcast_confined_to_owning_adapter` guard).
#[doc(hidden)]
pub fn __carrier_downcast_ref<'a, T: CarrierParse>(
    artifact: &'a FrameworkParseArtifact,
    token: &CarrierAccessToken,
) -> Option<&'a T> {
    if artifact.adapter_id != token.adapter_id {
        return None;
    }
    artifact.carrier.__verter_as_any().downcast_ref::<T>()
}

/// Token-gated raw carrier downcast (`Arc` form).
#[doc(hidden)]
pub fn __carrier_downcast_arc<T: CarrierParse>(
    artifact: &FrameworkParseArtifact,
    token: &CarrierAccessToken,
) -> Option<Arc<T>> {
    if artifact.adapter_id != token.adapter_id {
        return None;
    }
    Arc::clone(&artifact.carrier)
        .__verter_as_any_arc()
        .downcast::<T>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::LanguageRow;
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

    fn vue_artifact() -> FrameworkParseArtifact {
        FrameworkParseArtifact::new(
            FrameworkAdapterId::vue(),
            LanguageId::new("vue"),
            1,
            FrameworkParseCommon::default(),
            Arc::new(TestCarrier { value: 7 }),
        )
    }

    fn token_for(language: FileLanguage) -> CarrierAccessToken {
        let extension = match &language {
            FileLanguage::Framework { language_id, .. } => language_id.as_str().to_string(),
            _ => panic!("carrier language required"),
        };
        let (_row, token) = LanguageRow::carrier(&extension, language);
        token
    }

    #[test]
    fn owning_adapter_token_downcasts_to_typed_carrier() {
        let artifact = vue_artifact();
        let token = token_for(FileLanguage::vue());
        let carrier =
            __carrier_downcast_ref::<TestCarrier>(&artifact, &token).expect("owning downcast");
        assert_eq!(carrier.value, 7);

        let arc =
            __carrier_downcast_arc::<TestCarrier>(&artifact, &token).expect("owning Arc downcast");
        assert_eq!(arc.value, 7);
    }

    #[test]
    fn wrong_adapter_token_downcast_returns_none() {
        let artifact = vue_artifact();
        let svelte_token = token_for(FileLanguage::svelte());
        assert!(
            __carrier_downcast_ref::<TestCarrier>(&artifact, &svelte_token).is_none(),
            "a token minted for another adapter must NOT open the carrier"
        );
        assert!(
            __carrier_downcast_arc::<TestCarrier>(&artifact, &svelte_token).is_none(),
            "the Arc form must apply the same adapter gate"
        );
    }

    #[test]
    fn wrong_carrier_type_downcast_returns_none() {
        let artifact = vue_artifact();
        let token = token_for(FileLanguage::vue());
        assert!(
            __carrier_downcast_ref::<OtherCarrier>(&artifact, &token).is_none(),
            "a mismatched concrete carrier type must downcast to None"
        );
    }

    #[test]
    fn debug_does_not_leak_the_carrier() {
        let rendered = format!("{:?}", vue_artifact());
        assert!(rendered.contains("FrameworkParseArtifact"));
        assert!(
            !rendered.contains("TestCarrier"),
            "Debug must not expose the erased carrier payload"
        );
    }
}
