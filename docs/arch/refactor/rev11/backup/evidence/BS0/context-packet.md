# BS0 context packet — the exact dispatch prompts used

The real bytes dispatched to each seat, concatenated in dispatch order. The only
substitution is the absolute worktree root, replaced by `<WORKTREE>` so no
machine-specific path is tracked; nothing else is edited or reconstructed.

Implementation was performed directly by the track orchestrator in a dedicated
worktree, so there is no implementer dispatch prompt — the block's four
corrections are a single-pass implementation against the ratified charter and
its named `#[ignore]`d targets, not a delegated brief.

Two scoping consults ran BEFORE any code was written, under the maintainer's
review-budget ruling authorising liberal up-front use of a cheap seat for
premise verification and blast-radius checks. Both were answered by Grok 4.6 at
extra-high effort. Both changed the implementation: the first corrected the
consumer inventory for the shared Svelte props surface and identified the
legacy-fallback coupling that decides when that surface must stay `Missing`; the
second predicted, before it was observed, exactly which mapping anchors the
newly-emitting `props-events` cell would fail and enumerated four refusal-witness
sites the pre-scoping had missed.

Review seats were external CLIs only: Codex Sol at high reasoning effort for the
conformance mandate, and Grok 4.6 at extra-high effort with an explicit
default-to-BLOCK posture for the adversarial mandate. Per governance §2.2 the
architecture mandate is not required for a subsystem-class block and was not run;
no structural doubt arose that would have warranted it — the one owner question
(which layer owns the untyped props surface) was settled by the first scoping
consult against the live call-site inventory.

Dispatch order: the two scoping consults, then the conformance mandate, then the
adversarial mandate against the fixed tree.


---

## Dispatch: `grok-sv4-blastradius.md`

# Scoping question: blast radius of a Svelte props-surface change

You are scoping a change in the Verter repo. Working tree:
`<WORKTREE>` (this is your cwd).

This is a NEUTRAL verification task, not a review of a written change. Nothing has
been written yet. Your job is to CHECK CLAIMS about existing code by opening files
and citing `file:line`. A claim you cannot verify in the source is an UNKNOWN, not
a confirmation. Do not agree with a claim you did not check.

## Background

Today, a Svelte component written like this:

```svelte
<script>
  let { label, disabled = false } = $props();
</script>

<button {disabled}>{label}</button>
```

publishes an EMPTY props surface to TypeScript (`{}`), with no diagnostic. The
authored props `label` and `disabled` are invisible. That is a bug to be fixed.

## The claims to verify

**C1.** The empty surface ORIGINATES at
`crates/verter_session/src/typeinfo/framework_surface/svelte_exec.rs`, in
`resolve_runes_props`, at the early return

```rust
let Some(props_type) = facts.props_type else {
    return ResolvedOutcome::Missing;
};
```

`facts.props_type` is the authored TYPE-ANNOTATION payload; an untyped destructure
has none, so the function returns `Missing`.
— Verify: does that early return exist, and is `props_type` really the type
annotation rather than the destructure geometry?

**C2.** The downstream public-API projector
(`crates/verter_session/src/framework/api_projectors/svelte.rs`,
`resolve_public_props_text`) then falls through to an empty-text fallback because
`outcome.value()?.props.as_ref()?` short-circuits on the `Missing`.
— Verify the fall-through chain and what it finally renders.

**C3 (the important one).** `resolve_svelte_surface(..., SvelteSurfaceSource::RunesProps)`
— the function that dispatches to `resolve_runes_props` — has EXACTLY these
production consumers:
  - `crates/verter_session/src/resolver_core/component_meta/mod.rs` (component metadata)
  - `crates/verter_session/src/typeinfo/adapters/svelte/adapter.rs` (framework-surface wire executor)
  - `crates/verter_session/src/framework/api_projectors/svelte.rs` (public-API projector)

— Verify by an EXHAUSTIVE call-site search (production `src/`, not tests). Report
every call site you find, including any I have not listed. If there are more than
three, that is the finding.

**C4.** Therefore fixing this at the projector (C2) rather than at the origin (C1)
would produce a Svelte-only fork: the projector would see the props but
component-meta and the wire executor would still see an empty surface.
— Verify this follows from what you actually found in C3, or refute it.

## What I am considering doing

Inside `resolve_runes_props`, when `facts.props_type` is `None`, build the props
surface from the already-captured exact destructure geometry instead of returning
`Missing`. The geometry lives at
`crates/verter_semantic/src/analysis/framework_facts/svelte.rs` as
`SvelteScriptSyntaxFacts::props_calls()` → `ExactSveltePropsCalls` → per-call
`public_keys: Vec<SveltePropsPublicKey { name, span }>` plus a `has_rest: bool`
openness flag. Optionality would come from the existing `prop_defaults` loop
already in `resolve_runes_props`.

The evidence would be threaded ONLY on the "exact script facts" arm of
`SvelteFactObservations` (`svelte_exec.rs`, `fn exact`), never the `conservative`
arm.

## The blast-radius questions I need answered

**Q1.** What OBSERVABLE outputs change when `resolve_runes_props` starts returning
`Resolved` instead of `Missing` for an untyped `$props()` destructure? Walk each
consumer from C3 and say concretely what each would now emit that it did not before.

**Q2.** Is there any consumer that BRANCHES on `Missing` as a meaningful signal
rather than as an error? Specifically: `api_projectors/svelte.rs` appears to use
`matches!(&runes, ResolvedOutcome::Missing)` to decide "this is a LEGACY
(`export let`) component, use the legacy surface instead". Does making the runes
arm resolve for an untyped destructure break any legacy component? Under what
authored source, exactly? Cite the code.

**Q3.** Which EXISTING tests would flip from green to red? Search the test tree for
assertions that a Svelte props surface is EMPTY, or that an untyped `$props()`
component publishes nothing. Name each `file:line` + test name. This is the list I
must update, so a miss here costs me a broken gate.

**Q4.** Is `ExactSveltePropsCalls` genuinely a "negative evidence" type whose
absence is authoritative (i.e. an EMPTY inventory means "provably no `$props()`
calls" rather than "not computed")? If so, what constructs it and under what
condition is it NOT minted? I need to be sure an unavailable/partial parse can
never fabricate a props surface.

**Q5.** `has_rest` is true for `let { a, ...rest } = $props()`. In that case the
statically enumerated key set is OPEN. If I publish only the enumerated keys, the
generated TypeScript surface would be CLOSED and would reject valid extra props.
Is staying `Missing` (today's behaviour) for the `has_rest` case the right call, or
is there an existing repo mechanism for expressing an open key domain on a props
surface? Search for how the Vue side or the legacy Svelte side handles an open
props key set, and cite it.

**Q6.** Anything else you found that would bite this change. Be specific and cite.

## Output format

For each of C1–C4 and Q1–Q6: a verdict line (`CONFIRMED` / `REFUTED` / `UNKNOWN`)
then the evidence with `file:line`. Keep it tight — evidence, not prose. If you
cannot open a file, say so rather than guessing.

---

## Dispatch: `grok-gate-recharacterization.md`

# Scoping question: what a currently-refused Svelte cell will do once it emits

You are scoping a change in the Verter repo. Working tree:
`<WORKTREE>` (this is your cwd).

NEUTRAL verification task. Nothing has been written yet. CHECK CLAIMS by opening
files and citing `file:line`. A claim you cannot verify is an UNKNOWN, not a
confirmation. Do not agree with something you did not open.

## Background

`crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs`
is a characterization gate over three reachable Svelte CLIENT requests:

- `fixtures/svelte/basic-runes.svelte`
- `fixtures/svelte/legacy-slots.svelte`
- `fixtures/svelte/props-events.svelte`

Today `props-events.svelte` is REFUSED by the compiler (typed code
`svelte-runtime-unsupported-advanced-rune`), because a `$props()` local is read
from the instance script. I am about to REMOVE that refusal, so `props-events`
will start EMITTING a client module and will be handed to the harness's mapping
and structural oracles for the first time.

## The claims to verify

**C1.** The gate's `CharacterizedOutcome` enum has exactly two variants,
`EmitsAndFails { structural: bool, mapping: bool }` and
`Refuses { diagnostic_code }`, and `characterized_client_outcome` maps
`props-events.svelte` to `Refuses`.

**C2.** The gate's main test `every_committed_client_cell_is_driven_and_reaches_its_recorded_outcome`
never asserts the oracle's `verdict` field directly — it only compares whether a
`structural divergence` reason and a `not truthful about its own output` reason are
present against the recorded booleans, and rejects any OTHER reason as "an
unrecorded divergence".
— If that is right, then `EmitsAndFails { structural: false, mapping: false }`
would ALSO pass for a cell whose oracle verdict is `pass`, which makes the variant
NAME a lie. Confirm or refute, citing the exact assertions.

**C3.** `props-events.svelte` has its OWN required mapping anchors declared in
`packages/framework-conformance-harness/src/mapping-oracle.mjs`:
`script-onclick-declaration` at 0-based (line 3, column 11) and
`template-disabled-shorthand-binding` at (line 8, column 9), both
`expectRelations: ["verbatim-carry"]` and both required for the Svelte profile.
— Verify, and say what authored token each lands on in the fixture.

## The questions

**Q1.** I am separately fixing the client source map so that THREE producers emit
authored provenance they currently drop:
  (a) `let <name> = $state(...)` instance declarations
      (`crates/verter_compiler/src/svelte/runtime/instance_items.rs`,
       `SupportedInstanceScriptItem::StatePrimitive`, lowered in
       `client_plan_script.rs`),
  (b) `export let <name> = ...` legacy prop declarations (`ExportLetProps`),
  (c) the `{#if <test>}` condition
      (`client_block_emit.rs::emit_if_block`, which currently writes the test
       through `push_str` rather than `push_mapped`).

Do any of those three cover `props-events.svelte`'s two required anchors
(`onClick` in `function onClick() {`, and `disabled` in the shorthand attribute
`{disabled}`)? Walk it concretely: which producer, if any, emits the generated
`onClick` token and the generated `disabled` token, and does that producer
currently attach a mapping? Cite the code.

**Q2.** If the answer to Q1 is "no", then after the refusal is removed
`props-events` will emit and the mapping oracle will report violations for it.
What EXACTLY would the oracle's reasons look like — which requirement ids
(`anchor-missing`, `anchor-span-coverage`, `anchor-relation`,
`segment-provenance`, generated-only-range) would fire? Read
`packages/framework-conformance-harness/src/mapping-oracle.mjs` and answer from
its actual rules, not from the names.

**Q3.** Independently of mapping: would `props-events` diverge STRUCTURALLY from
its committed golden? The golden is under
`packages/framework-conformance-harness/goldens/`. Find the `props-events` client
golden and describe the shape the official compiler emits for this component
(the `$.prop($$props, 'disabled', 3, false)` defaulted-prop form, the
`$$props.label` / `$$props.ontoggle` direct reads, the event handler wiring).
Then look at how Verter's client backend lowers a prop READ — the vocabulary is
`PropRead::Getter` / `PropRead::PropsMember` in
`crates/verter_compiler/src/svelte/runtime/expr_emit.rs` — and say whether the two
shapes agree, disagree, or cannot be determined without running it.

**Q4.** The gate also has a committed per-cell record at
`crates/verter_session/src/svelte_conformance_cell_record.json`, held against the
live suite by `the_committed_cell_record_matches_what_the_suite_observes`. What
exactly in that file describes `props-events`, and what else in it names the
`{#each}` flags value or the `basic-runes` mapping anchors? I need the complete
list of what regenerating it will change.

**Q5.** Is there anywhere ELSE in the repo — outside that gate — that asserts
`props-events.svelte` (or its exact source bytes, inlined) is REFUSED, or asserts
a Svelte advanced-rune refusal using those bytes? Search the whole tree including
`packages/*/scripts/*.mjs`. Name each `file:line`. A miss here breaks a build I
cannot see from the gate.

**Q6.** Anything else that would bite. Be specific and cite.

## Output format

Verdict line (`CONFIRMED` / `REFUTED` / `UNKNOWN`) per claim, then evidence with
`file:line`. Tight — evidence, not prose.

---

## Dispatch: `review-conformance.md`

# Review: four Svelte compiler/session corrections

Repo worktree: `<WORKTREE>` (your cwd).
The change is `git diff dd84e5fa2..HEAD` — five WIP commits, to be squashed.

This is a CORRECTNESS review of RUNNING CODE. Judge the code and its tests, not
the prose. Every claim you make about the code must cite `file:line` you opened.
A claim you did not verify is an UNKNOWN, not a finding.

## What the change does

Four independently-ratified defects in Verter's Svelte support, each fixed in its
root-cause owner. The pinned official compiler is `svelte@5.56.8`, reachable at
`packages/framework-conformance-harness/.oracle-checkouts/svelte/` and invocable
through `packages/framework-conformance-harness/src/invoke-svelte-oracle.mjs`.

**1. `{#each}` item reactivity.** `EACH_ITEM_REACTIVE` was set whenever the item
binding was a signal kind — true for every `{#each}` — so Verter emitted
`$.each(ul, 21, …)` where official emits `20`. Replaced with the official
predicate (official source:
`.oracle-checkouts/svelte/packages/svelte/src/compiler/phases/3-transform/client/visitors/EachBlock.js`
lines ~45-83). Owner:
`crates/verter_compiler/src/svelte/runtime/lower_block.rs`
(`EachReactivityFacts::each_item_is_reactive`), consumed by
`client_block_plan.rs::project_each`.

The flag turned out to be COUPLED to the item's read form: with the flag clear
the runtime passes the raw item, so `$.get(item)` would dereference a non-signal.
So `lower_block.rs::finalize_each_item_reactivity` (called from
`mod.rs`, after the final mode is known) demotes non-reactive item bindings from
`EachSignal` to `PlainLocal` using the SAME predicate.

Supporting producer change: `PatternBindings` gained a `shape: PatternShape`
fact (`ir.rs`), minted by `expr.rs::parse_pattern`, because the official
`key_is_item` rule requires the each CONTEXT to be a bare identifier — a
single-name destructure `{#each xs as { id } (id)}` declares one name and is NOT
an identifier context.

**2. Instance-script `$props()` reads.** The prop-usage gate refused ANY
instance-script reference to a prop local. Official accepts READS. Narrowed to
WRITES only: `crates/verter_compiler/src/svelte/runtime/client_surface_script.rs`
(`PropRefScan` → `PropWriteScan`). No new code path was added — the read
lowering already existed.

**3. Client source-map provenance.** Three producers wrote unmapped text, so
authored script declarations and `{#if}` tests carried no provenance:
`instance_items.rs` (`StatePrimitive` / `ExportLetProps` gained name spans),
`client_plan_script.rs` (lowering attaches the mapping),
`client_block_emit.rs::emit_if_block` (now `push_mapped`s the test).

**4. Untyped `$props()` props surface.** An untyped destructure published an
EMPTY props surface to TypeScript. Fixed at the shared executor leg
`crates/verter_session/src/typeinfo/framework_surface/svelte_exec.rs`
(`runes_props_from_destructure_geometry`), NOT at the public-API projector,
because three consumers read that leg.

## The questions

**Q1 — Is the `{#each}` predicate faithful?** Open `EachBlock.js` and compare it
to `EachReactivityFacts::each_item_is_reactive`. In particular:
  - official skips a dependency whose `binding.scope.function_depth >=
    state.scope.function_depth`. The Rust code does NOT implement a depth check
    and argues (in its doc comment) that the skip can never apply because the
    collection expression is lowered in the each's PARENT scope. Is that argument
    sound? Find a source where it is WRONG if you can.
  - official's `dependencies` set — does it really contain every resolved
    binding, or is it filtered? Check
    `.oracle-checkouts/.../2-analyze/visitors/Identifier.js`.
  - `EACH_ITEM_IMMUTABLE` is deliberately left as `runes` alone where official is
    `runes && !uses_store`. That divergence is captured by an `#[ignore]`d test
    (`each_item_immutable_clears_when_a_runes_collection_subscribes_a_store` in
    `client_tests.rs`). Is leaving it defensible, or does it interact with the
    corrected `EACH_ITEM_REACTIVE` to produce a NEW wrong output that did not
    exist before?

**Q2 — Is the item demotion safe?** `finalize_each_item_reactivity` mutates
binding kinds after lowering. Find every consumer of
`BindingRuntimeKind::EachSignal` and of `is_signal_kind` and check whether any
of them runs BEFORE the finalizer, or whether any depends on the item being a
signal for a reason unrelated to the flag. Cite what you checked.

**Q3 — Is the write-only prop gate complete?** `PropWriteScan` reports a prop
write. Enumerate the JavaScript positions that WRITE a binding and check each is
covered: assignment (incl. compound, destructuring, member-rooted), update
(`++`/`--`, prefix and postfix, TS-wrapped), `for…of` / `for…in` heads. What
about a write reached some other way? Construct a source where a prop is written
from the instance script and Verter now EMITS instead of refusing — that is a
correctness bug, because the emitted write would not go through the prop setter.
Also check the shadowing model: does a write to a same-named LOCAL wrongly refuse?

**Q4 — Are the new source-map mappings correct, and can they be wrong?** The map
is lowered through `CodeTransform` in
`crates/verter_compiler/src/svelte/runtime/output.rs::finish`, which rejects
overlapping/out-of-bounds ranges as a typed error that STOPS the module
publishing. Check that every new mapping's generated range is exact, and
specifically look at `expr_emit.rs::declaration_name_mapped_code`: it computes
the name offset from `DECL_LET_PREFIX.len()` and falls back to an UNMAPPED
fragment if the prefix does not match. Is the fallback reachable? Is it the right
disposition, or does it hide a real bug?

**Q5 — Is the SV-4 props surface correct and honest?** `runes_props_from_destructure_geometry`
returns `Missing` in three cases (no exact inventory, no `$props()` call,
`has_rest`). Check each:
  - Does the `no $props() call` case genuinely keep the LEGACY `export let`
    surface reachable? Read `resolve_public_props_text` in
    `crates/verter_session/src/framework/api_projectors/svelte.rs`.
  - Is `has_rest` → `Missing` right, or is it a case that should publish?
  - Can a PARTIAL / UNAVAILABLE script-fact arm reach this function? Trace
    `SvelteFactObservations::conservative`.
  - A component with BOTH `export let` and an untyped `$props()` — what happens
    now, and is it worse than before?

**Q6 — Do the tests discriminate?** The change adds/updates these. For each, ask:
would it FAIL if the fix were reverted, and is it asserting the artifact or a
proxy?
  - `client_tests.rs::each_item_reactivity_matches_the_official_predicate_on_every_axis`
  - `client_tests.rs::the_each_item_flag_and_its_read_form_move_together`
  - `client_tests.rs::instance_script_prop_reads_lower_to_the_official_accessor_shapes`
  - `client_tests.rs::props_instance_script_prop_write_stays_fail_closed`
  - `client_tests.rs::a_state_declaration_carries_its_authored_name_provenance`,
    `an_export_let_prop_declaration_carries_its_authored_name_provenance`,
    `an_if_block_test_carries_its_authored_expression_provenance`
  - `svelte_exec_tests.rs::an_untyped_props_destructure_resolves_through_the_shared_surface`,
    `a_component_with_no_props_call_keeps_the_legacy_surface_reachable`,
    `an_open_rest_props_destructure_publishes_no_closed_surface`
  - the converted
    `public_api_typescript_observation.rs::a_svelte_props_type_annotation_does_not_change_which_props_are_published`

**Q7 — Deleted / weakened coverage.** The change DELETES two characterization
tests from `svelte_official_conformance_gate.rs` (they asserted the defects) and
REMOVES a `prop_read_instance_script` row from
`crates/verter_compiler/tests/cases/svelte_client_fail_matrix.rs`. It also
REPLACES the refusal-witness source in four places (a props READ component that
now compiles → a prop WRITE component). For each removal/replacement: is the
coverage genuinely subsumed by something else, or was a real assertion lost?

**Q8 — The gate scaffold.** `CharacterizedOutcome` gained an `EmitsAndPasses`
arm, and `EmittedDivergences` makes the empty divergence set unrepresentable
(private fields + three constructors in a child module). A verdict assertion was
added. Does the gate still fail in BOTH directions — a deepening defect AND a
silent correction? Could a cell now pass while emitting something wrong?

**Q9 — Anything else.** Regressions, unsound assumptions, an owner choice that
forks a shared surface, a comment that says something the code does not do.

## Verification commands

The four conformance targets live behind a feature flag and are INVISIBLE to the
default gate. Without `--features bf2-authoritative` a filter naming them matches
ZERO tests and still exits 0 — always read the `running N tests` line.

```
cargo test -p verter_compiler
cargo test -p verter_session --lib
cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1
cargo test -p verter_session --lib --features bf2-authoritative public_api_typescript_observation -- --test-threads=1
cd packages/svelte-runtime-tests && pnpm test
```

## Output

Per question: a verdict (`OK` / `FINDING` / `UNKNOWN`) and the evidence with
`file:line`. For every FINDING give a concrete failing case — inputs → the wrong
output — and severity (BLOCKING / MAJOR / MINOR / NIT). End with a single overall
verdict: LAND or BLOCK. Evidence, not prose.

---

## Dispatch: `review-adversarial.md`

# Adversarial review: break these four Svelte corrections

Repo worktree: `<WORKTREE>` (your cwd).
Change under review: `git diff dd84e5fa2..HEAD`.

## Posture — read this first

**Default to BLOCK.** LAND is the claim that needs proving, not the fallback.
Your job is to find the input that makes this code produce a WRONG ANSWER, and to
prove the new tests would not catch it. "Looks fine", "seems correct" and "no
issues found" are NOT acceptable terminal verdicts.

A finding is valid only with a **concrete failing case**: an authored Svelte
source, the output Verter produces, the output the pinned official compiler
produces, and `file:line`. A hedge ("this could possibly…") is not a finding.
An UNKNOWN is acceptable and is better than a guess.

Structure your answer as: **the strongest case AGAINST landing first**, then your
own rebuttal to it, then the verdict.

## The change

Four Svelte defects fixed in their root-cause owners. The pinned official
compiler is `svelte@5.56.8`. You can RUN it:

```js
// node, from packages/framework-conformance-harness
const { compileSvelteFixture } = await import("<abs path>/packages/framework-conformance-harness/src/invoke-svelte-oracle.mjs");
const out = compileSvelteFixture(source, "App.svelte", { generate: "client", dev: false });
console.log(out.js?.code ?? out.code);
```

Use it. Differential testing against the real official compiler is the highest-
value thing you can do here, and it is cheap.

1. **`{#each}` item reactivity.** `EACH_ITEM_REACTIVE` now follows the official
   predicate (`lower_block.rs::EachReactivityFacts::each_item_is_reactive`), and
   non-reactive item bindings are DEMOTED from `EachSignal` to `PlainLocal`
   (`lower_block.rs::finalize_each_item_reactivity`, called from `mod.rs`) so the
   read form matches the flag.
2. **Instance-script `$props()` reads accepted**; writes still refuse
   (`client_surface_script.rs::PropWriteScan`).
3. **Source-map provenance** for `$state` / `export let` declarations and `{#if}`
   tests (`instance_items.rs`, `client_plan_script.rs`, `client_block_emit.rs`).
4. **Untyped `$props()` publishes its props**
   (`typeinfo/framework_surface/svelte_exec.rs::runes_props_from_destructure_geometry`).

## Attack surface — go here first

**A. The each-flag / read-form coupling (highest risk).** The author discovered
that clearing `EACH_ITEM_REACTIVE` without demoting the binding produces
`$.get(item)` on a non-signal — a module that mounts and renders wrong. They
fixed it. Now find the case they MISSED:
  - nested `{#each}` where the inner collection references the outer item
  - an each item that is WRITTEN in the body (`item = x`, `item.a = 1`)
  - `bind:` to an each item
  - an each with an index, keyed and unkeyed
  - an each inside a `{#snippet}` / `{#await}` / a component slot
  - a store-subscribed collection in RUNES mode
  - an each whose collection is a literal, a call, a global
  For each: compile with Verter and with official, diff the `$.each(` flags AND
  every read of the item. A mismatch is a BLOCKING finding.

**B. `EACH_ITEM_IMMUTABLE` was deliberately NOT corrected** (`runes` alone where
official is `runes && !uses_store`). Does that interact with the corrected
`EACH_ITEM_REACTIVE` to produce a combination the runtime mishandles — one that
did NOT exist before this change? Prove it with official output.

**C. The prop write gate.** `PropWriteScan` decides which instance-script prop
usages still refuse. Find a WRITE it misses — Verter would then emit a plain
write where official emits a prop-setter call. Try: destructuring assignment,
`for (prop of …)`, a write inside a nested arrow, a write through a TS-wrapped
target, `delete o.prop`, `[a, b] = [b, a]`, a write inside a `$effect`, a write
in a legacy `$:` statement. Conversely, find a READ it wrongly refuses.

**D. Source maps.** A wrong mapping silently lands IDE navigation on the wrong
token. `output.rs::finish` rejects overlapping/out-of-range mappings by FAILING
the compile, so a bad mapping can also stop a module publishing entirely. Find a
component where a new mapping is misplaced or where the compile now fails. Try
unicode/multibyte identifiers, CRLF sources, a `$state` declared with unusual
spacing, `export let a, b, c;`, a `{#if}` whose test contains a string with `)`.

**E. The untyped props surface.** `runes_props_from_destructure_geometry` builds
a props surface from destructure geometry. Find a source where the published
surface is WRONG: aliased keys (`let { a: b } = $props()`), nested destructure,
computed keys, a duplicate key, `$bindable()`, a key that is a reserved word or
needs quoting, a component with BOTH `export let` and `$props()`. Check what
TypeScript then sees — the observation harness is
`crates/verter_session/src/compile/map_equality_tests/public_api_typescript_observation.rs`.

**F. Did the tests get weaker?** The change deletes two characterization tests,
removes a `prop_read_instance_script` fail-matrix row, and swaps the
refusal-witness source in four places. Prove each removal is subsumed — or show
what is now uncovered.

## Verification

The four conformance targets are behind `--features bf2-authoritative` and are
INVISIBLE to the default gate; without the feature a filter naming them matches
ZERO tests and exits 0. Always read `running N tests`.

```
cargo test -p verter_compiler
cargo test -p verter_session --lib
cargo test -p verter_session --lib --features bf2-authoritative svelte_official_conformance -- --test-threads=1
cargo test -p verter_session --lib --features bf2-authoritative public_api_typescript_observation -- --test-threads=1
cd packages/svelte-runtime-tests && pnpm test
```

You may edit files to run experiments, but REVERT everything before finishing and
say so.

## Output

1. The strongest case against landing.
2. Your rebuttal to it.
3. Findings: each with a concrete failing case (source → Verter output → official
   output), `file:line`, and severity (BLOCKING / MAJOR / MINOR / NIT).
4. What you checked and why it CANNOT fail — for every area you are calling OK.
5. Verdict: LAND or BLOCK.

## Already found and FIXED — do not re-report these; try to break the FIXES

A prior review seat found three defects, all now fixed in the tree you are
reviewing. Attack the fixes themselves:

1. **Demoting a non-reactive each item to `PlainLocal` made it a writable
   `bind:` root**, so `{#each xs as item}<input bind:value={item}/>` emitted a
   setter that wrote the callback parameter and never reached the collection.
   Fixed by minting a DISTINCT `BindingRuntimeKind::EachPlain`
   (`expr.rs`) that reads plainly but is not a writable bind root. Check every
   consumer of `EachPlain` / `EachSignal` / `is_signal_kind` /
   `is_writable_bind_root` for a place the new kind is mishandled.
2. **`PropWriteScan` read only the IMMEDIATE member object**, so `o.x.y = 1` on
   a prop escaped the gate; and it modelled no scope frame for `catch` params or
   `for` declarations, so it over-refused. Fixed via
   `bind_target::target_expr_root_ident` and new `visit_catch_clause` /
   `visit_for_statement` frames. Find a write it STILL misses, or a shadowing
   form it still over-refuses.
3. **A call-bearing `{#if}` test lost its provenance** because the derived thunk
   body was flattened to a `String`. Fixed by carrying it as `MappedCode`
   (`PreparedDerivedRead.thunk_body`). Check the emitted map is still valid for
   every `{#if}` shape — `output.rs::finish` turns a bad mapping into a compile
   FAILURE, so a regression here stops modules publishing.
