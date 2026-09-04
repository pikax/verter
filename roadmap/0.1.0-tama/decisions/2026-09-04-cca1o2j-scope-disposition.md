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

   The charter requires the new routes' returned value to preserve
   "diagnostics ... and public span and offset encoding". A diagnostic's
   `arguments` are the values its message is rendered from, and the FFI
   diagnostic DTO did not carry them, so the typed route could not preserve
   them. The in-charter alternative — projecting the arguments privately
   inside `verter_napi` — is forbidden by the Shared Optimized Codebase rule
   in `CLAUDE.md`: a reusable projection lands in the lowest owner crate that
   serves every consumer, and a consumer-local wrapper does not fork it. So
   the conversion is `verter_ffi::convert::host_diagnostic_arg_to_ffi` over a
   `verter_protocol` DTO, and the browser binding gets the same list from the
   same code. Complying with the charter's diagnostic-preservation
   requirement and with the shared-codebase rule at the same time requires
   exactly these two files.

2. `crates/verter_wasm/` and `packages/wasm/src/compile-request-types.ts` —
   the browser binding's share of that projection, and the test that proves
   its serde wire actually carries the field. This is the charter's named
   "browser binding enters" trigger.

3. `crates/verter_compiler/src/compile_request/{mod,vue,svelte}.rs` — the
   option path a request-construction refusal names.

   The charter's acceptance requires the refusal to "name the offending
   property where the schema names it". The refusal message is minted on the
   new routes; the option identity it needs is owned by the compiler's option
   inventory. `FrameworkOption` now reads the committed
   `vue-options.tsv`/`svelte-options.tsv` row for its path instead of
   case-lowering a Rust variant spelling, which named request fields that do
   not exist (`vue:transformOptionsHoistStatic` for the field `hoistStatic`).
   The inventory is the schema; naming the property "where the schema names
   it" cannot be done from `verter_napi`.

None of these is a legacy deletion, a second decode path, a profile
reconstruction, or a hand-written duplicate of a generated declaration — the
charter's four abort conditions all hold.

## What was NOT adopted

No consumer migration entered. The legacy profile-bearing methods, their
declarations, and their tests are untouched, as the charter requires.

## If ratification is refused

The separable piece is the diagnostic argument list plus its browser half
(items 1 and 2): moving it to its own node leaves the typed routes callable
but publishing diagnostics without their arguments, which fails the
charter's diagnostic-preservation acceptance until that node lands. Item 3
is not separable from the charter's refusal-naming acceptance.
