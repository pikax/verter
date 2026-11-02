// Hermetic: this suite replaces `fetch` with a failure, so generation is proven
// from bytes alone. Reproducing the real artifacts is `generate.mjs --check`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { gzipSync } from "node:zlib";
import {
  CORPUS_DIR,
  generateFixture,
  generateFromArtifacts,
  PINS,
  readTarEntries,
  recipeDigest,
  sri512,
  verifyIntegrity,
} from "./generate.mjs";
import { schemaValidator } from "./json-schema.mjs";
import {
  buildSnapshot,
  canonicalJson,
  deriveSnapshotId,
  sha256,
  SnapshotGenerationError,
} from "./snapshot-builder.mjs";

const corpus = (...parts) => join(CORPUS_DIR, ...parts);
const readJson = (...parts) => JSON.parse(readFileSync(corpus(...parts), "utf8"));

const bcdSchemaBytes = readFileSync(corpus("schemas", PINS.bcd.schemaFile));
const bcdBrowserSchemaBytes = readFileSync(corpus("schemas", PINS.bcd.browserSchemaFile));
const wfSchemaBytes = readFileSync(corpus("schemas", PINS.webFeatures.schemaFile));

function buildFixture(overrides = {}) {
  return buildSnapshot({ ...generateFixture(), ...overrides });
}

function recordOf(bytes, section, id) {
  const prefix = `${section}\t${id}\t`;
  const line = bytes
    .toString("utf8")
    .split("\n")
    .find((candidate) => candidate.startsWith(prefix));
  assert.notEqual(line, undefined, `${section} ${id} is present`);
  return JSON.parse(line.slice(prefix.length));
}

function reverseKeys(value) {
  if (Array.isArray(value)) return value.map(reverseKeys);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .reverse()
        .map((key) => [key, reverseKeys(value[key])]),
    );
  }
  return value;
}

function tarEntry(name, content) {
  const data = Buffer.from(content, "utf8");
  const header = Buffer.alloc(512);
  header.write(name, 0);
  header.write(`${data.length.toString(8).padStart(11, "0")}\0`, 124);
  const padded = Buffer.alloc(Math.ceil(data.length / 512) * 512);
  data.copy(padded);
  return Buffer.concat([header, padded]);
}

function artifactTarball(pin, { manifest = {}, entries = {} } = {}) {
  const manifestBytes = JSON.stringify({
    name: pin.package,
    version: pin.release,
    license: pin.license,
    ...manifest,
  });
  const files = { "package/package.json": manifestBytes, ...entries };
  return gzipSync(
    Buffer.concat([
      ...Object.entries(files).map(([name, content]) => tarEntry(name, content)),
      Buffer.alloc(1024),
    ]),
  );
}

describe("compatibility snapshot generation", () => {
  const realFetch = globalThis.fetch;
  before(() => {
    globalThis.fetch = () => {
      throw new Error("generation under test must not reach the network");
    };
  });
  after(() => {
    globalThis.fetch = realFetch;
  });

  it("reproduces the committed fixture snapshot byte for byte", () => {
    const committed = readFileSync(corpus("fixtures", "compat-snapshot.fixture.jsonl"));
    assert.ok(buildFixture().bytes.equals(committed));
  });

  it("is independent of input key order", () => {
    const shuffled = buildFixture({
      bcd: reverseKeys(readJson("fixtures", "upstream-browser-compat-data.json")),
      webFeatures: reverseKeys(readJson("fixtures", "upstream-web-features.json")),
    });
    assert.ok(shuffled.bytes.equals(buildFixture().bytes));
  });

  it("preserves every condition as a distinct fact", () => {
    const { bytes } = buildFixture();
    const flags = recordOf(bytes, "bcd", "api.Conditions.flag_with_value").support;
    assert.deepEqual(flags.chrome[0].flags, [
      { type: "preference", name: "#enable-feature", value_to_set: "Enabled" },
    ]);
    assert.equal(flags.firefox[0].partial_implementation, true);
    assert.deepEqual(flags.firefox[0].flags, [{ type: "runtime_flag", name: "--enable-feature" }]);
    assert.deepEqual(flags.safari[0].notes, ["First limit.", "Second limit."]);

    const alternatives = recordOf(bytes, "bcd", "api.Boundary.alternatives").support;
    assert.equal(alternatives.chrome[1].prefix, "webkit");
    assert.equal(alternatives.chrome[1].version_removed, "38");
    assert.equal(alternatives.firefox[0].alternative_name, "legacyAlternatives");

    const uncertain = recordOf(bytes, "bcd", "api.Uncertain.added_unknown_version").support;
    assert.equal(uncertain.chrome[0].version_added, true);
    assert.equal(uncertain.firefox[0].version_added, null);
    assert.equal(uncertain.safari[0].version_added, false);
    assert.equal(
      recordOf(bytes, "bcd", "api.Uncertain.added_at_or_before").support.chrome[0].version_added,
      "≤37",
    );
  });

  it("quarantines a schema mismatch at the narrowest record, never guessing a value", () => {
    const { bytes, header } = buildFixture();
    const oneBrowser = recordOf(bytes, "bcd", "api.Mismatch.one_browser").support;
    assert.deepEqual(oneBrowser.chrome, {
      rejected: { instance_path: "/support/chrome", keyword: "anyOf" },
    });
    assert.equal(oneBrowser.oculus.rejected.keyword, "anyOf");
    assert.equal(oneBrowser.firefox[0].version_added, "50");

    assert.deepEqual(recordOf(bytes, "bcd", "api.Mismatch.whole_record"), {
      rejected: { instance_path: "/mdn_url", keyword: "pattern" },
    });
    assert.deepEqual(recordOf(bytes, "bcd", "api.Mismatch.mirrored").support.chrome, {
      rejected: { instance_path: "/support/chrome", keyword: "mirror-unresolved" },
    });
    assert.equal(recordOf(bytes, "wf", "malformed-feature").rejected.keyword, "oneOf");

    assert.equal(header.counts.bcd_rejected_features, 1);
    assert.equal(header.counts.bcd_rejected_families, 3);
    assert.equal(header.counts.web_features_rejected, 1);
  });

  it("keeps Baseline as separate provenance and maps only what upstream names", () => {
    const { bytes } = buildFixture();
    const widely = recordOf(bytes, "bcd", "api.WidelyAvailable");
    assert.deepEqual(widely.web_features, ["widely-available"]);
    assert.equal(widely.baseline, undefined);
    assert.equal(widely.support.chrome[0].version_added, false);
    assert.deepEqual(recordOf(bytes, "bcd", "api.Boundary").web_features, [
      "limited-availability",
      "newly-available",
    ]);
    assert.equal(recordOf(bytes, "wf", "baseline-without-mapping").compat_features, undefined);
    assert.equal(recordOf(bytes, "wf", "widely-available").status.by_compat_key, undefined);
    assert.deepEqual(recordOf(bytes, "wf", "moved-feature"), {
      kind: "moved",
      redirect_target: "widely-available",
    });
  });

  it("orders records so a reader can binary-search them, and binds identity to content", () => {
    const { bytes, header } = buildFixture();
    const lines = bytes.toString("utf8").split("\n");
    assert.equal(lines.at(-1), "");
    const records = lines.slice(1, -1).map((line) => line.split("\t"));
    const sections = ["browser", "bcd", "wf"];
    for (const [index, [section, id]] of records.entries()) {
      assert.ok(sections.includes(section));
      if (index === 0) continue;
      const [previousSection, previousId] = records[index - 1];
      const order = sections.indexOf(section) - sections.indexOf(previousSection);
      assert.ok(order > 0 || (order === 0 && previousId < id), `${id} follows ${previousId}`);
    }
    const payload = bytes.subarray(bytes.indexOf(10) + 1);
    assert.equal(header.payload_digest, sha256(payload));
    assert.equal(header.snapshot_id, deriveSnapshotId(header));
    assert.notEqual(
      deriveSnapshotId({ ...header, recipe_digest: "sha256:other" }),
      header.snapshot_id,
    );
    assert.notEqual(
      deriveSnapshotId({ ...header, payload_digest: "sha256:other" }),
      header.snapshot_id,
    );
    const upgraded = structuredClone(header.datasets);
    upgraded[0].release = "fixture-2";
    assert.notEqual(deriveSnapshotId({ ...header, datasets: upgraded }), header.snapshot_id);
  });

  it("rejects a dataset whose release is not the admitted one", () => {
    const bcd = readJson("fixtures", "upstream-browser-compat-data.json");
    bcd.__meta.version = "fixture-2";
    assert.throws(() => buildFixture({ bcd }), { code: "dataset-release" });
  });

  it("rejects browser release data it cannot trust rather than omitting it", () => {
    // Positive control: every browser passes the single-browser schema envelope.
    assert.equal(buildFixture().header.counts.browsers, 4);
    const missingEngineVersion = readJson("fixtures", "upstream-browser-compat-data.json");
    delete missingEngineVersion.browsers.chrome.releases["100"].engine_version;
    assert.throws(() => buildFixture({ bcd: missingEngineVersion }), {
      code: "browser-record",
    });
    const invalidDroppedField = readJson("fixtures", "upstream-browser-compat-data.json");
    invalidDroppedField.browsers.chrome.releases["9"].release_notes = "http://example.test/9";
    assert.throws(() => buildFixture({ bcd: invalidDroppedField }), {
      code: "browser-record",
    });
    for (const releaseDate of ["not-a-date", "2011-13-45", "2023-02-29", "2011-1-05"]) {
      const malformedDate = readJson("fixtures", "upstream-browser-compat-data.json");
      malformedDate.browsers.chrome.releases["9"].release_date = releaseDate;
      assert.throws(() => buildFixture({ bcd: malformedDate }), {
        code: "browser-record",
        message: /release_date \(format\)/,
      });
    }
    const bcd = readJson("fixtures", "upstream-browser-compat-data.json");
    bcd.browsers.chrome.releases.latest = { status: "current" };
    assert.throws(() => buildFixture({ bcd }), { code: "browser-record" });
    const unknownStatus = readJson("fixtures", "upstream-browser-compat-data.json");
    unknownStatus.browsers.chrome.releases["100"].status = "stable";
    assert.throws(() => buildFixture({ bcd: unknownStatus }), { code: "browser-record" });
    const noBrowsers = readJson("fixtures", "upstream-browser-compat-data.json");
    noBrowsers.browsers = {};
    assert.throws(() => buildFixture({ bcd: noBrowsers }), { code: "browser-record" });
  });

  it("verifies integrity before ingestion and rejects changed bytes", () => {
    const bytes = Buffer.from("artifact");
    verifyIntegrity("artifact", bytes, sri512(bytes));
    verifyIntegrity("artifact", bytes, sha256(bytes));
    assert.throws(() => verifyIntegrity("artifact", Buffer.from("changed"), sri512(bytes)), {
      code: "integrity",
    });
    assert.throws(
      () =>
        generateFromArtifacts({
          bcdArtifact: artifactTarball(PINS.bcd),
          webFeaturesArtifact: artifactTarball(PINS.webFeatures),
          bcdSchemaBytes,
          bcdBrowserSchemaBytes,
        }),
      { code: "integrity" },
    );
    assert.throws(
      () =>
        generateFromArtifacts({
          bcdArtifact: Buffer.alloc(0),
          webFeaturesArtifact: Buffer.alloc(0),
          bcdSchemaBytes: Buffer.concat([bcdSchemaBytes, Buffer.from(" ")]),
          bcdBrowserSchemaBytes,
        }),
      { code: "integrity" },
    );
    assert.throws(
      () =>
        generateFromArtifacts({
          bcdArtifact: Buffer.alloc(0),
          webFeaturesArtifact: Buffer.alloc(0),
          bcdSchemaBytes,
          bcdBrowserSchemaBytes: Buffer.concat([bcdBrowserSchemaBytes, Buffer.from(" ")]),
        }),
      /integrity: browsers\.schema\.json/,
    );
  });

  it("reads only exact tar entries and rejects duplicates, truncation and absence", () => {
    const tar = Buffer.concat([tarEntry("package/a", "first"), tarEntry("package/b", "second")]);
    assert.equal(readTarEntries(tar, ["package/b"]).get("package/b").toString(), "second");
    assert.throws(() => readTarEntries(tar, ["package/c"]), { code: "artifact-layout" });
    const duplicate = Buffer.concat([tarEntry("package/a", "1"), tarEntry("package/a", "2")]);
    assert.throws(() => readTarEntries(duplicate, ["package/a"]), { code: "artifact-layout" });
    const truncated = tar.subarray(0, 700);
    assert.throws(() => readTarEntries(truncated, ["package/b"]), { code: "artifact-layout" });
  });

  it("fails closed on a schema keyword it does not implement", () => {
    const validator = schemaValidator({ definitions: {} });
    assert.throws(() => validator.validate({ uniqueItems: true }, []), /unsupported keyword/);
    assert.throws(() => validator.validate({ $ref: "#/definitions/absent" }, 1), /dangling/);
    assert.equal(validator.validate({ type: "boolean", nullable: true }, null), null);
    assert.equal(validator.validate({ type: "boolean" }, null).keyword, "type");
  });

  it("asserts the date and uri formats and fails closed on any other", () => {
    const validator = schemaValidator({});
    const date = { format: "date" };
    for (const valid of ["2011-02-08", "2024-02-29", "2000-02-29", "1999-12-31"]) {
      assert.equal(validator.validate(date, valid), null, valid);
    }
    for (const invalid of [
      "",
      "not-a-date",
      "2011-13-45",
      "2011-00-10",
      "2011-01-00",
      "2011-04-31",
      "2023-02-29",
      "1900-02-29",
      "2011-2-8",
      " 2011-02-08",
      "2011-02-08T00:00:00Z",
    ]) {
      assert.deepEqual(
        validator.validate(date, invalid, "/release_date"),
        { instance_path: "/release_date", keyword: "format" },
        invalid,
      );
    }
    const uri = { format: "uri" };
    for (const valid of ["https://example.test/a?b#c", "mailto:a@example.test"]) {
      assert.equal(validator.validate(uri, valid), null, valid);
    }
    for (const invalid of ["", "example.test/a", "/relative", "https://example.test/a b", "1a:b"]) {
      assert.equal(validator.validate(uri, invalid).keyword, "format", invalid);
    }
    // A format constrains strings only; the type keyword owns every other value.
    assert.equal(validator.validate(date, 20110208), null);
    assert.equal(validator.validate(uri, null), null);
    assert.throws(
      () => validator.validate({ format: "email" }, "a@example.test"),
      /unsupported format/,
    );
    assert.throws(() => validator.validate({ format: "email" }, 1), /unsupported format/);
  });

  it("enforces property dependencies and preserves schema dependencies", () => {
    const validator = schemaValidator({});
    const schema = { dependencies: { engine: ["engine_version"] } };
    assert.equal(validator.validate(schema, {}), null);
    assert.equal(validator.validate(schema, { engine: "Blink", engine_version: "100" }), null);
    assert.deepEqual(validator.validate(schema, { engine: "Blink" }), {
      instance_path: "/engine_version",
      keyword: "required",
    });
    const dependentSchema = { dependencies: { engine: { required: ["engine_version"] } } };
    assert.equal(validator.validate(dependentSchema, { engine: "Blink" }).keyword, "required");
    assert.equal(
      validator.validate(dependentSchema, { engine: "Blink", engine_version: "100" }),
      null,
    );
  });

  it("keeps the committed outputs bound to this recipe and the pinned inputs", () => {
    const snapshot = readFileSync(corpus("pinned", "compat-snapshot.jsonl"));
    const newline = snapshot.indexOf(10);
    const header = JSON.parse(snapshot.subarray(0, newline).toString("utf8"));
    assert.equal(canonicalJson(header), snapshot.subarray(0, newline).toString("utf8"));
    assert.equal(header.recipe_digest, recipeDigest(), "regenerate after changing the recipe");
    assert.equal(header.payload_digest, sha256(snapshot.subarray(newline + 1)));
    assert.equal(header.snapshot_id, deriveSnapshotId(header));

    assert.deepEqual(
      header.datasets.map(({ package: name, release, integrity, artifact_url, license }) => ({
        name,
        release,
        integrity,
        artifact_url,
        license,
      })),
      [PINS.bcd, PINS.webFeatures].map((pin) => ({
        name: pin.package,
        release: pin.release,
        integrity: pin.integrity,
        artifact_url: pin.artifactUrl,
        license: pin.license,
      })),
    );
    assert.equal(sha256(bcdSchemaBytes), PINS.bcd.schemaDigest);
    assert.equal(sha256(bcdBrowserSchemaBytes), PINS.bcd.browserSchemaDigest);
    const schemaDigests = {
      "compat-data.schema.json": sha256(bcdSchemaBytes),
      "browsers.schema.json": sha256(bcdBrowserSchemaBytes),
    };
    assert.equal(
      header.datasets[0].schema_digest,
      sha256(Buffer.from(canonicalJson(schemaDigests))),
    );
    schemaDigests["browsers.schema.json"] = "sha256:changed";
    const changedSchemas = structuredClone(header.datasets);
    changedSchemas[0].schema_digest = sha256(Buffer.from(canonicalJson(schemaDigests)));
    assert.notEqual(deriveSnapshotId({ ...header, datasets: changedSchemas }), header.snapshot_id);
    assert.equal(header.datasets[1].schema_digest, sha256(wfSchemaBytes));
    assert.equal(
      header.datasets[0].license_digest,
      sha256(readFileSync(corpus("pinned", PINS.bcd.licenseFile))),
    );
    assert.equal(
      header.datasets[1].license_digest,
      sha256(readFileSync(corpus("pinned", PINS.webFeatures.licenseFile))),
    );
    assert.ok(readFileSync(corpus("pinned", "NOTICE.md"), "utf8").includes("## Modifications"));
    assert.ok(new SnapshotGenerationError("code", "message").message.startsWith("code:"));
  });

  it("admits only official artifact hosts", () => {
    for (const pin of [PINS.bcd, PINS.webFeatures]) {
      assert.ok(pin.artifactUrl.startsWith("https://registry.npmjs.org/"));
    }
    assert.ok(
      PINS.bcd.browserSchemaUrl.startsWith(
        `https://raw.githubusercontent.com/mdn/browser-compat-data/v${PINS.bcd.release}/`,
      ),
    );
    assert.ok(
      PINS.bcd.schemaUrl.startsWith(
        `https://raw.githubusercontent.com/mdn/browser-compat-data/v${PINS.bcd.release}/`,
      ),
    );
  });
});
