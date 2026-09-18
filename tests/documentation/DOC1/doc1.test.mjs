// node --test tests/documentation/DOC1/doc1.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalizeReceipt,
  loadWorkspacePackages,
  resolvePackageExport,
  resolveShippedCommand,
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
const manifest = JSON.parse(
  readFileSync(join(repoRoot, "examples/reference/manifest.json"), "utf8"),
);

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

test("an example citing a surface absent from the capability catalog fails", async () => {
  const mutated = structuredClone(manifest);
  mutated.examples[0].surfaces = ["invented.capability.matrix"];
  const receipt = await validate(
    opts({
      overlays: {
        "examples/reference/manifest.json": JSON.stringify(mutated),
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
