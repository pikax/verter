# Debugging

Guide structure for debugging with the Verter SDK and official extensions.
Status: **pending** — the executable example slot for this topic
(`sdk/debugging`) is produced by the SDK authoring tools node; see
[`../manifest.json`](../manifest.json).

The finished debugging guide must cover:

- running a typecheck through the shipped `verter-tsc` bin and reading its
  public result envelope
- extension-side diagnostics: where the VS Code extension and the language
  server surface errors, and which reference page owns each surface
- reproducing a docs-side example failure with the reference harness
  receipt (completeness state, error codes, source digest) instead of a
  transcript

Existing shipped truth this guide links to, and must not duplicate: the
[LSP features reference](../../../docs/editor/lsp-features.md) and the
[language API reference](../../../docs/api/native.md).

Required example slot: an executable debugging walkthrough
(`sdk/debugging`) that invokes a shipped bin through an executable source
file. A static sample manifest does not satisfy this topic.
