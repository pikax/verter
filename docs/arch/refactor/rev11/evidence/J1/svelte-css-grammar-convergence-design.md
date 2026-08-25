# J1 row 5 — Svelte CSS grammar convergence: design findings and remaining work

Status as of this document: the full convergence this document designs is now IMPLEMENTED and
landed — `analyze.rs`, `match.rs` (+ its six helper files), `render.rs`, and `mod.rs` all operate
directly on the shared `verter_css_syntax::StyleSyntaxIr`; the legacy Svelte-local grammar
(`parse.rs`, the grammar-owning portion of `types.rs`, `validate_svelte_style_ir`) is deleted, and
no staging module names (`analyze_shared`/`analyze_shared_tests`) survive. §1-§6 below are kept as
the historical design record and reasoning trail, not a live TODO list — read §6f for what each
module's final shape needed to become; treat items there as DONE unless a later note in this file
says otherwise.

## 0. What this covers

`crates/verter_compiler/src/svelte/runtime/css/{parse,types,analyze,match,hash,render}.rs` — see
the charter (`docs/arch/refactor/rev11/charters/J1.md` §4 "Row 5") and the ratified scope consult
(`docs/arch/refactor/rev11/evidence/j1-svelte-css-grammar-scope-consult.md`) for why this is in
scope and the required disposition (Converge). This document is the concrete engineering plan for
that convergence, written after reading `parse.rs`, `types.rs`, `analyze.rs`, `mod.rs`, `hash.rs`
in full and `match.rs`/`render.rs` in part, plus `verter_css_syntax`'s `style_ir.rs`, `selector.rs`,
`parser.rs`, and `svelte_compat.rs` in full.

## 1. Landed: typed percentage / nth-of selector-shape projection (A11's groundwork)

`verter_css_syntax`'s selector grammar (`parser.rs::parse_selector_list`) has no production for a
keyframe percentage step (`50% { }`) or for Svelte's lenient "any pseudo-class argument that looks
like an An+B formula becomes an opaque token" rule (Svelte applies this inside EVERY pseudo-class's
arguments, not only `:nth-child`/`:nth-last-child`, which is all the shared grammar's
`PseudoFunctionKind::NthChild`/`NthLastChild` + `parse_an_plus_b_tokens` structurally support today).
A bare `50% { }` selector currently parses as a `SelectorCompound` with ZERO recognized
`SelectorComponent`s — the shared tokenizer swallows the `Percentage` token into the compound's raw
span without building a typed node for it, because no arm of `parse_selector_list`'s match handles
`TokenKind::Percentage`.

The byte-matching ALGORITHM for both shapes already existed, landed and tested, inside
`svelte_compat.rs`'s private validation-only reject-gate cursor (`percentage_len` / `nth_of_len`,
ported from upstream's `REGEX_PERCENTAGE` / `REGEX_NTH_OF`). Rather than re-deriving a second
independent matcher for the real AST-building path (which A11d and the "one canonical parse"
rule would forbid), I extracted the matching logic into free functions
(`percentage_len_at` / `nth_of_len_at` / their arm helpers) and added two typed projection
entry points:

- `verter_css_syntax::svelte_percentage_selector_span(source: &CssSource, span: Span) -> Option<Span>`
- `verter_css_syntax::svelte_nth_of_selector_span(source: &CssSource, span: Span) -> Option<Span>`

Both classify an ALREADY-DELIMITED span (a compound's span for percentage; a pseudo's
`argument_span()` for nth-of) — the classification decision lives in `verter_css_syntax` (the sole
syntax authority), and a Svelte-side consumer only ever calls these typed functions and slices the
returned span, never re-derives the shape itself. `style_body_reject_code`'s cursor now delegates
to the same free functions instead of maintaining its own copy.

Tests: `crates/verter_css_syntax/tests/cases/svelte_lenient_selector_spans.rs` (13 cases, pinned
against the same upstream regex derivation the reject-gate cursor's doc comments already document).
Full crate suite green: `cargo nextest run -p verter_css_syntax` — 144/144 passed.

These two functions are NOT yet wired into `parse_selector_list`'s compound-parsing loop or into
`SelectorSink`/`SelectorComponent` as a new `SelectorComponentKind` variant — they are typed
classifiers a CONSUMER calls given a span the existing grammar already produces (an empty
compound's span; a pseudo's argument span). §3 below explains why I judged this the lower-risk
design over teaching the core selector grammar a new component kind.

## 2. Confirmed additional gaps (not yet closed)

Verified by reading `verter_css_syntax::selector::SelectorAttribute` in full: it carries
`span` / `matcher: Option<AttributeMatcher>` / `name_span: Option<Span>` / `value_span:
Option<Span>` — there is NO field for the trailing case-sensitivity flags run (`i` / `s`) that
Svelte's `SimpleSelector::Attribute.flags: Option<String>` carries (`[attr~="v" i]`). This is a
second additive-extension gap, same shape as §1: the shared grammar's `parse_selector_attribute`
does not capture a flags span at all today. Needs: extend `SelectorAttribute` with a
`flags_span: Option<Span>` field populated the same way `value_span`/`matcher` are (reading the
already-tokenized attribute-selector token run — no new tokenizer work, just a new accessor on data
already scanned).

Not yet verified with the same rigor (time-boxed out of this session — flagged as risk, not fact):

- Declaration VALUE reconstruction. Svelte's official `Declaration.value: String` is `read_value`'s
  trimmed, comment-stripped, quote/`url()`-respecting raw text with escapes RE-ENCODED as `\` + the
  following char (not decoded). The shared CST's `StyleDeclaration.value(): &ComponentValueTree` is
  a structured token list (`ComponentValue::{Token,String,Comment,Function,Block,Interpolation}`)
  built by the general tokenizer. Reconstructing Svelte's exact trimmed/re-encoded string from the
  `ComponentValueTree`'s token spans (skip `Comment` values, concatenate the raw source text of the
  rest, trim with the SAME `trim_js_whitespace` `svelte_compat.rs` already exports) is very likely
  sufficient for the vast majority of declarations, but the exact escape/whitespace edge cases (an
  escaped newline inside a value, an unterminated string inside a value) need a byte-for-byte
  comparison against `read_value` before trusting it — `read_value`'s own module-doc note in
  `svelte_compat.rs` (lines ~27-36) explains why THAT reader stayed bespoke rather than reusing the
  shared lexer's string-token handling; the same caution applies to reconstructing the value text
  from the shared `ComponentValueTree` for the real (non-validation) render path.
- At-rule NAME decoding. `StyleDirective` (shared) stores only `head_span` (the raw first-token
  span) and `opaque_args: ComponentValueTree`. Svelte's `Atrule.name: String` is the DECODED
  identifier after the `@` (unicode escapes resolved via the same `read_identifier` rule
  `decode_css_identifier` in `token.rs` already implements) — recoverable by slicing
  `head_span` (minus the leading `@` byte) and calling `decode_css_identifier`, which is already
  `pub` and used by `selector.rs`. Prelude TRIMMED text (`Atrule.prelude: String`) needs the same
  reconstruction-from-`ComponentValueTree` treatment as declaration values (previous bullet).
- `PseudoFunctionKind` does not distinguish `:global` / `:host` / `:root` from any other unknown
  pseudo (only `Is`/`Where`/`Not`/`Has`/`NthChild`/`NthLastChild`/`Unknown` are named) — `analyze.rs`
  compares literal pseudo NAMES (`name == "global"`, `"host"`, `"root"`, `"has"`, `"is"`, `"where"`,
  `"not"`). This does NOT need a grammar change: `SelectorComponent::name_span()` already gives the
  exact pseudo-name span for `PseudoClass`/`PseudoElement`/`FunctionalPseudo` component kinds; a
  Svelte-side comparison against `source[name_span]` (decoded via `decode_css_identifier`, the same
  function the shared grammar itself uses) is a span-slice read, not new classification — compliant
  with A11d Part 2. Confirmed by reading `selector_name_span` in `selector.rs`.

## 3. Design: side-table facts, not a mutated/competing AST

A11d's own required test text is explicit that the retained policy functions' TREE-shaped
parameter must be nominally `StyleSyntaxIr` — e.g. `const _: fn(&str, &mut StyleSyntaxIr) ->
Result<CssAnalysis, CssAnalysisError> = analyze_stylesheet;`. `StyleSyntaxIr`'s node types
(`StyleRule`, `ComplexSelector`, `SelectorCompound`, `SelectorComponent`, …) are immutable
(private fields, getters only, built once by the parse sink) and carry no slot for Svelte's own
analysis facts (`is_global_block`, `is_nested`, `has_global_selectors`, `is_global` /
`is_global_like` per relative-selector step, `used` / `scoped` per complex selector). Per the row-5
required work text: "the shared CST has no room for Svelte's own analysis facts... those need an
out-of-band side table... A side table is NOT a competing grammar/AST — a second parallel
StyleSheet-shaped struct tree IS."

Planned design: `CssAnalysis` (already exists, returned by `analyze_stylesheet`) grows three
pointer-identity-keyed maps, populated by the analyzer walk and read (and, for `used`/`scoped`,
further mutated) by the matcher:

```rust
pub struct CssAnalysis {
    pub keyframes: Vec<KeyframeName>,
    pub global_keyframes: Vec<GlobalKeyframeName>,
    pub has_global: bool,
    pub rules: FxHashMap<usize, RuleFacts>,        // key: &StyleRule as *const _ as usize
    pub complex: FxHashMap<usize, ComplexFacts>,   // key: &ComplexSelector as *const _ as usize
    pub compounds: FxHashMap<usize, CompoundFacts>, // key: &SelectorCompound as *const _ as usize
}
```

Pointer identity is stable and unique for the lifetime of one owned, never-cloned `StyleSyntaxIr`
value: its `Vec`s are built once by the parse sink and never reallocated afterward, so a `*const T`
taken from a borrow of the tree stays valid and distinct across the analyze → match → render
pipeline AS LONG AS the same owned `StyleSyntaxIr` is threaded through unmoved (never
`.clone()`d) — which the current `mod.rs` pipeline already does (`AnalyzedStyleBody` owns it and
moves, never clones, into `complete_style_scope_plan`). This needs stating explicitly as an
invariant comment on `CssAnalysis` and a debug assertion or test that a clone (should one ever be
introduced) is caught, since a clone silently invalidates every key in these maps.

`RelativeSelector` has no 1:1 shared-CST equivalent: it is a synthesized (combinator, compound)
pair. `ComplexSelector::parts(): &[ComplexSelectorPart]` interleaves `Compound`/`Combinator` in
source order; a "relative selector" step is reconstructed by pairing each `Compound` with the
immediately PRECEDING `Combinator` part (`None` for the first compound — matching Svelte's own
"the combinator joining this compound to the PREVIOUS one" definition). The facts side table keys
on the `SelectorCompound` (not a synthesized pair type), since combinators carry no analysis facts
of their own in the official model.

Deviation flagged for reviewer ratification: the charter's illustrative witness signature uses
`&mut StyleSyntaxIr`. This design instead proposes `&StyleSyntaxIr` (read-only) with the facts
table owned by the RETURNED `CssAnalysis` value, not attached to `StyleSyntaxIr` itself, on the
grounds that (a) A11d's stated purpose is nominal-type proof of the tree parameter, which an
immutable borrow satisfies identically to a mutable one, and (b) attaching Svelte-specific fact
storage as a field on the shared, framework-neutral `StyleSyntaxIr` type would leak a
framework-specific concept into the shared authority, which the row-5 required-work text's "may
survive as framework-specific policy only" language and the general shared-authority principle
both argue against. This has NOT been ratified — flag it explicitly at the next review rather than
silently deviating further downstream (e.g. in `match.rs`'s own required witness).

## 3a. Attempted, reverted: `analyze.rs` rewrite (draft in this directory, NOT compiled/verified)

I wrote a full rewrite of `analyze.rs` implementing §3's design (side-table `CssAnalysis` keyed by
`StyleRule`/`ComplexSelector`/`SelectorCompound` pointer identity, `relative_steps` reconstruction,
`component_name` span-slice+decode helper, the `:global`/nesting placement validation family, the
keyframes/global collection) against `verter_css_syntax`'s shared types. It is saved, UNCOMPILED and
UNTESTED, at `svelte-css-analyze-rs-draft.txt` in this directory as a head-start for whoever
continues — it is a genuine, careful line-by-line translation of the original algorithm (see its own
doc comments for the mapping reasoning) but carries ZERO verification: I never ran `cargo check`
against it, because doing so requires `mod.rs`/`match.rs`/`render.rs`/`types.rs` to compile against
the SAME new shape too (§3b explains why), which I did not reach. Treat every line as a claim to
re-derive and check, not as verified fact. I reverted the live `analyze.rs` back to its original
content so the tree stays green and buildable.

## 3b. Why `analyze.rs` cannot be verified in isolation, and why `match.rs` is the harder module

Rust compiles a whole crate at once: `mod.rs` calls `analyze::analyze_stylesheet(source, &mut ast)`
where `ast: types::StyleSheet` (the Svelte-local AST `parse.rs` builds), and `match.rs`/`render.rs`
consume that SAME `ast` value across the pipeline (`AnalyzedStyleBody` carries it forward). Changing
`analyze_stylesheet`'s signature to take `&StyleSyntaxIr` instead breaks `mod.rs` and therefore the
whole crate immediately — there is no way to compile-check (let alone test) an isolated `analyze.rs`
rewrite without ALSO converting `match.rs` (all 6 files) and `render.rs` and `mod.rs` and
`types.rs`'s `ProvenStyleScopePlan.ast` field in the same change. This is not a preference; it is
forced by the pipeline's design (parse once → analyze in place → match in place → render from the
same tree) and matches CLAUDE.md's "one clean cutover" rule — there genuinely is no smaller
independently-compilable increment here.

Having read `match.rs` in full (1430 lines; the 6 helper files `match_attribute.rs` /
`match_certainty.rs` / `match_index.rs` (1295 lines) / `match_relsel.rs` / `match_values.rs` /
`match_writeback.rs` — roughly 1836 more lines — were NOT read this session), it is substantially
harder to converge than `analyze.rs`, for a reason specific to it: the matcher does not just READ
selector nodes, it SYNTHESIZES new ones at match time. `nesting_selector()` / `any_selector()` /
`descendant_combinator()` construct brand-new `RelativeSelector`/`SimpleSelector`/`Combinator`
values with a sentinel span (`Span::new(u32::MAX, u32::MAX)`, matching upstream's `start: -1`
nodes), and `apply_selector`'s `:has(...)` handling clones an existing `RelativeSelector` with its
combinator swapped out (`let mut owned = first.as_ref().clone(); owned.combinator = None;`). This
works today because `RelativeSelector`/`SimpleSelector`/`Combinator` are plain, publicly
constructible, cheaply-cloneable value types. `verter_css_syntax`'s shared CST types
(`SelectorCompound`, `SelectorComponent`, `SelectorPseudo`) are deliberately NOT publicly
constructible (private fields, built only through the parse sink) — the matcher's synthesis pattern
cannot target them directly.

This does not mean the design in §3 is wrong — it means `match.rs`'s port additionally needs its own
small, match-algorithm-INTERNAL view type (something like today's `RelView<'ast> =
Cow<'ast, RelativeSelector>`, but wrapping either a borrowed shared-CST compound+combinator pair or a
synthesized/modified variant with no real span). This is judged NOT to be "a competing grammar/AST"
in A11d's forbidden sense — it never independently parses source text; it only borrows already-parsed
CST nodes or constructs zero-span synthetic markers for the algorithm's own bookkeeping, exactly as
`RelView`/`Cow` already do today. But this judgment itself needs review before implementation starts,
and designing the view type's exact shape (what it can represent, what `Cow::Owned` variants exist,
how `SelectorPseudo`'s `.selector_list()` — needed for `:is`/`:where`/`:has`/`:not`/nesting argument
recursion — composes with it) is real, undone design work, not a mechanical translation like most of
`analyze.rs` was.

`match.rs` also cross-calls `analyze.rs`'s `is_outer_global` / `is_unscoped_pseudo_class` (already
converged to the new `(source, compound|component)` signatures in the draft) and reads
`RelativeSelector.metadata.is_global` / `.is_global_like` directly (`is_global_with_rule`'s first
check) — under the side-table design these become `CssAnalysis::compound_facts(compound).is_global`
reads, meaning `Matcher` needs read access to the `CssAnalysis` the analyzer produced (a new field on
`Matcher`/a new parameter on `match_stylesheet`), which it does not have today (today it mutates the
SAME struct fields the analyzer wrote, no separate table to reference). Small but real plumbing
change, not just a rename.

## 4. Module-by-module remaining work (none implemented yet)

1. **`hash.rs`** — NO CHANGE NEEDED. Verified: it hashes raw bytes/filename text only, takes no
   parsed-tree parameter, and needs no type-witness anchor per the charter's own text.
2. **`analyze.rs`** (801 lines) — rewrite `analyze_stylesheet` to take `&StyleSyntaxIr` per §3. A
   full draft exists (§3a, `svelte-css-analyze-rs-draft.txt`) but is UNVERIFIED — re-derive/check it
   rather than trusting it, and it still needs `analyze_tests.rs` (344 lines) ported case-by-case:
   each case's assertions currently read `.metadata.*` off the Svelte AST and need rewriting to read
   the side-table maps by rule/complex/compound identity instead (the draft does not include ported
   tests).
3. **`match.rs` + `match_attribute.rs` + `match_certainty.rs` + `match_index.rs` +
   `match_relsel.rs` + `match_values.rs` + `match_writeback.rs`** (~5000 lines total) — the
   selector-to-template matcher (`css-prune.js` port). Same shape of rewrite as analyze.rs but
   substantially larger: this is the highest-risk module (it decides which template elements
   receive the scope class — a correctness-critical, byte-exact-pinned surface) and was NOT read in
   full this session. Needs its own dedicated read-then-design pass before implementation starts.
   `match_writeback.rs`'s name suggests it currently WRITES `scoped`/`used` back onto the Svelte AST
   in place — under the side-table design this becomes "insert/update an entry in
   `CssAnalysis::compounds`/`::complex`" instead; confirm this by reading the file before assuming
   it.
4. **`render.rs`** (1072 lines) — ALREADY edits through `CodeTransform` over the original source
   (verified: `render.rs:159` calls `code.build_string()` on a `CodeTransform`). What must change
   is which tree it WALKS to find edit spans (the shared CST + the `CssAnalysis` side tables,
   instead of the Svelte-local AST) — the edit MECHANISM likely needs no change. Depends on
   analyze.rs + match.rs being converged first (it consumes both's output).
5. **`parse.rs` (982 lines) + the grammar-owning ~16 type names in `types.rs`** — delete once (2)
   through (4) no longer depend on them. `types.rs`'s non-grammar policy/output types (`CssMode`,
   `KeyframeName`, `GlobalKeyframeName`, `ProvenStyleScopePlan`, `MatchedTemplateFacts`,
   `CssScopeFacts`) stay, with `ProvenStyleScopePlan.ast: StyleSheet` retargeted to whatever the
   converged pipeline's final carried-forward tree type is (likely `StyleSyntaxIr` plus
   `CssAnalysis`, or a small wrapper pairing them).
6. **`mod.rs`** — delete the `validate_svelte_style_ir` admission reparse (A11b: this is the
   literal double-parse the row exists to remove) once `analyze_style_body`'s real analysis IS the
   trust gate (a construct `validate_svelte_style_ir` currently rejects — `DynamicClass` /
   `Interpolation` component kinds, which never occur under `CssDialect::Css` today but must still
   fail closed if ever produced — needs to become a check inside the converged `analyze_stylesheet`
   itself, or a thin pre-check reading `tree.has_dynamic_selectors()` — that accessor already exists
   on `StyleSyntaxIr`, verified in `style_ir.rs`).
7. **Required new tests** (A11b/A11d/A11e, all still to write): the `single_parse_per_style_block_call_count`
   and `svelte_convergence_introduces_no_hidden_second_parse_or_reconstruct` call-count probes in
   `mod.rs`; the `analyze_stylesheet`/`match_stylesheet` type-witness `const _: fn(...)` anchors.

## 5. Why this session stopped here

Given the true scope confirmed by reading the code directly — a faithful, byte-exact port of
~5000 lines of Svelte-specific selector-matching and rendering logic onto a structurally different,
dialect-generic shared CST, with at least two confirmed grammar gaps (percentage, attribute flags),
several unverified reconstruction risks (declaration-value/at-rule-prelude text fidelity), and one
confirmed harder-than-expected design gap (`match.rs`'s match-time selector SYNTHESIS needing its own
reviewed view-type design, §3b) — completing this to a standard I could honestly certify against the
pinned `verter_svelte_conformance` corpus was not achievable in the remaining budget of this session.
I attempted the `analyze.rs` module concretely (§3a) and confirmed FIRSTHAND, by trying, that it
cannot even be compile-checked in isolation — the pipeline's tight coupling forces converting
`analyze.rs` + all of `match.rs` + `render.rs` + `mod.rs` + `types.rs` together before any test can
run, and `match.rs`'s selector-synthesis pattern (§3b) is real additional undone design work, not
mechanical translation. Per the project's stub-prevention and verified-claims rules, committing a
rushed, never-compiled rewrite of correctness-critical selector-matching logic — or worse, forcing a
crate-wide conversion under time pressure I could not adequately verify — would be a worse outcome
than stopping with an honest, evidence-backed design document, one small, fully tested, landed
increment (§1), and an unverified-but-substantial draft (§3a) that gives the next attempt a real head
start instead of a blank page. The additive `verter_css_syntax` extension in §1 is genuine, verified,
tested progress; §2-§4 (plus the §3a draft) are the concrete map and head start for continuation.

## 6. Round 2: a reviewing architecture consult, three more landed increments, and the
   fully-informed `match.rs` design

A codex architecture review of §3's open questions ran between round 1 and round 2 (full text
preserved in the round-2 implementer brief, not duplicated here). Its rulings, now applied:

- **§3's `FxHashMap<usize, Facts>` pointer-identity keying is REJECTED** — a real ABA hazard (a
  dropped tree's allocation reused by a later tree turns a stale lookup into silently-wrong output,
  not a loud failure). Replaced with `Span`-keyed newtypes (`RuleKey`/`ComplexKey`/`CompoundKey`,
  each wrapping a `Span`) — every node these tables key on occupies a disjoint byte range in one
  valid parse, so a span is exactly as collision-free as pointer identity was meant to be, without
  the reuse-after-free class. The `&mut StyleSyntaxIr` → `&StyleSyntaxIr` deviation from the
  charter's illustrative witness stands (ratified in round 1); a further deviation — the return type
  becomes `AnalyzedStylesheet<'ast>` (below), not the charter's illustrative bare `CssAnalysis` — is
  newly ratified by this same review.
- **`AnalyzedStylesheet<'ast> { tree: &'ast StyleSyntaxIr, analysis: CssAnalysis }`** lifetime-couples
  the analysis to the exact tree it was computed from, so a caller cannot construct a mismatched
  `(tree, analysis)` pair. Implemented exactly as reviewed, in
  `crates/verter_compiler/src/svelte/runtime/css/analyze_shared.rs`.
- **The matcher's synthesis problem is real and needs its own view-type algebra** (`match.rs`
  synthesizes brand-new `RelativeSelector`/`SimpleSelector` nodes with sentinel spans — the shared
  CST's `SelectorCompound`/`SelectorComponent` are deliberately not publicly constructible). §7 below
  is the CONCRETE algebra, informed by now having read all of `match.rs` + all 6 helper files (round
  1 had read `match.rs` in full but none of the six helpers).
- **Declaration-value / at-rule-prelude / keyframe-name reconstruction is CONFIRMED insufficient** as
  a plain trim over the raw span (§2's risk, now a confirmed gap with an oracle-verified
  counterexample: `URL(/*x*/foo)` strips its comment while `url(/*x*/foo)` does not — the general
  grammar's case-insensitive `url(` token recognition cannot reproduce upstream's case-SENSITIVE
  `value.ends_with("url")` byte check after the fact). Fixed with a genuine `verter_css_syntax`
  extension (§6a), not a Svelte-side re-derivation.

### 6a. Landed: the compat value/prelude/keyframe-name projection + attribute flags

Three more additive `verter_css_syntax` extensions landed, tested, oracle-verified against the pinned
`svelte@5.56.10` compiler's own `parse()`/`compile()` output (reproduction invocations recorded in the
test files' doc comments, not duplicated here):

1. `SelectorAttribute::flags_span()` (`selector.rs`) — the trailing case-sensitivity flags run
   (`[attr~="v" i]`). Tests: `crates/verter_css_syntax/tests/cases/selectors.rs`.
2. `svelte_read_value_text(source, span)` (`svelte_compat.rs`) — `read_value`'s exact
   trimmed/comment-stripped/quote-and-url-respecting reconstruction over an ALREADY-DELIMITED
   `ComponentValueTree` span (a declaration's `value().span()` or an at-rule's
   `opaque_args().span()`); `svelte_first_significant_value_span(tree)` — the keyframe-name span read
   as a typed fact off the already-parsed value list (skip `Comment`/trivia), replacing the
   round-1 draft's rejected `keyframes_name_token_span` byte scan (which the ruling confirmed
   misreads a leading comment, e.g. `@keyframes /* c */ spin`, landing on `/*` instead of `spin`).
   Tests: `crates/verter_css_syntax/tests/cases/svelte_value_prelude_reconstruction.rs` (9 cases:
   case-sensitive `url(`/`URL(` divergence, a CSS-escape spelling of `url` never entering url-mode,
   backslash re-encoding, raw/escaped embedded newlines, leading/trailing comment-skip for keyframe
   names, an at-rule prelude with a comment, an entirely-trivial prelude).
3. `svelte_trailing_type_selector_span(source, span)` (`svelte_compat.rs`) — a THIRD grammar gap,
   discovered (not anticipated by the design doc or the codex review) via the differential test port
   in §6b: `:global(.x)div` must raise `css_type_selector_invalid_placement` on `div`, but the general
   CSS3 grammar requires a type selector FIRST in a compound, so `parse_selector_list` never builds a
   typed `Type` component for a bare identifier trailing a pseudo-class with no combinator — the
   compound simply closes with `div` unclassified in its own raw span (confirmed directly:
   `compound.components().len() == 1`, `compound.span()` extends 3 bytes past the one recognized
   component). Classifies an already-delimited LEFTOVER span (from the last recognized component's
   end to the compound's own span end) as a complete CSS identifier, reusing the same
   `consume_name_profiled`/`IdentifierProfile::SvelteCompat` primitive `read_identifier` already uses
   — never a second identifier grammar. Tests:
   `crates/verter_css_syntax/tests/cases/svelte_lenient_selector_spans.rs` (5 cases, including the
   oracle-confirmed `-x` leading-hyphen accept and `0div` leading-digit reject — the latter actually
   fails to PARSE upstream at all, `css_expected_identifier`, so it never reaches this classifier in
   production, but the classifier itself must not misreport it).

### 6b. Landed: `analyze_shared.rs`, differentially proven, two real bugs caught

`crates/verter_compiler/src/svelte/runtime/css/analyze_shared.rs` — `analyze_stylesheet` converged
onto `&StyleSyntaxIr` per §6's ratified design (Span-keyed `CssAnalysis`, `AnalyzedStylesheet<'ast>`
return type). STAGING MODULE: declared `#[allow(dead_code)] pub mod analyze_shared;` in `mod.rs` but
NOT wired into the live pipeline (which still calls the legacy `analyze::analyze_stylesheet`) — the
wiring + legacy-module deletion happens in the one final cutover per §3b/the round-2 brief's staging
guidance.

`analyze_shared_tests.rs` differentially ports EVERY assertion in the legacy `analyze_tests.rs`
(24 tests, all passing) reading facts through `analysis.rule_facts(rule)` /
`.complex_facts(complex)` / `.compound_facts(compound)` instead of `.metadata.*`. This differential
proof caught two real defects before they could land:

- **The leading-combinator top-level check was silently dropped**, not "moved to the caller" as the
  round-1 draft's own comment claimed (`> .a { color: red; }` must raise `css_selector_invalid`; the
  first draft raised nothing). The check exists in the LIVE `analyze.rs` inside
  `analyze_relative_selector` itself (verified: `analyze.rs:473-479`) — the draft's claim that it was
  "performed by the caller" was never true of either the caller or the callee. Fixed by restoring the
  check in `analyze_relative_selector`, now additionally threading the paired leading combinator
  through from `analyze_complex_selector`'s `relative_steps` call (the draft had discarded it: `for
  (index, (_, compound)) in steps.iter().enumerate()`).
- **The `:global(.class)element` (no separator) case was never caught** — `type_selector_after_global_arg_is_invalid`
  (`:global(.x)div`) failed to raise `css_type_selector_invalid_placement`. Root cause: the shared
  grammar produces exactly ONE recognized component in that compound (confirmed via a throwaway probe
  test), so the legacy check's `components.get(i + 1)` (checking for an EXPLICIT next component) never
  fires. Fixed via the new `svelte_trailing_type_selector_span` extension (§6a item 3) plus an `else
  if` arm at the same call site checking the compound's own unclaimed trailing span when no explicit
  next component exists.

Both fixes are oracle-verified independently (direct `compiler.compile()` invocations against the
pinned 5.56.10 package, reproduction commands preserved in this document's own drafting history) before
being trusted, not merely inferred from re-reading the JS source.

### 6c. Confirmed by reading all 6 `match.rs` helpers in full: scope is smaller than round 1 estimated

Round 1 flagged "`match.rs` + 6 helper files, ~5000 lines total" as one undifferentiated block. Having
now read every one of the six helpers in full (round 1 read none of them), FOUR need ZERO changes for
the grammar convergence — they never reference a CSS-AST-owned type at all:

- **`match_index.rs`** (1295 lines) — the `TemplateIndex` + DOM-neighborhood walk operates entirely
  over the RUNTIME IR (`SvelteRuntimeIr`, `NodeId`, `IrNode`, …), never the CSS AST.
- **`match_certainty.rs`** — the `MatchCertainty` tri-state type + its `and`/`or` folds; pure logic,
  no CSS-AST reference.
- **`match_values.rs`** — the `get_possible_values`/`gather_possible_values` port over the OWNED
  `MatcherExpr` template-expression projection; no CSS-AST reference.
- **`match_attribute.rs`** — `test_attribute` + the JS string helpers (`unescape_backslashes`,
  `unquote`, the `\s` edge tests); pure `&str` functions, no CSS-AST reference.

Two DO need real (but now precisely scoped) work:

- **`match_writeback.rs`** (61 lines) — currently walks the legacy AST mutating
  `complex.metadata.used`/`relative.metadata.scoped` in place, span-keyed against `MatchSink`'s
  `FxHashSet<Span>` sets. Under the converged design this becomes SIMPLER than a tree walk: since
  `CssAnalysis`'s `complex`/`compounds` maps are ALSO `Span`-keyed, write-back is a direct
  span-to-span backfill (`for span in &sink.used_selectors { analysis.mark_used_by_span(span) }` and
  the `scoped` counterpart) — no tree walk needed at all. (`mark_used`/`mark_scoped` on
  `CssAnalysis` currently take a real node reference; a `_by_span` sibling — or retargeting the
  existing methods to accept `Span` directly, since they only ever use the reference to compute its
  key — closes this trivially.)
- **`match_relsel.rs`** (101 lines) + **`match.rs`** (1430 lines) — the real port, needing the view-type
  algebra below.

### 6d. The `StepView` algebra — concrete, not illustrative (informed by the full `match.rs` read)

Every synthesis/filter site in `match.rs` + `match_relsel.rs` is now enumerated exhaustively (round 1's
§3b flagged the CLASS of problem; this is the exhaustive INSTANCE list a design must cover):

1. `match_relsel.rs::get_relative_selectors` — prepends a synthetic NESTING step (`&`, no combinator)
   when a nested rule's selector list has no explicit `&`; when doing so, the (previously-first) real
   step's combinator is force-set to a synthetic DESCENDANT combinator if it had none.
2. `match_relsel.rs::truncate` — drops trailing global/global-like steps; for a REAL step whose
   compound contains a `:root` component (and is not global-like), replaces its component list with
   ONLY the `:has(...)` components (a real compound, filtered view — never a clone with edited
   fields, unlike the legacy `Vec<SimpleSelector>` retain).
3. `match.rs::apply_selector`'s `:has(...)` handling (`relative_selector_might_apply_to_node`) —
   builds an "including self" list (the FIRST truncated step with its combinator forced to `None`)
   and an "excluding self" list (a synthetic ANY (`*`) step prepended, taking over the first step's
   combinator or defaulting it to synthetic DESCENDANT).

Algebra (rename freely — this is the reviewed SHAPE, not a mandated exact API):

```rust
enum CombinatorView<'ast> {
    Parsed(&'ast SelectorCombinator),
    None,
    SyntheticDescendant,
}

enum CompoundView<'ast> {
    Parsed(&'ast SelectorCompound),
    /// A real compound, but iteration sees only its `:has(...)` components
    /// (site 2 above) — carries the real compound for facts lookup (a
    /// `:root...:has(...)` compound's `scoped`/`is_global*` facts are its
    /// own, unaffected by which components the WALK considers).
    OnlyHas(&'ast SelectorCompound),
    /// `*` (site 3, "excluding self") — no real compound backs this; a
    /// component iterator yields exactly one conceptual Type("*") entry.
    SyntheticAny,
    /// `&` (site 1) — no real compound backs this; a component iterator
    /// yields exactly one conceptual Nesting entry.
    SyntheticNesting,
}

impl<'ast> CompoundView<'ast> {
    /// `Some` only for `Parsed`/`OnlyHas` — the `SelectorCompound` a
    /// `scoped`/`is_global`/`is_global_like` fact lookup or write targets;
    /// `None` for a synthetic step (nothing to write back, matching
    /// upstream's own write-to-a-singleton-`-1`-span no-op).
    fn origin(&self) -> Option<&'ast SelectorCompound> { .. }

    /// The components the WALK sees — real components for `Parsed`, the
    /// `:has(...)`-only subset for `OnlyHas` (small allocation; correctness
    /// over a chase for zero-alloc here, matching the legacy `Vec` retain's
    /// own allocation), and one synthetic marker for the two synthetic
    /// variants. The walk needs KIND (+ a couple of name comparisons) per
    /// component, not a real `&SelectorComponent` — so this yields a small
    /// `ComponentView<'ast>`, not `&'ast SelectorComponent`:
    fn components(&self) -> Vec<ComponentView<'ast>> { .. }
}

enum ComponentView<'ast> {
    Real(&'ast SelectorComponent),
    SyntheticAnyType,   // matches `relative_selector_might_apply_to_node`'s `SimpleSelector::Type { name: "*", .. }` arm
    SyntheticNesting,   // matches its `SimpleSelector::Nesting { .. }` arm
}

struct StepView<'ast> {
    combinator: CombinatorView<'ast>,
    compound: CompoundView<'ast>,
}
```

Constraints carried over from the round-1 brief, now confirmed against the full read: no parser/
tokenizer calls anywhere in this algebra; no owned source-derived names/values inside a view (a
`ComponentView` decodes its name lazily, span-slice + `decode_css_identifier`, exactly like
`analyze_shared.rs`'s `component_name`); no rendering; no fake spans (`SyntheticAny`/`SyntheticNesting`
carry no span at all — there is no writeback target, so no sentinel span is needed the way the legacy
`Span::new(u32::MAX, u32::MAX)` was; a `used_selectors`/`scoped_selectors` insertion keys on the REAL
compound's span via `.origin()`, and a `None` origin simply never inserts, which is the exact same
observable no-op the legacy sentinel-span/singleton-write achieved by a different mechanism). Do NOT
add public synthetic constructors to `SelectorCompound`/`SelectorComponent`/`SelectorPseudo` in
`verter_css_syntax` — `CompoundView`/`ComponentView` are match-module-internal, matching the round-1
brief's explicit prohibition.

### 6e. Why this session stopped before the `match.rs`/`match_relsel.rs` port itself

The algebra above is now concrete and full-read-informed, not a guess — but IMPLEMENTING it means
converting `match.rs`'s ~15 functions (`prune_*`, `apply_selector`, `apply_combinator`,
`every_is_global`, `is_global_with_rule`, `relative_selector_might_apply_to_node` — the single
largest and most intricate function, ~360 lines — `compute_has_include_self`,
`mark_complex_used_recursive`, `attribute_matches`, `get_possible_values`) plus `match_relsel.rs`,
each needing exact behavioral parity with `match_tests.rs` (1728 lines — roughly 5x
`analyze_tests.rs`'s size, and testing deeply recursive selector-list traversal:
`:has`/`:is`/`:where`/`:not`/nesting interacting with sibling/ancestor combinators and the tri-state
certainty fold). Porting this to the same differential-proof standard §6b applied to `analyze.rs` —
which is the standard this project's stub-prevention/verified-claims rules require, and which is
exactly what caught two real bugs in the smaller, simpler `analyze.rs` port — is a substantial
dedicated unit of work in its own right, matching what the round-2 brief itself anticipated
("`match.rs`... needs its own dedicated read-then-design pass before implementation starts").
Attempting it in the remainder of this session without that same rigor would risk landing an
unverified, correctness-critical selector-matching rewrite — exactly what the project's stub-prevention
rule forbids and what round 1 already declined to do for the same reason. `render.rs`'s raw-scan
inventory (the codex ruling's other named finding — whitespace runs, comma-splitting, comment-boundary
detection, an animation-value token scan) was NOT reached this session either; `render.rs` still needs
its own reading pass before its port can be designed.

### 6f. Remaining work (revised from round 1's §4, superseded by the above where they differ)

1. **`match_relsel.rs` + `match.rs`** — port onto the §6d algebra; differentially test against
   `match_tests.rs`'s existing 1728 lines case-by-case, the same discipline §6b applied.
2. **`match_writeback.rs`** — the span-backfill simplification in §6c; small once (1) lands.
3. **`render.rs`** — needs its own reading pass first (not done this session); known requirement per
   the round-2 brief: eliminate its raw scans (whitespace runs, comma-splitting, comment-boundary
   detection, an animation-value token scan) using the §6a compat projection or further additive
   `verter_css_syntax` typed facts, keeping its existing (confirmed-correct) `CodeTransform` usage.
4. **Wire `mod.rs` onto `analyze_shared`/the converged matcher/render in ONE change**; delete
   `parse.rs`, the grammar-owning portion of `types.rs`, `validate_svelte_style_ir`, `analyze.rs`,
   `match.rs`'s legacy body, and this session's own `analyze_shared`/`analyze_shared_tests` STAGING
   names (folded into the final module names) in the SAME change — no staging scaffolding survives to
   the landed state.
5. **Required new tests** (A11b/A11d/A11e type-witness anchors + call-count probes) — not yet written;
   depend on (4)'s final signatures.
6. **Full `verter_svelte_conformance` corpus run** — the mandatory acceptance gate, not yet
   attempted (nothing is wired into the live pipeline yet to run it against).

Landed and verified this session: the `verter_css_syntax` extensions in §6a (all with oracle-grounded
tests, `cargo nextest run -p verter_css_syntax` green, 160/160), `analyze_shared.rs` + its 24-case
differential suite (green), and this section's `match.rs`-informed design. Nothing in this section
has been wired into the live pipeline; `node scripts/gate.mjs` was not run this session (targeted
`cargo nextest run -p verter_css_syntax -p verter_compiler --lib`, `cargo clippy -p verter_css_syntax
-p verter_compiler --all-targets -- -D warnings`, and `cargo fmt` for the touched packages were, all
clean) — the full canonical gate is owed before this branch is considered land-ready, and matters more
once the pipeline is actually wired onto the converged modules.
