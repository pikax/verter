#!/usr/bin/env node
// The tsc arm of the semantic benchmark's session workloads: runs one
// session script through TypeScript 7.0.2's native API in ONE live server
// and writes one JSON record.
//
//   node tsc-session-probe.mjs --job <session.json> --out <result.json>
//
// The job is the SAME session job the Verter probe reads (directory,
// tsconfig, script) plus the verified TypeScript package directory and
// native tsc executable, and the Verter probe executable that reads the
// server's OS statistics. It uses the asynchronous API client
// (`typescript/unstable/async`, the server's `--async` mode), so a
// concurrent demand step issues every request before awaiting any.
//
// tsc's own incremental facility serves the edits: an edit writes the file
// in the invocation's project copy and calls `updateSnapshot` with
// `fileChanges.changed` naming it, so the server derives the next snapshot
// from the previous one (program reuse) instead of opening the project
// again. The record keeps the change notification and the snapshot ids as
// the evidence.
//
// Times: a sequential request's time is the server's own processing time
// (`collectTiming`); a concurrent step's is the client's round trip for the
// whole step (requests in flight together have no separable server time).
// An edit's time is the server's `updateSnapshot` time. Each step's answers
// are printed right after it, outside every timer (as the Verter probe
// observes them). A `meta` step (Vue component metadata) has no tsc
// counterpart and is recorded as not applicable.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { INCREMENTAL_FACILITY } from "./sessions.mjs";

const RESULT_SCHEMA = 1;
// TypeFormatFlags.NoTruncation | TypeFormatFlags.InTypeAlias.
const PRINT_FLAGS = 1 | (1 << 23);

function flag(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

function aliasNamePosition(text, alias) {
  const marker = `type ${alias} `;
  const at = text.indexOf(marker);
  if (at < 0 || text.indexOf(marker, at + 1) >= 0)
    throw new Error(`the module must declare ${alias} exactly once`);
  return at + "type ".length;
}

async function main() {
  const jobPath = flag("--job");
  const outPath = flag("--out");
  if (!jobPath || !outPath) {
    console.error("usage: tsc-session-probe.mjs --job <session.json> --out <result.json>");
    process.exit(2);
  }
  const replace = (path, text) => {
    writeFileSync(`${path}.tmp`, text);
    renameSync(`${path}.tmp`, path);
  };
  const history = [];
  const phase = (name) => {
    const atMs = Date.now();
    history.push({ phase: name, atMs });
    replace(`${outPath}.phase`, JSON.stringify({ phase: name, atMs, history }));
  };
  const job = JSON.parse(readFileSync(jobPath, "utf8"));
  if (job.schema !== 1) throw new Error(`session job schema ${job.schema} is not 1`);
  if (!job.steps?.length) throw new Error("a session runs at least one step");
  const { API } = await import(
    pathToFileURL(join(job.tsPackageDir, "dist", "api", "async", "api.js")).href
  );
  const dir = job.dir.replace(/\\/g, "/");
  const path = (file) => `${dir}/${file}`;
  const configPath = path(job.tsconfig);
  // The module texts as the server sees them (an edit replaces one).
  const texts = new Map();
  const textOf = (file) => {
    if (!texts.has(file)) texts.set(file, readFileSync(path(file), "utf8"));
    return texts.get(file);
  };

  phase("spawn");
  let t = performance.now();
  const api = new API({ cwd: dir, collectTiming: true, tsserverPath: job.tscExe });
  const spawnMs = performance.now() - t;
  const serverTime = async (method) =>
    (await api.getTimingInfo()).recentRequests.filter((r) => r.method === method).at(-1)
      ?.serverTimeMs ?? null;

  phase("setup");
  t = performance.now();
  let snapshot = await api.updateSnapshot({ openProjects: [configPath] });
  const setupRoundTripMs = performance.now() - t;
  const setupMs = await serverTime("updateSnapshot");
  const serverPid = api.client?.process?.pid;
  if (typeof serverPid !== "number") throw new Error("the tsc API server pid is not reachable");
  let project = snapshot.getProject(configPath) ?? snapshot.getProjects()[0];
  if (snapshot.getProjects().length !== 1)
    throw new Error(`expected one project, got ${snapshot.getProjects().length}`);
  const rootFiles = project.rootFiles.map((f) => f.replace(/\\/g, "/"));

  const typeAt = async (file, alias) => {
    const position = aliasNamePosition(textOf(file), alias);
    try {
      const type = await project.checker.getTypeAtPosition(path(file), position);
      return { type, outcome: type ? { kind: "value" } : { kind: "miss" } };
    } catch (err) {
      return { type: undefined, outcome: { kind: "fault", detail: String(err?.message ?? err) } };
    }
  };
  const print = async (type) => {
    if (!type)
      return { text: null, error: "no value to observe", errorType: null, unionMembers: null };
    try {
      // A union is printed member by member (tsc's printer elides very large
      // types), each member parenthesised, as the probe arm prints.
      const members = type.isUnionType() ? await type.getTypes() : null;
      const text = members
        ? (
            await Promise.all(
              members.map(
                async (m) => `(${await project.checker.typeToString(m, undefined, PRINT_FLAGS)})`,
              ),
            )
          ).join(" | ")
        : await project.checker.typeToString(type, undefined, PRINT_FLAGS);
      return {
        text,
        error: null,
        errorType: type.isErrorType(),
        unionMembers: members ? members.length : null,
      };
    } catch (err) {
      return {
        text: null,
        error: String(err?.message ?? err),
        errorType: null,
        unionMembers: null,
      };
    }
  };

  phase("init");
  const initStart = performance.now();
  const init = await typeAt(job.initFile, job.initAlias);
  const initRoundTripMs = performance.now() - initStart;
  const initMs = await serverTime("getTypeAtPosition");

  const steps = [];
  for (const [index, step] of job.steps.entries()) {
    phase(`step-${index}`);
    if (step.kind === "meta") {
      steps.push({ kind: "meta", file: step.file, applicable: false });
    } else if (step.kind === "edit") {
      writeFileSync(path(step.file), step.text);
      texts.set(step.file, step.text);
      const fileChanges = { changed: [path(step.file)] };
      const superseded = snapshot;
      const start = performance.now();
      snapshot = await api.updateSnapshot({ fileChanges });
      const roundTripMs = performance.now() - start;
      // An undisposed snapshot stays active in the server and would inflate the
      // memory reading taken after the steps.
      await superseded.dispose();
      project = snapshot.getProject(configPath) ?? snapshot.getProjects()[0];
      steps.push({
        kind: "edit",
        file: step.file,
        textSha256: createHash("sha256").update(step.text).digest("hex"),
        serverMs: await serverTime("updateSnapshot"),
        roundTripMs,
        snapshot: snapshot.id,
        fileChanges,
      });
    } else if (step.kind === "demand") {
      const records = [];
      let wallMs;
      if (step.concurrent && step.requests.length > 1) {
        const start = performance.now();
        const answered = await Promise.all(step.requests.map((r) => typeAt(r.file, r.alias)));
        wallMs = performance.now() - start;
        for (const [i, r] of step.requests.entries())
          records.push({
            file: r.file,
            alias: r.alias,
            serverMs: null,
            outcome: answered[i].outcome,
            type: answered[i].type,
          });
      } else {
        wallMs = 0;
        for (const r of step.requests) {
          const answered = await typeAt(r.file, r.alias);
          const serverMs = await serverTime("getTypeAtPosition");
          wallMs += serverMs ?? NaN;
          records.push({
            file: r.file,
            alias: r.alias,
            serverMs,
            outcome: answered.outcome,
            type: answered.type,
          });
        }
      }
      // Observed after the step, outside its timer.
      for (const record of records) {
        record.observation = await print(record.type);
        delete record.type;
      }
      steps.push({
        kind: "demand",
        concurrent: Boolean(step.concurrent && step.requests.length > 1),
        threads: step.concurrent ? step.requests.length : 1,
        basis: step.concurrent && step.requests.length > 1 ? "round-trip" : "server",
        wallMs,
        requests: records,
      });
    } else throw new Error(`unknown step kind ${step.kind}`);
  }

  phase("stats");
  const statsErrors = [];
  const r = spawnSync(job.statsExe, ["stats", "--pid", String(serverPid)], {
    encoding: "utf8",
    timeout: 30_000,
  });
  let serverAfterSteps = null;
  if (r.status !== 0)
    statsErrors.push(
      `after steps: stats --pid ${serverPid}: exit ${r.status} ${r.stderr?.trim() ?? ""}`,
    );
  else serverAfterSteps = JSON.parse(r.stdout);
  const result = {
    schema: RESULT_SCHEMA,
    tool: "tsc",
    kind: "session",
    stage: "complete",
    incremental: INCREMENTAL_FACILITY,
    tscExe: job.tscExe,
    statsExe: job.statsExe,
    serverPid,
    rootFiles,
    phases: { spawnMs, setupMs, setupRoundTripMs, initMs, initRoundTripMs },
    initOutcome: init.outcome,
    steps,
    serverAfterSteps,
    statsErrors,
  };

  phase("teardown");
  await api.close();
  replace(outPath, JSON.stringify(result, null, 2));
  phase("done");
}

main().catch((err) => {
  console.error(err?.stack ?? String(err));
  process.exit(3);
});
