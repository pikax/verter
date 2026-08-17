# BRT0 — landing record

Partial landing. `TR-1` and `BND-2` are corrected and gated; `RT-1` is **not executed**
and remains open. Context in [`context-packet.md`](context-packet.md).

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

### `RT-1` — NOT EXECUTED

Its correction site is the active file of a concurrently running block, and the change
alters which responses reach that block's in-flight transaction construction. Held
rather than split by line range. The finding, its owner and its target test are
unchanged.

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

## Reviews

| seat | agent | verdict |
|---|---|---|
| adversarial | `codex` gpt-5.6-sol @ high | **BLOCK** at first pass — four P1 and one P2; all dispositioned above. **CLEAR** on the first fix delta, with its own plants re-run |
| conformance | `grok` 4.6 @ xhigh, default-to-BLOCK | **LAND** — no admissible blocking finding; each exit criterion checked against the code and the built artifacts, not only the test |
| scope consult | `codex` gpt-5.6-sol @ high | **DEFER** ruled for the source-map shift, with owner, gate and acceptance test |
| contract check | `grok` 4.6 @ xhigh, default-to-BLOCK | **CONTRACT CORRECT** — see above |
| delta (structural hardening) | `codex` gpt-5.6-sol @ high | **BLOCK** on two P2s, both inaccurate CLAIMS rather than code defects: a residual generic status mapping meant "neither binding names the variant" was too strong, and this record still described the superseded implementation. Both corrected here and in the classifier's own documentation. Executable behaviour verified unchanged on both bindings, plants proven in both directions |

The architecture mandate was not run: this is a subsystem-class block and no structural
doubt arose. Conformance and adversarial both ran.
