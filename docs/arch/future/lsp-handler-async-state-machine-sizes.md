# LSP handler async state-machine sizes (inventory)

## Symptom

A peer agent established that a single `textDocument/definition` path has a
**code-shape-constant** serve-thread stack peak (release **117 KiB**, debug
**1857 KiB** — debug died on the first request) driven by **async state-machine
bloat**: nested `async fn` futures store their state inline, so a deep chain
becomes one large future whose poll frame is proportional to the whole chain.

This document inventories the **rest of that class** on the LSP serve surface:
every major `LanguageServer` trait method future and its inner body, measured
with `size_of_val` on constructed futures (not estimates).

**Largest future found in this inventory:** `LanguageServer::completion` at
**38,096 B** (37.2 KiB). Polled on the tower-lsp-server serve thread after the
router boxes it into a `BoxFuture` and `buffer_unordered` holds the slot
(concurrency **64** — `LSP_MAX_CONCURRENCY`).

## Mechanism

Chain depth on the hot audited navigation path (deepest Verter-owned nesting):

| Layer | Site | Role |
|---|---|---|
| 0 | `tower_lsp_server::Server::serve` | `block_on` / `join!` / framed I/O |
| 1 | `buffer_unordered(LSP_MAX_CONCURRENCY)` | concurrent request slots (`transport.rs`) |
| 2 | `LspService::call` → `Box::pin` | boxes the router future (`service.rs`) |
| 3 | `LanguageServer::{method}` | trait method `async fn` in `server/mod.rs` |
| 4 | `handle_*_with_audit` | `nav_features_audit.rs` / `aux_features.rs` |
| 5 | `run_with_audit` / `run_with_deadline` | `audit_harness.rs` |
| 6 | `with_deadline(timeout(body))` | `verter_type_runtime::deadline` + `tokio::time::timeout` |
| 7 | `handle_*` body | e.g. `nav_features_navigation::handle_goto_definition` |
| 8 | `ensure_provider_synced` | nested under body (`sync_orchestration.rs`) |
| 9 | `TypeProvider::*` | **already** `Pin<Box<dyn Future>>` — **16 B** |

Other poll sites (not the serve thread, separate tasks):

- `sync_coordinator::coordinator_loop` — spawned background task
- `workspace_scanner::scanner_loop` — spawned background task
- `background_init` / drain paths — `tokio::spawn` / `spawn_blocking`
- Type-provider transports (tsgo/tsserver IPC) — boxed `ProviderFuture`s

`verter_scheduler` has **no** `async fn` surface (CPU-pool / sync admission).

Reference (serve-loop single-thread / HOL blocking, no future-size numbers):
peer note `lsp-serve-loop-single-thread-head-of-line-blocking.md` (other branch).

## Reproduction

Synthetic only. From the repo root:

```bash
cargo test -p verter_lsp --lib future_size_measure -- --nocapture --ignored
cargo test -p verter_lsp --lib future_size_measure --release -- --nocapture --ignored
```

Harness: `crates/verter_lsp/src/future_size_measure_tests.rs` (ignored tests).
Constructs a minimal `VerterLanguageServer` + mock type provider, opens a
tiny synthetic SFC, builds each future, prints `size_of_val`, drops without
polling. No real corpus.

## Evidence

**Toolchain:** `cargo test -p verter_lsp --lib` (debug = unoptimized +
debuginfo; release = optimized). Future sizes are **byte-identical** across
debug and release (matches the peer’s “size constant, stack layout differs”
observation).

### Wrapper layers (tiny body)

| future | size (debug = release) |
|---|---|
| tiny `async { Ok(7u8) }` | 1 B |
| `tokio::time::timeout(tiny)` | 120 B |
| `with_deadline(timeout(tiny))` | 288 B |
| `run_with_deadline(tiny)` | 328 B |
| `run_with_audit(tiny)` (audit on or off) | 752 B |
| `ProviderFuture` / `BoxFuture` slot | 16 B |

### `LanguageServer` trait methods (what the router boxes)

| method | size |
|---|---|
| **`completion`** | **38,096 B** |
| `rename` | 37,680 B |
| `goto_definition` | 37,168 B |
| `goto_type_definition` | 25,208 B |
| `hover` | 21,352 B |
| `code_action` | 6,192 B |
| `references` | 5,928 B |
| `semantic_tokens_full` | 4,728 B |
| `inlay_hint` | 4,704 B |
| `did_open` | 7,456 B |
| `did_change` | 4,440 B |
| `completion_resolve` | 3,912 B |
| `signature_help` | 3,032 B |
| `document_highlight` | 2,768 B |
| `document_symbol` | 2,392 B |

### Inner bodies vs audit wrappers

| future | body | `*_with_audit` / deadline wrap |
|---|---|---|
| completion | 9,000 B | 37,912 B |
| rename | 8,952 B | 37,528 B |
| goto_definition | 8,824 B | 37,016 B |
| goto_type_definition | 8,248 B | (deadline path; trait method 25,208 B) |
| hover | 4,912 B | 21,224 B |
| references | 1,000 B | 5,768 B |
| code_action | 968 B | (trait 6,192 B) |
| inlay_hint | 736 B | (trait 4,704 B) |
| semantic_tokens_full | 728 B | (trait 4,728 B) |
| document_symbol | 144 B | (trait 2,392 B) |

### Sync sub-futures (nested under handlers)

| future | size |
|---|---|
| `ensure_provider_synced` | **7,832 B** (matches peer table) |
| `ensure_imported_carriers_synced_memoized` | 7,792 B |
| `ensure_current_file_synced` | 3,704 B |
| `TypeProvider::get_definition` / `get_hover` / `get_completions` | **16 B** each |

### Prior peer measurement on definition (for correlation)

| future | peer size |
|---|---|
| audited handler (`goto_definition_with_audit`) | 136,088 B |
| handler body | 16,824 B |
| `ensure_provider_synced` | 7,832 B |
| provider `get_definition` | 16 B |

Independent remeasurement on this tree yields a **smaller** audited definition
future (**37,016 B** body wrapper / **37,168 B** trait method) while
`ensure_provider_synced` and provider hops match the peer **exactly**. The
absolute handler numbers can move with code shape; the **class** (multi-tens-of-KiB
nested futures, 16 B provider hop, identical debug/release sizes) is stable.

Audit on vs off does **not** change future size: `audit_enabled` is a runtime
branch inside one monomorphized state machine that reserves both arms.

## Why deferred

This effort records the class for a later cut. Scope is documentation only —
no handler rewrite, no boxing change, no semantic-engine edits.

## Proposed fix + falsifiable prediction

**Not a blanket-box recommendation** (see
`where-async-boxing-is-justified.md`). If a future cut reduces stack/debug
fragility or heap under concurrency storms:

1. Split large handlers so post-await mapping does not keep the pre-await
   sync closure’s full state live, **or**
2. `Box::pin` only the largest trait methods at the `LanguageServer` boundary.

**Prediction:** after boxing `LanguageServer::completion` / `rename` /
`goto_definition` at the trait boundary, `size_of_val` of the trait future
drops to **16 B** and `capacity × size` heap for 64 in-flight requests drops
from ~2.3 MiB to ~1 KiB of slots (plus 64 heap boxes of the old size — same
heap mass, different residency shape). Debug stack peak on first request
should fall under 1 MiB if poll-frame layout was dominated by the unboxed
state (peer already showed release stack is transport-heavy).

## Blast radius

- **Depends on it:** every LSP client request on the serve thread; debug
  developer loops; any concurrent burst up to `LSP_MAX_CONCURRENCY`.
- **If fixed carefully:** no protocol change; possible micro cost of one
  heap allocation per request if boxing is chosen.
- **If left alone:** release continues to work (117 KiB stack); debug
  remains unusable without a raised stack; heap under a 64-wide storm stays
  ~2.3 MiB for the largest handlers.
