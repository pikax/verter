# Tracked debt — FC-SVELTE-001: Svelte output liveness stays BS1's

Disposition: **DEFER** (per CLAUDE.md "Explicit finding disposition").

## What happened

B3's canonical `SvelteCompileRequest`/`SvelteOptionAttempt` represents and
normalizes the `supported canonical` rows of the BF1 inventory
(`svelte-options.tsv`) that a real `Verter` product exists for, and the
neutral carrier (`RuntimeCompileOptions`) threads the ones it consumes
through end to end instead of hardcoding `None`/`false` regardless of what
the caller supplied. Per the ratified scope ruling (item 5,
`B3-scope-ruling-codex-1.md`), B3's obligation stops at *representing and
normalizing* these options on the canonical request — making the codegen
output for each option's live/observably-different behavior correct is
BS1's, not B3's.

This is NOT a uniform "every field reaches `RuntimeCompileOptions`" claim —
see the per-option table below for the exact status of each row. Three
distinct outcomes exist, and no field claims a status stronger than what it
actually has:

1. **Live**: the field reaches `RuntimeCompileOptions` and the underlying
   Svelte backend already implements the option's observable behavior
   (`fragments`, `preserveWhitespace`, `preserveComments`,
   `discloseVersion`, `runes`, the `html`-only branch of `namespace`). B3's
   carrier fix makes that behavior newly *reachable* from every production
   route — a genuine bug fix (the wiring gap), not itself deferred scope.
2. **Reaches an existing typed refusal**: the field reaches
   `RuntimeCompileOptions` but the backend does not yet implement the
   option's live behavior (`dev`-mode codegen — currently fails closed via
   `UnsupportedSvelteRuntimeSurface::DevMode`). B3's fix makes the canonical
   request construct successfully and reach that EXISTING typed refusal at
   execution, before any codegen runs. This is deferred: B3 does not
   implement dev-mode codegen output or non-`html` namespace root emission.
3. **Refuses at construction, or is not represented at all**: `generate` /
   `experimental.async` refuse at `SvelteOptionAttempt::into_request`
   itself — they never reach `RuntimeCompileOptions` because the
   `SVELTE-MODULE` capability they gate is `unsupported fail-closed` (see
   the Round-3 update below); `customElement.props.*` has no
   `CompileProfile` producer at all, so it is not even settable yet. Both
   are B3-owned representation gaps in different senses — one closes
   cleanly (a typed refusal IS the correct representation for an
   unsupported-fail-closed capability), the other is a genuine open gap.

## Ruling reference

`B3-scope-ruling-codex-1.md`, additional ruling item 5: "DEFER Svelte
output liveness to BS1. B3 does not close it. ... Where honoring an option
requires unimplemented Svelte semantics, the canonical request constructs
successfully but execution fails before emission with a typed 'capability
not yet accepted' result. BS1 owns namespaces, runes/legacy, dev/prod,
whitespace/comments, custom elements, and output correctness."

## Owner

BS1 (per the ruling reference above).

## Acceptance ID

`FC-SVELTE-001` — this debt row's own id is the SAME acceptance id
`capability-matrix.tsv` already assigns to every BS1-owned Svelte
`RuntimeClient`/`RuntimeServer`/`SVELTE-CUSTOM-ELEMENT` row (see the
`acceptance_id` column: `SVELTE-CLIENT-RUNES`, `SVELTE-CLIENT-LEGACY`,
`SVELTE-SERVER-RUNES`, `SVELTE-SERVER-LEGACY`, `SVELTE-COMPONENT`,
`SVELTE-CUSTOM-ELEMENT`, `SVELTE-SEMANTIC-CORE` all carry it). This row
is not a separate, free-floating acceptance id — it is the same gate
those rows already close under.

## Resolution gate

Concrete, not open-ended: BS1's charter closes `FC-SVELTE-001` when EACH of
the following has live codegen (or, for item 3, a real Verter product)
replacing the current typed refusal / representation gap, with a
discriminating test proving the change:

1. `UnsupportedSvelteRuntimeSurface::DevMode` — `ModuleCompileOptions.dev`
   dev-mode codegen.
2. `NamespaceUnsupported` — `CompileOptions.namespace` for `svg`/`mathml`
   (non-`html` root emission).
3. The `SVELTE-MODULE` capability cell itself moving off
   `unsupported fail-closed` in `capability-matrix.tsv` — a real Verter
   product for the `ModuleJavaScript` output family. Only once that lands
   do `generate`/`experimental.async` have anything to represent again
   (B3's construction-time refusal for both is the CORRECT representation
   of an `unsupported fail-closed` capability, not itself a gap to close).
4. The remaining rows in the table below (`CompileOptions.css`,
   `SvelteOptions.customElement.{tag,shadow}`) either gain live codegen
   consuming the now-representable canonical-request field, or are
   explicitly re-scoped out of `FC-SVELTE-001` by a ruling.
5. `SvelteOptions.customElement.props.*` gains a `CompileProfile` producer
   (a genuine B3-owned representation gap, not yet closed — see Round-3
   update) AND live codegen.

Partial closure (some typed refusals replaced, not all) keeps this row
open with an updated table, not a premature close.

## Round-2 update (B3)

`CompileProfile` gained six new fields (`svelte_generate_module`,
`svelte_experimental_async`, `svelte_css`, `svelte_custom_element_tag`,
`svelte_custom_element_shadow`, `svelte_compatibility`) and
`build_compile_request` threaded all of them onto
`SvelteOptionAttempt`/`SvelteCompileRequest` the same way the round-1 pass
wired `svelte_runes`/`svelte_namespace`/etc — closing an earlier overclaim
(the debt row previously implied every supported-canonical option was
represented; four were not). `SvelteCompileRequest` already carried
`generate_module`/`experimental_async`/`custom_element_descriptor`/`css`
fields before this pass (a pre-existing compiler-crate capability B3 had
not yet found a `CompileProfile` producer for) — confirmed by direct read
of `crates/verter_compiler/src/compile_request/svelte.rs`.

At that point none of the four newly-threaded fields reached
`RuntimeCompileOptions` or any execution-time consumer: unlike `svelte_dev`
/`svelte_namespace`, which DO reach `RuntimeCompileOptions` and trip an
EXISTING typed refusal (`DevMode`/`NamespaceUnsupported`) at execution,
`generate_module`/`experimental_async`/`css`/the custom-element
descriptor's `tag`/`shadow` were read nowhere downstream of
`SvelteCompileRequest` — constructing a `CompileRequest`/`SvelteOptionAttempt`
with them set neither refused nor changed behavior; it was silently inert
past the request boundary. `generate_module`/`experimental_async` no
longer have this problem — see the Round-3 update immediately below. `css`
and the custom-element `tag`/`shadow` descriptor fields still do
(`detect_css_mode` in `svelte/runtime/parse_refusal.rs` still derives CSS
mode purely from source; `resolve_custom_element` in
`svelte/runtime/custom_element.rs` still hardcodes `tag: None, shadow:
Open` from a bare `bool`) — closing that remains BS1 scope per the ruling
reference.

## Round-3 update (B3)

Round-2's framing of `generate_module`/`experimental_async` as "wired to
the request; inert past it" undersold the actual defect: `SvelteOption::
class()` correctly classifies `ModuleGenerate`/`ModuleExperimentalAsync`
`SupportedCanonical` as OPTIONS (they are real, well-formed Svelte options),
but `SvelteOptionAttempt::into_request` was unconditionally ACCEPTING
them regardless of the fact that the `SVELTE-MODULE` capability they gate
(`capability-matrix.tsv:24`, `product_family: ModuleJavaScript`) is
`unsupported fail-closed` — the exact "unknown/unsupported option silently
accepted" pattern the whole `CompileRequest` cutover exists to close, not a
downstream carrier-wiring gap.

Fixed: `SvelteOptionAttempt::into_request` now refuses BOTH options with a
typed `CompileRequestError::UnsupportedOption { option, capability: Some
(CapabilityCell::SvelteModule) }` — reusing the SAME `UnsupportedOption`
shape and the previously-always-`None` `capability` field every other
unsupported-fail-closed Svelte option already refuses through (`loose`,
`accessors`, `immutable`, `compatibility.componentApi`, `hmr`,
`customElement.extend`), now with that field finally carrying a real
`Some(CapabilityCell)` value. `generate_module`/`experimental_async` are
REMOVED from `SvelteCompileRequest` (a valid constructed request can never
carry them, matching the doc comment's existing "no field for any
`unsupported fail-closed` row" rule) but remain settable on
`SvelteOptionAttempt`/`CompileProfile`, so a caller CAN still express the
intent and gets an explicit typed refusal rather than silent drop.

This resolves the earlier headline/table self-contradiction: the module
doc comment previously claimed `RuntimeCompileOptions` threads every
supported option "end to end" while the table said `generate`/
`experimental.async` were merely "wired to the request; not threaded past
it" — neither framing was correct. The accurate statement (now reflected
above and in the table) is per-field: some fields reach `RuntimeCompileOptions`
and are live or reach an execution-time refusal (BS1's remaining scope);
`generate`/`experimental.async` correctly refuse at CONSTRUCTION because
their capability is unsupported (closed, B3's fix); `customElement.props.*`
has no `CompileProfile` producer at all yet (still open, B3-owned).

The `capability_matrix_compile_request_coverage.rs` `SVELTE-MODULE` row
moved from an `Err(...)` exemption to a real construction/refusal probe
proving both refusals and the `capability` field's value; the pre-existing
`SVELTE-ASYNC-EXPERIMENTAL` row (a DIFFERENT capability cell —
`RuntimeClient+RuntimeServer` component-level async/boundary runtime
reachability, disposition `ExplicitOptIn`, unrelated to the
`ModuleCompileOptions`-surface `experimental.async` option despite the
name overlap) was incorrectly using the same `experimental_async` field to
claim a successful construction; that claim no longer holds now that the
field always refuses, so the row moved to an `Err(...)` exemption alongside
`SVELTE-HYDRATION`/`SVELTE-SEMANTIC-CORE` (runtime reachability, not a
`CompileRequest` construction option).

## Per-option disposition (svelte-options.tsv `supported canonical` rows)

| Option | Canonical request field | Carrier channel | Live today? |
|---|---|---|---|
| `ModuleCompileOptions.dev` | `RuntimeCompileOptions.svelte_dev` | wired (B3) | No — reaches the existing `UnsupportedSvelteRuntimeSurface::DevMode` typed refusal at execution (BS1 to implement) |
| `ModuleCompileOptions.generate` | none — refuses at `SvelteOptionAttempt::into_request` | n/a | No — `UnsupportedOption { capability: Some(SvelteModule) }` at construction (B3, round 3); correct until BS1 lands a real `ModuleJavaScript` product |
| `ModuleCompileOptions.experimental.async` | none — refuses at `SvelteOptionAttempt::into_request` | n/a | No — same `SvelteModule` refusal (B3, round 3) |
| `CompileOptions.customElement` | `RuntimeCompileOptions.custom_element` (pre-existing) | already wired | Partial — boolean policy live; source-authored descriptor fields below |
| `CompileOptions.namespace` | `RuntimeCompileOptions.svelte_namespace` | wired (B3) | Partial — `html` resolves normally; `svg`/`mathml` reach the existing `NamespaceUnsupported` typed refusal (BS1 to implement non-html root emission) |
| `CompileOptions.css` | `CompileProfile.svelte_css` → `SvelteCompileRequest.css` | wired to the request (B3, round 2); not threaded past it | No — `detect_css_mode` (`svelte/runtime/parse_refusal.rs`) still derives CSS mode purely from source, never consults the request field |
| `CompileOptions.preserveComments` | `RuntimeCompileOptions.svelte_preserve_comments` | wired (B3) | **Yes** — verified live via `svelte_carrier_runtime_compile_options_channel.rs` |
| `CompileOptions.preserveWhitespace` | `RuntimeCompileOptions.svelte_preserve_whitespace` | wired (B3) | Yes (already tested at the `SvelteRuntimeOptions` level; carrier gap was the only blocker) |
| `CompileOptions.fragments` | `RuntimeCompileOptions.svelte_fragments` | wired (B3) | **Yes** — verified live via `svelte_carrier_runtime_compile_options_channel.rs` |
| `CompileOptions.runes` | `RuntimeCompileOptions.svelte_runes` | wired (B3) | Yes — participates directly in the existing 3-tier runes/legacy mode resolution; session-level regression added round 2 (`session_profile_svelte_runes_reaches_the_compiled_main`) |
| `CompileOptions.discloseVersion` | `RuntimeCompileOptions.svelte_disclose_version` | wired (B3) | Yes (already tested at the `SvelteRuntimeOptions` level; carrier gap was the only blocker) |
| `CompileOptions.compatibility` | `CompileProfile.svelte_compatibility` → `SvelteCompileRequest.compatibility` (sealed/presence-only — its only sub-field, `componentApi`, is `unsupported fail-closed`) | wired to the request (B3, round 2); presence-only, no live sub-field to wire | No — no live sub-field exists to wire |
| `SvelteOptions.customElement.{tag,shadow}` | `CompileProfile.svelte_custom_element_{tag,shadow}` → `SvelteCompileRequest.custom_element_descriptor` | wired to the request (B3, round 2); not threaded past it | No — `resolve_custom_element` (`svelte/runtime/custom_element.rs`) still hardcodes `tag: None, shadow: Open` from a bare `bool`, never consults the descriptor |
| `SvelteOptions.customElement.props.*.{attribute,reflect,type}` | `SvelteCustomElementDescriptor.props` exists on the compiler-crate type but has no `CompileProfile` producer | not represented | No — per-prop descriptor map would need a new nested-map `CompileProfile` shape; left as residual, not attempted (open, B3-owned representation gap) |

Rows marked "wired to the request... not threaded past it" (`css`,
`customElement.{tag,shadow}`) are settable and admit construction, but
reaching live codegen (or, short of that, an execution-time typed refusal
the way `dev`/`namespace` already do) is unchanged BS1 scope.
`generate`/`experimental.async` are DIFFERENT: they are not "inert" — they
refuse outright, which is the correct terminal state for an
`unsupported fail-closed` capability, and B3 considers that row CLOSED.
`customElement.props.*` remains the one genuine B3-owned representation
gap — no `CompileProfile` producer exists for it at all.
