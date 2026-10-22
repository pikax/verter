#!/usr/bin/env node

// Tests for observe-lane.mjs. Run: node --test scripts/observe-lane.test.mjs

import assert from "node:assert/strict";
import test from "node:test";

import {
  buildObserveLaneArgs,
  buildObserveLaneBuildArgs,
  observeLanePackages,
} from "./observe-lane.mjs";

function metadata(packages, members = packages.map((pkg) => pkg.id)) {
  return { packages, workspace_members: members };
}

const pkg = (name, features = {}) => ({ id: `${name}-id`, name, features });

test("selects exactly the workspace members declaring the observation feature", () => {
  const meta = metadata(
    [
      pkg("verter_session", { "semantic-observe": ["verter_audit/semantic-observe"] }),
      pkg("verter_audit", { "semantic-observe": [] }),
      pkg("verter_span", { serde: [] }),
      pkg("outside", { "semantic-observe": [] }),
    ],
    ["verter_session-id", "verter_audit-id", "verter_span-id"],
  );
  assert.deepEqual(observeLanePackages(meta), ["verter_audit", "verter_session"]);
});

test("switches the feature on for every selected package under the given filter", () => {
  const meta = metadata([
    pkg("b", { "semantic-observe": [] }),
    pkg("a", { "semantic-observe": [] }),
  ]);
  assert.deepEqual(buildObserveLaneArgs(meta, "all()"), [
    "nextest",
    "run",
    "--no-fail-fast",
    "-p",
    "a",
    "-p",
    "b",
    "--features",
    "a/semantic-observe,b/semantic-observe",
    "-E",
    "all()",
  ]);
});

test("the fixture-metadata build selection is the run's own package and feature set", () => {
  const meta = metadata([
    pkg("b", { "semantic-observe": [] }),
    pkg("a", { "semantic-observe": [] }),
  ]);
  const build = buildObserveLaneBuildArgs(meta);
  assert.deepEqual(build, [
    "-p",
    "a",
    "-p",
    "b",
    "--features",
    "a/semantic-observe,b/semantic-observe",
  ]);
  assert.deepEqual(buildObserveLaneArgs(meta, "all()").slice(3, 3 + build.length), build);
});

test("an empty selection fails closed instead of running nothing", () => {
  assert.throws(() => buildObserveLaneArgs(metadata([pkg("a")]), "all()"), /no workspace member/);
  assert.throws(() => buildObserveLaneBuildArgs(metadata([pkg("a")])), /no workspace member/);
});
