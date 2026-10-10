#!/usr/bin/env node

/**
 * ci-impact.mjs — the one lane-selection authority for ci.yml.
 *
 * `detect-changes` hands this module the changed-file list and publishes the
 * job gates it computes. Selection has two halves, both owned here:
 *   - the PATH half (`PATH_FILTERS`): every non-crate input is owned by the
 *     filters of the lanes that read it — a script, a fixture tree, a tool
 *     config, a package;
 *   - the CRATE half (`LANE_ROOTS`): a path filter can only say `crates/**`,
 *     which would run every artifact lane — the release NAPI binding, the
 *     wasm32 build and browser gate, the debug and release LSP builds with the
 *     VS Code shards and editor contracts behind them — for a change to a
 *     crate none of those artifacts link. A lane is impacted instead when a
 *     directly changed crate is in the transitive dependency closure of one
 *     of that lane's root crates, as `cargo metadata` reports it.
 *
 * Two kinds of gate come out of it (`LANE_GATES`):
 *   - a SUBSYSTEM gate ("the native binding changed", "wasm changed") is the
 *     OR of the lane's path filters and its impact lanes, and drives the
 *     suites that prove that subsystem;
 *   - an ARTIFACT gate ("the native artifact is needed") is the OR of the
 *     gates of every consumer of that artifact, and drives the producer job,
 *     so a consumer can never be scheduled behind a producer that was not.
 *
 * Fail closed, never under-select:
 *   - `ESCAPE_HATCHES` that change how every crate builds (workspace
 *     manifests and lockfile, nextest config, the toolchain pin and `.cargo/`,
 *     proc-macro crates, the foundational identity crate) turn every
 *     impact-bearing lane on; the ones that change what CI runs or how this
 *     selection is made (ci.yml, the local actions, this classifier and the
 *     crate-graph library) turn EVERY lane on;
 *   - a changed file that maps to no crate, is owned by no path filter and is
 *     not on the explicit CI-inert list (`CI_INERT_PATHS`) turns every lane
 *     on too; no directory is inert by assumption — `schemas/` and
 *     `test-corpora/` hold files Rust tests read, so their owner is the rust
 *     filter, not silence. A test holds every tracked file owned or inert, so
 *     the fallback is reached only by a path nobody has classified yet;
 *   - a filter pattern that is not a plain positive glob is refused, and a
 *     lane root that is not a workspace member is a configuration error: both
 *     refuse to run rather than yield a silently wrong lane;
 *   - if `cargo metadata` itself cannot be read the CLI reports a full
 *     fallback with a warning rather than a narrowed selection.
 *
 * The lane roots are derived from the lanes' own inventories where one
 * exists (the provider selectors, the compile-contract owners), so the test
 * authority and the selection authority cannot drift apart.
 *
 * Usage (CI; the file holds the changed-file list as a JSON array):
 *   node scripts/ci-impact.mjs --changed-files "$RUNNER_TEMP/changed-files.json" \
 *     --github-output "$GITHUB_OUTPUT" --summary "$GITHUB_STEP_SUMMARY"
 * The list comes from a file, never the environment or argv: on a large diff
 * it exceeds Linux's 128 KiB cap on one environment string and the process
 * cannot be started at all. CI_IMPACT_CHANGED_FILES_JSON remains for small
 * local runs.
 * Usage (local, the gates CI would publish for a range):
 *   node scripts/ci-impact.mjs --range origin/main..HEAD
 * Usage (CI and local, the configuration against the tree — every tracked
 * file owned, every glob live; exits 1 naming each problem):
 *   node scripts/ci-impact.mjs --audit
 */

import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import process from "node:process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, resolve } from "node:path";

import {
  buildReverseDependencyGraph,
  buildWorkspaceIndex,
  mapPathToCrate,
  transitiveDependents,
} from "./lib/crate-graph.mjs";
import { PROVIDER_LIVE_SELECTORS } from "./provider-ci-internals.mjs";

/**
 * The compile-contract lane's owners (`node scripts/compile-contracts.mjs
 * --list-owners`) and the crate each owner's fixtures compile against. The
 * runner crate declares these as features with no Cargo edge, so the graph
 * alone cannot see them; `ci-impact.test.mjs` pins this map to the runner's
 * own owner list.
 */
export const COMPILE_CONTRACT_OWNER_CRATES = Object.freeze({
  audit: "verter_audit",
  "compiler-default": "verter_compiler",
  "css-syntax": "verter_css_syntax",
  identity: "verter_identity",
  language: "verter_language",
  semantic: "verter_semantic",
  session: "verter_session",
  "session-query": "verter_session_query",
  "type-runtime": "verter_type_runtime",
  workspace: "verter_workspace",
});

/** Every package the serial provider lanes own tests in. */
export const PROVIDER_PACKAGES = Object.freeze([
  ...new Set(PROVIDER_LIVE_SELECTORS.map((selector) => selector.package)),
]);

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * The sfc-projection fixtures and probes verter_compiler's own tests
 * `include_str!` (the vue_bridge, vue_projection and backend suites, and
 * verter_session's host-resolve tests for the STP12 fixtures). Every lane that
 * runs those tests reads them: the core Rust suite, the compile-contract
 * lane's isolated lib tests, the feature-gated Svelte oracle (which runs every
 * verter_compiler test) and the verifiers themselves.
 */
const RUST_READ_SFC_FIXTURES = Object.freeze([
  "tests/sfc-projection/STP12/fixtures/**",
  "tests/sfc-projection/STP15/probes/components/**",
  "tests/sfc-projection/STP16/probes/components/**",
  "tests/sfc-projection/STP18/probes/**",
  "tests/sfc-projection/STP19/probes/**",
  "tests/sfc-projection/STP20/probes/**",
  "tests/sfc-projection/STP32/probes/**",
]);

const STP1_INVENTORY = "tests/sfc-projection/STP1/products/current-feature-inventory.json";

/**
 * The fixtures and benchmark manifests the STP1 inventory selects. The
 * sfc-projection verifier fails when one of them is missing, so a change that
 * moves or deletes one must run it. Read from the inventory itself, so the
 * filter and the coverage it guards cannot drift apart.
 *
 * Never throws: the ARH verifiers, the lane self-tests and the specs that
 * import this module must keep working when a change breaks or moves the
 * inventory. The problem is recorded instead (`SELECTION_CONFIG_ERRORS`):
 * the gates then run every lane, and the audit fails naming the file.
 */
function readStp1Inventory(root) {
  // Literal repository paths only, checked here rather than by `compileGlob`,
  // whose pattern constant is not initialised yet while this module loads.
  const literal = (path) =>
    typeof path === "string" && path !== "" && !/[*!?[\]{}\\]|^\.?\/|\/$|\/\//.test(path);
  try {
    const inventory = JSON.parse(readFileSync(resolve(root, STP1_INVENTORY), "utf8"));
    const paths = [...(inventory?.rows ?? []), ...(inventory?.benchmarkManifests ?? [])].map(
      (entry) => entry?.path,
    );
    if (paths.length === 0 || !paths.every(literal)) {
      throw new Error("expected a literal repository path for every selected fixture");
    }
    return { paths: [...new Set(paths)].sort(), error: null };
  } catch (error) {
    return { paths: [], error: `${STP1_INVENTORY}: ${error.message}` };
  }
}

const STP1 = readStp1Inventory(REPO_ROOT);

/**
 * Problems deriving the selection from repository data. Each one means a
 * filter may be missing paths, so the gates run every lane and the audit fails.
 */
const SELECTION_CONFIG_ERRORS = Object.freeze(STP1.error ? [STP1.error] : []);

/**
 * The non-crate inputs of each lane: the path half of selection.
 *
 * A filter is a list of globs, or `{ include, exclude }` when part of an
 * included tree belongs to another lane. A changed file hits a filter when it
 * matches an include and no exclude. `**` spans whole path segments, `*` stays
 * inside one segment, and a dot-file matches like any other name.
 *
 * Only plain positive globs are accepted (`compilePathFilters` refuses the
 * rest). The filters used to be paths-filter YAML, where a `'!dir/**'` line
 * under the default `some` quantifier matched every file OUTSIDE `dir`: the js
 * filter's playground exclusion turned the js and native lanes on for every
 * change, docs included, and hid every path no filter owned. An exclusion is
 * now its own field and can only remove what its includes matched.
 *
 * A file a crate owns is classified by the crate half too; a filter names one
 * only when a lane reads it outside the crate graph (a feature-gated harness,
 * a corpus a script checks). Each entry is here because a command the lane
 * runs reads that file; a file only another workflow reads is CI-inert below.
 */
export const PATH_FILTERS = Object.freeze({
  rust: [
    "crates/**",
    // Read by Rust tests (`verter_source_policy_gate` validates the schemas;
    // the style-IR corpora are read and `include_str!`-ed), so this lane owns
    // them; nothing else does.
    "schemas/**",
    "test-corpora/**",
    ...RUST_READ_SFC_FIXTURES,
    // verter_session's STS0 profile gate reads the policy and the evidence
    // every profile names.
    "tests/sfc-projection/STS0/products/svelte-projection-policy.json",
    "tests/sfc-projection/STS0/probes/**",
    // verter_analysis_inputs parses the example analysis config, requires the
    // private paths to be ignored, and scans committed source maps, the
    // changelog, its generator config and the deviation ledger for leaked
    // analysis paths.
    ".analysis/**",
    ".gitignore",
    "**/*.map",
    "CHANGELOG.md",
    "cliff.toml",
    // The docs Rust tests read: the audit footprint pages, the
    // signature-kernel page and evidence manifest, and the generated typeinfo
    // row counts the manifest generator byte-checks; and the component-meta
    // skill the audit-docs test cross-checks.
    "docs/audit-footprint/**",
    "docs/arch/signature-kernel.md",
    "docs/evidence/signature-kernel/**",
    "docs/generated/**",
    ".claude/skills/component-meta/SKILL.md",
    // Lint and format configuration (rust-clippy, rust-build-configs, rust-fmt).
    "clippy.toml",
    "rustfmt.toml",
    // Scripts Rust tests execute: the corpus-audit and typeinfo-manifest
    // generators (and the manifests they read), and the generated-artifact
    // freshness check with its generators. The archive build runs under the
    // cache wrapper.
    "scripts/gen-corpus-audit-tests.mjs",
    "scripts/gen-typeinfo-ignore-manifest.mjs",
    "scripts/manifests/**",
    "scripts/check-compiler-generated-artifacts.mjs",
    "scripts/generate-svelte-*.mjs",
    "scripts/run-cached.mjs",
    // verter_validation_probe's lane contract reads its own workflow.
    ".github/workflows/validation-probe.yml",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/**",
    ".config/nextest.toml",
    "rust-toolchain.toml",
    "scripts/check-integration-test-layout.mjs",
    "scripts/integration-test-layout-allowlist.json",
    "scripts/gate*.mjs",
    // Dedicated BF2 lane authority. A driver/self-test-only change must run
    // the lane it can weaken rather than passing through a skipped required
    // job.
    "scripts/bf2-authoritative.mjs",
    "scripts/bf2-ci-lane-selftest.mjs",
    "scripts/compiler-ci-lane-selftest.mjs",
    // The compile-contract lane's own selection: this script is the only
    // place the owner set it iterates is declared, so dropping an owner from
    // it silently narrows what that lane runs.
    "scripts/compile-contracts.mjs",
    "scripts/provider-ci*.mjs",
    "packages/framework-conformance-harness/**",
    "packages/language-shared/**",
    "packages/svelte-jsx/**",
    // BF2's workspace-domain TypeScript observation resolves both
    // @verter/svelte-jsx and @verter/types.
    "packages/types/**",
    "packages/typescript-plugin/**",
    ".npmrc",
    ".nvmrc",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
  ],
  proto: [
    "crates/verter_protocol/proto/**",
    "packages/proto/src/gen/**",
    "buf.gen.yaml",
    "scripts/check-proto-generated.mjs",
    // oxfmt reads the repository's ignore files from its working directory.
    ".prettierignore",
    ".gitignore",
    "package.json",
    "pnpm-lock.yaml",
  ],
  // The JavaScript workspace. It also drives the native binding's suites
  // (`native` below), which run the workspace packages against the real
  // binding, so a file only js-build-test reads belongs in `scripts`.
  js: {
    include: [
      "packages/**",
      "package.json",
      "pnpm-lock.yaml",
      "pnpm-workspace.yaml",
      // A workspace member: its manifest must still install from the frozen
      // lockfile, though nothing in CI builds the docs site.
      "docs/package.json",
      "tsconfig*.json",
      // The root vitest config: every repo-root vitest run, and the runs in
      // packages without a config of their own (vitest searches upward).
      "vitest.config.ts",
      // @verter/benchmark's perf-gate spec (native-test) reads its workflow.
      ".github/workflows/perf-gate.yml",
      "scripts/gen-svelte-goldens.mjs",
      "scripts/sccache-env.mjs",
      "scripts/sccache-env.test.mjs",
      // Same-file pairing as sccache-env: the layout test guards its
      // implementation and allowlist, so checker- or allowlist-only edits
      // must still run test:scripts.
      "scripts/check-integration-test-layout.mjs",
      "scripts/integration-test-layout-allowlist.json",
      "scripts/check-integration-test-layout.test.mjs",
      "scripts/compiler-ci-lane-selftest.mjs",
      "scripts/provider-ci*.mjs",
      // The lane classifier and the crate-graph library it composes gates
      // from: their unit tests run in test:scripts:ci.
      "scripts/ci-impact*.mjs",
      "scripts/lib/crate-graph*.mjs",
      "scripts/svelte-golden-lib.mjs",
      "scripts/browser-host-gate.mjs",
      "scripts/browser-host-gate.test.mjs",
      "crates/verter_compiler/tests/svelte_oracle_corpus/**",
      // test:scripts validates the required BF2 dependencies in the release
      // graph, and the native platform-matrix spec reads the release
      // workflow; release-only changes must execute both.
      ".github/workflows/release.yml",
    ],
    // The playground's own lane builds and tests it.
    exclude: ["packages/playground/**"],
  },
  // What only js-build-test reads: the repository scripts, whose self-tests
  // (`test:scripts:ci`) import one another freely, and the files those
  // self-tests and the job's release-packaging and binary-launcher specs
  // check — the lane-parsed workflows, the Netlify config and the ignore
  // rules. A script a lane executes is named in that lane's filter as well.
  scripts: [
    "scripts/**",
    ".github/workflows/nightly.yml",
    ".github/workflows/release-check.yml",
    ".github/workflows/release-ide.yml",
    ".github/workflows/release-tag.yml",
    "netlify.toml",
    ".gitignore",
  ],
  // Gates the `architecture-health` lane (ARH0/ARH1/ARH2/ARH4/ARH5/ARH7
  // verifiers). Their products join the live Rust tree — hotspot sources,
  // crate Cargo manifests and the cross-crate consumer population derived
  // from EVERY crate — so a product/verifier-only edit under
  // tests/architecture-health/ and any Rust dependency or surface change must
  // both run the guard. ARH2 also reads performance-gates.toml and imports
  // scripts/validate-performance-gates.mjs to bind runner class and
  // measurement cells. ARH7 joins packages/vue-vscode activation sources, so
  // an extension-only edit must still schedule this lane. ARH4 joins its API
  // migration notes to the audit API reference.
  arch: [
    "tests/architecture-health/**",
    // ARH5's public-example guard scans examples/reference for the displaced
    // VueCarrierCompiler compile_ide/compile_bundle routes, so example-only
    // edits must still schedule this lane.
    "examples/reference/**",
    "packages/vue-vscode/**",
    "docs/audit-footprint/api-reference.md",
    // ARH0 binds the MCP build identity and a debt-register candidate; ARH7
    // reads the VS Code product inventory and shared boundary; ARH4 requires
    // its perf evidence run to stay absent from the evidence-run directory.
    "mcp/verter.mcp.json",
    "STATUS-vue-collapse.md",
    "tests/vscode-product/VSC0/products/vscode-product-inventory.v1.json",
    "tests/vscode-product/VSC0/products/desktop-web-shared-boundary.v1.json",
    "scripts/benchmark/evidence-runs/**",
    "crates/**",
    "Cargo.toml",
    "Cargo.lock",
    "performance-gates.toml",
    "scripts/validate-performance-gates.mjs",
  ],
  // The non-crate inputs of the wasm32 artifact (`wasm-build`) and the
  // browser gate behind it; the crate half is verter_wasm's dependency
  // closure. Manifests and the lockfile are listed too (and are escape
  // hatches) because a dependency bump moves compiler output without
  // touching a single crate path.
  wasm: [
    // wasm-build verifies the committed carrier fixture against the bytes it
    // just built, runs the wasm32 JavaScript-boundary lane through the gate's
    // own helpers, optimizes through the wasm-opt cache and runs the
    // package's tests under the root vitest config.
    "packages/playground/scripts/capture-wasm-carrier-fixtures.mjs",
    "scripts/wasm-js-boundary-lane.mjs",
    "scripts/gate-internals.mjs",
    "scripts/wasm-opt-cache.mjs",
    "vitest.config.ts",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "packages/wasm/**",
    "packages/browser-host/**",
    "scripts/browser-host-gate.mjs",
    "scripts/browser-host-gate.test.mjs",
    "tests/browser-host/**",
    ".cargo/**",
  ],
  playground: ["packages/wasm/**", "packages/playground/**"],
  vscode: [
    "packages/vue-vscode/**",
    "packages/language-shared/**",
    "packages/typescript-plugin/**",
  ],
  dx: [
    "packages/dx-harness/**",
    "packages/lsp-test-client/**",
    // The tsserver carrier-membership plugin and the library it emits
    // against. The raw-LSP spine smoke BUILDS both as load-bearing
    // prerequisites and gets no carrier semantics without them, so a change
    // that breaks the plugin must trip this filter — otherwise the job that
    // would catch it is skipped for that very change.
    "packages/typescript-plugin/**",
    "packages/language-shared/**",
    "scripts/dx/**",
    // The harness unit suite checks the JetBrains comparison workflows, the
    // recorded Lapce captures and the replacement-deviation ledger.
    "tests/jetbrains-baseline/JBT0/products/required-workflow-matrix.v1.json",
    "tests/workspace-responsiveness/WSP1B/products/**",
    "tests/workspace-responsiveness/WSP1L/products/**",
    "scripts/manifests/replacement-deviations*",
  ],
  // Dependency closure for the editor-neutral raw-LSP contract. This is
  // intentionally broader than either the VS Code or DX-only filters: the
  // contract drives the real compiler/LSP, provider plugin, relay, and shared
  // typed test substrate without an editor process.
  editor_lsp: [
    "packages/dx-harness/**",
    "packages/lsp-test-client/**",
    "packages/language-shared/**",
    "packages/native/**",
    "packages/svelte-jsx/**",
    "packages/typescript-plugin/**",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "tsconfig*.json",
    "Cargo.toml",
    "Cargo.lock",
  ],
  svelte_oracle: [
    "scripts/gen-svelte-goldens.mjs",
    "scripts/svelte-golden-lib.mjs",
    "scripts/gen-svelte-name-parity-corpus.mjs",
    // The feature-gated parse-parity and reject matrices run their oracles
    // and corpus generator.
    "scripts/gen-svelte-parse-parity-corpus.mjs",
    "scripts/svelte-parse-parity-oracle.mjs",
    "scripts/svelte-reject-oracle.mjs",
    "crates/verter_compiler/tests/svelte_oracle_corpus/**",
    "crates/verter_compiler/tests/fixtures/svelte/name_parity_corpus.json",
    "crates/verter_compiler/tests/fixtures/svelte/remove_typescript_nodes.5.56.10.js",
    "crates/verter_compiler/tests/cases/svelte_oracle_harness.rs",
    "crates/verter_compiler/tests/cases/svelte_goldens_in_sync.rs",
    "crates/verter_compiler/src/svelte_oracle.rs",
    "crates/verter_compiler/Cargo.toml",
    ...RUST_READ_SFC_FIXTURES,
    "pnpm-lock.yaml",
  ],
  svelte_client_smokes: [
    "crates/verter_compiler/src/svelte/**",
    "packages/svelte-runtime-tests/**",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
  ],
  // Editor-client integration surface (Helix / Lapce / Zed / Neovim). The
  // four lanes share the single `build-editor-lsp` artifact; the crate half
  // of this gate is the verter_lsp + verter-editor-client dependency closure.
  // The Lapce and Zed extensions' clippy and rustfmt runs inherit the root
  // configuration.
  editors: [
    "editors/**",
    "extensions/lapce/**",
    "extensions/zed/**",
    "scripts/editor-contracts/**",
    "packages/lsp-test-client/**",
    "clippy.toml",
    "rustfmt.toml",
    "Cargo.toml",
    "Cargo.lock",
  ],
  // JetBrains plugin lane: the pinned Gradle build resolves its own WebStorm
  // SDK; only the plugin tree, its gate and the supported-IDE declaration
  // feed it. The Node pin and the gradlew line-ending attribute are checkout
  // and runtime inputs of its Gradle wrapper run.
  jetbrains: [
    "extensions/jetbrains/**",
    "scripts/jetbrains-gate.mjs",
    "scripts/jetbrains-gate.test.mjs",
    "tests/jetbrains-product/**",
    ".nvmrc",
    ".gitattributes",
  ],
  svelte_perf: [
    "packages/benchmark/package.json",
    "packages/benchmark/src/svelte-perf-*.ts",
    "packages/benchmark/src/svelte-perf-manifest.json",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "Cargo.toml",
    "Cargo.lock",
  ],
  // Non-crate inputs of the serial real-provider lane; its crate half is the
  // verter_lsp / verter_type_runtime / verter_tsgo_api closure. The tsserver
  // lane builds its plugin through the test helper, and the native-free
  // plugin specs run under the root vitest config.
  providers: [
    "packages/typescript-plugin/**",
    "packages/language-shared/**",
    "scripts/provider-ci*.mjs",
    "scripts/build-test-typescript-plugin.mjs",
    "vitest.config.ts",
    ".npmrc",
    ".nvmrc",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
  ],
  // Non-crate inputs of the native/WASM transport equivalence check (both
  // probes live in their packages); its crate half is either artifact's
  // dependency closure.
  transport: ["packages/wasm/**", "packages/native/**"],
  // Non-crate inputs of the compile-contract lane: its runner and the lane
  // self-test, the generated-artifact freshness check it runs and that
  // check's generators, and the sfc-projection files the isolated
  // verter_compiler lib tests read. Its crate half is the runner crates plus
  // every owner whose fixtures the runner compiles.
  compiler_contracts: [
    "scripts/compile-contracts.mjs",
    "scripts/compiler-ci-lane-selftest.mjs",
    "scripts/check-compiler-generated-artifacts.mjs",
    "scripts/generate-svelte-*.mjs",
    ...RUST_READ_SFC_FIXTURES,
  ],
  // Non-crate inputs of the BF2 authoritative lane (its workspace-domain
  // TypeScript observation resolves @verter/svelte-jsx and @verter/types);
  // its crate half is verter_session's dependency closure.
  bf2: [
    "packages/framework-conformance-harness/**",
    "packages/language-shared/**",
    "packages/svelte-jsx/**",
    "packages/types/**",
    "packages/typescript-plugin/**",
    "scripts/bf2-authoritative.mjs",
    "scripts/bf2-ci-lane-selftest.mjs",
    "scripts/gate*.mjs",
    ".npmrc",
    ".nvmrc",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
  ],
  // The sfc-projection node verifiers. Each node's protocol runs
  // `cargo test` in verter_compiler or verter_session and reads verter_language
  // and verter_validation_probe sources (the crate half). The nodes typecheck
  // their probes on both TypeScript engines, pinned by the root and
  // playground manifests and the lockfile; STS0 joins the Svelte profile lock
  // to the conformance harness's evidence and fixtures and typechecks against
  // @verter/svelte-jsx; STP1 requires every fixture its inventory selects.
  sfc_projection: [
    "tests/sfc-projection/**",
    "scripts/sfc-projection/**",
    "packages/framework-conformance-harness/evidence/**",
    "packages/framework-conformance-harness/fixtures/svelte/**",
    "packages/svelte-jsx/**",
    "packages/playground/package.json",
    ...STP1.paths,
    ".npmrc",
    ".nvmrc",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
  ],
});

const UNSUPPORTED_GLOB_SYNTAX = /[!?[\]{}\\]/;

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Compiles one filter glob. Refuses anything but a plain positive,
 * repository-relative glob: a negation (`!`), character classes, braces and
 * `?` would each change what a filter selects without saying so.
 */
export function compileGlob(glob) {
  if (typeof glob !== "string" || glob === "") {
    throw new Error(
      `ci-impact: a filter glob must be a non-empty string, got ${JSON.stringify(glob)}`,
    );
  }
  if (UNSUPPORTED_GLOB_SYNTAX.test(glob)) {
    throw new Error(
      `ci-impact: glob "${glob}" uses unsupported syntax; write a plain positive glob and put exclusions in the filter's \`exclude\``,
    );
  }
  if (glob.startsWith("/") || glob.startsWith("./") || glob.endsWith("/")) {
    throw new Error(`ci-impact: glob "${glob}" must be a repository-relative file glob`);
  }
  const segments = glob.split("/");
  let source = "";
  segments.forEach((segment, i) => {
    const last = i === segments.length - 1;
    if (segment.includes("**") && segment !== "**") {
      throw new Error(`ci-impact: glob "${glob}" may use \`**\` only as a whole path segment`);
    }
    if (segment === "") throw new Error(`ci-impact: glob "${glob}" has an empty path segment`);
    if (segment === "**") {
      source += last ? ".*" : "(?:[^/]*/)*";
      return;
    }
    source += segment.split("*").map(escapeRegExp).join("[^/]*");
    if (!last) source += "/";
  });
  return new RegExp(`^${source}$`, "u");
}

/** Validates and compiles `PATH_FILTERS`-shaped filters; throws on any malformed entry. */
export function compilePathFilters(filters) {
  const compiled = new Map();
  for (const [name, spec] of Object.entries(filters)) {
    const { include, exclude = [] } = Array.isArray(spec) ? { include: spec } : spec;
    if (!Array.isArray(include) || include.length === 0 || !Array.isArray(exclude)) {
      throw new Error(
        `ci-impact: filter "${name}" must be a non-empty glob list, or { include, exclude } glob lists`,
      );
    }
    compiled.set(name, { include: include.map(compileGlob), exclude: exclude.map(compileGlob) });
  }
  return compiled;
}

const COMPILED_PATH_FILTERS = compilePathFilters(PATH_FILTERS);

/**
 * The path half of selection: which filters the changed files hit, and which
 * files some filter owns.
 *
 * @param {string[]} changedFiles workspace-relative, forward-slash paths
 * @returns {{hits: Record<string, boolean>, owned: Set<string>}}
 */
export function matchPathFilters(changedFiles, filters = COMPILED_PATH_FILTERS) {
  const compiled = filters instanceof Map ? filters : compilePathFilters(filters);
  const hits = {};
  const owned = new Set();
  for (const [name, { include, exclude }] of compiled) {
    hits[name] = false;
    for (const file of changedFiles) {
      if (!include.some((re) => re.test(file)) || exclude.some((re) => re.test(file))) continue;
      hits[name] = true;
      owned.add(file);
    }
  }
  return { hits, owned };
}

/**
 * The crates each impact lane is built from. A lane is impacted when a
 * changed crate is one of these or anything they transitively depend on.
 * Every name must be a workspace member; `classifyCiImpact` refuses otherwise.
 */
export const LANE_ROOTS = Object.freeze({
  // The release N-API binding (`build-native-node`) and its consumers.
  native: ["verter_napi"],
  // The wasm32 artifact (`wasm-build`) and the browser gate behind it.
  wasm: ["verter_wasm"],
  // The debug LSP binaries: VS Code E2E, editor-neutral contract, DX harness.
  lsp: ["verter_lsp", "verter_relay_shim", "verter_mcp", "verter_dx_baseline"],
  // The release LSP binary plus the editor-client shipping plans.
  editors: ["verter_lsp", "verter-editor-client"],
  // The serial real-provider lane: every package its selectors own.
  providers: [...PROVIDER_PACKAGES],
  // Compile-fail contracts: the runner crates plus every owner whose fixtures
  // the runner compiles, the isolated verter_compiler build, and the
  // generated Svelte artifact check.
  compiler_contracts: [
    "verter_compile_contracts",
    "verter_compile_contracts_bench",
    "verter_compile_contracts_session_variants",
    ...new Set(Object.values(COMPILE_CONTRACT_OWNER_CRATES)),
  ],
  // The feature-gated BF2 inventory is verter_session's lib under a feature.
  bf2: ["verter_session"],
  svelte_conformance: ["verter_svelte_conformance"],
  // The PR-side Svelte benchmark contract proves the corpus against the
  // compiler; the measurement itself runs nightly.
  svelte_perf: ["verter_compiler"],
  // The playground build consumes the wasm and native artifacts.
  playground: ["verter_wasm", "verter_napi"],
  // The sfc-projection node protocols run `cargo test` in verter_compiler and
  // verter_session and read verter_language and verter_validation_probe
  // sources as products.
  sfc_projection: [
    "verter_compiler",
    "verter_language",
    "verter_session",
    "verter_validation_probe",
  ],
});

/**
 * The job gates `detect-changes` publishes, in evaluation order. A gate is
 * the OR of the named path filters (`filters`), the named impact lanes
 * (`impact`) and previously evaluated gates (`gates`). A gate with only
 * `filters` is a pure pass-through of its filters.
 */
export const LANE_GATES = Object.freeze({
  rust: { filters: ["rust"] },
  proto: { filters: ["proto"] },
  // js-build-test: the JavaScript workspace and the scripts' self-tests.
  js: { filters: ["js", "scripts"] },
  arch: { filters: ["arch"] },
  svelte_oracle: { filters: ["svelte_oracle"] },
  svelte_client_smokes: { filters: ["svelte_client_smokes"] },
  jetbrains: { filters: ["jetbrains"] },
  // Subsystem gates.
  wasm: { filters: ["wasm"], impact: ["wasm"] },
  native: { filters: ["js"], impact: ["native"] },
  playground: { filters: ["playground"], impact: ["playground"] },
  // Transport equivalence compares the two artifacts, so either subsystem
  // moving demands BOTH artifacts (see the artifact gates below).
  transport: { filters: ["transport"], impact: ["native", "wasm"] },
  // Artifact gates: a producer runs whenever any consumer of it runs.
  native_artifact: { gates: ["native", "playground", "transport"] },
  wasm_artifact: { gates: ["wasm", "playground", "transport"] },
  vscode: { filters: ["vscode"], impact: ["lsp"] },
  dx: { filters: ["dx"], impact: ["lsp"] },
  editor_lsp: { filters: ["editor_lsp"], impact: ["lsp"] },
  editors: { filters: ["editors"], impact: ["editors"] },
  providers: { filters: ["providers"], impact: ["providers"] },
  compiler_contracts: { filters: ["compiler_contracts"], impact: ["compiler_contracts"] },
  bf2: { filters: ["bf2"], impact: ["bf2"] },
  svelte_conformance: { filters: ["svelte_oracle"], impact: ["svelte_conformance"] },
  svelte_perf: { filters: ["svelte_perf"], impact: ["svelte_perf"] },
  sfc_projection: { filters: ["sfc_projection"], impact: ["sfc_projection"] },
});

/**
 * This classifier's own escape hatches. Narrower than `affected-tests.mjs`'s:
 * CI has explicit path owners, so `scripts/`, `packages/`, the JS install
 * graph and the rest are handled by the lane whose filter names them (an
 * unowned one still falls back, below). What stays global is what changes
 * how every crate compiles (every impact-bearing lane runs), and what changes
 * what CI runs or how this selection is made (`everything`: every lane runs).
 */
export const ESCAPE_HATCHES = Object.freeze([
  {
    id: "workspace-manifest",
    reason: "the workspace manifest or lockfile changes resolution for every crate",
    test: (p) => p === "Cargo.toml" || p === "Cargo.lock",
  },
  {
    id: "nextest-config",
    reason: "nextest.toml controls test execution semantics for every nextest lane",
    test: (p) => p === ".config/nextest.toml",
  },
  {
    id: "toolchain",
    reason: "the pinned toolchain and cargo configuration change how every crate builds",
    test: (p) => p === "rust-toolchain.toml" || p === ".cargo" || p.startsWith(".cargo/"),
  },
  {
    // The other workflows run on their own triggers; their files are owned
    // by the lanes whose self-tests read them, or are CI-inert.
    id: "ci-workflow",
    reason: "ci.yml decides what runs; a selector cannot reason about it",
    everything: true,
    test: (p) => p === ".github/workflows/ci.yml",
  },
  {
    id: "ci-actions",
    reason: "local composite actions are steps of every job that uses them",
    everything: true,
    test: (p) => p === ".github/actions" || p.startsWith(".github/actions/"),
  },
  {
    id: "lane-classifier",
    reason: "the selection logic itself, or the crate-graph library it is built on, changed",
    everything: true,
    test: (p) => p === "scripts/ci-impact.mjs" || p === "scripts/lib/crate-graph.mjs",
  },
  {
    id: "proc-macro-crate",
    reason:
      "a proc-macro crate expands into every consumer at build time, not along a linking edge",
    test: (p, index) => Boolean(mapPathToCrate(index, p)?.isProcMacro),
  },
  {
    id: "verter-identity",
    reason: "verter_identity is foundational; identity semantics reach every crate",
    test: (p) => p === "crates/verter_identity" || p.startsWith("crates/verter_identity/"),
  },
]);

/**
 * Files that positively have no consumer in any ci.yml job, as globs. This is
 * NOT the broad `KNOWN_NON_RUST_TOP_LEVEL` of `affected-tests.mjs`: that list
 * assumes whole directories are irrelevant to the Rust build, which is a fine
 * default for a local inner-loop selector and an under-selection for CI,
 * where `schemas/**` is validated by a Rust test and `test-corpora/**` is
 * read by Rust tests and `include_str!`. A file that no crate owns, no path
 * filter owns and this list does not name turns every lane on.
 *
 * Inert is about ci.yml only: a file another workflow runs on its own trigger
 * (a benchmark, a nightly corpus, the docs deploy, the release rehearsal) is
 * inert here when no ci.yml job reads it. A file a lane reads is owned by that
 * lane's filter even under an inert tree (`docs/audit-footprint/**`,
 * `tests/vscode-product/VSC0/products/…`): inert only means a change to it
 * does not force the fallback.
 *
 * Every tracked path a ci.yml job reads by NAME rather than content is out of
 * reach of any path list: `tracked_paths_are_portable` checks the names of
 * every tracked file whenever the rust lane runs, so a non-portable name added
 * under an inert tree is caught by the next change that runs it.
 */
export const CI_INERT_PATHS = Object.freeze([
  // Prose.
  "docs/**",
  ".github/BENCHMARK.md",
  ".github/INTEGRATION_TEST.md",
  "AGENTS.md",
  "CONTRIBUTING.md",
  "LICENSE",
  "readme.md",
  // Read for its existence only: verter_session tests locate the repository
  // root by it.
  "CLAUDE.md",
  // Agent skills, editor settings and the pre-commit hook.
  ".claude/**",
  ".husky/**",
  ".lintstagedrc.cjs",
  ".vscode/**",
  // Workflows that run on their own triggers and whose files no ci.yml job
  // reads.
  ".github/workflows/benchmark.yml",
  ".github/workflows/corpus-gate.yml",
  ".github/workflows/dx-extended.yml",
  ".github/workflows/editor-packages.yml",
  ".github/workflows/integration-test.yml",
  ".github/workflows/lsp-benchmark.yml",
  ".github/workflows/meta-benchmark.yml",
  // Product, architecture and evidence inventories that only local verifiers
  // or the docs build read.
  "tests/documentation/**",
  // Reviewed contract data with no ci.yml consumer. Future executable specs
  // and consumed products require an explicit lane owner.
  "tests/framework-angular/ANG0/manifest.json",
  "tests/framework-angular/ANG0/cases.md",
  "tests/framework-angular/ANG0/products/angular-version-lock.json",
  "tests/framework-angular/ANG0/products/angular-capability-matrix.json",
  "tests/framework-angular/ANG0/products/angular-activation-policy.json",
  // Reviewed Astro contract data; no ci.yml lane reads these bytes yet.
  // Keep executable specs and future products outside these inert entries.
  "tests/framework-astro/AST0/cases.md",
  "tests/framework-astro/AST0/corpus/**/*.astro",
  "tests/framework-astro/AST0/manifest.json",
  "tests/framework-astro/AST0/products/astro-activation-policy.json",
  "tests/framework-astro/AST0/products/astro-capability-matrix.json",
  "tests/framework-astro/AST0/products/astro-version-lock.json",
  "tests/framework-astro/evidence/AST0/cases.md",
  "tests/framework-liquid/**",
  "tests/jetbrains-baseline/**",
  "tests/kernel/**",
  "tests/playground/**",
  "tests/product-experience/**",
  "tests/skills/**",
  "tests/test-layout/**",
  "tests/vscode-product/**",
  "tests/vscode-web/**",
  "tests/web-product/**",
  "tests/workspace-responsiveness/**",
  // Example applications (examples/reference is the arch lane's), manual
  // tools and MCP client configurations.
  "examples/*",
  "examples/src/**",
  "tools/debug/**",
  "tools/tsgo-api-gate/**",
  "mcp/README.md",
  "mcp/verter-http.mcp.json",
  // A tracked test report js-build-test overwrites before reading it.
  "test-results/**",
]);

const COMPILED_CI_INERT_PATHS = CI_INERT_PATHS.map(compileGlob);

export function isCiInert(relPath) {
  return COMPILED_CI_INERT_PATHS.some((re) => re.test(relPath));
}

function matchHatch(relPath, index) {
  for (const rule of ESCAPE_HATCHES) {
    if (rule.test(relPath, index)) return rule;
  }
  return null;
}

/**
 * Pure decision core.
 *
 * @param {string[]} changedFiles workspace-relative, forward-slash paths
 * @param {object} metadata parsed `cargo metadata --format-version=1`
 * @param {Record<string, string[]>} laneRoots
 * @param {{ownedFiles?: Set<string> | null}} [options] files some path filter
 *   matched; a non-crate file outside this set and outside `CI_INERT_PATHS`
 *   forces the fallback. `null`/absent means nothing is known to be owned.
 * @returns `full` turns every impact-bearing lane on; `everything` (an
 *   unrecognized path, or a hatch that changes what CI runs) turns every gate
 *   on, pass-through ones included.
 */
export function classifyCiImpact(changedFiles, metadata, laneRoots = LANE_ROOTS, options = {}) {
  const ownedFiles = options.ownedFiles ?? new Set();
  const index = buildWorkspaceIndex(metadata);
  for (const [lane, roots] of Object.entries(laneRoots)) {
    for (const root of roots) {
      if (!index.byName.has(root)) {
        throw new Error(
          `ci-impact: lane "${lane}" root "${root}" is not a workspace member; fix LANE_ROOTS`,
        );
      }
    }
  }
  const reverse = buildReverseDependencyGraph(metadata, index);

  const fullReasons = [];
  const directCrates = new Set();
  const ownedNonRust = [];
  let everything = false;
  for (const file of changedFiles) {
    const hatch = matchHatch(file, index);
    if (hatch) {
      fullReasons.push({ file, id: hatch.id, reason: hatch.reason });
      if (hatch.everything) everything = true;
      continue;
    }
    const crate = mapPathToCrate(index, file);
    if (crate) {
      directCrates.add(crate.name);
      continue;
    }
    if (ownedFiles.has(file) || isCiInert(file)) {
      ownedNonRust.push(file);
      continue;
    }
    fullReasons.push({
      file,
      id: "unrecognized-path",
      reason: "no crate owns it and no path filter matched it; over-select rather than guess",
    });
    everything = true;
  }

  const full = fullReasons.length > 0;
  const impacted = full ? new Set() : transitiveDependents(reverse, directCrates);
  const lanes = {};
  for (const [lane, roots] of Object.entries(laneRoots)) {
    lanes[lane] = full || roots.some((root) => impacted.has(root));
  }
  return {
    full,
    everything,
    fullReasons,
    directCrates: [...directCrates].sort(),
    impactedCrates: [...impacted].sort(),
    ownedNonRust,
    lanes,
  };
}

/**
 * @param {Record<string, boolean>} filterHits `matchPathFilters(...).hits`
 * @param {{full: boolean, everything?: boolean, lanes: Record<string, boolean>}} impact
 * @param {typeof LANE_GATES} gates
 * @returns {Record<string, "true" | "false">}
 */
export function composeLaneGates(filterHits, impact, gates = LANE_GATES) {
  const result = {};
  for (const [gate, spec] of Object.entries(gates)) {
    let on = impact.everything === true;
    for (const filter of spec.filters ?? []) {
      if (!(filter in filterHits)) {
        throw new Error(
          `ci-impact: gate "${gate}" names filter "${filter}", which is not a path filter`,
        );
      }
      if (filterHits[filter] === true) on = true;
    }
    for (const lane of spec.impact ?? []) {
      if (impact.full) {
        on = true;
        continue;
      }
      if (!(lane in impact.lanes)) {
        throw new Error(`ci-impact: gate "${gate}" names unknown impact lane "${lane}"`);
      }
      if (impact.lanes[lane]) on = true;
    }
    for (const other of spec.gates ?? []) {
      if (!(other in result)) {
        throw new Error(
          `ci-impact: gate "${gate}" depends on gate "${other}", which is not evaluated before it`,
        );
      }
      if (result[other] === "true") on = true;
    }
    result[gate] = on ? "true" : "false";
  }
  return result;
}

/**
 * The selection's own configuration checked against the tree. `unowned`:
 * tracked files no crate, hatch, path filter or inert glob owns — every change
 * to one would run every lane. `dead`: globs that match no tracked file — a
 * lane that silently stopped selecting the file it was written for (a moved
 * script, a renamed test). `config`: repository data a filter is derived from
 * could not be read (`SELECTION_CONFIG_ERRORS`).
 *
 * @param {string[]} trackedFiles `git ls-files`, forward-slash paths
 * @param {object} workspaceMetadata `cargo metadata` (only the members are read)
 */
export function auditSelection(trackedFiles, workspaceMetadata) {
  const { owned } = matchPathFilters(trackedFiles);
  const impact = classifyCiImpact(
    trackedFiles,
    { ...workspaceMetadata, resolve: { nodes: [] } },
    LANE_ROOTS,
    { ownedFiles: owned },
  );
  const unowned = impact.fullReasons
    .filter((reason) => reason.id === "unrecognized-path")
    .map((reason) => reason.file);
  const dead = [];
  const anyMatch = (glob) => {
    const re = compileGlob(glob);
    return trackedFiles.some((file) => re.test(file));
  };
  for (const [name, spec] of Object.entries(PATH_FILTERS)) {
    const { include, exclude = [] } = Array.isArray(spec) ? { include: spec } : spec;
    for (const glob of [...include, ...exclude]) if (!anyMatch(glob)) dead.push(`${name}: ${glob}`);
  }
  for (const glob of CI_INERT_PATHS) if (!anyMatch(glob)) dead.push(`CI_INERT_PATHS: ${glob}`);
  return { unowned, dead, config: [...SELECTION_CONFIG_ERRORS] };
}

function reportConfigErrors(consequence) {
  for (const error of SELECTION_CONFIG_ERRORS) {
    process.stdout.write(`::error title=ci-impact selection config::${error} — ${consequence}\n`);
  }
}

function runAudit(cwd) {
  const tracked = execFileSync("git", ["ls-files", "-z"], { cwd, maxBuffer: 1024 * 1024 * 64 })
    .toString("utf8")
    .split("\0")
    .filter(Boolean);
  const workspace = JSON.parse(
    execFileSync("cargo", ["metadata", "--no-deps", "--format-version=1"], {
      cwd,
      maxBuffer: 1024 * 1024 * 64,
      stdio: ["ignore", "pipe", "inherit"],
    }).toString("utf8"),
  );
  const { unowned, dead, config } = auditSelection(tracked, workspace);
  reportConfigErrors("fix it, or point the classifier at where that data lives now");
  for (const file of unowned) {
    process.stdout.write(
      `::error title=ci-impact unowned path::${file} — give it an owner in PATH_FILTERS (the lanes that read it) or, when no ci.yml job reads it, a CI_INERT_PATHS entry\n`,
    );
  }
  for (const entry of dead) {
    process.stdout.write(
      `::error title=ci-impact dead glob::${entry} matches no tracked file — point it at what the lane reads now, or remove it\n`,
    );
  }
  if (unowned.length === 0 && dead.length === 0 && config.length === 0) {
    process.stdout.write(
      `ci-impact audit: ${tracked.length} tracked files all owned; every filter glob matches\n`,
    );
    return 0;
  }
  return 1;
}

/**
 * `project` and `ide`: a release pull request (`release: v…`, `release(ide): v…`).
 * `landed`: that pull request's squash commit, pushed to main.
 */
export const RELEASE_KINDS = ["project", "ide", "landed"];

/**
 * A release pull request runs the ordinary lanes its change selects (a project
 * version bump rewrites the workspace manifests, which already selects every
 * lane) and, beside them, the release rehearsal; nothing is turned off, and
 * the tag's release later reuses this run's tests and artifacts instead of
 * repeating them. Its squash commit on main has the tree that pull request's
 * CI just tested (the ruleset requires it to be up to date), so `landed`
 * turns every lane off rather than testing that tree a second time.
 */
export function releaseGates(gates, release) {
  if (release !== "landed") return gates;
  return Object.fromEntries(Object.keys(gates).map((gate) => [gate, "false"]));
}

/** `$GITHUB_OUTPUT` lines: one `gate_<name>` per gate, then the fallback flag. */
export function formatGithubOutput(gates, impact, release = "") {
  return [
    ...Object.entries(gates).map(([gate, value]) => `gate_${gate}=${value}`),
    `impact_full=${impact.full && release !== "landed" ? "true" : "false"}`,
    `release=${release}`,
  ];
}

function summaryMarkdown(changedFiles, filterHits, impact, gates) {
  const lines = ["### CI lane selection", ""];
  if (impact.full) {
    lines.push(
      impact.everything
        ? "**Full fallback** — every lane runs:"
        : "**Full crate fallback** — every impact-bearing lane runs:",
      "",
    );
    for (const r of impact.fullReasons) lines.push(`- \`${r.file}\` (${r.id}): ${r.reason}`);
  } else {
    lines.push(
      `Changed crates: ${impact.directCrates.map((c) => `\`${c}\``).join(", ") || "none"}`,
      "",
      `Impacted (changed + dependents): ${impact.impactedCrates.length} crate(s)`,
    );
  }
  const matched = Object.keys(filterHits).filter((name) => filterHits[name]);
  lines.push("", `Path filters matched: ${matched.map((n) => `\`${n}\``).join(", ") || "none"}`);
  lines.push("", "| Gate | Runs |", "| --- | --- |");
  for (const [gate, value] of Object.entries(gates)) lines.push(`| ${gate} | ${value} |`);
  lines.push("", `${changedFiles.length} changed file(s) classified.`, "");
  return lines.join("\n");
}

function loadCargoMetadata(cwd) {
  const raw = execFileSync(
    "cargo",
    ["metadata", "--format-version=1", "--all-features", "--locked"],
    { cwd, maxBuffer: 1024 * 1024 * 256, stdio: ["ignore", "pipe", "inherit"] },
  );
  return JSON.parse(raw.toString("utf8"));
}

function changedFilesFromRange(cwd, range) {
  return execFileSync("git", ["diff", "--no-renames", "--name-only", range], {
    cwd,
    maxBuffer: 1024 * 1024 * 64,
  })
    .toString("utf8")
    .split("\n")
    .filter(Boolean)
    .map((f) => f.replace(/\\/g, "/"));
}

function argValue(argv, flag) {
  const index = argv.indexOf(flag);
  return index >= 0 ? argv[index + 1] : undefined;
}

export function main(argv = process.argv.slice(2), env = process.env, cwd = process.cwd()) {
  if (argv.includes("--audit")) return runAudit(cwd);
  const range = argValue(argv, "--range");
  const githubOutput = argValue(argv, "--github-output");
  const summaryPath = argValue(argv, "--summary");
  const metadataPath = argValue(argv, "--metadata");
  const changedFilesPath = argValue(argv, "--changed-files");
  const release = argValue(argv, "--release") ?? env.CI_IMPACT_RELEASE ?? "";
  if (release && !RELEASE_KINDS.includes(release)) {
    process.stderr.write(`ci-impact: --release must be one of ${RELEASE_KINDS.join(", ")}, or empty
`);
    return 2;
  }

  let changedFiles;
  if (range) {
    changedFiles = changedFilesFromRange(cwd, range);
  } else if (changedFilesPath) {
    changedFiles = JSON.parse(readFileSync(resolve(cwd, changedFilesPath), "utf8"));
  } else if (env.CI_IMPACT_CHANGED_FILES_JSON) {
    changedFiles = JSON.parse(env.CI_IMPACT_CHANGED_FILES_JSON);
  } else {
    process.stderr.write(
      "ci-impact: pass --range <rev>..<rev>, --changed-files <file>, or CI_IMPACT_CHANGED_FILES_JSON\n",
    );
    return 2;
  }
  if (!Array.isArray(changedFiles) || !changedFiles.every((f) => typeof f === "string")) {
    process.stderr.write("ci-impact: the changed-file list must be a JSON array of paths\n");
    return 2;
  }
  changedFiles = changedFiles.map((f) => f.replace(/\\/g, "/"));

  const { hits: filterHits, owned } = matchPathFilters(changedFiles);
  let impact;
  try {
    const metadata = metadataPath
      ? JSON.parse(readFileSync(resolve(cwd, metadataPath), "utf8"))
      : loadCargoMetadata(cwd);
    impact = classifyCiImpact(changedFiles, metadata, LANE_ROOTS, { ownedFiles: owned });
  } catch (error) {
    if (/lane root|LANE_ROOTS/.test(String(error?.message))) {
      // A stale root is a configuration bug: fail the job so it gets fixed.
      process.stderr.write(`${error.message}\n`);
      return 1;
    }
    // Anything else (cargo unavailable, registry outage) is an unknown input:
    // without the graph no file can be classified, so run everything rather
    // than narrow on a guess.
    process.stdout.write(
      `::warning title=ci-impact full fallback::could not derive the crate graph: ${String(
        error?.message ?? error,
      )}\n`,
    );
    impact = {
      full: true,
      everything: true,
      fullReasons: [{ file: "(cargo metadata)", id: "graph-unavailable", reason: String(error) }],
      directCrates: [],
      impactedCrates: [],
      ownedNonRust: [],
      lanes: Object.fromEntries(Object.keys(LANE_ROOTS).map((lane) => [lane, true])),
    };
  }
  if (SELECTION_CONFIG_ERRORS.length > 0) {
    // A filter is derived from data that could not be read, so it may be
    // missing paths: run everything rather than narrow on a partial filter.
    reportConfigErrors("every lane runs until it is fixed");
    impact = {
      ...impact,
      full: true,
      everything: true,
      fullReasons: [
        ...impact.fullReasons,
        ...SELECTION_CONFIG_ERRORS.map((reason) => ({
          file: STP1_INVENTORY,
          id: "selection-config",
          reason,
        })),
      ],
      lanes: Object.fromEntries(Object.keys(LANE_ROOTS).map((lane) => [lane, true])),
    };
  }

  const gates = releaseGates(composeLaneGates(filterHits, impact), release);
  const lines = formatGithubOutput(gates, impact, release);
  if (githubOutput) appendFileSync(githubOutput, `${lines.join("\n")}\n`);
  const selection = summaryMarkdown(changedFiles, filterHits, impact, gates);
  const summary =
    release === "landed"
      ? "### CI lane selection\n\nLanded release commit: every lane is off; its release pull request's CI tested this tree."
      : release
        ? `${selection}\n\nRelease pull request (${release}): the release rehearsal runs beside these lanes.`
        : selection;
  if (summaryPath) appendFileSync(summaryPath, `${summary}\n`);
  process.stdout.write(`${summary}\n`);
  return 0;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  process.exitCode = main();
}
