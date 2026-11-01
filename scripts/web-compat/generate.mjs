#!/usr/bin/env node
// Explicit generation of the pinned compatibility snapshot. This is the only
// component that touches the network, and only the official artifacts pinned
// below; the session and every semantic query read the committed bytes.
//
//   node scripts/web-compat/generate.mjs            regenerate test-corpora/web-compat
//   node scripts/web-compat/generate.mjs --check    regenerate in memory, compare bytes
//   node scripts/web-compat/generate.mjs --artifacts <dir>
//                                                   read the pinned artifacts from <dir>
//                                                   (integrity is still verified)
//   node scripts/web-compat/generate.mjs --fixture  regenerate the offline test fixture

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { gunzipSync } from "node:zlib";
import {
  buildSnapshot,
  renderNotice,
  sha256,
  SnapshotGenerationError,
} from "./snapshot-builder.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
export const CORPUS_DIR = join(SCRIPT_DIR, "..", "..", "test-corpora", "web-compat");

export const PINS = Object.freeze({
  bcd: Object.freeze({
    name: "browser-compat-data",
    package: "@mdn/browser-compat-data",
    release: "5.7.6",
    artifactUrl:
      "https://registry.npmjs.org/@mdn/browser-compat-data/-/browser-compat-data-5.7.6.tgz",
    artifactFile: "browser-compat-data-5.7.6.tgz",
    integrity:
      "sha512-7xdrMX0Wk7grrTZQwAoy1GkvPMFoizStUoL+VmtUkAxegbCCec+3FKwOM6yc/uGU5+BEczQHXAlWiqvM8JeENg==",
    license: "CC0-1.0",
    licenseEntry: "package/LICENSE",
    licenseFile: "LICENSE.browser-compat-data",
    attribution: "MDN browser-compat-data",
    schemaUrl:
      "https://raw.githubusercontent.com/mdn/browser-compat-data/v5.7.6/schemas/compat-data.schema.json",
    schemaFile: "compat-data.schema.json",
    schemaDigest: "sha256:e5faa5c194391a0cc2cbf6e9e78bf2e2de10cc5762337c06ff5c7da13d9de97d",
  }),
  webFeatures: Object.freeze({
    name: "web-features",
    package: "web-features",
    release: "3.44.0",
    artifactUrl: "https://registry.npmjs.org/web-features/-/web-features-3.44.0.tgz",
    artifactFile: "web-features-3.44.0.tgz",
    integrity:
      "sha512-K/+YJI7uVE/b4G8SU67/F/4j81/ajJ6MfBCZyuQaqyuPlVXF8cd6dLqBznXIWJI9nwnvuY2dapKK08OHzTjYww==",
    license: "Apache-2.0",
    licenseEntry: "package/LICENSE.txt",
    licenseFile: "LICENSE.web-features.txt",
    attribution: "Web Platform DX web-features",
    schemaEntry: "package/data.schema.json",
    schemaFile: "web-features.data.schema.json",
  }),
});

const FIXTURE_RELEASE = "fixture-1";
const FIXTURE_RECIPE = "sha256:fixture-recipe";

// A generator or validator change is a recipe change: it must re-mint the snapshot.
const RECIPE_FILES = ["generate.mjs", "json-schema.mjs", "snapshot-builder.mjs"];

export function recipeDigest() {
  const hash = createHash("sha256");
  for (const name of RECIPE_FILES) {
    const bytes = readFileSync(join(SCRIPT_DIR, name));
    hash.update(`${name}\0${bytes.length}\0`);
    hash.update(bytes);
  }
  return `sha256:${hash.digest("hex")}`;
}

export function sri512(bytes) {
  return `sha512-${createHash("sha512").update(bytes).digest("base64")}`;
}

export function verifyIntegrity(label, bytes, expected) {
  const actual = expected.startsWith("sha512-") ? sri512(bytes) : sha256(bytes);
  if (actual !== expected) {
    throw new SnapshotGenerationError(
      "integrity",
      `${label}: expected ${expected}, the supplied bytes are ${actual}`,
    );
  }
}

/** Read named entries from an uncompressed tar; a duplicate named entry is a rejection. */
export function readTarEntries(tarBytes, wanted) {
  const found = new Map();
  let offset = 0;
  while (offset + 512 <= tarBytes.length) {
    const block = tarBytes.subarray(offset, offset + 512);
    if (block.every((byte) => byte === 0)) break;
    const field = (start, length) => {
      const raw = block.subarray(start, start + length);
      const end = raw.indexOf(0);
      return raw.subarray(0, end === -1 ? length : end).toString("utf8");
    };
    const prefix = field(345, 155);
    const name = prefix === "" ? field(0, 100) : `${prefix}/${field(0, 100)}`;
    const size = Number.parseInt(field(124, 12).trim(), 8);
    if (!Number.isSafeInteger(size) || size < 0) {
      throw new SnapshotGenerationError("artifact-layout", `invalid tar entry size for ${name}`);
    }
    const dataStart = offset + 512;
    const dataEnd = dataStart + size;
    if (dataEnd > tarBytes.length) {
      throw new SnapshotGenerationError("artifact-layout", `truncated tar entry ${name}`);
    }
    if (wanted.includes(name)) {
      if (found.has(name)) {
        throw new SnapshotGenerationError("artifact-layout", `duplicate tar entry ${name}`);
      }
      found.set(name, Buffer.from(tarBytes.subarray(dataStart, dataEnd)));
    }
    offset = dataStart + Math.ceil(size / 512) * 512;
  }
  for (const name of wanted) {
    if (!found.has(name)) {
      throw new SnapshotGenerationError("artifact-layout", `missing tar entry ${name}`);
    }
  }
  return found;
}

function assertPackageIdentity(pin, packageJsonBytes) {
  const manifest = JSON.parse(packageJsonBytes.toString("utf8"));
  if (
    manifest.name !== pin.package ||
    manifest.version !== pin.release ||
    manifest.license !== pin.license
  ) {
    throw new SnapshotGenerationError(
      "artifact-identity",
      `${pin.package}@${pin.release} (${pin.license}) does not match its package.json: ` +
        `${manifest.name}@${manifest.version} (${manifest.license})`,
    );
  }
}

function openArtifact(pin, artifactBytes, extraEntries = []) {
  verifyIntegrity(pin.package, artifactBytes, pin.integrity);
  const entries = readTarEntries(gunzipSync(artifactBytes), [
    "package/data.json",
    "package/package.json",
    pin.licenseEntry,
    ...extraEntries,
  ]);
  assertPackageIdentity(pin, entries.get("package/package.json"));
  return entries;
}

function datasetIdentity(pin, schemaDigest, licenseDigest) {
  return {
    name: pin.name,
    package: pin.package,
    release: pin.release,
    artifact_url: pin.artifactUrl,
    integrity: pin.integrity,
    license: pin.license,
    license_digest: licenseDigest,
    license_file: pin.licenseFile,
    attribution: pin.attribution,
    schema_digest: schemaDigest,
  };
}

async function fetchBytes(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new SnapshotGenerationError("fetch", `${url}: HTTP ${response.status}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

/** Derive every committed output from the three pinned inputs. Byte-deterministic and offline. */
export function generateFromArtifacts({ bcdArtifact, webFeaturesArtifact, bcdSchemaBytes }) {
  verifyIntegrity(PINS.bcd.schemaFile, bcdSchemaBytes, PINS.bcd.schemaDigest);
  const bcdEntries = openArtifact(PINS.bcd, bcdArtifact);
  const wfEntries = openArtifact(PINS.webFeatures, webFeaturesArtifact, [
    PINS.webFeatures.schemaEntry,
  ]);
  const wfSchemaBytes = wfEntries.get(PINS.webFeatures.schemaEntry);
  const bcdLicense = bcdEntries.get(PINS.bcd.licenseEntry);
  const wfLicense = wfEntries.get(PINS.webFeatures.licenseEntry);
  const datasets = [
    datasetIdentity(PINS.bcd, PINS.bcd.schemaDigest, sha256(bcdLicense)),
    datasetIdentity(PINS.webFeatures, sha256(wfSchemaBytes), sha256(wfLicense)),
  ];
  const { bytes, header } = buildSnapshot({
    bcd: JSON.parse(bcdEntries.get("package/data.json").toString("utf8")),
    webFeatures: JSON.parse(wfEntries.get("package/data.json").toString("utf8")),
    bcdSchema: JSON.parse(bcdSchemaBytes.toString("utf8")),
    webFeaturesSchema: JSON.parse(wfSchemaBytes.toString("utf8")),
    datasets,
    recipeDigest: recipeDigest(),
  });
  return {
    header,
    files: new Map([
      [join("pinned", "compat-snapshot.jsonl"), bytes],
      [join("pinned", PINS.bcd.licenseFile), bcdLicense],
      [join("pinned", PINS.webFeatures.licenseFile), wfLicense],
      [join("pinned", "NOTICE.md"), Buffer.from(renderNotice(datasets), "utf8")],
      [join("schemas", PINS.bcd.schemaFile), bcdSchemaBytes],
      [join("schemas", PINS.webFeatures.schemaFile), wfSchemaBytes],
    ]),
  };
}

/**
 * The test fixture: small upstream-shaped datasets through the same builder and
 * real schemas, under a fixed recipe label so a builder change shows up as a
 * fixture diff rather than as an unrelated identity change.
 */
export function generateFixture() {
  const corpus = (...parts) => readFileSync(join(CORPUS_DIR, ...parts));
  const bcdBytes = corpus("fixtures", "upstream-browser-compat-data.json");
  const wfBytes = corpus("fixtures", "upstream-web-features.json");
  const bcdSchemaBytes = corpus("schemas", PINS.bcd.schemaFile);
  const wfSchemaBytes = corpus("schemas", PINS.webFeatures.schemaFile);
  const fixtureIdentity = (pin, file, bytes, schemaBytes) => ({
    ...datasetIdentity(pin, sha256(schemaBytes), sha256(corpus("pinned", pin.licenseFile))),
    release: FIXTURE_RELEASE,
    artifact_url: `fixture:${file}`,
    integrity: sri512(bytes),
  });
  return {
    bcd: JSON.parse(bcdBytes.toString("utf8")),
    webFeatures: JSON.parse(wfBytes.toString("utf8")),
    bcdSchema: JSON.parse(bcdSchemaBytes.toString("utf8")),
    webFeaturesSchema: JSON.parse(wfSchemaBytes.toString("utf8")),
    datasets: [
      fixtureIdentity(PINS.bcd, "upstream-browser-compat-data.json", bcdBytes, bcdSchemaBytes),
      fixtureIdentity(PINS.webFeatures, "upstream-web-features.json", wfBytes, wfSchemaBytes),
    ],
    recipeDigest: FIXTURE_RECIPE,
  };
}

async function main(argv) {
  if (argv.includes("--fixture")) {
    const { bytes, header } = buildSnapshot(generateFixture());
    writeFileSync(join(CORPUS_DIR, "fixtures", "compat-snapshot.fixture.jsonl"), bytes);
    process.stdout.write(`${JSON.stringify({ snapshot_id: header.snapshot_id })}
`);
    return;
  }
  const check = argv.includes("--check");
  const artifactsFlag = argv.indexOf("--artifacts");
  const artifactsDir = artifactsFlag === -1 ? null : argv[artifactsFlag + 1];
  const read = (file, url) =>
    artifactsDir === null
      ? fetchBytes(url)
      : Promise.resolve(readFileSync(join(artifactsDir, file)));
  const [bcdArtifact, webFeaturesArtifact, bcdSchemaBytes] = await Promise.all([
    read(PINS.bcd.artifactFile, PINS.bcd.artifactUrl),
    read(PINS.webFeatures.artifactFile, PINS.webFeatures.artifactUrl),
    read(PINS.bcd.schemaFile, PINS.bcd.schemaUrl),
  ]);
  const { header, files } = generateFromArtifacts({
    bcdArtifact,
    webFeaturesArtifact,
    bcdSchemaBytes,
  });
  const stale = [];
  for (const [relative, bytes] of files) {
    const target = join(CORPUS_DIR, relative);
    if (check) {
      let committed = null;
      try {
        committed = readFileSync(target);
      } catch {
        // A missing committed file is reported below as stale.
      }
      if (committed === null || !committed.equals(bytes)) stale.push(relative);
    } else {
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, bytes);
    }
  }
  if (stale.length > 0) {
    throw new SnapshotGenerationError(
      "stale",
      `not reproducible from the pins: ${stale.join(", ")}`,
    );
  }
  process.stdout.write(
    `${JSON.stringify({ snapshot_id: header.snapshot_id, counts: header.counts })}\n`,
  );
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  });
}
