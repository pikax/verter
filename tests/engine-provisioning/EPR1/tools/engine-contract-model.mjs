// Interprets the contract's decision tables and encodings so their vectors can
// be checked. It never discovers, hashes or executes an engine; the production
// validator is owned by the receiving nodes named in the ownership catalog.
import { createHash } from "node:crypto";

const KEYWORDS = new Set([
  "$schema",
  "$defs",
  "$ref",
  "type",
  "oneOf",
  "properties",
  "required",
  "additionalProperties",
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

const satisfies = (value, schema, root, where) => {
  try {
    validate(value, schema, root, where);
    return true;
  } catch (error) {
    if (error instanceof SchemaViolation) return false;
    throw error;
  }
};

/**
 * Validates with draft 2020-12 meaning over the keyword subset the contract uses: an
 * object is closed only by its own `additionalProperties: false`, as a standard
 * validator reads it. A keyword outside the subset is an error rather than ignored,
 * so a schema cannot silently stop constraining a field. Refusing non-NFC text is the
 * one rule beyond the dialect; the contract's canonical encoding states it.
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
  if (schema.additionalProperties !== undefined && schema.additionalProperties !== false) {
    fail("additionalProperties must be false");
  }
  if (schema.oneOf !== undefined) {
    const matched = schema.oneOf.filter((branch) => satisfies(value, branch, root, where));
    if (matched.length !== 1) fail("oneOf");
  }
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
  if (Array.isArray(value) && schema.type === "array") {
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
      if (Object.hasOwn(schema.properties ?? {}, key)) {
        validate(value[key], schema.properties[key], root, `${where}.${key}`);
      } else if (schema.additionalProperties === false) {
        throw new SchemaViolation(`${where}.${key}`, "unknown field");
      }
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

/** Closure order is by UTF-8 bytes, Rust's `str` order, not JavaScript's UTF-16 string order. */
export const sortedClosureFiles = (files) =>
  [...files].sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)));

export const declarationDigest = (declaration) =>
  domainDigest("verter.engine-compatibility-declaration.v1", declaration);

export const requirementDigest = (requirement) =>
  domainDigest("verter.engine-compatibility-requirement.v1", requirement);

/**
 * Binds the declaration, the file a launch starts from and every file in the execution
 * closure, independent of enumeration order. This one digest is the artifact's content
 * digest, so a signature, origin receipt, key or receipt over it covers all of them.
 */
export const closureManifestDigest = ({ declarationDigest: declared, entryPoint, files }) =>
  domainDigest("verter.engine-closure-manifest.v1", {
    declarationDigest: declared,
    entryPoint,
    files: sortedClosureFiles(files),
  });

export const receiptDigest = (receipt) =>
  domainDigest("verter.engine-validation-receipt.v1", receipt);

const RESERVED_NAME = /^(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³]|conin\$|conout\$)(\.|$)/iu;

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

/** The first row whose every condition holds; a table ends with an unconditional row. */
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
  /(^|[\s"'=(])\/[^\s"']/u,
  /(^|[\s"'=(])~[\\/]/u,
  /\$[A-Za-z_]+[\\/]/u,
  /%[A-Za-z_]+%/u,
  /[A-Za-z]:[\\/]/u,
  /\\\\[^\s\\]+\\/u,
  /[^\s/:@]+:[^\s/@]+@/u,
  /\b(token|password|secret|authorization|bearer)\b/iu,
  /\b(npm|gh[opsu])_[A-Za-z0-9]+/u,
  /\[REDACTED\]/u,
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

/**
 * Ordered: a structural mismatch is reported before anything the version could explain.
 * `rules.flavorHosts` says where a flavor can run, so a declaration cannot move an
 * artifact onto a host its flavor does not support.
 */
export function compatibilityVerdict(rules, requirement, declaration, artifact, hostFeatures) {
  if (artifact.engine !== requirement.engine) return "WrongEngine";
  if (artifact.platform !== requirement.platform) return "WrongPlatform";
  if (!requirement.allowedFlavors.includes(artifact.flavor)) return "WrongFlavor";
  const flavor = rules.flavorHosts[artifact.flavor];
  if (!flavor.platforms.includes(artifact.platform)) return "WrongFlavor";
  const { host, moduleFormat, requiredHostFeatures } = declaration.hostConstraints;
  if (host !== flavor.host || !flavor.moduleFormats.includes(moduleFormat)) {
    return "HostUnsupported";
  }
  const needed = [
    ...flavor.requiredHostFeatures,
    ...requiredHostFeatures,
    ...requirement.hostFeatures,
  ];
  if (missing(needed, hostFeatures)) return "HostUnsupported";
  if (declaration.protocol !== requirement.protocol) return "UnsupportedProtocol";
  if (artifact.protocol !== requirement.protocol) return "UnsupportedProtocol";
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

/** The key a validator derives from content, so a change under unchanged IDs still misses. */
export function cacheKeyFor(facts) {
  const { descriptor, requirement, epochs } = facts;
  return {
    artifactKey: artifactKey(descriptor.id),
    closureManifestDigest: descriptor.integrity.closureManifestDigest,
    originReceiptId: facts.originReceiptId,
    locatorStatIdentity: facts.locatorStatIdentity,
    project: facts.project,
    profile: facts.profile,
    host: facts.host,
    hostFeatures: [...facts.hostFeatures].sort(),
    requirementId: requirement.requirementId,
    requirementDigest: requirementDigest(requirement),
    compatibilityPolicyId: requirement.compatibilityPolicyId,
    validationSchema: facts.validationSchema,
    policyEpoch: epochs.policy,
    trustEpoch: epochs.trust,
    revocationEpoch: epochs.revocation,
  };
}

const sameRoot = (a, b) => canonicalJson(a) === canonicalJson(b);

export const revocationCatalogDigest = ({ schema, issuerId, revocationEpoch, notAfter, entries }) =>
  domainDigest("verter.engine-revocation-catalog.v1", {
    schema,
    issuerId,
    revocationEpoch,
    notAfter,
    entries,
  });

/** Why a revocation catalog cannot be used, or null. Every reason fails closed. */
export function revocationCatalogHazard(view) {
  if (view === null) return "catalog-absent";
  const { catalog, issuers, approvedRoots, lastSeenEpoch, now } = view;
  if (!issuers.includes(catalog.issuerId)) return "issuer-unknown";
  if (catalog.signature.manifestDigest !== revocationCatalogDigest(catalog)) {
    return "signature-other-content";
  }
  if (!approvedRoots.some((root) => sameRoot(root, catalog.signature.trustRoot))) {
    return "root-not-approved";
  }
  if (catalog.revocationEpoch < lastSeenEpoch) return "epoch-regressed";
  if (now > catalog.notAfter) return "expired";
  return null;
}

const descriptorRoots = ({ originReceipts, signature }) => [
  ...originReceipts.map((receipt) => receipt.trustRoot),
  ...signature.signatures.map((entry) => entry.trustRoot),
];

const REVOCATION_MATCH = {
  artifact: (target, descriptor) => target.artifactKey === artifactKey(descriptor.id),
  origin: (target, descriptor) =>
    target.originId === descriptor.id.origin ||
    descriptor.originReceipts.some((receipt) => receipt.origin.originId === target.originId),
  signer: (target, descriptor) =>
    descriptor.signature.signatures.some((entry) => entry.signerId === target.signerId),
  "trust-root": (target, descriptor) =>
    descriptorRoots(descriptor).some((root) => sameRoot(root, target.root)),
};

/** The first catalog entry that applies to this descriptor, or null. */
export const matchingRevocation = (entries, descriptor) =>
  entries.find((entry) => REVOCATION_MATCH[entry.target.kind](entry.target, descriptor)) ?? null;

/** What the source row, the origin's trust class and captured policy demand of one origin. */
function requiredEvidence({ origin }, context) {
  const row = context.sourceEvidence.find((candidate) => candidate.source === origin.sourceKind);
  return new Set([
    ...(row?.required ?? []),
    ...(row?.requiredByTrustClass?.[origin.trustClass] ?? []),
    ...context.policyRequiredEvidence,
  ]);
}

/** Tokens not named here are proved by the validator observing them directly. */
const EVIDENCE_HOLDS = {
  signature: (descriptor) => descriptor.signature.signatures.length > 0,
  provenance: (descriptor) => descriptor.provenance.references.length > 0,
  "archive-integrity": ({ integrity }, context) =>
    integrity.archiveIntegrity !== undefined &&
    integrity.archiveIntegrity === context.observedArchiveIntegrity,
  "host-constraints": ({ id, declaration }, context) =>
    context.flavorHosts[id.flavor].host === declaration.hostConstraints.host,
  "development-policy": (_, context) => context.developmentPermitted === true,
};

/** Every obligation assigned to one trust row holds for every origin the descriptor names. */
const obligationsHold = (check, descriptor, context) =>
  descriptor.originReceipts.every((receipt) => {
    const required = requiredEvidence(receipt, context);
    return context.evidenceObligations.every(({ evidence, check: owner, proof }) => {
      if (owner !== check || !required.has(evidence) || proof === "structural") return true;
      const holds = EVIDENCE_HOLDS[evidence];
      return holds === undefined
        ? context.observedEvidence.includes(evidence)
        : holds(descriptor, context);
    });
  });

function originBound(receipt, descriptor, context) {
  const { origin } = receipt;
  const row = context.sourceEvidence.find((candidate) => candidate.source === origin.sourceKind);
  // An origin holds exactly one authorization, so it can never present as a second class.
  const authorized = context.authorizedOrigins.filter((item) => item.originId === origin.originId);
  return (
    origin.originId === descriptor.id.origin &&
    authorized.length === 1 &&
    authorized[0].sourceKind === origin.sourceKind &&
    authorized[0].trustClass === origin.trustClass &&
    authorized[0].authorizedChannel === origin.authorizedChannel &&
    row?.trustClasses.includes(origin.trustClass) === true &&
    receipt.closureManifestDigest === descriptor.id.contentDigest &&
    descriptor.locators.some((locator) => locator.locatorId === receipt.locatorId)
  );
}

/**
 * One function per row of the contract's trust table. `context` holds what the shared
 * validator observed and what captured policy says, never what the producer asserted.
 */
const EVIDENCE_CHECKS = {
  schemaValid: (descriptor, context) =>
    satisfies(descriptor, { $ref: "#/$defs/EngineArtifactDescriptor" }, context.schema, "$") &&
    closurePathHazard(descriptor.integrity.files.map((file) => file.path)) === null &&
    obligationsHold("schemaValid", descriptor, context),
  sourceAuthorized: ({ originReceipts }, context) =>
    originReceipts.every(({ origin }) => context.authorizedSources.includes(origin.sourceKind)),
  revocationDataCurrent: (_, context) => revocationCatalogHazard(context.revocation) === null,
  notRevoked: (descriptor, context) =>
    context.revocation !== null &&
    matchingRevocation(context.revocation.catalog.entries, descriptor) === null,
  originBound: (descriptor, context) =>
    descriptor.originReceipts.every((receipt) => originBound(receipt, descriptor, context)) &&
    obligationsHold("originBound", descriptor, context),
  closureComplete: (descriptor, context) => {
    const { files, entryPoint } = descriptor.integrity;
    const listed = new Set(files.map((file) => file.path));
    const entry = files.find((file) => file.path === entryPoint);
    return (
      entry !== undefined &&
      context.entryPointRoles.includes(entry.role) &&
      Object.keys(context.observedClosure).every((path) => listed.has(path)) &&
      [...listed].every((path) => Object.hasOwn(context.observedClosure, path)) &&
      obligationsHold("closureComplete", descriptor, context)
    );
  },
  bytesMatch: (descriptor, context) => {
    const { id, integrity, declaration } = descriptor;
    const recomputed = closureManifestDigest(integrity);
    return (
      integrity.files.every((file) => context.observedClosure[file.path] === file.digest) &&
      declarationDigest(declaration) === integrity.declarationDigest &&
      recomputed === integrity.closureManifestDigest &&
      recomputed === id.contentDigest &&
      obligationsHold("bytesMatch", descriptor, context)
    );
  },
  rootApproved: (descriptor, context) =>
    descriptorRoots(descriptor).every((root) =>
      context.approvedRoots.some((approved) => sameRoot(approved, root)),
    ),
  signatureValid: (descriptor, context) =>
    descriptor.signature.signatures.every(
      (entry) => entry.manifestDigest === descriptor.id.contentDigest,
    ) && obligationsHold("signatureValid", descriptor, context),
  provenanceComplete: (descriptor, context) =>
    descriptor.provenance.references.every(
      (reference) => reference.subjectDigest === descriptor.id.contentDigest,
    ) && obligationsHold("provenanceComplete", descriptor, context),
  artifactClassPermitted: (descriptor, context) =>
    obligationsHold("artifactClassPermitted", descriptor, context),
};

/**
 * Whether the evidence proves these exact bytes came through an authorized channel
 * under an approved root, in the order `trust` states. Returns the rejection reason, or
 * null. A row with no check fails closed rather than passing.
 */
export function evidenceVerdict(trust, descriptor, context) {
  for (const { check, reason } of trust) {
    if (EVIDENCE_CHECKS[check]?.(descriptor, context) !== true) return reason;
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
  if (archive.path.split("/").length > limits.maxPathComponents) return "too-deep";
  if (archive.entryKind !== "file" && archive.entryKind !== "directory") return "special-entry";
  return closurePathHazard([archive.path]);
}
