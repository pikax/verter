# Where async `Box::pin` is justified (and where it is not)

**Audit verdict (2026-07-22): NEGATIVE.** The measured data rejects blanket boxing as a fix; it would add allocations without reducing total retained handler state.

## Symptom

Async state-machine bloat invites a reflexive “box everything” fix. The
peer investigation already **rejected** boxing the definition path as a
cure for the **debug** stack overflow: release stack peak is only
**117 KiB** of a 1 MiB main-thread stack, and boxing would add a heap
allocation per request forever to treat a debug-only poll-frame layout
problem.

This note applies the same judgement across the class using measured
sizes.

## Mechanism

| surface | already boxed? | future size (outer) | hot path? | held many times? |
|---|---|---|---|---|
| `TypeProvider::*` | **yes** (`ProviderFuture`) | **16 B** | yes | yes (every hop) |
| `LspService::call` / router | **yes** (`BoxFuture`) | **16 B** slot | yes | up to 64 |
| inner handler state (inside the box) | **no** | **21–38 KiB** | yes | up to 64 on heap |
| `run_with_audit` / deadline wrappers | no (nested in handler) | +hundreds B tiny; multiplies large body | yes | with body |
| background `tokio::spawn` tasks | task owns future | N/A (separate task) | background | one per spawn |
| completion-detail `JoinSet` tasks | spawn | small per task | enrichment | ≤8 concurrent |

Boxing at the **trait object** boundary (`TypeProvider`) is already paid
and is required for `dyn TypeProvider`. Boxing at the **router** boundary
is already paid by tower-lsp-server. An **extra** `Box::pin` inside Verter
handlers would:

- **Not** reduce the heap mass of a 64-wide storm (still 64 × ~38 KiB of
  state somewhere), unless the point of boxing is to shrink *stack*
  during poll.
- **Would** add an allocation + indirect poll on every request if applied
  to every method.

## Reproduction

Sizes: same harness as `lsp-handler-async-state-machine-sizes.md`.
Judgement criteria: (large) AND (held many times) AND (hot path) AND
(stack or heap evidence that boxing moves the needle).

## Evidence

| candidate | size | ×64 heap | box justified? | reason |
|---|---|---|---|---|
| `LanguageServer::completion` | 38,096 B | ~2.32 MiB | **maybe later** | largest; hot; only if debug stack or heap shape is a product goal |
| `LanguageServer::rename` | 37,680 B | ~2.30 MiB | maybe later | same class |
| `LanguageServer::goto_definition` | 37,168 B | ~2.27 MiB | **no for release stack** | peer: release 117 KiB total; boxing is forever cost for debug |
| `LanguageServer::hover` | 21,352 B | ~1.30 MiB | no (same) | |
| small methods (≤6 KiB) | ≤6,192 B | ≤0.4 MiB | **no** | cost > benefit |
| `TypeProvider` hops | 16 B | n/a | **already boxed** | keep |
| recursive async | (none found) | — | **yes if introduced** | standard remedy; see negative doc |

**Preferred alternative to boxing:** shrink the state machine (split
sync vs query phases, drop locals after await) so both stack *and* heap
capacity × size fall without a permanent per-request alloc.

## Project-wide extension (non-LSP)

| surface | already boxed? | size | held many times? | box justified? |
|---|---|---|---|---|
| `TypeProvider::*` | **yes** | 16 B outer; ~168 B heap body | every hop | **keep** (dyn trait) |
| MCP `#[tool]` methods | **yes** (rmcp macro) | 16 B outer; ~40–64 B body | spawn-per-request | **keep** (already paid) |
| NAPI VFS async | NAPI runtime | 40–112 B | per JS call | **no further box** |
| tsgo_api `JsonRpcConnection::request` | no | ~32–280 B | per RPC | **no** (tiny) |
| Completion-detail tasks | spawn (heap) | ~192 B | ≤8 | **already task-isolated** |
| Scheduler jobs | n/a (sync) | — | — | n/a |
| Live session / semantic | n/a (sync) | — | — | n/a |
| Recursive async | none found | — | — | **yes if introduced** |

**Judgement:** outside LSP, boxing is already applied where the type
system requires it (`dyn TypeProvider`, `dyn` tool handlers). Further
`Box::pin` on 32–280 B request futures would not move the needle. The
only place boxing-or-shrink still earns a product discussion is the
**LSP trait methods at 21–38 KiB**, already analyzed above.

## Why deferred

Policy judgement for a later performance cut. No code change in this pass.

## Proposed fix + falsifiable prediction

If product requires debug builds to survive the first request **without**
raising the stack:

1. Prefer **shrinking** the top three futures (completion / rename /
   definition) by structural split.
2. Only if that is insufficient, `Box::pin` those three at the
   `LanguageServer` impl boundary (not every method).

**Prediction:**

- Shrink-only: trait future `size_of_val` drops; capacity × size heap
  drops proportionally; release latency within noise.
- Box-only: trait future becomes 16 B; heap mass ≈ same; one extra alloc
  per request visible in an allocator counter; debug stack peak falls if
  poll-frame layout was the overflow cause.

## Blast radius

- Boxing at the trait boundary: local to `server/mod.rs` method bodies;
  no protocol change.
- Over-boxing every small method: pure noise cost on the hot path.
- Leaving as-is: release fine; debug fragile; ~2.3 MiB storm heap for
  large methods.
