# CCA1O2J scope disposition: the shared refusal vocabulary and the browser binding (compiler.compiler-bridge)

- Status: proposed — awaiting operator/architect ratification
- Date: 2026-09-04
- Disposition: `ADOPT-NOW` for the crates and packages beyond CCA1O2J's
  named surfaces, recorded here because the review rounds found the expansion
  undispositioned, not because a fix owner may ratify their own rescope.
- Amends: nothing. No charter budget, no DAG edge, no other node's ledger row.
- Asks for: two operator decisions and one `DEFER` ruling (see the last two
  sections). The question a review round raised — whether this branch also
  lands `CCA1O3A` — is not one of them: that work has been reverted off the
  branch, so the candidate carries exactly one ledger transition, its own.
- Withdrawn: the diagnostic argument list and its browser half. That was the
  one item whose charter-acceptance argument this record itself refuted, and
  a later fix round REMOVED it from the branch rather than leaving an
  operator to ratify a wire expansion nothing produces. See "What was
  withdrawn".

## Why this record exists

Three review findings on the CCA1O2J candidate are the same finding: the
landed diff exceeds the charter's planning guidance and its mandatory
rescope thresholds, and no `ADOPT-NOW`/`DEFER`/`REJECT` disposition is
recorded anywhere. `CLAUDE.md`'s finding-disposition rule requires one before
related work continues, and the work packet's sizing policy requires a
scope-coherence explanation for material drift rather than a mechanical
split. This is that explanation. It is a record, not a ratification: the
candidate needs an operator to accept it.

## Measured footprint

Measured against the branch's merge base with `main` (`353a8ca04`) —
`git diff --numstat 353a8ca04 -- 'crates/*/src/*' 'packages/*/src/*'
'packages/native/*.ts' 'packages/wasm/*.ts'`, i.e. crate `src/` and
published TypeScript. An earlier revision of this record measured against
`f4d755241`, which is a commit ON this branch and not an ancestor of
`main`; that baseline hid `f4d755241`'s own contents from the measurement.
It is corrected here, and the work it contained no longer rides on this
branch (see "What was NOT adopted").

Production is +3863/−439 across 27 files in 5 crates and 2 packages:
`verter_compiler`, `verter_ffi`, `verter_napi`, `verter_session`,
`verter_wasm`, `packages/native`, `packages/wasm`. Seven of the 27 are test
artifacts the glob reaches — `*_tests.rs` modules that live under `src/`,
the fixture addon's `src/lib.rs`, `index.spec.ts` and
`host-types.test-d.ts`; counting only non-test files gives 20, +3640/−430.
The charter's guidance is ~500 LOC / 5 files /
2 related crates-or-packages; its mandatory rescope thresholds are 1500 LOC
/ 12 files / 3 unrelated packages. Production LOC and the file count still
breach; the unrelated-package count no longer does, because withdrawing the
diagnostic argument list took `verter_protocol` out of the diff entirely.

The charter's own budget line already anticipates part of this: "rescope only
under the program's mandatory thresholds **or when a consumer migration or
the browser binding enters**". The browser binding entered. That makes the
expansion a foreseen trigger rather than an unplanned overrun, but it does
not make it self-ratifying, which is why this record exists.

## What went beyond the named surfaces, and why each

The charter names `crates/verter_napi/src/lib.rs`,
`crates/verter_napi/src/host_compile_request.rs`, and
`packages/native/index.ts`.

1. `crates/verter_wasm/` and `packages/wasm/src/compile-request-types.ts`
   — the browser binding's share of the shared refusal vocabulary below,
   and the published note that the two bindings' response envelopes are not
   interchangeable. This is the charter's named "browser binding enters"
   trigger.

   (The diagnostic argument list that used to head this list, and the
   browser serde assertion that came with it, are no longer on the branch —
   see "What was withdrawn".)

2. `crates/verter_compiler/src/compile_request/{mod,vue,svelte,product,capability}.rs`
   — the option path a request-construction refusal names, and the refusal
   vocabulary itself.

   The charter's acceptance requires the refusal to "name the offending
   property where the schema names it". The refusal message is minted on the
   new routes; the property identity it needs is owned by the compiler.
   `FrameworkOption` now renders the HOST REQUEST's own field path
   (`VueOption::request_field` / `SvelteOption::request_field`), not the
   Rust variant spelling (`vue:transformOptionsHoistStatic` for the field
   `hoistStatic`) and not the official framework's option surface from
   `vue-options.tsv` / `svelte-options.tsv` (`vue:compatConfig.MODE` for
   the field `compatConfigMode`; and that inventory records `compatConfig`
   on two surfaces, which the request carries as the two distinct fields
   `compatConfig` and `transformCompatConfig`). The inventory remains the
   exhaustiveness proof it always was; it is simply a different namespace
   from the request object a caller writes.

   The same owner rule puts the refusal SENTENCE in `verter_compiler`
   (`Display for CompileRequestError`), with the wire spelling of every
   value it embeds owned by that value's own `Display`
   (`FrameworkOption`, `VueOnlyAxis`, `CapabilityCell`, `ProductKind`,
   `RuntimeStyleProcessing`), each backed by a single spelling accessor
   (`cell_id`, `wire_tag`, `wire_name`). Both bindings render that one
   vocabulary, so a refused request reads
   the same way natively and in the browser; the alternative — the native
   binding's own 11-arm renderer and 34-arm capability-cell table, with the
   browser binding still printing `{error:?}` — is precisely the fork the
   Shared Optimized Codebase rule forbids, and it is what this candidate
   originally landed.

3. `crates/verter_session/src/host_resolve/compile_request_build.rs` — one
   line, for the same reason as item 2.

   `request_construction_refused_diagnostics` rendered the same refusal
   with `{:?}`, publishing the Rust variant spelling into a user-visible
   host diagnostic (`MalformedOptionValue { option:
   Svelte(CompileOptionsCss), .. }`). It is the third render of that one
   refusal; leaving it would have kept a fork alive after removing two.
   The change is `{error:?}` to `{error}` plus the two assertions that
   quoted the variant spelling, which now quote the caller-facing sentence
   and additionally forbid the variant spelling.

4. `crates/verter_session/src/host_compile.rs` — the typed batch route's
   execution stage, moved onto the host batch coordinator.

   The charter's acceptance requires the batch route to "isolate a
   per-entry failure to that entry". The typed failure taxonomy already
   did; a per-entry PANIC did not. `compile_request_many` executed its
   inputs with a plain sequential iterator on the caller's thread and
   installed no catch boundary, so one input's codegen panic threw the
   whole call away — every sibling's compiled output, and every per-entry
   failure the binding had already recorded, with nothing naming which
   input did it. The profile-bearing `compile_many` route has isolated
   panics from the beginning and its README says so, so a caller moving
   onto the typed route silently lost the property.

   The fix routes the execution stage through the SAME
   `HostBatchCoordinator::run_batch` + `BatchPolicy::on_item_panic` seam
   `compile_many` uses, rather than adding a second bespoke
   `catch_unwind`: that file's own worker comment already states the catch
   is centralised at the coordinator so every batch client shares one
   coordination rule. It also puts the typed batch on the host-owned CPU
   pool, where the sequential map had made an N-file call cost the sum of
   N compiles on the calling thread — the JavaScript thread, on the native
   binding.

None of these is a legacy deletion, a second decode path, a profile
reconstruction, or a hand-written duplicate of a generated declaration — the
charter's four abort conditions all hold.

## What was withdrawn

The diagnostic argument list — `FfiDiagnosticArg` in
`crates/verter_protocol/src/types.rs`, its projection in
`crates/verter_ffi/src/convert/output.rs`, `NapiDiagnosticArg`,
`HostDiagnosticArg`, the `arguments` field on every published diagnostic,
and the browser serde assertion that proved it reached the wasm wire — is
no longer on this branch. A later fix round removed it.

Three things decided that, and none of them is that the work was wrong:

- Its charter-acceptance argument does not hold, as this record already
  said. `verter_parser::Diagnostic::with_argument` has no production call
  site, so every published diagnostic carried an empty list on every route.
- It changed the payload of the LEGACY profile-bearing routes too, because
  the per-diagnostic projection is shared. The charter says those routes
  "keep their existing declarations and behavior", so a field added to
  their published shape is a deviation from the charter's own text rather
  than only from its budget.
- A fix owner may not ratify their own rescope, and this was the one item
  that could be removed at no cost to the routes the node exists to add.
  Asking an operator to ratify a wire expansion for a capability nothing
  produces is a worse use of the ask than simply not making it.

What was withdrawn is a SHAPE, not a repair. The exactness half of it —
that a `u64` above 2^53 must not cross to JavaScript as a rounded double —
was correct and remains correct; it has nothing to round, because there is
no field. When a producer for diagnostic arguments lands, the field and its
decimal-digit encoding come back with it, in the node that owns the
producer. The retained part of the change is
`verter_ffi::convert::host_diagnostic_to_ffi`: one per-diagnostic
projection both bindings call, so severity spelling and UTF-16 span mapping
cannot fork between them. That is not a wire change.

The withdrawal initially missed two artifacts, and both are now cleaned. The
`.claude/skills/host-session/SKILL.md` bullet still described a shared
`host_diagnostic_arg_to_ffi` projection and a decimal-string integer
encoding for a field no producer, DTO or wire schema has; it is reduced to
the half that is real (one shared per-diagnostic projection, and the exact
five fields `FfiDiagnostic` carries). The `verter_language` dev-dependency on
`crates/verter_napi`, added for the withdrawn test that built a
`HostDiagnostic` with typed arguments, is dropped — the crate is named
nowhere in `verter_napi`, and nothing in this repo detects an unused
dependency edge.

## What was NOT adopted

No consumer migration rides on this branch, and the legacy profile-bearing
methods, their declarations, and their tests are untouched, as the charter
requires.

One did ride here and no longer does. `f4d755241` migrated
`packages/playground/scripts/capture-wasm-carrier-fixtures.mjs` off
`ensureIdeCompiled`/`getIde` onto the browser binding's `compileRequest`,
and flipped a SECOND node's ledger row (`CCA1O3A`) to implemented. Both are
excluded by this charter twice over — "Consumer migrations and every
legacy-profile deletion are excluded", and "add only CCA1O2J's ledger row" —
and the earlier revision of this record contradicted the tree by asserting
that no consumer migration had entered. A later commit on this branch
reverts the script to its `main` state and returns `CCA1O3A` to `pending`,
so the claim and the tree now agree: this candidate carries exactly one
ledger transition, its own. The migration itself is not rejected on its
merits; it belongs to its own node and its own review.

## Debt carried out of review

Every row the review rounds carried is closed by the landed candidate or
withdrawn with it, with its evidence named — except ONE, the two bindings'
divergent response envelopes, which is proposed as a `DEFER` and needs a
ruling. It is the last row of the table and it is restated as an operator
ask below.

An earlier revision of this section tabulated six of the eight rows then
tracked and declared them all closed; it also declared, wrongly, that
nothing was deferred. Both are corrected here.

| Row | Closed by |
| --- | --- |
| Batch `ideCompanion` responses must stay paired with their own entry's source — it is the only product whose payload (destructured-binding UTF-16 offsets) is computed FROM the source, so a mispairing publishes offsets into the wrong file silently rather than failing | `typed-batch-preserves-ide-utf16-offsets-per-entry`: two entries whose multi-byte prefixes differ, so each entry's offsets are wrong against the other's source |
| Published diagnostics must carry their argument list through serde, not just through the Rust DTO | Moot — the field is withdrawn (see "What was withdrawn"). It returns with the node that lands a producer for it |
| `docs/api/native.md` and `docs/api/wasm.md` must describe the published `arguments` field | Moot — withdrawn with the field; neither document describes a field the bindings do not publish |
| `publicApi` / `declarations` must refuse on the BATCH route too, isolated to its own entry beside a compiling sibling — not inherited from the singular route | `typed-batch-isolates-public-api-and-declarations-refusals` and `typed-single-refuses-public-api-and-declarations` |
| `runtimeServer` and `analysis` products must publish their payloads on BOTH routes | `typed-single-runtime-server-publishes-its-nodes`, `typed-single-vue-analysis-is-a-json-string`, `typed-batch-runs-analysis-and-runtime-server-products` |
| `Unsigned` / `Signed` diagnostic arguments must not silently round above 2^53 when crossing to JavaScript | Moot — withdrawn with the field. The requirement stands for whichever node lands the producer, and the decimal-digit encoding is the answer to it |
| Per-call budgets bound what a traversal retains NATIVELY, not the V8 handle scope it fills, and only the batch route opens a per-entry scope | `REJECT` — not a defect on the singular route. Within ONE graph the pinned set is bounded transitively: `materialize_nested` reserves an object's whole key count against `MAX_DECODED_VALUES_PER_REQUEST` BEFORE reading any of its keys, so a traversal cannot pin more than one key-list handle per visited object plus one property handle per reserved value, and a graph that would pin more is refused before it does. The bound does not compose ACROSS graphs, because the batch resets the decoded-value counter per entry — which is exactly what the per-entry scope is for. The module doc now states both halves rather than only that handles are "a separate resource with a separate bound" |
| `VALUE_REFUSED_*` option lists are hand-maintained mirrors whose only rail is a `verter_debug_assert!` inside `CompileRequestError::malformed_option_value`, which a direct struct literal bypasses entirely | `ADOPT-NOW`, closed structurally for the cross-crate half: `CompileRequestError::MalformedOptionValue` is `#[non_exhaustive]`, so outside `verter_compiler` the variant has no struct literal at all (E0639) and the constructor is the only way to produce one — sealed by the compile-fail contract `malformed_option_value_not_forgeable`. Inside the owning crate the assertion remains the rail; every current producer, in this crate and out of it, already routes through the constructor |
| The typed batch's aggregate retained-byte ceiling aborted the WHOLE call, discarding every sibling's decoded work and naming no input | `ADOPT-NOW`, closed: the ceiling now refuses PER ENTRY — the entry that crosses it and every entry after say so, naming their index and stating the ceiling is aggregate, while everything decoded before it still compiles and still answers. `typed-batch-attributes-the-aggregate-source-ceiling-per-entry`. What remains is the absence of a runtime override, which is operator decision 2 |
| A batch entry's own `canonicalId` / `source` / `request` were read with `Object::get`, a full `[[Get]]`, while the options object and the request graph were own-property-only | `ADOPT-NOW`, closed: `read_batch_entry_fields` enumerates the entry's own keys and reads each value through that list, so an entirely inherited entry states no field and fails as a missing one. `typed-batch-refuses-an-inherited-entry-wrapper` |
| The batch's id-position check failed the WHOLE call on any mismatch, resting on `resolve_alias_or_canonical` being idempotent — a property argued in prose and pinned by nothing | `ADOPT-NOW`, closed: a count mismatch still fails the call (it cannot be attributed to a position), but a position mismatch now fails only the entry it lands on. `typed-batch-accepts-a-registered-alias` exercises the alias map, which is the input whose id genuinely needs two resolutions to agree |
| Same-canonical concurrent compiles became newly reachable when the typed batch moved onto the CPU pool, on one non-repeated case | `ADOPT-NOW`, closed: `typed-batch-repeats-a-shared-canonical-under-concurrency` runs four entries over ONE canonical with two different requests, twelve times, and pins each entry to its own requested product and to byte-stable output |
| `compileRequest` exists on both bindings for one request schema, but their JavaScript response envelopes are not interchangeable: native nests the IDE payload under `ide`, stringifies `analysis` and throws a structured `Error`; the browser binding flattens the IDE DTO, returns `analysis` as an object and throws a string | Proposed `DEFER` — awaiting a ruling; see operator ask 3. The divergence PREDATES this node (the browser envelope is unchanged here) and converging it changes a published response shape, which is a consumer migration this charter excludes outright. It is documented on both sides (`docs/api/native.md`, `docs/api/wasm.md`, `packages/wasm/src/compile-request-types.ts`) so no consumer meets it unwarned |
| The typed batch's aggregate retained-byte ceiling latched on a SINGLE payload larger than the whole ceiling, refusing every later sibling with a ceiling the call had not reached and advising "compile fewer inputs per call" — the one remedy that cannot help one large file | `ADOPT-NOW`, closed: `JsValueMaterializationBudget::retain_bytes` now separates "this charge could not fit an empty budget either" (refused by its own size, aggregate counter untouched, every sibling still decodes) from "the call ran out of room" (the only state that latches). The aggregate refusal also names the bytes the call actually holds rather than the ceiling it did not reach. `typed-batch-isolates-a-single-source-above-the-whole-ceiling` beside the cumulative `typed-batch-attributes-the-aggregate-source-ceiling-per-entry` |
| The batch entry wrapper silently ignored an unknown own key while the batch options refused one by name and the request graph is `deny_unknown_fields` — three adjacent surfaces, two rules, so `{ canonicalId, source, requst }` reported a missing `request` and `{ …, request, requst }` compiled as though the stray key were absent | `ADOPT-NOW`, closed: `read_batch_entry_fields` refuses an unrecognised own key by name, and a key too long to be one of the three is refused by its MEASURED size (quoted only below the refusal-quoting bound, as the options object already does). `typed-batch-refuses-an-unknown-own-key-on-an-entry`; `docs/api/native.md` and `packages/native/README.md` state the closedness |
| `docs/api/native.md`'s new UTF-16 span sentence annotates the `HostDiagnostic` shape that `compileMany()` entry diagnostics ALSO publish, and that route projects with `source: None` — raw UTF-8 byte offsets, unchanged by this candidate | `ADOPT-NOW`, closed: the sentence now names `compileMany()` as the one exception, so the shared shape does not read as one coordinate space |
| `packages/native/index.spec.ts` landed a name-keyed source-text scanner over `dist/napi.generated.d.ts` / `index.ts` / `host-types.ts` for the typed routes' declared shapes | `ADOPT-NOW`, closed by DELETION. `CLAUDE.md`'s forward-only rule bans a landed guard keyed on spelled source names, and the structural rail for this surface already ships in this candidate: `packages/native/host-types.test-d.ts` asserts `Equal<typeof typedResponse, HostCompileResponse>` and `Equal<(typeof typedEntries)[number], HostCompileRequestsEntry>` against the real declared route signatures, the pre-existing prototype-reflection case (`every native prototype method should have a TS type declaration`) now lists both routes, and `real_js_host_request_boundary` calls both through the actual addon |
| `crates/verter_compiler/tests/cases/framework_option_wire_paths.rs` hand-parses `packages/native/host-compile-request.generated.ts`, keyed on `export interface` / `export type` and the two option-type names | `ADOPT-NOW`, retained under the cross-language-parity carve-out. The assertion is Rust enum arms against a GENERATED TypeScript declaration — the same class as the sanctioned `virtual_file_naming_ts_freshness` and `typeinfo_proto_ts_contract` — and no compiler, type-system, or tool-based rail spans the two languages, so there is no structural alternative to replace it with. It is recorded here rather than left undispositioned |

## Operator decisions this record asks for

1. **The expansion itself.** `ADOPT-NOW` for the crates beyond the
   charter's named surfaces, on the merits above. Every one of the four
   remaining items is required by an acceptance line of this charter: item
   1 by its named "browser binding enters" trigger, items 2 and 3 by
   "the refusal names the offending property where the schema names it",
   item 4 by "isolates a per-entry failure to that entry". The one item
   that was NOT so required has been withdrawn rather than put to this
   decision. The ask is therefore whether the charter's own acceptance
   lines justify exceeding its LOC and file guidance — not whether to
   admit unrelated work.
2. **The typed batch route's fixed 64 MiB aggregate retained-byte ceiling
   has no runtime override.** The ceiling itself no longer aborts the call
   or hides which input crossed it: the entry that exhausts it and every
   entry after it fail as `binding`, naming their index and stating the
   ceiling is aggregate, while everything decoded before it still compiles.
   What is left to accept is that the ceiling is a compile-time constant —
   a whole-project batch of average-sized SFCs reaches it well before the
   65 536-entry outer cap, and the caller's recourse is to resume from the
   index the refusals name. Making it a `HostConfig` field is a
   `verter_session` change this node does not make.
3. **A `DEFER` ruling for the two bindings' divergent response
   envelopes** (last row of the debt table). `CLAUDE.md` requires a
   codex-`DEFER` ruling plus a debt row naming the durable owner, the
   resolution gate, and the acceptance ID; a fix owner can propose the row
   but cannot issue the ruling. Proposed owner: whichever node first
   migrates a consumer onto BOTH bindings' `compileRequest` — that is the
   consumer whose code the divergence actually costs, and the node that
   can converge the envelope and migrate its caller in one change.
   Proposed resolution gate: no later than the close of this plan.
   `REJECT` is also available and is a defensible answer: the divergence is
   documented on both sides and no consumer targets both today.

## If ratification is refused

Nothing separable is left to remove. The one item whose charter-acceptance
argument did not hold is already withdrawn. Items 1 through 4 each answer a
named acceptance line of this charter, so refusing them means the routes
this node exists to add cannot meet their own acceptance in this node —
which is a re-scope of CCA1O2J itself, not a trim of this candidate.
