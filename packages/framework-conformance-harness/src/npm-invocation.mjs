// How this harness invokes npm: a (file, prefixArgs, execOptions) triple for
// `execFileSync` callers, plus a sync runner.
//
// Node's CVE-2024-27980 mitigation (Node >= 18.20/20.12/21.7) made spawning
// a `.cmd`/`.bat` shim such as `npm.cmd` throw `EINVAL` unless a shell is
// explicitly requested — so the historical `execFileSync("npm.cmd", ...)`
// fails before npm ever runs on every current Windows Node. Running npm's
// own CLI entry through the running Node executable needs no shim and no
// shell at all, which is the primary spelling here. The shell fallback (for
// a Node whose bundled npm is not beside the executable) pre-quotes every
// argument, because `execFileSync` with `shell: true` performs no quoting
// of its own.

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";

/** @returns {{ file: string, prefix: string[], execOptions: { shell?: boolean } }} */
export function npmInvocation() {
  if (process.platform !== "win32") return { file: "npm", prefix: [], execOptions: {} };
  const cli = path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js");
  if (existsSync(cli)) return { file: process.execPath, prefix: [cli], execOptions: {} };
  return { file: "npm.cmd", prefix: [], execOptions: { shell: true } };
}

/**
 * Runs `npm <args>` synchronously. Arguments pass through unchanged in the
 * no-shell spelling; the shell spelling quotes anything containing
 * whitespace so paths with spaces survive the round trip.
 */
export function execNpmSync(args, options = {}) {
  const { file, prefix, execOptions } = npmInvocation();
  if (execOptions.shell) {
    const quoted = [file, ...prefix, ...args].map((part) => (/\s/.test(part) ? `"${part}"` : part));
    return execFileSync(quoted[0], quoted.slice(1), { ...execOptions, ...options });
  }
  return execFileSync(file, [...prefix, ...args], { ...execOptions, ...options });
}
