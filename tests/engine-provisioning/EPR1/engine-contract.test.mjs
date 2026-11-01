import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  artifactKey,
  canonicalJson,
  closureManifestDigest,
  closurePathHazard,
  compatibilityVerdict,
  evidenceVerdict,
  extractionHazard,
  firstFailure,
  firstMatch,
  publicProjection,
  receiptDigest,
  reuseVerdict,
  transitionAllowed,
  unportableString,
  validate,
  versionVerdict,
} from "./tools/engine-contract-model.mjs";
import {
  controllerAuthority,
  main,
  nodeAuthority,
  OwnershipRefusal,
  parseInventory,
  selfDeclaredAuthority,
  selfDeclaredPolicyAuthority,
  validateDocumentRefs,
  validateOwnership,
  validatePolicyTransfer,
} from "./tools/validate-engine-validation-ownership.mjs";

const local = (name) => new URL(name, import.meta.url);
const json = (name) => JSON.parse(readFileSync(local(name), "utf8"));
const contract = json("./products/engine-artifact-contract.v1.json");
const schema = json("./schemas/engine-artifact.schema.json");
const ownershipSchema = json("./schemas/engine-validation-ownership.schema.json");
const inventory = parseInventory(
  readFileSync(local("./catalogs/engine-validation-ownership.toml"), "utf8"),
);
const policy = json("../EPR0/products/engine-policy.v1.json");
const population = contract.ownership.population;
const clone = (value) => structuredClone(value);
const def = (name) => ({ $ref: `#/$defs/${name}` });
const conforms = (value, name) => validate(value, def(name), schema);
const refuses = (value, name) => assert.throws(() => conforms(value, name));
const permute = (value) => Object.fromEntries(Object.entries(value).reverse());

function setPath(target, path, value) {
  const steps = path.split(".");
  const last = steps.pop();
  const holder = steps.reduce((node, step) => node[step], target);
  holder[last] = clone(value);
}

test("every schema definition has a conforming fixture and refuses omissions and added claims", () => {
  assert.deepEqual(Object.keys(contract.fixtures).sort(), Object.keys(schema.$defs).sort());
  for (const [name, fixture] of Object.entries(contract.fixtures)) {
    conforms(fixture, name);
    const definition = schema.$defs[name];
    if (definition.type !== "object") continue;
    for (const field of definition.required) {
      const omitted = clone(fixture);
      delete omitted[field];
      refuses(omitted, name);
    }
    refuses({ ...fixture, trusted: true }, name);
  }
});

test("the validator refuses a schema keyword it does not implement", () => {
  assert.throws(() => validate(1, { type: "integer", maximum: 0 }), /unsupported keyword/);
});

test("the contract defines each boundary the provisioning policy assigned to it", () => {
  for (const boundary of policy.boundaries.filter((row) => row.owner.startsWith("EPR1"))) {
    assert.ok(schema.$defs[boundary.name], boundary.name);
    if (boundary.fields !== undefined) {
      assert.deepEqual(
        Object.keys(schema.$defs[boundary.name].properties).sort(),
        [...boundary.fields].sort(),
        boundary.name,
      );
    }
  }
});

test("artifact identity separates every axis and keeps its key across locators and field order", () => {
  const { base, baseKey, axisMutations, refused, locatorIndependence } = contract.identity;
  assert.equal(artifactKey(base), baseKey);
  assert.equal(artifactKey(permute(base)), baseKey);

  const seen = new Set([baseKey]);
  for (const { field, value } of axisMutations) {
    const changed = { ...base, [field]: value };
    conforms(changed, "EngineArtifactId");
    const key = artifactKey(changed);
    assert.ok(!seen.has(key), `${field}=${value} aliases another artifact`);
    seen.add(key);
  }
  assert.deepEqual(
    [...new Set(axisMutations.map(({ field }) => field))].sort(),
    Object.keys(base).sort(),
  );

  for (const { field, value } of refused) refuses({ ...base, [field]: value }, "EngineArtifactId");
  for (const field of ["locator", "path", "originReceipt"]) {
    refuses({ ...base, [field]: "locator-1" }, "EngineArtifactId");
  }

  const { first, second } = locatorIndependence;
  conforms(first, "EngineOriginReceipt");
  conforms(second, "EngineOriginReceipt");
  assert.notEqual(first.receiptId, second.receiptId);
  assert.notEqual(first.locatorId, second.locatorId);
  assert.deepEqual(first.origin, second.origin);
  assert.equal(first.closureManifestDigest, base.contentDigest);
  assert.equal(second.closureManifestDigest, base.contentDigest);
});

test("platforms canonicalize through the manifest and flavors stay on their host family", () => {
  const triples = contract.platforms.map((row) => row.triple);
  assert.deepEqual([...triples].sort(), [...schema.$defs.EnginePlatform.enum].sort());
  for (const { aliases } of contract.platforms) {
    for (const alias of aliases) assert.ok(!triples.includes(alias), `${alias} is not canonical`);
  }
  assert.deepEqual(
    Object.keys(contract.flavorFamilies).sort(),
    [...schema.$defs.EngineFlavor.enum].sort(),
  );
  assert.deepEqual(contract.flavorFamilies["native-tsgo"], ["native"]);
  assert.ok(contract.flavorFamilies["wasm-single"].includes("browser"));
});

test("compatibility mutations each reach a distinct typed verdict", () => {
  const { requirement, declaration, artifact, hostFeatures, verdicts } = contract.compatibility;
  conforms(requirement, "EngineCompatibilityRequirement");
  conforms(declaration, "EngineCompatibilityDeclaration");
  const reached = new Set();
  for (const vector of verdicts) {
    const actual = compatibilityVerdict(
      { ...requirement, ...vector.requirement },
      { ...declaration, ...vector.declaration },
      { ...artifact, ...vector.artifact },
      vector.hostFeatures ?? hostFeatures,
    );
    assert.equal(actual, vector.expected, vector.id);
    conforms(actual, "EngineCompatibilityVerdict");
    reached.add(actual);
  }
  const { versionWindow, versionBoundaries, stableFloorBoundaries } = contract.compatibility;
  for (const [version, expected] of versionBoundaries) {
    assert.equal(versionVerdict(version, versionWindow), expected, String(version));
    reached.add(expected);
  }
  for (const [version, expected] of stableFloorBoundaries.cases) {
    assert.equal(versionVerdict(version, stableFloorBoundaries.window), expected, version);
  }
  assert.deepEqual([...reached].sort(), [...schema.$defs.EngineCompatibilityVerdict.enum].sort());
  for (const field of contract.compatibility.policyChange.keyFields) {
    assert.ok(Object.hasOwn(contract.cache.key, field), field);
  }
});

test("every source class has its own evidence obligation", () => {
  const rows = contract.sourceEvidence;
  assert.deepEqual(
    rows.map((row) => row.source).sort(),
    [...schema.$defs.EngineSourceKind.enum].sort(),
  );
  assert.deepEqual(
    [...new Set(rows.flatMap((row) => row.trustClasses))].sort(),
    [...schema.$defs.EngineTrustClass.enum].sort(),
  );
  for (const { kind } of policy.sources) {
    assert.ok(schema.$defs.EngineSourceKind.enum.includes(kind), kind);
  }
  for (const row of rows.filter((candidate) => candidate.mutation === "read-only")) {
    assert.ok(
      contract.install.corruption.some(
        (entry) => entry.source === row.source && entry.mutatesArtifact === false,
      ),
      row.source,
    );
  }
});

const evidenceContext = (observed = contract.evidence.observedClosure) => ({
  sourceEvidence: Object.fromEntries(
    contract.sourceEvidence.map((row) => [row.source, row.trustClasses]),
  ),
  authorizedOrigins: contract.evidence.authorizedOrigins,
  approvedRoots: contract.evidence.approvedRoots,
  observedClosure: observed,
});

test("byte, origin, signature and manifest mutations are refused before execution", () => {
  const { descriptor, mutations } = contract.evidence;
  conforms(descriptor, "EngineArtifactDescriptor");
  assert.equal(evidenceVerdict(descriptor, evidenceContext()), null);
  assert.equal(closureManifestDigest(descriptor.integrity.files), descriptor.id.contentDigest);
  assert.equal(
    closureManifestDigest([...descriptor.integrity.files].reverse()),
    descriptor.id.contentDigest,
  );
  for (const mutation of mutations) {
    const changed = clone(descriptor);
    for (const [path, value] of Object.entries(mutation.set ?? {})) setPath(changed, path, value);
    const observed = { ...contract.evidence.observedClosure, ...mutation.observed };
    if (mutation.removeObserved !== undefined) delete observed[mutation.removeObserved];
    assert.equal(
      evidenceVerdict(changed, evidenceContext(observed)),
      mutation.expected,
      mutation.id,
    );
  }
});

test("a browser development artifact can never present as a published one", () => {
  const browser = contract.sourceEvidence.find((row) => row.source === "BrowserArtifact");
  assert.deepEqual(browser.trustClasses, [
    "published-browser-manifest",
    "development-browser-manifest",
  ]);
  const { descriptor } = contract.evidence;
  const development = clone(descriptor);
  development.originReceipts[0].origin = {
    ...development.originReceipts[0].origin,
    sourceKind: "ManagedCache",
    trustClass: "development-browser-manifest",
  };
  assert.equal(evidenceVerdict(development, evidenceContext()), "OriginMismatch");
  const key = (build) => artifactKey({ ...descriptor.id, build });
  assert.notEqual(key("local-development-1"), key("publisher-build-1"));
});

test("closure paths refuse traversal, absolute, reserved and colliding names", () => {
  for (const [path, expected] of contract.evidence.closurePathHazards) {
    assert.equal(closurePathHazard([path]), expected, path);
  }
  assert.equal(closurePathHazard(contract.evidence.closureCollision), "collision");
});

test("trust checks are ordered and each failure keeps its own reason", () => {
  const { trust, precedence } = contract.validationOrder;
  const passing = Object.fromEntries(trust.map((row) => [row.check, true]));
  assert.equal(firstFailure(trust, passing), null);
  for (const row of trust) {
    assert.equal(firstFailure(trust, { ...passing, [row.check]: false }), row.reason, row.check);
  }
  for (const vector of precedence) {
    const facts = { ...passing };
    for (const check of vector.failing) facts[check] = false;
    assert.equal(firstFailure(trust, facts), vector.expected, vector.id);
  }
  const checks = trust.map((row) => row.check);
  assert.ok(checks.indexOf("sourceAuthorized") < checks.indexOf("bytesMatch"));
  assert.ok(checks.indexOf("bytesMatch") < checks.indexOf("signatureValid"));
});

test("every rejection has one typed row and none degrades to not-found", () => {
  const rows = contract.rejections;
  assert.deepEqual(
    rows.map((row) => row.reason).sort(),
    [...schema.$defs.EngineRejectionReason.enum].sort(),
  );
  const states = new Set(policy.outcomes.map((row) => row.state));
  const remediation = new Set(schema.$defs.EngineRejection.properties.remediation.enum);
  for (const row of rows) {
    assert.ok(states.has(row.state), row.reason);
    assert.ok(remediation.has(row.remediation), row.reason);
    assert.ok(schema.$defs.EngineRejection.properties.state.enum.includes(row.state), row.reason);
    assert.notEqual(row.state, "NotFound", row.reason);
    if (row.state === "IntegrityFailure" || row.state === "TrustFailure") {
      assert.equal(row.selection, "Terminate", `${row.reason} must not fall through`);
    }
  }
  for (const { state } of contract.operationalStatuses) assert.ok(states.has(state), state);
  for (const row of contract.afterRejection.filter((entry) => entry.class !== "incompatibility")) {
    assert.equal(row.automaticFallback, false, row.class);
  }
});

test("only READY is selectable and it is reachable only through the last step", () => {
  const { machine, crashPoints } = contract.install;
  assert.deepEqual(machine.selectable, ["Ready"]);
  assert.deepEqual(
    machine.transitions.filter((edge) => edge.to === "Ready").map((edge) => edge.from),
    ["Renamed"],
  );
  assert.deepEqual(
    machine.transitions.filter((edge) => edge.from === "Ready").map((edge) => edge.to),
    ["Quarantined"],
  );
  for (const edge of machine.transitions) assert.equal(edge.actor, "managed-installer");
  for (const [from, to] of machine.forbidden) {
    assert.equal(transitionAllowed(machine, from, to), false, `${from} -> ${to}`);
  }
  assert.deepEqual(crashPoints.map((point) => point.state).sort(), [...machine.states].sort());
  for (const point of crashPoints) {
    const selectableAfterValidation = point.reader === "ValidateThenSelectable";
    assert.equal(machine.selectable.includes(point.state), selectableAfterValidation, point.state);
  }
});

test("a reader refuses unsafe, incomplete and replaced entries with typed reasons", () => {
  const { readerChecks, readerScenarios } = contract.install;
  const passing = Object.fromEntries(readerChecks.map((row) => [row.check, true]));
  assert.equal(firstFailure(readerChecks, passing), null);
  const reasons = new Set(schema.$defs.EngineRejectionReason.enum);
  for (const row of readerChecks) {
    assert.ok(reasons.has(row.reason), row.reason);
    assert.equal(firstFailure(readerChecks, { ...passing, [row.check]: false }), row.reason);
  }
  for (const scenario of readerScenarios) {
    const facts = { ...passing };
    for (const check of scenario.failing) facts[check] = false;
    assert.equal(firstFailure(readerChecks, facts), scenario.expected, scenario.id);
  }
  conforms(contract.fixtures.EngineReadyMarker, "EngineReadyMarker");
  const layout = contract.fixtures.EngineInstallLayout;
  const withoutMarker = layout.entries.filter((entry) => entry !== "READY.json");
  refuses({ ...layout, entries: withoutMarker }, "EngineInstallLayout");
});

test("concurrent writers converge without overwriting or breaking a live lock", () => {
  for (const vector of contract.install.writerVectors) {
    assert.equal(firstMatch(contract.install.writers, vector.facts), vector.expected, vector.id);
  }
});

test("extraction stops at the first broken bound or unsafe entry", () => {
  for (const vector of contract.install.extraction) {
    assert.equal(
      extractionHazard(contract.work.limits, vector.archive),
      vector.expected,
      vector.id,
    );
  }
});

test("each supported operating system has private no-follow atomic primitives", () => {
  const rows = contract.install.platformPrimitives;
  assert.deepEqual(rows.map((row) => row.os).sort(), ["darwin", "linux", "windows"]);
  for (const row of rows) {
    for (const column of [
      "noFollowOpen",
      "exclusiveCreate",
      "atomicNoReplace",
      "privateRoot",
      "durability",
    ]) {
      assert.ok(row[column].length > 0, `${row.os} ${column}`);
    }
  }
  const natives = contract.platforms.filter((row) => row.family === "native");
  const osOf = { apple: "darwin", linux: "linux", windows: "windows" };
  for (const { triple } of natives) {
    const os = Object.entries(osOf).find(([token]) => triple.includes(token))?.[1];
    assert.ok(
      rows.some((row) => row.os === os),
      triple,
    );
  }
});

test("corruption mutates only artifacts a channel owns", () => {
  const rows = contract.install.corruption;
  assert.deepEqual(
    rows.map((row) => row.source).sort(),
    [...schema.$defs.EngineSourceKind.enum].sort(),
  );
  for (const row of rows) {
    const { mutation } = contract.sourceEvidence.find((entry) => entry.source === row.source);
    if (mutation === "read-only") assert.equal(row.mutatesArtifact, false, row.source);
  }
});

test("reuse needs an equal key, and each changed basis forces fresh validation", () => {
  const { key, reuse, changedBasis, activation } = contract.cache;
  conforms(key, "EngineCacheKey");
  const warm = { mutability: "immutable-ready", boundary: "retained-verified-handle" };
  assert.equal(reuseVerdict(reuse, key, clone(key), warm), "ReuseZeroWork");
  assert.deepEqual(changedBasis.map((row) => row.field).sort(), Object.keys(key).sort());
  for (const { change, field } of changedBasis) {
    const value = key[field];
    const other = typeof value === "number" ? value + 1 : `${value}0`;
    assert.equal(
      reuseVerdict(reuse, key, { ...key, [field]: other }, warm),
      "ValidateFresh",
      change,
    );
  }
  assert.equal(
    reuseVerdict(reuse, key, key, { mutability: "immutable-ready", boundary: "lost" }),
    "ValidateFresh",
  );
  for (const boundary of ["retained-verified-handle", "lost"]) {
    assert.equal(
      reuseVerdict(reuse, key, key, { mutability: "mutable", boundary }),
      "StatThenHashOnChange",
    );
    assert.equal(
      activation.find((row) => row.mutability === "mutable" && row.boundary === boundary).check,
      "RehashThroughVerifiedHandles",
    );
  }
});

test("only a complete compatible result is admitted", () => {
  const { completions, verdicts, admitOnly } = contract.cache.admission;
  assert.ok(completions.includes("Cancelled") && completions.includes("Partial"));
  assert.ok(completions.includes("Stale") && verdicts.includes("UnknownCompatibility"));
  const admitted = completions
    .flatMap((completion) => verdicts.map((verdict) => ({ completion, verdict })))
    .filter(
      (pair) => pair.completion === admitOnly.completion && pair.verdict === admitOnly.verdict,
    );
  assert.equal(admitted.length, 1);
  for (const field of ["completion", "compatibilityVerdict"]) {
    const refused = schema.$defs.EngineValidationReceipt.properties[field];
    assert.ok("const" in refused, `${field} must be fixed on a stored receipt`);
  }
});

test("revocation has an action at every stage and none reuses or falls back", () => {
  const { stages, events, transitions, forbiddenActions } = contract.revocation;
  const covered = new Set(transitions.map((row) => `${row.stage}/${row.event}`));
  assert.equal(covered.size, transitions.length);
  assert.equal(covered.size, stages.length * events.length);
  const members = new Set(population.map((member) => member.id));
  for (const row of transitions) {
    assert.ok(!forbiddenActions.includes(row.action), row.action);
    assert.ok(members.has(row.ownerMember), row.ownerMember);
  }
});

test("receipts are deterministic and bind their whole basis", () => {
  const receipt = contract.fixtures.EngineValidationReceipt;
  assert.equal(receiptDigest(receipt), contract.receiptGolden.digest);
  assert.equal(receiptDigest(permute(receipt)), contract.receiptGolden.digest);
  for (const { id, set } of contract.receiptGolden.mutations) {
    assert.notEqual(receiptDigest({ ...receipt, ...set }), contract.receiptGolden.digest, id);
  }
  for (const { id, set } of contract.receiptGolden.refused) {
    assert.throws(() => conforms({ ...receipt, ...set }, "EngineValidationReceipt"), undefined, id);
  }
});

test("canonical encoding refuses values two implementations could encode differently", () => {
  assert.throws(() => canonicalJson(1.5), RangeError);
  assert.throws(() => canonicalJson("é"), RangeError);
  assert.equal(canonicalJson({ b: 1, a: [true, null] }), '{"a":[true,null],"b":1}');
});

test("public status is an allowlisted projection free of roots and credentials", () => {
  const { allowlist, vectors, unportable, portable } = contract.publicStatus;
  assert.deepEqual(
    [...allowlist].sort(),
    Object.keys(schema.$defs.EnginePublicStatus.properties).sort(),
  );
  for (const vector of vectors) {
    assert.ok(unportableString(vector.input) !== null, `${vector.id} must hold private data`);
    const projected = publicProjection(allowlist, vector.input);
    assert.deepEqual(projected, vector.expected, vector.id);
    conforms(projected, "EnginePublicStatus");
    assert.equal(unportableString(projected), null, vector.id);
  }
  for (const text of unportable) assert.notEqual(unportableString({ text }), null, text);
  for (const text of portable) assert.equal(unportableString({ text }), null, text);
  for (const fixture of [
    contract.fixtures.EngineValidationReceipt,
    contract.fixtures.EngineRejection,
    contract.fixtures.EngineArtifactDescriptor,
  ]) {
    assert.equal(unportableString(fixture), null);
  }
});

test("work limits are finite and every zero-work mode names an owner", () => {
  for (const [limit, value] of Object.entries(contract.work.limits)) {
    assert.ok(Number.isSafeInteger(value) && value > 0, limit);
  }
  const counterNames = Object.keys(schema.$defs.EngineValidationWorkCounters.properties);
  const members = new Set(population.map((member) => member.id));
  for (const mode of contract.work.zeroWorkModes) {
    conforms(mode.counters, "EngineValidationWorkCounters");
    assert.ok(members.has(mode.ownerMember), mode.mode);
    for (const name of counterNames) {
      if (mode.nonZeroAllowed?.includes(name)) continue;
      if (mode.mode === "warm-immutable-reuse" && (name === "candidates" || name === "cacheHits")) {
        continue;
      }
      assert.equal(mode.counters[name], 0, `${mode.mode}.${name}`);
    }
  }
  const { warmControl } = contract.work;
  assert.ok(members.has(warmControl.ownerMember));
  assert.equal(warmControl.identicalRequests, 100);
  assert.equal(warmControl.retainedBytesGrowth, 0);
  for (const [name, value] of Object.entries(warmControl.additional)) {
    assert.ok(counterNames.includes(name), name);
    assert.equal(value, 0, name);
  }
});

// ---- Ownership transfer. The authority below is synthetic: no DAG is read or stored here.

const NON_PRODUCTION = "CONTRACTONLY";
const CONTRACT_ACCEPTANCE = `${NON_PRODUCTION}-AC1`;

function fixtureAuthority() {
  const declared = selfDeclaredAuthority(inventory);
  return (node) => {
    if (node === NON_PRODUCTION) {
      return {
        predecessors: ["EPR1"],
        production: false,
        optional: false,
        acceptances: [CONTRACT_ACCEPTANCE],
        subblocks: [],
      };
    }
    if (node === "EPR1") {
      return {
        predecessors: [],
        production: false,
        optional: false,
        acceptances: [],
        subblocks: [],
      };
    }
    return declared(node);
  };
}

const refusalCode = (mutated, members = population) => {
  try {
    validateOwnership(mutated, members, fixtureAuthority());
  } catch (error) {
    assert.ok(error instanceof OwnershipRefusal, String(error));
    return error.code;
  }
  return null;
};

test("the complete inventory satisfies its schema and covers its population", () => {
  validate(inventory, ownershipSchema);
  assert.equal(refusalCode(inventory), null);
  for (const role of ["outcome", "consumer", "displaced"]) {
    assert.ok(
      inventory.rows.some((row) => row.role === role),
      role,
    );
  }
});

test("the inventory refuses each invalid transfer for its intended reason", () => {
  const firstOf = (bad) => bad.rows[0];
  const cases = [
    ["unknown owner", (bad) => (firstOf(bad).owner = "NOSUCHNODE"), "unknown-owner"],
    [
      "unknown node on the path",
      (bad) => {
        firstOf(bad).path = ["EPR1", "NOSUCHNODE", firstOf(bad).owner];
      },
      "no-successor-path",
    ],
    ["empty path", (bad) => (firstOf(bad).path = []), "no-successor-path"],
    [
      "path not starting at the contract",
      (bad) => firstOf(bad).path.splice(0, 1, "EPR5"),
      "no-successor-path",
    ],
    [
      "path skipping its real edge",
      (bad) => {
        const row = bad.rows.find((candidate) => candidate.owner === "EPR5");
        row.path = ["EPR1", "EPR5"];
        bad.rows = [row, ...bad.rows.filter((candidate) => candidate !== row)];
      },
      "no-successor-path",
    ],
    ["missing acceptance", (bad) => (firstOf(bad).acceptance = ""), "acceptance-missing"],
    [
      "nonexistent acceptance",
      (bad) => (firstOf(bad).acceptance = "EPR4-AC-NOSUCH"),
      "acceptance-nonexistent",
    ],
    [
      "nonexistent subblock",
      (bad) => (firstOf(bad).subblocks = ["EPR4-SB99"]),
      "subblock-nonexistent",
    ],
    [
      "known contract-only owner",
      (bad) => {
        Object.assign(firstOf(bad), {
          owner: NON_PRODUCTION,
          path: ["EPR1", NON_PRODUCTION],
          subblocks: [],
          acceptance: CONTRACT_ACCEPTANCE,
        });
      },
      "non-production-owner",
    ],
    [
      "contract node retaining a runtime obligation",
      (bad) => {
        Object.assign(firstOf(bad), { owner: "EPR1", path: ["EPR1"] });
      },
      "no-successor-path",
    ],
    [
      "required outcome owned by an optional channel",
      (bad) => {
        const row = bad.rows.find((candidate) => candidate.applicability === "optional-channel");
        row.applicability = "required";
        bad.rows = [row, ...bad.rows.filter((candidate) => candidate !== row)];
      },
      "applicability-mismatch",
    ],
    [
      "unknown population member",
      (bad) => (firstOf(bad).id = "not-in-population"),
      "unknown-member",
    ],
    ["role drift", (bad) => (firstOf(bad).role = "consumer"), "role-mismatch"],
    ["source drift", (bad) => (firstOf(bad).sourceRef = "docs/elsewhere.md"), "source-mismatch"],
    [
      "conflicting owner",
      (bad) => bad.rows.push({ ...firstOf(bad), owner: "EPR5" }),
      "conflicting-owner",
    ],
    ["duplicate binding", (bad) => bad.rows.push({ ...firstOf(bad) }), "duplicate-binding"],
  ];
  for (const [name, mutate, expected] of cases) {
    const bad = clone(inventory);
    mutate(bad);
    assert.equal(refusalCode(bad), expected, name);
  }
  for (const role of ["outcome", "consumer", "displaced"]) {
    const bad = clone(inventory);
    bad.rows.splice(
      bad.rows.findIndex((row) => row.role === role),
      1,
    );
    assert.equal(refusalCode(bad), "omitted-member", role);
  }
});

test("the population holds every named boundary, source class and displaced route", () => {
  const ids = new Set(population.map((member) => member.id));
  for (const name of [
    "EngineArtifactId",
    "EnginePlatform",
    "EngineFlavor",
    "EngineVersion",
    "EngineOrigin",
    "EngineOriginReceipt",
    "EngineArtifactDescriptor",
    "EngineCompatibilityRequirement",
    "EngineCompatibilityVerdict",
    "EngineIntegrityEvidence",
    "EngineSignatureEvidence",
    "EngineValidationReceipt",
    "EngineInstallLayout",
    "EngineReadyMarker",
    "EngineCacheKey",
    "EngineCacheEntry",
    "EngineRejection",
    "EngineRevocation",
    "EngineRejectionReason",
  ]) {
    assert.ok(ids.has(name), name);
    assert.ok(schema.$defs[name], name);
  }
  const owners = new Map(inventory.rows.map((row) => [row.id, row]));
  for (const source of [
    "source-environment-manual",
    "source-project-local",
    "source-system-path",
    "source-editor-shared",
    "source-managed-cache",
    "source-managed-download",
    "source-bundled-closed",
    "source-bundled-opened",
    "source-browser-artifact",
  ]) {
    assert.equal(owners.get(source)?.role, "consumer", source);
  }
  assert.equal(owners.get("source-managed-download").applicability, "optional-channel");
  assert.equal(owners.get("source-bundled-opened").applicability, "optional-channel");
  assert.equal(owners.get("source-bundled-closed").applicability, "required");
  for (const member of population.filter((entry) => entry.role === "displaced")) {
    assert.ok(member.symbols.length > 0 && member.current.length > 0, member.id);
    assert.ok(!member.sourceRef.includes("#"), member.id);
  }
});

test("a required candidate boundary never depends on an optional channel", () => {
  const required = [
    "EngineInstallLayout",
    "EngineReadyMarker",
    "EngineCacheKey",
    "EngineCacheEntry",
  ];
  for (const row of inventory.rows.filter((candidate) => required.includes(candidate.id))) {
    assert.equal(row.applicability, "required", row.id);
  }
});

test("contract-document sources resolve to a heading", () => {
  validateDocumentRefs(population);
  const members = [{ id: "m", role: "outcome", sourceRef: "doc.md#gone" }];
  assert.throws(
    () => validateDocumentRefs(members, () => "## Present\n"),
    (error) => error.code === "source-missing",
  );
  assert.throws(
    () => validateDocumentRefs(members, () => null),
    (error) => error.code === "source-missing",
  );
  validateDocumentRefs(
    [{ id: "d", role: "displaced", sourceRef: "deleted/by/owner.rs" }],
    () => null,
  );
});

test("controller answers yield known-node, edge, production and acceptance facts", () => {
  const charter = [
    "#### NODE-SB1 - First",
    "- **NODE-AC-ONE:** first.",
    "**NODE-AC2.** second.",
    "- **NODE-AC3 — third:** third.",
  ].join("\n");
  const task = (labels, maxLoc, optional = false) => ({
    predecessors: [{ slug: "EPR1" }],
    labels,
    sizing: { maxLoc },
    optional,
  });
  const authority = nodeAuthority(task(["kind:implementation"], 800, true), charter);
  assert.deepEqual(authority, {
    predecessors: ["EPR1"],
    production: true,
    optional: true,
    acceptances: ["NODE-AC-ONE", "NODE-AC2", "NODE-AC3"],
    subblocks: ["NODE-SB1"],
  });
  assert.equal(nodeAuthority(task(["kind:convergence"], 800), charter).production, true);
  assert.equal(nodeAuthority(task(["kind:contract"], 0), charter).production, false);
  assert.equal(nodeAuthority(task(["kind:constitution"], 800), charter).production, false);
  assert.equal(nodeAuthority(task(["kind:implementation"], 0), charter).production, false);

  const calls = [];
  const run = (args) => {
    calls.push(args);
    if (args.includes("MISSING")) return null;
    return args[0] === "task" ? JSON.stringify(task(["kind:implementation"], 800)) : charter;
  };
  const resolve = controllerAuthority(run);
  assert.equal(resolve("MISSING"), null);
  assert.equal(resolve("BAD; rm -rf"), null);
  assert.equal(resolve("NODE").production, true);
  assert.equal(resolve("NODE").production, true);
  assert.equal(calls.filter((args) => args.includes("NODE")).length, 2);
  assert.ok(!calls.some((args) => args.join(" ").includes("rm -rf")));
});

test("the provisioning policy's sources and routes transfer without restating an owner", () => {
  const kinds = schema.$defs.EngineSourceKind.enum.filter((kind) => kind !== "BrowserArtifact");
  const authorityFor = (catalog) => {
    const declared = selfDeclaredPolicyAuthority(catalog, kinds);
    return (node) =>
      node === NON_PRODUCTION
        ? {
            predecessors: ["EPR0"],
            production: false,
            optional: false,
            acceptances: [CONTRACT_ACCEPTANCE],
            subblocks: [],
          }
        : declared(node);
  };
  const code = (catalog, authority = authorityFor(policy)) => {
    try {
      validatePolicyTransfer(catalog, kinds, authority);
    } catch (error) {
      assert.ok(error instanceof OwnershipRefusal, String(error));
      return error.code;
    }
    return null;
  };
  assert.equal(code(policy), null);

  const cases = [
    ["unclassified source", (bad) => bad.sources.shift(), "unclassified-source"],
    [
      "route without source files",
      (bad) => (bad.displacedRoutes[0].paths = []),
      "missing-source-atom",
    ],
    [
      "route without symbols",
      (bad) => (bad.displacedRoutes[0].symbols = []),
      "missing-source-atom",
    ],
    [
      "unknown route owner",
      (bad) => (bad.displacedRoutes[0].owner = "NOSUCHNODE"),
      "unknown-owner",
    ],
    [
      "route path to another owner",
      (bad) => (bad.displacedRoutes[0].successorPath = ["EPR0"]),
      "no-successor-path",
    ],
    [
      "missing acceptance",
      (bad) => (bad.sources[0].receivingAcceptance = ""),
      "acceptance-missing",
    ],
    [
      "nonexistent acceptance",
      (bad) => (bad.sources[0].receivingAcceptance = "EPR4-AC-NOSUCH"),
      "acceptance-nonexistent",
    ],
    [
      "documentation-only owner",
      (bad) => {
        Object.assign(bad.sources[0], {
          owner: NON_PRODUCTION,
          successorPath: ["EPR0", NON_PRODUCTION],
          receivingAcceptance: CONTRACT_ACCEPTANCE,
        });
      },
      "non-production-owner",
    ],
    [
      "conflicting source owner",
      (bad) => bad.sources.push({ ...bad.sources[0], owner: "EPR5" }),
      "conflicting-owner",
    ],
    ["duplicate source row", (bad) => bad.sources.push({ ...bad.sources[0] }), "duplicate-binding"],
    [
      "consumer acceptance absent",
      (bad) => (bad.consumers[0].acceptance = "EPR4-AC-NOSUCH"),
      "acceptance-nonexistent",
    ],
  ];
  for (const [name, mutate, expected] of cases) {
    const bad = clone(policy);
    mutate(bad);
    // The authority keeps the unmodified catalog's facts so only the mutation can refuse.
    assert.equal(code(bad), expected, name);
  }
});

test("the validator's command line requires an explicit mode and reports its refusal", () => {
  assert.match(main(["--structural"]), /controller not consulted/);
  assert.throws(
    () => main([]),
    (error) => error.code === "usage",
  );
  const script = fileURLToPath(local("./tools/validate-engine-validation-ownership.mjs"));
  const run = (args) => spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
  const structural = run(["--structural"]);
  assert.equal(structural.status, 0, structural.stderr);
  assert.match(structural.stdout, /bindings cover/);
  assert.equal(run([]).status, 1);
  const refused = main.bind(null, ["--live"], () => () => null);
  assert.throws(refused, (error) => error.code === "unknown-owner");
});
