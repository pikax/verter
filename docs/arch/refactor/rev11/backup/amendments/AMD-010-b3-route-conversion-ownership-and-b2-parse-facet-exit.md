# AMD-010 — B3 route-conversion ownership and B2 parse-facet exit

**Status:** RATIFIED (see §9.1), recorded 2026-08-16. The preparer did not and cannot
ratify, review, or satisfy any independent mandate; the recorded decision in §9.1 is
the designated maintainer's.

> **Landing note — added at landing, not part of the ratified text.** §7's commit
> embargo is discharged: BF3's unit landed on `program/architecture-lock` first, and
> this amendment lands next as its own squashed unit on the post-BF3 tip, exactly as
> §7's sequence requires. §7 states on its own face that it "records a scheduling
> constraint set by the program orchestrator … not a maintainer architecture decision",
> and the program orchestrator directed this landing after BF3's unit landed rather
> than after its acceptance; the constraint's stated purpose — not advancing BF3's base
> mid-flight — is satisfied either way. BF3 is NOT accepted, and nothing here accepts
> it, unlocks B2 or B3, or dispatches any block.
**Prepared against:** local `program/architecture-lock` commit
`75bf4f722d2a0f1c99efae8d15d2eb811f16b168`, tree
`54817b786bb326de5b9a823d6c5536480d1cd916`, clean working tree. Every `file:line`
citation below was read directly on that tree by the preparer, not taken from the
consults. The two scoping specs this amendment answers were prepared against
`040084bf0`; the only delta from `040084bf0` to the prepared-against tip is a
one-line ledger-pointer advance
(`docs/arch/architecture-lock/ledger/program-state.toml`), so no source citation
moved.
**Amends on ratification:** [`../charters/B3.md`](../charters/B3.md) (the
`Predecessor` declaration on line 3 and the ownership paragraph at lines 16–18),
[`../charters/B2.md`](../charters/B2.md) (the exit sentence at lines 15–17), and
one sentence of
[AMD-005](AMD-005-framework-compiler-conformance-rescope.md) §5 (lines 129–130).
Full enumeration in §5. **It changes no DAG edge, adds no block, retires no block,
and moves no acceptance owner off any capability-matrix cell.**

---

## 1. Binding direction

**Ratified 2026-08-16 in the NARROW form drafted below — 3 documents, 4 deltas.** The
maintainer sustained the preparer's rejection of two of the first consult's five
amendment targets (`program.md:413-417`, `product-inventory.md:37-40`) and the
preparer's narrowing of the third from `AMD-005:126-144` to `:129-130`, expressly
preserving AMD-005 §6. The emitter-mapping ledger ingress rows remain the recorded,
explicitly-labelled broader ALTERNATIVE and are DEFERRED, not adopted. The drafter's
Q-6 is resolved: B3's conversion obligation stops at the outermost Rust ingress (§3.3).
Full record: §9.1.

Two independent architecture consults — the second run unprimed, without knowledge
of the first — reached the same diagnosis: **B3's ratified charter is not executable
at its DAG position.** B3 is required to be the sole canonical typed compiler
request with no second option authority and no silently-ignored public option
([`B3.md:11-14`](../charters/B3.md)), while transport conversion is assigned to K2
([`B3.md:16-17`](../charters/B3.md); [AMD-005:129-130](AMD-005-framework-compiler-conformance-rescope.md)).
Every production request-construction point reachable at B3's dispatch discards,
defaults, or reinterprets public options before any canonical constructor could see
them (§3.2). Leaving those routes violates B3's exit; changing them violates B3's
ratified scope. That is a structural contradiction, not a difficulty.

The direction is: **assign B3 bounded ownership of option admission,
unknown-option refusal, and conversion into the canonical request at the production
request-construction points that exist at its dispatch; leave route replacement,
route exposure, publication, final carrier typing, and cross-route equivalence with
their existing later owners.** K2 retains what its own program text already
describes — final typed framework-private carrier representation and removal of the
residual `Any + Send + Sync` erasure — and performs no second semantic conversion.

Separately, two ratified-text defects are corrected: B3's charter declares the wrong
predecessor set (§2), and B2's exit criterion states an aggregate condition B2 cannot
satisfy alone (§4).

This amendment does not weaken
[AMD-005 §6](AMD-005-framework-compiler-conformance-rescope.md) (`:132-144`). Its
exactly-once classification, unknown-option refusal, no-silent-ignore, and
no-second-semantic-authority requirements are correct and remain in force verbatim.
The consults' remaining directives (inline/SSR and inline/Vapor refusal shapes, the
`framework_extras` ephemeral-carrier split, the `CompileTargetTag` audit-schema
replacement, the Svelte output-liveness DEFER to BS1) are implementation guidance for
B3's bound charter, follow existing authority, and are deliberately **not** amendment
content.

---

## 2. B3 predecessor correction

### 2.1 The verified conflict

Three sources disagree, and the two ratified ones agree with each other against the
charter:

| Source | States | Verified |
|---|---|---|
| [`charters/B3.md:3`](../charters/B3.md) | `**Predecessor:** BF3.` | read directly |
| [AMD-006 §2](AMD-006-vue-known-defect-correction.md) `:55-59` | "The B2 and B3 predecessor rows both become `predecessors = ["BV0", "BF3"]`" | read directly |
| [AMD-006 §8.1](AMD-006-vue-known-defect-correction.md) `:170-172` (recorded ratification) | "amend the DAG so B2 and B3 require both BV0 and BF3; and authorize no B2/B3 dispatch until both BV0 and BF3 are accepted" | read directly |
| [`../program-dag.toml:99-103`](../program-dag.toml) | `id = "B3"` … `predecessors = ["BV0", "BF3"]` | read directly |

The authoritative machine-readable DAG and the ratified amendment agree. Only the
charter's prose line is stale. It has been stale since AMD-006 landed at
`fdb6f6291`.

### 2.2 Charter delta

`charters/B3.md:3` — current:

> **Status:** PROPOSED amendment / LOCKED. **Predecessor:** BF3.

replaced by:

> **Status:** PROPOSED amendment / LOCKED. **Predecessors:** BV0 and BF3
> (`program-dag.toml` is the sole predecessor authority; see AMD-006 §2).

### 2.3 Sibling stale declarations — preparer-identified, NOT ADOPTED

The same staleness exists in three sibling locations. **Neither consult named
these; the preparer found them.** They were surfaced so the maintainer could include
or strike them deliberately rather than leave a known-stale statement unrecorded; the
decision was to strike:

| Location | Current text | Correct per `program-dag.toml` |
|---|---|---|
| [`charters/B2.md:3`](../charters/B2.md) | `**Predecessor:** BF3.` | `BV0` and `BF3` (`program-dag.toml:93-97`) |
| [`../program.md:150`](../program.md) | `**Predecessors:** ``B1``.` (B2 section) | `BV0`, `BF3` |
| [`../program.md:158`](../program.md) | `**Predecessors:** ``B1``.` (B3 section) | `BV0`, `BF3` |

`program.md`'s two lines have been stale since AMD-005 §4 amended the region
(`AMD-005:82-105`); neither AMD-005 nor AMD-006 edited them.

**RATIFIED 2026-08-16: NOT ADOPTED.** The ruling ratifies exactly four deltas and
`charters/B2.md:3` is not among them. All three sibling statements therefore remain
stale in the tree, recorded here rather than hidden, and are superseded in substance
by `program-dag.toml` — which §2.2's replacement text establishes as the sole
predecessor authority — together with AMD-006 §2 and §8.1. No correctness consequence
follows, because the authoritative source is machine-readable and unambiguous. Full
disposition: §8, Q-4.

---

## 3. B3 option-conversion ownership

### 3.1 The structural conflict

B3's charter simultaneously requires:

> Each semantics-affecting option maps exactly once from the BF1 inventory. Unknown
> semantic options and unsupported combinations fail request construction. No public
> option is silently ignored. A universal framework options bag or generic fact bag
> cannot remain as a second semantic authority.
> — [`charters/B3.md:11-14`](../charters/B3.md)

and:

> K2 later owns transport carriers and mechanical conversion into this request. K2
> cannot reinterpret defaults, frameworks, capabilities, options, or products. B3 does
> not own parser recovery, framework lowering/codegen, publication, or route exposure.
> — [`charters/B3.md:16-18`](../charters/B3.md)

mirrored in the ratified amendment:

> K2 still owns transport carriers and their conversion into B3's request; it may not
> reinterpret framework semantics.
> — [AMD-005:129-130](AMD-005-framework-compiler-conformance-rescope.md)

A canonical constructor cannot refuse an option that its route discarded, defaulted,
or reinterpreted upstream. So "no public option is silently ignored" is unreachable
without migrating the routes; migrating the routes is assigned elsewhere. Clean-cutover
governance forecloses the escape of leaving a temporary adapter: an accepted cutover
must prove "one surviving production implementation" and "every in-scope caller
migrated" ([`../governance.md:321-322`](../governance.md)).

K2 is twelve blocks downstream: `program-dag.toml:321-325` gives K2
`predecessors = ["B6", "K1"]`.

### 3.2 The production route families B3 must convert

`evidence/framework-conformance/product-inventory.md:24-35` enumerates ten routes.
Three are recorded as absent before B5/B6 (`:31-33`: direct one-shot final core,
prepared first/repeat final core, direct batch final core) — B3 cannot convert a
route that does not exist. **Seven route families are present.** Each is listed with
the production request-construction point the preparer verified on the
prepared-against tree, and the concrete way it defeats the canonical constructor.

**R1 — internal compiler one-shot** (`product-inventory.md:26`).
`crates/verter_compiler/src/standalone.rs:38-53` (`StandaloneCompiler::compile_source`)
and `:57-73` (`compile_source_with_parsed`) take `&CodegenOptions` +
`&VerterCompileOptions` + `&VueMacroSemanticInput` directly. Both are second option
authorities with their own derived defaults:
`crates/verter_compiler/src/compile/types.rs:176-236` (`CodegenOptions`, with
`resolve_comments` at `:262` and `resolve_inline` at `:282` implementing the `derived`
classification in code) and `:339-382` (`VerterCompileOptions`). The bitflags
materialisation set `CompileTarget` is `crates/verter_compiler/src/compile/types.rs:5-41`.
Reinterpretation exemplar: `crates/verter_compiler/src/compile/mod.rs:484`
(`let use_vapor = verter_options.force_vapor || parsed.is_vapor();`) and `:1211-1216`
(`mode: if verter_options.ssr { Ssr } else if use_vapor { Vapor } else { Vdom }`) —
an explicit `force_vapor` is silently demoted when `ssr` is set, with no diagnostic.
This is the `VUE-SSR-VAPOR-BACKEND` cell's "potentially constructible through legacy
flags" (`capability-matrix.tsv:6`) realised in production.

The emitter/mapping ledger **already** splits R1 the way §3.3 recommends, which is
independent corroboration that the narrow boundary is the intended one:
`emitter-mapping-dispositions.tsv:10` assigns
`compile/types.rs::{CodegenOptions,VerterCompileOptions,CompileTarget}` — the option
and target authority — `Replace` with acceptance owner **`B3`** (B3's only sole-owned
ledger row), while `:9` (EM-008, `compile/mod.rs`) and `:34` (EM-033,
`standalone.rs`) — the orchestration and the route itself — are `Replace` with
acceptance owner **`B5`**. Options to B3; route to B5. This amendment makes B3's
charter say what its ledger row already says.

**R2 — host per-file / virtual-product routes** (`product-inventory.md:27`).
`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2854` and `:3287`
are the two production `RuntimeCompileOptions` construction sites
(`crates/verter_compiler/src/framework_common/carrier_compiler.rs:348-417`), carrying
`framework_extras: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>` at
`carrier_compiler.rs:416` — the literal generic fact bag `B3.md:14` forbids. The
upstream authority is `CompileProfile`, `crates/verter_session/src/types.rs:1322-1411`,
with its own defaults at `:1427`. A third production construction is the Vue IDE
re-entry at `crates/verter_compiler/src/framework_common/vue_bridge.rs:570`; the
remaining `RuntimeCompileOptions` literals in that file (`:1723` onward) are inside
`#[cfg(test)] mod tests` at `:1407-1408` and are not production sites.

**R3 — host / NAPI `compile_many`** (`product-inventory.md:28`).
`crates/verter_session/src/host_compile.rs:327` (`compile_many`), target selection at
`:348`. The `HostBacked` arm **discards the caller's profile entirely** and
substitutes the frozen preset `compile_profile_for_bundler()` at `:264`; NAPI's own
comment states it: "HostBacked ignores the profile (its profile is the frozen bundler
preset)" — `crates/verter_napi/src/lib.rs:2398`. Route-local validation and
conversion live at `crates/verter_napi/src/lib.rs:2355` and `:2400-2443`, including a
route-local `delimiterOpen`/`delimiterClose` pairing rule.

**R4 — NAPI `compile_with_audit`** (`product-inventory.md:29`).
`crates/verter_napi/src/lib.rs:2526` reaches
`crates/verter_session/src/host_compile_audit.rs:90-104`, which **bypasses
`CompileProfile` entirely** and constructs the compiler option bags directly:
`VerterCompileOptions { .., ..VerterCompileOptions::default() }` at `:98-100` and
`CodegenOptions { .., ..CodegenOptions::default() }` at `:272-274`. Its singular audit
target mirror `target_to_tag` at `:64-72` collapses every non-IDE, non-forced-Vapor
request to `CompileTargetTag::Vdom`.

**R5 — WASM compile/audit and virtual products** (`product-inventory.md:30`).
`crates/verter_wasm/src/lib.rs:384` (`get_virtual_file`), `:468-475` (`get_ide`),
`:522-533` (`ensure_ide_compiled`) each deserialize `FfiCompileProfile` and convert
through `ffi_profile_to_host` — `crates/verter_ffi/src/convert/input.rs:69-133`,
which **starts from `host::CompileProfile::default()`** at `:72`, so an absent field
is indistinguishable from an explicitly-defaulted one.
`FfiCompileProfile` at `crates/verter_protocol/src/types.rs:68-97` carries
`#[serde(rename_all = "camelCase")]` with **no `deny_unknown_fields`** — the same
file uses `deny_unknown_fields` deliberately on another struct at `:113`, so this is a
per-struct choice, not an absent convention. An unknown semantic option is therefore
dropped at deserialization, before any Rust code can refuse it. The string target
decoder `ffi_target_to_compile_target` (`pub(super)`,
`crates/verter_ffi/src/convert/input.rs:174-185`, called at `:122`) is already the
fail-closed shape B3 needs and is a pattern to reuse. `crates/verter_wasm/src/lib.rs:875`
(`compile_with_audit`) accepts only `target: &str` and no options at all. The
`@verter/wasm` package exposes a separate four-field `CodegenOptions`
(`packages/wasm/src/index.ts:6-15`) forwarded at `:248-256`.

**R6 — project-aware staged compile** (`product-inventory.md:34`, recorded as
"existing pieces, final sealed route later"). **The preparer could not identify a
production request-construction point for this family distinct from R2 and R3.** It
is therefore NOT asserted here as a separate conversion obligation. See OPEN
QUESTION Q-1.

**R7 — bundler / unplugin managed publication** (`product-inventory.md:35`).
`packages/unplugin/src/index.ts:974-987` builds the `HostCompileProfile` literal.
`packages/unplugin/src/core/types.ts:45-51` declares a public **open** dictionary
(`template?: { compilerOptions?: { isCustomElement?; [key: string]: unknown };
[key: string]: unknown }`) documented at `:45` as "Accepted for compatibility with
`@vitejs/plugin-vue` but currently only `isCustomElement` is forwarded."
**Verified stronger than the consults stated: nothing forwards it.** The only
occurrences of that option path anywhere in `packages/unplugin/src/` outside the spec
file are the type declaration itself (`core/types.ts:45,49`); the production profile
construction never reads `opts.template`. So the documented partial forwarding is
itself inaccurate — every key of the public dictionary, `isCustomElement` included, is
silently dropped. This is `B3.md:13`'s "No public option is silently ignored" failing
on a documented public surface, in the most literal way available. Under the ratified
boundary (§3.3) this is **not** B3's to close; it is registered as **JS-1** in §3.3.1
and dispositioned in §3.3.2.b as a defect — a loose Vue-inherited shape, not a Verter
public surface — that is **out of program scope**, owned by the maintainer personally
and gated on program completion.

### 3.3 The boundary of B3's obligation — RATIFIED

**Ratified boundary.** B3's option-conversion obligation **stops at the outermost Rust
ingress** — NAPI, the WASM host, FFI, the session profile, and the compiler. **B3 does
not extend into the `packages/unplugin` or `packages/wasm` public JavaScript/TypeScript
surfaces.**

The boundary follows the retained charter exclusion ("B3 does not own … route
exposure", `B3.md:18`) and the ratified inventory instruction that "NAPI, WASM,
bundler, and managed publication retain their later owners"
(`product-inventory.md:39-40`), neither of which this amendment changes. At every
outermost Rust ingress B3 must admit-or-refuse the complete classified option
inventory; above that line, B3 records rather than repairs.

**Binding condition on the ratification.** Every residual JavaScript/TypeScript
silent-ignore left above the Rust ingress must be assigned a **named later owner**.
None may be left unowned. §3.3.1 discharges that condition, and the condition is
**SATISFIED**: the register's only Class 1 residual, JS-1, is dispositioned as an
ordinary defect (§3.3.2.b), so no residual remains unowned or unrecorded.

### 3.3.1 Residual JS/TS silent-ignore register — discharge of the binding condition

Two classes must be kept apart, because only the first is a silent ignore:

- **Class 1 — silent ignore.** The public surface *accepts* the option and *drops* it.
  In scope of `B3.md:13` and AMD-005 `:143-144`.
- **Class 2 — non-exposure.** The public surface never accepts the option at all. Not
  an ignore; it is a route-exposure gap, and it is what "later route-owning blocks
  retain final NAPI, WASM, bundler, and managed-publication equivalence"
  (`charters/C4.md:10-12`) already covers.

The preparer swept every tracked JavaScript/TypeScript package that sits above a Rust
compile ingress. `packages/vite-plugin/` was excluded: `git ls-files
packages/vite-plugin/` returns nothing — it contains only untracked `dist/` and
`node_modules/` build residue and is not a live surface.

| # | Residual | Location | Class | Disposition / owner |
|---|---|---|---|---|
| JS-1 | `template.compilerOptions` — an **open** dictionary (`[key: string]: unknown` at `:50` and `:52`) accepting any official Vue parser/transform option, including `isCustomElement` (`:49`), documented at `:45` as "currently only `isCustomElement` is forwarded" | `packages/unplugin/src/core/types.ts:44-53`; never read anywhere in `packages/unplugin/src/` production source; the profile literal at `packages/unplugin/src/index.ts:974-987` does not consult it | **1** | **RESOLVED — DEFECT, but OUT OF PROGRAM SCOPE (§3.3.2.b).** Not a public surface awaiting a program owner: the open index signature is Vue's looser inherited shape, and **Verter is strict by default**. **Owner: the maintainer, personally. Resolution gate: after the architecture program completes.** The program performs no fix, no tightening, and no further investigation here. The doc comment at `:45` is itself false — nothing forwards it, `isCustomElement` included — and is part of the same maintainer-owned post-program work |
| JS-2 | `HostCompileProfile.strictSlots` absent from the TypeScript mirror while `FfiCompileProfile.strict_slots` (`crates/verter_protocol/src/types.rs:93`) and `NapiCompileProfile.strictSlots` (`crates/verter_napi/src/lib.rs:271`) both accept it | `packages/native/host-types.ts:114-147` | **2** | Not a silent ignore. Route-exposure gap, covered by `charters/C4.md:10-12`'s later NAPI equivalence owner |
| JS-3 | `@verter/wasm`'s four-field `CodegenOptions` (`filename`, `isProduction`, `componentId`, `includeTsx`), forwarded at `packages/wasm/src/index.ts:255-262` | `packages/wasm/src/index.ts:6-15` | **neither** | **The route is dead.** No Rust `compile`/`compileBytes` free export exists — `/usr/bin/grep -rn 'fn compile' crates/verter_wasm/src/` returns only `VerterHost::compile_with_audit` (`crates/verter_wasm/src/lib.rs:875`). `packages/wasm/src/index.ts:233` binds `wasm.compile`, which is `undefined`, so `dispatchCompile` throws at `:250-252`. Corroborated by the (untracked) built `packages/wasm/wasm/verter_wasm.d.ts`, whose only exports are `MetaProject`, `MetaSession`, `VerterHost`, `init`, `initSync`, `__wbg_init`. A dead route ignores nothing. Recorded as a separate finding, not a residual |
| JS-4 | `@verter/native`'s `HostCompileProfile` as a whole | `packages/native/host-types.ts:114`; `packages/native/index.ts` | **neither** | Pass-through declaration. No JS-side mutation: a grep for `delete `, `Object.assign`, profile spread, and `JSON.parse` in `packages/native/index.ts` returns nothing. Not a drop layer |

**Correction to a finding that is NOT a JS/TS residual.** `HostCompileProfile.ssrModuleId`
(`packages/native/host-types.ts:126`, set by unplugin at `packages/unplugin/src/index.ts:725,978`)
is honoured on the `compile_many` `runtime-render` lane — the batch render-profile struct
carries it at `crates/verter_napi/src/lib.rs:3220`, consumed at `:2437` — and **silently
dropped on the per-file lane**, because `NapiCompileProfile` (`crates/verter_napi/src/lib.rs:250-274`)
has no such field and `FfiCompileProfile` (`crates/verter_protocol/src/types.rs:68-97`)
has no `ssr_module_id`, while the session's `CompileProfile` does
(`crates/verter_session/src/types.rs:1337`). That drop happens **at the Rust ingress**,
so under the ratified boundary it is **B3's to fix**, not a JS/TS residual. It is a
route-asymmetric silent ignore of one public option and B3's bound charter must name it.

### 3.3.2 JS-1 — escalated as unowned, then CLOSED as out-of-program-scope

**Status: CLOSED by maintainer disposition, 2026-08-16 (Rulings 3 and 4; §9.1
decision 7). Owner: the maintainer, personally; gate: after the program completes.**
The escalation below is retained rather than deleted: it was correct on the evidence
available when it was made, and the record should show both why it was raised and why
it closed. What follows is the escalation as written, then the disposition that
supersedes it.

#### 3.3.2.a The escalation as raised

**The preparer could not identify a legitimate owner for JS-1 from the ratified
charters, and declined to invent one.**

Searched exhaustively on the prepared-against tree:

- `emitter-mapping-dispositions.tsv` has **zero** rows matching `napi`, `wasm`,
  `unplugin`, `packages/`, `verter_ffi`, or `verter_protocol`. The ledger does not
  reach any of these surfaces.
- The route inventory names the owner only as an unbound phrase: "later route-owning
  block after C4 core proof" (`product-inventory.md:35`) and "NAPI, WASM, bundler, and
  managed publication retain their later owners" (`:39-40`).
- `charters/C4.md:10-12` repeats the same unbound phrase: "Later route-owning blocks
  retain final NAPI, WASM, bundler, and managed-publication equivalence where those
  routes do not yet exist."
- Grepping every charter, contract, and `architecture.md` for `unplugin` / `bundler`
  yields exactly two hits — `charters/BV0A.md:83` (an incidental mention) and
  `charters/C4.md:11` (the unbound phrase above).
- No block in `program.md` claims the surface. The nearest candidates were checked and
  all fail: K2 (`program.md:413-417`) is framework-private carrier typing and `Any`
  removal; K3 (`:419-424`) is `VerterHost` decomposition; H3 (`:373-378`) is companion
  and `SourceProjectionMap` publication; E1 (`:289-294`) names "every NAPI/WASM/wire/
  cache/test route" but only for the TypeExpr/component-meta/graph/protocol closure,
  not compiler options; C4 (`:215-221`) is a *proof* block and `charters/C4.md:10`
  states outright that it "cannot repair framework semantics or create missing routes".

**Consequence as raised.** "Later route-owning block" is a placeholder that no ratified
document binds to a block identifier. Under the ratified boundary, B3 records JS-1 and
does not close it — but there appeared to be nobody to record it *to*. Naming a block
would have been an invention, which Ruling 2's condition expressly forbids. Three
options were put to the maintainer: (1) bind the placeholder by amending
`product-inventory.md:35,39-40` and `charters/C4.md:10-12`; (2) create a new
route-exposure block, a DAG change outside AMD-010; or (3) close JS-1 at the source by
narrowing the open index signatures at `packages/unplugin/src/core/types.ts:50,52`. The
preparer recommended option 3 but would not assume a public-API narrowing of a shipped
package.

#### 3.3.2.b The disposition that closes it

**The escalation's framing was wrong, and the maintainer corrected it: JS-1 is not a
public surface at all.**

> "verter is strict by default, `[key: string]: unknown` is the default vue less strict
> type. Verter needs to be strict, verter types are not too bad as they stand now, just
> a few bugs here and there."
> — maintainer, 2026-08-16

The open index signature is a **loose Vue-inherited shape**, not a Verter public option
surface. The escalation implicitly treated it as a deliberate public contract that
therefore needed an owner to honour or retire it. It is not one. It silently accepts and
drops options Verter never consumes, which under the project's standing rule makes it an
ordinary defect — a wrong result — to be fixed test-first by whoever owns it, never a
tracking, guard, or ownership mechanism. **Who owns it is settled separately, below: not
the program.**

**Disposition: OUT OF PROGRAM SCOPE — maintainer-owned, post-program.**

> "verter vue public types will be handled by me after the program is done."
> — maintainer, 2026-08-16 (Ruling 4, superseding Ruling 3's disposition)

Ruling 3's *substance* stands unchanged and is recorded above: the open index signature
is a defect, not a legitimate public surface. Its *disposition* changed. JS-1 is not
`ADOPT-NOW` program work; it is the maintainer's own post-program work.

- **Owner: the maintainer, personally.** This is a named owner, not a placeholder — it
  is the concrete difference from §3.3.2.a's unowned state.
- **Resolution gate: after the architecture program completes.**
- **The program performs NO fix, NO tightening, and NO further investigation** of
  Verter's Vue public TypeScript surface. A broader public-TS looseness sweep opened
  under Ruling 3 was stopped on this ruling and produced no adopted findings.
- **No new program block, and no DAG change.** §3.3.2.a's options 1 and 2 are not taken.
- **No amendment delta.** The ratified delta set is unchanged at three documents and
  four deltas (§5). Neither Ruling 3 nor Ruling 4 adds to it.
- **The program does not land it at all** — so it is not part of the AMD-010 landing
  unit, and it is not a separate unit in §7's sequence either. §7 governs only what the
  program lands.
- **It gates nothing** — not AMD-010's ratification, not B3's dispatch, not any ledger
  transition.
- **`emitter-mapping-dispositions.tsv` gains no row for it.** The zero-row gap that made
  the escalation look unownable (§3.7) is unaffected in either direction: it remains a
  real gap for the *other* ingress carriers and remains DEFERRED there.

**Ruling 2's binding condition is SATISFIED.** That condition required every residual
JS/TS silent-ignore to carry a named later owner or an explicitly escalated unowned
record. JS-1 was the only Class 1 residual in the §3.3.1 register, and it now carries a
named owner — the maintainer, post-program. **No residual in §3.3.1 is left unowned or
unrecorded**, and the amendment can and does now make the claim §3.3.2.a said it could
not.

#### 3.3.2.c Standing consequence for the rest of the program

**No program block may absorb Verter's public TypeScript type-tightening into its
scope.** Verter Vue public types are maintainer-owned post-program work. A block that
encounters public-TS looseness records it and moves on; it does not fix it, does not
tighten it, and does not open an investigation into it. This applies to B2, B3, and
every later block, and it is the reason JS-1 appears in this amendment as a register
entry rather than as work.

This bounds §3.3's ratified boundary from the other side. B3 stops at the outermost Rust
ingress because route exposure is not its scope (§3.3); it also may not reach *upward*
into public TypeScript surfaces on the theory that tightening them would close a
silent-ignore. Both directions are now closed.

### 3.4 Charter delta — `charters/B3.md:16-18`

Current text:

> K2 later owns transport carriers and mechanical conversion into this request. K2
> cannot reinterpret defaults, frameworks, capabilities, options, or products. B3 does
> not own parser recovery, framework lowering/codegen, publication, or route exposure.

replaced by:

> B3 owns option admission, unknown-option refusal, and conversion into this request
> at every production request-construction point reachable at its dispatch. A route
> that discards, defaults, or reinterprets a public option before the canonical
> constructor sees it is migrated in B3's accepted cutover; a transport struct that
> survives that cutover is a pure syntax/serialization decoder that applies no
> default, selects no product, interprets no capability, and forms no cache or
> semantic authority. B3's obligation terminates at the outermost Rust
> request-construction point of each route family; for each JavaScript/TypeScript
> package surface above such a point, B3 records one of complete forwarding, a named
> later owner with an acceptance ID, or an explicitly recorded unowned residual
> escalated to the maintainer — never silence. B3's bound charter enumerates
> the exact present route families it converts, and, per governance §12, the
> declaration/DTO deletion or explicit compatibility-retention set for each. K2 later
> owns the final typed framework-private carrier representation and removal of the
> residual `Any + Send + Sync` erasure; K2 performs no second semantic conversion and
> cannot reinterpret defaults, frameworks, capabilities, options, or products. B3 does
> not own parser recovery, framework lowering/codegen, publication, route exposure,
> route replacement, or cross-route equivalence proof.

### 3.5 AMD-005 delta — `AMD-005:129-130`

Current sentence (the last sentence of AMD-005 §5):

> K2 still owns transport carriers and their conversion into B3's request; it may not
> reinterpret framework semantics.

replaced by:

> K2 owns the final typed framework-private carrier representation and removal of the
> residual `Any + Send + Sync` erasure; it may not reinterpret framework semantics and
> performs no second semantic conversion. Conversion of the production routes
> reachable at B3's dispatch into B3's canonical request is B3's, per AMD-010 §3.

Superseding a sentence of a ratified amendment has direct precedent in this program:
[AMD-008 §4](AMD-008-bv0a-assembly-neutral-exit.md) `:404-454` supersedes enumerated
sentences of AMD-007, including three inside AMD-007's own recorded ratification.
This amendment supersedes exactly one AMD-005 sentence and enumerates it above rather
than leaving it to inference.

### 3.6 No DAG change is required, and K2 cannot move earlier

**No edge changes.** The correction moves an ownership boundary between two blocks
that already exist in the ratified graph at their existing positions. `B3` stays at
`program-dag.toml:99-103` with `predecessors = ["BV0", "BF3"]`; `K2` stays at
`program-dag.toml:321-325` with `predecessors = ["B6", "K1"]`.

**Moving K2 before B3 is impossible — it would create a cycle.** Verified directly
against `program-dag.toml`:

```
B3  (:99-103)
  -> B4          (:105-109, predecessors = ["B2", "B3"])
  -> BV1 (:111-115) / BS1 (:117-121), both predecessors = ["B4"]
  -> B5          (:123-127, predecessors = ["BV1", "BS1"])
  -> B6          (:129-133, predecessors = ["B5"])
  -> K2          (:321-325, predecessors = ["B6", "K1"])
```

K2 already transitively depends on B3 through six edges. Placing K2 before B3 makes
`B3 -> … -> K2 -> B3`. The second path through `K1` is equally blocked: `K1`
(`:315-319`) depends on `C4`, which depends on `B6` and `C3`, and `C2`
(`:141-145`) depends on `B3` — so the `K1` branch also runs through B3.

The ratified inventory already anticipates this split of ownership and needs no
edit: "B3 must enumerate every public/default route and map it to one canonical
request. Transport conversion may not reinterpret semantic defaults. C4 proves only
routes that exist by B6/C3; NAPI, WASM, bundler, and managed publication retain their
later owners." — `evidence/framework-conformance/product-inventory.md:37-40`.

### 3.7 Recommended breadth, and the broader alternative

The two consults proposed different breadths. The narrower is recommended; the
broader is recorded here so the maintainer can adopt it deliberately.

**RECOMMENDED (narrow) — three deltas across two documents:** §2.2
(`charters/B3.md:3`), §3.4 (`charters/B3.md:16-18`), §3.5 (`AMD-005:129-130`), plus
§4 (`charters/B2.md:15-17`) and, at the maintainer's option, §2.3
(`charters/B2.md:3`).

The primed consult additionally proposed amending
`evidence/framework-conformance/product-inventory.md:37-40` and
`program.md:413-417`. **The preparer verified both and found neither necessary:**

- `product-inventory.md:37-40` **already** states the split this amendment ratifies
  (quoted verbatim in §3.6). Amending it would restate, not change, ratified text.
- `program.md:413-417` is the K2 section. Read directly, `:417` says: "Keep shared
  axes truly shared; make framework-private requests typed; remove final
  `Any + Send + Sync` downcasts; keep direct carriers free of managed
  erasure/thread-safety costs." **It never mentions transport conversion into B3's
  request.** The conflicting sentence exists only at `AMD-005:129-130` and
  `charters/B3.md:16-17`, both of which §3.4/§3.5 correct. `program.md`'s K2 text is
  already consistent with the corrected boundary.

Also verified as over-broad: the primed consult cited the passage to amend as
`AMD-005:126-144`. Lines `132-144` are AMD-005 §6, whose no-second-authority and
no-silent-ignore requirements **both** consults say must be preserved. Only
`:129-130` conflicts. This amendment touches only `:129-130`.

**RATIFIED 2026-08-16: the narrow form.** The maintainer sustained both rejections and
the narrowing, expressly against the first consult's recommendation, and expressly
preserved AMD-005 §6 (`:132-144`).

**ALTERNATIVE (broader) — add ingress-carrier rows to the emitter/mapping ledger.**
The primed consult also proposed adding explicit dispositions to
`evidence/framework-conformance/emitter-mapping-dispositions.tsv` for the NAPI,
protocol/FFI, session-profile, WASM, and unplugin ingress carriers.

*Rationale for adopting it:* the ledger is the program's convergence and deletion
inventory, and a route family with no ledger row has no recorded required final
state, so a reviewer cannot check B3's cutover completeness against it. The gap is
real and verified: the ledger's only `carrier_compiler.rs` row is **EM-005**
(`emitter-mapping-dispositions.tsv:6`), scoped to
`{RuntimeOutputDescriptor, RuntimeCompileOutput, IdeOutput}` with acceptance owner
`B3+B4`; `crates/verter_napi/src/lib.rs`, `crates/verter_protocol/src/types.rs`,
`crates/verter_ffi/src/convert/input.rs`, `crates/verter_wasm/src/lib.rs`,
`crates/verter_session/src/types.rs` (`CompileProfile`), and `packages/unplugin/` have
no ingress-carrier row at all.

*Rationale for deferring it:* governance §12 (`../governance.md:315-332`) already
requires an accepted cutover to enumerate its deletion/retention set, and §3.4 above
directs B3's bound charter to do exactly that for these families. Adding ledger rows
is a charter-authoring and evidence action that does not need amendment authority,
and the amendment is narrower without it.

*Recommendation:* **defer** — B3's bound charter carries the enumeration.

**RATIFIED 2026-08-16: DEFERRED, not adopted.** The alternative remains recorded and
explicitly labelled so that it is not re-proposed as a new discovery.

The zero-row gap is what made JS-1 *look* unownable in §3.3.2.a, but that turned out to
be the wrong diagnosis — JS-1 closed as a defect, not as an ownership question
(§3.3.2.b), so it neither depends on nor motivates this alternative. The gap remains
real and DEFERRED for the *other* ingress carriers (NAPI, protocol/FFI, session profile,
WASM), and if a future amendment binds their route-exposure owners, adopting this
alternative in the same change is the natural vehicle.

---

## 4. B2 exit-criterion correction

### 4.1 The verified conflict

`charters/B2.md:15-17` requires "official syntax/error cases assigned to B2 to have
no blocked row in a supported parse capability". The preparer verified the assigned
set directly from the two manifests on the prepared-against tree:

| Manifest | Owner column | Value | Rows | Disposition |
|---|---|---|---|---|
| `evidence/framework-conformance/vue-official-cases.tsv` (2004 lines; `provisional_owner` = col 9) | `provisional_owner` | `B2/BV1` | **509** | all `blocked` |
| `evidence/framework-conformance/svelte-official-cases.tsv` (3476 lines; `provisional_owner` = col 7) | `provisional_owner` | `B2/BS1` | **590** | all `blocked` |

**Total 1099 rows, and every one is jointly owned.** (Re-verified against the
svelte@5.56.10-regenerated manifest — see `svelte-case-identity-ledger.md`; the
Svelte row/owner counts moved from the originally-prepared 3457/589/1098 to
3475/590/1099, Vue is unaffected.) The owner-column value distributions confirm
there is no B2-sole row anywhere: Vue is `{BV1: 1494, B2/BV1: 509}`; Svelte is
`{BS1: 2706, B2/BS1: 590, BF1: 150, BS1/B4: 29}`. All 1099 B2-assigned rows
currently read `blocked`.

The "supported parse capability" restriction is exactly two cells, both owned solely
by B2: `capability-matrix.tsv:2` (`VUE-PARSE-LOCAL`, `target_disposition = supported`)
and `:18` (`SVELTE-PARSE-LOCAL`, `supported`).

A row carries **one** aggregate disposition —
`contracts/conformance-goldens.md:7-15` ("Every applicable official case has exactly
one disposition", over `imported | equivalent | not_applicable |
unsupported_fail_closed | blocked`) — and its acceptance requires, per
`contracts/conformance-goldens.md:44-50`, "requested products, atomic publication,
fragment contract, assembled parse, real-package link, normalized structure,
helper/import/call topology, official-runtime execution, SSR, hydration, diagnostics,
mappings, TypeScript observations, route equivalence, zero unrequested work, and
locked performance gates."

B2 cannot produce most of that. Its own charter excludes it: "B2 does not own
framework code generation, normalized compiler options, product planning, atomic
publication, framework runtime semantics, or conformance corrections."
(`charters/B2.md:10-13`). And the co-owners cannot help in time: `program-dag.toml`
places `BV1` (`:111-115`) and `BS1` (`:117-121`) after `B4` (`:105-109`), which is
itself after B2. **B2's exit as written cannot be met by B2 at its DAG position, for
any of the 1099 rows.**

### 4.2 Charter delta — `charters/B2.md:15-17`

Current text (the exit sentence at `:15-17`):

> Exit requires official syntax/error cases assigned to B2 to have no blocked row in a
> supported parse capability, recovery diagnostics to be deterministic and mapped, and
> syntax rejection to be typed and non-publishing.

replaced by:

> Exit requires official syntax/error cases assigned to B2 to have no blocked **B2
> parse facet** in a supported parse capability, recovery diagnostics to be
> deterministic and mapped, and syntax rejection to be typed and non-publishing. A
> jointly-owned row's single aggregate disposition leaves `blocked` only once every
> applicable facet has been closed by its owner; B2 closes and evidences its parse
> facet and defers the remainder to the row's named co-owner.

The following sentence at `:17-18` ("B2 and B3 overlap only with a reviewed disjoint
writable-ownership proof.") is **unchanged**.

### 4.3 Facet vocabulary — normative definition

"Facet" does not currently appear in any normative Revision 11 document (verified by
full-tree grep of `docs/arch/refactor/rev11/`; the only hits are review transcripts
quoting `CLAUDE.md`). This amendment therefore defines it, closed and exhaustive over
the presently-assigned owner values:

- **B2 parse facet** — the case's parse, recovery, syntax-diagnostic ordering and
  span mapping, and syntax-rejection behaviour. Owner: `B2`.
- **Framework-train facet** — the case's lowering, codegen, runtime observation,
  mapping, product set, TypeScript observation, and route equivalence. Owner: `BV1`
  for `B2/BV1` rows, `BS1` for `B2/BS1` rows.

Each facet closes with its own recorded evidence. The row's single `disposition`
column is written by the last facet to close, and its value remains drawn only from
the closed vocabulary at `contracts/conformance-goldens.md:9-15`. **No new
disposition value is introduced, and the manifests' schema is not changed by this
amendment** — that choice fixes the recording mechanism to the existing `evidence_id`
column (§8, Q-2), whose per-facet record shape is settled by B2's bound charter.

Consequence, stated explicitly because it is easy to miss: since **no** B2-assigned
row is B2-sole (§4.1), under this correction B2 never closes an aggregate row by
itself. Its exit is entirely a facet exit. That is the intended result — it is what
makes the exit satisfiable at B2's DAG position — and it is why the sentence about
the aggregate row is retained rather than deleted.

---

## 5. Scope of amendment and supersession

This amendment supersedes exactly the following and no more, enumerated sentence by
sentence rather than summarized, because a boundary change that leaves a conflicting
ratified sentence standing has not actually changed the boundary.

1. **`charters/B3.md:3`** — the `**Predecessor:** BF3.` declaration, replaced per
   §2.2. This corrects the charter to the already-ratified DAG; it changes no edge.
2. **`charters/B3.md:16-18`** — the three-sentence ownership paragraph, replaced per
   §3.4.
3. **`AMD-005:129-130`** — the single sentence "K2 still owns transport carriers and
   their conversion into B3's request; it may not reinterpret framework semantics.",
   replaced per §3.5. **No other AMD-005 sentence is touched.**
4. **`charters/B2.md:15-17`** — the first clause of the exit sentence, replaced per
   §4.2.

That is the complete ratified set: **three documents, four deltas.** In particular
`charters/B2.md:3` is **NOT** amended (§2.3, §8 Q-4), and neither is `program.md`.

**Unchanged by Rulings 3 and 4.** The JS-1 disposition (§3.3.2.b) adds **no** delta to
this set: it is a defect in `packages/unplugin`'s public TypeScript surface, out of
program scope, owned by the maintainer personally and gated on program completion. It
amends no program document and the program does not land it. The set remains exactly
**three documents and four deltas**.

It does **NOT**:

- change any DAG edge, add or retire any block, or edit `program-dag.toml`;
- weaken [AMD-005 §6](AMD-005-framework-compiler-conformance-rescope.md) `:132-144`
  — the exactly-once option classification, unknown-option refusal, no-silent-ignore,
  and no-second-semantic-authority requirements remain in force verbatim, and §3.4 is
  written to make them reachable rather than to relax them;
- touch [AMD-005 §7](AMD-005-framework-compiler-conformance-rescope.md) `:146-157`,
  including the `SSR x Vapor` fail-closed lock, or any `capability-matrix.tsv` row,
  disposition, maturity, or acceptance owner;
- touch AMD-005's compatibility-domain locks (§2), architecture rules (§3),
  oracle/exclusion/golden rules (§8), conformance acceptance (§9), or performance lock
  (§13);
- reopen, amend, or affect AMD-001 through AMD-004, AMD-006, AMD-007, or AMD-008 in
  any respect — AMD-006's BV0 allocation, BF3 narrowing, and BV1 preservation
  requirement all stand unchanged, and §2 merely brings `charters/B3.md` into line
  with what AMD-006 already ratified;
- change `evidence/framework-conformance/product-inventory.md`,
  `emitter-mapping-dispositions.tsv`, `capability-matrix.tsv`, `vue-options.tsv`,
  `svelte-options.tsv`, or either official-case manifest — including their schemas,
  their row counts, and their `provisional_owner` values;
- introduce a new conformance disposition value, a new capability cell, a new
  acceptance ID, a new compatibility domain, or a new option classification;
- accept, dispatch, or bind any B2 or B3 charter. Both remain `LOCKED` with an empty
  `charter_digest` (`docs/arch/architecture-lock/ledger/program-state.toml:363-365`
  and `:384-386`), and both remain undispatchable until BV0 and BF3 are individually
  accepted per AMD-006 §8.1;
- authorize any production change. No implementation is approved by this amendment.

---

## 6. Informational record — B2/B3 execution serialization (NON-AMENDING)

**This section amends nothing.** It records a directive that follows from existing
ratified authority, so that it is not re-litigated at dispatch.

**The question.** May B2 and B3 execute concurrently on a range-level disjointness
proof over their shared files?

**Answer: in principle yes; on the available proof, no.**

Range-level disjointness *can* satisfy AMD-005 §4. The clause requires disjoint
"code, fixtures, manifests, generated artifacts, and shared lockfiles"
(`AMD-005:98-101`) — disjoint *code*, not disjoint *files* — and the stacking contract
expressly anticipates windows that declare shared files: "Each window declares shared
owners/files/generated artifacts and integration tests."
(`contracts/stacked-prs.md:62`).

**The available proof fails the bar.** It named two shared files — EM-007
`crates/verter_compiler/src/framework_common/vue_bridge.rs`
(`emitter-mapping-dispositions.tsv:8`, acceptance `B2+B3+BV1+B5`) and EM-020
`crates/verter_compiler/src/svelte/carrier.rs` (`:21`, acceptance `B2+B3+BS1+B5`) —
and **omitted a third: `crates/verter_compiler/src/framework_common/carrier_compiler.rs`**,
which both blocks change:

| Block | Symbol in `carrier_compiler.rs` | Line | Required change |
|---|---|---|---|
| B2 | `pub struct ParseOptions` | `:40` | replaced by framework-local syntax profiles |
| B2 | `CarrierCompiler::parse` | `:707` | made fallible (typed non-publishing rejection, `B2.md:16-17`) |
| B3 | `pub struct RuntimeCompileOptions` | `:348` | replaced by the canonical request |
| B3 | `framework_extras: Option<Arc<dyn Any + Send + Sync>>` | `:416` | deleted (`B3.md:14`) |
| B3 | `CarrierCompiler::compile_bundle` | `:754` | option type replaced |

**Stronger than a file overlap: it is one trait declaration.** `parse` and
`compile_bundle` are both members of `pub trait CarrierCompiler: Send + Sync` at
`carrier_compiler.rs:684-761`. B2 changes `parse`'s signature and B3 changes
`compile_bundle`'s parameter type — inside the same declaration block. A range-level
proof cannot make one trait signature disjoint, and the consult's own stated bar
excludes exactly this ("no shared declaration, `use` list, macro invocation, trait
signature, constructor, or formatting-sensitive aggregate").

**Why the proof missed it.** The EM ledger's only `carrier_compiler.rs` row is
EM-005 (`emitter-mapping-dispositions.tsv:6`), scoped to
`{RuntimeOutputDescriptor, RuntimeCompileOutput, IdeOutput}` with acceptance owner
`B3+B4`. B2's parse seam in that file has **no ledger row at all**, so a proof
derived from the ledger cannot see the collision.

**Directive: serialize. B2 lands first; B3 then rebases onto B2's accepted tree and
revalidates.** Authority: `AMD-005:101` ("Absence of that proof means
serialization.") and `contracts/stacked-prs.md:62` ("A newly discovered ownership
overlap stops or serializes the affected work.").

**This is execution serialization between siblings, NOT a new semantic DAG edge.**
`program-dag.toml:93-97` and `:99-103` keep B2 and B3 as siblings on
`predecessors = ["BV0", "BF3"]`, and `:105-109` keeps B4 gated on **both**. Nothing in
the DAG changes; a later reviewed disjointness proof that does cover
`carrier_compiler.rs` could restore concurrency without any amendment.

**Maintainer status (2026-08-16):** affirmed as a standing ruling requiring no
amendment. Recorded here so it is not re-litigated at B2 or B3 dispatch.

---

## 7. Landing sequence and commit embargo

**AMD-010 MUST NOT be committed until BF3 lands.**

`program/architecture-lock` is the program's single landing branch, and BF3 is
mid-flight against its current tip. Committing this amendment now would advance BF3's
base and destroy its linear fast-forward landing, which
`contracts/stacked-prs.md:140` treats as a re-freeze-and-re-review event rather than an
equivalent landing: "A validated `landing_equivalence_digest` proves that the canonical
binary Git delta from reviewed base to reviewed candidate exactly equals the delta from
accepted base to accepted commit … If the delta differs, re-freeze and re-review; do
not call it equivalent."

The sequence is therefore:

1. **BF3 lands** on `program/architecture-lock` as its own unit.
2. **AMD-010 lands next**, as its own squashed unit, rebased onto the post-BF3 tip.
   `contracts/stacked-prs.md:142` binds this: "A single accepted program block must not
   be co-batched with unrelated changes in the same landing delta."
3. **B2 dispatches**, then **B3 rebases onto B2's accepted tree** per §6.

**JS-1 is not in this sequence.** The program does not land it at all: it is
maintainer-owned post-program work (§3.3.2.b), so it is neither part of the AMD-010
landing unit nor a separate unit after BF3. An amendment document must not carry a
production TypeScript change in any case, and `contracts/stacked-prs.md:142`
independently forbids co-batching unrelated changes in one landing delta.

Consequences that must be observed while the embargo holds:

- The ratification recorded in §9.1 is a **decision**, not a landing. AMD-010 is
  ratified and unlanded, and both states are true simultaneously.
- This amendment's `**Prepared against:**` commit and tree will not be the landing base.
  Per §9 the landing bundle must record the actual base and either re-bind the reviews
  to the final commit/tree or record the diff and prove it contains only review-history
  and §9.1 records.
- Neither B2 nor B3 becomes dispatchable by this landing. Both remain `LOCKED` with an
  empty `charter_digest` (`docs/arch/architecture-lock/ledger/program-state.toml:363-365`,
  `:384-386`) and both continue to require BV0 and BF3 acceptance per AMD-006 §8.1.
- This section records a scheduling constraint set by the program orchestrator. It is
  not a maintainer architecture decision and amends nothing.

---

## 8. Open questions — dispositions

The seven questions raised in the pre-ratification draft are dispositioned below. Two
were decided by the maintainer's ruling; five are settled by the ratified documents and
the tree, with the verification recorded. **One new maintainer-reserved item was
created by discharging Ruling 2's binding condition** — JS-1's ownership, §3.3.2.
Nothing is left merely "open".

**Q-1 — R6 "project-aware staged compile" — RESOLVED by the tree and the inventory.**
No distinct production request-construction point exists. `/usr/bin/grep -rn
'staged\|StagedCompile' crates/verter_session/src/*.rs` yields no compile entry point
(two unrelated hits: a test-fixture doc string and a `ReturnOnly` comment in
`semantic_query.rs`). The inventory itself records the family as "existing pieces,
final sealed route later" with owner "C2/C3 then C4 proof"
(`product-inventory.md:34`), and C2 is the block that *builds* the sealed route
(`program.md:199-205`, "Staged compile transaction and concrete sealed facade"). R6's
"existing pieces" **are** R2's and R3's construction points, which B3 already converts.
**Disposition:** R6 adds no separate conversion obligation; C2 later builds the sealed
route against B3's already-canonical request. B3's bound charter records this rather
than listing R6 as an eighth site.

**Q-2 — facet evidence recording — RESOLVED by the ratification itself.** §4.3 as
ratified states that the manifests' schema is not changed, which selects option (a):
the existing `evidence_id` column (Vue col 11, Svelte col 9) points at a facet evidence
record under `evidence/framework-conformance/`. This is consistent with
`contracts/conformance-goldens.md:16-17`, which requires each row to record a singular
"evidence ID". **Disposition:** option (a); no schema change; no generator change to
`evidence/framework-conformance/generate-official-case-manifests.mjs`. The facet
record's exact shape is charter-authoring work for B2's bound charter, not a
maintainer decision. Option (b) — adding `facet_owner`/`facet_disposition` columns —
remains available to a future amendment but is not taken here.

**Q-3 — Svelte non-B2 rows — RESOLVED by direct column extraction.** The joint
distribution of `provisional_owner` × `disposition` over
`svelte-official-cases.tsv` is exactly:

```
590  B2/BS1   blocked
150  BF1      not_applicable
2706 BS1      blocked
29   BS1/B4   blocked
```

(Re-verified against the svelte@5.56.10-regenerated manifest; originally 589/144/2695/29
— see `svelte-case-identity-ledger.md`.)

The 150 `BF1` rows are **exactly** the 150 `not_applicable` rows — already
dispositioned under `contracts/conformance-goldens.md:11`, requiring no facet and no
further work. The 29 `BS1/B4` rows are `blocked` but their owner column contains no
`B2`, so B2's exit does not reach them. **Disposition:** neither set needs facet
treatment under AMD-010, which scopes the facet vocabulary (§4.3) to B2-assigned rows
only. Whether `BS1/B4` needs its own facet split is a question for BS1's and B4's
charters, not this amendment.

**Q-4 — sibling stale predecessor declarations — RESOLVED by the ratified delta
count: NOT ADOPTED.** The ruling ratifies "3 documents, 4 deltas", enumerated as
`charters/B3.md:3`, `charters/B3.md:16-18`, `AMD-005:129-130`, and
`charters/B2.md:15-17`. `charters/B2.md:3` is **not** among them, so §2.3's optional
correction is **not** adopted. **Disposition:** three prose predecessor statements
remain stale in the tree and are recorded here rather than hidden —
`charters/B2.md:3` (`Predecessor: BF3`), `program.md:150` and `program.md:158` (both
`Predecessors: B1`). All three are superseded in substance by `program-dag.toml`,
which §2.2's replacement text establishes as the sole predecessor authority, and by
AMD-006 §2 and §8.1. A future editorial amendment may align the prose; none is
required for correctness, because the authoritative source is machine-readable and
unambiguous.

**Q-5 — ledger ingress rows — RESOLVED by Ruling 1: DEFERRED.** Recorded as the
labelled broader alternative in §3.7; not adopted. See the §3.7 note on its
interaction with §3.3.2.

**Q-6 — the JS/TS boundary — RESOLVED by Ruling 2.** B3 stops at the outermost Rust
ingress; B3 does not extend into `packages/unplugin` or `packages/wasm`. Discharged in
§3.3, with the residual register at §3.3.1 and the escalation at §3.3.2.

**Q-7 — `SSR x Vapor` exit versus R1 file ownership — RESOLVED by the ratified
ledger.** `emitter-mapping-dispositions.tsv:10` (EM-009) assigns
`compile/types.rs::{CodegenOptions,VerterCompileOptions,CompileTarget}` — the option
and target authority — to **B3**; `:9` (EM-008, `compile/mod.rs`) and `:34` (EM-033,
`standalone.rs`) assign the orchestration and the route to **B5**. The division is
already recorded and needs no maintainer input. **Disposition:** B3 refuses the
`ssr + force_vapor` combination at request construction and replaces the option types;
the demotion at `crates/verter_compiler/src/compile/mod.rs:484,1211-1216` becomes
unreachable for that combination and B5 deletes the site later. B3 is not read as
owning `compile/mod.rs`, and B3's exit is met by the refusal, not by the deletion.

### 8.1 The JS-1 escalation — CLOSED

Discharging Ruling 2's binding condition created one new maintainer-reserved item:
**JS-1's ownership**. `packages/unplugin`'s open `template.compilerOptions` dictionary
accepts any official Vue compiler option and forwards none, and no ratified document
bound an owner — the emitter/mapping ledger has zero rows for any JS package or
transport crate; `product-inventory.md:35,39-40` and `charters/C4.md:10-12` both say
only "later route-owning block(s)"; and no `program.md` block claims the surface (K2,
K3, H3, E1, and C4 each checked and excluded in §3.3.2.a). The preparer escalated it
rather than naming a block, since no further investigation could settle an allocation
that the tree never made.

**CLOSED 2026-08-16 by maintainer disposition (Rulings 3 and 4; §9.1 decision 7).** The
escalation's premise was corrected rather than its question answered, in two steps.
Ruling 3 established the substance: the open index signature is Vue's looser inherited
shape, **not** a Verter public surface, and Verter is strict by default — so JS-1 is a
**defect**, not an ownership question. Ruling 4 then fixed its disposition: Verter's Vue
public types are **out of program scope**, owned by the maintainer personally and gated
on program completion. The program performs no fix, no tightening, and no further
investigation there, and does not land it. It requires no program block, no DAG change,
and no amendment delta, and it gates nothing. The full escalation is retained at
§3.3.2.a because it was correct on the evidence then available; §3.3.2.c records the
standing consequence for every later block.

**Nothing in this amendment now stands open.** All seven pre-ratification questions are
dispositioned above, Ruling 2's binding condition is satisfied (§3.3.2.b), and the
assertion §3.3.2.a said it could not make — that no residual JS/TS silent-ignore is
left unowned or unrecorded — the amendment now makes.

---

## 9. Exact ratification action

After independent architecture, conformance, and adversarial/governance review each
close `PASS` on one exact reviewed amendment-package commit and tree binding this
amendment's text and its charter deltas, the designated maintainer records:

> Ratify AMD-010 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`, tree
> `<bundle-tree-oid>`; correct `charters/B3.md`'s predecessor declaration to BV0 and
> BF3 per the already-ratified DAG, and record `program-dag.toml` as the sole
> predecessor authority; assign B3 bounded ownership of option admission,
> unknown-option refusal, and conversion into the canonical request at every
> production request-construction point reachable at its dispatch, bounded at the
> outermost Rust ingress of each route family per §3.3, with a named later owner for
> every residual JavaScript/TypeScript silent-ignore per §3.3.1 — the sole Class 1
> residual, JS-1, being out of program scope and owned by the maintainer personally
> after program completion, with no program block permitted to absorb public-TypeScript
> type-tightening into its scope; supersede exactly the
> one AMD-005 §5 sentence assigning that conversion to K2, leaving K2 the final typed
> framework-private carrier representation and `Any + Send + Sync` removal and no
> second semantic conversion; confirm AMD-005 §6 and §7 are unweakened and that no
> DAG edge, capability cell, option classification, conformance disposition value, or
> manifest schema changes; amend B2's exit criterion to require no blocked B2 parse
> facet, with a jointly-owned row's aggregate disposition leaving `blocked` only once
> every applicable facet closes; record the B2-then-B3 execution serialization of §6
> as a scheduling directive that creates no semantic DAG edge and leaves B4 gated on
> both; and authorize no B2 or B3 dispatch, which continues to require BV0 and BF3
> acceptance per AMD-006 §8.1.

The maintainer additionally records a decision on §2.3, on §3.7's broader alternative,
and on Q-1 through Q-7. All seven are dispositioned in §8, and the JS-1 escalation
raised while discharging Ruling 2's binding condition is closed at §8.1. **Nothing in
this amendment stands open.**

**The ratification bundle may not carry unreviewed bytes.** Either all reviews bind
the final ratification-bundle commit and tree directly, or the bundle's diff against
the reviewed package must be recorded and must contain ONLY the review-history and
§9.1 ratification records, leaving this amendment's text and its charter deltas
byte-identical. The recorded decision states which of the two applies and cites the
diff. Because of §7's commit embargo the landing base will not equal the
`**Prepared against:**` tip, so this record is mandatory rather than optional here.

This ratification action approves the AMENDMENT TEXT and its enumerated charter
deltas only. It accepts no B2 or B3 candidate, binds no charter, and authorizes no
production change. Any changed reviewed-package byte requires regenerated identities
and fresh reports. Silence, review, merge, or this proposal's commit is not
ratification. The preparer cannot ratify, review, or satisfy any independent mandate.

### 9.1 Recorded ratification

**RATIFIED.**

**Ratifier:** Carlos Rodrigues &lt;carlos@hypermob.co.uk&gt; (GitHub: `pikax`), the
designated maintainer of `docs/arch/refactor/rev11/governance.md` §1.1.
**Recorded:** 2026-08-16.
**Solicited by:** the program orchestrator.
**Ruling of record:** [`maintainer-ruling.md`](../evidence/AMD-010/maintainer-ruling.md).

**Governance basis.** Ratification is maintainer-reserved. `governance.md:12-19`
(§1.1 Maintainer): the maintainer "alone may … accept `A6`, architecture/ADR
amendments, formal rescopes, and gate recalibrations; approve irreversible public
compatibility/protocol decisions." `governance.md:285-299` (§10 Architecture
deviation): where source disproves a charter assumption, "The maintainer decides
whether to amend the ADR/architecture/charter, split the block, or abort." Both
consults independently cited these two clauses as reserving the decision; the
preparer verified both by direct read.

**Evidence path.** Three independent passes converged on the same diagnosis before any
decision was solicited:

| Pass | Nature | Finding |
| --- | --- | --- |
| 1 | Architecture consult, primed with the B3 scoping spec | `RESCOPE_REQUIRED`; proposed five amendment targets |
| 2 | Architecture consult, **run unprimed** — not told pass 1 existed | "Not executable as ratified"; proposed three amendment targets, independently including the predecessor defect |
| 3 | Verifying drafting pass (this document) | Re-derived every structural claim from the tree; sustained the narrow intersection, rejected two of pass 1's five targets and narrowed a third |

The consults were treated as evidence, not authority. Appendix A records the three
consult assertions that pass 3 verified and did **not** carry forward, and the one it
found understated.

**Decisions recorded.**

| # | Decision |
| --- | --- |
| 1 | **AMD-010 breadth: RATIFIED AS DRAFTED — the NARROW form.** Three documents, four deltas (§5). The rejections of `program.md:413-417` and `product-inventory.md:37-40` are sustained against pass 1's recommendation; the narrowing of `AMD-005:126-144` to `:129-130` is sustained; AMD-005 §6 (`:132-144`) is expressly preserved. No DAG change. |
| 2 | **Emitter-mapping ledger ingress rows: DEFERRED**, retained as the recorded, explicitly-labelled broader alternative (§3.7). Not adopted. |
| 3 | **Q-6 resolved: B3's conversion obligation stops at the OUTERMOST RUST INGRESS** — NAPI, WASM host, FFI, session profile, compiler. B3 does not extend into the `packages/unplugin` or `packages/wasm` public JavaScript/TypeScript surfaces (§3.3). |
| 4 | **Binding condition on decision 3:** every residual JS/TS silent-ignore is assigned a named later owner; none may be left unowned. Discharged at §3.3.1. The one Class 1 residual (JS-1) was first escalated as unowned at §3.3.2.a rather than papered over, then given its named owner by decision 7. **SATISFIED.** |
| 5 | **`charters/B2.md:3` not amended** — excluded from the ratified delta set (§2.3, §8 Q-4). |
| 6 | **B2/B3 serialization affirmed as a standing ruling** requiring no amendment (§6). |
| 7 | **JS-1 resolved as a DEFECT, and placed OUT OF PROGRAM SCOPE** (Rulings 3 then 4, same maintainer, same date; Ruling 4 supersedes Ruling 3's disposition while leaving its substance intact). *Substance:* Verter is strict by default, and `[key: string]: unknown` at `packages/unplugin/src/core/types.ts:44-53` is Vue's looser inherited shape, not a Verter public surface. *Disposition:* **owner = the maintainer personally; resolution gate = after the architecture program completes.** The program performs no fix, no tightening, and no further investigation of Verter's Vue public TypeScript surface, and does not land it. **No new program block, no DAG change, no amendment delta.** It gates neither this ratification, nor B3 dispatch, nor any ledger transition. Ruling 2's binding condition is SATISFIED — the named owner is the maintainer, post-program (§3.3.2.b) — and §8.1's escalation is CLOSED. |
| 8 | **Standing consequence (Ruling 4):** no program block may absorb Verter's public TypeScript type-tightening into its scope. Recorded at §3.3.2.c; binds B2, B3, and every later block. |

**Maintainer decision: RATIFIED**, in the narrow form, with the Q-6 boundary as stated
and its binding condition discharged as recorded. Landing is subject to §7's commit
embargo: this amendment is ratified and **unlanded**, and lands as its own squashed
unit only after BF3 lands.

**Limits of this ratification.** It approves the amendment text and its four enumerated
charter deltas. It accepts no B2 or B3 candidate, binds no charter, authorizes no
production change, and does not itself perform the maintainer-owned post-program JS-1
fix (§3.3.2.b, §8.1) — which the program does not land at all. B2 and B3 remain
`LOCKED` and undispatchable until BV0 and BF3 are individually accepted per AMD-006
§8.1. Any changed byte in the landing bundle beyond review-history and §9.1 records
requires regenerated identities and fresh reports. The preparer did not ratify,
review, or satisfy any independent mandate.

---

## Appendix A — Evidence provenance

This amendment answers two independent architecture consults that reached the same
diagnosis of B3's executability. The second was run unprimed — it was not told the
first existed — and independently identified both the predecessor defect and the
option-conversion contradiction.

| Consult | Scope | Location |
|---|---|---|
| Primed, B3 | `RESCOPE_REQUIRED`; five amendment targets | `scratchpad/rulings/B3-scope-ruling-codex-1.md` |
| Primed, B2 | Joint-case exit; concurrency; five further rulings | `scratchpad/rulings/B2-scope-and-concurrency-ruling-codex-1.md` |
| Unprimed, B3 | `No — not executable`; three amendment targets | `/tmp/b3-second-opinion-out.txt` |

The consults are **evidence, not authority**. Every structural claim carried into
this amendment was independently re-verified by the preparer against the
prepared-against tree. **The maintainer sustained all three of the following
non-carry-forward findings on 2026-08-16 (§9.1 decision 1), expressly against the
primed consult's recommendation.** The three assertions verified and **not** carried
forward:

1. **`program.md:413-417` needs amending to restrict K2** (primed consult). Verified
   false as a necessity: `program.md:417` never assigns transport conversion to K2;
   it already says only "make framework-private requests typed; remove final
   `Any + Send + Sync` downcasts". No amendment needed. (§3.7)
2. **`product-inventory.md:37-40` needs amending to distinguish request-construction
   from route-exposure ownership** (primed consult). Verified unnecessary: `:37-40`
   already states that split verbatim. (§3.6, §3.7)
3. **The conflicting AMD-005 passage is `:126-144`** (primed consult). Verified
   over-broad: `:132-144` is §6, whose requirements both consults preserve. Only
   `:129-130` conflicts. (§3.7)

One consult claim was verified and found **understated**: the unprimed consult
reported that unplugin "does not consume `opts.template` at all". Verified true, and
additionally the surface's own documentation at
`packages/unplugin/src/core/types.ts:45` claims "only `isCustomElement` is forwarded"
— which is itself inaccurate, since nothing forwards it. (§3.2 R7)

One consult citation was verified as pointing at a doc comment rather than the
declaration: the primed B2 consult cited `carrier_compiler.rs:701` for
`CarrierCompiler::parse` and `:340` for `RuntimeCompileOptions`; the declarations are
at `:707` and `:348`. The substance is unaffected; §6 cites the declarations.
