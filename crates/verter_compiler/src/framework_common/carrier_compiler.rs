//! The carrier-compiler trait and its framework-neutral I/O vocabulary.
//!
//! [`CarrierCompiler`] is the compiler-side mirror of the session-side
//! `FrameworkAdapterRegistry`: one trait every carrier framework
//! implements, driving the four compiler-domain operations the host
//! reaches per carrier file — parse, position-preserving eval-source
//! blanking, IDE (TSX/JSX) codegen, and template-fact extraction. Vue is
//! the reference implementation (`vue_bridge::VueCarrierCompiler`),
//! delegating call-for-call to the existing Vue pipeline.
//!
//! The trait owns NO script-fact extraction surface: script facts for
//! EVERY framework (carrier or not) flow through the one host-registered
//! `ScriptFactProvider` seam in `verter_semantic`. A carrier compiler is
//! parse / eval / IDE / template ONLY.
//!
//! Source maps stay token-precise: each adapter's IDE codegen owns its
//! own [`crate::code_transform::CodeTransform`] (the single source of
//! truth for generated-code edits) and returns the rendered
//! [`IdeOutput`] it produced. The trait does NOT thread a borrowed
//! `CodeTransform` — a shared one would be a SECOND, coarse map that is
//! not the codegen authority, which would weaken the
//! CodeTransform-single-source invariant rather than honour it.

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, FrameworkParseArtifact, LanguageId};

use crate::compile::template_data::RawTemplateData;
use crate::compile::types::{CompileTarget, DestructuredBlockMeta};

/// Parse-affecting options threaded into [`CarrierCompiler::parse`].
///
/// These mirror the parse-cache-key inputs the framework's tokenizer
/// consumes. For Vue they map onto `parse_sfc`'s `delimiters` /
/// `custom_elements`. The default (`None` / `None`) is the exact input
/// the host's carrier parse path has always used.
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    /// Custom interpolation delimiters, when the framework supports them
    /// (Vue `{{ }}` override). `None` uses the framework default.
    pub delimiters: Option<(String, String)>,
    /// Tag-name prefixes treated as custom elements (skip component
    /// resolution). `None` uses the framework default.
    pub custom_elements: Option<Vec<String>>,
}

/// IDE-codegen options threaded into [`CarrierCompiler::compile_ide`].
///
/// The neutral subset the framework-neutral IDE seam needs: the file
/// name (for component-name + source-map identity) and the IDE-only
/// compile target bits. A framework whose IDE output needs richer
/// options extends this struct (a compile-visible decision), never a
/// side channel.
#[derive(Debug, Clone, Default)]
pub struct IdeCompileOptions {
    /// The carrier file name for component-name extraction and source-map
    /// identity. `None` falls back to the framework default.
    pub filename: Option<String>,
    /// When true, skip source-map generation (empty `source_map`).
    pub skip_source_map: bool,
    /// Embed the ambient `@verter/types` module block in the IDE output.
    pub embed_ambient_types: bool,
}

/// The rendered IDE (TSX/JSX) artifact a [`CarrierCompiler::compile_ide`]
/// produces.
///
/// Carries the codegen output verbatim from the adapter's own
/// `CodeTransform` — `code` and `source_map` are the SAME pair that
/// transform generated, so the map stays token-precise and the
/// single-source-of-truth invariant holds.
#[derive(Debug, Clone)]
pub struct IdeOutput {
    /// The generated TSX/JSX code.
    pub code: String,
    /// JSON source-map string (empty when source maps are disabled).
    pub source_map: String,
    /// `true` for JavaScript carriers (`.jsx` output), `false` for
    /// TypeScript (`.tsx`).
    pub is_jsx: bool,
    /// Generation duration in milliseconds.
    pub duration_ms: f64,
    /// Structured metadata for the destructured-block region, when the
    /// codegen produced one (enables direct diagnostic-to-source mapping).
    pub destructured_block: Option<DestructuredBlockMeta>,
}

/// The typed reason a carrier compiler cannot satisfy a requested IDE
/// compile.
///
/// Invariant 4: an unsupported [`CompileTarget`] bit (or a framework that
/// does not project an IDE file) returns this typed value, NEVER a silent
/// empty output and NEVER a panic. The neutral host seam matches on it
/// explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileUnsupported {
    /// The requested target did not include the IDE (`TSX`) bit, so there
    /// is no IDE artifact to produce. Carries the requested target for
    /// diagnostics.
    TargetMissingIde(CompileTarget),
    /// The framework does not project an IDE virtual file at all.
    NoIdeProjection {
        /// The adapter that declined the IDE compile.
        adapter_id: FrameworkAdapterId,
    },
}

/// Framework-neutral template facts extracted from a carrier parse.
///
/// A thin neutral wrapper over the compiler-native [`RawTemplateData`]
/// (component usages, bindings, elements, directives) so the host's
/// template-analysis conversion reaches one shape regardless of
/// framework. Vue populates it from the existing `TEMPLATE_DATA` codegen;
/// an empty `data` is the honest answer for a carrier with no template.
#[derive(Debug, Default)]
pub struct TemplateFacts {
    /// The extracted raw template data (default-empty when the carrier has
    /// no template region).
    pub data: RawTemplateData,
}

/// The compiler-side carrier framework trait.
///
/// One impl per carrier framework. The host's carrier dispatch reaches
/// these four operations through the [`super::registry`] lookup; Vue's
/// impl (`vue_bridge::VueCarrierCompiler`) delegates to the existing Vue
/// pipeline without editing any Vue parser/codegen module.
pub trait CarrierCompiler: Send + Sync {
    /// The adapter id this compiler answers to (the registry key).
    fn adapter_id(&self) -> FrameworkAdapterId;

    /// The carrier LANGUAGE id this compiler serves.
    ///
    /// One adapter may own several languages (a carrier file vs. an
    /// external template); only the CARRIER language dispatches into this
    /// compiler's parse path. The host's carrier dispatch validates a
    /// file's resolved carrier language against this id so a same-adapter
    /// non-carrier row (e.g. an external template) is NOT routed through
    /// the SFC parse path.
    fn carrier_language_id(&self) -> LanguageId;

    /// Parse carrier `source` into the framework-neutral artifact.
    ///
    /// Infallible at this surface: framework tokenizers collect
    /// diagnostics inline rather than failing the parse, so the artifact
    /// is always produced (its `common.diagnostics` / the host's parse
    /// channel carry any problems).
    fn parse(&self, source: &str, opts: &ParseOptions) -> Arc<FrameworkParseArtifact>;

    /// Build the POSITION-PRESERVING eval source for `source`.
    ///
    /// The result is byte-for-byte the SAME LENGTH as `source`: every
    /// script region's bytes sit at their RAW carrier offsets and every
    /// other byte is whitespace-blanked (line terminators preserved so
    /// line/column geometry is unchanged). Because the script text sits at
    /// its raw offsets, every span the downstream TS parser produces is
    /// carrier-absolute by construction.
    fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str>;

    /// Generate the IDE (TSX/JSX) artifact for the carrier, or a typed
    /// [`CompileUnsupported`].
    ///
    /// The adapter's IDE codegen owns its own `CodeTransform` (the
    /// single-source-of-truth for generated-code edits) and returns the
    /// rendered [`IdeOutput`] verbatim — no post-hoc string munging.
    fn compile_ide(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &IdeCompileOptions,
    ) -> Result<IdeOutput, CompileUnsupported>;

    /// Extract framework-neutral template facts from the carrier parse.
    fn template_data(&self, source: &str, artifact: &FrameworkParseArtifact) -> TemplateFacts;
}

#[cfg(test)]
mod contract_tests {
    //! `CarrierCompiler` contract tests against a minimal in-tree TEST
    //! carrier — the reusable contract harness every later carrier
    //! vertical re-runs against its own compiler. The fixture is NOT Vue:
    //! it pins the trait's framework-NEUTRAL contract (the eval-source
    //! length + raw-offset invariant, the typed unsupported answer) with
    //! no Vue coupling.

    use super::*;
    use std::any::Any;
    use verter_language::{
        CarrierParse, FrameworkAdapterId, FrameworkParseCommon, LanguageId, ScriptRegion,
        ScriptRegionKind, ScriptSourceType,
    };
    use verter_span::Span;

    /// A trivial carrier payload — the fixture's parse "result".
    #[derive(Debug)]
    struct FixtureCarrier;
    impl CarrierParse for FixtureCarrier {
        fn __verter_as_any(&self) -> &dyn Any {
            self
        }
        fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    /// A minimal carrier compiler. Its `parse` records ONE script region
    /// for the `@@...@@`-fenced run in the source (a stand-in for a
    /// framework's script block); the rest is "markup" to be blanked.
    struct FixtureCompiler;

    impl FixtureCompiler {
        const ADAPTER: &'static str = "fixture";

        /// Find the byte range between the first `@@` opener and the next
        /// `@@` closer, exclusive — the fixture's "script region".
        fn script_span(source: &str) -> Option<Span> {
            let open = source.find("@@")?;
            let content_start = open + 2;
            let close = source[content_start..].find("@@")? + content_start;
            Some(Span::new(content_start as u32, close as u32))
        }
    }

    impl CarrierCompiler for FixtureCompiler {
        fn adapter_id(&self) -> FrameworkAdapterId {
            FrameworkAdapterId::new(Self::ADAPTER)
        }

        fn carrier_language_id(&self) -> LanguageId {
            LanguageId::new(Self::ADAPTER)
        }

        fn parse(&self, source: &str, _opts: &ParseOptions) -> Arc<FrameworkParseArtifact> {
            let script_regions = Self::script_span(source)
                .map(|span| ScriptRegion {
                    span,
                    source_type: ScriptSourceType::Ts,
                    kind: ScriptRegionKind::Module,
                })
                .into_iter()
                .collect();
            Arc::new(FrameworkParseArtifact::new(
                self.adapter_id(),
                LanguageId::new(Self::ADAPTER),
                1,
                FrameworkParseCommon {
                    script_regions,
                    ..Default::default()
                },
                Arc::new(FixtureCarrier),
            ))
        }

        fn eval_source(&self, source: &str, artifact: &FrameworkParseArtifact) -> Arc<str> {
            let src = source.as_bytes();
            let mut out: Vec<u8> = src
                .iter()
                .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
                .collect();
            for region in &artifact.common.script_regions {
                let (s, e) = (region.span.start as usize, region.span.end as usize);
                if s <= e && e <= src.len() {
                    out[s..e].copy_from_slice(&src[s..e]);
                }
            }
            Arc::from(String::from_utf8(out).unwrap().as_str())
        }

        fn compile_ide(
            &self,
            _source: &str,
            _artifact: &FrameworkParseArtifact,
            opts: &IdeCompileOptions,
        ) -> Result<IdeOutput, CompileUnsupported> {
            // The fixture projects no IDE virtual file — it returns the
            // typed unsupported answer (invariant 4), never a silent empty.
            let _ = opts;
            Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            })
        }

        fn template_data(
            &self,
            _source: &str,
            _artifact: &FrameworkParseArtifact,
        ) -> TemplateFacts {
            TemplateFacts::default()
        }
    }

    #[test]
    fn adapter_id_is_the_registration_key() {
        let compiler = FixtureCompiler;
        assert_eq!(compiler.adapter_id(), FrameworkAdapterId::new("fixture"));
    }

    #[test]
    fn eval_source_is_position_preserving_same_length_with_script_bytes_at_raw_offsets() {
        let compiler = FixtureCompiler;
        // `markup @@const x = 1@@ trailing` — the script run is between @@.
        let source = "markup @@const x = 1@@ trailing\nsecond line";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let eval = compiler.eval_source(source, &artifact);

        // Length invariant: byte-for-byte same length.
        assert_eq!(
            eval.len(),
            source.len(),
            "eval source must be position-preserving (same byte length)"
        );

        // The script region's bytes sit at their RAW offsets, unchanged.
        let region = artifact.common.script_regions[0].span;
        let (s, e) = (region.start as usize, region.end as usize);
        assert_eq!(
            &eval[s..e],
            "const x = 1",
            "script bytes must be copied verbatim at their raw carrier offsets"
        );
        assert_eq!(&source[s..e], &eval[s..e]);

        // Every non-script, non-line-terminator byte is blanked to a space.
        for (i, (&sb, eb)) in source.as_bytes().iter().zip(eval.bytes()).enumerate() {
            if i >= s && i < e {
                continue; // script region — verified above
            }
            if sb == b'\n' || sb == b'\r' {
                assert_eq!(eb, sb, "line terminators are preserved at offset {i}");
            } else {
                assert_eq!(eb, b' ', "non-script byte at offset {i} must be blanked");
            }
        }
    }

    #[test]
    fn eval_source_with_no_script_region_is_all_blank_same_length() {
        let compiler = FixtureCompiler;
        let source = "no script here\njust markup";
        let artifact = compiler.parse(source, &ParseOptions::default());
        assert!(artifact.common.script_regions.is_empty());
        let eval = compiler.eval_source(source, &artifact);
        assert_eq!(eval.len(), source.len());
        for (&sb, eb) in source.as_bytes().iter().zip(eval.bytes()) {
            if sb == b'\n' || sb == b'\r' {
                assert_eq!(eb, sb);
            } else {
                assert_eq!(eb, b' ');
            }
        }
    }

    #[test]
    fn compile_ide_returns_typed_unsupported_never_silent_empty() {
        let compiler = FixtureCompiler;
        let source = "@@x@@";
        let artifact = compiler.parse(source, &ParseOptions::default());
        let err = compiler
            .compile_ide(source, &artifact, &IdeCompileOptions::default())
            .expect_err("the fixture projects no IDE file");
        assert_eq!(
            err,
            CompileUnsupported::NoIdeProjection {
                adapter_id: FrameworkAdapterId::new("fixture"),
            }
        );
    }

    #[test]
    fn target_missing_ide_carries_the_requested_target() {
        // The typed unsupported variant carries the offending target — a
        // bundler-only target has no IDE bit.
        let err = CompileUnsupported::TargetMissingIde(CompileTarget::BUNDLER);
        match err {
            CompileUnsupported::TargetMissingIde(t) => {
                assert!(!t.needs_tsx(), "BUNDLER carries no TSX bit");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
