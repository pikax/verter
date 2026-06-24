// Negative control + incrementality evidence for the tsgo --api gate.
//
// (A) Prove the off-disk carrier in a CONFIG-LESS / inferred-style context
//     (no openProject; parse the file standalone) FAILS where the configured
//     project passes — i.e. today's inferred overlay cannot satisfy the gate.
// (B) Prove updateSnapshot reports a real per-project incremental `changes`
//     delta (changed file listed; project reused) rather than a full rebuild.

import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(ROOT, "fixture");
const norm = (p) => p.replace(/\\/g, "/");
const require = createRequire(import.meta.url);
const opts = { paths: (process.env.NM_BASE || "").split(path.delimiter).filter(Boolean) };
const sourcePkgs = [process.env.TS7_SOURCE, "typescript", "@typescript/native-preview"].filter(
  Boolean,
);
// Import via the PUBLIC `<pkg>/unstable/sync` export (parameterized over the
// source package), honouring the package `exports` map — not a hand-built dist/ path.
let syncApiPath;
for (const p of sourcePkgs) {
  try {
    syncApiPath = require.resolve(`${p}/unstable/sync`, opts);
    break;
  } catch {
    /* try next */
  }
}
if (!syncApiPath)
  throw new Error(
    `could not resolve a TS>=7 sync API (<pkg>/unstable/sync) from ${JSON.stringify(sourcePkgs)}`,
  );
const { API } = await import(pathToFileURL(syncApiPath).href);

const TSCONFIG = path.join(FIXTURE, "tsconfig.json");
const CARRIER_PATH = path.join(FIXTURE, "src", "components", "Widget.carrier.tsx");
const STANDALONE = path.join(FIXTURE, "standalone-carrier.tsx"); // off-disk, outside src/include

const CARRIER = `
import { formatLabel, type FormatOptions } from "@/utils/format";
import { makeUser } from "@spike/shared";
const opts: FormatOptions = { upper: true };
export const label = formatLabel("hello", opts);
export const u = makeUser(1);
const flagKind: "verter-global" = VERTER_GLOBAL_FLAG.kind;
export { flagKind };
`;

const fmt = (ds) =>
  JSON.stringify(
    ds.map((d) => ({ code: d.code, text: d.text })),
    null,
    2,
  );

function overlay(targetPath, getContent) {
  const tNorm = norm(targetPath);
  const dirNorm = norm(path.dirname(targetPath));
  const base = path.basename(targetPath);
  const read = typeof getContent === "function" ? getContent : () => getContent;
  return {
    readFile: (f) => (norm(f) === tNorm ? read() : undefined),
    fileExists: (f) => (norm(f) === tNorm ? true : undefined),
    getAccessibleEntries: (d) => {
      if (norm(d) !== dirNorm) return undefined;
      let real;
      try {
        const e = fs.readdirSync(d, { withFileTypes: true });
        real = {
          files: e.filter((x) => x.isFile()).map((x) => x.name),
          directories: e.filter((x) => x.isDirectory()).map((x) => x.name),
        };
      } catch {
        real = { files: [], directories: [] };
      }
      if (!real.files.includes(base)) real.files.push(base);
      return real;
    },
  };
}

console.log("=== (A) NEGATIVE CONTROL: config-less / inferred-style standalone parse ===");
// Standalone path lives OUTSIDE src/ so it is NOT covered by tsconfig include,
// and we do NOT pass openProject — mirroring the inferred/config-less path.
{
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: overlay(STANDALONE, CARRIER),
  });
  try {
    // No openProject -> snapshot has no configured project. This is the
    // "config-less" condition the design doc says cannot pass the gate.
    const snap = api.updateSnapshot({});
    const projects = snap.getProjects();
    console.log(
      "projects without openProject:",
      projects.map((p) => norm(p.configFileName)),
    );

    let defProj;
    let assocError = "";
    try {
      defProj = snap.getDefaultProjectForFile(STANDALONE);
    } catch (e) {
      assocError = String(e.message || e);
    }
    console.log(
      "standalone default project:",
      defProj
        ? norm(defProj.configFileName)
        : `(none) ${assocError ? "-> server error: " + assocError : ""}`,
    );

    let inferredDiags = [];
    if (defProj) {
      try {
        inferredDiags = defProj.program.getSemanticDiagnostics(STANDALONE);
      } catch (e) {
        assocError = String(e.message || e);
      }
    }
    const has2307 = inferredDiags.some((d) => d.code === 2307);
    const has2304 = inferredDiags.some((d) => d.code === 2304);
    if (defProj)
      console.log("inferred/standalone diags:", inferredDiags.length ? fmt(inferredDiags) : "[]");
    const negConfirmed = projects.length === 0 || !!assocError || !defProj || has2307 || has2304;
    console.log(
      `NEGATIVE CONTROL RESULT: ${
        negConfirmed
          ? "CONFIRMED — config-less path (no openProject) yields NO configured project for the carrier (zero projects / 'no project found' / false TS2307|TS2304). The inferred/config-less overlay CANNOT satisfy the gate; the configured-project path is required."
          : "UNEXPECTED — config-less path resolved cleanly (would weaken the 'inferred cannot pass' claim)"
      }`,
    );
    if (!negConfirmed) process.exitCode = 1;
    snap.dispose();
  } finally {
    api.close();
  }
}

console.log(
  "\n=== (B) INCREMENTALITY EVIDENCE (informational): updateSnapshot `changes` delta ===",
);
// NOTE: this is INFORMATIONAL only — it introspects a private response field
// (`changes`) whose population varies across tsgo `--api` builds. The
// AUTHORITATIVE incrementality proof is in harness.mjs GATE 3 + incremental.mjs
// (observable: diagnostics flip on a stable project handle, unchanged dependency
// retained). Part (B) never affects the gate exit code; it only prints what the
// installed binary exposes so a version bump's delta shape is visible.
{
  let content = CARRIER;
  const api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: overlay(CARRIER_PATH, () => content),
  });
  try {
    const client = api["client"];
    const base = client.apiRequest("updateSnapshot", { openProject: TSCONFIG }); // snapshot 1
    const baseProjHandle = base.projects?.[0]?.id;
    console.log(
      "snapshot1 project handle:",
      baseProjHandle,
      "(changes absent on first snapshot:",
      base.changes === undefined,
      ")",
    );

    // Edit the off-disk carrier content, then push a changed-file delta.
    content = CARRIER.replace(
      'export const label = formatLabel("hello", opts);',
      'export const label: number = formatLabel("hello", opts); // now wrong',
    );
    const raw = client.apiRequest("updateSnapshot", {
      openProject: TSCONFIG,
      fileChanges: { changed: [CARRIER_PATH] },
    });
    console.log("snapshot2 response keys:", Object.keys(raw));
    console.log(
      "snapshot2 project handle:",
      raw.projects?.[0]?.id,
      "(handle stable:",
      baseProjHandle === raw.projects?.[0]?.id,
      ")",
    );
    console.log("changes delta (build-dependent):", JSON.stringify(raw.changes ?? null));
    const changed = raw.changes?.changedProjects
      ? Object.values(raw.changes.changedProjects)[0]
      : undefined;
    const carrierListed = changed?.changedFiles?.some((f) => norm(f) === norm(CARRIER_PATH));
    console.log(
      `INCREMENTAL EVIDENCE: ${
        raw.changes && changed
          ? `this build exposes a per-project incremental delta (changedProjects present; carrier in changedFiles=${!!carrierListed})`
          : `this build does NOT expose a public \`changes\` delta on the sync client (harness GATE 3 proves incrementality observably instead). Project handle stable across the edit: ${baseProjHandle === raw.projects?.[0]?.id}`
      }`,
    );
  } finally {
    api.close();
  }
}
