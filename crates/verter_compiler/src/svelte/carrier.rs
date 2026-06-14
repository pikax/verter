//! The Svelte carrier bridge.
//!
//! Owns [`SvelteParseCarrier`] — the concrete [`CarrierParse`] payload wrapping
//! a [`ParsedSvelte`] — and [`build_svelte_parse_artifact`], the producer that
//! lifts a parse into the framework-neutral [`FrameworkParseArtifact`] (typed
//! script regions for BOTH the instance and module `<script>` blocks, plus the
//! `<style>` regions). The instance script is the runes-or-legacy component
//! body; the module script is `<script module>` (5.5) / legacy
//! `<script context="module">`.
//!
//! [`SvelteCarrierCompiler`] is the second [`CarrierCompiler`] (Vue is the
//! reference). `parse` produces the neutral artifact, `eval_source` blanks
//! everything but BOTH script contents at their raw offsets (output length ==
//! input length), and `template_data` returns empty neutral facts (the Svelte
//! template-fact extraction is a later vertical). `compile_ide` returns the
//! typed unsupported answer until the IDE TSX projection (B8c) lands.

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    CarrierParse, ExternalLink, ExternalLinkKind, FrameworkAdapterId, FrameworkParseArtifact,
    FrameworkParseCommon, JsModuleKind, LanguageId, ScriptRegion, ScriptRegionKind,
    ScriptSourceType, StyleRegion,
};
use verter_span::Span;

use crate::framework_common::carrier_compiler::{
    CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput, ParseOptions, TemplateFacts,
};
use crate::framework_common::ctx::{receive_svelte_carrier_token, CarrierCompilerCtx};

use super::parser::{parse_svelte, ParsedSvelte, SvelteScript};

/// The concrete Svelte carrier: the parsed component behind the erasure seam.
#[derive(Debug)]
pub struct SvelteParseCarrier {
    parsed: Arc<ParsedSvelte>,
}

impl SvelteParseCarrier {
    /// Wrap a parsed Svelte component.
    #[must_use]
    pub fn new(parsed: Arc<ParsedSvelte>) -> Self {
        Self { parsed }
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
/// Svelte components are TypeScript-or-JavaScript; `lang="ts"` (or absent) is
/// TypeScript, `lang="tsx"` is TSX, `lang="jsx"`/`lang="js"` map to their JS
/// dialects. A `.svelte` script is module-grammar (top-level `import`/`export`
/// allowed), so JS dialects resolve the module module-kind.
fn svelte_script_source_type(script: Option<&SvelteScript>) -> ScriptSourceType {
    match script.and_then(|s| s.lang.as_deref()) {
        Some(lang) if lang.eq_ignore_ascii_case("tsx") => ScriptSourceType::Tsx,
        Some(lang) if lang.eq_ignore_ascii_case("jsx") => {
            ScriptSourceType::Jsx(JsModuleKind::Module)
        }
        Some(lang) if lang.eq_ignore_ascii_case("js") => ScriptSourceType::Js(JsModuleKind::Module),
        _ => ScriptSourceType::Ts,
    }
}

/// The Svelte carrier parser version stamped on produced artifacts.
pub const SVELTE_CARRIER_PARSER_VERSION: u32 = 1;

/// Lift a parsed Svelte component into the framework-neutral parse artifact.
///
/// The neutral common surface carries:
/// * one [`ScriptRegion`] per `<script>` block — `<script module>` →
///   [`ScriptRegionKind::Module`], the instance `<script>` →
///   [`ScriptRegionKind::Instance`] — each stamped with the block's resolved
///   [`ScriptSourceType`]; regions are SOURCE-ordered;
/// * one [`StyleRegion`] per component `<style>` block;
/// * external `src` links for script/style blocks (Svelte rarely uses `src`,
///   but the producer records them uniformly).
#[must_use]
pub fn build_svelte_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSvelte>,
    parser_version: u32,
) -> Arc<FrameworkParseArtifact> {
    let mut script_regions = Vec::new();
    let mut external_links = Vec::new();

    for script in [
        parsed.instance_script.as_ref(),
        parsed.module_script.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        let source_type = svelte_script_source_type(Some(script));
        let span = script
            .content
            .unwrap_or_else(|| Span::new(script.tag_open.end, script.tag_open.end));
        script_regions.push(ScriptRegion {
            span,
            source_type,
            kind: if script.is_module {
                ScriptRegionKind::Module
            } else {
                ScriptRegionKind::Instance
            },
        });
        if let Some((specifier, link_span)) = script_src(script, source) {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Script,
                specifier,
                span: Some(link_span),
            });
        }
    }
    // Source-ordered (the parser already discovers them in source order, but a
    // module script may precede the instance one).
    script_regions.sort_by_key(|region| region.span.start);

    let mut style_regions = Vec::new();
    for style in &parsed.styles {
        let span = style
            .content
            .unwrap_or_else(|| Span::new(style.tag_open.end, style.tag_open.end));
        style_regions.push(StyleRegion { span });
    }

    Arc::new(FrameworkParseArtifact::new(
        FrameworkAdapterId::svelte(),
        LanguageId::new("svelte"),
        parser_version,
        FrameworkParseCommon {
            script_regions,
            template_regions: Vec::new(),
            style_regions,
            external_links,
            diagnostics: Vec::new(),
        },
        Arc::new(SvelteParseCarrier::new(parsed)),
    ))
}

/// Read a `src="..."` specifier off a script block's attributes.
fn script_src(script: &SvelteScript, source: &str) -> Option<(String, Span)> {
    use super::parser::{SvelteAttributeKind, SvelteAttributeValue};
    script.attributes.iter().find_map(|attr| match &attr.kind {
        SvelteAttributeKind::Plain {
            name,
            value: Some(SvelteAttributeValue::Text(span)),
            ..
        } if name.eq_ignore_ascii_case("src") => Some((
            source[span.start as usize..span.end as usize].to_string(),
            *span,
        )),
        _ => None,
    })
}

/// The Svelte carrier compiler — the second [`CarrierCompiler`].
pub struct SvelteCarrierCompiler {
    ctx: CarrierCompilerCtx,
}

impl std::fmt::Debug for SvelteCarrierCompiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvelteCarrierCompiler")
            .finish_non_exhaustive()
    }
}

impl Default for SvelteCarrierCompiler {
    fn default() -> Self {
        Self {
            ctx: CarrierCompilerCtx::new(receive_svelte_carrier_token()),
        }
    }
}

impl SvelteCarrierCompiler {
    /// Reach the parsed component back out of a Svelte artifact, or `None` when
    /// the artifact is not a Svelte carrier.
    #[must_use]
    fn parsed_svelte<'a>(&self, artifact: &'a FrameworkParseArtifact) -> Option<&'a ParsedSvelte> {
        self.ctx
            .carrier_for::<SvelteParseCarrier>(artifact)
            .map(|carrier| carrier.parsed())
    }
}

impl CarrierCompiler for SvelteCarrierCompiler {
    fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::svelte()
    }

    fn carrier_language_id(&self) -> LanguageId {
        LanguageId::new("svelte")
    }

    fn parse(&self, source: &str, _opts: &ParseOptions) -> Arc<FrameworkParseArtifact> {
        let parsed = Arc::new(parse_svelte(source));
        build_svelte_parse_artifact(source, parsed, SVELTE_CARRIER_PARSER_VERSION)
    }

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        // Position-preserving blanking: every byte starts blanked (line
        // terminators preserved so line/column geometry is unchanged), then
        // each script region's RAW bytes are stamped back over their carrier-
        // absolute offsets. BOTH the instance and module script blocks are
        // preserved; everything else (template, styles) is blanked. Output
        // length == input length by construction.
        let src = source.as_bytes();
        let mut out: Vec<u8> = src
            .iter()
            .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
            .collect();
        for region in &artifact.common.script_regions {
            let start = region.span.start as usize;
            let end = region.span.end as usize;
            if start <= end && end <= src.len() {
                out[start..end].copy_from_slice(&src[start..end]);
            }
        }
        Arc::from(
            String::from_utf8(out)
                .unwrap_or_else(|_| source.to_string())
                .as_str(),
        )
    }

    fn compile_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &IdeCompileOptions,
    ) -> Result<IdeOutput, CompileUnsupported> {
        let _ = (source, opts);
        // The Svelte IDE TSX projection is a later vertical (B8c). Until it
        // lands the carrier declines the IDE compile with the typed answer —
        // never a silent empty output. A foreign artifact (not a Svelte
        // carrier) also declines.
        let _ = self.parsed_svelte(artifact);
        Err(CompileUnsupported::NoIdeProjection {
            adapter_id: self.adapter_id(),
        })
    }

    fn template_data(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts {
        let _ = (source, artifact);
        // Svelte template-fact extraction (component usages, bindings) is a
        // later vertical; the honest answer here is empty neutral facts.
        TemplateFacts::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        let parsed = Arc::new(parse_svelte(source));
        build_svelte_parse_artifact(source, parsed, SVELTE_CARRIER_PARSER_VERSION)
    }

    #[test]
    fn artifact_identity_names_the_svelte_adapter() {
        let artifact = artifact_for("<script>let a = 1;</script>");
        assert!(!artifact.adapter_id.is_vue());
        assert_eq!(artifact.adapter_id, FrameworkAdapterId::svelte());
        assert_eq!(artifact.language_id.as_str(), "svelte");
        assert_eq!(artifact.parser_version, SVELTE_CARRIER_PARSER_VERSION);
    }

    #[test]
    fn instance_and_module_scripts_carry_kind_and_source_type() {
        let source =
            "<script module>export const x = 1;</script>\n<script lang=\"ts\">let a = 1;</script>";
        let artifact = artifact_for(source);
        let regions = &artifact.common.script_regions;
        assert_eq!(regions.len(), 2);
        // Source-ordered: module script first.
        assert_eq!(regions[0].kind, ScriptRegionKind::Module);
        assert_eq!(regions[1].kind, ScriptRegionKind::Instance);
        assert_eq!(regions[0].span.slice(source).trim(), "export const x = 1;");
        assert_eq!(regions[1].span.slice(source).trim(), "let a = 1;");
    }

    #[test]
    fn legacy_context_module_classifies_as_module() {
        let source = "<script context=\"module\">export const x = 1;</script>";
        let artifact = artifact_for(source);
        assert_eq!(artifact.common.script_regions.len(), 1);
        assert_eq!(
            artifact.common.script_regions[0].kind,
            ScriptRegionKind::Module
        );
    }

    #[test]
    fn eval_source_is_position_preserving_with_both_scripts_at_raw_offsets() {
        let source = "<script module>export const x = 1;</script>\n<div>{count}</div>\n<script lang=\"ts\">let count = 0;</script>";
        let compiler = SvelteCarrierCompiler::default();
        let artifact = compiler.parse(source, &ParseOptions::default());
        let eval = compiler.eval_source(source, &artifact);
        assert_eq!(eval.len(), source.len(), "eval source must be same length");
        for region in &artifact.common.script_regions {
            let (s, e) = (region.span.start as usize, region.span.end as usize);
            assert_eq!(
                &eval[s..e],
                &source[s..e],
                "script bytes preserved at raw offsets"
            );
        }
        // The `<div>` markup is blanked (no `<` survives outside script).
        let markup_idx = source.find("<div>").unwrap();
        assert_eq!(eval.as_bytes()[markup_idx], b' ', "markup is blanked");
    }

    #[test]
    fn compile_ide_returns_typed_unsupported_until_b8c() {
        let compiler = SvelteCarrierCompiler::default();
        let source = "<script>let a = 1;</script>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let err = compiler
            .compile_ide(source, &artifact, &IdeCompileOptions::default())
            .expect_err("the Svelte IDE projection is not yet implemented");
        assert!(matches!(err, CompileUnsupported::NoIdeProjection { .. }));
    }

    #[test]
    fn template_data_is_empty_for_now() {
        let compiler = SvelteCarrierCompiler::default();
        let source = "<script>let a = 1;</script>\n<Child />";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let facts = compiler.template_data(source, &artifact);
        assert!(facts.data.components.is_empty());
    }
}
