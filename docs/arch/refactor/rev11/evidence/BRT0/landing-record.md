# BRT0 — landing record

Complete. `TR-1` and `BND-2` landed in a first pass; `RT-1` in a second, after the block
that owned its correction site landed. Context in [`context-packet.md`](context-packet.md).

## What shipped

### `TR-1` — one missing-product contract for both transports

**The contract decided, and why.** A virtual node that does not exist is reported as an
ABSENT response by both public transports. Every other host error still throws.

[`disposition-ruling.md`](../BF3/disposition-ruling.md) left the shape open — "either
uniform nullable absence or uniform typed failure could be valid; choosing between them
is product-contract ratification, not something an implementer may decide implicitly."
This block was dispatched as the owner of that choice, so the choice is recorded here
EXPLICITLY, with its reasons, for ratification at acceptance:

1. A node that does not exist is an ordinary negative answer about the carrier's
   structure — an SFC with no `<style>` block asked for `style[0]` — not a failure.
   Making the native binding throw would ADD an error path for a non-error, which the
   standing product ruling forbids: Verter compiles/builds and RETURNS.
2. Under a throw a caller cannot separate "no such node" from an invalid query, an
   unknown file, or a refused compilation without matching the error TEXT. Under an
   absent response the distinction is structural.
3. Absence is already spelled `null` everywhere else on both transports — `getIde`,
   `remove`, `resolve`, the document structure — so the throw was the outlier within
   the wasm binding's own surface, not a considered second convention.
4. The native binding, its published typings, and the public API tables already
   documented the absent response, so this is the lower-blast-radius direction.

**Code.** The absent/failed split is decided ONCE, in
`verter_ffi::convert::classify_host_virtual_file`, and both bindings' virtual-file entry
points consume its outcome — so reintroducing a divergence means removing a binding from
the shared classifier, not editing one in place. `packages/wasm/src/index.ts` declares the
nullable return. `docs/api/wasm.md` and `docs/api/native.md` corrected. A binding still
maps a host error to its own failure status elsewhere; what is confined is the decision
that an answer is absent rather than failed.

**Independently checked before landing.** A separate unprimed pass asked which contract
is right for a consumer, what depends on each today, and what breaks either way. Verdict
CONTRACT CORRECT, on evidence rather than assent: nothing in the tree reads the wasm
throw; four in-repo tools that assemble a carrier from its optional blocks
(`scripts/compare-per-file.mjs`, `packages/benchmark/src/compilers/verter.ts`,
`scripts/ssr-baseline/compare.mjs`, `scripts/vue-behavior-compare/run.mjs`) already branch
on the absent response and would break under the throw direction; and the native
binding's absent response is a Stable published contract.

**Gate.** `the_transports_report_a_missing_node_the_same_way` (previously `#[ignore]`d)
drives the three cases the ratified acceptance item requires — a structurally absent
node, a node absent because the compilation was refused, and a successful control on
the same carrier — over both built transports, and asserts parity, the settled spelling,
absence of any product, and that the control carries the host's own bytes.

`the_transports_serialize_a_missing_node_differently` is DELETED. It pinned the
divergence and fails if either shape moves, so it cannot survive the correction. What it
asserted that the target does not: the wasm thrown-error text. That assertion is the bug.

### `BND-2` — the non-Vite inline product retains its requested map

`packages/unplugin/src/index.ts` returns `main.sourceMap ?? null` from the non-Vite
carrier transform instead of discarding it. Absent a requested map the host publishes
none and the return is `null`.

**Gate.** `the_bundler_rollup_inline_transform_preserves_requested_source_maps`
(previously `#[ignore]`d) compares the published map to the host's own `Main` map for
the profile that request asked for — whole artifact, not an envelope check. The Vite
control `the_bundler_virtual_script_loads_publish_requested_source_maps` stays green and
the routing wrapper stays unmapped. Two live tests in
`packages/unplugin/src/index.spec.ts` add a JS-side regression plus a negative control
that fails if the map were attached to every transform return.

`packages/unplugin/scripts/probe-bundler-route.freshness.json` regenerated: the record
fingerprints production `src` plus the built `dist`, both of which moved.

### `RT-1` — the batch route selects each input carrier

The batch registered EVERY input under the Vue carrier regardless of the canonical id, so
a `.svelte` source in a batch was parsed and compiled by the Vue carrier: the route
published Vue-assembled bytes where the single-file route publishes a Svelte client
module, and neither Svelte runtime refusal could fire on that route at all.

**Code.** The language is derived from the canonical id by the host classifier — the same
authority every other session consumer reads — at one function that takes NO language
argument (`VerterHost::batch_upsert_request`), so a batch call site has nothing to get
wrong. The Stage-B skip decision consumes that BUILT request and compares source bytes AND
language row, because a canonical already holding those bytes under another carrier would
otherwise keep it for the whole batch: deriving the language is not enough if the decision
that skips the registration cannot see it.

**Gate.** `a_svelte_batch_matches_the_single_file_route_item_for_item` (previously
`#[ignore]`d) compares each item — published, refused, published, with the refusal in the
MIDDLE — against the single-file route for the same typed request.

The three tests that recorded the divergence now assert the corrected behaviour, each with
a NON-Svelte input in the same batch so a route that swapped one fixed carrier for another
fails them: carrier registration per input, both runtime refusals firing with the
single-file route's typed codes and no product beside them, and the same on the
host-backed lane. Two tests are added: the batch re-registers a canonical another carrier
left behind, and the derivation itself is read per path off the one function that performs
it.

**The edge this creates, recorded rather than discovered later.** An id that names no
carrier — no extension, an unknown extension, a `.ts` module — is no longer compiled into a
component module. That is the correct consequence of deriving the carrier from the path
(compiling a `.ts` path as a Vue SFC was the same defect), and `CompileBatchInput` has no
field with which a caller could state a language for a path that implies none. Both public
batch entry points document it and a test pins it with a compiling control. It is a
public-contract narrowing and is recorded here for ratification.

## Verification

| surface | invocation | result |
|---|---|---|
| transport + bundler suite | `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1` | `running 22 tests` → `22 passed; 0 failed; 0 ignored` |
| bundler JS regression | `pnpm --filter @verter/unplugin exec vitest --run src/index.spec.ts` | 97 passed, 1 skipped, 1 pre-existing failure (below) |
| wasm package | `pnpm --filter @verter/wasm exec vitest --run` | 16 passed |

WITHOUT `--features transport-authoritative` that filter matches ZERO tests and still
exits 0. Every run above was confirmed by its `running N tests` line, never its exit
status.

**The canonical gate**, run at landing readiness on the squashed tree
(`node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB`):
**VERDICT PASS, all three surfaces green** — 24391 run / 24391 passed on the
process-isolation surface, 3 suites clean in-process, 8624 run / 8624 passed on the
shipped-`cfg(debug_assertions)` surface.

Two earlier runs on the same tree failed and are recorded rather than dropped. The first
failed on `tracked_files_contain_no_machine_specific_path_markers`: this record's
companion embedded an absolute worktree path, which was removed. The second failed only
on `verter_type_runtime resilient::…::failed_respawn_retries_within_budget_and_recovers`,
a real-tsserver hover test this change does not touch: it passed in the run immediately
before, on a tree differing by one line of markdown, and standalone it failed once and
passed twice in three consecutive runs. It is flaky, not a regression.

Also green before the gate: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings`,
`cargo check --workspace --release`, `pnpm install --frozen-lockfile`.

The only change made after that PASS is this paragraph and the two above it; the guard
that reads tracked documentation was re-run green afterwards.

**RED before, GREEN after** — both targets, at the failure points the audit recorded:

- `the_transports_report_a_missing_node_the_same_way`: failed at the parity assertion
  with `napi {"outcome":"missing"}, wasm {"outcome":"error"}`; passes after.
- `the_bundler_rollup_inline_transform_preserves_requested_source_maps`: failed with
  `no source-map artifact was published (the map itself is null)` alongside
  `hostHasMap=true, publicTransformIsInline=true, publicTransformHasMap=false`; passes
  after.

**Discrimination proven by plant, each plant shown present, unique and new first:**
a wasm binding that answers the refusal class correctly and throws for the structural
class fails at the structural-absence parity assertion; a control that publishes
arbitrary bytes fails the host-byte comparison; a transform that returns `map: null`
fails the JS regression while the Vite negative control stays green; a map attached to
the Vite routing wrapper fails that control.

## Second pass — verification

| surface | invocation | result |
|---|---|---|
| batch route + derivation | `cargo test -p verter_session --lib -- svelte_batch_route the_batch_derives_each_inputs_language --test-threads=1` | `running 17 tests` → 15 passed, 2 ignored, 0 failed |
| whole session library | `cargo test -p verter_session --lib` | `running 6342 tests` → 5805 passed, 537 ignored, 0 failed |
| transports + bundler | `cargo test -p verter_session --lib --features transport-authoritative transport_route_equivalence -- --test-threads=1` | `running 22 tests` → 22 passed, 0 failed |
| session integration binaries | `cargo test -p verter_session --tests` | 2464 passed, 30 ignored, 0 failed (plus the library above) |

**RED before, GREEN after.** The target failed with Vue-assembled bytes for
`/batch/EqOne.svelte`, an absent refusal for `/batch/EqRefused.svelte`, a partial product
published beside it, and differing bytes for `/batch/EqTwo.svelte`; it passes after.

**Discrimination proven by plant, each shown present, unique and new first.** A fixed VUE
carrier fails 5 of the 14 batch tests; a fixed SVELTE carrier fails 8 — a different set,
which is the point: the item-for-item target alone cannot tell a correct route from one
pinned to Svelte, and the mixed-carrier tests are what do. A bytes-only skip predicate
fails the stale-carrier regression with `vue != svelte`. An always-upsert predicate is
caught by a submission count.

## Second pass — what the reachable refusals changed

Making Svelte classification reachable makes the Svelte runtime refusals reachable, so two
assertions in the transport suite that assumed every lane publishes bytes were re-measured:
the server lane refuses the Svelte carrier outright, and the refusal-shaped input is refused
on every lane. Those entries are now asserted to carry the typed refusal and no product,
while the non-contamination and source-map axes are asserted over the entries that publish,
with a guard that at least one does.

That suite ALSO no longer compiled: it still used the batch entry fields that a landed
sibling replaced with accessors, and being feature-gated it was not built by that block's
gate. It was migrated to the accessors in the same change — a suite that does not compile
proves nothing.

## Second pass — the canonical gate

Run at landing readiness on the squashed tree
(`node scripts/gate.mjs --test-threads 8 --memory-limit 18GiB`):
**VERDICT PASS, all three surfaces green** — 24417 run / 24417 passed on the
process-isolation surface, 3 suites clean in-process, 8638 run / 8638 passed on the
shipped-`cfg(debug_assertions)` surface.

An earlier run, before the rebase onto the Svelte correction block, failed on exactly one
non-tolerated test —
`verter_type_runtime resilient::resilient_tests::failed_respawn_retries_within_budget_and_recovers`
— and is recorded rather than dropped. It is discriminated as outside this change on three
grounds: `verter_type_runtime` does not depend on anything this change touches (its
dependencies are `verter_span`, `verter_language`, `verter_tsgo_api`; its dev-dependencies
`blake3`, `trybuild`, `tempfile`, `tokio` — neither `verter_session` nor `verter_napi` is
in its build graph); it passed four consecutive standalone re-runs on that same tree; and
it is a real-tsserver respawn/hover test already observed failing on the base by the
preceding block. It passed in the final run above.

Also green before the gate: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings`,
`cargo check --workspace --release`, `pnpm install --frozen-lockfile`, and the session
integration binaries (2464 passed, 30 ignored, 0 failed).

The only change made after that PASS is this section; the guard that reads tracked
documentation was re-run green afterwards.

## Second pass — the Svelte map re-measurement

The Svelte correction block landed while this one was gating, and this branch was rebased
onto it. Its map corrections flipped a characterization that this suite owns and that
explicitly named the flip as its own re-measure signal: the published Svelte
virtual-script map went from one segment naming no authored position to 7 generated
lines / 3 segments / 1 naming an authored position.

Re-measured rather than reverted, and strengthened while re-measuring:

* the characterization is now a FLOOR for BOTH carriers instead of an exact pin for Svelte
  and a floor for Vue, and it is renamed for the invariant it holds — that each published
  virtual-script map covers its output — rather than the divergence that no longer exists;
* the Svelte virtual-script product is promoted from the envelope-only oracle to the same
  MAPPED oracle the Vue product is held to, which the empty map could not have satisfied.

## Findings dispositioned

| finding | disposition | record |
|---|---|---|
| the parity gate covered only one of the two ways a request reaches "no product" | **ADOPT-NOW** | both transport probes gained a structurally absent case and a successful control; the target asserts the whole contract over both classes |
| the public API tables still promised a non-null virtual-file response | **ADOPT-NOW** | `docs/api/wasm.md`, `docs/api/native.md` |
| the SSR dead-code-elimination pass rewrites the compiled module with plain string replacement after the host produced its map, so published generated positions shift | **DEFER** | ruled DEFER on a separate scope consult. Owner: `packages/unplugin/src/core/ssr-transforms.ts` and its call site in `packages/unplugin/src/index.ts`. Gate: source-map-aware rewriting before release. Acceptance test: un-skip `maps a token that the SSR rewrite moved to its new generated column` in `packages/unplugin/src/index.spec.ts` and make its no-overshoot assertion pass. It pre-dates this change on the Vite path, which has always cached the same rewritten code beside the same host map; withholding the map when the pass fires is forbidden by the standing product ruling |
| `main module includes one style import per <style> block` fails with `StageRequiresPlainCss` | **REJECT as this change's defect** | reproduces with the base version of `packages/unplugin/src/index.ts` restored, on both Node 20.20.2 and 22.22.0, through a Vite-mode path this change does not touch. Pre-existing; owner is the style-preprocessing stage |
| the two bindings each decided the absent/failed split independently, which is how they drifted | **ADOPT-NOW** (structural hardening) | the decision moved into `verter_ffi::convert::classify_host_virtual_file`; both bindings consume its outcome. A `verter_ffi` unit test pins absent for a missing node and failure for `InvalidQuery` / `MissingSource` / `CompileError`, proven discriminating in both directions by plant |
| the playground's local `HostBinding` mirror still types `getVirtualFile` as non-null (`packages/playground/src/core/compiler.ts`) | **REJECT for now** | an internal mirror, not a published surface; the playground lists the nodes before requesting them, so it never reaches the absent answer. Widening it would open type-correctness work, which is waived for the program |

Zero open deferrals other than the one recorded row above, which carries its owner, gate
and acceptance test.

| a canonical already registered under another carrier kept it through a batch, because the upsert skip compared only the source bytes | **ADOPT-NOW** | the skip decision now consumes the BUILT registration and compares bytes and language; regression `a_batch_re_registers_a_canonical_that_was_left_under_another_carrier` compares a poisoned host against a fresh one given the same batch |
| an id naming no carrier is no longer compiled as a component | **ADOPT-NOW as a documented narrowing** | the correct consequence of deriving the carrier from the path; documented at both public batch entry points and pinned by a test with a compiling control. Recorded for ratification |
| the virtual-file route answers `missing source` for a file whose source IS registered but is not a carrier | **DEFER** | the taxonomy is that route's, and correcting it changes the error every consumer sees. Captured as `#[ignore]`d `a_non_carrier_batch_id_is_not_reported_as_a_missing_source`, proven to fail today, naming `host_resolve/virtual_file_pipeline.rs` as owner |
| the feature-gated transport suite no longer compiled against the landed batch entry API | **ADOPT-NOW** | migrated to the accessors; without it this block could not run its own gate |

## Reviews

| seat | agent | verdict |
|---|---|---|
| adversarial | `codex` gpt-5.6-sol @ high | **BLOCK** at first pass — four P1 and one P2; all dispositioned above. **CLEAR** on the first fix delta, with its own plants re-run |
| conformance | `grok` 4.6 @ xhigh, default-to-BLOCK | **LAND** — no admissible blocking finding; each exit criterion checked against the code and the built artifacts, not only the test |
| scope consult | `codex` gpt-5.6-sol @ high | **DEFER** ruled for the source-map shift, with owner, gate and acceptance test |
| contract check | `grok` 4.6 @ xhigh, default-to-BLOCK | **CONTRACT CORRECT** — see above |
| delta (structural hardening) | `codex` gpt-5.6-sol @ high | **BLOCK** on two P2s, both inaccurate CLAIMS rather than code defects: a residual generic status mapping meant "neither binding names the variant" was too strong, and this record still described the superseded implementation. Both corrected here and in the classifier's own documentation. Executable behaviour verified unchanged on both bindings, plants proven in both directions |


**Second pass.** adversarial `codex` gpt-5.6-sol @ high: **BLOCK** (one P1 — the cached-source
skip bypassing derivation — plus a public-contract P2 and a stale-comment P3), then **BLOCK**
on the fix delta for two stale comments only, with the functional half verified in both
directions including a submission-count probe of the warm path. conformance `grok` 4.6 @
xhigh, default-to-BLOCK: **LAND**, its strongest case against being exactly those stale
comments, all of which are corrected here.

The architecture mandate was not run: this is a subsystem-class block and no structural
doubt arose. Conformance and adversarial both ran.
