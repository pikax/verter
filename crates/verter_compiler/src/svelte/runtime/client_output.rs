//! Public output carriers produced by the native Svelte client compiler.

use crate::assembly::DeclaredImport;

/// The emitted client module: the JS source plus the structural facts a caller
/// (a topology gate / the carrier) reads back without re-parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientModule {
    /// The full emitted JS module source.
    pub code: String,
    /// The client JavaScript source-map JSON, produced only on demand from the
    /// same mapped accumulator that emitted [`Self::code`].
    pub source_map: Option<String>,
    /// Every import statement actually written into [`Self::code`]'s module
    /// prelude — the flag/runtime-namespace imports
    /// ([`super::helpers::ImportPlan::declared_imports`]) plus every
    /// `<script module>`/instance USER import
    /// ([`super::client_imports::UserImport::declared_imports`]), reported as
    /// a fact by the producer that wrote them rather than recovered by
    /// reparsing [`Self::code`]. Consumed by
    /// [`crate::standalone::StandaloneCompiler::compile`]'s direct one-shot
    /// core to publish a real `emitted_imports` fact through
    /// [`crate::assembly::publish`]'s atomicity check.
    pub declared_imports: Vec<DeclaredImport>,
    /// The EXTERNAL scoped-css artifact (the official `compiled.css` — the
    /// scoped `css.code` + its scope hash), produced for an external-mode
    /// scoped `<style>`. `Some` whenever an external-mode `<style>` exists —
    /// even when the rendered css is empty (the official `compiled.css` is
    /// NON-null: `{ code: '', hasGlobal: false, map }`). `None` only when the
    /// component has no `<style>` block, or in INJECTED mode (the injected css
    /// is inlined into [`code`](Self::code) as the `$$css` hoist +
    /// `$.append_styles` prelude — the official `inject_styles` routing nulls
    /// the artifact).
    pub css: Option<ScopedCssArtifact>,
}

/// The scoped-css payload of one compiled component — the official
/// `{ code, map, hasGlobal }` external `compiled.css` artifact plus the
/// scope hash. The INJECTED `$$css` object reads only the `{ hash, code }`
/// pair (the official inline shape carries no map and no global fact); the
/// map + `has_global` ride the EXTERNAL artifact out to the carrier's style
/// block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCssArtifact {
    /// The scope hash (`svelte-<djb2>`).
    pub hash: String,
    /// The rendered scoped stylesheet (the official `css.code`).
    pub code: String,
    /// The css source-map JSON (the official `css.map`) — `Some` ONLY when
    /// the compile demanded it (`want_source_map`), generated from the SAME
    /// shared transform that rendered [`code`](Self::code).
    pub source_map: Option<String>,
    /// Whether the component's css includes GLOBAL css (the official
    /// `css.hasGlobal` — `analysis.css.has_global`).
    pub has_global: bool,
}
