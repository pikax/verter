// node --test tests/documentation/DOC7/doc7.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalizeReceipt, validate } from "../../../docs/scripts/reference-harness.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const manifestRel = "examples/reference/manifest.json";
const packagingRel = "examples/reference/sdk/packaging.md";
const indexRel = "examples/reference/sdk/README.md";
const manifest = JSON.parse(readFileSync(join(repoRoot, manifestRel), "utf8"));
const topics = manifest.sdkGuides?.topics ?? [];
const topicIds = topics.map((topic) => topic.id).sort();
const product = JSON.parse(
  readFileSync(join(repoRoot, "tests/documentation/DOC7/products/sdk-docs-model.v1.json"), "utf8"),
);

function opts(extra = {}) {
  return { repoRoot, skipTypeinfoCheck: true, ...extra };
}

function mutatedManifest(edit) {
  const mutated = structuredClone(manifest);
  edit(mutated);
  return JSON.stringify(mutated);
}

test("the six charter topics are declared pending with a named producing node", () => {
  assert.deepEqual(topicIds, [
    "compatibility",
    "contribution",
    "debugging",
    "isolation",
    "packaging",
    "permissions",
  ]);
  for (const topic of topics) {
    assert.equal(topic.status, "pending");
    assert.equal(typeof topic.producingNode, "string");
    assert.ok(topic.producingNode.length > 0);
    assert.equal(topic.exampleId, undefined);
    assert.ok(topic.page.startsWith("sdk/"));
  }
});

test("clean tree passes and reports the sdk guide model truthfully", async () => {
  const receipt = canonicalizeReceipt(await validate(opts()));
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.errors.length, 0);
  assert.equal(receipt.sdkGuides.gateEnforced, false);
  assert.equal(receipt.sdkGuides.index, "sdk/README.md");
  assert.deepEqual(
    receipt.sdkGuides.topics.map((row) => row.id),
    topicIds,
  );
  assert.ok(receipt.sdkGuides.topics.every((row) => row.ok && row.status === "pending"));
});

test("the required topic set is enforced in both directions", async () => {
  const missing = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.sdkGuides.topics = m.sdkGuides.topics.filter((topic) => topic.id !== "compatibility");
        }),
      },
    }),
  );
  assert.ok(missing.errors.some((item) => item.code === "sdk-model-incomplete"));
  assert.notEqual(missing.completenessState, "complete");

  const extra = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.sdkGuides.topics.push({
            id: "localization",
            page: "sdk/localization.md",
            status: "pending",
            producingNode: "XSDK9",
          });
        }),
      },
    }),
  );
  assert.ok(extra.errors.some((item) => item.code === "sdk-model-unknown-topic"));
  assert.notEqual(extra.completenessState, "complete");
});

test("a supplied slot bound to a static sample manifest without an executable extension fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.examples.push({
            id: "sdk-static-sample",
            files: ["sdk/static/sample-manifest.json"],
          });
          const packaging = m.sdkGuides.topics.find((topic) => topic.id === "packaging");
          packaging.status = "supplied";
          packaging.exampleId = "sdk-static-sample";
          delete packaging.producingNode;
        }),
        "examples/reference/sdk/static/sample-manifest.json": '{\n  "commands": []\n}\n',
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "static-sdk-example");
  assert.ok(miss);
  assert.equal(miss.id, "packaging");
  assert.equal(miss.exampleId, "sdk-static-sample");
});

test("a supplied slot whose example declares no runnable entry fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.examples.push({
            id: "sdk-entryless",
            files: ["sdk/entryless/run.ts"],
          });
          const debugging = m.sdkGuides.topics.find((topic) => topic.id === "debugging");
          debugging.status = "supplied";
          debugging.exampleId = "sdk-entryless";
          delete debugging.producingNode;
        }),
        "examples/reference/sdk/entryless/run.ts": "export const untouched = 1;\n",
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "sdk-example-without-entry");
  assert.ok(miss);
  assert.equal(miss.id, "debugging");
});

test("a supplied slot that binds an executable example with a shipped entry passes", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.examples.push({
            id: "sdk-authoring",
            files: ["sdk/authoring/run.ts"],
            commands: ["verter-tsc"],
          });
          const packaging = m.sdkGuides.topics.find((topic) => topic.id === "packaging");
          packaging.status = "supplied";
          packaging.exampleId = "sdk-authoring";
          delete packaging.producingNode;
        }),
        "examples/reference/sdk/authoring/run.ts":
          'import { createProject } from "@verter/types";\nexport const project = createProject;\n',
      },
    }),
  );
  assert.equal(receipt.completenessState, "complete");
  const row = receipt.sdkGuides.topics.find((topic) => topic.id === "packaging");
  assert.equal(row.status, "supplied");
  assert.equal(row.exampleId, "sdk-authoring");
  assert.equal(row.ok, true);
});

test("the sdk documentation gate rejects pending topics", async () => {
  const receipt = await validate(opts({ sdkGate: true }));
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "sdk-gate-unsatisfied"));
  assert.equal(receipt.sdkGuides.gateEnforced, true);
});

test("the sdk documentation gate passes once every topic is supplied executably", async () => {
  const overlays = {
    "examples/reference/sdk/authoring/run.ts":
      'import { createProject } from "@verter/types";\nexport const project = createProject;\n',
    "examples/reference/sdk/isolation/run.ts":
      'import { openComponentMetaSession } from "@verter/component-meta";\nexport const open = openComponentMetaSession;\n',
    "examples/reference/sdk/permissions/run.ts":
      'import VerterVite from "@verter/unplugin/vite";\nexport const plugin = VerterVite;\n',
    "examples/reference/sdk/contribution/run.mjs":
      'import { PatchHidden } from "@verter/types";\nexport const hidden = PatchHidden;\n',
    "examples/reference/sdk/debugging/run.ts":
      'import { createProject } from "@verter/types";\nexport const project = createProject;\n',
    "examples/reference/sdk/compatibility/run.ts":
      'import { createProject } from "@verter/types";\nexport const project = createProject;\n',
  };
  const receipt = await validate(
    opts({
      sdkGate: true,
      overlays: {
        ...overlays,
        [manifestRel]: mutatedManifest((m) => {
          m.examples.push(
            { id: "sdk-authoring", files: ["sdk/authoring/run.ts"], commands: ["verter-tsc"] },
            { id: "sdk-isolation", files: ["sdk/isolation/run.ts"], commands: ["verter-lsp"] },
            { id: "sdk-permissions", files: ["sdk/permissions/run.ts"], commands: ["verter-mcp"] },
            {
              id: "sdk-contribution",
              files: ["sdk/contribution/run.mjs"],
              commands: ["verter-tsc"],
            },
            { id: "sdk-debugging", files: ["sdk/debugging/run.ts"], commands: ["verter-tsc"] },
            {
              id: "sdk-compatibility",
              files: ["sdk/compatibility/run.ts"],
              commands: ["verter-tsc"],
            },
          );
          for (const topic of m.sdkGuides.topics) {
            topic.status = "supplied";
            topic.exampleId = `sdk-${topic.id === "packaging" ? "authoring" : topic.id}`;
            delete topic.producingNode;
          }
        }),
      },
    }),
  );
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.sdkGuides.gateEnforced, true);
  assert.ok(receipt.sdkGuides.topics.every((row) => row.ok));
});

test("a pending slot without a producing node fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          delete m.sdkGuides.topics.find((topic) => topic.id === "isolation").producingNode;
        }),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "sdk-slot-unowned"));
});

test("a pending slot that presents an example fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          const topic = m.sdkGuides.topics.find((row) => row.id === "isolation");
          topic.exampleId = "component-meta-session";
        }),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "sdk-pending-slot-bound"));
});

test("a supplied slot bound to an unknown example fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          const topic = m.sdkGuides.topics.find((row) => row.id === "isolation");
          topic.status = "supplied";
          topic.exampleId = "sdk-never-landed";
          delete topic.producingNode;
        }),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "sdk-slot-unbound"));
});

test("a missing topic page is a partial result", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [packagingRel]: null,
      },
    }),
  );
  assert.equal(receipt.completenessState, "partial");
  assert.ok(receipt.errors.some((item) => item.code === "sdk-page-missing"));
});

test("a broken link inside a topic page fails", async () => {
  const page = readFileSync(join(repoRoot, packagingRel), "utf8");
  const receipt = await validate(
    opts({
      overlays: {
        [packagingRel]: `${page}\n[missing](./no-such-guide.md)\n`,
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "broken-link" && item.id === "packaging"));
});

test("a topic page the index does not list fails", async () => {
  const index = readFileSync(join(repoRoot, indexRel), "utf8");
  const receipt = await validate(
    opts({
      overlays: {
        [indexRel]: index.replace("(./packaging.md)", "(./not-packaging.md)"),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "sdk-page-unlisted");
  assert.ok(miss);
  assert.equal(miss.id, "packaging");
});

test("perturbed discovery order yields an identical canonical receipt", async () => {
  const left = canonicalizeReceipt(await validate(opts()));
  const right = canonicalizeReceipt(
    await validate(opts({ discoveryOrder: [...manifest.examples.map((row) => row.id)].reverse() })),
  );
  assert.deepEqual(right, left);
});

test("sdk pages participate in the source digest for incremental equality", async () => {
  const fresh = canonicalizeReceipt(await validate(opts()));
  const incremental = canonicalizeReceipt(await validate(opts({ priorReceipt: fresh })));
  assert.equal(fresh.completenessState, "complete");
  assert.deepEqual(incremental.sourceRevisions, fresh.sourceRevisions);
  assert.equal(incremental.completenessState, "complete");
});

test("edit fails and revert restores a complete receipt", async () => {
  const before = canonicalizeReceipt(await validate(opts()));
  const page = readFileSync(join(repoRoot, packagingRel), "utf8");
  const edited = await validate(
    opts({
      overlays: {
        [packagingRel]: `${page}\n[missing](./no-such-guide.md)\n`,
      },
    }),
  );
  assert.notEqual(edited.completenessState, "complete");
  const reverted = canonicalizeReceipt(await validate(opts()));
  assert.deepEqual(reverted, before);
});

test("the sdk guide home and product record the delivery contract", () => {
  const index = readFileSync(join(repoRoot, indexRel), "utf8");
  for (const topic of topics) {
    assert.ok(
      index.includes(`(./${topic.page.split("/").pop()})`),
      `${topic.id} linked from index`,
    );
    const page = readFileSync(join(repoRoot, "examples/reference", topic.page), "utf8");
    assert.match(page, new RegExp(`\\b${topic.id}\\b`));
    assert.match(page, /pending/);
    assert.doesNotMatch(page, /screenshot|roadmap claim/i);
  }
  assert.equal(product.id, "sdk-docs-model.v1");
  assert.equal(product.owner, "DOC7");
  assert.equal(product.harness, "docs-reference-harness");
  assert.equal(product.hostProfile.profile, "docs-domain");
  assert.equal(product.runtimeClaim, "none");
  assert.equal(product.gate.commandFlag, "--sdk-gate");
  assert.match(product.gate.rule, /executable/);
  assert.ok(product.uncertaintyNotes.length > 0);
  assert.ok(product.migrationNotes.length > 0);
  assert.ok(product.permissions.length > 0);
});
