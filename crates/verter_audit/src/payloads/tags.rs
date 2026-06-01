#![deny(missing_docs)]
//! Stringly-typed mirrors of session / consumer-crate enums that
//! cannot live in the substrate without creating dependency cycles.
//! Producers implement `From<Domain> for Tag` in their own crate.

use serde::{Deserialize, Serialize};

/// Compile target — mirror of `verter_compiler::CompileTarget`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum CompileTargetTag {
    /// VDOM render-function backend.
    Vdom,
    /// Vapor renderer backend.
    Vapor,
    /// IDE backend (valid TSX/JSX for type checking). Default —
    /// matches the LSP / tsgo path that drives most audited compiles.
    #[default]
    Ide,
}

/// Projection mode — mirror of
/// `verter_session::semantic_query::ProjectionMode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum ProjectionModeTag {
    /// Identity — pass-through, no projection. Default for
    /// data-only construction sites.
    #[default]
    Identity,
    /// Navigate — preserve carriers, no expansion.
    Navigate,
    /// Shallow — expose one level of surface members.
    Shallow,
    /// Expanded — recursively materialize.
    Expanded,
    /// Skeleton — open-generic body access for cycle detection.
    Skeleton,
}

/// Augmentation target kind — mirror of
/// `verter_session::file_artifact_store::AugmentationTargetKind`.
///
/// Discriminates the four shapes a `declare module "X" { ... }`
/// augmentation can target: an external specifier, a relative path
/// resolved against the augmenter, a wildcard ambient pattern, or
/// the global block. The concrete target value (specifier text,
/// resolved canonical, wildcard pattern) lives in the parallel
/// optional fields of the audit-event variants that carry this tag,
/// keeping the tag itself `Copy + Hash + Eq`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum AugmentationTargetKindTag {
    /// `declare module "vue" {}` — bare specifier resolved through
    /// the project's module resolver under the resolve env. Default
    /// because it is the most common archetype on real Vue / React
    /// codebases.
    #[default]
    ExternalSpecifier,
    /// `declare module "./local" {}` — relative path resolved
    /// against the augmenter's own canonical.
    ResolvedRelativeCanonical,
    /// `declare module "*.css" {}` — wildcard ambient module
    /// pattern.
    WildcardAmbient,
    /// `declare global { ... }` — augments the global scope.
    GlobalAugmentation,
}

/// Reason a `ValidatedFactCache` candidate admission was refused
/// by the fact-completeness guard.
///
/// Carried by
/// [`super::super::structured_event::StructuredAuditEvent::FactSignatureAdmissionRefused`].
/// `Copy` + `Hash` + `Eq` because audit consumers may aggregate
/// refusal counts per reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum AdmissionRefusalReason {
    /// The producer observed zero facts during cold compute, but the
    /// cache requires at least one observed fact to admit. Default
    /// because it is the canonical failure mode: an empty
    /// signature on a source-dependent cache indicates a missing
    /// `observe(...)` call upstream of the publish site.
    #[default]
    EmptySignature,
    /// The cache is not on the documented allowlist of source-
    /// independent kinds. Reserved for future use when the allowlist
    /// becomes a runtime gate.
    NonCacheableKind,
}

/// Bundler kind — mirror of the unplugin's bundler discriminator.
/// Not `Copy` because the `Other` variant carries an owned name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum BundlerKindTag {
    /// Vite — default bundler for the unplugin path.
    #[default]
    Vite,
    /// Webpack.
    Webpack,
    /// Rollup.
    Rollup,
    /// Esbuild.
    Esbuild,
    /// Rolldown.
    Rolldown,
    /// Other bundler — name preserved verbatim.
    Other(String),
}

/// LSP method — mirror of the LSP request method names emitted by
/// `verter_lsp::handlers`. Not `Copy` because the `Other` variant
/// carries an owned method name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum LspMethodTag {
    /// `textDocument/hover`. Default — most audited LSP paths
    /// originate from hover.
    #[default]
    Hover,
    /// `textDocument/definition`.
    GotoDefinition,
    /// `textDocument/completion`.
    Completion,
    /// `textDocument/references`.
    References,
    /// `textDocument/publishDiagnostics`.
    Diagnostics,
    /// `textDocument/documentSymbol`.
    DocumentSymbols,
    /// `textDocument/semanticTokens`.
    SemanticTokens,
    /// `textDocument/inlayHint`.
    InlayHints,
    /// `textDocument/codeAction`.
    CodeAction,
    /// `textDocument/rename`.
    Rename,
    /// Open-ended escape hatch for methods not enumerated above.
    Other(String),
}

/// Discriminator naming the action a [`FileArtifactStore`] entry
/// transitions through. Carried by
/// [`super::super::structured_event::StructuredAuditEvent::FileArtifactCache`].
///
/// `Copy` + `Hash` + `Eq` so consumers may bucket events by action.
///
/// [`FileArtifactStore`]: ../../../../verter_session/src/file_artifact_store.rs.html
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum FileArtifactCacheAction {
    /// A fresh `FileArtifacts` payload was admitted to the store —
    /// either a brand-new canonical or a new content-hash variant
    /// of an existing canonical. Default because admit dominates
    /// over evict on the steady-state baseline.
    #[default]
    Admit,
    /// An existing payload was evicted (LRU sweep, project
    /// generation bump, or explicit `remove_canonical`).
    Evict,
}

/// Discriminator naming the shape of a parse-domain
/// [`FactKey`] published into the registry. Carried by
/// [`super::super::structured_event::StructuredAuditEvent::FactRegistryWrite`].
///
/// `Copy` + `Hash` + `Eq` so consumers may bucket emission counts
/// per fact-key kind without owning string data.
///
/// Mirror of the structural-kind enumeration in
/// `verter_semantic::facts::registry::FactKey`. Only the parse-domain
/// kinds are mirrored — resolve-imports and route-surface domain
/// facts use the parallel `ResolvedImportFacts` / `RouteDb`
/// admission paths and emit their own typed events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum FactKeyKindTag {
    /// `FactKey::Export` — exported binding. Default because exports
    /// dominate the registry on the steady-state baseline.
    #[default]
    Export,
    /// `FactKey::ExportAlias` — `export { Foo as Bar }`.
    ExportAlias,
    /// `FactKey::SyntacticExportSet` — whole-file export set
    /// fingerprint.
    SyntacticExportSet,
    /// `FactKey::LocalDecl` — locally declared, non-exported
    /// binding.
    LocalDecl,
    /// `FactKey::Member` — lazy member body fingerprint.
    Member,
    /// `FactKey::MemberPresence` — eager member header
    /// fingerprint.
    MemberPresence,
    /// `FactKey::MemberShape` — ordered member shape.
    MemberShape,
    /// `FactKey::MacroSurface` — Vue macro invocation surface.
    MacroSurface,
    /// `FactKey::TemplateRoot` — Vue template root list shape.
    TemplateRoot,
    /// `FactKey::ImportRef` — one syntactic import.
    ImportRef,
    /// `FactKey::SyntacticReexportRef` — one syntactic re-export
    /// specifier.
    SyntacticReexportRef,
    /// `FactKey::ModuleAugmentation` — one `declare module "X" {}`
    /// augmenting declaration.
    ModuleAugmentation,
}

/// Caller-requested compile cache mode — mirror of
/// `verter_session::types::CompileCacheMode`.
///
/// Carried by
/// [`super::super::structured_event::StructuredAuditEvent::CompileModeDowngrade`]
/// at the `requested` / `actual` fields. `Copy + Hash + Eq` because
/// audit consumers may aggregate downgrade counts per (requested,
/// actual) pair.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum CompileCacheModeTag {
    /// Bypass host caches entirely. The compile runs fresh and no
    /// entry is published.
    Stateless,
    /// Consult the pure content-addressed cache only.
    Content,
    /// Consult the fact-validated session cache. Default — the
    /// most cache-rich mode.
    #[default]
    Session,
}

/// Why the runtime downgraded a requested `CompileCacheMode` — mirror
/// of `verter_session::types::DowngradeReason`. Carried by
/// [`super::super::structured_event::StructuredAuditEvent::CompileModeDowngrade`]
/// as a vector of every triggering reason in priority order.
///
/// `Copy + Hash + Eq` because audit consumers may aggregate downgrade
/// counts per reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum DowngradeReasonTag {
    /// Default — placeholder default; consumers compare on the
    /// actual triggering reason rather than relying on this.
    #[default]
    HasExternalSrc,
    /// The compile input has macro type dependencies.
    HasMacroTypeDeps,
    /// One of the compile input's script imports resolves through a
    /// workspace alias.
    HasWorkspaceAlias,
    /// The compile input depends on a file that participates in
    /// module augmentation.
    HasModuleAugmentation,
    /// The compile input carries a block override (preprocessed
    /// script / template).
    HasBlockOverride,
    /// The compile input carries a style override (preprocessed
    /// CSS).
    HasStyleOverride,
    /// The compile profile target is IDE-only analysis
    /// (`CompileTarget::TSX` without runtime codegen).
    HasIdeOnlyAnalysis,
    /// The host is in dev mode with `DevServeLastKnownGood` error
    /// policy.
    HasDevLastGood,
}

/// Which lane (`Semantic` or `Display`) a fact carries. Audit-side
/// mirror of `verter_semantic::facts::registry::FactLane`.
///
/// `Copy` + `Hash` + `Eq` for emission aggregation. Producers
/// translate the session-side enum to this tag at emission time so
/// the substrate stays leaf.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum FactLaneTag {
    /// Semantic lane — type-checker-relevant content. Default.
    #[default]
    Semantic,
    /// Display lane — cosmetic / human-readable rendering only.
    Display,
}
