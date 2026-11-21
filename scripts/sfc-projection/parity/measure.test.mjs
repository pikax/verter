import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  CORPUS_VARIABLE,
  MEMBERS,
  Refusal,
  aggregate,
  buildVerterTsc,
  locateCorpus,
  measure,
  memberCommands,
  procPeak,
  redact,
} from "./measure.mjs";

const cli = fileURLToPath(new URL("./measure.mjs", import.meta.url));
const repoRoot = path.resolve(path.dirname(cli), "../../..");
// Every diagnostic the fake checker prints carries this marker in its file
// name and its message; the report must contain neither.
const SECRET = "private-corpus-detail";

async function corpusWith(members = MEMBERS.map((member) => member.name)) {
  const root = await mkdtemp(path.join(os.tmpdir(), "verter-parity-corpus-"));
  for (const name of members) {
    await mkdir(path.join(root, name));
    await writeFile(path.join(root, name, "tsconfig.json"), "{}\n");
  }
  return root;
}

function cliInvoke(env) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [cli], { env });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => (stdout += chunk));
    child.stderr.setEncoding("utf8").on("data", (chunk) => (stderr += chunk));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

const withoutCorpus = () => {
  const env = { ...process.env };
  delete env[CORPUS_VARIABLE];
  return env;
};

test("the measurement refuses to run without a corpus and says so", async () => {
  const result = await cliInvoke(withoutCorpus());
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, new RegExp(`refusing to run: ${CORPUS_VARIABLE} is not set`));
});

test("the measurement refuses an absent corpus without printing its path", async () => {
  const absent = path.join(os.tmpdir(), `${SECRET}-absent`);
  const result = await cliInvoke({ ...withoutCorpus(), [CORPUS_VARIABLE]: absent });
  assert.equal(result.code, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /refusing to run: the corpus directory .* is absent/);
  assert.doesNotMatch(result.stderr, new RegExp(SECRET));
});

test("a corpus inside the Verter tree or without a member is refused", async () => {
  assert.throws(
    () => locateCorpus({ [CORPUS_VARIABLE]: path.join(repoRoot, "scripts") }, repoRoot),
    (error) => error instanceof Refusal && /inside the Verter tree/.test(error.message),
  );
  const root = await corpusWith(["web"]);
  try {
    assert.throws(
      () => locateCorpus({ [CORPUS_VARIABLE]: root }, repoRoot),
      (error) =>
        error instanceof Refusal &&
        /no web-admin member/.test(error.message) &&
        !error.message.includes(root),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("each member is checked the way it is built, and only verter-tsc takes the route", () => {
  const options = { verterTsc: ["verter-tsc"], stockCheck: ["vue-tsc"], engine: "tsgo" };
  assert.deepEqual(memberCommands({ name: "web", mode: "-b" }, options), {
    stock: ["vue-tsc", "-b", "tsconfig.json", "--pretty", "false"],
    verter: ["verter-tsc", "-b", "tsconfig.json", "--projection-route", "--tsgo-bin", "tsgo"],
  });
  assert.deepEqual(memberCommands({ name: "web-admin", mode: "-p" }, options), {
    stock: ["vue-tsc", "-p", "tsconfig.json", "--noEmit", "--pretty", "false"],
    verter: [
      "verter-tsc",
      "-p",
      "tsconfig.json",
      "--noEmit",
      "--projection-route",
      "--tsgo-bin",
      "tsgo",
    ],
  });
});

test("aggregation counts per code and cause and drops file order and rows", () => {
  const row = (code, file = `${SECRET}.vue`) => ({
    file,
    line: 1,
    col: 1,
    code,
    messageHead: SECRET,
  });
  const rows = aggregate("web", {
    differences: [
      { class: "right-only", count: 2, left: [], right: [row("TS2322"), row("TS2322")] },
      { class: "right-only", count: 1, left: [], right: [row("TS2322", "other.vue")] },
      { class: "right-only", count: 1, left: [], right: [row("VTER1009")] },
      { class: "left-only", count: 1, left: [row("TS2322")], right: [] },
      { class: "order", left: [`${SECRET}.vue`], right: [] },
    ],
  });
  assert.deepEqual(rows, [
    { member: "web", code: "TS2322", cause: "left-only", count: 1 },
    { member: "web", code: "TS2322", cause: "right-only", count: 3 },
    { member: "web", code: "VTER1009", cause: "right-only", count: 1 },
  ]);
});

test("the report holds aggregate rows and verter-tsc's own wall time and peak", async () => {
  const root = await corpusWith();
  const scripts = await mkdtemp(path.join(os.tmpdir(), "verter-parity-tools-"));
  try {
    const verter = path.join(scripts, "verter.mjs");
    const stock = path.join(scripts, "stock.mjs");
    await writeFile(
      verter,
      [
        `const file = process.cwd() + "/${SECRET}.vue";`,
        `const lines = [`,
        `  file + "(1,1): error TS2322: ${SECRET} one.",`,
        `  file + "(2,1): error TS2322: ${SECRET} two.",`,
        `  file + "(3,1): warning VTER1009: ${SECRET} three.",`,
        `];`,
        `process.stdout.write(lines.join("\\n") + "\\n");`,
        `process.exitCode = 1;`,
      ].join("\n"),
    );
    await writeFile(stock, "process.exitCode = 0;\n");
    const sampled = [];
    const report = await measure({
      members: locateCorpus({ [CORPUS_VARIABLE]: root }, repoRoot).members,
      verterTsc: [process.execPath, verter],
      engine: "tsgo",
      stockCheckFor: () => [process.execPath, stock],
      samplePeak: async (pid) => {
        sampled.push(pid);
        return 64 * 1024 * 1024;
      },
    });
    const text = JSON.stringify(report);
    assert.doesNotMatch(text, new RegExp(SECRET));
    assert.equal(text.includes(root), false);
    assert.deepEqual(
      report.members.map(({ member, stock, verter }) => [
        member,
        stock.rows,
        stock.exit,
        verter.rows,
        verter.exit,
        verter.peakBytes,
        Number.isFinite(verter.wallMs) && Number.isFinite(stock.wallMs),
      ]),
      MEMBERS.map((member) => [member.name, 0, 0, 3, 1, 64 * 1024 * 1024, true]),
    );
    assert.deepEqual(
      report.rows,
      MEMBERS.flatMap((member) => [
        { member: member.name, code: "TS2322", cause: "right-only", count: 2 },
        { member: member.name, code: "VTER1009", cause: "right-only", count: 1 },
      ]),
    );
    // Only the verter-tsc processes are sampled, one per member.
    assert.equal(new Set(sampled).size, MEMBERS.length);
  } finally {
    await rm(root, { recursive: true, force: true });
    await rm(scripts, { recursive: true, force: true });
  }
});

test("a measurement without an observed peak fails instead of reporting none", async () => {
  const root = await corpusWith();
  try {
    await assert.rejects(
      measure({
        members: locateCorpus({ [CORPUS_VARIABLE]: root }, repoRoot).members,
        verterTsc: [process.execPath, "-e", "process.exitCode = 0", "--"],
        engine: "tsgo",
        stockCheckFor: () => [process.execPath, "-e", "process.exitCode = 0", "--"],
        samplePeak: async () => null,
      }),
      /peak memory was not observed for the web member/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

function fakeCargo(lines, exit) {
  const calls = [];
  const spawnCommand = (command, args, options) => {
    calls.push({ command, args, options });
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    queueMicrotask(() => {
      child.stdout.end(lines.join("\n"));
      child.stdout.on("end", () => child.emit("close", exit, null));
      child.stdout.resume();
    });
    return child;
  };
  return { calls, spawnCommand };
}

test("the release build with the route feature yields cargo's verter-tsc executable", async () => {
  const artifact = (name, executable) =>
    JSON.stringify({ reason: "compiler-artifact", target: { name }, executable });
  const { calls, spawnCommand } = fakeCargo(
    [artifact("verter_session", null), "not json", artifact("verter-tsc", "/built/verter-tsc")],
    0,
  );
  assert.equal(await buildVerterTsc("/repo", spawnCommand), "/built/verter-tsc");
  assert.equal(calls[0].command, "cargo");
  assert.deepEqual(calls[0].args.slice(0, 7), [
    "build",
    "--release",
    "--locked",
    "-p",
    "verter_tsc",
    "--features",
    "projection-route-qualification",
  ]);
  await assert.rejects(buildVerterTsc("/repo", fakeCargo([], 101).spawnCommand), /exited 101/);
  await assert.rejects(buildVerterTsc("/repo", fakeCargo([], 0).spawnCommand), /no verter-tsc/);
});

test(
  "the peak is read only from the named process image",
  { skip: process.platform !== "linux" },
  async () => {
    const name = /^Name:\s*(.*)$/m.exec(readFileSync("/proc/self/status", "utf8"))[1];
    assert.ok((await procPeak(process.pid, name)) > 0);
    assert.equal(await procPeak(process.pid, `other-${name}`), null);
  },
);

test("redaction replaces the longest private path first", () => {
  assert.equal(
    redact("spawn /c/web/node_modules/.bin/vue-tsc ENOENT in /c", ["/c", "/c/web"]),
    "spawn <corpus>/node_modules/.bin/vue-tsc ENOENT in <corpus>",
  );
});
