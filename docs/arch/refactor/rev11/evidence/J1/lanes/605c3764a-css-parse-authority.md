# Lane verdict — css-parse-authority

**Tree reviewed:** `<worktree-root>/verter-j1`, branch `block/j1-integration`,
HEAD `605c3764a9fc25dc2b0008a18e8674686d4f2783` (verified with `git rev-parse`; working tree clean).
**Delta:** `git diff ddffe3d7e2449504dfaa621a1c0c20ae074a185b 605c3764a9fc25dc2b0008a18e8674686d4f2783`
— 13 files, +2465/-86.

**No compiler, test runner, or package manager was invoked.** The host memory hold was observed in
full; this lane needed only git, grep and file reads, and nothing in the answer below depends on a
build.

**Answer to the lane question: no.** Nothing in this integration reintroduces a second CSS parse
path, retains or re-adds a `lightningcss` dependency, or works a `verter_css_syntax` gap around
rather than implementing it. The three `fix(core)` commits are the opposite of a workaround: they
*extract* the direct parser's predicate and classifier into shared functions and make the
indentation-aware layout parser call them, deleting the approximations that had diverged.

---

## 1. `lightningcss` — nothing added, nothing re-added

Grep of the delta finds exactly two occurrences, both **prose in one evidence document**, neither in
code and neither a dependency:

- `docs/arch/refactor/rev11/evidence/J1/allocation-phase-attribution.md` — describes the legacy
  pipeline's `StyleSheet::parse` + `to_css` reserialize, and names legacy's "competing `lightningcss`
  parse" as the thing NOT to route around.

Tree-wide, `lightningcss` still appears in 14 files (`verter_compiler/src/css/**`,
`verter_compiler/Cargo.toml`, `verter_wasm/Cargo.toml`, `verter_session/src/types.rs`, and three
guard test files). **Not one of those files is in the delta.** That residue is the pre-existing
legacy engine J1's later slices own; per the brief it is body-note only, not a row.

## 2. Cargo manifests — no dependency change at all

`git diff --name-only` over the delta lists **no `Cargo.toml` and no `Cargo.lock`**. Neither
`crates/verter_css_syntax/Cargo.toml` nor `crates/verter_compiler/Cargo.toml` is touched by this
integration. Nothing was added, and no feature default moved.

## 3. `parse_style_ir` is still the sole entry, and the counter still proves it

`crates/verter_css_syntax/src/style_ir.rs:1086-1100`. The `STYLE_IR_PARSE_INVOCATIONS` increment is
an **unchanged context line** in the diff — no resolution moved, guarded, or removed it — and it sits
inside `parse_style_ir` itself, before `StyleSyntaxIrSink::new`, so any caller anywhere moves it.

Every `<style>`-body parse in the tree routes through it. Enumerated call sites outside
`verter_css_syntax/src` (`style_planner.rs:264`, `svelte/runtime/css/mod.rs:174`,
`analysis/style_syntax.rs:26,39`) all call `parse_style_ir`; **none of those files is in the delta**,
so no sibling path was introduced. `parse_component_value_tree` and `parse_selector_structure` are
pre-existing narrower entry points into the *same* parser, not second engines; the delta changes
`parse_component_value_tree` only in which sink constructor it uses
(`StyleSyntaxIrSink::with_entry_point`, a capacity-sizing change).

Slice 2's `parse_with_sink` / `parse_selector_structure` usage is confined to test code
(`crates/verter_compiler/tests/allocator_canaries.rs:500` drives the shared parser with a `NoopSink`
purely to attribute lexer+parser allocation; `crates/verter_css_syntax/tests/cases/selectors.rs`
uses the public `parse_selector_structure` / `parse_lossless`). Both are measurements of the shared
parser, not alternative parses of a style body. `allocator_canaries.rs` is a pre-existing
allowlisted standalone test target, not a new test binary.

## 4. The three `fix(core)` commits are implementations in the shared owner

The brief expected four; there are three (`6be4b82c59`, `c79e8314d4`, `f3aa9fab92`). I read the code,
not the messages.

**`c79e8314d4` — declaration block value.** `crates/verter_css_syntax/src/parser.rs:634-712`: the
value-shape half of `Parser::looks_like_declaration` is lifted out verbatim into a free
`pub(crate) fn declaration_value_shape_admits(source, tokens)`, and `looks_like_declaration` now
*calls it* (`return declaration_value_shape_admits(&self.source, clone)`).
`crates/verter_css_syntax/src/layout.rs:1022-1049`: `colon_starts_declaration` deletes its old
byte-adjacency approximation (`next.start != separator.end`) and delegates to the same function.
Two callers, one predicate. This is the exact shape a "gap implemented, not worked around" is
supposed to take.

**`6be4b82c59` — statement classification.** `classify_at_rule` is promoted from private to
`pub(crate)` (`parser.rs:1487`) and layout.rs replaces its hardcoded `SyntaxKind::UnknownAtRule` with
a call to it (`layout.rs:390`), so `@media` in Sass/Stylus now classifies as `GroupAtRule` exactly as
in the direct path. The custom-property arm (`layout.rs:220-228`, `1037-1042`) mirrors
`looks_like_declaration`'s unconditional custom-property admit (`parser.rs:625,633`) — I checked both
sides. The variable-colon tightening (`layout.rs:398-411`) turns a malformed `$tone junk: red` into
`Ambiguous`, converging with the direct parser rather than special-casing it.

**`f3aa9fab92` — indented unknown at-rule body.** `layout.rs:288-308` (braced) and `346-363`
(indented) both now splice the body through `replay_subparse_unwrapped`, which runs the **shared**
`parse_with_sink` at `CssEntryPoint::ComponentValueList` and drops the wrapper node. I verified the
parity claim against `parser.rs:868-870`: the direct parser's `(SyntaxKind::UnknownAtRule, _)` arm
calls `parse_component_values(sink, Some(RightBrace))` directly inside `AtRuleBlock` with no
`ComponentValueList` wrapper — which is precisely what dropping the wrapper reproduces. The braced
arm and the indented arm now behave identically, which was the whole point.

None of the three is a dialect bypass, a caller-side fixup, or a special-case branch that leaves the
two paths divergent. The layout parser remains what it already was: an indentation front-end that
delegates content to the shared recursive-descent parser via `parse_with_sink` subparses. This
integration *reduces* its divergence.

I did check `replay_subparse_unwrapped`'s unconditional `events.remove(0); events.pop();` and
`replay_subparse_retagged`'s first/last retag for the unbalanced-stream hazard — `Parser::parse`
appends a trailing `while self.peek().is_some() { recover_current(...) }` loop *after* the entry-point
match (`parser.rs:205-207`), which would put non-`FinishNode` events last. It cannot fire for
`CssEntryPoint::ComponentValueList`: `parse_component_values(sink, None)` consumes closing delimiters
via `recover_current` and only returns at EOF (`parser.rs:896-921`), so the lexer is always drained.
Both helpers hardcode `ComponentValueList`. Sound as written.

## 5. Test-only escapes are compiled out of production

`style_ir.rs:1049-1084` — the `parse_phase_probe` module (and its `PROBE` TLS), the public
`set_style_ir_parse_phase_probe`, and the live `notify_parse_phase` are all
`#[cfg(any(test, feature = "test-support"))]`; the production build gets
`#[cfg(not(any(test, feature = "test-support")))] pub(crate) fn notify_parse_phase(_phase: &'static str) {}`
(line 1083-1084). `STYLE_IR_PARSE_INVOCATIONS` (1025-1039) and its accessor are gated the same way,
as is the `lib.rs:46-47` re-export.

The gate holds end to end: `crates/verter_css_syntax/Cargo.toml` declares `test-support = []` with
**no `default` feature enabling it**, and the only two edges that turn it on are
`[dev-dependencies]` entries in `verter_compiler/Cargo.toml:126` and `verter_semantic/Cargo.toml:68`.
The workspace sets `resolver = "2"` (`Cargo.toml:3`), so dev-dependency features are not unified into
non-test builds — a `-p verter_napi` / `-p verter_wasm` / `-p verter_lsp` release build carries
neither TLS nor any armable probe call. A consumer cannot arm it: the setter does not exist in that
build. The test side installs it through a `Drop` guard that restores the prior probe
(`allocator_canaries.rs:352-363`), so a panicking measurement cannot leak a probe onto the thread.

## 6. Additional consequences of the known `self.tokens` starvation — I found none

Per the brief I did not re-derive the known `html_comment_in_body_is_clean` regression, but I did
enumerate what else the `return Ok(())` at `style_ir.rs:970` starves. Result: **nothing beyond the two
facts already reported.**

- `StyleSyntaxIr` has exactly two token-derived fields, and `finish()` (`style_ir.rs:658-684`) is the
  only place they are computed: `comment_spans` and `unpaired_cdo_span`. Both are already in the
  known report. There is no third.
- The other `self.tokens` reader is `close_frame`'s `&self.tokens[frame.token_start..]`
  (`style_ir.rs:715`), used for a frame's `first`/`last` token. `first` is consumed only by the
  `Declaration` / at-rule / `MixinOrFunctionHeader` arms and `last` only by the
  `Function`/`ComponentValueBlock`/`Interpolation` arms — none of which can enclose a selector
  region. I confirmed this by enumerating every `SyntaxKind::SelectorList` emission site
  (`parser.rs:990`, reached from `parser.rs:197,469,1392,1459`): the only one visible to
  `StyleSyntaxIrSink` is the one inside `QualifiedRule`, and the `QualifiedRule` arm
  (`style_ir.rs:826-841`) reads `frame.selector_list` / `frame.block`, never `first` or `last`. The
  pseudo/nth-of lists are nested inside an already-active `SelectorSink` and are handled by depth,
  not by a fresh entry.
- Value accumulation is unaffected: in the pre-integration code the `StartNode(SelectorList)` frame
  was pushed first, so `self.open.last()` inside a selector region was never a value frame and its
  tokens never entered an enclosing `ComponentValueTree` either. The selector region was already a
  gap there; it still is.
- Frame balance is preserved. The `SelectorList` `StartNode` no longer pushes a frame and its
  `FinishNode` no longer pops one, so `self.open` stays balanced and
  `finish()`'s `verter_debug_assert!(self.open.is_empty())` is not newly exposed.
- Diagnostics are **not** starved: the `ParseEvent::Diagnostic` handling was deliberately hoisted
  above the selector branch (`style_ir.rs:940-946`), so both `self.diagnostics` and the
  `frame.recovered` propagation still see selector-region diagnostics. That part of the restructure
  is correct.

So the blast radius of the known root cause is exactly `comment_spans` + `unpaired_cdo_span`, and it
is fully described in the existing report. No row.

## Body-only observations (deliberately not rows)

- **Pre-existing `lightningcss`** in `verter_compiler/src/css/**`, `verter_compiler/Cargo.toml`,
  `verter_wasm/Cargo.toml`, `verter_session/src/types.rs`. Untouched by this integration; later J1
  slices own the removal.
- **`debt-J1-css-parse-allocation-ceiling-residual.md`** defers an *allocation-count ceiling*, not a
  CSS-correctness gap. It explicitly refuses to rebase the bound or add a `performance-gates.toml`
  cell, names slice 4 as durable owner with a pre-acceptance resolution gate, and carries a codex
  ruling reference. It defers nothing to a consumer and proposes no second path — outside this
  lane's invariant.
- **`find_matching_right_brace`** (`layout.rs:1001-1020`) tracks brace depth only and ignores
  paren/bracket nesting, unlike `find_boundary`'s general scan. A `{` opened inside parens within an
  unknown at-rule body could in principle mis-locate the closer. Contrived, donor-branch content
  (not an integration artefact), and out of scope for a row per the brief.
- **One residual layout/direct divergence I could not verify without a compiler:**
  `looks_like_declaration` admits an `ScssVariable`/`LessVariable` name unconditionally
  (`parser.rs:632-634`), while layout's `colon_starts_declaration` sends non-`--` names through the
  shape predicate. In layout, a `$x: …` statement is classified `Variable` before reaching that
  function, so the paths likely still agree on the statement kind and differ at most in where a
  brace-valued `$x: bar { … }` places its block. Donor-scope, unverified, explicitly not a row —
  recorded only so a downstream reviewer with a build available can close it cheaply.

---

## Verdict

Every check in the lane passes. The integration's conflict resolutions preserved the
single-authority routing rather than "keeping both sides working": the parse counter is intact inside
the sole entry point, no manifest moved, no `lightningcss` code or dependency was added, the probe is
structurally absent from production, and the three dialect-alignment fixes landed as shared
implementations in `parser.rs`/`layout.rs` with the approximations deleted.

===VERTER-RECEIPT-BEGIN===
LANE: css-parse-authority
RESULT: PASS
REVIEWED: 605c3764a9fc25dc2b0008a18e8674686d4f2783
FINDINGS: none
===VERTER-RECEIPT-END===
