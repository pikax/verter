# Lane verdict — guard-discrimination

Reviewed tree: `b75140abfa7514f3644a1291ee8908d604e5e0c0` (worktree
`<worktree-root>/verter-j1`, branch `block/j1-integration`).

**Answer to the lane question: YES. Every guard planted went RED, with the expected
magnitude and the expected first-tripping category. No finding.**

---

## BLOCKING PROCESS ALERT — not a defect in the work under review, but it must not be lost

While this lane was running, another agent (`general-purpose`) ran `git add -A && git commit`
in this shared worktree while one of my mutations was live on disk. **Commit `9bd83025d2ca698d9afc6c67afd7f483e56ca5ea`,
subject `docs(arch): correct the count of moved legacy baselines`, contains a 6-line
PRODUCTION SOURCE mutation in `crates/verter_css_syntax/src/style_ir.rs` in addition to its
doc file.** The committed hunk is my out-of-order plant:

```rust
if self.selector_sink.is_some() {
    self.comment_spans.insert(0, Span::new(token.start, token.end));
} else {
    self.comment_spans.push(Span::new(token.start, token.end));
}
```

I detected this independently (`git show HEAD:crates/verter_css_syntax/src/style_ir.rs`
carried the plant; HEAD's blob is `c461e7043`, the correct blob is `a598808fb`) before the
peer's own notification arrived, and restored the file from the raw good blob rather than
from HEAD — `git checkout --` would have re-installed the plant while making the tree look
clean. I did NOT commit, reset, revert or stash. The peer stated it will remove the
contamination from the branch itself. **`block/j1-integration` must not be landed until
commit `9bd83025d` is cleaned.** I attempted to reply via SendMessage; the address
`general-purpose` was no longer reachable, so this paragraph is the channel.

This is not a defect in the integration and therefore takes no `FINDING` row. It is a
commit-path process gap (a blanket `git add -A` in a worktree with concurrent activity),
not a coverage gap: the mutation it swept in *is* guarded — see plant B below, which goes
RED. It reached a commit because no test was run between the `add` and the `commit`.

## Tree state at completion

| check | result |
|---|---|
| `crates/verter_css_syntax/src/style_ir.rs` | blob `a598808fb…` — **matches `b75140abf`** |
| `crates/verter_css_syntax/src/selector.rs` | blob `1e73bb457…` — **matches `b75140abf`** |
| `crates/verter_compiler/tests/allocator_canaries.rs` | blob `47283a99b…` — **matches `b75140abf`** |
| `git diff b75140abf --stat` (whole tree) | one file: the peer's `debt-J1-css-parse-allocation-ceiling-residual.md` — **none of mine** |
| `git status --short` | ` M crates/verter_css_syntax/src/style_ir.rs` |
| `git rev-parse HEAD` | `9bd83025d…` (moved by the peer, not by me) |

Those are the three files I planted in; all three are byte-identical to the reviewed sha.
The non-empty `git status` and the moved HEAD are **entirely** the peer's commit, and the
`M` line is the CORRECT state: the working tree holds the right content and HEAD holds the
plant. Resolving that `M` by checking out from HEAD would reinstall the mutation. Final
clean re-runs after all reverts: `verter_css_syntax` 207/207 passed, `verter_compiler
--test allocator_canaries` 14/14 passed (inner exit 0 both).

## Plant discipline applied

Every plant was proved present-and-unique in the pre-image before application, then proved
present, unique and NEW by a `git diff` showing exactly the intended hunk and nothing else,
before any run. No plant reported GREEN without that proof. Two plants were re-authored
after a first attempt failed to apply cleanly (plant 3 hit a use-after-move in the struct
literal and was rewritten to precompute the capacity; plant C was re-applied on a verified
clean base after the contamination was discovered) — in both cases the first attempt was
discarded and the run was taken only against the proven hunk.

---

## Part 1 — slice 2's allocation guards

All five briefed plants RED. Inner exit `100` (real test failure) on each.

| # | plant | site | result | evidence |
|---|---|---|---|---|
| 1 | outer selector-list transfer as `structure.list().clone()` | `style_ir.rs:982` | **RED** | `class_rules: … got 150 calls / 14000 bytes; 150 boxes would be 6000 bytes`; `ir_total` 408 → 558 |
| 2 | nested functional-pseudo list as `Some(Box::new(value.clone()))` | `selector.rs:895` | **RED** | `deep_rules: … got 200 calls / 16000 bytes; 200 boxes would be 8000 bytes`; `ir_total` 757 → 907 |
| 3 | per-rule `SelectorSink` token buffer pre-sized `source.text().len() / 32` | `selector.rs:774` | **RED** | `class_rules: per-rule requested bytes must not grow … 5733.9 at 50 rules -> 11379.4 at 400 rules, 1.985x` against the 1.10 bound |
| 4 | `PHASE_CAP = 2` | `allocator_canaries.rs:372` | **RED** | loud panic: `phase log overflowed PHASE_CAP=2: attribution columns would be silently truncated …` — a panic, not a silent truncation |
| 5 | suppress the diagnostic record inside the selector-sink window | `style_ir.rs:938` | **RED** | `dialects.rs:526` — `left: [ExpectedRuleBlock]`, `right: [UnterminatedBlock, ExpectedRuleBlock]` for `[ {}` |

### Plant 2 — the brief's highest-value concern, answered directly

The brief flagged that the `selector_clone_enter` / `_exit` bracket placement changed during
conflict resolution, and that a bracket no longer enclosing the whole transfer would let a
clone hide in the gap and report a false GREEN. It does not. Both by inspection
(`selector.rs:894-896` places `notify_parse_phase("selector_clone_enter")` immediately
before `nested_list = Some(Box::new(value));` and `_exit` immediately after, so the `Box::new`
allocation is INSIDE the bracket, as its own in-source comment says) and by the plant, which
was attributed to the `selector_clone` bucket at exactly the predicted 200 calls with
off-size bytes (16,000 against 8,000 admissible). The bucket's exact-bytes equality — not a
zero-call assertion — is what makes it discriminating, and it survived integration intact.

### Plant 5 — suppressed for real, not with the `if false` sham

Slice 2's own evidence records a plant that failed to apply because guarding a hoist as
`if false { } else if let … {}` leaves the `else if` arm live. I did not repeat it. The
mutation wraps the record itself:

```rust
if let ParseEvent::Diagnostic(diagnostic) = event {
    if self.selector_sink.is_none() {          // <-- planted
        self.diagnostics.push(diagnostic);
    }
    …
```

`.a[ {}` lost its `UnterminatedBlock` exactly as predicted. Note the failure tripped on the
`CssDialect::Css` assertion at `dialects.rs:526`, BEFORE reaching the per-dialect loop —
which independently confirms the evidence doc's fourth honest control (that the per-dialect
loop is layout-path coverage, not an independent discriminator).

### Extra plant, not briefed — conservation against a genuinely dropped marker (GREEN, but not a finding)

The conservation assertion's in-source comment claims it catches "a dropped marker, a
mis-folded bracket". I tested the first half directly by deleting
`notify_parse_phase("after_parse_emit")` from `parse_style_ir`. **It went GREEN** (14/14
passed). The reason is structural and benign: `delta()` sums every logged record by name,
and the four summed buckets are the only names the probe ever emits, so a dropped marker
merely re-attributes allocations between buckets (measured: `class_rules` parse_emit
406 → 403, finish 0 → 3) while the sum stays equal to the total. The assertion is a TOTAL
conservation check — which its own message states accurately ("phase columns must sum to
the parse total") — and it does discriminate the overflow-truncation class the evidence
claims for it (plant 4, RED, loud). This is a slight overstatement in a code comment, is
identical on the unintegrated donor branch, and is therefore explicitly out of scope for a
row per the brief. Recording it so the next reader is not misled by that comment.

### Honest controls — verified unchanged, no promotion or demotion

All three assertions the evidence doc names as non-discriminating are still labelled
`CONTROL.` in the integrated `crates/verter_compiler/tests/allocator_canaries.rs`, with the
doc's own reasoning preserved verbatim in the comments:

- `admission.calls <= 4` / `admission.bytes >= css.len()` (lines 682-696) — "Both bound
  `Arc::<str>::from(&str)` — a std behaviour, not a verter one."
- `ir.total.calls > admission.calls * 10` (lines 705-715) — "it cannot fail while they pass."
- `ir.total.calls > parser_noop.calls` (lines 716-727) — "`parser_noop` measures zero in
  every category … so this reduces to `> 0`."

The fourth control (the per-dialect loop in
`diagnostics_raised_inside_a_selector_list_still_reach_the_style_ir`) is still framed as
path coverage in `dialects.rs:540-543`, and plant 5 empirically confirmed the Css assertion
trips first, as the doc claims.

Nothing was promoted or demoted. The genuinely load-bearing assertions —
`source_wrap.calls == 0`, `ir.total.calls > 50`, the `selector_clone` exact-bytes equality,
the four-bucket conservation sum, and the 1.10x per-rule growth bound — carry no `CONTROL`
label and each was independently proven RED above (plants 1-4). The control claims also
remain factually true in the integrated tree: `parser_noop` measures `0/0` in all eleven
categories, and `admission` measures exactly `1` call per category.

---

## Part 2 — the fix's new regression tests, re-proved independently

| # | plant | result |
|---|---|---|
| A | revert the mechanism: derive `comment_spans` + `unpaired_cdo_span` from `self.tokens` in `finish()`, tests untouched | **RED — all six required tests** |
| B | out-of-order append: `insert(0, …)` for a prelude comment instead of `push` | **RED** |
| C | double-observation: observe the token in both the hoisted position and the main `ParseEvent::Token` arm | **RED** |

### A — every new prelude test discriminates, and so does the Svelte compat test

Ran with `--no-fail-fast` specifically so no required RED could be masked by an earlier
failure. 6 failed / 201 passed:

- `cases::comment_spans::comment_spans_in_finds_a_comment_inside_a_selector_prelude` — RED
- `cases::comment_spans::comment_spans_in_finds_a_comment_inside_a_functional_pseudo_prelude` — RED
- `cases::comment_spans::comment_spans_stay_in_source_order_across_prelude_and_block` — RED
- `cases::comment_spans::unpaired_cdo_span_reports_a_cdo_opened_inside_a_selector_prelude` — RED
- `cases::comment_spans::unpaired_cdo_span_is_cleared_by_a_cdc_inside_a_selector_prelude` — RED
- `cases::svelte_compat_profile::html_comment_in_body_is_clean` — RED

Every test the brief required to go RED went RED. **None stayed green.** The four
pre-existing block-position `comment_spans` tests correctly stayed GREEN, which is the
right discrimination boundary — it confirms the new tests cover a position the old ones
genuinely did not, rather than merely duplicating them.

### B — the sortedness `partition_point` depends on IS guarded

`comment_spans_stay_in_source_order_across_prelude_and_block` failed with
`left: ["/* b */", "/* a */", "/* c */", "/* d */"]` vs
`right: ["/* a */", "/* b */", "/* c */", "/* d */"]`. The brief asked whether the
sortedness assumption behind `comment_spans_in`'s `partition_point` is unguarded; it is
not. Note the peer's suggestion that the accidental commit of this exact mutation is
evidence the sortedness is unguarded — it is not: the guard exists and fires. The mutation
reached a commit because `git add -A` ran with no test between add and commit, which is a
commit-path process gap, addressed in the alert above.

### C — a double-count is caught

`comment_spans_in_finds_a_comment_fully_contained_in_range` failed with `left: 2, right: 1`;
`comment_spans_in_finds_every_comment_in_source_order` and
`comment_spans_stay_in_source_order_across_prelude_and_block` also went RED. The fix's
"observe exactly once at event time" property is guarded, not merely asserted in a comment.
(The plant necessarily double-counts only non-selector tokens, since selector-window events
return before reaching the main match arm — so the RED comes from a declaration-block
comment, which is the strictest available witness.)

---

## Body notes — out of scope for a row, recorded for the next reader

**1. The evidence table in `intra-parser-allocation.md` is stale against the integrated tree.**
Slice 1's independent reductions moved the measured columns; the doc's "Isolated split" table
still carries the donor-branch numbers. Measured here at `b75140abf`:

| category | doc (`IR total` calls/bytes) | integrated | doc `parse_emit` | integrated |
|---|---|---|---|---|
| class_rules | 458 / 277,232 | **408 / 251,296** | 456 / 259,712 | **406 / 233,104** |
| deep_rules | 807 / 472,456 | **757 / 420,392** | 755 / 456,288 | **705 / 403,680** |
| selector_lists | 807 / 487,140 | **657 / 395,748** | 805 / 465,888 | **655 / 373,680** |

The pattern is a uniform ~1 fewer call per rule and fewer bytes — i.e. the integration is
BETTER than the donor record, and the "Before / after" and "ESCALATION" ratio tables
downstream of it are correspondingly stale in the favourable direction. The `list transfer`
column is the one that still matches exactly (deep/slotted/global 50 / 2,000; mixed_vue
33 / 1,320), which is itself reassuring for the bracket. This concerns the allocation-ceiling
residual, which the brief explicitly places outside this lane, so it takes no row — but the
doc should be re-measured before anyone reads its escalation ratios as current.

**2. Verified there is no second CSS parse path or shim.** Nothing encountered in
`style_ir.rs`, `selector.rs` or `svelte_compat.rs` re-lexes or re-scans source to recover a
fact: `comment_spans` / `unpaired_cdo_span` are minted from the parse's own token stream at
event time and read back through `comment_spans_in`'s binary search, and
`svelte_reject_from_ir` consumes `ir.unpaired_cdo_span()` rather than re-scanning for the
`<!--`/`-->` pairing. The fix moved the derivation EARLIER (event time) rather than adding a
second pass, which is the correct direction under the single-CSS-authority rule.

## Commands run

All through `rust-lock.sh j1s2-plant --`, strictly one at
a time, `--test-threads 4 --build-jobs 4` (never `-j` together with `--test-threads`). Inner
exit status read on every run: `0` on the two baselines and the two final clean re-runs,
`100` (genuine test failure) on every RED plant. **No run returned 123/124/125/127**; no
killed or OOM'd command occurred and nothing was retried.

===VERTER-RECEIPT-BEGIN===
LANE: guard-discrimination
RESULT: PASS
REVIEWED: b75140abfa7514f3644a1291ee8908d604e5e0c0
FINDINGS: none
===VERTER-RECEIPT-END===
