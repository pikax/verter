// Checks the runtime-ownership transfer of the engine artifact contract.
//   --structural  schema, population coverage and documentation anchors; no controller access.
//   --live        also resolves owners, paths and acceptance IDs against the plan controller.
// The DAG stays in the controller: nothing here is a copy of it.
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { SchemaViolation, validate } from "./engine-contract-model.mjs";

const PACKAGE_ROOT = fileURLToPath(new URL("../", import.meta.url));
const REPOSITORY_ROOT = resolve(PACKAGE_ROOT, "../../..");
const CONTRACT_NODE = "EPR1";
const POLICY_NODE = "EPR0";
const POLICY_PRODUCT = "../EPR0/products/engine-policy.v1.json";
const BROWSER_SOURCE_KIND = "BrowserArtifact";
const PLAN = "0.1.0-tama";

export class OwnershipRefusal extends Error {
  constructor(code, member, detail = "") {
    super(`${code}: ${member}${detail === "" ? "" : ` (${detail})`}`);
    this.code = code;
    this.member = member;
  }
}

/** Reads the catalog's closed TOML subset: top-level scalars and `[[rows]]` tables. */
export function parseInventory(text) {
  const inventory = { rows: [] };
  let target = inventory;
  for (const raw of text.split(/\r?\n/u)) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    if (line === "[[rows]]") {
      target = {};
      inventory.rows.push(target);
      continue;
    }
    const match = /^([A-Za-z]+) = (.+)$/u.exec(line);
    if (match === null || Object.hasOwn(target, match[1])) {
      throw new OwnershipRefusal("inventory-syntax", line);
    }
    target[match[1]] = JSON.parse(match[2]);
  }
  return inventory;
}

/**
 * Refuses the first defect in the transfer. `authority(node)` returns
 * `{ predecessors, production, optional, acceptances, subblocks }`, or null for an
 * unknown node. A pending node is a valid owner: this checks the transfer, not
 * whether the owner has been implemented.
 */
export function validateOwnership(inventory, population, authority, contractNode = CONTRACT_NODE) {
  const members = new Map(population.map((member) => [member.id, member]));
  if (members.size !== population.length) throw new OwnershipRefusal("duplicate-population", "*");
  const owners = new Map();
  for (const row of inventory.rows) {
    const member = members.get(row.id);
    if (member === undefined) throw new OwnershipRefusal("unknown-member", row.id);
    if (owners.has(row.id)) {
      const code = owners.get(row.id) === row.owner ? "duplicate-binding" : "conflicting-owner";
      throw new OwnershipRefusal(code, row.id);
    }
    owners.set(row.id, row.owner);
    if (row.role !== member.role) throw new OwnershipRefusal("role-mismatch", row.id);
    if (row.sourceRef !== member.sourceRef) throw new OwnershipRefusal("source-mismatch", row.id);

    const owner = authority(row.owner);
    if (owner === null) throw new OwnershipRefusal("unknown-owner", row.id, row.owner);
    const path = row.path;
    if (path.length < 2 || path[0] !== contractNode || path.at(-1) !== row.owner) {
      throw new OwnershipRefusal("no-successor-path", row.id);
    }
    for (let i = 1; i < path.length; i++) {
      const node = authority(path[i]);
      if (node === null || !node.predecessors.includes(path[i - 1])) {
        throw new OwnershipRefusal("no-successor-path", row.id, `${path[i - 1]} -> ${path[i]}`);
      }
    }
    if (row.acceptance === "") throw new OwnershipRefusal("acceptance-missing", row.id);
    if (!owner.acceptances.includes(row.acceptance)) {
      throw new OwnershipRefusal("acceptance-nonexistent", row.id, row.acceptance);
    }
    for (const subblock of row.subblocks) {
      if (!owner.subblocks.includes(subblock)) {
        throw new OwnershipRefusal("subblock-nonexistent", row.id, subblock);
      }
    }
    if (!owner.production) throw new OwnershipRefusal("non-production-owner", row.id, row.owner);
    if ((row.applicability === "optional-channel") !== owner.optional) {
      throw new OwnershipRefusal("applicability-mismatch", row.id, row.owner);
    }
  }
  for (const member of population) {
    if (!owners.has(member.id)) {
      throw new OwnershipRefusal("omitted-member", member.id, member.role);
    }
  }
}

/**
 * Reads the provisioning policy's own source and route rows as a transfer, so its
 * catalog stays the one authority for them: nothing here restates an owner. Every
 * source kind the policy governs must be classified, and every route must name the
 * source atoms it displaces.
 */
export function policyTransfer(policy, sourceKinds) {
  const rows = [];
  for (const source of policy.sources) {
    rows.push({
      id: `source:${source.kind}`,
      role: "consumer",
      sourceRef: "policy",
      applicability: "required",
      owner: source.owner,
      path: source.successorPath,
      subblocks: [],
      acceptance: source.receivingAcceptance,
    });
  }
  for (const route of policy.displacedRoutes) {
    if (route.paths.length === 0 || route.symbols.length === 0) {
      throw new OwnershipRefusal("missing-source-atom", route.id);
    }
    rows.push({
      id: `route:${route.id}`,
      role: "displaced",
      sourceRef: "policy",
      applicability: "required",
      owner: route.owner,
      path: route.successorPath,
      subblocks: [],
      acceptance: route.receivingAcceptance,
    });
  }
  const classified = new Set(policy.sources.map((source) => source.kind));
  for (const kind of sourceKinds) {
    if (!classified.has(kind)) throw new OwnershipRefusal("unclassified-source", kind);
  }
  const population = [
    ...new Map(
      rows.map((row) => [row.id, { id: row.id, role: row.role, sourceRef: row.sourceRef }]),
    ).values(),
  ];
  return { inventory: { rows }, population };
}

/** Checks the policy's transfer plus the acceptance each of its named consumers receives. */
export function validatePolicyTransfer(policy, sourceKinds, authority) {
  const { inventory, population } = policyTransfer(policy, sourceKinds);
  validateOwnership(inventory, population, authority, POLICY_NODE);
  for (const consumer of policy.consumers) {
    const owner = authority(consumer.consumer);
    if (owner === null) throw new OwnershipRefusal("unknown-owner", consumer.id, consumer.consumer);
    if (!owner.acceptances.includes(consumer.acceptance)) {
      throw new OwnershipRefusal("acceptance-nonexistent", consumer.id, consumer.acceptance);
    }
  }
}

/** The authority a policy catalog implies for itself, for a run with no controller. */
export function selfDeclaredPolicyAuthority(policy, sourceKinds) {
  const consumers = policy.consumers.map((consumer) => ({
    owner: consumer.consumer,
    path: [],
    subblocks: [],
    acceptance: consumer.acceptance,
  }));
  const { inventory } = policyTransfer(policy, sourceKinds);
  return selfDeclaredAuthority({ rows: [...inventory.rows, ...consumers] });
}

function readRepositoryFile(file) {
  const path = resolve(REPOSITORY_ROOT, file);
  return existsSync(path) ? readFileSync(path, "utf8") : null;
}

const slug = (heading) =>
  heading
    .toLowerCase()
    .replace(/[^a-z0-9 -]/gu, "")
    .replace(/ /gu, "-");

/**
 * Contract-document sources must resolve to a heading. Displaced routes are read
 * only under --live: their owners delete those sources, and that deletion must not
 * turn this contract's own check red.
 */
export function validateDocumentRefs(population, read = readRepositoryFile) {
  for (const member of population) {
    if (member.role === "displaced") continue;
    const [file, anchor] = member.sourceRef.split("#");
    const text = read(file);
    if (text === null) throw new OwnershipRefusal("source-missing", member.id, file);
    const headings = [...text.matchAll(/^#{2,4} (.+)$/gmu)].map((match) => slug(match[1]));
    if (!headings.includes(anchor)) {
      throw new OwnershipRefusal("source-missing", member.id, member.sourceRef);
    }
  }
}

const PRODUCTION_KINDS = new Set(["kind:implementation", "kind:convergence"]);

/** Parses one node's authority from the controller's task record and charter text. */
export function nodeAuthority(task, charter) {
  return {
    predecessors: task.predecessors.map((node) => node.slug),
    production: task.labels.some((label) => PRODUCTION_KINDS.has(label)) && task.sizing.maxLoc > 0,
    optional: task.optional === true,
    acceptances: [...charter.matchAll(/\*\*([A-Z0-9]+-AC[A-Z0-9-]*)\b/gu)].map((match) => match[1]),
    subblocks: [...charter.matchAll(/^#### ([A-Z0-9]+-SB\d+) - /gmu)].map((match) => match[1]),
  };
}

function runController(args) {
  // The controller CLI is a shell shim on every platform. Node identifiers are
  // checked by the caller and every other argument is a literal.
  const result = spawnSync(`tama-plan-verter ${args.join(" ")}`, {
    encoding: "utf8",
    maxBuffer: 2 ** 24,
    shell: true,
  });
  return result.status === 0 ? result.stdout : null;
}

export function controllerAuthority(run = runController) {
  const cache = new Map();
  return (node) => {
    if (!/^[A-Z][A-Z0-9-]*$/u.test(node)) return null;
    if (!cache.has(node)) {
      const task = run(["task", "--plan", PLAN, node, "--json"]);
      const charter = run(["charter", "--plan", PLAN, node]);
      cache.set(
        node,
        task === null || charter === null ? null : nodeAuthority(JSON.parse(task), charter),
      );
    }
    return cache.get(node);
  };
}

/**
 * The authority the catalog itself implies, for a run with no controller: it proves
 * coverage, uniqueness and internal consistency, and claims nothing about the DAG.
 */
export function selfDeclaredAuthority(inventory) {
  const nodes = new Map();
  const node = (id) => {
    if (!nodes.has(id)) {
      nodes.set(id, {
        predecessors: [],
        production: true,
        optional: false,
        acceptances: [],
        subblocks: [],
      });
    }
    return nodes.get(id);
  };
  for (const row of inventory.rows) {
    row.path.slice(1).forEach((id, index) => node(id).predecessors.push(row.path[index]));
    const owner = node(row.owner);
    owner.acceptances.push(row.acceptance);
    owner.subblocks.push(...row.subblocks);
    if (row.applicability === "optional-channel") owner.optional = true;
  }
  return (id) => nodes.get(id) ?? null;
}

function loadContract() {
  const json = (name) => JSON.parse(readFileSync(resolve(PACKAGE_ROOT, name), "utf8"));
  return {
    inventory: parseInventory(
      readFileSync(resolve(PACKAGE_ROOT, "catalogs/engine-validation-ownership.toml"), "utf8"),
    ),
    population: json("products/engine-artifact-contract.v1.json").ownership.population,
    schema: json("schemas/engine-validation-ownership.schema.json"),
    policy: json(POLICY_PRODUCT),
    policySourceKinds: json(
      "schemas/engine-artifact.schema.json",
    ).$defs.EngineSourceKind.enum.filter((kind) => kind !== BROWSER_SOURCE_KIND),
  };
}

export function main(argv, authorityFor = controllerAuthority) {
  const mode = argv.length === 1 ? argv[0] : undefined;
  if (mode !== "--structural" && mode !== "--live") {
    throw new OwnershipRefusal("usage", "pass exactly one of --structural or --live");
  }
  const { inventory, population, schema, policy, policySourceKinds } = loadContract();
  validate(inventory, schema);
  validateDocumentRefs(population);
  const live = mode === "--live" ? authorityFor() : null;
  validateOwnership(inventory, population, live ?? selfDeclaredAuthority(inventory));
  validatePolicyTransfer(
    policy,
    policySourceKinds,
    live ?? selfDeclaredPolicyAuthority(policy, policySourceKinds),
  );
  if (mode === "--live") {
    for (const member of population) {
      if (member.role !== "displaced") continue;
      if (readRepositoryFile(member.sourceRef) === null) {
        throw new OwnershipRefusal("source-missing", member.id, member.sourceRef);
      }
    }
  }
  return (
    `${mode}: ${inventory.rows.length} bindings cover ${population.length} members, ` +
    `plus the policy's ${policy.sources.length} sources and ${policy.displacedRoutes.length} routes` +
    (mode === "--live" ? "; owners resolved against the controller" : "; controller not consulted")
  );
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    console.log(main(process.argv.slice(2)));
  } catch (error) {
    if (!(error instanceof OwnershipRefusal || error instanceof SchemaViolation)) throw error;
    console.error(error.message);
    process.exitCode = 1;
  }
}
