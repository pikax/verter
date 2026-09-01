import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const NODE_FIELDS = [
  "id",
  "name",
  "predecessors",
  "phase",
  "train",
  "product",
  "kind",
  "semantic_role",
  "class",
  "owner",
  "conflict_domains",
  "resource_class",
  "gate_profile",
  "review_profile",
  "implementation_effort_min",
  "implementation_effort_default",
  "review_effort_min",
  "review_effort_default",
  "verification_effort_min",
  "verification_effort_default",
  "confirmation_effort_min",
  "confirmation_effort_default",
  "dispatchable",
  "optional",
  "release_gating",
  "external_requirements",
  "charter",
  "size",
  "max_production_loc",
  "max_production_files",
  "max_related_packages",
  "rescope_loc",
  "rescope_files",
  "rescope_unrelated_packages",
];

const REQUIRED_NODE_FIELDS = NODE_FIELDS;
const OPTIONAL_NODE_FIELDS = ["gh_milestone"];
const ARRAY_FIELDS = new Set(["predecessors", "conflict_domains", "external_requirements"]);
const BOOL_FIELDS = new Set(["dispatchable", "optional"]);
const INT_FIELDS = new Set([
  "max_production_loc",
  "max_production_files",
  "max_related_packages",
  "rescope_loc",
  "rescope_files",
  "rescope_unrelated_packages",
]);
const FORBIDDEN_KEYS = new Set(["__proto__", "prototype", "constructor"]);
export function normalizeProductionSurface(surface, nodeId = "unknown node") {
  const parts = typeof surface === "string" ? surface.split("/") : [];
  if (
    typeof surface !== "string" ||
    !/^[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*$/.test(surface) ||
    parts.some((part) => part === "." || part === "..")
  )
    throw new Error(`${nodeId}: production surface is not a repository path: ${surface}`);
  return surface.replace(/\/+$/, "");
}

function assertSafeKey(key, lineNumber) {
  if (FORBIDDEN_KEYS.has(key))
    throw new Error(`TOML line ${lineNumber}: unsafe prototype-bearing key ${key}`);
}

function safeRelative(relative, label) {
  if (
    typeof relative !== "string" ||
    !relative ||
    relative.includes("\\") ||
    relative.includes("\0") ||
    path.posix.isAbsolute(relative) ||
    path.win32.isAbsolute(relative)
  )
    throw new Error(`${label}: unsafe path ${relative}`);
  const parts = relative.split("/");
  if (parts.some((part) => !part || part === "." || part === ".." || FORBIDDEN_KEYS.has(part)))
    throw new Error(`${label}: unsafe path ${relative}`);
  return parts;
}

function isInside(root, candidate) {
  const relative = path.relative(root, candidate);
  return (
    relative === "" ||
    (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))
  );
}

export function confinedFile(root, relative, label = "authority input") {
  const parts = safeRelative(relative, label);
  const absoluteRoot = path.resolve(root);
  const rootReal = fs.realpathSync(absoluteRoot);
  let cursor = absoluteRoot;
  for (const part of parts) {
    cursor = path.join(cursor, part);
    const stat = fs.lstatSync(cursor);
    if (stat.isSymbolicLink()) throw new Error(`${label}: symlink is forbidden: ${relative}`);
  }
  const absolute = path.resolve(absoluteRoot, ...parts);
  const real = fs.realpathSync(absolute);
  if (!isInside(rootReal, real)) throw new Error(`${label}: path is not confined: ${relative}`);
  if (!fs.statSync(real).isFile()) throw new Error(`${label}: expected regular file: ${relative}`);
  return real;
}

function stripComment(line) {
  let quoted = false;
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (char === "\\" && quoted) {
      escaped = true;
      continue;
    }
    if (char === '"') quoted = !quoted;
    if (char === "#" && !quoted) return line.slice(0, index);
  }
  return line;
}

function parseValue(raw, lineNumber) {
  const value = raw.trim();
  if (value.startsWith('"')) {
    try {
      return JSON.parse(value);
    } catch (error) {
      throw new Error(`TOML line ${lineNumber}: invalid string: ${error.message}`);
    }
  }
  if (value.startsWith("[")) {
    try {
      const parsed = JSON.parse(value);
      if (!Array.isArray(parsed)) throw new Error("not an array");
      return parsed;
    } catch (error) {
      throw new Error(`TOML line ${lineNumber}: invalid array: ${error.message}`);
    }
  }
  if (value === "true") return true;
  if (value === "false") return false;
  if (/^-?\d+$/u.test(value)) {
    const parsed = Number(value);
    if (!Number.isSafeInteger(parsed))
      throw new Error(`TOML line ${lineNumber}: integer is not safe: ${value}`);
    return parsed;
  }
  throw new Error(`TOML line ${lineNumber}: unsupported value ${value}`);
}

export function parseToml(text) {
  if (typeof text !== "string") throw new Error("TOML input must be a string");
  const root = {};
  let target = root;
  const declaredTables = new Set();
  for (const [index, original] of text.replaceAll("\r\n", "\n").split("\n").entries()) {
    const lineNumber = index + 1;
    const line = stripComment(original).trim();
    if (!line) continue;
    const arrayTable = line.match(/^\[\[([A-Za-z0-9_.-]+)\]\]$/u);
    if (arrayTable) {
      const key = arrayTable[1];
      if (key.includes("."))
        throw new Error(`TOML line ${lineNumber}: nested array tables are unsupported`);
      assertSafeKey(key, lineNumber);
      if (root[key] !== undefined && !Array.isArray(root[key]))
        throw new Error(`TOML line ${lineNumber}: table type conflict ${key}`);
      root[key] ||= [];
      target = {};
      root[key].push(target);
      continue;
    }
    const table = line.match(/^\[([A-Za-z0-9_.-]+)\]$/u);
    if (table) {
      const parts = table[1].split(".");
      for (const part of parts) assertSafeKey(part, lineNumber);
      const tableName = parts.join(".");
      if (declaredTables.has(tableName))
        throw new Error(`TOML line ${lineNumber}: duplicate table ${tableName}`);
      declaredTables.add(tableName);
      target = root;
      for (const part of parts) {
        if (
          target[part] !== undefined &&
          (typeof target[part] !== "object" || Array.isArray(target[part]))
        )
          throw new Error(`TOML line ${lineNumber}: table type conflict ${part}`);
        target[part] ||= {};
        target = target[part];
      }
      continue;
    }
    const assignment = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/u);
    if (!assignment) throw new Error(`TOML line ${lineNumber}: malformed statement`);
    const [, key, raw] = assignment;
    assertSafeKey(key, lineNumber);
    if (Object.hasOwn(target, key))
      throw new Error(`TOML line ${lineNumber}: duplicate key ${key}`);
    target[key] = parseValue(raw, lineNumber);
  }
  return root;
}

export function readToml(file) {
  try {
    return parseToml(fs.readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`${file}: ${error.message}`);
  }
}

function schemaTypeMatches(value, type) {
  if (type === "object")
    return value !== null && typeof value === "object" && !Array.isArray(value);
  if (type === "array") return Array.isArray(value);
  if (type === "integer") return Number.isSafeInteger(value);
  if (type === "number") return typeof value === "number" && Number.isFinite(value);
  return typeof value === type;
}

function schemaErrors(value, schema, location) {
  const errors = [];
  if (schema.const !== undefined && JSON.stringify(value) !== JSON.stringify(schema.const))
    errors.push(`${location}: expected constant ${JSON.stringify(schema.const)}`);
  if (Array.isArray(schema.enum) && !schema.enum.includes(value))
    errors.push(
      `${location}: expected one of ${schema.enum.map((item) => JSON.stringify(item)).join(", ")}`,
    );
  if (schema.type && !schemaTypeMatches(value, schema.type)) {
    errors.push(`${location}: expected ${schema.type}`);
    return errors;
  }
  if (schema.type === "object") {
    for (const key of schema.required || [])
      if (!Object.hasOwn(value, key)) errors.push(`${location}: missing required property ${key}`);
    const properties = schema.properties || {};
    if (schema.additionalProperties === false)
      for (const key of Object.keys(value))
        if (!Object.hasOwn(properties, key)) errors.push(`${location}: additional property ${key}`);
    for (const [key, child] of Object.entries(properties))
      if (Object.hasOwn(value, key))
        errors.push(...schemaErrors(value[key], child, `${location}.${key}`));
  } else if (schema.type === "array") {
    if (schema.minItems !== undefined && value.length < schema.minItems)
      errors.push(`${location}: requires at least ${schema.minItems} items`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems)
      errors.push(`${location}: allows at most ${schema.maxItems} items`);
    if (
      schema.uniqueItems &&
      new Set(value.map((item) => JSON.stringify(item))).size !== value.length
    )
      errors.push(`${location}: array items must be unique`);
    if (schema.items)
      for (const [index, item] of value.entries())
        errors.push(...schemaErrors(item, schema.items, `${location}[${index}]`));
  } else if (schema.type === "string") {
    if (schema.minLength !== undefined && value.length < schema.minLength)
      errors.push(`${location}: string is shorter than ${schema.minLength}`);
    if (schema.pattern && !new RegExp(schema.pattern, "u").test(value))
      errors.push(`${location}: string does not match ${schema.pattern}`);
  } else if (schema.type === "integer" || schema.type === "number") {
    if (schema.minimum !== undefined && value < schema.minimum)
      errors.push(`${location}: value is below ${schema.minimum}`);
  }
  return errors;
}

export function validateSchemaObject(value, schema, location = "object") {
  return schemaErrors(value, schema, location);
}

const FINDING_CARRY_FORWARD_SCHEMA = "finding-carry-forward.schema.json";
const FINDING_CARRY_FORWARD_REQUIRED = ["issue", "severity", "owner"];
const FINDING_CARRY_FORWARD_FORBIDDEN_FIELDS = new Set([
  "dag_id",
  "node_id",
  "predecessors",
  "closed",
  "labels",
  "ready",
  "implemented",
  "status",
  "train",
  "pull_request",
]);

export function validateFindingCarryForward(
  value,
  location = "finding-carry-forward",
  packageRoot = PACKAGE_ROOT,
) {
  return validateSchemaObject(
    value,
    loadSchema(packageRoot, FINDING_CARRY_FORWARD_SCHEMA),
    location,
  );
}

function validateFindingCarryForwardSchema(packageRoot) {
  const errors = [];
  try {
    const schema = loadSchema(packageRoot, FINDING_CARRY_FORWARD_SCHEMA);
    if (schema.additionalProperties !== false)
      errors.push(`${FINDING_CARRY_FORWARD_SCHEMA}: additionalProperties must be false`);
    const required = schema.required || [];
    for (const key of FINDING_CARRY_FORWARD_REQUIRED)
      if (!required.includes(key))
        errors.push(`${FINDING_CARRY_FORWARD_SCHEMA}: ${key} must be required`);
    for (const key of Object.keys(schema.properties || {}))
      if (FINDING_CARRY_FORWARD_FORBIDDEN_FIELDS.has(key))
        errors.push(`${FINDING_CARRY_FORWARD_SCHEMA}: forbidden field ${key}`);
  } catch (error) {
    errors.push(error.message);
  }
  return errors;
}

function loadSchema(packageRoot, name) {
  const file = confinedFile(path.join(packageRoot, "schemas"), name, `schema ${name}`);
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`schema parse failure ${name}: ${error.message}`);
  }
}

export function loadAuthority(packageRoot = PACKAGE_ROOT) {
  const rootFile = confinedFile(packageRoot, "authority/root.toml", "authority root");
  const metadata = readToml(rootFile);
  if (!Array.isArray(metadata.modules)) throw new Error(`${rootFile}: modules must be an array`);
  const nodes = [];
  const moduleModels = [];
  for (const relative of metadata.modules) {
    if (typeof relative !== "string" || !relative.startsWith("dag/") || !relative.endsWith(".toml"))
      throw new Error(`${rootFile}: invalid module path ${relative}`);
    const file = confinedFile(
      path.join(packageRoot, "authority"),
      relative,
      "authority DAG module",
    );
    const model = readToml(file);
    if (!Array.isArray(model.node)) throw new Error(`${file}: missing [[node]] rows`);
    moduleModels.push({ relative, file, model });
    nodes.push(...model.node.map((node) => ({ ...node, _module: relative })));
  }
  const ledgerFile = confinedFile(
    path.join(packageRoot, "authority"),
    metadata.implemented_ledger,
    "implemented-node ledger",
  );
  const ledger = readToml(ledgerFile);
  if (!Array.isArray(ledger.implemented))
    throw new Error(`${ledgerFile}: missing [[implemented]] rows`);
  return { packageRoot, rootFile, metadata, nodes, moduleModels, ledgerFile, ledger };
}

function predecessorsOf(node) {
  return Array.isArray(node?.predecessors) ? node.predecessors : [];
}

function graphMaps(nodes) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const children = new Map(nodes.map((node) => [node.id, []]));
  for (const node of nodes)
    for (const predecessor of predecessorsOf(node))
      if (children.has(predecessor)) children.get(predecessor).push(node.id);
  return { byId, children };
}

export function topological(nodes) {
  const { byId, children } = graphMaps(nodes);
  const indegree = new Map(nodes.map((node) => [node.id, 0]));
  for (const node of nodes)
    for (const predecessor of predecessorsOf(node))
      if (byId.has(predecessor)) indegree.set(node.id, indegree.get(node.id) + 1);
  const ready = [...indegree]
    .filter(([, degree]) => degree === 0)
    .map(([id]) => id)
    .sort();
  const order = [];
  while (ready.length) {
    const id = ready.shift();
    order.push(byId.get(id));
    for (const child of (children.get(id) || []).sort()) {
      indegree.set(child, indegree.get(child) - 1);
      if (indegree.get(child) === 0) {
        ready.push(child);
        ready.sort();
      }
    }
  }
  return {
    order,
    cyclic: order.length !== nodes.length,
    unresolved: [...indegree]
      .filter(([, degree]) => degree > 0)
      .map(([id]) => id)
      .sort(),
  };
}

function criticalPath(nodes) {
  const { order } = topological(nodes);
  const distance = new Map();
  const prior = new Map();
  for (const node of order) {
    let best = 0;
    let bestId = null;
    for (const predecessor of predecessorsOf(node)) {
      const candidate = distance.get(predecessor) || 0;
      if (candidate > best) {
        best = candidate;
        bestId = predecessor;
      }
    }
    distance.set(node.id, best + 1);
    prior.set(node.id, bestId);
  }
  const end = [...distance].sort(
    (left, right) => right[1] - left[1] || left[0].localeCompare(right[0]),
  )[0] || [null, 0];
  const ids = [];
  for (let id = end[0]; id; id = prior.get(id)) ids.push(id);
  return { length: end[1], nodes: ids.reverse() };
}

function topologyWidths(nodes) {
  const { order } = topological(nodes);
  const level = new Map();
  const levels = {};
  for (const node of order) {
    const value = 1 + Math.max(0, ...predecessorsOf(node).map((id) => level.get(id) || 0));
    level.set(node.id, value);
    levels[value] = (levels[value] || 0) + 1;
  }
  return { max: Math.max(0, ...Object.values(levels)), levels };
}

export function metrics(authority) {
  return {
    nodes: authority.nodes.length,
    edges: authority.nodes.reduce((sum, node) => sum + predecessorsOf(node).length, 0),
    modules: authority.moduleModels.length,
    charters: authority.nodes.length,
    critical_path: criticalPath(authority.nodes),
    topological_width: topologyWidths(authority.nodes),
  };
}

function parseCharterHeader(text) {
  const match = text.match(/^<!-- unified-charter-v2\n([\s\S]*?)\n-->/u);
  if (!match) return null;
  const result = {};
  for (const line of match[1].split("\n")) {
    const [key, ...rest] = line.split("=");
    if (!key || !rest.length || Object.hasOwn(result, key)) return null;
    result[key] = rest.join("=");
  }
  return result;
}

export function validateCharters(nodes, packageRoot = PACKAGE_ROOT) {
  const errors = [];
  const expectedCharters = new Set();
  for (const node of nodes) {
    if (expectedCharters.has(node.charter))
      errors.push(`${node.id}: duplicate charter path ${node.charter}`);
    expectedCharters.add(node.charter);
    let file;
    try {
      file = confinedFile(packageRoot, node.charter, `${node.id} charter`);
    } catch (error) {
      errors.push(`${node.id}: ${error.message}`);
      continue;
    }
    const header = parseCharterHeader(fs.readFileSync(file, "utf8"));
    if (!header) {
      errors.push(`${node.id}: missing charter metadata header`);
      continue;
    }
    for (const key of NODE_FIELDS) {
      const value = node[key];
      const expected = Array.isArray(value)
        ? value.join(",")
        : value === undefined
          ? ""
          : String(value);
      if (header[key] !== expected)
        errors.push(
          `${node.id}: charter header ${key} differs from DAG (${header[key]} != ${expected})`,
        );
    }
  }
  const charterRoot = path.join(packageRoot, "charters");
  if (!fs.existsSync(charterRoot))
    errors.push(`charter inventory: missing directory ${charterRoot}`);
  else {
    const inventory = exactRegularFileInventory(charterRoot, "charter inventory");
    errors.push(...inventory.errors);
    const actualCharters = new Set(inventory.files.map((relative) => `charters/${relative}`));
    for (const relative of actualCharters) {
      if (!relative.endsWith(".md")) errors.push(`charter inventory: unsupported file ${relative}`);
      else if (!expectedCharters.has(relative))
        errors.push(`charter inventory: orphan file ${relative}`);
    }
    for (const relative of expectedCharters)
      if (!actualCharters.has(relative)) errors.push(`charter inventory: missing file ${relative}`);
  }
  return errors;
}

export function validateGraphModel(nodes, options = {}) {
  const errors = [];
  const byId = new Map();
  for (const node of nodes) {
    for (const key of Object.keys(node))
      if (!NODE_FIELDS.includes(key) && !OPTIONAL_NODE_FIELDS.includes(key) && key !== "_module")
        errors.push(`${node.id}: unknown field ${key}`);
    if (typeof node.id !== "string" || !/^[A-Z][A-Z0-9-]*$/u.test(node.id))
      errors.push(`invalid node id ${node.id}`);
    if (byId.has(node.id)) errors.push(`duplicate node id ${node.id}`);
    byId.set(node.id, node);
    for (const field of REQUIRED_NODE_FIELDS) {
      if (!Object.hasOwn(node, field)) errors.push(`${node.id}: missing field ${field}`);
      else if (ARRAY_FIELDS.has(field) && !Array.isArray(node[field]))
        errors.push(`${node.id}: ${field} must be an array`);
      else if (BOOL_FIELDS.has(field) && typeof node[field] !== "boolean")
        errors.push(`${node.id}: ${field} must be boolean`);
      else if (INT_FIELDS.has(field) && (!Number.isInteger(node[field]) || node[field] < 0))
        errors.push(`${node.id}: ${field} must be a non-negative integer`);
      else if (
        !ARRAY_FIELDS.has(field) &&
        !BOOL_FIELDS.has(field) &&
        !INT_FIELDS.has(field) &&
        typeof node[field] !== "string"
      )
        errors.push(`${node.id}: ${field} must be a string`);
    }
    for (const field of OPTIONAL_NODE_FIELDS) {
      if (
        Object.hasOwn(node, field) &&
        (typeof node[field] !== "string" || node[field].length === 0)
      ) {
        errors.push(`${node.id}: ${field} must be a non-empty string`);
      }
    }
    const predecessors = predecessorsOf(node);
    if (new Set(predecessors).size !== predecessors.length)
      errors.push(`${node.id}: duplicate predecessor`);
    if (predecessors.includes(node.id)) errors.push(`${node.id}: self predecessor`);
  }
  for (const node of nodes)
    for (const predecessor of predecessorsOf(node))
      if (!byId.has(predecessor)) errors.push(`${node.id}: missing predecessor ${predecessor}`);
  const topo = topological(nodes);
  if (topo.cyclic) errors.push(`cycle detected: ${topo.unresolved.join(", ")}`);
  if (!options.skipCharters && options.packageRoot)
    errors.push(...validateCharters(nodes, options.packageRoot));
  return errors;
}

const RETAINED_CATALOG_SCHEMAS = [
  ["conflict-domains.toml", "conflict-domain-catalog.schema.json", "domain", "id"],
  ["gate-profiles.toml", "gate-profile-catalog.schema.json", "profile", "id"],
  ["review-profiles.toml", "review-profile-catalog.schema.json", "profile", "id"],
  ["resource-profiles.toml", "resource-profile-catalog.schema.json", "profile", "id"],
  [
    "native-checker-family-manifest.toml",
    "native-checker-family-manifest.schema.json",
    "slice",
    "id",
  ],
  ["github-milestones.toml", "github-milestone-catalog.schema.json", "milestone", "title"],
  ["github-issue-content.toml", "github-issue-content-catalog.schema.json", "issue", "node_id"],
  ["github-train-issues.toml", "github-train-issue-catalog.schema.json", "train_issue", "train"],
];

export function validateRetainedCatalogSchemas(packageRoot = PACKAGE_ROOT) {
  const errors = [];
  for (const [fileName, schemaName, table, identityField] of RETAINED_CATALOG_SCHEMAS) {
    try {
      const model = readToml(path.join(packageRoot, "catalogs", fileName));
      errors.push(
        ...validateSchemaObject(model, loadSchema(packageRoot, schemaName), `catalogs.${fileName}`),
      );
      const seen = new Set();
      for (const row of model[table] || []) {
        const identity = row[identityField];
        if (typeof identity !== "string") continue;
        if (seen.has(identity))
          errors.push(`catalogs.${fileName}: duplicate ${table} ${identityField} ${identity}`);
        seen.add(identity);
      }
    } catch (error) {
      errors.push(error.message);
    }
  }
  return errors;
}

function validateStaticSchemas(authority) {
  const errors = [];
  try {
    errors.push(
      ...validateSchemaObject(
        authority.metadata,
        loadSchema(authority.packageRoot, "root.schema.json"),
        "authority.root",
      ),
    );
    const moduleSchema = loadSchema(authority.packageRoot, "dag-module.schema.json");
    const nodeSchema = loadSchema(authority.packageRoot, "node.schema.json");
    for (const { relative, model } of authority.moduleModels) {
      errors.push(...validateSchemaObject(model, moduleSchema, `authority.${relative}`));
      for (const [index, node] of model.node.entries())
        errors.push(
          ...validateSchemaObject(node, nodeSchema, `authority.${relative}.node[${index}]`),
        );
    }
  } catch (error) {
    errors.push(error.message);
  }
  errors.push(...validateRetainedCatalogSchemas(authority.packageRoot));
  errors.push(...validateFindingCarryForwardSchema(authority.packageRoot));
  return errors;
}

export function validateGitHubProgramCatalog(authority, githubProgram) {
  const errors = [];
  try {
    errors.push(
      ...validateSchemaObject(
        githubProgram,
        loadSchema(authority.packageRoot, "github-control-plane-program.schema.json"),
        "catalogs.github-control-plane-program",
      ),
    );
  } catch (error) {
    errors.push(error.message);
  }
  const githubTrains = new Set([
    "governance.github-control-plane",
    "governance.feedback-intake",
    "governance.release-control",
  ]);
  const expected = authority.nodes.filter((node) => githubTrains.has(node.train));
  const expectedById = new Map(expected.map((node) => [node.id, node]));
  const seen = new Set();
  for (const row of githubProgram.node || []) {
    if (seen.has(row.id)) errors.push(`GitHub program catalog: duplicate node ${row.id}`);
    seen.add(row.id);
    const node = expectedById.get(row.id);
    if (!node) {
      errors.push(`GitHub program catalog: unexpected node ${row.id}`);
      continue;
    }
    if (row.name !== node.name)
      errors.push(`GitHub program catalog: ${row.id} name differs from authority`);
    if (row.train !== node.train)
      errors.push(`GitHub program catalog: ${row.id} train differs from authority`);
    if (JSON.stringify(row.predecessors || []) !== JSON.stringify(node.predecessors || []))
      errors.push(`GitHub program catalog: ${row.id} predecessors differ from authority`);
  }
  for (const node of expected)
    if (!seen.has(node.id)) errors.push(`GitHub program catalog: missing node ${node.id}`);
  return errors;
}

function validateCatalogReferences(authority) {
  const errors = [];
  const catalogs = [
    ["conflict-domains.toml", "domain", "conflict_domains"],
    ["gate-profiles.toml", "profile", "gate_profile"],
    ["review-profiles.toml", "profile", "review_profile"],
    ["resource-profiles.toml", "profile", "resource_class"],
  ];
  for (const [fileName, table, field] of catalogs) {
    const model = readToml(path.join(authority.packageRoot, "catalogs", fileName));
    const ids = new Set((model[table] || []).map((row) => row.id));
    for (const node of authority.nodes) {
      const values = Array.isArray(node[field]) ? node[field] : [node[field]];
      for (const value of values)
        if (!ids.has(value)) errors.push(`${node.id}: unknown ${field} ${value}`);
    }
  }
  const milestoneCatalog = readToml(
    path.join(authority.packageRoot, "catalogs", "github-milestones.toml"),
  );
  const milestoneTitles = new Set((milestoneCatalog.milestone || []).map((row) => row.title));
  for (const node of authority.nodes) {
    if (node.gh_milestone != null && !milestoneTitles.has(node.gh_milestone)) {
      errors.push(`${node.id}: unknown gh_milestone ${node.gh_milestone}`);
    }
  }
  const issueContentCatalog = readToml(
    path.join(authority.packageRoot, "catalogs", "github-issue-content.toml"),
  );
  const nodeIds = new Set(authority.nodes.map((node) => node.id));
  for (const row of issueContentCatalog.issue || []) {
    if (!nodeIds.has(row.node_id)) {
      errors.push(`github-issue-content.toml: unknown node_id ${row.node_id}`);
    }
  }
  const trainIssueCatalog = readToml(
    path.join(authority.packageRoot, "catalogs", "github-train-issues.toml"),
  );
  const knownTrains = new Set(authority.nodes.map((node) => node.train));
  for (const row of trainIssueCatalog.train_issue || []) {
    if (!knownTrains.has(row.train)) {
      errors.push(`github-train-issues.toml: unknown train ${row.train}`);
    }
    if (!milestoneTitles.has(row.gh_milestone)) {
      errors.push(`github-train-issues.toml: unknown gh_milestone ${row.gh_milestone}`);
    }
  }
  const githubProgram = readToml(
    path.join(authority.packageRoot, "catalogs", "github-control-plane-program.toml"),
  );
  errors.push(...validateGitHubProgramCatalog(authority, githubProgram));
  return errors;
}

export function validateAuthority(authority, options = {}) {
  const errors = [];
  const requiredRoot = [
    ["schema", "number"],
    ["revision", "number"],
    ["package", "string"],
    ["implementation_state", "string"],
    ["commit_locator_fields", "object"],
    ["implemented_ledger", "string"],
    ["final_rev11_gate", "string"],
    ["successor_promotion_gate", "string"],
    ["modules", "object"],
  ];
  for (const [key, type] of requiredRoot)
    if (typeof authority.metadata[key] !== type) errors.push(`authority root: invalid ${key}`);
  if (authority.metadata.implementation_state !== "ledger_presence")
    errors.push("authority root: implementation_state must be ledger_presence");
  const knownNodes = new Set(authority.nodes.map((node) => node.id));
  const finalRev11Gate = authority.metadata.final_rev11_gate;
  const successorPromotionGate = authority.metadata.successor_promotion_gate;
  if (typeof finalRev11Gate === "string" && !knownNodes.has(finalRev11Gate))
    errors.push(`authority root: unknown final Rev11 gate ${finalRev11Gate}`);
  if (typeof successorPromotionGate === "string" && !knownNodes.has(successorPromotionGate))
    errors.push(`authority root: unknown successor promotion gate ${successorPromotionGate}`);
  const successorPromotionNode = authority.nodes.find((node) => node.id === successorPromotionGate);
  if (
    knownNodes.has(finalRev11Gate) &&
    knownNodes.has(successorPromotionGate) &&
    (!Array.isArray(successorPromotionNode?.predecessors) ||
      !successorPromotionNode.predecessors.includes(finalRev11Gate))
  )
    errors.push(
      `authority root: successor promotion gate ${successorPromotionGate} must directly depend on final Rev11 gate ${finalRev11Gate}`,
    );
  try {
    errors.push(
      ...validateSchemaObject(
        authority.ledger,
        loadSchema(authority.packageRoot, "implementation-ledger.schema.json"),
        "authority.implementation-ledger",
      ),
    );
  } catch (error) {
    errors.push(error.message);
  }
  const implemented = new Set();
  for (const row of authority.ledger.implemented) {
    if (!knownNodes.has(row.node_id))
      errors.push(`implementation ledger: unknown node ${row.node_id}`);
    if (implemented.has(row.node_id))
      errors.push(`implementation ledger: duplicate node ${row.node_id}`);
    implemented.add(row.node_id);
  }
  const mappedNodes = new Set();
  const mappedIssues = new Set();
  // Identity is {node_id, gh_issue} unique both ways; sync_to_github is not a key.
  for (const row of authority.ledger.github_issue || []) {
    if (!knownNodes.has(row.node_id))
      errors.push(`GitHub issue ledger: unknown node ${row.node_id}`);
    if (mappedNodes.has(row.node_id))
      errors.push(`GitHub issue ledger: duplicate node ${row.node_id}`);
    if (mappedIssues.has(row.gh_issue))
      errors.push(`GitHub issue ledger: duplicate issue ${row.gh_issue}`);
    mappedNodes.add(row.node_id);
    mappedIssues.add(row.gh_issue);
  }
  const mappedTrains = new Set();
  for (const row of authority.ledger.github_train_issue || []) {
    if (!authority.nodes.some((node) => node.train === row.train)) {
      errors.push(`GitHub train issue ledger: unknown train ${row.train}`);
    }
    if (mappedTrains.has(row.train)) {
      errors.push(`GitHub train issue ledger: duplicate train ${row.train}`);
    }
    if (mappedIssues.has(row.gh_issue)) {
      errors.push(`GitHub train issue ledger: duplicate issue ${row.gh_issue}`);
    }
    mappedTrains.add(row.train);
    mappedIssues.add(row.gh_issue);
  }
  if (options.strict) errors.push(...validateStaticSchemas(authority));
  errors.push(...validateGraphModel(authority.nodes, { packageRoot: authority.packageRoot }));
  errors.push(...validateCatalogReferences(authority));
  return errors;
}

export function listGitHubIssues(ledger) {
  return [...(ledger?.github_issue || [])]
    .map((row) => ({
      node_id: row.node_id,
      gh_issue: row.gh_issue,
      sync_to_github: row.sync_to_github,
    }))
    .sort((left, right) => left.node_id.localeCompare(right.node_id));
}

export function githubIssueByNumber(ledger, issue) {
  if (!Number.isSafeInteger(issue) || issue < 1)
    throw new Error("GitHub issue lookup requires a positive safe integer");
  const row = listGitHubIssues(ledger).find((candidate) => candidate.gh_issue === issue);
  if (!row) throw new Error(`GitHub issue #${issue} is not mapped`);
  return row;
}

export function deriveState(authority, options = {}) {
  const commits = new Map(
    (options.implemented || authority.ledger.implemented).map((row) => [
      row.node_id,
      {
        commitMessage: row.commit_message,
        commitDate: row.commit_date,
        pullRequest: row.pull_request,
        locator: `${row.commit_message} @ ${row.commit_date}${row.pull_request === undefined ? "" : ` (PR #${row.pull_request})`}`,
      },
    ]),
  );
  const states = new Map();
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const ancestorCache = new Map();
  const ancestorsOf = (id) => {
    if (ancestorCache.has(id)) return ancestorCache.get(id);
    const ancestors = new Set();
    for (const predecessor of predecessorsOf(byId.get(id))) {
      ancestors.add(predecessor);
      for (const ancestor of ancestorsOf(predecessor)) ancestors.add(ancestor);
    }
    ancestorCache.set(id, ancestors);
    return ancestors;
  };
  const { order } = topological(authority.nodes);
  for (const node of order) {
    const commit = commits.get(node.id) || null;
    const complete = Boolean(commit);
    const missingAncestors = [...ancestorsOf(node.id)].filter((id) => !commits.has(id)).sort();
    let status;
    if (complete) status = "COMPLETE";
    else if (node.dispatchable && missingAncestors.length === 0) status = "READY";
    else status = "BLOCKED";
    states.set(node.id, {
      status,
      commit,
      missing_ancestors: missingAncestors,
    });
  }
  return {
    states,
    commits,
    errors: [],
  };
}

export function explainNode(authority, state, id) {
  const node = authority.nodes.find((candidate) => candidate.id === id);
  if (!node) throw new Error(`unknown node ${id}`);
  const row = state.states.get(id);
  return {
    id,
    name: node.name,
    status: row.status,
    commit: row.commit
      ? {
          message: row.commit.commitMessage,
          date: row.commit.commitDate,
          pull_request: row.commit.pullRequest ?? null,
          locator: row.commit.locator,
        }
      : null,
    missing_ancestors: row.missing_ancestors,
    external_requirements: node.external_requirements,
    charter: node.charter,
  };
}

export function packetFor(authority, state, id) {
  const node = authority.nodes.find((candidate) => candidate.id === id);
  if (!node) throw new Error(`unknown node ${id}`);
  const row = state.states.get(id);
  const charter = fs.readFileSync(
    confinedFile(authority.packageRoot, node.charter, `${id} charter`),
    "utf8",
  );
  return `# Tama work packet: ${id}\n\nStatus: ${row.status}\nName: ${node.name}\nTrain: ${node.train}\nPredecessors: ${node.predecessors.join(", ") || "none"}\nMissing ancestors: ${row.missing_ancestors.join(", ") || "none"}\nExternal requirements (agent-checked): ${node.external_requirements.join(", ") || "none"}\n\n## Completion ledger\n\nBefore squashing or starting review, add this trusted row to \`authority/${authority.metadata.implemented_ledger}\` as part of the implementation patch:\n\n    [[implemented]]\n    node_id = "${id}"\n    commit_message = "<planned squash commit message or useful search phrase>"\n    commit_date = "<approximate squash date with timezone>"\n    # pull_request = 1234 # optional; uncomment when known\n\nThen squash once using the planned message and review that candidate. No after-commit ledger update or amend is required. The row is authoritative by presence. The three commit fields are loose locator hints only. Tooling does not resolve or validate them, require an exact message/date match, compare content, inspect ancestry, or contact GitHub. If a message search returns several commits, use the date to choose the closest result; use the PR number when available.\n\n## Charter\n\n${charter}`;
}

export function exactRegularFileInventory(root, label = "inventory") {
  const files = [];
  const errors = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) errors.push(`${label}: symlink is forbidden: ${absolute}`);
      if (entry.isDirectory()) walk(absolute);
      else if (entry.isFile()) files.push(path.relative(root, absolute).replaceAll(path.sep, "/"));
      else if (!entry.isSymbolicLink()) errors.push(`${label}: unsupported entry: ${absolute}`);
    }
  };
  walk(root);
  return { files: files.sort(), errors };
}
