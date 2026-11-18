import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { PassThrough } from "node:stream";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { run, runCommand } from "./run.mjs";
import { normalize } from "./normalize.mjs";

const cli = fileURLToPath(new URL("./run.mjs", import.meta.url));
const a = "src/A.vue(2,4): error TS2322: Wrong value.";
const b = "src/A.vue(3,7): error TS2339: Missing property.";
const command = (output, exit = 1, stream = "stdout") =>
  JSON.stringify([
    process.execPath,
    "-e",
    `process.${stream}.write(${JSON.stringify(output)}); process.exitCode = ${exit};`,
  ]);

function cliInvoke(args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [cli, ...args], { cwd });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.setEncoding("utf8").on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) =>
      resolve({ code, stdout, stderr, report: stdout ? JSON.parse(stdout) : null }),
    );
  });
}

async function invoke(args, cwd) {
  const pathOptions = new Set([
    "--left-file",
    "--right-file",
    "--left-root",
    "--right-root",
    "--accepted",
    "--report",
  ]);
  const resolved = args.map((arg, index) =>
    pathOptions.has(args[index - 1]) && !arg.startsWith("--") ? path.resolve(cwd, arg) : arg,
  );
  try {
    const report = await run(resolved);
    return { code: report.pass ? 0 : 1, stderr: "", report };
  } catch (error) {
    return { code: 2, stderr: error.message, report: null };
  }
}

async function fixture(t) {
  const dir = await mkdtemp(path.join(os.tmpdir(), "diagnostic-parity-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  await writeFile(path.join(dir, "left.txt"), a);
  await writeFile(path.join(dir, "right.txt"), a);
  return dir;
}
const files = ["--left-file", "left.txt", "--right-file", "right.txt"];
const acceptedRow = () => ({
  file: "src/A.vue",
  line: 3,
  code: "TS2339",
  count: 1,
  class: "correction",
  justification: "Missing property is a real error.",
  evidence: "evidence.txt",
  fixedBy: "Declare the property.",
});

test("compares recordings, writes a JSON report and fails on differences", async (t) => {
  const dir = await fixture(t);
  const clean = await invoke([...files, "--report", "report.json"], dir);
  assert.equal(clean.code, 0, clean.stderr);
  assert.deepEqual(JSON.parse(await readFile(path.join(dir, "report.json"), "utf8")), clean.report);
  await writeFile(path.join(dir, "right.txt"), `${a}\n${b}`);
  const delta = await invoke(files, dir);
  assert.equal(delta.code, 1);
  assert.equal(delta.report.differences[0].class, "right-only");
});

test("the CLI emits JSON and a failing exit status for a diagnostic difference", async (t) => {
  const dir = await fixture(t);
  await writeFile(path.join(dir, "right.txt"), `${a}\n${b}`);
  const result = await cliInvoke(files, dir);
  assert.equal(result.code, 1, result.stderr);
  assert.equal(result.report.pass, false);
  assert.equal(result.report.differences[0].class, "right-only");
});

test("a positionless diagnostic is a reported difference, not a failed run", async (t) => {
  const dir = await fixture(t);
  await writeFile(
    path.join(dir, "right.txt"),
    `${a}\nsrc/B.vue: error VTER1001: Failed.\nerror TS18003: No inputs were found.`,
  );
  const result = await cliInvoke([...files, "--mode", "stock"], dir);
  assert.equal(result.code, 1, result.stderr);
  const [unpositioned, global, order] = result.report.differences;
  assert.deepEqual(
    [unpositioned, global].map((d) => [d.class, d.right[0].file, d.right[0].line, d.right[0].code]),
    [
      ["right-only", "src/B.vue", null, "VTER1001"],
      ["right-only", null, null, "TS18003"],
    ],
  );
  assert.deepEqual(order, {
    class: "order",
    left: ["src/A.vue"],
    right: ["src/A.vue", "src/B.vue"],
  });
});

test("replays recordings from Windows roots independently of the current host", async (t) => {
  const dir = await fixture(t);
  const leftFile = path.join(dir, "left.txt");
  const rightFile = path.join(dir, "right.txt");
  await writeFile(leftFile, `C:\\recorded\\left\\${a.replaceAll("/", "\\")}`);
  await writeFile(rightFile, `D:/recorded/right/${a}`);
  const report = await run([
    "--left-file",
    leftFile,
    "--right-file",
    rightFile,
    "--left-root",
    "C:\\recorded\\left",
    "--right-root",
    "D:/recorded/right",
  ]);
  assert.equal(report.pass, true);
  assert.equal(report.left.rows[0].file, "src/A.vue");
});

test("runs argv commands in their roots and compares stderr diagnostics and exit codes", async (t) => {
  const dir = await fixture(t);
  const result = await invoke(
    [
      "--left-command",
      command(a),
      "--right-command",
      command(a, 2, "stderr"),
      "--left-root",
      dir,
      "--right-root",
      dir,
    ],
    dir,
  );
  assert.equal(result.code, 1);
  assert.equal(result.report.gates.set, true);
  assert.equal(result.report.gates.exit, false);
});

test("runs a command in its root opposite a recording", async (t) => {
  const dir = await fixture(t);
  const rooted = JSON.stringify([
    process.execPath,
    "-e",
    "process.stdout.write(process.cwd() + '/src/A.vue(2,4): error TS2322: Wrong value.');",
  ]);
  assert.equal(
    (await invoke(["--left-command", rooted, "--right-file", "right.txt", "--left-root", dir], dir))
      .code,
    0,
  );
});

for (const [name, argv] of [
  [
    "a command cannot start",
    [
      "--left-command",
      JSON.stringify(["missing-diagnostic-parity-program"]),
      "--right-file",
      "right.txt",
    ],
  ],
  [
    "the left command fails without diagnostics",
    ["--left-command", command("bad config", 2), "--right-file", "right.txt"],
  ],
  [
    "the right command fails without diagnostics",
    ["--left-file", "left.txt", "--right-command", command("bad config", 2)],
  ],
  [
    "a command terminates by signal",
    [
      "--left-command",
      JSON.stringify([process.execPath, "-e", "process.kill(process.pid, 'SIGTERM')"]),
      "--right-file",
      "right.txt",
    ],
  ],
  [
    "a change run has no seeded diagnostics",
    ["--mode", "change", "--left-command", command("", 0), "--right-file", "right.txt"],
  ],
]) {
  test(`fails closed when ${name}`, async (t) => {
    const dir = await fixture(t);
    const result = await invoke(argv, dir);
    assert.equal(result.code, 2, result.stdout);
    assert.ok(result.stderr.trim());
  });
}

test("two clean commands may both produce no diagnostics", async (t) => {
  const dir = await fixture(t);
  assert.equal(
    (await invoke(["--left-command", command("", 0), "--right-command", command("", 0)], dir)).code,
    0,
  );
});

test("preserves observed diagnostic order across stdout and stderr, including partial lines", async () => {
  const second = "src/B.vue(3,7): error TS2339: Missing property.";
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  const resultPromise = runCommand(["synthetic-checker"], "/repo", () => child);
  child.stdout.write(a.slice(0, 15));
  child.stderr.write(`${second}\n`);
  child.stdout.write(`${a.slice(15)}\n`);
  child.stderr.write("src/C.vue(1,1): error TS2304: Missing name.");
  child.stdout.end();
  child.stderr.end();
  child.emit("close", 1, null);
  const result = await resultPromise;
  assert.deepEqual(
    normalize(result.output, "/repo").map((row) => row.file),
    ["src/B.vue", "src/A.vue", "src/C.vue"],
  );
});

test("stock accepts only exact evidenced right-only counts, retaining their reported differences", async (t) => {
  const dir = await fixture(t);
  await writeFile(path.join(dir, "right.txt"), `${a}\n${b}`);
  await writeFile(path.join(dir, "evidence.txt"), b);
  for (const classification of ["correction", "engine"]) {
    await writeFile(
      path.join(dir, "accepted.json"),
      JSON.stringify([{ ...acceptedRow(), class: classification }]),
    );
    const result = await invoke([...files, "--mode", "stock", "--accepted", "accepted.json"], dir);
    assert.equal(result.code, 0, result.stderr);
    assert.equal(result.report.accepted.length, 1);
    assert.equal(result.report.differences[0].class, "right-only");
  }
  await writeFile(path.join(dir, "right.txt"), `${a}\n${b}\n${b}`);
  assert.equal(
    (await invoke([...files, "--mode", "stock", "--accepted", "accepted.json"], dir)).code,
    2,
  );
});

test("rejects malformed, duplicate, stale, left-only and unsupported accepted rows", async (t) => {
  const dir = await fixture(t);
  await writeFile(path.join(dir, "evidence.txt"), b);
  await writeFile(path.join(dir, "right.txt"), `${a}\n${b}`);
  const row = acceptedRow();
  const invalid = [
    null,
    {},
    [row, row],
    [{ ...row, class: "left-only" }],
    [{ ...row, class: "defect" }],
    [{ ...row, count: 0 }],
    [{ ...row, line: 1.5 }],
    [{ ...row, file: "src/*" }],
    [{ ...row, evidence: "missing.txt" }],
    [{ ...row, code: "TS233" }],
    [{ ...row, line: 99 }],
  ];
  for (const field of Object.keys(row)) {
    const missing = { ...row };
    delete missing[field];
    invalid.push([missing], [{ ...row, [field]: "" }]);
  }
  for (const value of invalid) {
    await writeFile(path.join(dir, "accepted.json"), JSON.stringify(value));
    const result = await invoke([...files, "--mode", "stock", "--accepted", "accepted.json"], dir);
    assert.equal(result.code, 2, JSON.stringify(value));
  }
  await writeFile(path.join(dir, "accepted.json"), JSON.stringify([row]));
  await writeFile(path.join(dir, "evidence.txt"), "TS23390 is a different code");
  assert.equal(
    (await invoke([...files, "--mode", "stock", "--accepted", "accepted.json"], dir)).code,
    2,
  );
  await writeFile(path.join(dir, "evidence.txt"), b);
  await writeFile(path.join(dir, "left.txt"), `${a}\n${b}`);
  await writeFile(path.join(dir, "right.txt"), a);
  assert.equal(
    (await invoke([...files, "--mode", "stock", "--accepted", "accepted.json"], dir)).code,
    2,
  );
});

test("validates CLI inputs and allows a recording opposite a command", async (t) => {
  const dir = await fixture(t);
  for (const argv of [
    [],
    [...files, "--mode", "unknown"],
    [...files, "--wat"],
    [...files, "--left-file", "again.txt"],
    [...files, "--left-command", command(a)],
    ["--left-command", "[]", "--right-file", "right.txt"],
    [...files, "--accepted", "accepted.json"],
    [...files, "--report"],
  ]) {
    assert.equal((await invoke(argv, dir)).code, 2);
  }
  assert.equal(
    (await invoke(["--left-file", "left.txt", "--right-command", command(a)], dir)).code,
    0,
  );
});
