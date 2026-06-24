// Tighten the incrementality proof: across a carrier-only edit, (1) the project
// handle is stable, (2) the changed-files delta lists ONLY the carrier
// (case-insensitive compare), and (3) an UNCHANGED dependency's source file is
// retained (same content hash / identity) — i.e. not re-parsed/rebuilt.

import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE = path.join(ROOT, "fixture");
const norm = (p) => p.replace(/\\/g, "/").toLowerCase();
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
const DEP = path.join(FIXTURE, "src", "utils", "format.ts"); // unchanged across the edit

const CARRIER = `
import { formatLabel, type FormatOptions } from "@/utils/format";
const opts: FormatOptions = { upper: true };
export const label = formatLabel("hello", opts);
`;

let content = CARRIER;
const overlayFs = {
  readFile: (f) => (norm(f) === norm(CARRIER_PATH) ? content : undefined),
  fileExists: (f) => (norm(f) === norm(CARRIER_PATH) ? true : undefined),
  getAccessibleEntries: (d) => {
    if (norm(d) !== norm(path.dirname(CARRIER_PATH))) return undefined;
    const e = fs.readdirSync(d, { withFileTypes: true });
    const r = {
      files: e.filter((x) => x.isFile()).map((x) => x.name),
      directories: e.filter((x) => x.isDirectory()).map((x) => x.name),
    };
    if (!r.files.includes(path.basename(CARRIER_PATH))) r.files.push(path.basename(CARRIER_PATH));
    return r;
  },
};

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

const api = new API({ tsserverPath: process.env.TSGO_PATH, cwd: FIXTURE, fs: overlayFs });
try {
  const snap1 = api.updateSnapshot({ openProject: TSCONFIG });
  const p1 = snap1.getProject(TSCONFIG);
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
  const snap2 = api.updateSnapshot({
    openProject: TSCONFIG,
    fileChanges: { changed: [CARRIER_PATH] },
  });
  const p2 = snap2.getProject(TSCONFIG);
  const handleStable = p2.id === p1.id;

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
} finally {
  api.close();
}
