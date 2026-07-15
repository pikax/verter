// JS PARITY ORACLE harness.
//
// Runs a fixed set of tsgo `--api` operations through the OFFICIAL JS client
// (`<pkg>/unstable/sync`) against a caller-provided fixture project, and prints
// the results as JSON on stdout. The Rust parity test runs the SAME ops through
// the Rust client and asserts the two result sets are IDENTICAL — this is the
// primary automated check that the hand-written Rust wire matches the official
// client's behavior.
//
// The engine source package is PARAMETERIZED via TS7_SOURCE (and the binary via
// TSGO_PATH): the rc `typescript` (7.x line) package. Never hardcoded.
//
// Invocation (the Rust test sets these):
//   TS7_SOURCE=<pkg> TSGO_PATH=<exe> NM_BASE=<repo-root> \
//     node parity-oracle.mjs <fixtureDir> <tsconfig> <carrierPath> <carrierContent-b64>
//
// Output (stdout): a single JSON object — see `buildResult` below.

import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import path from "node:path";
import fs from "node:fs";

const [, , fixtureDir, tsconfigArg, carrierPathArg, carrierB64] = process.argv;
if (!fixtureDir || !tsconfigArg || !carrierPathArg) {
  console.error(
    "usage: parity-oracle.mjs <fixtureDir> <tsconfig> <carrierPath> <carrierContent-b64>",
  );
  process.exit(2);
}
const carrierContent = carrierB64 ? Buffer.from(carrierB64, "base64").toString("utf8") : "";

const norm = (p) => p.replace(/\\/g, "/");
const TSCONFIG = norm(tsconfigArg);
const CARRIER = norm(carrierPathArg);
const SRC_DIR = norm(path.dirname(carrierPathArg));

// Resolve the OFFICIAL sync API through the PUBLIC `<pkg>/unstable/sync` export,
// parameterized over the source package (matches tools/tsgo-api-gate/harness.mjs).
const require = createRequire(import.meta.url);
const searchPaths = (process.env.NM_BASE || "").split(path.delimiter).filter(Boolean);
const opts = { paths: searchPaths.length ? searchPaths : undefined };
const sourcePkgs = [process.env.TS7_SOURCE, "typescript"].filter(Boolean);
let syncApiPath;
for (const p of sourcePkgs) {
  try {
    syncApiPath = require.resolve(`${p}/unstable/sync`, opts);
    break;
  } catch {
    /* try next */
  }
}
if (!syncApiPath) {
  console.error(`could not resolve a TS>=7 sync API from ${JSON.stringify(sourcePkgs)}`);
  process.exit(3);
}
const { API } = await import(pathToFileURL(syncApiPath).href);

// The off-disk carrier is served purely through the FS overlay (never on disk),
// merged with the real directory entries — exactly the Rust snapshot's contract.
function makeFs() {
  const carrierNorm = CARRIER;
  return {
    readFile(fileName) {
      if (norm(fileName) === carrierNorm) return carrierContent;
      return undefined; // fall through to the real FS
    },
    fileExists(fileName) {
      if (norm(fileName) === carrierNorm) return true;
      return undefined;
    },
    getAccessibleEntries(dirName) {
      if (norm(dirName) !== SRC_DIR) return undefined;
      let real;
      try {
        const dirents = fs.readdirSync(dirName, { withFileTypes: true });
        real = {
          files: dirents.filter((e) => e.isFile()).map((e) => e.name),
          directories: dirents.filter((e) => e.isDirectory()).map((e) => e.name),
        };
      } catch {
        real = { files: [], directories: [] };
      }
      const base = path.basename(carrierPathArg);
      if (!real.files.includes(base)) real.files.push(base);
      return real;
    },
  };
}

const diagSummary = (diags) =>
  diags
    .map((d) => ({ code: d.code, pos: d.pos, end: d.end }))
    .sort((a, b) => a.code - b.code || a.pos - b.pos);

const api = new API({ tsserverPath: process.env.TSGO_PATH, cwd: fixtureDir, fs: makeFs() });

const result = {};
try {
  // updateSnapshot(openProject)
  const snap = api.updateSnapshot({ openProject: TSCONFIG });
  const project = snap.getProject(TSCONFIG);
  result.projectOpened = !!project;
  result.carrierInRootFiles = project ? project.rootFiles.some((f) => norm(f) === CARRIER) : false;

  // getSemanticDiagnostics on the carrier
  const diags = project ? project.program.getSemanticDiagnostics(CARRIER) : [];
  result.semanticDiagnostics = diagSummary(diags);

  // type-at-position: the `x` declaration's type. Find the offset of `x` in the
  // carrier and read the type there.
  const xOff = carrierContent.indexOf("x:");
  if (project && xOff >= 0) {
    const t = project.checker.getTypeAtPosition(CARRIER, xOff);
    result.typeAtX = t ? project.checker.typeToString(t) : null;
  } else {
    result.typeAtX = null;
  }

  snap.dispose?.();
} catch (e) {
  result.__error = String(e && e.stack ? e.stack : e);
} finally {
  try {
    api.close();
  } catch {
    /* ignore */
  }
}

process.stdout.write(JSON.stringify(result));
