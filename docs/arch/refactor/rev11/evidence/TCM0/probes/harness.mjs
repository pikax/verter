// Shared harness for the TCM0 semantic-API probes.
//
// Every probe resolves the candidate TypeScript package from an explicit path so the probe is never
// silently satisfied by whatever `typescript` happens to be installed in the repository. Pass the
// directory containing the installed candidate via --ts <dir> or TS_CANDIDATE_DIR; see README.md.
//
// The fixture is written to a fresh OS temp directory at run time rather than committed into the
// documentation tree, so the probe cannot be affected by (or affect) repository tooling.

import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";

export const EXPECTED_VERSION = "7.1.0-dev.20260822.1";

function argValue(flag) {
  const i = process.argv.indexOf(flag);
  return i !== -1 && i + 1 < process.argv.length ? process.argv[i + 1] : undefined;
}

/** Resolve the candidate package root, refusing to run against any other version. */
export function resolveCandidate() {
  const dir = argValue("--ts") ?? process.env.TS_CANDIDATE_DIR;
  if (!dir) {
    throw new Error(
      "no candidate package given: pass --ts <dir-containing-node_modules/typescript> or set TS_CANDIDATE_DIR",
    );
  }
  const require = createRequire(join(dir, "noop.js"));
  const pkgPath = require.resolve("typescript/package.json");
  const pkg = require("typescript/package.json");
  if (pkg.version !== EXPECTED_VERSION) {
    throw new Error(
      `refusing to run: resolved typescript@${pkg.version} at ${pkgPath}, expected ${EXPECTED_VERSION}`,
    );
  }
  return { pkgPath, version: pkg.version, gitHead: pkg.gitHead, require };
}

/** Import the candidate's sync API entry point. */
export async function loadSyncApi(candidate) {
  const entry = candidate.require.resolve("typescript/unstable/sync");
  return import(pathToFileURL(entry).href);
}

/** Import the candidate's async API entry point (the only one that can attach over a socket/pipe). */
export async function loadAsyncApi(candidate) {
  const entry = candidate.require.resolve("typescript/unstable/async");
  return import(pathToFileURL(entry).href);
}

const MAIN_TS = `import { helper, type Shape } from "./dep.ts";

export interface Widget {
    id: string;
    size: number;
    nested: { deep: boolean };
}

export function makeWidget(id: string, size: number): Widget {
    return { id, size, nested: { deep: true } };
}

export const w = makeWidget("a", 1);
export const viaHelper = helper(w);

export function useShape(s: Shape): number {
    return s.area;
}

export const repeated = makeWidget("b", 2);
export const alsoRepeated = makeWidget("c", 3);

export const idOf = w.id;
`;

const DEP_TS = `import type { Widget } from "./main.ts";

export interface Shape {
    area: number;
}

export function helper(w: Widget): string {
    return w.id;
}
`;

const BROKEN_TS = `export const bad: number = "not a number";
export function missingReturn(): string {
}
`;

/** Create a throwaway fixture project. Returns paths plus a dispose function. */
export function makeFixture() {
  const root = mkdtempSync(join(tmpdir(), "tcm0-probe-"));
  const tsconfig = join(root, "tsconfig.json");
  const main = join(root, "main.ts");
  const dep = join(root, "dep.ts");
  const broken = join(root, "broken.ts");
  writeFileSync(
    tsconfig,
    JSON.stringify(
      {
        compilerOptions: {
          target: "es2022",
          module: "nodenext",
          moduleResolution: "nodenext",
          strict: true,
          allowImportingTsExtensions: true,
          noEmit: true,
          declaration: true,
        },
        include: ["*.ts"],
      },
      null,
      2,
    ),
  );
  writeFileSync(main, MAIN_TS);
  writeFileSync(dep, DEP_TS);
  writeFileSync(broken, BROKEN_TS);
  return {
    root,
    tsconfig,
    main,
    dep,
    broken,
    mainText: MAIN_TS,
    depText: DEP_TS,
    brokenText: BROKEN_TS,
    dispose: () => rmSync(root, { recursive: true, force: true }),
  };
}

/** Byte offset of the first occurrence of `needle` in `text`, asserted present. */
export function offsetOf(text, needle, occurrence = 1) {
  let idx = -1;
  for (let n = 0; n < occurrence; n++) {
    idx = text.indexOf(needle, idx + 1);
    if (idx === -1)
      throw new Error(`fixture offset not found: ${JSON.stringify(needle)} #${occurrence}`);
  }
  return idx;
}

let failures = 0;

export function record(label, value) {
  console.log(`  ${label}: ${value}`);
}

/** Run one named check. `fn` returns a string describing the observation. */
export function check(label, fn) {
  try {
    const observed = fn();
    console.log(`  PASS  ${label} — ${observed}`);
  } catch (err) {
    failures++;
    console.log(`  FAIL  ${label} — ${err && err.message ? err.message : String(err)}`);
  }
}

/** Assert `fn` throws, and report the message. Used for fail-closed behaviour probes. */
export function checkThrows(label, fn) {
  let threw = false;
  let message = "";
  try {
    fn();
  } catch (err) {
    threw = true;
    message = err && err.message ? err.message : String(err);
  }
  if (threw) {
    console.log(`  PASS  ${label} — threw: ${message}`);
  } else {
    failures++;
    console.log(`  FAIL  ${label} — did NOT throw (expected fail-closed)`);
  }
}

export function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

export function section(title) {
  console.log(`\n== ${title}`);
}

export function finish() {
  console.log(`\nFAILURES: ${failures}`);
  if (failures > 0) {
    process.exitCode = 1;
    return;
  }
  // A probe can fail outside the check()/checkThrows() counters — e.g. a caught timeout or thrown
  // error in a try/catch that sets `process.exitCode = 1` directly (see probe8's attach-path catch
  // block). Only claim success when nothing has already recorded a nonzero exit code; never stomp an
  // existing failure signal back to 0.
  if (process.exitCode !== undefined && process.exitCode !== 0) {
    return;
  }
  process.exitCode = 0;
}
