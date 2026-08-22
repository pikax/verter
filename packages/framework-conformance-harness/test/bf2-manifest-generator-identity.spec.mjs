// Regression coverage for two defects found integrating the Svelte
// case-identity rebase onto trunk's separate Vue rc.3 -> rc.5 repin:
//
// 1. EXPECTED_VUE in generate-official-case-manifests.mjs /
//    verify-b2-parse-facets.mjs was once wired to the live VUE_DOMAIN.commit
//    pin (domain-pin.mjs). That is harmless only by coincidence when
//    VUE_DOMAIN happens to still be rc.3 -- it silently breaks the moment
//    VUE_DOMAIN moves, because this BF1/BF2 evidence package is documented
//    (version-domain.md) as frozen at its OWN separate rc.3 pin,
//    independent of the live runtime-oracle domain.
// 2. performance-gates.toml's BF2_VUE_ORACLE_MANIFEST_GENERATE /
//    BF2_SVELTE_ORACLE_MANIFEST_GENERATE cells pin the generator script's
//    exact git-blob hash as part of their measured workload identity. Any
//    future edit to that script that does not re-bind those citations
//    leaves the recorded identity silently pointing at stale content.
//
// Both are proven here against LIVE content, not a hardcoded expectation of
// "correct" -- test 1 is falsifiable right now because VUE_DOMAIN.commit on
// this tree is already rc.5, and test 2 recomputes the generator's actual
// git-blob hash and SHA-256 request digests from the files on disk.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { VUE_DOMAIN } from "../src/domain-pin.mjs";
import { EXPECTED_VUE } from "../../../docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs";

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(TEST_DIR, "..", "..", "..");
const GENERATOR_REL_PATH =
  "docs/arch/refactor/rev11/evidence/framework-conformance/generate-official-case-manifests.mjs";
const GENERATOR_PATH = join(REPO_ROOT, GENERATOR_REL_PATH);
const GATES_PATH = join(REPO_ROOT, "performance-gates.toml");

// The documented frozen BF1/BF2 Vue pin (version-domain.md), independent of
// domain-pin.mjs's live VUE_DOMAIN re-resolutions.
const FROZEN_BF2_VUE_COMMIT = "3adb225775c9b28223a56e07f7a2f874b6fbb138";

function gitBlobHash(content) {
  const buf = Buffer.isBuffer(content) ? content : Buffer.from(content);
  const header = Buffer.from(`blob ${buf.length}\0`);
  return createHash("sha1")
    .update(Buffer.concat([header, buf]))
    .digest("hex");
}

describe("BF1/BF2 official-case-manifest evidence-package identity", () => {
  it("EXPECTED_VUE stays independently frozen, never aliased to the live VUE_DOMAIN pin", () => {
    // Proves this is a real, live discriminator: VUE_DOMAIN has already
    // moved off the frozen commit on this tree, so a re-introduced
    // `EXPECTED_VUE = VUE_DOMAIN.commit` alias fails this assertion for
    // real, not only in a hypothetical future bump.
    expect(VUE_DOMAIN.commit).not.toBe(FROZEN_BF2_VUE_COMMIT);
    expect(EXPECTED_VUE).toBe(FROZEN_BF2_VUE_COMMIT);
    expect(EXPECTED_VUE).not.toBe(VUE_DOMAIN.commit);
  });

  it("performance-gates.toml's BF2 manifest-generator cells cite the generator's live git-blob hash", () => {
    const generatorBlob = gitBlobHash(readFileSync(GENERATOR_PATH));
    const gates = readFileSync(GATES_PATH, "utf8");

    const corpusFingerprintBlobs = [
      ...gates.matchAll(
        /corpus_fingerprint = "git-blob:([0-9a-f]{40}) \(docs\/arch\/refactor\/rev11\/evidence\/framework-conformance\/generate-official-case-manifests\.mjs\)/g,
      ),
    ].map((m) => m[1]);
    const commentGeneratorBlobs = [
      ...gates.matchAll(/#\s+verter-rev11-cell:\S*generator=git-blob:([0-9a-f]{40})/g),
    ].map((m) => m[1]);

    // Both BF2 cells (Vue + Svelte) carry both citation forms.
    expect(corpusFingerprintBlobs).toHaveLength(2);
    expect(commentGeneratorBlobs).toHaveLength(2);
    for (const blob of [...corpusFingerprintBlobs, ...commentGeneratorBlobs]) {
      expect(blob).toBe(generatorBlob);
    }
  });

  it("performance-gates.toml's BF2 manifest-generator request digests match a fresh SHA-256 of their pinned invocation string", () => {
    const gates = readFileSync(GATES_PATH, "utf8");

    // Each cell pairs one `#   verter-rev11-cell:...` comment line with the
    // `normalized_product_request_digest` line that immediately follows it
    // (possibly across a blank comment separator) -- recompute and compare.
    const pairs = [
      ...gates.matchAll(
        /^#\s+(verter-rev11-cell:.*)$\r?\n(?:#.*\r?\n)*normalized_product_request_digest = "([0-9a-f]{64})"/gm,
      ),
    ];

    expect(pairs).toHaveLength(2);
    for (const [, invocationLine, recordedDigest] of pairs) {
      const freshDigest = createHash("sha256").update(invocationLine, "utf8").digest("hex");
      expect(freshDigest).toBe(recordedDigest);
    }
  });
});
