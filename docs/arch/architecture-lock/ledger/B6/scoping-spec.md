# B6 — implementation scoping spec

Written by the B6 block orchestrator before any implementer dispatch, per the
high-risk-block requirement in the orchestrator brief. This is the binding
implementation plan; the charter (`docs/arch/refactor/rev11/charters/B6.md`)
and AMD-011 remain the authority on INTENT — this spec is the authority on
MECHANISM. Every file:line citation below was read directly on this worktree
at base commit `e30b42ba1`. Re-verify each cited fact against the live file
before writing code — this spec is precise, not infallible; if a cited fact
is wrong, fix the plan and note the correction, do not silently code around
the discrepancy.

## 0. What already exists (do not re-derive, do not duplicate)

- `crates/verter_compiler/src/standalone.rs` (1220 lines) — B5's accepted
  direct core. `StandaloneCompiler::compile` (`:186`) dispatches to
  `compile_vue` (`:223`) or `compile_svelte` (`:409`). This file is FROZEN
  behavior (B5 ACCEPTED, byte-identity is load-bearing for later blocks).
  B6 EXTENDS it in place — same file, same module — never a parallel file
  that reimplements its logic.
- `crates/verter_compiler/src/compile/mod.rs`:
  - `parse_sfc(input: &str, delimiters: Option<(&str,&str)>, custom_elements: Option<&[String]>) -> ParsedSfc`
    (`:99`, `pub(crate)`, infallible). Its own doc comment (`:96-98`)
    already says: "The result can be cached and passed to
    `compile_from_parsed` to avoid re-parsing the same source." This is the
    sanctioned prepare/reuse seam — B6 does not invent a new one.
  - `compile_from_parsed(input, parsed: &ParsedSfc, request, execution_inputs, macro_semantics, allocator) -> Result<VerterCompileResult, CompileRequestError>`
    (`:628`, `pub(crate)`) — the reuse-a-parse compile entry. **Correction
    (post-recon):** already a production call site —
    `framework_common/vue_bridge.rs:498` (documented at `:204-205`) uses the
    same `parse_sfc` + `compile_from_parsed` split for the carrier path.
    B6 is not the first CALLER, but is the first caller from
    `standalone.rs`/the direct-compile-core surface. Two behavioral gaps
    vs `compile_with_parsed_impl` that do NOT matter for B6 (verified):
    `compile_from_parsed` always sets `parse_duration_ms = 0.0` and never
    calls `observer.record_phase_timing("compile.parse", ...)` — irrelevant,
    `DirectCompileOutput` carries no timing; and it has **no**
    `request.vue()`-mismatch `FrameworkMismatch` early return the way
    `compile_with_parsed_impl` does (`compile/mod.rs:583-590`) — calling it
    with a Svelte request PANICS inside `resolve_vue_backend`'s `.expect`
    (`compile_request/mod.rs:287`) rather than returning a typed error.
    **This is exactly why `compile_prepared`'s three-way framework check
    (§4 step 1) is not a nicety — it is the only thing standing between a
    caller bug and a panic**, since the code path it guards has no
    fallback check of its own.
  - `compile_with_parsed_impl` (`:576`) = `parse_sfc` + `resolve_vue_backend`
    + `derive_legacy_vue_options` + `compile_inner`. `compile_from_parsed`
    (`:628` on) is the SAME sequence minus the parse — confirmed by reading
    both bodies. This is why swapping one call for the other inside
    `compile_vue` is behavior-preserving as long as the `parsed` value was
    produced from the same source + the same `(delimiters, is_custom_element)`
    pair `compile_with_parsed_impl` would have used.
- `crates/verter_compiler/src/svelte/parser/tokenizer.rs:43` —
  `parse_svelte(source: &str) -> ParsedSvelte`, infallible, takes NO
  request-derived options. `compile_client` (`client_compile.rs:99`)
  already takes `parsed: &ParsedSvelte` as a separate argument from
  `source` — Svelte's reuse seam is already fully mechanical, no
  extraction needed.
- `crates/verter_session/src/host_compile.rs` — an UNRELATED, pre-existing
  HOST-BACKED batch (`CompileBatchInput`/`CompileBatchOptions`/
  `CompileManyTarget`, `:56`-`:330`). It goes through `VerterHost`, the VFS,
  and the scheduler CPU pool. B6's batch route is NOT this — B6 operates
  only on `StandaloneCompiler`, no host, no VFS, no scheduler dependency.
  Borrow ONE convention from it: `crates/verter_session/tests/cases/
  architecture_guards.rs:16107` guards `CompileBatchOptions` against ever
  growing a per-call thread-count field. B6's new batch type(s) must not
  add one either — do not add a `BatchOptions` struct with any concurrency
  knob. If a bounded internal parallelism is genuinely wanted later, that
  is out of scope for this landing; ship the sequential version.
- `crates/verter_identity/src/encoding.rs:163` — `CanonicalEncoder` already
  wraps `blake3::hash`. `blake3` is a workspace dependency (`Cargo.toml:24`,
  `blake3 = "1.5"`) but is NOT yet a `verter_compiler` dependency — add
  `blake3 = { workspace = true }` to `crates/verter_compiler/Cargo.toml`.
  Do not pull in `verter_identity` for this — a raw `blake3::hash(bytes)`
  call is enough; `CanonicalEncoder`'s domain-tag ceremony is for a
  different purpose (canonical multi-field encoding) and is not needed for
  a plain byte-digest binding check.
- `crates/verter_bench/benches/fixtures/*.vue` — 6 independently-authored
  local Vue fixtures (`simple.vue`, `medium.vue`, `large.vue`,
  `kitchen-sink.vue`, `composition-heavy.vue`, `template-heavy.vue`).
  `crates/verter_svelte_conformance/corpus/fixtures/*.svelte` — many
  independently-authored local Svelte fixtures. Use these two directories
  as the perf-cell corpus source; do NOT touch `.integration-tests/repos`
  (third-party, excluded by policy) and do not author a large new fixture
  set when small existing ones already cover Vue VDOM/Vapor-shaped and
  Svelte-shaped inputs — pick ~5-8 total from what already exists, small
  enough that a corpus run completes quickly under the bounded machine
  protocol.

## 1. Scope boundary (what B6 is and is not)

B6 adds FOUR new entry points to `StandaloneCompiler`, all living in
`standalone.rs`:

1. `prepare(&self, source: &str, request: &CompileRequest) -> PreparedCarrier`
2. `prepare_owned(&self, source: String, request: &CompileRequest) -> PreparedCarrier`
3. `compile_prepared(&self, source: &str, prepared: &PreparedCarrier, request: &CompileRequest, inputs: DirectExecutionInputs) -> Result<DirectCompileOutput, DirectCompileError>`
4. `compile_batch(&self, items: &[BatchCompileItem]) -> BatchCompileOutput`

**Correction (post-review).** This section originally locked THREE entry points and
omitted `prepare_owned`, which contradicted the program's own owned-scope line for
this block: "Add explicit borrowed/owned preparation…" (`program.md`, the B6 owned
scope. The program is the higher authority, so the omission is corrected here rather
than the entry point being removed.

Borrowed and owned preparation are two explicit choices by design. `prepare_owned`
takes `String` BY VALUE and moves it into the carrier — it does not copy, which is
the point: a caller that already owns its source hands ownership over instead of
paying for a second allocation. The carrier then reports that capacity in its
retained weight, so the cost of retaining is visible rather than hidden. An earlier
revision of this correction described the parameter as `&str` and said the carrier
"retains a copy"; both were wrong about the implementation and are fixed above.

No semantic projection, no TypeInfo, no cross-file resolution — B6 never
reaches C2's `plan`/`project` stages. `prepare` is a parse-only step over a
SINGLE already-in-hand source string. This is the ENTIRE scope; anything
resembling caching across `StandaloneCompiler::compile_batch` calls, a
persistent prepared-carrier pool, or file-system/VFS awareness is OUT OF
SCOPE — do not add it.

### Where the "can this route produce X?" answer lives

Two different questions were being conflated, and they have different owners.

**Svelte's server surface is a framework-semantic answer, and the runtime owns it.**
`refuse_unproducible_runtime_surface` is the single authority. The preflight consults it
to refuse before parsing, the compile loop derives its admitted kinds from it, and the
refusal a caller sees is the runtime's own typed error rather than a reconstruction. One
edit to that function moves all three. This block owns WHEN the question is asked, never
the answer.

**Vue's producible product set is this route's own catalogue, and it stays here.** It is
not a Vue language rule: it states which artifact kinds this direct core emits, which is
lifecycle, not framework semantics. It cannot be derived from the compile path, because
the whole purpose of the preflight is to answer before anything is parsed. So it is
declared once, in `VUE_PRODUCIBLE_KINDS`, and bound in BOTH directions by
`vue_route_produces_exactly_the_kinds_it_declares_producible`: a kind in the list that
the route does not actually publish fails, and a kind absent from the list that the route
would silently accept fails. The test states its expected set independently of the
constant on purpose — an oracle that read the constant would move with it and could not
see it change.

That two-sided binding is the correction. The earlier refusal tests only ever named kinds
already absent from the list, so removing a real kind was caught incidentally by the
corpus while ADDING an unproducible one was invisible: the preflight admitted it and the
compile failed later with a different error nothing asserted.

## 2. `PreparedCarrier` (new code, appended to `standalone.rs`)

```rust
pub struct VuePreparedCarrier {
    parsed: ParsedSfc,
    source_digest: [u8; 32],
    parse_identity_digest: [u8; 32],
}
pub struct SveltePreparedCarrier {
    parsed: crate::svelte::parser::ParsedSvelte, // verify exact import path
    source_digest: [u8; 32],
}
pub enum PreparedCarrier {
    Vue(VuePreparedCarrier),
    Svelte(SveltePreparedCarrier),
}
```

- `source_digest = blake3::hash(source.as_bytes())` (as `[u8; 32]`, via
  `.into()` / `*hash.as_bytes()`).
- `parse_identity_digest` (Vue only — Svelte's `parse_svelte` takes no
  options, so it needs no second digest; CONFIRM this remains true by
  re-reading `parse_svelte`'s signature before coding) is computed over
  exactly the two fields `compile_with_parsed_impl` (`compile/mod.rs:592-598`)
  feeds to `parse_sfc`: the `vue.delimiters` pair and `vue.is_custom_element`
  slice, taken from `request.vue()`. Feed both into one `blake3::Hasher`
  in a fixed, documented order (e.g. delimiters open/close bytes with a
  length-prefixed or NUL-separated encoding to avoid ambiguity, then each
  `is_custom_element` entry length-prefixed) — do not rely on `Debug`
  formatting for a hash input.
- `prepare()` dispatches on `request.framework()`:
  - Vue: `let vue = request.vue().expect("dispatch already matched Vue");`
    (mirror the existing `.expect` idiom already used at `standalone.rs:310`
    /`:358`/`:783`-ish — grep the exact wording used nearby and match it),
    call `crate::compile::parse_sfc(source, vue.delimiters.as_ref().map(|(o,c)| (o.as_str(), c.as_str())), Some(vue.is_custom_element.as_slice()))`,
    compute both digests, wrap in `VuePreparedCarrier`.
  - Svelte: call `crate::svelte::parse_svelte(source)` (verify the exact
    `use` path already imported at `standalone.rs:53`-`54` —
    `crate::svelte::runtime::{...}` is imported there but `parse_svelte`
    itself is used via `crate::svelte::parse_svelte` inside `compile_svelte`
    at `:427`; use that exact same path), compute `source_digest`, wrap in
    `SveltePreparedCarrier`.
  - `prepare` is INFALLIBLE (both parse functions are infallible) — return
    `PreparedCarrier` directly, no `Result`. Do not invent a `PrepareError`
    type with no reachable variant (Stub Prevention).

## 3. Refactor `compile_vue` / `compile_svelte` to share a parsed-input core

This is the one required edit to B5's frozen file. It changes ZERO output
bytes — it only moves where parsing happens. The proof obligation is
twofold: (a) `cargo test -p verter_compiler --lib standalone::` (currently
18/18, see B5's landing record) stays 18/18 unchanged; (b) the new
cross-route result-identity test (§5) passes, which independently proves
direct-route output is unaffected by the refactor since it compares direct
output against itself transitively through the other routes.

### 3a. Vue

Read the FULL current body of `compile_vue` (`standalone.rs:223`-`~407`)
before editing — this spec describes the shape, not a line-for-line diff.

Split into:

```rust
fn compile_vue(&self, source: &str, request: &CompileRequest, execution_inputs: &VueExecutionInputs, macro_semantics: &VueMacroSemanticInput) -> Result<DirectCompileOutput, DirectCompileError> {
    let vue = request.vue().expect("dispatch already matched Vue");
    let parsed = crate::compile::parse_sfc(
        source,
        vue.delimiters.as_ref().map(|(o, c)| (o.as_str(), c.as_str())),
        Some(vue.is_custom_element.as_slice()),
    );
    self.compile_vue_from_parsed(source, &parsed, request, execution_inputs, macro_semantics)
}

fn compile_vue_from_parsed(&self, source: &str, parsed: &ParsedSfc, request: &CompileRequest, execution_inputs: &VueExecutionInputs, macro_semantics: &VueMacroSemanticInput) -> Result<DirectCompileOutput, DirectCompileError> {
    let allocator = Allocator::new();
    let mut result = crate::compile::compile_from_parsed(source, parsed, request, execution_inputs, macro_semantics, &allocator)
        .map_err(DirectCompileError::Vue)?;
    // ... EVERYTHING from the current `compile_vue` body starting at its
    // `let tsx = result.tsx.take();` line, UNCHANGED, through to the final
    // `Ok(DirectCompileOutput { ... })` — with exactly one further edit:
}
```

The one further edit is the DUAL-RUNTIME secondary compile
(`standalone.rs:345`-`352` in the pre-refactor file: `let (secondary_parsed,
secondary_result) = crate::compile::compile_with_parsed(source,
&secondary_request, execution_inputs, macro_semantics,
&secondary_allocator)...`). Replace with:

```rust
let secondary_result = crate::compile::compile_from_parsed(
    source, parsed, &secondary_request, execution_inputs, macro_semantics, &secondary_allocator,
).map_err(DirectCompileError::Vue)?;
```

reusing the SAME `parsed` (the shared, outer one) instead of a fresh
`secondary_parsed`. Every downstream use of `secondary_parsed` (the
`direct_vue_dialect(&secondary_parsed, ...)` call) becomes
`direct_vue_dialect(parsed, ...)`.

**Confirmed by recon** (do not re-derive, this was independently checked
against the live body): `direct_vue_dialect` (`standalone.rs:657-673`)
reads ONLY `sfc_script_dialect(parsed.script_setup(), parsed.script())` +
`force_js` — it does NOT read `is_vapor`. `single_runtime_product_request`
(`:564-585`) clones `request.framework()` (hence `vue.delimiters`/
`is_custom_element`) and `force_js` unchanged onto the secondary request —
confirmed at `compile_request/mod.rs:192-199` (`CompileRequest::new` stores
`framework` as given) and `compile_request/vue.rs:535-536`. So primary and
secondary sub-requests parse the identical source with identical options,
and `compile_inner` does not re-parse the SFC internally
(`parse_sfc(` production call sites are only `compile/mod.rs:592` and
`vue_bridge.rs:588` — neither is reached from inside `compile_inner`).
Sharing one `parsed` between primary and secondary is therefore not an
approximation — it removes a redundant, provably-identical second parse
the direct route was already paying for.

### 3b. Svelte

Read the FULL current body of `compile_svelte` (`standalone.rs:409`-`~528`)
before editing. Split into:

```rust
fn compile_svelte(&self, source: &str, request: &CompileRequest, execution_inputs: &SvelteExecutionInputs) -> Result<DirectCompileOutput, DirectCompileError> {
    let parsed = crate::svelte::parse_svelte(source);
    self.compile_svelte_from_parsed(source, &parsed, request, execution_inputs)
}

fn compile_svelte_from_parsed(&self, source: &str, parsed: &ParsedSvelte, request: &CompileRequest, execution_inputs: &SvelteExecutionInputs) -> Result<DirectCompileOutput, DirectCompileError> {
    // EVERYTHING from the current `compile_svelte` body starting at its
    // `let plan = ProductPlan::from_request(request);` line (the parse and
    // allocator-declaration lines above it are dropped — the allocator
    // declaration stays, only the `parse_svelte` call moves out), UNCHANGED.
    // The existing loop already calls `compile_client(source, &parsed, ...)`
    // — `parsed` there already refers to whatever binding is in scope, so
    // this body needs NO further edit beyond taking `parsed` as a
    // parameter instead of a local `let`.
}
```

Svelte needs no secondary-compile edit — `compile_svelte`'s existing loop
already reuses the SAME `parsed` binding for both the server and client
sub-compiles in a dual-runtime request (confirmed: `compile_client` is
called twice inside one `for kind in [RuntimeServer, RuntimeClient]` loop
over the SAME `&parsed`, `standalone.rs:454`, loop at `446-513`).

**Disclosed, accepted order change (confirmed by recon, not a defect):**
today `compile_svelte`'s unsupported-product refusal loop and
`direct_svelte_runtime_options` both run BEFORE `parse_svelte` (`:415-429`).
Under this refactor, `compile_svelte`'s wrapper parses FIRST, then
delegates — so a request naming an unsupported product now still returns
the identical `Err(DirectCompileError::UnsupportedProduct(..))` (proven by
the existing `svelte_unsupported_product_is_refused_before_publish` test
staying green — verified: it only asserts the `Err` variant, not that no
parse occurred), it just does one extra (cheap, side-effect-free)
`parse_svelte` call first. This is REQUIRED, not incidental: `prepare()`
is product-agnostic by design (§1 — prepared state must not encode which
products were requested, per the charter's "prepared state may not change
request... products" line), so a prepared carrier legitimately gets built
before its eventual request's product list is known to be unsupported.
Do not try to preserve the old check-before-parse order by special-casing
`compile_svelte`'s wrapper — that would silently reintroduce a second,
divergent code path between the direct and prepared routes.

## 4. `compile_prepared`

```rust
pub fn compile_prepared<'a>(
    &self,
    source: &'a str,
    prepared: &PreparedCarrier,
    request: &CompileRequest,
    inputs: DirectExecutionInputs<'a>,
) -> Result<DirectCompileOutput, DirectCompileError>
```

1. Three-way framework agreement: `request.framework()`, `prepared`'s
   variant, `inputs`'s variant must all name the same framework. Reuse the
   existing `DirectCompileError::FrameworkMismatch { expected, actual }`
   variant (`standalone.rs:110`-ish) — do not add a new variant for this;
   mirror the exact 4-arm match `StandaloneCompiler::compile` already uses
   at `standalone.rs:186`-`221` (read it before writing this match) for
   the request/inputs pair, and add the `prepared` variant as a THIRD
   condition inside the matching arm (a `PreparedCarrier::Svelte` reaching
   the Vue arm is exactly the same class of error, so it maps to the same
   variant).
2. Recompute `blake3::hash(source.as_bytes())`; compare against the
   prepared carrier's stored `source_digest`. Recompute
   `parse_identity_digest` from `request.vue()` (Vue only) and compare.
   On EITHER mismatch, return a NEW error variant:
   ```rust
   StalePreparedInput { reason: StalePreparedReason },
   ```
   with
   ```rust
   pub enum StalePreparedReason { SourceChanged, ParseOptionsChanged }
   ```
   Never silently reuse a mismatched carrier — this is exit criterion #2
   (stable reuse without stale inputs) and needs its own discriminating
   test (§5).
3. On a full match, dispatch to `compile_vue_from_parsed` /
   `compile_svelte_from_parsed` (§3) with the prepared carrier's retained
   `parsed` value and the CALLER'S `source`/`request`/`inputs` (not
   anything cached from `prepare()` time beyond the parse itself — the
   request and inputs are always fresh per call, exactly like the direct
   route).

## 5. `compile_batch`

```rust
pub struct BatchCompileItem<'a> {
    pub source: &'a str,
    pub request: &'a CompileRequest,
    pub inputs: DirectExecutionInputs<'a>,
}
pub struct CompileBatchReport {
    pub cold_build_count: usize,
    pub reuse_count: usize,
}
pub struct BatchCompileOutput {
    pub results: Vec<Result<DirectCompileOutput, DirectCompileError>>,
    pub report: CompileBatchReport,
}
```

Definitions (lock these — the perf cell in §6 depends on them being
unambiguous): `cold_build_count` = number of `prepare()` calls
`compile_batch` actually performs (one per distinct group, see below).
`reuse_count` = number of `compile_prepared()` calls it performs. Even an item
that is the ONLY member of its group is still served via `compile_prepared`,
never via a direct `compile_vue`/`compile_svelte` call.

**Correction (post-review).** This definition originally added "so `reuse_count`
always equals `items.len()`". That equality holds only when every item is
admitted. An item refused by the product/capability preflight is recorded as `Err`
in its own slot and is never prepared and never served, so it increments neither
count — which is what "zero unrequested work" requires, and counting it would make
the report claim work that did not happen. Both counts are therefore over ADMITTED
items, and `reuse_count == items.len()` exactly when no item is refused. The
definition above ("calls it performs") was always the operative one; only the
appended equality was wrong, so the code is correct and this text is corrected to
match it.

Algorithm:

1. `items.is_empty()` → return `BatchCompileOutput { results: vec![], report: CompileBatchReport { cold_build_count: 0, reuse_count: 0 } }` immediately — zero-demand zero-initialization, no allocator/parse work at all.
2. Group key = `(framework tag, source_digest, parse_identity_digest-or-unit-for-Svelte)`.
   Compute each item's digests using the SAME digest logic `prepare()`
   uses (do not duplicate the hashing code — factor the digest computation
   out of `prepare()` into a small private helper both `prepare()` and
   `compile_batch()`'s grouping step call, e.g.
   `fn source_digest(source: &str) -> [u8; 32]` and
   `fn vue_parse_identity_digest(vue: &VueCompileRequest) -> [u8; 32]`).
3. Walk `items` in order, maintaining a `Vec<(GroupKey, PreparedCarrier)>`
   (linear scan for a matching key is fine — batch sizes here are small;
   do not add a new `indexmap`/`HashMap` dependency for this). First time a
   key is seen: call `self.prepare(item.source, item.request)`, push
   `(key, carrier)`, `cold_build_count += 1`. Every time (first or repeat):
   call `self.compile_prepared(item.source, &carrier_for_key, item.request, item.inputs)`,
   `reuse_count += 1`, push the `Result` into `results` at this item's
   ORIGINAL index (results must be built as `Vec::with_capacity(items.len())`
   pushed in input order — batch never reorders).
4. One item's `Err` must not affect any other item's entry — this holds by
   construction (each iteration only reads the shared, immutable
   `PreparedCarrier` for its group and writes its own `results[i]`; no
   shared mutable state crosses items) — write a test proving it anyway
   (§5, atomicity test): a batch with item 0 malformed (guaranteed refusal)
   and item 1 valid must return `results[0]` as `Err(...)` and `results[1]`
   as a full, correct `Ok(DirectCompileOutput)`.

No `BatchOptions` type, no thread/concurrency field (§0 — mirror the
`verter_session` guard's intent even though this is a different crate; do
not introduce the pattern the guard exists to forbid).

## 5. Required tests (TDD — write failing first, per CLAUDE.md)

All in `standalone.rs`'s own `#[cfg(test)] mod tests` (existing, per B5's
"18/18 in `standalone::tests`") unless the file organization threshold
(~400 inline test lines) is hit, in which case extract to a sibling
`standalone_prepared_tests.rs` per the Rust test file organization rule —
implementer's call, not prescribed here.

1. **Result identity (exit #1)** — for each of ~5-8 corpus fixtures (§0,
   mixed Vue VDOM/Vapor + Svelte), for at least one Vue dual-runtime
   (`RuntimeClient`+`RuntimeServer` together) case and one Svelte single
   case: compute `DirectCompileOutput` via (a) `compile()` directly, (b)
   `prepare()` then `compile_prepared()` once, (c) the SAME prepared
   carrier reused for a SECOND `compile_prepared()` call
   (prepared-repeat), (d) `compile_batch()` with all corpus items in one
   call. Assert every artifact's `code`/`source_map`/`emitted_imports` and
   `styles`/`diagnostics` are byte-identical across all four. Use a single
   combined digest (blake3 over a canonical concatenation of artifact
   fields) per route per fixture as the comparison, so a mismatch reports
   which fixture/route diverged rather than a giant string diff.
2. **Stable reuse without stale inputs (exit #2)** — prepare from source A;
   call `compile_prepared` with a DIFFERENT source B (same framework) using
   the same carrier → assert `Err(DirectCompileError::StalePreparedInput { reason: SourceChanged })`,
   never a silently-wrong compiled result. Separately: prepare a Vue source
   with delimiters X; call `compile_prepared` with a request specifying
   delimiters Y → assert `Err(StalePreparedInput { reason: ParseOptionsChanged })`.
3. **Atomic per-request results (exit #3)** — the batch partial-failure
   test from §5-item-4 above. Additionally: a single `compile_prepared`
   call that would fail (e.g. an unsupported product) must return `Err`
   with no `DirectCompileOutput` constructed at all (already true by
   the `Result` return type + no partial-construction pattern already
   established by `compile_vue`/`compile_svelte` — assert it explicitly
   for the new entry points too, not just inherited by inspection).
4. **Zero unrequested work (exit #4)** — a `compile_batch` call with 2
   items that share IDENTICAL source+framework+parse-options but different
   product requests must report `cold_build_count == 1` (one shared
   prepare) and `reuse_count == 2`. A batch with 2 items whose sources
   differ must report `cold_build_count == 2`. This is a precise,
   discriminating proxy for "no unrequested preparation work" — assert the
   exact counts, not just "less than N".
5. **Bounded memory (exit #5)** — a batch of, say, 50 items built from only
   3 distinct sources (repeated with different requests) must report
   `cold_build_count == 3` regardless of `items.len()` — proves the
   internal group storage is bounded by DISTINCT inputs, not batch size.
   (The real RSS measurement lives in the perf cell, §6 — this unit test
   is the cheap, exact, always-run proxy.)
6. **No cross-call state** — call `compile_batch` twice in a row with
   overlapping sources; assert the SECOND call's `cold_build_count` is
   NOT reduced by the first call (i.e. it still equals its own distinct-
   source count) — proves there is no hidden cache surviving across calls.
   Grep self-review: confirm no `static`/`thread_local`/`OnceLock` was
   introduced anywhere in this change.

## 6. `B6_COMPILER_ROUTE_OVERHEAD` perf cell

Add ONE new `verter_bench` example (not a criterion `[[bench]]` — this cell
needs custom counters/digests a criterion harness doesn't emit, so a plain
`examples/compiler_route_overhead.rs` binary that prints a small machine-readable
report is the right shape, matching the existing `profile_*`/`*_check`
example idiom already in that directory) that:

- Loads the same ~5-8 corpus fixtures used in §5's identity test (do not
  invent a second corpus — one corpus, referenced from both, keeps them
  from silently diverging; if the unit test's fixture list is a Rust
  `const`, have the example use the same list or read the same fixture
  files by path).
- Runs, per route, over the full corpus: direct (N cold `compile()`
  calls); prepared-first (N `prepare()` + N `compile_prepared()` — this is
  cold_build == N, reuse == N); prepared-repeat (reuse each already-
  prepared carrier for K more `compile_prepared()` calls, K configurable,
  e.g. 5 — cold_build == 0 for this leg, reuse == N*K); batch (one
  `compile_batch()` call over all N items — report gives cold_build/reuse
  directly).
- Computes an output digest per route (blake3 over the canonical
  concatenation used in §5's identity test — reuse that exact function,
  do not write a second one) and asserts (via `assert_eq!`, not a printed
  warning — a route producing a different digest must fail loudly) all
  four digests match.
- Reports wall-clock latency per route (`std::time::Instant`) and peak
  RSS. Use whatever RSS-reading approach an existing `verter_bench`
  example already uses if one exists (grep `examples/*.rs` for `rss`/
  `ru_maxrss`/`memory` before writing a new one); if none exists, a
  simple `/proc/self/status`-on-Linux + `getrusage`-on-macOS reader
  (platform-gated per the Cross-Platform Portability rule — must not
  break a Windows build; a `#[cfg(not(target_os = "windows"))]` best-
  effort RSS reader with a `None` fallback on Windows is acceptable IF
  disclosed in the report output, since a silent `0` would misreport).
- Prints `cold_build_count`/`reuse_count`/latency_ms/rss_bytes per route
  as one line of machine-parseable output (e.g. one JSON object per
  route) to stdout.

**Threshold-locking disposition**: `performance-impact.md` lists
`B6_COMPILER_ROUTE_OVERHEAD` as a required NEW cell but explicitly declines
to invent numeric thresholds for it ahead of a candidate — "deferred to
[the] owning block['s] own landing." Precedent (checked at scoping time):
neither BV1 nor BS1 froze their own listed cells at landing either
(`docs/arch/refactor/rev11/evidence/BV1/landing-record.md` — no
`BV1_VUE_VDOM_DIRECT_CORE` freeze; the block landed on its correctness/
conformance evidence and left the perf cell unaddressed). `performance-
gates.toml`'s own header requires any new locked cell to go through "a new
Implementation Lock Record digest and the same independent review class"
(ADR-016) — that is NOT B6's own review mandate (conformance/architecture/
adversarial), it is a SEPARATE, heavier authority this block was not
chartered to invoke. Disposition: B6 BUILDS the harness, RUNS it under the
bounded machine protocol, and RECORDS the measured numbers (digest match/
mismatch, cold/reuse counts, latency, RSS) as its own landing evidence —
satisfying the charter's "identical corpus... output digest, reuse/cold-
build counts, latency/RSS" requirement as a PROVEN MEASUREMENT — without
writing a new `[[cell]]` entry into `performance-gates.toml`. If the
program orchestrator or maintainer wants a formally locked threshold, that
is a follow-up ADR-016 action, not a silent gap: this disposition and its
reasoning must be stated verbatim in B6's landing record, not left
implicit.

## 7. Explicitly out of scope (do not build)

- No `PreparedCarrier` pool, registry, or cache keyed by content hash that
  survives past a single `prepare()`/`compile_batch()` call.
- No thread/concurrency knob on any new type (§0, §5).
- No change to `StandaloneCompiler::compile`'s own signature, error
  variants (beyond the one addition), or observable behavior.
- No audit/observer wiring (`verter_audit::current_observer()`) for the new
  routes. Do not add production audit events speculatively.

  **ADOPT-NOW (post-review scope correction).** This bullet originally also said
  the perf cell's counters must come from `CompileBatchReport` and direct
  call-counting in the bench harness, "not from the audit substrate". That
  mechanism is adopted against, and the change is recorded here rather than taken
  silently.

  Reason: direct call-counting in the harness is derived from the harness's own
  loop structure, so it reports the number of iterations the harness performed
  regardless of whether `compile_prepared` reused the carrier or silently
  re-parsed. That is a non-discriminating counter — a stub under the project's
  Stub Prevention rule — and it was empirically shown to be one: the pre-fix
  harness passed with exit 0 against a `compile_prepared` mutated to re-parse
  (recorded under "Mutation proof" in
  `B6_COMPILER_ROUTE_OVERHEAD-measurement.md`). `CompileBatchReport` is
  `compile_batch`'s own self-report and cannot cross-check itself.

  Adopted instead: the harness reads the EXISTING `compiler.carrier_parse.calls`
  attribution counter around each leg. This block adds no production audit event —
  those attribution sites predate it — and the harness's `required-features =
  ["attribution"]` makes an unmeasured build a hard error rather than a silent
  pass. The prohibition on adding NEW production audit events for these routes
  stands unchanged; only "may not read an existing counter" is lifted.
- No semantic projection / TypeInfo / cross-file resolution anywhere in
  this block (C2's job, not B6's).
