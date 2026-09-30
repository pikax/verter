#!/usr/bin/env node
// The tsc arm of the equivalent-demand semantic benchmark: answers one job
// through TypeScript 7.0.2's native API (the `typescript/unstable/sync`
// client driving the native `tsc --api` server) and writes one JSON record.
//
//   node tsc-probe.mjs --job <job.json> --out <result.json>
//
// The job is the SAME job the Verter probe reads (same directory, files,
// aliases, warm repetitions) plus the resolved TypeScript package directory
// and the Verter probe executable used to read the tsc server's OS
// statistics (so both tools' memory comes from the same reader).
//
// Phases, each timed separately and never overlapping:
//   spawn     start the native server and connect (`new API`);
//   setup     open the project (`updateSnapshot`): read, parse, program
//             (server-side time; the round trip is recorded beside it);
//   init      getTypeAtPosition on `__BenchInit`'s name (absorbs one-time
//             lazy checker initialisation);
//   cold      getTypeAtPosition on `__Probe`'s name: the declared type of
//             the alias, exactly what the Verter arm resolves;
//   observe   typeToString / error-type / union members (outside the timer);
//   warm      the same request repeated;
//   stats     the server process's OS statistics, read with the server alive;
//   diagnostics  the scenario file's semantic diagnostics (after the stats,
//             so the whole-file check inflates neither timing nor retained
//             memory);
//   teardown  close the API (terminates the server).
// A request's time is recorded both as the client's round trip and as the
// server's own processing time (`collectTiming`); the report uses the server
// time, which excludes the IPC a native in-process engine does not pay.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

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
  const job = JSON.parse(readFileSync(jobPath, "utf8"));
  if (job.schema !== 1) throw new Error(`job schema ${job.schema} is not 1`);
  if (job.probes.length < 1) throw new Error("a job demands at least one probe");
  const { API } = await import(pathToFileURL(join(job.tsPackageDir, "dist", "api", "sync", "api.js")).href);

  const dir = job.dir.replace(/\\/g, "/");
  const scenarioFile = `${dir}/${job.scenario}`;
  const text = readFileSync(scenarioFile, "utf8");

  let t = performance.now();
  const api = new API({ cwd: dir, collectTiming: true });
  const spawnMs = performance.now() - t;
  const serverPid = api.client?.channel?.child?.pid;
  if (typeof serverPid !== "number") throw new Error("the tsc API server pid is not reachable");

  t = performance.now();
  const snapshot = api.updateSnapshot({ openProjects: [`${dir}/${job.tsconfig}`] });
  const projects = snapshot.getProjects();
  if (projects.length !== 1) throw new Error(`expected one project, got ${projects.length}`);
  const project = projects[0];
  const setupRoundTripMs = performance.now() - t;
  // The snapshot's server-side time, like every request's: it excludes the
  // IPC an in-process engine does not pay.
  const setupServerMs =
    api.getTimingInfo().recentRequests.filter((r) => r.method === "updateSnapshot").at(-1)?.serverTimeMs ?? null;
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
    const timing = api.getTimingInfo();
    const server = timing.recentRequests.filter((r) => r.method === "getTypeAtPosition").at(-1);
    const outcome = error ? { kind: "fault", detail: error } : type ? { kind: "value" } : { kind: "miss" };
    return { record: { roundTripMs, serverMs: server?.serverTimeMs ?? null, outcome }, type };
  };

  const init = request(job.initAlias).record;
  const probes = [];
  for (const alias of job.probes) {
    const cold = request(alias);
    t = performance.now();
    let observation;
    if (cold.type) {
      try {
        // tsc's printer elides a very large type even with NoTruncation, so
        // a union is printed member by member (one set, whatever the order).
        const members = cold.type.isUnionType() ? cold.type.getTypes() : null;
        const printed = members
          ? members.map((member) => project.checker.typeToString(member, undefined, PRINT_FLAGS)).join(" | ")
          : project.checker.typeToString(cold.type, undefined, PRINT_FLAGS);
        observation = {
          text: printed,
          error: null,
          errorType: cold.type.isErrorType(),
          typeFlags: cold.type.flags,
          unionMembers: members ? members.length : null,
        };
      } catch (err) {
        observation = { text: null, error: String(err?.message ?? err), errorType: null, typeFlags: null, unionMembers: null };
      }
    } else {
      observation = { text: null, error: "no value to observe", errorType: null, typeFlags: null, unionMembers: null };
    }
    const observeMs = performance.now() - t;
    const warm = [];
    for (let i = 0; i < job.warmRepeats; i++) warm.push(request(alias).record);
    probes.push({ alias, cold: cold.record, warm, observeMs, observation });
  }

  const statsErrors = [];
  const readStats = (pid) => {
    const r = spawnSync(job.statsExe, ["stats", "--pid", String(pid)], { encoding: "utf8", timeout: 30_000 });
    if (r.status !== 0) {
      statsErrors.push(`stats --pid ${pid}: exit ${r.status} ${r.stderr?.trim() ?? ""}`);
      return null;
    }
    return JSON.parse(r.stdout);
  };
  const serverAfterRequests = readStats(serverPid);
  const clientUsage = process.resourceUsage();

  let diagnostics = null;
  try {
    diagnostics = project.program.getSemanticDiagnostics(scenarioFile).map((d) => ({ code: d.code, text: d.text }));
  } catch (err) {
    statsErrors.push(`diagnostics: ${err?.message ?? err}`);
  }

  t = performance.now();
  api.close();
  const teardownMs = performance.now() - t;

  const result = {
    schema: RESULT_SCHEMA,
    tool: "tsc",
    serverPid,
    rootFiles,
    phases: { spawnMs, setupMs: setupServerMs, setupRoundTripMs, initMs: init.serverMs, teardownMs },
    init,
    probes,
    serverAfterRequests,
    client: { maxRssBytes: clientUsage.maxRSS * 1024, note: "the node client driving the API; not tsc's memory" },
    diagnostics,
    statsErrors,
  };
  writeFileSync(outPath, JSON.stringify(result, null, 2));
}

main().catch((err) => {
  console.error(err?.stack ?? String(err));
  process.exit(3);
});
