// node --test tests/documentation/DOC5/doc5.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalizeReceipt,
  classifyJourneyOutcome,
  validate,
} from "../../../docs/scripts/reference-harness.mjs";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const manifestRel = "examples/reference/manifest.json";
const cssRel = "examples/reference/guides/css.md";
const indexRel = "examples/reference/guides/README.md";
const fixtureRoot = join(repoRoot, "tests", "documentation", "DOC5", "fixtures", "example-home");
const manifest = JSON.parse(readFileSync(join(repoRoot, manifestRel), "utf8"));
const topics = manifest.recipes?.topics ?? [];
const topicIds = topics.map((topic) => topic.id).sort();
const journeys = manifest.journeys ?? [];
const product = JSON.parse(
  readFileSync(
    join(repoRoot, "tests/documentation/DOC5/products/web-product-guides-model.v1.json"),
    "utf8",
  ),
);

function opts(extra = {}) {
  return { repoRoot, skipTypeinfoCheck: true, ...extra };
}

function mutatedManifest(edit) {
  const mutated = structuredClone(manifest);
  edit(mutated);
  return JSON.stringify(mutated);
}

test("the eight charter recipe topics are declared pending with a named producing node", () => {
  assert.deepEqual(topicIds, [
    "accessibility",
    "compatibility",
    "css",
    "debug",
    "performance",
    "runtime",
    "security",
    "tests",
  ]);
  for (const topic of topics) {
    assert.equal(topic.status, "pending");
    assert.equal(typeof topic.producingNode, "string");
    assert.ok(topic.producingNode.length > 0);
    assert.equal(topic.exampleId, undefined);
    assert.ok(topic.page.startsWith("guides/"));
  }
});

test("a real native journey is declared over the shipped cli example and stays not-run by default", async () => {
  const smoke = journeys.find((journey) => journey.id === "cli-version-smoke");
  assert.ok(smoke, "cli-version-smoke journey is declared");
  assert.equal(smoke.executionClass, "NativeOnly");
  assert.equal(smoke.steps.length, 1);
  assert.equal(smoke.steps[0].exampleId, "shipped-cli");
  assert.equal(smoke.steps[0].command, "verter-tsc");

  const receipt = canonicalizeReceipt(await validate(opts()));
  assert.equal(receipt.completenessState, "complete");
  const row = receipt.journeys.find((journey) => journey.id === "cli-version-smoke");
  assert.ok(row);
  assert.equal(row.execution.state, "not-run");
  assert.equal(row.ok, true);
});

test("clean tree passes and reports the recipe model truthfully", async () => {
  const receipt = canonicalizeReceipt(await validate(opts()));
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.errors.length, 0);
  assert.equal(receipt.recipes.gateEnforced, false);
  assert.equal(receipt.recipes.index, "guides/README.md");
  assert.deepEqual(
    receipt.recipes.topics.map((row) => row.id),
    topicIds,
  );
  assert.ok(receipt.recipes.topics.every((row) => row.ok && row.status === "pending"));
});

test("the required recipe topic set is enforced in both directions", async () => {
  const missing = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.recipes.topics = m.recipes.topics.filter((topic) => topic.id !== "css");
        }),
      },
    }),
  );
  assert.ok(missing.errors.some((item) => item.code === "recipes-model-incomplete"));
  assert.notEqual(missing.completenessState, "complete");

  const extra = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.recipes.topics.push({
            id: "localization",
            page: "guides/localization.md",
            status: "pending",
            producingNode: "L10N0",
          });
        }),
      },
    }),
  );
  assert.ok(extra.errors.some((item) => item.code === "recipes-model-unknown-topic"));
  assert.notEqual(extra.completenessState, "complete");
});

test("an explicitly empty journeys model is rejected while an absent section stays legal", async () => {
  const empty = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.journeys = [];
        }),
      },
    }),
  );
  assert.notEqual(empty.completenessState, "complete");
  assert.ok(empty.errors.some((item) => item.code === "journeys-empty"));

  const absent = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          delete m.journeys;
        }),
      },
    }),
  );
  assert.equal(absent.completenessState, "complete");
  assert.equal(absent.journeys.length, 0);
});

test("a supplied recipe bound to a static sample manifest without an executable extension fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.examples.push({
            id: "recipe-static-sample",
            files: ["guides/static/sample-manifest.json"],
            surfaces: ["vue.css.semantics"],
          });
          const css = m.recipes.topics.find((topic) => topic.id === "css");
          css.status = "supplied";
          css.exampleId = "recipe-static-sample";
          delete css.producingNode;
        }),
        "examples/reference/guides/static/sample-manifest.json": '{\n  "commands": []\n}\n',
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "recipes-static-example");
  assert.ok(miss);
  assert.equal(miss.id, "css");
});

test("a supplied recipe whose example declares no capability link fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.examples.push({
            id: "recipe-surfaceless",
            files: ["guides/surfaceless/run.ts"],
            commands: ["verter-tsc"],
          });
          const css = m.recipes.topics.find((topic) => topic.id === "css");
          css.status = "supplied";
          css.exampleId = "recipe-surfaceless";
          delete css.producingNode;
        }),
        "examples/reference/guides/surfaceless/run.ts":
          'import { createProject } from "@verter/types";\nexport const project = createProject;\n',
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "recipes-without-capability-link");
  assert.ok(miss);
  assert.equal(miss.id, "css");
});

test("the recipes documentation gate rejects pending topics", async () => {
  const receipt = await validate(opts({ recipesGate: true }));
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "recipes-gate-unsatisfied"));
  assert.equal(receipt.recipes.gateEnforced, true);
});

test("the recipes documentation gate passes once every topic is supplied executably with a capability link", async () => {
  const surfaceByTopic = {
    accessibility: "vue.language_service.typing",
    compatibility: "vue.tsc.project_check",
    css: "vue.css.semantics",
    debug: "vue.language_service.edit",
    performance: "vue.direct.compile.raw",
    runtime: "vue.direct.compile.css",
    security: "svelte.host_lint",
    tests: "svelte.tsc.project_check",
  };
  const receipt = await validate(
    opts({
      recipesGate: true,
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          for (const topic of m.recipes.topics) {
            const exampleId = `recipe-${topic.id}`;
            m.examples.push({
              id: exampleId,
              files: [`guides/${topic.id}/run.ts`],
              commands: ["verter-tsc"],
              surfaces: [surfaceByTopic[topic.id]],
            });
            topic.status = "supplied";
            topic.exampleId = exampleId;
            delete topic.producingNode;
          }
        }),
        ...Object.fromEntries(
          topicIds.map((id) => [
            `examples/reference/guides/${id}/run.ts`,
            'import { createProject } from "@verter/types";\nexport const project = createProject;\n',
          ]),
        ),
      },
    }),
  );
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.recipes.gateEnforced, true);
  assert.ok(receipt.recipes.topics.every((row) => row.ok && row.status === "supplied"));
});

test("journey structural validation rejects unknown examples, unshipped commands and outside entries", async () => {
  const unknown = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.journeys.push({
            id: "broken",
            description: "binds nothing",
            executionClass: "NativeOnly",
            steps: [{ exampleId: "never-declared", command: "verter-tsc", expectExit: 0 }],
          });
        }),
      },
    }),
  );
  assert.ok(unknown.errors.some((item) => item.code === "journey-step-unknown-example"));

  const unshipped = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.journeys.push({
            id: "broken",
            description: "runs a cargo command",
            executionClass: "NativeOnly",
            steps: [{ exampleId: "shipped-cli", command: "cargo", expectExit: 0 }],
          });
        }),
      },
    }),
  );
  assert.ok(unshipped.errors.some((item) => item.code === "journey-step-unshipped-command"));

  const outside = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.journeys.push({
            id: "broken",
            description: "runs a command the example does not declare",
            executionClass: "NativeOnly",
            steps: [
              {
                exampleId: "component-meta-session",
                command: "verter-tsc",
                expectExit: 0,
              },
            ],
          });
        }),
      },
    }),
  );
  assert.ok(outside.errors.some((item) => item.code === "journey-step-command-outside-example"));

  const classMismatch = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.journeys.push({
            id: "broken",
            description: "declares NativeOnly but runs a file",
            executionClass: "NativeOnly",
            steps: [
              { exampleId: "types-helpers", file: "types-helpers/example.ts", expectExit: 0 },
            ],
          });
        }),
      },
    }),
  );
  assert.ok(classMismatch.errors.some((item) => item.code === "journey-class-mismatch"));
});

test("a duplicate journey id fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.journeys.push(structuredClone(m.journeys[0]));
        }),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "duplicate-journey"));
});

test("node journeys execute for real and record runtime observation", async () => {
  const receipt = canonicalizeReceipt(
    await validate({
      repoRoot,
      examplesRoot: fixtureRoot,
      skipTypeinfoCheck: true,
      runJourneys: true,
    }),
  );
  assert.equal(receipt.completenessState, "complete");
  assert.equal(receipt.errors.length, 0);
  const ok = receipt.journeys.find((journey) => journey.id === "echo-ok");
  assert.equal(ok.execution.state, "executed");
  assert.equal(ok.execution.steps.length, 1);
  assert.equal(ok.execution.steps[0].exitCode, 0);
  assert.equal(ok.execution.steps[0].ok, true);
  assert.equal(ok.execution.steps[0].stdoutDigest.length, 64);
  const failing = receipt.journeys.find((journey) => journey.id === "echo-expect-fail");
  assert.equal(failing.execution.steps[0].exitCode, 7);
  assert.equal(failing.execution.steps[0].expectExit, 7);
  assert.equal(failing.execution.steps[0].ok, true);
});

test("an executed step that misses its expected exit code fails the journey lane", async () => {
  const fixtureManifest = JSON.parse(readFileSync(join(fixtureRoot, "manifest.json"), "utf8"));
  const receipt = await validate({
    repoRoot,
    examplesRoot: fixtureRoot,
    skipTypeinfoCheck: true,
    runJourneys: true,
    overlays: {
      "tests/documentation/DOC5/fixtures/example-home/manifest.json": JSON.stringify({
        ...fixtureManifest,
        journeys: [
          {
            id: "echo-wrong-expectation",
            description: "expects zero but the script exits seven",
            executionClass: "NodeOnly",
            steps: [{ exampleId: "echo", file: "echo/fail.mjs", expectExit: 0 }],
          },
        ],
      }),
    },
  });
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "journey-step-mismatch");
  assert.ok(miss);
  assert.equal(miss.journey, "echo-wrong-expectation");
  assert.equal(miss.exitCode, 7);
});

test("a step that exceeds its bounded runtime times out and fails", async () => {
  const fixtureManifest = JSON.parse(readFileSync(join(fixtureRoot, "manifest.json"), "utf8"));
  const receipt = await validate({
    repoRoot,
    examplesRoot: fixtureRoot,
    skipTypeinfoCheck: true,
    runJourneys: true,
    journeyStepTimeoutMs: 250,
    overlays: {
      "tests/documentation/DOC5/fixtures/example-home/manifest.json": JSON.stringify({
        ...fixtureManifest,
        journeys: [
          {
            id: "echo-hang",
            description: "sleeps past the bound",
            executionClass: "NodeOnly",
            steps: [{ exampleId: "echo", file: "echo/slow.mjs", expectExit: 0 }],
          },
        ],
      }),
    },
  });
  assert.notEqual(receipt.completenessState, "complete");
  assert.ok(receipt.errors.some((item) => item.code === "journey-step-timeout"));
});

test("aborting during a step cancels the journey lane and kills the child", async () => {
  const fixtureManifest = JSON.parse(readFileSync(join(fixtureRoot, "manifest.json"), "utf8"));
  const controller = new AbortController();
  const pending = validate({
    repoRoot,
    examplesRoot: fixtureRoot,
    skipTypeinfoCheck: true,
    runJourneys: true,
    signal: controller.signal,
    overlays: {
      "tests/documentation/DOC5/fixtures/example-home/manifest.json": JSON.stringify({
        ...fixtureManifest,
        journeys: [
          {
            id: "echo-abort",
            description: "sleeps until aborted",
            executionClass: "NodeOnly",
            steps: [{ exampleId: "echo", file: "echo/slow.mjs", expectExit: 0 }],
          },
        ],
      }),
    },
  });
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 500));
  controller.abort();
  const receipt = await pending;
  assert.equal(receipt.completenessState, "cancelled");
  const row = receipt.journeys.find((journey) => journey.id === "echo-abort");
  assert.equal(row.execution.state, "cancelled");
});

test("perturbed discovery order yields an identical canonical receipt", async () => {
  const left = canonicalizeReceipt(await validate(opts()));
  const right = canonicalizeReceipt(
    await validate(opts({ discoveryOrder: [...manifest.examples.map((row) => row.id)].reverse() })),
  );
  assert.deepEqual(right, left);
});

test("recipe pages and journeys participate in the source digest for incremental equality", async () => {
  const fresh = canonicalizeReceipt(await validate(opts()));
  const incremental = canonicalizeReceipt(await validate(opts({ priorReceipt: fresh })));
  assert.equal(fresh.completenessState, "complete");
  assert.deepEqual(incremental.sourceRevisions, fresh.sourceRevisions);
  assert.equal(incremental.completenessState, "complete");
});

test("edit fails and revert restores a complete receipt", async () => {
  const before = canonicalizeReceipt(await validate(opts()));
  const page = readFileSync(join(repoRoot, cssRel), "utf8");
  const edited = await validate(
    opts({
      overlays: {
        [cssRel]: `${page}\n[missing](./no-such-guide.md)\n`,
      },
    }),
  );
  assert.notEqual(edited.completenessState, "complete");
  const reverted = canonicalizeReceipt(await validate(opts()));
  assert.deepEqual(reverted, before);
});

test("a missing recipe page is a partial result", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [cssRel]: null,
      },
    }),
  );
  assert.equal(receipt.completenessState, "partial");
  assert.ok(receipt.errors.some((item) => item.code === "recipes-page-missing"));
});

test("a recipe page the index does not list fails", async () => {
  const index = readFileSync(join(repoRoot, indexRel), "utf8");
  const receipt = await validate(
    opts({
      overlays: {
        [indexRel]: index.replace("(./css.md)", "(./not-css.md)"),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const miss = receipt.errors.find((item) => item.code === "recipes-page-unlisted");
  assert.ok(miss);
  assert.equal(miss.id, "css");
});

test("the recipe home and product record the delivery contract", () => {
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
  assert.equal(product.id, "web-product-guides-model.v1");
  assert.equal(product.owner, "DOC5");
  assert.equal(product.harness, "docs-reference-harness");
  assert.equal(product.hostProfile.profile, "docs-domain");
  assert.equal(product.gate.commandFlag, "--recipes-gate");
  assert.match(product.gate.rule, /capability/);
  assert.equal(product.journeys.executionFlag, "--run-journeys");
  assert.ok(product.journeys.stepTimeoutMs >= 1000);
  assert.equal(product.runtimeClaim, "none by default");
  assert.ok(product.uncertaintyNotes.length > 0);
  assert.ok(product.migrationNotes.length > 0);
  assert.ok(product.permissions.length > 0);
});

test("a duplicate recipe topic id fails", async () => {
  const receipt = await validate(
    opts({
      overlays: {
        [manifestRel]: mutatedManifest((m) => {
          m.recipes.topics.push(structuredClone(m.recipes.topics[0]));
        }),
      },
    }),
  );
  assert.notEqual(receipt.completenessState, "complete");
  const dup = receipt.errors.find((item) => item.code === "recipes-duplicate-topic");
  assert.ok(dup);
  assert.equal(dup.id, manifest.recipes.topics[0].id);
});

test("an overlaid journey entry is refused instead of executing the on-disk file", async () => {
  const runRel = "tests/documentation/DOC5/fixtures/example-home/echo/run.mjs";
  const receipt = await validate({
    repoRoot,
    examplesRoot: fixtureRoot,
    skipTypeinfoCheck: true,
    runJourneys: true,
    overlays: {
      [runRel]: `${readFileSync(join(repoRoot, runRel), "utf8")}\n// edited through an overlay\n`,
    },
  });
  assert.notEqual(receipt.completenessState, "complete");
  const refused = receipt.errors.find((item) => item.code === "journey-step-overlaid");
  assert.ok(refused);
  assert.equal(refused.journey, "echo-ok");
  assert.equal(refused.overlay, runRel);
  const row = receipt.journeys.find((journey) => journey.id === "echo-ok");
  assert.equal(row.ok, false);
  assert.equal(row.execution.state, "executed");
  assert.equal(row.execution.steps[0].stdoutDigest, null);
});

test("a child terminated by a signal is a failed step unless the caller aborted", () => {
  const bySignal = { exitCode: null, signal: "SIGKILL", timedOut: false };
  assert.equal(classifyJourneyOutcome(bySignal, false), "signalled");
  assert.equal(classifyJourneyOutcome(bySignal, true), "cancelled");
  assert.equal(
    classifyJourneyOutcome({ exitCode: 0, signal: null, timedOut: false }, false),
    "exited",
  );
  assert.equal(
    classifyJourneyOutcome({ exitCode: null, signal: null, timedOut: true }, true),
    "timed-out",
  );
});
