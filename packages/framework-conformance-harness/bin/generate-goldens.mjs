#!/usr/bin/env node
/**
 * Generator — official-core conformance goldens (BF2 harness).
 *
 * Runs the PINNED official Vue 3.6.0-rc.3 / Svelte 5.56.8 compilers over
 * every independently-authored fixture under `fixtures/` and writes
 * IMMUTABLE golden JSON records under `goldens/`, each carrying full
 * provenance (source commit/tree, package-lock digest, generator digest,
 * normalized options, environment, raw artifact digest, normalizer
 * version/digest, normalized digest — conformance-goldens.md).
 *
 * This is the ONLY script that ever writes under `goldens/`. Candidate
 * (Verter) output is never an input to this script and can never reach
 * this write path — see src/golden-store.mjs's header comment for why that
 * holds structurally, not just by convention.
 *
 * Usage:
 *   node bin/generate-goldens.mjs           # regenerate all goldens
 *   node bin/generate-goldens.mjs --check   # verify committed == fresh regen
 */

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { VUE_DOMAIN, SVELTE_DOMAIN, EVIDENCE_LOCK_DIGESTS } from "../src/domain-pin.mjs";
import {
  NORMALIZER_VERSION,
  parseModule,
  canonicalize,
  canonicalDigest,
} from "../src/normalize.mjs";
import { writeGoldenFile, readGoldenFile, sha256 } from "../src/golden-store.mjs";
import { HARNESS_ROOT, GOLDENS_ROOT, FIXTURES_ROOT } from "../src/paths.mjs";

const CHECK_MODE = process.argv.includes("--check");
const GENERATOR_SCRIPT_SHA256 = sha256(readFileSync(new URL(import.meta.url), "utf8"));

const VUE_BACKENDS = ["vdom", "vapor", "ssr"];
const SVELTE_TARGETS = ["client", "server"];

/** fixture basename -> whether it compiles under runes mode. */
const SVELTE_FIXTURES = [
  { file: "basic-runes.svelte", runes: true },
  { file: "props-events.svelte", runes: true },
  { file: "legacy-slots.svelte", runes: false },
];

function listVueFixtures() {
  return readdirSync(path.join(FIXTURES_ROOT, "vue"))
    .filter((f) => f.endsWith(".vue"))
    .sort();
}

function buildProvenance({
  framework,
  domain,
  packageLockSha256,
  fixturePath,
  fixtureSource,
  options,
}) {
  return {
    schemaVersion: 1,
    framework,
    domain: {
      upstream: domain.upstream,
      commit: domain.commit,
      tree: domain.tree,
      packageVersion: domain.packageVersion,
    },
    packageLockSha256,
    generator: { file: "bin/generate-goldens.mjs", sha256: GENERATOR_SCRIPT_SHA256 },
    fixture: { path: fixturePath, sha256: sha256(fixtureSource) },
    options,
    environment: { node: process.version, platform: process.platform, arch: process.arch },
  };
}

function finalizeRecord(provenance, artifact) {
  const rawCodeSha256 = artifact.code === null ? null : sha256(artifact.code);
  let normalizedDigest = null;
  let normalizerRenameCount = null;
  if (artifact.code !== null) {
    const ast = parseModule(artifact.code, provenance.fixture.path);
    const canonical = canonicalize(ast);
    normalizedDigest = sha256(canonicalDigest(canonical.tree));
    normalizerRenameCount = canonical.renameCount;
  }
  return {
    ...provenance,
    diagnostics: artifact.diagnostics,
    raw: { codeSha256: rawCodeSha256, mapPresent: artifact.map !== null },
    normalizer: {
      version: NORMALIZER_VERSION,
      normalizedDigestSha256: normalizedDigest,
      normalizerRenameCount,
    },
    code: artifact.code,
    map: artifact.map,
  };
}

function vueGoldens() {
  const records = [];
  for (const file of listVueFixtures()) {
    const fixturePath = `fixtures/vue/${file}`;
    const source = readFileSync(path.join(FIXTURES_ROOT, "vue", file), "utf8");
    const caseName = file.replace(/\.vue$/, "");
    for (const backend of VUE_BACKENDS) {
      for (const sourceMap of [false, true]) {
        for (const isProd of [false, true]) {
          const artifact = compileVueFixture(source, fixturePath, { backend, sourceMap, isProd });
          const provenance = buildProvenance({
            framework: "vue",
            domain: VUE_DOMAIN,
            packageLockSha256: EVIDENCE_LOCK_DIGESTS.vuePackageLockSha256,
            fixturePath,
            fixtureSource: source,
            options: { backend, sourceMap, isProd },
          });
          const record = finalizeRecord(provenance, artifact);
          const outName = `${caseName}__${backend}__map${sourceMap ? 1 : 0}__prod${isProd ? 1 : 0}.json`;
          records.push({ outPath: path.join(GOLDENS_ROOT, "vue", outName), record });
        }
      }
    }
  }
  return records;
}

function svelteGoldens() {
  const records = [];
  for (const { file, runes } of SVELTE_FIXTURES) {
    const fixturePath = `fixtures/svelte/${file}`;
    const source = readFileSync(path.join(FIXTURES_ROOT, "svelte", file), "utf8");
    const caseName = file.replace(/\.svelte$/, "");
    for (const generate of SVELTE_TARGETS) {
      for (const dev of [false, true]) {
        const artifact = compileSvelteFixture(source, fixturePath, {
          generate,
          runes,
          dev,
          sourceMap: true,
        });
        const provenance = buildProvenance({
          framework: "svelte",
          domain: SVELTE_DOMAIN,
          packageLockSha256: EVIDENCE_LOCK_DIGESTS.sveltePackageLockSha256,
          fixturePath,
          fixtureSource: source,
          options: { generate, runes, dev },
        });
        const record = finalizeRecord(provenance, artifact);
        const outName = `${caseName}__${generate}__runes${runes ? 1 : 0}__dev${dev ? 1 : 0}.json`;
        records.push({ outPath: path.join(GOLDENS_ROOT, "svelte", outName), record });
      }
    }
  }
  return records;
}

function main() {
  const all = [...vueGoldens(), ...svelteGoldens()];
  if (all.length === 0) throw new Error("golden generation produced zero records");
  for (const { record } of all) {
    if (record.diagnostics.some((d) => d.kind !== "warning")) {
      throw new Error(
        `fixture ${record.fixture.path} (${JSON.stringify(record.options)}) produced an unexpected diagnostic: ${JSON.stringify(record.diagnostics)}`,
      );
    }
  }

  if (CHECK_MODE) {
    let drift = 0;
    for (const { outPath, record } of all) {
      let committed;
      try {
        committed = readGoldenFile(outPath);
      } catch {
        console.error(`MISSING: ${path.relative(HARNESS_ROOT, outPath)}`);
        drift += 1;
        continue;
      }
      const fresh = JSON.parse(JSON.stringify(record));
      if (JSON.stringify(committed) !== JSON.stringify(fresh)) {
        console.error(`DRIFT: ${path.relative(HARNESS_ROOT, outPath)}`);
        drift += 1;
      }
    }
    if (drift > 0) {
      console.error(`${drift} golden(s) drifted or missing. Run without --check to regenerate.`);
      process.exit(1);
    }
    console.log(`OK: ${all.length} goldens match a fresh regeneration.`);
    return;
  }

  for (const { outPath, record } of all) writeGoldenFile(outPath, record);
  console.log(JSON.stringify({ goldens_written: all.length }));
}

main();
