# Tests

Guide recipe for test-product and runner integration in Verter web
products. Status: **pending** — the executable example slot for this
recipe (`guides/tests`) is produced by the testing product node; see
[`../manifest.json`](../manifest.json).

The finished tests recipe must cover:

- how to run a project's test suite through the Verter test-product
  integration, with the exact entry (shipped bin or public import) the
  recipe pins
- how test identity, source identity and result envelopes are preserved
  end to end, including failing and skipped results
- how to distinguish a real runner integration from a declared-but-
  unsupported one, reporting each truthfully

Existing shipped truth this recipe links to, and must not duplicate: the
[linting guide](../../../docs/guide/linting.md) and the
[project-check CLI](../../../docs/api/native.md).

Required example slot: an executable test-runner walkthrough
(`guides/tests`) — an example whose files include an executable source
file, whose entry is a shipped bin or public import, and which cites the
capability surface it exercises. A static sample or an example without a
capability link does not satisfy this recipe; the recipes gate rejects
it.
