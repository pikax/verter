import assert from "node:assert/strict";
import test from "node:test";

import { IssueSyncError, renderIssueDescription, validateIssueContentCatalog } from "../index.mjs";

function catalog(overrides = {}) {
  return validateIssueContentCatalog({
    schema: 1,
    issue: [
      {
        node_id: "WORK",
        title: "Keep published results deterministic",
        problem:
          "Published results can currently lose their stable identity when equivalent inputs are processed more than once. This leaves consumers with stale or misattributed output, makes repeated execution unreliable, and prevents maintainers from distinguishing a valid publication from an accidental fallback.",
        expected_outcome:
          "Equivalent inputs retain one stable publication identity and preserve the provenance required by every downstream consumer.",
        acceptance: [
          "Equivalent inputs produce the same observable result.",
          "Incomplete inputs fail without publishing a misleading value.",
          "Repeated execution preserves identity and provenance.",
        ],
        ...overrides,
      },
    ],
  });
}

const authority = { nodes: [{ id: "WORK" }] };

test("reviewed issue content renders one stable human description", () => {
  const rendered = renderIssueDescription({
    nodeId: "WORK",
    authority,
    contentCatalog: catalog(),
  });
  assert.equal(rendered.title, "Keep published results deterministic");
  assert.match(rendered.body, /^## Problem\n\n/u);
  assert.match(rendered.body, /\n## Expected outcome\n\n/u);
  assert.match(rendered.body, /\n## Acceptance\n\n/u);
  assert.equal(rendered.body.endsWith("\nAI-Generated\n"), true);
  assert.equal(rendered.body.match(/^AI-Generated$/gmu)?.length, 1);
  assert.doesNotMatch(rendered.body, /\bWORK\b/u);
});

test("missing reviewed content fails before issue rendering", () => {
  assert.throws(
    () =>
      renderIssueDescription({
        nodeId: "WORK",
        authority,
        contentCatalog: validateIssueContentCatalog({ schema: 1, issue: [] }),
      }),
    (error) =>
      error instanceof IssueSyncError && /author stable human issue content/u.test(error.message),
  );
});

test("catalog validation rejects machine-oriented issue fields", () => {
  assert.throws(
    () => catalog({ model: "temporary-model" }),
    (error) => error instanceof IssueSyncError && /unknown field model/u.test(error.message),
  );
});

test("rendering rejects program terminology in the issue title", () => {
  assert.throws(
    () =>
      renderIssueDescription({
        nodeId: "WORK",
        authority,
        contentCatalog: catalog({ title: "Advance the DAG train" }),
      }),
    (error) => error instanceof IssueSyncError && /prohibited program prose/u.test(error.message),
  );
});
