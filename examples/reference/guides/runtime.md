# Runtime

Guide recipe for runtime observation and source correlation in Verter web
products. Status: **pending** — the executable example slot for this
recipe (`guides/runtime`) is produced by the runtime inspector node; see
[`../manifest.json`](../manifest.json).

The finished runtime recipe must cover:

- how to observe a running project (reactivity, rendering, execution)
  and correlate observations back to authored sources, with the exact
  entry (shipped bin or public import) the recipe pins
- what a runtime observation receipt records — source revisions, engine
  and host identity, completeness state — and why static proof never
  substitutes for it
- teardown: how sessions and observation channels are closed so no
  runtime outlives the journey that started it

Existing shipped truth this recipe links to, and must not duplicate: the
[native API](../../../docs/api/native.md) and the
[architecture guide](../../../docs/guide/architecture.md).

Required example slot: an executable runtime walkthrough
(`guides/runtime`) — an example whose files include an executable source
file, whose entry is a shipped bin or public import, and which cites the
capability surface it exercises. A static sample or an example without a
capability link does not satisfy this recipe; the recipes gate rejects
it.
