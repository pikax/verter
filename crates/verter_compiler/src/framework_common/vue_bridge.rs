//! The Vue carrier bridge.
//!
//! Owns [`VueParseCarrier`] — the concrete [`CarrierParse`] payload
//! wrapping a parsed Vue SFC — and [`build_vue_parse_artifact`], the
//! Vue producer that lifts a [`ParsedSfc`] into the framework-neutral
//! [`FrameworkParseArtifact`] at parse time (typed script/template/
//! style regions, external links, the resolved per-region
//! [`ScriptSourceType`]).
//!
//! Consumers outside the Vue adapter read the artifact's typed
//! [`FrameworkParseCommon`] surface; the typed carrier is reachable
//! ONLY through the blessed token-gated `carrier_for::<VueParseCarrier>`
//! wrappers (see the `carrier_downcast_confined_to_owning_adapter`
//! architecture guard).

use std::any::Any;
use std::sync::Arc;

use verter_language::{
    CarrierParse, ExternalLink, ExternalLinkKind, FrameworkAdapterId, FrameworkParseArtifact,
    FrameworkParseCommon, JsModuleKind, LanguageId, ScriptRegion, ScriptRegionKind,
    ScriptSourceType, StyleRegion, TemplateRegion,
};
use verter_span::Span;

use verter_parser::parser::types::ParsedSfc;
use verter_parser::types::NodeProp;

use crate::compile::types::{CodegenOptions, CompileTarget, VerterCompileOptions};
use crate::compile::{compile_from_parsed, parse_sfc};
use crate::framework_common::carrier_compiler::{
    CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput, ParseOptions,
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeCustomBlock, RuntimeDiagnostic,
    RuntimeMainModule, RuntimeScriptBlock, RuntimeStyleBlock, RuntimeTemplateBlock, TemplateFacts,
};
use crate::framework_common::ctx::{receive_vue_carrier_token, CarrierCompilerCtx};

/// The concrete Vue carrier: the full parsed SFC behind the erasure
/// seam.
#[derive(Debug)]
pub struct VueParseCarrier {
    parsed: Arc<ParsedSfc>,
}

impl VueParseCarrier {
    /// Wrap a parsed SFC.
    pub fn new(parsed: Arc<ParsedSfc>) -> Self {
        Self { parsed }
    }

    /// The wrapped parse result.
    pub fn parsed(&self) -> &ParsedSfc {
        &self.parsed
    }

    /// The wrapped parse result, as the shared handle.
    pub fn parsed_arc(&self) -> &Arc<ParsedSfc> {
        &self.parsed
    }
}

impl CarrierParse for VueParseCarrier {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }
    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

/// Attribute lookup mirroring the session's historical
/// `extract_attrs` + `find_attr` semantics byte-for-byte: names match
/// case-insensitively, a present-but-empty value reads as `"true"`.
/// Returns the value string plus the value span when one exists.
fn attr_value(props: &[NodeProp], source: &str, name: &str) -> Option<(String, Option<Span>)> {
    props.iter().find_map(|p| {
        let attr_name = &source[p.start as usize..p.name_end as usize];
        if !attr_name.eq_ignore_ascii_case(name) {
            return None;
        }
        match (p.value_start, p.value_end) {
            (Some(s), Some(e)) => {
                let value = &source[s as usize..e as usize];
                if value.is_empty() {
                    Some(("true".to_string(), Some(Span::new(s, e))))
                } else {
                    Some((value.to_string(), Some(Span::new(s, e))))
                }
            }
            _ => Some(("true".to_string(), None)),
        }
    })
}

/// Resolve the SFC's script source type from `<script lang>` data —
/// the first script block (plain `<script>` before `<script setup>`)
/// carrying an explicit `lang` attribute decides; `tsx`/`jsx`/`js`
/// (ASCII-case-insensitively) map to their dialects, anything else is
/// TypeScript.
pub fn vue_script_source_type(parsed: &ParsedSfc, source: &str) -> ScriptSourceType {
    let lang = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .find_map(|script| {
            attr_value(&script.attributes, source, "lang")
                .map(|(value, _)| value)
                .filter(|v| v != "true")
        });

    // The JS module kinds pin the historical Vue carrier dialects:
    // `lang="js"` resolves the classic-script grammar
    // (`JsModuleKind::Script`) and `lang="jsx"` the module grammar
    // (`JsModuleKind::Module`) — the exact OXC `SourceType`s
    // (`script()` / `jsx()`) the Vue parse pipeline has always
    // computed for these rows.
    match lang.as_deref().map(|value| value.to_ascii_lowercase()) {
        Some(lang) if lang == "tsx" => ScriptSourceType::Tsx,
        Some(lang) if lang == "jsx" => ScriptSourceType::Jsx(JsModuleKind::Module),
        Some(lang) if lang == "js" => ScriptSourceType::Js(JsModuleKind::Script),
        _ => ScriptSourceType::Ts,
    }
}

/// Lift a parsed Vue SFC into the framework-neutral parse artifact.
///
/// The ONE Vue carrier producer: every host slot that stores a Vue
/// parse stores the artifact this function builds. The typed common
/// surface carries:
///
/// * one [`ScriptRegion`] per script block (plain `<script>` →
///   [`ScriptRegionKind::Module`], `<script setup>` →
///   [`ScriptRegionKind::Instance`]), each stamped with the SFC's
///   resolved [`ScriptSourceType`] (Vue requires every script block of
///   one SFC to share a dialect, so the SFC-level resolution is the
///   per-region truth);
/// * template/style content regions in source order;
/// * external `src` links for script/template/style blocks;
/// * NO duplicated diagnostics — Vue's parse diagnostics flow through
///   the host's existing `ParseSnapshot` channel; mirroring them here
///   would create a second source for the same data.
pub fn build_vue_parse_artifact(
    source: &str,
    parsed: Arc<ParsedSfc>,
    parser_version: u32,
) -> Arc<FrameworkParseArtifact> {
    let source_type = vue_script_source_type(&parsed, source);

    let mut script_regions = Vec::new();
    let mut external_links = Vec::new();

    for script in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
    {
        let span = script
            .content
            .unwrap_or_else(|| Span::new(script.tag_open.end, script.tag_open.end));
        script_regions.push(ScriptRegion {
            span,
            source_type,
            kind: if script.is_setup {
                ScriptRegionKind::Instance
            } else {
                ScriptRegionKind::Module
            },
        });
        if let Some(src_span) = script.src {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Script,
                specifier: source[src_span.start as usize..src_span.end as usize].to_string(),
                span: Some(src_span),
            });
        }
    }
    // `FrameworkParseCommon.script_regions` is SOURCE-ordered: a Vue file
    // may place `<script setup>` before the plain `<script>`, while the
    // parser accessors expose them in fixed plain-then-setup order.
    script_regions.sort_by_key(|region| region.span.start);

    let mut template_regions = Vec::new();
    if let Some(template) = parsed.template_ast() {
        let span = template
            .root
            .content
            .as_ref()
            .map(|content| Span::new(content.start, content.end))
            .unwrap_or_else(|| Span::new(template.root.tag_open.end, template.root.tag_open.end));
        template_regions.push(TemplateRegion { span });
        if let Some((specifier, span)) = attr_value(&template.root.attributes, source, "src") {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Template,
                specifier,
                span,
            });
        }
    }

    let mut style_regions = Vec::new();
    for style in parsed.style_nodes() {
        let span = style
            .content
            .unwrap_or_else(|| Span::new(style.tag_open.end, style.tag_open.end));
        style_regions.push(StyleRegion { span });
        if let Some((specifier, span)) = attr_value(&style.attributes, source, "src") {
            external_links.push(ExternalLink {
                kind: ExternalLinkKind::Style,
                specifier,
                span,
            });
        }
    }

    Arc::new(FrameworkParseArtifact::new(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        parser_version,
        FrameworkParseCommon {
            script_regions,
            template_regions,
            style_regions,
            external_links,
            diagnostics: Vec::new(),
        },
        Arc::new(VueParseCarrier::new(parsed)),
    ))
}

/// Vue-PRIVATE resolved runtime-compile inputs, carried opaquely through
/// [`RuntimeCompileOptions::framework_extras`](crate::framework_common::RuntimeCompileOptions::framework_extras)
/// and downcast here.
///
/// These are the host-resolved cross-file inputs the Vue runtime compile
/// consumes: `external_types` (the resolved macro-type surface for
/// `defineProps<ExternalType>()`), `prop_constness_overrides`, and
/// `style_v_bind_vars`. They live HERE (the Vue module) rather than on the
/// neutral [`RuntimeCompileOptions`] so Vue's eager type-surface output type
/// never enters the cross-framework carrier contract — a non-Vue carrier never
/// names or sees it.
#[derive(Debug, Default)]
pub struct VueRuntimeCompileExtras {
    /// Pre-resolved external macro types, keyed by type name. The host resolves
    /// these before the compile; the Vue codegen merges them into its
    /// type-resolution context.
    pub external_types: Option<
        rustc_hash::FxHashMap<String, crate::utils::oxc::script::type_surface::ResolvedElements>,
    >,
    /// Props known const across all call sites (cross-file analysis).
    pub prop_constness_overrides: Option<rustc_hash::FxHashSet<String>>,
    /// Binding names referenced in style `v-bind()` expressions.
    pub style_v_bind_vars: Vec<String>,
}

/// The Vue carrier parser version stamped on the produced artifact's
/// `parser_version` field. Bumps invalidate every Vue artifact whose
/// post-parse shape this producer owns.
///
/// The session's `LEGACY_PARSER_VERSION` (the `FileArtifactStore` legacy
/// key dimension) and this constant are conceptually distinct — one keys
/// the store, the other stamps the artifact — and currently agree by
/// value, so the rehoused dispatch produces byte-identical artifacts.
pub const VUE_CARRIER_PARSER_VERSION: u32 = 3;

/// The Vue carrier compiler — the reference [`CarrierCompiler`].
///
/// Delegates call-for-call to the existing Vue pipeline (`parse_sfc` +
/// `compile_from_parsed`): it edits NO Vue parser or codegen module and
/// reaches the parsed SFC back out of the type-erased artifact through
/// the compiler-side blessed [`CarrierCompilerCtx`] downcast (D-m).
pub struct VueCarrierCompiler {
    ctx: CarrierCompilerCtx,
}

impl std::fmt::Debug for VueCarrierCompiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VueCarrierCompiler").finish_non_exhaustive()
    }
}

impl Default for VueCarrierCompiler {
    fn default() -> Self {
        Self {
            ctx: CarrierCompilerCtx::new(receive_vue_carrier_token()),
        }
    }
}

impl VueCarrierCompiler {
    /// Reach the parsed SFC back out of a Vue artifact, or `None` when the
    /// artifact is not a Vue carrier (foreign adapter id or non-Vue
    /// erased payload). The blessed downcast home for the Vue bridge.
    fn parsed_sfc<'a>(&self, artifact: &'a FrameworkParseArtifact) -> Option<&'a ParsedSfc> {
        self.ctx
            .carrier_for::<VueParseCarrier>(artifact)
            .map(|carrier| carrier.parsed())
    }
}

impl CarrierCompiler for VueCarrierCompiler {
    fn adapter_id(&self) -> FrameworkAdapterId {
        FrameworkAdapterId::vue()
    }

    fn carrier_language_id(&self) -> LanguageId {
        // The `.vue` SFC carrier language. A same-adapter non-carrier row
        // (e.g. an external Vue template) is NOT this language and is not
        // routed through the SFC parse path.
        LanguageId::new("vue")
    }

    fn parse(&self, source: &str, opts: &ParseOptions) -> Arc<FrameworkParseArtifact> {
        let delimiters = opts
            .delimiters
            .as_ref()
            .map(|(open, close)| (open.as_str(), close.as_str()));
        let parsed = Arc::new(parse_sfc(
            source,
            delimiters,
            opts.custom_elements.as_deref(),
        ));
        build_vue_parse_artifact(source, parsed, VUE_CARRIER_PARSER_VERSION)
    }

    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
        // Position-preserving blanking from the artifact's own typed
        // script regions: every byte starts blanked (line terminators
        // preserved so line/column geometry is unchanged), then each
        // script region's RAW bytes are stamped over their carrier-
        // absolute offsets. Output length == input length by
        // construction.
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
        // Every replaced byte is single-byte ASCII and every preserved
        // range is copied wholesale from valid UTF-8, so `out` is valid
        // UTF-8 and exactly `source.len()` bytes long.
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
        let Some(parsed) = self.parsed_sfc(artifact) else {
            return Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            });
        };

        // The Vue IDE pipeline owns its own unified `CodeTransform`
        // internally and produces the token-precise TSX/JSX + source map.
        // The bridge delegates and lifts the result verbatim — no caller
        // CodeTransform, no post-build string munging.
        let core_opts = CodegenOptions {
            filename: opts.filename.clone(),
            target: CompileTarget::IDE,
            skip_source_map: opts.skip_source_map,
            embed_ambient_types: opts.embed_ambient_types,
            ..Default::default()
        };
        let verter_opts = VerterCompileOptions {
            source_map: !opts.skip_source_map,
            ..Default::default()
        };
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_from_parsed(source, parsed, &core_opts, &verter_opts, &alloc);

        match result.tsx {
            Some(tsx) => Ok(IdeOutput {
                code: tsx.code,
                source_map: tsx.source_map,
                is_jsx: tsx.is_jsx,
                duration_ms: tsx.duration_ms,
                destructured_block: tsx.destructured_block,
            }),
            // `CompileTarget::IDE` always sets `TSX`, so a missing `tsx`
            // block means the codegen produced no IDE artifact for this
            // carrier — the typed unsupported answer, never a silent empty.
            None => Err(CompileUnsupported::TargetMissingIde(core_opts.target)),
        }
    }

    fn template_data(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts {
        let Some(parsed) = self.parsed_sfc(artifact) else {
            return TemplateFacts::default();
        };
        let core_opts = CodegenOptions {
            target: CompileTarget::TEMPLATE_DATA,
            skip_source_map: true,
            ..Default::default()
        };
        let verter_opts = VerterCompileOptions {
            extract_template_data: true,
            source_map: false,
            ..Default::default()
        };
        let alloc = oxc_allocator::Allocator::new();
        let result = compile_from_parsed(source, parsed, &core_opts, &verter_opts, &alloc);
        TemplateFacts {
            data: result.template_data.unwrap_or_default(),
        }
    }

    fn compile_bundle(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, CompileUnsupported> {
        let Some(parsed) = self.parsed_sfc(artifact) else {
            return Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            });
        };

        // The Vue runtime target. The host's old `compile_entry` always
        // emitted the bundler blocks (script/template/styles/custom) and
        // additionally requested the IDE TSX + template-data bits when its
        // scope required them; the `want_*` flags drive the SAME target
        // composition so the produced blocks stay byte-identical.
        let mut target = CompileTarget::BUNDLER;
        if opts.want_ide {
            target |= CompileTarget::TSX;
        }
        if opts.want_template_data {
            target |= CompileTarget::TEMPLATE_DATA;
        }

        let core_opts = CodegenOptions {
            filename: opts.filename.clone(),
            is_production: opts.is_production,
            // The host always assembles a standalone `function render()` via
            // its main-module assembly, so inline mode is off (otherwise the
            // template emits bare identifiers missing the `$setup.` prefix).
            inline: Some(false),
            component_id: opts.component_id.clone(),
            delimiters: opts.delimiters.clone(),
            custom_elements: opts.custom_elements.clone(),
            comments: opts.comments,
            runtime_module_name: opts.runtime_module_name.clone(),
            types_module_name: opts.types_module_name.clone(),
            target,
            embed_ambient_types: opts.embed_ambient_types,
            conditional_root_narrowing: opts.conditional_root_narrowing,
            strict_slots: opts.strict_slots,
            ..CodegenOptions::default()
        };
        // The Vue-private resolved inputs ride opaquely on `framework_extras`;
        // downcast them here (a foreign / absent extras yields defaults, so a
        // generic caller that did not supply Vue extras still compiles).
        let extras = opts
            .framework_extras
            .as_ref()
            .and_then(|any| any.downcast_ref::<VueRuntimeCompileExtras>());
        let verter_opts = VerterCompileOptions {
            force_vapor: opts.force_vapor,
            force_js: opts.force_js,
            source_map: opts.source_map,
            ssr: opts.ssr,
            external_types: extras.and_then(|e| e.external_types.clone()),
            extract_template_data: opts.want_template_data,
            prop_constness_overrides: extras.and_then(|e| e.prop_constness_overrides.clone()),
            style_v_bind_vars: extras
                .map(|e| e.style_v_bind_vars.clone())
                .unwrap_or_default(),
        };

        // Vue uses `VerterCompileResult` INTERNALLY here; the returned bundle
        // re-expresses every field neutrally so session assembly never sees
        // the Vue-shaped result.
        let result = compile_from_parsed(source, parsed, &core_opts, &verter_opts, alloc);

        Ok(vue_result_to_runtime_bundle(result))
    }
}

/// Re-express a Vue [`VerterCompileResult`] as the framework-neutral
/// [`RuntimeCompileOutput`]. Vue leaves `main.body_code` `None` — the host
/// assembles the `_sfc_main` module from the neutral block fields (its
/// virtual-file concern: style/custom virtual imports + HMR).
fn vue_result_to_runtime_bundle(
    result: crate::compile::VerterCompileResult,
) -> RuntimeCompileOutput {
    let script = result.script.map(|s| RuntimeScriptBlock {
        code: s.code,
        source_map: s.source_map,
        setup: s.setup,
    });
    let template = result.template.map(|t| RuntimeTemplateBlock {
        code: t.code,
        source_map: t.source_map,
        imports: t.imports.iter().map(|s| (*s).to_string()).collect(),
        ssr_imports: t.ssr_imports.iter().map(|s| (*s).to_string()).collect(),
    });
    let styles = result
        .styles
        .into_iter()
        .map(|s| RuntimeStyleBlock {
            code: s.code,
            lang: s.lang,
        })
        .collect();
    let custom_blocks = result
        .custom_blocks
        .into_iter()
        .map(|b| RuntimeCustomBlock {
            block_type: b.block_type,
            content: b.content,
        })
        .collect();
    let tsx = result.tsx.map(|tsx| IdeOutput {
        code: tsx.code,
        source_map: tsx.source_map,
        is_jsx: tsx.is_jsx,
        duration_ms: tsx.duration_ms,
        destructured_block: tsx.destructured_block,
    });
    let diagnostics = result
        .errors
        .into_iter()
        .map(|d| RuntimeDiagnostic {
            severity: d.severity.into(),
            code: d.code,
            message: d.message,
            span: d.span,
        })
        .collect();

    RuntimeCompileOutput {
        // Vue: host-assembled main module — no directly-emitted body.
        main: RuntimeMainModule::default(),
        script,
        template,
        styles,
        custom_blocks,
        scope_id: result.scope_id,
        tsx,
        template_data: result.template_data,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        let parsed = Arc::new(parse_sfc(source, None, None));
        build_vue_parse_artifact(source, parsed, 1)
    }

    #[test]
    fn script_regions_carry_kind_and_resolved_source_type() {
        let source =
            "<script>export default {}</script>\n<script setup lang=\"tsx\">const a = 1</script>";
        let artifact = artifact_for(source);
        let regions = &artifact.common.script_regions;
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].kind, ScriptRegionKind::Module);
        assert_eq!(regions[1].kind, ScriptRegionKind::Instance);
        // The first block WITH a lang attribute decides for the SFC —
        // both regions are stamped with the resolved dialect.
        assert_eq!(regions[0].source_type, ScriptSourceType::Tsx);
        assert_eq!(regions[1].source_type, ScriptSourceType::Tsx);
        // Content spans slice to the script text.
        assert_eq!(regions[0].span.slice(source).trim(), "export default {}");
        assert_eq!(regions[1].span.slice(source).trim(), "const a = 1");
    }

    #[test]
    fn lang_resolution_matches_the_historical_attr_scan() {
        // (source, expected) — the historical `sfc_script_source_type`
        // semantics: lowercase compare, unknown/absent → Ts.
        let cases = [
            ("<script lang=\"ts\">a</script>", ScriptSourceType::Ts),
            ("<script lang=\"tsx\">a</script>", ScriptSourceType::Tsx),
            ("<script lang=\"TSX\">a</script>", ScriptSourceType::Tsx),
            (
                "<script lang=\"jsx\">a</script>",
                ScriptSourceType::Jsx(JsModuleKind::Module),
            ),
            (
                "<script lang=\"js\">a</script>",
                ScriptSourceType::Js(JsModuleKind::Script),
            ),
            ("<script lang=\"coffee\">a</script>", ScriptSourceType::Ts),
            ("<script lang>a</script>", ScriptSourceType::Ts),
            ("<script>a</script>", ScriptSourceType::Ts),
            ("<template><div /></template>", ScriptSourceType::Ts),
        ];
        for (source, expected) in cases {
            let parsed = Arc::new(parse_sfc(source, None, None));
            assert_eq!(
                vue_script_source_type(&parsed, source),
                expected,
                "lang resolution drifted for {source:?}"
            );
        }
    }

    #[test]
    fn template_style_regions_and_external_links_are_populated() {
        let source = "<template src=\"./tpl.html\"></template>\n<style src=\"./a.css\"></style>\n<style>.x{}</style>\n<script src=\"./impl.ts\"></script>";
        let artifact = artifact_for(source);
        assert_eq!(artifact.common.template_regions.len(), 1);
        assert_eq!(artifact.common.style_regions.len(), 2);
        let links: Vec<(&ExternalLinkKind, &str)> = artifact
            .common
            .external_links
            .iter()
            .map(|l| (&l.kind, l.specifier.as_str()))
            .collect();
        assert!(links.contains(&(&ExternalLinkKind::Script, "./impl.ts")));
        assert!(links.contains(&(&ExternalLinkKind::Template, "./tpl.html")));
        assert!(links.contains(&(&ExternalLinkKind::Style, "./a.css")));
        // The second style block has no src.
        assert_eq!(
            links
                .iter()
                .filter(|(k, _)| matches!(k, ExternalLinkKind::Style))
                .count(),
            1
        );
    }

    #[test]
    fn script_regions_are_source_ordered_when_setup_precedes_plain_script() {
        // The parser exposes plain-script-then-setup accessor order; the
        // artifact must re-order by source position.
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<script lang=\"ts\">export default {}</script>";
        let artifact = artifact_for(source);
        let regions = &artifact.common.script_regions;
        assert_eq!(regions.len(), 2);
        assert_eq!(
            regions[0].kind,
            ScriptRegionKind::Instance,
            "the <script setup> block appears first in source"
        );
        assert_eq!(regions[1].kind, ScriptRegionKind::Module);
        assert!(
            regions[0].span.start < regions[1].span.start,
            "regions must be ordered by source position"
        );
    }

    #[test]
    fn artifact_identity_names_the_vue_adapter() {
        let artifact = artifact_for("<script>a</script>");
        assert!(artifact.adapter_id.is_vue());
        assert_eq!(artifact.language_id.as_str(), "vue");
        assert_eq!(artifact.parser_version, 1);
        assert!(artifact.common.diagnostics.is_empty());
    }

    // ── Vue CarrierCompiler impl ───────────────────────────────────

    #[test]
    fn vue_compiler_parse_stamps_the_carrier_parser_version_and_vue_identity() {
        let compiler = VueCarrierCompiler::default();
        assert!(compiler.adapter_id().is_vue());
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<template><div /></template>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        assert!(artifact.adapter_id.is_vue());
        assert_eq!(artifact.parser_version, VUE_CARRIER_PARSER_VERSION);
        assert_eq!(artifact.common.script_regions.len(), 1);
        assert_eq!(artifact.common.template_regions.len(), 1);
    }

    #[test]
    fn vue_compiler_eval_source_is_position_preserving_with_script_at_raw_offsets() {
        let compiler = VueCarrierCompiler::default();
        let source = "<template><div/></template>\n<script setup lang=\"ts\">const a = 1</script>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let eval = compiler.eval_source(source, &artifact);
        // Length invariant.
        assert_eq!(
            eval.len(),
            source.len(),
            "eval source must equal SFC length"
        );
        // The script region's bytes sit at their raw offsets.
        let region = artifact.common.script_regions[0].span;
        let (s, e) = (region.start as usize, region.end as usize);
        assert_eq!(&eval[s..e], "const a = 1");
        // The `<template>` markup is blanked (no `<` survives outside script).
        assert!(
            !eval[..s].contains('<'),
            "markup before the script must be blanked"
        );
    }

    #[test]
    fn vue_compiler_compile_ide_produces_tsx_for_a_typescript_sfc() {
        let compiler = VueCarrierCompiler::default();
        let source = "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let opts = IdeCompileOptions {
            filename: Some("App.vue".to_string()),
            ..Default::default()
        };
        let out = compiler
            .compile_ide(source, &artifact, &opts)
            .expect("a TS SFC compiles to a TSX IDE artifact");
        assert!(!out.is_jsx, "a lang=ts SFC yields TSX, not JSX");
        assert!(!out.code.is_empty(), "IDE code must be produced");
    }

    #[test]
    fn vue_compiler_compile_ide_rejects_a_foreign_artifact_with_typed_unsupported() {
        let compiler = VueCarrierCompiler::default();
        // An artifact stamped for another adapter cannot be opened by the
        // Vue ctx — the bridge returns the typed unsupported answer.
        let foreign = Arc::new(FrameworkParseArtifact::new(
            FrameworkAdapterId::new("svelte"),
            LanguageId::new("svelte"),
            1,
            FrameworkParseCommon::default(),
            Arc::new(VueParseCarrier::new(Arc::new(parse_sfc(
                "<script>a</script>",
                None,
                None,
            )))),
        ));
        let err = compiler
            .compile_ide(
                "<script>a</script>",
                &foreign,
                &IdeCompileOptions::default(),
            )
            .expect_err("a foreign artifact has no Vue carrier to open");
        assert!(matches!(err, CompileUnsupported::NoIdeProjection { .. }));
    }

    #[test]
    fn vue_compiler_template_data_extracts_component_usages() {
        let compiler = VueCarrierCompiler::default();
        let source = "<script setup lang=\"ts\">import Child from './Child.vue'</script>\n<template><Child :foo=\"1\" /></template>";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let facts = compiler.template_data(source, &artifact);
        assert!(
            facts.data.components.iter().any(|c| c.tag_name == "Child"),
            "template_data must surface the <Child> component usage"
        );
    }
}
