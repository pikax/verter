# Tracked debt — FC-OPTIONS-002: `VueOptionAttempt`/`SvelteOptionAttempt` unwired at production boundaries

**Status: CLOSED as of this record.** All three originally-unwired
construction sites now route through real per-field admission. See
"Closure evidence" below.

Ruling reference: `AMD-010-renumbered.md` §3.2, which names both of the
sites closed in this round (R4: `request_from_target`; R5:
`ffi_profile_to_host`) as B3's ratified obligation.

## What happened (original finding)

`VueOptionAttempt`/`SvelteOptionAttempt` (`crates/verter_compiler/src/compile_request/{vue,svelte}.rs`)
are the typed transport-decode admission surface for the full 118-row Vue
/ 35-row Svelte option classification (46 `supported canonical` + 18
`unsupported fail-closed` rows combined). Originally verified: NO
production caller constructed a request through either's `into_request()`
— every real route built `CompileRequest`/`VueCompileRequest`/
`SvelteCompileRequest` via direct struct-literal construction instead.

## Closure evidence

1. **Session route** (`crates/verter_session/src/host_resolve/
   compile_request_build.rs::build_compile_request`) — closed in the
   prior round. Routes every Vue/Svelte option `CompileProfile` carries
   through `VueOptionAttempt`/`SvelteOptionAttempt::into_request()`.

2. **R4 — the audited route** (`crates/verter_session/src/
   host_compile_audit.rs::request_from_target`) — closed this round.
   Now builds a `VueOptionAttempt` and calls `.into_request()?` instead
   of constructing `VueCompileRequest { ..Default::default() }` directly.
   `CompileAuditOverrides` has no field mapping onto any of the 12
   unsupported Vue slots today, so this can never actually refuse given
   current inputs — the fix is the structural admission gate, not a
   reachability argument, matching the same pattern used for R4's
   session-route sibling.

3. **R5 — the NAPI/WASM boundary** (`crates/verter_ffi/src/convert/
   input.rs::ffi_profile_to_host`) — closed this round via two
   complementary fixes, since `ffi_profile_to_host` produces a
   framework-neutral `CompileProfile` (not a `VueCompileRequest`/
   `SvelteCompileRequest`) and cannot know which framework's
   `*OptionAttempt` applies at this decode point:
   - `#[serde(deny_unknown_fields)]` added to `FfiCompileProfile`
     (`crates/verter_protocol/src/types.rs`) — an unrecognized wire KEY
     now refuses at deserialization (proven: `ffi_compile_profile_refuses_an_unrecognized_json_field`,
     verified discriminating by removing the attribute and confirming
     the test fails).
   - `ffi_profile_to_host` now EXHAUSTIVELY destructures
     `FfiCompileProfile` (no `..` rest pattern) instead of field-by-field
     access — a field added to the wire struct without a corresponding
     admission arm here is now a COMPILE ERROR, not a silently-dropped
     option.

## Residual

None identified. Every field `FfiCompileProfile`/`CompileAuditOverrides`
carry maps onto either a validated closed-string enum (`hmrStrategy`,
`target`, `requestedMode` — already refuse on an unrecognized value) or a
free-form value with no restricted domain (filenames, ids, booleans) —
there is no remaining option value silently accepted without admission at
either boundary. If a future field addition reintroduces this class of
gap, the compile-time exhaustiveness checks above are what will surface
it (a new `FfiCompileProfile` field breaks `ffi_profile_to_host`'s
destructuring; a new unsupported-slot-shaped field on
`CompileAuditOverrides` needs its own admission arm since
`CompileAuditOverrides` is NOT itself a `*OptionAttempt` type).
