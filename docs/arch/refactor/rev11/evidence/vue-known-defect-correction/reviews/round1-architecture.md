# BV0 — Architecture Review

**Candidate:** `c40a1ca96` (`fix(core): correct Vue VDOM/Vapor/SSR conformance defects against the rc.3 oracle`)
**Base:** `b64358705`
**Charter:** `docs/arch/refactor/rev11/charters/BV0.md`
**Scope of this review:** owned-scope adherence, charter prohibitions, and the global CLAUDE.md architecture rules (Two Template Codegen Paths, CodeTransform SoT, Compiled-Output Conformance, phase archaeology, owner-crate placement).

---

## 1. What the candidate actually changes

Source-only footprint (excluding regenerated corpora/goldens):

| Area | Files | Net |
|---|---|---|
| `verter_compiler/src` (Vue script + template codegen) | 35 | ~+6.7k / −3.0k |
| `verter_vue_conformance/tests` | 9 (2 new suites) | ~+1.1k |
| `packages/framework-conformance-harness` (src/bin/test) | 6 (4 new) | ~+1.1k |
| Vue goldens `3.6.0-rc.1` → `3.6.0-rc.3` | 95 `.js` renamed (11 with content change, **all under `vapor/`**) | — |
| **Svelte oracle corpus + pins `5.56.3` → `5.56.8`** | **~2027 corpus files + 3 production `src` constants + 10 `package.json` + lockfile** | — |

The largest single structural move is a rewrite of the Vapor backend from a walker-driven emitter into a two-pass **plan → emit** design, landed as a new `template/code_gen/vapor/block_plan/` module (2279 lines across `mod.rs`/`plan.rs`/`emit.rs`/`tests.rs`), with `vapor/comment.rs` and `vapor/text.rs` deleted.

---

## 2. Charter prohibitions — checked one by one

| Prohibition | Verdict | Evidence |
|---|---|---|
| B3's canonical request | **Clean** | No request/envelope type introduced; nothing in the diff touches `verter_protocol`, `verter_session` request surfaces, or a canonical-request abstraction. |
| B4's publication architecture | **Clean** | No publication/registry surface added; the diff stays inside `verter_compiler`'s script/template codegen and the conformance test crates. |
| A new universal IR | **Clean** | `BlockPlan`/`PlanNode`/`BoundaryOp` are **Vapor-private** (`pub(super)` items inside `vapor::block_plan`). `grep` for `block_plan` outside `code_gen/vapor/` returns nothing. It is a backend-local lowering structure, not a cross-backend IR — VDOM, SSR and IDE are untouched by it. |
| Fixture-identity branches | **Clean** | No `fixture == "…"` / `case_id == "…"` control flow in `official_seed_matrix.rs` or `vapor_runtime_behavior.rs`. The only `fixture.contains(&debug)` is an opt-in `VERTER_CONFORMANCE_DEBUG` printer. No `#[ignore]` added anywhere in the diff. |
| Generated-output scanning | **Clean** | No `.replace(`, `Regex`, or post-`build_string()` mutation added to `verter_compiler/src`. Comparison is AST/structural (`compare_modules`), never source-text scanning of generated output. |
| Official-compiler production fallback | **Clean** | The Vue oracle is invoked only from `packages/framework-conformance-harness/{bin,src,test}` and from `verter_vue_conformance/tests`. No production crate references `check-candidate.mjs`, `run-vapor-scenario.mjs`, `execute-vue-vapor.mjs`, or `execute-vapor-runtime.mjs`. |
| A known-divergence allowlist | **VIOLATED in effect** — see Finding 1. The file was not *created* here, but BV0 added two new waiver rows for cells that previously passed. |
| A temporary typed refusal | **Clean** | No `todo!`/`unimplemented!`/new refusal or "unsupported" diagnostic added to `verter_compiler/src`. |
| The complete BV1 official-case pack | **Clean** | The new matrix covers exactly the three BF2 seed fixtures × 3 backends × sourceMap × isProd = 36 cells. No broader official case pack imported. |

---

## 3. Global CLAUDE.md rules

### Two Template Codegen Paths (CRITICAL) — **PASS**
`crates/verter_compiler/src/ide/**` is **entirely untouched** by the diff (`git diff --name-only … -- crates/verter_compiler/src/ide` is empty). The rewrite is confined to `template/code_gen/{vapor,vdom,ssr,shared}`, i.e. the runtime path. The IDE JSX/TSX projection cannot be affected.

### CodeTransform Is the Single Source of Truth (CRITICAL) — **PASS**
The `__expose` work builds its wrapper text into a `String` and hands it to a CodeTransform op (`process.rs:495` → `overwrite_or_root_prefix(setup.tag_open.start, setup.tag_open.end, &wrapper_start)`). No post-`build_string()` splicing, no regex, no string replacement on transformed output anywhere in the added lines. The pre-existing "the body strip skips imports, so this reconstruct is the only edit on the span — no nested-overwrite corruption" invariant is preserved and its comment extended.

### Compiled-Output Conformance (CRITICAL) — **PASS, with one narrowing to ratify (Finding 4)**
- No re-printer, pretty-printer, paren canonicalizer, or cosmetic-mimicry machinery introduced (`grep` for `reprint|prettif|canonicaliz|format_js|pretty_print` on added lines: zero hits).
- `verter_vue_conformance/src/compare.rs` and `src/canon/` are **unchanged** — the structural comparator's semantics are identical before and after, which is what makes the corpus deltas in §4 comparable at all.
- The new harness `compare.mjs` code explicitly *rejects* byte-exact cross-compiler comparison with a documented rationale, which is the correct posture under this rule.
- The `namespaceOverrides` addition to `checkLinkValidity` is correctly bounded: the exact-package-identity check still runs through the real resolver against `baseDir`, so an override cannot mask a wrong or missing installed package.

### No phase archaeology in production code (MANDATORY) — **PASS**
`grep -niE 'phase [0-9]|post-cutover|pre-Phase|d-cutover|cutover|deleted in |retired in |rev11|BV0|BF2'` over `crates/verter_compiler/src`, `crates/verter_vue_conformance/tests`, and the harness `src`/`bin`: **zero hits**. New module docs describe the invariant (static shells / reactive boundaries / insertion-site contract), not the plan that produced them.

### Owner-crate / module placement — **PASS**
- `vapor/block_plan/` is correctly under the Vapor backend, private to it, and *reuses* shared owners rather than forking them: it imports `code_gen::shared::helpers` (`push_u32`, `VaporHelper`, template-declaration writers), `code_gen::types` (`CodeGenOutput`, `VaporElementState`, `VaporTextPart`, `VaporEffect`), and `code_gen::binding::BindingType`.
- `shared/const_source.rs` (v-for source constancy) is correctly placed in `shared/` — it is genuinely consumed by more than one backend and is a pure classification function over binding metadata + parsed OXC data, with no text heuristics.
- `shared/helpers.rs` correctly absorbs the `u32 → u64` helper-flag widening and the `TEMPLATE_FLAG_ROOT|STATIC` bitmask; the delegated-events list stays in `shared/`.
- `ScriptCodeGenOptions::ssr → is_ssr` and the `__vapor` marker gate are in the script owner (`script/mod.rs`, `compile/mod.rs`), with the official-behaviour rationale recorded inline. Correct layer.

---

## 4. Findings

### Finding 1 — BLOCKING. Two previously-passing Vue VDOM routes regressed and were absorbed into the known-divergence allowlist instead of fixed.

`crates/verter_vue_conformance/corpus/known-divergences.json` gains two rows for cells that carried **no** entry at `b64358705` — and an absent entry in that file means the cell **passed** the comparator (the suite fails on any unlisted divergence):

```
+ components/dynamic-multi-root | vdom | non-inline   total=2
+ elements-text/multi-root      | vdom | non-inline   total=2
```

Both rows carry the identical reason pair:

```
[structure] Program[0]/import           — node kind: verter `VariableDeclaration` vs golden `import`
[structure] Program[1]/VariableDeclaration — node kind: verter `import` vs golden `VariableDeclaration`
```

This is Verter-side, not oracle drift. Both goldens are **byte-identical** between rc.1 and rc.3:

```
0	0	goldens/{3.6.0-rc.1 => 3.6.0-rc.3}/vdom/elements-text/multi-root.js
0	0	goldens/{3.6.0-rc.1 => 3.6.0-rc.3}/vdom/components/dynamic-multi-root.js
```

In fact **no `vdom/` or `vdom-inline/` golden `.js` changed content at all** in this commit — all 11 content-changed goldens are under `vapor/`. So the VDOM oracle these two cells compare against is bit-for-bit what it was, the comparator (`src/compare.rs`, `src/canon/`) is unchanged, and the difference is a new module-item ordering divergence Verter introduced.

Why this is blocking rather than cosmetic:

- BV0's **required procedure** ends with "prove unaffected routes retain their prior successful result contract." Two routes that previously produced an in-contract-clean module no longer do.
- BV0's **owned scope** states BV0 "does not introduce … a known-divergence allowlist." BV0 did not create the file, but adding new waiver rows to absorb a defect BV0 itself introduced is functionally the prohibited mechanism, and it converts a red signal into a green one.
- The row's own `note` argues the divergence is benign ("not a signature-shape divergence"), but top-level module-item ordering between an `import` declaration and a lowered `VariableDeclaration` is exactly the "imports, helper families, helper call sequence where order is semantic" category the Compiled-Output Conformance rule keeps **in contract**. It is not on the cosmetic waiver list.

Required: fix the import/module-item lowering order so both cells return to zero divergences and both rows are deleted — or obtain an explicit maintainer ruling that these two rows are ratified, recorded as a `DEFER` with a debt row per CLAUDE.md's Explicit-finding-disposition rule. A self-authored `note` in the allowlist is not a disposition.

### Finding 2 — BLOCKING. Out-of-scope Svelte oracle migration rides in a Vue-only charter.

The candidate bumps the Svelte oracle pin `5.56.3 → 5.56.8` across the monorepo:

- ~2027 regenerated files under `crates/verter_compiler/tests/svelte_oracle_corpus/`;
- three **production** `crates/verter_compiler/src/svelte/runtime/` constants (`SVELTE_ORACLE_VERSION`, the vendored `remove_typescript_nodes.*.js` include path, the `entity_table.rs` provenance header) plus a rewritten handler fingerprint;
- the `remove_typescript_nodes.5.56.3.js → .5.56.8.js` fixture rename and a new `.prettierignore` rule for it;
- 9 `package.json` files and `pnpm-lock.yaml`.

BV0's objective and owned scope are Vue-only ("Correct the genuine **Vue** VDOM, Vapor, SSR, assembly, and mapping defects…"; all four owned-scope items name Vue constructs). Nothing in the charter authorises a Svelte dependency migration. The immediate predecessor commit `fdb6f6291` — *"docs(arch): split immediate Vue defect correction from the Svelte-focused safety retraction"* — is an explicit decision that Svelte work belongs to a **different** block; landing it here reverses that split in the very next commit.

This is also not incidental to the Vue bump: the Svelte pin change is an independent `package.json` edit, not a transitive consequence of `vue 3.6.0-rc.1 → rc.3`.

Practical cost beyond the scope rule: it inflates the candidate to 2426 files, which makes the Vue-side conformance evidence materially harder to audit — the effect is visible in this review, where separating Vue signal from Svelte churn required per-path filtering at every step.

Required: split the Svelte oracle migration into its own commit/block under its own charter, or record a maintainer ruling widening BV0's scope. (No defect was found *in* the Svelte changes themselves — they are mechanical and internally consistent. The finding is scope, not correctness.)

### Finding 3 — BLOCKING. A charter exit criterion is not structurally satisfied: link/runtime axes silently skip on an unprovisioned checkout.

BV0's required exits state: *"The isolated oracle install is present so link checks genuinely execute."*

`packages/framework-conformance-harness/bin/check-candidate.mjs` documents the opposite as the default state:

> *"Skip semantics (hermeticity): link and runtime need the realized oracle install; when the one-time offline provisioning (`node scripts/provision-oracle-npm-cache.mjs`) has not been run, those axes report `skipped` … NEVER a fabricated pass."*

and `official_seed_matrix.rs:366` folds that per-axis skip "through unmodified rather than special-casing it". On a clean checkout where the one-time provisioning has not run, all 36 cells pass with the **exact-package-link** and **deterministic-runtime** axes never executing.

The harness's honesty (skip-with-reason, never a fabricated pass) is the right design and is not the problem. The problem is that the charter's exit asserts the install *is* present, and nothing in the tree enforces or verifies that — which is precisely the failure mode CLAUDE.md's **Verification Must Prove Execution (MANDATORY)** names ("unexpected prerequisite skips were zero"; "required source, build, and fixture prerequisites matched the tested tree").

Required: either make the provisioned oracle install a hard, fail-closed prerequisite of the seed-matrix suite (the pattern `gate.mjs`'s build-prerequisite preflight already establishes — exit 127 with a `BUILD-PREREQUISITE MISSING` marker naming the producer command), or produce fresh evidence from a provisioned run showing zero skipped axes across all 36 cells.

### Finding 4 — Non-blocking, needs ratification. The mapping acceptance axis was narrowed from candidate-vs-official to candidate-self-consistency.

`compare.mjs` adds a well-argued block explaining why byte-exact `mappings`/`sourcesContent` comparison between two independently authored compilers is unsound (independent line-breaking, and the golden generator's `reAnchorMapLines` blank-line padding), and replaces it with a candidate-only check: valid schema, decodable VLQ, every segment resolving to in-bounds source coordinates, expected `sources` entry.

The reasoning is sound and the new check is genuinely discriminating. Two caveats worth a ruling rather than a silent adoption:

- BV0's owned scope item 3 names *"source-map differences **after harness artifacts are removed**"* — i.e. the charter's expectation is *strip the harness artifact, then compare and fix the residue*, not *abandon the comparison*. The commit identifies the harness artifact (`reAnchorMapLines`) but does not normalise it out and resume comparing.
- CLAUDE.md's Compiled-Output Conformance rule lists **"sourcemap mappings"** among the categories that *"remain in contract"* — it is not on the cosmetic waiver list.

This is still a net improvement over the prior state (`seed_conformance.rs` treated source maps as no conformance dimension at all), which is why it is not blocking. But narrowing a charter-named acceptance axis is an `ADOPT-NOW` / `DEFER` disposition decision, not an implementer call.

### Finding 5 — Observation. Pre-existing Vue tracking artifacts remain, against a literal exit criterion.

BV0's required exits state *"No Vue tracking, backlog, waiver, or retraction artifact remains."* Three survive:

1. `corpus/known-divergences.json` — the seed-corpus parity backlog (84 rows). `official_seed_matrix.rs` correctly refuses to reuse it for its own domain and says so in its header, so BV0's own gate is clean; the artifact belongs to the separately-ratified seed corpus.
2. `docs/arch/future/vue-vdom-parity-backlog.md` D6 — referenced from a **production** comment in `script/process.rs`, and that comment was *edited* by this commit (rc.1 → rc.3, scope narrowed to companion imports). This is the one instance BV0 itself touched and left standing.
3. `docs/arch/ssr-noninline-shape-divergence.md` — a ratified interim divergence record, correctly **narrowed** here (signature-count parity achieved; only body routing remains divergent). This one is an improvement.

Reading the exit criterion literally would require all three gone; reading it as scoped to artifacts BV0 would itself produce leaves all three legitimate. Flagged for the maintainer to confirm the intended reading rather than treated as a violation.

---

## 5. Explicitly checked and cleared (so the fix agent does not chase them)

**The corpus divergence-count growth is mostly an artifact of the comparator descending further after a genuine fix, not a regression.** 48 of 85 pre-existing rows show a higher `total` than at `b64358705`, which looks alarming in isolation. Spot-checking the two worst VDOM cases explains it:

- `script-setup/props-type-withdefaults|vdom`: before `["[structure] Program — child count: verter 6 vs golden 5"]`; after, that top-level mismatch is **gone** (BV0 removed the extra host-assembly statement) and the comparator now descends into the setup body, surfacing 9 previously-masked `private binding key` scope-ordinal diffs plus one real `ArrowFunctionExpression` vs `LogicalExpression` node-kind diff.
- `v-on/inline|vdom`: identical pattern — top-level child count now matches, deeper diffs unmasked.

Those cells got structurally **closer** to the oracle. The scope/decl-ordinal rows are the alpha-equivalence class CLAUDE.md keeps structural only because the oracle does not implement scope-aware alpha-equivalence; they cascade from the remaining structural diffs, as the file's own notes state. Likewise, the 17 rows whose totals *shrank* and the 3 `vdom|inline` rows deleted outright are genuine parity wins.

**The vapor `v-on` discriminator rewrite is faithful to the new oracle, not a weakening.** `conformance_discriminator.rs` swaps its planted mutations from `_delegateEvents("click")` / `n0.$evtclick` to `_on(n0, "click", …)` / `"click"→"dblclick"`. Diffing the goldens confirms **official Vue itself** changed between rc.1 and rc.3 — rc.1 emitted `delegateEvents`/`createInvoker`/`$evtclick`, rc.3 emits `_on(...)`. Both replacement plants remain discriminating (`DiffDim::Structure` and `DiffDim::Literal`).

**Other clean checks:** no `#[ignore]` added; no new refusal/`todo!`/`unimplemented!` in production; `verter_vue_conformance/src` untouched (so all corpus deltas are apples-to-apples); the `.prettierignore` addition is correctly justified (byte-verbatim vendored upstream source); `compare.mjs`'s per-PID importer path fixes a real concurrent-truncation race now that the Rust cells run in parallel.

---

## 6. Assessment of the Vapor rewrite (in-scope, no defect found)

The `block_plan` two-pass design is the correct architecture for the charter's item 3 ("invalid Vapor module references/imports, fragment and patch topology"): the previous walker-driven emitter could not express the insertion-site contract the Vapor runtime expects, and the boundary test now runs *before* a child is appended to the shell rather than being inferred from depth. The `IfOp`/`ForOp`/`SlotOp` flag packers (`if_flags`, `for_flags`) derive their bits from the block's own semantic facts (shape, inertness, single-returned-child) rather than from output text, which is the right side of the typed-IR line. Placement, visibility, and shared-owner reuse are all correct.

Its size (2279 lines replacing ~1600) is well beyond "minimum reusable correction" as the charter's procedure phrases it, but the charter also expressly owns "shared lower-owner corrections when the same root cause serves multiple Vue backends," and the deleted `comment.rs`/`text.rs` plus the removed dead `VaporRootElement`/`reset()`/`observe_dom_*` surface show this is a replacement, not an addition alongside a preserved legacy path — consistent with the Legacy Code Deletion rule. No architecture objection.

---

VERDICT: BLOCKING

1. Two previously-passing Vue VDOM cells (`components/dynamic-multi-root|vdom|non-inline`, `elements-text/multi-root|vdom|non-inline`) regressed to a module-item ordering divergence and were absorbed as **new** `known-divergences.json` rows rather than fixed. Both goldens are byte-identical between rc.1 and rc.3 and the comparator is unchanged, so this is Verter-side. Violates BV0's required procedure ("prove unaffected routes retain their prior successful result contract") and leans on the allowlist mechanism the charter's owned scope bars BV0 from introducing. Fix the lowering order and delete both rows, or obtain a recorded maintainer `DEFER` with a debt row.
2. An out-of-scope Svelte oracle migration (`5.56.3 → 5.56.8`: ~2027 corpus files, 3 production `src/svelte/runtime` constants, 9 `package.json` + lockfile) rides in a Vue-only charter, one commit after `fdb6f6291` explicitly split Svelte work into a separate block. Split it out or obtain a recorded scope widening.
3. The charter exit "The isolated oracle install is present so link checks genuinely execute" is not structurally satisfied — `check-candidate.mjs` reports link and runtime axes as `skipped` whenever the one-time `provision-oracle-npm-cache.mjs` provisioning has not run, and `official_seed_matrix.rs` folds that skip through unmodified, so all 36 cells pass on a clean checkout without either axis executing. Make the provisioned install a fail-closed prerequisite, or supply fresh evidence of a provisioned run with zero skipped axes.
