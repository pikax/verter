// One-command driver for the tsgo --api capability gate.
// Discovers the user-installed tsgo binary (@typescript/native-preview) from the
// repo node_modules, sets TSGO_PATH + NM_BASE, and runs all four gate scripts
// (harness, control, incremental, companion-identity).
// Exit 0 only if every script passes (GO). Used as the version-bump capability gate.

import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import path from "node:path";
import fs from "node:fs";
import os from "node:os";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(ROOT, "..", "..");
const require = createRequire(import.meta.url);

// Locate the platform tsgo exe the way Verter's production engine SELECTION will: the
// TS>=7 distribution declares a per-os/arch optional dependency whose bin/lib/ holds
// tsgo[.exe]. We discover it portably here rather than hardcoding a platform path.
//
// Engine SELECTION rule (mirrored here; full text in README/§2.8 of the design doc):
//   - The INSTALLED `typescript` package whose MAJOR is >= 7 WINS ALWAYS (regardless of
//     the exact version/dist-tag), with platform binaries `@typescript/typescript-<plat>-<arch>`.
//   - The `@typescript/native-preview` dev-preview channel (binaries
//     `@typescript/native-preview-<plat>-<arch>`) is the same engine/API and is accepted
//     as a FALLBACK SOURCE when no installed `typescript@>=7` is present. Both ship the
//     IDENTICAL `--api` surface (`./unstable/sync` -> dist/api/sync/api.js).
//   - This gate only DISCOVERS an installed/repo binary. The production NO-TYPESCRIPT
//     fallback (not exercised here) DOWNLOADS the npm `typescript` package at the `rc`
//     dist-tag (the current TS7 channel — `npm view typescript@rc` = 7.0.1-rc today; npm
//     `latest` is still the 6.x line), retargeting `latest` once TS7 ships stable, and
//     fails closed when offline. It is download-only — never a bundled/forked binary.
const TS7_SOURCES = [
  {
    pkg: "typescript",
    binRe: /@typescript\+typescript-(?!.*native)/,
    sibRe: /^typescript-(?!.*native)/,
  },
  {
    pkg: "@typescript/native-preview",
    binRe: /@typescript\+native-preview-/,
    sibRe: /native-preview-/,
  },
];

function discoverTsgo() {
  const exeName = os.platform() === "win32" ? "tsgo.exe" : "tsgo";
  const pnpmDir = path.join(REPO_ROOT, "node_modules", ".pnpm");
  for (const src of TS7_SOURCES) {
    // Resolve the source package's version (must be >=7 to be a tsgo distribution).
    let version = null;
    try {
      const pkgJson = require.resolve(`${src.pkg}/package.json`, { paths: [REPO_ROOT] });
      const meta = JSON.parse(fs.readFileSync(pkgJson, "utf8"));
      if (!/^([7-9]|\d{2,})\./.test(meta.version)) continue; // skip a TS<7 `typescript`
      version = meta.version;
    } catch {
      continue;
    }

    const candidates = [];
    // pnpm layout: node_modules/.pnpm/<scope+name>-<plat>-<arch>@<ver>/node_modules/<scope>/<name>-<plat>-<arch>/{lib,bin}/tsgo[.exe]
    if (fs.existsSync(pnpmDir)) {
      for (const entry of fs.readdirSync(pnpmDir)) {
        if (!src.binRe.test(entry)) continue;
        const inner = path.join(pnpmDir, entry, "node_modules");
        if (!fs.existsSync(inner)) continue;
        for (const scope of fs.readdirSync(inner)) {
          const scopeDir = path.join(inner, scope);
          try {
            for (const p of fs.readdirSync(scopeDir)) {
              candidates.push(path.join(scopeDir, p, "lib", exeName));
              candidates.push(path.join(scopeDir, p, "bin", exeName));
            }
          } catch {
            /* not a dir */
          }
        }
      }
    }
    // npm/classic layout: sibling platform dirs under node_modules/@typescript/.
    try {
      const scopeRoot = path.join(REPO_ROOT, "node_modules", "@typescript");
      for (const sibling of fs.readdirSync(scopeRoot)) {
        if (!src.sibRe.test(sibling)) continue;
        candidates.push(path.join(scopeRoot, sibling, "lib", exeName));
        candidates.push(path.join(scopeRoot, sibling, "bin", exeName));
      }
    } catch {
      /* ignore */
    }

    const hit = candidates.find((c) => fs.existsSync(c));
    if (hit) return { exe: hit, version, source: src.pkg };
  }
  throw new Error(
    `Could not locate a tsgo binary (${exeName}). Looked for the user-installed \`typescript@>=7\` ` +
      `(binaries @typescript/typescript-<plat>-<arch>) then @typescript/native-preview. ` +
      `Install one, or set TSGO_PATH explicitly.`,
  );
}

const { exe, version, source } = process.env.TSGO_PATH
  ? {
      exe: process.env.TSGO_PATH,
      version: "(env-provided)",
      source: process.env.TS7_SOURCE || "(env)",
    }
  : discoverTsgo();

console.log(`tsgo binary: ${exe.replace(/\\/g, "/")}`);
console.log(`TS>=7 source: ${source} @ ${version}`);
console.log("");

const env = { ...process.env, TSGO_PATH: exe, NM_BASE: REPO_ROOT, TS7_SOURCE: source };
const scripts = ["harness.mjs", "control.mjs", "incremental.mjs", "companion-identity.mjs"];
let allOk = true;
for (const s of scripts) {
  console.log(`\n########## ${s} ##########`);
  const r = spawnSync(process.execPath, [path.join(ROOT, s)], { env, stdio: "inherit" });
  if (r.status !== 0) allOk = false;
}
console.log(`\n=== GATE OVERALL: ${allOk ? "GO" : "NO-GO"} (${source} @ ${version}) ===`);
process.exit(allOk ? 0 : 1);
