# AMD-019 — proof contract for J1's newly bound obligations

**Status:** RATIFIED 2026-08-25 by the architecture seat under the maintainer's
standing delegation, jointly and atomically with its binding half
[`AMD-016-j1-open-obligations-bound.md`](AMD-016-j1-open-obligations-bound.md).
The ratifying verdict is the `joint-ratification` lane receipt at reviewed sha
`bb22b5c81fdbf7d308d20d694c8554cdb669e533` — `RESULT: PASS`, zero P0/P1, one carried P2
against this document (§6). **That receipt binds the PROPOSED bytes it reviewed**
(`535e9cabba360ae61b1c6b52e7e52cd3378a3e45c23017f4a2d181acec25169f`); this line is §13
step 1 of the binding half, applied after that verdict, so the bytes registered in the
authority registry are the post-ratification ones and differ from the reviewed digest by
exactly this paragraph. See §6.

**This instrument does not stand alone.** It is the companion proof contract the
recorded rescope requires, and it ratifies and registers **atomically** with
[`AMD-016-j1-open-obligations-bound.md`](AMD-016-j1-open-obligations-bound.md).
Either both ratify or neither does: a binding without proofs is non-enforceable
acceptance authority, and proofs without their binding name nothing.

**Prepared against:** local `program/architecture-lock` commit
`e70e7519b936ae535d9c0ced223e567bb472f871`, tree
`830f27c040e9debfd902185e5f52d21032f7363f`. Every `file:line` citation was read
directly on that tree. **This instrument changes documents only** and specifies
work it does not perform; no gate below is claimed executed, and none is claimed
to exist yet unless §3 says it already does.

**Amends on ratification:** nothing by itself. It supplies the Gate column
content for the ten rows `AMD-016` adds to
[`../charters/J1.md`](../charters/J1.md) §2.1, which are applied in the same
change as that amendment's own edits.

---

## 1. Why this exists

Three ratification rounds rejected `AMD-016` and the convergence cap was reached.
A decision seat then rejected both proposed exits — ratify the binding and brief
the proofs, or run a fourth round on the whole — and prescribed this instrument.
Its reason is the contract's own governing sentence:

> **Binding a requirement without binding its proof is non-enforceable acceptance
> authority — the same defect the amendment exists to end, one step later.**

That is not a procedural preference. `governance.md`'s mandatory charter includes
correctness/failure proof and performance gates; `CLAUDE.md` requires a planned
test or gate for every stable acceptance ID **before** an implementation brief is
dispatched; and J1 strengthens both locally — §2.1's opening requires every row
to name a concrete executing test or gate, and the charter's own header records
"named a suite or module but no concrete test function" as the defect its eighth
ratification round closed. `AMD-016`'s rows reproduced that defect: most named no
concrete function. This instrument closes it.

## 2. The standard every gate below is held to

Set by the decision seat, and applied uniformly:

1. **Public-boundary proof where the outcome is observable at one** — a positive
   assertion of the required result AND of the forbidden or fail-closed result.
   A unit or selection test supplements it and never substitutes for it.
2. **Universe parity is independently tree-derived** — where a gate iterates a
   set (routes, categories, entries, counters), that set is derived from the tree
   or from a typed schema, and the derivation is asserted to match the live
   source. **A hand-maintained list is a check that matches its own source.**
   This rule is written because `AMD-016`'s own draft enumerated a public parse
   surface by hand and missed two live entries.
3. **Per-surface applied negative controls** — every gate names the mutation that
   makes it fail, and the mutation is proven to have APPLIED before its red is
   believed. A plant that silently fails to apply reports a pass.
4. **Exact-set comparison for performance gates** — a ratio comparison runs only
   after both sides' identity sets are proven equal; a missing category fails
   rather than passing quietly.
5. **Structural enforcement, never a name-keyed scanner** — per `CLAUDE.md`'s
   landed-scanner bar. Where an invariant genuinely cannot be automated
   structurally, the gate is declared review-enforced and says why.

**One declared primary gate per ID**, per `CLAUDE.md`'s "exactly one declared
primary gate" rule. Where an ID spans two public surfaces, the second surface
carries a **required supplement** — named, with its own discrimination — and the
ID is not covered until both pass. A supplement never substitutes for the
primary; it covers a surface the primary structurally cannot reach.

## 3. The contract

Rust integration cases live under `crates/<crate>/tests/cases/` and are wired
through that crate's single `tests/main.rs`, per the anti-binary-growth layout
rule; no new top-level `tests/*.rs` target is created by any row below. A31 adds one
`src/bin` target, which that rule does not govern — it bounds integration-test binaries,
not ordinary bins.

### A25 — dead-path deletion and the coupling that keeps it dead

**PARTLY BUILT — and the split is stated exactly, because an earlier revision of this
line said "everything in this row was constructed" and that was false.** BUILT AND
MEASURED: the shared typed name authority, both production consumers, the exhaustive
semantic mapping, the derived dialect authority, the primary gate, its three applied
mutations and its in-run control. STILL
PLANNED, not built and not measured: the semantic supplement
`special_pseudo_nested_facts.rs::nested_special_pseudo_facts_unchanged_and_no_secondary_parse`
and its two-plant evidence. A planned gate is legitimate here (§5); presenting one as
measured is not, and that is the defect this instrument exists to end.


**One typed name authority, not two lists agreeing — LANDED.** The special-pseudo
name set is now `SpecialPseudoName` (`crates/verter_css_syntax/src/special_pseudo.rs`:
`Deep`, `Global`, `Slotted`, with `ALL`, `as_str`, `matches_css_identifier`), consumed by BOTH
production paths — `is_selector_list_pseudo`'s classification
(`parser.rs:1534-1537`, a literal list today) and `collect_special_pseudo`'s
recognition (`style_syntax.rs:228-233`, an inline `match` today). With one source
the coupling is structural: a name cannot exist for recognition and be absent from
classification, because there is only one place to add it. The semantic side maps the name
through an EXHAUSTIVE match, so a fourth variant fails compilation until it is classified —
the coupling is a compile error before it is ever a test failure. The invariant test below
guards the property rather than substituting for the structure.

- **Primary gate, BUILT:** `crates/verter_css_syntax/tests/cases/special_pseudo_typed_list_totality.rs::every_special_pseudo_name_parses_with_typed_selector_list_in_all_dialects`
- **What it asserts:** for every name in the shared authority, over the kinds
  `collect_special_pseudo` is dispatched for (`PseudoClass`, `FunctionalPseudo`)
  and all five dialects, a functional occurrence has `selector_list() == Some`.
- **Universe derivation, BOTH halves, and the second half was wrong until it was
  fixed.** The name set is read from the shared typed authority production consumes.
  The dialect set is `CssDialect::ALL`, generated from the SAME token list that
  generates the enum's variants (`crates/verter_css_syntax/src/dialect.rs`, the
  `css_dialects!` macro): a variant exists **iff** it is in that list, so "added to
  the enum but missing from the universe" is unrepresentable rather than merely
  discouraged. An earlier revision claimed this and a review round proved it false —
  the test then built its dialect array by hand, and replacing one dialect with a
  duplicate of another still compiled, still reported the full assertion counts, and
  left that dialect untested. **A restated array beside an exhaustive match reads as
  derived and is not**: the match only examines values the array already holds.
- **Negative control, with a known answer:** `:foo(.a .b)` must VIOLATE the same
  predicate inside the same run — it is a dispatched `FunctionalPseudo` with no
  typed list and a non-empty argument span, measured directly on this tree. A run
  in which the control does not violate it is a broken gate, not a pass.
- **Applied mutation — NOT removal of a name.** Removing a name from the shared
  authority removes it from production AND from the gate's universe at once, so the
  gate iterates a smaller set and stays green: the self-enumeration failure §2
  forbids, reachable through a derived universe as easily as a hand-written one.
  The mutation instead keeps the name in the universe and breaks its PRODUCTION
  behaviour — make `is_selector_list_pseudo` return false for exactly one retained
  name, so that name is still iterated and no longer satisfies the predicate. The
  gate must redden for that name alone. Prove the edit is present and unique first.
- **Required supplement — semantic behaviour parity, which the primary cannot
  reach. PLANNED, not yet built:** `crates/verter_semantic/tests/cases/special_pseudo_nested_facts.rs::nested_special_pseudo_facts_unchanged_and_no_secondary_parse`
  — asserts the nested `:global(.a .b)`, `:deep(.a, .b:hover)` and `:slotted(.a)`
  facts are byte-identical to the pre-change results (positive, the parity
  `AMD-016` A25 requires) AND that `parse_selector_structure` invocations
  originating in `style_syntax` are 0 while the canonical `project_style` parse
  remains exactly 1 (forbidden). A typed-list unit test in the parser crate cannot
  observe either; it is the primary because it guards the structure, not because
  it covers the outcome.
- **Applied mutation for the supplement — TWO plants, both proven applied. NOT YET RUN.**
  Reintroducing the fallback alone does not execute it: the totality invariant this
  same ID installs makes `selector_list()` present for every dispatched special
  name, so the restored branch is never entered and the count stays zero. The
  supplement is exercised only by restoring the fallback AND independently forcing
  one retained special name to lack its typed list. With both applied the
  secondary-parse count must redden while the nested-fact assertions stay green,
  which is what proves the two halves discriminate independently rather than
  moving together.
- **MEASURED, on the tree.** Unmutated: PASS, with per-run coverage counts (45 functional
  cells, 15 bare, 15 controls) so a silently-shrunk universe is visible in the output rather
  than inferred. Mutation applied to PRODUCTION consumption while leaving the universe
  intact — `is_selector_list_pseudo` made to skip exactly one retained name — proven present
  (1 occurrence in file, 1 repo-wide, new against the pristine tip; the marker cannot
  pre-exist because the type is new). Red output: *"':slotted(...)' parsed WITHOUT a typed
  selector list — the semantic re-parse fallback for special pseudos is now reachable
  (grammar/semantic name universes diverged)"*. Restored: marker count 0, PASS. **This is
  the mutation shape §2 demands**: removing the name instead would have shrunk the derived
  universe alongside production and stayed green.
- **The dialect universe's own mutations, measured.** Omitting a dialect from the
  authority fails the build with `E0599` naming the missing variant; substituting one
  dialect for a duplicate of another fails with `E0428` (duplicate definition) plus `E0599`
  for the variant the substitution removed — while APPENDING a duplicate, which retains
  every variant, fails with `E0428` plus the non-exhaustive-match `E0004`. An earlier
  revision recorded the appended-duplicate diagnostic against the substitution edit; the
  architecture is unaffected, since both edits fail to compile, but a recorded diagnostic
  that the stated edit cannot reproduce is not evidence. Both redden at COMPILE time rather than at assertion
  time, which is stronger than the gate required: there is no runtime state in which
  the universe can be short. Restored: green, 208/208 in the crate, fmt and clippy
  clean.
- **Control verified in the same run:** `:foo(.a .b)` is a dispatched `FunctionalPseudo`
  with a non-empty argument span and `selector_list() == None` — asserted 15 times per run,
  matching the standalone measurement recorded in `AMD-016` §4.1.
- **Scope fence:** neither gate asserts anything about the `::`-spelling. Typing
  that spelling's argument list would newly expose nested facts on a public
  surface; that is a behaviour change this ID does not bind.

### A26 — no fallback re-parse of an unstructurable selector

- **Primary gate:** `packages/native/index.spec.ts::"an unstructurable selector is skipped, not re-parsed"`
- **What it asserts (public boundary, both directions):** a style block carrying
  one structurable and one non-structurable selector returns the same
  selector-match result as before (positive), and zero `parse_selector`
  invocations occur while that request is served (forbidden).
- **Required supplement, second public surface:**
  `crates/verter_wasm/tests/cases/selector_structure_boundary.rs::wasm_boundary_skips_unstructurable_selector`
  — the same fixture and the same two assertions against the WASM binding, which
  the NAPI spec structurally cannot reach. The ID is uncovered until both pass.
- **Applied mutation:** restore the `None =>` re-parse arm in one binding at a
  time; each surface's gate must redden for its own binding and only for it.
- **Review-enforced supplement:** the arms are not reintroduced under another
  name. Automating this needs a name-keyed source scanner, which the
  landed-scanner bar forbids; it is declared review-enforced for that reason.

### A27 — every CSS-domain counter stays chargeable

- **Primary gate:** `crates/verter_session/tests/cases/css_attribution_chargeable.rs::every_css_domain_counter_is_chargeable_by_production`
- **What it asserts:** for every counter in the attribution schema's `Css`
  domain, a workload performing the work that counter names charges it at least
  once, with attribution enabled.
- **Universe derivation:** the counter set comes from the typed attribution
  schema (`crates/verter_audit/src/attribution/schema.rs:274-276`), **not** from
  `performance-gates.toml`. Deleting a name from the gate file therefore cannot
  shrink the tested universe — the defect this row's own first draft committed.
- **Required supplement:** `::asserted_zero_counters_resolve_to_schema_counters`
  — every name in `zero_counter_assertions` resolves to a counter in that domain,
  so a typo or a stale name fails rather than silently asserting nothing.
- **Applied mutation:** delete a charge site without rehoming it; the primary
  must redden for that counter alone.

### A28 — edit topology across the full transform universe

- **Primary gate:** `crates/verter_compiler/src/direct_result_tests/style_planner.rs::build_string_call_count_matches_edit_composition_depth`,
  **extended** to the complete category universe.
- **Universe derivation, typed:** the categories become a typed enum in the test
  module, and the primary iterates it through an exhaustive `match` so the
  compiler — not a list — fails the build when a variant is added without an
  expectation. Its correspondence to J1 §2.1's A10 row and the A10d-h rows is a
  **declared review-enforced parity step**, justified because deriving it by
  scanning charter prose would be a name-keyed scanner, which the landed-scanner
  bar forbids. The reviewer states which rows were compared.
- **Applied mutation, PER CATEGORY:** for every category the primary exercises,
  perturb its expected depth by one — and separately force one extra nested
  sub-span build in the production path — and require the primary to redden for
  that category alone. A single mutation somewhere in the suite is not this gate's
  control; an omitted category or a wrong M must be individually detectable.
- **Required supplement:** `::zero_edit_routes_construct_no_code_transform` — a
  construction probe, in the shape of the existing `build_string` counter,
  asserting zero `CodeTransform` constructions on every zero-edit route. The
  existing `::zero_edit_style_block_returns_unchanged_variant`
  (`style_planner.rs:842-880`) asserts only the outcome variant and passes code
  that constructs one and discards it, so it is retained but is not the gate.
- **Applied mutation for the supplement:** construct and discard a `CodeTransform`
  on one zero-edit route; the supplement must redden for that route.

### A29 — allocation ceiling

- **Primary gate:** `crates/verter_compiler/tests/allocator_canaries.rs::converged_style_pipeline_allocation_within_ratified_ceiling`
- **What it asserts:** per category, converged allocation count is at most 1.2x
  the retained legacy count.
- **Prerequisite this row owns:** the legacy per-category counts are committed as
  retained values. `evidence/J1/perf-baseline.md` currently records the
  allocation baseline as *"Deferred"*, and the existing canaries state in the
  file that they "do not freeze a ratio ceiling" — so neither half exists today
  and both are inside this ID.
- **Universe derivation and exact-set rule:** categories come from the
  `css_bench.rs` generators, not from the canary file's own list, and the ratio
  comparison runs only after the retained set and the measured set are proven
  equal.
- **Applied mutation:** inflate one category's converged count past the ceiling;
  the gate must redden for that category. Non-`#[ignore]`d, or the ID is
  uncovered.
- **Also closes:** the ratified DEFER of `J1-CSS-ALLOC-001`
  (`evidence/J1/debt-J1-css-parse-allocation-ceiling-residual.md`), whose
  resolution gate names this ceiling but no acceptance ID, because none existed.

### A30 — fan-out

- **Primary gate:** A14's type-state gate —
  `crates/verter_session/tests/cases/preprocessor_boundary_contract.rs`'s
  `trybuild` compile-fail case — as the structural half: no path passes raw
  SCSS/Sass/Less/Stylus bytes to the transform, so no site can reach for a
  preprocessor.
- **Declared review-enforced supplement, with its reason:** the delta introduces
  no `std::process`, `std::fs` or network call on the Rust style path. A
  signature or `trybuild` proof cannot establish this — a function taking no
  handle, path or provider can still call `std::fs` — and a source scanner is
  barred by the landed-scanner bar. Review enforcement is therefore the correct
  discipline here, not a convenience, and the reviewer states what was inspected.

### A31 — latency ceiling

- **Primary gate:** a named runner that PRODUCES the candidate evidence and then
  gates on it — `crates/verter_bench/src/bin/css_latency_gate.rs`, invoked as
  `cargo run -p verter_bench --bin css_latency_gate`. A comparator over data
  someone else produced is not a gate: it stays green over a stale artifact or a
  unit fixture while the live pipeline exceeds the ceiling.
- **What the runner does, in order, failing closed at each step:** executes the
  benchmark from the frozen tree; records the candidate artifact together with a
  **full measurement-protocol and environment identity** — tree object id,
  `css_bench.rs` blob digest, machine class, toolchain, target triple, cargo
  profile, feature set, criterion sampling mode, and the load and thermal policy
  under which it ran; refuses to proceed when ANY of those differs from the
  baseline record, or when the artifact is absent; then compares. Tree provenance
  alone is not enough: a regressed candidate measured on a faster machine, or under
  a different sampling mode, sits under 1.2x of an M3 baseline while every identity
  and tree check passes.
- **The committed baseline does not yet carry that identity.**
  `evidence/J1/perf-baseline.md:13-21` records a partial environment and a
  `--quick` sampling command. Recapturing or recalibrating it to the full protocol,
  through the governing procedure, is a prerequisite of this ID and precedes legacy
  deletion — named here rather than discovered when the comparator first refuses to
  run.
- **Exact-set rule:** the benchmark-identity universe is derived from
  `crates/verter_bench/benches/css_bench.rs` (its generator functions and the
  `BenchmarkId`s they produce — 42 identities at this base); the comparison runs
  only after that universe, the committed baseline record and the fresh candidate
  are proven to be the same set.
- **Six applied mutations:** a category missing from the baseline; a category
  present in the candidate and absent from the universe; one category over 1.2x; a
  candidate whose recorded tree provenance does not match; a candidate whose
  **machine class** does not match the baseline's; and a candidate whose **sampling
  mode** does not match. Each must redden, and each must be shown to have applied —
  the last two because an environment mismatch is the failure a tree-only
  provenance check cannot see.
- **Ordering obligation:** deletion of the legacy pipeline (A1/A2) does not land
  before this runner has exited green on the converged tree.

### A32 — parse-once, enforced structurally

**BUILT AND PROVEN for the session-reachable routes, and the tree corrected the
specification while it was built (§3.1).** Five routes are landed with one applied
double-parse mutation each; only the mutated route's assertion reddens, which is what
establishes that they are distinct routes rather than one surface counted five times.

**The two counters NEST — a correction, not a detail.** `parse_inline_style_declarations`
implements its parse by wrapping the attribute value in a synthetic rule and entering
`parse_style_ir` once (`crates/verter_css_syntax/src/inline_style.rs:87-92`). An earlier
draft of this row would have had the inline routes assert no style-grammar entry, which is
FALSE on a correct tree. The inline routes assert `inline == 1` AND `style_ir == 1` — the
single nested entry — which still reddens on any additional entry. Established by stack
trace, not by reading.


- **Primary gate:** `crates/verter_css_syntax/tests/cases/parse_gateway_closure.rs::public_parse_surface_is_exactly_the_gateway`
- **What it asserts:** the crate's public parse entries are exactly the gateway,
  plus any entry recorded with a justification; every other entry is
  crate-private.
- **Universe derivation, and why it is written this way:** the forbidden-entry
  universe is derived from the crate's own export list and the derivation is
  asserted to match, so an entry added later joins the universe instead of
  escaping it. A hand-written list produced exactly this failure in this
  instrument's own draft: it named six entries and missed `parse_style_body`
  (`lib.rs:56`, which calls `parse_style_ir` at `svelte_compat.rs:54-64`) and the
  exported `Parser` (`lib.rs:36`) with its public `Parser::parse`
  (`parser.rs:181`), leaving a compile-fail proof that would pass with the
  surface open.
- **Required supplements — one PER PUBLIC SURFACE, each naming its exact function,
  because a lower test cannot execute a higher binding.** `verter_lsp`,
  `verter_napi`, `verter_wasm` and `verter_mcp` all depend downward on
  `verter_session` (their `Cargo.toml` manifests at `:29`, `:17`, `:19`, `:21`), so
  a `verter_session` integration test cannot link or invoke any of them, and a
  second parse introduced solely in one of those crates would pass it. A file name
  is not a gate either — the charter's eighth round replaced module-level
  references with concrete functions for exactly this reason, and an earlier draft
  of this row named four files and no functions:
  * `crates/verter_session/tests/cases/one_parse_per_style_block.rs::each_session_route_charges_one_top_level_parse_entry`
    — the **Vue** SFC compile, the **Svelte** SFC compile, the **three distinct inline
    `style=""` routes** and the DOM-query routes below. The charter's own inventory
    separates the inline consumers into VDOM (`props.rs::emit_static_style_object`,
    row 7), SSR (`ssr/mod.rs::css_to_js_object`, row 8) and semantic extraction
    (`template.rs::extract_static_style_vars`, row 9); an earlier draft of this row
    collapsed all three into one unnamed "inline read", so a fixture exercising one
    while another parsed twice would have stayed green.
  * `crates/verter_lsp/tests/cases/one_parse_per_style_block.rs::lsp_css_analysis_request_charges_one_top_level_parse_entry`
  * `crates/verter_napi/tests/cases/one_parse_per_style_block.rs::napi_boundary_charges_one_top_level_parse_entry`
  * `crates/verter_wasm/tests/cases/one_parse_per_style_block.rs::wasm_boundary_charges_one_top_level_parse_entry`
  * `crates/verter_mcp/tests/cases/one_parse_per_style_block.rs::mcp_boundary_charges_one_top_level_parse_entry`
  The ID is uncovered until every one passes.
- **The manifest is keyed by ROUTE x PRODUCT SET, not route alone.** A request may ask for
  several products at once (`compile_request/mod.rs:147`), and the second parse this gate
  exists to catch appears only when two of them both need style work. The Vue supplement
  therefore runs every allowed product set that activates both the style cascade and the IDE
  path — runtime alone, IDE alone, and runtime+IDE together — asserting one top-level entry
  per style content in each, with **one applied double-parse mutation per configuration**.
  A single-product fixture cannot see this case; the five built route tests use one product
  each and are green over it today.
- **The outer route universe is a MANIFEST bound to tree inspection, not prose.** The
  routes are enumerated as a typed manifest the session case iterates exhaustively,
  and its correspondence to the live routes is established two ways: structurally
  where a typed source exists (the `DomQueryKind` variants below), and by a
  **declared review-enforced parity step** for the carrier and boundary routes,
  justified because deriving them would require scanning source for call sites,
  which the landed-scanner bar forbids. The reviewer states which routes were
  compared against §1.1's inventory rows. A route absent from the manifest is the
  escape this step exists to close — an earlier draft left the outer set as prose
  with no parity mechanism at all.
- **The DOM-query route universe is FOUR, derived from the typed enum.**
  `DomQueryKind` (`crates/verter_semantic/src/analysis/types.rs:810-816`) has four
  variants — `QuerySelector`, `QuerySelectorAll`, `GetElementById`,
  `GetElementsByClassName` — independently selected from four public method names
  (`build.rs:1901-1906`). An earlier draft counted three, because two variants
  share one parse expression today; sharing an expression does not merge two
  separately selectable routes, and a mutation conditional on one would not redden
  a fixture exercising the other. The session case iterates the enum exhaustively
  and parity-checks it against the live dispatch.
- **The five session-reachable routes are LANDED, each with its own proven mutation.**
  `crates/verter_compiler/src/direct_result_tests/style_parse_once.rs` carries four —
  `vue_sfc_compile_enters_style_grammar_once_per_style_block`,
  `svelte_compile_enters_style_grammar_once_per_style_block`,
  `vdom_compile_enters_inline_style_declaration_list_once_per_style_attr`,
  `ssr_compile_enters_inline_style_declaration_list_once_per_style_attr` — and
  `crates/verter_session/src/template_convert_tests.rs::template_convert_enters_inline_style_declaration_list_once_per_style_attr`
  carries the fifth. Each mutation was proven present, unique repo-wide and new, produced
  `left: 2, right: 1` on its own route, left the other four green, and was restored to a
  marker count of 0. The four higher public surfaces named below (LSP, NAPI, WASM, MCP) are
  not yet built.
- **Every inline route asserts BOTH nested counters — and the fifth did not until a review
  round found it.** The two counters nest, so an inline route legitimately shows `inline == 1`
  AND `style_ir == 1`. The fifth route originally read only the inline counter, which left a
  hole its own siblings did not have: a **direct** second `parse_style_ir` call on that route
  gives `inline == 1`, `style_ir == 2`, and the test stayed green. Closed at the source rather
  than downgraded in prose. The discriminating mutation is deliberately NOT a duplicated
  `extract_static_style_vars` call — that moves both counters and is the case the test already
  caught — but a direct `parse_style_ir` entry ahead of it, which moves only the second. Its
  red is on the new assertion specifically (`left: 2, right: 1`) with the inline assertion
  passing ten lines earlier, so execution is proven to have reached the case that was
  previously invisible. Restored: marker count 0, green.
- **Applied control, PER ROUTE:** that route parses twice; only its own case may
  redden — one plant per surface and one per `DomQueryKind` variant, each proven
  present before its red is believed. A fixture over a subset cannot speak for a
  route it never runs, and a single shared mutation cannot show which route
  detected it.
- **Required supplement — forward handoff:**
  `crates/verter_session/tests/cases/one_parse_per_style_block.rs::forward_handoff_of_parsed_ir_charges_no_further_parse_entry`
  — a route already holding a parsed `StyleSyntaxIr` and needing it again charges
  zero further parse entries, asserted by call count, not by cache inspection. Its
  applied mutation: make the handoff re-enter the gateway instead of forwarding the
  value; only this case may redden. An earlier draft named neither a file nor a
  function nor a mutation for it, which left the one thing separating reuse from a
  second parse ungated.
- **Ownership fence:** J1 constructs no content identity and owns no cache;
  `J1.md:229` assigns that model to the downstream block. The property gated here
  is "no second top-level parse entry per style block within a request", which is
  ownable without one.
- **Stated exactly:** one TOP-LEVEL parse entry, not one `parse_with_sink`
  execution — the Sass and Stylus layout grammars subparse internally by design.

### A33 — warm reuse across invocations

- **Primary gate:** `crates/verter_session/tests/cases/warm_style_parse_reuse.rs::warm_style_content_charges_no_additional_parse`
- **Why it is at the session boundary and not in the compiler:** Warm means reuse
  of an already-cached content identity ACROSS compile invocations, and that cache
  is the host's. The compiler's `direct_result_tests` module drives the
  pre-assembly `compile()` entry directly and says so in its own comment
  (`direct_result_tests/style_planner.rs:53-60`); `style_planner`'s parse entry
  charges unconditionally when called, so a test there would either stay red while
  the existing session cache satisfies the bound, or push J1 into building
  cross-invocation cache state — taking architecture `J1.md:225-231` assigns
  downstream. The previous draft named that module and was wrong on both counts.
- **What it asserts:** two compile invocations over the same unchanged style block
  charge one parse and then zero additional, observed through the existing host
  cache. J1 constructs no new identity and no new cache to satisfy this.
- **Parameterised over BOTH frameworks:** the bound says "a style content
  identity" with no Vue-only fence, so the case runs the Vue and the Svelte
  compile routes separately; Svelte warm reuse is otherwise untested.
- **Applied mutation:** force a re-parse on the second invocation, per route; each
  route's case must redden for its own route. This is a non-regression criterion —
  the bound preserves the existing cost model — which prose asserting "unchanged
  behaviour" cannot do.

### A34 — the external-preprocessor half of the cold bound

- **Primary gate:** `crates/verter_session/tests/cases/preprocessor_round_trip_parse_count.rs::preprocessed_result_adds_exactly_one_parse`
- **What it asserts:** over the sealed round-trip, a byte-changing preprocessor
  result is a distinct content identity adding exactly one further parse, and the
  worst case — a non-CSS dialect with all three Vue stages present and rewriting
  — totals exactly five (`J1.md:335`).
- **Why an existing gate does not reach it:** A14 proves the boundary's shape and
  type-state, not a count; A10i counts only the Vue cascade.
- **Applied mutations:** parse the preprocessed result twice; and add a fourth
  parse to the worst-case path. Each must redden.

### 3.1 What building the gates corrected in this contract

Two gates were built against the tree and watched to fail before this revision was written.
Six things the tree said are recorded here, including the two that contradicted the
specification — that contradiction is the reason the work was done this way rather than
argued for a fourth time.

1. **The two counters nest.** Recorded in A32 above. The specification asked for an
   assertion that is false on a correct tree.
2. **Svelte's scoped CSS is not in the client module's code.** External mode — the default —
   emits it as a separate `css` artifact; injected mode inlines it. A sanity assertion
   reading the code field would have passed for the wrong reason.
3. **Helper-level once-per-call tests already existed** for the SSR and semantic inline
   consumers, but only at the helper boundary. The ROUTE-level counts — full compile, full
   conversion — had no gate, and the existing Vue-cascade reuse test covers the cascade, not
   the compile route. The gap this ID names was real and narrower than stated.
4. **The three inline consumers are genuinely three routes.** Proven, not assumed: with the
   VDOM route mutated to parse twice, the SSR route's assertion stayed green, and with SSR
   mutated the session route stayed green. An earlier draft collapsed them into one.
5. **A double parse exists, and it is INSIDE this gate's obligation — not adjacent to it.**
   A compile requesting both a runtime product and the IDE product re-parses the same
   authored `<style>` bytes: the style cascade runs for the runtime product
   (`compile/mod.rs:885`), and the IDE path separately calls
   `extract_style_v_bind_usage_for_dialects` (`compile/mod.rs:1815`) which enters
   `transform_vue_v_bind` again (`compile/style_usage.rs:33`) when the caller supplies no
   precomputed usage; `CompileRequest` accepts multiple products
   (`compile_request/mod.rs:147`). The built route helper constructs one product, so all
   five route tests pass while runtime+IDE parses twice. **An earlier revision recorded this
   as out of scope. That was wrong** — A32's own requirement is "no second top-level parse
   entry per style block within a request", and this is a second entry within one request.
   Excluding it would have left the gate passing over the exact case it exists to catch.
   The route manifest therefore carries PRODUCT-SET configurations, and A32 above is
   corrected accordingly.
6. **A pre-existing test still carries the uncoupled pattern A25 exists to kill** —
   `carrier_blind_pseudo_classification.rs` types out its own `deep`/`global`/`slotted`
   list. Left untouched under the scope ceiling; it should consume the shared typed
   authority once that landing is in.

## 4. Coverage map

| ID | Primary gate | Required supplements |
|---|---|---|
| A25 | `special_pseudo_typed_list_totality.rs::every_special_pseudo_name_parses_with_typed_selector_list_in_all_dialects` (BUILT) | semantic nested-fact parity + secondary-entry counter |
| A26 | `packages/native/index.spec.ts::"an unstructurable selector is skipped, not re-parsed"` | WASM boundary case; review-enforced no-reintroduction |
| A27 | `css_attribution_chargeable.rs::every_css_domain_counter_is_chargeable_by_production` | asserted-zero names resolve to schema counters |
| A28 | `style_planner.rs::build_string_call_count_matches_edit_composition_depth` (extended) | zero-edit construction probe |
| A29 | `allocator_canaries.rs::converged_style_pipeline_allocation_within_ratified_ceiling` | retained legacy baseline committed |
| A30 | A14's `trybuild` type-state case | declared review-enforced delta check |
| A31 | `css_latency_gate.rs` runner (produces candidate evidence, binds provenance, then compares) | — |
| A32 | `parse_gateway_closure.rs::public_parse_surface_is_exactly_the_gateway` | five named per-surface cases (session, LSP, NAPI, WASM, MCP); named forward-handoff case |
| A33 | `warm_style_parse_reuse.rs::warm_style_content_charges_no_additional_parse` (session boundary, Vue and Svelte) | — |
| A34 | `preprocessor_round_trip_parse_count.rs::preprocessed_result_adds_exactly_one_parse` | — |

Ten IDs, ten primary gates, no ID uncovered and none covered twice. Test-function
names are the contract; a differently-named function satisfying the same
assertion is a deviation to record, not a silent substitution.

## 5. What this does NOT do

- It does not ratify, accept, unlock or move the status of any block.
- It does not write any field under `docs/arch/architecture-lock/ledger/`, or any
  registry row — including its own.
- It does not amend any charter, ruling, contract, DAG edge or locked cell. Its
  content reaches `J1.md` only through `AMD-016`'s application step.
- It does not claim any gate below exists or has run, **with one stated exception**: the
  A25 primary, its shared typed authority, its mutation and its control, and the five
  session-reachable A32 route cases WERE built and measured on branch
  `gates/session-style-ir-counter` (which carries `gates/dialect-universe` and `gates/style-proof-contract`). Everything else here is planned. §3.1 carries what building
  those two gates corrected, and A25 states precisely which half of it is still unbuilt. Where a gate's prerequisite
  is missing today — A29's retained baseline, A28's zero-edit probe — it says so.
- It does not create a second acceptance authority: every row here fills the Gate
  column of a row `AMD-016` defines, and defines no requirement of its own.

## 6. Ratification

Sought from the architecture seat under the maintainer's standing delegation,
**jointly with `AMD-016`**, in the falsification form this program uses: are the
two ratifiable together, and what blocks them. Receipts are filed under
`verify/results/J1/<reviewed-sha>/` and validated with
`scripts/orchestration/check-results.mjs`. A `RESULT: FAIL` is a structurally
sound result and is read as a verdict, not as form.

## 7. Application

This instrument carries no application steps of its own. `AMD-016` §13 is the
sequence for both, with two additions:

- Step 1 ratifies **both** `**Status:**` lines, and both digests are computed on
  post-ratification bytes.
- Step 4 registers **both** as `[[document]]` rows of `kind = "AMENDMENT"` and
  adds **both** to `J1`'s `[[authorization]]` closure. Registering one without
  the other would leave a binding with no proofs or proofs with no binding, which
  is the state the rescope exists to prevent.
