// Record the WSP1L real-client capture artifact.
//
// node tests/workspace-responsiveness/WSP1L/capture-real-client.mjs \
//   --lapce <instrumented lapce binary> --volt <volt dir> --server <verter-lsp binary> \
//   [--out products/real-client-capture.v1.json]
//
// Spawns the instrumented reference build with the production volt and the
// native server, drives the recorded script over the WSP1L drive channel,
// and writes the digest-sealed artifact the committed tests verify against.
// Reference-machine tooling only — never part of hermetic CI.

import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import {
  REAL_LAPCE_AUTOMATION_PATH,
  buildDrivenCaptureArtifact,
  correlateServerTraces,
  driveLapceSession,
  writeDrivenCaptureArtifact,
} from "../../../packages/dx-harness/dist/lapce/index.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");

function arg(name, fallback) {
  const at = process.argv.indexOf(`--${name}`);
  return at !== -1 ? process.argv[at + 1] : fallback;
}

const lapceBin = path.resolve(arg("lapce"));
const voltDir = path.resolve(arg("volt"));
const serverBin = path.resolve(arg("server"));
const outPath = path.resolve(
  arg("out", path.join(here, "products", "real-client-capture.v1.json")),
);
const patchRecordedAs = "packages/dx-harness/lapce/instrumented-client/lapce-0.4.6-wsp1l.patch";
const patchPath = path.join(repoRoot, patchRecordedAs);

// Only Windows paths use the backslash as a separator; on POSIX it is a legal
// filename character, so a blanket replacement would corrupt a real path.
const toSlashes = (value) => (process.platform === "win32" ? value.replaceAll("\\", "/") : value);

// Materialize the fixture workspace into a temp dir with the server path bound.
const workspaceDir = mkdtempSync(path.join(os.tmpdir(), "wsp1l-ws-"));
cpSync(path.join(here, "fixtures/ws"), workspaceDir, { recursive: true });
const settingsPath = path.join(workspaceDir, ".lapce", "settings.toml");
const settingsTemplate = readFileSync(`${settingsPath}.template`, "utf8");
mkdirSync(path.dirname(settingsPath), { recursive: true });
writeFileSync(settingsPath, settingsTemplate.replaceAll("<verter-lsp>", toSlashes(serverBin)));

const fixture = path.join(workspaceDir, "Fixture.vue");
const helper = path.join(workspaceDir, "Helper.vue");
const script = [
  // Two opens: the close step then closes the Fixture tab while the Helper
  // editor remains, so the post-close frame is a real presented editor frame.
  { kind: "open", requestEpoch: 1, sourceEpoch: null, uri: helper },
  { kind: "open", requestEpoch: 2, sourceEpoch: null, uri: fixture, line: 4, column: 20 },
  { kind: "navigate", requestEpoch: 3, sourceEpoch: null },
  { kind: "type", requestEpoch: 4, sourceEpoch: null, text: " " },
  { kind: "complete", requestEpoch: 5, sourceEpoch: null },
  { kind: "close", requestEpoch: 6, sourceEpoch: null },
];

try {
  const session = await driveLapceSession({
    lapceBin,
    workspaceDir,
    voltDir,
    script,
    stepTimeoutMs: 180_000,
  });
  const correlated = correlateServerTraces(session);
  const artifact = buildDrivenCaptureArtifact({
    session,
    correlated,
    recordedAs: toSlashes(path.relative(repoRoot, outPath)),
    lapceClientSource:
      "Lapce v0.4.6 (github.com/lapce/lapce tag v0.4.6 source tarball) + the WSP1L instrumented-client patch",
    patchPath,
    patchRecordedAs,
    automationPath: REAL_LAPCE_AUTOMATION_PATH,
  });
  writeDrivenCaptureArtifact(artifact, outPath);
  console.log(
    `recorded ${artifact.session.sessionId}: ${artifact.capturedLines.length} lines -> ${outPath}`,
  );
} finally {
  rmSync(workspaceDir, { recursive: true, force: true });
}
