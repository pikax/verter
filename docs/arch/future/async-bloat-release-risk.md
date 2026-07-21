# Async state-machine bloat: release risk vs debug-only

## Symptom

The headline stack finding is **debug-dominant**:

| profile | serve-thread stack peak (peer, definition path) | notes |
|---|---|---|
| release | **117 KiB** of 1 MiB | transport prelude ~83% of path |
| debug | **1857 KiB** | exceeds 1 MiB; **first request dies** |

Future **sizes** are the same in debug and release (this inventory:
completion **38,096 B** identical on both profiles). The stack difference
is unoptimized poll-frame layout, not larger state.

Question for the rest of the class: is any path a **genuine release**
risk (stack or memory), or is the real non-debug cost memory under
concurrency?

## Mechanism

1. **Stack (release):** one large future polled through a boxed router
   slot on the serve task. Peer measured 117 KiB total for definition —
   well under a typical 1 MiB main stack / 2 MiB tokio worker stack.
   Other handlers are the same order of magnitude (21–38 KiB futures);
   none is an order larger than definition in this inventory.
2. **Stack (debug):** 16× worse poll frames; already fatal on first
   request for definition. Same class applies to completion / rename
   (slightly larger futures than definition).
3. **Heap (release and debug):** `buffer_unordered(64)` retains up to 64
   boxed handler states → worst measured **~2.32 MiB** for completion
   (see `lsp-buffer-unordered-heap-multiplication.md`). Not a stack
   issue; modest but real under storms.
4. **Provider / scheduler:** provider futures already boxed (16 B);
   scheduler is not async. Background drains run on separate tasks.

## Reproduction

Stack peaks: peer methodology (instrumented serve thread) — not
re-run here for every method (would need the same stack sampler).

Future sizes and capacity × size: this branch’s
`future_size_measure_tests` harness (debug + release).

## Evidence

| claim | evidence |
|---|---|
| Future sizes profile-independent | debug ≡ release for every measured handler |
| Largest trait future | `completion` 38,096 B |
| Definition in same band | 37,168 B trait / 37,016 B with_audit |
| Release stack near limit? | **No** for definition (117 KiB / 1 MiB); no larger future class found |
| Debug stack risk | **Yes** — established fatal; class extends to all large handlers |
| Release heap risk | **Mild** — ~2.3 MiB at full concurrency for largest method |
| Release stack risk for other methods | **Theoretical only** — no stack peak remeasured per method; sizes are ≤ completion, so unlikely worse than definition unless poll depth differs radically |

**Plain statement:** there is **no measured evidence** that release stack
is close to a hard limit on any path. The **genuine release cost** in
this class is **memory under concurrent load** (capacity × size), not
stack overflow. Debug remains unusable without a raised stack or a
future-size / boxing cut.

## Why deferred

Scope is documentation. Raising debug stack, changing concurrency, or
shrinking handlers is a later product decision.

## Proposed fix + falsifiable prediction

Priority order for a future cut:

1. **Developer experience:** make debug survivable (shrink top futures or
   document required stack size / use release-dbg for LSP).
2. **Release heap:** only if RSS storms show the pending-request set as
   a top contributor (measure with a heap profiler under 64-wide load).
3. **Release stack:** no action indicated by current numbers.

**Prediction:** a release build under a 64-wide completion storm shows
RSS +≈2–3 MiB attributable to request futures while stack high-water
stays &lt;256 KiB on the serve thread. If stack exceeds 512 KiB on
release for any method, revisit this document’s “theoretical” clause
with that measurement.

## Blast radius

- **If treated as release-critical incorrectly:** permanent per-request
  boxing cost for a non-problem.
- **If ignored entirely:** debug stays broken; mild heap under storms
  continues; no known release crash from this class alone.
