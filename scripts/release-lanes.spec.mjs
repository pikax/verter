import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { test } from "node:test";

import {
  parseReleaseSubject,
  releaseLane,
  releaseSubject,
  RELEASE_LANES,
} from "./release-lanes.mjs";

const ROOT = resolve(import.meta.dirname, "..");
const script = join(ROOT, "scripts/release-lanes.mjs");

/**
 * The lane table is what decides whether a push to main becomes a tag, and
 * which one. Everything here is a case that would otherwise be discovered by a
 * release that did not happen, or one that happened twice.
 */

test("an unscoped release subject is the monorepo lane", () => {
  assert.deepEqual(parseReleaseSubject("release: v0.0.1-beta.5"), {
    lane: "",
    version: "0.0.1-beta.5",
  });
  assert.equal(releaseLane("").tag("0.0.1-beta.5"), "v0.0.1-beta.5");
  assert.equal(releaseSubject("", "0.0.1-beta.5"), "release: v0.0.1-beta.5");
});

test("a scoped release subject names its lane and tags under it", () => {
  assert.deepEqual(parseReleaseSubject("release(ide): v0.1.0"), {
    lane: "ide",
    version: "0.1.0",
  });
  assert.equal(releaseLane("ide").tag("0.1.0"), "ide/v0.1.0");
  assert.equal(releaseSubject("ide", "0.1.0"), "release(ide): v0.1.0");
});

test("the scoped and unscoped patterns cannot swallow each other", () => {
  // The `v` before the version is the whole reason: a scope can never appear
  // where the unscoped pattern expects a version, and vice versa.
  assert.equal(parseReleaseSubject("release: vscode 0.1.0"), null);
  assert.equal(parseReleaseSubject("release(ide): 0.1.0"), null);
  assert.equal(parseReleaseSubject("release(ide): v0.1.0").lane, "ide");
});

test("an ordinary commit is not a release commit", () => {
  for (const subject of [
    "feat(lsp): add hover provenance",
    "release",
    "release:",
    "release: v1",
    "release: v1.2",
    "release: v1.2.3 (#601)",
    "chore: release: v1.2.3",
    "release(IDE): v1.2.3", // lanes are lowercase
    "release(): v1.2.3",
  ]) {
    assert.equal(parseReleaseSubject(subject), null, `should not match: ${subject}`);
  }
});

test("prerelease versions parse, because the monorepo releases on them", () => {
  assert.equal(parseReleaseSubject("release: v0.0.1-beta.5").version, "0.0.1-beta.5");
  assert.equal(parseReleaseSubject("release: v1.0.0-rc.1").version, "1.0.0-rc.1");
});

test("an unknown lane is refused, and `-` is the monorepo", () => {
  assert.equal(releaseLane("nope"), null);
  assert.equal(releaseLane("-").id, "");
  assert.equal(releaseLane("").id, "");
  assert.equal(releaseLane(undefined).id, "");
});

test("every lane resolves the version its own source actually holds", () => {
  // `resolve` is what release-tag.yml reads; if a lane pointed at a file that
  // does not carry a version, the tagging job would tag nothing and say why
  // only in hindsight.
  for (const id of Object.keys(RELEASE_LANES)) {
    const out = execFileSync(process.execPath, [script, "resolve", id || "-"], {
      cwd: ROOT,
      encoding: "utf8",
    }).trim();
    const [version, tag] = out.split(" ");
    assert.match(version, /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/, `lane ${id || "(monorepo)"}`);
    assert.equal(tag, releaseLane(id).tag(version));
  }
});

test("the CLI refuses an unknown lane instead of resolving nothing", () => {
  assert.throws(() =>
    execFileSync(process.execPath, [script, "resolve", "nope"], {
      cwd: ROOT,
      encoding: "utf8",
      stdio: ["pipe", "pipe", "pipe"],
    }),
  );
});

test("no two lanes share a tag prefix", () => {
  // Two lanes tagging the same way would each start the other's release
  // workflow, and `git describe --match` could not tell their histories apart.
  const prefixes = Object.values(RELEASE_LANES).map((lane) => lane.tag(""));
  assert.equal(new Set(prefixes).size, prefixes.length, prefixes.join(", "));
});

test("release-tag.yml resolves and verifies through the table, not its own copy", () => {
  const workflow = readFileSync(join(ROOT, ".github/workflows/release-tag.yml"), "utf8");
  assert.match(workflow, /node scripts\/release-lanes\.mjs resolve/);
  assert.match(workflow, /node scripts\/release-lanes\.mjs verify/);
  // The workflow's own subject pattern must accept exactly what the table's
  // does — it is the first gate, and a stricter one would drop releases.
  const bash = workflow.match(/re='(\^release.*?)'/)?.[1];
  assert.ok(bash, "release-tag.yml must declare the release-subject pattern");
  // The bash ERE the workflow matches with is valid JS regex syntax as written,
  // so it can be exercised here directly rather than restated.
  const asJs = new RegExp(bash);
  for (const subject of ["release: v1.2.3", "release(ide): v0.1.0", "release: v0.0.1-beta.5"]) {
    assert.ok(asJs.test(subject), `release-tag.yml pattern must accept: ${subject}`);
    assert.ok(parseReleaseSubject(subject), `the table must accept: ${subject}`);
  }
  for (const subject of ["feat: x", "release: vscode 0.1.0"]) {
    assert.ok(!asJs.test(subject), `release-tag.yml pattern must reject: ${subject}`);
    assert.equal(parseReleaseSubject(subject), null, `the table must reject: ${subject}`);
  }
});

test("each lane's verification names real scripts", () => {
  for (const [id, lane] of Object.entries(RELEASE_LANES)) {
    assert.ok(lane.verify.length > 0, `lane ${id || "(monorepo)"} verifies nothing`);
    for (const [entry] of lane.verify) {
      readFileSync(join(ROOT, entry), "utf8");
    }
  }
});
