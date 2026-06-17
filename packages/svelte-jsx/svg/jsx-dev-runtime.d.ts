// The dev-mode entry for the Verter-owned Svelte SVG JSX namespace.
//
// Under `jsx: "react-jsxdev"` (or a dev build) TypeScript consults
// `<jsxImportSource>/jsx-dev-runtime`; this re-exports the single SVG `JSX`
// namespace authority from `./jsx-runtime` so both runtime modes resolve the
// identical Svelte-true svg namespace. Types-only — no runtime factory.

export { JSX } from "./jsx-runtime";
