//! Framework-neutral carrier-compile I/O.
//!
//! Parse, IDE codegen, runtime bundle. Eval-source
//! and template facts belong to
//! [`super::capability::FrameworkSemanticAuthority`]. Script facts go
//! through the host `ScriptFactProvider` seam — not this surface.
//!
//! Each adapter's IDE codegen owns its own
//! [`crate::code_transform::CodeTransform`]. No entry here threads
//! a borrowed transform (that would be a second, coarse map).

use std::sync::Arc;

use sha2::{Digest, Sha256};
use verter_language::FrameworkAdapterId;

use crate::compile::types::{CompileDiagnosticSeverity, DestructuredBlockMeta};

/// IDE-codegen options threaded into the carrier IDE-compile entry.
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

/// The rendered IDE (TSX/JSX) artifact a carrier IDE compile
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
    /// A product leg was demanded without (or with the wrong) consume-once
    /// execution grant for it. Product-backend legs execute only under a
    /// grant carved off the demand's admission (or minted crate-privately
    /// by the registry bundle route); a demanded-but-ungranted leg fails
    /// closed instead of executing without admission evidence.
    ProductExecutionUngranted {
        /// The demanded product whose execution grant was absent or wrong.
        product: crate::compile_request::ProductKind,
    },
    /// Vue main-module assembly refused after the runtime blocks were
    /// produced. No partial main body is published.
    VueMainAssemblyFailed(String),
}

/// The registry-dispatched routes' grant mint: the
/// `compile_bundle` / `compile_ide` entries mint their leg grants
/// directly from the neutral option demand
/// at the route boundary, because those routes carry no host-issued
/// admission. Crate-private: an external caller reaches a product backend
/// only through an admission carve, so grant minting authority never
/// leaves the crate.
pub(crate) fn registry_route_execution_grants(
    opts: &RuntimeCompileOptions,
) -> super::capability::ProductExecutionGrants {
    use super::capability::{ProductExecutionGrant, ProductExecutionGrants};
    use crate::compile_request::ProductKind;
    ProductExecutionGrants {
        runtime: opts.want_runtime.then(|| {
            ProductExecutionGrant::mint(if opts.ssr {
                ProductKind::RuntimeServer
            } else {
                ProductKind::RuntimeClient
            })
        }),
        projection: opts
            .want_ide
            .then(|| ProductExecutionGrant::mint(ProductKind::IdeCompanion)),
    }
}

/// Runtime-codegen options threaded into the carrier runtime-bundle compile.
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
    /// Per-leg override for the IDE companion's own source-map demand.
    /// `None` couples the IDE leg to [`source_map`](Self::source_map) (the
    /// compatibility route's historical behavior); `Some(x)` is read ONLY
    /// by the IDE projection leg, so a request-derived caller can honor
    /// `RuntimeProductRequest.runtime_source_map` and
    /// `IdeProductRequest.want_source_map` independently.
    pub ide_source_map: Option<bool>,
    /// Server-side rendering mode (the carrier emits its SSR backend).
    pub ssr: bool,
    /// Style-pipeline ownership for this runtime product. Vue's authored-only mode
    /// defers plain-CSS-only modules/scoping to a bundler style lane.
    pub style_processing: crate::compile_request::RuntimeStyleProcessing,
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
    /// Host-owned identifiers and request axes for Vue main-module assembly.
    /// Topology is compiler-owned; empty identifiers skip host virtual imports.
    pub vue_main: crate::assembly::VueMainDecoration,
    /// Assemble and attach the Vue `_sfc_main` body. Independent of
    /// [`want_runtime`]: STYLE-only runtime compilation does not publish Main.
    pub want_main: bool,
    /// Authored `<script>` inventory for map-required checks.
    pub vue_has_script: bool,
    /// Authored `<template>` inventory for map-required checks.
    pub vue_has_template: bool,
    /// Authored script lang (`"js"`/`"ts"`/`"jsx"`/`"tsx"`). `None` ⇒ JavaScript.
    pub vue_script_lang: Option<String>,
    /// Host canonical identity for `__file` and SSR registration fallback.
    /// Distinct from [`Self::filename`].
    pub vue_canonical_id: Option<String>,
    /// Test-only: drop the generated script map before Main assembly so the
    /// maps-on demand refuses as [`CompileUnsupported::VueMainAssemblyFailed`].
    #[cfg(any(test, feature = "test-support"))]
    pub drop_required_script_map: bool,
    /// Admitted external preprocessing results, one slot per style block in
    /// inventory order. A present slot is the ONLY bytes that block's
    /// framework rewrite may consume: the authored block is never read in
    /// its place. Excluded from request/cache identity.
    pub style_continuations: Vec<Option<BoundStyleContinuation>>,
}

/// One admitted external style continuation together with the host-minted
/// identity of the produced bytes it carries.
///
/// The continuation owns stage, basis, provenance and map validity. The token
/// pair is the block-content identity the HOST minted for exactly those
/// produced bytes, and it exists because an output declared over a continued
/// block must declare that identity rather than mint a space of its own: the
/// host already holds the produced-to-authored map under these tokens, so
/// declaring them is what lets the two map chains join. A compiler-minted
/// carrier space over the same bytes would name a space nothing else knows.
#[derive(Debug, Clone)]
pub struct BoundStyleContinuation {
    /// The admitted continuation — the sole authority for the produced bytes,
    /// their stage and their basis.
    pub continuation: Arc<crate::style_planner::ExternalStyleContinuation>,
    /// Host-minted source space containing the produced bytes.
    pub source_space_token: String,
    /// Host-minted identity of the produced byte artifact (code plus any map).
    pub content_artifact_token: String,
    /// The host-admitted produced-to-authored map. An output rendered from
    /// the produced bytes publishes its map chained through this one; without
    /// it that output has no honest map to publish.
    pub source_map: Option<Arc<str>>,
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
            ide_source_map: None,
            ssr: false,
            style_processing: crate::compile_request::RuntimeStyleProcessing::Complete,
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
            vue_main: crate::assembly::VueMainDecoration::default(),
            // Unrequested Main is zero-work. Host publication
            // sets this when `publishes_runtime_module()` is true.
            want_main: false,
            vue_has_script: true,
            vue_has_template: true,
            vue_script_lang: None,
            vue_canonical_id: None,
            #[cfg(any(test, feature = "test-support"))]
            drop_required_script_map: false,
            style_continuations: Vec::new(),
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
    /// The external tool the host names for these bytes, when they are an
    /// external producer's output rather than the carrier's own content.
    ///
    /// `None` means the bytes are the carrier's own — inline or an external
    /// `src` file — and the compiler records them at the stage they were
    /// authored in. `Some` carries the identity the admitting host holds, so
    /// the compiler records the tool that actually made them instead of
    /// claiming it made them itself. Only the host can answer this: the bytes
    /// look identical either way.
    pub producer: Option<verter_css_syntax::PreprocessorIdentity>,
    /// Content identity of the AUTHORED bytes this selection was validated
    /// against, observed by the host under the same fence that selected
    /// `code`.
    ///
    /// For an external producer's output this is the input the tool
    /// consumed, which `code` is NOT: the two are different byte spaces and
    /// only the host can state the relationship between them. Recomputing it
    /// from a later read of the carrier would bind a revision the host never
    /// validated, so a consumer that needs a basis reads it here or refuses.
    /// `None` when the host could not state one.
    pub authored_basis: Option<crate::assembly::ContentId>,
    /// Diagnostics the external producer reported for these bytes, in
    /// production order. Empty for the carrier's own content.
    pub diagnostics: Vec<verter_css_syntax::StyleDiagnostic>,
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

/// A compiled `<style>` output qualified by the stage identity of its bytes.
///
/// The stylesheet travels as a [`verter_css_syntax::QualifiedStyleResult`],
/// so its stage, dialect, producer and diagnostics are facts of the value
/// rather than assumptions a consumer makes about a bare string.
/// [`Self::consumed_stage`] names the byte space the framework rewrite read:
/// the carrier's own authored block, or an admitted external continuation's
/// produced bytes — the space [`Self::source_map`] addresses and
/// [`Self::output_descriptor`] declares.
#[derive(Debug, Clone)]
pub struct QualifiedRuntimeStyle {
    /// The published stage-qualified stylesheet — authored, preprocessed or
    /// framework-rewritten, as the result's own stage says.
    pub result: verter_css_syntax::QualifiedStyleResult,
    /// The stage of the bytes the framework rewrite consumed.
    pub consumed_stage: verter_css_syntax::StyleStage,
    /// Source map from [`Self::result`]'s bytes into the consumed space —
    /// `Some` ONLY when the compile demanded maps.
    pub source_map: Option<String>,
    /// The scope hash the stylesheet's selectors are scoped under.
    pub scope_hash: Option<String>,
    /// Whether the stylesheet includes global css.
    pub has_global: bool,
    /// Qualified identity and source-map chain for this emitted unit.
    pub output_descriptor: RuntimeOutputDescriptor,
}

impl QualifiedRuntimeStyle {
    /// The `lang` spelling of the published bytes' dialect, read off the
    /// result's own dialect through the one spelling authority.
    #[must_use]
    pub fn lang(&self) -> &'static str {
        let dialect = self.result.dialect();
        verter_css_syntax::CssDialect::LANG_SPELLINGS
            .iter()
            .find(|(_, spelled)| *spelled == dialect)
            .map_or("css", |(spelling, _)| spelling)
    }
}

/// A framework-neutral custom block (`<i18n>`, `<docs>`, …).
#[derive(Debug, Clone)]
pub struct RuntimeCustomBlock {
    /// The block tag (e.g. `"i18n"`).
    pub block_type: String,
    /// Raw block content.
    pub content: String,
}

/// The neutral runtime bundle a carrier runtime compile produces.
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
    /// The request's ONE staged compile handoff for the framework-owned
    /// runtime module: the complete artifact set plus the typed identity,
    /// dialect and map of the module artifact inside it. `None` ⇒ there is
    /// no runtime surface to publish. Vue and Svelte both stage their
    /// assembled module here; a consumer reads the published bytes,
    /// language and map off the staged root artifact and never
    /// reconstructs framework topology from block fields.
    pub main: Option<crate::assembly::StagedCompileArtifacts>,
    /// The compiled `<script>` block, when present.
    pub script: Option<RuntimeScriptBlock>,
    /// The compiled template / render-function block, when present.
    pub template: Option<RuntimeTemplateBlock>,
    /// Stage-qualified compiled `<style>` outputs in source order. Every
    /// carrier publishes every style here: a `<style>` output leaves the
    /// carrier boundary carrying the stage, dialect and producer of its
    /// bytes, so there is no shape in which an unqualified stylesheet
    /// travels beside a qualified one.
    pub qualified_styles: Vec<QualifiedRuntimeStyle>,
    /// Custom blocks in source order.
    pub custom_blocks: Vec<RuntimeCustomBlock>,
    /// Source-backed custom-block descriptors beside [`Self::custom_blocks`]
    /// (the legacy adapter), admitted to their own compile-artifact set: one
    /// `"sfc"` source unit bound to the registered carrier (registered file
    /// lineage, carrier-bytes revision), one `"sfc"` analysis artifact, and
    /// one attached descriptor per block in source order. Minted once by the
    /// Vue bridge from the admitted artifact, whether or not Main is
    /// demanded. `None` for a carrier without custom blocks, for Svelte (no
    /// custom-block producer cell), and on routes that never hold the
    /// registered artifact. Not yet consumed downstream.
    pub custom_block_artifacts: Option<crate::assembly::CompileArtifactSet>,
    /// Retained parse facts in transit from the runtime leg to the Vue
    /// bridge, which takes them to mint [`Self::custom_block_artifacts`].
    /// Opaque outside the bridge; `pub` only so cross-crate struct-update
    /// construction (`..Default::default()`) still compiles.
    pub custom_block_facts: super::vue_bridge::StagedCustomBlockFacts,
    /// The scope id (`data-v-xxxxxxxx`), empty when none.
    pub scope_id: String,
    /// The IDE (TSX/JSX) artifact, present when `want_ide` was requested AND
    /// the carrier projects one.
    pub tsx: Option<IdeOutput>,
    /// Framework-neutral template facts, present when `want_template_data`
    /// was requested AND the catalog semantic authority produced them.
    /// Catalog miss / parse-key mismatch / producer failure stay `None`
    /// (typed refusal). A valid template-free carrier is `Some` empty facts.
    ///
    /// The product keeps the extraction's own diagnostics attached to the
    /// data: the bundle producer additionally publishes them on
    /// [`Self::diagnostics`] (its route's channel, deduplicated), while the
    /// attached copy travels with the facts so every downstream conversion
    /// of the data carries the same diagnostic set — a consumer that takes
    /// the data and drops the diagnostics erases the file's template
    /// expression errors on its route.
    pub template_data: Option<super::registered_carrier_projection::TemplateFactsProduct>,
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
        self.main.is_some()
            || self.script.is_some()
            || self.template.is_some()
            || !self.qualified_styles.is_empty()
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
