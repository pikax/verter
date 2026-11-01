// Interprets the contract's decision tables and encodings so their vectors can
// be checked. It never discovers, hashes or executes an engine; the production
// validator is owned by the receiving nodes named in the ownership catalog.
import { createHash } from "node:crypto";

const KEYWORDS = new Set([
  "$schema",
  "$defs",
  "$ref",
  "type",
  "properties",
  "required",
  "items",
  "enum",
  "const",
  "pattern",
  "minLength",
  "minimum",
  "minItems",
  "uniqueItems",
]);

export class SchemaViolation extends Error {
  constructor(where, rule) {
    super(`${where}: ${rule}`);
    this.where = where;
    this.rule = rule;
  }
}

const jsonType = (value) =>
  Array.isArray(value) ? "array" : value === null ? "null" : typeof value;

/**
 * Validates against the closed JSON-Schema subset the contract uses. Objects are
 * closed, and a keyword outside the subset is an error rather than ignored, so a
 * schema cannot silently stop constraining a field.
 */
export function validate(value, schema, root = schema, where = "$") {
  for (const keyword of Object.keys(schema)) {
    if (!KEYWORDS.has(keyword)) throw new SchemaViolation(where, `unsupported keyword ${keyword}`);
  }
  if (schema.$ref !== undefined) {
    const name = /^#\/\$defs\/([A-Za-z0-9]+)$/u.exec(schema.$ref)?.[1];
    if (name === undefined || !Object.hasOwn(root.$defs ?? {}, name)) {
      throw new SchemaViolation(where, `unresolved ${schema.$ref}`);
    }
    validate(value, root.$defs[name], root, where);
    return;
  }
  const fail = (rule) => {
    throw new SchemaViolation(where, rule);
  };
  if (schema.type === "integer") {
    if (!Number.isSafeInteger(value)) fail("type");
  } else if (schema.type !== undefined && jsonType(value) !== schema.type) {
    fail("type");
  }
  if ("const" in schema && value !== schema.const) fail("const");
  if (schema.enum !== undefined && !schema.enum.includes(value)) fail("enum");
  if (typeof value === "string") {
    if (value.normalize("NFC") !== value) fail("not NFC");
    if (value.length < (schema.minLength ?? 0)) fail("minLength");
    if (schema.pattern !== undefined && !new RegExp(schema.pattern, "u").test(value)) {
      fail("pattern");
    }
  }
  if (typeof value === "number" && schema.minimum !== undefined && value < schema.minimum) {
    fail("minimum");
  }
  if (Array.isArray(value)) {
    if (value.length < (schema.minItems ?? 0)) fail("minItems");
    if (schema.uniqueItems && new Set(value.map(canonicalJson)).size !== value.length) {
      fail("uniqueItems");
    }
    if (schema.items === undefined) fail("array without items");
    value.forEach((item, index) => validate(item, schema.items, root, `${where}[${index}]`));
  }
  if (jsonType(value) === "object") {
    for (const key of schema.required ?? []) {
      if (!Object.hasOwn(value, key)) throw new SchemaViolation(`${where}.${key}`, "required");
    }
    for (const key of Object.keys(value)) {
      if (!Object.hasOwn(schema.properties ?? {}, key)) {
        throw new SchemaViolation(`${where}.${key}`, "unknown field");
      }
      validate(value[key], schema.properties[key], root, `${where}.${key}`);
    }
  }
}

/** RFC 8785 member order over the contract's domain: safe integers and NFC strings only. */
export function canonicalJson(value) {
  if (value === null || typeof value === "boolean") return JSON.stringify(value);
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value)) throw new RangeError("only safe integers are canonical");
    return String(value);
  }
  if (typeof value === "string") {
    if (value.normalize("NFC") !== value) throw new RangeError("only NFC strings are canonical");
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
}

const domainDigest = (domain, value) =>
  `sha256:${createHash("sha256")
    .update(`${domain}\0${canonicalJson(value)}`, "utf8")
    .digest("hex")}`;

export const artifactKey = (id) => domainDigest("verter.engine-artifact-id.v1", id);

const byPortablePath = (a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);

/** Binds every file in the execution closure, independent of enumeration order. */
export const closureManifestDigest = (files) =>
  domainDigest("verter.engine-closure-manifest.v1", [...files].sort(byPortablePath));

export const receiptDigest = (receipt) =>
  domainDigest("verter.engine-validation-receipt.v1", receipt);

const RESERVED_NAME = /^(con|prn|aux|nul|com[1-9]|lpt[1-9]|conin\$|conout\$)(\.|$)/iu;

/** The first hazard in a closure's relative file paths, or null. */
export function closurePathHazard(paths) {
  const folded = new Set();
  for (const path of paths) {
    if (path === "" || path.startsWith("/") || path.includes("\\")) return "not-relative";
    if (/^[A-Za-z]:/u.test(path)) return "not-relative";
    // eslint-disable-next-line no-control-regex
    if (/[<>:"|?*\u0000-\u001f]/u.test(path)) return "illegal-character";
    for (const segment of path.split("/")) {
      if (segment === "" || segment === "." || segment === "..") return "traversal";
      if (segment.endsWith(".") || segment.endsWith(" ")) return "illegal-character";
      if (RESERVED_NAME.test(segment)) return "reserved-name";
    }
    const fold = path.normalize("NFC").toLowerCase();
    if (folded.has(fold)) return "collision";
    folded.add(fold);
  }
  return null;
}

const IDENTIFIER = "(?:0|[1-9]\\d*|\\d*[A-Za-z-][0-9A-Za-z-]*)";
const SEMVER = new RegExp(
  `^(0|[1-9]\\d*)\\.(0|[1-9]\\d*)\\.(0|[1-9]\\d*)` +
    `(?:-(${IDENTIFIER}(?:\\.${IDENTIFIER})*))?` +
    `(?:\\+([0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*))?$`,
  "u",
);

function parseVersion(text) {
  const match = SEMVER.exec(text ?? "");
  if (match === null) return null;
  return {
    core: [BigInt(match[1]), BigInt(match[2]), BigInt(match[3])],
    pre: match[4]?.split(".") ?? null,
    build: match[5] ?? null,
  };
}

const order = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

function compareCore(a, b) {
  for (let i = 0; i < 3; i++) {
    const compared = order(a.core[i], b.core[i]);
    if (compared !== 0) return compared;
  }
  return 0;
}

function comparePrecedence(a, b) {
  const core = compareCore(a, b);
  if (core !== 0) return core;
  if (a.pre === null || b.pre === null) return a.pre === b.pre ? 0 : a.pre === null ? 1 : -1;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    const [x, y] = [a.pre[i], b.pre[i]];
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const [xNumeric, yNumeric] = [/^\d+$/u.test(x), /^\d+$/u.test(y)];
    if (xNumeric !== yNumeric) return xNumeric ? -1 : 1;
    const compared = xNumeric ? order(BigInt(x), BigInt(y)) : order(x, y);
    if (compared !== 0) return compared;
  }
  return 0;
}

/** The verdict a version alone yields under one support window. */
export function versionVerdict(text, policy) {
  const version = parseVersion(text);
  if (version === null) return "UnknownCompatibility";
  if (version.build !== null) return "Unsupported";
  const [floor, ceiling] = [parseVersion(policy.floor), parseVersion(policy.ceilingExclusive)];
  if (compareCore(version, ceiling) >= 0) return "TooNew";
  if (version.pre !== null) {
    const admitted =
      policy.prereleaseChannels.includes(version.pre[0]) &&
      compareCore(version, floor) === 0 &&
      comparePrecedence(version, floor) >= 0;
    return admitted ? "Compatible" : "Unsupported";
  }
  return comparePrecedence(version, floor) < 0 ? "TooOld" : "Compatible";
}

/** Ordered rows: the first check that does not hold names the outcome. */
export function firstFailure(rows, facts) {
  for (const row of rows) {
    if (facts[row.check] !== true) return row.reason;
  }
  return null;
}

/** Reuse needs every key field equal; then mutability and the trust boundary decide the work. */
export function reuseVerdict(table, storedKey, currentKey, entry) {
  const changed = Object.keys(storedKey).filter(
    (field) => canonicalJson(storedKey[field]) !== canonicalJson(currentKey[field]),
  );
  if (changed.length > 0) return table.keyMismatch;
  return (
    table.rows.find((row) => row.mutability === entry.mutability && row.boundary === entry.boundary)
      ?.verdict ?? table.keyMismatch
  );
}

/** Whether a recorded transition exists; READY has no outgoing edge, so it is immutable. */
export const transitionAllowed = (machine, from, to) =>
  machine.transitions.some((edge) => edge.from === from && edge.to === to);

/** The first row whose every condition holds. */
export function firstMatch(rows, facts) {
  return rows.find((row) =>
    Object.entries(row.when).every(([key, expected]) => facts[key] === expected),
  )?.verdict;
}

/** Copies only allowlisted fields, so anything else stays private by construction. */
export function publicProjection(allowlist, status) {
  return Object.fromEntries(
    allowlist.filter((key) => Object.hasOwn(status, key)).map((key) => [key, status[key]]),
  );
}

// A scheme or drive prefix (`https:/`, `file:/`, `C:\`) is never portable, credentialed or not.
const UNPORTABLE = [
  /(^|[\s"'=(])\/[^\s"']+\/[^\s"']/u,
  /[A-Za-z]:[\\/]/u,
  /\\\\[^\s\\]+\\/u,
  /\b(token|password|secret|authorization|bearer)\b/iu,
  /-----BEGIN /u,
];

function* strings(value) {
  if (typeof value === "string") yield value;
  else if (Array.isArray(value)) for (const item of value) yield* strings(item);
  else if (value !== null && typeof value === "object") {
    for (const [key, item] of Object.entries(value)) {
      yield key;
      yield* strings(item);
    }
  }
}

/** The first string a portable record must not carry, or null. */
export function unportableString(value) {
  for (const text of strings(value)) {
    if (UNPORTABLE.some((pattern) => pattern.test(text))) return text;
  }
  return null;
}

const missing = (required, declared) => required.some((item) => !declared.includes(item));

/** Ordered: a structural mismatch is reported before anything the version could explain. */
export function compatibilityVerdict(requirement, declaration, artifact, hostFeatures) {
  if (artifact.platform !== requirement.platform) return "WrongPlatform";
  if (!requirement.allowedFlavors.includes(artifact.flavor)) return "WrongFlavor";
  const required = declaration.hostConstraints.requiredHostFeatures;
  if (missing(required, hostFeatures)) return "HostUnsupported";
  if (declaration.protocol !== requirement.protocol) return "UnsupportedProtocol";
  if (declaration.api !== requirement.api) return "UnsupportedApi";
  if (missing(requirement.commands, declaration.commands)) return "MissingCommand";
  if (missing(requirement.features, declaration.features)) return "MissingFeature";
  if (declaration.toolchainFamily !== requirement.toolchainFamily) return "ToolchainIncompatible";
  const verdict = versionVerdict(artifact.version, {
    floor: requirement.versionFloor,
    ceilingExclusive: requirement.versionCeilingExclusive,
    prereleaseChannels: requirement.prereleaseChannels,
  });
  if (verdict !== "Compatible") return verdict;
  return requirement.exactPin === undefined || requirement.exactPin === artifact.version
    ? "Compatible"
    : "Unsupported";
}

const sameRoot = (a, b) => canonicalJson(a) === canonicalJson(b);

/**
 * Whether the evidence proves these exact bytes came through an authorized channel
 * under an approved root. `context` holds what the shared validator observed itself,
 * never what the producer asserted. Returns the rejection reason, or null.
 */
export function evidenceVerdict(descriptor, context) {
  const { id, integrity, signature, provenance, originReceipts } = descriptor;
  if (closurePathHazard(integrity.files.map((file) => file.path)) !== null) return "InvalidSchema";
  const allowed = context.sourceEvidence;
  for (const receipt of originReceipts) {
    const { origin } = receipt;
    if (origin.originId !== id.origin) return "OriginMismatch";
    if (!context.authorizedOrigins.includes(origin.originId)) return "OriginMismatch";
    if (!(allowed[origin.sourceKind] ?? []).includes(origin.trustClass)) return "OriginMismatch";
    if (receipt.closureManifestDigest !== id.contentDigest) return "OriginMismatch";
    if (!context.approvedRoots.some((root) => sameRoot(root, receipt.trustRoot))) {
      return "TrustFailure";
    }
  }
  const listed = new Map(integrity.files.map((file) => [file.path, file.digest]));
  const observed = Object.entries(context.observedClosure);
  if (observed.some(([path]) => !listed.has(path))) return "ManifestIncomplete";
  if ([...listed.keys()].some((path) => !Object.hasOwn(context.observedClosure, path))) {
    return "ManifestIncomplete";
  }
  if (observed.some(([path, digest]) => listed.get(path) !== digest)) return "IntegrityFailure";
  const recomputed = closureManifestDigest(integrity.files);
  if (recomputed !== integrity.closureManifestDigest || recomputed !== id.contentDigest) {
    return "IntegrityFailure";
  }
  if (signature.requirement === "required" && signature.signatures.length === 0) {
    return "SignatureFailure";
  }
  for (const entry of signature.signatures) {
    if (entry.manifestDigest !== id.contentDigest) return "SignatureFailure";
    if (!context.approvedRoots.some((root) => sameRoot(root, entry.trustRoot))) {
      return "TrustFailure";
    }
  }
  if (provenance.requirement === "required" && provenance.references.length === 0) {
    return "ProvenanceIncomplete";
  }
  if (provenance.references.some((reference) => reference.subjectDigest !== id.contentDigest)) {
    return "ProvenanceIncomplete";
  }
  return null;
}

/** The first extraction bound or entry kind an archive breaks, or null. */
export function extractionHazard(limits, archive) {
  if (archive.entries > limits.maxArchiveEntries) return "too-many-entries";
  if (archive.expandedBytes > limits.maxExpandedBytes) return "too-large";
  if (archive.expandedBytes > archive.compressedBytes * limits.maxCompressionRatio) {
    return "ratio-exceeded";
  }
  if (archive.pathComponents > limits.maxPathComponents) return "too-deep";
  if (archive.entryKind !== "file" && archive.entryKind !== "directory") return "special-entry";
  return closurePathHazard([archive.path]);
}
