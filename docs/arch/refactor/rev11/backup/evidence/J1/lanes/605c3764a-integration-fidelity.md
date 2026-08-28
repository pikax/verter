# Lane: integration-fidelity — `605c3764a9fc25dc2b0008a18e8674686d4f2783`

Worktree `<worktree-root>/verter-j1`, branch `block/j1-integration`, clean tree at
the reviewed SHA. Read-only git and file reads only. **No `cargo`, `nextest`, `check`, `clippy`, `fmt`,
`build`, `pnpm` or test command was run** — the memory hold was honoured and the lane did not need a
compiler to answer its question.

## Verdict on the ONE question

**The integration did not change behaviour relative to either slice in isolation.** Every byte of the
composed tree is attributable to one side or the other; the one genuine interaction site preserves both
sides' intent exactly; no commit was lost or altered.

The single behavioural regression in this tree (`html_comment_in_body_is_clean`) is the already-known,
already-owned one, produced by git's *clean* auto-merge — not by a conflict resolution. Per the brief it
is not re-filed here. I re-derived its consequence set independently (below) and found no third starved
fact, so no additional row.

---

## 1. Non-conflicted files are untouched

File sets derived independently:

* `git diff --name-only 30ca1c557 ddffe3d7e` → 35 paths (slice 1)
* `git diff --name-only 30ca1c557 de2021491` → 12 paths (slice 2)
* Intersection = **exactly** the five named conflicted files, nothing more, nothing less.
* Slice-1-only = 30 paths; slice-2-only = 7 paths.

Every one was diffed against its owning side at the integration SHA:

* 7/7 slice-2-only paths: `git diff 605c3764a de2021491 -- <f>` **empty**.
* 30/30 slice-1-only paths: `git diff 605c3764a ddffe3d7e -- <f>` **empty**.

Add/delete operations survived too — `git diff --name-status 30ca1c557 605c3764a` carries slice 1's three
deletions (`svelte/runtime/css/{match_writeback.rs,parse.rs,parse_tests.rs}`, confirmed absent on disk) and
all four of its file additions, plus slice 2's two doc additions. Module wiring is complete on both sides:
`lib.rs` declares `mod layout;` (slice 2's module) and `tests/cases/mod.rs` declares slice 1's four new test
modules alongside slice 2's pre-existing ones. No conflict markers anywhere in `crates/` or `docs/`. No
duplicate top-level items in any resolved file (the one `notify_parse_phase` pair is slice 2's intentional
`cfg`-gated pair).

Set arithmetic turned up **one** path in the integration that is in neither slice's delta —
`docs/arch/refactor/rev11/evidence/J1/debt-J1-css-parse-allocation-ceiling-residual.md`. See §4.

## 2. The five resolutions, read against both originals

| file | vs slice 2 | vs slice 1 | resolution reads as |
|---|---|---|---|
| `tests/allocator_canaries.rs` | +75 / −0 | +503 / −4 | slice 2's byte-counting allocator + intra-parser module, plus slice 1's `svelte_css_analysis_fact_reread_allocation_probe` module verbatim. The 4 deletions vs slice 1 are exactly slice 2's `increment_alloc_counter()` → `increment_alloc_counter(bytes)` signature change and its 3 call sites. |
| `css_syntax/src/lib.rs` | +12 / −5 | +2 / −0 | a clean union of the export lists: slice 1's `CompoundTail`, `SvelteNthArg`, the `svelte_compat` re-export block and `parse_style_ir_thread_invocations`, plus slice 2's `set_style_ir_parse_phase_probe`. Both `cfg`-gates preserved. |
| `css_syntax/src/selector.rs` | +316 / −5 | +30 / −15 | see §3. The 5 deletions vs slice 2 are slice 1 expanding two expressions into block form (`SelectorAttribute` literal; `argument_span` hoisted to a local); the 15 vs slice 1 are slice 2's move-instead-of-clone rewrite. |
| `css_syntax/src/style_ir.rs` | +98 / −3 | +87 / −20 | slice 1's `prelude_text`/`comment_spans`/`unpaired_cdo_span`/`Clone`/invocation counter on top of slice 2's `with_entry_point` pre-sizing, diagnostic hoist, `event()` restructure and phase probe. The 3 deletions vs slice 2 are slice 1 hoisting the inline `opaque_args:` initialiser into a local so `prelude_text` can be derived from its span. |
| `css_syntax/tests/cases/selectors.rs` | +64 / −0 | +119 / −0 | pure superset of both sides — **zero** deletion lines in either direction. No duplicate `fn` names. |

Every difference in every direction is the other side's independent content. **No line in any of the five
files is attributable to neither donor.**

## 3. The one genuine interaction — `SelectorPseudo` construction (`selector.rs:878-984`)

Both halves survived. Verified line by line at the integration SHA:

* **Slice 2's clone is gone.** The `open.children.iter().find_map(|child| … Box::new(value.clone()))`
  extraction is absent. `grep 'clone()' selector.rs` returns exactly one hit — `SelectorSink::new(source.clone())`
  at line 1129, an unrelated `Arc` clone. `nested_components` likewise now `push(component)` by move instead
  of `.clone()`.
* **The move is bracketed correctly.** Inside the consuming `for child in open.children` loop:
  `notify_parse_phase("selector_clone_enter"); nested_list = Some(Box::new(value)); notify_parse_phase("selector_clone_exit");`
  — `Box::new` is **inside** the bracket, which is the whole point of slice 2's marker placement (its
  exact-transfer-cost canary asserts the bucket's bytes equal `calls * size_of::<SelectorList>()`, which a
  clone hidden between exit and box would defeat). The second bracketed transfer in `style_ir.rs:965-967`
  (`rule.selector_list = Some(structure.into_list())`) is intact too, with `SelectorStructure::into_list`
  present and `list()` retained for its other callers.
* **Slice 1's call is intact and receives the same list.**
  `classify_argument_is_empty(&self.source, argument_span, nested_list.as_deref())` — the `as_deref()`
  target changed name from `selector_list` to `nested_list` but is the same value: slice 1's `find_map`
  took the **first** `BuiltSelectorNode::List` child; slice 2's loop guard `if nested_list.is_none()` also
  takes the first and drops the rest via `_ => {}`. Identical selection.
* **Slice 1's fields survive.** `svelte_nth_arg` and `argument_is_empty` are both constructed and both
  present in the `SelectorPseudo` literal, alongside `selector_list: nested_list`.
* **No cross-contamination of the measured bucket.** `classify_svelte_nth_arg` and
  `classify_argument_is_empty` are called *outside* the `selector_clone_*` bracket, so slice 1's new work
  does not land in slice 2's exact-cost bucket. This was the sharpest way the resolution could have
  silently broken slice 2's canary, and it is clean.
* `argument_span = pseudo_argument_span(tokens, span)` derives from `tokens`, not from the now-moved
  `open.children`, so slice 1's fields are unaffected by the move. The only other `open.children` use
  (`selector.rs:1034`) is in a different match arm.

## 4. Commits

`git log --oneline ddffe3d7e..605c3764a` holds **18** commits, not 17. Seventeen of them are the donor's,
1:1 in the same order — a `diff` of the two subject lists is empty. Linear replay: zero merge commits,
`ddffe3d7e` is an ancestor.

Per-commit `git patch-id --stable` comparison against the donor:

* **12 of 17 patch-identical.**
* **5 differ** — `885da9082`, `9c15347cb`, `6be4b82c5`, `506ce9cad`, `fd2caac27` — and for every one of
  them the patch restricted to files *outside* the five-file conflict set is **patch-identical** to the
  donor's. Divergence is confined to the conflicted files, which is what a rebase resolution is.
* One file-set difference: donor `d8f4b2810` also touched `lib.rs`; its replay `506ce9cad` does not. That
  hunk was a pure re-ordering of the `set_style_ir_parse_phase_probe` export above the
  `pub use style_ir::{…}` block. The resolution had already placed it there, so the hunk became a no-op and
  rebase dropped it. **Content is present** (`lib.rs:46-47`) — nothing lost.

**The 18th commit is a divergence, disclosed here, and is not a defect.** `605c3764a docs(arch): defer the
CSS parse-allocation ceiling residual` adds one file, 148 lines, `docs/…/evidence/J1/debt-J1-css-parse-allocation-ceiling-residual.md`,
and touches no code. It is the `DEFER` debt row CLAUDE.md's "Explicit finding disposition" requires for the
escalation slice 2 raised in its own final commit; the document itself states the residual is inherited and
that the integration "neither improves nor worsens" it. It cannot change behaviour relative to either slice.
I report it because the brief asked for any divergence from "exactly 17", not because it is wrong.

## 5. Consequences of the known root cause — independent enumeration

I re-derived the consumer set of `StyleSyntaxIrSink::self.tokens` rather than trusting the handed summary.
There are exactly three, and **no fourth starved fact exists**, so no additional row:

1. `finish()` → `comment_spans` (slice 1). **Already named** in the brief.
2. `finish()` → `unpaired_cdo_span` (slice 1). **Already named** — the failing test.
3. `close_frame()` → `&self.tokens[frame.token_start..]` (pre-existing, slice-2-internal). Used only for
   `Function` / `ComponentValueBlock` / `Interpolation` frames, and only to read the window's first/last
   token. Slice 2's early return starves the window and suppresses the frame that would read it *together*,
   so indices stay consistent. Not a new starved fact, and identical on the unintegrated donor.

**Blast-radius detail worth attaching to the existing finding (not a new one).** The brief calls the
`comment_spans` half "a second, untested consequence"; it has a concrete production consumer. `render.rs:911`
(`Renderer::escape_comment_close`) walks `self.tree.comment_spans_in(span)` where `span` is the **whole
wrapped rule**, prelude included, and escapes each `*/` so the wrapping `/* … */` survives. A comment inside
a selector prelude is now missing from that inventory, so its `*/` is not escaped and the wrapping comment
terminates early — malformed emitted CSS, not merely a missing span. That raises the known finding's
severity; it does not add a finding.

## Body-only observations (deliberately not rows)

* **Slice 2's allocation evidence memos are now measured against a tree that no longer exists.**
  `allocation-phase-attribution.md` / `intra-parser-allocation.md` record phase splits taken before slice 1
  added a per-at-rule `Arc<str>` prelude decode (lands in the `parse_emit` bucket) and a per-parse
  comment-span `Vec` collect (lands in `finish`). The canaries' *structural* assertions still hold by
  construction — phase conservation is a sum over buckets that both additions fall inside, and the exact
  `selector_clone` cost is untouched because slice 1 adds nothing between the markers — but the recorded
  percentages are stale. Documentation accuracy, not behaviour, and only observable after composition.
* **Residual risk this lane cannot discharge.** Whether slice 1's new allocations perturb slice 2's
  numeric envelopes (the 400x bytes-per-source-byte bound, the 4x/11.5x quadratic discriminator) is a
  measurement question. Structurally I see no path to a break — slice 1's cost is O(at-rules) and
  O(comments), both zero in slice 2's generators — but this is exactly the separate build/test lane's job
  and its absence here is by design, not a gap.
* **Pre-existing, equally wrong on the donor:** `style_ir.rs:457-465` — the doc comment for
  `comment_spans_in` is attached to `unpaired_cdo_span` instead, leaving `comment_spans_in` undocumented
  and `unpaired_cdo_span` carrying two unrelated doc paragraphs. Byte-identical on `ddffe3d7e`; a slice-1
  authoring slip, not an integration artifact.
* No `lightningcss`, second CSS parse path, shim, or deferred-gap-to-consumer was introduced anywhere in
  this integration. `verter_css_syntax` remains the single CSS authority.

## Method / limits

Pure git composition analysis: independent file-set derivation, per-file byte-identity proof against the
owning side, both-directions diffs of all five resolutions, source reading of the interaction site and of
every `self.tokens` consumer, per-commit `patch-id` comparison with conflict-set exclusion, and structural
checks for conflict markers, duplicate items, and module wiring. **Nothing was compiled, built, or executed.**

===VERTER-RECEIPT-BEGIN===
LANE: integration-fidelity
RESULT: PASS
REVIEWED: 605c3764a9fc25dc2b0008a18e8674686d4f2783
FINDINGS: none
===VERTER-RECEIPT-END===
