#!/usr/bin/env node
// Thin CLI over the TypeScript-observation validator
// (`src/typescript-observe.mjs`). It adds NO validation semantics of its own:
// it reads an artifact set, hands it to `observeTypeScript` unchanged, and
// prints the resulting observation record as JSON on stdout.
//
// Usage:
//   node bin/observe-typescript.mjs --input <file.json>
//
// where the input file is
//   { "frameworkDomain": "vue" | "svelte" | "workspace" | null,
//     "checkDeclarationFiles": true | false,
//     "artifacts": [{ "fileName": "/a.d.ts", "code": "…" }] }
//
// `frameworkDomain` names the realized pinned framework closure the artifacts'
// module references are resolved against; omit it (or pass null) only for
// artifacts that reference no external module. A module reference that does not
// resolve REFUSES the observation — it is never degraded to `any`.
//
// Artifact file names must be ROOTED ("/component.d.ts") so relative imports
// between artifacts resolve unambiguously, exactly as `observeTypeScript`
// documents; the names in the printed record are the ones passed in.
//
// Exit codes: 0 = an observation was produced (which may itself contain
// diagnostics — that is a RESULT, not a failure); 2 = the invocation was
// malformed; 3 = the observation was REFUSED (unresolved module references),
// with the refusal printed as JSON on stdout so a caller can report it.

import { readFileSync } from "node:fs";

import {
  ModuleResolutionError,
  WorkspaceDomainError,
  observeTypeScript,
} from "../src/typescript-observe.mjs";

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

const args = process.argv.slice(2);
let inputPath = null;
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === "--input") {
    inputPath = args[i + 1] ?? null;
    i += 1;
  } else {
    fail(`unknown argument: ${args[i]}`);
  }
}
if (inputPath === null) fail("missing --input <file.json>");

let payload;
try {
  payload = JSON.parse(readFileSync(inputPath, "utf8"));
} catch (error) {
  fail(`cannot read ${inputPath}: ${String(error?.message ?? error)}`);
}

const artifacts = payload?.artifacts;
if (!Array.isArray(artifacts) || artifacts.length === 0) {
  fail("the input carries no `artifacts` array");
}
for (const artifact of artifacts) {
  if (typeof artifact?.fileName !== "string" || typeof artifact?.code !== "string") {
    fail("every artifact needs a string `fileName` and a string `code`");
  }
  if (!artifact.fileName.startsWith("/")) {
    fail(`artifact file names must be rooted, got ${JSON.stringify(artifact.fileName)}`);
  }
}
const frameworkDomain = payload?.frameworkDomain ?? null;
if (frameworkDomain !== null && typeof frameworkDomain !== "string") {
  fail("`frameworkDomain` must be a string or null");
}

try {
  process.stdout.write(
    JSON.stringify(
      observeTypeScript(artifacts, {
        frameworkDomain,
        checkDeclarationFiles: payload?.checkDeclarationFiles === true,
      }),
    ),
  );
} catch (error) {
  if (error instanceof ModuleResolutionError) {
    process.stdout.write(
      JSON.stringify({ refused: "module-resolution", unresolved: error.unresolved }),
    );
    process.exit(3);
  }
  if (error instanceof WorkspaceDomainError) {
    process.stdout.write(JSON.stringify({ refused: "workspace-domain", missing: error.missing }));
    process.exit(3);
  }
  throw error;
}
