# Security

Guide recipe for security analysis and threat handling in Verter web
products. Status: **pending** — the executable example slot for this
recipe (`guides/security`) is produced by the web security node; see
[`../manifest.json`](../manifest.json).

The finished security recipe must cover:

- how to run a security analysis pass over a project and read its
  findings, with the exact entry (shipped bin or public import) the
  recipe pins
- how secret and sensitive-data handling is treated in analysis inputs,
  and what may never be recorded in a receipt
- how to report negative, partial and unsupported security outcomes
  truthfully instead of hiding them

Existing shipped truth this recipe links to, and must not duplicate: the
[configuration reference](../../../docs/configuration.md) and the
[MCP server guide](../../../docs/editor/mcp-server.md).

Required example slot: an executable security walkthrough
(`guides/security`) — an example whose files include an executable
source file, whose entry is a shipped bin or public import, and which
cites the capability surface it exercises. A static sample or an example
without a capability link does not satisfy this recipe; the recipes gate
rejects it.
