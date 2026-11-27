import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { existsSync, readFileSync, readdirSync, realpathSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  CORPUS_VARIABLE,
  MEMBERS,
  ROWS_VARIABLE,
  Refusal,
  aggregate,
  buildVerterTsc,
  locateCorpus,
  locateRows,
  main,
  measure,
  memberCommands,
  procPeak,
  privatePaths,
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
  // A directory whose name merely starts with two dots is still inside.
  const tree = await corpusWith([]);
  try {
    await mkdir(path.join(tree, "..corpus"));
    assert.throws(
      () => locateCorpus({ [CORPUS_VARIABLE]: path.join(tree, "..corpus") }, tree),
      (error) => error instanceof Refusal && /inside the Verter tree/.test(error.message),
    );
  } finally {
    await rm(tree, { recursive: true, force: true });
  }
  assert.throws(
    () => locateCorpus({ [CORPUS_VARIABLE]: repoRoot }, repoRoot),
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

test("redaction covers a stock checker hoisted above the corpus root", () => {
  const hoisted = path.join(path.sep, "work", "client", "node_modules", ".bin", "vue-tsc");
  const corpusRoot = path.join(path.sep, "work", "client", "apps");
  const message = redact(
    `spawn ${hoisted} EACCES`,
    privatePaths(corpusRoot, corpusRoot, [hoisted]),
  );
  assert.equal(message.includes("client"), false, message);
});

/**
 * A synthetic corpus, a verter-tsc that reports two rows marked with SECRET
 * and a silent stock check, plus the options `main` takes to use them in
 * place of cargo, vue-tsc and /proc.
 */
async function syntheticRun() {
  const root = await corpusWith();
  const tools = await mkdtemp(path.join(os.tmpdir(), "verter-parity-tools-"));
  const verter = path.join(tools, "verter.mjs");
  const stock = path.join(tools, "stock.mjs");
  await writeFile(
    verter,
    [
      `const file = process.cwd() + "/${SECRET}.vue";`,
      `process.stdout.write(file + "(4,2): error TS2322: ${SECRET} one.\\n");`,
      `process.stdout.write(file + "(9,1): error TS2769: ${SECRET} two.\\n");`,
      `process.exitCode = 1;`,
    ].join("\n"),
  );
  await writeFile(stock, "process.exitCode = 0;\n");
  let stdout = "";
  let stderr = "";
  const io = {
    repoRoot,
    verterTsc: [process.execPath, verter],
    stockCheckFor: () => [process.execPath, stock],
    samplePeak: async () => 1024,
    stdout: { write: (text) => (stdout += text) },
    stderr: { write: (text) => (stderr += text) },
  };
  const env = { [CORPUS_VARIABLE]: root, VERTER_TSGO_BIN: "tsgo" };
  const listing = async () =>
    JSON.stringify([root, tools].map((dir) => readdirSync(dir, { recursive: true }).sort()));
  return {
    root,
    tools,
    env,
    io,
    listing,
    output: () => ({ stdout, stderr }),
    cleanup: () =>
      Promise.all([root, tools].map((dir) => rm(dir, { recursive: true, force: true }))),
  };
}

test("the private row option writes each member's full report and prints none of it", async () => {
  const run = await syntheticRun();
  const rows = await mkdtemp(path.join(os.tmpdir(), "verter-parity-rows-"));
  try {
    const code = await main({ ...run.env, [ROWS_VARIABLE]: rows }, [], run.io);
    const { stdout, stderr } = run.output();
    assert.equal(code, 0, stderr);
    for (const member of MEMBERS) {
      const report = JSON.parse(await readFile(path.join(rows, `${member.name}.json`), "utf8"));
      assert.equal(report.mode, "stock");
      assert.deepEqual(
        report.right.rows.map(({ file, line, code, messageHead }) => [
          file,
          line,
          code,
          messageHead,
        ]),
        [
          [`${SECRET}.vue`, 4, "TS2322", `${SECRET} one.`],
          [`${SECRET}.vue`, 9, "TS2769", `${SECRET} two.`],
        ],
      );
    }
    assert.deepEqual(
      readdirSync(rows).sort(),
      MEMBERS.map((member) => `${member.name}.json`).sort(),
    );
    assert.doesNotMatch(stdout + stderr, new RegExp(SECRET));
    assert.equal(JSON.parse(stdout).rows.length, 2 * MEMBERS.length);
  } finally {
    await rm(rows, { recursive: true, force: true });
    await run.cleanup();
  }
});

test("a row directory inside the Verter tree is refused before anything runs", async () => {
  const inside = path.join(repoRoot, "scripts", `${SECRET}-rows`);
  assert.throws(
    () => locateRows({ [ROWS_VARIABLE]: inside }, repoRoot),
    (error) =>
      error instanceof Refusal &&
      /row directory named by VERTER_PARITY_ROWS is inside the Verter tree/.test(error.message),
  );
  assert.equal(locateRows({}, repoRoot), null);
  const run = await syntheticRun();
  try {
    const code = await main({ ...run.env, [ROWS_VARIABLE]: inside }, [], run.io);
    const { stdout, stderr } = run.output();
    assert.equal(code, 2);
    assert.equal(stdout, "");
    assert.match(stderr, /inside the Verter tree/);
    assert.doesNotMatch(stderr, new RegExp(SECRET));
    assert.equal(existsSync(inside), false);
  } finally {
    await run.cleanup();
  }
});

test("without the row option nothing row-level is written or printed", async () => {
  const run = await syntheticRun();
  try {
    const before = await run.listing();
    const code = await main(run.env, [], run.io);
    const { stdout, stderr } = run.output();
    assert.equal(code, 0, stderr);
    assert.doesNotMatch(stdout + stderr, new RegExp(SECRET));
    assert.equal(await run.listing(), before);
    assert.deepEqual(
      JSON.parse(stdout)
        .rows.map(({ code, cause, count }) => [code, cause, count])
        .slice(0, 2),
      [
        ["TS2322", "right-only", 1],
        ["TS2769", "right-only", 1],
      ],
    );
  } finally {
    await run.cleanup();
  }
});

test("publishing prints each member's report on stderr and keeps stdout aggregate", async () => {
  const run = await syntheticRun();
  try {
    const code = await main(run.env, ["--publish-rows"], run.io);
    const { stdout, stderr } = run.output();
    assert.equal(code, 0, stderr);
    assert.doesNotMatch(stdout, new RegExp(SECRET));
    const published = stderr
      .split("\n")
      .filter((line) => line.startsWith("parity-rows "))
      .map((line) => JSON.parse(line.slice("parity-rows ".length)));
    assert.deepEqual(
      published.map(({ member, report }) => [member, report.right.rows.length]),
      MEMBERS.map((member) => [member.name, 2]),
    );
    assert.match(published[0].report.right.rows[0].messageHead, new RegExp(SECRET));
  } finally {
    await run.cleanup();
  }
});

test("a corpus named through a link is measured at the directory it points to", async (t) => {
  const root = await corpusWith();
  const links = await mkdtemp(path.join(os.tmpdir(), "verter-parity-link-"));
  try {
    const link = path.join(links, "current");
    try {
      await symlink(root, link, process.platform === "win32" ? "junction" : "dir");
    } catch (error) {
      if (error.code === "EPERM") return t.skip("symbolic links are not permitted here");
      throw error;
    }
    const located = locateCorpus({ [CORPUS_VARIABLE]: link }, repoRoot);
    assert.equal(located.root, realpathSync.native(root));
    assert.deepEqual(
      located.members.map((member) => member.root),
      MEMBERS.map((member) => path.join(realpathSync.native(root), member.name)),
    );
  } finally {
    await rm(links, { recursive: true, force: true });
    await rm(root, { recursive: true, force: true });
  }
});
