import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { GitHubAdapterError, rehearsalIdentity, releasePlan } from "../index.mjs";
import {
  CLEAN_ROOM_KIND,
  assertCleanRoomHosted,
  declaredEntrypoints,
  runCleanRoomCheck,
} from "../clean-room.mjs";
import { FakeGitHubAdapter } from "../fake.mjs";
import { writeLedgerFixture } from "./ledger-fixture.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLEAN_ROOM = path.join(HERE, "../clean-room.mjs");
const MILESTONE = "v0.1.0";

// REL3 performance N/A: the check runs once per rehearsal and owns no hot parse/resolve path.

function tmp(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

function writePkg(root, folder, pkg, files) {
  const dir = path.join(root, "packages", folder);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "package.json"), `${JSON.stringify(pkg, null, 2)}\n`);
  for (const [rel, body] of Object.entries(files)) {
    const full = path.join(dir, rel);
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, body);
  }
  return dir;
}

function pack(dir, dest) {
  fs.mkdirSync(dest, { recursive: true });
  const before = new Set(fs.readdirSync(dest));
  const result = spawnSync("npm", ["pack", "--pack-destination", dest], {
    cwd: dir,
    encoding: "utf8",
    shell: process.platform === "win32",
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const created = fs.readdirSync(dest).filter((name) => name.endsWith(".tgz") && !before.has(name));
  assert.equal(created.length, 1, `expected one tarball, got ${created.join(", ")}`);
  return path.join(dest, created[0]);
}

function unitFrom(dir) {
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
  return { name: pkg.name, dir, kind: "package" };
}

function check(units, extra = {}) {
  const workDir = extra.workDir ?? tmp("rel3-run-");
  return runCleanRoomCheck({
    repoRoot: extra.repoRoot ?? path.dirname(path.dirname(units[0].dir)),
    workDir,
    units,
    tarballs:
      extra.tarballs ??
      units.map((unit) => ({
        name: unit.name,
        tarball: pack(unit.dir, extra.packDir ?? tmp("rel3-pack-")),
      })),
    skip: extra.skip,
  });
}

function hostedReleaseYml() {
  return `name: Release
jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - run: echo validate
  clean-room:
    name: Clean-room published-package smoke
    needs: [validate]
    runs-on: ubuntu-latest
    steps:
      - run: node scripts/githubctl/clean-room.mjs
`;
}

test("REL3-AC2 declaredEntrypoints covers import, require, and types-only", () => {
  assert.deepEqual(
    declaredEntrypoints({
      name: "@rel3/dual",
      exports: {
        ".": {
          types: "./dist/index.d.ts",
          import: "./dist/index.mjs",
          require: "./dist/index.cjs",
        },
      },
    }).map((row) => `${row.subpath}:${row.conditions.join("+")}:${row.file}`),
    [".:import:dist/index.mjs", ".:require:dist/index.cjs"],
  );
  assert.equal(
    declaredEntrypoints({
      name: "@rel3/types",
      exports: { ".": { types: "./jsx-runtime.d.ts" } },
    })[0].typesOnly,
    true,
  );
});

test("REL3-AC2 every published package and entrypoint is derived from the publish set", () => {
  const root = tmp("rel3-ws-");
  const alpha = writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: {
        ".": { import: "./index.js" },
        "./util": { import: "./util.js" },
      },
    },
    {
      "index.js": 'export function cleanRoomPing() { return "alpha"; }\n',
      "util.js": 'export function cleanRoomPing() { return "util"; }\n',
    },
  );
  const dual = writePkg(
    root,
    "dual",
    {
      name: "@rel3/dual",
      version: "1.0.0",
      exports: {
        ".": { import: "./index.mjs", require: "./index.cjs" },
      },
    },
    {
      "index.mjs": 'export function cleanRoomPing() { return "dual-esm"; }\n',
      "index.cjs": 'exports.cleanRoomPing = () => "dual-cjs";\n',
    },
  );
  const types = writePkg(
    root,
    "types",
    {
      name: "@rel3/types",
      version: "1.0.0",
      type: "module",
      exports: { ".": { types: "./index.d.ts" } },
    },
    { "index.d.ts": "export const ping: string;\n" },
  );
  const report = check([unitFrom(alpha), unitFrom(dual), unitFrom(types)], { repoRoot: root });
  assert.equal(report.ok, true, report.message ?? JSON.stringify(report.failures, null, 2));
  assert.equal(report.skipped, false);
  assert.equal(report.kind, CLEAN_ROOM_KIND);
  const keys = report.entrypoints
    .map((row) => `${row.package} ${row.entrypoint} ${row.condition}`)
    .sort();
  assert.deepEqual(keys, [
    "@rel3/alpha . import",
    "@rel3/alpha ./util import",
    "@rel3/dual . import",
    "@rel3/dual . require",
    "@rel3/types . types",
  ]);
});

test("REL3-AC2 a package in the publish set that is not packed fails rather than passing silently", () => {
  const root = tmp("rel3-uncovered-");
  const alpha = writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": 'export function cleanRoomPing() { return "alpha"; }\n' },
  );
  const packed = pack(alpha, tmp("rel3-pack-"));
  const report = runCleanRoomCheck({
    repoRoot: root,
    workDir: tmp("rel3-run-"),
    units: [
      unitFrom(alpha),
      { name: "@rel3/missing", dir: path.join(root, "packages", "missing"), kind: "package" },
    ],
    tarballs: [{ name: "@rel3/alpha", tarball: packed }],
  });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "uncovered-package");
  assert.match(report.message, /@rel3\/missing/u);
});

test("REL3-AC2 does not hard-maintain the published package list", () => {
  const text = fs.readFileSync(CLEAN_ROOM, "utf8");
  assert.match(text, /computePublishSet/u);
  assert.doesNotMatch(text, /@verter\/proto/u);
  assert.doesNotMatch(text, /hand-?maintained/iu);
});

test("REL3-AC1 scratch consumer cannot reach the repository", () => {
  const root = tmp("rel3-iso-");
  const alpha = writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": 'export function cleanRoomPing() { return "alpha"; }\n' },
  );
  const report = check([unitFrom(alpha)], { repoRoot: root });
  assert.equal(report.ok, true, report.message);
  assert.equal(inside(report.consumerDir, root), false);
  for (const row of report.entrypoints.filter((item) => item.resolved)) {
    assert.equal(inside(row.resolved, report.consumerDir), true, row.resolved);
    assert.equal(inside(row.resolved, root), false, row.resolved);
  }
});

test("REL3-AC1 a source-tree fallback consumer fails the check", () => {
  const repoRoot = tmp("rel3-src-");
  const workDir = path.join(repoRoot, "inside-repo");
  fs.mkdirSync(workDir, { recursive: true });
  const alpha = writePkg(
    repoRoot,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": 'export function cleanRoomPing() { return "alpha"; }\n' },
  );
  const report = runCleanRoomCheck({
    repoRoot,
    workDir,
    units: [unitFrom(alpha)],
    tarballs: [{ name: "@rel3/alpha", tarball: pack(alpha, tmp("rel3-pack-")) }],
  });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "source-tree-fallback");
});

test("REL3-AC3 a previously used work directory cannot be reused as evidence", () => {
  const root = tmp("rel3-fresh-");
  const alpha = writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": 'export function cleanRoomPing() { return "alpha"; }\n' },
  );
  const workDir = tmp("rel3-run-");
  const first = check([unitFrom(alpha)], { repoRoot: root, workDir });
  assert.equal(first.ok, true, first.message);
  const second = check([unitFrom(alpha)], { repoRoot: root, workDir });
  assert.equal(second.ok, false);
  assert.equal(second.reason, "reused-cache");
});

test("REL3-AC4 sibling import without .js extension fails for its own reason", () => {
  const root = tmp("rel3-noext-");
  const dir = writePkg(
    root,
    "noext",
    {
      name: "@rel3/noext",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    {
      "index.js": 'export { cleanRoomPing } from "./sibling";\n',
      "sibling.js": 'export function cleanRoomPing() { return "noext"; }\n',
    },
  );
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "missing-js-extension");
});

test("REL3-AC4 missing exports condition fails for its own reason", () => {
  const root = tmp("rel3-blocked-");
  const dir = writePkg(
    root,
    "blocked",
    {
      name: "@rel3/blocked",
      version: "1.0.0",
      type: "module",
      main: "index.js",
      exports: { ".": { types: "./index.d.ts" } },
    },
    {
      "index.js": 'export function cleanRoomPing() { return "blocked"; }\n',
      "index.d.ts": "export function cleanRoomPing(): string;\n",
    },
  );
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "missing-exports-condition");
});

test("REL3-AC4 declared entrypoint absent from the tarball fails for its own reason", () => {
  const root = tmp("rel3-ghost-");
  const dir = writePkg(
    root,
    "ghost",
    {
      name: "@rel3/ghost",
      version: "1.0.0",
      type: "module",
      files: ["present.js"],
      exports: { ".": { import: "./missing.js" } },
    },
    {
      "present.js": 'export function cleanRoomPing() { return "present"; }\n',
      "missing.js": 'export function cleanRoomPing() { return "ghost"; }\n',
    },
  );
  const pkgPath = path.join(dir, "package.json");
  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  pkg.files = ["present.js"];
  fs.writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  fs.unlinkSync(path.join(dir, "missing.js"));
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "missing-entrypoint-file");
});

test("REL3 skipped check cannot be reported as PASS", () => {
  const report = runCleanRoomCheck({ skip: true });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "skipped");
});

test("REL3-AC1 rehearsal identity refuses a skipped or missing clean-room host", () => {
  assert.throws(
    () =>
      assertCleanRoomHosted("name: Release\njobs:\n  publish-npm:\n    runs-on: ubuntu-latest\n"),
    GitHubAdapterError,
  );
  assert.throws(
    () =>
      assertCleanRoomHosted(`name: Release
jobs:
  clean-room:
    if: needs.validate.outputs.dry-run != 'true'
    steps:
      - run: node scripts/githubctl/clean-room.mjs
`),
    /skipped during rehearsal/u,
  );
  const hosted = assertCleanRoomHosted(hostedReleaseYml());
  assert.equal(hosted.kind, CLEAN_ROOM_KIND);
  assert.equal(hosted.hosted, true);
  assert.equal(hosted.skipped, false);
});

test("REL3 rehearsal evidence records the hosted clean-room check", () => {
  const identity = rehearsalIdentity(REPO_ROOT);
  assert.equal(identity.workflow, "release-check.yml");
  const fxLedger = writeLedgerFixture("githubctl-rel3-", {
    implemented: ["A"],
    issues: [{ node_id: "B", gh_issue: 10, sync_to_github: true }],
  });
  const adapter = new FakeGitHubAdapter({
    owner: "pikax",
    repo: "verter",
    milestones: [{ title: MILESTONE, number: 1 }],
    issues: [{ number: 10, title: "B", body: "b", milestone: MILESTONE }],
  });
  const report = releasePlan({
    adapter,
    mode: "check",
    milestone: MILESTONE,
    ledgerPath: fxLedger,
    authority: {
      nodes: [
        {
          id: "A",
          name: "A",
          train: "t",
          predecessors: [],
          dispatchable: true,
          conflict_domains: [],
          resource_class: "ts-heavy",
        },
        {
          id: "B",
          name: "B",
          train: "t",
          predecessors: ["A"],
          dispatchable: true,
          conflict_domains: [],
          resource_class: "ts-heavy",
        },
      ],
      ledgerFile: fxLedger,
      ledger: {
        implemented: [{ node_id: "A" }],
        github_issue: [{ node_id: "B", gh_issue: 10, sync_to_github: true }],
        github_train_issue: [],
      },
    },
  });
  assert.equal(report.rehearsal.clean_room.kind, CLEAN_ROOM_KIND);
  assert.equal(report.rehearsal.clean_room.hosted, true);
  assert.equal(report.rehearsal.clean_room.skipped, false);
});

function inside(inner, outer) {
  const resolvedInner = fs.realpathSync(inner);
  const resolvedOuter = fs.realpathSync(outer);
  const prefix = resolvedOuter.endsWith(path.sep) ? resolvedOuter : `${resolvedOuter}${path.sep}`;
  return resolvedInner === resolvedOuter || resolvedInner.startsWith(prefix);
}
