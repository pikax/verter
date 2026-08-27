/**
 * @ai-generated - Exercises immutable authority amendment creation, chaining,
 * exact impact computation, schema validation, and tamper refusal.
 */

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { buildActiveAmendmentReactivation, createAmendment, validateAmendments } from "./amendments.mjs";

const PACKAGE_ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const DIGEST_ROOTS = ["authority", "catalogs", "charters", "contracts", "provenance", "schemas", "sources", "state", "templates", "tools"];

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

const TRUSTED_RATIFIER = "test-maintainer";
const TRUSTED_RECEIPT_BYTES = "fixture: test maintainer may ratify authority amendments\n";
const TRUSTED_RECEIPT_SHA256 = sha256(TRUSTED_RECEIPT_BYTES);

function digestPackage(packageRoot) {
  const rows = [];
  const walk = (directory, relativeDirectory) => {
    if (!fs.existsSync(directory)) return;
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const relative = path.posix.join(relativeDirectory, entry.name);
      if (relative === "authority/state/authority-lock.toml" || relative.startsWith("authority/state/amendments/")) continue;
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(absolute, relative);
      else if (entry.isFile()) rows.push([relative, fs.readFileSync(absolute)]);
      else throw new Error(`unsupported fixture entry ${relative}`);
    }
  };
  for (const root of DIGEST_ROOTS) walk(path.join(packageRoot, root), root);
  const hash = crypto.createHash("sha256");
  for (const [relative, bytes] of rows.sort(([left], [right]) => left.localeCompare(right))) {
    hash.update(relative);
    hash.update("\0");
    hash.update(String(bytes.length));
    hash.update("\0");
    hash.update(bytes);
    hash.update("\0");
  }
  return hash.digest("hex");
}

function loadFixtureAuthority(packageRoot) {
  return {
    packageRoot,
    nodes: JSON.parse(fs.readFileSync(path.join(packageRoot, "authority/dag/fixture.json"), "utf8"))
      .map((node) => ({ ...node, _module: "dag/fixture.json" })),
  };
}

function dependencies(overrides = {}) {
  return {
    computeAuthorityDigest: digestPackage,
    loadAuthority: loadFixtureAuthority,
    deriveInvalidatedReceipts: ({ impactClosure }) => impactClosure.map((id) => `${id}:${sha256(`accepted:${id}`)}`).sort(),
    ...overrides,
  };
}

function makePackage(nodes) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "verter-amendments-"));
  fs.mkdirSync(path.join(root, "authority/state/amendments"), { recursive: true });
  fs.mkdirSync(path.join(root, "authority/dag"), { recursive: true });
  fs.mkdirSync(path.join(root, "authority/state/ratification-receipts"), { recursive: true });
  fs.mkdirSync(path.join(root, "schemas"), { recursive: true });
  fs.writeFileSync(path.join(root, "authority/dag/fixture.json"), `${JSON.stringify(nodes, null, 2)}\n`);
  for (const node of nodes) {
    const charterFile = path.join(root, node.charter);
    fs.mkdirSync(path.dirname(charterFile), { recursive: true });
    fs.writeFileSync(charterFile, `# Charter ${node.id}\n`);
  }
  fs.writeFileSync(path.join(root, "authority/state/ratification-receipts/test-maintainer.txt"), TRUSTED_RECEIPT_BYTES);
  fs.writeFileSync(path.join(root, "authority/state/trusted-ratifications.toml"), [
    "schema = 2",
    "",
    "[[slot]]",
    'purpose = "authority-amendment"',
    `ratified_by = "${TRUSTED_RATIFIER}"`,
    'receipt_path = "authority/state/ratification-receipts/test-maintainer.txt"',
    `receipt_sha256 = "${TRUSTED_RECEIPT_SHA256}"`,
    "",
  ].join("\n"));
  for (const name of ["amendment.schema.json", "authority-lock.schema.json", "trusted-ratifications.schema.json"]) {
    fs.copyFileSync(path.join(PACKAGE_ROOT, "schemas", name), path.join(root, "schemas", name));
  }
  return root;
}

function clonePackage(source) {
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "verter-amendments-"));
  fs.cpSync(source, target, { recursive: true });
  return target;
}

function authority(root) {
  return loadFixtureAuthority(root);
}

function resignAmendment(text) {
  const marker = text.search(/^payload_sha256\s*=/m);
  assert.notEqual(marker, -1);
  const prefix = text.slice(0, marker);
  return `${prefix}payload_sha256 = "${sha256(prefix)}"\n`;
}

function headDigest(text) {
  return /^payload_sha256 = "([0-9a-f]{64})"$/m.exec(text)?.[1];
}

function replaceLockHead(lockText, amendmentId, digest) {
  return lockText
    .replace(/^last_amendment_id = ".*"$/m, `last_amendment_id = "${amendmentId}"`)
    .replace(/^last_amendment_sha256 = ".*"$/m, `last_amendment_sha256 = "${digest}"`);
}

const BASE_NODES = [
  { id: "A", name: "A", predecessors: [], charter: "charters/fixture/A.md" },
  { id: "B", name: "B", predecessors: ["A"], charter: "charters/fixture/B.md" },
  { id: "C", name: "C", predecessors: ["B"], charter: "charters/fixture/C.md" },
];

test("an empty baseline lock validates only while its exact authority digest remains current", (t) => {
  const root = makePackage(BASE_NODES);
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const digest = digestPackage(root);
  fs.writeFileSync(path.join(root, "authority/state/authority-lock.toml"), [
    "schema = 2",
    `baseline_authority_sha256 = "${digest}"`,
    `current_authority_sha256 = "${digest}"`,
    "generation = 0",
    'last_amendment_id = ""',
    'last_amendment_sha256 = ""',
    "",
  ].join("\n"));
  assert.deepEqual(validateAmendments(authority(root), dependencies()), []);

  fs.writeFileSync(path.join(root, "authority/dag/fixture.json"), `${JSON.stringify([{ ...BASE_NODES[0], name: "unamended" }, BASE_NODES[1], BASE_NODES[2]], null, 2)}\n`);
  assert.match(validateAmendments(authority(root), dependencies()).join("\n"), /authority lock current digest .* does not match/);
});

test("creation refuses a placeholder ratification receipt without writing chain state", (t) => {
  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));

  assert.throws(() => createAmendment(authority(afterRoot), {
    id: "AMD-NOT-RATIFIED",
    beforeRoot,
    ratifiedBy: TRUSTED_RATIFIER,
    ratificationReceiptSha256: "0".repeat(64),
  }, dependencies()), /non-placeholder SHA-256/);
  assert.equal(fs.existsSync(path.join(afterRoot, "authority/state/authority-lock.toml")), false);
  assert.deepEqual(fs.readdirSync(path.join(afterRoot, "authority/state/amendments")), []);
});

test("creation rejects a forged ratifier, an unknown receipt, and a self-installed trust slot", async (t) => {
  const cases = [
    { name: "forged ratifier", ratifiedBy: "forged-maintainer", receipt: TRUSTED_RECEIPT_SHA256 },
    { name: "unknown receipt", ratifiedBy: TRUSTED_RATIFIER, receipt: "9".repeat(64) },
  ];
  for (const row of cases) await t.test(row.name, () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage([{ ...BASE_NODES[0], name: row.name }, BASE_NODES[1], BASE_NODES[2]]);
    try {
      assert.throws(() => createAmendment(authority(afterRoot), {
        id: "AMD-UNTRUSTED",
        beforeRoot,
        ratifiedBy: row.ratifiedBy,
        ratificationReceiptSha256: row.receipt,
      }, dependencies()), /trusted authority-amendment ratification slot.*before and after authority/);
      assert.equal(fs.existsSync(path.join(afterRoot, "authority/state/authority-lock.toml")), false);
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });

  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([{ ...BASE_NODES[0], name: "self-authorized" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));
  fs.writeFileSync(path.join(beforeRoot, "authority/state/trusted-ratifications.toml"), "schema = 2\nslot = []\n");
  fs.unlinkSync(path.join(beforeRoot, "authority/state/ratification-receipts/test-maintainer.txt"));
  assert.throws(() => createAmendment(authority(afterRoot), {
    id: "AMD-SELF-AUTHORIZED",
    beforeRoot,
    ratifiedBy: TRUSTED_RATIFIER,
    ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies()), /trusted authority-amendment ratification slot.*before and after authority/);
});

test("validation hashes the exact confined trusted receipt bytes and rejects a re-signed forged ratifier", (t) => {
  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));
  const result = createAmendment(authority(afterRoot), {
    id: "AMD-TRUST-CHECK",
    beforeRoot,
    ratifiedBy: TRUSTED_RATIFIER,
    ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  const original = fs.readFileSync(result.amendmentFile, "utf8");
  const forged = resignAmendment(original.replace(`ratified_by = "${TRUSTED_RATIFIER}"`, 'ratified_by = "forged-maintainer"'));
  fs.writeFileSync(result.amendmentFile, forged);
  const lockFile = path.join(afterRoot, "authority/state/authority-lock.toml");
  fs.writeFileSync(lockFile, replaceLockHead(fs.readFileSync(lockFile, "utf8"), "AMD-TRUST-CHECK", headDigest(forged)));
  assert.match(validateAmendments(authority(afterRoot), dependencies()).join("\n"), /not authorized by a trusted authority-amendment ratification slot/);

  fs.writeFileSync(path.join(afterRoot, "authority/state/ratification-receipts/test-maintainer.txt"), "tampered receipt bytes\n");
  assert.match(validateAmendments(authority(afterRoot), dependencies()).join("\n"), /trusted ratification receipt digest mismatch/);
});

test("createAmendment records the exact path, changed-node, and before+after descendant closure", (t) => {
  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([
    { ...BASE_NODES[0], name: "A changed" },
    BASE_NODES[1],
    BASE_NODES[2],
    { id: "D", name: "D", predecessors: ["B"], charter: "charters/fixture/D.md" },
  ]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));

  const result = createAmendment(authority(afterRoot), {
    id: "AMD-TEST-1",
    beforeRoot,
    ratifiedBy: TRUSTED_RATIFIER,
    ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());

  assert.deepEqual(result.amendment.changed_paths, ["authority/dag/fixture.json", "charters/fixture/D.md"]);
  assert.deepEqual(result.amendment.changed_nodes, ["A", "D"]);
  assert.deepEqual(result.amendment.impact_closure, ["A", "B", "C", "D"]);
  assert.deepEqual(result.amendment.invalidated_receipts, ["A", "B", "C", "D"].map((id) => `${id}:${sha256(`accepted:${id}`)}`));
  assert.equal(result.lock.baseline_authority_sha256, digestPackage(beforeRoot));
  assert.equal(result.lock.current_authority_sha256, digestPackage(afterRoot));
  assert.deepEqual(validateAmendments(authority(afterRoot), dependencies()), []);
});

test("charter-only changes seed their node while global authority changes seed every node", async (t) => {
  await t.test("charter-only", () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage(BASE_NODES);
    try {
      fs.writeFileSync(path.join(afterRoot, "charters/fixture/B.md"), "# Changed B charter\n");
      const result = createAmendment(authority(afterRoot), {
        id: "AMD-CHARTER-ONLY", beforeRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
      }, dependencies());
      assert.deepEqual(result.amendment.changed_nodes, ["B"]);
      assert.deepEqual(result.amendment.impact_closure, ["B", "C"]);
      assert.deepEqual(result.amendment.invalidated_receipts, ["B", "C"].map((id) => `${id}:${sha256(`accepted:${id}`)}`));
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });

  await t.test("global schema", () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage(BASE_NODES);
    try {
      fs.writeFileSync(path.join(afterRoot, "schemas/global-policy.txt"), "global validation changed\n");
      const result = createAmendment(authority(afterRoot), {
        id: "AMD-GLOBAL", beforeRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
      }, dependencies());
      assert.deepEqual(result.amendment.changed_nodes, ["A", "B", "C"]);
      assert.deepEqual(result.amendment.impact_closure, ["A", "B", "C"]);
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });
});

test("receipt invalidation is mechanically derived and rejects caller authority", async (t) => {
  await t.test("callback is required", () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
    try {
      assert.throws(() => createAmendment(authority(afterRoot), {
        id: "AMD-NO-DERIVER", beforeRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
      }, dependencies({ deriveInvalidatedReceipts: undefined })), /deriveInvalidatedReceipts dependency is required/);
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });

  await t.test("caller override is forbidden", () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
    try {
      assert.throws(() => createAmendment(authority(afterRoot), {
        id: "AMD-CALLER-INVALIDATION",
        beforeRoot,
        ratifiedBy: TRUSTED_RATIFIER,
        ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
        invalidatedReceipts: [],
      }, dependencies()), /invalidatedReceipts is derived and cannot be supplied/);
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });

  await t.test("deriver cannot name a receipt outside the impact closure", () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage([{ ...BASE_NODES[2], name: "changed" }, BASE_NODES[0], BASE_NODES[1]]);
    try {
      assert.throws(() => createAmendment(authority(afterRoot), {
        id: "AMD-OUTSIDE-INVALIDATION",
        beforeRoot,
        ratifiedBy: TRUSTED_RATIFIER,
        ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
      }, dependencies({
        deriveInvalidatedReceipts: () => [`A:${sha256("accepted:A")}`],
      })), /derived receipt node A is outside the impact closure/);
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });
});

test("validation rejects omitted and extra mechanically derived invalidated receipts", async (t) => {
  for (const mutation of ["omitted", "extra"]) await t.test(mutation, () => {
    const beforeRoot = makePackage(BASE_NODES);
    const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
    try {
      const result = createAmendment(authority(afterRoot), {
        id: `AMD-INVALIDATION-${mutation.toUpperCase()}`,
        beforeRoot,
        ratifiedBy: TRUSTED_RATIFIER,
        ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
      }, dependencies());
      const original = fs.readFileSync(result.amendmentFile, "utf8");
      const expected = ["A", "B", "C"].map((id) => `${id}:${sha256(`accepted:${id}`)}`);
      const forgedValues = mutation === "omitted" ? expected.slice(0, -1) : [...expected, `Z:${sha256("accepted:Z")}`];
      const forged = resignAmendment(original.replace(/^invalidated_receipts = .*$/m, `invalidated_receipts = ${JSON.stringify(forgedValues)}`));
      fs.writeFileSync(result.amendmentFile, forged);
      const lockFile = path.join(afterRoot, "authority/state/authority-lock.toml");
      fs.writeFileSync(lockFile, replaceLockHead(fs.readFileSync(lockFile, "utf8"), result.amendment.amendment_id, headDigest(forged)));
      assert.match(validateAmendments(authority(afterRoot), dependencies()).join("\n"), /invalidated_receipts must exactly equal mechanically derived receipts.*(?:missing|extra)/);
    } finally {
      fs.rmSync(beforeRoot, { recursive: true, force: true });
      fs.rmSync(afterRoot, { recursive: true, force: true });
    }
  });
});

test("validation rejects content after the single final payload digest", (t) => {
  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));
  const result = createAmendment(authority(afterRoot), {
    id: "AMD-TEST-2", beforeRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  fs.appendFileSync(result.amendmentFile, "forged = true\n");

  assert.match(validateAmendments(authority(afterRoot), dependencies()).join("\n"), /payload_sha256.*single final field/);
});

test("validation rejects a schema-validly re-signed unknown amendment field", (t) => {
  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));
  const result = createAmendment(authority(afterRoot), {
    id: "AMD-TEST-3", beforeRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  const original = fs.readFileSync(result.amendmentFile, "utf8");
  const forged = resignAmendment(original.replace(/^payload_sha256/m, "forged = \"yes\"\npayload_sha256"));
  fs.writeFileSync(result.amendmentFile, forged);
  const lockFile = path.join(afterRoot, "authority/state/authority-lock.toml");
  fs.writeFileSync(lockFile, replaceLockHead(fs.readFileSync(lockFile, "utf8"), "AMD-TEST-3", headDigest(forged)));

  assert.match(validateAmendments(authority(afterRoot), dependencies()).join("\n"), /additional property forged/);
});

test("validation recomputes and rejects an incomplete impact closure even after re-signing", (t) => {
  const beforeRoot = makePackage(BASE_NODES);
  const afterRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(beforeRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(afterRoot, { recursive: true, force: true }));
  const result = createAmendment(authority(afterRoot), {
    id: "AMD-TEST-4", beforeRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  const original = fs.readFileSync(result.amendmentFile, "utf8");
  const forged = resignAmendment(original.replace(/^impact_closure = .*$/m, 'impact_closure = ["A"]'));
  fs.writeFileSync(result.amendmentFile, forged);
  const lockFile = path.join(afterRoot, "authority/state/authority-lock.toml");
  fs.writeFileSync(lockFile, replaceLockHead(fs.readFileSync(lockFile, "utf8"), "AMD-TEST-4", headDigest(forged)));

  assert.match(validateAmendments(authority(afterRoot), dependencies()).join("\n"), /impact_closure must exactly equal/);
});

test("the chain is append-only and duplicate amendment creation leaves the lock unchanged", (t) => {
  const baselineRoot = makePackage(BASE_NODES);
  const currentRoot = makePackage([{ ...BASE_NODES[0], name: "first" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(baselineRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(currentRoot, { recursive: true, force: true }));
  createAmendment(authority(currentRoot), {
    id: "AMD-CHAIN-1", beforeRoot: baselineRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  const secondBefore = clonePackage(currentRoot);
  t.after(() => fs.rmSync(secondBefore, { recursive: true, force: true }));
  fs.writeFileSync(path.join(currentRoot, "authority/dag/fixture.json"), `${JSON.stringify([
    { ...BASE_NODES[0], name: "first" },
    BASE_NODES[1],
    { ...BASE_NODES[2], name: "second" },
  ], null, 2)}\n`);
  createAmendment(authority(currentRoot), {
    id: "AMD-CHAIN-2", beforeRoot: secondBefore, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  assert.deepEqual(validateAmendments(authority(currentRoot), dependencies()), []);

  const lockFile = path.join(currentRoot, "authority/state/authority-lock.toml");
  const lockBeforeDuplicate = fs.readFileSync(lockFile, "utf8");
  const duplicateBefore = clonePackage(currentRoot);
  t.after(() => fs.rmSync(duplicateBefore, { recursive: true, force: true }));
  fs.writeFileSync(path.join(currentRoot, "authority/dag/fixture.json"), `${JSON.stringify([
    { ...BASE_NODES[0], name: "first" },
    { ...BASE_NODES[1], name: "duplicate attempt" },
    { ...BASE_NODES[2], name: "second" },
  ], null, 2)}\n`);
  assert.throws(() => createAmendment(authority(currentRoot), {
    id: "AMD-CHAIN-2", beforeRoot: duplicateBefore, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies()), /already exists/);
  assert.equal(fs.readFileSync(lockFile, "utf8"), lockBeforeDuplicate);

  fs.unlinkSync(path.join(currentRoot, "authority/state/amendments/AMD-CHAIN-1.toml"));
  assert.match(validateAmendments(authority(currentRoot), dependencies()).join("\n"), /broken|does not contain every amendment/);
});

test("amendment generations are monotonic, explicit, and portable across copied roots", (t) => {
  const baselineRoot = makePackage(BASE_NODES);
  const currentRoot = makePackage([{ ...BASE_NODES[0], name: "first" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(baselineRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(currentRoot, { recursive: true, force: true }));

  const first = createAmendment(authority(currentRoot), {
    id: "AMD-GENERATION-1", beforeRoot: baselineRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  assert.equal(first.amendment.before_generation, 0);
  assert.equal(first.amendment.after_generation, 1);
  assert.equal(first.lock.generation, 1);

  const secondBefore = clonePackage(currentRoot);
  t.after(() => fs.rmSync(secondBefore, { recursive: true, force: true }));
  fs.writeFileSync(path.join(currentRoot, "authority/dag/fixture.json"), `${JSON.stringify([
    { ...BASE_NODES[0], name: "first" },
    { ...BASE_NODES[1], name: "second" },
    BASE_NODES[2],
  ], null, 2)}\n`);
  const second = createAmendment(authority(currentRoot), {
    id: "AMD-GENERATION-2", beforeRoot: secondBefore, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  assert.equal(second.amendment.before_generation, 1);
  assert.equal(second.amendment.after_generation, 2);
  assert.equal(second.lock.generation, 2);
  assert.deepEqual(validateAmendments(authority(currentRoot), dependencies()), []);

  const copiedRoot = clonePackage(currentRoot);
  t.after(() => fs.rmSync(copiedRoot, { recursive: true, force: true }));
  assert.deepEqual(validateAmendments(authority(copiedRoot), dependencies()), []);
  assert.equal(
    fs.readFileSync(path.join(copiedRoot, "authority/state/authority-lock.toml"), "utf8"),
    fs.readFileSync(path.join(currentRoot, "authority/state/authority-lock.toml"), "utf8"),
  );
  const priorTransition = `ACT-COPIED:${sha256("copied prior activation")}`;
  const previous = {
    previousPackageState: "ACTIVE",
    previousActivationTransition: priorTransition,
    previousAuthoritySha256: second.amendment.before_authority_sha256,
    previousAuthorityGeneration: second.amendment.before_generation,
  };
  assert.deepEqual(
    buildActiveAmendmentReactivation(authority(copiedRoot), previous, dependencies()),
    buildActiveAmendmentReactivation(authority(currentRoot), previous, dependencies()),
  );
});

test("validation rejects non-monotonic amendment generations and a mismatched lock generation", (t) => {
  const baselineRoot = makePackage(BASE_NODES);
  const currentRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(baselineRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(currentRoot, { recursive: true, force: true }));
  const result = createAmendment(authority(currentRoot), {
    id: "AMD-GENERATION-TAMPER", beforeRoot: baselineRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());

  const original = fs.readFileSync(result.amendmentFile, "utf8");
  const forged = resignAmendment(original.replace(/^after_generation = 1$/m, "after_generation = 7"));
  fs.writeFileSync(result.amendmentFile, forged);
  const lockFile = path.join(currentRoot, "authority/state/authority-lock.toml");
  fs.writeFileSync(lockFile, replaceLockHead(fs.readFileSync(lockFile, "utf8"), result.amendment.amendment_id, headDigest(forged)));
  assert.match(validateAmendments(authority(currentRoot), dependencies()).join("\n"), /generation must advance by exactly one/);

  fs.writeFileSync(result.amendmentFile, original);
  fs.writeFileSync(lockFile, fs.readFileSync(lockFile, "utf8")
    .replace(/^generation = [0-9]+$/m, "generation = 9")
    .replace(/^last_amendment_sha256 = ".*"$/m, `last_amendment_sha256 = "${result.amendmentSha256}"`));
  assert.match(validateAmendments(authority(currentRoot), dependencies()).join("\n"), /lock generation does not equal/);
});

test("ACTIVE amendment reactivation is derived from the validated head and prior ACTIVE identity", (t) => {
  const baselineRoot = makePackage(BASE_NODES);
  const currentRoot = makePackage([{ ...BASE_NODES[0], name: "changed" }, BASE_NODES[1], BASE_NODES[2]]);
  t.after(() => fs.rmSync(baselineRoot, { recursive: true, force: true }));
  t.after(() => fs.rmSync(currentRoot, { recursive: true, force: true }));
  const amendment = createAmendment(authority(currentRoot), {
    id: "AMD-ACTIVE-1", beforeRoot: baselineRoot, ratifiedBy: TRUSTED_RATIFIER, ratificationReceiptSha256: TRUSTED_RECEIPT_SHA256,
  }, dependencies());
  const priorTransition = `ACT-INITIAL:${sha256("prior activation")}`;

  const binding = buildActiveAmendmentReactivation(authority(currentRoot), {
    previousPackageState: "ACTIVE",
    previousActivationTransition: priorTransition,
    previousAuthoritySha256: amendment.amendment.before_authority_sha256,
    previousAuthorityGeneration: amendment.amendment.before_generation,
  }, dependencies());
  assert.deepEqual(binding, {
    from_state: "ACTIVE",
    to_state: "ACTIVE",
    previous_activation_transition: priorTransition,
    authority_amendment: `${amendment.amendment.amendment_id}:${amendment.amendmentSha256}`,
    before_authority_sha256: amendment.amendment.before_authority_sha256,
    after_authority_sha256: amendment.amendment.after_authority_sha256,
    before_generation: 0,
    after_generation: 1,
    reactivated_by: TRUSTED_RATIFIER,
    ratification_receipt_sha256: TRUSTED_RECEIPT_SHA256,
  });

  assert.throws(() => buildActiveAmendmentReactivation(authority(currentRoot), {
    previousPackageState: "ACTIVE",
    previousActivationTransition: priorTransition,
    previousAuthoritySha256: "9".repeat(64),
    previousAuthorityGeneration: 0,
  }, dependencies()), /prior ACTIVE authority digest/);
  assert.throws(() => buildActiveAmendmentReactivation(authority(currentRoot), {
    previousPackageState: "ACTIVE",
    previousActivationTransition: priorTransition,
    previousAuthoritySha256: amendment.amendment.before_authority_sha256,
    previousAuthorityGeneration: 1,
  }, dependencies()), /prior ACTIVE authority generation/);
  assert.throws(() => buildActiveAmendmentReactivation(authority(currentRoot), {
    previousPackageState: "ACTIVE",
    previousActivationTransition: `ACT-INITIAL:${"0".repeat(64)}`,
    previousAuthoritySha256: amendment.amendment.before_authority_sha256,
    previousAuthorityGeneration: 0,
  }, dependencies()), /non-placeholder prior activation transition/);
  assert.throws(() => buildActiveAmendmentReactivation(authority(currentRoot), {
    previousPackageState: "DORMANT",
    previousActivationTransition: priorTransition,
    previousAuthoritySha256: amendment.amendment.before_authority_sha256,
    previousAuthorityGeneration: 0,
  }, dependencies()), /prior ACTIVE package state/);
  assert.throws(() => buildActiveAmendmentReactivation(authority(currentRoot), {
    previousPackageState: "ACTIVE",
    previousActivationTransition: priorTransition,
    previousAuthoritySha256: amendment.amendment.before_authority_sha256,
    previousAuthorityGeneration: 0,
    reactivatedBy: "forged-maintainer",
  }, dependencies()), /reactivatedBy is not caller-controlled/);
});
