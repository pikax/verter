// Pure, offline derivation of the pinned compatibility snapshot from already
// integrity-verified dataset bytes. The output is byte-deterministic: object key
// order in the input never reaches it, and array order is preserved only where
// upstream order is meaningful (support statements and notes).
//
// Layout, one record per line, LF-terminated:
//   line 1: the JSON header (identity, provenance, payload digest)
//   then:   "<section>\t<id>\t<json>" sorted by section (browser, bcd, wf) and
//           by id within a section, so a reader binary-searches without parsing
//           records it was not asked about.

import { createHash } from "node:crypto";
import { schemaValidator } from "./json-schema.mjs";

export const FORMAT = "verter-compat-snapshot";
export const FORMAT_VERSION = 1;

const IDENTIFIER_PATTERN = /^[a-zA-Z_0-9\-$@]+$/u;
const RELEASE_VERSION_PATTERN = /^\d+(\.\d+)*$/u;
const RELEASE_STATUSES = new Set([
  "retired",
  "current",
  "exclusive",
  "beta",
  "nightly",
  "esr",
  "planned",
]);

/** A failure that stops generation outright; no partial snapshot is ever written. */
export class SnapshotGenerationError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = "SnapshotGenerationError";
    this.code = code;
  }
}

export function sha256(bytes) {
  return `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
}

/** JSON with recursively sorted object keys, so input key order cannot change a byte. */
export function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const members = Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`);
    return `{${members.join(",")}}`;
  }
  return JSON.stringify(value);
}

function compareBytes(a, b) {
  return a < b ? -1 : a > b ? 1 : 0;
}

function assertRecordId(id) {
  if (!/^[\x21-\x7e]+$/u.test(id)) {
    throw new SnapshotGenerationError("record-id", `unencodable record id ${JSON.stringify(id)}`);
  }
}

function projectBrowsers(browsers) {
  const lines = [];
  for (const id of Object.keys(browsers ?? {})) {
    const browser = browsers[id];
    assertRecordId(id);
    if (typeof browser?.name !== "string" || typeof browser.type !== "string") {
      throw new SnapshotGenerationError("browser-record", `${id}: missing name or type`);
    }
    const releases = {};
    for (const version of Object.keys(browser.releases ?? {})) {
      const release = browser.releases[version];
      if (!RELEASE_VERSION_PATTERN.test(version) || !RELEASE_STATUSES.has(release?.status)) {
        throw new SnapshotGenerationError(
          "browser-record",
          `${id} ${version}: invalid release version or status`,
        );
      }
      releases[version] = {
        status: release.status,
        ...(release.release_date === undefined ? {} : { release_date: release.release_date }),
        ...(release.engine === undefined ? {} : { engine: release.engine }),
        ...(release.engine_version === undefined ? {} : { engine_version: release.engine_version }),
      };
    }
    if (Object.keys(releases).length === 0) {
      throw new SnapshotGenerationError("browser-record", `${id}: no releases`);
    }
    lines.push({
      id,
      record: {
        name: browser.name,
        type: browser.type,
        ...(browser.accepts_flags === undefined ? {} : { accepts_flags: browser.accepts_flags }),
        releases,
      },
    });
  }
  if (lines.length === 0) {
    throw new SnapshotGenerationError("browser-record", "the dataset carries no browsers");
  }
  return lines;
}

function projectStatement(statement) {
  const { impl_url: _dropped, notes, ...kept } = statement;
  return notes === undefined ? kept : { ...kept, notes: Array.isArray(notes) ? notes : [notes] };
}

function projectFamily(validator, supportBlock, family, value) {
  if (value === "mirror") {
    return { rejected: { instance_path: `/support/${family}`, keyword: "mirror-unresolved" } };
  }
  const failure = validator.validate(supportBlock, { [family]: value }, "/support");
  if (failure !== null) return { rejected: failure };
  return (Array.isArray(value) ? value : [value]).map(projectStatement);
}

function projectCompat(validator, schema, category, compat) {
  const definitions = schema.definitions;
  const compatSchema =
    category === "webextensions"
      ? definitions.webextensions_identifier.properties.__compat
      : definitions.identifier.properties.__compat;
  // The envelope and each browser's statements are validated separately: one
  // browser's malformed statement must not erase another browser's valid record.
  const { support, ...envelope } = compat ?? {};
  const envelopeFailure = validator.validate(compatSchema, {
    ...envelope,
    ...(support === undefined ? {} : { support: {} }),
  });
  if (envelopeFailure !== null || support === null || typeof support !== "object") {
    return {
      rejected: envelopeFailure ?? { instance_path: "/support", keyword: "type" },
    };
  }
  const projected = {};
  let rejectedFamilies = 0;
  for (const family of Object.keys(support)) {
    assertRecordId(family);
    projected[family] = projectFamily(
      validator,
      definitions.support_block,
      family,
      support[family],
    );
    if (!Array.isArray(projected[family])) rejectedFamilies += 1;
  }
  const specUrl = compat.spec_url;
  return {
    rejectedFamilies,
    record: {
      ...(compat.mdn_url === undefined ? {} : { mdn_url: compat.mdn_url }),
      ...(specUrl === undefined ? {} : { spec_url: Array.isArray(specUrl) ? specUrl : [specUrl] }),
      ...(compat.status === undefined ? {} : { status: compat.status }),
      support: projected,
    },
  };
}

function walkBcd(node, path, category, visit) {
  for (const key of Object.keys(node)) {
    if (key === "__compat") {
      visit(path.join("."), category, node[key]);
      continue;
    }
    if (!IDENTIFIER_PATTERN.test(key) || node[key] === null || typeof node[key] !== "object") {
      throw new SnapshotGenerationError(
        "bcd-structure",
        `invalid identifier ${JSON.stringify([...path, key].join("."))}`,
      );
    }
    walkBcd(node[key], [...path, key], category, visit);
  }
}

function projectWebFeature(validator, schema, feature) {
  const failure = validator.validate(schema.properties.features.additionalProperties, feature);
  if (failure !== null) return { rejected: failure };
  if (feature.kind === "moved") {
    return { kind: "moved", redirect_target: feature.redirect_target };
  }
  if (feature.kind === "split") {
    return { kind: "split", redirect_targets: feature.redirect_targets };
  }
  const { by_compat_key: _dropped, ...status } = feature.status;
  return {
    kind: "feature",
    name: feature.name,
    status,
    ...(feature.group === undefined ? {} : { group: feature.group }),
    ...(feature.compat_features === undefined ? {} : { compat_features: feature.compat_features }),
    ...(feature.discouraged === undefined ? {} : { discouraged: feature.discouraged }),
  };
}

/** The identity law both this generator and the session loader derive `snapshot_id` from. */
export function deriveSnapshotId(header) {
  const parts = [`${FORMAT}-id`, String(header.format_version)];
  for (const dataset of header.datasets) {
    parts.push(
      dataset.name,
      dataset.release,
      dataset.integrity,
      dataset.schema_digest,
      dataset.license_digest,
    );
  }
  parts.push(header.recipe_digest, header.payload_digest);
  return sha256(Buffer.from(parts.join("\0"), "utf8"));
}

/**
 * Build the snapshot. `datasets` is the ordered list of constituent identities;
 * its entries are recorded verbatim and must describe the bytes parsed into `bcd`
 * and `webFeatures`, which the caller has already verified.
 */
export function buildSnapshot({
  bcd,
  webFeatures,
  bcdSchema,
  webFeaturesSchema,
  datasets,
  recipeDigest,
}) {
  const bcdRelease = datasets.find((dataset) => dataset.name === "browser-compat-data")?.release;
  if (bcd?.__meta?.version !== bcdRelease) {
    throw new SnapshotGenerationError(
      "dataset-release",
      `data.json declares ${JSON.stringify(bcd?.__meta?.version)}, admitted ${JSON.stringify(bcdRelease)}`,
    );
  }

  const webFeaturesValidator = schemaValidator(webFeaturesSchema);
  const reverseMapping = new Map();
  const wfLines = [];
  let rejectedWebFeatures = 0;
  for (const id of Object.keys(webFeatures?.features ?? {})) {
    assertRecordId(id);
    const record = projectWebFeature(
      webFeaturesValidator,
      webFeaturesSchema,
      webFeatures.features[id],
    );
    if (record.rejected !== undefined) rejectedWebFeatures += 1;
    for (const compatKey of record.compat_features ?? []) {
      if (!reverseMapping.has(compatKey)) reverseMapping.set(compatKey, []);
      reverseMapping.get(compatKey).push(id);
    }
    wfLines.push({ id, record });
  }
  if (wfLines.length === 0) {
    throw new SnapshotGenerationError("web-features-structure", "the dataset carries no features");
  }

  const bcdValidator = schemaValidator(bcdSchema);
  const bcdLines = [];
  let rejectedRecords = 0;
  let rejectedFamilies = 0;
  for (const category of Object.keys(bcd)) {
    if (category === "__meta" || category === "browsers") continue;
    if (bcd[category] === null || typeof bcd[category] !== "object") {
      throw new SnapshotGenerationError("bcd-structure", `invalid category ${category}`);
    }
    walkBcd(bcd[category], [category], category, (id, recordCategory, compat) => {
      assertRecordId(id);
      const projected = projectCompat(bcdValidator, bcdSchema, recordCategory, compat);
      if (projected.rejected !== undefined) {
        rejectedRecords += 1;
        bcdLines.push({ id, record: { rejected: projected.rejected } });
        return;
      }
      rejectedFamilies += projected.rejectedFamilies;
      const mappedFeatures = reverseMapping.get(id);
      bcdLines.push({
        id,
        record: {
          ...projected.record,
          ...(mappedFeatures === undefined
            ? {}
            : { web_features: mappedFeatures.sort(compareBytes) }),
        },
      });
    });
  }
  if (bcdLines.length === 0) {
    throw new SnapshotGenerationError("bcd-structure", "the dataset carries no support records");
  }

  const browserLines = projectBrowsers(bcd.browsers);
  const render = (section, lines) =>
    lines
      .sort((a, b) => compareBytes(a.id, b.id))
      .map(({ id, record }) => `${section}\t${id}\t${canonicalJson(record)}\n`)
      .join("");
  const payload = Buffer.from(
    render("browser", browserLines) + render("bcd", bcdLines) + render("wf", wfLines),
    "utf8",
  );

  const header = {
    format: FORMAT,
    format_version: FORMAT_VERSION,
    datasets,
    recipe_digest: recipeDigest,
    payload_digest: sha256(payload),
    counts: {
      browsers: browserLines.length,
      bcd_features: bcdLines.length,
      bcd_rejected_features: rejectedRecords,
      bcd_rejected_families: rejectedFamilies,
      web_features: wfLines.length,
      web_features_rejected: rejectedWebFeatures,
    },
  };
  header.snapshot_id = deriveSnapshotId(header);
  return {
    header,
    bytes: Buffer.concat([Buffer.from(`${canonicalJson(header)}\n`, "utf8"), payload]),
  };
}

/** The retained attribution and modification notice, rendered from the dataset identities. */
export function renderNotice(datasets) {
  const entries = datasets
    .map(
      (dataset) =>
        `- **${dataset.attribution}**: \`${dataset.package}@${dataset.release}\` from ` +
        `${dataset.artifact_url} (${dataset.integrity}), licensed ${dataset.license}; ` +
        `the license text is retained as \`${dataset.license_file}\`.`,
    )
    .join("\n");
  return `# Third-party data notice

\`compat-snapshot.jsonl\` is derived from the datasets below and is not an original
Verter work. It is generated by \`scripts/web-compat/generate.mjs\`.

${entries}

## Modifications

The upstream records were projected, not edited. No support value is added,
inferred or corrected.

- browser-compat-data: \`impl_url\`, \`tags\`, \`description\` and \`source_file\` are
  dropped, as are browser \`pref_url\`, \`preview_name\`, \`accepts_webextensions\` and
  release \`release_notes\`; \`notes\` and \`spec_url\` are normalised to arrays and a
  single support statement to a one-element array.
- web-features: \`description\`, \`description_html\`, \`spec\`, \`caniuse\`, \`snapshot\`,
  \`status.by_compat_key\`, and the \`browsers\`, \`groups\` and \`snapshots\` tables are
  dropped; \`group\` is normalised to an array.
- Both: a record, or one browser's statements, failing the upstream schema is
  replaced by a typed rejection naming the failure, and a reverse index from each
  browser-compat-data key to its web-features ids is added.
`;
}
