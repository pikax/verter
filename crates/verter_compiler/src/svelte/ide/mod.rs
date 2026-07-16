//! The Svelte IDE TypeScript/JavaScript projection.
//!
//! Projects a [`ParsedSvelte`] into one valid `.svelte.tsx` or `.svelte.jsx` file that
//! type-checks CLEAN through the TSGO/tsserver path (the LSP parity
//! contract). Every edit goes through one [`CodeTransform`] (the single source of
//! truth for generated-code edits — no post-hoc string munging), so the
//! emitted source map stays token-precise: script expressions and template
//! expressions keep their original source spans (hover / go-to-definition land
//! on the right token), and the ambient prelude is pure insertion that shifts
//! no mapped position.
//!
//! The projection shape:
//!
//! ```text
//! <prelude>                    // unmapped: pragma + runes + 3 checkers
//! <module script body>         // mapped, hoisted to top-level
//! <instance script body>       // mapped, hoisted to top-level
//! ;function __verter_render() {
//!   <snippet declarators>      // hoisted to the top of the scope (source order)
//!   return (<> ...template... </>);
//! }
//! ```
//!
//! The matrix transforms (`{#if}` → ternary, `{#each}` → `.map()`, events
//! verbatim-lowercase, `{#snippet}` → branded `__verter_snippet`, …) are a
//! pure SYNTACTIC transform via `CodeTransform` — NO type lowering runs here
//! (the thin-adapters guard). A row's SUPPORTED / OUT-OF-SCOPE disposition is
//! the matrix's; an OUT-OF-SCOPE construct projects to a void-checked
//! expression with a typed-unsupported diagnostic (never a crash, never a
//! silent drop).

mod await_scan;
mod emit;
pub mod prelude;
mod projector;
mod store_scan;

use super::parser::ParsedSvelte;

/// The script dialect of a generated Svelte component IDE carrier.
///
/// Svelte's component grammar treats only an exact `lang="ts"` script as
/// TypeScript. A component with no TypeScript script is projected as real
/// JavaScript + JSX, with generated types expressed through JSDoc. When either
/// module or instance script is TypeScript the combined carrier must remain
/// TSX because both script bodies share the one generated module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvelteIdeDialect {
    TypeScript,
    JavaScript,
}

impl SvelteIdeDialect {
    #[must_use]
    pub(crate) fn for_component(parsed: &ParsedSvelte) -> Self {
        let has_typescript = [
            parsed.module_script.as_ref(),
            parsed.instance_script.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|script| script.lang.as_deref() == Some("ts"));
        if has_typescript {
            Self::TypeScript
        } else {
            Self::JavaScript
        }
    }

    #[must_use]
    pub(crate) const fn is_javascript(self) -> bool {
        matches!(self, Self::JavaScript)
    }
}

#[cfg(test)]
mod projector_tests;
#[cfg(test)]
mod store_scan_tests;

pub use emit::DiagnosticSeverity;
pub use projector::{project_svelte_ide, SvelteIdeProjection, SvelteIdeUnsupportedDiagnostic};
