# Accessibility

Guide recipe for accessibility analysis of web products built with Verter.
Status: **pending** — the executable example slot for this recipe
(`guides/accessibility`) is produced by the accessibility intelligence
node; see [`../manifest.json`](../manifest.json).

The finished accessibility recipe must cover:

- how to run an accessibility pass over a project and read its findings,
  with the exact entry (shipped bin or public import) the recipe pins
- what an accessibility finding means semantically (element, rule,
  severity) and how it maps back to source locations
- how to distinguish a verified accessibility capability from an
  unsupported or partially supported case, reporting each truthfully

Existing shipped truth this recipe links to, and must not duplicate: the
[language-service features](../../../docs/editor/lsp-features.md) and the
[feature overview](../../../docs/guide/features.md).

Required example slot: an executable accessibility walkthrough
(`guides/accessibility`) — an example whose files include an executable
source file, whose entry is a shipped bin or public import, and which
cites the capability surface it exercises. A static sample or an example
without a capability link does not satisfy this recipe; the recipes gate
rejects it.
