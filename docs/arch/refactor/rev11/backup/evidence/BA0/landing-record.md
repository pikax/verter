# BA0 — landing record

Base `17c25a03d`. Candidate `017760d31`. Dispatch context:
[`context-packet.md`](context-packet.md).

## What shipped

A compile request now names its product set and is atomic over it.

- The carrier's terminal result is a sum — produced the requested products, or
  refused the runtime surface and carries none. `RuntimeCompileOutput` lost its
  `runtime_surface_refused` flag, so the mixed value is not representable. The
  refusal reason travels as a typed code and message, and the host's
  `starts_with("svelte-runtime-unsupported-")` prefix scan is deleted.
- `CompileTarget::needs_runtime_module` (`STYLE|SCRIPT|TEMPLATE`) decides whether a
  carrier attempts its runtime compile, so a request that asked for no runtime
  product can never be refused one. `CompileTarget::publishes_runtime_module`
  (`SCRIPT|TEMPLATE`) decides whether the assembled main module publishes. Each
  virtual node publishes only under its own bit, gated at the insertion point.
- A refused transaction publishes no product and no scheduler artifact.
  `ensure_compiled` gained a "transaction completed" demand and answers identically
  cold and warm. A validated produced serve is terminal for node demands, so a
  target-excluded node no longer forces a recompile per read.
- `CompileBatchEntry` carries one outcome: produced with a product, or failed with a
  non-empty error list — never both. All nine construction sites route through it;
  the transports flatten it arm by arm, so the JS-visible shape is unchanged.

`verter_dx_baseline` was switched from `ensure_compiled` to `ensure_ide_compiled`:
it consumes only the IDE product and the public API, so it should not request
runtime output.

## Acceptance targets

| id | target | state |
|---|---|---|
| `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY` | `a_refused_combined_request_publishes_no_product_at_all` | `#[ignore]` REMOVED; live and green |
| `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` | `the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error` | retained `#[ignore]`d under its ratified name, restated against the corrected construction; green under `--ignored` |

AT-1's body was re-expressed over a genuinely combined token (`BUNDLER | IDE`);
the old body called a TSX-only profile "combined" and observed a separate later
request as if it were the same one. The re-expression was ruled legitimate and
required by an unprimed consult before implementation — see the context packet.
It was proven RED on the base tree for the ATOMICITY assertion specifically
(`the IDE/TSX product was published under the refused combined identity`), not for
a compile error or an unmet precondition. Four controls keep it from being
satisfiable by publishing nothing: a distinct IDE-only identity still publishes a
non-empty IDE product; the PublicApi identity still renders; both request orders on
independent hosts agree; and a SUPPORTED component under the same combined token
publishes BOTH its runtime module and its IDE product.

AT-2 carried no required-RED target and no Svelte-refusal obligation, per the
maintainer act. Its hazard is removed structurally rather than characterised: an
adversarial plant restoring the old product-plus-errors shape failed to COMPILE
(`E0027`).

## Review arc

Class `foundational-atomic`; all three mandates required. Seats were external CLIs,
run sequentially in a throwaway worktree detached at the candidate.

**Round 1** (`f404f735c`) — conformance PASS; architecture BLOCKING (3); adversarial
BLOCKING (4 + 1 nit). All eight adversarial plants went RED with proof of
application, so the suite discriminated; the findings were coverage gaps, not fake
tests. Closed by one fix round: `ensure_compiled` cold/warm divergence; a refused
transaction still committing a scheduler artifact; requests publishing products they
did not ask for (`STYLE` alone, `ANALYSIS`, `META`, and Vue under `IDE|TEMPLATE_DATA`
publishing a main module); stale transport docs. Each fix carried a captured RED.

**Round 2** (`02278891f`) — conformance PASS; architecture PASS with one P2;
adversarial BLOCKING (2). The P2 was a candidate-induced regression: with
target-scoped publication, a demand for a target-excluded node missed the warm gate
and recompiled forever — reachable from the LSP, which probes every parse-derived
node under its IDE profile. The adversarial BLOCKING that mattered was that the new
cold/warm test compared only returned strings and never asserted a cache hit, so
both calls could recompile and still pass — a non-discriminating assertion. Both
fixed; the test now asserts the compile-count artifact.

**Round 3, targeted delta only** (`59e8deefb`) — FIX 1 and FIX 2 verified by plant/
red/revert/green; one documentation defect inside the delta (two comments falsified
by the dev last-known-good path). Fixed, and the author's own re-read found two more
of the same class.

## Dispositions

- **REJECT** — an adversarial plant that builds the Svelte IDE projection and
  DISCARDS it before the refusal return leaves the acceptance target green. It
  changes no observable output: `project_ide` is a pure free function whose returns
  are dropped, every refusal arm returns before it, and `RuntimeSurfaceRefusal` has
  no field that could hold a projection. The charter's property is that a refused
  request PUBLISHES no product, not that it never CONSTRUCTS one; detecting
  discarded work needs a work-count proxy, which ranks below asserting the artifact.
  Independently confirmed correct by the delta review seat.
- **DEFER → B3** — `ensure_compiled` has two warm implementations (its private
  precheck and the shared consult in `ensure_compile_artifacts`). Deleting the
  precheck would make cold==warm structural rather than asserted. Ruled a correct
  deferral by the delta review seat, which assigned it to B3 as the owner of the
  sole canonical typed request and the request-construction cutover — B4 owns
  publication, not this route convergence. Resolution gate: B3 acceptance, no later
  than plan close. Acceptance id: `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY`, whose
  cold/warm equivalence is currently held behaviourally by
  `ensure_compiled_answers_the_same_cold_and_warm_for_one_identity`.
- **ADOPT-NOW** — one deliberate deviation is recorded rather than silently taken:
  the warm-satisfaction change exempts the `Ide` demand, which still requires
  `tsx.is_some()`. The dev last-known-good fallback publishes a produced slot with
  `tsx: None` on a FAILED compile and is the default policy, so an always-satisfy
  `Ide` arm would pin `ensure_ide_compiled` to `Ok(false)` with no retry. Verified
  correct by the delta review seat against the code.
- **Carried, not fixed** — under `CompileCacheMode::Content`, `ensure_ide_compiled`
  answers `Ok(true)` while `get_ide` reads `None`, because content mode publishes to
  the content-addressed node while `get_ide` reads the session cache. Proven
  PRE-EXISTING on `dd84e5fa2` by both the author and the conformance seat. Captured
  as the `#[ignore]`d `ensure_ide_compiled_and_get_ide_agree_under_content_cache_mode`
  asserting the CORRECT contract, paired with a live `..._under_session_cache_mode`
  guarding the mode interactive consumers use. No guard was added. Owner is
  unassigned; this block does not own it.

## Gate

`node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB`, run once at landing
readiness on the rebased tree, which is byte-identical to the landed tree.

Terminal summary produced; VERDICT **FAIL**, 4 non-tolerated failures, all on
Surface 1. Surface 2 clean (3 suites). Surface 3 (shipped `cfg(debug_assertions)`
OFF) clean: 8631 run, 8631 passed. Surface 1: 24401 run, 24397 passed.

All four failures were discriminated against the base tree `17c25a03d` in a
separate worktree and are PRE-EXISTING:

| failure | evidence |
|---|---|
| `pending_nav_request_is_unreachable_outside_vapor` | FAILS on base. A `trybuild` expected-output mismatch: rustc reports `enum PendingNavRequest is private` where the pin expects `module template is private` |
| `segmented_overwrite_authority_is_unreachable_outside_the_crate` | FAILS on base |
| `hot_materialize_and_script_fact_structural_rails_smoke` | TIMES OUT on base (360s limit) |
| `resilient::resilient_tests::failed_respawn_retries_within_budget_and_recovers` | `verter_type_runtime` has NO dependency path to any crate changed here — `cargo tree -e normal,dev` returns nothing for `verter_session`/`verter_compiler`/`verter_napi`/`verter_wasm`/`verter_dx_baseline`/`verter_lsp`, and the test binary did not recompile. Measured 5 runs per state in one worktree: 4/5 failures on this branch AND 4/5 on base. A wall-clock race against two deliberate failed respawns plus a real tsserver spawn, on a fixed ~9.75s retry budget |

The gate verdict is reported as FAIL rather than reinterpreted. No failure is
attributable to this change; the program branch is red on these four independently.

Also run on the landed tree: `cargo fmt --all --check` clean;
`cargo clippy --workspace --all-targets -- -D warnings` clean;
`cargo check -p verter_napi` and `cargo check -p verter_wasm --target
wasm32-unknown-unknown` clean. Reverse-dependency sweep of every crate the change
can reach that the routine sweeps missed (`verter_mcp`, `verter_tsc`, `verter_ffi`,
`verter_dx_baseline`, `verter_bench`, and both conformance crates): 568 run, 568
passed. No manifest and no lockfile was touched anywhere on the branch.

## Scope held

No Svelte emitter, map or projector change; no standalone CSS change; no batch
carrier-selection change (RT-1 stays with BRT0, and the batch route still hardcodes
its carrier); no transport missing-product parity change; no final canonical request
model and no final publication substrate — none of `prepare/plan/project/emit`,
`CompilePlanToken`, `ProjectionPlanToken` or `ProductSubplanToken` appears. No
production guard, typed refusal mechanism, withhold path, retraction, tracking
artifact, fixture-identity branch, known-divergence list or string-scanning second
authority was added; one string-scanning authority was DELETED. No type-correctness
work was opened.
