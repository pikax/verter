# Editor-neutral LSP contract fixture

This configured project is the shared behavioral fixture for raw LSP and real-editor
drivers. It intentionally has no `jsxImportSource`: Verter must provide the correct
Vue and Svelte JSX environments itself. The source matrix covers Vue and Svelte in
TypeScript and JavaScript/JSDoc modes, direct carrier imports, and a two-hop barrel.

The fixture is immutable during a run. Rename is asserted from the returned
`WorkspaceEdit`; tests do not apply edits to the checked-in files.
