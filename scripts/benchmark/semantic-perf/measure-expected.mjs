#!/usr/bin/env node
// Measure tsc 7.0.2's answer for every scenario in all four
// strictNullChecks x noImplicitAny settings and write expected.json, the
// reference both benchmark arms are classified against.
//
//   node scripts/benchmark/semantic-perf/measure-expected.mjs
//       [--only a,b] [--supervisor <exe> | --via-tsc-capped <tsc-capped.mjs>]
//       [--mem-mb 8192] [--timeout-ms 300000] [--allow-sampled] [--typescript-from <dir>] [--work <dir>]
//
// Method (the CLI, independent of the API the benchmark's tsc arm uses): the
// scenario module plus
//     type __BenchExpand<T> = T extends __BenchNothing ? never : [T];
//     interface __BenchNothing { readonly __benchNothing: 1 }
//     declare const __bench_v: [__BenchExpand<__Probe>];
//     const __bench_s: [never] = __bench_v;
//     type __BenchIsNever = [__Probe] extends [never] ? "yes" : "no";
//     const __bench_n: "never-check" = null! as __BenchIsNever;
// checked by `tsc -p` over the benchmark's own tsconfig (noLib, the library
// as a root file, noErrorTruncation). The head line of the TS2322 message
// prints the one-element tuple (a union source would be elaborated member by
// member instead) of the probe's type rebuilt by a distributive conditional
// that wraps each member in a one-element tuple (a fresh union, so tsc prints
// the members rather than an alias or union origin naming them); the wrappers
// are removed again. An `any` beside TS2589 / TS2590 is recorded as tsc's
// error-any. Every other diagnostic's code is recorded. A run that exhausts
// the cap or the deadline
// is recorded as `killed`, never retried with more.
//
// Every tsc process runs contained: under verter-supervise, or under the
// capped wrapper named by --via-tsc-capped.

import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { RESOURCE_CODES } from "./analyze.mjs";
import { canonicalDigest, canonicalType, canonicalUnionMembers, singleTupleElement } from "./canonical.mjs";
import { resolveTypeScript, sha256Text, TYPESCRIPT_VERSION } from "./provenance.mjs";
import { SETTINGS, selectScenarios, tsconfigText } from "./scenarios.mjs";
import { resolveSupervisor, runSupervised } from "./supervisor.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..", "..");
const EXPECTED = join(HERE, "expected.json");
const MARKER = "const __bench_s: [never] = __bench_v;";
const NEVER_MARKER = "const __bench_n: \"never-check\" = null! as __BenchIsNever;";
export const MEASURING_SUFFIX = `type __BenchExpand<T> = T extends __BenchNothing ? never : [T];
interface __BenchNothing { readonly __benchNothing: 1 }
declare const __bench_v: [__BenchExpand<__Probe>];
${MARKER}
type __BenchIsNever = [__Probe] extends [never] ? "yes" : "no";
${NEVER_MARKER}
`;

function arg(name, fallback) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : fallback;
}

function runCapped(wrapper, tscArgs, cwd, memMb, timeoutMs) {
  return new Promise((done) => {
    const child = spawn(process.execPath, [wrapper, ...tscArgs], {
      cwd,
      env: { ...process.env, TSC_MEM_GB: String(memMb / 1024), TSC_TIMEOUT_SEC: String(Math.ceil(timeoutMs / 1000)) },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("close", (code) => done({ exit: code, stdout, stderr }));
  });
}

/** Parse the answer out of a checked measuring module's output. */
export function parseMeasurement(stdout, source) {
  const markerLine = source.split("\n").findIndex((l) => l.includes(MARKER)) + 1;
  const diagnostics = [];
  for (const line of stdout.split(/\r?\n/)) {
    const m = /^(.+?)\((\d+),(\d+)\): error TS(\d+): (.*)$/.exec(line);
    // Elaboration lines (indented) belong to the diagnostic above; only the
    // head line carries the printed type.
    if (m) diagnostics.push({ file: m[1], line: Number(m[2]), code: Number(m[4]), message: m[5] });
    else if (/^error TS(\d+):/.test(line)) diagnostics.push({ file: "", line: 0, code: Number(/TS(\d+)/.exec(line)[1]), message: line });
  }
  const onMarker = diagnostics.filter((d) => d.file.endsWith("scenario.ts") && d.line === markerLine && d.code === 2322);
  // Only `[never]` is assignable to `[never]` (`any` is not assignable to
  // `never`), so no TS2322 on the measuring line means `never` — accepted
  // only when the positive check on the next line agrees.
  const neverLine = source.split("\n").findIndex((l) => l.includes(NEVER_MARKER)) + 1;
  const neverCheck = diagnostics.find((d) => d.file.endsWith("scenario.ts") && d.line === neverLine && d.code === 2322);
  const isNever = /^Type '"yes"' is not assignable/.test(neverCheck?.message ?? "")
    ? true
    : /^Type '"no"' is not assignable/.test(neverCheck?.message ?? "")
      ? false
      : null;
  if (isNever === null) throw new Error(`the never check printed no verdict: ${(neverCheck?.message ?? "<no diagnostic>").slice(0, 200)}`);
  if (!onMarker.length && !isNever) throw new Error("the measuring assignment reported no TS2322 but the probe is not never");
  if (onMarker.length && isNever) throw new Error("the measuring assignment failed but the probe is never");
  let printed = "never";
  if (onMarker.length) {
    const message = onMarker[0].message;
    const start = message.indexOf("Type '");
    const end = message.lastIndexOf("' is not assignable to type '[never]'");
    if (start !== 0 || end < 0) throw new Error(`unexpected TS2322 message: ${message.slice(0, 200)}`);
    const tuple = message.slice("Type '".length, end).trim();
    if (!tuple.startsWith("[") || !tuple.endsWith("]")) throw new Error(`the measuring tuple did not print: ${tuple.slice(0, 200)}`);
    printed = tuple.slice(1, -1);
  }
  const codes = [...new Set(diagnostics.filter((d) => !onMarker.includes(d) && d !== neverCheck).map((d) => d.code))].sort((a, b) => a - b);
  // `printed` is `never`, `any` (an `any` or error-any probe makes the
  // conditional itself `any`) or a union of one-element tuples `[X]`. tsc's
  // printer elides a very large type even under noErrorTruncation, leaving
  // bare `any` members among the tuples: such a print is not the answer.
  const bare = canonicalType(printed);
  let text = bare;
  if (bare !== "never" && bare !== "any") {
    const members = canonicalUnionMembers(printed);
    const elements = [];
    for (const member of members) {
      try {
        elements.push(singleTupleElement(member));
      } catch {
        return { truncated: true, codes };
      }
    }
    text = elements.join(" | ");
  }
  const errorAny = canonicalType(text) === "any" && codes.some((c) => RESOURCE_CODES.includes(c));
  return { text, codes, errorAny };
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
  const previous = (() => {
    try {
      return JSON.parse(readFileSync(EXPECTED, "utf8"));
    } catch {
      return null;
    }
  })();
  const resume = process.argv.includes("--resume");
  const out = previous && (only.length || resume) ? previous : { scenarios: {} };
  if (previous && previous.libSha256 !== sha256Text(lib)) out.scenarios = {};
  out.schema = 1;
  out.typescript = TYPESCRIPT_VERSION;
  out.libSha256 = sha256Text(lib);
  out.measuredWith =
    "tsc -p (CLI) over the benchmark tsconfig (noLib, lib.bench.d.ts as a root file, noErrorTruncation) of the scenario plus " +
    "`type __BenchExpand<T> = T extends __BenchNothing ? never : [T]; declare const __bench_v: [__BenchExpand<__Probe>]; const __bench_s: [never] = __bench_v;` " +
    "and a never check, read off the head of the TS2322 messages; a print tsc elides (bare `any` among the tuples) is recorded as truncated, a run past the cap or deadline as killed; " +
    (viaCapped ? "run under the capped tsc wrapper" : "run under verter-supervise");
  const save = () => {
    out.scenarios = Object.fromEntries(Object.entries(out.scenarios).sort(([a], [b]) => a.localeCompare(b)));
    writeFileSync(EXPECTED, JSON.stringify(out, null, 2) + "\n");
  };
  for (const scenario of selectScenarios(only)) {
    const entry = { sourceSha256: sha256Text(scenario.source), settings: {} };
    if (resume && out.scenarios[scenario.id]?.sourceSha256 === entry.sourceSha256) {
      console.log(`${scenario.id}: kept (already measured on this source)`);
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
      let stdout;
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
        exit = r.record?.killedBy === "timeout" ? 124 : r.record?.killedBy === "memory" ? 137 : r.record?.exitCode;
        if (r.supervisorExit === 125 || !r.record?.launched) throw new Error(`supervisor failed for ${scenario.id}/${setting.id}: ${r.stderr}`);
        stdout = readFileSync(r.record.stdoutPath, "utf8");
      }
      let result;
      if (exit === 124 || exit === 137) result = { killed: exit === 124 ? "timeout" : "memory", codes: [] };
      else if (![0, 1, 2].includes(exit)) result = { unmeasurable: `tsc exited ${exit}`, codes: [] };
      else {
        try {
          const parsed = parseMeasurement(stdout, source);
          if (parsed.truncated) result = { truncated: true, codes: parsed.codes };
          else {
            const digest = canonicalDigest(parsed.text);
            result = { digest, errorAny: parsed.errorAny, codes: parsed.codes };
            if (digest.length <= 4000) result.text = parsed.text;
          }
        } catch (err) {
          result = { unmeasurable: String(err.message ?? err).slice(0, 300), codes: [] };
        }
      }
      entry.settings[setting.id] = result;
      const shown = result.killed
        ? `killed (${result.killed})`
        : result.truncated
          ? "truncated print"
          : result.unmeasurable
            ? `unmeasurable: ${result.unmeasurable}`
            : result.errorAny
              ? "error-any"
              : result.digest.preview.slice(0, 80);
      console.log(`${scenario.id}/${setting.id}: ${shown} ${result.codes.map((c) => `TS${c}`).join(",")}`);
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
