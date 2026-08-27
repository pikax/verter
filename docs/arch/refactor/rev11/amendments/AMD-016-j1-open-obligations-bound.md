# AMD-016 — J1's open obligations are bound to acceptance criteria

**Status:** RATIFIED 2026-08-25 by the architecture seat under the maintainer's
standing delegation, jointly and atomically with
[`AMD-019-j1-open-obligations-proof-contract.md`](AMD-019-j1-open-obligations-proof-contract.md).
The ratifying verdict is the `joint-ratification` lane receipt at reviewed sha
`bb22b5c81fdbf7d308d20d694c8554cdb669e533` — `RESULT: PASS`, zero P0/P1, one carried
P2 recorded in §12. **That receipt binds the PROPOSED bytes it reviewed**
(`6145a39024decf37a1812d74a88eedf009138e62d5de29810e208a8950a8062e`); this line is §13
step 1, applied after that verdict, so the bytes registered in the authority registry are
the post-ratification ones and differ from the reviewed digest by exactly this paragraph.
See §12.

Three ratification rounds on this instrument alone returned `FAIL` — ten findings, then
nine, then six — and a fourth, joint with its companion proof contract, returned six more.
Every finding was correct. One of them the second draft wrongly disputed; **that finding
stands and the dispute is what §4.1 retracts.** All are retained as evidence. §16 records
rounds 1 and 2; §17 records round 3, the decision that followed, and the joint round.

**The convergence cap is reached and the rescope is recorded (§17).** `orchestration/review.md`
allows two substantive fix cycles; three rounds ran. A decision seat then rejected both
proposed exits and prescribed a third: this amendment stays unratified until a companion proof
contract binds a named gate to each of its new rows, and the two ratified together. The
corrections below are made because leaving a known falsehood or scope violation in a text about
to be ratified is not an option — not because a further round is assumed.

**Prepared against:** local `program/architecture-lock` commit
`e70e7519b936ae535d9c0ced223e567bb472f871`, tree
`830f27c040e9debfd902185e5f52d21032f7363f`. Every `file:line` citation was read directly
on that tree, and every count carries a control that was made to fail on purpose before
its result was believed. **This amendment changes documents only**, so `cargo fmt --all`
and `cargo clippy --workspace --all-targets -- -D warnings` have nothing to run against;
that is stated rather than skipped silently. One CLAIM needed execution rather than
reading and got it — a throwaway probe, run under the machine semaphore and removed
afterwards, whose result forced a retraction (§4.1). Nothing it touched is part of this
change.

**Amends on ratification:** [`../charters/J1.md`](../charters/J1.md) only — §1.1's
inventory, §2.1's acceptance table, and the stale claims elsewhere in the charter that
the new rows would otherwise contradict (§9). **It accepts no block, moves no block
status, changes no DAG edge or predecessor, adds or retires no ledger block, loosens no
gate, removes no existing obligation, touches no A6-locked cell, and writes nothing under
`docs/arch/architecture-lock/ledger/`.** §13 lists the writes application requires and
names their owner; one of them is a precondition this amendment cannot satisfy itself.

---

## 1. Why one instrument rather than a referral per item

J1 landed as an integration milestone and its row is `IN_PROGRESS`. Its acceptance is
defined by coverage of its acceptance IDs
([`../rulings/MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md`](../rulings/MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md)
§3). Work J1's ratified text never **bound** has been surfacing one item at a time, each
discovered when a downstream block became its first real consumer and referred it back.

**The defect is not the individual items. An item named as a logical owner's, without
being bound to that owner, is not owned at all** — no criterion covers it, so nothing
detects it until someone trips over it. J1's own ledger row records the state in those
terms — "three items are named without being bound to any ratified text" — and routes "a
single charter amendment covering that set."

The delivery contract now forbids the landing that produces this state
([`../orchestration/delivery.md`](../orchestration/delivery.md), the acceptance-coverage
step: "A candidate may not land carrying work that no ratified acceptance criterion
covers … Naming a logical owner is not binding one"). That closes the class going
forward. This closes the instance already on the tree.

## 2. The sweep — method, controls, result

Three items were supplied to this block as the open set. **Taking a supplied list as the
complete set is the same defect the amendment exists to end**, so the charter and the
block record were swept independently: which acceptance IDs name no gate; which
obligations are attributed to a block, file or future change with no binding row; which
work is described as owned by something with no acceptance criterion; and what the
landing left unfinished. The tree was then swept for CSS readers §1.1 does not account
for.

Each measurement was controlled, because a probe that cannot return a different answer
proves nothing:

- **Acceptance-ID enumeration.** Extracting the ID column of §2.1 and §3.1 and expanding
  the `A10d-h` range yields **38** distinct IDs, of which `A18` is explicitly *"reserved —
  not used"* — **37 substantive**. Control: appending one synthetic row to a copy moved
  the count to 39. *Independently re-derived by both ratification seats.*
- **Selector re-parse census.** `verter_semantic::analysis::style::parse_selector` has
  **six** production call sites in four crates (§5); each was opened and read, and the
  remaining sites are inside `#[cfg(test)]` modules or a `*_tests.rs` file. Two controls
  on a copy of the tree: deleting the `verter_napi` call line moved that crate's count
  from 1 to 0, and adding a probe line moved it back to 1, so the census detects an
  omitted site and an added one. *Independently confirmed by both seats.*
- **Attribution charge sites.** Exactly three production sites charge the three counters
  `performance-gates.toml` asserts zero (§7), two of them inside the tree A2 requires
  deleted. *Independently confirmed by both seats.*

**Result: the supplied set of three is not the set.** One of the three is already bound
and needs no amendment (§3); two are genuine; and the sweep found **nine further items** —
eight in the first pass and one more that round 2 found in the closure this document
proposed (§8, A34). Two non-blocking discoveries are recorded (§10) and three residuals
are routed rather than assumed (§11).

*Round 2 also settled the authority question: adding identifiers for already-ratified
bounds, and adding a discovered reader through §1.1's own mechanism, are not a formal
rescope, and A30's review enforcement is permitted by the landed-scanner rule.*

## 3. Correction — the typed preprocessor fields are ALREADY bound

The supplied set named "typed preprocessor result fields" as unbound. **It is not.** J1's
ratified text owns it twice: §2's required-outcomes bullet ("J1 OWNS the complete
external-preprocessor RESULT boundary (row 18) … not folded into today's single untyped
`supplied_provenance: Option<String>`") and §4's "Row 18 — required work" at
[`../charters/J1.md`](../charters/J1.md) line 513 — with **acceptance ID A14**.

The work is genuinely outstanding: `crates/verter_session/src/types.rs:2381-2397` still
carries `supplied_provenance: Option<String>` with no `dependencies`/`diagnostics` field,
while the wire record `StampedBlockContentResultV1` already types the same slot as
`BlockContentOriginFingerprintV1`
(`crates/verter_source_policy_gate/tests/cases/scanners_replacement.rs:764`; the type
exists at `crates/verter_language/src/parse_artifact/carrier_inventory.rs:94`).

**Outstanding is not unbound.** Minting a second ID for work A14 already names would
create the duplicate authority this program forbids. J1's ledger note lists this item
among those "named without being bound to any ratified text"; on the charter's own
evidence that part of the note is wrong, and correcting it is one of the ledger writes
§13 hands over. *Confirmed by both seats.* **No charter change is made for this item.**

## 4. The nested selector re-parse (A25)

`crates/verter_semantic/src/analysis/style_syntax.rs:246-260`: while walking a special
pseudo, when the component carries no typed selector list, `collect_special_pseudo` slices
its own already-parsed source (`self.source.slice(span)`), builds a fresh `CssSource`, and
calls `parse_selector_structure` on it — a second parse of input the walk is already
inside, which the governing invariant forbids "regardless of which surface performs it"
([`../rulings/MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE.md`](../rulings/MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE.md)).
**That branch is unreachable today — measured, §4.1 — so this is a latent second parse
rather than a live one.** The distinction changes what the criterion must require and is
stated up front rather than buried.

It predates J1 (`git log -L` attributes those lines to `e6191e280`, 2026-08-07, before the
2026-08-21 ratification), so it was live and unlisted when §1.1 dispositioned row 13 as
**Preserve — already correctly scoped**, acceptance ID `n/a`. Nothing binds it: A24's
landing review is scoped to §1.1's inventory, and that inventory asserts there is nothing
here to check.

### 4.1 The branch is UNREACHABLE — measured, and the previous draft was wrong

Round 2 found this branch unreachable. **The previous draft disputed that finding. The
dispute was wrong, and this section is its retraction**, because a document that argues
with a correct finding is worse than one that never argued.

**Measured, not argued.** An executed probe on this tree walked every selector component
produced by 19 inputs across all five dialects — both spellings of `deep`/`global`/
`slotted`, the `::v-*` forms, empty, malformed, unterminated, whitespace-only and
uppercase arguments, and nested combinations — and evaluated the fallback's exact guard at
each component. **0 fires.** The probe discriminates: widening only its name predicate to
admit `:foo(.a .b)` — an `UnknownPseudoFunction`, which is a dispatched `FunctionalPseudo`
with no typed selector list and a non-empty argument span — reports **4 fires**, so every
conjunct of the guard is simultaneously satisfiable and a zero is a real zero rather than a
probe that cannot speak. The probe was removed after measuring and is not part of this
change.

**The hop the previous draft missed** was one line, and everything it argued before that
line was true and irrelevant. `collect_component_fact` (`style_syntax.rs:204-206`)
dispatches `collect_special_pseudo` only for `SelectorComponentKind::PseudoClass` and
`FunctionalPseudo`, while `SyntaxKind::PseudoElement` maps to
`SelectorComponentKind::PseudoElement` (`selector.rs:1089`). So the `::slotted(...)`
spelling — correctly shown to produce no typed selector list — never reaches the branch at
all.

**Why the branch is dead**, stated so the reason is auditable rather than incidental: for
the three names `collect_special_pseudo` accepts, a functional occurrence always classifies
as `SyntaxKind::PseudoSelectorList` (`parser.rs:1367`, against the pseudo-class list at
`parser.rs:1534-1537`), and that kind is the only one whose arguments run
`parse_selector_list` (`parser.rs:1391-1396`), which installs the typed list at
`selector.rs:886-895`. A non-functional occurrence has an empty argument span and fails the
other guard.

**What keeps it dead is an unenforced coupling between two files.** Nothing connects
`collect_special_pseudo`'s name set in `verter_semantic` to `is_selector_list_pseudo`'s
pseudo-class list in `verter_css_syntax`. The `:foo` control is the demonstration: add a
fourth special-pseudo name that is not in the selector-list list and the dead branch becomes
a live second parse, silently. That is what A25 binds.

**The consequence for this item is a change of kind, not of importance.** It is dead code to
delete plus an invariant to enforce, not a live re-parse — and §1.1 row 13's disposition is
wrong either way, because the module holds three parse entries beside its canonical one
(`style_syntax.rs:25-26`, `:38-39`, `:62`) while the charter records it as consuming row 1
only.

| ID | Requirement | Gate |
|---|---|---|
| A25 | The `!has_typed_selector_list` fallback at `style_syntax.rs:246-260` is **deleted**, not wrapped: it is unreachable today (§4.1) and re-parses already-parsed input if it ever stops being so. **This row is dead-path deletion plus the invariant that keeps the path dead. It binds no behaviour change**, and in particular does NOT require the double-colon spelling to gain a typed selector list: `::slotted(...)` classifies as `PseudoElement` (`parser.rs:1367`) and is never dispatched to `collect_special_pseudo` (`style_syntax.rs:205`), so making its list typed would newly expose nested facts — a semantic change to a public surface, which is not this row's and would need its own criterion and its own pre-change-red/post-change-green boundary proof. The invariant is therefore scoped to the component kinds the function is actually dispatched for: for every special-pseudo name `collect_special_pseudo` recognises, a functional occurrence **that reaches it** yields a typed selector list, so the shared authority carries the guarantee instead of a scanner standing by in case it fails | Three parts. (1) **Coupling invariant, the durable half:** for every name `collect_special_pseudo` recognises, over the dispatched kinds (`PseudoClass`, `FunctionalPseudo`) across all five dialects, a functional occurrence has `selector_list() == Some`. Its discrimination control is `:foo(.a .b)`, which VIOLATES the same predicate — §4.1's measurement already establishes that it does, so the control has a known answer rather than a hopeful one. The invariant reddens the moment a name is present in `collect_special_pseudo`'s set and absent from `is_selector_list_pseudo`'s pseudo-class list, which is the regression the deleted branch was silently insuring against. (2) **Parse-entry counter:** invocations of `parse_selector_structure` originating in `style_syntax` are 0, while the canonical `project_style` parse (`style_syntax.rs:25-26`) remains exactly 1 — an unqualified "zero parse entries" assertion is impossible, since the canonical construction is itself a parse entry, and would be a stub. (3) **Behaviour positive:** nested selector facts for `:global(.a .b)`, `:deep(.a, .b:hover)` and `:slotted(.a)` — the single-colon forms, which are the only ones this row touches — are identical before and after. **No fixture may claim to reach the deleted branch.** It is measured unreachable; a test asserting otherwise asserts a falsehood, and one reading 0 on both trees discriminates nothing |

## 5. The selector re-parse surface the inventory never swept (A26)

**Found by the sweep; named in no referral.** `parse_selector_authority`
(`style_syntax.rs:36-52`), exposed as
`verter_semantic::analysis::style::parse_selector`, has six production call sites in four
crates:

- `crates/verter_napi/src/lib.rs:3233-3240` and `crates/verter_wasm/src/lib.rs:1459` —
  both iterate `css.selectors`, the parsed `AnalyzedSelector` list, and when
  `selector.structure` is `None` **re-parse `selector.text`**: a fallback parse over text
  the one parse already read, taken precisely where that parse recorded "not
  structurable", against an invariant whose text is "No second parser, no private grammar,
  no fallback parser". Stated exactly: the fallback is unlikely to return a different
  answer, because `parse_selector_authority` fails closed unless the selector is complete
  and static — the finding is the shape and its absence from the inventory, not a claimed
  live miscompile.
- `crates/verter_semantic/src/analysis/build.rs:1924`, `:1928`, `:1932` (`querySelector`,
  `getElementById`, `getElementsByClassName` arguments) and
  `crates/verter_mcp/src/server.rs:726` (a selector supplied in an MCP request) — **first**
  parses of independently-supplied text, not re-parses. In scope for a recorded
  disposition, not for convergence.

§1.1's 22 rows name no reader in `verter_napi`, `verter_wasm`, `verter_mcp` or
`verter_semantic::analysis::build`. The inventory swept `verter_compiler`, `verter_lsp`,
`verter_actions` and two `verter_semantic::analysis` modules; the FFI and MCP surfaces
were never swept. §1.1 anticipates exactly this ("it is not claimed exhaustive; a newly
discovered five-dialect CSS reader is presumptively in scope and needs a recorded
disposition against this same rule"), so adding the row is the charter's own prescribed
mechanism. *Confirmed by both seats.*

| ID | Requirement | Gate |
|---|---|---|
| A26 | The selector-structure surface carries no fallback re-parse: `AnalyzedSelector.structure` is authoritative, and no consumer re-parses `AnalyzedSelector.text` when it is absent. The `None =>` arms at `verter_napi/src/lib.rs:3236` and `verter_wasm/src/lib.rs:1459` are deleted, not wrapped; a selector with no structure is skipped exactly as it already is when the re-parse fails | **Both public boundaries are executed**, because they are two public surfaces and reviewing that both arms disappeared does not prove either result held. NAPI: `packages/native/index.spec.ts` — a style block with one structurable and one non-structurable selector produces an unchanged selector-match result, plus an exact assertion of zero `parse_selector` invocations while that request is served. WASM: the equivalent positive and exact call-count case against the `@verter/wasm` boundary, so a WASM-only regression cannot pass. The review-enforced no-reintroduction-under-another-name check is supplemental to both, never a substitute — automating it would need a name-keyed source scanner, which `CLAUDE.md`'s landed-scanner bar forbids |

## 6. A24's completeness check reads its own source (A32)

A24's gate is "the landing commit's diff is reviewed against the full §1.1 inventory".
**That is a check against the source it validates.** It cannot surface a reader the
inventory omits — which is how §4's re-parse survived eleven ratification rounds and why
§5's surface was never looked for.

This is not a J1 quirk. It is the third instance recorded in one session, in three
unrelated blocks, of one class: a verifier claiming more than its mechanism establishes,
and a structural-soundness result read as a passing one. **A check that enumerates from
the source it validates cannot fail for the case it exists to catch**, and it reads as
rigorous precisely because it always agrees with itself. This document committed the same
error twice — in its first draft's A27 and its second draft's A32 — which is the strongest
available evidence that naming the pattern is not the same as escaping it.

| ID | Requirement | Gate |
|---|---|---|
| A32 | Parse-once is enforced structurally, not sampled. `verter_css_syntax` exposes **one crate-public parse gateway**; every other public parse entry either becomes the gateway, becomes crate-private, or is recorded with its justification. **The forbidden-entry universe is derived from the crate's own export list, never from a hand-written copy** — a hand-written list is how the previous draft named six entries and missed `parse_style_body` (`lib.rs:56`, which calls `parse_style_ir` at `svelte_compat.rs:54-64`) and the publicly exported `Parser` (`lib.rs:36`) with its public `Parser::parse` (`parser.rs:181`), leaving a compile-fail proof that would pass with the surface wide open. The entries live at `lib.rs:25,34,36,38,49,56` today; the criterion is the derivation, not that enumeration. **J1 does not own a content identity and this row does not give it one:** `J1.md:229` assigns the canonical style content-identity/cache-key model to the downstream block, so the gateway constructs no key, owns no cache, and derives no identity. What J1 owns is the property it can own without one — **no second top-level parse entry per style block within a request** — and reuse is expressed by handing an already-parsed `StyleSyntaxIr` forward, the same shape A10i and A33 already use. Route coverage is likewise derived from an independently enumerated route universe: **Vue** SFC compile with a `<style>` block and **Svelte** SFC compile, as separate routes; the inline `style=""` read; the LSP CSS analysis request; the NAPI, WASM and MCP surfaces of §5; **and the four `verter_semantic::analysis::build` DOM-query routes** — the `DomQueryKind`
variants at `crates/verter_semantic/src/analysis/types.rs:810-816`, selected from four public
method names at `build.rs:1901-1906`; two of them share one parse expression today, which does
not merge two separately selectable routes | Three parts. (1) **Structural, over the COMPLETE surface:** one `trybuild` compile-fail case per forbidden entry class in the derived universe, plus an assertion that the derived universe and the crate's actual public parse exports agree exactly — so an entry added later joins the universe instead of escaping it. One case for a chosen entry proves only that its own selection is private while its siblings stay exported. (2) **Per-route parity:** one executed case per route in the enumerated universe, each asserting one top-level parse entry per style block, each with its own negative control proving it reddens when its own route parses twice. **A single fixture over a subset of routes is explicitly NOT sufficient** — a reader on an unexercised route need not affect it, which is A24's error repeated. (3) **Forward-handoff:** a route that already holds a parsed `StyleSyntaxIr` and needs it again charges zero further parse entries, asserted by call count rather than by cache inspection, so the criterion stays inside J1's mandate. **Stated exactly:** the assertion is one TOP-LEVEL parse entry, not one `parse_with_sink` execution, because the Sass and Stylus layout grammars subparse internally by design. A24's landing review is retained as the qualitative half; §9 extends A24's own union text so the two do not contradict each other |

## 7. The CSS attribution outcome and gate (A27)

`performance-gates.toml:181-185` asserts three counters are zero:

```toml
zero_counter_assertions = [
  "compiler.css_parse",
  "compiler.css_transform",
  "compiler.style_analysis",
]
```

Each has exactly one production charge site: `crates/verter_compiler/src/css/mod.rs:69`
(`attribute_n!(CssParse, css.len())`), `:96` (`attribute_scope!(CssTransform)`), and
`crates/verter_compiler/src/svelte/runtime/css/mod.rs:155`
(`attribute_scope!(StyleAnalysis)`). **Two of the three live in the tree J1's own A2
requires deleted.** Once A2 lands, `compiler.css_parse` and `compiler.css_transform`
become structurally unchargeable and their zero assertions keep passing because nothing
*can* charge them — a gate that cannot fail for the case it exists to catch.

`performance-gates.toml:177-180` already names this failure mode for other counters and
excludes them for it ("They record zero because this workload's lane does not reach them,
which is a known gap, not a requirement. Freezing a gap as a gate would make a later
block's correct fix fail"). A2 silently moves two counters into that category while
leaving them asserted.

**The gate's universe is derived independently of the file it validates** — the `Css`
domain of the attribution schema
(`crates/verter_audit/src/attribution/schema.rs:274-276`), which enumerates
`CssParse`/`CssTransform`/`StyleAnalysis` without reference to any gate file.

| ID | Requirement | Gate |
|---|---|---|
| A27 | Every CSS-domain attribution counter remains chargeable by a production path at the tree that deletes `crates/verter_compiler/src/css/` (A2). `compiler.css_parse` and `compiler.css_transform` are rehomed onto the surviving shared parse/transform owner **in the same change that deletes their current sites**, not as a follow-up. **Forbidden: a counter left asserted zero with no production site able to charge it** | Two independent assertions. (1) **Chargeability, universe from the schema:** for every counter in the attribution schema's `Css` domain, a workload performing the work that counter names charges it at least once, with attribution enabled. Deleting a name from `zero_counter_assertions` does not shrink this universe, so a wrongly-removed counter still fails. (2) **Coverage:** every name in `zero_counter_assertions` resolves to a counter in that schema domain. The pair fails on a tree where A2 deleted the sites with no rehome, and it is not a source scanner — it reads a typed schema and exercises production |

**If the surviving path genuinely performs no CSS parse or transform**, the counters
belong in the documented not-asserted-zero list instead — but that edits an A6-locked
file whose own header restricts change to the recalibration procedure (`verification.md`
8.1, a new Implementation Lock Record digest, the same independent review class). **That
route is not J1's to take unilaterally and this amendment does not authorise it**; J1
stops and escalates under §7 of its charter, and the criterion is satisfied only by
rehoming or by a completed recalibration.

## 8. The Bounds enter the acceptance closure (A28-A31, A33, A34)

§2's Bounds are ratified charter text and say so on their face: "Material, numeric,
checked at landing — not deferred to implementation discretion." Six are stated. **Only
part of one is bound to an acceptance ID.** Since acceptance is defined as coverage of the
acceptance IDs, most of a ratified numeric contract sits outside the closure that defines
acceptance. **A ratified bound that no acceptance ID reaches is not a requirement; it is a
sentence.**

Giving them IDs adds no obligation. It makes an already-ratified one countable. Two
corrections from the review rounds are folded in: Warm's cross-invocation half is a
non-regression obligation, not a clarification, and Cold is only half-bound — A10i covers
the Vue `1 + K` cascade, while the charter's separate requirement that a byte-changing
external-preprocessor result add exactly one parse, for a worst case of four
([`../charters/J1.md`](../charters/J1.md) line 335), is bound by nothing.

| ID | Requirement | Gate |
|---|---|---|
| A28 | Edit topology (§2 Bounds): a 0-edit style block returns `StyleRewriteOutcome::Unchanged` **before any `CodeTransform` is constructed**; an edited block calls `build_string()` exactly M times for the construct's edit-composition depth, **for every A10/A10d-h transform category** | The category universe is enumerated independently from §2.1's A10 row and the A10d-h rows — plain passthrough, v-bind, scoped, `:deep`, `:global`, `:slotted`, `:is`/`:where`, keyframes, CSS nesting, modules, **no modern-syntax normalization** (named separately in A10 at `J1.md:301` and omitted from the previous draft's list, which is why "complete universe" is asserted against the charter rows rather than against a hand-written list), and the five dialects — not from the existing test's own coverage. The existing `style_planner.rs::build_string_call_count_matches_edit_composition_depth` covers three of them (flat scoped, `:deep`, `:slotted`); the rest are added, each with a per-category negative control. The zero-edit half is NOT covered by `::zero_edit_style_block_returns_unchanged_variant` (`direct_result_tests/style_planner.rs:842-880`), which asserts only the outcome variant and so passes code that constructs a `CodeTransform` and discards it: a construction probe — the same shape as the existing `build_string` counter — asserts **zero constructions on every zero-edit route**, with a proven mutation that constructs one and reddens it |
| A29 | Allocation ceiling (§2 Bounds): the converged pipeline's per-category allocation count is at most 1.2x the legacy per-category baseline | **Neither half exists today and both are part of this criterion.** `evidence/J1/perf-baseline.md` records the allocation baseline as *"Deferred"*, and `crates/verter_compiler/tests/allocator_canaries.rs` states in its own text that its assertions "do not freeze a ratio ceiling". A29 requires the legacy per-category counts committed as retained values; executed, non-`#[ignore]`d converged canaries; and a per-category assertion that each ratio is at most 1.2x. The category universe is derived from the `css_bench.rs` generators, not from the canary file's own list. **This is also the acceptance ID the ratified DEFER of `J1-CSS-ALLOC-001` requires and currently lacks** — `evidence/J1/debt-J1-css-parse-allocation-ceiling-residual.md` names its resolution gate but no ID, because none existed |
| A30 | Fan-out (§2 Bounds): the Rust style path introduces no new cross-process, filesystem or network call for preprocessing | A14's type-state gate is the structural half — no path passes raw SCSS/Sass/Less/Stylus bytes to the transform, so no site can reach for a preprocessor. The delta half is **review-enforced at landing** and named as such: the diff introduces no `std::process`, `std::fs` or network call on the Rust style path. A signature or `trybuild` proof is explicitly **not** sufficient (a function taking no handle, path or provider can still call `std::fs`), and a source scanner is barred by the landed-scanner rule — which is why review enforcement is correct here rather than convenient. *Round 2 confirmed this is permitted.* |
| A31 | Latency ceiling (§2 Bounds): the converged pipeline's parse+transform wall-clock is at most 1.2x the committed pre-convergence baseline, for **every** `css_bench.rs` benchmark identity | An **executable per-category comparator**, not a benchmark invocation: `cargo bench` succeeds whether or not the ceiling is met. The comparator derives the benchmark-identity universe independently from `crates/verter_bench/benches/css_bench.rs` (its generator functions and the `BenchmarkId`s they produce), then requires **exact set equality** between that universe, the committed baseline record, and the candidate run, before comparing ratios — so a category missing from either input fails rather than passing quietly. Three negative controls: a missing category, an extra category, and one category over 1.2x. Deletion of the legacy pipeline (A1/A2) does not land before the comparator has run green on the converged tree |
| A33 | Warm (§2 Bounds), cross-invocation half: a style content identity already parsed costs **0 additional `parse_style_ir` calls** on a later compile invocation | A `parse_style_ir` call-count assertion across two compile invocations over the same unchanged style block: the first charges one parse, the second charges zero additional. A non-regression criterion — the bound asserts the existing cost model is preserved — which fails if convergence regresses cache reuse, something "unchanged behaviour" prose cannot do. Its intra-cascade sibling stays A10i |
| A34 | Cold (§2 Bounds), external-preprocessor half ([`../charters/J1.md`](../charters/J1.md) line 335): a byte-changing external-preprocessor result is a distinct content identity that adds **exactly one** further parse, and the worst case — a non-CSS dialect with all three Vue stages present and rewriting — totals **four** | An executed `parse_style_ir` call-count over the sealed round-trip: the preprocessed-result identity adds exactly 1, and the worst-case scenario totals exactly 4. A14 proves the boundary's shape and type-state, not this count, and A10i counts only the Vue cascade — so neither reaches it. *Found by round 2 inside the closure this document proposed, which is why the sweep is reported as nine further items rather than eight* |

## 9. The charter edits

Two corrections to §1.1, the single-source table:

1. **Row 13** (`verter_semantic::analysis::style_syntax`) — "Parses/re-derives?" changes
   from *"Consumes row 1 only"* to record the nested selector re-parse; disposition changes
   from **Preserve — already correctly scoped** to **Converge**; acceptance ID changes from
   `n/a` to `A25`. The prior classification was factually wrong on the tree it was ratified
   against, which this amendment records rather than quietly overwrites.
2. **A new row** for the selector-structure surface across `verter_napi`, `verter_wasm`,
   `verter_semantic::analysis::build` and `verter_mcp` — **Converge** for the two fallback
   re-parse arms, **Preserve** for the four first-parse consumers, acceptance ID `A26`.

Four edits elsewhere, **without which the charter contradicts its own new rows** — a
half-applied amendment is worse than none, because it hands acceptance reviewers two
authorities:

3. **Line 11's "seven readers in and three out"** — replaced by a reference to §1.1's rows,
   the same non-numeric form rounds 8-10 imposed on the other stale counts.
4. **A24's union at line 325** — extended with A25, A26, A27 and A32, so the union text and
   A32's row agree; and its verification bullet in §5 gains A32's executable half beside
   the retained landing review.
5. **Lines 410-414's claim** that `verter_semantic::analysis::style`/`style_syntax` "remain
   direct `StyleSyntaxIr` consumers with no private bypass of their own" — false on the
   tree for the reason row 13 now records; replaced by a reference to row 13's disposition.
   The same bullet's "Reconfirm during implementation" becomes A32's route enumeration,
   which is what reconfirmation now means operationally.
6. **Lines 356 and 422's present-tense claim** that "no committed wall-clock baseline exists
   today" — false since `evidence/J1/perf-baseline.md` landed, and directly contradictory to
   A31, which requires comparison against exactly that committed baseline. Both are replaced
   by a reference to the committed baseline and the A31 comparator that consumes it.

And the ten new rows are added to §2.1's table (A25-A34), keeping `A18` reserved.

No other row's disposition value changes, and no row is restated outside §1.1 — the
single-source structure rounds 8-10 enforced is preserved.

## 10. Non-blocking discoveries — recorded, not bound

Per `governance.md` §11, and deliberately **not** given acceptance IDs, because neither is
a parse-once violation and binding them here would expand J1's scope on this block's own
authority:

- `DISC-ARCH` — `parse_selector_authority` (`style_syntax.rs:37-39`) synthesises
  `"{selector}{} "` and runs a full `parse_style_ir` to recover one selector, where
  `parse_selector_structure` is available and is what `style_syntax.rs:253` already uses.
  The wrapping technique itself is sanctioned and landed —
  `parse_inline_style_declarations` (`crates/verter_css_syntax/src/inline_style.rs:72-92`)
  uses it deliberately and cites this function as its precedent, because the shared grammar
  exposes no bare-list entry point. One parse of wrapped text is not a re-parse; the
  observation is the heavier entry point.
- `DISC-ARCH` — `verter_semantic::analysis::style::extract_var_references`
  (`analysis/style.rs:945-951`) is `pub` with **zero** production callers workspace-wide;
  its only callers are that crate's own tests. A deletion candidate under the charter's §6
  discipline, not a live reader.

## 11. Residuals routed upward, not assumed

Two items have no owner and no resolution point, and neither is a block owner's to settle.
A third — the binding ruling's stale acceptance count — is now SETTLED and is recorded here
rather than routed:

- **SETTLED: the ruling's count is an erratum, not an edit.**
  [`../rulings/MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md`](../rulings/MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md)
  §3 says J1's acceptance completes when "every one of its 41 acceptance IDs is covered"
  (line 61, one occurrence). The operative count is **38**. The ruling text STANDS: a miscount
  is not a decision, and a maintainer ruling is not edited to fix one. The correction is an
  erratum on J1's ledger row — operative count 38 — which closes the fail-open exposure
  without touching the ruling. **It IS recorded**, in the notes field of J1's ledger row
  (`program-state.toml:1553`): *"ERRATUM — THE OPERATIVE COUNT IS 38, NOT THE 41 THAT RULING
  STATES"*, with the count derived independently from the charter. §13 step 0 is therefore
  MET. This amendment neither amends the ruling nor writes around it.

  **A previous revision said the opposite, and how it got there is worth more than the
  correction.** It searched the row for the bare token `41`, found two occurrences, and
  concluded no erratum existed — but one of those occurrences is the erratum *quoting the
  figure it corrects*, so the very evidence of the record was read as proof of its absence.
  The check was also run against a base that predated the write, so its scope excluded the
  fact it was asked about. Either error alone produces the false claim. **A search for a
  token cannot answer a question about a record**, and a check whose scope is narrower than
  its conclusion is the defect this document names three times in other people's work. A follow-on claim in an earlier revision — that a
  stale figure survived in a comment on the same row — is ALSO false: that comment reads 38
  (`program-state.toml:1532-1533`). **That is three false statements about this one ledger
  row in this document's history, every one of them relayed rather than read.** The pattern
  is the finding: a fact arriving through a channel is not a fact read on the tree, and this
  document has now been wrong in both directions — asserting a record absent when present,
  and asserting a stale figure present when corrected.
- **Two conformance gates are pinned to versions the workspace no longer resolves**, and
  correcting them is a recalibration, not an edit. Affected IDs and fixtures, enumerated so
  an eventual recalibration cannot omit affected evidence: `svelte@5.56.3` is named by
  **A11c** (line 309), **A16** (line 317), §4's mandatory parity gate (line 554) and §7's
  abort trigger (line 694), while the tree's goldens carry `"oracleVersion": "5.56.10"` and
  root `package.json` pins `svelte 5.56.10`. `@vue/compiler-sfc@3.5.34` is named by **A5**
  (line 296) and by §3.1's prose and oracle table (lines 438, 452), whose literals **A6**,
  **A7** and **A10d-h** all compare against; root `package.json:82` pins `3.6.0-rc.5`, so
  the charter's own reproduction command run at the repository root produces a different
  compiler's output than the table records. **This amendment does not correct them.** These
  are ratified compatibility gates and captured corpora, so `governance.md` §4.1 applies:
  written cause, retained old and new calibration data, an independent reviewer, an
  Implementation Lock Record amendment, and rerun of affected evidence. A
  "resolve-it-from-the-workspace" rule was considered and is **rejected** — a floating pin
  lets a routine package update move a ratified standard silently. The correction must name
  exact replacement versions.
- **Runtime CSS Modules ownership** (§1.1 row 19, §4 "Row 19 — required work": "that
  decision requires its own ratification, tracked as an open question"). Recorded in
  [`../rulings/ARCH-RULING-J-TRAIN-FIVE-FORKS.md`](../rulings/ARCH-RULING-J-TRAIN-FIVE-FORKS.md)
  as one of four deliberately-undecided open questions, and A10b already forbids presenting
  it as Native while it stays open, so nothing is at risk today. What is missing is a block
  that resolves it. *Both seats agreed it needs no binding here.*

Two further items are already on the ledger row: the removal work is not yet reconstructed
onto the landed result, and one fresh review of the final frozen candidate by an agent that
has not seen it is owed. **Two** donor branches remain partly unlanded, not one, while the
ledger note enumerates only the later of them. Spot-checked here: neither `apply_span_edits`
nor `measure_converged` — two functions the earlier branch adds — exists anywhere under
`crates/*/src`, against a control (`run_vue_style_cascade`, present) proving the probe can
return non-zero.

## 12. Ratification

Ratification is sought from the architecture seat under the maintainer's standing
delegation, in the falsification form this program uses: **is this amendment ratifiable,
and what blocks it** — not confirmation of a conclusion. Receipts are filed under
`verify/results/J1/<reviewed-sha>/` and validated with
`scripts/orchestration/check-results.mjs`. A `RESULT: FAIL` is a structurally sound result
and is read as a verdict, not as form.

**The cap is reached and the decision is taken.** Rounds 1, 2 and 3 all returned `FAIL`. The
rule's exits at the cap are to stop and rescope, replace the agent, or request a decision; a
decision was requested and the seat prescribed a rescope. §17 records it. This document does
not seek ratification on its own — it ratifies together with the companion proof contract that
rescope requires.

## 13. Application

**Neither instrument is a landing candidate on its own.** They accumulate on J1's single
integration line together with the rest of J1's open work, and that line lands once as one
accepted J1. Landing a piece of J1 separately is what produced the state these instruments
exist to correct, so the steps below describe how the pair is APPLIED and registered, not a
separate landing. No squash, no fast-forward, no independent candidate.

The amendment declares its edits and does not apply them, following AMD-012 and AMD-014.
Round 2 established that the previous recipe could not pass its own validator; this one is
ordered so that it can, and every step names its owner.

0. **PRECONDITION — MET.** J1's ledger row carries the acceptance-count erratum in its
   notes field (`program-state.toml:1553`): operative count 38, against the 41 the binding
   ruling states (§11). The ruling itself is not edited. This step was recorded as unmet in
   a previous revision on a mis-scoped check; it is met.
1. Ratify: the `**Status:**` line becomes RATIFIED with its date and authority. This is
   **first**, because `scripts/validate-program-state.mjs:2382-2390` rejects a registered
   `AMENDMENT` whose status does not parse as ratified, and because every digest below must
   be computed on the final bytes.
2. Apply §4-§8's ten acceptance rows and §9's six charter edits to
   [`../charters/J1.md`](../charters/J1.md).
3. Recompute `sha256(docs/arch/refactor/rev11/charters/J1.md)` and rebind it in **both**
   places that carry it: the `J1-CHARTER` `[[document]]` row at
   [`../../../architecture-lock/ledger/authority-registry.toml`](../../../architecture-lock/ledger/authority-registry.toml)
   line 439, and `charter_digest` on `J1`'s ledger row. Both are the program orchestrator's.
   The current value is `ad982473306df5a093526968de0d1c42fbe4f8f514372a35bb5d96e55791fa17`
   and matches both today, so leaving either stale is itself a violation.
4. Register this document as a `[[document]]` of `kind = "AMENDMENT"` with the sha256 of
   its **post-ratification** bytes, and add it to `J1`'s `[[authorization]]` closure,
   updating that entry's `documents` list and its `scope` text (line 449 and line 452),
   which still describes "seven readers converging under one parse".
5. Correct `J1`'s ledger row: the acceptance-ID count (§14), and the note's claim that the
   typed preprocessor fields are unbound — A14 binds them (§3).
6. ```
   node scripts/validate-program-state.mjs \
     --dag docs/arch/refactor/rev11/program-dag.toml \
     --state docs/arch/architecture-lock/ledger/program-state.toml \
     --mode live \
     --authority docs/arch/architecture-lock/ledger/authority-registry.toml
   ```
   `--authority` is required for the registry row, both digest bindings and the
   `**Status:**` parse to be exercised at all. Expected: the violation count is unchanged
   from before the change — which holds only if steps 1, 3 and 4 are all done, and is the
   reason they are enumerated rather than summarised.

No Cargo command is required to apply or verify this amendment. The ten gates are
implementation work for whoever implements them, and none is claimed executed here.

## 14. The acceptance-ID count

The binding ruling states **41**. The charter enumerates **38** (37 substantive; `A18`
reserved and unused) — measured by extraction with a control (§2), and independently
re-derived by all three ratification seats and by the decision seat. **38 is the operative
count**, recorded as an erratum on J1's ledger row (§11); the ruling text stands.

After this amendment the charter enumerates **48** (47 substantive). **This amendment writes
neither number** — both are ledger writes (§13 steps 0 and 5), recorded here so they are made
against a measured count rather than a quoted one.

## 15. What this does NOT do

- It does not accept, unlock, or move the status of `J1` or any other block.
- It does not write any field under `docs/arch/architecture-lock/ledger/`.
- It does not change `program-dag.toml`, any predecessor edge, or any block's class.
- It does not amend `performance-gates.toml`, any A6-locked cell, or the Implementation
  Lock Record, and does not authorise a gate recalibration. §7 and §11 name that procedure
  as the route where one is needed, rather than taking it.
- It does not amend any ruling. §11's first residual is precisely the case where a ruling
  needs correcting and this instrument declines to do it.
- It does not amend any other charter, contract, or capability-matrix cell, and it binds no
  other block's text: binding another block from inside a charter amendment would be this
  same defect in the other direction.
- It does not mint an ID for the typed preprocessor fields; A14 already covers them (§3).
- It does not weaken, reword, renumber or remove any existing acceptance ID or bound. `A18`
  stays reserved. A24's review half is retained and its union text is extended so the
  charter stays consistent.

## 16. Rounds 1 and 2, and what changed

Round 3 and the decision that followed it are in §17; this section covers the first two.

**Round 1** (`FAIL`, nine P1 and one P2) was right on all ten. Seven concerned gate quality
— criteria that could pass while their requirement went unmet, which is an unbound
obligation wearing a gate's clothes. It also caught the worst error: the Warm bound had
been *removed* rather than bound, on the grounds that it "asserts no change". A
non-regression obligation is still an obligation. And it caught A27 reading its test
universe out of the file it validates — §6's defect, committed by the document that names
it.

**Round 2** (`FAIL`, eight P1 and one P2) was right on all nine — including the one the
previous draft disputed. It found: A28 claimed
an existing test covered exact M when that test covers three categories of the ratified
universe; A31's comparator had no independent category-closure check, so omitting a
category made it pass; A32's privacy boundary proved callers use the gateway but not that
the gateway parses once, and its route list omitted the DOM-query routes this document had
itself named; the Cold bound's external-preprocessor half was unbound by anything (now
A34); the application recipe left the registry's charter digest stale and registered an
amendment still marked `PROPOSED`; the declared edit set left the charter self-contradictory
at three places; the stale ruling count turns fail-closed into fail-open once the ID set
grows; and the pin residual omitted affected IDs. Each is addressed above.

**Round 2's F1 STANDS. What is retracted is the previous draft's DISPUTE of it** — the
finding was correct and the dispute was not, and the two must not be confused. That draft
argued
the branch was reachable through the pseudo-element spelling, with a citation at every hop —
and every hop was true except the one it never checked: the dispatcher never routes a
pseudo-element component to that function at all. Execution settled it: 0 fires across 19
inputs and five dialects, against a control that fires 4 times, so the branch is dead code.
§4.1 carries the measurement and the retraction; A25 is rewritten around what is actually
there — an unenforced coupling between two files, which the deleted branch was silently
insuring against.

**The lesson is the one this document was written to serve.** A reasoned chain with a
citation on every hop still concluded the opposite of the truth, because it never checked
whether the function under discussion is ever called. That is the same shape as a check
that enumerates from the source it validates (§6): internally consistent, and unable to see
its own gap.

## 17. State at the cap
## 17. The rescope, and the state handed over

Three ratification rounds returned `FAIL` (10, then 9, then 6 findings) and the convergence
cap was reached. The cap's sanctioned exits are to stop and rescope, replace the agent, or
request a decision. A decision was requested, put unweighted to the architecture seat as two
routes — ratify the binding half and brief the gates, or run a fourth round on the whole —
with this document's own preference flagged as a position to test rather than the answer.

**The seat rejected both and prescribed a third route. That verdict is the recorded decision**
(lane `binding-split-decision`, receipt filed under this block's results directory).

**Why "ratify the binding, brief the gates" fails.** Gate specification is not private
implementation detail here. `governance.md`'s mandatory charter includes correctness/failure
proof and performance gates; `CLAUDE.md` requires a planned test or gate for every stable
acceptance ID before an implementation brief is dispatched; and J1 strengthens both locally —
§2.1's opening requires every row to name a concrete executing test or gate, and the charter's
own history records "named a suite but no concrete test function" as a ratification defect its
eighth round closed. Binding an owner and a requirement without binding the proof is
non-enforceable acceptance authority: the same defect this instrument exists to end, one step
later. A companion contract would need its own ratification and registry row anyway, so the
split saves no necessary work.

**Why a fourth round on the whole fails.** The finding counts narrow but the defects repeat:
A25, A28 and A32 were rejected in all three rounds, and the seat identified a cross-cutting
defect a fourth round would find anyway — **the new rows do not meet J1's own concrete-gate
naming rule.** A25, A26, A27, A29 and A31-A34 name no concrete test function, and A26's
file-level reference plus an unspecified "equivalent" case reproduce precisely the
suite-without-function defect round 8 closed. Thematic repetition outweighs a declining total.

**The recorded rescope:**

1. This amendment stays **unratified**.
2. Its independently confirmed material carries forward: the inventory corrections, the
   bounds, the application ordering and its precondition.
3. A25's semantic scope and A32's public-surface model are corrected before their requirement
   text is treated as settled. **Both are corrected above** — A25 is now dead-path deletion
   plus a coupling invariant scoped to the dispatched kinds, binding no double-colon behaviour
   change; A32 derives its forbidden-entry universe from the crate's export list rather than a
   hand-written one, after that list was shown to omit `parse_style_body` and the public
   `Parser::parse`.
4. A **companion proof contract** maps every ID from A25 to A34 to exactly one named primary
   gate.
5. Those gates are held to: public-boundary positive-and-forbidden proof where observable;
   independently tree-derived universe parity; per-surface applied negative controls;
   exact-set performance comparison; and structural enforcement rather than name-keyed
   scanners.
6. The binding amendment and the proof contract are **ratified and registered together**, and
   only then is the implementation brief dispatched.

Items 4 through 6 need a second instrument, and its identifier is allocated centrally rather
than taken from the next free number. Until that identifier is issued, this document is
complete as far as it can go and correct as far as it has been checked — three rounds plus a
decision seat agree on every fact it asserts, and the two requirement texts the decision named
are corrected. What it does not yet carry is the proof contract that makes its rows
enforceable, and it does not claim otherwise.

## 18. The joint round

The pair's first joint review returned `FAIL` with five P1 findings and one P2 — **all five
P1 on the proof contract, none on this instrument's binding half.** That seat independently
re-derived and confirmed the acceptance-identifier counts, the six selector call sites, the
three attribution charge sites with two inside the deletion tree, the deferred allocation
baseline, the canaries that freeze no ratio, the outcome-only zero-edit test, and the two
public parse entries a hand-written list had missed. It found **no additional unbound
J1-owned obligation**, confirmed A30's review enforcement is permitted rather than a second
authority, confirmed A29's missing baseline is an owned prerequisite rather than a
ratification blocker, and confirmed the joint application order in §13 is validator-compatible.

Its P2 landed here and is corrected: §16 covered two rounds while the introduction said it
covered all of them, and it said round 2's F1 "was retracted" when what was retracted is the
previous draft's DISPUTE of F1 — the finding itself stands. Confusing a retracted challenge
with a retracted finding would invite a later reader to reopen a correct result.

The five gate defects are the companion contract's and are corrected there: A25 lacked the
semantic-boundary parity its own requirement text demands and coupled two lists rather than
binding one typed name authority both production paths consume; A28 named no applied
discriminator for a wrong edit-composition depth; A31 compared data without binding the
production of fresh candidate evidence to the tree under test; A32 placed its route
supplement in `verter_session`, below the four public surfaces that depend on it and which it
therefore cannot execute; and A33 named a compiler module that drives the uncached
pre-assembly entry, so it would have been red against a satisfied bound or pushed J1 into
building downstream-owned cache state.

**The second joint round cleared this instrument.** It returned `FAIL` with three P1
findings, all on the companion contract, and recorded that **`AMD-016` has no remaining
blocking defect**. The three were gate-discrimination defects of a kind worth naming
because two of them are this document's own stated rules turned back on it: an applied
mutation that removes a name from a DERIVED universe shrinks the test set along with
production, so the gate stays green — self-enumeration reached through a derived list
rather than a hand-written one; a second mutation could not execute the branch it
restored, because the same criterion's totality invariant keeps that branch unreachable;
and a route universe was asserted at three when the live typed enum carries four, since
two variants sharing one parse expression are still two separately selectable routes. All
three are corrected in the companion contract.

**The third joint round returned `FAIL` with four P1 findings, and the cap for this pair is
now spent** — two fix cycles, three joint rounds. Two of the four were this document's own
inconsistencies rather than design failures: the binding half still required three
DOM-query routes while the proof half and the typed source carry four, and this text
asserted the operative-count erratum was already on the ledger row when it is not. That
second one is the sharper lesson and is recorded rather than quietly fixed: **the claim was
taken on report and written as present-tense fact without being read on the tree**, which is
the defect this instrument spends its length describing, committed by the instrument. The
row still reads 41 at `program-state.toml:1525` and `:1546` with no erratum, so §13 step 0
is unmet today.

The other two were real gate gaps: a latency runner that bound tree provenance but not the
measurement environment, so a regressed candidate on a faster machine or a different
sampling mode passes a 1.2x check; and a route universe left as prose, collapsing three
distinct inline consumers the charter's own inventory separates (VDOM row 7, SSR row 8,
semantic extraction row 9) into one unnamed route. All four are corrected. **They are
corrected but UNREVIEWED**, and no further round is dispatched on this block's authority.

**The fourth joint round verified the built evidence rather than reading the claim about
it, and that is the change that mattered.** It confirmed on the implementation branch that
the shared typed name authority exists and both production paths consume it, that the
semantic mapping is genuinely exhaustive, that the named primary gate and all five route
cases exist, and that the in-run control is really asserted. Its three findings were text
defects, not design ones — a different failure mode from rounds 1-3, which is why a further
round was authorised rather than the cap being read as a stop.

Two of the three were this document's, and both are the classes it exists to name. It
claimed one criterion had been "constructed and measured" when half of it was never built,
while the same document disclaimed elsewhere that anything had been built — **a planned gate
presented as a measured one, inside the instrument written to end exactly that**. And it
excluded a live double parse as out of scope when A32's own requirement forbids precisely a
second top-level entry within one request; **being wrong cautiously is still being wrong**,
and the exclusion would have left the gate passing over the case it exists to catch. The
third was a stale function name in the coverage map, which the contract makes binding.

A fourth correction is recorded in §11 rather than here, because it is the same class again
and it was this document's: it asserted a ledger erratum was missing after searching for a
bare token on a stale base, when the record was present and one of the token hits WAS the
erratum quoting the figure it corrects.
