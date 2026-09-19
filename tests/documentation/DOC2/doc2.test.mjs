// node --test tests/documentation/DOC2/doc2.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalizeReceipt, validate } from "../../../docs/scripts/reference-harness.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const modelRel = "tests/documentation/DOC2/products/contributor-docs-model.v1.json";
const modelPath = join(repoRoot, ...modelRel.split("/"));
const model = JSON.parse(readFileSync(modelPath, "utf8"));
const walkthroughRel = model.pages.find((page) => page.id === "first-contribution").path;
const walkthroughPath = join(repoRoot, ...walkthroughRel.split("/"));
const walkthrough = readFileSync(walkthroughPath, "utf8");
const queryPageRel = model.pages.find((page) => page.id === "query-lifetimes").path;
const queryPagePath = join(repoRoot, ...queryPageRel.split("/"));
const queryPage = readFileSync(queryPagePath, "utf8");
const sourcePageRel = model.pages.find((page) => page.id === "source-identity").path;
const sourcePagePath = join(repoRoot, ...sourcePageRel.split("/"));
const sourcePage = readFileSync(sourcePagePath, "utf8");
const architecturePageRel = model.pages.find((page) => page.id === "architecture-contracts").path;
const architecturePage = readFileSync(join(repoRoot, ...architecturePageRel.split("/")), "utf8");
const schedulerRel = "crates/verter_scheduler/src/scheduler.rs";
const schedulerSource = readFileSync(join(repoRoot, ...schedulerRel.split("/")), "utf8");
const indexRel = model.index;
const indexPath = join(repoRoot, ...indexRel.split("/"));
const index = readFileSync(indexPath, "utf8");
const contracts = readFileSync(
  join(repoRoot, ...model.contractSources.dependencyContracts.split("/")),
  "utf8",
);

function opts(extra = {}) {
  return { repoRoot, skipTypeinfoCheck: true, ...extra };
}

async function clean() {
  return canonicalizeReceipt(await validate(opts()));
}

test("clean contributor docs pass and cover every contract hotspot", async () => {
  const receipt = await clean();
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.errors.length, 0);
  assert.ok(receipt.contributorDocs, "contributor docs receipt is present");
  assert.ok(receipt.contributorDocs.pages.length >= 4);
  assert.ok(receipt.contributorDocs.pages.every((page) => page.ok));
  assert.ok(receipt.contributorDocs.taughtInterfaces.length >= 8);
  assert.ok(receipt.contributorDocs.taughtInterfaces.every((row) => row.ok));
  for (const hotspot of receipt.contributorDocs.hotspots) {
    assert.equal(hotspot.documented, true, `hotspot ${hotspot.path} must be documented`);
  }
});

test("every contributor page is registered in the DOC0 docs inventory", () => {
  const inventory = JSON.parse(
    readFileSync(
      join(repoRoot, "tests", "documentation", "DOC0", "products", "docs-inventory.v1.json"),
      "utf8",
    ),
  );
  const byPath = new Map(inventory.assets.map((row) => [row.path, row]));
  for (const page of model.pages) {
    const row = byPath.get(page.path);
    assert.ok(row, `${page.path} is missing from the DOC0 inventory`);
    assert.equal(row.owner, "DOC2");
    assert.equal(row.audience, "contributor");
  }
  assert.ok(byPath.has(model.index));
});

test("the walkthrough teaches the canonical owners, not local substitutes", () => {
  const taught = model.taughtInterfaces.filter((row) => row.taughtIn === walkthroughRel);
  assert.ok(
    taught.some((row) => row.path === "crates/verter_diagnostics/src/rules/mod.rs"),
    "the walkthrough teaches lint registration through the owning registry",
  );
  assert.ok(
    walkthrough.includes("crates/verter_diagnostics/src/rules/reactivity/mod.rs"),
    "the walkthrough wires the category barrel before register_builtin_rules",
  );
  assert.ok(
    walkthrough.includes("pub use your_rule::YourRule"),
    "the walkthrough shows the category re-export",
  );
  assert.ok(
    walkthrough.includes("docs/lint-rules.md"),
    "the walkthrough names the authored lint reference",
  );
  assert.ok(
    walkthrough.includes("lint-count-drift"),
    "the walkthrough names the live registry count check",
  );
  assert.ok(
    !/regenerate rather than hand-edit/.test(walkthrough),
    "the walkthrough must not invent a lint-page generator",
  );
  assert.ok(
    !/write your own (import )?resolver|local resolver|your own cache|HashMap<.*cache/i.test(
      walkthrough,
    ),
    "the walkthrough must not teach a local resolver or a duplicate cache",
  );
  assert.ok(
    !/\btest_[a-z_]+\(/.test(walkthrough),
    "the walkthrough must not invoke test-only hooks",
  );
});

test("architecture contracts distinguish sync constructors from the native driver", () => {
  assert.ok(architecturePage.includes("new_sync"));
  assert.ok(architecturePage.includes("drive_one"));
  assert.ok(!/every constructor[\s\S]*spawns exactly one driver thread/.test(architecturePage));
  assert.ok(
    architecturePage.includes("node tests/architecture-health/ARH1/verify.mjs"),
    "the page cites the canonical ARH1 verifier",
  );
  assert.ok(
    !/whose `--provenance` mode\s+proves the pinned candidate is a real ancestor/.test(
      architecturePage,
    ),
    "the page must not claim --provenance currently proves ancestry",
  );
});

test("source identity names the live PositionMapper APIs", () => {
  assert.ok(sourcePage.includes("tsx_to_carrier"));
  assert.ok(sourcePage.includes("carrier_to_tsx"));
  assert.ok(!sourcePage.includes("tsx_to_vue"));
  assert.ok(!sourcePage.includes("vue_to_tsx"));
  assert.ok(sourcePage.includes("crates/verter_lsp/src/vue_assets.rs"));
  assert.ok(!/sole\s+`std::fs` boundary/.test(sourcePage));
});

test("a tutorial that teaches a local resolver fails", async () => {
  const localResolverRel = "docs/contributing/assets/local_resolver.rs";
  const mutated = structuredClone(model);
  mutated.taughtInterfaces.push({
    capability: "import-resolution",
    path: localResolverRel,
    symbol: "resolve_import",
    taughtIn: walkthroughRel,
  });
  const receipt = await validate(
    opts({
      overlays: {
        [modelRel]: JSON.stringify(mutated),
        [localResolverRel]: "pub fn resolve_import() {}\n",
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "unowned-taught-path"));
});

test("a tutorial that teaches a duplicate cache fails", async () => {
  const fakeCacheRel = "crates/verter_lsp/src/session_cache.rs";
  const mutated = structuredClone(model);
  mutated.taughtInterfaces.push({
    capability: "semantic-query-memo",
    path: fakeCacheRel,
    symbol: "SessionCache",
    taughtIn: queryPageRel,
  });
  const receipt = await validate(
    opts({
      overlays: {
        [modelRel]: JSON.stringify(mutated),
        [fakeCacheRel]: "pub struct SessionCache;\n",
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "second-authority"));
});

test("a tutorial that teaches a test-only scheduler hook fails", async () => {
  const mutated = structuredClone(model);
  mutated.taughtInterfaces.push({
    capability: "scheduler-submission",
    path: "crates/verter_scheduler/src/scheduler.rs",
    symbol: "test_new",
    taughtIn: queryPageRel,
  });
  const receipt = await validate(
    opts({
      overlays: { [modelRel]: JSON.stringify(mutated) },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "test-only-api"));
});

test("a page-only test-only hook recommendation fails without a model edit", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [queryPageRel]: `${queryPage}\nCall \`test_new()\` to construct a scheduler.\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "test-only-api"));
});

test("a taught symbol dropped from its documentation page fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [sourcePageRel]: sourcePage.replaceAll("PositionMapper", "position mapper"),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "taught-page-missing-symbol"));
});

test("a taught interface whose symbol left its source fails", async () => {
  const mutated = structuredClone(model);
  const row = mutated.taughtInterfaces.find((item) => item.capability === "scheduler-submission");
  row.symbol = "submit_batch_renamed";
  const receipt = await validate(
    opts({
      overlays: { [modelRel]: JSON.stringify(mutated) },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "taught-symbol-missing"));
});

test("a cited source path that no longer exists fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [walkthroughRel]: `${walkthrough}\nRetired home: \`crates/verter_diagnostics/src/rules/retired.rs\`.\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "broken-citation"));
});

test("a dropped contract citation fails", async () => {
  const citations = model.pages.find((page) => page.id === "architecture-contracts");
  const pageRel = citations.path;
  const page = readFileSync(join(repoRoot, ...pageRel.split("/")), "utf8");
  const receipt = await validate(
    opts({
      overlays: {
        [pageRel]: page.replaceAll(
          model.contractSources.dependencyContracts,
          "dependency contracts",
        ),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "contract-citation-missing"));
});

test("an undocumented contract hotspot fails", async () => {
  const mutated = structuredClone(model);
  const dropped = "crates/verter_session/src/flow_slice_content.rs";
  for (const page of mutated.pages) {
    page.documentsHotspots = (page.documentsHotspots ?? []).filter(
      (hotspot) => hotspot !== dropped,
    );
  }
  const receipt = await validate(
    opts({
      overlays: { [modelRel]: JSON.stringify(mutated) },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "hotspot-undocumented"));
});

test("a missing contributor page is a partial result", async () => {
  const receipt = await validate(
    opts({
      overlays: { [walkthroughRel]: null },
    }),
  );
  assert.equal(receipt.completenessState, "partial");
  assert.ok(receipt.errors.some((item) => item.code === "docs-page-missing"));
});

test("a page unlisted by the contributing index fails", async () => {
  const stripped = index.replace(
    /\[First Contribution\]\(\.\/first-contribution(?:\.md)?\)/,
    "First Contribution",
  );
  const receipt = await validate(
    opts({
      overlays: { [indexRel]: stripped },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "docs-page-unlisted"));
});

test("a missing contract source fails", async () => {
  const receipt = await validate(
    opts({
      overlays: { [model.contractSources.dependencyContracts]: null },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "contract-source-missing"));
});

test("perturbed model order yields an identical canonical receipt", async () => {
  const perturbed = structuredClone(model);
  perturbed.pages.reverse();
  perturbed.taughtInterfaces.reverse();
  const left = await clean();
  const right = canonicalizeReceipt(
    await validate(
      opts({
        overlays: { [modelRel]: JSON.stringify(perturbed) },
      }),
    ),
  );
  assert.deepEqual(right, left);
});

test("incremental receipt with the same digest matches a fresh run", async () => {
  const fresh = canonicalizeReceipt(await validate(opts()));
  const incremental = canonicalizeReceipt(await validate(opts({ priorReceipt: fresh })));
  assert.equal(fresh.completenessState, "complete");
  assert.deepEqual(incremental.sourceRevisions, fresh.sourceRevisions);
  assert.equal(incremental.completenessState, "complete");
});

test("a taught-source edit changes receipt identity", async () => {
  const fresh = canonicalizeReceipt(await validate(opts()));
  const renamed = await validate(
    opts({
      overlays: {
        [schedulerRel]: schedulerSource.replaceAll("submit_batch_atomic", "submit_batch_renamed"),
      },
      priorReceipt: fresh,
    }),
  );
  assert.notEqual(renamed.completenessState, "complete");
  assert.ok(renamed.errors.some((item) => item.code === "taught-symbol-missing"));
  assert.notEqual(renamed.sourceRevisions.digest, fresh.sourceRevisions.digest);
  assert.ok(!renamed.errors.some((item) => item.code === "incremental-mismatch"));

  const commented = canonicalizeReceipt(
    await validate(
      opts({
        overlays: { [schedulerRel]: `${schedulerSource}\n// contributor-docs identity probe\n` },
      }),
    ),
  );
  assert.equal(commented.completenessState, "complete");
  assert.notEqual(commented.sourceRevisions.digest, fresh.sourceRevisions.digest);
});

test("edit fails and revert restores a complete receipt", async () => {
  const before = await clean();
  const edited = await validate(
    opts({
      overlays: {
        [walkthroughRel]: `${walkthrough}\nAlso see \`crates/verter_diagnostics/src/rules/retired.rs\`.\n`,
      },
    }),
  );
  assert.notEqual(edited.completenessState, "complete");
  const reverted = await clean();
  assert.deepEqual(reverted, before);
});

test("pre-aborted signal yields cancelled and never complete", async () => {
  const receipt = await validate(opts({ signal: AbortSignal.abort() }));
  assert.equal(receipt.completenessState, "cancelled");
  assert.ok(receipt.errors.some((item) => item.code === "cancelled"));
});

test("mid-page cancellation yields cancelled with a source digest", async () => {
  let seen = 0;
  const signal = {
    get aborted() {
      seen += 1;
      return seen > 12;
    },
  };
  const receipt = await validate(opts({ signal }));
  assert.equal(receipt.completenessState, "cancelled");
  assert.equal(typeof receipt.sourceRevisions.digest, "string");
  assert.match(receipt.sourceRevisions.digest, /^[0-9a-f]{64}$/);
  assert.ok(receipt.errors.some((item) => item.code === "cancelled"));
});

test("the walkthrough cites the current contracts and real code", () => {
  assert.ok(model.pages.every((page) => Array.isArray(page.requiredCitations)));
  const architecturePage = model.pages.find((page) => page.id === "architecture-contracts");
  assert.ok(architecturePage.requiredCitations.includes(model.contractSources.dependencyContracts));
  assert.ok(contracts.includes(architecturePage.documentsHotspots[0]));
});
