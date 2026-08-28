# BF3 context packet — the exact dispatch prompts used

These are the real bytes dispatched to each worker and review seat, concatenated in
dispatch order. The only substitution is the absolute worktree root, replaced by the
token `<WORKTREE>` so no machine-specific path is tracked; nothing else is edited,
summarised, or reconstructed after the fact.

Workers were `claude` CLI processes in a dedicated worktree, one resumed session across
all implementation and fix phases. Review seats were external CLIs only — Codex Sol at
high reasoning effort for the conformance, architecture and delta mandates, and Grok 4.6
at extra-high effort with an explicit default-to-BLOCK posture for the adversarial
mandate. The architecture consults whose rulings this block acts on are recorded
separately and verbatim in `scope-consult-*.md`, `adjudication-*.md` and
`disposition-*.md`.

Dispatch order: implementation phases A–E, fix rounds F–H, gate-fix I, then the
conformance, architecture, adversarial and targeted-delta review briefs.


---

## Dispatch: `bf3-impl-brief-A.md`

# Task: build and run the Svelte official-conformance gate (phase A)

You are implementing in the git worktree `<WORKTREE>`
(branch `work/bf3-implementation`). It already has `pnpm install --frozen-lockfile` and
`pnpm build:ts` done, and the conformance harness's gitignored oracle artifacts
(`.oracle-npm-cache`, `.oracle-installs`, `.oracle-checkouts`) are provisioned. Do not
touch any other checkout.

Be TERSE in every report. Lead with the verdict and the numbers.

## Non-negotiable rules

- **TDD.** Failing test first, verified failing by actually running it, then implement.
  Never write an implementation before its test has been observed red.
- **No stubs.** No empty test bodies, no unconditional default returns presented as
  implementation, no always-true assertions, no "real body in a follow-up". A
  characterization test must FAIL pre-change and PASS post-change.
- **Any claim about code you did not write is a QUESTION, not a finding.** Open the file
  and cite `file:line`, or report it as unknown. Before you report, list every claim you
  made about a sibling consumer or a shared owner with the `file:line` where you verified
  it.
- **You change ZERO production behaviour in this phase.** No new production guard, typed
  refusal, publication gate, withholding path, tracker, or known-divergence list. No edit
  to any compiler emit path. If you believe production must change, STOP and report — do
  not do it.
- **No program vocabulary anywhere you write** — no `BF3`, `BF2`, `BV0`, `rev11`, block
  IDs, charter/amendment references, phase/round/cutover words — in source, comments, test
  names, file names, or commit messages. Name things after the invariant they characterize.
  (Pre-existing `bf2_*` names in the tree are grandfathered; do not add new ones, and do not
  rename the existing ones in this phase.)
- **Targeted tests only.** Never run `node scripts/gate.mjs`, never a bare workspace
  `cargo build`/`cargo test`/`cargo nextest`. Use `cargo test -p <crate> <filter>` and the
  specific package scripts. The machine has 24 GB RAM and has been hard-rebooted twice by
  unbounded gate runs.
- WIP-commit freely and often (one commit per finished sub-step) so partial work survives.

## Context you need

The repository contains a hermetic official-compiler conformance harness at
`packages/framework-conformance-harness`. Its committed golden set
(`goldens/manifest.json`) has 48 entries: 36 `vue/*` and 12 `svelte/*`. The 12 Svelte
entries are 3 fixtures x {client, server} x {dev0, dev1} — `basic-runes` (runes),
`props-events` (runes), `legacy-slots` (legacy).

The harness's accepted entry point is `packages/framework-conformance-harness/bin/check-candidate.mjs`,
invoked as
`node bin/check-candidate.mjs --golden <logical-name> --candidate <file.json> --authoritative`,
where the candidate JSON is `{ "code": string, "map": object|null, "diagnostics"?: array }`.
It reports six axes — `parse`, `link`, `structural`, `diagnostics`, `mapping`, `runtime` —
each with `ran` / `skipped` / `not-applicable`, and under `--authoritative` a `skipped` axis
is a hard failure. It has been verified working in this worktree: feeding a golden's own
recorded `code`+`map` back as the candidate returns `verdict: "pass"` with parse, link,
structural, diagnostics, mapping all `ran` and runtime `not-applicable`.

The Vue side already has the exact machinery you are mirroring:

- `crates/verter_session/src/compile/map_equality_tests/bf2_seed_matrix.rs` — reads the
  golden manifest, verifies each record's digest and each fixture's authored-source hash,
  compiles the fixture through the genuine shipped path, and drives the harness CLI.
  Note `read_seed_matrix` filters entries with `name.starts_with("vue/")`.
- `crates/verter_session/src/compile/map_equality_tests/bf2_full_axis_gate.rs` — the
  correctness gate: requires every axis to have `ran` (or `not-applicable`) and the overall
  verdict to be `"pass"`, plus a mutation-discrimination test that plants a reversible
  defect per axis family and proves the CLI reports `fail` naming that axis.
- Both are behind the `bf2-authoritative` cargo feature (`crates/verter_session/Cargo.toml`),
  because the harness needs Node plus the gitignored oracle install.

No Svelte cell has ever been driven through this comparator. The Svelte client compile
backend lives in `crates/verter_compiler/src/svelte/runtime/` and is refuse-by-default; the
production route that reaches it is `CarrierCompiler::compile_bundle`, whose production
caller is `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`. Verify all of
that yourself and cite lines — those are my claims, not yours.

## What to build

### A1 — establish the shipped Svelte client route

Determine, by reading code, the exact production path a `.svelte` client compile request
takes from a public/default entry down to generated JavaScript plus source map, and the
smallest faithful way a Rust test can drive that same path (mirroring what the Vue seed
matrix does for Vue). Cite every step `file:line`. If the only reachable route diverges
from what the golden records describe (`options.generate = "client"`, `options.runes`,
`options.dev`), say exactly how, with citations.

Report this before writing the gate. If you cannot find a faithful shipped route, STOP and
report — do not synthesise one.

### A2 — the Svelte seed-matrix loader

Add a Svelte counterpart to the Vue seed-matrix loader. It must:

- read the SAME committed `goldens/manifest.json`, selecting the `svelte/` entries;
- verify each golden record's SHA-256 against the digest the manifest names;
- verify each fixture on disk matches the authored source the record records (allow the
  same CRLF-normalised comparison the Vue loader allows);
- expose the per-cell axes from the record's `options` (`generate`, `runes`, `dev`) as a
  typed cell, and map them onto the real compile options — do not hardcode a fixture list;
- separate the 6 **client** cells from the 6 **server** cells as distinct inventories.

Reuse the Vue loader's helpers wherever they are already shared rather than duplicating
them; where a helper is private and duplicating two lines of path arithmetic is genuinely
cheaper than widening a module surface, say so in a comment (there is precedent for that
exact judgement in `bf2_full_axis_gate.rs`). Do not weaken any existing Vue assertion, and
do not change `read_seed_matrix`'s Vue behaviour.

### A3 — the Svelte authoritative full-axis gate

Add the Svelte counterpart of the full-axis gate. Per client cell it must:

1. compile the fixture through the genuine shipped path established in A1 (never a
   test-local re-implementation of the emitter, never a harness-synthesised candidate);
2. hand the real generated `code` and `map` to `bin/check-candidate.mjs --authoritative`
   unchanged;
3. require every axis to report `ran`, or `not-applicable` where that is a structural fact
   about the artifact rather than a skip — assert the `axes` map directly, not only the
   CLI's `reasons`, so a future harness change that stopped surfacing a reason could not
   silently defeat the gate;
4. require the overall verdict to be `"pass"`.

Also add a **mutation-discrimination** test in the same style as the Vue one: take one
genuine compiled cell per relevant axis family, apply a single reversible textual mutation
to the exact bytes the CLI would otherwise receive, and require a `fail` verdict whose
`reasons` name that axis. **Prove each plant actually applied** — the mutated string must
differ from the pristine one, and for the parse family it must be textually distinct — and
keep an unplanted control cell green in the same run. An exit code is never proof a
mutation landed.

Gate the new code behind the same opt-in cargo feature as the Vue gate (or a sibling
feature if that is cleaner — justify it), keep it inside the existing test binary layout
(one `tests/main.rs` per crate; do not add a second top-level `tests/*.rs`), and keep the
default hermetic gate free of it.

**Expect the gate to fail.** Its first run is a probe, not a pass requirement. Do not
adjust the gate, the fixtures, the goldens, the normalizer, or the compiler to make it
green. A failing gate here is the deliverable's most valuable output.

### A4 — record the server cells' existing behaviour

For each of the 6 `svelte/*server*` cells, drive the same shipped route and record exactly
what the production path returns today (the typed refusal, its variant, and where it is
decided). Add a test that characterises that refusal as it stands. Add nothing to the
refusal, and do not extend it.

### A5 — report

Write `/tmp/bf3-phaseA-report.md` and print the same content at the end of your run:

- the A1 route with `file:line` citations;
- a per-cell table for all 6 client cells: golden name, verdict, `reasons`, and each axis's
  status;
- for each failing cell, the concrete observed difference — quote the smallest decisive
  excerpt of candidate vs golden, and state which axis reported it;
- for each failure, your honest classification with evidence: genuine compiler defect /
  harness or normalizer artifact / source-content or comparison artifact / route-assembly
  artifact / unknown. Mark "unknown" freely; do not guess.
- the 6 server cells' recorded refusal;
- the mutation-discrimination results (which plant, which axis reported, control green);
- every claim you made about code you did not write, each with the `file:line` that
  verifies it;
- anything you could not do and why.

Then `touch /tmp/bf3-phaseA-done`.

Do NOT dispose of any failure (no owner assignment, no debt row, no fix). Do NOT fix any
compiler defect. Phase B will do that with a separate brief once I have read your evidence.

---

## Dispatch: `bf3-impl-brief-B.md`

# Phase B — exhaust the remaining product/route inventory

Same worktree, same rules as phase A. Re-read them if you need to; the ones that matter most
again here:

- **TDD**: failing test first, observed red, then implement.
- **No stubs**, no always-true assertions, no empty test bodies, no non-discriminating
  characterization tests.
- **Any claim about code you did not write is a QUESTION** — open it, cite `file:line`, or
  report it as unknown.
- **You change ZERO production behaviour.** No guard, refusal, publication gate, withholding
  path, tracker, or known-divergence list. No compiler emit-path edit. If you think
  production must change, STOP and report.
- **No program vocabulary** in source, test names, file names, comments, or commit messages.
- **Targeted tests only** — never `node scripts/gate.mjs`, never a bare workspace cargo
  build/test/nextest.
- WIP-commit per finished sub-step.
- Do NOT dispose of any finding: no owner assignment, no debt row, no fix.

Phase A is done and committed (`d2496f046`, `20c69e1ce`, `b98ef74b7`). Do not revisit the
six Svelte client cells or the six server cells — they are covered.

## The remaining inventory

The block's probe scope beyond the Svelte runtime cells is recorded in
`docs/arch/refactor/rev11/evidence/framework-conformance/bf3-safety-retraction-scope.md`.
Its remaining rows are:

| family | what to enumerate | minimum oracle |
|---|---|---|
| PublicApi / TSC / declaration | Vue and Svelte public/default route+profile combinations | exact TypeScript observable fixture |
| diagnostics / maps / CSS | every route that publishes them alongside another product | atomic set; diagnostic and map validity; no unrequested artifact |
| NAPI / WASM / host / bundler | every public/default spelling that currently returns success | the same semantic probe plus route identity |

Two exclusions, both firm:

- **Vue VDOM / Vapor / SSR runtime-render output is OUT of scope.** Those rows are owned and
  already corrected elsewhere. Do not probe, re-probe, or comment on Vue runtime-render
  correctness. Vue's NON-runtime products (PublicApi, TSC/TSX, declaration, diagnostics,
  maps, CSS) ARE in scope.
- Svelte's existing typed server refusal is already recorded; do not extend it.

## What to do

### B1 — enumerate the actual reachable-success surface, from code

Produce a real inventory, derived from the source tree, not from the table above. For every
PUBLIC or DEFAULT spelling by which a caller can request an in-scope product, record:

- the exact entry point (`file:line`) and its request type;
- the route it takes to the shared owner;
- which products it can publish, and under which profile axes;
- whether it currently returns success for a representative in-scope input.

Cover at minimum, verifying each yourself rather than trusting this list to be complete or
correct: the host virtual-file products (`VerterHost::get_virtual_file` and its
`VirtualNodeKind` variants), the host compile/analyze entry points, NAPI (`crates/verter_napi`
and `packages/native`), WASM (`crates/verter_wasm` and `packages/wasm`), the bundler plugin
(`packages/unplugin`), and any other public spelling you find. If a spelling exists that this
list omits, that omission is itself a finding — report it.

Where two spellings converge on the same typed request, say so explicitly and treat them as
ONE semantic case with TWO route identities. Do not inflate the inventory by counting route
aliases as distinct semantic cases — but DO prove the route identity separately for each
alias, because a transport that silently reinterprets a semantic default is exactly the
defect class this enumeration exists to catch.

Commit the inventory as a machine-checkable artifact plus a test that fails if the tree grows
a public in-scope spelling the inventory does not name. That test must be discriminating:
prove it by adding a spelling locally, watching it fail, and reverting. Report that proof.

### B2 — probe each enumerated cell

For every in-scope cell that currently returns success, drive it and record the exact result.
Use the strongest oracle already available in-tree for that product; do not invent a new
oracle and do not weaken an existing one. For TypeScript-observable products, the harness
already has `packages/framework-conformance-harness/src/typescript-observe.mjs` and
`test/typescript-observation.spec.mjs` — read them and reuse rather than reimplement.

Where no oracle exists for a product, say so plainly and record the cell as UNPROVEN rather
than asserting a pass. An unproven cell recorded honestly is worth more than a fabricated one.

### B3 — atomic publication

For every in-scope route, prove the publication contract in BOTH directions:

- on success, exactly the requested products are published and nothing else;
- on every refusal, NO partial product is published — no JavaScript, no PublicApi, no TSC
  output, no declaration, no CSS, no diagnostic map, no source map.

Include the Svelte server refusal and the Svelte `props-events` client refusal found in phase
A as refusal cases. Make these tests discriminating: each must fail if the corresponding
publication rule is broken. Prove that by breaking it locally, watching the test go red, and
reverting — and prove the mutation actually applied before trusting the red.

### B4 — cold-path preservation

Add tests proving that cells NOT implicated by any phase-A or phase-B finding retain their
current behaviour exactly. These are the regression net for whatever correction work follows.
Pick them so that they would genuinely break if a correction over-reached; a test that
passes no matter what is worthless here.

### B5 — report

Write `/tmp/bf3-phaseB-report.md` and print the same content:

- the full B1 inventory table, with `file:line` for every entry point;
- per-cell B2 results: pass / fail / refused / unproven, with the evidence;
- for each failure, the concrete observed difference and your honest classification
  (genuine defect / harness or oracle artifact / route-assembly artifact / unknown) — mark
  unknown freely;
- B3 results including the red-green proof for each publication test;
- B4 coverage;
- every claim you made about code you did not write, with the `file:line` that verifies it;
- anything you could not do and why.

Then `touch /tmp/bf3-phaseB-done`.

---

## Dispatch: `bf3-impl-brief-C.md`

# Phase C — correct the conformance evidence against an independent re-investigation

Same worktree, same rules as phases A and B (TDD; no stubs; any claim about code you did not
write is a QUESTION with a `file:line`; ZERO production behaviour change; no program
vocabulary; targeted tests only; WIP-commit per sub-step; dispose of nothing).

An independent skeptical re-investigation of your phase-A result found real errors in it. It
confirmed some of your findings and overturned others. Your job now is to make the evidence
match what is actually true. Do not defend the earlier conclusions; correct them.

## What it CONFIRMED

- `basic-runes` client, production request: genuine defect. Bit `1` in the `$.each` flags
  argument is `EACH_ITEM_REACTIVE`
  (`packages/framework-conformance-harness/.oracle-checkouts/svelte/packages/svelte/src/constants.js:1-6`);
  `20 = EACH_IS_CONTROLLED | EACH_ITEM_IMMUTABLE`, `21 = 20 | EACH_ITEM_REACTIVE`. Verter
  sets it in `crates/verter_compiler/src/svelte/runtime/client_block_plan.rs:156-167` where
  the pinned official compiler's narrower dependency logic
  (`.oracle-checkouts/svelte/packages/svelte/src/compiler/phases/3-transform/client/visitors/EachBlock.js:45-83`)
  does not. It changes reactivity/effect topology — in contract, not cosmetic.
- `props-events` client, production request: genuine gap. The re-investigation independently
  invoked the realized pinned `svelte@5.56.8` compiler and it ACCEPTED the fixture
  (`{"dev":false,"accepted":true,"jsBytes":617,...}`). Verter refuses at
  `crates/verter_compiler/src/svelte/runtime/client_surface_script.rs:31-62`.
- No public/default route can request Svelte dev codegen. Confirmed independently across
  NAPI (`crates/verter_napi/src/lib.rs:248-275`), the protocol/WASM input
  (`crates/verter_protocol/src/types.rs:67-96`), the host `CompileProfile`
  (`crates/verter_session/src/types.rs:1322`), and unplugin
  (`packages/unplugin/src/index.ts:721-733`).
- The `script-title-declaration` mapping anchor is a legitimate in-contract requirement, and
  the mapping oracle is candidate-relative (it never compares against the official map), so
  there is no `sources`/`sourcesContent` scope-mismatch artifact.
- You weakened no pre-existing assertion; you changed no production behaviour; there are no
  stubs in the new files.

## What it OVERTURNED — fix each of these

**C1. The inventory is 3 reachable requests, not 6.** Because `dev` has no public spelling,
`dev0` and `dev1` issue the IDENTICAL production request — your own test at
`svelte_official_conformance_matrix.rs:366-404` proves it. So the six client records collapse
to three distinct reachable production requests (`basic-runes` client, `legacy-slots` client,
`props-events` client), and the three `dev1` goldens are NOT reachable-success cells at all:
they describe a request no public/default route can spell.

Make this a first-class, tested classification rather than prose. The reachable inventory must
be DERIVED from the option-expressibility fact you already prove, not hand-listed: a golden
whose recorded options cannot be expressed by any public request is classified
out-of-inventory by construction, and the gate must not report it as a failure. Add a
discriminating test that fails if a currently-inexpressible axis later becomes expressible
without the inventory following, and one that fails if an expressible axis is wrongly excluded.

Your phase-A headline "6/6 client cells fail" is materially overbroad and must not survive in
any evidence file. The honest statement is about three reachable requests.

**C2. The diagnostics plant has no application proof.** Unlike the other plants it never
calls `assert_plant_applied` (`svelte_official_conformance_gate.rs:420-445`). Give it a real,
equivalent proof that the planted diagnostic is absent before and present exactly once after.

**C3. The mapping plant does not discriminate the machinery the `legacy-slots` finding rests
on.** Corrupting `sourcesContent[0]` (`:447-485`) only proves the axis catches a blatant
content-integrity violation; a broken or disabled anchor/segment-coverage checker would still
pass it. Add a plant that specifically exercises anchor and segment-coverage enforcement —
for example, remove or displace exactly the segment that satisfies a required anchor in an
otherwise-passing map, and require the mapping axis to report the anchor failure. Prove the
plant applied (absent before, present exactly once after, strings differ), and keep the
unplanted control green. Keep the existing `sourcesContent` plant too; this is an addition.

**C4. The `legacy-slots` map measurement is UNDETERMINED, not established.** The
re-investigation could not decode the emitted candidate map (its sandbox was read-only), so
"1 source-bearing segment vs 34" is your claim, unverified. Settle it empirically: decode the
candidate's own emitted mappings, enumerate every source-bearing segment as
`line:column` in authored coordinates, and pin that inventory in a characterization test.
State the real number. If your original claim was wrong, say so.

## C5 — land the right test shape for the genuine defects

BF3 owns no compiler correction, so do NOT fix any of these defects. Land the evidence in a
shape that is honest in both directions:

1. **A conformance-target test per genuine defect.** Asserts the CORRECT official behaviour —
   authored from the official oracle's own output, not from Verter's. It cannot pass today, so
   mark it `#[ignore]` with a doc comment stating exactly what is wrong, the official
   behaviour it demands, and that un-ignoring it is the correction's acceptance gate. The body
   must be real and discriminating — never empty, never trivially true. There is precedent for
   exactly this shape in this repository.
2. **A characterization test per genuine defect.** Green today, pinning the exact current
   divergence (the exact flags value; the exact refusal code and message; the exact emitted
   segment inventory). It must fail if the behaviour changes in EITHER direction, so a silent
   regression or a partial fix is caught. This is test-side characterization only — it must
   never be read by, or influence, any production path.
3. The gate itself asserts the characterized current state for the three reachable requests,
   so it is green and discriminating, plus the `#[ignore]`d conformance targets above.

Name every test after the invariant it characterizes. No block IDs, no phase words.

## C6 — report

Rewrite `/tmp/bf3-phaseA-report.md` in place so it states what is actually true (three
reachable requests, corrected classifications, the settled map measurement), and write
`/tmp/bf3-phaseC-report.md` with:

- what changed and why, per overturned item;
- the settled `legacy-slots` segment inventory, with the decoded numbers;
- the new mutation plants and their applied-proofs;
- the list of `#[ignore]`d conformance-target tests by exact name, each with the defect it
  gates;
- the list of characterization tests by exact name;
- every claim about code you did not write, with its verifying `file:line`;
- anything you could not do and why.

Then `touch /tmp/bf3-phaseC-done`.

---

## Dispatch: `bf3-impl-brief-D.md`

# Phase D — close the three exhaustion gaps

Same worktree, same rules (TDD; no stubs; any claim about code you did not write is a
QUESTION with a `file:line`; ZERO production behaviour change; no program vocabulary;
WIP-commit per sub-step; dispose of nothing — no owner assignment, no debt row, no fix).

An independent architecture adjudication ruled that your phase-B `UNPROVEN` records do NOT
satisfy the exhaustion requirement, and that two of your stated reasons were wrong. Close
them. Its exact words on each are quoted below; they are the specification.

## D1 — the TypeScript-observable family

**Your premise was ruled wrong.** You recorded PublicApi/TSC/declaration as UNPROVEN because
supplying a `vue`/`svelte` type environment would "build a new oracle". The ruling:

> The premise that this necessarily "builds a new oracle" is wrong. Supplying the exact
> framework type environment is provisioning the observation domain. TypeScript remains the
> observer; the pinned framework declarations supply the meaning of the imports.
>
> This is conformance-harness work and is inside reshaped BF3's exhaustion responsibility. It
> is not production TypeScript-product correction.

So: provision it. The harness already realizes the exact pinned framework closures under
`packages/framework-conformance-harness/.oracle-installs` (verified working — the Svelte
conformance gate drives them). Those installs carry the pinned packages' own `.d.ts` files.
Make the existing TypeScript observation host resolve framework imports against THOSE, at the
pinned versions, rather than degrading them to `any`.

The ruling's closure conditions, all five required:

1. the exact pinned Vue/Svelte declaration and dependency closure is resolvable by the
   existing TypeScript host;
2. candidate and reference observations run under the identical TypeScript version, options,
   framework closure, and module-resolution environment;
3. **module-resolution failure FAILS the authoritative observation instead of degrading
   silently** — this is the defect that made your phase-B run meaningless, and it must be
   impossible for a future run to repeat it;
4. a planted control proves a correct prop surface and an empty/wrong one produce DIFFERENT
   observations (your `any == any` result is exactly what this control exists to prevent);
5. semantic assertions over props, events, exports, bindings, diagnostics, and
   declaration-only behaviour — not merely byte agreement or symbol presence.

Then drive every in-scope PublicApi / TSC / declaration cell through it and record a real
result per cell. Vue's non-runtime products are in scope; Vue's VDOM/Vapor/SSR runtime-render
output is not.

Expect this to confirm your finding that an untyped Svelte `$props()` destructure publishes an
empty props surface — and, this time, to prove it with an oracle that could have said
otherwise.

## D2 — Svelte `compile_many`

> It must be executed. Route is part of capability-cell identity, and "same semantic cells"
> does not establish batch equivalence. Closure requires successful and refused Svelte inputs
> through `compile_many`, per-item comparison with the corresponding single-file route, stable
> ordering, no cross-item contamination, and proof of the batch failure/partial-result
> contract. Source citations showing delegation are useful route-identity evidence, not an
> executed result.

Do exactly that. Include at least one supported Svelte component and both refusal cases
(`generate: "server"`, and the `props-events` advanced-rune refusal) in one batch, and prove
per-item results match the single-file route, that ordering is stable, and that one item's
outcome does not contaminate another's.

## D3 — NAPI and WASM transports

> Reachable public transports must be built and invoked. They need not repeat the full
> semantic matrix when structural delegation proves the same typed request, but each transport
> must execute representative success and refusal/optional-product cases proving option
> conversion, serialization, artifact presence/absence, and route equivalence. The need for
> `napi build --release` or `wasm-pack` is an execution prerequisite, not grounds for
> `UNPROVEN`.

Build them and drive them. Choose the cheapest build that is still faithful (a debug NAPI
build is acceptable if it exercises the same conversion and serialization code — say which you
chose and why). **Bound every build**: pass an explicit job cap (e.g. `CARGO_BUILD_JOBS=4`)
and never run a bare unbounded workspace build. This machine has 24 GB of RAM and has been
hard-rebooted twice by unbounded builds. Never run `node scripts/gate.mjs`.

Per transport, execute representative cases covering: at least one success publishing its
products, at least one typed refusal, and at least one optional-product axis (source map on
and off). Assert option conversion, serialization shape, artifact presence AND absence, and
equivalence with the in-process host route for the same typed request.

The ruling also addressed the completeness guard you could not land:

> The forbidden name-keyed scanner is not required. Repository policy expressly prohibits
> landing such guards. Exhaustion is a claim about the pinned current tree; independent export
> enumeration plus executed known routes can close it.

So enumerate each transport's exports independently (from the built artifact's own surface,
not by grepping source) and prove every in-scope exported spelling is either executed or
explicitly classified out of scope with a reason.

## The standard your records must meet

> An honest `UNPROVEN` record identifies the exact claim, the missing discriminating
> observation, why existing evidence cannot decide it, its owner, and a falsifiable closure
> condition — and it blocks acceptance. A gap dressed as `UNPROVEN` uses nondiscriminating
> equality such as `any == any`, calls a batch or transport route "the same" without executing
> its boundary, or lets explanatory prose count as an actual result.

If anything still cannot be closed, record it to that standard and say so plainly. Do not
dress a gap as an honest unknown, and do not assert a pass you did not observe.

## D4 — report

Write `/tmp/bf3-phaseD-report.md` and print it:

- per-cell results for the whole TypeScript-observable family, with the planted control's
  red-green evidence and proof the plant applied;
- the `compile_many` batch results and the per-item comparison against the single-file route;
- per-transport execution results, the export enumeration, and the equivalence proofs;
- the exact build commands you ran and their job caps;
- anything still unproven, to the standard quoted above;
- every claim about code you did not write, with its verifying `file:line`;
- a corrected phase-B statement wherever phase B said something this phase disproved.

Then `touch /tmp/bf3-phaseD-done`.

---

## Dispatch: `bf3-impl-brief-E.md`

# Phase E — close the last two residues

Same worktree, same rules (TDD; no stubs; any claim about code you did not write is a QUESTION
with a `file:line`; ZERO production behaviour change; no program vocabulary; bounded builds
only, never `node scripts/gate.mjs`, never a bare workspace build; WIP-commit per sub-step;
dispose of nothing).

Your phase-D report named exactly two residues. The exhaustion exit does not admit an unproven
cell, so close both. Then a short hygiene pass.

## E1 — the IDE/TSX product family under the TypeScript oracle

You named the closure condition yourself:

> a second observation domain resolving the workspace's built `@verter/svelte-jsx` and
> `@verter/types` declarations, plus a planted control proving a correct and a broken IDE
> surface observe differently.

Build exactly that. The workspace TypeScript packages are already built in this worktree
(`pnpm build:ts` was run before you started), so their emitted declarations exist on disk —
find them and resolve against them rather than hand-writing shims. Keep every property the
framework domain already has: identical TypeScript version and options across candidate and
reference, module-resolution failure REFUSING the observation rather than degrading to `any`,
and a planted control that could have said otherwise.

Then drive the IDE/TSX surfaces for BOTH carriers through it and record a real result per cell.
The Svelte carrier's IDE projection is documented as type-checking clean through TSGO
(`crates/verter_compiler/src/svelte/carrier.rs:186`) — that is a claim; your observation is
what decides it. If it does not hold, that is a finding, recorded and not fixed.

If the workspace declarations genuinely cannot be resolved for a reason you can demonstrate,
say so to the same standard you used in phase D — exact claim, missing discriminating
observation, why existing evidence cannot decide it, owner, falsifiable closure condition —
and show the evidence that the obstacle is real rather than inconvenient.

## E2 — `compile_many` through the NAPI transport

Your own closure condition:

> the same per-item comparison, run through `VerterHost#compileMany` on the built artifact.

Run it. The NAPI artifact is already built. Include the same batch shape you used in-process:
a supported Svelte component, plus both refusal-shaped inputs, plus ordering and
non-contamination assertions, compared item-for-item against the in-process host's answers for
the same typed request. Follow the pattern your transport tests already use — the Rust side
does the comparing against the host's own answers, never against transcribed constants.

## E3 — hygiene sweep before review

Check and fix, across the whole range `040084bf0..HEAD`:

1. **Machine paths.** No committed file may contain `<HOME>` (the known
   guard-file exemption aside). Check committed JSON artifacts and generated fixtures too —
   `git grep -n '/Users/' $(git rev-parse HEAD) -- .` style. Anything found must become a
   repo-relative or discovered path.
2. **Program vocabulary.** No block IDs, revision names, charter/amendment references, or
   phase/round/cutover words in any source, comment, test name, file name, or commit message.
   Audit commit BODIES, not just subjects: `git log --format=%B 040084bf0..HEAD`. Existing
   `bf2_*` file names in the tree are grandfathered; anything YOU introduced is not, and any
   `bf2-authoritative` feature reference you must keep is a pre-existing identifier, not new
   vocabulary — but say explicitly in your report where you relied on that.
3. **Layout and hermeticity.** Confirm you added no second top-level `tests/*.rs` in any crate,
   and that nothing you added requires a third-party repository checked out beside this one.
   Confirm every test you added that needs the gitignored oracle installs or a built
   native/wasm artifact either FAILS loudly naming its producing command, or is `#[ignore]`d by
   default — never silently skips into a false pass.
4. **Cross-platform.** No hardcoded `/` or `\` path concatenation in anything you added; paths
   built with `Path`/`PathBuf`/`join`. No reliance on a POSIX-only shell built-in inside a test.
5. **Stub sweep.** Re-open every test file you added across all phases and verify each body is
   non-empty and each assertion discriminating. Report the count of tests you re-read and any
   you changed.
6. Run, and report the exact output tail of each:
   - `cargo fmt --all --check`
   - `cargo clippy -p verter_session --lib --all-targets --features bf2-authoritative -- -D warnings`
   - the full Rust test set you have added, by filter, plus the harness's own
     `npx vitest run` in `packages/framework-conformance-harness`.

## E4 — report

Write `/tmp/bf3-phaseE-report.md` and print it: E1 per-cell results with the control's
red-green evidence and applied-proof; E2 results; the E3 sweep findings and what you changed;
the exact command outputs from E3.6; every claim about code you did not write with its
verifying `file:line`; and anything still unproven to the phase-D standard.

Then `touch /tmp/bf3-phaseE-done`.

---

## Dispatch: `bf3-impl-brief-F.md`

# Fix round 1 — close the review findings

Same worktree, same rules (TDD; no stubs; any claim about code you did not write is a QUESTION
with a `file:line`; ZERO production behaviour change; bounded builds only; never
`node scripts/gate.mjs`; WIP-commit per sub-step).

Two independent reviewers returned BLOCKING. Their findings below are the specification. Some
are things you must fix; two are things you must NOT fix because they are not yours. Do not
argue with a finding — either fix it, or report precisely why the reviewer's premise is wrong
with a `file:line` that settles it. "I disagree" without evidence is not a response.

## F1 — drive all SIX exact client cells, not three

Both reviewers read the charter's "probe the exact `svelte@5.56.8` client cells" as a quantifier
over the six committed client records, and read your three-request collapse as failing it.

Your reachability analysis is not wrong — `dev0` and `dev1` genuinely issue one production
request — but "these two cells map to one request" is an EXPLANATION, not a reason to leave two
cells without a result. Satisfy both readings: drive **every one of the six committed client
cells** through the oracle and record each cell's actual outcome, then keep the reachability
classification as the recorded reason a `dev1` cell's divergence is not attributed to the
compiler. More evidence, not less.

Do the same for the six server cells if any is not already individually recorded.

The refused `props-events` request currently never enters the comparator at all. Record it as an
explicit cell outcome — request, route, profile, the typed refusal, and the fact that no
candidate exists to compare — so no cell in the inventory is silent.

## F2 — remove every generated-output string-scan (this is a charter prohibition)

Both reviewers flagged this independently, and it is the finding I most want closed properly.
The charter says the block "cannot infer meaning from generated output or introduce
string-scanning as a second semantic authority". You currently do, in at least two places:

- `each_flags_argument` parses the emitted module with `split_once("$.each(")`
  (`crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs`
  around line 491).
- Batch carrier identity is classified by output substrings such as `svelte/internal/client`,
  `_sfc_main`, `?vue&type=` (`crates/verter_session/src/svelte_batch_route_tests.rs` around
  line 142).

Replace both with evidence that is not a text scan:

- For the `{#each}` flags: read the argument STRUCTURALLY. Parse the emitted module to an AST
  and read the call's second argument, or obtain the value from typed data the compiler already
  produces. The repository has real parsers; find the right one and cite it. A regex or
  `split_once` over generated text is exactly what the prohibition names.
- For batch carrier identity: use typed route evidence — what the host itself recorded about the
  carrier, language, or adapter that handled the item — not what the bytes look like. If, after
  looking, NO typed signal distinguishes them, say so with the `file:line` that proves the
  absence, and then use the conformance harness's own structural comparator rather than
  substrings. The absence of a typed signal would itself be a finding worth recording.

Sweep every test you added for any other instance of the same pattern and report the count you
checked.

## F3 — make the batch characterization discriminating in the worsening direction

`svelte_batch_route_tests.rs` around line 220 asserts only the ABSENCE of two refusal-code
strings, so arbitrarily worse output stays green. A characterization must fail if the behaviour
changes in either direction. Fix it, and re-check every other characterization you landed for
the same weakness — report how many you re-read and which you changed.

## F4 — make the refusal inventory structural, not hand-listed

`refusal_cells()` in `framework_product_surface_tests.rs` is a hand-written list, so
`FC-ATOMIC-001` is proven over the refusals you happened to think of. Derive the refusal
inventory from a typed source (the refusal enum, the typed unsupported-surface taxonomy, or
whatever the shared owner actually enumerates) so a newly added refusal variant is covered
automatically or fails the test. Do not do this with a source-tree text scan.

## F5 — close the enumerated-but-unexecuted routes

- `compile_with_audit_options` (`crates/verter_session/src/host_compile_audit.rs:111`) is a
  separate public spelling and is missing from the inventory. Add and drive it.
- `ensureIdeCompiled` and `getIde` were classified out of scope at
  `crates/verter_session/src/transport_route_equivalence_tests.rs` around line 526 as "a
  different product family". They are not out of scope — the IDE/TSX family is in this block's
  retained scope, and you proved it observable in the previous phase. Execute them on both
  transports.
- The bundler (`packages/unplugin`) route aliases are recorded as citations and never executed.
  Drive them against the built native binding. If some bundler spelling genuinely cannot be
  executed here, record it to the honest-unproven standard (exact claim, missing discriminating
  observation, why existing evidence cannot decide it, owner, falsifiable closure condition) —
  and show the obstacle is real rather than inconvenient.

Re-check whether anything else in your inventory is classified out of scope on a reason that
does not survive contact with the charter's retained scope.

## F6 — feature-gate the transport tests instead of `#[ignore]`

Six transport tests are `#[ignore]`d by default, so their owning gate never sees them. Put them
behind an opt-in cargo feature the way the conformance gate is, so they are a first-class
surface that a gate can enable, rather than invisible-by-default. Keep the loud-failure-on-
missing-artifact behaviour exactly as it is.

One reviewer also argued these tests sit at the wrong altitude — `verter_session` unit tests
orchestrating artifacts built FROM `verter_session`. Assess it honestly: if the coupling is only
a spawned process reading a built file (not a crate-graph edge), say so with the evidence and
keep them where they are; if it is a genuine dependency inversion, say so and state what the
correct home is. Do not move them on a guess.

## F7 — Windows portability

- `packages/wasm/scripts/probe-transport-surface.mjs` around line 24 builds `file://${jsEntry}`.
  On Windows that is not a valid file URL. Use Node's `pathToFileURL`.
- `packages/framework-conformance-harness/src/typescript-observe.mjs` around line 544 uses an
  unnormalized `path.relative`, so identity carries backslashes on Windows and the same
  observation would produce a different identity per platform. Normalize separators.

Sweep everything you added for the same two classes and report what you found.

## F8 — program vocabulary in source

Remove it. Known instances:

- `packages/framework-conformance-harness/test/typescript-observation-domain.spec.mjs` ~line 103
  ("phase-B")
- `crates/verter_session/src/framework_product_surface_tests.rs` ~line 721 ("phase-A or phase-B")
- `crates/verter_session/src/compile/map_equality_tests/public_api_typescript_observation.rs`
  ~line 417 ("phase-B")

Sweep everything you authored for phase/round/cutover/block vocabulary and report the full list
you found and fixed. Rewrite each comment to describe the invariant, not the process that
produced it.

## F9 — the committed per-cell and per-failure record

Both reviewers found no committed record of "exact request, route, profile, products, official
domain, and failure" — only temporary per-run JSON. Land a committed, machine-checkable record
under `docs/arch/refactor/rev11/evidence/BF3/` covering every cell in the inventory:
the exact request, route, profile, products, the pinned official domain, and the outcome
(pass / fail with the concrete divergence / refused with its typed code / unproven with its
closure condition).

Record FACTS only — do NOT assign correction owners, acceptance IDs, or dispositions. Those are
decided above you and will be added separately. Add a test that fails if a cell the suite drives
is missing from the record, or if a recorded outcome no longer matches what the suite observes.

## What you must NOT do

Two reviewer findings are correct but are NOT yours to close, and attempting them would be worse
than leaving them:

- The charter's steps 3–5 (pre-publication detection, typed non-success, whole-cell retraction)
  and its guard-removal-ID exit require a ratified amendment to supersede. Do not implement a
  production mechanism, and do not write an amendment. Leave them.
- The correction owners, DAG edges, and acceptance identifiers are decided above you. Do not
  invent a `BS0`/`BA0`/`BCSS0` reference in code or tests.

## Report

Write `/tmp/bf3-fix1-report.md` and print it: one section per finding F1–F9 with what you
changed and the evidence it works; the sweeps' counts; anything you assessed as a wrong premise
with the `file:line` that settles it; every claim about code you did not write with its
verifying `file:line`; and the exact output tail of `cargo fmt --all --check`, the clippy
invocation you used, and every test filter you ran.

Then `touch /tmp/bf3-fix1-done`.

---

## Dispatch: `bf3-impl-brief-G.md`

# Fix round 2 — adversarial findings

Same worktree, same rules (TDD; no stubs; any claim about code you did not write is a QUESTION
with a `file:line`; ZERO production behaviour change; bounded builds only; never
`node scripts/gate.mjs`; WIP-commit per sub-step).

An adversarial reviewer planted mutations against your suite and found real holes. Several of
its plants stayed GREEN — those are the findings that matter most. Fix each. Where you think a
premise is wrong, refute it with a `file:line`, do not argue.

**One thing changed above you:** the scope rulings have now been ratified by the maintainer. So
the "unratified deviation" class of finding is closed, and the disposition/owner work you were
previously told NOT to do is now partly in scope — but only where this brief says so, and never
in `crates/` source. Still no production mechanism, still no compiler fix.

## G1 — a non-discriminating test the reviewer's plant proved worthless

`the_options_taking_audited_compile_entry_honours_its_explicit_options` claims to exercise the
`source_map` axis in both directions but asserts only `canonical_id`. The reviewer forced
`source_map: true` in production and the test **stayed green**.

Fix it so it actually discriminates the axis it names, then **prove it**: re-apply that same
plant yourself, show the plant applied (marker absent before, present exactly once after),
observe RED, revert, observe GREEN, and show the marker count back to zero.

Then re-audit EVERY test you have landed with the same question — "does this assert the thing
its name claims?" — and report the count you re-read and every one you changed. A test whose
name promises an axis and whose body ignores it is the exact failure this round exists to catch.

## G2 — export completeness is a union, so a spelling can be dropped silently

Dropping `ensureIdeCompiled` from `NAPI_EXECUTED` keeps the completeness test green, because the
spelling also appears in the out-of-scope list. Two lists that may both contain the same name
cannot prove completeness.

Make the classification a PARTITION: every enumerated spelling belongs to exactly one class, and
membership in two is itself a failure. Prove the fix by dropping a spelling from the executed
list and observing red, then restoring it. `compileMany` is reportedly on both lists too — check
every entry.

## G3 — the fail-closed module-resolution gate has a bypass

The observation domain refuses an unresolvable `import("x")` type node, an import declaration,
and a module augmentation — but a `require("svelte")` call reportedly slips through and yields
`any`. A fail-closed gate with a known bypass is not fail-closed.

Close it for every specifier form TypeScript can resolve, and prove the fix with a planted
control per form (including the `require` form the reviewer found). Enumerate the forms you now
cover and say how you established that enumeration is complete, rather than asserting it.

## G4 — program vocabulary reached `crates/`

`svelte_official_conformance_gate.rs` around line 1332 does
`include_str!("…/docs/arch/refactor/rev11/evidence/BF3/svelte-cell-record.json")`, putting the
program's revision and block identifiers into crate source.

Move the machine-readable record to a home under `crates/verter_session/src/` — the same
treatment `framework_product_surface_inventory.json` already gets — named after what it
contains, and referenced from the evidence tree rather than the other way round. No path under
`crates/`, `packages/` or `scripts/` may name the program, its revision, or a block.

Sweep everything you authored for the same class (paths, `include_str!`, `include!`, string
literals, module docs) and report the full list.

## G5 — O2: the Vue cold-path test string-scans generated output

`a_vue_carrier_keeps_publishing_its_non_runtime_products` asserts
`code.contains("label: string")` and `!code.contains("defineComponent(")`. That is inferring
meaning from generated text — the charter prohibition you already removed two instances of.

You now have a TypeScript observation that reads the CHECKER's own view of the declaration. Use
it: assert the prop's presence and type, and the declaration-only property, from the observation
rather than from substrings. Then sweep once more for any remaining instance and report the
count of `.contains(` sites you re-classified, with the class of each.

## G6 — the pinned version is never actually asserted

Nothing in the new Rust assertions requires the domain to equal `5.56.8`; the public-API check
only requires `packageVersion.is_string()`. A suite that would pass against a different pin does
not discharge "must probe the exact `svelte@5.56.8` client cells".

Assert the exact pin wherever a cell's domain is observed, sourced from the committed domain
authority rather than a transcribed literal where one exists. Prove it: change the expected pin
locally, observe red, revert.

## G7 — the client runtime axis never runs

For all six client cells the `runtime` axis reports `not-applicable`, and the probe scope matrix
calls for a Svelte client runtime smoke. The harness makes only `generate: "server"` Svelte
goldens runtime-applicable, yet the harness also ships `src/execute-svelte-runtime.mjs`, and a
Svelte CLIENT module is executable in a DOM environment.

Determine which is true: is client runtime genuinely not applicable, or is the harness's
applicability rule a limitation rather than a fact? Read it and say which, citing lines. If it
is a limitation and a client runtime smoke can be driven for the cells that emit, drive it. If
it genuinely cannot be, record it to the honest-unproven standard (exact claim, missing
discriminating observation, why existing evidence cannot decide it, owner, falsifiable closure
condition).

## G8 — cold-path tests assert existence, not behaviour

`a_supported_svelte_client_component_keeps_publishing_its_module_and_its_css` asserts
`code_len > 0`, `lang`, and `has_map` — any non-empty output of the right language passes. That
is not "retains behaviour".

Strengthen every cold-path test to pin behaviour a correction could plausibly break: exact bytes,
or a structural property read from a real parser or from the TypeScript observation — never a
length or a substring. Report each cold-path test and what it now pins.

## G9 — remaining exhaustion gaps

- The inventory still says `compile.batch` svelte is `"not probed here — see the report's
  UNPROVEN rows"`. It IS probed now, and no such report is committed. Correct the row to state
  the actual outcome.
- `get_diagnostics` is listed as a route and is never called by any test you added. Drive it.
- No TSC product row and no TSC test exist, though the retained scope names TSC. Either drive it
  or record its absence to the honest-unproven standard with the evidence that it is not a
  reachable product.
- Every official cell hardcodes `source_map: true`, so the maps-off axis is never an official
  cell. State whether that is a property of the committed goldens (in which case say so with the
  evidence) or a gap you can close.
- Verify whether any spelling in the inventory is still unexecuted, and close or honestly record
  each.

## G10 — a genuine-refusal batch atomicity check

A reviewer reported the batch publishing a partial product for a refused item. Under the
wrong-carrier defect the Svelte refusal never fires, so that observation may be an artifact of a
Vue-shaped success rather than a genuine refusal-plus-product.

Settle it: construct a batch item whose refusal is GENUINE on the carrier the batch actually
selects, and determine whether a product is published alongside the typed refusal. Report which
it is, with the evidence. Add the appropriate test for whichever answer is true — a
characterization if the leak is real, or a proof of atomicity if it is not.

## G11 — non-vacuous test execution

A reviewer noted a test filter that matched zero tests and still exited 0. Every result you
report must show a non-zero executed count. When you report a filter's result, include the
`running N tests` line or the `N passed` summary, and make sure N is not zero. If any filter you
have used historically matched nothing, say which.

## G12 — the disposition ledger (newly in scope, evidence tree only)

The rulings are now ratified, so the per-failure dispositions must be recorded. Write
`docs/arch/refactor/rev11/evidence/BF3/dispositions.md` containing one row per finding with:
the finding id and description, its class, its disposition, its durable owner block, its
resolution gate, and its acceptance identifier and named acceptance test. Use exactly these
rows, which are the ratified table — do not invent, rename, or re-class any of them:

| id | finding | class | owner | acceptance id |
|---|---|---|---|---|
| SV-1 | `{#each}` flags set `EACH_ITEM_REACTIVE` where official does not (21 vs 20) | Svelte compiler defect | BS0 | `BF3-SV-1-EACH-FLAGS` → `FC-SVELTE-001` |
| SV-2 | `$props()` non-interpolation instance-script usage refused; official accepts | Svelte compiler gap | BS0 | `BF3-SV-2-PROPS-INSTANCE` → `FC-SVELTE-001` |
| SV-3 | client source map omits authored script-declaration provenance | Svelte compiler mapping defect | BS0 | `BF3-SV-3-CLIENT-MAP-SCRIPT` → `FC-SVELTE-001` |
| SV-4 | untyped `$props()` destructure publishes an empty props surface, no diagnostic | Svelte session-projector defect | BS0, distinct item | `BF3-SV-4-PROPS-SURFACE` → `FC-TS-001` |
| RT-1 | the batch route compiles `.svelte` as Vue and drops its refusals | public batch route / carrier-selection defect | BRT0 | `BF3-RT-1-BATCH-CARRIER-PARITY` → `FC-ROUTES-001` |
| AT-1 | a combined IDE-requesting compile publishes the TSX product after a runtime refusal | atomicity violation | BA0 | `BF3-AT-1-COMBINED-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` |
| AT-2 | a batch entry publishes a product together with a genuine typed refusal | per-entry atomicity violation | BA0, distinct item | `BF3-AT-2-BATCH-REFUSAL-ATOMICITY` → `FC-ATOMIC-001` |
| CSS-1 | the standalone CSS route accepts and ignores `sourcemap: true` | option/product-contract defect | BCSS0 | `BF3-CSS-1-STANDALONE-SOURCEMAP` → `FC-OPTIONS-001` |
| TR-1 | NAPI returns null where WASM throws for a missing product | portable transport-contract defect | BRT0, distinct item | `BF3-TR-1-MISSING-PRODUCT-PARITY` → `FC-ROUTES-001` |
| RA-1 | `list_virtual_files` names `Main` for a component whose runtime surface is refused | parse-derived route-assembly artifact | — | REJECTED as a defect |
| RA-2 | `has_runtime_surface` counts styles, so a refusal publishing CSS would take the wrong arm; no reachable state | latent | — | REJECTED as a defect |

Every DEFER's resolution gate is its owner block's acceptance, no later than plan close, and
before any downstream dispatch. Add rows for the two bundler facts you recorded (the
`transformInclude` / `transform` disagreement on a `.svelte` id, and the missing source map
despite a `sourceMap: true` profile) as `BND-1` and `BND-2`, owner `BRT0`, marked as
post-dating the ratified table and awaiting confirmation.

For each row name the existing test that gates it. Where a row has no test yet — `AT-1` is the
clear case — add a real `#[ignore]`d conformance target asserting the correct behaviour, with a
body that fails for the stated reason, and name it in the row.

**Block identifiers appear ONLY in this evidence file, never in `crates/`, `packages/` or
`scripts/`.**

## Report

Write `/tmp/bf3-fix2-report.md` and print it: one section per G1–G12 with what changed and the
red-green evidence (including applied-proofs for every plant you ran); the audit counts from G1,
G5 and G8; every claim about code you did not write with its verifying `file:line`; the exact
non-vacuous output of every test filter you ran; and anything you assessed as a wrong premise
with the `file:line` that settles it.

Then `touch /tmp/bf3-fix2-done`.

---

## Dispatch: `bf3-impl-brief-H.md`

# Final fix — scoped strictly to the delta review's findings

Same worktree, same rules (TDD; no stubs; ZERO production behaviour change; bounded builds; never
`node scripts/gate.mjs`; WIP-commit per sub-step). This is the LAST fix pass. Its scope is
exactly the items below and nothing else — do not open new work.

## H1 — BLOCKING: your runtime-axis test is RED, and your report said it passed

`every_emitting_client_request_mounts_and_renders_what_the_golden_renders` FAILS. I ran it myself:

```
cargo test -p verter_session --lib --features bf2-authoritative every_emitting_client_request_mounts -- --test-threads=1 --nocapture
```

```
svelte/basic-runes__client__runes1__dev0: the official module did not mount, so this smoke
cannot decide anything: "TypeError: Cannot read properties of undefined (reading 'call')
    at get_first_child (…/.oracle-installs/svelte/node_modules/svelte/src/internal/client/dom/operations.js:91:64)
    at …/internal/client/dom/template.js:74:58
    at Basic_runes (…/.bf2-scratch/…/svelte-client-ddba8529e2587f1e.mjs:12:12)
    …
test result: FAILED. 0 passed; 1 failed
```

An independent reviewer found the same thing. Your fix-round-2 report claimed "both emitting
client requests mount and render byte-identically to their goldens" — that claim was false.
Before you fix anything, work out how you came to report a passing result for a test that fails,
and say so in your report: that process failure matters more than this one test.

The control itself is what breaks — the pinned OFFICIAL module does not mount — so the axis
currently decides nothing. `get_first_child` reading `.call` of `undefined` points at a DOM
global or prototype accessor the executor never installed, not at the candidate.

Fix it properly: make the official control mount, and only then compare the candidate against it.
Two outcomes are acceptable and no others:

1. The executor is corrected, the official control mounts, and both emitting client requests are
   genuinely compared — with the candidate's rendered output pinned. Prove it by running the test
   and showing `1 passed`.
2. You establish, with evidence, that a client mount genuinely cannot be driven in this
   environment. Then REMOVE the axis rather than leave a red test, and record it to the
   honest-unproven standard — exact claim, missing discriminating observation, why existing
   evidence cannot decide it, owner, falsifiable closure condition — in the evidence tree.

Do not weaken the assertion to make it pass. Do not `#[ignore]` it as a disposal: an `#[ignore]`
is only legitimate for a conformance TARGET that states correct behaviour the compiler does not
yet meet, and this is not that — this is your own harness failing to establish its control.

## H2 — the zero-test-filter disclosure is not in the tree

You disclosed in your report that three test-filter invocations matched ZERO tests and exited 0
(the `"the_tsc_product\|the_diagnostics_route"` alternation, and two suites run without the
`bf2-authoritative` feature). The reviewer searched the evidence tree and found no trace of it —
so the disclosure exists only in a temporary file.

Land it in the evidence tree: which filters matched nothing, why (libtest treats an alternation
as one literal substring; the modules are feature-gated), and the correct invocation for each
suite. A future reader must be able to tell a genuine green from a filter that ran nothing.

While you are there, make each suite's own module documentation state the exact invocation
including its required feature, so a wrong invocation is discoverable at the source.

## H3 — the Windows-path fix is asserted but never exercised

`pathToFileURL` and the separator normalization were verified by reading, not by running. Add a
cheap discriminating check: drive the identity-building code with a Windows-shaped path and
assert the resulting identity contains no backslash. Prove it fails if the normalization is
removed.

## H4 — one reference form still slips the fail-closed gate

The reviewer found `/// <reference path="./missing.d.ts" />` is not refused by the module-
resolution gate; it surfaces only as a `TS6053` diagnostic. Your gate covers
`typeReferenceDirectives` but evidently not path references. Close it the same way you closed the
others, and extend the per-form refusal table to include it.

## Report

Write `/tmp/bf3-fix3-report.md` and print it: H1 with the root cause, the fix, and the test
output showing a non-zero passing count (or the removal plus its honest-unproven record); your
account of how the false pass was reported; H2's landed disclosure; H3 and H4 with their
red-green evidence; and the exact output of `cargo fmt --all --check` plus every clippy and test
invocation you ran, each showing a non-zero executed count.

Then `touch /tmp/bf3-fix3-done`.

---

## Dispatch: `bf3-impl-brief-I.md`

# Gate fixes — five architecture guards this work broke

The branch to work on is **`work/bf3-landing`** in the usual worktree (it is checked out
already; do not switch branches). Same rules as before: TDD, no stubs, ZERO production
behaviour change, no program vocabulary in `crates/`/`packages/`/`scripts/`, bounded builds,
never run `node scripts/gate.mjs`, WIP-commit per sub-step.

I ran the canonical gate on the final tree. It FAILED with six distinct guard failures.
**Five are ours.** Fix exactly those five. The sixth is not ours and you must not touch it.

```
FAIL cases::g_misc1::no_lib_rs_growth::lib_rs_stays_under_line_ceiling
FAIL cases::architecture_guards::foundations_guards::no_std_fs_in_semantic_session_paths
FAIL cases::architecture_guards::foundations_guards::no_std_fs_outside_native_fs_or_allow_list
FAIL cases::architecture_guards::foundations_guards::vfs_boundary_is_authoritative
FAIL cases::architecture_guards::no_direct_oxc_parser_calls_outside_scheduler_path
FAIL cases::tracked_paths_no_machine_roots::…   <-- NOT OURS, see below
```

## I1 — `lib_rs_stays_under_line_ceiling`

```
crates/verter_session/src/lib.rs has 862 lines (ceiling: 857)
```

`lib.rs` was 856 lines at the base; our six `#[cfg(test)] mod` declarations pushed it to 862.
The guard's own message names the sanctioned remedy pattern.

Do NOT raise the ceiling. Move our test-module declarations off `lib.rs` — for example into a
single `#[cfg(test)]` aggregator module that owns them — so `lib.rs` returns to its base line
count or below, and every test we added still runs. Prove it: show the new `wc -l` for
`lib.rs`, and re-run every one of our suites showing a non-zero executed count and the same
pass/ignore numbers as before.

## I2 — the three `std::fs` guards

```
no_std_fs_in_semantic_session_paths
no_std_fs_outside_native_fs_or_allow_list
vfs_boundary_is_authoritative
```

Our new test modules call `std::fs::` directly (writing candidate payloads for the oracle CLI,
reading committed fixtures, staging scratch modules). There is an established, sanctioned
precedent for exactly this: the existing tool-output allowlist already carries
`crates/verter_session/src/compile/map_equality_tests/bf2_seed_matrix.rs`
(`crates/verter_workspace/tool-output-allowlist.toml:119`), which does the same thing for the
same reason.

Read that entry and its rationale, then add our modules the same way, with a rationale that
states the actual reason per entry — a test driving an external tool needs real files on disk;
it is not a semantic-session read path and never goes through the VFS.

Do NOT weaken any guard, broaden a pattern, or add a wildcard. Allowlist the exact files. If a
call can reasonably avoid `std::fs` instead of being allowlisted, prefer that. Report per file
which choice you made and why.

## I3 — `no_direct_oxc_parser_calls_outside_scheduler_path`

Our structural reader for the `{#each}` flags argument parses the emitted module with
`oxc_parser` directly. That is the right implementation — it is what replaced a forbidden
string-scan — but the guard does not know it is a test reading generated output rather than a
production parse path.

Handle it the same way: read the guard at
`crates/verter_session/tests/cases/architecture_guards.rs:10476` and its allowlist mechanism,
and register our reader exactly, with a rationale saying it parses a candidate's own generated
output inside a test and is not a file-processing path. Again: exact entry, no wildcard, no
weakening.

## NOT OURS — do not touch

```
cases::tracked_paths_no_machine_roots::tracked_files_contain_no_machine_specific_path_markers
  docs/arch/architecture-lock/ledger/program-state.toml: contains machine-specific marker `<HOME>`
```

I verified this is pre-existing: that marker is present at our base commit and on the current
program tip, and our diff does not touch that file. It is the program ledger, which is owned
elsewhere and which we are forbidden to edit. Leave it. Do not edit the ledger, do not
allowlist it, do not "fix" it. It is being reported upward separately.

## Verification

After the fixes, run and report with non-zero executed counts:

- `wc -l crates/verter_session/src/lib.rs`
- the five previously-failing guards, individually, by name, each now passing:
  `cargo test -p verter_session --test main <guard_name>`
- every suite we added, at its correct invocation including its feature
- `cargo fmt --all --check`
- `cargo clippy -p verter_session --all-targets --features bf2-authoritative,transport-authoritative -- -D warnings`

Do NOT run the full gate; I will.

## Report

Write `/tmp/bf3-gatefix-report.md` and print it: per fix, what changed and the evidence; for
each allowlist entry, the exact file and the rationale you wrote; the command outputs above;
and confirmation that no guard was weakened and no production file changed.

Then `touch /tmp/bf3-gatefix-done`.

---

## Dispatch: `bf3-review-conformance.md`

# HARD EFFICIENCY CONTRACT — READ FIRST

ONE turn. Never `cat`/`nl -ba` a file over 120 lines — use `sed -n 'A,Bp'` or `rg -n -C 5`.
Budget your commands; when in doubt, stop investigating and write the report. An unfinished
review is a FAILED dispatch. The report is the deliverable; do not emit a plan.

# Mandate: CONFORMANCE

You are the conformance reviewer. Your question is narrow and total: **does this candidate do
exactly what its charter requires, prove it non-vacuously, and delete what it said it would?**

You are not asked whether the work is good. You are asked whether each charter obligation is
DISCHARGED, with evidence you verified yourself. Run the tests you are citing — do not accept a
report's word for a result. The candidate's own reports under `/tmp/bf3-phase*-report.md` are
CLAIMS BY THE AUTHOR, not evidence; treat them as a map of where to look, never as proof.

Specific things a conformance reviewer must check here and not assume:

- Does the test suite actually EXECUTE against the exact pinned `svelte@5.56.8` domain, or
  could it pass against a different pin? Prove which.
- Does every test that needs a gitignored oracle install or a built native/wasm artifact FAIL
  loudly when it is absent, or can it silently skip into a false green? Test this by moving the
  artifact aside and re-running, then restoring it.
- Are the `#[ignore]`d conformance-target tests real? Run them with `--ignored` and confirm each
  fails for the stated reason and not for an unrelated one (a missing file, a compile error, a
  panic in setup).
- Do the characterization tests fail in BOTH directions — if the defect got worse AND if it got
  silently fixed?
- Is the claimed inventory actually exhaustive, or does it merely assert its own completeness?
# The candidate under review

Worktree: `<WORKTREE>`,
branch `work/bf3-implementation`. Base (accepted predecessor tip): `040084bf0`.
Review the full range `040084bf0..HEAD`.

This is a conformance-audit block in an in-flight architecture-lock program. Its charter is
`docs/arch/refactor/rev11/charters/BF3.md` (49 lines — READ IT IN FULL; it is the contract you
are checking against). Two documents in the candidate itself are load-bearing:

- `docs/arch/refactor/rev11/evidence/BF3/scope-memo.md` — a recorded deviation: the charter's
  steps 3–5 mandate a PRODUCTION mechanism (pre-publication detection, typed non-success,
  whole-capability-cell retraction). An independent architecture consult ruled that mechanism
  architecturally wrong and reshaped the block into conformance-exhaustion plus
  correction-dispatch, adding NO production mechanism.
- `docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md` — that consult's verbatim
  ruling.

You are NOT asked to rubber-stamp that deviation. If you think the deviation is wrong, or is
not properly recorded, or exceeds what a track-level deviation may decide, say so as a
finding.

# How to report — this format is mandatory

**Enumerate EVERY numbered exit criterion and owned-scope bullet from `BF3.md`, and for each
one, cite the specific evidence — file, line, test name, exact command output — that proves it
satisfied.** A criterion with no cited evidence is a BLOCKING finding by default, never an
assumed pass.

A green test NAME is never sufficient. For each criterion, explain why the assertion covers
every word of the criterion, **including its quantifiers** — "every", "all", "only", "exact",
"exhausted", "no". A test that covers 3 of 5 cases does not satisfy a criterion saying "every".

There is no "PASS with caveat". Use exactly these verdicts per item:

- `PASS` — you cite evidence that discharges the whole criterion, quantifiers included.
- `NOT_PROVEN` — the evidence is missing, partial, or does not discriminate. Not a pass.
- `BLOCKING` — behaviour or structure actually violates the criterion.

End with ONE overall verdict from the same three values, and a prioritised finding list. Your
overall verdict is the worst individual verdict; do not average.

The criteria to enumerate, at minimum (derive the exact wording from the charter yourself —
this list is an index, not a substitute):

**Required exits**
- E1 `FC-ATOMIC-001` passes for success AND every refusal.
- E2 the full reachable-success inventory within the retained scope is EXHAUSTED — both the
  exact Svelte client inventory and the remaining in-scope product/route inventory.
- E3 every failure has a disposition, a local regression, a named correction owner, and a
  removal/acceptance identifier.
- E4 cold-path tests prove unaffected cells retain behaviour.
- E5 the block cannot accept before that exhaustion.

**Owned scope and prohibitions**
- O1 owns NO broad parser, semantic-model, lowering, helper, hydration, SSR, mapping, or
  TypeScript-product correction.
- O2 cannot infer meaning from generated output, and cannot introduce string-scanning as a
  second semantic authority.
- O3 must probe the exact `svelte@5.56.8` client cells; results against `svelte@5.56.3` do not
  satisfy the exit.
- O4 the existing typed Svelte `ServerGenerate` refusal is already a non-successful cell and
  receives no new production mechanism.

**Abort/rescope conditions** — were either triggered, and if so was the block's response
correct?
- A1 stop if typed information cannot discriminate the bad subset.
- A2 stop if a proposed mechanism requires broad backend repair or would publish a partial
  artifact.

**Repository rules the candidate must also satisfy** (from `CLAUDE.md`; check, do not assume):
- ZERO production behaviour change, per the recorded deviation. Verify with
  `git diff 040084bf0..HEAD --stat` and by inspecting anything outside a `#[cfg(test)]` module.
- No stubs: no empty test bodies, no unconditional default returns presented as
  implementation, no always-true assertions, no non-discriminating characterization test.
  For every committed assertion ask: would this catch the defect it exists to catch?
- No program vocabulary in source, comments, test names, file names, or commit messages —
  no block IDs, revision names, charter/amendment references, phase/round/cutover words.
  Check commit BODIES, not just subjects: `git log --format=%B 040084bf0..HEAD`.
- Landed guards are structural, never name-keyed source-tree scanners.
- Testing hermeticity; the one-`tests/main.rs`-per-crate integration-test layout;
  cross-platform path portability.
- No machine-specific absolute paths (`/Users/...`) committed outside the known guard-file
  exemption.

Be terse. Evidence and verdicts, not narrative.

---

## Dispatch: `bf3-review-architecture.md`

# HARD EFFICIENCY CONTRACT — READ FIRST

ONE turn. Never `cat`/`nl -ba` a file over 120 lines — use `sed -n 'A,Bp'` or `rg -n -C 5`.
Budget your commands; when in doubt, stop investigating and write the report. An unfinished
review is a FAILED dispatch. The report is the deliverable; do not emit a plan.

# Mandate: ARCHITECTURE

You are the architecture reviewer. Your question: **is authority, ownership, dependency
direction, lifetime, platform behaviour, public boundary, determinism, and conceptual
complexity correct — and does this candidate leave the codebase in a state a later block can
build on without unwinding it?**

Judge the DESIGN, not the checklist. Specific things to decide here:

- The candidate records a deviation from its charter's steps 3–5 (it adds NO production
  guard/refusal/withholding mechanism, on the authority of an independent consult recorded in
  its own evidence). Is that deviation architecturally correct? Is it recorded at the right
  altitude, or does it decide something only a formal amendment may decide? Say plainly if the
  candidate has exceeded what a track-level deviation may settle.
- Test-side characterization of known-wrong behaviour is explicitly permitted in this
  repository; production-side tracking is forbidden. Does anything the candidate landed cross
  that line — is any pinned value, inventory, or committed artifact READ BY, or capable of
  influencing, a production path? Trace it, do not assume.
- The candidate adds a conformance gate that drives a Node CLI from a Rust test, provisions
  framework type environments, and builds native/wasm artifacts. Is that machinery in the right
  crate, at the right visibility, behind the right feature gate, with the right dependency
  direction? Does it create a second authority for anything the shared substrate already owns?
- Several existing items were widened from private to `pub(super)` to enable reuse. Is that the
  right call, or does it leak a surface that should have stayed closed?
- Committed inventory artifacts (JSON) — are they a durable design or a maintenance liability?
  Is the completeness mechanism structural (type system, exhaustive match) or a name-keyed
  source scanner? This repository forbids landing the latter as a guard.
- Determinism and platform: does anything the candidate landed depend on machine paths, host
  ordering, a TypeScript checker-instance id, wall-clock, or a POSIX-only assumption?
# The candidate under review

Worktree: `<WORKTREE>`,
branch `work/bf3-implementation`. Base (accepted predecessor tip): `040084bf0`.
Review the full range `040084bf0..HEAD`.

This is a conformance-audit block in an in-flight architecture-lock program. Its charter is
`docs/arch/refactor/rev11/charters/BF3.md` (49 lines — READ IT IN FULL; it is the contract you
are checking against). Two documents in the candidate itself are load-bearing:

- `docs/arch/refactor/rev11/evidence/BF3/scope-memo.md` — a recorded deviation: the charter's
  steps 3–5 mandate a PRODUCTION mechanism (pre-publication detection, typed non-success,
  whole-capability-cell retraction). An independent architecture consult ruled that mechanism
  architecturally wrong and reshaped the block into conformance-exhaustion plus
  correction-dispatch, adding NO production mechanism.
- `docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md` — that consult's verbatim
  ruling.

You are NOT asked to rubber-stamp that deviation. If you think the deviation is wrong, or is
not properly recorded, or exceeds what a track-level deviation may decide, say so as a
finding.

# How to report — this format is mandatory

**Enumerate EVERY numbered exit criterion and owned-scope bullet from `BF3.md`, and for each
one, cite the specific evidence — file, line, test name, exact command output — that proves it
satisfied.** A criterion with no cited evidence is a BLOCKING finding by default, never an
assumed pass.

A green test NAME is never sufficient. For each criterion, explain why the assertion covers
every word of the criterion, **including its quantifiers** — "every", "all", "only", "exact",
"exhausted", "no". A test that covers 3 of 5 cases does not satisfy a criterion saying "every".

There is no "PASS with caveat". Use exactly these verdicts per item:

- `PASS` — you cite evidence that discharges the whole criterion, quantifiers included.
- `NOT_PROVEN` — the evidence is missing, partial, or does not discriminate. Not a pass.
- `BLOCKING` — behaviour or structure actually violates the criterion.

End with ONE overall verdict from the same three values, and a prioritised finding list. Your
overall verdict is the worst individual verdict; do not average.

The criteria to enumerate, at minimum (derive the exact wording from the charter yourself —
this list is an index, not a substitute):

**Required exits**
- E1 `FC-ATOMIC-001` passes for success AND every refusal.
- E2 the full reachable-success inventory within the retained scope is EXHAUSTED — both the
  exact Svelte client inventory and the remaining in-scope product/route inventory.
- E3 every failure has a disposition, a local regression, a named correction owner, and a
  removal/acceptance identifier.
- E4 cold-path tests prove unaffected cells retain behaviour.
- E5 the block cannot accept before that exhaustion.

**Owned scope and prohibitions**
- O1 owns NO broad parser, semantic-model, lowering, helper, hydration, SSR, mapping, or
  TypeScript-product correction.
- O2 cannot infer meaning from generated output, and cannot introduce string-scanning as a
  second semantic authority.
- O3 must probe the exact `svelte@5.56.8` client cells; results against `svelte@5.56.3` do not
  satisfy the exit.
- O4 the existing typed Svelte `ServerGenerate` refusal is already a non-successful cell and
  receives no new production mechanism.

**Abort/rescope conditions** — were either triggered, and if so was the block's response
correct?
- A1 stop if typed information cannot discriminate the bad subset.
- A2 stop if a proposed mechanism requires broad backend repair or would publish a partial
  artifact.

**Repository rules the candidate must also satisfy** (from `CLAUDE.md`; check, do not assume):
- ZERO production behaviour change, per the recorded deviation. Verify with
  `git diff 040084bf0..HEAD --stat` and by inspecting anything outside a `#[cfg(test)]` module.
- No stubs: no empty test bodies, no unconditional default returns presented as
  implementation, no always-true assertions, no non-discriminating characterization test.
  For every committed assertion ask: would this catch the defect it exists to catch?
- No program vocabulary in source, comments, test names, file names, or commit messages —
  no block IDs, revision names, charter/amendment references, phase/round/cutover words.
  Check commit BODIES, not just subjects: `git log --format=%B 040084bf0..HEAD`.
- Landed guards are structural, never name-keyed source-tree scanners.
- Testing hermeticity; the one-`tests/main.rs`-per-crate integration-test layout;
  cross-platform path portability.
- No machine-specific absolute paths (`/Users/...`) committed outside the known guard-file
  exemption.

Be terse. Evidence and verdicts, not narrative.

---

## Dispatch: `bf3-review-adversarial.md`

# POSTURE — READ FIRST AND OBEY

**Your default verdict is BLOCK.** You may only reach a non-blocking verdict for an item where
you have personally executed something that would have failed had the item been wrong. Absence
of a visible problem is NOT evidence of correctness; it is an unexamined item, which is
`NOT_PROVEN`, which is not a pass. Do not be agreeable. Do not give credit for effort, volume,
thorough-looking reports, or confident prose. A candidate that documents its own gaps honestly
still fails any criterion those gaps leave undischarged.

You will be tempted to accept this candidate because its author's reports are detailed and
self-critical. Detailed self-criticism is not verification. Verify.

# EFFICIENCY CONTRACT

ONE turn. Never `cat`/`nl -ba` a file over 120 lines — use `sed -n 'A,Bp'` or `rg -n -C 5`.
Budget your commands; when in doubt, stop investigating and write the report. An unfinished
review is a FAILED dispatch. The report is the deliverable; do not emit a plan.

# Mandate: ADVERSARIAL

Your question: **can I break this, and would the suite notice?**

## Required: plant-red-green on the test suite itself

This is mandatory and is not deferrable to a later round. Pick the load-bearing mechanisms —
the conformance gate, the reachability/inventory derivation, the mapping-anchor check, the
TypeScript observation domain's fail-closed module resolution, the atomic-publication
assertions, the export-enumeration completeness tests — and for EACH:

1. plant a defect, or bypass the real mechanism, in the code under test;
2. **prove the plant actually applied** — `perl`/`sed`/`grep` all exit 0 on a non-match, so an
   exit code is never proof. Show the marker is ABSENT before (count 0) and present EXACTLY
   ONCE after, and that the file bytes changed;
3. prove the designated test goes RED, and that it goes red for YOUR reason and not an
   unrelated one;
4. revert, and prove GREEN again and the marker count back to 0.

**A green planted run means the plant failed, until you have proven otherwise.** Report every
plant that did NOT produce red — those are the findings that matter most.

## Required: fresh black-box probes you author yourself

Do not reuse the candidate's fixtures or its framing. Author new inputs targeting the
defect-prone families:

- **drift** — does a stale artifact, a changed pin, or a moved fixture go undetected?
- **missing provenance** — can a result be published with no source attribution and pass?
- **erasure** — can a product silently vanish and a test still go green?
- **comment loss** — are tool-consumed comments preserved where the project's conformance rule
  says they must be?
- **partial-failure atomicity** — can a refusal leak one artifact while withholding the rest?
- **field-level comparison gaps** — does any comparator compare a summary, a length, or a digest
  where it should compare fields, so a wrong field of the right shape passes?

Store your probes by digest and report the digests; do NOT commit your probe code into the
candidate.

## Also decide

- Is any test in the candidate a stub, always-true, or non-discriminating?
- Does any claimed "proof of application" actually prove application?
- Could any test pass against a DIFFERENT framework pin than the one it claims?
- Is any recorded UNPROVEN actually a gap dressed as an honest unknown? The standard: an honest
  UNPROVEN identifies the exact claim, the missing discriminating observation, why existing
  evidence cannot decide it, an owner, and a falsifiable closure condition. Anything less is a
  gap, and a gap is BLOCKING.
# The candidate under review

Worktree: `<WORKTREE>`,
branch `work/bf3-implementation`. Base (accepted predecessor tip): `040084bf0`.
Review the full range `040084bf0..HEAD`.

This is a conformance-audit block in an in-flight architecture-lock program. Its charter is
`docs/arch/refactor/rev11/charters/BF3.md` (49 lines — READ IT IN FULL; it is the contract you
are checking against). Two documents in the candidate itself are load-bearing:

- `docs/arch/refactor/rev11/evidence/BF3/scope-memo.md` — a recorded deviation: the charter's
  steps 3–5 mandate a PRODUCTION mechanism (pre-publication detection, typed non-success,
  whole-capability-cell retraction). An independent architecture consult ruled that mechanism
  architecturally wrong and reshaped the block into conformance-exhaustion plus
  correction-dispatch, adding NO production mechanism.
- `docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md` — that consult's verbatim
  ruling.

You are NOT asked to rubber-stamp that deviation. If you think the deviation is wrong, or is
not properly recorded, or exceeds what a track-level deviation may decide, say so as a
finding.

# How to report — this format is mandatory

**Enumerate EVERY numbered exit criterion and owned-scope bullet from `BF3.md`, and for each
one, cite the specific evidence — file, line, test name, exact command output — that proves it
satisfied.** A criterion with no cited evidence is a BLOCKING finding by default, never an
assumed pass.

A green test NAME is never sufficient. For each criterion, explain why the assertion covers
every word of the criterion, **including its quantifiers** — "every", "all", "only", "exact",
"exhausted", "no". A test that covers 3 of 5 cases does not satisfy a criterion saying "every".

There is no "PASS with caveat". Use exactly these verdicts per item:

- `PASS` — you cite evidence that discharges the whole criterion, quantifiers included.
- `NOT_PROVEN` — the evidence is missing, partial, or does not discriminate. Not a pass.
- `BLOCKING` — behaviour or structure actually violates the criterion.

End with ONE overall verdict from the same three values, and a prioritised finding list. Your
overall verdict is the worst individual verdict; do not average.

The criteria to enumerate, at minimum (derive the exact wording from the charter yourself —
this list is an index, not a substitute):

**Required exits**
- E1 `FC-ATOMIC-001` passes for success AND every refusal.
- E2 the full reachable-success inventory within the retained scope is EXHAUSTED — both the
  exact Svelte client inventory and the remaining in-scope product/route inventory.
- E3 every failure has a disposition, a local regression, a named correction owner, and a
  removal/acceptance identifier.
- E4 cold-path tests prove unaffected cells retain behaviour.
- E5 the block cannot accept before that exhaustion.

**Owned scope and prohibitions**
- O1 owns NO broad parser, semantic-model, lowering, helper, hydration, SSR, mapping, or
  TypeScript-product correction.
- O2 cannot infer meaning from generated output, and cannot introduce string-scanning as a
  second semantic authority.
- O3 must probe the exact `svelte@5.56.8` client cells; results against `svelte@5.56.3` do not
  satisfy the exit.
- O4 the existing typed Svelte `ServerGenerate` refusal is already a non-successful cell and
  receives no new production mechanism.

**Abort/rescope conditions** — were either triggered, and if so was the block's response
correct?
- A1 stop if typed information cannot discriminate the bad subset.
- A2 stop if a proposed mechanism requires broad backend repair or would publish a partial
  artifact.

**Repository rules the candidate must also satisfy** (from `CLAUDE.md`; check, do not assume):
- ZERO production behaviour change, per the recorded deviation. Verify with
  `git diff 040084bf0..HEAD --stat` and by inspecting anything outside a `#[cfg(test)]` module.
- No stubs: no empty test bodies, no unconditional default returns presented as
  implementation, no always-true assertions, no non-discriminating characterization test.
  For every committed assertion ask: would this catch the defect it exists to catch?
- No program vocabulary in source, comments, test names, file names, or commit messages —
  no block IDs, revision names, charter/amendment references, phase/round/cutover words.
  Check commit BODIES, not just subjects: `git log --format=%B 040084bf0..HEAD`.
- Landed guards are structural, never name-keyed source-tree scanners.
- Testing hermeticity; the one-`tests/main.rs`-per-crate integration-test layout;
  cross-platform path portability.
- No machine-specific absolute paths (`/Users/...`) committed outside the known guard-file
  exemption.

Be terse. Evidence and verdicts, not narrative.

---

## Dispatch: `bf3-review-targeted.md`

# HARD EFFICIENCY CONTRACT — READ FIRST

ONE turn. Never `cat`/`nl -ba` a file over 120 lines — use `sed -n 'A,Bp'` or `rg -n -C 5`.
Budget your commands; when in doubt, stop investigating and write the report. An unfinished
review is a FAILED dispatch. The report is the deliverable; do not emit a plan.

# TARGETED review of a fix delta — two questions only

This is deliberately NOT a fresh hunt across the branch. Two independent review rounds already
ran and returned findings; those findings were then fixed. You are reviewing **only the fix
delta**, and you must answer exactly two questions:

1. **Did each fix do what it claims?**
2. **Does any fix introduce a defect?**

Anything outside the fix delta is out of scope for this review, however tempting. If you find
something outside it, name it in one line at the end under "outside the delta, recorded" and
move on — do not investigate it.

Worktree: `<WORKTREE>`.
**The fix delta is `34651db22..HEAD`.** Everything before `34651db22` is prior, already-reviewed
work.

Your default posture is BLOCK: a fix is proven only if you personally executed something that
would have failed had the fix not worked. "The code looks right" is `NOT_PROVEN`.

## The claimed fixes, in order — verify each

Round 1 fixes (`d74130fa3`..`b09a114df`):

- **F1** all six committed Svelte client cells (and all six server cells) are now individually
  driven and recorded, with reachability demoted from a filter to an attribution label.
- **F2** every generated-output string-scan removed: the `{#each}` flags argument is now read by
  parsing the emitted module with `oxc_parser` and resolving the runtime namespace import
  binding; batch carrier identity now comes from the host's own `file_language` adapter id.
- **F3** batch characterizations now assert the positive (byte equality against the single-file
  route) as well as the absence, so arbitrarily different output fails.
- **F4** the refusal inventory is derived from an exhaustive `match` over the compiler's typed
  unsupported-surface enum, so a new variant is a compile error until classified.
- **F5** `compile_with_audit_options`, `ensureIdeCompiled`/`getIde` on both transports, and the
  bundler `transform` hook are now executed rather than cited.
- **F6** the transport suite moved from `#[ignore]` to a `transport-authoritative` cargo feature.
- **F7** `pathToFileURL` instead of string-concatenated `file://`; normalized path separators.
- **F8** program vocabulary removed from source comments.
- **F9** a committed per-cell record, held against the live suite by a test.

Round 2 fixes (`e93af0900`..`HEAD`):

- **G1** a test that claimed to exercise a `source_map` axis but asserted only the request now
  asserts the PRODUCT; 6 input-comparing assertions rewritten to compare outputs.
- **G2** 9 generated-output string scans removed (the count claimed).
- **G3** the TypeScript observation's fail-closed module-resolution gate now delegates to
  TypeScript's own `preProcessFile` and gates both imported files and type-reference directives,
  claimed to close a `require()` bypass and three other forms.
- **G4** the committed cell record moved out of a program-named path into
  `crates/verter_session/src/`.
- **G6** the pinned official domain is now derived from the digest-verified goldens and asserted
  everywhere a cell is observed.
- **G7** a Svelte CLIENT runtime axis now exists and mounts compiled modules against the pinned
  install.
- **G8** cold-path tests now pin exact bytes instead of `len > 0`.
- **G9** `compileWithAudit` is now driven on both transports; the classification lists became a
  partition.
- **G10** batch per-entry atomicity was re-measured and the earlier "product beside a refusal"
  reading was withdrawn as an artifact of a Vue-shaped success.
- **G11** disclosed that three earlier test-filter invocations matched ZERO tests and exited 0.
- **G12** a disposition ledger landed in the evidence tree.

## What to actually do

Pick the fixes whose failure would matter most and TEST them, rather than reading all of them.
At minimum:

- Re-run the plant that the previous reviewer found stayed green (forcing the audited entry's
  `source_map` axis on in production) and confirm the fixed test now goes RED. Prove the plant
  applied — absent before, present exactly once after — then revert and prove green.
- Attack the completeness partition: drop one spelling from an executed list and confirm red.
- Attack the module-resolution gate with a reference form the fix claims to cover, and one it
  might not.
- Check that the exact-pin assertions really would fail against a different pin.
- Confirm the round-2 claim that the batch is atomic per genuine failure, rather than accepting
  the withdrawal at face value.
- Check that no fix introduced a NEW non-discriminating assertion, a new string-scan, a new
  program-vocabulary path, or a new machine path.
- Verify every test filter you run reports a non-zero executed count; a filter matching zero
  tests exiting 0 is one of the defects this delta was fixing.

## Report

For each of F1–F9 and G1–G12: `PASS` (you executed something that proves it), `NOT_PROVEN`
(you did not, or the evidence does not discriminate), or `BLOCKING` (it does not do what it
claims, or it introduced a defect). Cite the command or `file:line` for each.

Then: does the delta introduce any defect? List them.

End with ONE overall verdict — `PASS`, `NOT_PROVEN`, or `BLOCKING` — equal to the worst
individual verdict, plus a one-line "outside the delta, recorded" list.
