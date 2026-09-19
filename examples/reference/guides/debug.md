# Debug

Guide recipe for debugging, mapping and diagnostics in Verter web
products. Status: **pending** — the executable example slot for this
recipe (`guides/debug`) is produced by the web debugger node; see
[`../manifest.json`](../manifest.json).

The finished debug recipe must cover:

- how to start a debugging session for a project and attach to its
  surfaces, with the exact entry (shipped bin or public import) the
  recipe pins
- how source mapping correlates a runtime or diagnostic location back to
  the authored template and style sources
- how to collect and read diagnostics without fabricating conformance:
  unsupported debug scenarios are reported as unsupported

Existing shipped truth this recipe links to, and must not duplicate: the
[LSP features](../../../docs/editor/lsp-features.md) and the
[language-service API](../../../docs/api/component-meta.md).

Required example slot: an executable debugging walkthrough
(`guides/debug`) — an example whose files include an executable source
file, whose entry is a shipped bin or public import, and which cites the
capability surface it exercises. A static sample or an example without a
capability link does not satisfy this recipe; the recipes gate rejects
it.
