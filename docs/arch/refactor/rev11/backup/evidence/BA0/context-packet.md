# BA0 — landing context packet

The dispatch context this block actually ran on. Prompt bytes are reproduced as
issued, not summarised. The only substitution is the absolute worktree root,
replaced by the token `<WORKTREE>` so no machine-specific path is tracked;
nothing else is edited.

## Predecessor state at dispatch

`program/architecture-lock` at `dd84e5fa2`; BF3 ACCEPTED at `c6da941ee`. BA0, BS0,
BCSS0 and BRT0 READY; B2/B3 LOCKED. The branch was later rebased onto `17c25a03d`
(BRT0's partial landing plus a ledger commit) before the landing gate, and landed
from that base.

## Binding inputs

- Charter: [`charters/BA0.md`](../../charters/BA0.md), read as AMENDED — the AT-2
  row and the Required-procedure / Required-exits paragraphs carry the maintainer
  act of 2026-08-17.
- Ratified findings: the `AT-1` and `AT-2` rows of
  [`../BF3/dispositions.md`](../BF3/dispositions.md) plus its observation note.
- Maintainer acts: the bugs-and-types standing ruling, the gate-scope ruling, the
  AT-2 naming act and its scope clarification, all recorded under
  [`../BF3/`](../BF3/).

## The design question this block had to answer first

The charter's live acceptance target, `a_refused_combined_request_publishes_no_product_at_all`,
was `#[ignore]`d with a body that called a TSX-only profile (`CompileTarget::IDE`)
a "combined" request and then observed a LATER, separate `ensure_ide_compiled`
call as if it were the same request. Two independent unprimed consults were run
before any implementation.

The first enumerated the candidate corrections and concluded that no correction
satisfies both the ratified independent-identity contract and that body as
written, recommending `RESCOPE_REQUIRED`. The second was asked to rule on a
fourth candidate the first had not evaluated — a typed requested-product set plus
an atomic terminal outcome, with the target re-expressed over a genuinely combined
token (`BUNDLER | IDE`). It ruled that candidate correct, ruled the body
re-expression legitimate and required rather than a gate-bypass (the old body's
profile does not construct the request it names, and its `Ok(false)` expectation
repurposes a value the API documents as "no IDE surface exists"), and confirmed
the re-expressed target stays RED on the pre-change tree. Both consult prompts and
their full replies are the dispatch record for this decision.

## Dispatch prompt — implementation

The implementer brief issued to the worker, verbatim:

<!-- BEGIN implementer-brief -->
You are the implementer for a ratified correction block in the Verter monorepo. Work ONLY in this worktree: `<WORKTREE>` (branch `block/ba0-atomicity`). Deps are installed and `pnpm build:ts` has been run. Be TERSE in all reporting.

# 0. Non-negotiable working rules

- **Any claim you make about code you did not write is a QUESTION, not a finding.** Open it and cite `file:line`, or report it as UNKNOWN. Before you report, list every claim you make about a sibling consumer or shared owner with the `file:line` where you verified it.
- **TDD is mandatory.** Write/adjust the failing test FIRST, run it, capture the genuine RED output, then implement, then GREEN. Paste real command output in your report — never a paraphrase.
- **No stubs.** No empty test bodies, no unconditional default returns dressed as implementation, no always-true assertions, no "deferred to a follow-up commit". A characterization test must FAIL pre-change and PASS post-change.
- **Prove every mutation applied.** `perl`/`sed`/`grep` exit 0 on a non-match; an exit code is never proof a plant landed, and a hit on a pre-existing occurrence is a false positive. A green planted run means the plant failed until proven otherwise.
- **Never run the full gate** (`node scripts/gate.mjs`). Run TARGETED tests only: `cargo test -p verter_session <name>`, `cargo test -p verter_compiler <name>`, `cargo nextest run -p verter_session -E 'test(<pattern>)'`. The landing gate is run by the orchestrator, once, later.
- **WIP-commit freely** (conventional commits). Commit after each coherent step so partial work survives. **Zero program vocabulary** in commit subjects OR bodies: no block IDs, no `BA0`/`BF3`/`AT-1`/`AT-2`/`rev11`, no charter/amendment references, no "phase"/"cutover" language. No AI attribution, no `Co-Authored-By`.
- **A wrong output is a BUG, not an error path.** You may NOT add a production guard, typed refusal *mechanism bolted on after the fact*, withhold path, retraction, runtime tracking artifact, known-divergence allowlist, fixture-identity branch, or string-scanning second authority. The correction is to make the mixed state *unconstructible*, not to filter it after construction.
- **Types are WAIVED for this program.** Do not open type-correctness work.
- **STOP and report** (do not improvise) if: you must change a file owned by another correction (Svelte emitter/map/projector semantics, standalone CSS source maps, the batch route's carrier selection, NAPI-vs-WASM missing-product parity); or the Vue side of step 2.5 below cannot be done by gating and would require restructuring the Vue codegen entanglement; or you find the design below is wrong.

# 1. What is being corrected

Two independent items at the common compiler/session request-result-publication boundary.

## Item A — a refused runtime surface must not leave a sibling product published under the same request identity

Today:
- `want_ide = profile.target.needs_tsx()` (`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2884`). There is **no** `want_runtime`.
- The Svelte carrier therefore runs its client/runtime compile **unconditionally**, before and independently of the `if opts.want_ide` branch (`crates/verter_compiler/src/svelte/carrier.rs:358-412` vs `:517-529`), and its failure arms set `bundle.runtime_surface_refused = true` (`:442-515`).
- `RuntimeCompileOutput` carries `runtime_surface_refused: bool` **beside** `tsx: Option<IdeOutput>` (`crates/verter_compiler/src/framework_common/carrier_compiler.rs:602-645`) — the mixed value is first constructed there.
- The session preserves both halves and commits them into one `CompileSlot` per `(canonical, profile_hash)` (`virtual_file_pipeline.rs:2979-2983`, `:3157-3167`, `:3216-3226`, `:1804-1820`, `:1926-1935`; `crates/verter_session/src/cache_runtime/compile_output_node.rs:146-171`, `:574-598`).
- `get_virtual_file` recovers the refusal reason by **scanning diagnostic code prefixes** (`virtual_file_pipeline.rs:1087-1107`, `d.code.starts_with("svelte-runtime-unsupported-")`).

Consequence: a single request whose profile asks for BOTH runtime products and the IDE product (e.g. `CompileTarget::BUNDLER | CompileTarget::IDE`) refuses the runtime surface and still publishes the TSX under the same identity.

## Item B — the batch entry construction can express a product beside an error

`crates/verter_session/src/host_compile.rs`: `CompileBatchEntry` exposes product fields (`code`/`lang`/`source_map`) and `errors` independently (`:111-142`). Of its nine construction sites, the HostBacked `get_virtual_file` `Ok(response)` arm (`:743-771`) derives `errors` by severity-filtering the response's diagnostics **while independently retaining** the product; the other eight hardcode one side. No reachable input demonstrates the mixed state — it is a latent construction hazard, not a demonstrated defect. The obligation is to make the mixed state **structurally unconstructible**, with no guard and no retraction.

# 2. The design you will implement

This design was ruled correct by an independent architecture consult. Implement it as written; if you find it wrong, STOP and report with evidence.

## 2.1 A requested-product set that includes runtime

Add to `crates/verter_compiler/src/compile/types.rs`, on `impl CompileTarget`:

```rust
/// Whether the request asks for RUNTIME output products (the main module and
/// its script / template / style side-files).
pub fn needs_runtime_module(self) -> bool {
    self.intersects(Self::STYLE | Self::SCRIPT | Self::TEMPLATE)
}
```

**It must NOT be `needs_script()`** — `needs_script()` includes `TEMPLATE_DATA`, and the LSP's profile is `CompileTarget::IDE | CompileTarget::TEMPLATE_DATA` (`crates/verter_lsp/src/documents/mod.rs:296-301`). Using `needs_script()` would make the LSP request runtime output and lose its IDE surface for every runtime-refused Svelte component. Verify this yourself before writing the predicate.

Add `pub want_runtime: bool` to `RuntimeCompileOptions` (`crates/verter_compiler/src/framework_common/carrier_compiler.rs:~340-389`), documented as the runtime half of the requested-product set, alongside `want_ide` / `want_template_data`.

Set it at the single session producer (`virtual_file_pipeline.rs:~2854-2887`):
`want_runtime: profile.target.needs_runtime_module(),`

The requested-product set is therefore a pure function of the profile's target bits, which already participate in `profile_hash` — so the publication identity already carries the requested-product set and **you must not re-key any cache**.

## 2.2 The carrier's terminal result becomes a sum

`RuntimeCompileOutput.runtime_surface_refused: bool` is **DELETED**. In its place, the carrier's terminal result is a sum type in `carrier_compiler.rs`, e.g.:

```rust
/// The carrier's terminal result for one compile request. A refusal carries
/// NO products: the mixed "refused runtime + published sibling product" state
/// is not representable.
pub enum CarrierCompileOutcome {
    Produced(RuntimeCompileOutput),
    RuntimeSurfaceRefused(RuntimeSurfaceRefusal),
}

/// Why the carrier fail-closed on the requested runtime surface, carried
/// structurally so no consumer recovers it by scanning diagnostic text.
pub struct RuntimeSurfaceRefusal {
    pub diagnostic_code: String,
    pub message: String,
    pub span: Option<verter_span::Span>,
    /// Diagnostics accumulated before the refusal (non-fatal).
    pub diagnostics: Vec<RuntimeDiagnostic>,
}
```

Exact names/shape are yours; the invariants are fixed:
- the refusal variant has **no** product field of any kind (no `tsx`, no `main`, no `styles`, no `template_data`);
- the produced variant has **no** refusal field;
- the refusal reason is carried **structurally** (code + message), never recovered by a string prefix scan downstream.

The carrier compile entry point returns this outcome (wrapped in whatever `Result<_, CompileUnsupported>` it already uses).

## 2.3 Svelte carrier

`crates/verter_compiler/src/svelte/carrier.rs`:
- Run the client/runtime compile **only when `opts.want_runtime`**.
- When runtime is requested and the compile fail-closes, return `CarrierCompileOutcome::RuntimeSurfaceRefused { .. }` **before** the `if opts.want_ide` block — the IDE projection is not constructed at all for that request.
- When runtime is NOT requested, no runtime attempt occurs, no refusal can arise, and the IDE projection (and template data) is produced normally.
- Delete the now-dead `runtime_surface_refused` assignments.
- The compiler-side unit tests that assert "refusal AND `tsx.is_some()`" (around `crates/verter_compiler/src/svelte/carrier.rs:846-895` — verify the exact range) must be re-stated for the corrected contract: with `want_runtime: true` a refusal carries no products; with `want_runtime: false` the IDE artifact is produced and there is no refusal.

## 2.4 Vue carrier / bridge

Vue never refuses a runtime surface, but the required exit is "success publishes all and only the products requested by that identity". Honor `want_runtime` in `crates/verter_compiler/src/framework_common/vue_bridge.rs`: when it is false, do not emit the runtime script / template / style / custom-block products; still emit the IDE artifact when `want_ide`, and template data when `want_template_data`.

Note `compile_script_unit` builds `script_target` as `CompileTarget::SCRIPT` plus `TSX` when `want_ide` (`vue_bridge.rs:~283-287`). The gating shape is: start from an empty target, add `SCRIPT` when `want_runtime`, add `TSX` when `want_ide`. Verify that TSX codegen does not require the `SCRIPT` bit before relying on this (`crates/verter_compiler/src/compile/types.rs:50-110` documents `needs_script` / `needs_tsx` / `needs_runtime_macro_semantics`).

**If this cannot be done by gating and needs the Vue codegen restructured, STOP and report** — that would exceed this block's scope.

## 2.5 Session

`crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`:
- Thread the carrier's terminal outcome through the compile record, `CompileOutputValue`, and `CompileServe` so a refusal is a **variant**, not a flag beside products.
- **A refused transaction commits no products.** A payload-free terminal refusal record MAY be cached (so a refused component does not recompile on every request) — but that record must be constructible only without products; it must not be a product-carrying record with an extra flag set.
- `compile_serve_satisfies_demand` (`:2044`) must treat a cached terminal refusal as SATISFYING both a `VirtualNode` demand and an `Ide` demand — it is terminal for that identity.
- Public route behaviour on a refused identity:
  - `get_virtual_file` with `VirtualNodeKind::Main` → `HostError::RuntimeSurfaceRefused { canonical_id, diagnostic_code, message }`, with code+message taken **structurally** from the refusal, not from a diagnostic prefix scan. **Delete the `starts_with("svelte-runtime-unsupported-")` scan at `:1087-1107`.**
  - `get_virtual_file` with any other node kind → `HostError::MissingVirtualNode` (unchanged; this preserves the existing green test `a_refused_runtime_surface_publishes_no_javascript_no_css_and_no_source_map`).
  - `ensure_ide_compiled` → `Err(HostError::RuntimeSurfaceRefused { .. })`. It must NOT be collapsed into `Ok(false)`: that value is documented as "the loaded file has NO IDE projection surface … never a real failure" (`:2082-2091`), and a refusal is a real failure. Update that doc comment to name the refusal arm.
  - `get_ide` (a pure cached read) → `None`, because nothing was committed.
  - `get_public_api_with_mode` is a separate identity with its own render path (`:2363-2400`) and is **unchanged**.

## 2.6 Item B — batch entry

`crates/verter_session/src/host_compile.rs`: replace the flat product+errors fields of `CompileBatchEntry` with an exhaustive sum, e.g.

```rust
pub enum CompileBatchOutcome {
    /// Compiled. Carries the product and any NON-error diagnostics.
    Produced { code: String, lang: Option<String>, source_map: Option<String> },
    /// Failed. Carries the errors and NO product field.
    Failed { errors: Vec<String> },
}
```

Invariants: `Produced` has no `errors` field; `Failed` has no product field of any kind; the errors of `Failed` are non-empty by construction (a constructor that rejects an empty list, or a non-empty collection type). Route **all nine** construction sites through it (enumerate them yourself and list them with `file:line` in your report — a prior enumeration counted seven failure/panic sites hardcoding empty products at `:569-580`, `:689-700`, `:798-824`, `:869-895`, `:912-926`, one success site hardcoding empty errors at `:841-856`, and the hazard site at `:743-771`; verify, do not trust).

The HostBacked `Ok(response)` arm must decide the variant from the **typed** terminal result, not by severity-filtering a successful response's diagnostics. A genuine failure already arrives as `Err(HostError)` (including `RuntimeSurfaceRefused`).

**Preserve the external wire shape.** NAPI (`crates/verter_napi/src/lib.rs:~2457-2475`) and WASM must flatten the sum **arm by arm** in an exhaustive match — success emits the product with an empty error list, failure emits an empty product with the errors — so the serialized JS-visible shape is byte-identical to today. Do not change any TypeScript. Verify the WASM batch path too and cite it.

# 3. Tests you must write / change

## 3.1 The live acceptance target (item A) — must go RED then GREEN

`crates/verter_session/src/framework/framework_product_surface_tests.rs:1445-1494`,
`fn a_refused_combined_request_publishes_no_product_at_all`.

**Keep the function name.** Remove the `#[ignore]`. Re-express the body so it drives a genuinely COMBINED request identity. Its current cells use `CompileProfile { target: CompileTarget::IDE, .. }` — the TSX bit ONLY — and then observe the later, separate `ensure_ide_compiled` call as if it were the same request; that is why the body must change. Use `CompileTarget::BUNDLER | CompileTarget::IDE` on the same two refusing Svelte sources (`SVELTE_STYLED` under `ssr: true`; `SVELTE_PROPS_EVENTS`). The re-expressed body must assert, per cell:

1. The combined request's runtime node is the typed refusal: `read_node(host, canonical, VirtualNodeKind::Main, &combined)` is `NodeOutcome::Refused { diagnostic_code }`, and the code equals the compiler's own `UnsupportedSvelteRuntimeSurface::…diagnostic_code()` for that cell (take it from `refusal_cells()`-style construction, never a transcribed literal).
2. **No product at all is published under that combined token**: every other `VirtualNodeKind` is `NodeOutcome::Missing`, AND `host.get_ide(canonical, &combined)` is `None`, AND `host.ensure_ide_compiled(canonical, &combined)` is `Err(HostError::RuntimeSurfaceRefused { .. })`.
3. **Positive control — the correction is not "never produce TSX".** On the same source, a distinct IDE-ONLY token (`CompileTarget::IDE`, or `IDE | TEMPLATE_DATA`) succeeds: `ensure_ide_compiled` is `Ok(true)` and `get_ide` returns a NON-EMPTY IDE product.
4. **Positive control — PublicApi is an independent identity**: `get_public_api_with_mode(canonical, PublicApiMode::Public, None)` is `Ok(Some(_))`.
5. **Order controls**: run the sequence in both orders on independent hosts — combined-then-IDE-only, and IDE-only-then-combined — and assert the same outcomes both ways, so an earlier IDE success cannot leak into the combined refusal and an earlier combined refusal cannot poison the IDE-only request.
6. **Supported combined-success control**: a SUPPORTED Svelte component under the same combined token publishes BOTH its runtime `Main` and a non-empty IDE product — otherwise "refuse every combined request" would satisfy the refusal half. (`a_supported_svelte_client_component_keeps_publishing_its_module_and_its_css` names a suitable supported source; verify.)

**RED proof required.** Before implementing anything, land the re-expressed body on the unmodified tree and capture the genuine failure. On today's tree the combined token refuses the runtime surface and still publishes the TSX, so assertion 2 must fail. Paste the real `cargo test` output showing WHICH assertion failed. A RED run that fails for a compile error or for assertion 1 is NOT the required RED — fix the test until it fails on the atomicity assertion specifically.

## 3.2 The item-A characterization

`framework_product_surface_tests.rs:755-786`, currently `a_refused_runtime_surface_still_publishes_the_ide_and_public_api_products`, characterizes the pre-correction contract and will break. Re-state it for the corrected contract and RENAME it to describe what it now characterizes: the refusal is scoped to the runtime-requesting identity, while a separate IDE-only identity and the separate PublicApi identity still publish. Record the rename in your report (old name → new name → why).

## 3.3 The item-B artifacts

- `crates/verter_session/src/framework/svelte_batch_route_tests.rs`, the `#[ignore]`d `the_host_backed_success_construction_is_never_fed_a_response_that_carries_an_error` (~`:1219-1233`): **keep it `#[ignore]`d** and keep its name (it is a ratified artifact), but re-state its body and doc comment so it asserts what the corrected construction makes true, and so its doc no longer says the construction might read an error beside a product. Run it explicitly with `--ignored` and paste the passing output.
- **Add a new, NOT-ignored structural test** proving the sum cannot express a product beside an error: an exhaustive `match` over `CompileBatchOutcome` (no wildcard arm — a new variant must fail to COMPILE) asserting the produced arm exposes a product and no errors and the failed arm exposes non-empty errors and no product, plus the conversion direction (a successful `VirtualFileResponse` maps to `Produced`; a `RuntimeSurfaceRefused` maps to `Failed`). This test must be discriminating, not a tautology.
- `a_genuinely_failing_batch_entry_publishes_no_partial_product` and `searching_for_a_batch_entry_that_serves_a_stale_product_beside_fresh_errors_finds_none` must keep passing. Run them and paste the output.

## 3.4 Regression sweep

Run and report, with real output and counts:
```
cargo test -p verter_compiler --lib 2>&1 | tail -30
cargo test -p verter_session --lib 2>&1 | tail -40
cargo nextest run -p verter_session --no-fail-fast 2>&1 | tail -40
cargo nextest run -p verter_lsp --no-fail-fast 2>&1 | tail -30
```
Every failure is either fixed or reported with a `file:line` explanation of why it is a legitimate contract change. Do NOT delete or `#[ignore]` a test to make it pass.

# 4. Explicitly out of scope — STOP if you need any of these

- Svelte emitter / source-map / props-surface semantics.
- The standalone CSS route's source-map handling.
- The batch route's carrier selection (it hardcodes the Vue carrier for every batch input at `host_compile.rs:469-478`); leave it exactly as it is.
- NAPI-vs-WASM missing-product serialization parity.
- Any final canonical compile-request model or plan-token system, and any generalized publication substrate (`docs/arch/refactor/rev11/contracts/compile-transaction.md` describes a LATER block's target — do not implement it).
- Type-correctness work of any kind.

# 5. Report format (TERSE)

1. What changed, file by file, one line each.
2. The RED proof for §3.1 — the exact command and the exact failing assertion text.
3. The GREEN proof — exact commands and pass counts for §3.1, §3.3, §3.4.
4. The nine batch construction sites, enumerated with `file:line`.
5. Every claim you made about a consumer you did not author, with the `file:line` you verified it at.
6. Anything you could not verify, listed as UNKNOWN.
7. Any STOP condition you hit.
<!-- END implementer-brief -->

## Fix-round prompts

Three fix rounds followed the reviews; their prompts are reproduced in the landing
record's review arc rather than duplicated here.

## Review dispatch

All three mandates were required (class `foundational-atomic`). Every seat was an
external CLI, run sequentially in a throwaway worktree detached at the candidate so
plants never touched the implementation tree. Every prompt carried the same shared
context block: the candidate SHA, the charter as binding spec, the standing rules
(a wrong output is a bug not an error path; types waived; AT-2 carries no
required-RED target and no Svelte-refusal obligation; the named out-of-scope
areas), the requirement to enumerate every Required-procedure obligation and every
Required-exits sentence with per-item evidence, and the instruction that an uncited
item is BLOCKING by default and `NOT-EVIDENCED` a legitimate verdict.

The adversarial seat additionally ran plant/red/green mutation checks against the
test suite at its FIRST pass, with proof of application (present, unique, NEW)
mandatory and a green planted run treated as a failed plant until proven otherwise.
