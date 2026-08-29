import assert from "node:assert/strict";
import test from "node:test";

import {
  AI_GENERATED_FOOTER,
  countAiGeneratedFooters,
  ensureAiGeneratedFooter,
} from "../index.mjs";

test("issue provenance has one stable footer and removes model attribution", () => {
  const body = ensureAiGeneratedFooter(
    "Human description\n\nModel: first\nModel: second\n\nAI-Generated\nAI-Generated\n",
  );

  assert.equal(body, `Human description\n\n${AI_GENERATED_FOOTER}\n`);
  assert.equal(countAiGeneratedFooters(body), 1);
  assert.doesNotMatch(body, /^Model:/mu);
});

test("issue provenance preserves prose while adding the footer", () => {
  assert.equal(
    ensureAiGeneratedFooter("Human description\n"),
    "Human description\n\nAI-Generated\n",
  );
});

test("compliant provenance preserves original line endings byte for byte", () => {
  const body = "Human description\r\n\r\nAI-Generated\r\n";
  assert.equal(ensureAiGeneratedFooter(body), body);
});
