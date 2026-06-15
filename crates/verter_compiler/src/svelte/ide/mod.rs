//! The Svelte IDE TSX projection.
//!
//! Projects a [`ParsedSvelte`] into ONE valid `.svelte.tsx` file that
//! type-checks CLEAN through the TSGO/tsserver path (the LSP parity contract,
//! D-u). Every edit goes through one [`CodeTransform`] (the single source of
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
//!   <snippet declarators>      // hoisted to the top of the scope (D-ap order)
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
mod bind_contract;
mod bind_contract_data;
mod emit;
pub mod prelude;
mod projector;
mod store_scan;

#[cfg(test)]
mod projector_tests;
#[cfg(test)]
mod store_scan_tests;

pub use projector::{project_svelte_ide, SvelteIdeProjection, SvelteIdeUnsupportedDiagnostic};
