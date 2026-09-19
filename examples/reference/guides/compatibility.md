# Compatibility

Guide recipe for compatibility targets and compatibility data in Verter
web products. Status: **pending** — the executable example slot for this
recipe (`guides/compatibility`) is produced by the web compatibility
node; see [`../manifest.json`](../manifest.json).

The finished compatibility recipe must cover:

- how to declare the compatibility targets a project must satisfy and how
  Verter resolves compatibility data for them, citing the exact entry the
  recipe pins
- how to read a compatibility verdict, including negative, partial and
  unsupported outcomes, without overstating support
- how to upgrade or withdraw a compatibility claim when a target or
  package version changes

Existing shipped truth this recipe links to, and must not duplicate: the
[API stability policy](../../../docs/api-stability.md) and the
[project-check CLI](../../../docs/api/native.md).

Required example slot: an executable compatibility walkthrough
(`guides/compatibility`) — an example whose files include an executable
source file, whose entry is a shipped bin or public import, and which
cites the capability surface it exercises. A static sample or an example
without a capability link does not satisfy this recipe; the recipes gate
rejects it.
