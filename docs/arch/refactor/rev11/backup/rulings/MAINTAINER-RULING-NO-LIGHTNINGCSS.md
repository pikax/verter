---
ruling_id: "NO-LIGHTNINGCSS"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["Track J / J1", "BCSS0 (superseded within this document)", "CSS/style pipeline"]
source_file: "MAINTAINER-RULING-NO-LIGHTNINGCSS.md"
summary: "Binding, project-wide: lightningcss is not Verter's CSS engine and is to be removed; verter_css_syntax is the single CSS authority; a capability gap is a build instruction, not a reason to keep lightningcss. Recorded before its own supporting architecture consult returned, so it forecloses that consult's 'lightningcss must stay' arm. The remainder of the document is a same-day ratchet of sequencing decisions (recorded chronologically within this one file, each dated 2026-08-17): BCSS0 held pending scope ruling -> released to proceed on lightningcss as originally scoped (removal deferred to a later train, debt row CSS-ENGINE-001) -> corrected to name J1 (an existing in-program, dependency-eligible block) as the owner rather than a new out-of-program train -> BCSS0 attempts to re-point its source-map correction at the canonical style_planner route -> found infeasible (byte-identity conflict with BCSS0's own invariant) -> BCSS0's entire product (engine swap + standalone CSS source-map correction) transfers to J1, BCSS0 SUPERSEDED -> final directive: ALL CSS WORK STOPS UNTIL THE J TRAIN, the J1 charter draft PARKED (unratified), BCSS0 removed from B2/B3 predecessor lists entirely."
supersedes: []
superseded_by:
  - ruling: "CSS-CLEAN-CUTOVER"
    claim: "The final in-document directive's 'ALL CSS WORK STOPS UNTIL THE J TRAIN / J1 charter draft PARKED, not to be advanced / no further CSS consults, drafts or amendments' state — superseded by the 2026-08-20 directive which resumes Track J planning immediately. The core architectural decision (lightningcss removal; verter_css_syntax sole authority) is retained and carried forward, not superseded."
contradicts: []
notes: "Internally self-superseding chronology: read as a ratchet where each later dated entry in the same file supersedes the immediately preceding sequencing decision in that file, while the top-level architectural decision (lightningcss removal) never wavers. The document also records debt row CSS-ENGINE-001 (later folded into J1's CSS-AUTH-001) and an undispositioned style-path wrong-output violation (style_planner.rs:745,942; compile/mod.rs:608) deferred to J1 as CSS-REFUSE-001. States BCSS0 is formally SUPERSEDED and its branch block/bcss0 (tip 74a5a0291, 8 commits, nothing landed) retained only as reference — this requires a program-dag.toml amendment (BCSS0 removed from B2/B3 predecessor lists) that this document states is being authored for ratification, not itself landed by this document."
---

# Maintainer ruling — Verter owns its CSS engine; lightningcss goes (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
Binding project-wide. Recorded before the supporting architecture consult returned, so it
FORECLOSES that consult's "lightningcss must stay" arm; the consult's remaining value is its
concrete inventory of what `verter_css_syntax` lacks.

## Verbatim ruling

> we shouldn't be using lightningcss....

and, on being shown that `verter_css_syntax` may not cover every capability the lightningcss
path currently provides:

> verter css should be able to do those, if not we need to implement whatever we need

## Normalized rules

1. **lightningcss is NOT Verter's CSS engine and is to be removed.** `verter_css_syntax` is the
   single CSS authority. There is no "keep both" outcome and no compatibility shim.
2. **A capability gap in `verter_css_syntax` is a BUILD instruction, not a reason to keep
   lightningcss.** Whatever the standalone CSS route needs — parsing, scoped-selector rewriting,
   CSS Modules, `v-bind()`, preprocessing, printing — Verter implements in its own crate.
3. This follows the existing Shared Optimized Codebase rule directly: two engines for one concern
   is a rule violation to DELETE, not to preserve. The CSS pipeline currently has exactly that —
   `crates/verter_compiler/src/css/` on lightningcss versus `style_planner.rs` +
   `svelte/runtime/css/` on `verter_css_syntax`.
4. **Do not acquire dependencies that exist only to serve lightningcss.** `parcel_sourcemap` was
   added solely to fill lightningcss's hard-typed printer slot
   (`lightningcss-1.0.0-alpha.71/src/printer.rs:22`); it should not survive the migration.

## The evidence that prompted it

`crates/verter_css_syntax/` is a Verter-owned crate with its own `StyleSyntaxIr`
(`src/style_ir.rs`), dialect handling (`src/dialect.rs`) and WPT `css-syntax` fixtures
(`tests/fixtures/wpt/css-syntax`) — a spec-tested CSS parser. lightningcss is nevertheless used
across `crates/verter_compiler/src/css/{mod,scoped,modules,prepass,walk,types}.rs`, declared at
`crates/verter_compiler/Cargo.toml:91` and workspace `Cargo.toml:42`.

## Still open — NOT decided by this ruling

Scope and sequencing are unresolved and are being ruled on by architecture consult:
- Does the removal belong to the in-flight narrow block BCSS0, to a NEW block (a DAG amendment,
  which is a maintainer act), or after the program?
- What happens to BCSS0's 8 in-flight commits: does its authored-anchoring correction survive
  re-expressed over `verter_css_syntax` + `CodeTransform` with no printer map at all, or is it
  inherent to the lightningcss printer?
- If removal cannot land immediately, the interim must be recorded as an explicit DEFER with a
  named owner, a resolution gate and an acceptance ID — never allowed to become permanent by
  default.

BCSS0 is HELD (not landed, nothing discarded) pending that scope ruling.

## Sequencing ruling — DEFERRED to a later train (2026-08-17, same maintainer)

> we don't need to build all the css now, we can delay to a better train

**The removal DIRECTION above is unchanged and still binding.** What is deferred is the WORK, not
the decision. lightningcss is still slated for removal and `verter_css_syntax` is still the single
intended CSS authority.

### What this authorizes now

- BCSS0 proceeds AS ORIGINALLY SCOPED on the existing lightningcss path. Its hold is RELEASED. Its
  narrow standalone-CSS source-map correction lands on the current engine.
- `parcel_sourcemap` is accepted as an INTERIM dependency, on the record that it exists only to
  fill lightningcss's hard-typed printer slot and is expected to disappear with the migration.
- No block in the current program opens CSS-engine unification work. BCSS0 does not rescope, does
  not migrate, and does not delete lightningcss.

### Debt row — CSS-ENGINE-001

- **Finding.** Two live CSS authorities: `crates/verter_compiler/src/css/` on lightningcss versus
  `crates/verter_compiler/src/style_planner.rs` + `crates/verter_compiler/src/svelte/runtime/css/`
  on `verter_css_syntax`. This violates the Shared Optimized Codebase rule (a second parallel engine
  for one concern is to be deleted, not preserved).
- **Disposition.** DEFER, by direct maintainer act.
- **Owner.** A dedicated CSS-engine unification train, to be created outside the current
  architecture program's DAG. NOT owned by BCSS0 and not by any existing block.
- **Scope.** Implement whatever `verter_css_syntax` lacks for the standalone CSS route — parse,
  scoped-selector rewriting, CSS Modules, `v-bind()`, preprocessing, printing — then delete the
  lightningcss path and the `parcel_sourcemap` dependency. The concrete HAS/LACKS inventory is being
  produced by an architecture consult and by BCSS0's separability report; both feed this train's
  scope so it does not start from a blank sheet.
- **Resolution gate.** Before the CSS pipeline is declared final for release. This deferral must not
  survive into a shipped stable release, and it must not be closed by anything other than the
  removal actually landing.
- **Acceptance criterion.** Zero references to `lightningcss` and zero to `parcel_sourcemap` under
  `crates/`, with the standalone CSS route's existing byte-pin and source-map tests passing
  unchanged against `verter_css_syntax`.
- **Anti-drift condition.** BCSS0's committed evidence must state that its correction is built on a
  path slated for removal and reference this row, so the future train inherits the context rather
  than rediscovering it.

## CORRECTION to the debt row above — the owner is J1, an EXISTING in-program block

My debt row named "a dedicated CSS-engine unification train, to be created outside the current
architecture program's DAG". **That was wrong**, and an architecture consult caught it.

`J1` — "CSS owner reconciliation" — ALREADY EXISTS in `program-dag.toml` (verified: `class =
"subsystem"`, `predecessors = ["A4", "A6"]`, and BOTH predecessors are ACCEPTED). It is LOCKED only
because it has no ratified charter — just `charters/J1.template.md`. It is therefore
dependency-eligible NOW.

Critically, J1's own charter text already requires that every path be classified
Preserve/Converge/Replace/Delete and **already requires rescoping when multiple semantic authorities
exist** (`J1.template.md:10`). This situation is squarely inside its existing mandate; nothing new
needs inventing.

Corrected debt row fields:
- **Owner: J1** (existing block), NOT a new out-of-program train.
- Acceptance ID: `CSS-AUTH-001` (adopting the consult's identifier).
- Acceptance: every Vue style product — compiler, framework bridge, NAPI/unplugin, session — uses
  `StyleSyntaxIr`, `style_planner` and `CodeTransform`; the legacy `crates/verter_compiler/src/css/`
  implementation is DELETED; `cargo tree -i lightningcss` has no Verter Rust root.
- Deviation to record: the failed premise that `verter_compiler::css` was the correct durable
  product owner.

J1 becoming the owner means it needs a ratified charter, which is a maintainer act. It is currently
`subsystem` class; the consult argues it should be `foundational`.

## A SEPARATE live rule violation found by the same consult — not yet dispositioned

Independent of the engine question: the current style paths can **return errors, delete untrusted
rules, or clear the stylesheet** (`style_planner.rs:745`, `style_planner.rs:942`,
`compile/mod.rs:608`). Those behaviours violate the ratified wrong-output-is-a-bug ruling, which
forbids production defect-recognition, refusal and withhold paths.

Under the maintainer's standing bugs-and-types rule the interim handling is an added `#[ignore]`d
characterization test with the fix deferred to a named owner. Proposed owner: **J1**, as the block
that owns this pipeline's reconciliation. Not yet ratified — recorded here so it is not lost.

## Consult's own sequencing recommendation — CONFLICTS with the maintainer's, recorded for the record

The consult ruled: J1 first, BCSS0 held and marked `RESCOPE_REQUIRED`, with an amendment making J1
Foundational and adding a `J1 -> BCSS0` edge. Its reasoning: landing BCSS0 on lightningcss produces
implementation and tests that must be re-authored after J1, and acquiring `parcel_sourcemap` serves a
path already ordered for deletion.

It also confirmed the authored-anchoring correction IS **separable** from lightningcss's printer,
because the canonical planner already generates mappings from the same `CodeTransform`
(`style_planner.rs:237`).

The maintainer's sequencing instruction ("we don't need to build all the css now, we can delay to a
better train") was given BEFORE these facts were known — specifically before it was known that the
later train is an existing, dependency-eligible in-program block, and that BCSS0's correction is
separable. The sequencing question is therefore returned to the maintainer rather than resolved by
either the consult or the orchestrator.

## Maintainer sequencing act — BCSS0 re-points at the canonical route (2026-08-17)

Issued after the maintainer was shown the two facts that were unknown at the time of the earlier
"delay to a better train" instruction: that the later train is the EXISTING, dependency-eligible
block J1, and that BCSS0's authored-anchoring correction is SEPARABLE from lightningcss's printer.

**Ruled: BCSS0 authors its source-map correction over `style_planner` + `CodeTransform` now, instead
of over lightningcss's printer — if feasible.**

Consequences:
- No printer-map composition and no `parcel_sourcemap`. The interim-dependency acceptance recorded
  earlier is WITHDRAWN: the dependency is not to be acquired, because the correction no longer needs
  it.
- BCSS0's work SURVIVES J1 rather than being re-authored after it. This is the point of the ruling.
- The full CSS-engine unification and the deletion of `crates/verter_compiler/src/css/` remain
  DEFERRED to J1 under debt row `CSS-AUTH-001`. BCSS0 does NOT migrate the engine, does not delete
  lightningcss, and does not take on J1's scope.
- B2/B3 stay OFF the CSS critical path — the reason the maintainer declined the consult's
  "J1 first, hold BCSS0" recommendation.

**Scope authority.** BCSS0's charter excludes unrelated refactoring (`BCSS0.md:10`, `:47`). Changing
HOW the standalone route's source map is produced is not unrelated refactoring — it IS the product
BCSS0 owns ("standalone CSS source-map product correction"). This maintainer act is the explicit
scope authority for that reading, so a track-level actor is not inferring it. If a review seat still
reads the re-pointing as J1-owned refactoring, that escalates to the maintainer rather than being
argued at track level.

**Feasibility is UNCONFIRMED and conditions this act.** Whether `style_planner` can serve the
standalone CSS route today is UNKNOWN pending BCSS0's HAS/LACKS inventory. If it proves infeasible,
BCSS0 reports the specific blocker with evidence and STOPS — it must NOT silently fall back to the
lightningcss printer, and must not quietly absorb J1 scope to make it work.

## Maintainer act — ratify J1, and defer the style-path violation to it (2026-08-17)

> ratify J1.
> - yes

Two decisions:

**1. J1 is to be RATIFIED** as the owner of CSS authority reconciliation.

Execution note: there is no charter TEXT to ratify — `charters/J1.md` does not exist, only
`charters/J1.template.md`. The charter is therefore being AUTHORED against this ruling and returned
to the maintainer for ratification of the actual bytes, per the same flow used for the preceding
amendments. This act records the intent and the binding content; it is not itself the ratification
of a specific charter file.

Two points resolve from the template's own text rather than needing separate acts:

- **Class: FOUNDATIONAL, self-executing.** `J1.template.md:4` reads "Subsystem, promoted to
  Foundational if it changes shared syntax ownership or public compatibility." J1 makes
  `verter_css_syntax` the sole CSS authority, which is precisely a shared-syntax-ownership change.
  The promotion is the template's own conditional firing. (This also matches the architecture
  consult's independent recommendation.)
- **The template's abort condition is ALREADY TRIPPED.** `J1.template.md:27` says "Stop if source
  evidence shows multiple semantic syntax authorities." That is exactly the current state. The
  ratified charter must therefore RESOLVE that trigger by carrying the maintainer's ruling — single
  authority, lightningcss deleted, gaps implemented — instead of inheriting a stop condition it is
  guaranteed to hit on day one.

The ratified charter carries: the template's evidence/inventory obligations; the per-path
Preserve/Converge/Replace/Delete/Defer disposition requirement; the single-authority outcome; and
ownership of debt row `CSS-AUTH-001`. It does NOT add a `J1 -> BCSS0` edge — the maintainer declined
that sequencing so B2/B3 stay off the CSS critical path.

**2. "yes" — the style-path wrong-output violation is DEFERRED TO J1.**

The current style paths can return errors, delete untrusted rules, or clear the stylesheet
(`style_planner.rs:745`, `style_planner.rs:942`, `compile/mod.rs:608`), contradicting the ratified
wrong-output-is-a-bug rule which forbids production defect-recognition, refusal and withhold paths.

Handling, per the maintainer's own standing bugs-and-types rule: an added `#[ignore]`d
characterization test per behaviour, fix deferred, owner **J1**. Acceptance ID `CSS-REFUSE-001`.
Resolution gate: J1 acceptance. No production guard, tracker or allowlist is added in the interim —
the characterization is test-side only.

## Maintainer act — BCSS0's product transfers to J1 (2026-08-17)

Issued after BCSS0 reported, with verified evidence, that re-pointing its correction at
`style_planner` is INFEASIBLE within its charter: the map is separable from lightningcss's printer
MAP but not from its PRINTER, because the canonical route preserves authored bytes while
`process_style` emits lightningcss-normalized bytes. Re-pointing would change emitted CSS on a
shipped public route, violating BCSS0's own byte-identity invariant. That change is J1's engine
reconciliation. BCSS0 stopped rather than absorbing it.

**Ruled: hand the whole product to J1.** BCSS0 is superseded. J1 owns BOTH the engine swap AND the
standalone CSS source-map correction, authoring the correction ONCE over the canonical
`verter_css_syntax` + `style_planner` + `CodeTransform` route. Zero rework.

The maintainer accepted the stated cost: this places the CSS migration on the critical path, because
B2 and B3 cannot proceed until this product is resolved.

### Required DAG amendment — this act cannot take effect without it

`BCSS0` is a declared predecessor of BOTH `B2` and `B3` (per AMD-009). The ledger validator requires
every direct predecessor to be **ACCEPTED**; `SUPERSEDED` does not satisfy it. So marking BCSS0
SUPERSEDED without amending the graph leaves B2 and B3 permanently unreachable.

The amendment must therefore:
1. Record `BCSS0` as `SUPERSEDED`, its product transferred to `J1`, with its charter retained as
   historical text.
2. Replace `BCSS0` with `J1` in the predecessor lists of `B2` and `B3` — faithful to this ruling,
   since the product still gates them and it is now J1's.
3. Ratify the J1 charter carrying the added source-map product scope.
4. Preserve BCSS0's acceptance intent: J1 must satisfy the standalone CSS source-map contract, not
   quietly drop it.

This is a maintainer-reserved amendment and is being authored for ratification of its actual bytes.

### What J1 inherits from BCSS0 (all already produced — J1 does not start from a blank sheet)

- The verified HAS/LACKS inventory of `verter_css_syntax` for this route.
- The infeasibility finding and its byte-divergence evidence (the two concrete pins).
- **An open BLOCKING discrimination hole:** BCSS0's adversarial mutation battery found that skipping
  the walker composition, so mappings keep pre-rewrite columns, leaves the suite GREEN. A real test
  gap that must be closed by whoever owns this product.
- **The transport axis is UNPROVEN:** `hasMap == true` has never executed (0 selected under default
  features; with `transport-authoritative` on it stops at a missing-NAPI-artifact prerequisite before
  the assertion). Closure condition: build `@verter/native` debug, regenerate the transport-surface
  probe record, then run it.
- BCSS0's behavioural test INTENT — authored-anchoring contract, UTF-16 seam, byte-identity pins,
  discrimination proofs — to be re-expressed over the canonical route.
- Its `CodeTransform`-as-sole-geometry-authority work, which moves in the right direction regardless.
- **`parcel_sourcemap` is NOT inherited.** It dies with the printer; J1 must not acquire it.
- Branch `block/bcss0` (tip `74a5a0291`, 8 commits, nothing landed) is retained as reference.

### Charter inaccuracy confirmed, not edited

BCSS0's charter implied NAPI production changes were needed. They are not: NAPI already forwards
losslessly (`verter_napi/src/lib.rs:120`, `:123`, `:164-165`, `:174`) and the Rust result already
carried the field (`css/types.rs:81`). The hard-coded `None`s are at `css/mod.rs:110` and `:145` —
the consult's `:104`/`:143` were slightly off. Charter left untouched; correcting it is a maintainer
act folded into the amendment.

## Maintainer direction — ALL CSS WORK STOPS UNTIL THE J TRAIN (2026-08-17)

> I'm getting a bit tired of this CSS wasting time at this step, lightningcss is meant to be removed
> already, I want you to advance the implementation and ONLY work on CSS during J, we are wasting so
> much time and tokens in something that is not meant to be used at all (lightningcss), we been
> implementing this plan for over a week and the commits I see is mostly docs that are not production
> code!! I want to see more things working correctly!

Verified and accepted: of 78 commits on `program/architecture-lock`, only **19** touch
`crates/`/`packages/`/`scripts/`; **59** are docs-only. 203k lines of documentation against 85k of
code. The criticism is factually correct.

### Binding rules from this point

1. **NO CSS WORK until the J train.** The J1 charter draft
   (`scratchpad/J1-CHARTER-DRAFT.md`, 684 lines, fully verified) is PARKED as-is. It is not
   ratified, not landed, and not to be advanced. No A6 CSS-cell extension now. No further CSS
   consults, drafts or amendments.
2. **All CSS obligations consolidate into J** — `CSS-AUTH-001` (two/three live CSS authorities),
   `CSS-REFUSE-001` (mid-compile rule-deletion and stylesheet-clearing), the standalone CSS
   source-map product inherited from BCSS0, the third-authority `Defer` for
   `svelte/runtime/css/parse.rs`, and the four items needing rehoming out of
   `crates/verter_compiler/src/css/`. The parked draft already carries all of it.
3. **CSS must not gate the program.** BCSS0 is a declared predecessor of B2 and B3; leaving it
   there makes B2/B3 unreachable and blocks the implementation the maintainer wants advanced. The
   amendment therefore REMOVES `BCSS0` from B2's and B3's predecessor lists rather than replacing it
   with `J1` — CSS defers wholly to the J train instead of sitting on the critical path.
4. **Orchestrator behaviour change.** Stop producing documents where production code is the
   deliverable. Reserve maintainer ratification for acts that genuinely cannot proceed without one,
   batch them, and never block implementation waiting on one that the current work does not depend
   on. Prefer advancing a running track over authoring another record.
