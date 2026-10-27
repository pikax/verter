// JS PARITY ORACLE harness.
//
// Runs a fixed set of tsgo `--api` operations through the OFFICIAL JS client
// (`typescript/sync`) against a caller-provided fixture project, and prints the
// results as JSON on stdout. The Rust parity test runs the SAME ops through the
// Rust client and asserts the two result sets are IDENTICAL — this is the
// primary automated check that the hand-written Rust wire matches the official
// client's behavior.
//
// The engine source package is PARAMETERIZED via TS7_SOURCE (and the binary via
// TSGO_PATH): the pinned `typescript` package. Never hardcoded.
//
// Invocation (the Rust test sets these):
//   TS7_SOURCE=<pkg> TSGO_PATH=<exe> NM_BASE=<repo-root> \
//     node parity-oracle.mjs <fixtureDir> <tsconfig> <carrierPath> <carrierContent-b64>
//
// Output (stdout): a single JSON object — see the `result` fields below.
//
// Disposal: the API, every Snapshot it hands out (and the Projects a Snapshot
// owns) are Disposable. They are declared with `using`, so each is disposed
// exactly once on every exit path, in reverse order (the snapshot before the
// API that owns its connection), and nothing touches a disposed object.

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

// Resolve the OFFICIAL sync API and the server-FS sentinels through the PUBLIC
// `<pkg>/sync` and `<pkg>/fs` exports, parameterized over the source package.
const require = createRequire(import.meta.url);
const searchPaths = (process.env.NM_BASE || "").split(path.delimiter).filter(Boolean);
const opts = { paths: searchPaths.length ? searchPaths : undefined };
const sourcePkg = process.env.TS7_SOURCE || "typescript";
let syncApiPath;
let fsApiPath;
try {
  syncApiPath = require.resolve(`${sourcePkg}/sync`, opts);
  fsApiPath = require.resolve(`${sourcePkg}/fs`, opts);
} catch (e) {
  console.error(`could not resolve the TypeScript 7 sync API from ${sourcePkg}: ${e.message}`);
  process.exit(3);
}
const { API } = await import(pathToFileURL(syncApiPath).href);
const { serverFS } = await import(pathToFileURL(fsApiPath).href);

// The off-disk carrier is served purely through the FS overlay (never on disk),
// merged with the real directory entries — exactly the Rust snapshot's contract.
// Every other path is answered by the engine from the host filesystem
// (`serverFS.useOS`), which is what the Rust actor sends for a path the overlay
// does not know.
function makeFs() {
  const carrierNorm = CARRIER;
  return {
    readFile(fileName) {
      return norm(fileName) === carrierNorm ? carrierContent : serverFS.useOS;
    },
    fileExists(fileName) {
      return norm(fileName) === carrierNorm ? true : serverFS.useOS;
    },
    directoryExists: serverFS.useOS,
    getAccessibleEntries(dirName) {
      if (norm(dirName) !== SRC_DIR) return serverFS.useOS;
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
    realpath: serverFS.useOS,
    stat: serverFS.useOS,
    writeFile: serverFS.useOS,
    removeFile: serverFS.useOS,
  };
}

const diagSummary = (diags) =>
  diags
    .map((d) => ({ code: d.code, pos: d.pos, end: d.end }))
    .sort((a, b) => a.code - b.code || a.pos - b.pos);

function run() {
  const result = {};
  using api = new API({ tsserverPath: process.env.TSGO_PATH, cwd: fixtureDir, fs: makeFs() });
  using snap = api.createSnapshot({ openProjects: [TSCONFIG] });
  const project = snap.getConfiguredProject(TSCONFIG);
  result.projectOpened = !!project;
  result.carrierInRootFiles = project
    ? project.parsedCommandLine.fileNames.some((f) => norm(f) === CARRIER)
    : false;

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
  return result;
}

let result;
try {
  result = run();
} catch (e) {
  result = { __error: String(e && e.stack ? e.stack : e) };
}
process.stdout.write(JSON.stringify(result));
