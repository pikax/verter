#!/usr/bin/env node
// Resolves the host Rust target triple from `rustc -vV`.
//
// Used so every host-artifact build command (NAPI, LSP) passes the SAME
// explicit `--target` instead of some passing it implicitly (napi build)
// and others omitting it (a bare `cargo build`) — see the "one explicit
// host target" rule in
// docs/arch/refactor/rev11/rulings/MAINTAINER-DIRECTIVE-BUILD-LANE-SEPARATION.md.
//
// Node stdlib only, no hardcoded per-OS triple table: the triple comes from
// the toolchain actually installed, so it is correct on any platform/arch
// combination rustc supports, including ones not enumerated anywhere in this
// repo.

import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

let cachedTriple;

/** Resolve (and cache for the process lifetime) the host target triple. */
export function resolveHostTarget() {
  if (cachedTriple) return cachedTriple;
  let output;
  try {
    output = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  } catch (error) {
    throw new Error(
      `Could not run 'rustc -vV' to resolve the host target triple: ${error.message}`,
    );
  }
  const match = /^host:\s*(\S+)/m.exec(output);
  if (!match) {
    throw new Error(`Could not find a 'host:' line in 'rustc -vV' output:\n${output}`);
  }
  cachedTriple = match[1];
  return cachedTriple;
}

// CLI entry point: print the resolved triple. Lets non-Node command
// composition (napi build's `--target`, cargo's `--target`) pick it up
// without spawning a shell for command substitution, which does not
// portably exist across POSIX shells and cmd.exe/PowerShell. Compares
// resolved filesystem paths (not raw URL/argv strings) so this is correct
// on Windows, where `import.meta.url` uses `file:///C:/...` and
// `process.argv[1]` uses backslashes.
const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  try {
    process.stdout.write(resolveHostTarget());
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
  }
}
