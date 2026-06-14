// The dev-mode entry for the Verter-owned Svelte JSX namespace.
//
// Under `jsx: "react-jsxdev"` (or a dev build) TypeScript consults
// `<jsxImportSource>/jsx-dev-runtime`; this re-exports the single `JSX`
// namespace authority from `./jsx-runtime` so both runtime modes resolve the
// identical Svelte-true namespace (D-ae). Types-only — no runtime factory.

export { JSX } from "./jsx-runtime";
