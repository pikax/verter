# Lane `fix-review` — verdict on `b75140abfa7514f3644a1291ee8908d604e5e0c0`

Worktree: `<worktree-root>/verter-j1`, branch `block/j1-integration`, HEAD
confirmed `b75140abfa7514f3644a1291ee8908d604e5e0c0`, working tree clean. No file was modified, no
commit made, and **no build/test/lint command of any kind was run** — the lane was pure `git`,
`grep`, `sed`. Every conclusion below is derived from source reading.

Delta under review: exactly two commits.

- `dd20f5c31e` — `fix(core): derive whole-source CSS token facts at event time`
- `b75140abfa` — `docs(arch): re-measure the CSS parse-allocation residual on the composed tree`

Four files, +164/−18: `crates/verter_css_syntax/src/style_ir.rs`,
`crates/verter_css_syntax/tests/cases/comment_spans.rs`,
`crates/verter_compiler/src/svelte/runtime/css/render_tests.rs`, and the debt evidence document.

---

## Q1 — Does the fix implement the endorsed direction? **Yes.**

`StyleSyntaxIrSink` gains two fields (`comment_spans: Vec<Span>`, `unpaired_cdo_span: Option<Span>`),
the whole derivation loop is deleted from `finish()` (which now just moves the two accumulated
fields into the IR), and a single `if let ParseEvent::Token(token) = event { … }` block is inserted
in `event()` at `style_ir.rs:947-961`, **above** the selector-sink branch. That is the mandated
"derive incrementally at event time" mechanism, and nothing else.

I checked explicitly for each forbidden alternative, and none is present:

- **No selector tokens pushed back into `self.tokens`.** The `ParseEvent::Token` arm of the
  non-selector `match` (`style_ir.rs:1017`) is byte-identical to before; the selector branch still
  ends in the same `return Ok(())` at `style_ir.rs:989` without touching `tokens`. `token_start`
  frame windows are unchanged, so the storage the allocation work removed stays removed.
- **No re-lex.** No new `Lexer`, `CssSource::slice`, or byte scan anywhere in the delta.
- **No second parse path, no shim, no fallback.** The diff touches no dispatch, no consumer, and
  adds no alternate branch. The single consumer, `escape_comment_close`
  (`crates/verter_compiler/src/svelte/runtime/css/render.rs:911`), is untouched — the gap was
  implemented inside `verter_css_syntax`, which is what the standing "single CSS authority" rule
  requires.
- **No derivation moved to a consumer.** `comment_spans_in` (`style_ir.rs:467`) is unchanged, and
  `grep` confirms it and `unpaired_cdo_span()` have exactly one production consumer each
  (`render.rs:911`, `svelte_compat.rs:80`), neither modified.

## Q2 — Is each token observed exactly once? **Yes.**

Three call paths reach `StyleSyntaxIrSink::event`, and none can re-enter it:

1. **The recursive-descent parser.** `ParseEvent::Token` is minted in exactly one place —
   `Parser::bump` (`parser.rs:359` and `parser.rs:362`, the two arms of one `if`, so one emit per
   bumped token). `bump` drains a single-slot `lookahead` or pulls the lexer forward; there is no
   rewind, no checkpoint, no replay. `emit` forwards to `sink.event` once
   (`parser.rs:268`).
2. **The layout (indentation-dialect) parser**, reachable from `parse_style_ir` via
   `Parser::parse` → `layout::parse_layout` (`parser.rs:186-188`). It pre-lexes, then walks a
   monotonically advancing `self.cursor`, emitting each index once; sub-ranges are handed to
   `replay_subparse*`, which re-lexes that subspan into a private `VecSink` (`layout.rs:857`) and
   replays through the same one-per-event `emit`. Emitted ranges do not overlap: the only selector
   emission, `emit_selector_range(start_index, header_end)` (`layout.rs:247`), is immediately
   followed by `self.cursor = header_end`.
3. **A test tee sink**, `Peers` in `crates/verter_css_syntax/tests/cases/ir.rs:20-23`, which
   forwards each event to the CST sink and the IR sink once apiece.

The inner `sink.event(event)` at `style_ir.rs:966` and `:997` is **not** re-entry: `SelectorSink`
(`selector.rs:766`) is a distinct struct holding only `source`/`open`/`tokens`/`list`, with no
reference back to `StyleSyntaxIrSink`; its own `event` impl (`selector.rs:1041`) only pushes to its
private buffers. So a selector-region token is observed once by the new block and then forwarded —
not observed twice.

## Q3 — Is `comment_spans` still sorted by start? **Yes.**

`comment_spans_in` (`style_ir.rs:467-475`) does `partition_point(|c| c.start < range.start)` then
`take_while(|c| c.end <= range.end)`, so it needs ascending, non-overlapping spans. Push order is
event order, and event order is strictly ascending in source position on every path above (lexer
forward-only; layout cursor forward-only with non-overlapping replayed subspans whose `CssSource`
carries the absolute `start_pos` origin, so replayed spans stay absolute and in place). Selector
tokens are emitted at their true source position — a prelude comment is bumped by
`consume_selector_trivia` (`parser.rs:1258-1260`) at the moment the parser reaches it, i.e. before
the rule's `{` and before any block token — so a prelude comment now lands *between* the preceding
and following comments rather than being appended late. This is exactly what the new test at
`comment_spans.rs:88-104` pins.

Note the `take_while` remains sound with the newly added entries: for sorted non-overlapping spans,
`c[i].end > range.end` implies `c[i+1].start >= c[i].end > range.end`, so nothing in range is
skipped by the early stop.

## Q4 — Every construction path covered? **Yes, structurally.**

`StyleSyntaxIrSink::new` (`style_ir.rs:641-643`) is a one-line delegation to
`with_entry_point`, which is the sole struct-literal site (`style_ir.rs:655-670`) and is where the
two new fields are initialised. `parse_component_value_tree` (`style_ir.rs:1124`) therefore gets
the identical sink. (Rust would in any case reject a second literal missing the new fields, and
there is no `..Default::default()` anywhere in the file.)

## Q5 — Are the new tests discriminating? **All six are.** Per test:

Pre-fix behaviour I am reasoning against: `finish()` derived both facts from `self.tokens`, and the
selector branch early-returns at `style_ir.rs:989` before the `tokens.push`, so **no** token between
`StartNode{SelectorList}` and its matching finish ever reached that vector.

1. `comment_spans.rs:63` `comment_spans_in_finds_a_comment_inside_a_selector_prelude` —
   `.a /* note */ .b { color: red; }`. `Comment` is trivia (`token.rs:88`), and inside
   `parse_selector_list` trivia is bumped by `consume_selector_trivia` **inside** the `SelectorList`
   node. Pre-fix the inventory is empty, so `assert_eq!(texts, vec!["/* note */"])` fails. **Would
   have failed before.**
2. `comment_spans.rs:79` `…_inside_a_functional_pseudo_prelude` — `:is(.a /* n */ , .b) { … }`. The
   pseudo is parsed by `parse_selector_pseudo` from inside the selector list, so a nested
   `SelectorList` only bumps the sink's depth counter; the comment is still inside the outer
   selector region. Pre-fix empty → **fails before.**
3. `comment_spans.rs:88` `comment_spans_stay_in_source_order_across_prelude_and_block` —
   `/* a */ .x /* b */ .y { /* c */ … } /* d */`. Pre-fix the vector is `[a, c, d]`, so the
   `assert_eq!` on the four-element vector fails. This is the test that actually pins ordering, not
   just presence; the accompanying `windows(2)` sortedness assertion is a second, weaker guard.
   **Fails before.**
4. `comment_spans.rs:108` `unpaired_cdo_span_is_cleared_by_a_cdc_inside_a_selector_prelude` —
   `<!-- c --> .a { color: red; }`. `Cdo` is bumped at rule-list level (`parser.rs:439-440`) so it is
   observed pre-fix; `c` then opens a qualified rule, and `-->` falls to the catch-all
   `_ => self.bump(sink)?` inside `parse_selector_list` (`parser.rs:1148-1151`), i.e. inside the
   selector region. Pre-fix the CDC is invisible, so `unpaired_cdo_span()` is `Some(…)` and the
   `assert_eq!(…, None)` fails. **Fails before.**
5. `comment_spans.rs:122` `unpaired_cdo_span_reports_a_cdo_opened_inside_a_selector_prelude` —
   `.a <!-- .b { … }`. The CDO is inside the selector region via the same catch-all, so pre-fix the
   fact is `None` and the `.expect(…)` panics. **Fails before.**
6. `render_tests.rs:964` `comment_close_inside_an_unused_rule_selector_prelude_is_escaped` — this is
   the public-boundary test for the actual production defect. `.missing` is unused against the
   fixture markup (`<div class="card"><p class="title">x</p></div>`), so the whole rule is wrapped;
   pre-fix `comment_spans_in` over the wrapped span yields nothing, `escape_comment_close` inserts
   no backslash, and the emitted string is `/* (unused) .missing /* x */ span { … }*/` — the
   `assert_eq!` against the `*\/` form fails, and so does the negative
   `assert!(!code.contains("/* x */"))`. **Fails before.** The expected shape matches the existing
   sibling at `render_tests.rs:276-278`.

No empty bodies, no always-true assertions; four of the six carry an explicit negative or ordering
assertion alongside the positive one. All six sit in already-registered test files, so none is
orphaned from a runner.

I also checked the fix cannot silently break the neighbouring golden expectations that contain a
prelude comment. In `selector_list_prune_boundary_search_skips_a_comment_holding_its_own_comma`
(`render_tests.rs:213`) and `…_lands_on_the_comma_past_trailing_whitespace` (`render_tests.rs:239`),
`/* x,y */` sits *after* the wrapped `.dead` span, so the newly-visible entry is excluded by
`comment_spans_in`'s `end <= range.end` bound and those exact-string expectations are unaffected.

## Q6 — Is the documentation commit accurate? **Its stated conclusion is; one supporting sentence is not.**

Checked for internal consistency only; no re-measurement attempted.

- **"Four categories exceed 1.2x, not five" — consistent.** From the appended table: `descendant_selectors`
  1.23x, `deep_rules` 1.45x, `slotted_rules` 1.61x, `global_rules` 2.05x are above; `pseudo_selectors`
  is 1.10x, and the original table at the top of the document recorded it at 1.22x, so the stated
  1.22x → 1.10x drop is exactly what the two tables show.
- **Every ratio in the new table checks out** against its own two columns (408/929=0.44, 658/822=0.80,
  409/422=0.97, 359/371=0.97, 639/648=0.99, 408/371=1.10, 458/371=1.23, 758/522=1.45, 758/472=1.61,
  758/370=2.05). Eleven rows, matching "all eleven categories".
- **The deferral is not weakened and no bound is rebased.** The section states the DEFER is unchanged,
  calls its own comparison a lower bound, and says explicitly that nothing supports a rebase and none
  was made. It adds no `performance-gates.toml` cell (the diff touches no such file) and does not edit
  the Acceptance-criterion or Resolution-gate sections above it.
- **"14/14 passing" is not in tension with a missed ceiling.** `allocator_canaries.rs:339` records
  that the canaries "do not freeze a ratio ceiling", so the suite can be green while the per-category
  1.2x comparison misses — consistent with the document's own claim that no cell for that ceiling
  exists.
- **The "contributes nothing to these numbers" claim is consistent** with the generators: none of the
  eleven in `allocator_canaries.rs:100-190` emits a comment, so the comment inventory stays empty and
  the new accumulation allocates nothing there.
- **One sentence contradicts the document's own tables** — see FINDING F1. The bullet at line 183
  says three categories now measure a legacy count 3 lower than recorded; comparing the two tables,
  it is all five of the originally cited categories (374→371, 374→371, 525→522, 475→472, 373→370).

---

## Findings

**F1 — P3** — `docs/arch/refactor/rev11/evidence/J1/debt-J1-css-parse-allocation-ceiling-residual.md:183`.
"Three of the categories cited in the original escalation now measure a legacy count 3 lower than
recorded" undercounts: all **five** categories in the original table moved by exactly −3
(descendant_selectors 374→371, pseudo_selectors 374→371, deep_rules 525→522, slotted_rules 475→472,
global_rules 373→370). The surrounding conclusion is unaffected — the point of the bullet ("the
legacy baseline itself moved; re-measure both columns") holds a fortiori, and the deferral and bound
are untouched. But this is an evidence document a future owner will read as the record of what
changed, and the number is wrong by inspection of its own two tables. Smallest sufficient fix:
change "Three" to "All five" (or "Every"). Nothing else in the document needs to move.

## Body-only notes (deliberately not findings)

- **Pre-existing conditions, not touched by this delta and not re-filed:** the three
  `cargo clippy -D warnings` errors (`parser.rs:637`, `svelte_compat.rs:93`, `svelte_compat.rs:193`)
  and the eleven `cargo fmt --check` diffs. The fix does not modify `svelte_compat.rs` at all, and
  its `style_ir.rs` edit is nowhere near `parser.rs:637`. I did not run either tool.
- **No allocation regression from the fix.** `comment_spans` is built by incremental `push` on a
  `Vec::new()` where it was previously `.collect()`ed from a `filter` iterator in `finish()` — a
  `filter` iterator has a lower size-hint bound of 0, so the old path also grew by doubling. Same
  final allocation, moved earlier. For the canary corpora (no comments) it is zero either way.
- **The `unpaired_cdo_span` behaviour change is a semantic improvement, correctly scoped.** Its one
  consumer is `svelte_reject_from_ir` (`svelte_compat.rs:80`), which projects Svelte's
  `read/style.js` reject codes. Svelte reads raw source, so a `<!--`/`-->` in a selector prelude
  counting toward the pairing is the behaviour that matches the oracle; the old value was wrong in
  both directions (missed opens, missed closes). I found no test anywhere in the tree asserting the
  old behaviour — `grep` for `unpaired_cdo` returns only `style_ir.rs`, `svelte_compat.rs:80`, and
  the two new tests.
- **Commit hygiene is clean.** Neither message names the program, its revision, or a block
  identifier, and neither body carries plan vocabulary.

## Verdict

The fix implements the mandated mechanism exactly, in the single owning crate, with no second parse
path, no shim, and no consumer-side workaround — so the standing single-CSS-authority rule is
satisfied. Observation is once-per-token on every reachable path, ordering is preserved by
construction, and both constructors are covered structurally. All six new tests would have failed
against the pre-fix tree. The documentation commit's stated conclusion is internally consistent and
weakens nothing; one supporting sentence miscounts against its own tables (P3).

No P0/P1. **PASS.**

===VERTER-RECEIPT-BEGIN===
LANE: fix-review
RESULT: PASS
REVIEWED: b75140abfa7514f3644a1291ee8908d604e5e0c0
FINDINGS: 1
FINDING F1 | P3 | docs/arch/refactor/rev11/evidence/J1/debt-J1-css-parse-allocation-ceiling-residual.md:183 | "Three of the categories" undercounts the moved legacy baseline; its own two tables show all five moved by -3
===VERTER-RECEIPT-END===
