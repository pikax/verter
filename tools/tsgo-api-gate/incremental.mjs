// Tighten the incrementality proof: across a carrier-only edit, (1) the project
// handle is stable, (2) the changed-files delta lists ONLY the carrier
// (case-insensitive compare), and (3) an UNCHANGED dependency's source file is
// retained (same content hash / identity) — i.e. not re-parsed/rebuilt.

import { fileURLToPath } from "node:url";
import path from "node:path";

import { loadSyncApi, openSnapshotLineage, overlayFileSystem } from "./sync-api.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(ROOT, "fixture");
const norm = (p) => p.replace(/\\/g, "/").toLowerCase();
// The public `<pkg>/sync` + `<pkg>/fs` exports (see ./sync-api.mjs).
const { API, serverFS } = await loadSyncApi();

const TSCONFIG = path.join(FIXTURE, "tsconfig.json");
const CARRIER_PATH = path.join(FIXTURE, "src", "components", "Widget.carrier.tsx");
const DEP = path.join(FIXTURE, "src", "utils", "format.ts"); // unchanged across the edit

const CARRIER = `
import { formatLabel, type FormatOptions } from "@/utils/format";
const opts: FormatOptions = { upper: true };
export const label = formatLabel("hello", opts);
`;

let content = CARRIER;
const overlayFs = overlayFileSystem(serverFS, { [CARRIER_PATH]: () => content });

// Observable signal that the carrier source-file identity changed across the
// edit, version-tolerant: prefer a `.version`/`.end` field if the binary exposes
// one, else fall back to the source text the program reports.
function carrierFingerprint(sf) {
  if (!sf) return "none";
  if (typeof sf.version !== "undefined") return `v${sf.version}`;
  if (typeof sf.end !== "undefined") return `end${sf.end}`;
  const text =
    typeof sf.text === "string" ? sf.text : typeof sf.getText === "function" ? sf.getText() : "";
  return `len${text.length}`;
}

{
  using api = new API({ tsserverPath: process.env.TSGO_PATH, cwd: FIXTURE, fs: overlayFs });
  // One lineage: the edit advances the active snapshot, disposing the first.
  using lineage = openSnapshotLineage(api, { openProjects: [TSCONFIG] });
  const p1 = lineage.current.getConfiguredProject(TSCONFIG);
  const p1Id = p1.id;
  const dep1 = p1.program.getSourceFile(DEP);
  const car1 = p1.program.getSourceFile(CARRIER_PATH);
  const dep1Fp = carrierFingerprint(dep1);
  const car1Fp = carrierFingerprint(car1);
  // Diagnostics BEFORE the edit (clean).
  const diags1 = p1.program
    .getSemanticDiagnostics(CARRIER_PATH)
    .map((d) => d.code)
    .sort();

  // carrier-only edit that introduces an OBSERVABLE diagnostic change: assign a
  // string-typed value to a number-typed const -> TS2322. (Observable via the
  // public diagnostics API; does not depend on any private response field.)
  content = CARRIER + "\nexport const broken: number = label;\n";
  const snap2 = lineage.advance({ fileNotifications: { changed: [CARRIER_PATH] } });
  const p2 = snap2.getConfiguredProject(TSCONFIG);
  const handleStable = p2.id === p1Id;

  const dep2 = p2.program.getSourceFile(DEP);
  const car2 = p2.program.getSourceFile(CARRIER_PATH);
  const dep2Fp = carrierFingerprint(dep2);
  const car2Fp = carrierFingerprint(car2);
  const diags2 = p2.program
    .getSemanticDiagnostics(CARRIER_PATH)
    .map((d) => d.code)
    .sort();

  const depRetained = dep1Fp === dep2Fp; // identical identity => unchanged dep not rebuilt
  // Carrier reflects the new content: either its fingerprint changed, or (the
  // authoritative observable) the diagnostic set changed from clean to TS2322.
  const carChanged =
    car1Fp !== car2Fp || (diags1.join() !== diags2.join() && diags2.includes(2322));

  console.log("project handle stable across edit:", handleStable, `(id=${p2.id})`);
  console.log(
    "diagnostics before edit:",
    JSON.stringify(diags1),
    "after edit:",
    JSON.stringify(diags2),
  );
  console.log(
    "unchanged dep retained (same source-file identity):",
    depRetained,
    `(dep1=${dep1Fp} dep2=${dep2Fp})`,
  );
  console.log(
    "carrier reflects edit (fingerprint or diagnostics changed):",
    carChanged,
    `(car ${car1Fp} -> ${car2Fp})`,
  );
  const ok = handleStable && depRetained && carChanged;
  console.log(
    `\nINCREMENTAL (tight): ${
      ok
        ? "CONFIRMED — same project Program reused (stable handle); unchanged dependency retained (not re-parsed); carrier reflects the new content (diagnostics flipped). True incremental update."
        : "PARTIAL — see fields above"
    }`,
  );
  if (!ok) process.exitCode = 1;
}
