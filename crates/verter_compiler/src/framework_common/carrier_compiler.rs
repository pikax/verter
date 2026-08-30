//! Carrier-compiler trait and framework-neutral I/O.
//!
//! One trait per carrier: parse, IDE codegen, runtime bundle.
//! Vue is the reference (`vue_bridge::VueCarrierCompiler`). Eval-source
//! and template facts belong to
//! [`super::capability::FrameworkSemanticAuthority`]. Script facts go
//! through the host `ScriptFactProvider` seam — not this trait.
//!
//! Each adapter's IDE codegen owns its own
//! [`crate::code_transform::CodeTransform`]. The trait does not thread
//! a borrowed transform (that would be a second, coarse map).

use std::sync::Arc;

use sha2::{Digest, Sha256};
use verter_language::{
    FrameworkAdapterId, LanguageId, ParseOptions, SyntaxReject, UnregisteredFrameworkParseArtifact,
};

use super::FrameworkParseArtifact;

use crate::compile::template_data::RawTemplateData;
use crate::compile::types::{CompileDiagnosticSeverity, DestructuredBlockMeta};

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
    /// Host-validated semantic block bytes for IDE multi-unit lowering.
    pub block_content: RuntimeBlockContentInputs,
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
    /// Qualified identity and source-map chain for this emitted unit.
    pub output_descriptor: RuntimeOutputDescriptor,
    /// Internal generated splice point used by multi-source lowering.
    #[doc(hidden)]
    pub generated_template_hole: Option<std::ops::Range<u32>>,
    /// Internal generated template chunk used by multi-source lowering.
    #[doc(hidden)]
    pub generated_template_chunk: Option<crate::compile::types::GeneratedCodeChunk>,
}

/// Role of one source space participating in an emitted compiler unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSourceSpaceKind {
    /// The registered carrier source.
    Owner,
    /// A host-admitted external block source.
    External,
    /// One derived compiler output over a single source space.
    DerivedTransform,
    /// One generated unit containing chunks from multiple source spaces.
    GeneratedComposite,
}

/// Truthfulness grade of a source-map edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMapFidelity {
    /// Every mapped run preserves an exact byte-for-byte correspondence.
    Exact,
    /// At least one replacement is anchored only approximately.
    Approximate,
}

/// Compiler-side mirror of the host's source-space descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSourceSpaceDescriptor {
    pub token: String,
    pub kind: OutputSourceSpaceKind,
    pub source_token: String,
    pub content_hash: String,
    pub utf8_byte_len: u64,
}

/// Identity of the exact bytes emitted for one compiler unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputContentArtifactDescriptor {
    pub token: String,
    pub source_space_token: String,
    pub content_hash: String,
    pub utf8_byte_len: u64,
}

/// Source map qualified by both its generated destination and all declared
/// input spaces. A missing raw map is an identity only when destination and the
/// sole declared source token are equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedOutputSourceMap {
    pub map_hash: String,
    pub destination_space_token: String,
    pub declared_space_tokens: Vec<String>,
    pub raw_map: Option<String>,
    pub fidelity: SourceMapFidelity,
}

/// Complete provenance declaration for one emitted code unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOutputDescriptor {
    pub source_space: OutputSourceSpaceDescriptor,
    pub content_artifact: OutputContentArtifactDescriptor,
    pub source_map: QualifiedOutputSourceMap,
}

fn descriptor_hash(domain: &[u8], parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part);
    }
    let mut out = String::with_capacity(71);
    out.push_str("sha256:");
    for byte in digest.finalize() {
        use std::fmt::Write;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

impl RuntimeOutputDescriptor {
    /// Describe compiler-generated bytes over one or more declared sources.
    #[must_use]
    pub fn generated(
        code: &str,
        raw_map: Option<&str>,
        declared_sources: &[(&str, &str)],
        fidelity: SourceMapFidelity,
    ) -> Self {
        let content_hash = descriptor_hash(b"verter.output.content.v1\0", &[code.as_bytes()]);
        let mut source_tokens = declared_sources
            .iter()
            .map(|(space, _)| (*space).to_string())
            .collect::<Vec<_>>();
        source_tokens.sort();
        source_tokens.dedup();
        let mut identity_parts = source_tokens
            .iter()
            .map(|value| value.as_bytes())
            .collect::<Vec<_>>();
        identity_parts.push(content_hash.as_bytes());
        let source_space_token =
            descriptor_hash(b"verter.output.source-space.v1\0", &identity_parts);
        let source_token = declared_sources
            .first()
            .map_or("unattributed", |(_, artifact)| *artifact)
            .to_string();
        let artifact_token = descriptor_hash(
            b"verter.output.artifact.v1\0",
            &[source_space_token.as_bytes(), content_hash.as_bytes()],
        );
        let map_hash = descriptor_hash(
            b"verter.output.qualified-map.v1\0",
            &[
                source_space_token.as_bytes(),
                raw_map.unwrap_or("unmapped").as_bytes(),
            ],
        );
        let kind = if source_tokens.len() > 1 {
            OutputSourceSpaceKind::GeneratedComposite
        } else {
            OutputSourceSpaceKind::DerivedTransform
        };
        let utf8_byte_len = code.len() as u64;
        Self {
            source_space: OutputSourceSpaceDescriptor {
                token: source_space_token.clone(),
                kind,
                source_token,
                content_hash: content_hash.clone(),
                utf8_byte_len,
            },
            content_artifact: OutputContentArtifactDescriptor {
                token: artifact_token,
                source_space_token: source_space_token.clone(),
                content_hash,
                utf8_byte_len,
            },
            source_map: QualifiedOutputSourceMap {
                map_hash,
                destination_space_token: source_space_token,
                declared_space_tokens: source_tokens,
                raw_map: raw_map.map(str::to_string),
                fidelity,
            },
        }
    }

    /// Provenance for a rewrite whose caller did not request a source map.
    ///
    /// Skips SHA-256 of the rewritten bytes and of the map payload. Isolated
    /// style stages use this when `want_source_map` is false so a caller that
    /// only observes rewritten code or facts does not pay identity hashing.
    #[must_use]
    pub fn generated_without_map(
        utf8_byte_len: u64,
        source_space_token: &str,
        content_artifact_token: &str,
    ) -> Self {
        Self {
            source_space: OutputSourceSpaceDescriptor {
                token: source_space_token.to_string(),
                kind: OutputSourceSpaceKind::DerivedTransform,
                source_token: content_artifact_token.to_string(),
                content_hash: String::new(),
                utf8_byte_len,
            },
            content_artifact: OutputContentArtifactDescriptor {
                token: String::new(),
                source_space_token: source_space_token.to_string(),
                content_hash: String::new(),
                utf8_byte_len,
            },
            source_map: QualifiedOutputSourceMap {
                map_hash: String::new(),
                destination_space_token: source_space_token.to_string(),
                declared_space_tokens: vec![source_space_token.to_string()],
                raw_map: None,
                fidelity: SourceMapFidelity::Exact,
            },
        }
    }

    /// Describe byte-identical output that remains in an admitted input space.
    #[must_use]
    pub fn identity(code: &str, source_space_token: &str, content_artifact_token: &str) -> Self {
        let content_hash = descriptor_hash(b"verter.output.content.v1\0", &[code.as_bytes()]);
        let utf8_byte_len = code.len() as u64;
        Self {
            source_space: OutputSourceSpaceDescriptor {
                token: source_space_token.to_string(),
                kind: OutputSourceSpaceKind::External,
                source_token: content_artifact_token.to_string(),
                content_hash: content_hash.clone(),
                utf8_byte_len,
            },
            content_artifact: OutputContentArtifactDescriptor {
                token: content_artifact_token.to_string(),
                source_space_token: source_space_token.to_string(),
                content_hash,
                utf8_byte_len,
            },
            source_map: QualifiedOutputSourceMap {
                map_hash: descriptor_hash(
                    b"verter.output.identity-map.v1\0",
                    &[source_space_token.as_bytes()],
                ),
                destination_space_token: source_space_token.to_string(),
                declared_space_tokens: vec![source_space_token.to_string()],
                raw_map: None,
                fidelity: SourceMapFidelity::Exact,
            },
        }
    }

    /// Deterministic identity for direct compiler callers without host tokens.
    #[must_use]
    pub fn carrier_source(source: &str) -> (String, String) {
        let content_hash =
            descriptor_hash(b"verter.output.carrier-content.v1\0", &[source.as_bytes()]);
        let source_space = descriptor_hash(
            b"verter.output.carrier-space.v1\0",
            &[content_hash.as_bytes()],
        );
        let artifact = descriptor_hash(
            b"verter.output.carrier-artifact.v1\0",
            &[source_space.as_bytes(), content_hash.as_bytes()],
        );
        (source_space, artifact)
    }
}

/// The typed reason a carrier compiler cannot satisfy a requested IDE
/// compile.
///
/// Invariant 4: an unsupported `CompileTarget` bit (or a framework that
/// does not project an IDE file) returns this typed value, NEVER a silent
/// empty output and NEVER a panic. The neutral host seam matches on it
/// explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileUnsupported {
    /// The requested target did not include the IDE (`TSX`) bit, so there
    /// is no IDE artifact to produce. No longer carries the requested
    /// target: `CompileTarget` is crate-private (the canonical
    /// `CompileRequest` is the only production-reachable option authority),
    /// and every external match on this variant already discards the
    /// payload (`TargetMissingIde(_)`).
    TargetMissingIde,
    /// The framework does not project an IDE virtual file at all.
    NoIdeProjection {
        /// The adapter that declined the IDE compile.
        adapter_id: FrameworkAdapterId,
    },
    /// Runtime lowering cannot truthfully compile selected template/script
    /// bytes that live outside the carrier source space.
    BlockContentRuntimeUnavailable {
        /// The adapter that declined the runtime compile.
        adapter_id: FrameworkAdapterId,
    },
    /// IDE lowering cannot truthfully include selected template/script bytes
    /// that live outside the carrier source space in its unified TS surface.
    BlockContentIdeUnavailable {
        /// The adapter that declined the IDE compile.
        adapter_id: FrameworkAdapterId,
    },
    /// The canonical `CompileRequest` refused at post-parse resolution —
    /// `SSR x Vapor` (implicit `<template vapor>` marker) or `inline x
    /// Vapor` (deferred capability). Every other fail-closed rule is
    /// enforced at `CompileRequest::new` (before any carrier compile is
    /// attempted at all); this is the residual post-parse half.
    RequestExecutionRefused(crate::compile_request::CompileRequestError),
}

/// Runtime-codegen options threaded into [`CarrierCompiler::compile_bundle`].
///
/// The neutral, framework-shared subset of the host's compile profile a
/// carrier consults to produce its executable module. Each carrier reads
/// only the options it supports (Vue ignores Svelte-specific output knobs
/// and vice-versa). A framework whose runtime output needs a richer option
/// extends this struct (a compile-visible decision), never a side channel.
#[derive(Debug, Clone)]
pub struct RuntimeCompileOptions {
    /// The carrier file name for component-name + source-map identity.
    /// `None` falls back to the framework default.
    pub filename: Option<String>,
    /// Production mode: strips dev-only code (`__file`, HMR, dev instrumentation).
    pub is_production: bool,
    /// Compile the carrier as a custom element. This does not classify
    /// template tags; each carrier maps it to its explicit compile-option axis.
    pub custom_element: bool,
    /// Generate source maps for the produced runtime output.
    pub source_map: bool,
    /// Server-side rendering mode (the carrier emits its SSR backend).
    pub ssr: bool,
    /// Runtime module name for helper imports (Vue default `"vue"`).
    pub runtime_module_name: Option<String>,
    /// Explicit component / scope id for scoped-style hashing.
    pub component_id: Option<String>,
    /// The RESOLVED Svelte `cssHash` scope-class override — the official user
    /// `cssHash` callback's already-computed result, preserved byte-exact. When
    /// `Some`, the Svelte carrier uses it VERBATIM as the scope class (in the
    /// serialized HTML skeleton and the external/injected CSS) instead of the
    /// default `svelte-<hash>` derivation. The callback runs OUTSIDE the compiler
    /// (session/API boundary); this carries only its resolved value. Vue ignores
    /// it. Distinct from `component_id` (a Vue explicit scope id — never overloaded
    /// for the Svelte cssHash override).
    pub svelte_css_hash_override: Option<String>,
    /// Svelte `ModuleCompileOptions.dev`. Vue ignores it.
    pub svelte_dev: Option<bool>,
    /// Svelte `CompileOptions.runes` — explicit `true`/`false` selection;
    /// `None` defers to source/usage inference. Vue ignores it.
    pub svelte_runes: Option<bool>,
    /// Svelte `CompileOptions.namespace` — `"html"`, `"svg"`, or
    /// `"mathml"`. Vue ignores it.
    pub svelte_namespace: Option<String>,
    /// Svelte `CompileOptions.fragments` — `"html"` or `"tree"`. Vue
    /// ignores it.
    pub svelte_fragments: Option<String>,
    /// Svelte `CompileOptions.preserveWhitespace`. Vue ignores it.
    pub svelte_preserve_whitespace: Option<bool>,
    /// Svelte `CompileOptions.preserveComments`. Vue ignores it.
    pub svelte_preserve_comments: Option<bool>,
    /// Svelte `CompileOptions.discloseVersion`. Vue ignores it.
    pub svelte_disclose_version: Option<bool>,
    /// Force JavaScript output (strip TypeScript syntax).
    pub force_js: bool,
    /// Force Vapor-mode codegen regardless of template attributes (Vue).
    pub force_vapor: bool,
    /// Preserve HTML comments in template output (`None` = framework default).
    pub comments: Option<bool>,
    /// Custom template expression delimiters (Vue `{{ }}` override).
    pub delimiters: Option<(String, String)>,
    /// Tag names treated as custom elements (skip component resolution).
    pub custom_elements: Option<Vec<String>>,
    /// When true, request the RUNTIME output products — the executable main
    /// module and its script / template / style side-files.
    ///
    /// The runtime half of the requested-product set, alongside
    /// [`want_ide`](Self::want_ide) and
    /// [`want_template_data`](Self::want_template_data). A carrier attempts its
    /// runtime compile ONLY when this is true, so a request that did not ask
    /// for a runtime product can never be refused one.
    pub want_runtime: bool,
    /// When true, ALSO request the IDE (`tsx`) artifact in the same pass so
    /// the host populates its `CachedTsx` slot from one compile.
    pub want_ide: bool,
    /// When true, ALSO fill framework-neutral template facts in the same
    /// pass from the catalog semantic authority (for template analysis).
    pub want_template_data: bool,
    /// Types module name for IDE/TSX helper imports (default `"$verter/types"`).
    pub types_module_name: Option<String>,
    /// Embed `declare module "@verter/types"` in any produced IDE output.
    pub embed_ambient_types: bool,
    /// Experimental: enable conditional root generic narrowing (IDE codegen).
    pub conditional_root_narrowing: bool,
    /// Experimental: strict slot children type checking (IDE codegen).
    pub strict_slots: bool,
    /// Host-validated content artifacts for blocks whose bytes do not live in
    /// the carrier source space (external `src` or supplied preprocessor
    /// output). The host derives these parser-local slots only after resolving
    /// an exact sealed block reference; no ordinal crosses the public handoff.
    pub block_content: RuntimeBlockContentInputs,
    /// Inline the render function inside `setup()` (Vue production topology,
    /// official `compileScript({ inlineTemplate: true })`). `None` resolves to
    /// `is_production` — matching the official default (inline in prod
    /// builds). Only the client VDOM backend honors this; Vapor inline and
    /// inline SSR are deferred (the carrier falls back to non-inline).
    pub inline: Option<bool>,
    /// Vue-PRIVATE resolved compile inputs — the host-resolved macro DTO,
    /// prop-constness overrides, and style `v-bind()` usage facts. Typed
    /// (replaces the former `framework_extras: Option<Arc<dyn Any + Send +
    /// Sync>>` opaque downcast channel per the ruling's ephemeral
    /// execution-input carrier: excluded from `CompileRequest` identity,
    /// but no longer erased — a Svelte carrier simply ignores it.
    pub vue_facts: Option<crate::compile::types::VueExecutionInputs>,
    /// Host-retained parsed style IRs, one slot per style block in inventory
    /// order. Excluded from request/cache identity.
    pub prepared_styles: Vec<Option<crate::style_planner::PreparedStyleIr>>,
}

impl Default for RuntimeCompileOptions {
    /// The plain RUNTIME compile: the runtime products are requested, the IDE
    /// and template-data products are not.
    ///
    /// Written out rather than derived because [`want_runtime`](Self::want_runtime)
    /// must default to `true`. This struct is the option set for
    /// `compile_bundle`, whose subject is the runtime module; a derived `false`
    /// would make the default request ask for NO product at all and answer with
    /// a silently empty bundle instead of a compile.
    fn default() -> Self {
        Self {
            filename: None,
            is_production: false,
            custom_element: false,
            source_map: false,
            ssr: false,
            runtime_module_name: None,
            component_id: None,
            svelte_css_hash_override: None,
            svelte_dev: None,
            svelte_runes: None,
            svelte_namespace: None,
            svelte_fragments: None,
            svelte_preserve_whitespace: None,
            svelte_preserve_comments: None,
            svelte_disclose_version: None,
            force_js: false,
            force_vapor: false,
            comments: None,
            delimiters: None,
            custom_elements: None,
            want_runtime: true,
            want_ide: false,
            want_template_data: false,
            types_module_name: None,
            embed_ambient_types: false,
            conditional_root_narrowing: false,
            strict_slots: false,
            block_content: RuntimeBlockContentInputs::default(),
            inline: None,
            vue_facts: None,
            prepared_styles: Vec::new(),
        }
    }
}

/// One compiler-owned block input selected by the registered host.
#[derive(Debug, Clone)]
pub struct RuntimeBlockContentInput {
    pub code: Arc<str>,
    pub source_map: Option<Arc<str>>,
    pub lang: String,
    /// Host-minted identity of the exact selected bytes (including any
    /// supplied map). Compiler code carries this opaquely; it never accepts
    /// caller-selected parser ordinals.
    pub content_artifact_token: String,
    /// Host-minted source space containing `code`.
    pub source_space_token: String,
    /// Parsed IR for these exact bytes, when the host admitted one.
    pub parsed: Option<crate::style_planner::PreparedStyleIr>,
}

/// Parser-local projection of validated block content. Ordering exists only at
/// this compiler boundary and is derived from the carrier inventory; callers
/// cannot address a block by these positions.
#[derive(Debug, Clone, Default)]
pub struct RuntimeBlockContentInputs {
    pub template: Option<RuntimeBlockContentInput>,
    pub script: Option<RuntimeBlockContentInput>,
    pub script_setup: Option<RuntimeBlockContentInput>,
    pub styles: Vec<Option<RuntimeBlockContentInput>>,
    pub custom_blocks: Vec<Option<RuntimeBlockContentInput>>,
}

impl RuntimeBlockContentInputs {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.template.is_none()
            && self.script.is_none()
            && self.script_setup.is_none()
            && self.styles.iter().all(Option::is_none)
            && self.custom_blocks.iter().all(Option::is_none)
    }

    /// Whether selected template or script bytes live outside the carrier
    /// source space and therefore require multi-unit semantic lowering.
    #[must_use]
    pub fn has_external_semantic_unit(&self) -> bool {
        self.template.is_some() || self.script.is_some() || self.script_setup.is_some()
    }
}

/// A framework-neutral severity for a runtime-bundle diagnostic.
///
/// Mirrors [`CompileDiagnosticSeverity`] in a carrier-shared shape so the
/// host lifts bundle diagnostics into its `DiagnosticsSnapshot` without
/// naming any framework-specific diagnostic type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeDiagnosticSeverity {
    /// A hard error (fails the compile).
    Error,
    /// A non-fatal warning.
    Warning,
    /// Informational.
    Info,
}

impl From<CompileDiagnosticSeverity> for RuntimeDiagnosticSeverity {
    fn from(value: CompileDiagnosticSeverity) -> Self {
        match value {
            CompileDiagnosticSeverity::Error => Self::Error,
            CompileDiagnosticSeverity::Warning => Self::Warning,
            CompileDiagnosticSeverity::Info => Self::Info,
        }
    }
}

/// A framework-neutral runtime-bundle diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDiagnostic {
    /// Severity.
    pub severity: RuntimeDiagnosticSeverity,
    /// A framework-defined code string.
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Carrier-absolute source span. A diagnostic with no finer-grained
    /// location (a whole-component oracle result) carries the whole-source
    /// span rather than an absent one — the producer decides that mapping,
    /// never a downstream call site.
    pub span: verter_span::Span,
}

/// The framework-OWNED ESM body the carrier emits for the runtime module.
///
/// Vue's carrier currently produces NO standalone body here — the Vue
/// `_sfc_main` shape is assembled host-side from the neutral block fields
/// (host virtual-file concern: style/custom virtual imports + HMR), so Vue
/// leaves `body_code` `None` and the host assembles. A framework whose
/// runtime module is a single self-contained ESM (e.g. Svelte's official-shaped
/// runtime output) returns `Some` here and the host emits it verbatim.
#[derive(Debug, Clone, Default)]
pub struct RuntimeMainModule {
    /// The framework-owned ESM body, when the carrier emits one directly.
    /// `None` ⇒ the host assembles the main module from the block fields.
    pub body_code: Option<String>,
    /// Source map for `body_code` (empty when none / disabled).
    pub source_map: String,
    /// The language id of the produced body (`"js"` / `"ts"`), when known.
    pub lang: Option<String>,
}

/// A framework-neutral compiled `<script>` block.
#[derive(Debug, Clone)]
pub struct RuntimeScriptBlock {
    /// The generated script code.
    pub code: String,
    /// Source map (empty when none / disabled).
    pub source_map: String,
    /// Whether the block is a `<script setup>` (Vue) / instance script.
    pub setup: bool,
    /// Qualified identity and source-map chain for this emitted unit.
    pub output_descriptor: RuntimeOutputDescriptor,
    #[doc(hidden)]
    pub generated_template_hole: Option<std::ops::Range<u32>>,
    #[doc(hidden)]
    pub runtime_imports: Vec<String>,
    /// Every `__sfc__`→`_sfc_main` rename target plus the removable
    /// terminal default-export statement, declared by this block's own
    /// producer — see `crate::assembly::fragment::SfcExportPlacement`.
    #[doc(hidden)]
    pub sfc_export_placement: Option<crate::assembly::fragment::SfcExportPlacement>,
}

/// The property name the compiled template's exported render function must be
/// assigned to on the component object — a fact of which codegen backend
/// produced the block, declared once by that backend rather than inferred
/// downstream from the generated function's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateRenderExport {
    /// `_sfc_main.render = render`.
    Render,
    /// `_sfc_main.ssrRender = ssrRender`.
    SsrRender,
}

/// A framework-neutral compiled template / render-function block.
#[derive(Debug, Clone)]
pub struct RuntimeTemplateBlock {
    /// The generated template / render-function code.
    pub code: String,
    /// Source map (empty when none / disabled).
    pub source_map: String,
    /// Runtime helper imports from the runtime module.
    pub imports: Vec<String>,
    /// SSR runtime helper imports (empty for non-SSR builds).
    pub ssr_imports: Vec<String>,
    /// Which property the emitted function attaches to — declared by the
    /// producing backend, never recovered by scanning [`Self::code`].
    pub render_export: TemplateRenderExport,
    /// Qualified identity and source-map chain for this emitted unit.
    pub output_descriptor: RuntimeOutputDescriptor,
}

/// A framework-neutral compiled `<style>` block.
#[derive(Debug, Clone)]
pub struct RuntimeStyleBlock {
    /// The processed CSS code.
    pub code: String,
    /// Source map for [`code`](Self::code) — `Some` ONLY when the compile
    /// demanded maps ([`RuntimeCompileOptions::source_map`]) AND the
    /// framework produces one for its css artifact (Svelte's external
    /// `css.map`, generated from the same transform that rendered the code).
    /// `None` otherwise.
    pub source_map: Option<String>,
    /// Optional explicit language (`scss`, …); `None` ⇒ plain CSS.
    pub lang: Option<String>,
    /// The component SCOPE HASH the block's selectors are scoped under, when
    /// the framework scopes by class (Svelte's `svelte-<djb2>` — the same hash
    /// baked into the component's markup). `None` when the framework carries
    /// its scope on [`RuntimeCompileOutput::scope_id`] instead (Vue's
    /// `data-v-…` attribute scoping) or the block is unscoped.
    pub scope_hash: Option<String>,
    /// Whether the block's css includes GLOBAL css (Svelte's `:global(...)`
    /// — the official `css.hasGlobal`). Always `false` for a framework whose
    /// style pipeline carries no such fact (Vue).
    pub has_global: bool,
    /// Qualified identity and source-map chain for this emitted unit.
    pub output_descriptor: RuntimeOutputDescriptor,
}

/// A framework-neutral custom block (`<i18n>`, `<docs>`, …).
#[derive(Debug, Clone)]
pub struct RuntimeCustomBlock {
    /// The block tag (e.g. `"i18n"`).
    pub block_type: String,
    /// Raw block content.
    pub content: String,
}

/// The neutral runtime bundle a [`CarrierCompiler::compile_bundle`] produces.
///
/// NOT `VerterCompileResult` (which is Vue-shaped). It losslessly carries
/// every field the host's runtime assembly + virtual-file population needs:
/// the framework-owned main body (when the carrier emits one directly), the
/// block side-files, styles + custom blocks, the scope id, the IDE artifact
/// (when requested in the same pass), template facts, and diagnostics.
///
/// It carries only what the request ASKED FOR: a field is populated when its
/// `want_*` option was set, so a bundle is not a whole-artifact set. A carrier
/// that FAIL-CLOSES on a requested runtime surface does not produce a bundle at
/// all — it returns the product-free
/// [`CarrierCompileOutcome::RuntimeSurfaceRefused`] arm instead, so there is no
/// shape in which a refusal carries an IDE artifact beside it.
///
/// Not `Clone` — `RawTemplateData` is move-only, and the bundle is consumed
/// once by the host's virtual-file population.
#[derive(Debug, Default)]
pub struct RuntimeCompileOutput {
    /// The framework-owned main module (body / map / lang). `body_code`
    /// `None` ⇒ the host assembles from the block fields (Vue) OR there is
    /// no runtime surface (Svelte).
    pub main: RuntimeMainModule,
    /// The compiled `<script>` block, when present.
    pub script: Option<RuntimeScriptBlock>,
    /// The compiled template / render-function block, when present.
    pub template: Option<RuntimeTemplateBlock>,
    /// Compiled `<style>` blocks in source order.
    pub styles: Vec<RuntimeStyleBlock>,
    /// Custom blocks in source order.
    pub custom_blocks: Vec<RuntimeCustomBlock>,
    /// The scope id (`data-v-xxxxxxxx`), empty when none.
    pub scope_id: String,
    /// The IDE (TSX/JSX) artifact, present when `want_ide` was requested AND
    /// the carrier projects one.
    pub tsx: Option<IdeOutput>,
    /// Framework-neutral template facts, present when `want_template_data`
    /// was requested AND the catalog semantic authority produced them.
    /// Catalog miss / parse-key mismatch / producer failure stay `None`
    /// (typed refusal). A valid template-free carrier is `Some` empty facts.
    pub template_data: Option<RawTemplateData>,
    /// Diagnostics emitted during the runtime compile. The host lifts these
    /// into its `DiagnosticsSnapshot`; an error here fails the compile.
    pub diagnostics: Vec<RuntimeDiagnostic>,
    /// Whether the render function was inlined into `setup()` (Vue production
    /// topology). When true, `script` contains the complete component and
    /// `template` is `None` — host assembly must NOT attach a standalone
    /// render function or filter the setup return.
    pub inline: bool,
}

impl RuntimeCompileOutput {
    /// Whether this bundle carries a RUNTIME surface (a directly-emitted
    /// body OR host-assemblable block side-files). A carrier that produced
    /// only an IDE artifact (Svelte) returns `false` — the host populates
    /// the `CachedTsx` slot and emits no `Main` virtual node.
    #[must_use]
    pub fn has_runtime_surface(&self) -> bool {
        self.main.body_code.is_some()
            || self.script.is_some()
            || self.template.is_some()
            || !self.styles.is_empty()
            || !self.custom_blocks.is_empty()
    }

    /// Whether any diagnostic in the bundle is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == RuntimeDiagnosticSeverity::Error)
    }
}

/// Test-only convenience over [`CarrierCompiler::compile_bundle`] for fixtures
/// whose carrier is known to PRODUCE.
///
/// Deliberately test-only: production code matches the outcome exhaustively, so
/// a refusal can never be unwrapped into "some bundle" there. A test that is
/// ABOUT the refusal calls `compile_bundle` directly and matches the arm.
#[cfg(test)]
pub(crate) trait CompileBundleProducedExt {
    fn compile_bundle_expect_produced(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, CompileUnsupported>;
}

#[cfg(test)]
impl<T: CarrierCompiler + ?Sized> CompileBundleProducedExt for T {
    fn compile_bundle_expect_produced(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, CompileUnsupported> {
        self.compile_bundle(source, artifact, opts, alloc)
            .map(|outcome| {
                outcome
                    .into_produced()
                    .expect("this fixture's carrier produces a runtime surface")
            })
    }
}

/// Why a carrier FAIL-CLOSED on the runtime surface a request asked for.
///
/// The reason is carried STRUCTURALLY — a stable code plus its message and
/// span — so a consumer reads it from typed fields rather than recovering it by
/// scanning diagnostic text for a framework-specific prefix.
#[derive(Debug, Clone)]
pub struct RuntimeSurfaceRefusal {
    /// The framework-defined stable code for the refused surface.
    pub diagnostic_code: String,
    /// Human-readable reason.
    pub message: String,
    /// Carrier-absolute source span of the refusing construct. A refusal
    /// with no construct-specific location (a whole-component oracle
    /// result) carries the whole-source span — decided by the producer,
    /// never fabricated by a downstream call site.
    pub span: verter_span::Span,
    /// Diagnostics accumulated BEFORE the refusal (non-fatal). A refusal
    /// carries no product, so these are the whole of what it reports.
    pub diagnostics: Vec<RuntimeDiagnostic>,
}

/// The carrier's TERMINAL result for one compile request.
///
/// A sum, not a product-plus-flag: a refusal carries NO output of any kind, so
/// "the runtime surface was refused AND a sibling product was published under
/// the same request identity" is not representable. A request whose runtime
/// surface fail-closes therefore yields no `tsx`, no `main`, no styles and no
/// template data — the request's outcome is atomic across the products it asked
/// for.
///
/// A carrier that was NOT asked for a runtime product (`want_runtime == false`)
/// attempts no runtime compile and so can only ever be [`Self::Produced`].
///
/// `clippy::large_enum_variant` is allowed rather than boxing the produced arm.
/// The bundle is ~1.6 KiB, but this value is returned once per compile and
/// destructured immediately by its single caller, and the bundle was ALREADY
/// returned by value before this sum existed — so boxing would add a heap
/// allocation to every compile that the previous shape did not pay, purely to
/// shrink the rare refusal arm's stack footprint.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum CarrierCompileOutcome {
    /// The compile produced the requested product set.
    Produced(RuntimeCompileOutput),
    /// The requested runtime surface was refused. No product accompanies it.
    RuntimeSurfaceRefused(RuntimeSurfaceRefusal),
}

impl CarrierCompileOutcome {
    /// The produced bundle, or `None` when the runtime surface was refused.
    ///
    /// There is deliberately no accessor that yields products from the refusal
    /// arm — the refusal simply has none.
    #[must_use]
    pub fn into_produced(self) -> Option<RuntimeCompileOutput> {
        match self {
            Self::Produced(output) => Some(output),
            Self::RuntimeSurfaceRefused(_) => None,
        }
    }
}

/// The compiler-side carrier framework trait.
///
/// One impl per carrier framework. The host's carrier dispatch reaches
/// these operations through the [`super::registry`] lookup; Vue's impl
/// (`vue_bridge::VueCarrierCompiler`) delegates to the existing Vue
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
    /// Recoverable malformed syntax is NOT a rejection: framework
    /// tokenizers collect diagnostics inline, so ordinary malformed input
    /// still returns `Ok` with the problem recorded on the artifact's
    /// mapped diagnostic channel (`common.diagnostics` / the host's parse
    /// channel). `Err(SyntaxReject)` is reserved for a request this
    /// frontend cannot honor at all — an explicitly unsupported
    /// parse-option combination, or a construction failure that is not
    /// itself recoverable syntax — and is returned BEFORE any artifact is
    /// constructed: nothing downstream publishes for a rejected request.
    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject>;

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

    /// Produce the framework-neutral RUNTIME bundle for the carrier.
    ///
    /// `artifact` MUST match `source` plus the parse-affecting options the
    /// carrier requires (the same precondition the per-framework
    /// pre-parsed compile entry requires today): the host owns the
    /// cached-parse validity decision and hands over either the valid
    /// cached artifact or a fresh carrier parse of the merged source.
    ///
    /// The carrier owns the typed downcast and native compile and returns a
    /// [`CarrierCompileOutcome`]: either a neutral [`RuntimeCompileOutput`]
    /// re-expressing every field the host's runtime assembly + virtual-file
    /// population needs, or a product-free
    /// [`RuntimeSurfaceRefusal`]. A carrier that cannot serve the
    /// requested target AT ALL returns a typed [`CompileUnsupported`].
    ///
    /// The carrier produces exactly the products the request asked for: the
    /// runtime module and its side-files under `want_runtime`, the IDE artifact
    /// under `want_ide`. Template facts under `want_template_data` are filled
    /// from the catalog semantic authority — `compile_bundle` does not
    /// independently extract them. It attempts the runtime compile only under
    /// `want_runtime`, so a request that did not ask for a runtime product can
    /// never be refused one — and a request that IS refused publishes nothing
    /// at all.
    ///
    /// The adapter's codegen owns its own `CodeTransform` (the single
    /// source of truth for generated-code edits); the returned `code` /
    /// `source_map` pairs are produced by that transform verbatim.
    fn compile_bundle(
        &self,
        source: &str,
        artifact: &FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<CarrierCompileOutcome, CompileUnsupported>;
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
    use verter_language::{CarrierParse, FrameworkAdapterId, LanguageId};
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

        fn inventory(source: &str) -> verter_language::CarrierBlockInventory {
            use verter_language::parse_artifact::carrier_inventory::{
                BlockId, CarrierBlock, CarrierBlockInventory, InternedNameId, MarkupSyntaxArena,
                NormalizedNameTable, ScriptRole, ScriptSourceType as InventoryScriptSourceType,
                SectionRole, SourceSlice, SourceSpaceDescriptor, SourceSpaceId, SourceSpan,
                SyntaxTermination, TaggedSyntax,
            };
            use verter_language::registered_source_authority::{
                CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
            };

            let authority = RegisteredSourceAuthority::new().expect("fixture source authority");
            let registered = authority
                .register_source(
                    CanonicalFileId::new("fixture://carrier"),
                    FileIncarnation::new(1),
                    SourceGeneration::new(1),
                    verter_language::FileLanguage::vue(),
                    Arc::from(source),
                )
                .expect("fixture source registration");
            let source_space = SourceSpaceId(0);
            let blocks: Arc<[CarrierBlock]> = Self::script_span(source)
                .map(|span| {
                    let content = SourceSpan::new(source_space, span.start, span.end);
                    let empty = SourceSpan::new(source_space, 0, 0);
                    CarrierBlock::Section {
                        id: BlockId(0),
                        role: SectionRole::Script {
                            role: ScriptRole::Module,
                            dialect: InventoryScriptSourceType::TypeScript,
                        },
                        syntax: TaggedSyntax {
                            authored_name: SourceSlice::new(empty),
                            normalized_name: InternedNameId(0),
                            opening_span: SourceSpan::new(source_space, 0, span.start),
                            opening_name_span: empty,
                            attribute_insertion_anchor: empty,
                            content_span: content,
                            closing_span: Some(SourceSpan::new(
                                source_space,
                                span.end,
                                source.len() as u32,
                            )),
                            closing_name_span: Some(empty),
                            full_span: SourceSpan::new(source_space, 0, source.len() as u32),
                            termination: SyntaxTermination::Closed,
                            attributes: Arc::default(),
                        },
                    }
                })
                .into_iter()
                .collect::<Vec<_>>()
                .into();
            CarrierBlockInventory::new_registered(
                Arc::from([SourceSpaceDescriptor::registered(source_space, &registered)]),
                Arc::new(NormalizedNameTable {
                    values: Arc::from([Arc::<str>::from("script")]),
                }),
                blocks,
                Arc::new(MarkupSyntaxArena::default()),
                &[&registered],
            )
            .expect("fixture inventory")
        }

        fn registered(&self, source: &str) -> Arc<FrameworkParseArtifact> {
            let parsed = self
                .parse(source, &ParseOptions::default())
                .expect("fixture compiler parse");
            crate::framework_common::registered_carrier_projection::registered_artifact_for_tests(
                &parsed,
                Arc::new(Self::inventory(source)),
                Arc::new(FixtureCarrier),
            )
        }
    }

    impl CarrierCompiler for FixtureCompiler {
        fn adapter_id(&self) -> FrameworkAdapterId {
            FrameworkAdapterId::new(Self::ADAPTER)
        }

        fn carrier_language_id(&self) -> LanguageId {
            LanguageId::new(Self::ADAPTER)
        }

        fn parse(
            &self,
            source: &str,
            _opts: &ParseOptions,
        ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, verter_language::SyntaxReject>
        {
            let language = verter_language::FileLanguage::vue();
            let syntax_profile = verter_language::syntax_profile_id_for(
                &language,
                &verter_language::ParseOptions::default(),
            )
            .expect("Vue syntax profile");
            let parse_key = verter_language::parse_key_for(
                source,
                &language,
                verter_language::VUE_SYNTAX_COMPATIBILITY_DOMAIN,
                verter_language::VUE_SYNTAX_COMPATIBILITY_EPOCH,
                &syntax_profile,
            )
            .expect("Vue parse key");
            Ok(Arc::new(UnregisteredFrameworkParseArtifact::new(
                self.adapter_id(),
                LanguageId::new(Self::ADAPTER),
                Arc::new(parse_key),
                Arc::new(syntax_profile),
                Vec::new(),
                Arc::new(FixtureCarrier),
            )))
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

        fn compile_bundle(
            &self,
            _source: &str,
            _artifact: &FrameworkParseArtifact,
            _opts: &RuntimeCompileOptions,
            _alloc: &oxc_allocator::Allocator,
        ) -> Result<CarrierCompileOutcome, CompileUnsupported> {
            // The fixture produces no runtime module — the typed unsupported
            // answer (invariant 4), never a silent empty bundle.
            Err(CompileUnsupported::NoIdeProjection {
                adapter_id: self.adapter_id(),
            })
        }
    }

    #[test]
    fn adapter_id_is_the_registration_key() {
        let compiler = FixtureCompiler;
        assert_eq!(compiler.adapter_id(), FrameworkAdapterId::new("fixture"));
    }

    #[test]
    fn compile_ide_returns_typed_unsupported_never_silent_empty() {
        let compiler = FixtureCompiler;
        let source = "@@x@@";
        let artifact = compiler.registered(source);
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
    fn target_missing_ide_is_constructible() {
        let err = CompileUnsupported::TargetMissingIde;
        assert!(matches!(err, CompileUnsupported::TargetMissingIde));
    }
}
