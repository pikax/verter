# SDK and official-extension guide home

This directory holds the guide structure for building with the Verter SDK
(public `@verter/*` packages and shipped `verter-*` bins) and for the
official extensions (VS Code extension, `verter-lsp`, `verter-mcp`,
`verter-tsc`). It is part of the published example home validated by the
[docs reference harness](../../../docs/scripts/reference-harness.mjs) via
[`../manifest.json`](../manifest.json).

Each topic below is a structure contract: it fixes what the finished guide
must cover, which existing shipped page owns related truth, and the
executable example slot the topic requires. Until the producing nodes land,
every slot is recorded as **pending** in the manifest — no topic claims a
working example it does not have.

- [Contribution](./contribution.md) — authoring and reviewing SDK and extension changes
- [Isolation](./isolation.md) — process, workspace and analysis isolation boundaries
- [Permissions](./permissions.md) — what builds, extensions and agents may read, write and run
- [Packaging](./packaging.md) — package, bin and extension packaging rules
- [Debugging](./debugging.md) — diagnostics and debugging workflows
- [Compatibility](./compatibility.md) — supported versions, stability classes and migration

The SDK documentation gate: once the producing nodes land executable
examples, the docs build runs `node docs/scripts/reference-harness.mjs
--sdk-gate`. A topic that is still pending, or one whose sample is a static
manifest without an executable extension, fails that gate.
