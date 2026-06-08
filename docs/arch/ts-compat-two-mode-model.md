# TS-Compat Single-Spec / Correction-Overlay Model (LOCKED design)

> Status: **LOCKED.** This document specifies the design the implementation block
> builds. It defines a DATA discipline — the recompute-gated tsgo snapshot, the
> review-gated correction-overlay artifact, the checked-in divergence registry + criteria,
> and the harness data comparison that reads them — that **does not exist in code yet**.
> Nothing here is production Rust/TS, a test, or a guard: it NAMES each planned guard for
> the implementation block.
>
> **One-line thesis.** Verter's type output is **correct by default vs TypeScript, not
> bug-for-bug**. The resolver **ALWAYS** produces the **`Correct`** value. There is **no
> compat mode** and **no spec dimension on any cache key**. Where TypeScript has a genuine,
> conceded type-system bug, the bug-included value is recorded as **DATA** (the tsgo
> snapshot = the `TsCompat` value), and the corrected value is recorded as a **review-gated
> correction overlay** (the `Correct` value). The harness compares the single-spec
> resolver's output against that data; it never runs the resolver in a compat mode.
>
> **Two values, one engine.** This is a **single-spec resolver + a correction overlay** —
> two recorded *values* (snapshot + overlay), not two resolver behaviors. `TsCompat` is
> **harness/overlay DATA**, never produced at production runtime.
>
> **One-engine guarantee — strengthened.** This adds **no second resolver** and **no
> seam/second branch**. The sole query-time resolver remains `SemanticQueryKey →
> ProjectSemanticDispatch::execute → SemanticGraphStore` (CLAUDE.md → "Exactly one
> type-resolution engine"), producing exactly one (`Correct`) value.
>
> **Filename note.** The `two-mode` in this file's name (`ts-compat-two-mode-model.md`) is
> **historical**; the model is **single-spec, two recorded VALUES, ONE engine** — not two
> resolver behaviors. The file is deliberately NOT renamed because guards/tests reference its
> path.

---

## Table of contents

- [1. Context and scope (Decision 0)](#1-context-and-scope-decision-0)
  - [1.1 Scope discipline](#11-scope-discipline-load-bearing)
  - [1.2 Honest scoping of this oracle's divergence set](#12-honest-scoping-of-this-oracles-divergence-set)
- [2. Default direction: one global `Correct` default (Decision 1)](#2-default-direction-one-global-correct-default-decision-1)
  - [2.1 The co-presence invariant](#21-the-co-presence-invariant)
  - [2.2 The surface matrix](#22-the-surface-matrix)
  - [2.3 Why `Correct` global and not compat-by-default (the inversion trap)](#23-why-correct-global-and-not-compat-by-default-the-inversion-trap)
- [3. Oracle / manifest: correction OVERLAY, not two values in the snapshot (Decision 2)](#3-oracle--manifest-correction-overlay-not-two-values-in-the-snapshot-decision-2)
  - [3.1 The correction-overlay artifact](#31-the-correction-overlay-artifact)
  - [3.2 Two explicit, different trust models](#32-two-explicit-different-trust-models)
  - [3.3 Overlay guards](#33-overlay-guards-specified-not-implemented-here)
- [4. Resolver: strictly single-spec, one engine, zero spec dimension (Decision 3)](#4-resolver-strictly-single-spec-one-engine-zero-spec-dimension-decision-3)
- [5. The divergence-site registry (Decision 4)](#5-the-divergence-site-registry-decision-4)
- [6. "What is a TS bug" criteria (Decision 5)](#6-what-is-a-ts-bug-criteria-decision-5)
- [7. Harness contract (Decision 6)](#7-harness-contract-decision-6)
- [8. Guards + the tie chain (Decision 7)](#8-guards--the-tie-chain-decision-7)
- [9. Composition with the paused substrate + §6.3 (Decision 8)](#9-composition-with-the-paused-substrate--63-decision-8)
- [10. Worked examples (Decision 9)](#10-worked-examples-decision-9)
- [11. Honest residuals](#11-honest-residuals)
- [12. Planned-guard index](#12-planned-guard-index)

---

## 1. Context and scope (Decision 0)

Verter is building a full TypeScript-parity native typeinfo engine
(`docs/arch/native-typeinfo-parity.md`) and, as a sibling follow-up, a native
checker over the *same* resolver (`docs/arch/native-checker.md`). The end state is
**replacement** — Verter is the type authority, not a permanent satellite of `tsc`.

That end state forces a stance on a question the parity oracle (`U0`) left implicit:
when Verter's resolver and TypeScript disagree, **which answer is "right"?** The U0
harness assumed "tsgo = the one true answer per row"
(u0 §1 Context — "assert it agrees with **TypeScript-7's**
answer"). That assumption is correct for ~95% of rows and wrong, in a load-bearing way,
for the rest.

**The locked stance.** Verter's type output is **CORRECT BY DEFAULT vs TypeScript, not
bug-for-bug.** Where TypeScript has a genuine, conceded type-system bug, Verter's output
is the *correct* answer, not TypeScript's wrong one. The resolver has exactly one
behavior. The bug-included output is not *produced* by Verter — it is **recorded as data**
(the tsgo snapshot), and the corrected output is recorded as a review-gated correction
overlay. Both answers exist as DATA for the small set of registered divergence rows; the
resolver runs once, in its only (`Correct`) mode.

### 1.1 Scope discipline (LOAD-BEARING)

The value of "correct by default" is destroyed if "correct" becomes a license to
redefine the type system. The discipline is therefore tighter than the feature:

- **DEFAULT to TS-parity for everything.** A divergence is the rare exception, not the
  posture.
- **Diverge ONLY for genuine, documented, clear-cut TS bugs** — ideally TS-issue-backed,
  with the TS team conceding wrongness (the admission criteria are §6).
- **NEVER diverge on TS BY-DESIGN behavior.** Structural typing, variance, literal
  widening/narrowing, contextual typing, excess-property (freshness) checks,
  declaration-merge ordering, distributivity, apparent-type behavior — these are TS
  *semantics*, not bugs. Matching them IS correctness. Diverging from them is a defect in
  Verter, not a "correction."

**Net effect.** ~95% of oracle rows have `correct == tsgo` → they carry **no correction**:
the row asserts `resolver(row) == snapshot.oracle_value` and there is nothing further to
record. Only a small, individually-justified bug-set carries a correction (two recorded
answers: the snapshot's tsgo value and the overlay's correct value).

### 1.2 Honest scoping of *this* oracle's divergence set

This must be stated plainly, because it bounds the size of the problem the
`TypeExpr`-projection oracle actually faces:

Most genuine TypeScript bugs live in the **verdict families** — relation/assignability,
inference, control-flow narrowing — i.e. answers that are a *verdict*, not a structured
type. The U0 harness is a **hover-sourced `TypeExpr`-projection oracle**: it serves only
`TypeExpr`-valued projection rows and explicitly excludes the relation/call/assignability
families (u0 §Scope, the `Relate`-free ≤122-row
ceiling). Therefore:

- The **projection-divergence set** addressable by *this* oracle is a handful of rows
  (conditional/mapped/template reduction, utility-type composition, declaration-merge
  *surface* projection — the projection-representable bug classes).
- The **verdict-family divergence story** is *deferred* to the future structured /
  `relation_verdict` oracle (u0 §Q1 `oracle_value_kind` documents that
  extension point; native-typeinfo-parity §6.3 owns the verdict
  budget). That future oracle **reuses this same DATA PRINCIPLE** — the recompute-gated
  snapshot, the review-gated correction overlay, the divergence registry + criteria, and
  the harness data comparison. **Design once, reuse.** It reuses none of a resolver spec
  mechanism, because there is none: the resolver stays single-spec for verdict families
  exactly as for projection families. The only thing the verdict oracle adds is its own
  recorded answers (its snapshot store + its corrections); it compares its single-spec
  resolver output against that data the same way this oracle does.

---

## 2. Default direction: one global `Correct` default (Decision 1)

**ONE GLOBAL DEFAULT: `Correct`.** Every surface that emits a Verter-resolved type emits
the `Correct` value — because the resolver only produces that value. There is **no surface
that requests, or can request, a `TsCompat` value**: the resolver has no compat mode and
no spec dimension to select. The surface matrix (§2.2) is therefore uniform: every row is
`Correct`.

### 2.1 The co-presence invariant

The co-presence rule below is **analytical, not a resolver switch**: it identifies WHERE a
correct-but-different answer is *user-visible as a contradiction*, so a surface can decide
whether to render an optional explanatory annotation or a side-by-side `DualDisplay`. It
never changes what the resolver produces (always `Correct`).

> **Co-presence rule.** A correct-vs-TS divergence is *user-visible as a contradiction* IF
> AND ONLY IF a surface presents a Verter-RESOLVED type as authoritative for the **same
> symbol** that a **different, co-present TypeScript toolchain** also resolves and surfaces
> to the **same user** (side-by-side, or Verter's resolved type feeds a value the user
> then re-checks with `tsc` expecting byte-identity).
>
> "Authoritative resolved type" **EXCLUDES**: (a) source-literal annotations echoed back
> (inputs the user wrote — they cannot diverge); (b) kind / navigation / Vue-label
> decorations; (c) emitted TSX that `tsc` checks under TS semantics (the resolver does not
> alter the TSX's TS-semantics — see verter-tsc and playground below).

The rule's intent: divergence is only a *usability* problem where two authorities for the
same symbol sit in front of the same user. Everywhere else, correctness is simply correct.
Two responses are available at such a surface, and **neither is a resolver compat value**:

- **An optional explanatory annotation.** A surface may render its `Correct` value
  annotated `"TS reports a known bug — ts#NNNNN"`. This is a cosmetic UX enhancement over
  the `Correct` value; the annotation text comes from recorded registry data, not from a
  second resolver run. **Production-data limit (optional/future).** The correction overlay +
  snapshot are hermetic, review-gated TEST-tree artifacts, so the tsgo *value* lives ONLY in
  the test snapshot and is NOT production-accessible. A PRODUCTION annotation may therefore
  cite only the **compiled-in `DivergenceSite` registry's static `ts_issue`** (static data
  linkable into the surface) — never the tsgo value. Rendering the actual tsc value would
  require a distinct production-accessible data source (or running `tsc`/`tsgo`, the
  `DualDisplay` path below).
- **A `DualDisplay` surface.** A surface that deliberately shows Verter-resolved vs
  tsc-resolved side-by-side obtains the **tsc side by actually running `tsc`/`tsgo`** over
  the symbol — NOT by asking Verter to produce a bug-for-bug value. Verter contributes its
  one `Correct` value; the competing toolchain contributes its own. Today the only such
  surface is the future compare-resolved-types pane (§2.2).

### 2.2 The surface matrix

Each cell below is `Correct`, because the resolver produces only the `Correct` value. The
matrix documents WHY no surface is a co-presence contradiction (or, where it is, how it is
handled without a resolver compat value).

| Surface | Co-present competing TS authority for the same resolved symbol? | Profile | Justification (verified) |
|---|---|---|---|
| **LSP hover — `typeProvider` active** (`auto`/`tsgo`/`tsserver`/`extension`; default `auto`) | No co-PRESENCE contradiction: tsgo/tsserver text IS the authoritative type block | `Correct` (moot) | When a TypeProvider is present, the merged hover's authoritative TYPE TEXT is the provider's. `merge_hover` strips Verter's leading code block and uses the provider's "richer type signature" as the type block (`crates/verter_lsp/src/tsgo/merge.rs:200-215`); Verter contributes only the Vue kind-label decoration (`merge.rs:205-207`, `replace_kind_prefix`) and the source-LITERAL annotation text (`format_binding_hover` → `binding.type_annotation`, `crates/verter_lsp/src/features/hover.rs:1034`), which is text the user wrote and cannot diverge. |
| **LSP hover — child-component pre-merge targets** (all THREE `ChildHoverTarget` variants: `ComponentTag` `<Foo>`, `ImportBinding`, `EventAttribute`) | **No** — this hover returns BEFORE the tsgo merge, so there is no co-present tsgo for the same display | `Correct` (optional UX annotation available) | The pre-merge child-hover surface is the closed `ChildHoverTarget` enum with THREE variants (`crates/verter_lsp/src/features/hover.rs:23-27`), all dispatched through the SAME early return: `handle_hover` calls `child_hover_for_target` and returns its result directly (`crates/verter_lsp/src/server/nav_features.rs:104-108`), and each variant resolves the child's surface through Verter's own `get_public_api` (`crates/verter_lsp/src/server/component_resolve.rs:1037`/`:1056`/`:1077`). Because all three return before the merge, **no co-present tsgo resolves the same symbol** — so by the IFF (§2.1) there is no competing TS authority. (A surface MAY optionally annotate `"TS reports X — ts#NNNNN"` over the `Correct` value as a cosmetic UX enhancement.) |
| **LSP hover — `typeProvider: off`** | **No** — Verter is the sole authority | `Correct` | Under `--type-provider=off` (`crates/verter_lsp/src/main.rs:252-260`) Verter is the sole authority and there is NO co-present `tsc`/`tsgo` resolving the same symbol. The dominant hover field is anyway the source-literal `binding.type_annotation` (`hover.rs:1034`) which cannot diverge. |
| **LSP completions** | No (completion labels are name/kind surfaces, not authoritative resolved-type displays) | `Correct` | Falls under exclusion (b) of the invariant. |
| **LSP diagnostics** | No — the authoritative resolved-TYPE TEXT rests on hover, not diagnostics | `Correct` | Verter diagnostics are Verter LINT verdicts (unknown-prop, missing-slot), not resolved-type strings competing with tsc's resolved type — so there is no "type X vs type Y" contradiction here. (The suppression on the `sync_coordinator` publish path is narrow and partial: `publish_merged_diagnostics` drops only `verter/unknown-prop` / `verter/unknown-model` when a TypeProvider is active, `crates/verter_lsp/src/sync_coordinator.rs:381-388`; the background-publish path does not apply that `retain`, `crates/verter_lsp/src/background_init.rs:404-444`.) |
| **LSP custom methods — `$/verter/getComponentMeta*`** | No competing co-present TS authority | `Correct` | The three custom methods (`crates/verter_lsp/src/server/custom_methods/component_meta.rs:50-74, :84-104, :116-`) return type-bearing component-meta payloads; standalone Verter semantic surfaces with no side-by-side tsc display of the same symbol. |
| **Native component-meta** (`@verter/component-meta`, native path) | No competing co-present TS authority | `Correct` | Standalone semantic authority; `docs/arch/component-meta` native layer owns resolution. |
| **`@verter/typeinfo`** | No | `Correct` | Standalone typed-IR consumer; no side-by-side tsc display of the same symbol. |
| **MCP server** | No | `Correct` | MCP tools surface Verter analysis / resolved type info standalone (`crates/verter_mcp/src/server.rs:2164,2193`). No co-present tsc display of the same symbol. |
| **unplugin** | No | `Correct` | Build-time transform; emits TSX that downstream `tsc` checks under TS semantics (exclusion (c)). |
| **Playground — Verter pane** | No | `Correct` | The Verter pane shows Verter's compiled output (`CompiledFile.types` / `compilerDiagnostics`, `packages/playground/src/core/types.ts`); it checks emitted TSX under TS semantics (`tscCode`), not a competing resolved string. |
| **Playground — future compare-resolved-types pane** | **Yes, by construction** | `Correct` + `DualDisplay` | A pane that deliberately shows Verter-resolved vs tsc-resolved side-by-side is the *canonical* co-presence case. It obtains the **tsc side by running `tsc`/`tsgo`** (§2.1), not by a Verter-produced compat value; Verter contributes its one `Correct` value. Listed so the future surface inherits the rule rather than re-deciding it. |
| **verter-tsc** | No (verdict is tsc's by construction) | `Correct` (moot) | verter-tsc generates `.tsc.tsx` from Verter's `get_public_api` and runs `tsgo`/`tsc` over it as a subprocess (`crates/verter_tsc/src/checker.rs:20`, `:88`, `:209`). The verdict is tsc's by construction; emitted TSX is checked under TS semantics (exclusion (c)). |
| **`@verter/component-meta/compat`** | **No** | `Correct` | This package is a `vue-component-meta` **output-SHAPE** drop-in, NOT a bug-for-bug type-value reproducer: it calls the normal native `getComponentMeta()` and then projects the result into the `vue-component-meta` output shape (`packages/component-meta/package.json:63` exports `./compat`; README — "matches `vue-component-meta` shape while delegating … to native Verter"; `packages/component-meta/src/compat/checker.ts:1884` calls native `getComponentMeta()`). It surfaces no co-present competing `tsc` resolution of the same symbol to the same user, so by the §2.1 IFF its profile is `Correct` — identical to native component-meta. The compat entry-point matches the *shape* of `vue-component-meta`'s API; it does not reproduce TypeScript's bug-included type *values*. |

**No surface is non-`Correct`.** Because the resolver produces only the `Correct` value and
carries no spec selector, the matrix is uniform `Correct`. The only per-surface variation is
cosmetic: an optional explanatory annotation, or a `DualDisplay` pane that obtains its tsc
side by running a real `tsc`/`tsgo`. The matrix is descriptive documentation, not a
declaration registry the resolver consults.

### 2.3 Why `Correct` global and not compat-by-default (the inversion trap)

The endgoal is **replacement** (`docs/arch/native-checker.md` — Verter becomes the
checker over the one resolver). Two consequences pin the default:

1. **The inversion trap.** A compat-by-default resolver would privilege TS bugs as THE
   spec. The day Verter becomes the authoritative checker, every default would have to
   invert — every surface that silently meant "reproduce TS" would have to be re-audited
   and flipped to "be correct." A single-spec `Correct` resolver has **no default to
   invert** and no per-surface compat to flip: there is only correctness, plus a small,
   version-controlled set of recorded corrections that *shrinks* as Verter's correctness
   becomes the reference.

2. **The exercised path is the maintained path.** The single resolver behavior is the path
   that runs on ~95% of rows and every query. There is no second behavior that could rot
   from disuse: the correctness path is the only path, always hot and continuously
   validated by the oracle.

`TsCompat` is therefore never a resolver behavior — it is recorded DATA (the tsgo
snapshot) used only by the harness and by optional UX annotations.

---

## 3. Oracle / manifest: correction OVERLAY, not two values in the snapshot (Decision 2)

The tsgo snapshot stays **exactly** as locked in U0 — it is the **`ts_compat`
oracle** (the recorded `TsCompat` value). This is a *reframing*, not a schema change: the
snapshot's `oracle_value` (u0 §Q1) is, and remains, the machine-captured tsgo answer,
byte-for-byte. **No `correct` field is added inside the snapshot.**

**Why not a `correct` field in the snapshot.** The snapshot's core integrity guarantee is
"regenerate → byte-identical": its filename `snapshot_id` is registry-derivable without
tsgo (u0 §Q1 `snapshot_id`), its `oracle_env_hash` is recomputed offline on read
(u0 §Q1 `oracle_env_hash`), and the whole model is hermetic and offline-re-derivable. The
`correct` value is, by definition, **not** what tsgo emits (tsgo emits the bug). Injecting
a non-tsgo-regenerable value into that artifact would break the "regenerate →
byte-identical" property. The snapshot must stay a pure recompute-gated tsgo capture.

### 3.1 The correction-overlay artifact

A divergence is a **separate, review-gated artifact**, rooted in the hermetic U0 test
tree as a sibling of `oracle_snapshots/`:

```
crates/verter_session/src/typeinfo/typeinfo_tests/oracle_corrections/<family>/<snapshot_id>.correction.json
```

(The exact crate-relative root mirrors the snapshot root
`crates/verter_session/src/typeinfo/typeinfo_tests/oracle_snapshots/<family>/`; the
runtime read path is a hermetic `std::fs::read` of that file — no VCS archaeology, no
network. The `no_orphan_correction` guard, §3.3, recursively enumerates the
`oracle_corrections/` tree.) Each correction is keyed by the same `row_ref` as the
snapshot it overrides. Required fields:

| Field | Meaning |
|---|---|
| `diverges_from_snapshot_id` | The EXACT `snapshot_id` (u0 §Q1 `snapshot_id`) this correction overrides. Resolves to a real, present snapshot. |
| `correct_value` | The human-authored correct answer, in the SAME `TypeExpr::to_json_value()` codec as `oracle_value` (u0 §Q1 `oracle_value`; fact 2, u0 §1.1) — so ONE decoder compares both sides. This is what `resolver(query)` is asserted to equal, per corrected query (§7). |
| `divergence_registry_id` | Mandatory. Resolves to a `DivergenceSite` registry entry (§5). |
| `ts_issue` | The TypeScript GitHub issue id. MUST EQUAL the registry entry's `ts_issue` (cross-checked by Guard D, §8). |
| `criteria_justification_id` | Resolves to the criteria-justification record (§6). MUST EQUAL the registry entry's `criteria_justification_id` (cross-checked by Guard D). |
| `correction_file_id` | A content-addressed id over the **FULL canonical correction object** — every field above (`diverges_from_snapshot_id`, `correct_value`, `divergence_registry_id`, `ts_issue`, `criteria_justification_id`) under canonical-JSON, EXCLUDING only the `correction_file_id` field itself — for tamper-evidence. Uses the same canonical-encoding + hash family rules as the snapshot (u0 §Q1, the snapshot JSON schema + canonical-encoding rules). Strict decode is required (a correction that does not strictly decode + re-hash to its stored `correction_file_id` FAILS), mirroring `strict_snapshot_decode`. |

A divergence is admissible as a divergence row ONLY when BOTH answers exist as DATA — the
`Correct` value (the correction's `correct_value`) AND the recorded `TsCompat` value (the
snapshot's `oracle_value`). A row with **no** correction is a plain agreement row:
`correct ≡ tsgo ≡ snapshot`, asserted per query by `resolver(query) == snapshot.oracle_value`.

### 3.2 Two explicit, different trust models

This is the crux and must be stated without hedging:

- **The snapshot is RECOMPUTE-gated.** Its truth root is machine regeneration: re-run
  tsgo against the frozen vendored corpus → byte-identical bytes. Drift is caught by
  re-derivation (u0 §3 invariant 3).
- **The correction is REVIEW-gated.** It **cannot be regenerated** — tsgo emits the bug,
  so no machine produces the `correct_value`. Its truth root is **adversarial human
  review plus a conceded TypeScript issue** (§6), not machine regeneration.

**Honest, irreducible residual.** The `correct_value` cannot be offline *re-derived*:
there is no oracle for "what TypeScript *should* have done." This is acceptable ONLY
because (a) the set is tiny (§1.2), (b) each entry is individually justified against the
§6 criteria, and (c) the snapshot's recompute-gated guarantees are left completely
untouched — the residual is fenced into the overlay and never contaminates the snapshot's
integrity model.

### 3.3 Overlay guards (specified; not implemented here)

- Every correction's `diverges_from_snapshot_id` resolves to a real, present snapshot
  (`correction_overrides_real_snapshot`).
- **Path identity (`correction_id_is_exact_path`).** `CorrectionId` is the exact
  `(family, snapshot_id)` path identity (§9.2). The correction file STEM (the
  `<snapshot_id>` segment of `oracle_corrections/<family>/<snapshot_id>.correction.json`)
  MUST EQUAL the correction's `diverges_from_snapshot_id`; that snapshot's `row_ref` MUST
  EQUAL the corrected divergence row; and the divergence proof row's REGISTRY-DERIVED
  `snapshot_id` MUST EQUAL `correction.diverges_from_snapshot_id`. (These are the overlay
  half of the four hard equalities Guard D checks — §8.)
- A correction's `correct_value` MUST DIFFER from that snapshot's `oracle_value`
  (`correction_differs_from_snapshot`). A correction equal to the snapshot is a
  **non-divergence** → REJECT (it would assert a divergence while claiming the two answers
  agree — a contradiction that hides a dead registry entry).
- Every `divergence_registry_id` resolves to a registry entry carrying a non-empty
  `ts_issue` and a `criteria_justification_id` (§5, §6), and the correction's `ts_issue` +
  `criteria_justification_id` **EQUAL** the registry entry's (the tamper/consistency
  cross-check; part of Guard D).
- **Tamper-evidence (`correction_file_id_tamper_evident`):** every correction strictly
  decodes and re-hashes the FULL canonical object (all fields except `correction_file_id`)
  to its stored `correction_file_id` (the overlay analogue of `strict_snapshot_decode`); a
  hand-edited `correct_value` / `ts_issue` / registry pointer that no longer re-hashes
  FAILS.
- **No orphan (`no_orphan_correction`):** the on-disk correction set SET-EQUALS the
  registry-derived expected set, keyed at QUERY granularity by
  `(row_file, row_function, query_ordinal, snapshot_id)` — NOT per row. The expected set is
  derived from the divergence proof rows' per-query corrections (each divergence
  `OracleAndGuard` row names one `QueryCorrection` per corrected `query_ordinal`,
  §9.2), recursively enumerated from `oracle_corrections/`. A correction file with no
  corrected query, or a corrected query with no correction file, FAILS — mirroring U0's
  `no_orphan_snapshot` (u0 §Q5). (This set-equality intentionally overlaps
  `every_correction_is_discharged` (§4) — see that guard's belt-and-suspenders note.)

These are the overlay guards listed in §8 and §12.

---

## 4. Resolver: strictly single-spec, one engine, zero spec dimension (Decision 3)

The resolver **always produces the `Correct` value**. **No cache key carries a spec
dimension** — not the graph-memo `FamilyKey`, not the flat component-meta keys
(`ComponentMetaResultDb` / `MaterializeStructureDb` / `ShapeCacheDb`), not `RelateMemoKey`,
not any current or future cache. There is no `SpecVariant` enum, no `spec_variant` field,
no per-query canonicalization, no reachability fact, no `spec.diverge` seam, and no
`requested_spec` carrier. The sole query-time resolver remains `SemanticQueryKey →
ProjectSemanticDispatch::execute → SemanticGraphStore` (CLAUDE.md → "Exactly one
type-resolution engine"), and it has exactly one output per query.

**Why this is sound — it makes the failure CLASS structurally impossible.** The dangerous
class is "a spec-dependent reduced value reaches a spec-blind cache, which then serves the
wrong spec." A design that threaded a spec dimension *through* the caches had to discharge
a **universally-quantified obligation over an OPEN, growing set**: *every* cache in
`ProjectTypeStore` — now and *every future one* — must fold the spec dimension soundly. That
obligation is reopened by every new cache layer. The `ShapeCacheDb` per-member shape cache
is one concrete diagnostic instance of that infinite series: a value reduced under a spec
could leak into a spec-blind per-member slot, and the only fix was to extend the fold into
yet another cache — the same obligation leaking to the next layer. Single-spec dissolves the
class at the root: with **one** spec, **"wrong spec" has no referent** — there is no second
value any cache could mis-route — so no current or future cache can serve the wrong spec.
The obligation collapses from `∀`-over-an-open-cache-set to a **closed absence invariant**
(no spec dimension exists anywhere) plus a **finite `∃`-discharge** over a version-controlled
correction set.

**Why this is also minimal.** The ~95% common case (in fact 100% of production queries)
pays **zero** machinery: no extra key dimension, no canonicalization stage, no reachability
walk, no seam cost. It is strictly cheaper than any spec-threading design.

**Invariants — all preserved or strengthened.**

- **One resolver** (CLAUDE.md → "Exactly one type-resolution engine") — *strengthened*:
  no seam, no second branch, one output per query.
- **Typed-IR-only resolver** (CLAUDE.md → "Typed-IR-Only Resolver Rule") — untouched.
- **R6** (query-identity keys carry no content/version hash, no `fact_dep_signature`) —
  there is no spec dimension to smuggle into any key.
- **R21** (each cache layer keys only on the dimensions it depends on; no bundled hash) —
  no spec dimension is added to any layer, so no layer fragments.
- **No-warm-serve-wrong-spec** — holds *vacuously and in its strongest form*: there is no
  wrong spec to serve.

**The TWO replacement guards (specified; not implemented here).**

1. **`resolver_is_single_spec`** — a **CLOSED absence invariant** enforced by **two
   complementary mechanical mechanisms** (neither relies on a prose "or equivalent" clause)
   plus **one acknowledged human-review link** for the residual fully-novel-named-selector
   case (the SECOND human-review link in this design, peer to §6.4's issue-concession link):

   **(a) Enumerated EXACT-TOKEN scan.** A source-scan over the resolver / cache /
   session production crates asserting **ZERO** hits for the CLOSED, checked-in set of
   EXACT tokens — `SpecVariant`, `spec_variant`, `spec.diverge`, `TsCompat`, `ts_compat`,
   `bug_for_bug`, `compat_profile`. The list is the complete forbidden set; there is NO
   `*_profile` glob and NO "or any equivalent spec-routing token" escape hatch (a glob
   would false-fail the legitimate, non-spec compile/query profile fields `query_profile` /
   `compile_profile` / `tsx_profile`, which are render/query compile profiles, NOT spec
   selectors). Production resolver / cache / session code may not reference the
   oracle-harness / correction-metadata modules at all; only the harness and tests may name
   these tokens (they compare recorded data and never resolve types). Same guard family as
   `no_phase_archaeology_in_production_code` (CLAUDE.md → "No phase archaeology").

   **(b) Structural field-inventory assertion over an EXPLICIT, CLOSED target list.** A
   field-level check complementing (a)'s token scan. It operates over an explicitly
   enumerated, closed list of cache-key / context types — `SemanticQueryKey`, `FamilyKey`,
   `ComponentMetaResultKey`, `MaterializeStructureCacheKey`, `ShapeCacheKey`, the named
   session-/per-key-context structs (`SessionResolverContext`, `InstantiateContext`,
   `MacroPayloadContext`, `ProjectionReductionContext` — a CLOSED enumerated list, not the
   open category "session context structs"), and the `SemanticQueryKeySpec` axes — asserting
   NONE gains a field whose NAME is in the (a) deny-set OR whose TYPE is in a **CLOSED,
   checked-in forbidden-selector-type set**. That forbidden-type set is **NOT a `*Spec`
   glob** — a glob is a semantic/shape test (the very thing this model rejects elsewhere) and
   is ambiguous against the legitimate non-selector key-axes descriptor `SemanticQueryKeySpec`
   — it is an explicit enumeration the registration meta-rule (below) keeps current. An
   explicit **ALLOWLIST** owns the existing legitimate non-spec profile fields —
   `query_profile`, `compile_profile`, `tsx_profile` — so they are accounted for, never
   false-flagged.

   **Exact mechanical reach of (a)+(b).** (a) catches any reintroduction that reuses a
   deny-token ANYWHERE in resolver / cache / session production code — **including as an enum
   VARIANT name**. So `enum SemanticMode { Correct, TsCompat }` and a `compat_profile` axis are
   caught by **(a)** (`TsCompat` and `compat_profile` are deny-tokens); neither "dodges" the
   token list. (b) catches any field, over the closed target list, whose NAME is a deny-token
   OR whose TYPE is in the closed forbidden-selector-type set. Together (a)+(b) mechanically
   catch every reintroduction that reuses a deny-token (incl. variant names) OR adds a
   forbidden-typed / deny-named field.

   **The residual — a fully novel-named selector is NOT caught mechanically; it is caught by
   a human-review link.** A selector that reuses NO deny-token and whose TYPE has not (yet)
   been recorded in the forbidden-selector-type set — e.g. `enum Posture { Strict, Lenient }`
   added to `InstantiateContext` — trips NEITHER (a) NOR (b) at the moment it is introduced.
   The design closes this **honestly**, not by pretending (b) is exhaustive: a **registration
   meta-rule** (same shape as the existing `u2_spec_table` drift guard at
   `crates/verter_session/tests/g_block/u2_spec_table_guards.rs`) requires that any NEW
   `derive(Hash)` type used as a `ProjectTypeStore` cache key / context (i) be registered into
   (b)'s scanned inventory AND (ii) carry an explicit **`is_spec_selector` attestation**
   recorded by the registering reviewer. (b) then flags any field whose type is attested a
   selector. The `is_spec_selector` attestation is a **human judgement** — whether a
   newly-introduced typed dimension is a spec/correction selector cannot be decided by a name
   or type glob — so it is an **acknowledged human-review link**, the **SECOND** such link in
   this design, peer to §6.4's "does the cited TS issue truly concede the bug?" link. (a)+(b)
   are fully mechanical for everything they cover; the novel-named-selector case rests on this
   one attestation, and the doc states that plainly rather than claiming full mechanizability.

   **Why single-spec is preserved under future caches (the load-bearing root).** The
   invariant's true root is the CLOSED resolver-INPUT/dispatch surface: `SemanticQueryKey`
   plus the enumerated dispatch / session context carry NO spec, so no downstream value the
   resolver computes is spec-dependent, so no downstream cache CAN become spec-dependent.
   That is the sense in which single-spec is "trivially preserved" — scoped to this closed
   input surface, NOT to "any new cache" generically. The registration meta-rule (in (b)
   above) keeps (b)'s scan closed as caches grow: a reintroduction that reuses a deny-token
   (incl. a variant name) OR adds a forbidden-typed / deny-named field FAILS the
   `resolver_is_single_spec` build check **mechanically**; a fully novel-named selector is
   caught at registration by the mandatory `is_spec_selector` attestation (the human-review
   link above). This is a strictly stronger posture than the rejected uniform-fold obligation
   (which every new cache reopened regardless of naming) — but it is **NOT** a claim that (b)
   alone catches every novel selector under any name mechanically; that residual is the
   acknowledged attestation link.
2. **`every_correction_is_discharged`** — a harness + data guard asserting the on-disk
   correction set **set-equals** the registry-derived corrected-QUERY set (keyed at QUERY
   granularity by `(row_file, row_function, query_ordinal, snapshot_id)` — NOT per row), and
   that for each corrected query `resolver(query) == correction.correct_value` while that
   query's `snapshot.oracle_value` documents the recorded TS bug. This is the `∃`-discharge
   over the version-controlled correction set: every recorded correction is exercised by a
   live data comparison, and no correction is orphaned. **Intentional overlap
   (belt-and-suspenders):** this set-equality deliberately overlaps `no_orphan_correction`
   (§3.3) — the two have distinct primary jobs (live-resolver discharge here vs
   overlay-integrity hygiene there) and neither is subsumed by, nor should be deleted in
   favor of, the other.

---

## 5. The divergence-site registry (Decision 4)

The registry is **overlay/registry METADATA** — checked-in data that governs which corpus
rows are allowed a correction. It is **not** a resolver code path: there is no seam, no
call site, and no construct-reachability classification reading it. It exists so a
correction cannot be authored without a justified, tied-down registry entry.

A checked-in **static slice**, one entry per registered divergence:

```rust
// illustrative shape — not production code
struct DivergenceSite {
    id: DivergenceId,                       // enum variant; 1:1 with this entry; a registry/correction
                                            //   identity that ties a correction to its justification —
                                            //   NOT a resolver branch selector
    ts_issue: &'static str,                 // REQUIRED, non-empty — the TS GitHub issue id
    concession_class: ConcessionClass,      // closed enum (§6), gates admission
    fixed_in_ts_version: Option<&'static str>, // set when concession_class = FixedInLaterVersion
    expected_correct: &'static str,         // the correct behavior, issue/spec-stated
    criteria_justification_id: &'static str, // resolves to the §6 criteria record
}
```

`DivergenceId` is a **data identity**: a correction's `divergence_registry_id` and a
divergence proof row's `divergence_id` resolve to the same `DivergenceSite`, tying the
recorded correction to its conceded-issue justification. It selects no resolver behavior.

The registry↔correction tie is enforced statically by Guard B (`every_divergence_id_has_registry_entry`)
and Guard C (`every_registry_entry_is_exercised`), and the full FORM of the chain
(registry → criteria, and correction → registry) by Guard D (§8).

---

## 6. "What is a TS bug" criteria (Decision 5)

The decision procedure mirrors U0's **closed positive-allowlist, default-REJECT**
posture. This gate is the firewall against type-system-redefinition creep, so it is
deliberately strict. It governs which corpus rows are allowed a correction overlay.

### 6.1 ADMIT a divergence ONLY IF ALL of:

1. **A TypeScript GitHub issue exists** for the behavior.
2. **The TS team CONCEDES wrongness**, recorded as a value of a **closed**
   `ConcessionClass` enum:
   ```rust
   enum ConcessionClass {
       FixedInLaterVersion, // the GOLD class (§6.3)
       TsTeamBugLabel,      // issue carries the TS team's "Bug" label
       MaintainerConcession,// a TS maintainer explicitly conceded it on the issue
       OnFixMilestone,      // issue is assigned to a fix milestone
   }
   ```
   `"Suggestion"`, `"Working as Intended"`, `"Design Limitation"`, `"By Design"` are
   **NOT** members → such issues cannot admit a divergence.
3. **The construct is representable in the ACTIVE ORACLE VALUE KIND** — for THIS oracle
   the `TypeExpr`-projection kind (`oracle_value_kind == structured_type_expr`), so both
   the correct and tsgo values are carriable as data; verdict-only bugs defer to the
   future structured oracle, §1.2. Stated at this generality so the verdict oracle (which
   reuses this mechanism, §1.2) substitutes its own verdict payload kind (e.g.
   `relation_verdict`) for `TypeExpr` without re-deciding the gate.
4. **The correct behavior is unambiguous and issue/spec-stated** — taken from the issue
   resolution or the spec, **not** proposer-asserted.
5. **A closed `CriteriaJustification` record (§6.5) is present and complete**, resolvable
   by the entry's / correction's `criteria_justification_id`.

### 6.2 REJECT categorically

- TS **by-design** behavior: structural typing, literal widening/narrowing, variance,
  excess-property (freshness) rules, declaration-merge ordering, apparent-type behavior,
  distributivity, contextual typing.
- Anything labelled `"Working as Intended"` / `"Design Limitation"` / `"By Design"`.
- Any divergence with **no** TS issue.
- Any justification phrased as `"surprising"`, `"unintuitive"`, or `"should be"` — these
  are taste, not conceded bugs.

### 6.3 PREFER `FixedInLaterVersion` (the gold class)

When the bug is fixed in a *later* TypeScript than the pinned tsgo, "correct" means
"what a newer TypeScript already does." This is maximally defensible and
forward-compatible: Verter is merely **ahead of the pinned tsgo**, converging toward
where TypeScript itself moved. The registry records `fixed_in_ts_version`. This class
should be the default target for any new divergence proposal; the other classes are for
genuine bugs not yet shipped-fixed.

### 6.4 Process + the honest limit

- **Proposer ≠ sole approver.** A divergence requires review by someone other than its
  author. This is a process rule, not a code guard.
- **Honest limit (stated plainly).** An arch-guard can enforce the **FORM** — a
  non-empty `ts_issue`, a `concession_class` drawn from the closed enum, a present
  `expected_correct`, a resolvable `criteria_justification_id` with a complete closed
  `CriteriaJustification` record (Guard D, §8, §6.5). It **cannot verify that the cited
  issue truly concedes the bug** — tests have no network access, so no guard can fetch and
  read the issue. The trust root for "does the issue really concede it" is **adversarial
  human review**. This is one of the design's TWO acknowledged human-review links (the other
  is the `is_spec_selector` attestation that catches a fully novel-named resolver-input
  selector, §4); within the correction/registry/criteria TIE CHAIN (§8) it is the one
  un-mechanizable link, and is labelled as such everywhere it appears.

### 6.5 The closed `CriteriaJustification` schema

Guard D (§8) must do more than check that a `criteria_justification_id` *resolves* — a
free-text "surprising / should be" rationale could otherwise hide inside a resolvable
record and still pass the tie chain. The justification is therefore a **closed,
mechanically-validated record**:

```rust
// illustrative shape — not production code
struct CriteriaJustification {
    id: &'static str,                          // resolved by criteria_justification_id
    expected_behavior: &'static str,           // the exact correct behavior (issue/spec-stated)
    concession_class: ConcessionClass,         // the closed enum (§6.1) — duplicated for self-containment, cross-checked == registry entry's
    affected_pinned_ts_version: &'static str,  // the pinned tsgo version that exhibits the bug
    fixed_in_ts_version: Option<&'static str>, // evidence the bug is fixed in a later TS (set iff FixedInLaterVersion)
    by_design_negatives: ByDesignChecklist,    // CLOSED enum of by-design categories; EXPLICIT "not this category" mark per member (below)
    snapshot_ref: &'static str,                // MUST EQUAL correction.diverges_from_snapshot_id (equality, not just resolvability)
    repro_ref: &'static str,                   // the corrected row/query: MUST EQUAL the corrected row_ref (equality, not just resolvability)
}

// The CLOSED ENUM of by-design categories a proposer must EXPLICITLY disclaim.
// Every member MUST be explicitly marked `NotThisCategory`. A justification that
// leaves any member unmarked, or marks one `IsThisCategory`, is REJECTED by Guard D
// (it would be a by-design behavior, §6.2) — a free-text "expected_behavior" cannot
// satisfy this; each category is a discrete enumerated mark the guard reads.
struct ByDesignChecklist {
    structural_typing: ByDesignMark,
    literal_widening_narrowing: ByDesignMark,   // "widening"
    variance: ByDesignMark,
    excess_property_freshness: ByDesignMark,     // "excess-property"
    contextual_typing: ByDesignMark,
    distributivity: ByDesignMark,
    declaration_merge_ordering: ByDesignMark,    // "decl-merge-order"
    apparent_type_behavior: ByDesignMark,        // the BROAD category: subsumes apparent-type freshness,
                                                 // primitive-apparent-member resolution, boxed-primitive
                                                 // apparent surfaces, and any other by-design apparent-type
                                                 // semantics. §6.2 rejects "apparent-type behavior"
                                                 // categorically, so the checklist negative covers the whole
                                                 // category.
}
enum ByDesignMark { NotThisCategory, IsThisCategory }
```

**Guard D mechanically REJECTS (exactly what it scans):**

- a missing field; an empty `expected_behavior`;
- a `concession_class` that is not the same closed-enum value the registry entry carries
  (`registry.concession_class == criteria.concession_class` — both real records; the
  correction overlay carries no `concession_class` to duplicate);
- a `FixedInLaterVersion` class with no `fixed_in_ts_version`; a `fixed_in_ts_version`
  PRESENT on a non-`FixedInLaterVersion` class (the field is present **iff** the class is
  the gold class); or a `registry.fixed_in_ts_version != criteria.fixed_in_ts_version` (the
  two records must carry the IDENTICAL version string);
- any one of the eight CLOSED `ByDesignChecklist` members left unmarked or marked
  `IsThisCategory` — the proposer must EXPLICITLY mark "not-this-category" for EACH of
  structural-typing / widening / variance / excess-property / contextual-typing /
  distributivity / decl-merge-order / **apparent-type-behavior** (the BROAD category:
  covers apparent-type freshness, primitive-apparent-member resolution, and any other
  by-design apparent-type semantics — re-audited 1:1 against §6.2's categorical-reject list,
  every reject category has a matching checklist negative);
- **a banned taste-rationale TOKEN anywhere in `expected_behavior` or the justification
  text:** Guard D performs a literal substring scan for `"surprising"`, `"unintuitive"`,
  and `"should be"` and REJECTS on any hit. This is a real mechanical scan (not "the field
  must be issue/spec-stated, therefore the tokens cannot appear" — that would be
  unfalsifiable hand-waving). `expected_behavior` is free text, so the banned tokens ARE
  mechanically rejectable by scanning for them;
- a `snapshot_ref` that does not RESOLVE, or that does not EQUAL
  `correction.diverges_from_snapshot_id`;
- a `repro_ref` that does not RESOLVE, or that does not EQUAL the corrected `row_ref` — so
  a criteria record cannot justify an unrelated correction merely by resolving.

The two load-bearing mechanical firewalls are therefore the **closed `ByDesignChecklist`**
(every by-design category from §6.2 is an enumerated negative the proposer must explicitly
disclaim) and the **banned-token scan** over `expected_behavior`.

**The one un-mechanizable link in the tie chain stays human review:** whether the cited
issue *truly concedes* the bug (§6.4) — no schema field can verify issue CONTENT. (This is
one of the design's two human-review links; the other is the §4 `is_spec_selector`
attestation.)

---

## 7. Harness contract (Decision 6)

The harness compares the **single-spec resolver's** output against recorded DATA. The
resolver runs **once** per query, in its only (`Correct`) mode. There is no second run, no
compat mode, and nothing to re-derive.

### 7.1 A divergence asserts BOTH recorded answers PER CORRECTED QUERY (as data)

A row issues N oracle queries (`query_ordinal` `0..N`, §9.2 / u0 §Q5), and corrections bind
at `(row, query_ordinal)` granularity, so a row may MIX corrected and ordinary queries. For
each corrected `(row, query_ordinal)` — a query with a correction overlay — the harness
asserts both recorded answers, but only one of them comes from running the resolver:

- **The `Correct` side (resolver vs overlay):** `resolver(<that query>) ==
  correction.correct_value`. This is the live assertion — the single-spec resolver must
  produce the corrected answer for that query.
- **The `TsCompat` side (recorded vs recorded):** that query's `snapshot.oracle_value` IS
  the recorded `TsCompat` value (the captured tsgo bug). No resolver run produces it — it is
  data captured at generation time. The harness asserts only that it is present and that it
  DIFFERS from `correction.correct_value` (the `correction_differs_from_snapshot` overlay
  guard, §3.3), so the query genuinely records a divergence.

Every OTHER query in the row (whether or not the row has any corrected query) asserts
`resolver(query) == snapshot.oracle_value` (§7.2). Both values decode through the one
`TypeExpr` codec (§3.1).

### 7.2 Non-divergence (ordinary) queries

An ordinary query with **no** correction is a plain agreement point. The harness asserts a
single fact:

- `resolver(query) == snapshot.oracle_value` — the resolver's one (`Correct`) value equals
  the recorded tsgo value, because at this query `correct == tsgo`.

There is no "mode" distinction, no family-key comparison, and no second run: the resolver is
single-spec, so each query either has a correction (assert against `correction.correct_value`)
or it does not (assert against `snapshot.oracle_value`). The whole harness rule, applied PER
QUERY:

> For each corpus `(row, query_ordinal)`: assert `resolver(query) == correction.correct_value`
> if a correction exists for that `(row_file, row_function, query_ordinal, snapshot_id)`, else
> `resolver(query) == snapshot.oracle_value`.

### 7.3 Cache-order safety is vacuous under single-spec

A cross-mode cache-order hazard (a first request's result poisoning a second request's
warm hit under a different spec) requires two specs to exist. Under single-spec **there is
only one spec**, so **cross-mode warm-hit poisoning has no referent**: no second-spec
request exists that a first request's entry could mis-serve. The cache-order obligation
collapses to nothing — there is one value per query and one cache entry per key, exactly as
for every other resolver query. No dedicated cache-order guard is needed (and a
`spec_variant`-keyed cache-order test must not be introduced — it would scan for a spec
dimension that, by `resolver_is_single_spec`, does not exist).

### 7.4 tsgo stays generation-only

The resolver must NEVER shell to tsgo at query time. tsgo is **generation-only**
(u0 §3 invariants 1–2): it captures the snapshot's `oracle_value` once at build/test time
and is never on the query path. The existing `tsgo_not_reachable_from_resolver` and
`oracle_consumption_path_has_no_tsgo_spawn` guards enforce this and are unchanged.

There is no compat-side resolver obligation here. The earlier model required a compat mode to
"re-derive the exact bug from its own reducers, never shelling to tsgo"; under single-spec
there is no compat mode and no runtime consumer of a bug-for-bug value, so that obligation
does not exist. The recorded `snapshot.oracle_value` already IS the captured bug — no engine
re-produces it.

---

## 8. Guards + the tie chain (Decision 7)

These guards land **with the mechanism block**, not now: each would be vacuous today
(nothing for them to scan). This section NAMES them and describes precisely **how each
discriminates** — i.e. the bad input each FAILS on — so the implementation block has an
unambiguous target.

| Guard | What it checks | How it discriminates (the failing input) |
|---|---|---|
| **`resolver_is_single_spec`** (the closed absence invariant) | TWO complementary MECHANICAL mechanisms (§4) plus ONE acknowledged human-review link: **(a)** an EXACT-token scan over the resolver / cache / session production crates for ZERO hits of the CLOSED set — `SpecVariant`, `spec_variant`, `spec.diverge`, `TsCompat`, `ts_compat`, `bug_for_bug`, `compat_profile` (NO `*_profile` glob, NO "or equivalent" clause — a glob would false-fail the legitimate non-spec `query_profile` / `compile_profile` / `tsx_profile` fields), catching any deny-token reused ANYWHERE incl. as an enum VARIANT name; production code may not reference the oracle-harness / correction-metadata modules at all. **(b)** a STRUCTURAL field-inventory assertion over an EXPLICIT, CLOSED target list — `SemanticQueryKey`, `FamilyKey`, the flat cache keys (`ComponentMetaResultKey` / `MaterializeStructureCacheKey` / `ShapeCacheKey`), the named session-/per-key-context structs (`SessionResolverContext` / `InstantiateContext` / `MacroPayloadContext` / `ProjectionReductionContext`), and `SemanticQueryKeySpec` axes — that none gains a field whose NAME is in the (a) deny-set OR whose TYPE is in a CLOSED, checked-in forbidden-selector-type set (NOT a `*Spec` glob — a glob is a semantic test ambiguous against the legitimate descriptor `SemanticQueryKeySpec`), with an explicit ALLOWLIST owning the legitimate non-spec `query_profile` / `compile_profile` / `tsx_profile` fields. Rooted on the closed resolver-INPUT surface (no spec input ⇒ no spec-dependent downstream value), plus a registration meta-rule: any new `derive(Hash)` `ProjectTypeStore` cache-key / context type MUST register into (b)'s inventory AND the registering reviewer MUST record an explicit `is_spec_selector` attestation for the new type. The only code permitted to NAME the (a) tokens is the oracle-harness / correction-metadata code and tests. Same family as `no_phase_archaeology_in_production_code`. | (a) A reducer / cache-key / session path that introduces any deny-token — INCLUDING as an enum variant name (`enum SemanticMode { Correct, TsCompat }` and a `compat_profile` axis both CONTAIN a deny-token, so both are caught by **(a)**, not (b)) — **fails the build**. (b) A field over the closed target list whose NAME is a deny-token OR whose TYPE is attested an `is_spec_selector` **fails the build**. A FULLY NOVEL-NAMED selector that reuses no deny-token and whose type is not yet attested (e.g. `enum Posture { Strict, Lenient }` on `InstantiateContext`) is NOT caught by (a) or (b) mechanically — it is caught at registration by the mandatory `is_spec_selector` attestation, the design's SECOND acknowledged human-review link (peer to §6.4). (Oracle-harness / correction-metadata mentions pass; they compare data and never resolve types.) |
| **`every_correction_is_discharged`** (the finite ∃-discharge) | A harness + data guard: the on-disk correction set SET-EQUALS the registry-derived corrected-QUERY set (keyed by `(row_file, row_function, query_ordinal, snapshot_id)`), AND for each corrected query `resolver(query) == correction.correct_value` while that query's `snapshot.oracle_value` is present and DIFFERS from it (the recorded TS bug). Its set-equality intentionally OVERLAPS `no_orphan_correction` (§3.3) — belt-and-suspenders, distinct live-discharge vs overlay-integrity jobs; neither is subsumed. | A correction whose `resolver(query)` does not equal its `correct_value` **fails** (the engine did not produce the corrected answer); a correction not exercised by a live corrected query, or a corrected query with no correction, **fails** (set-equality). |
| **Guard B — `every_divergence_id_has_registry_entry`** | Exhaustive enum → registry: every `DivergenceId` variant has a `DivergenceSite` entry. | A `DivergenceId` variant added with no registry entry (a correction-identity with no justification) **fails**. |
| **Guard C — `every_registry_entry_is_exercised`** | Every registry entry is exercised by ≥1 correction artifact tested by the §7.1 data comparison. | A registry entry with no correction artifact (a dead/unjustified entry) **fails** — no entry can sit in the registry without a discriminating correction row behind it. |
| **Guard D — `divergence_tie_chain_is_whole`** | The FORM of the tie chain. **(1) Resolution + registry FORM:** every correction's `divergence_registry_id` → a registry entry; every entry has a non-empty `ts_issue`, a closed-enum `concession_class`, a present `expected_correct`, and a resolvable `criteria_justification_id` whose closed `CriteriaJustification` record (§6.5) is COMPLETE. **(2) `concession_class` cross-check on the REAL records that carry it:** the correction overlay (§3.1) carries NO `concession_class` field — it carries `divergence_registry_id` + `criteria_justification_id`, which RESOLVE to the registry entry and the criteria record, both of which DO carry `concession_class`. So the cross-check is `registry.concession_class == criteria.concession_class` (two real records). The correction↔registry equalities that ARE checked are the fields the overlay actually carries: `correction.ts_issue == registry.ts_issue` and `correction.criteria_justification_id == registry.criteria_justification_id`. **(3) Criteria tied to the correction it justifies (equality, not just resolvability):** `criteria.snapshot_ref == correction.diverges_from_snapshot_id` AND `criteria.repro_ref == the corrected row_ref` — so a criteria record cannot justify an unrelated correction merely by resolving. **(4) Closed-schema check (§6.5):** mechanically rejects a missing field, an unmarked / `IsThisCategory` `ByDesignChecklist` member, a `FixedInLaterVersion` class without `fixed_in_ts_version`, or a banned-rationale token in `expected_behavior`. **(5) `fixed_in_ts_version` tie:** `registry.fixed_in_ts_version == criteria.fixed_in_ts_version` AND `fixed_in_ts_version.is_some()` **iff** `concession_class == FixedInLaterVersion`. **(6) `CorrectionId` ↔ snapshot/query/registry path identity (the four HARD EQUALITIES, §9.2, PER CORRECTED QUERY):** (a) the correction file STEM (= `CorrectionId.snapshot_id`) `== correction.diverges_from_snapshot_id`; (b) that snapshot's `row_ref` (`{ row_file, row_function, query_ordinal }`) `==` the divergence query being corrected (the proof row at the `QueryCorrection`'s `query_ordinal`); (c) the corrected query's REGISTRY-DERIVED `snapshot_id` `== correction.diverges_from_snapshot_id`; (d) `correction.divergence_registry_id == QueryCorrection.divergence_id`. | A correction pointing at a missing registry id; an entry with an empty `ts_issue`, a `concession_class` outside the closed enum, or a missing `expected_correct`; a dangling or incomplete `criteria_justification_id`; an unmarked by-design negative; a `registry.concession_class != criteria.concession_class`; a `registry.fixed_in_ts_version != criteria.fixed_in_ts_version`, or a `fixed_in_ts_version` present without `FixedInLaterVersion` (or absent with it); a `correction.ts_issue` / `correction.criteria_justification_id` that differs from its registry entry's; a `criteria.snapshot_ref != correction.diverges_from_snapshot_id`; a `criteria.repro_ref` that does not equal the corrected `row_ref`; a correction-file stem ≠ its `diverges_from_snapshot_id`; a snapshot whose `row_ref` ≠ the corrected query; a corrected query whose registry-derived `snapshot_id` ≠ `correction.diverges_from_snapshot_id`; or a `correction.divergence_registry_id != QueryCorrection.divergence_id` — all **fail**. (It cannot verify the issue's *content* — §6.4 honest limit.) |
| **overlay guards** (§3.3) | `correction_overrides_real_snapshot` / `correction_id_is_exact_path` / `correction_differs_from_snapshot` / `correction_file_id_tamper_evident` / `no_orphan_correction`. | See §3.3 — a dangling snapshot ref, a path-identity mismatch, a correction equal to its snapshot, a non-re-hashing tamper, or an orphaned correction/corrected-query each **fail**. |
| **§6.3: `registered_divergence_excluded_from_budget`** (§9.4) | A registered divergence (a row with a correction) is EXCLUDED from the differential oracle's per-family defect budget M, because the resolver intentionally diverges there. | A registered divergence charged against M (counted as an accidental defect) **fails**. |
| **reuse (unchanged):** `tsgo_not_reachable_from_resolver`, `oracle_consumption_path_has_no_tsgo_spawn`, `cache_key_axes_are_minimal_and_normalized`, `cache_satisfaction_is_demand_lattice_not_enum_order` | The existing U0 / parity-engine guards: tsgo never on the query path; the static `DemandAxis` minimality mask; the demand-lattice warm-hit/backfill rule. | Unchanged and untouched by this model — there is no spec dimension interacting with them. |

### 8.1 The tie chain, end to end

```
correction (divergence_registry_id → correct_value) ──▶ registry entry (id → ts_issue + criteria + concession_class)
        │                                                       │
        └──▶ resolver(Correct) == correct_value                 └──▶ criteria-justification record
             snapshot.oracle_value == recorded tsgo bug
```

| Link | Mechanical? |
|---|---|
| resolver single-spec (no spec dimension anywhere) — `resolver_is_single_spec` | **Mechanical** for deny-token / forbidden-typed reintroductions (closed absence source-scan); **+ one human-review link** — the `is_spec_selector` attestation a reviewer records when registering a new cache-key/context type, which catches a fully novel-named selector (§4) |
| every correction discharged: `resolver(query) == correct_value` per corrected query, set-equality vs the corrected-query set — `every_correction_is_discharged` | **Mechanical** (live data comparison + set-equality) |
| every `DivergenceId` → registry entry (Guard B); every entry → a correction (Guard C) | **Mechanical** (exhaustive enum + registry⋈correction) |
| correction → registry id → criteria, closed schema, four hard equalities (Guard D) | **Mechanical** (FORM: ids resolve, fields present + complete, cross-equal) |
| overlay integrity (§3.3 five overlay guards) | **Mechanical** (path identity + tamper-evidence + differ + no-orphan) |
| "does the cited TS issue truly concede the bug?" | **NOT mechanical** — adversarial human review only (§6.4) |

The design has **two acknowledged human-review links**, both fenced and labelled as such:
(1) the `is_spec_selector` attestation that catches a fully novel-named resolver-input
selector at registration (§4), and (2) "does the cited TS issue truly concede the bug?"
(§6.4). Every OTHER link in the tie chain is enforced by a discriminating mechanical guard.

---

## 9. Composition with the paused substrate + §6.3 (Decision 8)

This model must land **before** row-lift resumes in the foundational substrate block.

### 9.1 Why land first (concrete rework if deferred)

If rows are lifted with no correction-overlay notion, a row asserts `resolver(row) == tsgo
snapshot` unconditionally and thereby **PINS Verter's output to the TS bug** at a
divergence site — the exact inverse of the "correct by default" directive. Discovering
later that the row is a divergence forces a full **re-lift**: change the proof, author a
correction overlay, add a registry entry. Landing the correction-overlay model first means
a divergence row is born with its correction and never has to be unwound.

### 9.2 Proof-model change

`ProofRequirement` (`crates/verter_session/tests/typeinfo_ignored_test_manifest.rs`,
arms `Ts7Oracle(OracleId)`, `StructuralGuard`, `NegativeGuard`,
`OracleAndGuard { oracle, guard }`, `RowTestGuard`) carries a divergence row as an
**`OracleAndGuard { oracle, guard }`** whose `oracle` half is the `ts_compat` snapshot and
whose `guard` half is the registered `DivergenceCorrection` prover — divergence is one of
the five obligation KINDS that promote a row to `OracleAndGuard` (u0 §Q4), NOT a separate
proof variant. The correction overlay machinery the prover consults stays:

```rust
// the review-gated correction overlay the DivergenceCorrection prover consults
// one per corrected (row, query_ordinal); the row's other queries carry none
struct QueryCorrection {
    query_ordinal: u16,        // WHICH of the row's N queries this correction binds
    correction: CorrectionId,  // the review-gated overlay (§3) = the Correct value for that query
    divergence_id: DivergenceId,
}
```

**`CorrectionId` is the EXACT `(family, snapshot_id)` path identity.** `CorrectionId` is
NOT an opaque handle: it is the `(family, snapshot_id)` pair that addresses one correction
file at `oracle_corrections/<family>/<snapshot_id>.correction.json` (§3.1) — its
`snapshot_id` component IS the file stem (per-query, since snapshots are one file per
`(row, query)`, u0 §Q1). Because each `QueryCorrection` carries its `query_ordinal`, its
`correction: CorrectionId`, and its `divergence_id: DivergenceId` as independently-resolvable
fields, the tie chain must mechanically pin them to the SAME query/snapshot/registry-entry,
or a correction authored for snapshot B could attach to query A and still pass each field's
bare resolvability. Guard D / the divergence proof contract therefore assert **four HARD
EQUALITIES per corrected query** (not mere resolvability):

- **(a)** the correction file STEM (the `<snapshot_id>` path segment, =
  `CorrectionId.snapshot_id`) `==` the correction's `diverges_from_snapshot_id` field;
- **(b)** that snapshot's `row_ref` (`{ row_file, row_function, query_ordinal }`) `==` the
  divergence query being corrected (the proof row at the `QueryCorrection`'s `query_ordinal`);
- **(c)** the corrected query's **registry-derived** `snapshot_id` (recomputed from the
  row's per-query registry entry at that `query_ordinal`, tsgo-free, u0 §Q1 `snapshot_id`)
  `==` `correction.diverges_from_snapshot_id`;
- **(d)** `correction.divergence_registry_id` `==` the `QueryCorrection.divergence_id`.

These four close the "correction attaches to the wrong query/snapshot" hole: every pointer
must resolve to the SAME `(row, query_ordinal, snapshot, registry-entry)` tuple. Added to
Guard D (§8) and the §3.3 overlay guards.

`kind_eligibility_gate` (u0 §3 invariant 7 / §4 `kind_eligibility_gate`)
learns: a divergence row MUST carry, for each corrected `query_ordinal`, a correction whose
`divergence_id` resolves to a registry entry (§5) AND asserts both recorded answers (§7.1) —
`resolver(query) == correction.correct_value` and a present, differing `snapshot.oracle_value`
for that query. The divergence linkage is the `DivergenceCorrection` obligation KIND — one of
the five `OracleAndGuard` obligation kinds (u0 §Q4) — proved by a registered live prover, NOT
stored as a typed set on any ledger record. A divergence row is `OracleAndGuard { oracle, guard }`
whose `guard` resolves to the registered `DivergenceCorrection` prover; that prover runs PER
corrected `query_ordinal`, asserting for each corrected query that its named correction overlay
AND a registry entry whose id equals `divergence_id` resolve to the SAME
`(correction, registry-entry)`, never `WholeRow`. A row asserting NO independent non-`TypeExpr`
obligation stays bare `Ts7Oracle`; a row asserting one (footprint / audit / warm-cache /
declared-dependency / divergence-correction) is promoted to `OracleAndGuard`. The linkage is
proved by `every_correction_is_discharged` + Guard C's data comparison.

### 9.3 What does NOT change

- The `row → block_id` partition is **orthogonal** to divergence. Divergence rows still
  count toward the **362** total and still partition by `block_id`. The manifest
  partition and all all-row-sensitive guards (length / orphan / per-file) are unaffected.
- Divergence rows do **NOT** move the `Relate`-free **≤122** upper bound
  (u0 §Scope) — projection-divergence rows are not
  `Relate`-carrying.
- `snapshot_id` derivability, the hermetic / offline-re-derivable guarantees, and the
  canonical-encoding rules are **untouched** — the correction is a *separate* artifact
  with its own (review-gated) trust model (§3.2).
- The **resolver** does not change at all — it is single-spec and produces `Correct`
  whether or not a correction exists for the row.

Only the **PROOF** (a divergence row seated as `OracleAndGuard` with the `DivergenceCorrection`
prover) and the **HARNESS DRIVER** (assert against the correction's `correct_value` where a
correction exists) change.

### 9.4 §6.3 amendment (a strengthening, not a loosening)

The differential tsgo-parity oracle's per-family divergence **budget M**
(native-typeinfo-parity §6.3, the "N conformance cases per family;
divergence budget M per family" gate) measures **unintended** divergence =
**defects**. A **registered** divergence is **intentional** → it must be **excluded from
M**.

Under single-spec there is one budget run, not two modes:

- **The budget compares `resolver(Correct)` against tsgo.** A row where they disagree is a
  candidate defect — UNLESS the row carries a registered correction, in which case the
  disagreement is **intended** (the resolver's `Correct` value equals
  `correction.correct_value`, which by construction differs from tsgo). Registered
  divergences are therefore **subtracted from M**: M counts only UNREGISTERED
  resolver-vs-tsgo disagreements (genuine accidental defects).
- **Registered corrections are confirmed by the data comparison**, not the budget:
  `resolver(Correct) == correction.correct_value` (the §7.1 / `every_correction_is_discharged`
  assertion).

This separates the two questions today's single budget conflates:

1. *"Did we accidentally diverge?"* → unregistered resolver-vs-tsgo disagreement, budgeted
   (target M → 0).
2. *"Did we intentionally diverge, correctly?"* → `resolver(Correct) ==
   correction.correct_value`, registry-gated.

A registered divergence is never charged against the defect budget, and an **unregistered**
divergence is still caught — it has no correction, so it shows up as a resolver-vs-tsgo miss
(M > 0). Pinned by `registered_divergence_excluded_from_budget` (§8). The owning §6.3 doc
(`docs/arch/native-typeinfo-parity.md`) records this reformulation in-place — see its §6.3
amendment.

---

## 10. Worked examples (Decision 9)

### 10.1 A FLAGGED bug (illustrative template — registration needs a real conceded issue)

> **Honesty note.** This subprocess has no network access, so it cannot confirm a
> specific TypeScript issue number actually concedes the behavior below. Per the brief,
> an honest **labelled template** with realistic values is preferred over a fabricated
> citation. At implementation time, registration REQUIRES a real conceded/fixed TS issue
> id satisfying §6 — the `ts_issue` field below is a placeholder (`ts#NNNNN`) precisely
> so it cannot masquerade as verified.

A conditional-type-distribution-over-`boolean` projection bug — the kind of construct
that is `TypeExpr`-representable (so this oracle can carry both recorded values) and that
historically has had genuine, fixed reduction bugs in TS's conditional/distributive
machinery.

Registry entry (`DivergenceSite`):

| Field | Value |
|---|---|
| `id` | `DivergenceId::ConditionalBooleanDistribution` |
| `ts_issue` | `ts#NNNNN` *(placeholder — a real conceded/fixed issue id required at registration)* |
| `concession_class` | `FixedInLaterVersion` *(the gold class, §6.3)* |
| `fixed_in_ts_version` | `"5.x.y"` *(the version that shipped the fix)* |
| `expected_correct` | `"distributing over `boolean` yields `A | B`, not the collapsed `B`"` |
| `criteria_justification_id` | `"crit-conditional-boolean-distribution"` |

Correction overlay (`oracle_corrections/conditional_distributive/<snapshot_id>.correction.json`):

| Field | Value |
|---|---|
| `diverges_from_snapshot_id` | `<the exact ts_compat snapshot's id>` |
| `correct_value` | `{"kind":"union","members":[{"kind":"ref","name":"A"},{"kind":"ref","name":"B"}]}` *(the fixed-TS answer, in the `TypeExpr` codec)* |
| `divergence_registry_id` | `"ConditionalBooleanDistribution"` |
| `ts_issue` | `ts#NNNNN` *(EQUALS the registry entry's, Guard D)* |
| `criteria_justification_id` | `"crit-conditional-boolean-distribution"` *(EQUALS the registry entry's, Guard D)* |
| `correction_file_id` | `blake3:…` over the FULL canonical correction object (all fields except `correction_file_id` itself) under canonical-JSON |

The snapshot's `oracle_value` (the recorded **`TsCompat`** value = the wrong tsgo output)
is the collapsed `{"kind":"ref","name":"B"}`. The two recorded answers are therefore data:

- **`TsCompat` (recorded):** `snapshot.oracle_value` = the collapsed `B` — captured from
  tsgo at generation time, never produced by the resolver.
- **`Correct` (resolver + overlay):** `correction.correct_value` = the union `A | B`.

The harness asserts (§7.1), for the corrected `(row, query_ordinal)` (this illustrative row
issues a single query): `resolver(query) == correction.correct_value` (the union `A | B`) —
the single-spec resolver, run once, must produce the corrected answer for that query — and
that the query's `snapshot.oracle_value` (the collapsed `B`) is present and differs from it.
**No resolver compat run** produces the `B`: it is the captured tsgo bug, recorded as data.

### 10.2 A by-design NON-divergence (rejected — shows the discipline)

**Literal widening of a `const` initializer.** Consider `const x = 5`. TypeScript's
*apparent* / widened type rules give `x` the type `number` in some positions and the
literal `5` in others (the `const` narrowing / widening behavior).

Suppose a proposer claims Verter "should" report `5` everywhere because "`number` is
surprising." Run it through §6:

- **Gate 1 (issue exists)** — there is no bug-conceding issue; the behavior is
  documented as intended.
- **Gate 2 (concession)** — literal widening is **`"Working as Intended"`** / by design.
  `ConcessionClass` has no member for "working as intended," so this cannot be admitted.
- The justification reduces to `"surprising"` — a §6.2 categorical-reject phrasing.

**Verdict: REJECTED.** No `DivergenceId`, no registry entry, no correction. The row carries
**no correction**: the harness asserts `resolver(row) == snapshot.oracle_value` (§7.2), and
the resolver's one `Correct` value already equals tsgo here. Matching TypeScript's widening
here **IS** correctness (§1.1) — diverging would be a Verter defect, not a correction.

---

## 11. Honest residuals

Stated plainly, no hand-waving:

1. **The `correct_value` is not offline-re-derivable.** There is no oracle for "what
   TypeScript should have done"; the correction's truth root is human review + a conceded
   issue (§3.2). Mitigated only by the set being tiny, each entry individually justified,
   and the snapshot's recompute-gated guarantees left untouched.

2. **Two human-review links, both acknowledged.** (a) No guard can verify an issue truly
   concedes a bug (§6.4): guards enforce the FORM of the tie chain, but the content of the
   cited issue is a human-review trust link. (b) No guard can mechanically classify a fully
   novel-named resolver-input selector (§4): a reviewer must record an `is_spec_selector`
   attestation when registering a new `ProjectTypeStore` cache-key / context type, and (b)'s
   field-inventory then flags any field whose type is so attested. These are the design's two
   un-mechanizable links and are labelled as such throughout — everything else in the tie
   chain and the absence invariant is enforced by a discriminating mechanical guard.

3. **The projection-divergence set this oracle can carry is small** (§1.2). The bulk of
   genuine TS bugs are verdict-family; their divergence story defers to the future
   structured oracle, which reuses this DATA PRINCIPLE (the recompute-gated snapshot, the
   review-gated correction overlay, the registry/criteria, the harness data comparison) —
   adding only its own recorded answers, never a resolver spec dimension (there is none).
   This document does not claim to cover verdict-family divergences — it claims to be the
   data discipline that future oracle will reuse.

4. **Future escape hatch (documented, NOT built now).** If a real live bug-for-bug runtime
   consumer ever materializes — a surface that genuinely needs Verter to *produce* a
   TS-bug-for-bug value at query time on arbitrary code — the correct design is **boundary
   containment**, NOT a spec dimension threaded through the shared caches: capture the
   reduction frame during the one (`Correct`) reduction, and apply a bug-reintroducing patch
   at the **consumer edge, AFTER the value has exited all shared caches**, so the divergent
   value never enters any shared cache. This is explicitly **not built now**: nothing needs
   it (§2.2 — every surface is `Correct`; the only co-presence case obtains its tsc side by
   running a real `tsc`), and building it speculatively would re-introduce exactly the
   open-ended cross-cache obligation single-spec was chosen to eliminate (§4). It is recorded
   here so the escape route is known and is *contained at the boundary* if it is ever
   needed, rather than threaded through the resolver.

---

## 12. Planned-guard index

All deferred to the mechanism block (each is vacuous today). Named here so every guarded
claim above has a target.

| Guard | Owner area | Discriminates (§ref) |
|---|---|---|
| `resolver_is_single_spec` | resolver / cache / session: (a) exact-token scan + (b) structural field-inventory over a closed target list, plus a registration `is_spec_selector` attestation (human-review) for novel-named selectors | (a) a re-introduced exact token — `SpecVariant` / `spec_variant` / `spec.diverge` / `TsCompat` / `ts_compat` / `bug_for_bug` / `compat_profile` reused ANYWHERE incl. as an enum VARIANT name (so `enum SemanticMode { Correct, TsCompat }` and a `compat_profile` axis are caught by **(a)**, not (b); NO `*_profile` glob, no "or equivalent" clause — `query_profile` / `compile_profile` / `tsx_profile` are legitimate non-spec profiles); (b) a deny-named OR forbidden-selector-TYPED field over the CLOSED list `SemanticQueryKey` / `FamilyKey` / `ComponentMetaResultKey` / `MaterializeStructureCacheKey` / `ShapeCacheKey` / named session-/per-key-context structs (`SessionResolverContext` / `InstantiateContext` / `MacroPayloadContext` / `ProjectionReductionContext`) / `SemanticQueryKeySpec` (TYPE in the CLOSED forbidden-selector-type set — NOT a `*Spec` glob — OR NAME in the (a) set; `query_profile` / `compile_profile` / `tsx_profile` allowlisted); rooted on the closed resolver-input surface + a registration meta-rule (new `derive(Hash)` `ProjectTypeStore` cache keys MUST register into (b) AND carry an `is_spec_selector` attestation). A FULLY NOVEL-NAMED selector is caught NOT by (a)/(b) mechanically but by that attestation — the SECOND acknowledged human-review link (peer to §6.4); oracle-harness / correction-metadata mentions are whitelisted (§4, §8) |
| `every_correction_is_discharged` | harness + data | a correction whose `resolver(query) != correct_value`; a correction not exercised by a live corrected query, or a corrected query with no correction (set-equality keyed by `(row_file, row_function, query_ordinal, snapshot_id)`; intentionally overlaps `no_orphan_correction`) (§4, §7.1, §8) |
| `every_divergence_id_has_registry_entry` (Guard B) | registry | a `DivergenceId` with no entry (§5, §8) |
| `every_registry_entry_is_exercised` (Guard C) | registry + harness | a dead entry with no correction artifact behind it (§5, §8) |
| `divergence_tie_chain_is_whole` (Guard D) | registry + corrections | a broken FORM link, an incomplete closed `CriteriaJustification` (§6.5), `registry.concession_class ≠ criteria.concession_class`, `registry.fixed_in_ts_version ≠ criteria.fixed_in_ts_version` or `fixed_in_ts_version` present without `FixedInLaterVersion`, a `correction.ts_issue`/`criteria_justification_id` ≠ its registry entry's, a `criteria.snapshot_ref ≠ correction.diverges_from_snapshot_id`, a `criteria.repro_ref` ≠ the corrected `row_ref`, any of the four `CorrectionId` path-identity equalities broken, or a banned-rationale token in `expected_behavior` (§6.5, §8, §9.2) |
| overlay: `correction_overrides_real_snapshot` | corrections | a `diverges_from_snapshot_id` with no snapshot (§3.3) |
| overlay: `correction_id_is_exact_path` | corrections | a correction whose file stem ≠ `diverges_from_snapshot_id`, a snapshot whose `row_ref` ≠ the corrected query, or a corrected query whose registry-derived `snapshot_id` ≠ `correction.diverges_from_snapshot_id` — `CorrectionId` is the exact `(family, snapshot_id)` path identity (§3.3, §9.2) |
| overlay: `correction_differs_from_snapshot` | corrections | a `correct_value` equal to `oracle_value` (§3.3) |
| overlay: `correction_file_id_tamper_evident` | corrections | a correction that does not strictly decode + re-hash the full canonical object to its stored `correction_file_id` (§3.1, §3.3) |
| overlay: `no_orphan_correction` | corrections | a correction file with no corrected query, or a corrected query with no correction file (set-equality keyed by `(row_file, row_function, query_ordinal, snapshot_id)`, §3.3) |
| §6.3: `registered_divergence_excluded_from_budget` | differential oracle | a registered divergence charged against M (§9.4) |
| reuse (unchanged): `tsgo_not_reachable_from_resolver`, `oracle_consumption_path_has_no_tsgo_spawn`, `cache_key_axes_are_minimal_and_normalized`, `cache_satisfaction_is_demand_lattice_not_enum_order` | parity engine / U0 | tsgo never on the query path (§7.4); the static `DemandAxis` minimality mask and the demand-lattice warm-hit/backfill rule — UNCHANGED and untouched by this model (there is no spec dimension to interact with them) |
