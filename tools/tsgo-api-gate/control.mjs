// Negative control + incrementality evidence for the tsgo --api gate.
//
// (A) Prove the off-disk carrier in a CONFIG-LESS / inferred-style context
//     (no openProjects; parse the file standalone) FAILS where the configured
//     project passes — i.e. today's inferred overlay cannot satisfy the gate.
// (B) Show the snapshot update's per-project incremental `changes`
//     delta (changed file listed; project reused) rather than a full rebuild.

import { fileURLToPath } from "node:url";
import path from "node:path";

import { loadSyncApi, overlayFileSystem } from "./sync-api.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(ROOT, "fixture");
const norm = (p) => p.replace(/\\/g, "/");
// The public `<pkg>/sync` + `<pkg>/fs` exports (see ./sync-api.mjs).
const { API, serverFS } = await loadSyncApi();

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
  return overlayFileSystem(serverFS, { [targetPath]: getContent });
}

console.log("=== (A) NEGATIVE CONTROL: config-less / inferred-style standalone parse ===");
// Standalone path lives OUTSIDE src/ so it is NOT covered by tsconfig include,
// and we do NOT pass openProjects — mirroring the inferred/config-less path.
{
  using api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: overlay(STANDALONE, CARRIER),
  });
  {
    // No openProjects -> snapshot has no configured project. This is the
    // "config-less" condition the design doc says cannot pass the gate.
    using snap = api.createSnapshot();
    const projects = snap.getProjects();
    console.log(
      "projects without openProjects:",
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
          ? "CONFIRMED — config-less path (no openProjects) yields NO configured project for the carrier (zero projects / 'no project found' / false TS2307|TS2304). The inferred/config-less overlay CANNOT satisfy the gate; the configured-project path is required."
          : "UNEXPECTED — config-less path resolved cleanly (would weaken the 'inferred cannot pass' claim)"
      }`,
    );
    if (!negConfirmed) process.exitCode = 1;
  }
}

console.log(
  "\n=== (B) INCREMENTALITY EVIDENCE (informational): snapshot update `changes` delta ===",
);
// NOTE: this is INFORMATIONAL only — it introspects a private response field
// (`changes`) whose population varies across tsgo `--api` builds. The
// AUTHORITATIVE incrementality proof is in harness.mjs GATE 3 + incremental.mjs
// (observable: diagnostics flip on a stable project handle, unchanged dependency
// retained). Part (B) never affects the gate exit code; it only prints what the
// installed binary exposes so a version bump's delta shape is visible.
{
  let content = CARRIER;
  using api = new API({
    tsserverPath: process.env.TSGO_PATH,
    cwd: FIXTURE,
    fs: overlay(CARRIER_PATH, () => content),
  });
  // The base snapshot goes through the public API (it initializes the session
  // and owns the base handle's lifetime); only the update is a raw request, so
  // the raw response — including `changes` — is visible, and its handle is
  // released explicitly below.
  using base = api.createSnapshot({ openProjects: [TSCONFIG] }); // snapshot 1
  const client = api["client"];
  let raw;
  try {
    const baseProjHandle = base.getConfiguredProject(TSCONFIG)?.id;
    console.log("snapshot1 project handle:", baseProjHandle);

    // Edit the off-disk carrier content, then push a changed-file delta.
    content = CARRIER.replace(
      'export const label = formatLabel("hello", opts);',
      'export const label: number = formatLabel("hello", opts); // now wrong',
    );
    raw = client.apiRequest("updateSnapshot", {
      snapshot: base.id,
      changes: { fileNotifications: { changed: [CARRIER_PATH] }, ensurePrograms: true },
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
    // changedFiles are canonical path keys (lower-cased on case-insensitive hosts).
    const carrierListed = changed?.changedFiles?.some(
      (f) => norm(f).toLowerCase() === norm(CARRIER_PATH).toLowerCase(),
    );
    console.log(
      `INCREMENTAL EVIDENCE: ${
        raw.changes && changed
          ? `this build exposes a per-project incremental delta (changedProjects present; carrier in changedFiles=${!!carrierListed})`
          : `this build does NOT expose a public \`changes\` delta on the sync client (harness GATE 3 proves incrementality observably instead). Project handle stable across the edit: ${baseProjHandle === raw.projects?.[0]?.id}`
      }`,
    );
  } finally {
    if (raw && raw.snapshot !== base.id) client.apiRequest("release", { snapshot: raw.snapshot });
  }
}
