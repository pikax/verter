// The dev-mode entry for the Verter-owned Svelte MathML JSX namespace.
//
// Under `jsx: "react-jsxdev"` (or a dev build) TypeScript consults
// `<jsxImportSource>/jsx-dev-runtime`; this re-exports the single MathML `JSX`
// namespace authority from `./jsx-runtime` so both runtime modes resolve the
// identical Verter-owned mathml namespace. Types-only — no runtime
// factory.

export { JSX } from "./jsx-runtime";
