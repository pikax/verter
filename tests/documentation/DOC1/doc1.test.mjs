// node --test tests/documentation/DOC1/doc1.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalizeReceipt,
  createBoundedText,
  createTextDigester,
  loadWorkspacePackages,
  resolveExampleManifest,
  resolvePackageExport,
  resolveShippedCommand,
  sha256Text,
  shippedBins,
  validate,
} from "../../../docs/scripts/reference-harness.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const viteConfigRel = "examples/reference/unplugin-vite/vite.config.ts";
const commandsRel = "examples/reference/cli/commands.json";
const generatedRel = "docs/generated/typeinfo-row-registry-counts.md";
const viteConfig = readFileSync(join(repoRoot, viteConfigRel), "utf8");
const commands = readFileSync(join(repoRoot, commandsRel), "utf8");
const generated = readFileSync(join(repoRoot, generatedRel), "utf8");
// The manifest is a sharded index (DXM0): the header keeps home/pin and the
// two index pages, rows register through fragment files, and readers observe
// only the merged view resolved by the shared loader.
const manifestRel = "examples/reference/manifest.json";
const manifestHeader = JSON.parse(readFileSync(join(repoRoot, manifestRel), "utf8"));
const { manifest } = resolveExampleManifest(join(repoRoot, "examples/reference"));
const exampleFragment = (id) => `examples/reference/${id}/example.json`;

function opts(extra = {}) {
  return { repoRoot, skipTypeinfoCheck: true, ...extra };
}

async function clean() {
  return canonicalizeReceipt(await validate(opts()));
}

test("clean public examples pass against shipped exports and bins", async () => {
  const receipt = await clean();
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.errors.length, 0);
  assert.deepEqual(
    receipt.examples.map((row) => row.id),
    [...manifest.examples.map((row) => row.id)].sort(),
  );
  assert.ok(receipt.examples.every((row) => row.ok));
  assert.equal(receipt.generatedReference.exists, true);
  assert.equal(typeof receipt.generatedReference.lintRuleCount, "number");
  assert.ok(receipt.generatedReference.lintRuleCount > 0);
});

test("public export map accepts shipped subpaths and rejects source internals", () => {
  const packages = loadWorkspacePackages(repoRoot);
  const vite = resolvePackageExport(packages, "@verter/unplugin/vite");
  assert.equal(vite.ok, true);
  const types = resolvePackageExport(packages, "@verter/types");
  assert.equal(types.ok, true);
  const internals = resolvePackageExport(packages, "@verter/unplugin/src/vite");
  assert.equal(internals.ok, false);
  const privatePkg = resolvePackageExport(packages, "example");
  assert.equal(privatePkg.ok, false);
});

test("shipped bins accept package bin names and reject cargo", () => {
  const bins = shippedBins(loadWorkspacePackages(repoRoot));
  assert.equal(resolveShippedCommand(bins, "verter-tsc").ok, true);
  assert.equal(resolveShippedCommand(bins, "verter-lsp").ok, true);
  assert.equal(resolveShippedCommand(bins, "verter-mcp").ok, true);
  const cargo = resolveShippedCommand(bins, "cargo run -p verter_tsc");
  assert.equal(cargo.ok, false);
  assert.equal(cargo.code, "unshipped-toolchain");
  const absent = resolveShippedCommand(bins, "verter-dev");
  assert.equal(absent.ok, false);
  assert.equal(absent.code, "unshipped-command");
});

test("an example that imports package source internals fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [viteConfigRel]: `${viteConfig}\nimport internals from "../../../packages/unplugin/src/vite.ts";\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "internal-import"));
});

test("a side-effect import of package source internals fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [viteConfigRel]: `${viteConfig}\nimport "../../../packages/unplugin/src/vite.ts";\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "internal-import"));
});

test("a declared node: builtin import is not treated as an unpublished export", async () => {
  const unplugin = structuredClone(manifest.examples.find((row) => row.id === "unplugin-vite"));
  unplugin.imports = [...unplugin.imports, "node:fs"];
  const receipt = await validate(
    opts({
      overlays: {
        [exampleFragment("unplugin-vite")]: JSON.stringify(unplugin),
        [viteConfigRel]: `${viteConfig}\nimport fs from "node:fs";\n`,
      },
    }),
  );
  assert.equal(receipt.completenessState, "complete");
  assert.equal(
    receipt.errors.some((item) => item.specifier === "node:fs"),
    false,
  );
});

test("an example that invokes cargo rather than a shipped bin fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [commandsRel]: JSON.stringify({ commands: ["cargo run -p verter_tsc"] }, null, 2),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "unshipped-command"));
});

test("an example that invokes a bin absent from shipped packages fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [commandsRel]: JSON.stringify({ commands: ["verter-dev"] }, null, 2),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "unshipped-command");
  assert.ok(miss);
  assert.equal(miss.reason, "unshipped-command");
});

test("an example citing a surface absent from the capability catalog fails", async () => {
  const unplugin = structuredClone(manifest.examples.find((row) => row.id === "unplugin-vite"));
  unplugin.surfaces = ["invented.capability.matrix"];
  const receipt = await validate(
    opts({
      overlays: {
        [exampleFragment("unplugin-vite")]: JSON.stringify(unplugin),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "unknown-surface"));
});

test("a missing example source is a partial result", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [viteConfigRel]: null,
      },
    }),
  );
  assert.equal(receipt.completenessState, "partial");
  assert.ok(receipt.errors.some((item) => item.code === "missing-source"));
});

test("a generated page unbound from its generator is stale", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [generatedRel]: "# hand written\n",
      },
    }),
  );
  assert.equal(receipt.completenessState, "stale");
  assert.ok(receipt.errors.some((item) => item.code === "stale-generated"));
});

test("pre-aborted signal yields cancelled and never complete", async () => {
  const receipt = await validate(opts({ signal: AbortSignal.abort() }));
  assert.equal(receipt.completenessState, "cancelled");
  assert.ok(receipt.errors.some((item) => item.code === "cancelled"));
});

test("mid-discovery cancellation yields cancelled with a source digest", async () => {
  let seen = 0;
  const signal = {
    get aborted() {
      seen += 1;
      return seen > 1;
    },
  };
  const receipt = await validate(opts({ signal }));
  assert.equal(receipt.completenessState, "cancelled");
  assert.equal(typeof receipt.sourceRevisions.digest, "string");
  assert.match(receipt.sourceRevisions.digest, /^[0-9a-f]{64}$/);
  assert.ok(receipt.errors.some((item) => item.code === "cancelled"));
});

test("overlay-added README is walked using overlay-aware listing", async () => {
  const extraRel = "examples/reference/overlay-home.md";
  const receipt = await validate(
    opts({
      overlays: {
        [extraRel]: "[missing](./does-not-exist.md)\n",
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(
    receipt.errors.some((item) => item.code === "broken-link" && item.file === "overlay-home.md"),
  );
  assert.ok(receipt.links.some((item) => item.from === extraRel && item.ok === false));
});

test("digest labels follow posixRel of the examples root", async () => {
  const receipt = await clean();
  assert.equal(receipt.projectConfiguration.manifest, "examples/reference/manifest.json");
  assert.equal(receipt.projectConfiguration.examplesRoot, "examples/reference");
});

// The complete pre-conversion manifest, frozen as the monolithic file
// recorded it before the DXM0 sharding (collections in the loader id order).
// The resolved manifest must deep-equal it across all six root fields and
// every row field; the header keeps home/pin and the two index pages only.
const FROZEN_MANIFEST = {
  home: "examples/reference",
  pin: {
    node: ">=22",
    typescript: "7.1.0-beta",
    vue: "3.6.0-rc.5",
  },
  examples: [
    {
      id: "component-meta-session",
      files: ["component-meta/session.ts"],
      imports: ["@verter/component-meta"],
      surfaces: ["vue.component_meta.scalar"],
      executionClass: "NativeOnly",
      proofClass: "static-proof",
    },
    {
      id: "shipped-cli",
      files: ["cli/commands.json"],
      commands: ["verter-tsc", "verter-lsp", "verter-mcp"],
      surfaces: ["vue.tsc.project_check"],
      executionClass: "NativeOnly",
      proofClass: "static-proof",
    },
    {
      id: "types-helpers",
      files: ["types-helpers/example.ts"],
      imports: ["@verter/types"],
      surfaces: ["vue.language_service.typing"],
      executionClass: "ExternalOwner",
      proofClass: "static-proof",
    },
    {
      id: "unplugin-vite",
      files: ["unplugin-vite/vite.config.ts"],
      imports: ["vite", "@verter/unplugin/vite"],
      peers: ["vite"],
      surfaces: ["vue.managed.compile.raw"],
      executionClass: "NativeOnly",
      proofClass: "static-proof",
    },
  ],
  sdkGuides: {
    index: "sdk/README.md",
    topics: [
      {
        id: "compatibility",
        page: "sdk/compatibility.md",
        status: "pending",
        producingNode: "XSDK9",
      },
      {
        id: "contribution",
        page: "sdk/contribution.md",
        status: "pending",
        producingNode: "XSDK9",
      },
      {
        id: "debugging",
        page: "sdk/debugging.md",
        status: "pending",
        producingNode: "XSDK9",
      },
      {
        id: "isolation",
        page: "sdk/isolation.md",
        status: "pending",
        producingNode: "XSDK9",
      },
      {
        id: "packaging",
        page: "sdk/packaging.md",
        status: "pending",
        producingNode: "XSDK9",
      },
      {
        id: "permissions",
        page: "sdk/permissions.md",
        status: "pending",
        producingNode: "XSDK9",
      },
    ],
  },
  recipes: {
    index: "guides/README.md",
    topics: [
      {
        id: "accessibility",
        page: "guides/accessibility.md",
        status: "pending",
        producingNode: "AX0",
      },
      {
        id: "compatibility",
        page: "guides/compatibility.md",
        status: "pending",
        producingNode: "WBC0",
      },
      {
        id: "css",
        page: "guides/css.md",
        status: "pending",
        producingNode: "CSS0",
      },
      {
        id: "debug",
        page: "guides/debug.md",
        status: "pending",
        producingNode: "DBG0",
      },
      {
        id: "performance",
        page: "guides/performance.md",
        status: "pending",
        producingNode: "WPF0",
      },
      {
        id: "runtime",
        page: "guides/runtime.md",
        status: "pending",
        producingNode: "RTI0",
      },
      {
        id: "security",
        page: "guides/security.md",
        status: "pending",
        producingNode: "SEC0",
      },
      {
        id: "tests",
        page: "guides/tests.md",
        status: "pending",
        producingNode: "TST0",
      },
    ],
  },
  journeys: [
    {
      id: "cli-version-smoke",
      description: "run the shipped verter-tsc bin and observe its version banner",
      executionClass: "NativeOnly",
      pins: {
        node: ">=22",
      },
      steps: [
        {
          exampleId: "shipped-cli",
          command: "verter-tsc",
          args: ["--version"],
          expectExit: 0,
        },
      ],
    },
  ],
};

test("the sharded reference manifest resolves deep-equal to the frozen pre-conversion file", () => {
  assert.deepEqual(
    Object.keys(manifestHeader).sort(),
    ["home", "pin", "recipes", "sdkGuides"],
    "the header must keep only home, pin and the two index pages",
  );
  assert.equal(manifestHeader.sdkGuides.index, "sdk/README.md");
  assert.equal(manifestHeader.recipes.index, "guides/README.md");
  assert.deepEqual(manifest, FROZEN_MANIFEST);

  // Twin: a planted fragment that drops one field of one row breaks the
  // frozen comparison (the discriminator re-runs, not just the mutation).
  const planted = structuredClone(manifest);
  delete planted.examples.find((row) => row.id === "types-helpers").proofClass;
  assert.throws(
    () => assert.deepEqual(planted, FROZEN_MANIFEST),
    /proofClass/,
    "an altered fragment row must fail the frozen comparison",
  );
});

test("a fragment edit changes the source digest", async () => {
  const fresh = await validate(opts());
  const row = structuredClone(manifest.examples.find((r) => r.id === "shipped-cli"));
  row.commands = [...row.commands];
  const edited = await validate(
    opts({ overlays: { [exampleFragment("shipped-cli")]: JSON.stringify(row) } }),
  );
  assert.equal(edited.completenessState, fresh.completenessState);
  assert.notEqual(edited.sourceRevisions.digest, fresh.sourceRevisions.digest);
});

// The pre-conversion vitepress sidebar, frozen section by section; the docs
// sidebar is generated from docs/.vitepress/sidebar/<section>.json fragments.
const FROZEN_SIDEBAR = {
  "/guide/": [
    {
      text: "Introduction",
      items: [
        {
          text: "What is Verter?",
          link: "/guide/",
        },
        {
          text: "Getting Started",
          link: "/guide/getting-started",
        },
      ],
    },
    {
      text: "Bundler Integration",
      items: [
        {
          text: "Vite",
          link: "/guide/vite",
        },
        {
          text: "webpack",
          link: "/guide/webpack",
        },
        {
          text: "Other Bundlers",
          link: "/guide/other-bundlers",
        },
      ],
    },
    {
      text: "Features",
      items: [
        {
          text: "Overview",
          link: "/guide/features",
        },
        {
          text: "Performance",
          link: "/guide/performance",
        },
        {
          text: "Cross-File Optimization",
          link: "/guide/cross-file-optimization",
        },
        {
          text: "Linting",
          link: "/guide/linting",
        },
        {
          text: "Architecture",
          link: "/guide/architecture",
        },
      ],
    },
    {
      text: "Advanced",
      items: [
        {
          text: "Reactivity Classification",
          link: "/guide/advanced/reactivity-classification",
        },
        {
          text: "Template Compilation",
          link: "/guide/advanced/template-compilation",
        },
      ],
    },
  ],
  "/api/": [
    {
      text: "API Reference",
      items: [
        {
          text: "@verter/unplugin",
          link: "/api/unplugin",
        },
        {
          text: "@verter/native",
          link: "/api/native",
        },
        {
          text: "@verter/wasm",
          link: "/api/wasm",
        },
        {
          text: "@verter/types",
          link: "/api/types",
        },
        {
          text: "@verter/component-meta",
          link: "/api/component-meta",
        },
      ],
    },
  ],
  "/editor/": [
    {
      text: "Editor Support",
      items: [
        {
          text: "VS Code Extension",
          link: "/editor/vscode",
        },
        {
          text: "Other Editors (Lapce/Zed/Helix/Neovim)",
          link: "/editor/other-editors",
        },
        {
          text: "LSP Features",
          link: "/editor/lsp-features",
        },
        {
          text: "MCP Server",
          link: "/editor/mcp-server",
        },
        {
          text: "Settings Reference",
          link: "/editor/settings",
        },
      ],
    },
  ],
  "/contributing/": [
    {
      text: "Contributing",
      items: [
        {
          text: "How to Contribute",
          link: "/contributing/",
        },
        {
          text: "Rust Setup",
          link: "/contributing/rust-setup",
        },
        {
          text: "Testing",
          link: "/contributing/testing",
        },
        {
          text: "Semantic Benchmark",
          link: "/contributing/semantic-benchmark",
        },
        {
          text: "CI/CD",
          link: "/contributing/ci-cd",
        },
      ],
    },
  ],
};

test("the vitepress sidebar composes from per-section fragments only", () => {
  const configRel = "docs/.vitepress/config.ts";
  const config = readFileSync(join(repoRoot, configRel), "utf8");
  const composed = {};
  for (const section of Object.keys(FROZEN_SIDEBAR)) {
    const name = section.replaceAll("/", "");
    const fragmentRel = `docs/.vitepress/sidebar/${name}.json`;
    assert.ok(config.includes(`./sidebar/${name}.json`), `${configRel} must import ${fragmentRel}`);
    composed[section] = JSON.parse(readFileSync(join(repoRoot, fragmentRel), "utf8"));
  }
  assert.deepEqual(composed, FROZEN_SIDEBAR);

  // No monolithic row survives in config.ts, and a planted fragment that
  // drops an item breaks the frozen comparison.
  assert.ok(!config.includes("/guide/getting-started"), "sidebar links live in fragments");
  const planted = structuredClone(composed);
  planted["/editor/"][0].items.shift();
  assert.throws(
    () => assert.deepEqual(planted, FROZEN_SIDEBAR),
    /text/,
    "an altered sidebar fragment must fail the frozen comparison",
  );
});

test("custom examplesRoot labels receipts with that path", async () => {
  const root = "examples/alt-reference";
  const receipt = await validate(
    opts({
      examplesRoot: join(repoRoot, root),
      overlays: {
        [`${root}/manifest.json`]: JSON.stringify({ home: root }),
        [`${root}/alt/example.json`]: JSON.stringify({
          id: "alt",
          files: ["alt.ts"],
          imports: ["@verter/types"],
          surfaces: ["vue.language_service.typing"],
        }),
        [`${root}/alt.ts`]:
          'import type { PatchHidden } from "@verter/types";\nexport type T = PatchHidden<{}, {}>;\n',
      },
    }),
  );
  assert.equal(receipt.projectConfiguration.manifest, `${root}/manifest.json`);
  assert.equal(receipt.projectConfiguration.examplesRoot, root);
  assert.equal(receipt.completenessState, "complete");
});

test("executable-examples plan does not claim harness execution", () => {
  const plan = JSON.parse(
    readFileSync(
      join(repoRoot, "tests/documentation/DOC0/products/executable-examples-plan.v1.json"),
      "utf8",
    ),
  );
  const harness = JSON.parse(
    readFileSync(
      join(repoRoot, "tests/documentation/DOC1/products/reference-harness.v1.json"),
      "utf8",
    ),
  );
  assert.equal(harness.runtimeClaim, "none");
  const claimed = [...plan.requirements, ...plan.doc1Acceptance].join("\n");
  assert.equal(/the harness executes every published example/i.test(claimed), false);
  assert.match(claimed, /static-proof/);
  assert.match(claimed, /does not execute examples/);
});

test("perturbed discovery order yields an identical canonical receipt", async () => {
  const ids = manifest.examples.map((row) => row.id);
  const left = canonicalizeReceipt(await validate(opts({ discoveryOrder: ids })));
  const right = canonicalizeReceipt(await validate(opts({ discoveryOrder: [...ids].reverse() })));
  assert.deepEqual(right, left);
});

test("incremental receipt with the same digest matches a fresh run", async () => {
  const fresh = canonicalizeReceipt(await validate(opts()));
  const incremental = canonicalizeReceipt(await validate(opts({ priorReceipt: fresh })));
  assert.equal(fresh.completenessState, "complete");
  assert.deepEqual(incremental.sourceRevisions, fresh.sourceRevisions);
  assert.equal(incremental.completenessState, "complete");
});

test("edit fails and revert restores a complete receipt", async () => {
  const before = await clean();
  const edited = await validate(
    opts({
      overlays: {
        [viteConfigRel]: `${viteConfig}\nimport internals from "../../../packages/unplugin/src/vite.ts";\n`,
      },
    }),
  );
  assert.notEqual(edited.completenessState, "complete");
  const reverted = await clean();
  assert.deepEqual(reverted, before);
});

const typesExampleRel = "examples/reference/types-helpers/example.ts";
const typesExample = readFileSync(join(repoRoot, typesExampleRel), "utf8");
const planRel = "tests/documentation/DOC0/products/generated-reference-plan.v1.json";

test("a compound command is rejected before the bin lookup", async () => {
  const bins = shippedBins(loadWorkspacePackages(repoRoot));
  assert.equal(resolveShippedCommand(bins, "verter-tsc").ok, true);
  for (const command of [
    "verter-tsc && cargo run -p verter_tsc",
    "verter-tsc || cargo run -p verter_tsc",
    "verter-tsc; cargo run -p verter_tsc",
    "verter-tsc | cat",
    "verter-tsc $(cargo run -p verter_tsc)",
  ]) {
    const resolved = resolveShippedCommand(bins, command);
    assert.equal(resolved.ok, false, command);
    assert.equal(resolved.code, "compound-command", command);
  }
  const shippedCli = structuredClone(manifest.examples.find((row) => row.id === "shipped-cli"));
  shippedCli.commands = ["verter-tsc && cargo run -p verter_tsc"];
  const receipt = await validate(
    opts({ overlays: { [exampleFragment("shipped-cli")]: JSON.stringify(shippedCli) } }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const rejected = receipt.errors.find((item) => item.code === "unshipped-command");
  assert.ok(rejected);
  assert.equal(rejected.reason, "compound-command");
});

test("an example file outside the example home is rejected", async () => {
  const typesHelpers = structuredClone(manifest.examples.find((row) => row.id === "types-helpers"));
  typesHelpers.files = ["../../package.json"];
  const receipt = await validate(
    opts({ overlays: { [exampleFragment("types-helpers")]: JSON.stringify(typesHelpers) } }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const outside = receipt.errors.find((item) => item.code === "example-outside-home");
  assert.ok(outside);
  assert.equal(outside.id, "types-helpers");
  assert.equal(outside.file, "../../package.json");
  assert.equal(
    receipt.errors.some((item) => item.code === "missing-source"),
    false,
  );
});

test("an observed subpath of a declared package must resolve on its own", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [typesExampleRel]: `${typesExample}\nimport "@verter/types/not-exported";\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const leak = receipt.errors.find(
    (item) => item.code === "unpublished-export" && item.specifier === "@verter/types/not-exported",
  );
  assert.ok(leak);
  assert.equal(leak.reason, "missing-export");
});

test("an observed import of an undeclared non-peer package is unknown", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [typesExampleRel]: `${typesExample}\nimport pad from "left-pad";\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const unknown = receipt.errors.find(
    (item) => item.code === "unpublished-export" && item.specifier === "left-pad",
  );
  assert.ok(unknown);
  assert.equal(unknown.reason, "unknown-package");
});

test("manifest pins are validated against the running engine and the workspace", async () => {
  const header = {
    ...manifestHeader,
    pin: { ...manifestHeader.pin, node: ">=999", typescript: "0.0.0-not-installed" },
  };
  const receipt = await validate(opts({ overlays: { [manifestRel]: JSON.stringify(header) } }));
  assert.notEqual(receipt.completenessState, "complete");
  const node = receipt.errors.find(
    (item) => item.code === "pin-mismatch" && item.package === "node",
  );
  assert.ok(node);
  assert.equal(node.actual, process.version);
  const ts = receipt.errors.find(
    (item) => item.code === "pin-mismatch" && item.package === "typescript",
  );
  assert.ok(ts);
  assert.equal(ts.pin, "0.0.0-not-installed");
  assert.equal(typeof ts.actual, "string");
  const cleanReceipt = await validate(opts());
  assert.equal(
    cleanReceipt.errors.some((item) => item.code === "pin-mismatch" || item.code === "pin-invalid"),
    false,
  );
});

test("a workspace export map edit changes the source digest", async () => {
  const fresh = await clean();
  const pkgRel = "packages/types/package.json";
  const pkg = JSON.parse(readFileSync(join(repoRoot, pkgRel), "utf8"));
  pkg.exports = { ...pkg.exports, "./extra": "./index.js" };
  const edited = await validate(opts({ overlays: { [pkgRel]: JSON.stringify(pkg) } }));
  assert.notEqual(edited.sourceRevisions.digest, fresh.sourceRevisions.digest);
});

test("a generated-reference plan edit changes the source digest", async () => {
  const fresh = await clean();
  const plan = JSON.parse(readFileSync(join(repoRoot, planRel), "utf8"));
  const edited = await validate(
    opts({ overlays: { [planRel]: JSON.stringify({ ...plan, note: "edited" }) } }),
  );
  assert.equal(edited.completenessState, "complete");
  assert.notEqual(edited.sourceRevisions.digest, fresh.sourceRevisions.digest);
});

test("canonical receipts order capabilities by id and surface", () => {
  const canonical = canonicalizeReceipt({
    capabilities: [
      { id: "b", surface: "x" },
      { id: "a", surface: "y" },
      { id: "a", surface: "x" },
    ],
  });
  assert.deepEqual(
    canonical.capabilities.map((row) => `${row.id}:${row.surface}`),
    ["a:x", "a:y", "b:x"],
  );
});

test("streamed child output hashes like the whole text and retains bounded stderr", () => {
  const text = `line one\r\nline two\r\n${"x".repeat(200_000)}\r\n`;
  const digester = createTextDigester();
  digester.update(text.slice(0, 9));
  digester.update(text.slice(9, 10));
  for (let index = 10; index < text.length; index += 4096) {
    digester.update(text.slice(index, index + 4096));
  }
  assert.equal(digester.digest(), sha256Text(text));
  const bounded = createBoundedText(8);
  bounded.update("abcde");
  bounded.update("fghij");
  bounded.update("klm");
  assert.equal(bounded.value(), "abcdefgh");
  assert.equal(bounded.truncated(), true);
});
