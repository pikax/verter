# Future work — provider heartbeat + machine-adaptive backpressure

Status: **ONE CANDIDATE, not the chosen solution — pending research into proven IDE patterns.**
The heartbeat was a user SUGGESTION to explore, not a decision. This problem (feeding documents to
a TS backend at scale without crashing it, prioritizing the active file) is ALREADY SOLVED by
established systems — VS Code's built-in TypeScript extension, Volar, tsserver's own project system,
tsgo. A research task (`report-ide-document-feeding-patterns.md`) is finding their proven pattern,
which may be STRUCTURAL rather than a heartbeat: established extensions largely do NOT flood the
backend with every file — they open the active/demand-driven set and let the backend's PROJECT
SYSTEM load the rest lazily. If that is the answer, the fix is to change Verter's 'sweep every
companion' feeding model, and a heartbeat may be unnecessary (a workaround for a wrong feeding
model). Treat the design below as the fallback candidate; adopt the proven pattern the research
identifies first. The active-document-PRIORITY rule (open files served first, never starved by
background work) is proven-universal and holds regardless.

## Problem

Verter feeds documents (carrier IDE companions) to an external TypeScript provider (tsgo or
tsserver) for a whole repo: active documents, their child imports, and a background codebase sweep.
On a large monorepo the sweep is a FLOOD — hundreds of companion opens + diagnostic pulls. Two
things then go wrong, and BOTH have been observed (user test, 2026-07-20, on a ~72-SFC ui package):

1. **Starvation / priority inversion** — the sweep flood starves the ACTIVE document's companion,
   so the file the user is looking at never becomes serveable (hover returns nothing). Observed on
   BOTH managed tsgo AND tsserver — neither served intellisense under the flood.
2. **Provider overload** — tsserver is much slower than tsgo and flooding it with documents can
   CRASH it. tsgo is faster but is NOT proven immune (it also failed to serve under the flood).

A FIXED flood limit cannot solve this: capacity is **machine-dependent**. A document count that is
safe on a fast dev box can crash a low-end laptop or needlessly under-serve a big CI runner. We need
to know — **cheaply, safely, predictably — how much each provider can absorb right now**, and steer
the feed rate against that.

## Requirement (user directive 2026-07-20)

> "low end machines might handle different loads, we must have a way to know how much each can handle
> in a safe and predictable way, maybe we could have a heartbeat command that the tsserver or tsgo
> will fail; there should be something to tell us cheaply that it has crashed or too busy to handle
> more requests."

So: a **cheap health/heartbeat probe** to the provider that distinguishes three states —
- **HEALTHY** — responds promptly; safe to feed more.
- **SATURATED / TOO BUSY** — alive but slow/backed-up; apply backpressure (slow or pause the sweep),
  do NOT feed more, do NOT crash it.
- **CRASHED / DEAD** — no response / connection gone; restart + replay the desired set (recover).

— and an **adaptive backpressure** loop that uses it to feed the provider only as fast as it can
absorb, always prioritizing active documents.

## Design sketch

### Signal (cheap, must not itself add load)
- Reuse existing primitives first — do NOT invent a parallel mechanism:
  - `$/verter/getStatistics` is already used as a wedge-liveness probe by the corpus harness.
  - B12 added crash detection: `crash_notify`, the writer-stall watchdog, and the hang threshold /
    consecutive-failure counter in the resilient transport.
- The heartbeat is a lightweight provider request (a no-op/status ping, or a cheap existing query)
  with a deadline. Interpret: prompt reply → HEALTHY; slow reply / rising in-flight depth / rising
  round-trip latency → SATURATED; deadline miss + transport crash/EOF → CRASHED.
- Cost control: piggyback on traffic that already flows where possible; only actively ping when idle
  or when saturation is suspected, so the probe never becomes part of the flood.

### Adaptive backpressure loop
- **Priority is absolute** (see the sync-priority rule): active documents > child imports > sweep.
  Active-doc companion commits are never throttled and never superseded by sweep traffic.
- The SWEEP feed rate adapts to the health signal: increase concurrency while HEALTHY, hold/shrink
  on SATURATED, pause + recover on CRASHED. A control loop (e.g. additive-increase /
  multiplicative-decrease on an in-flight-documents window, bounded by observed latency) gives a
  self-tuning limit that lands where the machine + provider actually saturate — no magic constant.
- **Per-provider policy** (not one-size-fits-all): tsserver is slower + floodable-into-a-crash, so
  its window stays conservative/bounded (stability over warming speed); tsgo may open its window
  wider — but ONLY validated by the same health signal, since tsgo is not proven immune.

### Recovery
- CRASHED → the resilient layer restarts the provider and replays the desired document set, with
  the active documents replayed FIRST (priority preserved across restart).
- The backpressure window resets conservatively after a crash and re-widens as health recovers, so a
  crash-loop cannot be re-triggered by immediately re-flooding.

## Observability
- Expose the current health state, the adaptive window size, and in-flight depth per provider (a
  `getStatistics` field / structured trace) so the corpus/endurance harness can assert the loop
  behaves (window shrinks under load, active docs never starve, no crash-loop) and so the endurance
  goal is measurable across machine classes.

## Acceptance (when scheduled)
- A cheap probe distinguishes HEALTHY / SATURATED / CRASHED at the provider level, on tsgo AND
  tsserver, without materially adding load.
- Under a scaled sweep flood, the active document is served promptly on BOTH providers (priority
  holds), the sweep self-throttles to the machine's capacity, and tsserver is never flooded into a
  crash. Test on a constrained (low-resource) configuration to prove machine-adaptivity, not just a
  fast box.
- A crashed provider recovers (restart + priority replay) without a crash-loop.

## Relationship to the sync-priority / active-doc-starvation block
That block MUST reproduce the starvation red-first and fix the priority inversion + add a
BOUNDED-and-SAFE throttle that consumes a health signal (even coarse) and is designed to plug in this
richer heartbeat. It must NOT ship a hardcoded flood constant with no feedback. This doc is the
full adaptive subsystem that throttle grows into.

## Confidentiality
The motivating project is a private third-party monorepo used only for local repro; never name it in
committed content. Tests use hermetic synthetic corpora + a constrained-resource configuration.
