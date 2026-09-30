#!/usr/bin/env node
// Measure tsc 7.0.2's answer for every scenario in all four
// strictNullChecks x noImplicitAny settings and write expected.json, the
// reference both benchmark arms are classified against.
//
//   node scripts/benchmark/semantic-perf/measure-expected.mjs
//       [--only a,b] [--resume] [--supervisor <exe> | --via-tsc-capped <tsc-capped.mjs>]
//       [--mem-mb 8192] [--timeout-ms 300000] [--allow-sampled] [--typescript-from <dir>] [--work <dir>]
//
// Method (the CLI, independent of the API the benchmark's tsc arm uses): the
// scenario module plus MEASURING_SUFFIX,
//     type __BenchExpand<T> = T extends unknown ? [T] : never;
//     declare const __bench_v: [__BenchExpand<__Probe>];
//     const __bench_s: [never] = __bench_v;
//     type __BenchIsNever = [__Probe] extends [never] ? "yes" : "no";
//     const __bench_n: "never-check" = null! as __BenchIsNever;
// checked by `tsc -p` over the benchmark's own tsconfig (noLib, the library
// as a root file, noErrorTruncation). The distributive conditional rebuilds
// the probe's type as a fresh union of one-element tuples, one per member
// and filtering none (every type extends `unknown`), so tsc prints the
// members rather than an alias or union origin naming them; the outer tuple
// makes the head line of the TS2322 message print the whole type (a union
// source would be elaborated member by member). The second assignment prints
// whether the probe is `never`, the one answer the first cannot print.
//
// This script records only what tsc printed — the raw text inside the outer
// tuple, the never verdict, every other diagnostic's code and a digest of the
// whole output — so the interpretation (reference.mjs) can be re-derived and
// audited without measuring again. A run past the cap or the deadline is
// recorded as `killed`, never retried with more.
//
// Every tsc process runs contained: under verter-supervise, or under the
// capped wrapper named by --via-tsc-capped.

import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { resolveTypeScript, sha256Text, TYPESCRIPT_VERSION } from "./provenance.mjs";
import { interpretMeasurement } from "./reference.mjs";
import { SETTINGS, selectScenarios, tsconfigText } from "./scenarios.mjs";
import { resolveSupervisor, runSupervised } from "./supervisor.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..", "..");
const EXPECTED = join(HERE, "expected.json");
const MARKER = "const __bench_s: [never] = __bench_v;";
const NEVER_MARKER = 'const __bench_n: "never-check" = null! as __BenchIsNever;';
export const MEASURING_SUFFIX = `type __BenchExpand<T> = T extends unknown ? [T] : never;
declare const __bench_v: [__BenchExpand<__Probe>];
${MARKER}
type __BenchIsNever = [__Probe] extends [never] ? "yes" : "no";
${NEVER_MARKER}
`;
/** A printed answer larger than this is recorded by digest only. */
const MAX_STORED_PRINT = 1 << 20;

function arg(name, fallback) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : fallback;
}

function runCapped(wrapper, tscArgs, cwd, memMb, timeoutMs) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [wrapper, ...tscArgs], {
      cwd,
      env: {
        ...process.env,
        TSC_MEM_GB: String(memMb / 1024),
        TSC_TIMEOUT_SEC: String(Math.ceil(timeoutMs / 1000)),
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("close", (code) => done({ exit: code, stdout, stderr }));
  });
}

/**
 * What tsc printed for a checked measuring module: the raw text inside the
 * outer tuple of the TS2322 head line (null when there is none), the never
 * verdict, and every other diagnostic's code. Throws on output it cannot
 * read as a measurement; never guesses.
 */
export function parseMeasurement(stdout, source) {
  const lines = source.split("\n");
  const markerLine = lines.findIndex((l) => l.includes(MARKER)) + 1;
  const neverLine = lines.findIndex((l) => l.includes(NEVER_MARKER)) + 1;
  const diagnostics = [];
  for (const line of stdout.split(/\r?\n/)) {
    const m = /^(.+?)\((\d+),(\d+)\): error TS(\d+): (.*)$/.exec(line);
    // Elaboration lines (indented) belong to the diagnostic above; only the
    // head line carries the printed type.
    if (m) diagnostics.push({ file: m[1], line: Number(m[2]), code: Number(m[4]), message: m[5] });
    else if (/^error TS(\d+):/.test(line))
      diagnostics.push({ file: "", line: 0, code: Number(/TS(\d+)/.exec(line)[1]), message: line });
  }
  const inScenario = (d, line) =>
    d.file.endsWith("scenario.ts") && d.line === line && d.code === 2322;
  const onMarker = diagnostics.filter((d) => inScenario(d, markerLine));
  const neverCheck = diagnostics.find((d) => inScenario(d, neverLine));
  const verdict = /^Type '"yes"' is not assignable/.test(neverCheck?.message ?? "")
    ? true
    : /^Type '"no"' is not assignable/.test(neverCheck?.message ?? "")
      ? false
      : null;
  if (verdict === null)
    throw new Error(
      `the never check printed no verdict: ${(neverCheck?.message ?? "<no diagnostic>").slice(0, 200)}`,
    );
  // Only `[never]` is assignable to `[never]` (`any` is not assignable to
  // `never`), so the two assignments must agree.
  if (!onMarker.length && !verdict)
    throw new Error("the measuring assignment reported no TS2322 but the probe is not never");
  if (onMarker.length && verdict)
    throw new Error("the measuring assignment failed but the probe is never");
  let printed = null;
  if (onMarker.length) {
    const message = onMarker[0].message;
    const end = message.lastIndexOf("' is not assignable to type '[never]'");
    if (!message.startsWith("Type '") || end < 0)
      throw new Error(`unexpected TS2322 message: ${message.slice(0, 200)}`);
    const tuple = message.slice("Type '".length, end).trim();
    if (!tuple.startsWith("[") || !tuple.endsWith("]"))
      throw new Error(`the measuring tuple did not print: ${tuple.slice(0, 200)}`);
    printed = tuple.slice(1, -1);
  }
  const codes = [
    ...new Set(
      diagnostics.filter((d) => !onMarker.includes(d) && d !== neverCheck).map((d) => d.code),
    ),
  ].sort((a, b) => a - b);
  return { never: verdict, printed, codes };
}

async function main() {
  const only = (arg("--only", "") ?? "").split(",").filter(Boolean);
  const memMb = Number(arg("--mem-mb", 8192));
  const timeoutMs = Number(arg("--timeout-ms", 300_000));
  const viaCapped = arg("--via-tsc-capped", null);
  const typescript = resolveTypeScript(resolve(arg("--typescript-from", ROOT)));
  const supervisor = viaCapped ? null : resolveSupervisor(ROOT, arg("--supervisor", null)).path;
  const work = resolve(arg("--work", join(tmpdir(), `semantic-perf-expected-${Date.now()}`)));
  const lib = readFileSync(join(HERE, "lib", "bench-globals.d.ts"), "utf8");
  // The method identity every cell is measured by; a cell measured by any
  // other is re-measured, never relabelled. A capped wrapper runs its own
  // tsc, which this script cannot verify, so its cells carry no verified
  // executable.
  const method = {
    measuringSuffixSha256: sha256Text(MEASURING_SUFFIX),
    libSha256: sha256Text(lib),
    tscExeSha256: viaCapped ? null : typescript.exeSha256,
    tscVersion: typescript.versionText,
    platform: `${process.platform}-${process.arch}`,
    memMb,
    timeoutMs,
    launcher: viaCapped ? "capped tsc wrapper" : "verter-supervise",
  };
  const methodKey = JSON.stringify(method);
  const previous = (() => {
    try {
      return JSON.parse(readFileSync(EXPECTED, "utf8"));
    } catch {
      return null;
    }
  })();
  const sameMethod = previous?.schema === 3 && JSON.stringify(previous.method) === methodKey;
  const resume = process.argv.includes("--resume");
  const out = previous && sameMethod && (only.length || resume) ? previous : { scenarios: {} };
  out.schema = 3;
  out.typescript = TYPESCRIPT_VERSION;
  out.method = method;
  out.measuredWith =
    "tsc -p (CLI) over the benchmark tsconfig (noLib, lib.bench.d.ts as a root file, noErrorTruncation) of the scenario plus the measuring suffix " +
    "(a distributive one-element-tuple wrapper that filters no member, and a never check); the raw print inside the outer tuple of the TS2322 head line is recorded";
  const save = () => {
    out.scenarios = Object.fromEntries(
      Object.entries(out.scenarios).sort(([a], [b]) => a.localeCompare(b)),
    );
    writeFileSync(EXPECTED, JSON.stringify(out, null, 1) + "\n");
  };
  for (const scenario of selectScenarios(only)) {
    const entry = { sourceSha256: sha256Text(scenario.source), settings: {} };
    if (resume && out.scenarios[scenario.id]?.sourceSha256 === entry.sourceSha256) {
      console.log(`${scenario.id}: kept (already measured on this source by this method)`);
      continue;
    }
    for (const setting of SETTINGS) {
      const dir = join(work, scenario.id, setting.id);
      mkdirSync(dir, { recursive: true });
      const source = scenario.source + MEASURING_SUFFIX;
      writeFileSync(join(dir, "lib.bench.d.ts"), lib);
      writeFileSync(join(dir, "scenario.ts"), source);
      writeFileSync(join(dir, "tsconfig.json"), tsconfigText(setting));
      const tscArgs = ["-p", join(dir, "tsconfig.json")];
      let exit;
      let stdout = "";
      let termination = null;
      if (viaCapped) {
        const r = await runCapped(viaCapped, tscArgs, dir, memMb, timeoutMs);
        exit = r.exit;
        stdout = r.stdout;
      } else {
        const r = await runSupervised(supervisor, {
          memMb,
          timeoutMs,
          out: join(dir, "sup.json"),
          cwd: dir,
          argv: [typescript.exe, ...tscArgs],
          allowSampled: process.argv.includes("--allow-sampled"),
        });
        if (r.supervisorExit === 125 || !r.record?.launched || (r.record.errors ?? []).length)
          throw new Error(
            `supervisor failed for ${scenario.id}/${setting.id}: ${(r.record?.errors ?? []).join("; ")} ${r.stderr}`,
          );
        // The evidence a kill is read against (see reference.mjs).
        termination = {
          killedBy: r.record.killedBy ?? null,
          backend: r.record.backend ?? null,
          containment: r.record.containment ?? null,
          memLimitBytes: r.record.memLimitBytes ?? null,
          killTriggerBytes: r.record.killTriggerBytes ?? null,
          timeoutMs: r.record.timeoutMs ?? null,
          wallMs: r.record.wallMs ?? null,
          peakBytes: r.record.peakBytes ?? null,
        };
        exit =
          r.record.killedBy === "timeout"
            ? 124
            : r.record.killedBy === "memory"
              ? 137
              : r.record.exitCode;
        stdout = readFileSync(r.record.stdoutPath, "utf8");
      }
      const receipt = {
        exit,
        stdoutSha256: sha256Text(stdout),
        stdoutBytes: stdout.length,
        tsconfigSha256: sha256Text(tsconfigText(setting)),
        sourceSha256: sha256Text(source),
        method: methodKey,
        termination,
      };
      let result;
      if (exit === 124 || exit === 137)
        result = { killed: exit === 124 ? "timeout" : "memory", codes: [], receipt };
      else if (![0, 1, 2].includes(exit))
        result = { unmeasurable: `tsc exited ${exit}`, codes: [], receipt };
      else {
        try {
          const parsed = parseMeasurement(stdout, source);
          result = { never: parsed.never, codes: parsed.codes, receipt };
          if (parsed.printed !== null) {
            if (parsed.printed.length <= MAX_STORED_PRINT) result.printed = parsed.printed;
            else
              result.printedOversize = {
                sha256: sha256Text(parsed.printed),
                length: parsed.printed.length,
              };
          }
        } catch (err) {
          result = { unmeasurable: String(err.message ?? err).slice(0, 300), codes: [], receipt };
        }
      }
      entry.settings[setting.id] = result;
      let shown;
      try {
        const answer = interpretMeasurement(result);
        shown = answer.gap ?? (answer.errorAny ? "error-any" : answer.digest.preview.slice(0, 80));
      } catch (err) {
        shown = `uninterpretable: ${err.message}`;
      }
      console.log(
        `${scenario.id}/${setting.id}: ${shown} ${result.codes.map((c) => `TS${c}`).join(",")}`,
      );
    }
    out.scenarios[scenario.id] = entry;
    save();
  }
  save();
  console.log(`wrote ${EXPECTED}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err?.stack ?? String(err));
    process.exit(2);
  });
}
