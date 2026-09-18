import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import { GitHubAdapterError, rehearsalIdentity, releasePlan } from "../index.mjs";

import {
  CLEAN_ROOM_KIND,
  assertCleanRoomHosted,
  declaredEntrypoints,
  inspectConsumer,
  runCleanRoomCheck,
} from "../clean-room.mjs";
import { FakeGitHubAdapter } from "../fake.mjs";
import { writeLedgerFixture } from "./ledger-fixture.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
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

test("REL3-AC2 derives the published set from computePublishSet and packs it", () => {
  const root = tmp("rel3-derive-");
  writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
      dependencies: { "@rel3/beta": "1.0.0" },
    },
    { "index.js": 'export const alpha = "alpha";\n' },
  );
  writePkg(
    root,
    "beta",
    {
      name: "@rel3/beta",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": 'export const beta = "beta";\n' },
  );
  const report = runCleanRoomCheck({
    repoRoot: root,
    workDir: tmp("rel3-run-"),
    roots: ["@rel3/alpha"],
  });
  assert.equal(report.ok, true, report.message ?? JSON.stringify(report.failures, null, 2));
  const names = new Set(report.entrypoints.map((row) => row.package));
  assert.equal(names.has("@rel3/alpha"), true);
  assert.equal(names.has("@rel3/beta"), true);
});

test("REL3-AC2 a package added to the publication config is covered or fails", () => {
  const root = tmp("rel3-added-");
  writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
      dependencies: { "@rel3/gamma": "1.0.0" },
    },
    { "index.js": 'export const alpha = "alpha";\n' },
  );
  writePkg(
    root,
    "gamma",
    {
      name: "@rel3/gamma",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./missing.js" } },
      files: ["index.js"],
    },
    { "index.js": 'export const gamma = "gamma";\n' },
  );
  const report = runCleanRoomCheck({
    repoRoot: root,
    workDir: tmp("rel3-run-"),
    roots: ["@rel3/alpha"],
  });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "missing-entrypoint-file");
  assert.match(report.message, /@rel3\/gamma/u);
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
  assert.throws(
    () =>
      assertCleanRoomHosted(`name: Release
jobs:
  clean-room:
    if: inputs.dry_run
    steps:
      - run: node scripts/githubctl/clean-room.mjs
`),
    /skipped during rehearsal/u,
  );
  assert.throws(
    () =>
      assertCleanRoomHosted(`name: Release
jobs:
  clean-room:
    runs-on: ubuntu-latest
    steps:
      - run: node scripts/githubctl/clean-room.mjs
        continue-on-error: true
`),
    /skipped during rehearsal/u,
  );
  assert.throws(
    () =>
      assertCleanRoomHosted(`name: Release
jobs:
  clean-room:
    runs-on: ubuntu-latest
    steps:
      # node scripts/githubctl/clean-room.mjs
      - run: echo skip
`),
    GitHubAdapterError,
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
  assert.equal(identity.clean_room.hosted, true);
  assert.equal(identity.clean_room.skipped, false);
});

test("REL3-AC1 a packaged module that imports the source tree fails the check", () => {
  const repoRoot = tmp("rel3-reach-");
  const secret = path.join(repoRoot, "secret.js");
  fs.writeFileSync(secret, 'export function cleanRoomPing() { return "secret"; }\n');
  const href = pathToFileURL(secret).href;
  const alpha = writePkg(
    repoRoot,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    {
      "index.js": `import { cleanRoomPing } from ${JSON.stringify(href)};\nexport { cleanRoomPing };\n`,
    },
  );
  const report = check([unitFrom(alpha)], { repoRoot });
  assert.equal(report.ok, false, JSON.stringify(report, null, 2));
  assert.equal(report.reason, "source-tree-fallback");
});

test("REL3-AC1 consumer-side patch and workspace protocol are rejected", () => {
  const consumer = tmp("rel3-patch-");
  fs.writeFileSync(
    path.join(consumer, "package.json"),
    `${JSON.stringify(
      {
        name: "consumer",
        pnpm: { patchedDependencies: { "left-pad@1.0.0": "patches/left-pad.patch" } },
      },
      null,
      2,
    )}\n`,
  );
  const patched = inspectConsumer(consumer, tmp("rel3-repo-"));
  assert.equal(patched?.reason, "consumer-patch");

  const ws = tmp("rel3-wscons-");
  fs.writeFileSync(
    path.join(ws, "package.json"),
    `${JSON.stringify({ name: "consumer", dependencies: { helper: "workspace:*" } }, null, 2)}\n`,
  );
  const resolved = inspectConsumer(ws, tmp("rel3-repo-"));
  assert.equal(resolved?.reason, "workspace-resolved");

  const root = tmp("rel3-wspkg-");
  const dir = writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
      dependencies: { helper: "workspace:*" },
    },
    { "index.js": "export const alpha = 1;\n" },
  );
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "workspace-resolved");
});

test("REL3-AC2 TypeScript import targets are types-only", () => {
  const rows = declaredEntrypoints({
    name: "@rel3/types",
    exports: {
      ".": { types: "./dist/index.d.ts", import: "./dist/index.js" },
      "./gen": { import: "./gen.ts", default: "./gen.ts" },
    },
  });
  const gen = rows.filter((row) => row.subpath === "./gen");
  assert.equal(gen.length > 0, true);
  assert.equal(
    gen.every((row) => row.typesOnly),
    true,
  );
  const root = tmp("rel3-ts-");
  const dir = writePkg(
    root,
    "types",
    {
      name: "@rel3/typespkg",
      version: "1.0.0",
      type: "module",
      files: ["index.js", "gen.ts"],
      exports: {
        ".": { import: "./index.js" },
        "./gen": { import: "./gen.ts", default: "./gen.ts" },
      },
    },
    {
      "index.js": "export const value = 1;\n",
      "gen.ts": "export const generated = 1;\n",
    },
  );
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, true, report.message ?? JSON.stringify(report.failures, null, 2));
  const keys = report.entrypoints.map((row) => `${row.entrypoint} ${row.condition}`).sort();
  assert.deepEqual(keys, [". import", "./gen types"]);
});

test("REL3-AC2 packed manifest is the source of entrypoints, not the workspace package.json", () => {
  const root = tmp("rel3-packed-manifest-");
  const dir = writePkg(
    root,
    "alpha",
    {
      name: "@rel3/alpha",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": "export const alpha = 1;\n" },
  );
  const tarball = pack(dir, tmp("rel3-pack-"));
  const pkgPath = path.join(dir, "package.json");
  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
  pkg.exports["./ghost"] = { import: "./ghost.js" };
  fs.writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  fs.writeFileSync(path.join(dir, "ghost.js"), "export const ghost = 1;\n");
  const report = runCleanRoomCheck({
    repoRoot: root,
    workDir: tmp("rel3-run-"),
    units: [unitFrom(dir)],
    tarballs: [{ name: "@rel3/alpha", tarball }],
  });
  assert.equal(report.ok, true, report.message ?? JSON.stringify(report.failures, null, 2));
  assert.equal(
    report.entrypoints.some((row) => row.entrypoint === "./ghost"),
    false,
  );
});

test("REL3-AC2 libc-mismatched platform units are packed, not installed", () => {
  const root = tmp("rel3-libc-");
  let hostLibc = null;
  if (process.platform === "linux") {
    try {
      const header = process.report?.getReport()?.header;
      if (header?.glibcVersionRuntime) hostLibc = "glibc";
    } catch {
      hostLibc = null;
    }
  }
  const mismatchedLibc = hostLibc === "musl" ? "glibc" : "musl";
  const musl = writePkg(
    root,
    "musl",
    {
      name: "@rel3/native-musl",
      version: "1.0.0",
      os: [process.platform],
      cpu: [process.arch],
      libc: [mismatchedLibc],
      main: "addon.node",
      files: ["addon.node"],
    },
    { "addon.node": "not-a-real-addon" },
  );
  const js = writePkg(
    root,
    "js",
    {
      name: "@rel3/js",
      version: "1.0.0",
      type: "module",
      exports: { ".": { import: "./index.js" } },
    },
    { "index.js": "export const js = 1;\n" },
  );
  const report = check([{ ...unitFrom(musl), kind: "platform" }, unitFrom(js)], { repoRoot: root });
  assert.equal(report.ok, true, report.message ?? JSON.stringify(report.failures, null, 2));
  const muslRows = report.entrypoints.filter((row) => row.package === "@rel3/native-musl");
  assert.equal(muslRows.length > 0, true);
  assert.equal(
    muslRows.every((row) => row.condition === "packed"),
    true,
  );
});

test("REL3-AC2 binary-only platform packages are covered by packed payload", () => {
  const root = tmp("rel3-binonly-");
  const binName = process.platform === "win32" ? "tool.cmd" : "tool";
  const binBody =
    process.platform === "win32" ? "@echo off\r\necho ok\r\n" : "#!/bin/sh\necho ok\n";
  const dir = writePkg(
    root,
    "tool",
    {
      name: "@rel3/tool-host",
      version: "1.0.0",
      os: [process.platform],
      cpu: [process.arch],
      files: [binName],
    },
    { [binName]: binBody },
  );
  if (process.platform !== "win32") {
    fs.chmodSync(path.join(dir, binName), 0o755);
  }
  const report = check([{ ...unitFrom(dir), kind: "platform" }], { repoRoot: root });
  assert.equal(report.ok, true, report.message ?? JSON.stringify(report.failures, null, 2));
  assert.equal(
    report.entrypoints.some((row) => row.package === "@rel3/tool-host"),
    true,
  );
  assert.equal(report.reason, undefined);
});

test("REL3-AC4 bin with a missing require fails as unloadable", () => {
  const root = tmp("rel3-binmiss-");
  const dir = writePkg(
    root,
    "cli",
    {
      name: "@rel3/cli",
      version: "1.0.0",
      bin: { tool: "./bin.js" },
      files: ["bin.js"],
    },
    { "bin.js": 'require("this-package-does-not-exist-rel3");\n' },
  );
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, false);
  assert.equal(report.reason, "unloadable");
});

test("REL3 wildcard exports emit a structured report instead of throwing", () => {
  const root = tmp("rel3-wild-");
  const dir = writePkg(
    root,
    "wild",
    {
      name: "@rel3/wild",
      version: "1.0.0",
      type: "module",
      exports: { "./*": "./files/*.js" },
    },
    { "files/a.js": "export const a = 1;\n" },
  );
  const report = check([unitFrom(dir)], { repoRoot: root });
  assert.equal(report.ok, false);
  assert.equal(report.kind, CLEAN_ROOM_KIND);
  assert.equal(report.reason, "unloadable");
  assert.match(report.message, /wildcard/u);
});

function inside(inner, outer) {
  const resolvedInner = fs.realpathSync(inner);
  const resolvedOuter = fs.realpathSync(outer);
  const prefix = resolvedOuter.endsWith(path.sep) ? resolvedOuter : `${resolvedOuter}${path.sep}`;
  return resolvedInner === resolvedOuter || resolvedInner.startsWith(prefix);
}
