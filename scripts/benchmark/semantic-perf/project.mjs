// The project workload of the semantic benchmark: the selected scenarios
// combined into ONE project, answered by ONE instance of each tool.
//
// The per-scenario sections start a fresh process for every demand, which
// charges a whole-program checker its full start-up for one answer. Here
// every tool gets the whole project once: tsc -p and the open-source
// checkers check it in one process, and Verter and the tsc API answer every
// scenario's demand, one after another, in one live process (a session
// script). A whole-program checker amortises its one pass over every
// answer; a demand-driven engine pays for each demand.
//
// Layout: each member scenario lives in its own directory `s/<id>/` (its
// companion files beside its module), so modules never collide; the module
// is the measuring program (the scenario plus the reference's measuring
// suffix), which the whole-program arms check and the demand arms read the
// probe from. Scenarios that contribute to the global scope (a global
// interface, enum or library augmentation) collide with the other sizes of
// their series, so a project holds one member per global scope (the
// largest selected size). A scenario whose standalone reference has no
// answer (tsc exhausted it, or its print is unrecorded) is left out: one
// exhausted demand would end the whole program's run for every tool.
//
// Every member's answer is classified against its standalone measured
// reference, so the reference arms (`tsc -p`) must reproduce each one in
// the combined program: a combination that changes an answer fails
// validation.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  classifyVerterAnswer,
  invocationEnd,
  observationEvidence,
  runLimits,
  stats,
  tscAnswerStatus,
} from "./analyze.mjs";
import { canonicalDigest } from "./canonical.mjs";
import { MEASURING_SUFFIX } from "./measure-expected.mjs";
import { sha256Text } from "./provenance.mjs";
import { companionFiles, SETTINGS } from "./scenarios.mjs";
import { probeDigest, referenceFor } from "./summary.mjs";
import { supervisorRecordProblems } from "./supervisor.mjs";
import { classifyOssAnswer, readOssAnswer, tscMeasureStatus } from "./oss/answers.mjs";
import {
  ossArm,
  ossCommand,
  ossInvocationEnd,
  prepareTools,
  REFERENCE_ARMS,
  referenceArms,
  toolOfArm,
} from "./oss/checkers.mjs";
import { provisioned } from "./oss/provision.mjs";

/** The directory of one member, relative to the project root. */
export const memberDir = (id) => `s/${id}`;

/** The alias every member's module declares for the init request. */
export const PROJECT_INIT_ALIAS = "__BenchInit";

/**
 * Choose the project's members from `scenarios` (catalog order) for
 * `settingIds`: `{ members, excluded: [{ id, reason }] }`.
 */
export function composeProject(scenarios, expected, settingIds) {
  const excluded = [];
  const byScope = new Map();
  const members = [];
  for (const scenario of scenarios) {
    const gap = settingIds
      .map((setting) => referenceFor(expected, scenario.id, setting))
      .find((ref) => !ref || ref.gap);
    if (gap !== undefined) {
      excluded.push({
        id: scenario.id,
        reason: `its standalone reference has no answer (${gap?.gap ?? "not measured"})`,
      });
      continue;
    }
    if (scenario.globalScope) {
      const previous = byScope.get(scenario.globalScope);
      if (previous) {
        members.splice(members.indexOf(previous), 1);
        excluded.push({
          id: previous.id,
          reason: `it shares the global scope ${scenario.globalScope} with ${scenario.id}`,
        });
      }
      byScope.set(scenario.globalScope, scenario);
    }
    members.push(scenario);
  }
  return { members, excluded };
}

/** The root files of the project, in tsconfig order (the library first). */
export function projectFiles(members) {
  return members.flatMap((m) =>
    [...companionFiles(m), "scenario.ts"].map((f) => `${memberDir(m.id)}/${f}`),
  );
}

/** The project's tsconfig for `setting`. */
export function projectTsconfigText(setting, members) {
  return (
    JSON.stringify(
      {
        compilerOptions: {
          strict: true,
          strictNullChecks: setting.strictNullChecks,
          noImplicitAny: setting.noImplicitAny,
          noLib: true,
          noEmit: true,
          target: "es2022",
          module: "esnext",
          skipLibCheck: false,
          noErrorTruncation: true,
        },
        files: ["lib.bench.d.ts", ...projectFiles(members)],
      },
      null,
      2,
    ) + "\n"
  );
}

/** The measuring module of one member. */
export const memberSource = (scenario) => scenario.source + MEASURING_SUFFIX;

/** Write the project for `setting` under `dir`; returns its input digests. */
export function materializeProject(dir, members, setting, libText) {
  mkdirSync(dir, { recursive: true });
  const inputs = {};
  const write = (rel, text) => {
    const path = join(dir, ...rel.split("/"));
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, text);
    inputs[rel] = sha256Text(text);
  };
  write("lib.bench.d.ts", libText);
  for (const m of members) {
    for (const [name, text] of Object.entries(m.files ?? {}))
      write(`${memberDir(m.id)}/${name}`, text);
    write(`${memberDir(m.id)}/scenario.ts`, memberSource(m));
  }
  write("tsconfig.json", projectTsconfigText(setting, members));
  return inputs;
}

/** The session script answering every member's demand, in member order. */
export function projectSteps(members) {
  return members.map((m) => ({
    kind: "demand",
    concurrent: false,
    requests: [{ file: `${memberDir(m.id)}/scenario.ts`, alias: "__Probe" }],
  }));
}

/** The session job the Verter probe and the tsc API driver read. */
export function projectJob(dir, members, libMode) {
  return {
    schema: 1,
    dir,
    tsconfig: "tsconfig.json",
    lib: "lib.bench.d.ts",
    libMode,
    files: projectFiles(members),
    initFile: `${memberDir(members[0].id)}/scenario.ts`,
    initAlias: PROJECT_INIT_ALIAS,
    steps: projectSteps(members),
    observability: false,
  };
}

/**
 * The diagnostics lines of one whole-program output that belong to one
 * member (its module and its companions), whatever spelling the tool uses
 * for the path (relative, absolute, backslashed, `\\?\`-prefixed).
 */
export function memberLines(stdout, id) {
  const marker = `${memberDir(id)}/`;
  return stdout
    .split(/\r?\n/)
    .filter((line) => {
      const m = /^(.+?)\(\d+,\d+\): error TS\d+:/.exec(line);
      if (!m) return false;
      const path = m[1].replace(/\\/g, "/");
      return path.startsWith(marker) || path.includes(`/${marker}`);
    })
    .map((line) => {
      // Re-root the path so the measurement reader sees the member's own
      // `scenario.ts` (it reads the marker line of that file).
      const at = line.indexOf("(");
      const path = line.slice(0, at).replace(/\\/g, "/");
      const rel = path.slice(path.lastIndexOf(marker) + marker.length);
      return rel + line.slice(at);
    })
    .join("\n");
}

/** Every member's answer from one whole-program output, by id. */
export function readProjectAnswers(format, stdout, members) {
  const out = {};
  for (const m of members)
    out[m.id] = readOssAnswer(format, memberLines(stdout, m.id), memberSource(m));
  return out;
}

// ---------------------------------------------------------------- running

export const PROJECT_SCHEMA = 1;

/** The demand arms of the project section: one live process answering every member in turn. */
export const PROJECT_DEMAND_ARMS = ["verter", "tsc-api"];

/** The project section's arms for a run's options and its project-capable tools, in schedule order. */
export const projectArms = (options, toolIds) => [
  ...referenceArms(options),
  "verter",
  ...(options?.noTsc ? [] : ["tsc-api"]),
  ...toolIds.map(ossArm),
];

/**
 * A whole-program tool can check the project only through a project
 * command (a tsconfig) and a reader of tsc's diagnostic format.
 */
export function projectCapability(tool) {
  if (tool.format !== "tsc")
    return `it prints diagnostics in its own format (${tool.format}), with no per-file answer to read`;
  if (tool.argv.some((a) => a.includes("{scenario}")))
    return "its command checks one file, not a project";
  return null;
}

/**
 * The project's deadline: the run's per-demand deadline for every member,
 * plus one startup allowance (each tool answers every member in one process).
 */
export function projectDeadlineMs(options, memberCount) {
  return (options.timeoutMs ?? 0) * memberCount + (options.startupAllowanceMs ?? 0);
}

const HERE = dirname(fileURLToPath(import.meta.url));

/** A session record reduced to what the section reads (answers become digests). */
export function compactSession(record, members) {
  if (!record) return null;
  const tool = record.tool;
  const answers = {};
  members.forEach((m, i) => {
    const req = record.steps?.[i]?.requests?.[0];
    if (!req) return;
    const obs = req.observation ?? null;
    let digest = null;
    let canonicalError = null;
    if (obs && typeof obs.text === "string" && (obs.error === undefined || obs.error === null)) {
      try {
        digest = canonicalDigest(obs.text);
      } catch (err) {
        canonicalError = String(err.message ?? err);
      }
    }
    answers[m.id] = {
      outcome: req.outcome ?? null,
      ms: tool === "verter" ? (req.micros ?? NaN) / 1000 : (req.serverMs ?? null),
      evidence: observationEvidence(tool, obs),
      observeError: obs ? (obs.error ?? null) : "the answer was not observed",
      errorType: obs?.errorType ?? null,
      unknownLeaves: obs?.unknownLeaves ?? 0,
      unknownSamples: obs?.unknownSamples ?? [],
      shape: obs?.shape ?? null,
      digest: digest ? { sha256: digest.sha256, preview: digest.preview } : null,
      canonicalError,
    };
  });
  return {
    tool,
    stage: record.stage ?? null,
    phases: record.phases ?? null,
    peakBytes: (tool === "verter" ? record.afterSteps : record.serverAfterSteps)?.peakBytes ?? null,
    statsErrors: record.statsErrors ?? [],
    answers,
  };
}

/** Every member's reading from a whole-program output (digests, never the print). */
export function compactReadings(format, stdout, members) {
  const out = {};
  for (const [id, read] of Object.entries(readProjectAnswers(format, stdout, members)))
    out[id] = read.unreadable
      ? { unreadable: read.unreadable }
      : {
          answer: {
            digest: { sha256: read.answer.digest.sha256, preview: read.answer.digest.preview },
            errorAny: read.answer.errorAny,
            codes: read.answer.codes,
          },
        };
  return out;
}

/** The settings a run's options select. */
const settingsOf = (options) =>
  options?.settings === "all" ? SETTINGS : SETTINGS.filter((s) => s.id === "strict");

/**
 * Run the project section. `ctx`: root, outDir, opts, scenarios, expected,
 * tools (the manifest, or null), ids (the checker ids the run selects),
 * supervisor, verterProbe, typescript, libText, runtimeEnv, schedule,
 * runSupervised, log, jobs.
 */
export async function runProject(ctx) {
  const { outDir, opts, typescript, runtimeEnv, log } = ctx;
  const settings = settingsOf(opts);
  const { members, excluded } = composeProject(
    ctx.scenarios,
    ctx.expected,
    settings.map((s) => s.id),
  );
  let available = {};
  let status = {};
  if (ctx.ids.length) {
    ({ available, status } = await prepareTools(ctx.root, ctx.tools, ctx.ids, {
      supervisor: ctx.supervisor,
      log,
      jobs: ctx.jobs,
    }));
    for (const id of Object.keys(available)) {
      const why = projectCapability(ctx.tools[id]);
      if (why) {
        delete available[id];
        status[id] = { ...status[id], status: "not-applicable", reason: why };
      }
    }
  }
  const arms = projectArms(opts, Object.keys(available));
  const projects = {};
  const deadlineMs = projectDeadlineMs(opts, members.length);
  if (!members.length) log("project: no scenario can join the project");
  for (const setting of members.length ? settings : []) {
    const dir = join(outDir, "project", setting.id);
    const inputs = materializeProject(dir, members, setting, ctx.libText);
    const job = projectJob(dir, members, opts.libMode);
    const jobPath = join(outDir, "project", `${setting.id}.job.json`);
    const tscJobPath = join(outDir, "project", `${setting.id}.tsc-job.json`);
    writeFileSync(jobPath, JSON.stringify(job, null, 2));
    writeFileSync(
      tscJobPath,
      JSON.stringify(
        {
          ...job,
          tsPackageDir: typescript.packageDir,
          tscExe: typescript.exe,
          statsExe: ctx.verterProbe,
        },
        null,
        2,
      ),
    );
    projects[setting.id] = { setting: setting.id, dir, inputs, jobPath, tscJobPath };
  }
  const plan = ctx.schedule(Object.keys(projects), arms, opts.repeat, opts.warmup);
  const invocations = [];
  for (const [index, step] of plan.entries()) {
    const project = projects[step.key];
    const runDir = join(outDir, "project-runs", step.key, step.arm);
    mkdirSync(runDir, { recursive: true });
    const runBase = join(runDir, `${step.warmup ? "warmup" : "rep"}-${step.rep}`);
    const sessionOut = `${runBase}.session.json`;
    const command =
      step.arm === "verter"
        ? [ctx.verterProbe, "session", "--job", project.jobPath, "--out", sessionOut]
        : step.arm === "tsc-api"
          ? [
              process.execPath,
              join(HERE, "tsc-session-probe.mjs"),
              "--job",
              project.tscJobPath,
              "--out",
              sessionOut,
            ]
          : ossCommand(step.arm, {
              tscExe: typescript.exe,
              tools: ctx.tools,
              available,
              measureDir: project.dir,
            });
    const supOut = `${runBase}.sup.json`;
    const spawnedAtMs = Date.now();
    const result = await ctx.runSupervised(ctx.supervisor, {
      memMb: opts.memMb + opts.infraMb,
      timeoutMs: deadlineMs,
      out: supOut,
      env: runtimeEnv,
      cwd: project.dir,
      argv: command,
      allowSampled: opts.allowSampled,
    });
    const record = result.record
      ? { ...result.record, samples: undefined, sampleCount: result.record.samples?.length ?? 0 }
      : null;
    const inv = {
      index,
      setting: step.key,
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command,
      supervisorOut: supOut,
      supervisorExit: result.supervisorExit,
      supervisorReadError: result.readError ?? result.spawnError ?? null,
      supervisor: record,
      spawnedAtMs,
    };
    if (PROJECT_DEMAND_ARMS.includes(step.arm)) {
      inv.sessionOut = sessionOut;
      try {
        const marker = JSON.parse(readFileSync(`${sessionOut}.phase`, "utf8"));
        inv.phase = marker.phase ?? null;
        inv.phaseHistory = marker.history ?? null;
      } catch {
        inv.phase = null;
      }
      try {
        inv.session = compactSession(JSON.parse(readFileSync(sessionOut, "utf8")), members);
      } catch (err) {
        inv.session = null;
        inv.sessionReadError = String(err.message ?? err);
      }
    } else if (record?.launched && !record.killedBy && record.stdoutPath) {
      try {
        const stdout = readFileSync(record.stdoutPath, "utf8");
        const tool = toolOfArm(step.arm);
        inv.stdoutSha256 = sha256Text(stdout);
        inv.readings = compactReadings(tool ? ctx.tools[tool].format : "tsc", stdout, members);
      } catch (err) {
        inv.readError = `the output could not be read: ${err.message}`;
      }
    }
    invocations.push(inv);
    const end = record?.killedBy ? `killed:${record.killedBy}` : `exit ${record?.exitCode ?? "?"}`;
    log(
      `[project ${index + 1}/${plan.length}] ${step.key} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: ${end} ${record?.wallMs?.toFixed?.(0) ?? "?"} ms`,
    );
  }
  return {
    schema: PROJECT_SCHEMA,
    members: members.map((m) => m.id),
    excluded,
    deadlineMs,
    tools: status,
    toolsAfter: Object.fromEntries(
      Object.keys(available).map((id) => [
        id,
        provisioned(ctx.root, id, ctx.tools[id]).sha256 ?? null,
      ]),
    ),
    verter: { binary: ctx.verterProbe },
    arms,
    projects,
    plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    invocations,
  };
}

// ---------------------------------------------------------------- reading

const isReferenceArm = (arm) => Boolean(REFERENCE_ARMS[arm]);

/** How a demand arm's invocation ended (a session record exists only complete). */
export function projectSessionEnd(inv, limits) {
  let end = invocationEnd(inv, limits);
  if (end.kind === "exited" && end.exitCode !== 0)
    end = { kind: "child-failure", detail: `exit ${end.exitCode}` };
  if (end.kind === "observe-killed") end = { kind: "exited", exitCode: 0 };
  if (end.kind === "exited" && !inv.session)
    end = { kind: "child-failure", detail: inv.sessionReadError ?? "no session record" };
  return end;
}

/** One member's answer from a demand arm, in the shape the probe classifiers read. */
function memberDemandAnswer(end, a, peakBytes) {
  return {
    end,
    outcome: a?.outcome ?? { kind: "missing" },
    warmKinds: [],
    warmSame: [],
    observed: Boolean(a),
    digest: a?.digest ?? null,
    canonicalError: a?.canonicalError ?? null,
    observeError: a ? a.observeError : "the answer was not observed",
    evidence: a?.evidence ?? false,
    errorType: a?.errorType ?? null,
    unknownLeaves: a?.unknownLeaves ?? 0,
    unknownSamples: a?.unknownSamples ?? [],
    shape: a?.shape ?? null,
    enginePeakBytes: peakBytes,
  };
}

const beyondDigests = new Map();
function beyondOf(scenario) {
  if (!scenario?.beyond) return null;
  if (!beyondDigests.has(scenario.id)) {
    let d = null;
    try {
      d = canonicalDigest(scenario.beyond);
    } catch {
      d = null;
    }
    beyondDigests.set(scenario.id, d);
  }
  return beyondDigests.get(scenario.id);
}

/**
 * One invocation's result for one member: `{ class, detail }` for Verter
 * and the tools, `{ status, problem?, detail? }` for the tsc arms.
 */
export function memberResult(inv, scenario, reference, limits) {
  const beyond = beyondOf(scenario);
  if (PROJECT_DEMAND_ARMS.includes(inv.arm)) {
    const end = projectSessionEnd(inv, limits);
    const answer = memberDemandAnswer(
      end,
      end.kind === "exited" ? inv.session?.answers?.[scenario.id] : null,
      inv.session?.peakBytes ?? null,
    );
    if (inv.arm === "verter")
      return classifyVerterAnswer(answer, {
        reference,
        beyond,
        probe: probeDigest(scenario),
        budgetBytes: limits.budgetBytes,
      });
    return tscAnswerStatus(answer, reference, beyond, limits.budgetBytes);
  }
  const end = ossInvocationEnd(inv, limits);
  const read =
    inv.readings?.[scenario.id] ??
    (end.kind === "exited"
      ? { unreadable: inv.readError ?? "the output was not read" }
      : { unreadable: "no output" });
  if (isReferenceArm(inv.arm)) return tscMeasureStatus(end, read, reference);
  return classifyOssAnswer(end, read, { reference, beyond });
}

const metric = (inv) => {
  const s = inv.supervisor ?? {};
  return {
    wallMs: s.wallMs ?? null,
    peakBytes: s.peakBytes ?? null,
    cpuMs:
      typeof s.cpuUserMs === "number" && typeof s.cpuKernelMs === "number"
        ? s.cpuUserMs + s.cpuKernelMs
        : null,
  };
};

const demandMsOf = (inv, id) => {
  const v = inv.session?.answers?.[id]?.ms;
  return typeof v === "number" && Number.isFinite(v) ? v : null;
};

/** The section's summary: per setting and arm, the whole-process figures and every member's result. */
export function summarizeProject(project, expected, scenarios, options) {
  const limits = runLimits(options);
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  const settings = {};
  for (const settingId of Object.keys(project.projects ?? {})) {
    const arms = {};
    for (const arm of project.arms ?? []) {
      const invs = (project.invocations ?? []).filter(
        (i) => i.setting === settingId && i.arm === arm,
      );
      const measured = invs.filter((i) => !i.warmup);
      const demand = PROJECT_DEMAND_ARMS.includes(arm);
      const perMember = {};
      const counts = {};
      for (const id of project.members ?? []) {
        const reference = referenceFor(expected, id, settingId);
        const results = measured.map((i) => memberResult(i, byId.get(id), reference, limits));
        const labels = [...new Set(results.map((r) => r.class ?? r.status))];
        const label = labels.length === 1 ? labels[0] : labels.length ? "inconsistent" : "none";
        const detail =
          results.find((r) => r.problem)?.problem ?? results.find((r) => r.detail)?.detail ?? "";
        perMember[id] = {
          result: label,
          detail,
          ...(demand ? { demandMs: stats(measured.map((i) => demandMsOf(i, id))) } : {}),
        };
        counts[label] = (counts[label] ?? 0) + 1;
      }
      const metrics = measured.map(metric);
      arms[arm] = {
        invocations: invs.length,
        measured: measured.length,
        ends: [
          ...new Set(
            measured.map((i) => {
              const s = i.supervisor ?? {};
              return s.killedBy ? `killed:${s.killedBy}` : `exit ${s.exitCode ?? "?"}`;
            }),
          ),
        ],
        wallMs: stats(metrics.map((m) => m.wallMs)),
        peakBytes: stats(metrics.map((m) => m.peakBytes)),
        cpuMs: stats(metrics.map((m) => m.cpuMs)),
        ...(demand
          ? {
              demandTotalMs: stats(
                measured.map((i) => {
                  const xs = (project.members ?? []).map((id) => demandMsOf(i, id));
                  return xs.every((x) => x !== null) ? xs.reduce((t, x) => t + x, 0) : null;
                }),
              ),
            }
          : {}),
        counts,
        perMember,
      };
    }
    settings[settingId] = { arms };
  }
  return { members: (project.members ?? []).length, excluded: project.excluded ?? [], settings };
}

// ---------------------------------------------------------------- validation

const stable = (value) => JSON.stringify(value);

/** Validate the project section of a run. */
export function validateProject(project, expected, scenarios, options, schedule) {
  const failures = [];
  const fail = (m) => failures.push(`project: ${m}`);
  const limits = runLimits(options);
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  const settings = settingsOf(options);
  const { members, excluded } = composeProject(
    scenarios,
    expected,
    settings.map((s) => s.id),
  );
  if (stable(members.map((m) => m.id)) !== stable(project.members ?? []))
    fail("the members are not the composition of the run's scenarios");
  if (stable(excluded) !== stable(project.excluded ?? []))
    fail("the left-out scenarios are not the composition's");
  if (!members.length) return { ok: failures.length === 0, failures };
  if (project.deadlineMs !== projectDeadlineMs(options, members.length))
    fail(`the deadline ${project.deadlineMs} ms is not the per-demand deadline for every member`);
  const capable = Object.entries(project.tools ?? {})
    .filter(([, t]) => t.status === "available")
    .map(([id]) => id);
  if (stable(project.arms ?? []) !== stable(projectArms(options, capable)))
    fail("the arms are not the reference arms, Verter, the tsc API and every capable tool");
  for (const [id, sha] of Object.entries(project.toolsAfter ?? {}))
    if (sha !== project.tools?.[id]?.binary?.sha256)
      fail(`${id}: its binary changed during the run`);
  for (const setting of settings) {
    const p = project.projects?.[setting.id];
    if (!p) {
      fail(`no project for the ${setting.id} setting`);
      continue;
    }
    if (p.inputs?.["tsconfig.json"] !== sha256Text(projectTsconfigText(setting, members)))
      fail(`${setting.id}: the tsconfig is not the composition's`);
    if (expected.method?.libSha256 !== p.inputs?.["lib.bench.d.ts"])
      fail(`${setting.id}: the library is not the one the references were measured with`);
    for (const m of members) {
      if (p.inputs?.[`${memberDir(m.id)}/scenario.ts`] !== sha256Text(memberSource(m)))
        fail(`${setting.id}: ${m.id}'s module is not the catalog's measuring program`);
      for (const [name, text] of Object.entries(m.files ?? {}))
        if (p.inputs?.[`${memberDir(m.id)}/${name}`] !== sha256Text(text))
          fail(`${setting.id}: ${m.id}'s ${name} is not the catalog's`);
    }
  }
  const expectedPlan = schedule(
    Object.keys(project.projects ?? {}),
    project.arms ?? [],
    options.repeat ?? 0,
    options.warmup ?? 0,
  ).map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`);
  if (stable(expectedPlan) !== stable(project.plan ?? []))
    fail("the plan is not the counterbalanced schedule of its projects and arms");
  const seen = new Set();
  for (const inv of project.invocations ?? []) {
    const id = `${inv.setting}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
    if (seen.has(id)) fail(`${id}: recorded twice`);
    seen.add(id);
    if (inv.supervisor) {
      for (const p of supervisorRecordProblems(inv.supervisor)) fail(`${id}: ${p}`);
      if (inv.supervisor.timeoutMs !== project.deadlineMs)
        fail(
          `${id}: deadline ${inv.supervisor.timeoutMs} is not the project's ${project.deadlineMs} ms`,
        );
    }
    const demand = PROJECT_DEMAND_ARMS.includes(inv.arm);
    const end = demand ? projectSessionEnd(inv, limits) : ossInvocationEnd(inv, limits);
    if (end.kind === "harness-failure") fail(`${id}: failed child: ${end.detail}`);
    if (demand && end.kind === "exited") {
      if (inv.session?.stage !== "complete") fail(`${id}: the session record is not complete`);
      for (const e of inv.session?.statsErrors ?? []) fail(`${id}: ${e}`);
    }
    // tsc must reproduce every member's standalone reference: a combination
    // that changes an answer is not the catalog's demand.
    if (isReferenceArm(inv.arm) || inv.arm === "tsc-api")
      for (const m of members) {
        const r = memberResult(
          inv,
          byId.get(m.id),
          referenceFor(expected, m.id, inv.setting),
          limits,
        );
        if (r.status === "problem") fail(`${id}: ${m.id}: ${r.problem}`);
      }
  }
  for (const entry of project.plan ?? []) if (!seen.has(entry)) fail(`${entry}: no record`);
  return { ok: failures.length === 0, failures };
}

// ---------------------------------------------------------------- report

const esc = (s) =>
  String(s ?? "")
    .replace(/\|/g, "\\|")
    .replace(/\n/g, " ");
const msOf = (s) => (s ? `${s.median.toFixed(0)} ms` : "—");
const mibOf = (s) => (s ? `${(s.median / 1024 / 1024).toFixed(0)} MiB` : "—");

/** The project section of the report. */
export function renderProjectMarkdown(project) {
  const lines = [];
  const push = (...l) => lines.push(...l);
  const summary = project.summary ?? { settings: {}, excluded: [] };
  push("# Project: every scenario in one project, one instance of each tool (`--project`)", "");
  push(
    `Validation of this section: **${project.validation?.ok ? "PASSED" : "FAILED"}**`,
    "",
    `- The ${project.members?.length ?? 0} member scenarios are combined into one project, each in its own directory with its companion files. **tsc -p** and the open-source checkers check the whole project in one process; **Verter** and the **tsc API** answer every member's demand, one after another, in one live process (a session script, in catalog order). A whole-program checker pays for one pass over everything; a demand-driven engine pays for every demand.`,
    "- Every member's answer is classified against its standalone measured reference; the tsc arms must reproduce each one in the combined program (a combination that changes an answer fails validation).",
    `- Figures are **whole-process**: wall time from process start to exit, the process tree's peak memory, CPU time; the demand arms also report the sum of their per-demand engine times. Each process has the run's per-demand deadline for every member (${project.deadlineMs ?? "?"} ms) and the run's memory cap.`,
    "",
  );
  if (summary.excluded?.length) {
    push("Left out of the project:", "");
    for (const e of summary.excluded) push(`- \`${e.id}\`: ${esc(e.reason)}`);
    push("");
  }
  const tools = Object.entries(project.tools ?? {});
  if (tools.length) {
    push("| tool | status |", "|---|---|");
    for (const [id, t] of tools)
      push(
        `| ${esc(t.name ?? id)} (\`${id}\`) | ${t.status === "available" ? "available" : `**${esc(t.status)}**: ${esc(t.reason)}`} |`,
      );
    push("");
  }
  for (const [settingId, s] of Object.entries(summary.settings ?? {})) {
    push(`## ${settingId}`, "");
    push(
      "| arm | ends | answers matched | wall | peak | CPU | sum of demands |",
      "|---|---|---|---|---|---|---|",
    );
    for (const [arm, a] of Object.entries(s.arms)) {
      const good = (a.counts.matched ?? 0) + (a.counts.reference ?? 0);
      const others = Object.entries(a.counts)
        .filter(([k]) => k !== "matched" && k !== "reference")
        .map(([k, n]) => `${k} ${n}`)
        .join(", ");
      push(
        `| ${arm} | ${a.ends.join(", ")} | ${good}/${summary.members}${others ? ` (${esc(others)})` : ""} | ${msOf(a.wallMs)} | ${mibOf(a.peakBytes)} | ${msOf(a.cpuMs)} | ${a.demandTotalMs ? msOf(a.demandTotalMs) : "—"} |`,
      );
    }
    push("");
    const armNames = Object.keys(s.arms);
    push(`| member | ${armNames.join(" | ")} |`, `|---|${armNames.map(() => "---").join("|")}|`);
    for (const id of project.members ?? [])
      push(
        `| ${id} | ${armNames
          .map((arm) => {
            const r = s.arms[arm].perMember[id];
            const t = r.demandMs ? ` ${r.demandMs.median.toFixed(1)} ms` : "";
            return `${esc(r.result)}${t}`;
          })
          .join(" | ")} |`,
      );
    push("");
  }
  if (project.validation && !project.validation.ok) {
    push("## Validation failures", "");
    for (const f of project.validation.failures.slice(0, 100)) push(`- ${esc(f)}`);
    push("");
  }
  return lines.join("\n");
}
