# Isolation

Guide structure for isolation in the Verter SDK and official extensions.
Status: **pending** — the executable example slot for this topic
(`sdk/isolation`) is produced by the SDK authoring tools node; see
[`../manifest.json`](../manifest.json).

The finished isolation guide must cover:

- workspace isolation: what an SDK session may observe (one project root,
  its tsconfig basis, its declared files) and what it must not reach across
- extension isolation: how the language server, the MCP server and the
  compiler share or refuse shared state; the component-meta session as the
  worked example of a scoped, closable session
- why examples in this home never depend on repository internals — the
  harness rejects imports of `packages/*/src` and `file:` specifiers

Existing shipped truth this guide links to, and must not duplicate: the
[component-meta API reference](../../../docs/api/component-meta.md) and the
[LSP features reference](../../../docs/editor/lsp-features.md).

Required example slot: an executable isolation walkthrough
(`sdk/isolation`) that opens and closes a scoped session through a public
export. A static sample manifest does not satisfy this topic.
