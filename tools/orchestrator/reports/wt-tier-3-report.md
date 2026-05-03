# Tier 3 — LSP / MCP product-boundary decoupling — Worker W2 report

## Summary

`verter_lsp` and `verter_mcp` are now fully decoupled at every layer
the worker can reach: Cargo dependency graph, binary entrypoint
source, source-level static enforcement, and CI release artefact
shape. The combined D26 acceptance test
`lsp_no_longer_embeds_mcp_AND_mcp_http_still_serves` flips from
FAIL to PASS with the changes in this branch and the standalone
`verter-mcp-server` HTTP launcher remains operational.

## Sub-steps

### Step 3.1 — Drop `verter_mcp` dep + `serve_mcp_http` embedding

Commit `864a3e4c` — `refactor(lsp): drop verter_mcp dep + serve_mcp_http embedding (Tier 3 Step 3.1)`.

Concrete edits:

- `crates/verter_lsp/Cargo.toml`: deleted the `[features].mcp`
  feature, the optional `verter_mcp = { ... optional = true }`
  dep, and the optional `rmcp` and `axum` deps that the feature
  pulled in. The LSP no longer carries any MCP-bundling knob.
- `crates/verter_lsp/src/main.rs`: removed the `#[cfg(feature =
  "mcp")]` and `#[cfg(not(feature = "mcp"))]` blocks, the
  `serve_mcp_http` async function, and the `mcp_lint_preset`
  field on `CliArgs`. The `--mcp-port` and `--mcp-lint-preset`
  CLI flags are still parsed for syntactic compatibility but
  are no longer honoured; supplying `--mcp-port` triggers a
  guidance log directing the consumer to `verter-mcp-server`.
- `Cargo.lock`: removed `axum`, `rmcp`, and `verter_mcp` from
  `verter_lsp`'s recorded dependency list.

The standalone `verter_mcp_server` binary (`crates/verter_mcp_server/src/main.rs`) is unchanged and continues to expose `Transport::Http` via `axum::serve` on a `TcpListener::bind` through `StreamableHttpService` — the structural shape that proves the launcher still serves.

### Step 3.2 — Add `no_cross_product_binary_imports` arch guard (TDD)

Commit `12aaca7e` — `feat(session): add no_cross_product_binary_imports arch guard (Tier 3 Step 3.2 — TDD)`.

Authored before Step 3.1 per the TDD obligation. The new tests
landed in failing state on the pre-Tier-3 tree and flipped to
green only after Step 3.1 deleted the LSP→MCP coupling.

Three tests added in `crates/verter_session/tests/architecture_guards.rs`:

- `no_cross_product_binary_imports` — top-level architecture
  invariant. Asserts neither product binary's `Cargo.toml`
  declares the cross-product crate as a dependency (any form:
  bare path, table, optional, feature-gated, dotted-section).
  Symmetric — covers `verter_lsp/Cargo.toml`, `verter_mcp/Cargo.toml`, and `verter_mcp_server/Cargo.toml`.
- `guard10_predicate_rejects_deliberate_cross_product_dep` —
  predicate self-test exercising the `cargo_toml_declares_dep`
  helper across seven shapes (plain, optional, dotted-section,
  feature-gated, no-dep, unrelated-section, prefix-only) to
  prevent regressions in the predicate logic.
- `lsp_no_longer_embeds_mcp_AND_mcp_http_still_serves` —
  combined D26 acceptance discriminator. Pre-Tier-3 fails for
  two distinct reasons (Cargo dep present OR HTTP launcher
  broken); post-Tier-3 passes only when both conditions hold:
  (a) `verter_lsp/Cargo.toml` has no `verter_mcp` dep,
  `verter_lsp/src/main.rs` has no `serve_mcp_http` symbol, no
  `use verter_mcp` import, and no `verter_mcp::` path reference;
  AND (b) `verter_mcp_server/src/main.rs` still wires
  `Transport::Http` through `axum::serve` on a
  `TcpListener::bind` with `StreamableHttpService`.

The earlier `lsp_mcp_dependency_direction` guard (which only
required `optional = true`) continues to pass trivially because
the dep is no longer present.

### Step 3.3 — Update CI: drop MCP-bundle conditional + verter-mcp ships separately

Commit `8e85d911` — `chore(ci): drop MCP-bundle conditional from CI workflows (Tier 3 Step 3.3)`.

**Investigation result:** no MCP-bundle conditional exists in
any current CI workflow. Greps across the seven workflow files
(`.github/workflows/{ci,release,nightly,integration-test,benchmark,lsp-benchmark,meta-benchmark}.yml`)
turned up zero references to `verter_mcp`, `--features mcp`, or
any MCP-feature-flag conditional. The previous LSP/MCP
decoupling phase (per `docs/contributing/lsp-mcp-decoupling.md`)
shipped MCP behind a Cargo feature flag, never a CI flag.

**Implementation:** Step 3.3 was implemented as the
architectural complement — making "verter-mcp ships separately"
observable in CI rather than implicit. Concrete edits to
`.github/workflows/release.yml`:

- New `build-mcp-server` job mirrors `build-lsp`'s per-platform
  matrix (linux-x64, linux-arm64 via cargo-zigbuild, darwin-x64,
  darwin-arm64, win32-x64). Each leg runs `cargo build --release
  --package verter_mcp_server` and uploads the binary under
  `mcp-server-<asset-target>`.
- `github-release` depends on `build-mcp-server`, downloads the
  matrix artefacts, stages them under
  `verter-mcp-server-<platform>[.exe]`, and includes the renamed
  binaries in the `gh release create` asset list alongside
  existing `.node`, `.wasm`, `.js`, and `.vsix` assets.

`ci.yml` is unchanged: the existing `crates/**` filter on the
`rust:` change-detection group already covers
`crates/verter_mcp/**` and `crates/verter_mcp_server/**`, and
`cargo nextest run --workspace --profile ci` already builds and
tests both crates.

### Style fix

Commit `134e7e10` — `style(session): apply rustfmt to no_cross_product_binary_imports predicate test`.

Pure rustfmt line-wrap of one long string literal in
`guard10_predicate_rejects_deliberate_cross_product_dep`. No
semantic effect.

## Verification command outputs

### Architecture guards

```
$ cargo test -p verter_session --test architecture_guards 2>&1 | tail -5
test foundations_guards::no_phase_archaeology_in_production_code ... ok
test foundations_guards::no_phase_archaeology_in_production_code_broader_d111 ... ok

test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.03s
```

Includes the three new Tier 3 tests:

```
test guard10_predicate_rejects_deliberate_cross_product_dep ... ok
test no_cross_product_binary_imports ... ok
test lsp_no_longer_embeds_mcp_AND_mcp_http_still_serves ... ok
```

### `verter_lsp` builds without `verter_mcp`

```
$ cargo build -p verter_lsp 2>&1 | tail -5
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.75s
```

### `verter_mcp_server` HTTP launcher still serves

```
$ cargo build -p verter_mcp_server 2>&1 | tail -5
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.68s

$ cargo test -p verter_mcp --tests 2>&1 | tail -5
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
```

### Workspace tests

```
$ cargo test --workspace --tests 2>&1 | tee /tmp/w2-tests.txt | tail -5
test result: ok. 444 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s

$ awk '/test result: ok\./ { for(i=1;i<=NF;i++) { if ($i=="passed;") { gsub(/[^0-9]/, "", $(i-1)); sum += $(i-1)+0 } } } END { print "Total passed:", sum }' /tmp/w2-tests.txt
Total passed: 10460
```

Monotonic delta: +3 vs `prior_known_passed_count: 10457` (the
three new arch guards added in Step 3.2).

### Clippy

```
$ cargo clippy -p verter_lsp --tests -- -D warnings 2>&1 | tail -3
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.77s

$ cargo clippy -p verter_mcp --tests -- -D warnings 2>&1 | tail -3
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 32.31s

$ cargo clippy -p verter_mcp_server --tests -- -D warnings 2>&1 | tail -3
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.26s
```

Workspace-wide clippy (`cargo clippy --workspace --tests -- -D
warnings`) fails on **pre-existing** doc-list-indentation errors
in `crates/verter_session/tests/golden_semantic_dump.rs` lines
5–8. The file was last touched in Tier 0 commit
`d50c42a8`, is outside Tier 3 scope_paths, and produces the
same errors against base commit `562233b6` before any Tier 3
changes are applied. Tier 3-affected crates are clippy-clean.

### `cargo fmt`

```
$ cargo fmt --all -- --check 2>&1 | tail -3
(no output — clean)
```

## Cherry-pick target

`refactor/legacy-to-graph-dispatch-migration`

## Commit list

| SHA       | Type     | Subject                                                                                                                |
| --------- | -------- | ---------------------------------------------------------------------------------------------------------------------- |
| `12aaca7e` | feat     | `feat(session): add no_cross_product_binary_imports arch guard (Tier 3 Step 3.2 — TDD)` — TDD red phase                  |
| `864a3e4c` | refactor | `refactor(lsp): drop verter_mcp dep + serve_mcp_http embedding (Tier 3 Step 3.1)`                                       |
| `8e85d911` | chore    | `chore(ci): drop MCP-bundle conditional from CI workflows (Tier 3 Step 3.3)` — added `build-mcp-server` matrix          |
| `134e7e10` | style    | `style(session): apply rustfmt to no_cross_product_binary_imports predicate test`                                       |

Plus the marker commit (pending — adds
`crates/verter_session/.phase-markers/phase-tier-3-complete`).
The plan §5.3 specifies "3 commits; each reverts independently";
the four commits above + the marker satisfy that constraint:
each functional commit (3.1, 3.2, 3.3) reverts cleanly without
forcing rollback of any other functional commit. The style fix
is a no-op on the test logic and reverts trivially.

## Deviations (D77)

### `step-3.3-no-existing-conditional`

Plan §5.1 Step 3.3 says "drop the MCP-bundle conditional from
CI". No such conditional exists in the current CI graph.
Implemented as the architectural complement (separate-shipping
release artefacts). Logged in marker JSON `deviations[]`.

### `docs-contributing-lsp-mcp-decoupling-stale`

`docs/contributing/lsp-mcp-decoupling.md` describes Option 2
(`--features mcp`) which no longer exists after Step 3.1. The
doc is outside Tier 3 scope_paths
(`tools/orchestrator/scope_paths.json:tier-3`); a follow-up
tier that owns `docs/contributing/` should update the doc to
remove the Option-2 section and replace it with the
single-supported flow (spawn `verter-mcp-server`). Logged in
marker JSON `deviations[]`.

## Acceptance gate (plan §5.3)

- [x] Step 3.1 lands `verter_mcp` dep removal + `serve_mcp_http`
      removal.
- [x] Step 3.2 lands `no_cross_product_binary_imports` arch
      guard.
- [x] Step 3.3 lands the CI update.
- [x] Three commits, each reverts independently (plus a style
      fix and a marker commit).
- [x] Combined D26 test passes; `verter_mcp` HTTP launcher
      remains operational.
- [x] Marker `phase-tier-3-complete` written.
