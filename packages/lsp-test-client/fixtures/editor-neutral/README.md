# Editor-neutral LSP contract fixture

This configured project is the shared behavioral fixture for raw LSP and real-editor
drivers. It intentionally has no `jsxImportSource`: Verter must provide the correct
Vue and Svelte JSX environments itself. The source matrix covers Vue and Svelte in
TypeScript and JavaScript/JSDoc modes, direct carrier imports, and a two-hop barrel.

The fixture is immutable during a run. Rename is asserted from the returned
`WorkspaceEdit`: every edit must carry the requested name and select an exact
authored token range, but tests do not apply edits to the checked-in files.
Definitions must resolve to an exact authored declaration range (or the explicit
file-start target used for an imported SFC), and local script↔markup definitions
are requested twice to cover both the first and repeated provider path.
