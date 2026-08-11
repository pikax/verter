//! The Vue carrier bridge.
//!
//! Owns [`VueParseCarrier`] — the concrete [`CarrierParse`] payload
//! wrapping a parsed Vue SFC — and [`build_vue_parse_artifact`], the
//! Vue producer that wraps a [`ParsedSfc`] for the registered projector. The
//! projector is the sole owner of the neutral inventory geometry.
//!
//! Consumers outside the Vue adapter read the artifact's typed
//! [`FrameworkParseCommon`] surface; the typed carrier is reachable
//! ONLY through the blessed token-gated `carrier_for::<VueParseCarrier>`
//! wrappers (see the `carrier_downcast_confined_to_owning_adapter`
//! architecture guard).

use std::any::Any;
use std::sync::Arc;

use verter_css_syntax::CssDialect;
use verter_language::{
    CarrierParse, ExternalLinkKind, FrameworkAdapterId, FrameworkParseArtifact,
    FrameworkParseCommon, JsModuleKind, LanguageId, ScriptSourceType,
};
use verter_span::Span;

use verter_parser::parser::types::ParsedSfc;
use verter_parser::types::NodeProp;

use crate::compile::types::{
    CodegenOptions, CompileTarget, VerterCompileOptions, VueMacroSemanticInput,
};
use crate::compile::{
    compile_from_parsed, parse_script_block, parse_sfc, parse_template_block,
    template_unit_used_vars,
};
use crate::framework_common::carrier_compiler::{
    CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput, ParseOptions,
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeCustomBlock, RuntimeDiagnostic,
    RuntimeMainModule, RuntimeOutputDescriptor, RuntimeScriptBlock, RuntimeStyleBlock,
    RuntimeTemplateBlock, SourceMapFidelity, TemplateFacts,
};
use crate::framework_common::ctx::{receive_vue_carrier_token, CarrierCompilerCtx};
use crate::framework_common::generated_chunk::{
    compose_generated_chunk, GeneratedFragment, GeneratedUnit,
};
use crate::style_planner::{
    transform_vue_css_modules, transform_vue_scoped_css, transform_vue_v_bind, AuthoredStyleInput,
    PlainCssInput, StyleRewriteOutcome,
};

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

/// Wrap a parsed Vue SFC for the registered projector.
pub fn build_vue_parse_artifact(
    _source: &str,
    parsed: Arc<ParsedSfc>,
    parser_version: u32,
) -> Arc<FrameworkParseArtifact> {
    Arc::new(FrameworkParseArtifact::new(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        parser_version,
        FrameworkParseCommon {
            inventory: Arc::default(),
            diagnostics: Vec::new(),
        },
        Arc::new(VueParseCarrier::new(parsed)),
    ))
}

/// Vue-PRIVATE resolved runtime-compile inputs, carried opaquely through
/// [`RuntimeCompileOptions::framework_extras`](crate::framework_common::RuntimeCompileOptions::framework_extras)
/// and downcast here.
///
/// These are the host-resolved inputs the Vue runtime compile consumes:
/// authoritative macro runtime semantics, `prop_constness_overrides`, and
/// `style_v_bind_vars`. They live HERE (the Vue module) rather than on the
/// neutral [`RuntimeCompileOptions`] so Vue's typed macro DTO never enters the
/// cross-framework carrier contract — a non-Vue carrier never names or sees it.
#[derive(Debug, Default)]
pub struct VueRuntimeCompileExtras {
    /// Authoritative runtime projection produced once by the session.
    pub macro_runtime: Option<std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>>,
    /// Props known const across all call sites (cross-file analysis).
    pub prop_constness_overrides: Option<rustc_hash::FxHashSet<String>>,
    /// Binding names referenced in style `v-bind()` expressions.
    pub style_v_bind_vars: Vec<String>,
    /// Whether every projected-style `v-bind()` expression was parsed.
    pub style_v_bind_usage_complete: bool,
}

/// The Vue carrier parser version stamped on the produced artifact's
/// `parser_version` field. Bumps invalidate every Vue artifact whose
/// post-parse shape this producer owns.
///
/// The session's `LEGACY_PARSER_VERSION` (the `FileArtifactStore` legacy
/// key dimension) and this constant are conceptually distinct — one keys
/// the store, the other stamps the artifact — and currently agree by
/// value, so the rehoused dispatch produces byte-identical artifacts.
///
/// Bumped 3 → 4: carrier script facts and declaration inventories now
/// retain exact top-level lexical owners, so the carrier artifact's
/// post-parse shape changed. This mirrors the session's
/// `LEGACY_PARSER_VERSION` 3 → 4 bump; the two must agree by value so the
/// rehoused dispatch neither serves nor is spuriously evicted against the
/// legacy key dimension.
///
/// Bumped 4 to 5: analyzed call-signature emit fields retain their declaration
/// span for exact occurrence joins. This mirrors the session's
/// `LEGACY_PARSER_VERSION` 4 to 5 bump.
///
/// Bumped 5 to 6: shallow import targets are authored-only and no longer retain
/// a resolved canonical. This mirrors the session's `LEGACY_PARSER_VERSION`
/// 5 to 6 bump.
pub const VUE_CARRIER_PARSER_VERSION: u32 = 6;
pub const VUE_CARRIER_ARTIFACT_VERSION: verter_language::carrier_versions::CarrierParserVersion =
    match verter_language::carrier_versions::CarrierParserVersion::new(VUE_CARRIER_PARSER_VERSION) {
        Some(version) => version,
        None => panic!("Vue carrier parser version must be nonzero"),
    };

/// The Vue carrier compiler — the reference [`CarrierCompiler`].
///
/// Delegates call-for-call to the existing Vue pipeline (`parse_sfc` +
/// `compile_from_parsed`): it edits NO Vue parser or codegen module and
/// reaches the parsed SFC back out of the type-erased artifact through
/// the compiler-side blessed [`CarrierCompilerCtx`] downcast.
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
    pub(super) fn carrier_arc(
        &self,
        artifact: &FrameworkParseArtifact,
    ) -> Option<Arc<VueParseCarrier>> {
        self.ctx.carrier_for_arc::<VueParseCarrier>(artifact)
    }

    /// Reach the parsed SFC back out of a Vue artifact, or `None` when the
    /// artifact is not a Vue carrier (foreign adapter id or non-Vue
    /// erased payload). The blessed downcast home for the Vue bridge.
    pub(super) fn parsed_sfc<'a>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a ParsedSfc> {
        self.ctx
            .carrier_for::<VueParseCarrier>(artifact)
            .map(|carrier| carrier.parsed())
    }

    fn compile_projected_script_bundle(
        &self,
        source: &str,
        parsed: &ParsedSfc,
        input: &crate::framework_common::RuntimeBlockContentInput,
        is_setup: bool,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, CompileUnsupported> {
        if (is_setup && parsed.script().is_some()) || (!is_setup && parsed.script_setup().is_some())
        {
            return Err(CompileUnsupported::BlockContentRuntimeUnavailable {
                adapter_id: self.adapter_id(),
            });
        }
        let extras = opts
            .framework_extras
            .as_ref()
            .and_then(|any| any.downcast_ref::<VueRuntimeCompileExtras>());
        let macro_semantics = extras
            .and_then(|extras| extras.macro_runtime.clone())
            .map(VueMacroSemanticInput::Runtime)
            .unwrap_or_default();
        let script_parsed = parse_script_block(&input.code, &input.lang, is_setup);
        let template_used_vars = template_unit_used_vars(
            source,
            parsed,
            opts.delimiters.clone(),
            opts.custom_elements.clone(),
            alloc,
        );
        let mut script_target = CompileTarget::SCRIPT;
        if opts.want_ide {
            script_target |= CompileTarget::TSX;
        }
        let script_result = compile_from_parsed(
            &input.code,
            &script_parsed,
            &CodegenOptions {
                filename: opts.filename.clone(),
                is_production: opts.is_production,
                custom_element: opts.custom_element,
                component_id: opts.component_id.clone(),
                inline: Some(false),
                runtime_module_name: opts.runtime_module_name.clone(),
                types_module_name: opts.types_module_name.clone(),
                target: script_target,
                embed_ambient_types: opts.embed_ambient_types,
                conditional_root_narrowing: opts.conditional_root_narrowing,
                strict_slots: opts.strict_slots,
                ide_chunk_boundaries: opts.want_ide,
                ..Default::default()
            },
            &VerterCompileOptions {
                force_js: opts.force_js,
                source_map: opts.source_map || opts.want_ide,
                template_used_vars: Some(template_used_vars),
                ..Default::default()
            },
            &macro_semantics,
            alloc,
        );
        let binding_metadata = script_result.template_binding_metadata.clone();
        let mut script_bundle =
            vue_result_to_runtime_bundle(&input.code, &script_parsed, script_result);
        let mut carrier_view = parsed.clone();
        carrier_view.script_node = None;
        carrier_view.script_setup_node = None;
        let mut carrier_target = CompileTarget::BUNDLER;
        if opts.want_ide {
            carrier_target |= CompileTarget::TSX;
        }
        if opts.want_template_data {
            carrier_target |= CompileTarget::TEMPLATE_DATA;
        }
        let carrier_result = compile_from_parsed(
            source,
            &carrier_view,
            &CodegenOptions {
                filename: opts.filename.clone(),
                is_production: opts.is_production,
                component_id: opts.component_id.clone(),
                inline: Some(false),
                delimiters: opts.delimiters.clone(),
                custom_elements: opts.custom_elements.clone(),
                comments: opts.comments,
                runtime_module_name: opts.runtime_module_name.clone(),
                types_module_name: opts.types_module_name.clone(),
                target: carrier_target,
                embed_ambient_types: opts.embed_ambient_types,
                conditional_root_narrowing: opts.conditional_root_narrowing,
                strict_slots: opts.strict_slots,
                ide_chunk_boundaries: opts.want_ide,
                ..Default::default()
            },
            &VerterCompileOptions {
                force_vapor: opts.force_vapor,
                force_js: opts.force_js,
                source_map: opts.source_map || opts.want_ide,
                ssr: opts.ssr,
                extract_template_data: opts.want_template_data,
                prop_constness_overrides: binding_metadata.const_props.clone(),
                template_binding_metadata: Some(binding_metadata),
                ..Default::default()
            },
            &VueMacroSemanticInput::default(),
            alloc,
        );
        let mut bundle = vue_result_to_runtime_bundle(source, &carrier_view, carrier_result);
        let mut script = script_bundle.script.take().ok_or(
            CompileUnsupported::BlockContentRuntimeUnavailable {
                adapter_id: self.adapter_id(),
            },
        )?;
        script.output_descriptor = RuntimeOutputDescriptor::generated(
            &script.code,
            (!script.source_map.is_empty()).then_some(script.source_map.as_str()),
            &[(
                input.source_space_token.as_str(),
                input.content_artifact_token.as_str(),
            )],
            SourceMapFidelity::Approximate,
        );
        bundle.script = Some(script);
        bundle.diagnostics.extend(script_bundle.diagnostics);

        if opts.want_ide {
            let mut shell =
                script_bundle
                    .tsx
                    .take()
                    .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                        adapter_id: self.adapter_id(),
                    })?;
            let fragment =
                bundle
                    .tsx
                    .take()
                    .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                        adapter_id: self.adapter_id(),
                    })?;
            let hole = shell.generated_template_hole.clone().ok_or(
                CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                },
            )?;
            let chunk = fragment.generated_template_chunk.as_ref().ok_or(
                CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                },
            )?;
            let (carrier_space, carrier_artifact) = RuntimeOutputDescriptor::carrier_source(source);
            let composed = compose_generated_chunk(
                "",
                GeneratedUnit {
                    code: &shell.code,
                    source_map: &shell.source_map,
                    source_space: &input.source_space_token,
                    source: &input.code,
                },
                hole,
                GeneratedFragment {
                    unit: GeneratedUnit {
                        code: &chunk.code,
                        source_map: &chunk.source_map,
                        source_space: &carrier_space,
                        source,
                    },
                    range: 0..chunk.code.len() as u32,
                },
            )
            .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                adapter_id: self.adapter_id(),
            })?;
            shell.code = composed.code;
            shell.source_map = composed.source_map;
            shell.generated_template_hole = None;
            shell.generated_template_chunk = None;
            shell.output_descriptor = RuntimeOutputDescriptor::generated(
                &shell.code,
                Some(&shell.source_map),
                &[
                    (
                        input.source_space_token.as_str(),
                        input.content_artifact_token.as_str(),
                    ),
                    (carrier_space.as_str(), carrier_artifact.as_str()),
                ],
                SourceMapFidelity::Approximate,
            );
            bundle.tsx = Some(shell);
        }
        Ok(bundle)
    }
}

/// Compile a Vue artifact already elected by a registered publication store.
/// No raw-source parsing is reachable through this boundary.
#[doc(hidden)]
pub fn compile_registered_vue_artifact(
    source: &str,
    artifact: &FrameworkParseArtifact,
    options: &CodegenOptions,
    verter_options: &VerterCompileOptions,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &oxc_allocator::Allocator,
) -> Result<crate::compile::VerterCompileResult, CompileUnsupported> {
    let compiler = VueCarrierCompiler::default();
    let Some(parsed) = compiler.parsed_sfc(artifact) else {
        return Err(CompileUnsupported::NoIdeProjection {
            adapter_id: artifact.adapter_id.clone(),
        });
    };
    let exact_source = artifact
        .common
        .inventory
        .source_spaces()
        .first()
        .is_some_and(|space| space.bytes().as_ref() == source);
    if !exact_source {
        return Err(CompileUnsupported::NoIdeProjection {
            adapter_id: artifact.adapter_id.clone(),
        });
    }
    let result = compile_from_parsed(
        source,
        parsed,
        options,
        verter_options,
        macro_semantics,
        allocator,
    );
    // Determinism digest over the emitted block LENGTHS: two runs over the
    // same source must emit the same sizes. A cheap tripwire for
    // nondeterministic codegen — it does not hash the bytes.
    verter_audit::attribute_digest!(
        CompiledOutputDigest,
        (result.script.as_ref().map_or(0usize, |b| b.code.len()) as u64).wrapping_mul(0x1000_0001)
            ^ (result.template.as_ref().map_or(0usize, |b| b.code.len()) as u64)
                .wrapping_mul(0x1000_0003)
            ^ (result.tsx.as_ref().map_or(0usize, |b| b.code.len()) as u64)
                .wrapping_mul(0x1000_0007)
            ^ (result.styles.len() as u64).wrapping_mul(0x1000_000d)
    );
    Ok(result)
}

impl CarrierCompiler for VueCarrierCompiler {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }

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
        for region in artifact.script_regions() {
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
        if !opts.block_content.is_empty() {
            let alloc = oxc_allocator::Allocator::new();
            return self
                .compile_bundle(
                    source,
                    artifact,
                    &RuntimeCompileOptions {
                        filename: opts.filename.clone(),
                        source_map: !opts.skip_source_map,
                        want_ide: true,
                        embed_ambient_types: opts.embed_ambient_types,
                        block_content: opts.block_content.clone(),
                        ..Default::default()
                    },
                    &alloc,
                )?
                .tsx
                .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                });
        }

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
        let result = compile_from_parsed(
            source,
            parsed,
            &core_opts,
            &verter_opts,
            &VueMacroSemanticInput::Unavailable,
            &alloc,
        );

        match result.tsx {
            Some(tsx) => {
                let (space, artifact) = RuntimeOutputDescriptor::carrier_source(source);
                let output_descriptor = RuntimeOutputDescriptor::generated(
                    &tsx.code,
                    (!tsx.source_map.is_empty()).then_some(tsx.source_map.as_str()),
                    &[(space.as_str(), artifact.as_str())],
                    SourceMapFidelity::Approximate,
                );
                Ok(IdeOutput {
                    code: tsx.code,
                    source_map: tsx.source_map,
                    is_jsx: tsx.is_jsx,
                    duration_ms: tsx.duration_ms,
                    destructured_block: tsx.destructured_block,
                    output_descriptor,
                    generated_template_hole: tsx.generated_template_hole,
                    generated_template_chunk: tsx.generated_template_chunk,
                })
            }
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
        let result = compile_from_parsed(
            source,
            parsed,
            &core_opts,
            &verter_opts,
            &VueMacroSemanticInput::Unavailable,
            &alloc,
        );
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

        // A selected template fragment is inserted at the compiler-registered
        // IDE hole. For a carrier with only a plain script that hole is the
        // script-content boundary, which is mid-module rather than a valid JSX
        // statement position. These are parser-owned carrier facts plus the
        // host-selected block input; do not rediscover the geometry by scanning
        // source text. Keep this exact partially-capable class fail-closed.
        if opts.want_ide
            && opts.block_content.template.is_some()
            && parsed.script().is_some()
            && parsed.script_setup().is_none()
        {
            return Err(CompileUnsupported::BlockContentIdeUnavailable {
                adapter_id: self.adapter_id(),
            });
        }

        match (
            opts.block_content.script.as_ref(),
            opts.block_content.script_setup.as_ref(),
        ) {
            (Some(_), Some(_)) if opts.want_ide => {
                return Err(CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                });
            }
            (Some(_), Some(_)) => {
                return Err(CompileUnsupported::BlockContentRuntimeUnavailable {
                    adapter_id: self.adapter_id(),
                });
            }
            (Some(_), None) if opts.want_ide => {
                return Err(CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                });
            }
            (Some(_), None) => {
                return Err(CompileUnsupported::BlockContentRuntimeUnavailable {
                    adapter_id: self.adapter_id(),
                });
            }
            (None, Some(_)) if opts.want_ide => {
                return Err(CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                });
            }
            (None, Some(input)) => {
                return self
                    .compile_projected_script_bundle(source, parsed, input, true, opts, alloc);
            }
            (None, None) => {}
        }

        let supplied_inline_template = opts.block_content.template.is_some()
            && opts.inline.unwrap_or(opts.is_production)
            && parsed.script_setup().is_some()
            && !artifact
                .common
                .external_links()
                .iter()
                .any(|link| link.kind == ExternalLinkKind::Template)
            && !opts.force_vapor
            && !opts.ssr;

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
            custom_element: opts.custom_element,
            // Inline-template topology flows from the runtime options (`None`
            // resolves to `is_production`, matching the official default:
            // inline in prod builds). The compiler falls back to non-inline
            // for Vapor / SSR / template-only / script-only SFCs; the bundle
            // carries the RESOLVED topology (`inline`) so the host assembly
            // never sniffs source text.
            inline: opts.inline,
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
            ide_chunk_boundaries: opts.want_ide && opts.block_content.template.is_some(),
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
            source_map: opts.source_map || (opts.want_ide && opts.block_content.template.is_some()),
            ssr: opts.ssr,
            extract_template_data: opts.want_template_data,
            prop_constness_overrides: extras.and_then(|e| e.prop_constness_overrides.clone()),
            style_v_bind_vars: extras
                .map(|e| e.style_v_bind_vars.clone())
                .unwrap_or_default(),
            style_v_bind_usage_complete: opts
                .block_content
                .styles
                .iter()
                .any(Option::is_some)
                .then(|| extras.is_some_and(|e| e.style_v_bind_usage_complete)),
            template_binding_metadata: None,
            template_used_vars: None,
            runtime_template_hole: supplied_inline_template,
            runtime_inline_template_chunk: false,
        };

        // Vue uses `VerterCompileResult` INTERNALLY here; the returned bundle
        // re-expresses every field neutrally so session assembly never sees
        // the Vue-shaped result.
        let macro_semantics = extras
            .and_then(|extras| extras.macro_runtime.clone())
            .map(VueMacroSemanticInput::Runtime)
            .unwrap_or_default();
        let mut carrier_view = parsed.clone();
        if opts.block_content.template.is_some() {
            carrier_view.template_ast = None;
        }
        for (style, selected) in carrier_view
            .style_nodes
            .iter_mut()
            .zip(opts.block_content.styles.iter())
        {
            if selected.is_some() {
                // The registered block-content projection owns these bytes.
                // Preserve the parser-owned slot and attributes, but do not
                // let the carrier-authored dialect enter either rewrite stage.
                style.content = None;
            }
        }
        let selected_template = opts.block_content.template.as_ref().map(|input| {
            let parsed_template = parse_template_block(
                &input.code,
                opts.delimiters
                    .as_ref()
                    .map(|(open, close)| (open.as_str(), close.as_str())),
                opts.custom_elements.as_deref(),
            );
            let used_vars = template_unit_used_vars(
                &input.code,
                &parsed_template,
                opts.delimiters.clone(),
                opts.custom_elements.clone(),
                alloc,
            );
            (input, parsed_template, used_vars)
        });
        let mut verter_opts = verter_opts;
        verter_opts.template_used_vars = selected_template
            .as_ref()
            .map(|(_, _, used_vars)| used_vars.clone());
        let result = compile_from_parsed(
            source,
            &carrier_view,
            &core_opts,
            &verter_opts,
            &macro_semantics,
            alloc,
        );
        let binding_metadata = result.template_binding_metadata.clone();
        let mut bundle = vue_result_to_runtime_bundle(source, &carrier_view, result);

        if let Some((input, parsed_template, _)) = selected_template {
            let template_core_opts = CodegenOptions {
                filename: opts.filename.clone(),
                is_production: opts.is_production,
                component_id: opts.component_id.clone(),
                delimiters: opts.delimiters.clone(),
                custom_elements: opts.custom_elements.clone(),
                comments: opts.comments,
                runtime_module_name: opts.runtime_module_name.clone(),
                target: CompileTarget::TEMPLATE
                    | if opts.want_ide {
                        CompileTarget::TSX
                    } else {
                        CompileTarget::empty()
                    }
                    | if opts.want_template_data {
                        CompileTarget::TEMPLATE_DATA
                    } else {
                        CompileTarget::empty()
                    },
                ide_chunk_boundaries: opts.want_ide,
                ..CodegenOptions::default()
            };
            let template_verter_opts = VerterCompileOptions {
                force_vapor: opts.force_vapor,
                force_js: opts.force_js,
                source_map: opts.source_map || opts.want_ide,
                ssr: opts.ssr,
                extract_template_data: opts.want_template_data,
                prop_constness_overrides: binding_metadata.const_props.clone(),
                template_binding_metadata: Some(binding_metadata),
                runtime_inline_template_chunk: supplied_inline_template,
                ..Default::default()
            };
            let compiled = compile_from_parsed(
                &input.code,
                &parsed_template,
                &template_core_opts,
                &template_verter_opts,
                &VueMacroSemanticInput::default(),
                alloc,
            );
            let mut selected =
                vue_result_to_runtime_bundle(&input.code, &parsed_template, compiled);
            if let Some(template) = selected.template.as_mut() {
                template.output_descriptor = RuntimeOutputDescriptor::generated(
                    &template.code,
                    (!template.source_map.is_empty()).then_some(template.source_map.as_str()),
                    &[(
                        input.source_space_token.as_str(),
                        input.content_artifact_token.as_str(),
                    )],
                    SourceMapFidelity::Approximate,
                );
            }
            if supplied_inline_template {
                let mut shell = bundle.script.take().ok_or(
                    CompileUnsupported::BlockContentRuntimeUnavailable {
                        adapter_id: self.adapter_id(),
                    },
                )?;
                let template = selected.template.as_ref().ok_or(
                    CompileUnsupported::BlockContentRuntimeUnavailable {
                        adapter_id: self.adapter_id(),
                    },
                )?;
                let hole = shell.generated_template_hole.clone().ok_or(
                    CompileUnsupported::BlockContentRuntimeUnavailable {
                        adapter_id: self.adapter_id(),
                    },
                )?;
                let mut imports = template
                    .imports
                    .iter()
                    .filter(|name| !shell.runtime_imports.contains(name))
                    .cloned()
                    .collect::<Vec<_>>();
                imports.sort_unstable();
                imports.dedup();
                let preamble = if imports.is_empty() {
                    String::new()
                } else {
                    let specifiers = imports
                        .iter()
                        .map(|name| crate::compile::format_import_specifier(name))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "import {{ {specifiers} }} from \"{}\"\n",
                        opts.runtime_module_name.as_deref().unwrap_or("vue")
                    )
                };
                let (carrier_space, carrier_artifact) =
                    RuntimeOutputDescriptor::carrier_source(source);
                let composed = compose_generated_chunk(
                    &preamble,
                    GeneratedUnit {
                        code: &shell.code,
                        source_map: &shell.source_map,
                        source_space: &carrier_space,
                        source,
                    },
                    hole,
                    GeneratedFragment {
                        unit: GeneratedUnit {
                            code: &template.code,
                            source_map: &template.source_map,
                            source_space: &input.source_space_token,
                            source: &input.code,
                        },
                        range: 0..template.code.len() as u32,
                    },
                )
                .ok_or(CompileUnsupported::BlockContentRuntimeUnavailable {
                    adapter_id: self.adapter_id(),
                })?;
                shell.code = composed.code;
                shell.source_map = composed.source_map;
                shell.generated_template_hole = None;
                shell.output_descriptor = RuntimeOutputDescriptor::generated(
                    &shell.code,
                    Some(&shell.source_map),
                    &[
                        (carrier_space.as_str(), carrier_artifact.as_str()),
                        (
                            input.source_space_token.as_str(),
                            input.content_artifact_token.as_str(),
                        ),
                    ],
                    SourceMapFidelity::Approximate,
                );
                bundle.script = Some(shell);
                bundle.template = None;
                bundle.inline = true;
            } else {
                bundle.template = selected.template;
            }
            bundle.template_data = selected.template_data;
            bundle.diagnostics.extend(selected.diagnostics);
            if opts.want_ide {
                let mut shell =
                    bundle
                        .tsx
                        .take()
                        .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                            adapter_id: self.adapter_id(),
                        })?;
                let fragment =
                    selected
                        .tsx
                        .take()
                        .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                            adapter_id: self.adapter_id(),
                        })?;
                let hole = shell.generated_template_hole.clone().ok_or(
                    CompileUnsupported::BlockContentIdeUnavailable {
                        adapter_id: self.adapter_id(),
                    },
                )?;
                let template_chunk = fragment.generated_template_chunk.as_ref().ok_or(
                    CompileUnsupported::BlockContentIdeUnavailable {
                        adapter_id: self.adapter_id(),
                    },
                )?;
                let (carrier_space, carrier_artifact) =
                    RuntimeOutputDescriptor::carrier_source(source);
                let composed = compose_generated_chunk(
                    "",
                    GeneratedUnit {
                        code: &shell.code,
                        source_map: &shell.source_map,
                        source_space: &carrier_space,
                        source,
                    },
                    hole,
                    GeneratedFragment {
                        unit: GeneratedUnit {
                            code: &template_chunk.code,
                            source_map: &template_chunk.source_map,
                            source_space: &input.source_space_token,
                            source: &input.code,
                        },
                        range: 0..template_chunk.code.len() as u32,
                    },
                )
                .ok_or(CompileUnsupported::BlockContentIdeUnavailable {
                    adapter_id: self.adapter_id(),
                })?;
                shell.code = composed.code;
                shell.source_map = composed.source_map;
                shell.generated_template_hole = None;
                shell.generated_template_chunk = None;
                shell.output_descriptor = RuntimeOutputDescriptor::generated(
                    &shell.code,
                    Some(&shell.source_map),
                    &[
                        (carrier_space.as_str(), carrier_artifact.as_str()),
                        (
                            input.source_space_token.as_str(),
                            input.content_artifact_token.as_str(),
                        ),
                    ],
                    SourceMapFidelity::Approximate,
                );
                bundle.tsx = Some(shell);
            }
        }

        let style_scope = bundle
            .scope_id
            .strip_prefix("data-v-")
            .unwrap_or(&bundle.scope_id)
            .to_string();
        for ((slot, selected), node) in opts
            .block_content
            .styles
            .iter()
            .zip(bundle.styles.iter_mut())
            .zip(parsed.style_nodes())
        {
            if let Some(input) = slot {
                let authored_dialect = match node.lang {
                    None | Some(crate::parser::types::StyleLang::Css) => Some(CssDialect::Css),
                    Some(crate::parser::types::StyleLang::Scss) => Some(CssDialect::Scss),
                    Some(crate::parser::types::StyleLang::Sass) => Some(CssDialect::Sass),
                    Some(crate::parser::types::StyleLang::Less) => Some(CssDialect::Less),
                    Some(crate::parser::types::StyleLang::Stylus) => Some(CssDialect::Stylus),
                    Some(crate::parser::types::StyleLang::Unknown) => None,
                };
                let selected_dialect = match input.lang.as_str() {
                    "css" => CssDialect::Css,
                    "scss" => CssDialect::Scss,
                    "sass" => CssDialect::Sass,
                    "less" => CssDialect::Less,
                    "stylus" | "styl" => CssDialect::Stylus,
                    _ if authored_dialect.is_none() => CssDialect::Css,
                    _ => {
                        return Err(CompileUnsupported::BlockContentRuntimeUnavailable {
                            adapter_id: self.adapter_id(),
                        });
                    }
                };
                let supplied_preprocessor_output = selected_dialect == CssDialect::Css
                    && authored_dialect != Some(CssDialect::Css);
                if authored_dialect.is_none() && !supplied_preprocessor_output {
                    return Err(CompileUnsupported::BlockContentRuntimeUnavailable {
                        adapter_id: self.adapter_id(),
                    });
                }
                let mut current = input.code.to_string();
                let mut current_map = if opts.source_map {
                    input.source_map.as_deref().map(str::to_string)
                } else {
                    None
                };
                let mut descriptor = RuntimeOutputDescriptor::identity(
                    &current,
                    &input.source_space_token,
                    &input.content_artifact_token,
                );
                let mut applied_rewrite_stages = 0u8;

                if !supplied_preprocessor_output {
                    let authored = AuthoredStyleInput::new(
                        &current,
                        selected_dialect,
                        &input.source_space_token,
                        &input.source_space_token,
                        &input.content_artifact_token,
                    );
                    match transform_vue_v_bind(authored, &style_scope).map_err(|_| {
                        CompileUnsupported::BlockContentRuntimeUnavailable {
                            adapter_id: self.adapter_id(),
                        }
                    })? {
                        StyleRewriteOutcome::Unchanged { .. } => {}
                        StyleRewriteOutcome::Rewritten {
                            code,
                            source_map,
                            output_descriptor,
                            ..
                        } => {
                            current = code;
                            current_map = opts.source_map.then_some(source_map);
                            descriptor = *output_descriptor;
                            applied_rewrite_stages = applied_rewrite_stages.saturating_add(1);
                        }
                    }
                }

                if node.module && selected_dialect == CssDialect::Css {
                    let plain = PlainCssInput::try_new(
                        &current,
                        selected_dialect,
                        &input.source_space_token,
                        &input.source_space_token,
                        &input.content_artifact_token,
                    )
                    .map_err(|_| {
                        CompileUnsupported::BlockContentRuntimeUnavailable {
                            adapter_id: self.adapter_id(),
                        }
                    })?;
                    match transform_vue_css_modules(plain, &style_scope).map_err(|_| {
                        CompileUnsupported::BlockContentRuntimeUnavailable {
                            adapter_id: self.adapter_id(),
                        }
                    })? {
                        StyleRewriteOutcome::Unchanged { .. } => {}
                        StyleRewriteOutcome::Rewritten {
                            code,
                            source_map,
                            output_descriptor,
                            ..
                        } => {
                            current = code;
                            current_map = opts.source_map.then_some(source_map);
                            descriptor = *output_descriptor;
                            applied_rewrite_stages = applied_rewrite_stages.saturating_add(1);
                        }
                    }
                }

                if node.scoped {
                    let plain = PlainCssInput::try_new(
                        &current,
                        selected_dialect,
                        &input.source_space_token,
                        &input.source_space_token,
                        &input.content_artifact_token,
                    )
                    .map_err(|_| {
                        CompileUnsupported::BlockContentRuntimeUnavailable {
                            adapter_id: self.adapter_id(),
                        }
                    })?;
                    match transform_vue_scoped_css(plain, &style_scope).map_err(|_| {
                        CompileUnsupported::BlockContentRuntimeUnavailable {
                            adapter_id: self.adapter_id(),
                        }
                    })? {
                        StyleRewriteOutcome::Unchanged { .. } => {}
                        StyleRewriteOutcome::Rewritten {
                            code,
                            source_map,
                            output_descriptor,
                            ..
                        } => {
                            current = code;
                            current_map = opts.source_map.then_some(source_map);
                            descriptor = *output_descriptor;
                            applied_rewrite_stages = applied_rewrite_stages.saturating_add(1);
                        }
                    }
                }

                if applied_rewrite_stages > 1 {
                    descriptor.source_map.fidelity = SourceMapFidelity::Approximate;
                }

                selected.code = current;
                selected.source_map = current_map;
                selected.lang = Some(input.lang.clone());
                selected.output_descriptor = descriptor;
            }
        }
        for (slot, selected) in opts
            .block_content
            .custom_blocks
            .iter()
            .zip(bundle.custom_blocks.iter_mut())
        {
            if let Some(input) = slot {
                selected.content = input.code.to_string();
            }
        }

        Ok(bundle)
    }
}

/// Re-express a Vue [`VerterCompileResult`] as the framework-neutral
/// [`RuntimeCompileOutput`]. Vue leaves `main.body_code` `None` — the host
/// assembles the `_sfc_main` module from the neutral block fields (its
/// virtual-file concern: style/custom virtual imports + HMR).
///
/// Public so conformance/test harnesses can drive the genuine
/// compile → bundle → assemble pipeline without re-implementing the
/// conversion (the Vue conformance seed in `verter_vue_conformance`).
pub fn vue_result_to_runtime_bundle(
    source: &str,
    parsed: &ParsedSfc,
    result: crate::compile::VerterCompileResult,
) -> RuntimeCompileOutput {
    let (carrier_space, carrier_artifact) = RuntimeOutputDescriptor::carrier_source(source);
    let declared = [(carrier_space.as_str(), carrier_artifact.as_str())];
    let script = result.script.map(|s| {
        let output_descriptor = RuntimeOutputDescriptor::generated(
            &s.code,
            (!s.source_map.is_empty()).then_some(s.source_map.as_str()),
            &declared,
            SourceMapFidelity::Approximate,
        );
        RuntimeScriptBlock {
            code: s.code,
            source_map: s.source_map,
            setup: s.setup,
            output_descriptor,
            generated_template_hole: s.generated_template_hole,
            runtime_imports: s
                .runtime_imports
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        }
    });
    let template = result.template.map(|t| {
        let output_descriptor = RuntimeOutputDescriptor::generated(
            &t.code,
            (!t.source_map.is_empty()).then_some(t.source_map.as_str()),
            &declared,
            SourceMapFidelity::Approximate,
        );
        RuntimeTemplateBlock {
            code: t.code,
            source_map: t.source_map,
            imports: t.imports.iter().map(|s| (*s).to_string()).collect(),
            ssr_imports: t.ssr_imports.iter().map(|s| (*s).to_string()).collect(),
            output_descriptor,
        }
    });
    let styles = result
        .styles
        .into_iter()
        .zip(parsed.style_nodes())
        .map(|(s, node)| {
            let exact_map = node.content.as_ref().and_then(|content| {
                let authored = &source[content.start as usize..content.end as usize];
                (authored == s.code)
                    .then(|| exact_slice_source_map(source, content.start, authored))
            });
            let output_descriptor = RuntimeOutputDescriptor::generated(
                &s.code,
                exact_map.as_deref(),
                &declared,
                if exact_map.is_some() {
                    SourceMapFidelity::Exact
                } else {
                    SourceMapFidelity::Approximate
                },
            );
            RuntimeStyleBlock {
                code: s.code,
                // Vue's style pipeline produces no per-block css map here (style
                // post-processing happens host/JS-side), and carries no
                // `:global` fact.
                source_map: None,
                lang: s.lang,
                // Vue scoping rides the `data-v-…` attribute on `scope_id`, not a
                // per-block class hash.
                scope_hash: None,
                has_global: false,
                output_descriptor,
            }
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
    let tsx = result.tsx.map(|tsx| {
        let output_descriptor = RuntimeOutputDescriptor::generated(
            &tsx.code,
            (!tsx.source_map.is_empty()).then_some(tsx.source_map.as_str()),
            &declared,
            SourceMapFidelity::Approximate,
        );
        IdeOutput {
            code: tsx.code,
            source_map: tsx.source_map,
            is_jsx: tsx.is_jsx,
            duration_ms: tsx.duration_ms,
            destructured_block: tsx.destructured_block,
            output_descriptor,
            generated_template_hole: tsx.generated_template_hole,
            generated_template_chunk: tsx.generated_template_chunk,
        }
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
        // Vue always emits a runtime surface (or a genuine compile error); it never
        // fails closed on an unsupported runtime surface the way Svelte does.
        runtime_surface_refused: false,
        // The RESOLVED inline topology — the compiler already merged the
        // render into `setup()` when true, so host assembly takes the inline
        // branch (no render attach, no setup-return filter).
        inline: result.inline,
    }
}

fn exact_slice_source_map(source: &str, source_start: u32, output: &str) -> String {
    let resolver = crate::cursor::position::PositionResolver::new(source);
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("carrier", source);
    let mut source_offset = source_start as usize;
    for (generated_line, line) in output.split_inclusive('\n').enumerate() {
        let (source_line, source_column) = resolver.offset_to_line_and_col(source_offset);
        builder.add_token(
            generated_line as u32,
            0,
            (source_line - 1) as u32,
            (source_column - 1) as u32,
            Some(source_id),
            None,
        );
        source_offset += line.len();
    }
    builder.into_sourcemap().to_json_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework_common::{
        carrier_compiler::OutputSourceSpaceKind, RuntimeBlockContentInput,
        RuntimeBlockContentInputs,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use verter_language::{ExternalLinkKind, ScriptRegionKind};

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crate is <workspace>/crates/verter_compiler")
            .to_path_buf()
    }

    fn typescript_launcher() -> PathBuf {
        let launcher = workspace_root().join("node_modules/typescript/lib/tsc.js");
        assert!(
            launcher.is_file(),
            "output-validity tests require the pinned TypeScript launcher at {}; run pnpm install",
            launcher.display()
        );
        launcher
    }

    fn typescript_syntax_check(
        node_program: &Path,
        launcher: &Path,
        name: &str,
        code: &str,
        is_jsx: bool,
        extra_args: &[&str],
    ) -> Result<(), String> {
        let project = tempfile::tempdir().expect("create TSX validity directory");
        let extension = if is_jsx { "jsx" } else { "tsx" };
        let path = project.path().join(format!("{name}.{extension}"));
        let environment = project.path().join("jsx-runtime.d.ts");
        fs::write(&path, code).expect("write emitted TSX");
        fs::write(
            &environment,
            concat!(
                "declare module 'vue/jsx-runtime' {\n",
                "  export const Fragment: any;\n",
                "  export function jsx(...args: any[]): any;\n",
                "  export function jsxs(...args: any[]): any;\n",
                "}\n",
            ),
        )
        .expect("write JSX runtime validity stub");
        let output = Command::new(node_program)
            .arg(launcher)
            .args([
                "--noEmit",
                "--skipLibCheck",
                "--ignoreConfig",
                "--allowJs",
                "--checkJs",
                "false",
                "--jsx",
                "preserve",
                "--strictNullChecks",
                "false",
                "--pretty",
                "false",
                "--listFiles",
            ])
            .args(extra_args)
            .arg(&environment)
            .arg(&path)
            .current_dir(project.path())
            .output()
            .map_err(|error| format!("failed to run tsc syntax gate: {error}"))?;
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("TSC_VALIDITY {name} EXIT={:?}", output.status.code());
        let Some(exit_code) = output.status.code() else {
            return Err(format!(
                "tsc terminated without an exit code for {name}:\n{diagnostics}"
            ));
        };
        let target_name = format!("{name}.{extension}");
        let analyzed_target = diagnostics.lines().any(|line| {
            Path::new(line.trim())
                .file_name()
                .is_some_and(|file_name| file_name == target_name.as_str())
        });

        let mut parsed_diagnostics = Vec::new();
        for line in diagnostics.lines() {
            let Some(code_start) = line.find("error TS") else {
                continue;
            };
            let digits = line[code_start + "error TS".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            let diagnostic_code = digits.parse::<u32>().map_err(|_| {
                format!("unparseable tsc diagnostic for {name}: {line}\n{diagnostics}")
            })?;
            parsed_diagnostics.push((diagnostic_code, line));
        }

        let rejected = parsed_diagnostics
            .iter()
            .filter(|(diagnostic_code, _)| !matches!(diagnostic_code, 2307 | 7026))
            .map(|(_, line)| *line)
            .collect::<Vec<_>>();
        if !analyzed_target
            || !rejected.is_empty()
            || (exit_code != 0 && parsed_diagnostics.is_empty())
        {
            let failure_output = if rejected.is_empty() {
                diagnostics.clone()
            } else {
                rejected.join("\n")
            };
            return Err(format!(
                "tsc did not complete a clean syntax analysis for {name} (exit {exit_code}, analyzed_target={analyzed_target}):\n{}\n--- emitted ---\n{code}",
                failure_output
            ));
        }
        Ok(())
    }

    fn assert_typescript_syntax_valid(name: &str, code: &str, is_jsx: bool) {
        typescript_syntax_check(
            Path::new("node"),
            &typescript_launcher(),
            name,
            code,
            is_jsx,
            &[],
        )
        .unwrap_or_else(|error| panic!("{error}"));
    }

    fn javascript_module_check(node_program: &Path, name: &str, code: &str) -> Result<(), String> {
        let project = tempfile::tempdir().expect("create JS validity directory");
        let path = project.path().join(format!("{name}.mjs"));
        fs::write(&path, code).expect("write emitted module");
        let output = Command::new(node_program)
            .arg("--check")
            .arg(&path)
            .current_dir(project.path())
            .output()
            .map_err(|error| format!("failed to run node syntax gate: {error}"))?;
        println!("NODE_CHECK {name} EXIT={:?}", output.status.code());
        let Some(exit_code) = output.status.code() else {
            return Err(format!(
                "node --check terminated without an exit code for {name}"
            ));
        };
        if exit_code != 0 {
            return Err(format!(
                "node --check rejected {name}:\n{}{}\n--- emitted ---\n{code}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    fn assert_javascript_module_valid(name: &str, code: &str) {
        javascript_module_check(Path::new("node"), name, code)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// @ai-generated - Negative control for the real TypeScript gate: JSX
    /// closing-tag syntax must be rejected rather than treated as environment noise.
    #[test]
    fn typescript_syntax_gate_rejects_jsx_mismatched_closing_tag() {
        let rejected = typescript_syntax_check(
            Path::new("node"),
            &typescript_launcher(),
            "mismatched_jsx",
            "const view = <Foo><Bar></Foo>;\n",
            false,
            &[],
        );
        assert!(rejected.is_err(), "the gate accepted malformed JSX");
    }

    /// @ai-generated - Execution and malformed-output controls for the tsc gate.
    #[test]
    fn typescript_syntax_gate_proves_execution_and_rejects_invalid_output() {
        let launcher = typescript_launcher();
        let invalid = typescript_syntax_check(
            Path::new("node"),
            &launcher,
            "invalid_emitted_fixture",
            "export const value = ;\n",
            false,
            &[],
        );
        assert!(invalid.is_err(), "the gate accepted invalid emitted TSX");

        let non_invocation = typescript_syntax_check(
            Path::new("node"),
            &launcher,
            "unknown_option",
            "export const value = 1;\n",
            false,
            &["--ignoreConfigZZ"],
        )
        .expect_err("an unknown option must not pass without analysing the fixture");
        assert!(
            non_invocation.contains("TS5023") || non_invocation.contains("analyzed_target=false"),
            "unexpected non-invocation evidence: {non_invocation}"
        );

        let missing_binary = workspace_root().join("__missing_node_for_tsc_validity_gate__");
        assert!(
            typescript_syntax_check(
                &missing_binary,
                &launcher,
                "missing_binary",
                "export const value = 1;\n",
                false,
                &[],
            )
            .is_err(),
            "a missing node binary passed the tsc gate"
        );
    }

    /// @ai-generated - Execution and malformed-output controls for node --check.
    #[test]
    fn javascript_syntax_gate_proves_execution_and_rejects_invalid_output() {
        assert!(
            javascript_module_check(
                Path::new("node"),
                "invalid_emitted_fixture",
                "export const value = ;\n",
            )
            .is_err(),
            "node --check accepted an invalid emitted module"
        );

        let missing_binary = workspace_root().join("__missing_node_for_js_validity_gate__");
        assert!(
            javascript_module_check(&missing_binary, "missing_binary", "export {};\n").is_err(),
            "a missing node binary passed the node --check gate"
        );
    }

    fn artifact_for(source: &str) -> Arc<FrameworkParseArtifact> {
        use verter_language::carrier_grammar::{
            CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
            FrameworkAdapterSemanticVersion,
        };
        use verter_language::registered_source_authority::{
            CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
        };
        let source_authority = RegisteredSourceAuthority::new().unwrap();
        let grammar_authority = CarrierGrammarAuthority::new().unwrap();
        let config = CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap();
        grammar_authority
            .register_carrier_grammar(
                verter_language::FileLanguage::vue(),
                FrameworkAdapterSemanticVersion::new(1).unwrap(),
                CarrierParserGrammarVersion::new(1).unwrap(),
                config.clone(),
            )
            .unwrap();
        let snapshot = source_authority
            .register_source(
                CanonicalFileId::new("file:///fixture.vue"),
                FileIncarnation::new(1),
                SourceGeneration::new(1),
                verter_language::FileLanguage::vue(),
                Arc::from(source),
            )
            .unwrap();
        let accepted = grammar_authority
            .accept_registered_source(&source_authority, &snapshot, &config)
            .unwrap();
        Arc::new(
            crate::framework_common::registered_carrier_projection::__project_registered_carrier_for_store_leader(
                &VueCarrierCompiler::default(),
                &accepted,
            )
            .into_framework_parse_artifact(),
        )
    }

    fn projected_script(code: &str, lang: &str) -> RuntimeBlockContentInput {
        RuntimeBlockContentInput {
            code: Arc::from(code),
            source_map: None,
            lang: lang.to_string(),
            content_artifact_token: format!("content:{lang}"),
            source_space_token: format!("space:{lang}"),
        }
    }

    #[test]
    fn two_projected_script_spaces_remain_typed_unavailable() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<script setup src=\"./setup.js\"></script>",
            "<template><div /></template>"
        );
        let compiler = VueCarrierCompiler::default();
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                block_content: RuntimeBlockContentInputs {
                    script: Some(projected_script("export default {}", "js")),
                    script_setup: Some(projected_script("const answer = 42", "js")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );

        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. })
        ));
    }

    /// @ai-generated - Proves external template lowering receives the inline
    /// setup binding metadata instead of resolving an imported component by name.
    #[test]
    fn projected_template_and_inline_setup_share_runtime_bindings() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>import Foo from './Foo.vue'</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Projected.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<Foo />", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template has a truthful single-source runtime path");
        let template = output.template.expect("standalone render output");

        assert!(
            !template
                .imports
                .iter()
                .any(|name| name.contains("resolveComponent")),
            "a setup import must not degrade to runtime name resolution: {:?}\n{}",
            template.imports,
            template.code
        );
        assert!(
            !template.code.contains("_component_Foo")
                && (template.code.contains("$setup.Foo")
                    || template.code.contains("$setup[\"Foo\"]")
                    || template.code.contains("$setup['Foo']")),
            "the render must address the transferred setup binding:\n{}",
            template.code
        );
        assert_eq!(
            template.output_descriptor.source_map.declared_space_tokens,
            vec!["space:html".to_string()],
            "the standalone render map belongs to the selected template source space"
        );
    }

    #[test]
    fn two_projected_script_spaces_keep_ide_typed_unavailable() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<script setup src=\"./setup.js\"></script>"
        );
        let compiler = VueCarrierCompiler::default();
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("Projected.vue".to_string()),
                want_ide: true,
                block_content: RuntimeBlockContentInputs {
                    script: Some(projected_script("export default {}", "js")),
                    script_setup: Some(projected_script("const answer = 42", "js")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );

        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));
    }

    #[test]
    fn projected_plain_script_runtime_is_typed_unavailable_until_its_module_is_valid() {
        let source = concat!(
            "<script src=\"./logic.js\"></script>",
            "<template><div>{{ a }}</div></template>"
        );
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact_for(source),
            &RuntimeCompileOptions {
                filename: Some("ProjectedPlain.vue".to_string()),
                block_content: RuntimeBlockContentInputs {
                    script: Some(projected_script(
                        "export default { data: () => ({ a: 1 }) }",
                        "js",
                    )),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );

        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. })
        ));
    }

    /// @ai-generated - A single IDE surface may merge carrier script bindings
    /// with a projected template only when both source spaces remain declared.
    #[test]
    fn projected_template_ide_composes_two_source_spaces() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Projected.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("two-source IDE lowering has a composed-map path");
        let ide = output.tsx.expect("IDE output");

        assert!(
            ide.code.contains("count = 1"),
            "script chunk missing:\n{}",
            ide.code
        );
        assert!(
            ide.code.contains("{ count }"),
            "template chunk missing:\n{}",
            ide.code
        );
        assert_eq!(
            ide.output_descriptor.source_space.kind,
            OutputSourceSpaceKind::GeneratedComposite
        );
        assert_eq!(
            ide.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate
        );
        assert_eq!(
            ide.output_descriptor.source_map.declared_space_tokens.len(),
            2
        );
        let raw_map = ide
            .output_descriptor
            .source_map
            .raw_map
            .as_deref()
            .expect("composed map");
        assert!(
            raw_map.contains("space:html"),
            "external source absent from map: {raw_map}"
        );
    }

    /// @ai-generated - The Options API script geometry places an external
    /// template fragment mid-module, so both compiler IDE entry points must fail closed.
    #[test]
    fn external_template_with_plain_script_is_ide_typed_unavailable() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script>export default { data: () => ({ count: 1 }) }</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let artifact = artifact_for(source);
        let block_content = RuntimeBlockContentInputs {
            template: Some(projected_script("<div>{{ count }}</div>", "html")),
            ..Default::default()
        };
        let alloc = oxc_allocator::Allocator::new();

        let bundle = compiler.compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some("ExternalPlain.vue".to_string()),
                source_map: true,
                want_ide: true,
                block_content: block_content.clone(),
                ..Default::default()
            },
            &alloc,
        );
        assert!(matches!(
            bundle,
            Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));

        let direct = compiler.compile_ide(
            source,
            &artifact,
            &IdeCompileOptions {
                filename: Some("ExternalPlain.vue".to_string()),
                block_content,
                ..Default::default()
            },
        );
        assert!(matches!(
            direct,
            Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
        ));
    }

    #[test]
    fn direct_compile_ide_accepts_the_block_content_channel() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let output = compiler
            .compile_ide(
                source,
                &artifact_for(source),
                &IdeCompileOptions {
                    filename: Some("Direct.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<p>{{ count }}</p>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("direct IDE block-content lowering");
        assert!(output.code.contains("{ count }"));
        assert_eq!(
            output
                .output_descriptor
                .source_map
                .declared_space_tokens
                .len(),
            2
        );
    }

    /// @ai-generated - Supplied plain CSS is stage-two input and keeps its external source space.
    #[test]
    fn supplied_external_style_is_scoped_in_its_own_source_space() {
        let source =
            "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalStyle.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(".x { color: red; }"),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-css".to_string(),
                            source_space_token: "space:theme-css".to_string(),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied external style compiles");
        let style = &output.styles[0];

        assert!(style.code.contains(".x[data-v-scope123]"), "{}", style.code);
        assert_eq!(style.lang.as_deref(), Some("css"));
        assert_eq!(
            style.output_descriptor.source_map.declared_space_tokens,
            vec!["space:theme-css"]
        );
        assert_ne!(
            style.output_descriptor.source_space.token,
            "space:theme-css"
        );
    }

    /// @ai-generated - Sequential style stages must not claim an uncomposed exact map.
    #[test]
    fn sequential_external_style_rewrites_report_approximate_map_fidelity() {
        let source =
            "<template><div class=\"x\"/></template><style scoped src=\"./theme.css\"></style>";
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalStyle.vue".to_string()),
                    component_id: Some("scope123".to_string()),
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        styles: vec![Some(RuntimeBlockContentInput {
                            code: Arc::from(".x { color: v-bind(tone); }"),
                            source_map: None,
                            lang: "css".to_string(),
                            content_artifact_token: "artifact:theme-css".to_string(),
                            source_space_token: "space:theme-css".to_string(),
                        })],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied external style compiles");
        let style = &output.styles[0];

        assert!(
            style.code.contains("var(--scope123-tone)"),
            "{}",
            style.code
        );
        assert!(style.code.contains(".x[data-v-scope123]"), "{}", style.code);
        assert_eq!(
            style.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate
        );
    }

    /// @ai-generated - Supplied output for an authored inline template keeps
    /// Vue's production setup-return topology and declares both source spaces.
    #[test]
    fn supplied_inline_template_composes_into_production_script() {
        let source = concat!(
            "<template lang=\"pug\">div {{ count }}</template>",
            "<script setup>const count = 1</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("Supplied.vue".to_string()),
                    is_production: true,
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied inline template has a generated-chunk path");

        assert!(
            output.inline,
            "production supplied-inline topology was lost"
        );
        assert!(
            output.template.is_none(),
            "inline topology emitted a detached render"
        );
        let script = output.script.expect("composed runtime script");
        assert!(script.code.contains("const count = 1"));
        assert!(
            script.code.contains("return (_ctx"),
            "inline render absent:\n{}",
            script.code
        );
        assert_eq!(
            script.output_descriptor.source_space.kind,
            OutputSourceSpaceKind::GeneratedComposite
        );
        assert_eq!(
            script
                .output_descriptor
                .source_map
                .declared_space_tokens
                .len(),
            2
        );
    }

    #[test]
    fn generated_template_hole_uses_registered_geometry_not_user_marker_text() {
        let source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>",
            "const marker = \"/* verter-generated-template-hole */\";",
            "const count = 1",
            "</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("MarkerCollision.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template IDE composition");
        let ide = output.tsx.expect("IDE surface");
        assert!(
            ide.code
                .contains("const marker = \"/* verter-generated-template-hole */\";"),
            "the user's marker literal was used as geometry:\n{}",
            ide.code
        );
        assert!(ide.code.contains("{ count }"));
    }

    #[test]
    fn runtime_template_hole_uses_registered_geometry_not_user_marker_text() {
        let source = concat!(
            "<template lang=\"pug\">div {{ count }}</template>",
            "<script setup>",
            "const marker = \"/* verter-runtime-template-hole */\";",
            "const count = 1",
            "</script>"
        );
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("RuntimeMarkerCollision.vue".to_string()),
                    is_production: true,
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied inline template composition");
        let script = output.script.expect("runtime script");
        assert!(
            script
                .code
                .contains("const marker = \"/* verter-runtime-template-hole */\";"),
            "the user's marker literal was used as geometry:\n{}",
            script.code
        );
        assert!(script.code.contains("return (_ctx"));
    }

    #[test]
    fn supplied_inline_template_with_carrier_parse_error_is_typed_unavailable() {
        let source = "<template lang=\"pug\">div</template>\n<script setup>\nconst a = 1";
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        let result = compiler.compile_bundle(
            source,
            &artifact_for(source),
            &RuntimeCompileOptions {
                filename: Some("BrokenInline.vue".to_string()),
                is_production: true,
                block_content: RuntimeBlockContentInputs {
                    template: Some(projected_script("<div></div>", "html")),
                    ..Default::default()
                },
                ..Default::default()
            },
            &alloc,
        );
        assert!(matches!(
            result,
            Err(CompileUnsupported::BlockContentRuntimeUnavailable { .. })
        ));
    }

    /// @ai-generated - Projected scripts stay IDE-gated until their composed
    /// carrier is valid TypeScript and contains no raw SFC markup.
    #[test]
    fn projected_scripts_are_ide_typed_unavailable() {
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();
        for (source, block_content) in [
            (
                concat!(
                    "<script setup src=\"./logic.ts\"></script>",
                    "<template><div>{{ count }}</div></template>"
                ),
                RuntimeBlockContentInputs {
                    script_setup: Some(projected_script("const count = 1", "ts")),
                    ..Default::default()
                },
            ),
            (
                concat!(
                    "<script src=\"./logic.ts\"></script>",
                    "<template><div>{{ count }}</div></template>"
                ),
                RuntimeBlockContentInputs {
                    script: Some(projected_script(
                        "export default { data: () => ({ count: 1 }) }",
                        "ts",
                    )),
                    ..Default::default()
                },
            ),
        ] {
            let result = compiler.compile_bundle(
                source,
                &artifact_for(source),
                &RuntimeCompileOptions {
                    filename: Some("ProjectedScript.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content,
                    ..Default::default()
                },
                &alloc,
            );
            assert!(matches!(
                result,
                Err(CompileUnsupported::BlockContentIdeUnavailable { .. })
            ));
        }
    }

    /// @ai-generated - Pins per-unit output source-space and artifact identity.
    #[test]
    fn runtime_units_publish_qualified_output_descriptors() {
        let source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>",
            "<style>.root { color: red }</style>"
        );
        let compiler = VueCarrierCompiler::default();
        let artifact = artifact_for(source);
        let alloc = oxc_allocator::Allocator::new();
        let output = compiler
            .compile_bundle(
                source,
                &artifact,
                &RuntimeCompileOptions {
                    filename: Some("Qualified.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("native carrier compile");

        let script = output.script.expect("script output");
        let template = output.template.expect("template output");
        let style = output.styles.first().expect("style output");
        let ide = output.tsx.expect("IDE output");

        for descriptor in [
            &script.output_descriptor,
            &template.output_descriptor,
            &style.output_descriptor,
            &ide.output_descriptor,
        ] {
            assert!(!descriptor.source_space.token.is_empty());
            assert!(!descriptor.content_artifact.token.is_empty());
            assert_eq!(
                descriptor.content_artifact.source_space_token,
                descriptor.source_space.token
            );
            assert_eq!(
                descriptor.source_map.destination_space_token,
                descriptor.source_space.token
            );
            assert_eq!(
                descriptor.source_space.utf8_byte_len,
                descriptor.content_artifact.utf8_byte_len
            );
        }

        assert_eq!(
            script.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Approximate,
            "script rewrites are represented honestly as approximate mappings"
        );
        assert_eq!(
            style.output_descriptor.source_map.fidelity,
            SourceMapFidelity::Exact,
            "an unchanged native style is an exact one-space identity"
        );
    }

    /// @ai-generated - Exercises every currently lifted IDE carrier/template
    /// geometry through the real, execution-proving TypeScript syntax gate.
    #[test]
    fn lifted_ide_classes_pass_tsc_syntax_validation() {
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();

        let native_source = concat!(
            "<script setup>const count = 1</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let native = compiler
            .compile_bundle(
                native_source,
                &artifact_for(native_source),
                &RuntimeCompileOptions {
                    filename: Some("Native.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("native IDE compile")
            .tsx
            .expect("native IDE output");
        assert_typescript_syntax_valid("native", &native.code, native.is_jsx);

        let native_plain_source = concat!(
            "<script lang=\"ts\">export default { data: () => ({ count: 1 }) }</script>",
            "<template><div>{{ count }}</div></template>"
        );
        let native_plain = compiler
            .compile_bundle(
                native_plain_source,
                &artifact_for(native_plain_source),
                &RuntimeCompileOptions {
                    filename: Some("NativePlain.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    ..Default::default()
                },
                &alloc,
            )
            .expect("native plain-script IDE compile")
            .tsx
            .expect("native plain-script IDE output");
        assert_typescript_syntax_valid(
            "native_plain_script",
            &native_plain.code,
            native_plain.is_jsx,
        );

        let external_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let external = compiler
            .compile_bundle(
                external_source,
                &artifact_for(external_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalTemplate.vue".to_string()),
                    source_map: true,
                    want_ide: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template IDE compile")
            .tsx
            .expect("external template IDE output");
        assert_typescript_syntax_valid("external_template", &external.code, external.is_jsx);

        for (name, source, selected_template) in [
            (
                "external_no_script",
                "<template src=\"./view.html\"></template>",
                "<div>external-only</div>",
            ),
            (
                "external_both_scripts",
                concat!(
                    "<template src=\"./view.html\"></template>",
                    "<script lang=\"ts\">export default {}</script>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "<div>{{ count }}</div>",
            ),
            (
                "external_empty_template",
                concat!(
                    "<template src=\"./view.html\"></template>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "",
            ),
            (
                "external_jsx_hostile_attribute",
                concat!(
                    "<template src=\"./view.html\"></template>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "<div title=\"a`b\">{{ count }}</div>",
            ),
            (
                "validated_supplied_template",
                concat!(
                    "<template lang=\"pug\">div authored-only</template>",
                    "<script setup lang=\"ts\">const count = 1</script>"
                ),
                "<div>{{ count }}</div>",
            ),
        ] {
            let ide = compiler
                .compile_bundle(
                    source,
                    &artifact_for(source),
                    &RuntimeCompileOptions {
                        filename: Some(format!("{name}.vue")),
                        source_map: true,
                        want_ide: true,
                        block_content: RuntimeBlockContentInputs {
                            template: Some(projected_script(selected_template, "html")),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    &alloc,
                )
                .unwrap_or_else(|error| panic!("{name} IDE compile failed: {error:?}"))
                .tsx
                .unwrap_or_else(|| panic!("{name} IDE output missing"));
            assert_typescript_syntax_valid(name, &ide.code, ide.is_jsx);
        }

        let direct = compiler
            .compile_ide(
                external_source,
                &artifact_for(external_source),
                &IdeCompileOptions {
                    filename: Some("DirectExternalTemplate.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("direct external-template IDE compile");
        assert_typescript_syntax_valid("direct_external_template", &direct.code, direct.is_jsx);
    }

    /// @ai-generated - Exercises every lifted runtime topology, including the
    /// carrier plain-script arm, through real node --check.
    #[test]
    fn lifted_runtime_classes_pass_node_check() {
        let compiler = VueCarrierCompiler::default();
        let alloc = oxc_allocator::Allocator::new();

        let projected_setup_source = concat!(
            "<script setup src=\"./logic.js\"></script>",
            "<template><div>{{ count }}</div></template>"
        );
        let projected_setup = compiler
            .compile_bundle(
                projected_setup_source,
                &artifact_for(projected_setup_source),
                &RuntimeCompileOptions {
                    filename: Some("ProjectedSetup.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        script_setup: Some(projected_script("const count = 1", "js")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("projected setup runtime compile");
        assert_javascript_module_valid(
            "projected_setup_script",
            &projected_setup.script.expect("projected setup script").code,
        );
        assert_javascript_module_valid(
            "projected_setup_template",
            &projected_setup
                .template
                .expect("carrier template output")
                .code,
        );

        let external_template_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script setup>const count = 1</script>"
        );
        let external_template = compiler
            .compile_bundle(
                external_template_source,
                &artifact_for(external_template_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalTemplate.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template runtime compile");
        assert_javascript_module_valid(
            "external_template_script",
            &external_template
                .script
                .expect("carrier script output")
                .code,
        );
        assert_javascript_module_valid(
            "external_template_render",
            &external_template
                .template
                .expect("external render output")
                .code,
        );

        let external_plain_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script>export default { data: () => ({ count: 1 }) }</script>"
        );
        let external_plain = compiler
            .compile_bundle(
                external_plain_source,
                &artifact_for(external_plain_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalPlain.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template with carrier plain script runtime compile");
        assert_javascript_module_valid(
            "external_plain_script",
            &external_plain
                .script
                .expect("carrier plain script output")
                .code,
        );
        assert_javascript_module_valid(
            "external_plain_render",
            &external_plain
                .template
                .expect("external plain-script render output")
                .code,
        );

        let external_both_source = concat!(
            "<template src=\"./view.html\"></template>",
            "<script>export default {}</script>",
            "<script setup>const count = 1</script>"
        );
        let external_both = compiler
            .compile_bundle(
                external_both_source,
                &artifact_for(external_both_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalBoth.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template with both carrier scripts runtime compile");
        assert_javascript_module_valid(
            "external_both_script",
            &external_both.script.expect("both-scripts output").code,
        );
        assert_javascript_module_valid(
            "external_both_render",
            &external_both
                .template
                .expect("both-scripts render output")
                .code,
        );

        let external_no_script_source = "<template src=\"./view.html\"></template>";
        let external_no_script = compiler
            .compile_bundle(
                external_no_script_source,
                &artifact_for(external_no_script_source),
                &RuntimeCompileOptions {
                    filename: Some("ExternalNoScript.vue".to_string()),
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>external-only</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("external template without carrier scripts runtime compile");
        if let Some(script) = external_no_script.script {
            assert_javascript_module_valid("external_no_script_shell", &script.code);
        }
        assert_javascript_module_valid(
            "external_no_script_render",
            &external_no_script
                .template
                .expect("no-script render output")
                .code,
        );

        let supplied_inline_source = concat!(
            "<template lang=\"pug\">div {{ count }}</template>",
            "<script setup>const count = 1</script>"
        );
        let supplied_inline = compiler
            .compile_bundle(
                supplied_inline_source,
                &artifact_for(supplied_inline_source),
                &RuntimeCompileOptions {
                    filename: Some("SuppliedInline.vue".to_string()),
                    is_production: true,
                    source_map: true,
                    block_content: RuntimeBlockContentInputs {
                        template: Some(projected_script("<div>{{ count }}</div>", "html")),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &alloc,
            )
            .expect("supplied inline runtime compile");
        assert_javascript_module_valid(
            "supplied_inline_script",
            &supplied_inline.script.expect("inline runtime script").code,
        );
    }

    #[test]
    fn script_regions_carry_kind_and_resolved_source_type() {
        let source =
            "<script>export default {}</script>\n<script setup lang=\"tsx\">const a = 1</script>";
        let artifact = artifact_for(source);
        let regions = artifact.script_regions();
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
        assert_eq!(artifact.common.template_regions().len(), 1);
        assert_eq!(artifact.common.style_regions().len(), 2);
        let external_links = artifact.common.external_links();
        let links: Vec<(&ExternalLinkKind, &str)> = external_links
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
        let regions = artifact.script_regions();
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
        assert_eq!(artifact.parser_version, VUE_CARRIER_PARSER_VERSION);
        assert!(artifact.common.diagnostics.is_empty());
    }

    // ── Vue CarrierCompiler impl ───────────────────────────────────

    #[test]
    fn vue_compiler_parse_stamps_the_carrier_parser_version_and_vue_identity() {
        let compiler = VueCarrierCompiler::default();
        assert!(compiler.adapter_id().is_vue());
        let source = "<script setup lang=\"ts\">const a = 1</script>\n<template><div /></template>";
        let artifact = artifact_for(source);
        assert!(artifact.adapter_id.is_vue());
        assert_eq!(artifact.parser_version, VUE_CARRIER_PARSER_VERSION);
        assert_eq!(artifact.script_regions().len(), 1);
        assert_eq!(artifact.common.template_regions().len(), 1);
    }

    #[test]
    fn vue_compiler_eval_source_is_position_preserving_with_script_at_raw_offsets() {
        let compiler = VueCarrierCompiler::default();
        let source = "<template><div/></template>\n<script setup lang=\"ts\">const a = 1</script>";
        let artifact = artifact_for(source);
        let eval = compiler.eval_source(source, &artifact);
        // Length invariant.
        assert_eq!(
            eval.len(),
            source.len(),
            "eval source must equal SFC length"
        );
        // The script region's bytes sit at their raw offsets.
        let region = artifact.script_regions()[0].span;
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
        let artifact = artifact_for(source);
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
        let artifact = artifact_for(source);
        let facts = compiler.template_data(source, &artifact);
        assert!(
            facts.data.components.iter().any(|c| c.tag_name == "Child"),
            "template_data must surface the <Child> component usage"
        );
    }
}
