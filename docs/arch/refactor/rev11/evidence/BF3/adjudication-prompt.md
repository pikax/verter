# HARD EFFICIENCY CONTRACT — READ FIRST

ONE turn. Never `cat`/`nl -ba` a file over 80 lines — use `sed -n 'A,Bp'` or `rg -n -C 5`.
At most ~15 short shell commands, then STOP investigating and WRITE THE RULING. An
unfinished investigation is a FAILED dispatch. The ruling is the deliverable; do not emit a
plan. Cite `file:line` for facts you verified.

---

# Three adjudications for a compiler-conformance block

You are an independent architect ruling on three open questions. Demand the best
architecture; breaking changes are allowed. Total honesty — "your premise is wrong" is
welcome. Do not pick from a menu; there is none.

Repository: this worktree, read-only. Program: an in-flight architecture-lock program under
`docs/arch/refactor/rev11/`. The block is `BF3`
(`docs/arch/refactor/rev11/charters/BF3.md`, 49 lines — read it).

## Established context

A prior open-ended consult in this same worktree already ruled that BF3 must build **no**
production guard, typed refusal, artifact-withholding path, retraction table, or runtime
tracking mechanism for incorrect-but-successful output, and is reshaped into a
conformance-exhaustion and correction-dispatch block. That ruling is recorded verbatim at
`docs/arch/refactor/rev11/evidence/BF3/scope-consult-ruling.md`, with the conflict it
resolved at `docs/arch/refactor/rev11/evidence/BF3/scope-memo.md`. Treat that as settled
unless one of the questions below forces you to revisit it — if it does, say so plainly.

BF3's probe has now RUN and produced results. Confirmed genuine defects in its retained
(Svelte and non-Vue-runtime) scope, each independently re-verified:

- **D1** `basic-runes` Svelte client: Verter emits `$.each(ul, 21, …)` where the pinned
  official `svelte@5.56.8` emits `20`. Bit `1` is `EACH_ITEM_REACTIVE`
  (`packages/framework-conformance-harness/.oracle-checkouts/svelte/packages/svelte/src/constants.js`).
  Verter allocates an unnecessary per-item signal — a reactivity/effect-topology divergence.
  Site: `crates/verter_compiler/src/svelte/runtime/client_block_plan.rs:156-167`.
- **D2** `props-events` Svelte client: Verter REFUSES with a typed
  `AdvancedRune { rune: "$props() non-interpolation usage" }`
  (`crates/verter_compiler/src/svelte/runtime/client_surface_script.rs:31-62`). The pinned
  official compiler was independently invoked and ACCEPTS the same fixture.
- **D3** `legacy-slots` Svelte client: the emitted source map covers only the template
  interpolation; script-region declarations carry no provenance, so a required authored-source
  mapping anchor is unsatisfied. Site: the Svelte runtime map builder,
  `crates/verter_compiler/src/svelte/runtime/output.rs:121-210`.
- **D4** an UNTYPED Svelte `$props()` destructure publishes an EMPTY published props surface
  (`import("svelte").Component<{}, {}, "">`) with no diagnostic, while the type-annotated form
  publishes correctly. Site:
  `crates/verter_session/src/framework/api_projectors/svelte.rs:284-286`.
- **D5** the standalone CSS spelling (NAPI `processStyle`, no host route) accepts a
  `sourcemap: true` request and always returns no map — both return sites of
  `verter_compiler::css::process_style` hard-code `source_map: None`
  (`crates/verter_compiler/src/css/mod.rs:110`, `:145`).

BF3's charter states it "owns no broad parser, semantic model, lowering, helper, hydration,
SSR, mapping, or TypeScript-product correction", and its step 7 says to "Name BV1 or BS1 as
correction owner". The DAG (`docs/arch/refactor/rev11/program-dag.toml`) places BS1 AFTER B4,
while B2 and B3 depend on BF3.

## Question 1 — does `FC-ATOMIC-001` hold, given what the probe found?

`FC-ATOMIC-001` is defined as "no partial artifact publication on success or refusal"
(`docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:208`).
BF3's required exits say it "passes for success and every refusal".

The probe established, with red-green proofs: when a Svelte component's RUNTIME surface is
refused, every virtual node is withheld — no JavaScript, no CSS, no source map — but the
IDE/TSX projection and the public-API declaration for that same component ARE still published
(`crates/verter_compiler/src/svelte/carrier.rs:517-530` produces the IDE artifact
unconditionally of the runtime outcome). The implementer characterized this as the current
contract rather than asserting the rule and leaving a red test, and added no withholding path.

Rule on it: is publishing the IDE and public-API products for a component whose runtime
product was refused a violation of `FC-ATOMIC-001`, or are they independent product requests
whose own results are complete and therefore outside it? If it IS a violation, say what must
change and who owns it — bearing in mind the settled ruling forbids BF3 adding a withholding
path. If it is NOT, state the correct reading of `FC-ATOMIC-001`'s scope precisely enough that
a reviewer can check the exit against it without re-litigating.

## Question 2 — is "UNPROVEN" acceptable against an "exhausted inventory" exit?

BF3's exit requires its reachable-success inventory be EXHAUSTED. Two families came back
UNPROVEN rather than pass or fail:

- **PublicApi / TSC / declaration.** The conformance harness ships a real TypeScript
  observation mechanism (`packages/framework-conformance-harness/src/typescript-observe.mjs`)
  but no resolvable `vue` / `svelte` type environment. Every produced declaration's meaning
  lives behind a framework module reference (`import("vue").PublicProps`,
  `import("svelte").Component<…>`), and the in-memory host silently degrades an unresolvable
  `import("…")` to `any` with ZERO diagnostics — so the oracle recorded `any` for BOTH a
  correct and an empty declaration and could not distinguish them. Supplying a faithful type
  environment is building a new oracle, which the implementer was told not to do. Structural
  assertions that do hold (prop presence, declaration-only-ness, cross-spelling byte
  agreement) did land, and D4 above was found by one of them.
- **`compile_many` for Svelte.** Enumerated with its route and transport aliases cited, but
  not driven; the cells it would exercise are the same ones `get_virtual_file` already covers.

Also: transport route identity for NAPI and WASM is read-verified (citations), not executed,
because driving them needs `napi build --release` / `wasm-pack`. And the completeness test
that would catch a NEW NAPI/WASM method name did not land, because the only mechanism that
catches it is a name-keyed source-tree scanner, which `CLAUDE.md` forbids as a landed guard
("Landed guards are structural, never name-keyed file scanners").

Rule: can BF3 satisfy "exhausted" with these recorded as UNPROVEN plus their reasons, or must
it close them first? If it must close them, say exactly what closing looks like and whether
that is inside BF3's ownership at all. If UNPROVEN is acceptable, state the standard a
reviewer applies to distinguish an honest UNPROVEN from a gap dressed as one.

## Question 3 — who owns D1–D5, and what unblocks B2/B3?

BF3 cannot correct any of D1–D5 (its charter disclaims lowering, mapping, and
TypeScript-product correction). Its step 7 names BV1 or BS1. BS1 sits after B4; B2 and B3
depend on BF3 and would therefore proceed while these defects are live and shipping.

The earlier consult observed that safety here comes from refusing to advance the program
rather than from a production mechanism, and floated an immediate pre-B2/B3 Svelte correction
block by analogy with the Vue one that already exists and landed (`BV0`, created by
`docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md` for exactly this
situation on the Vue side).

Rule concretely:

- who owns each of D1–D5 (they are not all Svelte-backend defects — D4 is a session-side
  framework projector, D5 is a CSS route with no host at all);
- whether BF3 can be accepted with these dispositioned-but-uncorrected, or must remain open;
- what exactly must gate B2/B3, and what DAG or amendment change that implies;
- whether the Vue precedent (`AMD-006` creating an immediate correction block ahead of the
  post-B4 owner) is the right template here, or whether something else is.

Be specific enough that a program orchestrator could act on your answer without a further
consult, and name anything that genuinely requires maintainer ratification rather than
architectural determination.
