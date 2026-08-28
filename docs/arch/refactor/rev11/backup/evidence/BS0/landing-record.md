# BS0 landing record

Four ratified Svelte findings corrected in their root-cause owners. Charter:
[`charters/BS0.md`](../../charters/BS0.md). Ratified rows `SV-1`–`SV-4`:
[`evidence/BF3/dispositions.md`](../BF3/dispositions.md). Dispatch prompts:
[`context-packet.md`](context-packet.md).

Everything below was observed by running it. The pinned oracle is the official
`svelte@5.56.8` compiler, driven through the conformance harness's own
`src/invoke-svelte-oracle.mjs`; every "official emits X" claim in the code
comments and tests was measured, not transcribed.

## What shipped

| row | owner corrected | outcome |
|---|---|---|
| SV-1 | `verter_compiler` `svelte/runtime/lower_block.rs` — `EachReactivityFacts` | `{#each}` item reactivity follows the official predicate on all four of its axes |
| SV-2 | `verter_compiler` `svelte/runtime/client_surface_script.rs` — `PropWriteScan` | instance-script `$props()` READS accepted; WRITES still fail closed |
| SV-3 | `verter_compiler` `svelte/runtime/{instance_items,client_plan_script,client_block_emit}.rs` | `$state` / `export let` declarations and `{#if}` tests carry authored provenance |
| SV-4 | `verter_session` `typeinfo/framework_surface/svelte_exec.rs` — `runes_props_from_destructure_geometry` | an untyped `$props()` destructure publishes its authored props to TypeScript |

All four named `#[ignore]`d correct-behaviour targets are **enabled and green**;
their suites run with **zero ignored tests**.

### SV-1 needed a second half the row did not name

Clearing `EACH_ITEM_REACTIVE` alone emits a module that mounts and renders
wrongly: with the flag clear the runtime hands the render callback the RAW item,
so the surviving `$.get(item)` read dereferences a non-signal. The flag and the
item's read form are two halves of one decision, so `finalize_each_item_reactivity`
demotes non-reactive item bindings using the SAME predicate the flag projects
from. The demoted kind is a DISTINCT `BindingRuntimeKind::EachPlain`, not
`PlainLocal`: an each item is still not an assignment target, and reusing
`PlainLocal` let the `bind:` gate accept it and emit a setter that wrote the
callback parameter without ever reaching the collection.

### SV-4's real owner is one layer below the charter's wording

The charter names "the session-side Svelte PublicApi projector's untyped
`$props()` surface". The empty surface ORIGINATES at `resolve_runes_props`
returning `Missing` when there is no authored props type — one layer below. That
leg has three production consumers (component-meta, the framework-surface wire
executor, and the public-API projector), verified by exhaustive call-site search
during the pre-implementation consult. Correcting it in the projector would have
served one consumer and forked the shared surface, against the Shared Optimized
Codebase rule. **The correction was made in the shared owner.** This is recorded
as a charter-wording inaccuracy, not a scope change; the charter text is a
maintainer artifact and was not edited.

## Evidence

Directly observed, with the exact invocations. The four conformance targets sit
behind `--features bf2-authoritative` and are invisible to the default gate — a
filter without the feature matches zero tests and still exits 0, so every run
below is quoted with its `running N tests` line.

| surface | invocation | result |
|---|---|---|
| Svelte conformance gate | `cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1` | `running 18 tests` — 18 passed, **0 ignored** |
| public-API TypeScript observation | `cargo test -p verter_session --lib --features bf2-authoritative public_api_typescript_observation -- --test-threads=1` | `running 8 tests` — 8 passed, **0 ignored** |
| compiler | `cargo test -p verter_compiler` | 6050 + 496 passed, 0 failed |
| session (lib) | `cargo test -p verter_session --lib` | 5797 passed, 0 failed |
| session (integration) | `cargo test -p verter_session --tests` | 2464 + 5797 passed, 0 failed |
| Svelte behavioural runtime | `pnpm test` in `packages/svelte-runtime-tests` | 7 files, 35 tests passed |
| canonical gate | `node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB` | **VERDICT: PASS** on the exact landing tree — surface 1: 24413 run / 24413 passed; surface 2: 3 suites clean; surface 3: 8634 run / 8634 passed; exit 0 |
| workspace lints | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| wasm lints | `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | exit 0 |
| release profile | `cargo check --workspace --release` | exit 0 |
| formatting | `cargo fmt --all --check` | exit 0 |

### The gate was run twice, and the first run is recorded

The first gate run returned **FAIL** on four tests: three trybuild compile-fail
suites that TIMED OUT (180s / 180s / 360s) and one process-respawn budget test
(`failed_respawn_retries_within_budget_and_recovers`, failing at 14.4s). It ran
while a second track's gate had just finished and other cargo work was live;
that concurrent gate reported the **identical four**. Each was then re-run
alone: the respawn test passes in 13.9s, and the 360s trybuild suite completes
in 116s. None of the four is in this block's diff, and none has any Svelte
coupling. The second run, on an idle machine, is the PASS above.

Recorded rather than discarded because a FAIL verdict is a verdict: the pass
claimed here is a later run's, and the first run's cause is machine contention,
demonstrated by isolation rather than assumed from the test names.

The PASS quoted above is from a THIRD run, on the exact tree being landed. The
branch was rebased onto the integration tip after a sibling block landed, which
required resolving one conflict in a shared Svelte product-surface test — that
sibling had re-shaped the very row whose refusal witness this block replaced.
The gate was re-run afterwards rather than relying on the pre-rebase pass,
because the landing tree is the union of two separately-gated changes and only
the union was never gated.

Two of the three reachable Svelte client requests — `basic-runes` and
`legacy-slots` — now reach the oracle's `pass` verdict outright, structurally
and by mapping. `props-events` emits for the first time and mounts and renders
**identically to the official module**; its recorded outcome is a mapping-only
divergence on two anchors whose producers are named and deferred below.

## Deferred, with owners

Every item below was reproduced against the official compiler and is captured as
an `#[ignore]`d characterization that fails for the stated reason. None is a
production guard, refusal, or tracking artifact.

| defect | test | owner |
|---|---|---|
| `EACH_ITEM_IMMUTABLE` set from runes mode alone; official is `runes && !uses_store` | `each_item_immutable_clears_when_a_runes_collection_subscribes_a_store` | Svelte client block planner |
| a single-name destructure each context binds the ITEM under the field's name | `a_single_name_destructure_each_binds_the_item_not_its_field` | Svelte each-block lowering |
| an each keyed by its own INDEX is unkeyed for official | `an_each_keyed_by_its_own_index_is_unkeyed_for_official` | Svelte each-block lowering |
| official accepts a MEMBER bind rooted at an each item | `a_member_bind_rooted_at_an_each_item_is_accepted_by_official` | Svelte bind-target classifier |
| a non-ASCII identifier PANICS on a char-boundary slice — **pre-existing**, verified by compiling the same source against `dd84e5fa2`, which panics identically | `a_non_ascii_identifier_compiles_instead_of_panicking` | Svelte expression rewriter's replacement planner |
| a `function` declaration's name carries no map provenance | `a_function_declaration_carries_its_authored_name_provenance` | Svelte instance-script lowering |
| a shorthand attribute binding carries no map provenance | `a_shorthand_attribute_binding_carries_its_authored_name_provenance` | Svelte client attribute emitter |

The last two are exactly `props-events`'s two unmet map anchors, so its recorded
mapping divergence is fully accounted for.

## Discrimination

Every correction's test was proven to fail against the un-fixed code by planting
the superseded implementation, running it RED, and reverting — each plant proven
present, unique and new in the source before the run, never inferred from an
exit code.

| plant | effect |
|---|---|
| restore the superseded each-reactivity predicate | 3 matrix rows RED (17/25/17 against 16/24/16) |
| drop the declaration name mappings | both declaration-provenance tests RED |
| revert the `{#if}` test to unmapped text | the if-provenance test RED |
| demote the each item to `PlainLocal` instead of `EachPlain` | the bind-root test RED |
| skip the item demotion entirely | the flag/read-form coupling test RED |
| return `Missing` for an untyped destructure | the shared-surface test RED |

The gate scaffold was hardened while it was being re-characterized: an emitting
cell that diverges in NO family is now **unrepresentable** as `EmitsAndFails`
(private fields, three named constructors in a child module) rather than merely
unasserted, and the oracle's `verdict` is now compared against the recorded
families — it was previously never read, so `EmitsAndFails { false, false }` was
spellable and would have silently recorded a PASSING cell under a name asserting
it fails.

## Reviews

Two mandates, both external CLI seats, both run against running code.
Architecture was not required for a subsystem-class block (governance §2.2) and
was not run; no structural doubt arose that would have warranted it.

| mandate | seat | verdict | outcome |
|---|---|---|---|
| conformance | Codex Sol, high | BLOCK → resolved | 3 findings, all fixed: the `PlainLocal` bind-root regression, `PropWriteScan` missing nested member roots and catch/for scope frames, and a call-bearing `{#if}` test losing provenance |
| adversarial | Grok 4.6, extra-high, default-to-BLOCK | BLOCK → resolved | 2 blocking findings: a false whole-program claim by a flags-only matrix row (scoped, plus the real defect characterized), and `key_is_item` not matching official on a TS-wrapped key (fixed) |
| targeted delta | Codex Sol, high | LAND | confirm on the round-2 fix delta only; its one out-of-scope observation (official also erases `TSInstantiationExpression`) was closed in the same delta, completing the erasure set to all five TypeScript-only wrapper forms |

Both seats found real defects in running code. Two of the five were regressions
this block introduced and would have shipped without the review.
