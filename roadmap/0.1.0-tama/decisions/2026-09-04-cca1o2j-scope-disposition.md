# CCA1O2J scope disposition: shared diagnostic projection and the browser binding (compiler.compiler-bridge)

- Status: proposed — awaiting operator/architect ratification
- Date: 2026-09-04
- Disposition: `ADOPT-NOW` for the crates and packages beyond CCA1O2J's
  named surfaces, recorded here because the review rounds found the expansion
  undispositioned, not because a fix owner may ratify their own rescope.
- Amends: nothing. No charter budget, no DAG edge, no other node's ledger row.
- Asks for: two operator decisions (see the last section). The third
  question a review round raised — whether this branch also lands `CCA1O3A`
  — is not one of them: that work has been reverted off the branch, so the
  candidate carries exactly one ledger transition, its own.

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
`git diff --numstat main`, restricted to crate `src/` and published
TypeScript. An earlier revision of this record measured against
`f4d755241`, which is a commit ON this branch and not an ancestor of
`main`; that baseline hid `f4d755241`'s own contents from the measurement.
It is corrected here, and the work it contained no longer rides on this
branch (see "What was NOT adopted").

Production is +3547/−402 across 23 files in 6 crates and 2 packages:
`verter_compiler`, `verter_ffi`, `verter_napi`, `verter_protocol`,
`verter_session`, `verter_wasm`, `packages/native`, `packages/wasm`. (Three
of the 23 are `*_tests.rs` modules that live under `src/`; counting only
non-test files gives 20.) The charter's guidance is ~500 LOC / 5 files /
2 related crates-or-packages; its mandatory rescope thresholds are 1500 LOC
/ 12 files / 3 unrelated packages. Production LOC, the file count and the
unrelated-package count all breach.

The charter's own budget line already anticipates part of this: "rescope only
under the program's mandatory thresholds **or when a consumer migration or
the browser binding enters**". The browser binding entered. That makes the
expansion a foreseen trigger rather than an unplanned overrun, but it does
not make it self-ratifying, which is why this record exists.

## What went beyond the named surfaces, and why each

The charter names `crates/verter_napi/src/lib.rs`,
`crates/verter_napi/src/host_compile_request.rs`, and
`packages/native/index.ts`.

1. `crates/verter_protocol/src/types.rs` and
   `crates/verter_ffi/src/convert/output.rs` — the diagnostic argument list.

   **The charter-acceptance argument for this item does not hold, and the
   operator should accept it on other grounds or refuse it.** An earlier
   draft of this record argued that the charter's "preserve diagnostics"
   acceptance could not be met without carrying `arguments` through the FFI
   DTO. That is not true today: `verter_parser::Diagnostic::with_argument`
   (`crates/verter_parser/src/diagnostics.rs`) has no production call site,
   every `Diagnostic::error`/`warning`/`info` constructor initialises
   `arguments` empty, and `HostDiagnostic.arguments` is a straight clone of
   that. Every production diagnostic on every route — legacy and typed,
   native and browser — publishes an empty list. The typed route could
   therefore have satisfied the charter's diagnostic-preservation acceptance
   without this DTO change, and every value-bearing assertion in the
   candidate constructs its argument by hand because no producer makes one.

   What the work IS: a forward-looking wire shape landed ahead of its
   producer, plus the exactness repair that goes with it (`Unsigned` /
   `Signed` arguments crossed both bindings as `f64`, silently rounding any
   value above 2^53; they now cross as exact decimal digits). Landing it
   here rather than in `verter_napi` follows the Shared Optimized Codebase
   rule in `CLAUDE.md` — a reusable projection belongs in the lowest owner
   crate that serves every consumer — so given that the shape lands at all,
   these are the right two files. Whether it should land in THIS node is the
   operator's call, and it is a wire-shape decision, not a charter
   requirement.

2. `crates/verter_wasm/` and `packages/wasm/src/compile-request-types.ts` —
   the browser binding's share of that projection, and the test that proves
   its serde wire actually carries the field. This is the charter's named
   "browser binding enters" trigger.

3. `crates/verter_compiler/src/compile_request/{mod,vue,svelte,product,capability}.rs`
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

4. `crates/verter_session/src/host_resolve/compile_request_build.rs` — one
   line, for the same reason as item 3.

   `request_construction_refused_diagnostics` rendered the same refusal
   with `{:?}`, publishing the Rust variant spelling into a user-visible
   host diagnostic (`MalformedOptionValue { option:
   Svelte(CompileOptionsCss), .. }`). It is the third render of that one
   refusal; leaving it would have kept a fork alive after removing two.
   The change is `{error:?}` to `{error}` plus the two assertions that
   quoted the variant spelling, which now quote the caller-facing sentence
   and additionally forbid the variant spelling.

None of these is a legacy deletion, a second decode path, a profile
reconstruction, or a hand-written duplicate of a generated declaration — the
charter's four abort conditions all hold.

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

## Debt carried out of review: all closed, none deferred

Every row the review rounds carried is closed by the landed candidate, with
its evidence named. None is deferred, so no `DEFER` ruling and no debt row
is owed, and nothing here needs an owner block or a resolution gate.

| Row | Closed by |
| --- | --- |
| Batch `ideCompanion` responses must stay paired with their own entry's source — it is the only product whose payload (destructured-binding UTF-16 offsets) is computed FROM the source, so a mispairing publishes offsets into the wrong file silently rather than failing | `typed-batch-preserves-ide-utf16-offsets-per-entry`: two entries whose multi-byte prefixes differ, so each entry's offsets are wrong against the other's source |
| Published diagnostics must carry their argument list through serde, not just through the Rust DTO | `published_diagnostics_carry_their_argument_list_through_serde`, plus the browser-side serde assertion proving the field reaches the wasm wire |
| `docs/api/native.md` and `docs/api/wasm.md` must describe the published `arguments` field | Both documents describe it, alongside both routes and their budgets |
| `publicApi` / `declarations` must refuse on the BATCH route too, isolated to its own entry beside a compiling sibling — not inherited from the singular route | `typed-batch-isolates-public-api-and-declarations-refusals` and `typed-single-refuses-public-api-and-declarations` |
| `runtimeServer` and `analysis` products must publish their payloads on BOTH routes | `typed-single-runtime-server-publishes-its-nodes`, `typed-single-vue-analysis-is-a-json-string`, `typed-batch-runs-analysis-and-runtime-server-products` |
| `Unsigned` / `Signed` diagnostic arguments must not silently round above 2^53 when crossing to JavaScript | They cross as exact decimal STRINGS on both bindings, asserted on both |

## Operator decisions this record asks for

1. **The expansion itself.** `ADOPT-NOW` for the crates and package beyond
   the charter's named surfaces, on the merits above — noting that item 1's
   charter-acceptance argument does not hold and it should be accepted (or
   refused) as a forward-looking wire-shape decision.
2. **The typed batch route's fixed 64 MiB aggregate retained-byte
   ceiling.** Exceeding it aborts the WHOLE call, with no per-entry
   attribution and no runtime override; a whole-project batch of
   average-sized SFCs reaches it well before the 65 536-entry outer cap.
   The behaviour is documented in `docs/api/native.md` and beside the
   constant, so callers can chunk. Making the ceiling a `HostConfig` field
   is a `verter_session` change this node does not make.

## If ratification is refused

The separable piece is the diagnostic argument list plus its browser half
(items 1 and 2): it is the one whose charter-acceptance argument does not
hold, so moving it to its own node costs the typed routes nothing today —
every production diagnostic publishes an empty argument list either way.
Item 3 is not separable from the charter's refusal-naming acceptance.
