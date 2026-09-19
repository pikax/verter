# Packaging

Guide structure for packaging Verter SDK packages and official extensions.
Status: **pending** — the executable example slot for this topic
(`sdk/packaging`) is produced by the SDK authoring tools node; see
[`../manifest.json`](../manifest.json).

The finished packaging guide must cover:

- what makes a package publishable for this home: a workspace package that
  is not private, with public `exports` and an exact version — the
  reference harness resolves every example import against those published
  maps
- what ships a command: a bin of a public package (`verter-tsc`,
  `verter-lsp`, `verter-mcp`); examples cite commands by bin name
- how the official VS Code extension is packaged and installed, deferring
  to the extension's own guide while the marketplace listing remains
  unpublished

Existing shipped truth this guide links to, and must not duplicate: the
[unplugin API reference](../../../docs/api/unplugin.md) and the
[VS Code extension guide](../../../docs/editor/vscode.md).

Required example slot: an executable packaging walkthrough
(`sdk/packaging`) — an example whose files include an executable source
file and whose entry is a shipped bin. A static sample manifest without an
executable extension does not satisfy this topic; the SDK documentation
gate rejects it.
