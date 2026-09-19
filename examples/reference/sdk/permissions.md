# Permissions

Guide structure for permissions in the Verter SDK and official extensions.
Status: **pending** — the executable example slot for this topic
(`sdk/permissions`) is produced by the SDK authoring tools node; see
[`../manifest.json`](../manifest.json).

The finished permissions guide must cover:

- read permissions: which inputs a build, an extension or an agent session
  may read (project files, tsconfig, installed packages) and the boundary
  at which reads stop
- write and execution permissions: which shipped bins may be invoked, that
  examples invoke bins of public packages rather than toolchain commands,
  and that the reference harness itself only reads the repository and
  spawns the existing typeinfo freshness check
- agent-facing permissions: what the MCP server exposes and what it must
  not expose, recorded on the extension's own reference page

Existing shipped truth this guide links to, and must not duplicate: the
[MCP server reference](../../../docs/editor/mcp-server.md) and the
[editor settings reference](../../../docs/editor/settings.md).

Required example slot: an executable permissions walkthrough
(`sdk/permissions`) running through a public plugin or session export with
a shipped-bin entry. A static sample manifest does not satisfy this topic.
