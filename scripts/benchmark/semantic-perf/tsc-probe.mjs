#!/usr/bin/env node
// The tsc arm of the equivalent-demand semantic benchmark: answers one job
// through TypeScript 7.0.2's native API (the `typescript/unstable/sync`
// client driving the native `tsc --api` server) and writes one JSON record.
//
//   node tsc-probe.mjs --job <job.json> --out <result.json>
//
// The job is the SAME job the Verter probe reads (same directory, files,
// aliases, warm repetitions) plus the verified TypeScript package directory,
// the verified native tsc executable (passed to the API explicitly), and the
// Verter probe executable used to read the tsc server's OS statistics (so
// both tools' memory comes from the same reader).
//
// Phases, timed separately and never overlapping, in this order (the same
// order as the Verter probe):
//   spawn        start the native server and connect (client-side time,
//                process start included; reported, never compared);
//   engineStart  the API session's `initialize` request (server time): an
//                empty engine, ready;
//   setup        open the project (`updateSnapshot`, server time);
//   init         getTypeAtPosition on `__BenchInit`'s name (server time);
//   cold         getTypeAtPosition on `__Probe`'s name — the declared type of
//                the alias, exactly what the Verter arm resolves;
//   warm         the same request repeated;
//   stats        the server process's OS statistics, read with the server
//                alive and before anything is observed;
//   observe      typeToString (a union member by member: tsc's printer
//                elides very large types) and the error-type flag, outside
//                every timer; then the server's statistics again
//                (observation-inclusive, reported apart);
//   teardown     close the API (terminates the server).
// Request times are the server's own processing time (`collectTiming`),
// which excludes the IPC an in-process engine does not pay; the client's
// round trip is recorded beside each. The record is written as soon as the
// statistics are read and rewritten after observation, and `<out>.phase`
// names the phase running, so a stopped invocation still says how far it
// got. No whole-file diagnostics run here: the demand's success never
// depends on work the Verter arm does not do.

import { spawnSync } from "node:child_process";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const RESULT_SCHEMA = 2;
// TypeFormatFlags.NoTruncation | TypeFormatFlags.InTypeAlias.
const PRINT_FLAGS = 1 | (1 << 23);

function flag(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

function aliasNamePosition(text, alias) {
  const marker = `type ${alias} `;
  const at = text.indexOf(marker);
  if (at < 0 || text.indexOf(marker, at + 1) >= 0) throw new Error(`the scenario must declare ${alias} exactly once`);
  return at + "type ".length;
}

async function main() {
  const jobPath = flag("--job");
  const outPath = flag("--out");
  if (!jobPath || !outPath) {
    console.error("usage: tsc-probe.mjs --job <job.json> --out <result.json>");
    process.exit(2);
  }
  // Replace a file atomically, so a reader never sees a torn record.
  const replace = (path, text) => {
    writeFileSync(`${path}.tmp`, text);
    renameSync(`${path}.tmp`, path);
  };
  // The phase marker. While a demand phase runs, this client is blocked in a
  // synchronous request and holds only what it held when the phase began:
  // its own memory then (read by the same reader, outside every timer) is
  // the evidence that attributes a containment kill to the server.
  const DEMAND_PHASES = ["spawn", "engine-start", "setup", "init", "cold", "warm"];
  const phase = (name) => {
    let clientBytes = null;
    if (DEMAND_PHASES.includes(name)) {
      const r = spawnSync(job.statsExe, ["stats", "--pid", String(process.pid)], { encoding: "utf8", timeout: 30_000 });
      if (r.status === 0) clientBytes = JSON.parse(r.stdout).currentBytes ?? null;
    }
    replace(`${outPath}.phase`, JSON.stringify({ phase: name, clientBytes }));
  };
  const job = JSON.parse(readFileSync(jobPath, "utf8"));
  if (job.schema !== 1) throw new Error(`job schema ${job.schema} is not 1`);
  if (job.probes.length < 1) throw new Error("a job demands at least one probe");
  const { API } = await import(pathToFileURL(join(job.tsPackageDir, "dist", "api", "sync", "api.js")).href);

  const dir = job.dir.replace(/\\/g, "/");
  const scenarioFile = `${dir}/${job.scenario}`;
  const text = readFileSync(scenarioFile, "utf8");
  const serverTime = (method) => api.getTimingInfo().recentRequests.filter((r) => r.method === method).at(-1)?.serverTimeMs ?? null;

  phase("spawn");
  let t = performance.now();
  const api = new API({ cwd: dir, collectTiming: true, tsserverPath: job.tscExe });
  const spawnMs = performance.now() - t;
  const serverPid = api.client?.channel?.child?.pid;
  if (typeof serverPid !== "number") throw new Error("the tsc API server pid is not reachable");

  phase("engine-start");
  t = performance.now();
  api.ensureInitialized();
  const engineStartRoundTripMs = performance.now() - t;
  const engineStartMs = serverTime("initialize");

  phase("setup");
  t = performance.now();
  const snapshot = api.updateSnapshot({ openProjects: [`${dir}/${job.tsconfig}`] });
  const setupRoundTripMs = performance.now() - t;
  const setupMs = serverTime("updateSnapshot");
  const projects = snapshot.getProjects();
  if (projects.length !== 1) throw new Error(`expected one project, got ${projects.length}`);
  const project = projects[0];
  const rootFiles = project.rootFiles.map((f) => f.replace(/\\/g, "/"));

  const request = (alias) => {
    const position = aliasNamePosition(text, alias);
    const start = performance.now();
    let type;
    let error = null;
    try {
      type = project.checker.getTypeAtPosition(scenarioFile, position);
    } catch (err) {
      error = String(err?.message ?? err);
    }
    const roundTripMs = performance.now() - start;
    const serverMs = serverTime("getTypeAtPosition");
    const outcome = error ? { kind: "fault", detail: error } : type ? { kind: "value" } : { kind: "miss" };
    return { record: { roundTripMs, serverMs, outcome }, type };
  };

  phase("init");
  const init = request(job.initAlias).record;
  const probes = [];
  const types = [];
  for (const alias of job.probes) {
    phase("cold");
    const cold = request(alias);
    phase("warm");
    const warm = [];
    const warmTypes = [];
    for (let i = 0; i < job.warmRepeats; i++) {
      const w = request(alias);
      warm.push(w.record);
      warmTypes.push(w.type);
    }
    probes.push({ alias, cold: cold.record, warm, observeMs: null, observation: null });
    types.push({ cold: cold.type, warm: warmTypes });
  }

  phase("stats");
  const statsErrors = [];
  const readStats = (pid, label) => {
    const r = spawnSync(job.statsExe, ["stats", "--pid", String(pid)], { encoding: "utf8", timeout: 30_000 });
    if (r.status !== 0) {
      statsErrors.push(`${label}: stats --pid ${pid}: exit ${r.status} ${r.stderr?.trim() ?? ""}`);
      return null;
    }
    return JSON.parse(r.stdout);
  };
  const result = {
    schema: RESULT_SCHEMA,
    tool: "tsc",
    stage: "measured",
    tscExe: job.tscExe,
    serverPid,
    rootFiles,
    phases: { spawnMs, engineStartMs, engineStartRoundTripMs, setupMs, setupRoundTripMs, initMs: init.serverMs, teardownMs: null },
    init,
    probes,
    serverAfterRequests: readStats(serverPid, "after requests"),
    serverAfterObserve: null,
    client: null,
    statsErrors,
  };
  replace(outPath, JSON.stringify(result, null, 2));

  phase("observe");
  const print = (type) => {
    // tsc's printer elides a very large type even with NoTruncation, so a
    // union is printed member by member (one set, whatever the order), each
    // member parenthesised so a function member keeps its extent.
    const members = type.isUnionType() ? type.getTypes() : null;
    const printed = members
      ? members.map((member) => `(${project.checker.typeToString(member, undefined, PRINT_FLAGS)})`).join(" | ")
      : project.checker.typeToString(type, undefined, PRINT_FLAGS);
    return { printed, members: members ? members.length : null };
  };
  probes.forEach((probe, index) => {
    const start = performance.now();
    const { cold, warm } = types[index];
    let coldPrint = null;
    if (cold) {
      try {
        const { printed, members } = print(cold);
        coldPrint = printed;
        probe.observation = { text: printed, error: null, errorType: cold.isErrorType(), typeFlags: cold.flags, unionMembers: members };
      } catch (err) {
        probe.observation = { text: null, error: String(err?.message ?? err), errorType: null, typeFlags: null, unionMembers: null };
      }
    } else {
      probe.observation = { text: null, error: "no value to observe", errorType: null, typeFlags: null, unionMembers: null };
    }
    probe.warm.forEach((record, i) => {
      const w = warm[i];
      if (!cold || !w) record.sameAnswerAsCold = false;
      else if (w.id === cold.id) record.sameAnswerAsCold = true;
      else {
        try {
          record.sameAnswerAsCold = coldPrint !== null && print(w).printed === coldPrint && w.isErrorType() === cold.isErrorType();
        } catch {
          record.sameAnswerAsCold = false;
        }
      }
    });
    probe.observeMs = performance.now() - start;
  });
  result.serverAfterObserve = readStats(serverPid, "after observe");
  result.client = { maxRssBytes: process.resourceUsage().maxRSS * 1024, note: "the node client driving the API; not tsc's memory" };

  phase("teardown");
  t = performance.now();
  api.close();
  result.phases.teardownMs = performance.now() - t;
  result.stage = "complete";
  replace(outPath, JSON.stringify(result, null, 2));
  phase("done");
}

main().catch((err) => {
  console.error(err?.stack ?? String(err));
  process.exit(3);
});
