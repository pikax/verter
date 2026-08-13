// Golden provenance identity (conformance-goldens.md "Golden provenance").
//
// A golden record binds, beyond the oracle domain and fixture:
//  - the generator's GIT identity (repo commit + tree at generation time,
//    plus whether any generation-relevant path was dirty) — traceability of
//    which committed code produced the set;
//  - a digest over the COMPLETE generation implementation — the generator
//    entry script plus every local module it transitively imports — so
//    changing ANY module on the generation path (normalizer, oracle
//    invokers, oracle-install gate, golden store, …) changes the recorded
//    identity and a stale committed set fails --check until regenerated
//    (the previous script-only digest missed every non-entry module);
//  - the exact REALIZED-closure digest of the isolated oracle installation
//    the compilers loaded from (oracle-install.mjs) — the physical
//    enumeration digest, not the lock's claim.
//
// Comparison semantics at --check: implementation and closure digests are
// content-bound (identical for identical trees) and compare STRICTLY —
// they are the binding force. The git commit/tree/dirty fields are
// generation-TIME identity: an honest regeneration at a later commit of an
// unchanged implementation must not read as drift, so --check normalizes
// them out of the byte comparison and instead (a) validates their presence
// and shape on every committed record (validateRecordProvenance), and
// (b) relies on the content-address + manifest digest to catch any
// mutation of the committed bytes.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";

import { HARNESS_ROOT, REPO_ROOT, EVIDENCE_ROOT } from "./paths.mjs";

function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function git(args) {
  return execFileSync("git", args, { cwd: HARNESS_ROOT, encoding: "utf8" }).trim();
}

/**
 * The generation-relevant input paths for the dirty check: the harness
 * implementation and fixtures plus the committed oracle evidence. The
 * goldens directory is deliberately EXCLUDED — it is the generation
 * OUTPUT, which a just-completed regeneration legitimately dirties.
 */
function generationInputPaths() {
  const paths = [
    path.join(HARNESS_ROOT, "bin"),
    path.join(HARNESS_ROOT, "src"),
    path.join(HARNESS_ROOT, "fixtures"),
    path.join(HARNESS_ROOT, "package.json"),
  ];
  // A drift-refusal self-test may point BF2_EVIDENCE_ROOT outside the
  // repository; `git status -- <path>` refuses out-of-repo paths, so the
  // evidence tree enters the dirty check only when it is repo-contained.
  if (!path.relative(REPO_ROOT, EVIDENCE_ROOT).startsWith("..")) {
    paths.push(path.join(EVIDENCE_ROOT, "oracles"));
  }
  return paths;
}

/** @returns {{ commit: string, tree: string, worktreeDirty: boolean }} */
export function generatorGitIdentity() {
  const commit = git(["rev-parse", "HEAD"]);
  const tree = git(["rev-parse", "HEAD^{tree}"]);
  const status = git(["status", "--porcelain", "--", ...generationInputPaths()]);
  return { commit, tree, worktreeDirty: status.length > 0 };
}

// Matches static and dynamic relative-module imports of local .mjs files.
const RELATIVE_IMPORT_RE = /(?:from|import\()\s*"(\.{1,2}\/[^"]+\.mjs)"/g;

/** Comments never contribute import edges (an example path in prose must not). */
function stripComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^[ \t]*\/\/.*$/gm, "");
}

/**
 * The COMPLETE generation implementation: the entry file plus the
 * transitive closure of its relative-module imports.
 *
 * @param {string} entryPath absolute path of the generator entry script
 * @returns {string[]} absolute file paths, sorted by repo-relative name
 */
export function generationImplementationFiles(entryPath) {
  const seen = new Set();
  const queue = [path.resolve(entryPath)];
  while (queue.length > 0) {
    const file = queue.pop();
    if (seen.has(file)) continue;
    seen.add(file);
    const source = stripComments(readFileSync(file, "utf8"));
    for (const match of source.matchAll(RELATIVE_IMPORT_RE)) {
      queue.push(path.resolve(path.dirname(file), match[1]));
    }
  }
  return [...seen].sort((a, b) =>
    path.relative(HARNESS_ROOT, a).localeCompare(path.relative(HARNESS_ROOT, b)),
  );
}

/**
 * Digest over the complete generation implementation: each file's
 * repo-relative path AND content (line-ending-normalized for
 * Cross-Platform Portability) enter the hash in sorted order.
 */
export function generationImplementationSha256(entryPath) {
  const hash = createHash("sha256");
  for (const file of generationImplementationFiles(entryPath)) {
    hash.update(path.relative(HARNESS_ROOT, file).split(path.sep).join("/"), "utf8");
    hash.update("\0", "utf8");
    hash.update(readFileSync(file, "utf8").replace(/\r\n/g, "\n"), "utf8");
    hash.update("\0", "utf8");
  }
  return hash.digest("hex");
}

export class ProvenanceValidationError extends Error {
  constructor(message, details) {
    super(message);
    this.name = "ProvenanceValidationError";
    this.details = details;
  }
}

const HEX40 = /^[0-9a-f]{40}$/;
const HEX64 = /^[0-9a-f]{64}$/;

/**
 * Validates a record's provenance binding: every bound field present and
 * well-formed. Runs on every record at GENERATION (before publish — a
 * generator that stops producing a field refuses to publish) and on every
 * COMMITTED record at --check (a committed set missing a field fails the
 * check regardless of the byte comparison).
 */
export function validateRecordProvenance(name, record) {
  const problems = [];
  const need = (condition, field, why) => {
    if (!condition) problems.push(`${field}: ${why}`);
  };
  need(record.schemaVersion === 3, "schemaVersion", `expected 3, got ${record.schemaVersion}`);
  need(typeof record.generator === "object" && record.generator !== null, "generator", "missing");
  const generator = record.generator ?? {};
  need(
    typeof generator.file === "string" && generator.file.length > 0,
    "generator.file",
    "missing",
  );
  need(HEX64.test(generator.sha256 ?? ""), "generator.sha256", "not a sha-256 hex digest");
  need(HEX40.test(generator.commit ?? ""), "generator.commit", "not a git object id");
  need(HEX40.test(generator.tree ?? ""), "generator.tree", "not a git object id");
  need(typeof generator.worktreeDirty === "boolean", "generator.worktreeDirty", "missing");
  need(
    HEX64.test(generator.implementationSha256 ?? ""),
    "generator.implementationSha256",
    "not a sha-256 hex digest",
  );
  need(
    HEX64.test(record.realizedClosureSha256 ?? ""),
    "realizedClosureSha256",
    "not a sha-256 hex digest",
  );
  need(HEX64.test(record.packageLockSha256 ?? ""), "packageLockSha256", "not a sha-256 hex digest");
  need(HEX64.test(record.fixture?.sha256 ?? ""), "fixture.sha256", "not a sha-256 hex digest");
  need(
    typeof record.normalizer?.version === "number",
    "normalizer.version",
    "missing behavior version",
  );
  need(
    HEX64.test(record.normalizer?.implementationSha256 ?? ""),
    "normalizer.implementationSha256",
    "not a sha-256 hex digest",
  );
  if (problems.length > 0) {
    throw new ProvenanceValidationError(
      `golden ${name}: provenance binding invalid — ${problems.join("; ")}`,
      { name, problems },
    );
  }
}

/**
 * The projection compared byte-for-byte at --check: generation-TIME git
 * identity fields are normalized to null on BOTH sides (see the module
 * header for why); every other field — including the content-bound
 * implementation and realized-closure digests — compares strictly.
 */
export function checkComparableRecord(record) {
  return {
    ...record,
    generator: {
      ...record.generator,
      commit: null,
      tree: null,
      worktreeDirty: null,
    },
  };
}

export { sha256 as provenanceSha256 };
