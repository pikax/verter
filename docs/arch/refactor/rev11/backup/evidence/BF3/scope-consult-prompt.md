# HARD EFFICIENCY CONTRACT — READ THIS FIRST

You have ONE turn and a limited context budget. Two previous dispatches of this exact
consult FAILED because the model spent its entire budget printing whole files (`nl -ba`,
`cat`) and never produced a ruling. An unfinished investigation is a FAILED dispatch, not
a partial answer.

Rules for this turn:
- NEVER print a whole file. Use `sed -n 'A,Bp' <file>` on a narrow range, or
  `rg -n 'pattern' <path>` with at most `-C 3`. Never `cat`/`nl -ba` a file over 80 lines.
- Do NOT read `CLAUDE.md`, `.claude/skills/**`, or `docs/arch/refactor/rev11/ORCHESTRATOR.md`
  in full. The quotes you need from them are already reproduced below or are not needed.
- Spend AT MOST ~12 short shell commands on verification, total. Then STOP investigating
  and WRITE THE RULING.
- The RULING is the deliverable. Emit it as your final message, in full prose, with
  file:line citations for the handful of facts you actually verified. Do not emit a plan.
- If you are unsure whether you have budget left, stop investigating and write the ruling.

---

# Architecture consult — determine the correct scope for a compiler-safety block

You are an independent architect. Determine the CORRECT scope for the block described
below, and rule on a live rule conflict inside it. Demand the best architecture. Breaking
changes are allowed and expected if they are right. Total honesty is required: if my
framing is wrong, if the evidence does not support the question I am asking, or if the
whole block should be reshaped or cancelled, say exactly that. "Your premise is wrong" is
a welcome answer. Do NOT pick from a menu — I am deliberately not giving you options.

Repository: this worktree (read-only). Program: an in-flight architecture-lock program
whose normative entry point is `docs/arch/refactor/rev11/ORCHESTRATOR.md`. The block is
`BF3`, charter at `docs/arch/refactor/rev11/charters/BF3.md`. Read the files yourself; the
quotes below are for orientation, not a substitute for reading them.

---

## 1. What BF3's charter mandates today

`docs/arch/refactor/rev11/charters/BF3.md`, "Required procedure per successful cell":

> 1. Run minimum assembled parse, real-package link, and relevant conformance probes.
> 2. Record exact request, route, profile, products, official domain, and failure.
> 3. Detect the affected request before artifact publication using existing typed data.
> 4. Return typed non-success and publish no partial JavaScript, PublicApi, TSC,
>    declaration, CSS, diagnostic map, or source map.
> 5. Retract the whole capability cell when the broken subset is not safely
>    distinguishable.
> 6. Add an independently authored discriminating regression.
> 7. Name BV1 or BS1 as correction owner and bind guard deletion to that acceptance.

And its abort/rescope conditions:

> Stop and retract the complete cell if typed information cannot discriminate the bad
> subset. Stop if a proposed guard requires a broad backend repair or would publish a
> partial artifact.

Steps 3–5 are a PRODUCTION mechanism: a runtime pre-publication detector, a typed
non-success return, and whole-capability-cell withdrawal, each with a removal ID bound to
a later block's acceptance.

## 2. The standing project-wide rule that appears to contradict it

A maintainer rule recorded 2026-08-13T08:06Z, stated as project-wide and explicitly
generalizing beyond the Vue case that prompted it:

> **A wrong output is not an error — it's a bug.** If the compiler/app returns an
> incorrect-but-successful result, that is NOT grounds to add an error path, a typed
> refusal, a runtime guard, or a tracking/backlog artifact consumed by production code.
> Production paths must stay clean of clutter — only the checks actually necessary for the
> product belong there.
>
> The correct response to a known-wrong result is: (1) write a failing test that
> characterizes the bug precisely (TDD); (2) fix the actual bug so the output becomes
> correct; (3) do NOT add production-side machinery (guards, refusals, tracking JSON
> consumed at runtime, known-divergence allowlists that suppress or gate on the bug) as a
> substitute for fixing it — that's clutter, and it doesn't fix anything, it just hides it.
>
> Nuance — test-side characterization is fine and expected. A TEST-side tracking artifact
> (like the existing `known-divergences.json` pattern) is NOT what this rule forbids. The
> rule is about PRODUCTION code paths specifically.

## 3. The amendment that already narrowed BF3, and the ruling it already recorded

`docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md` (RATIFIED,
landed as commit `fdb6f6291`, committed 2026-08-13T09:37+01:00 — i.e. AFTER the rule in §2
was recorded, and with knowledge of it).

§1 encodes the §2 direction, but textually scoped to Vue only:

> For Vue VDOM, Vapor, and SSR findings produced by BF2/BF3 conformance probes, production
> compilation continues returning success and generated Vue output. BF3 must not add a
> typed non-success, publication guard, artifact-withholding path, known-divergence
> allowlist, temporary tracker, or backlog mechanism for those findings. Confirmed defects
> are corrected in the compiler.

§4 keeps the original mechanism for everything else:

> BF3's Vue VDOM/Vapor/SSR runtime-render rows are removed from its retraction and tracking
> scope and assigned to BV0 correction. BF3 retains the original procedure for in-scope
> Svelte and non-Vue-runtime reachable-success cells.

§8.1 records that this exact conflict was already challenged by all three independent
review mandates and ruled on:

> The architecture, conformance, and governance reports … each closed `BLOCKING_FINDINGS`
> (round 1) … on one shared finding: whether the general "fix, don't guard" project rule
> retroactively supersedes BF3's AMD-005-ratified Svelte-domain safety-retraction
> mechanism. A Codex Sol xhigh architecture ruling resolved this
> RETROACTIVE-NO-FORWARD-ONLY: the rule governs BV0's Vue findings and any future findings
> outside BF3's already-ratified retained inventory; it does not repeal BF3's existing,
> already-ratified typed-non-success/whole-cell-retraction/removal-owner mechanism for its
> retained Svelte and non-Vue-runtime scope. No package content change was required.

The live program ledger row for BF3 mirrors that ruling verbatim.

So: the conflict has a ratified answer on the books. I am asking you to determine whether
that answer is CORRECT and what BF3's scope should actually be — not to rubber-stamp it. If
it is right, say so and say why. If it is wrong, say so plainly and say what replaces it.
Consider specifically whether "RETROACTIVE-NO-FORWARD-ONLY" is a principled distinction or
a governance-convenience artifact that preserves a mechanism the maintainer's own standing
rule would reject on the merits, and whether the answer should differ by domain (Vue vs
Svelte) at all when the maintainer has separately recorded that "Svelte is first-class in
Verter alongside Vue — fixes must be carrier-generic/adapter-driven, never Vue-only."

## 4. What BF3's retained inventory concretely is, as best I can establish

Established by direct inspection of this tree (verify, do not trust):

- `packages/framework-conformance-harness/goldens/manifest.json` has 48 golden entries:
  36 `vue/*` (now BV0's domain, already corrected and landed) and 12 `svelte/*`.
- The 12 Svelte goldens are 3 fixtures × {client, server} × {dev0, dev1}:
  `basic-runes` (runes1), `props-events` (runes1), `legacy-slots` (runes0).
- That makes the "exact `svelte@5.56.8` client cells" BF3's charter names exactly **6**
  cells. The 6 Svelte SERVER cells correspond to a Verter backend that already returns a
  typed `ServerGenerate` refusal — per AMD-006 §4 those are already non-successful cells
  and receive no new BF3 mechanism.
- Verter's Vue side has a working per-cell correctness gate in Rust
  (`crates/verter_session/src/compile/map_equality_tests/bf2_full_axis_gate.rs` +
  `bf2_seed_matrix.rs`, feature `bf2-authoritative`) that compiles each fixture through the
  genuine shipped path and drives the harness's `bin/check-candidate.mjs --authoritative`
  across all six axes (parse, link, structural, diagnostics, mapping, runtime). It filters
  `entries` on the `vue/` prefix — **there is no Svelte equivalent**. Nobody has yet run
  Verter's Svelte client output against these 6 goldens. I have not run it either at the
  time of writing this consult.
- The charter's remaining "in-scope product/route inventory" is enumerated in
  `docs/arch/refactor/rev11/evidence/framework-conformance/bf3-safety-retraction-scope.md`
  and covers PublicApi/TSC/declaration, diagnostics/maps/CSS, and NAPI/WASM/host/bundler
  route spellings — for BOTH frameworks in the original text, though AMD-006 removed the
  Vue runtime-render rows specifically.
- Separately: the repo root `package.json` pins `svelte` at `5.56.3`, and there are
  pre-existing Svelte corpora (`crates/verter_svelte_conformance/corpus`,
  `crates/verter_compiler/tests/svelte_oracle_corpus`) built against that older pin, while
  the program's ratified Svelte domain is `5.56.8`. A separate standing project rule says
  to keep exactly one conformance corpus pin per framework, migrate to latest, and retire
  the stale pin. I do not know whether closing that is BF3's, BS1's, or nobody's.

## 5. The precedent that makes me distrust any severe conclusion here

BF3's earlier Vue probe concluded "0/36 cells fail — retract Vue wholesale". A second
independent re-investigation proved that materially overbroad: the 36 cells were ~9 distinct
fixture/backend combinations with repeated axes, one axis's "failures" were a
`sourcesContent` comparison artifact rather than real divergence, and a control test was
trivial. The corrected disposition was eventually not retraction at all but correction
(BV0), which has since landed. So a scope answer here that leans on "the probe will
presumably show X" is worth nothing, and a mechanism that is easy to build but hard to
remove is a real hazard.

## 6. What I need from you

Determine the correct scope for BF3, on the merits, given everything above. In particular I
need a defensible, actionable answer to:

- whether BF3 should build ANY production mechanism at all, and if so, exactly which
  production behaviour is justified, under what predicate, and what deletes it;
- what BF3's obligations are toward the 6 Svelte client cells and the remaining
  product/route inventory if the answer is "no production mechanism";
- how BF3 satisfies its own ratified exit criteria (exhausted inventory, per-failure
  disposition + local regression + named correction owner + removal ID,
  `FC-ATOMIC-001` for success and every refusal, cold-path tests for unaffected cells)
  under whatever scope you rule correct — including whether any of those exits become
  vacuous or need restating;
- whether the correct answer requires a formal amendment / re-ratification rather than a
  deviation memo, and if so what exactly must be amended;
- whether the stale-5.56.3 Svelte corpora belong in BF3's scope or elsewhere.

State your reasoning and cite files/lines you actually read. Be concrete enough that an
implementer could be briefed directly from your answer.

If — and only if — you conclude that a designated-maintainer-only ruling is genuinely
required and that no further architecture consult can resolve it, say so explicitly, name
precisely which decision is maintainer-reserved and why it cannot be resolved on
architectural grounds, and stop there. Do not invoke that escape hatch to avoid a hard call.
