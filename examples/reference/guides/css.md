# CSS

Guide recipe for CSS semantics and style analysis in Verter web products.
Status: **pending** — the executable example slot for this recipe
(`guides/css`) is produced by the CSS intelligence node; see
[`../manifest.json`](../manifest.json).

The finished css recipe must cover:

- how to run CSS semantic analysis over a project's styles and template
  styles, with the exact entry (shipped bin or public import) the recipe
  pins
- how to read a CSS finding: selector, cascade and source mapping back to
  the originating style or template location
- how scoped, template-compiled and external style analysis differ, and
  which surfaces cover each

Existing shipped truth this recipe links to, and must not duplicate: the
[feature overview](../../../docs/guide/features.md) and the
[linting guide](../../../docs/guide/linting.md).

Required example slot: an executable CSS analysis walkthrough
(`guides/css`) — an example whose files include an executable source
file, whose entry is a shipped bin or public import, and which cites the
capability surface it exercises. A static sample or an example without a
capability link does not satisfy this recipe; the recipes gate rejects
it.
