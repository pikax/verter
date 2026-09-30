#!/usr/bin/env node
// Measure tsc 7.0.2's answer for every program of the thenable catalog
// (thenable.mjs) and write thenable-expected.json, the reference the Biome
// comparison classifies every tool against. The method is the main
// reference's (measure-expected.mjs): `tsc -p` over the benchmark tsconfig
// (strict) of the program plus MEASURING_SUFFIX, under verter-supervise; the
// raw print is recorded and interpreted by reference.mjs.
//
//   node scripts/benchmark/semantic-perf/oss/measure-thenable.mjs [--supervisor <exe>]
//       [--typescript-from <dir>] [--allow-sampled]

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { MEASURING_SUFFIX, parseMeasurement } from "../measure-expected.mjs";
import { resolveTypeScript, sha256Text } from "../provenance.mjs";
import { interpretMeasurement } from "../reference.mjs";
import { SETTINGS, tsconfigText } from "../scenarios.mjs";
import { resolveSupervisor, runSupervised } from "../supervisor.mjs";
import { isThenable, THENABLE_EXPECTED, thenableCases } from "./thenable.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..", "..", "..", "..");
const MEM_MB = 8192;
const TIMEOUT_MS = 300_000;

const arg = (name, fallback) => {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : fallback;
};

async function main() {
  const typescript = resolveTypeScript(resolve(arg("--typescript-from", ROOT)));
  const supervisor = resolveSupervisor(ROOT, arg("--supervisor", null)).path;
  const lib = readFileSync(join(HERE, "..", "lib", "bench-globals.d.ts"), "utf8");
  const strict = SETTINGS.find((s) => s.id === "strict");
  const method = {
    measuringSuffixSha256: sha256Text(MEASURING_SUFFIX),
    libSha256: sha256Text(lib),
    tscExeSha256: typescript.exeSha256,
    tscVersion: typescript.versionText,
    platform: `${process.platform}-${process.arch}`,
    memMb: MEM_MB,
    timeoutMs: TIMEOUT_MS,
    launcher: "verter-supervise",
  };
  const work = join(tmpdir(), `thenable-expected-${Date.now()}`);
  const out = { schema: 1, typescript: typescript.version, method, setting: "strict", cases: {} };
  for (const c of thenableCases()) {
    const dir = join(work, c.id);
    mkdirSync(dir, { recursive: true });
    const source = c.source + MEASURING_SUFFIX;
    writeFileSync(join(dir, "lib.bench.d.ts"), lib);
    writeFileSync(join(dir, "scenario.ts"), source);
    writeFileSync(join(dir, "tsconfig.json"), tsconfigText(strict));
    const r = await runSupervised(supervisor, {
      memMb: MEM_MB,
      timeoutMs: TIMEOUT_MS,
      out: join(dir, "sup.json"),
      cwd: dir,
      argv: [typescript.exe, "-p", join(dir, "tsconfig.json")],
      allowSampled: process.argv.includes("--allow-sampled"),
    });
    const rec = r.record;
    if (
      !rec?.launched ||
      rec.killedBy ||
      (rec.errors ?? []).length ||
      ![0, 1, 2].includes(rec.exitCode)
    )
      throw new Error(`${c.id}: tsc did not finish normally (${rec?.killedBy ?? rec?.exitCode})`);
    const stdout = readFileSync(rec.stdoutPath, "utf8");
    const parsed = parseMeasurement(stdout, source);
    const answer = interpretMeasurement({
      never: parsed.never,
      codes: parsed.codes,
      printed: parsed.printed ?? undefined,
    });
    if (answer.gap) throw new Error(`${c.id}: ${answer.gap}`);
    out.cases[c.id] = {
      sourceSha256: sha256Text(c.source),
      never: parsed.never,
      printed: parsed.printed,
      codes: parsed.codes,
      thenable: isThenable(answer.node),
      receipt: {
        exit: rec.exitCode,
        stdoutSha256: sha256Text(stdout),
        sourceSha256: sha256Text(source),
        tsconfigSha256: sha256Text(tsconfigText(strict)),
      },
    };
    console.log(
      `${c.id}: ${answer.digest.preview} -> ${out.cases[c.id].thenable ? "thenable" : "not thenable"}`,
    );
  }
  writeFileSync(THENABLE_EXPECTED, JSON.stringify(out, null, 1) + "\n");
  console.log(`wrote ${THENABLE_EXPECTED}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err?.stack ?? String(err));
    process.exit(2);
  });
}
