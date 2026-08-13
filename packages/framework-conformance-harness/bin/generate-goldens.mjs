#!/usr/bin/env node
/**
 * Generator — official-core conformance goldens (BF2 harness).
 *
 * Runs the PINNED official Vue 3.6.0-rc.3 / Svelte 5.56.8 compilers over
 * every independently-authored fixture under `fixtures/` and publishes
 * IMMUTABLE golden records under `goldens/` through ONE atomic commit point
 * (content-addressed records + an atomically-renamed manifest — see
 * src/golden-store.mjs). Each record carries full provenance: source
 * commit/tree, package-lock digest, generator digest, normalizer
 * version + implementation digest, normalized options, environment, raw and
 * normalized artifact digests (conformance-goldens.md).
 *
 * This is the ONLY script that ever writes under `goldens/`. Candidate
 * (Verter) output is never an input to this script and can never reach
 * this write path — see src/golden-store.mjs's header comment for why that
 * holds structurally, not just by convention.
 *
 * Package/closure pins are asserted before the first compiler invocation
 * (assertVuePinned/assertSveltePinned run at each oracle's entry, and both
 * include the full transitive-closure layers of src/package-pin.mjs).
 *
 * Usage:
 *   node bin/generate-goldens.mjs           # regenerate + publish atomically
 *   node bin/generate-goldens.mjs --check   # verify committed == fresh regen
 */

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
import {
  publishGoldenSet,
  readGoldenSet,
  serializeGoldenRecord,
  sha256,
} from "../src/golden-store.mjs";
import {
  checkComparableRecord,
  generationImplementationSha256,
  generatorGitIdentity,
  validateRecordProvenance,
} from "../src/provenance.mjs";
import { ensureOracleDomain } from "../src/oracle-install.mjs";
import { GOLDENS_ROOT, FIXTURES_ROOT } from "../src/paths.mjs";
import { fileURLToPath } from "node:url";

const CHECK_MODE = process.argv.includes("--check");
const GENERATOR_ENTRY = fileURLToPath(import.meta.url);
const GENERATOR_SCRIPT_SHA256 = sha256(readFileSync(GENERATOR_ENTRY, "utf8"));
// The COMPLETE generation implementation — this entry plus every local
// module it transitively imports (provenance.mjs) — and the generator's
// git identity at generation time.
const GENERATION_IMPLEMENTATION_SHA256 = generationImplementationSha256(GENERATOR_ENTRY);
const GENERATOR_GIT = generatorGitIdentity();
const NORMALIZER_IMPLEMENTATION_SHA256 = sha256(
  readFileSync(new URL("../src/normalize.mjs", import.meta.url), "utf8"),
);
// The exact realized-closure digests of the isolated installations the
// oracle compilers/runtimes actually load from (validated by the
// oracle-install gate before any compile below).
const REALIZED_CLOSURES = {
  vue: ensureOracleDomain("vue").realizedClosureSha256,
  svelte: ensureOracleDomain("svelte").realizedClosureSha256,
};

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
    schemaVersion: 3,
    framework,
    domain: {
      upstream: domain.upstream,
      commit: domain.commit,
      tree: domain.tree,
      packageVersion: domain.packageVersion,
    },
    packageLockSha256,
    realizedClosureSha256: REALIZED_CLOSURES[framework],
    generator: {
      file: "bin/generate-goldens.mjs",
      sha256: GENERATOR_SCRIPT_SHA256,
      commit: GENERATOR_GIT.commit,
      tree: GENERATOR_GIT.tree,
      worktreeDirty: GENERATOR_GIT.worktreeDirty,
      implementationSha256: GENERATION_IMPLEMENTATION_SHA256,
    },
    fixture: { path: fixturePath, sha256: sha256(fixtureSource) },
    options,
    environment: { node: process.version, platform: process.platform, arch: process.arch },
  };
}

function finalizeRecord(provenance, artifact) {
  const rawCodeSha256 = artifact.code === null ? null : sha256(artifact.code);
  let normalizedDigest = null;
  if (artifact.code !== null) {
    const ast = parseModule(artifact.code, provenance.fixture.path);
    const canonical = canonicalize(ast);
    normalizedDigest = sha256(canonicalDigest(canonical.tree));
  }
  return {
    ...provenance,
    diagnostics: artifact.diagnostics,
    raw: { codeSha256: rawCodeSha256, mapPresent: artifact.map !== null },
    normalizer: {
      version: NORMALIZER_VERSION,
      implementationSha256: NORMALIZER_IMPLEMENTATION_SHA256,
      normalizedDigestSha256: normalizedDigest,
    },
    code: artifact.code,
    map: artifact.map,
  };
}

function vueGoldens() {
  const entries = [];
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
          const name = `vue/${caseName}__${backend}__map${sourceMap ? 1 : 0}__prod${isProd ? 1 : 0}`;
          entries.push({ name, record });
        }
      }
    }
  }
  return entries;
}

function svelteGoldens() {
  const entries = [];
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
        const name = `svelte/${caseName}__${generate}__runes${runes ? 1 : 0}__dev${dev ? 1 : 0}`;
        entries.push({ name, record });
      }
    }
  }
  return entries;
}

function main() {
  const all = [...vueGoldens(), ...svelteGoldens()];
  if (all.length === 0) throw new Error("golden generation produced zero records");
  for (const { name, record } of all) {
    if (record.diagnostics.some((d) => d.kind !== "warning")) {
      throw new Error(
        `fixture ${record.fixture.path} (${JSON.stringify(record.options)}) produced an unexpected diagnostic: ${JSON.stringify(record.diagnostics)}`,
      );
    }
    // A generator that stops producing a bound provenance field refuses to
    // publish (and refuses to treat its own output as a valid check arm).
    validateRecordProvenance(name, record);
  }

  if (CHECK_MODE) {
    let committed;
    try {
      committed = readGoldenSet(GOLDENS_ROOT);
    } catch (error) {
      console.error(`cannot read committed golden set: ${error.message}`);
      process.exit(1);
    }
    let drift = 0;
    for (const { name, record } of all) {
      const committedRecord = committed.get(name);
      if (committedRecord === undefined) {
        console.error(`MISSING: ${name}`);
        drift += 1;
        continue;
      }
      // Committed records must carry the full provenance binding — a
      // missing or malformed bound field fails the check outright.
      try {
        validateRecordProvenance(name, committedRecord);
      } catch (error) {
        console.error(`PROVENANCE: ${error.message}`);
        drift += 1;
        continue;
      }
      // Byte comparison over the check projection: generation-time git
      // identity normalized on both sides; the content-bound
      // implementation + realized-closure digests compare strictly.
      if (
        serializeGoldenRecord(checkComparableRecord(committedRecord)) !==
        serializeGoldenRecord(checkComparableRecord(record))
      ) {
        console.error(`DRIFT: ${name}`);
        drift += 1;
      }
    }
    for (const name of committed.keys()) {
      if (!all.some((entry) => entry.name === name)) {
        console.error(`STALE: ${name} (committed but no longer generated)`);
        drift += 1;
      }
    }
    if (drift > 0) {
      console.error(
        `${drift} golden(s) drifted, missing, or stale. Run without --check to regenerate.`,
      );
      process.exit(1);
    }
    console.log(`OK: ${all.length} goldens match a fresh regeneration.`);
    return;
  }

  const { published } = publishGoldenSet(GOLDENS_ROOT, all, {
    generator: {
      file: "bin/generate-goldens.mjs",
      sha256: GENERATOR_SCRIPT_SHA256,
      commit: GENERATOR_GIT.commit,
      tree: GENERATOR_GIT.tree,
      worktreeDirty: GENERATOR_GIT.worktreeDirty,
      implementationSha256: GENERATION_IMPLEMENTATION_SHA256,
    },
    realizedClosures: REALIZED_CLOSURES,
    normalizer: {
      version: NORMALIZER_VERSION,
      implementationSha256: NORMALIZER_IMPLEMENTATION_SHA256,
    },
  });
  console.log(JSON.stringify({ goldens_published: published }));
}

main();
