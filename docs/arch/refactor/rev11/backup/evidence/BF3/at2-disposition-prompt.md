# Disposition consult — a ratified defect row whose failure class may have no reachable instance

You are an independent architecture/governance decider for the Verter repository
(the checkout root; every path below is repository-relative). Answer on the merits. If any premise below is
wrong, say so — "your premise is wrong" is a welcome answer. Do not tell me what I want to hear.
You have read-only access; verify anything you doubt by opening the files.

## The situation, stated as verified facts

An audit block recorded thirteen findings. Each genuine defect carries (a) a `#[ignore]`d
conformance target asserting the CORRECT behaviour, which fails today and is the correction owner's
acceptance gate, and (b) a green characterization test pinning the exact current divergence.

One row, `AT-2`, reads:

> **finding:** a batch entry publishes a product together with a genuine typed refusal
> **class:** per-entry atomicity violation
> **disposition:** DEFER, owner `BA0`
> **gating test:** `a_genuinely_failing_batch_entry_publishes_no_partial_product`

That row is RATIFIED — it appears in a maintainer-ratified findings table
(`docs/arch/refactor/rev11/evidence/BF3/dispositions.md`, the "Ratified rows" table), and the
block's own record states the row's "class and owner are left exactly as ratified rather than
quietly re-classed".

Three independent external review seats each returned `NOT-EVIDENCED` on the governing charter
requirement (`docs/arch/refactor/rev11/charters/BF3.md`, owned-scope item 6: *"Add a precise
independently discriminating regression for every genuine defect"*), because the cited gating test
measures a DIFFERENT failure class: it drives two batch inputs naming the same canonical with
conflicting sources — the batch's own duplicate-canonical conflict — which publishes no product at
all, so the test is green and says nothing about the ratified claim.

A fresh, independent source investigation has now enumerated every construction site of
`CompileBatchEntry` in `crates/verter_session/src/host_compile.rs`:

| # | line | origin | `errors` | `code`/`lang`/`source_map` | atomic? |
|---|---|---|---|---|---|
| 1 | 569 | Stage-D `group_errors` fan-out (dup-canonical conflict, upsert failure) | `vec![err]` | hardcoded empty | YES |
| 2 | 689 | `compile_one_in_batch` precomputed-error short circuit | `vec![err]` | hardcoded empty | YES |
| 3 | 755 | HostBacked `get_virtual_file` → `Ok(response)` | filtered from `response.diagnostics` by `severity == Error` | `response.code` / `source_map` / `lang` | **NO — the only non-atomic site** |
| 4 | 798 | HostBacked `Err(CompileError)` | non-empty by construction | hardcoded empty | YES |
| 5 | 812 | HostBacked `Err(other HostError)` incl. `RuntimeSurfaceRefused` | one formatted string | hardcoded empty | YES |
| 6 | 841 | RuntimeRender `Ok(render)` | **hardcoded `Vec::new()`** | product | YES |
| 7 | 869 | RuntimeRender `Err(CompileError)` | non-empty | hardcoded empty | YES |
| 8 | 883 | RuntimeRender `Err(other)` | one string | hardcoded empty | YES |
| 9 | 912 | `compile_panic_entry` | one string | hardcoded empty | YES |

Further verified facts:

- The only way site 3 can yield `Ok` WITH an error-severity diagnostic is the
  `DevServeLastKnownGood` stale-serve arm in
  `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:1755-1766`, which pairs the
  previous compile's outputs with the NEW compile's error diagnostics. Its precondition is a
  surviving `fallback_last_good` slot; every invalidation route that could make an unchanged-profile
  compile newly fail also clears that slot (`host_upsert.rs:754-761`, `block_content.rs:1613-1618`,
  `host_lifecycle.rs:530`/`:916`, `host_manage/analysis_io.rs:2044`/`:2092`), and a cross-file
  dependency edit dies with the warm hit because `peek_last_good` validates the same fact signature
  as `lookup` (`cache_runtime/compile_output_node.rs:665` vs `:549`).
- The two existing tests that name that fallback (`crates/verter_session/tests/cases/g_misc0/host_tests.rs:491-523`
  and `:910-943`) both assert `result.is_err()` — the fallback did not fire.
- A residual crack was identified but NOT demonstrated: `lookup` acquires a store view
  unconditionally while `peek_last_good` skips its validator when the fact rail is empty, and a
  self-contained SFC records zero facts. Reaching the stale serve would additionally need a cold
  recompile of unchanged bytes to fail (only the transient macro-semantic outcomes could do that).
  Status: UNKNOWN, not reproduced.
- Svelte typed refusals cannot reach the batch at all: `host_compile.rs:469-478` hardcodes
  `file_language: FileLanguage::vue()` for every batch input, and the `RuntimeRender` lane never
  reads the runtime-surface-refused flag (`virtual_file_pipeline.rs:3463-3467`). That carrier
  defect is a SEPARATE ratified row (`RT-1`, owner `BRT0`).
- Vue carrier error recovery does emit a module, but the Vue bridge emits no error-severity runtime
  diagnostic at all (`crates/verter_compiler/src/framework_common/vue_bridge.rs:1379`), so site 3's
  severity filter yields an empty `errors` list for it.
- `CompileBatchEntry.errors` is `Vec<String>`; there is no typed-refusal variant, no diagnostic-code
  field. `HostError::RuntimeSurfaceRefused`'s machine code survives only as a substring of a
  formatted message.

So: **no deterministically reproducible batch input is known that yields a non-empty `errors` list
together with a published product.** The claimed failure class appears to have no reachable
instance today, while the type-level hazard at site 3 is real and unguarded.

## Constraints that bind any answer

1. The audit block that owns this row implements NO compiler, session, route, transport, CSS or
   conformance CORRECTION. It may add tests and evidence text only. Production behaviour changes,
   including guards and refusal paths, are forbidden to it.
2. A standing maintainer rule: a wrong output is a BUG to fix test-first, never a production guard,
   refusal, tracking artifact, or known-divergence list consumed by production code.
3. The repository forbids stub tests: an empty body, an unconditional pass, an always-true
   assertion, or a test that would not catch the defect it names is a gate bypass, not a pass.
4. The charter's own words: *"`UNPROVEN` records an open proof gap and cannot count as
   exhaustion."*
5. The repository's disposition rule requires every scope-deviating correctness finding to be
   recorded as `ADOPT-NOW`, `DEFER` (with a debt row naming owner, resolution gate, acceptance
   ID/test) or `REJECT` — never a TODO.
6. Landed enforcement must be structural (compiler/type-system/tool-based), never a name-keyed
   source-tree scanner.

## What I am asking you to decide

**What is the correct disposition of `AT-2`, and what exactly must be built to satisfy charter item
6 ("a precise independently discriminating regression for every genuine defect") for it?**

Answer these explicitly:

1. Given the enumeration above, is `AT-2` — as its ratified text states it — a GENUINE defect with a
   reachable instance, a genuine LATENT construction-site hazard with no reachable instance, or not
   a defect at all? Cite what settles it.
2. Does charter item 6 oblige a regression for `AT-2` at all under your answer to (1), and if so,
   what is the *precise* thing the regression must discriminate? Describe the test that would
   satisfy it, in terms of what it drives and what it asserts, such that the test would FAIL if the
   defect were introduced and PASS otherwise.
3. Would an `#[ignore]`d correct-behaviour acceptance target be legitimate here, or would it be a
   stub (since no input drives it to its assertion)? If a test can only reach the hazard through a
   `#[cfg(test)]` injection seam that does not yet exist in `crates/verter_session/src/host_compile.rs`
   or `virtual_file_pipeline.rs`, is adding such a seam inside the audit block's zero-production-change
   ceiling, or is it a scope breach that must be routed to the correction owner?
4. Is changing the recorded CLASS, OWNER, or gating test of a ratified findings row within a track
   orchestrator's authority, or does it require a maintainer act? If a track orchestrator may not
   change it, what is the maximum it MAY do, and what exactly must be escalated?
5. Is it acceptable for the block to close item 6 by (a) building an exhaustive per-entry atomicity
   regression that drives EVERY reachable class of failing batch entry on BOTH lanes and asserts the
   entry publishes no code, map or output language while its neighbour is unaffected, PLUS (b)
   recording the enumeration above with the single non-atomic construction site named and its
   unreachability argument stated — or is that still `NOT-EVIDENCED` under the charter's words?

Be concrete and decisive. Give the ruling, the reasoning, and the exact acceptance bar. If your
ruling requires a maintainer act, say precisely what act, in one sentence a maintainer could issue
verbatim.
