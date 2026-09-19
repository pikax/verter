# Performance

Guide recipe for application-performance measurement of Verter web
products. Status: **pending** — the executable example slot for this
recipe (`guides/performance`) is produced by the web performance node;
see [`../manifest.json`](../manifest.json).

The finished performance recipe must cover:

- how to measure application performance under the ratified performance
  methodology, with the exact entry (shipped bin or public import) the
  recipe pins
- how to bind a latency, work, allocation or RSS budget before measuring,
  and how to report measured numbers with their receipt basis
- why estimates are labelled estimates and never promoted to measured
  speedup claims

Existing shipped truth this recipe links to, and must not duplicate: the
[performance guide](../../../docs/guide/performance.md) and the
[feature overview](../../../docs/guide/features.md).

Required example slot: an executable performance walkthrough
(`guides/performance`) — an example whose files include an executable
source file, whose entry is a shipped bin or public import, and which
cites the capability surface it exercises. A static sample or an example
without a capability link does not satisfy this recipe; the recipes gate
rejects it.
