/**
 * ONE attempt at acquiring a VS Code host into {@link VSCODE_TEST_CACHE_PATH}.
 *
 *   node out-test/e2e/acquireVscode.js <version>
 *
 * On success the last stdout line is `VERTER_VSCODE_EXECUTABLE=<path>` and the
 * exit code is 0; on failure the exit code is non-zero. A host that is already
 * complete in the cache (test-electron's `is-complete` marker) is reported
 * without touching the network, because a pinned version with a local install
 * skips test-electron's version check too.
 *
 * Two callers, one resolution: the e2e runners spawn this through
 * `acquireVscodeInChildProcess` (sharedLaunch.ts), and CI runs it directly in
 * a retry loop before the test step so the runners find the host cached. Each
 * caller owns the retry; this script makes one attempt.
 *
 * It is a separate process because test-electron 2.5.x leaks a rejected
 * checksum promise when the archive stream is reset mid-download. That leaked
 * rejection is only an echo of the stream error test-electron has already
 * caught and is retrying, so here it is reported and otherwise ignored: the
 * library's own retry continues, and `is-complete` is written only after a
 * full download, extraction and checksum, so nothing half-done is ever
 * reported as installed. Anything else that goes wrong still ends this process
 * non-zero, and the caller's retry handles it.
 */
import { downloadAndUnzipVSCode } from "@vscode/test-electron";

import { VSCODE_EXECUTABLE_MARKER, VSCODE_TEST_CACHE_PATH } from "./sharedLaunch";

async function main(): Promise<void> {
  const version = process.argv[2];
  if (!version) {
    console.error("usage: node acquireVscode.js <version>");
    process.exit(2);
  }

  process.on("unhandledRejection", (reason) => {
    console.warn(
      `acquireVscode: ignoring a rejection test-electron left unhandled during its download ` +
        `retry (${reason instanceof Error ? reason.message : String(reason)})`,
    );
  });

  const executable = await downloadAndUnzipVSCode({
    version,
    cachePath: VSCODE_TEST_CACHE_PATH,
  });
  console.log(`${VSCODE_EXECUTABLE_MARKER}${executable}`);
}

main().catch((error: unknown) => {
  console.error(
    `acquireVscode: could not download VS Code ${process.argv[2]} into ${VSCODE_TEST_CACHE_PATH}:`,
    error,
  );
  process.exit(1);
});
