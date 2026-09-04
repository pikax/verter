# CCA1O2J scope disposition: shared diagnostic projection and the browser binding (compiler.compiler-bridge)

- Status: proposed — awaiting operator/architect ratification
- Date: 2026-09-04
- Disposition: `ADOPT-NOW` for the two crates and one package beyond CCA1O2J's
  named surfaces, recorded here because the review rounds found the expansion
  undispositioned, not because a fix owner may ratify their own rescope.
- Amends: nothing. No charter budget, no DAG edge, no other node's ledger row.

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

Against merge base `f4d755241`, the candidate is +4104/−170 across 23 files.
Production (crate `src/`, published TypeScript) is +2394/−148 across 14 files
in 5 crates and 2 packages. The charter's guidance is ~500 LOC / 5 files / 2
related crates-or-packages; its mandatory rescope thresholds are 1500 LOC /
12 files / 3 unrelated packages. Production LOC and the unrelated-package
count both breach; the file count does not.

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
   (`Display for CompileRequestError`), with `ProductKind::wire_tag`,
   `CapabilityCell::cell_id` and `RuntimeStyleProcessing::wire_name` beside
   it. Both bindings render that one vocabulary, so a refused request reads
   the same way natively and in the browser; the alternative — the native
   binding's own 11-arm renderer and 34-arm capability-cell table, with the
   browser binding still printing `{error:?}` — is precisely the fork the
   Shared Optimized Codebase rule forbids, and it is what this candidate
   originally landed.

None of these is a legacy deletion, a second decode path, a profile
reconstruction, or a hand-written duplicate of a generated declaration — the
charter's four abort conditions all hold.

## What was NOT adopted

No consumer migration entered. The legacy profile-bearing methods, their
declarations, and their tests are untouched, as the charter requires.

## Open items carried out of review

The earlier round recorded a five-item debt list. Four of those items are
closed by the landed candidate and are not carried forward: the public
declaration and `docs/api/native.md` describe both routes and their budgets;
a serde assertion proves the argument list reaches the browser wire; the
64-bit argument values cross as exact decimal digits; and the batch route
carries its own `publicApi`/`declarations` refusal-isolation case rather
than inheriting the singular route's.

One item remains, now narrowed: the batch route's `ideCompanion` product.
It is the only product whose payload is computed FROM the paired source, so
a zip that paired one entry's response with another's source would publish
UTF-16 offsets into the wrong file — silently. This round closes it with
`typed-batch-preserves-ide-utf16-offsets-per-entry`, two entries whose
multi-byte prefixes differ so each entry's offset is wrong against the
other's source. No debt is carried past this candidate.

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
