//! The embedded `@verter/svelte-jsx` shim assets.
//!
//! The Svelte IDE TSX projection opens each `.svelte.tsx` with the per-file
//! pragma `/** @jsxImportSource @verter/svelte-jsx */`, which directs
//! TypeScript to resolve the `JSX` namespace from the Verter-owned shim
//! package `@verter/svelte-jsx`. The canonical hand-written content
//! lives in the real workspace package `packages/svelte-jsx/`; the host
//! materializes a version-matched copy on disk and path-maps the inferred
//! project at it (the host data directory for provider-inferred/TSGO
//! consumers — `configure_paths`).
//!
//! These embedded constants are the bytes the host materializes. They are
//! `include_str!`'d from IN-CRATE MIRROR files (`svelte_jsx_assets/`), NOT
//! from the cross-tree package files — a cross-tree `include_str!` would
//! make the byte-pin compare the package against itself (vacuous) and break
//! crates.io packaging (`cargo package` cannot reach outside the crate
//! root). The byte-pin freshness test
//! (`crates/verter_session/tests/svelte_jsx_shim_freshness.rs`) compares each
//! in-crate mirror against its `packages/svelte-jsx/` canonical, so drift
//! between the authority and the embedded copy fails the gate.

/// The embedded `@verter/svelte-jsx/jsx-runtime.d.ts` shim — the `JSX`
/// namespace authority (`Element`/`ElementAttributesProperty`/
/// `IntrinsicElements extends SvelteHTMLElements`).
pub const SVELTE_JSX_RUNTIME_DTS: &str = include_str!("svelte_jsx_assets/jsx-runtime.d.ts");

/// The embedded `@verter/svelte-jsx/jsx-dev-runtime.d.ts` shim — the
/// dev-mode re-export of the `JSX` namespace.
pub const SVELTE_JSX_DEV_RUNTIME_DTS: &str = include_str!("svelte_jsx_assets/jsx-dev-runtime.d.ts");

/// The shim package name (the pragma's import source).
pub const SVELTE_JSX_PACKAGE_NAME: &str = "@verter/svelte-jsx";

/// The `jsx-runtime` subpath specifier the pragma resolves
/// (`@verter/svelte-jsx/jsx-runtime`).
pub const SVELTE_JSX_RUNTIME_SPECIFIER: &str = "@verter/svelte-jsx/jsx-runtime";

/// The `jsx-dev-runtime` subpath specifier
/// (`@verter/svelte-jsx/jsx-dev-runtime`).
pub const SVELTE_JSX_DEV_RUNTIME_SPECIFIER: &str = "@verter/svelte-jsx/jsx-dev-runtime";

// --- F10 svg-namespace entrypoint (`@verter/svelte-jsx/svg`) ---

/// The embedded `@verter/svelte-jsx/svg/jsx-runtime.d.ts` shim — the SVG-only
/// `JSX` namespace (`IntrinsicElements` = the SVG element set via
/// `SVGAttributes`). Selected by a `<svelte:options namespace="svg">`.
pub const SVELTE_JSX_SVG_RUNTIME_DTS: &str = include_str!("svelte_jsx_assets/svg/jsx-runtime.d.ts");

/// The embedded `@verter/svelte-jsx/svg/jsx-dev-runtime.d.ts` shim — the
/// dev-mode re-export of the SVG `JSX` namespace.
pub const SVELTE_JSX_SVG_DEV_RUNTIME_DTS: &str =
    include_str!("svelte_jsx_assets/svg/jsx-dev-runtime.d.ts");

/// The `svg/jsx-runtime` subpath specifier (resolved by the
/// `@jsxImportSource @verter/svelte-jsx/svg` pragma).
pub const SVELTE_JSX_SVG_RUNTIME_SPECIFIER: &str = "@verter/svelte-jsx/svg/jsx-runtime";

/// The `svg/jsx-dev-runtime` subpath specifier.
pub const SVELTE_JSX_SVG_DEV_RUNTIME_SPECIFIER: &str = "@verter/svelte-jsx/svg/jsx-dev-runtime";

// --- F10 mathml-namespace entrypoint (`@verter/svelte-jsx/mathml`) ---

/// The embedded `@verter/svelte-jsx/mathml/jsx-runtime.d.ts` shim — the
/// Verter-owned MathML `JSX` namespace (svelte ships no MathML types).
pub const SVELTE_JSX_MATHML_RUNTIME_DTS: &str =
    include_str!("svelte_jsx_assets/mathml/jsx-runtime.d.ts");

/// The embedded `@verter/svelte-jsx/mathml/jsx-dev-runtime.d.ts` shim — the
/// dev-mode re-export of the MathML `JSX` namespace.
pub const SVELTE_JSX_MATHML_DEV_RUNTIME_DTS: &str =
    include_str!("svelte_jsx_assets/mathml/jsx-dev-runtime.d.ts");

/// The `mathml/jsx-runtime` subpath specifier (resolved by the
/// `@jsxImportSource @verter/svelte-jsx/mathml` pragma).
pub const SVELTE_JSX_MATHML_RUNTIME_SPECIFIER: &str = "@verter/svelte-jsx/mathml/jsx-runtime";

/// The `mathml/jsx-dev-runtime` subpath specifier.
pub const SVELTE_JSX_MATHML_DEV_RUNTIME_SPECIFIER: &str =
    "@verter/svelte-jsx/mathml/jsx-dev-runtime";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_runtime_declares_the_svelte_true_jsx_namespace() {
        // The namespace is Svelte-true (SvelteHTMLElements), brands snippets,
        // and checks component props through `$props` — never Vue's table.
        assert!(SVELTE_JSX_RUNTIME_DTS.contains("export namespace JSX"));
        assert!(SVELTE_JSX_RUNTIME_DTS.contains("IntrinsicElements extends SvelteHTMLElements"));
        assert!(SVELTE_JSX_RUNTIME_DTS.contains("ElementAttributesProperty"));
        assert!(SVELTE_JSX_RUNTIME_DTS.contains("$props"));
        assert!(SVELTE_JSX_RUNTIME_DTS.contains("ReturnType<Snippet>"));
        // It never stubs the `svelte` package itself — it IMPORTS the real one.
        assert!(SVELTE_JSX_RUNTIME_DTS.contains("import type { Snippet } from \"svelte\""));
    }

    #[test]
    fn embedded_dev_runtime_reexports_the_namespace() {
        assert!(SVELTE_JSX_DEV_RUNTIME_DTS.contains("export { JSX } from \"./jsx-runtime\""));
    }

    #[test]
    fn embedded_svg_runtime_declares_an_svg_only_namespace() {
        // F10: the svg entrypoint's `IntrinsicElements` is the SVG element set
        // (svg-only — it REPLACES the HTML table, sourced from `svelte/elements`'
        // `SVGAttributes`), and stays Svelte-true (snippet result + $props).
        assert!(SVELTE_JSX_SVG_RUNTIME_DTS.contains("export namespace JSX"));
        assert!(SVELTE_JSX_SVG_RUNTIME_DTS
            .contains("import type { SVGAttributes } from \"svelte/elements\""));
        assert!(SVELTE_JSX_SVG_RUNTIME_DTS.contains("svg: SVGAttributes<SVGSVGElement>"));
        assert!(SVELTE_JSX_SVG_RUNTIME_DTS.contains("circle: SVGAttributes<SVGCircleElement>"));
        assert!(SVELTE_JSX_SVG_RUNTIME_DTS.contains("ReturnType<Snippet>"));
        // It does NOT extend the HTML table (svg-only, no catch-all).
        assert!(!SVELTE_JSX_SVG_RUNTIME_DTS.contains("extends SvelteHTMLElements"));
        assert!(SVELTE_JSX_SVG_DEV_RUNTIME_DTS.contains("export { JSX } from \"./jsx-runtime\""));
    }

    #[test]
    fn embedded_mathml_runtime_declares_a_closed_mathml_namespace() {
        // F10: the mathml entrypoint is a Verter-owned closed MathML table (no
        // svelte MathML types exist); typed via a hand-written `MathMLAttributes`
        // base using structured primitives, never `any`.
        assert!(SVELTE_JSX_MATHML_RUNTIME_DTS.contains("export namespace JSX"));
        assert!(SVELTE_JSX_MATHML_RUNTIME_DTS.contains("interface MathMLAttributes"));
        assert!(SVELTE_JSX_MATHML_RUNTIME_DTS.contains("math: MathMLAttributes"));
        assert!(SVELTE_JSX_MATHML_RUNTIME_DTS.contains("mrow: MathMLAttributes"));
        assert!(!SVELTE_JSX_MATHML_RUNTIME_DTS.contains("extends SvelteHTMLElements"));
        // No `any` in the mathml attribute typing (structured primitives only).
        assert!(
            !SVELTE_JSX_MATHML_RUNTIME_DTS.contains(": any"),
            "mathml attrs use structured primitives, not `any`"
        );
        assert!(SVELTE_JSX_MATHML_DEV_RUNTIME_DTS.contains("export { JSX } from \"./jsx-runtime\""));
    }
}
