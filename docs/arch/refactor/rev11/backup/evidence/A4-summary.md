# A4 — Measurement-only attribution and the captured baseline

Installs a typed, measurement-only work-attribution substrate and captures a
baseline dataset with it. No semantics change; nothing branches on a counter.

## What landed

**The substrate** — `crates/verter_audit/src/attribution/`:

- `schema.rs` — a CLOSED `WorkSite` enum declared in ONE macro invocation, each
  variant carrying a stable dotted id (`<owner>.<chokepoint>`), a `WorkDomain`
  (the charter's measurement categories) and a `WorkUnit`
  (`Calls`/`Items`/`Bytes`/`Nanoseconds`/`Gauge`/`Digest`). Sites cannot be
  minted ad hoc at a call site the way a string-keyed counter can, and
  `WorkSite::ALL` makes the inventory enumerable at compile time.
- `table.rs` — a dense `[SiteCell; WorkSite::COUNT]` of relaxed atomics:
  calls, amount (sum, or high-water mark for a gauge), nanos, an
  order-independent digest, and alloc count / alloc bytes / dealloc bytes.
- `scope.rs` — `ScopeGuard`, timing its region inclusively and publishing its
  site as the thread's innermost open scope via a `const`-init thread-local
  (const-init is required: the allocator reads it, and an allocating read
  would recurse forever).
- `alloc.rs` — `AttributingAllocator<A>`, a `GlobalAlloc` wrapper charging
  every allocation to the innermost open scope. Installed only by a
  measurement binary; no library installs it and none can.
- `report.rs` — deterministic TSV/JSON renderers plus domain roll-ups.

**The macros** — `attribute!`, `attribute_n!`, `attribute_max!`,
`attribute_scope!`, `attribute_digest!`, exported at the crate root so a call
site needs no import.

**The call sites** — 70 chokepoints across `verter_workspace`,
`verter_session`, `verter_compiler`, `verter_scheduler`, `verter_napi` and
`verter_wasm`, covering every domain the charter names.

## No semantic authority, held structurally

The schema types compile unconditionally, because the disabled macro arm names
a site. **Everything that can produce a number** — `snapshot`, `snapshot_all`,
`read`, `reset`, `SiteSample`, the `record_*` entry points, the renderers — is
behind the non-default `attribution` feature.

So a production build cannot write `if attribution::read(site).calls > n`: the
path does not resolve. There is no runtime flag to audit and no "disabled" stub
returning zero that a caller could branch on by accident. The only edge in the
workspace requesting the feature is `verter_bench`'s own `attribution` feature;
no production binary does.

This is proven from OUTSIDE the crate by a trybuild fixture
(`crates/verter_audit/tests/cases/compile-fail/attribution_reader_absent.rs`)
that names the whole reader surface and branches on a counter. It must fail to
compile, and the pinned stderr captures rustc's "the item is gated behind the
`attribution` feature". The in-crate disabled tests cannot prove this — a test
able to observe the reader's absence would itself be the reader.

## Disabled overhead

Not measurable. Full argument and numbers: [`A4/disabled-overhead.md`](A4/disabled-overhead.md).

Three proofs: the OFF arm expands to a `const` item and emits no code; a
discriminating test proves the amount expression is never evaluated (with a
control proving the probe works); and a three-arm wall-clock A/B against the
pre-instrumentation tree at `839645e3e` shows a control-vs-disabled delta that
straddles zero. The enabled arm costs ~+7–13%, mostly the global allocator.

## The baseline

`verter_bench`'s `attribution_baseline` example. The corpus is SYNTHESISED
in-process — 40 Vue components sharing one TS types module — so the same
command produces the same work on any machine with no fixture directory and no
external checkout. Each run builds a fresh host, upserts, loads, requests
component metadata per component, then compiles the corpus.

Dataset: [`A4/baseline-40-components.tsv`](A4/baseline-40-components.tsv)
(one row per site that recorded something; 40 components, release profile).

### What the baseline says

Ranked by call count, the pipeline is dominated by work that is not parsing,
resolution or codegen:

| site                               | calls  | per file |
| ---------------------------------- | ------ | -------- |
| `session.fact_observe`             | 16,917 | 413      |
| `workspace.normalize_canonical_id` | 11,313 | 276      |
| `session.analysis_snapshot_copy`   | 8,194  | 200      |
| `session.indexed_ready_build`      | 8,032  | 196      |
| `session.read_set_signature_build` | 5,115  | 125      |
| `session.semantic_dispatch`        | 4,216  | 103      |

Against 41 files the corpus parses **40 carrier parses, 42 eval-program
parses** — parsing is already once-per-file and is not the cost. The
repetition is in the layers above it:

- **Path normalisation runs 276× per file.** 11,313 calls over 41 canonical
  ids, 175 KB of path text normalised, for a corpus with 41 distinct paths.
- **`ensure_indexed_ready_serve` runs 196× per file** and is the second-largest
  timed scope (7.1 ms) — the artifact is built once but the serve path is
  re-entered constantly.
- **Fact observation is the most frequently CALLED site in the system**, at
  16,917 calls (413 per file). Those calls observe 73,923 facts (the `amount`
  column — 1,803 per file), feeding 5,115 read-set signature builds. Calls and
  observed items are different columns and are not interchangeable: the
  fan-out per call is ~4.4.
- **Resolution is the largest timed region**: `semantic_dispatch` sums
  216 ms of INCLUSIVE scope intervals across 4,216 dispatches, of which
  3,153 (75%) are warm hits and 1,063 are cold builds. Every cold build was
  admitted cacheable — zero `ReturnOnly` on this corpus.

  **216 ms is not a share of wall clock and is not comparable to one.** This
  workload's total wall clock medians ~75–80 ms (the arm table in
  [`A4/disabled-overhead.md`](A4/disabled-overhead.md)), so the figure is
  ~2.7–2.9× the ENTIRE run and cannot be a share of it. Scope timing is
  inclusive, and `semantic_dispatch` re-enters itself on
  cold builds, so each nested frame records the full interval again and the
  column double-counts by recursion depth. Read it as "summed inclusive
  intervals", never as "time spent here"; it may not be added to other sites'
  intervals either. What the number supports is the RANKING — resolution is the
  deepest and most re-entered region on this corpus — not a percentage.
- **Most allocation is unattributed**: 641,923 of the 1,151,051 allocations
  (55.8% by count, 112.6 MB — 50.0% of the 225 MB total bytes) land outside
  any open scope guard. The largest single attributed region is
  `semantic_dispatch` at 98.6 MB across 413,131 allocations. Widening scope
  coverage is the obvious next step for anyone using this rail.

Determinism: the component-meta digest AGREES across two consecutive
in-process runs. The compiled-output digest records no observation on this
corpus, so its check reports N/A rather than a vacuous `0 == 0` agreement.

## Deviations from the charter's site list, and why

The charter names measurement CATEGORIES; the initial schema guessed at
concrete sites. Five guessed sites turned out to have no production
chokepoint, and were DELETED rather than attached to something approximate —
a counter wired to a near-miss is worse than an absent one, because it reads
as covered.

| deleted site               | why                                                                                             |
| -------------------------- | ----------------------------------------------------------------------------------------------- |
| `shallow_file_state_parse` | No production parser. Shallow state builds from the retained eval program; only `#[cfg(test)]` parsers exist. |
| `external_frontier_parse`  | Same — the frontier reuses the retained snapshot.                                                |
| `queue_enqueue`            | No single chokepoint: seven scattered inbox sends. The signal is already carried by `submit_request` / `submit_batch` / `queue_depth`. |
| `relation_oracle_probe`    | The oracle probe is `#[cfg(feature = "oracle-gen")]` tooling, not a query-time path.             |

Four site ids were retargeted because the real owner crate differed from the
guess (`carrier_parse`, `style_analysis` → `compiler.*`; `typeinfo_graph_encode`
→ `ffi.*`; `audit_record_encode` → `napi.*`). Ids are `<owner>.<chokepoint>`
and a wrong owner makes a report row misattribute work.

Every remaining declared site has at least one production call site.

## Known gaps and deferrals

- **Gate coverage for the `attribution` / `compile-fail` features is deferred
  to A5.** Neither feature is compiled or run by `node scripts/gate.mjs`, so
  two things can rot silently: the ENABLED arm (its amount expressions are
  type-checked only under `--features attribution`, and this block already hit
  one such error that the default arm accepted), and the compile-fail trybuild
  seal that proves the reader path is absent — the negative control for the
  no-semantic-authority claim, which is never executed by the canonical gate.
  Verification for this block ran both arms manually; that is a per-block
  action, not a standing guard. Deferred to A5, which reconciles the surviving
  instrumentation owners and is therefore where the gate wiring for whichever
  rails survive that reconciliation should be decided. Debt owner: A5.
  Raised as adversarial review finding F5 on this block; `scripts/gate.mjs` is
  deliberately untouched here.
- **`compiler.compiled_output_digest` records zero on this corpus.** It sits on
  the Vue bridge compile entry, which `compile_many`'s host-backed lane does
  not reach for these inputs. The site is wired and correct; this workload does
  not exercise it. A Svelte or bundler-lane workload would.
- **CSS sites record zero.** Same cause — `process_style` is not on this
  workload's path.
- **`verter_span`-level path canonicalisation is unattributed.** The counted
  site is `verter_workspace`'s wrapper; `verter_span::path::canonicalize_path`
  sits BELOW `verter_audit` in the dependency order and cannot be instrumented
  without inverting the leaf-crate rule. Direct callers of the span-level
  function are therefore not counted, so `normalize_canonical_id` is a lower
  bound.
- **FFI counts are scope-only.** Both boundary sites sit on each crate's shared
  `catch_panic` wrapper, which every entry funnels through — correct for
  counting crossings, but it does not distinguish which entry crossed.
- **No landed guard asserts every site has a call site.** Such a guard would be
  a name-keyed source scanner, which the repo's forward-only rule forbids as
  landed enforcement. Coverage was verified during this block by inspection;
  the durable replacement would be structural, not a scanner.

## Verification

- `cargo check --workspace --all-targets` — clean.
- `cargo check --workspace --all-targets --features verter_audit/attribution` — clean.
  Both arms are required: the disabled arm does not type-check amount
  expressions, and this block hit exactly that (a wrong field name in the
  compiled-output digest passed the default arm and failed under the feature).
- `cargo test -p verter_audit` — 46 pass in the default arm, 13 attribution
  tests with the feature on.
- `cargo test -p verter_audit --features compile-fail` — the reader-absence
  fixture passes.
- `cargo test -p verter_workspace -p verter_scheduler -p verter_compiler --tests`
  — 7,583 pass, 0 fail.
- `cargo test -p verter_session --tests` — 8,179 pass, 0 fail (re-run after the
  two guard fixes below; the pre-fix run had 2 failures).
- `cargo clippy` on the instrumented leaf crates, both arms — clean under
  `-D warnings`.

Two architecture guards failed on the first full session run and were fixed,
not suppressed:

- `audit_substrate_isolation` — the guard rejects any `verter_*` token under
  `crates/verter_audit/src`, and the scope macro's local binding name
  (`_verter_attribution_scope`) tripped it. The substrate genuinely still
  depends only on `verter_span`; the binding was renamed rather than the guard
  relaxed.
- `tracked_paths_no_machine_roots` — the per-run orchestration brief under
  `.agent-run/` had been committed by an over-broad `git add -A`, and it embeds
  absolute machine paths. `.agent-run/` is now untracked and gitignored.

The full `node scripts/gate.mjs` was NOT run for this block — see the block
report for that trade-off.
