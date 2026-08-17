# Deviation memo — AT-2's ratified claim is not supported by any reachable instance

**Raised by:** the track orchestrator closing this block's evidence gaps.
**Status:** ESCALATED to the maintainer. Not dispositioned at track level, because the change it
implies is to a RATIFIED findings row, which no track-level actor may amend.
**Effect on this block:** owned-scope item 6 stays `NOT-EVIDENCED` for AT-2, and the block stays
NOT acceptance-recommended.

## What was found

AT-2's ratified row reads: *"a batch entry publishes a product together with a genuine typed
refusal"*, classed as a per-entry atomicity violation, DEFER to `BA0`, gated by
`a_genuinely_failing_batch_entry_publishes_no_partial_product`.

Three independent review seats returned `NOT-EVIDENCED` on item 6 for that row, because the cited
gating test drives the batch's DUPLICATE-CANONICAL CONFLICT — a different failure class, which
publishes no product at all, so the test is green and says nothing about the ratified claim. The
observation note already recorded that the driven evidence does not reproduce the row.

An independent source investigation then enumerated EVERY construction site of `CompileBatchEntry`
in `crates/verter_session/src/host_compile.rs`:

| # | line | origin | `errors` | `code` / `lang` / `source_map` | atomic? |
|---|---|---|---|---|---|
| 1 | 569 | Stage-D `group_errors` fan-out (duplicate-canonical conflict, upsert failure) | one string | hardcoded empty | YES |
| 2 | 689 | `compile_one_in_batch` precomputed-error short circuit | one string | hardcoded empty | YES |
| 3 | 755 | HostBacked `get_virtual_file` → `Ok(response)` | error-severity diagnostics of the response | `response.code` / `source_map` / `lang` | **NO** |
| 4 | 798 | HostBacked `Err(CompileError)` | non-empty by construction | hardcoded empty | YES |
| 5 | 812 | HostBacked `Err(other)` — including `RuntimeSurfaceRefused` | one formatted string | hardcoded empty | YES |
| 6 | 841 | RuntimeRender `Ok(render)` | hardcoded `Vec::new()` | product | YES |
| 7 | 869 | RuntimeRender `Err(CompileError)` | non-empty | hardcoded empty | YES |
| 8 | 883 | RuntimeRender `Err(other)` | one string | hardcoded empty | YES |
| 9 | 912 | `compile_panic_entry` | one string | hardcoded empty | YES |

Eight of the nine are atomic by hardcoded literal. Site 3 is the only construction that reads both
halves independently, and it is therefore the only shape that could express a product beside a
fatal-looking `errors` list.

The typed refusal itself does NOT reach site 3. `HostError::RuntimeSurfaceRefused`
(`crates/verter_session/src/types.rs:2472-2488`) is returned as `Err` by `get_virtual_file`
(`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1087-1108`) and lands on site 5,
which publishes no product. So the actual typed-refusal path is atomic.

The only known upstream producer of an `Ok` carrying error-severity diagnostics is the
`DevServeLastKnownGood` stale serve
(`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1755-1766`), which pairs the
PREVIOUS compile's outputs with the NEW compile's error diagnostics — and whose own source says it
is not a fresh refusal. Every invalidation route that could make an unchanged-profile compile newly
fail also clears the last-good slot (`host_upsert.rs:754-761`, `block_content.rs:1613-1618`,
`host_lifecycle.rs:530`/`:916`, `host_manage/analysis_io.rs:2044`/`:2092`), and a cross-file edit
dies with the warm hit because `peek_last_good` validates the same fact signature as `lookup`
(`cache_runtime/compile_output_node.rs:665` vs `:549`). Both existing tests that name the fallback
(`crates/verter_session/tests/cases/g_misc0/host_tests.rs:491-523`, `:910-943`) assert the request
ERRORED — the fallback did not fire.

Svelte typed refusals cannot reach the batch at all: `host_compile.rs:469-478` hardcodes
`file_language: FileLanguage::vue()` for every batch input, and the `RuntimeRender` lane never reads
the runtime-surface-refused flag (`virtual_file_pipeline.rs:3463-3467`). That carrier defect is the
separate ratified row `RT-1`, owner `BRT0`.

One residual is **UNKNOWN, not closed**: `lookup` acquires a store view unconditionally while
`peek_last_good` skips its validator when the fact rail is empty
(`cache_runtime/compile_output_node.rs:665` vs `:549`), and a self-contained SFC records zero facts.
Reaching the stale serve through that crack would additionally require a cold recompile of unchanged
bytes to fail, which only the transient macro-semantic outcomes could do. It was probed and not
reproduced.

## The independent ruling

An unprimed disposition consult (prompt: [`at2-disposition-prompt.md`](at2-disposition-prompt.md);
verbatim ruling: [`at2-disposition-ruling.md`](at2-disposition-ruling.md)) ruled:

- AT-2 **as ratified is not an evidenced genuine defect**; its factual claim is wrong.
- What exists is a **latent result-shape hazard at construction site 3**, with reachability
  unproven — it must not keep masquerading as an observed defect.
- Charter item 6 applies to "every genuine defect", so after a correct reclassification AT-2 creates
  no defect-specific item-6 regression obligation — but the charter's **separate atomicity exit**
  is still owed, as a green table-driven public regression over every DEMONSTRATED failure class on
  both lanes, with success, warning-only and stale-success controls.
- An `#[ignore]`d target that fails only because `RT-1` prevents Svelte classification would **not**
  be independently discriminating for atomicity — it would be a stub.
- **A track orchestrator may not change the finding, class, owner or gating test of a ratified row.**
  Its maximum is: record the evidence, mark item 6 `NOT-EVIDENCED`, file this memo, add
  non-production tests without mislabelling them as the ratified AT-2 gate, and stop acceptance.
- Under the current ratified bytes the atomicity suite plus this enumeration is **still
  `NOT-EVIDENCED`** for AT-2, and the unreproduced residual above is itself an open proof gap that
  the charter's own words refuse to count as exhaustion.

## What this block did in response

1. Built the atomicity regression the charter's atomicity exit calls for — table-driven over every
   DEMONSTRATED failing-entry class on both lanes, each proven to have entered its intended class,
   each asserted to publish no code, no map and no output language, with an unaffected neighbour,
   plus ordinary-success and warning-only controls so a diagnostic is never equated with a refusal.
2. Recorded the enumeration and the reachability argument above, including the unreproduced
   residual, stated as UNKNOWN rather than as a closure.
3. Changed NOTHING in the ratified findings table — no re-class, no re-owner, no substituted gating
   test — and added no production guard, refusal, withhold path, retraction or removal id.

## The act this memo asks for

Recommended by the ruling, reproduced verbatim so it can be issued as written:

> **AMEND AT-2: reject the ratified claim that a reachable batch entry currently publishes a product
> with a genuine typed refusal; reclassify AT-2 as a latent HostBacked result-outcome construction
> hazard with reachability unproven, retain DEFER to BA0, replace its acceptance with a structurally
> typed success/stale-success/failure boundary and discriminating conversion tests, and remove the
> requirement that a Svelte-refusal atomicity target be RED unless an independently reproduced mixed
> outcome is first demonstrated.**

The bytes that act would touch are `dispositions.md`'s ratified AT-2 row and
[`../../charters/BA0.md`](../../charters/BA0.md) lines 28 and 37, whose instruction that a
Svelte-refusal atomicity target be added and be RED rests on the same unsupported assumption. **This
block edited neither.** Both are left exactly as ratified, because a post-binding charter edit made
without an act is the precise governance defect that blocked this block once already.

Until that act exists, item 6 is `NOT-EVIDENCED` for AT-2 and this block is not
acceptance-recommendable.
