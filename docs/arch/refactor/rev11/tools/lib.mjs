import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import childProcess from "node:child_process";
import os from "node:os";
import { fileURLToPath } from "node:url";
import { createAmendment as createAuthorityAmendment, validateAmendments as validateAmendmentChain } from "./amendments.mjs";
import { assessNodeEffort, createLocalLifecycle, effortPolicyFor, readLocalAnchor, reinitializeLocalLifecycle } from "./trusted-local.mjs";

export const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const NODE_FIELDS = ["id", "name", "predecessors", "conditional_predecessors", "phase", "train", "product", "kind", "semantic_role", "class", "owner", "conflict_domains", "resource_class", "gate_profile", "review_profile", "implementation_effort_min", "implementation_effort_default", "review_effort_min", "review_effort_default", "verification_effort_min", "verification_effort_default", "confirmation_effort_min", "confirmation_effort_default", "dispatchable", "optional", "release_gating", "source_refs", "external_requirements", "activation_gate", "charter", "size", "max_production_loc", "max_production_files", "max_related_packages", "rescope_loc", "rescope_files", "rescope_unrelated_packages", "initial_state"];
const REQUIRED_NODE_FIELDS = NODE_FIELDS.filter((field) => field !== "initial_state");
const ARRAY_FIELDS = new Set(["predecessors", "conditional_predecessors", "conflict_domains", "source_refs", "external_requirements"]);
const BOOL_FIELDS = new Set(["dispatchable", "optional"]);
const INT_FIELDS = new Set(["max_production_loc", "max_production_files", "max_related_packages", "rescope_loc", "rescope_files", "rescope_unrelated_packages"]);
const sha256 = (value) => crypto.createHash("sha256").update(value).digest("hex");
const quote = (value) => JSON.stringify(String(value));
const renderArray = (values) => `[${values.map(quote).join(", ")}]`;
const isFullSha = (value) => /^[0-9a-f]{40}$/.test(value || "");
const isDigest = (value) => /^[0-9a-f]{64}$/.test(value || "");
const FORBIDDEN_KEYS = new Set(["__proto__", "prototype", "constructor"]);
const CHILD_PROCESS_TIMEOUT_MS = 30_000;

export function defaultRuntimeRoot(packageRoot = PACKAGE_ROOT) {
  const repository = gitRoot(packageRoot) || fs.realpathSync(packageRoot);
  return path.join(os.tmpdir(), "verter-rev11-unified-runtime", sha256(repository).slice(0, 20));
}

function assertSafeKey(key, lineNumber) {
  if (FORBIDDEN_KEYS.has(key)) throw new Error(`TOML line ${lineNumber}: unsafe prototype-bearing key ${key}`);
}

function safeRelative(relative, label) {
  if (typeof relative !== "string" || !relative || relative.includes("\\") || relative.includes("\0") || path.posix.isAbsolute(relative) || path.win32.isAbsolute(relative)) {
    throw new Error(`${label}: unsafe path ${relative}`);
  }
  const parts = relative.split("/");
  if (parts.some((part) => !part || part === "." || part === ".." || FORBIDDEN_KEYS.has(part))) throw new Error(`${label}: unsafe path ${relative}`);
  return parts;
}

function pathsIntersect(left, right) {
  return left === right || left.startsWith(`${right}/`) || right.startsWith(`${left}/`);
}

function domainsConflict(left, right) {
  return (left?.path_roots || []).some((a) => (right?.path_roots || []).some((b) => pathsIntersect(a, b)))
    || (left?.symbols || []).some((symbol) => (right?.symbols || []).includes(symbol));
}

function conflictReasons(leftIds, rightIds, domains) {
  const reasons = [];
  for (const left of leftIds) for (const right of rightIds) if (left === right || domainsConflict(domains.get(left), domains.get(right))) reasons.push(left === right ? left : `${left}<->${right}`);
  return [...new Set(reasons)].sort();
}

function charterMutationRoots(authority, node) {
  const text = fs.readFileSync(confinedFile(authority.packageRoot, node.charter, `${node.id} charter ownership`), "utf8");
  const line = /^- Production surfaces: (.+)$/m.exec(text)?.[1] || "";
  const structured = [...line.matchAll(/`([^`]+)`/g)].map((match) => match[1]);
  if (structured.length) return structured;
  // J1 is the one grandfathered IN_FLIGHT unit. Its exact ratified live
  // charter predates the unified structured-surface field and the wrapper is
  // deliberately forbidden from rewriting that mandate. Its immutable
  // `style_semantics` domain is therefore the machine-readable projection of
  // the live charter's enumerated CSS migration inventory. No other node gets
  // this fallback.
  if (node.id === "J1" && node.initial_state === "IN_FLIGHT" && node.review_profile === "history") {
    const domains = catalogMap(authority.packageRoot, "conflict-domains.toml", "domain");
    return node.conflict_domains.flatMap((id) => domains.get(id)?.path_roots || []);
  }
  return [];
}

function isInside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

/** Resolve an existing regular file without permitting traversal or symlink indirection. */
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
  for (let i = 0; i < line.length; i += 1) {
    const char = line[i];
    if (escaped) { escaped = false; continue; }
    if (char === "\\" && quoted) { escaped = true; continue; }
    if (char === '"') quoted = !quoted;
    if (char === "#" && !quoted) return line.slice(0, i);
  }
  return line;
}

function parseValue(raw, lineNumber) {
  const value = raw.trim();
  if (value.startsWith('"')) {
    try { return JSON.parse(value); }
    catch (error) { throw new Error(`TOML line ${lineNumber}: invalid string: ${error.message}`); }
  }
  if (value.startsWith("[")) {
    try {
      const parsed = JSON.parse(value);
      if (!Array.isArray(parsed)) throw new Error("not an array");
      return parsed;
    } catch (error) { throw new Error(`TOML line ${lineNumber}: invalid array: ${error.message}`); }
  }
  if (value === "true") return true;
  if (value === "false") return false;
  if (/^-?\d+$/.test(value)) {
    const parsed = Number(value);
    if (!Number.isSafeInteger(parsed) || String(parsed) !== value.replace(/^-0$/, "0")) throw new Error(`TOML line ${lineNumber}: integer is not a safe integer: ${value}`);
    return parsed;
  }
  throw new Error(`TOML line ${lineNumber}: unsupported value ${value}`);
}

export function parseToml(text) {
  if (typeof text !== "string") throw new Error("TOML input must be a string");
  if (text.includes("\0") || /\r(?!\n)/.test(text)) throw new Error("TOML input contains unsupported control characters");
  const root = {};
  let target = root;
  const declaredTables = new Set();
  for (const [index, original] of text.replaceAll("\r\n", "\n").split("\n").entries()) {
    const lineNumber = index + 1;
    const line = stripComment(original).trim();
    if (!line) continue;
    const arrayTable = line.match(/^\[\[([A-Za-z0-9_.-]+)\]\]$/);
    if (arrayTable) {
      const key = arrayTable[1];
      if (key.includes(".")) throw new Error(`TOML line ${lineNumber}: nested array tables are unsupported`);
      assertSafeKey(key, lineNumber);
      if (declaredTables.has(key) && !Array.isArray(root[key])) throw new Error(`TOML line ${lineNumber}: table type conflict ${key}`);
      if (root[key] !== undefined && !Array.isArray(root[key])) throw new Error(`TOML line ${lineNumber}: table type conflict ${key}`);
      root[key] ||= [];
      target = {};
      root[key].push(target);
      continue;
    }
    const table = line.match(/^\[([A-Za-z0-9_.-]+)\]$/);
    if (table) {
      const parts = table[1].split(".");
      for (const part of parts) assertSafeKey(part, lineNumber);
      const tableName = parts.join(".");
      if (declaredTables.has(tableName)) throw new Error(`TOML line ${lineNumber}: duplicate table ${tableName}`);
      declaredTables.add(tableName);
      target = root;
      for (const part of parts) {
        if (target[part] !== undefined && (typeof target[part] !== "object" || Array.isArray(target[part]))) throw new Error(`TOML line ${lineNumber}: table type conflict ${part}`);
        target[part] ||= {};
        target = target[part];
      }
      continue;
    }
    const assignment = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
    if (!assignment) throw new Error(`TOML line ${lineNumber}: malformed statement`);
    const [, key, raw] = assignment;
    assertSafeKey(key, lineNumber);
    if (Object.hasOwn(target, key)) throw new Error(`TOML line ${lineNumber}: duplicate key ${key}`);
    target[key] = parseValue(raw, lineNumber);
  }
  return root;
}

export function readToml(file) {
  try { return parseToml(fs.readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`${file}: ${error.message}`); }
}

function schemaTypeMatches(value, type) {
  if (type === "object") return value !== null && typeof value === "object" && !Array.isArray(value);
  if (type === "array") return Array.isArray(value);
  if (type === "integer") return Number.isSafeInteger(value);
  if (type === "number") return typeof value === "number" && Number.isFinite(value);
  if (type === "null") return value === null;
  return typeof value === type;
}

function schemaErrors(value, schema, location) {
  const errors = [];
  if (Array.isArray(schema.oneOf)) {
    const matches = schema.oneOf.filter((candidate) => schemaErrors(value, candidate, location).length === 0);
    if (matches.length !== 1) errors.push(`${location}: expected exactly one schema variant, matched ${matches.length}`);
    return errors;
  }
  if (Array.isArray(schema.anyOf)) {
    if (!schema.anyOf.some((candidate) => schemaErrors(value, candidate, location).length === 0)) errors.push(`${location}: no schema variant matched`);
    return errors;
  }
  if (schema.const !== undefined && value !== schema.const) errors.push(`${location}: expected constant ${JSON.stringify(schema.const)}`);
  if (Array.isArray(schema.enum) && !schema.enum.includes(value)) errors.push(`${location}: expected one of ${schema.enum.map((item) => JSON.stringify(item)).join(", ")}`);
  if (schema.type && !schemaTypeMatches(value, schema.type)) {
    errors.push(`${location}: expected ${schema.type}`);
    return errors;
  }
  if (schema.type === "object") {
    for (const key of schema.required || []) if (!Object.hasOwn(value, key)) errors.push(`${location}: missing required property ${key}`);
    const properties = schema.properties || {};
    if (schema.additionalProperties === false) for (const key of Object.keys(value)) if (!Object.hasOwn(properties, key)) errors.push(`${location}: additional property ${key}`);
    for (const [key, child] of Object.entries(properties)) if (Object.hasOwn(value, key)) errors.push(...schemaErrors(value[key], child, `${location}.${key}`));
  } else if (schema.type === "array") {
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${location}: requires at least ${schema.minItems} items`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems) errors.push(`${location}: permits at most ${schema.maxItems} items`);
    if (schema.uniqueItems && new Set(value.map((item) => JSON.stringify(item))).size !== value.length) errors.push(`${location}: array items must be unique`);
    if (schema.items) for (const [index, item] of value.entries()) errors.push(...schemaErrors(item, schema.items, `${location}[${index}]`));
  } else if (schema.type === "string") {
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${location}: string is shorter than ${schema.minLength}`);
    if (schema.maxLength !== undefined && value.length > schema.maxLength) errors.push(`${location}: string is longer than ${schema.maxLength}`);
    if (schema.pattern && !new RegExp(schema.pattern, "u").test(value)) errors.push(`${location}: string does not match ${schema.pattern}`);
    if (schema.format === "date-time" && !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/.test(value)) errors.push(`${location}: expected UTC RFC3339 date-time`);
  } else if (schema.type === "integer" || schema.type === "number") {
    if (schema.minimum !== undefined && value < schema.minimum) errors.push(`${location}: value is below ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) errors.push(`${location}: value is above ${schema.maximum}`);
  }
  return errors;
}

export function validateSchemaObject(value, schema, location = "object") {
  return schemaErrors(value, schema, location);
}

function loadSchema(packageRoot, name) {
  const file = confinedFile(path.join(packageRoot, "schemas"), name, `schema ${name}`);
  try { return JSON.parse(fs.readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`schema parse failure ${name}: ${error.message}`); }
}

function validateTomlSchema(file, packageRoot, schemaName, location) {
  let model;
  try { model = readToml(file); }
  catch (error) { return { model: null, errors: [error.message] }; }
  let schema;
  try { schema = loadSchema(packageRoot, schemaName); }
  catch (error) { return { model, errors: [error.message] }; }
  return { model, errors: validateSchemaObject(model, schema, location) };
}

function tomlValue(value) {
  if (Array.isArray(value)) return renderArray(value);
  if (typeof value === "boolean" || typeof value === "number") return String(value);
  return quote(value);
}

export function renderNode(node) {
  return ["[[node]]", ...NODE_FIELDS.filter((key) => node[key] !== undefined).map((key) => `${key} = ${tomlValue(node[key])}`), ""].join("\n");
}

export function loadAuthority(packageRoot = PACKAGE_ROOT) {
  const rootFile = confinedFile(packageRoot, "authority/root.toml", "authority root");
  const metadata = readToml(rootFile);
  if (!Array.isArray(metadata.modules)) throw new Error(`${rootFile}: modules must be an array`);
  const nodes = [];
  const moduleModels = [];
  for (const relative of metadata.modules) {
    if (typeof relative === "string" && (relative.includes("generated") || path.basename(relative) === "program-dag.toml")) throw new Error(`${rootFile}: generated projection cannot be authority input: ${relative}`);
    if (typeof relative !== "string" || !relative.startsWith("dag/") || !relative.endsWith(".toml")) throw new Error(`${rootFile}: invalid module path ${relative}`);
    const file = confinedFile(path.join(packageRoot, "authority"), relative, "authority DAG module");
    if (/^# GENERATED\b/m.test(fs.readFileSync(file, "utf8"))) throw new Error(`${file}: generated projection cannot be authority input`);
    const model = readToml(file);
    if (!Array.isArray(model.node)) throw new Error(`${file}: missing [[node]] rows`);
    moduleModels.push({ relative, file, model });
    nodes.push(...model.node.map((node) => ({ ...node, _module: relative })));
  }
  return { packageRoot, rootFile, metadata, nodes, moduleModels };
}

function graphMaps(nodes) {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const children = new Map(nodes.map((node) => [node.id, []]));
  for (const node of nodes) for (const pred of node.predecessors || []) if (children.has(pred)) children.get(pred).push(node.id);
  return { byId, children };
}

export function topological(nodes) {
  const { byId, children } = graphMaps(nodes);
  const indegree = new Map(nodes.map((node) => [node.id, 0]));
  for (const node of nodes) for (const pred of node.predecessors || []) if (byId.has(pred)) indegree.set(node.id, indegree.get(node.id) + 1);
  const ready = [...indegree].filter(([, degree]) => degree === 0).map(([id]) => id).sort();
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
  return { order, cyclic: order.length !== nodes.length, unresolved: [...indegree].filter(([, degree]) => degree > 0).map(([id]) => id).sort() };
}

export function renderProjection(authority) {
  const { metadata, nodes } = authority;
  const { order } = topological(nodes);
  return [
    "# GENERATED by tools/build-program-dag.mjs. DO NOT EDIT; generated files are never authority inputs.",
    `schema = ${metadata.schema}`,
    `revision = ${metadata.revision}`,
    `package = ${quote(metadata.package)}`,
    `state = ${quote(metadata.state)}`,
    `pinned_commit = ${quote(metadata.pinned_commit)}`,
    `pinned_tree = ${quote(metadata.pinned_tree)}`,
    `entry_gate = ${quote(metadata.entry_gate)}`,
    `final_rev11_gate = ${quote(metadata.final_rev11_gate)}`,
    `successor_promotion_gate = ${quote(metadata.successor_promotion_gate)}`,
    "",
    ...order.map((node) => renderNode(Object.fromEntries(Object.entries(node).filter(([key]) => !key.startsWith("_"))))),
  ].join("\n");
}

function criticalPath(nodes) {
  const { order } = topological(nodes);
  const distance = new Map();
  const prior = new Map();
  for (const node of order) {
    let best = 0;
    let bestId = null;
    for (const pred of node.predecessors || []) {
      const candidate = distance.get(pred) || 0;
      if (candidate > best) { best = candidate; bestId = pred; }
    }
    distance.set(node.id, best + 1);
    prior.set(node.id, bestId);
  }
  const end = [...distance].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))[0] || [null, 0];
  const ids = [];
  for (let id = end[0]; id; id = prior.get(id)) ids.push(id);
  return { length: end[1], nodes: ids.reverse() };
}

function topologyWidths(nodes) {
  const { order } = topological(nodes);
  const level = new Map();
  for (const node of order) level.set(node.id, 1 + Math.max(0, ...(node.predecessors || []).map((id) => level.get(id) || 0)));
  const widths = {};
  for (const value of level.values()) widths[value] = (widths[value] || 0) + 1;
  const max = Math.max(0, ...Object.values(widths));
  return { max, levels: widths };
}

function admissibleWidths(authority, resourceLimited) {
  const { order } = topological(authority.nodes); const level = new Map(); const layers = new Map();
  for (const node of order) {
    const value = 1 + Math.max(0, ...node.predecessors.map((id) => level.get(id) || 0)); level.set(node.id, value);
    layers.set(value, [...(layers.get(value) || []), node]);
  }
  const domains = catalogMap(authority.packageRoot, "conflict-domains.toml", "domain");
  const capacities = catalogMap(authority.packageRoot, "resource-profiles.toml", "profile");
  const widths = {};
  for (const [layer, candidates] of layers) {
    let best = 0;
    const search = (index, chosen, usage) => {
      if (chosen.length + candidates.length - index <= best) return;
      if (index === candidates.length) { best = Math.max(best, chosen.length); return; }
      const node = candidates[index];
      const compatible = chosen.every((other) => conflictReasons(node.conflict_domains, other.conflict_domains, domains).length === 0);
      const withinCapacity = !resourceLimited || (usage.get(node.resource_class) || 0) < (capacities.get(node.resource_class)?.capacity_hint || 0);
      if (compatible && withinCapacity) {
        usage.set(node.resource_class, (usage.get(node.resource_class) || 0) + 1); chosen.push(node); search(index + 1, chosen, usage); chosen.pop();
        usage.set(node.resource_class, usage.get(node.resource_class) - 1);
      }
      search(index + 1, chosen, usage);
    };
    search(0, [], new Map()); widths[layer] = best;
  }
  return { max: Math.max(0, ...Object.values(widths)), levels: widths };
}

export function generatedFiles(authority) {
  const { packageRoot, nodes, metadata } = authority;
  const { byId, children } = graphMaps(nodes);
  const projection = renderProjection(authority);
  const edges = nodes.reduce((count, node) => count + node.predecessors.length + node.conditional_predecessors.length, 0);
  const roots = nodes.filter((node) => node.predecessors.length === 0).map((node) => node.id).sort();
  const sinks = nodes.filter((node) => children.get(node.id).length === 0).map((node) => node.id).sort();
  const phases = Object.fromEntries([...new Set(nodes.map((node) => node.phase))].sort().map((phase) => [phase, nodes.filter((node) => node.phase === phase).length]));
  const metrics = { schema: metadata.schema, state: metadata.state, nodes: nodes.length, edges, modules: metadata.modules.length, charters: nodes.length, roots, sinks, phases, critical_path: criticalPath(nodes), topological_width: topologyWidths(nodes), lease_compatible_width: admissibleWidths(authority, false), capacity_admissible_width: admissibleWidths(authority, true) };
  const index = ["# Node index", "", "Generated view; not authority.", "", "| ID | Train | Kind | Preds | Dispatch | Charter |", "|---|---|---|---:|---|---|", ...[...nodes].sort((a, b) => a.id.localeCompare(b.id)).map((node) => `| ${node.id} | ${node.train} | ${node.kind} | ${node.predecessors.length} | ${node.dispatchable} | \`${node.charter}\` |`), ""].join("\n");
  const coverage = readToml(path.join(packageRoot, "provenance/source-coverage.toml")).requirement || [];
  const coverageView = ["# Normative source coverage", "", "Generated view; every row is an exact digest-bound source requirement atom.", "", "| ID | Kind | Source lines | Disposition | Target |", "|---|---|---|---|---|", ...coverage.map((row) => `| ${row.id} | ${row.kind} | ${row.source}:${row.from_line}-${row.to_line} | ${row.disposition} | \`${row.target}\` |`), ""].join("\n");
  return new Map([
    ["program-dag.toml", projection],
    ["generated/NODE-INDEX.md", index],
    ["generated/SOURCE-COVERAGE.md", coverageView],
    ["generated/METRICS.json", `${JSON.stringify(metrics, null, 2)}\n`],
  ]);
}

export function writeGenerated(authority, outputRoot = authority.packageRoot) {
  const files = generatedFiles(authority);
  for (const [relative, content] of files) {
    const file = path.join(outputRoot, relative);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content.endsWith("\n") ? content : `${content}\n`);
  }
  return files;
}

function parseCharterHeader(text) {
  const match = text.match(/^<!-- unified-charter-v2\n([\s\S]*?)\n-->/);
  if (!match) return null;
  const result = {};
  for (const line of match[1].split("\n")) {
    const [key, ...rest] = line.split("=");
    if (!key || !rest.length || Object.hasOwn(result, key)) return null;
    result[key] = rest.join("=");
  }
  return result;
}

function readCatalogIds(file, tableName) {
  const model = readToml(file);
  return new Set((model[tableName] || []).map((row) => row.id));
}

export function validateGraphModel(nodes, options = {}) {
  const errors = [];
  const seen = new Set();
  const byId = new Map();
  for (const node of nodes) {
    for (const key of Object.keys(node)) if (!NODE_FIELDS.includes(key) && key !== "_module") errors.push(`${node.id}: unknown node field ${key}`);
    if (typeof node.id !== "string" || !/^[A-Z][A-Z0-9-]*$/.test(node.id)) errors.push(`invalid node id ${node.id}`);
    if (seen.has(node.id)) errors.push(`duplicate node id ${node.id}`);
    seen.add(node.id);
    byId.set(node.id, node);
    for (const field of REQUIRED_NODE_FIELDS) {
      if (!Object.hasOwn(node, field)) errors.push(`${node.id}: missing field ${field}`);
      else if (ARRAY_FIELDS.has(field) && !Array.isArray(node[field])) errors.push(`${node.id}: ${field} must be array`);
      else if (BOOL_FIELDS.has(field) && typeof node[field] !== "boolean") errors.push(`${node.id}: ${field} must be boolean`);
      else if (INT_FIELDS.has(field) && !Number.isInteger(node[field])) errors.push(`${node.id}: ${field} must be integer`);
      else if (!ARRAY_FIELDS.has(field) && !BOOL_FIELDS.has(field) && !INT_FIELDS.has(field) && typeof node[field] !== "string") errors.push(`${node.id}: ${field} must be string`);
    }
    if (new Set(node.predecessors || []).size !== (node.predecessors || []).length) errors.push(`${node.id}: duplicate predecessor`);
    if (new Set(node.conflict_domains || []).size !== (node.conflict_domains || []).length) errors.push(`${node.id}: duplicate conflict domain`);
    if ((node.conflict_domains || []).length === 0) errors.push(`${node.id}: missing conflict domain`);
    if ((node.conflict_domains || []).some((domain) => domain === node.train.replaceAll(/[.-]/g, "_") || domain === node.product)) errors.push(`${node.id}: train-wide blanket conflict domain ${node.train}`);
    if (node.predecessors?.includes(node.id)) errors.push(`${node.id}: self predecessor cycle`);
    if (!['delivery', 'convergence', 'history'].includes(node.semantic_role)) errors.push(`${node.id}: invalid semantic_role ${node.semantic_role}`);
    if (node.semantic_role === "convergence") {
      if (node.size !== "S" || node.max_production_loc > 300 || node.max_production_files > 3 || node.max_related_packages > 1) errors.push(`${node.id}: broad convergence exceeds 300 LOC/3 files/1 package`);
    }
    if ((node.semantic_role === "history") !== (node.review_profile === "history")) errors.push(`${node.id}: semantic_role history must exactly match review_profile history`);
    if (["proposal-subblock", "split"].includes(node.class)) errors.push(`${node.id}: mechanically generated split class is forbidden; dispatch the source-owned atomic node`);
    if (node.review_profile === "mechanical-final") errors.push(`${node.id}: mechanical-final is forbidden without an independently acceptable source-owned leaf`);
    if (node.dispatchable && node.max_production_loc > 1500) errors.push(`${node.id}: dispatchable leaf exceeds mandatory LOC rescope`);
    if (INT_FIELDS.has("max_production_loc") && INT_FIELDS.size && [...INT_FIELDS].some((field) => !Number.isInteger(node[field]) || node[field] < 0)) errors.push(`${node.id}: sizing values must be non-negative integers`);
    if (node.id !== "ORC0" && node.activation_gate !== "none" && node.activation_gate !== "ORC0") errors.push(`${node.id}: invalid activation gate`);
  }
  for (const node of nodes) for (const pred of node.predecessors || []) if (!byId.has(pred)) errors.push(`${node.id}: missing predecessor ${pred}`);
  const topo = topological(nodes);
  if (topo.cyclic) errors.push(`cycle detected: ${topo.unresolved.join(",")}`);
  const reaches = (start, target, seenReach = new Set()) => {
    if (start === target) return true;
    if (seenReach.has(start)) return false;
    seenReach.add(start);
    return (byId.get(start)?.predecessors || []).some((predecessor) => reaches(predecessor, target, seenReach));
  };
  for (const node of nodes) if (node.release_gating === "product" && !reaches(node.id, "BR0")) errors.push(`${node.id}: product release gate is not downstream of BR0`);
  if (!options.skipCharters && options.packageRoot) errors.push(...validateCharters(nodes, options.packageRoot));
  return errors;
}

function validateSourceRefs(authority) {
  const errors = [];
  const repoRoot = gitRoot(authority.packageRoot);
  if (!repoRoot) return ["unable to resolve repository root for source references"];
  const liveLock = readToml(confinedFile(path.join(authority.packageRoot, "provenance"), "live-source-lock.toml", "live source lock"));
  const liveRows = new Map((liveLock.source || []).map((row) => [row.ref, row]));
  for (const node of authority.nodes) for (const ref of node.source_refs) {
    if (ref.startsWith("source:")) {
      const match = ref.match(/^source:([A-Za-z0-9._-]+):L(\d+)$/);
      if (!match) { errors.push(`${node.id}: malformed source reference ${ref}`); continue; }
      try {
        const file = confinedFile(path.join(authority.packageRoot, "sources"), match[1], `${node.id} source reference`);
        if (Number(match[2]) < 1 || Number(match[2]) > fs.readFileSync(file, "utf8").split("\n").length) errors.push(`${node.id}: source line out of range ${ref}`);
      } catch (error) { errors.push(`${node.id}: missing or unsafe source reference ${ref}: ${error.message}`); }
    } else if (ref.startsWith("live:")) {
      const relative = ref.slice("live:".length).split("#")[0];
      try {
        safeRelative(relative, `${node.id} live reference`);
        const locked = liveRows.get(ref);
        if (locked?.commit) {
          if (!gitPathAt(locked.commit, relative, repoRoot)) throw new Error(`missing Git object ${locked.commit}:${relative}`);
        } else confinedFile(repoRoot, relative, `${node.id} live reference`);
      }
      catch (error) { errors.push(`${node.id}: missing or unsafe live reference ${ref}: ${error.message}`); }
    } else if (ref.startsWith("provenance:")) {
      const relative = ref.slice("provenance:".length).split(":")[0];
      try { confinedFile(path.join(authority.packageRoot, "provenance"), relative, `${node.id} provenance reference`); }
      catch (error) { errors.push(`${node.id}: missing or unsafe provenance reference ${ref}: ${error.message}`); }
    } else errors.push(`${node.id}: unsupported source reference ${ref}`);
  }
  return errors;
}

function validateSchemasAndTemplates(authority) {
  const errors = [];
  const { packageRoot } = authority;
  const schemaDir = path.join(packageRoot, "schemas");
  for (const entry of fs.readdirSync(schemaDir, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".schema.json")) { errors.push(`unsupported schema entry ${entry.name}`); continue; }
    const file = confinedFile(schemaDir, entry.name, `schema ${entry.name}`);
    try {
      const value = JSON.parse(fs.readFileSync(file, "utf8"));
      if (value.$schema !== "https://json-schema.org/draft/2020-12/schema" || value.additionalProperties !== false || value.type !== "object" || !Array.isArray(value.required) || !value.properties) errors.push(`schema contract incomplete ${entry.name}`);
    } catch (error) { errors.push(`schema parse failure ${entry.name}: ${error.message}`); }
  }
  const applications = [
    [authority.metadata, "root.schema.json", "authority.root"],
    [readToml(confinedFile(packageRoot, "authority/state/activation.toml", "activation state")), "activation.schema.json", "authority.activation"],
    [readToml(confinedFile(packageRoot, "authority/state/legacy-receipts.toml", "legacy manifest")), "legacy-manifest.schema.json", "authority.legacy_manifest"],
    [readToml(confinedFile(packageRoot, "authority/state/external-authorizations.toml", "external authorization manifest")), "external-authorization-manifest.schema.json", "authority.external_authorization_manifest"],
    [readToml(confinedFile(packageRoot, "authority/state/trusted-ratifications.toml", "trusted ratification ledger")), "trusted-ratifications.schema.json", "authority.trusted_ratifications"],
    [readToml(confinedFile(packageRoot, "authority/state/external-custody-boundary.toml", "external custody boundary")), "external-custody-boundary.schema.json", "authority.external_custody_boundary"],
    [readToml(confinedFile(packageRoot, "catalogs/conflict-domains.toml", "conflict domain catalog")), "conflict-domain-catalog.schema.json", "catalog.conflict_domains"],
    [readToml(confinedFile(packageRoot, "catalogs/gate-profiles.toml", "gate profile catalog")), "gate-profile-catalog.schema.json", "catalog.gate_profiles"],
    [readToml(confinedFile(packageRoot, "catalogs/review-profiles.toml", "review profile catalog")), "review-profile-catalog.schema.json", "catalog.review_profiles"],
    [readToml(confinedFile(packageRoot, "catalogs/resource-profiles.toml", "resource profile catalog")), "resource-profile-catalog.schema.json", "catalog.resource_profiles"],
    [JSON.parse(fs.readFileSync(confinedFile(packageRoot, "authority/state/preactivation-orc0-history.json", "preactivation ORC0 history"), "utf8")), "trusted-local-preactivation-history.schema.json", "authority.preactivation_orc0_history"],
    [JSON.parse(fs.readFileSync(confinedFile(packageRoot, "authority/state/historical-review-audit.json", "historical review audit"), "utf8")), "historical-review-audit.schema.json", "authority.historical_review_audit"],
  ];
  for (const { model, relative } of authority.moduleModels) applications.push([model, "dag-module.schema.json", `authority.${relative}`]);
  for (const node of authority.nodes) applications.push([Object.fromEntries(Object.entries(node).filter(([key]) => key !== "_module")), "node.schema.json", `node.${node.id}`]);
  for (const [value, schemaName, location] of applications) {
    try { errors.push(...validateSchemaObject(value, loadSchema(packageRoot, schemaName), location)); }
    catch (error) { errors.push(error.message); }
  }
  const templateSchemas = new Map([
    ["acceptance-receipt.template.toml", "receipt.schema.json"],
    ["candidate-finalization.template.toml", "candidate-finalization.schema.json"],
    ["dispatch.template.toml", "dispatch.schema.json"],
    ["landed-receipt.template.toml", "landed-receipt.schema.json"],
    ["lease.template.toml", "lease.schema.json"],
    ["external-authorization.template.toml", "external-authorization.schema.json"],
    ["amendment.template.toml", "amendment.schema.json"],
    ["gate-evidence.template.toml", "gate-evidence.schema.json"],
    ["review-evidence.template.toml", "review-evidence.schema.json"],
    ["trusted-local-harness-report.template.json", "trusted-local-harness-report.schema.json"],
    ["trusted-local-architect-decision.template.json", "trusted-local-architect-decision.schema.json"],
    ["trusted-local-architect-prompt.template.json", "trusted-local-architect-prompt.schema.json"],
    ["trusted-local-review-target.template.json", "trusted-local-review-target.schema.json"],
  ]);
  const templateDir = path.join(packageRoot, "templates");
  const actualTemplates = fs.readdirSync(templateDir).sort();
  if (JSON.stringify(actualTemplates) !== JSON.stringify([...templateSchemas.keys()].sort())) errors.push("template inventory/schema mapping mismatch");
  for (const [name, schemaName] of templateSchemas) {
    try {
      const file = confinedFile(templateDir, name, `template ${name}`);
      const model = name.endsWith(".json") ? JSON.parse(fs.readFileSync(file, "utf8")) : readToml(file);
      errors.push(...validateSchemaObject(model, loadSchema(packageRoot, schemaName), `template.${name}`));
    } catch (error) { errors.push(`template validation failure ${name}: ${error.message}`); }
  }
  return errors;
}

export function validateCharters(nodes, packageRoot = PACKAGE_ROOT) {
  const errors = [];
  const profiles = new Map((readToml(path.join(packageRoot, "catalogs/review-profiles.toml")).profile || []).map((profile) => [profile.id, profile]));
  const expected = new Set(nodes.map((node) => path.normalize(node.charter)));
  const charterRoot = path.join(packageRoot, "charters");
  const actual = [];
  if (fs.existsSync(charterRoot)) {
    const walk = (dir) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const entryPath = path.join(dir, entry.name);
        if (entry.isSymbolicLink()) errors.push(`charter tree contains forbidden symlink ${path.relative(packageRoot, entryPath)}`);
        else if (entry.isDirectory()) walk(entryPath);
        else if (entry.isFile() && entry.name.endsWith(".md")) actual.push(path.relative(packageRoot, entryPath));
        else errors.push(`charter tree contains unsupported entry ${path.relative(packageRoot, entryPath)}`);
      }
    };
    walk(charterRoot);
  }
  for (const relative of actual) if (!expected.has(path.normalize(relative))) errors.push(`orphan charter ${relative}`);
  for (const node of nodes) {
    let file;
    try {
      if (typeof node.charter !== "string" || !node.charter.startsWith("charters/") || !node.charter.endsWith(".md")) throw new Error(`unsafe charter path ${node.charter}`);
      file = confinedFile(packageRoot, node.charter, `${node.id} charter`);
    } catch (error) { errors.push(`${node.id}: unsafe charter path ${node.charter}: ${error.message}`); continue; }
    const text = fs.readFileSync(file, "utf8");
    const operativeText = text.split(/^## Transferred source requirement atoms$/m)[0];
    const header = parseCharterHeader(text);
    if (!header) { errors.push(`${node.id}: missing charter metadata header`); continue; }
    const parity = Object.fromEntries(NODE_FIELDS.map((key) => {
      const value = node[key];
      return [key, Array.isArray(value) ? value.join(",") : value === undefined ? "" : String(value)];
    }));
    for (const [key, expectedValue] of Object.entries(parity)) if (header[key] !== expectedValue) errors.push(`${node.id}: charter metadata mismatch ${key}: ${header[key]} != ${expectedValue}`);
    for (const ref of node.source_refs) if (!text.includes(`\`${ref}\``)) errors.push(`${node.id}: charter/source_refs mismatch ${ref}`);
    const historical = node.review_profile === "history";
    if (!historical) {
      const profile = profiles.get(node.review_profile);
      if (!profile || !operativeText.includes(`Final acceptance requires the complete ${profile.reviewers}/${profile.reviewers} current-round profile`) || !profile.lenses.every((lens) => operativeText.includes(`\`${lens}\``))) errors.push(`${node.id}: charter review contract does not match its risk-scaled profile`);
      for (const section of ["Independently acceptable outcome", "Concrete surfaces and APIs", "Exact predecessor contracts", "Source-specific scope", "Acceptance IDs and discriminating proof", "Deletions and forbidden designs", "Budgets and mandatory rescope", "Abort conditions", "Targeted verification", "Citations"]) if (!text.includes(`## ${section}`)) errors.push(`${node.id}: charter missing section ${section}`);
      if (/Suggested subblocks|Recommended delivery slices|reconcile owners\s*\/\s*implement one path|add proof\s*\/\s*measure/i.test(operativeText)) errors.push(`${node.id}: hidden-train or generic charter language`);
      if (/[A-Z][A-Z0-9-]*::[a-z][a-z0-9_]+/.test(operativeText)) errors.push(`${node.id}: synthetic ID::slug ownership token is forbidden`);
      const predecessorSection = /^## Exact predecessor contracts\n([\s\S]*?)(?=^## )/m.exec(operativeText)?.[1] || "";
      const namedPredecessors = [...predecessorSection.matchAll(/^- \*\*([A-Z][A-Z0-9-]*):\*\*/gm)].map((match) => match[1]);
      if (JSON.stringify(namedPredecessors) !== JSON.stringify(node.predecessors)) errors.push(`${node.id}: charter predecessor section is not exact DAG order/content`);
      for (const requirement of node.external_requirements) if (!predecessorSection.includes(`External custody ${requirement}:`) && !predecessorSection.includes(`Trusted-local activation ${requirement}:`)) errors.push(`${node.id}: charter omits exact external/trusted-local requirement ${requirement}`);
      for (const acceptance of ["AC1", "AC2", "AC3", "AC4"]) if (!text.includes(`${node.id}-${acceptance}`)) errors.push(`${node.id}: missing source-specific acceptance ID ${acceptance}`);
      if (!operativeText.includes("Preflight evidence selection:") || !operativeText.includes("Every proposed new test must name a plausible regression or contract boundary not already discriminated")) errors.push(`${node.id}: charter lacks proportionate preflight evidence selection`);
      if (!operativeText.includes("when the changed scope owns or affects incremental") || !operativeText.includes("when the changed scope owns or affects a hot path") || !operativeText.includes("Performance budget: when preflight identifies touched authority or a hot path") || !operativeText.includes("Bind the preflight evidence selection and terse rationale")) errors.push(`${node.id}: charter lacks proportionate applicability or evidence binding`);
      if (/sole-owner proof:\*\* add `|positive contract:\*\* add `|incremental equivalence:\*\* add `|bounded work:\*\* capture equivalent-work counters|Re-run the planted RED mutation, restore, then GREEN|after warmup, retained bytes may not increase across 100 identical requests/.test(operativeText)) errors.push(`${node.id}: charter retains a universal test quota`);
      if ((text.match(/`(?:crates|packages|scripts|docs)\//g) || []).length < 2) errors.push(`${node.id}: charter lacks concrete module/test/command surfaces`);
      if (!text.includes("Delete or structurally reject")) errors.push(`${node.id}: charter lacks exact legacy deletion target`);
      if (!text.includes("cargo nextest") && !text.includes("pnpm --filter") && !text.includes("validate-program-dag") && !text.includes("validate-negative-controls")) errors.push(`${node.id}: charter lacks exact targeted verification command`);
      if (!text.includes("source:") && !text.includes("live:") && !text.includes("provenance:")) errors.push(`${node.id}: charter lacks source citation`);
    } else {
      if (!text.includes("Exact live-charter SHA-256")) errors.push(`${node.id}: historical charter lacks digest binding`);
    }
  }
  return errors;
}

function validateSourceCoverage(packageRoot, nodes) {
  const errors = [];
  const lock = readToml(path.join(packageRoot, "provenance/source-lock.toml"));
  const coverageModel = readToml(path.join(packageRoot, "provenance/source-coverage.toml"));
  const coverage = coverageModel.requirement || [];
  if (coverageModel.schema !== 3 || !Array.isArray(coverageModel.requirement)) errors.push("source coverage schema must be exact v3 requirement atoms");
  const sourceByName = new Map();
  for (const source of lock.source || []) {
    let file;
    try { file = confinedFile(packageRoot, source.path, "normative source"); }
    catch (error) { errors.push(`missing or unsafe normative source ${source.path}: ${error.message}`); continue; }
    const bytes = fs.readFileSync(file);
    if (bytes.length !== source.bytes || sha256(bytes) !== source.sha256) errors.push(`normative source digest mismatch ${source.path}`);
    const name = path.basename(source.path);
    if (sourceByName.has(name)) errors.push(`duplicate normative source basename ${name}`);
    sourceByName.set(name, { ...source, lines: bytes.toString("utf8").split("\n") });
  }
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const seenIds = new Set();
  const expectedByTarget = new Map();
  for (const row of coverage) {
    if (seenIds.has(row.id)) errors.push(`duplicate source requirement atom ${row.id}`);
    seenIds.add(row.id);
    if (!/^(?:context|requirement|acceptance|deletion|forbidden)$/.test(row.kind || "") || row.disposition !== "transferred") errors.push(`invalid source requirement disposition ${row.id}`);
    const source = sourceByName.get(row.source);
    if (!source || row.source_sha256 !== source.sha256 || !Number.isSafeInteger(row.from_line) || !Number.isSafeInteger(row.to_line) || row.from_line < 1 || row.to_line < row.from_line || row.to_line > (source?.lines.length || 0)) {
      errors.push(`source requirement range/source mismatch ${row.id}`);
      continue;
    }
    // The atom generator removes Markdown hard-break padding on every selected
    // line and then trims the selected block boundary. Interior indentation is
    // preserved. This is deliberately not general whitespace normalization.
    const text = source.lines.slice(row.from_line - 1, row.to_line).map((line) => line.trimEnd()).join("\n").trim();
    if (row.text !== text || row.text_sha256 !== sha256(`${text}\n`)) errors.push(`source requirement text digest mismatch ${row.id}`);
    if (!Array.isArray(row.applicable_nodes) || row.applicable_nodes.length < 1 || new Set(row.applicable_nodes).size !== row.applicable_nodes.length) errors.push(`source requirement applicability invalid ${row.id}`);
    for (const id of row.applicable_nodes || []) if (!byId.has(id)) errors.push(`source requirement applicability names missing node ${row.id}:${id}`);
    let targetFile;
    if (typeof row.target === "string" && row.target.startsWith("node:")) {
      const node = byId.get(row.target.slice("node:".length));
      if (!node) errors.push(`source requirement target missing ${row.id}: ${row.target}`);
      else try { targetFile = confinedFile(packageRoot, node.charter, `source requirement target ${row.id}`); }
      catch (error) { errors.push(error.message); }
    } else if (typeof row.target === "string" && row.target.startsWith("contract:")) {
      const relative = row.target.slice("contract:".length);
      try {
        if (!relative.startsWith("contracts/") || !relative.endsWith(".md")) throw new Error("contract target must stay under contracts/");
        targetFile = confinedFile(packageRoot, relative, `source requirement target ${row.id}`);
      } catch (error) { errors.push(`${row.id}: unsafe contract target ${row.target}: ${error.message}`); }
    } else errors.push(`source requirement target invalid ${row.id}: ${row.target}`);
    if (targetFile) {
      const relative = path.relative(fs.realpathSync(packageRoot), targetFile);
      expectedByTarget.set(relative, [...(expectedByTarget.get(relative) || []), row]);
    }
  }
  for (const [relative, expected] of expectedByTarget) {
    const text = fs.readFileSync(confinedFile(packageRoot, relative, "source requirement target"), "utf8");
    const expectedClauses = expected.map((row) => `### ${row.id}\n\n- Kind: \`${row.kind}\`\n- Source: \`${row.source}:${row.from_line}-${row.to_line}\`\n- Applicability: ${row.applicable_nodes.map((id) => `\`${id}\``).join(", ")}\n- Exact text SHA-256: \`${row.text_sha256}\`\n\n~~~~markdown\n${row.text}\n~~~~`).join("\n\n");
    const actualClauses = text.split(/^## Transferred source requirement atoms$/m)[1]?.replace(/^\n+These clauses are operative only for the exact applicability set shown\. Cold packets include the exact applicable subset and its source digest\.\n+/m, "").split(/^## Live authority inputs$/m)[0].trim() || "";
    if (actualClauses !== expectedClauses) errors.push(`source requirement operative clause mismatch ${relative}`);
  }
  for (const node of nodes) {
    const charter = fs.readFileSync(confinedFile(packageRoot, node.charter, `${node.id} source clauses`), "utf8");
    if (/^- `SRC-[^`]+` `[0-9a-f]{64}`$/m.test(charter)) errors.push(`${node.id}: obsolete hash-list-only source atoms remain`);
  }
  const attachmentRoot = path.join(packageRoot, "provenance/packet-source-clauses");
  const expectedAttachments = new Set(nodes.map((node) => `${node.id}.md`));
  const actualAttachments = new Set(fs.readdirSync(attachmentRoot, { withFileTypes: true }).filter((entry) => entry.isFile() && entry.name.endsWith(".md")).map((entry) => entry.name));
  if (JSON.stringify([...actualAttachments].sort()) !== JSON.stringify([...expectedAttachments].sort())) errors.push("packet source-clause attachment inventory differs from exact node set");
  for (const node of nodes) {
    const rows = coverage.filter((row) => row.applicable_nodes?.includes(node.id)).sort((left, right) => left.id.localeCompare(right.id));
    const clauses = rows.length ? rows.map((row) => `### ${row.id}\n\n- Kind: \`${row.kind}\`; source: \`${row.source}:${row.from_line}-${row.to_line}\`; target: \`${row.target}\`; text SHA-256: \`${row.text_sha256}\`.\n\n~~~~markdown\n${row.text}\n~~~~`).join("\n\n") : "- none";
    const expected = `# Exact operative source-clause attachment — ${node.id}\n\nSchema: 1. Node: \`${node.id}\`. Clause count: ${rows.length}. Generated from \`provenance/source-coverage.toml\`; every clause below is exact, operative, and applicable to this node.\n\n${clauses}\n`;
    try {
      const actual = fs.readFileSync(confinedFile(attachmentRoot, `${node.id}.md`, `${node.id} packet source-clause attachment`), "utf8");
      if (actual !== expected) errors.push(`${node.id}: packet source-clause attachment is stale or incomplete`);
    } catch (error) { errors.push(`${node.id}: packet source-clause attachment missing/unsafe: ${error.message}`); }
  }
  return errors;
}

function validateCanonicalSuccessorLedger(authority) {
  const errors = [];
  const file = confinedFile(authority.packageRoot, "sources/successor-expansion.md", "canonical successor ledger");
  const source = fs.readFileSync(file, "utf8");
  const marker = "The TOML below is the sole canonical graph and node-classification ledger.";
  const start = source.indexOf("```toml", source.indexOf(marker));
  const end = source.indexOf("\n```", start);
  if (start < 0 || end < 0) return ["canonical successor ledger block is missing"];
  const block = source.slice(start + "```toml\n".length, end);
  const predecessors = new Map(); const metadata = new Map(); let section = "";
  for (const line of block.split("\n")) {
    const header = line.match(/^\[([^\]]+)\]$/); if (header) { section = header[1]; continue; }
    if (section === "predecessors") {
      const match = line.match(/^([A-Z0-9]+) = (\[.*\])$/); if (match) predecessors.set(match[1], JSON.parse(match[2]));
    } else if (section === "node") {
      const match = line.match(/^([A-Z0-9]+) = \{ kind = "([^"]+)", product = "([^"]+)", release_gating = "([^"]+)" \}$/);
      if (match) metadata.set(match[1], { kind: match[2], product: match[3], release_gating: match[4] });
    }
  }
  const amendmentMap = readToml(confinedFile(authority.packageRoot, "provenance/ratified-amendment-map.toml", "ratified amendment map"));
  if (amendmentMap.schema !== 1 || amendmentMap.source !== "sources/successor-expansion.md" || amendmentMap.source_sha256 !== sha256(fs.readFileSync(file)) || !Array.isArray(amendmentMap.amendment) || amendmentMap.amendment.length) errors.push("ratified successor amendment map must be exact and empty without external ratification");
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  for (const [id, expectedPredecessors] of predecessors) {
    const node = byId.get(id); const expected = metadata.get(id);
    if (!node) { errors.push(`canonical successor node missing ${id}`); continue; }
    if (JSON.stringify(node.predecessors) !== JSON.stringify(expectedPredecessors)) errors.push(`canonical successor predecessors differ ${id}`);
    for (const field of ["kind", "product", "release_gating"]) if (node[field] !== expected?.[field]) errors.push(`canonical successor classification differs ${id}.${field}`);
  }
  for (const node of authority.nodes) if ((node.phase === "expansion" || node.id === "BR0") && !predecessors.has(node.id)) errors.push(`invented successor node outside canonical ledger ${node.id}`);
  return errors;
}

function validateLiveSourceLock(authority) {
  const errors = [];
  const repoRoot = gitRoot(authority.packageRoot);
  if (!repoRoot) return ["unable to resolve repository root for live source lock"];
  const file = confinedFile(path.join(authority.packageRoot, "provenance"), "live-source-lock.toml", "live source lock");
  const model = readToml(file);
  if (model.schema !== 2 || !Array.isArray(model.source)) errors.push("live source lock schema mismatch");
  const rows = model.source || [];
  const byRef = new Map();
  for (const row of rows) {
    if (byRef.has(row.ref)) errors.push(`duplicate live source lock ${row.ref}`);
    byRef.set(row.ref, row);
    if (row.ref !== `live:${row.path}` || !Number.isSafeInteger(row.bytes) || !isDigest(row.sha256) || (row.commit !== undefined && !isFullSha(row.commit))) { errors.push(`invalid live source lock row ${row.ref}`); continue; }
    try {
      const bytes = row.commit ? gitPathAt(row.commit, row.path, repoRoot) : fs.readFileSync(confinedFile(repoRoot, row.path, `live source ${row.ref}`));
      if (!bytes) throw new Error(`missing Git object ${row.commit}:${row.path}`);
      if (bytes.length !== row.bytes || sha256(bytes) !== row.sha256) errors.push(`live source digest mismatch ${row.ref}`);
    } catch (error) { errors.push(`live source missing/unsafe ${row.ref}: ${error.message}`); }
  }
  const cited = new Set(authority.nodes.flatMap((node) => node.source_refs).filter((ref) => ref.startsWith("live:")));
  if (JSON.stringify([...cited].sort()) !== JSON.stringify([...byRef.keys()].sort())) errors.push("live source lock/ref parity mismatch");
  for (const node of authority.nodes) {
    const expected = node.source_refs.filter((ref) => ref.startsWith("live:")).map((ref) => {
      const row = byRef.get(ref);
      return row ? `${ref} — ${row.bytes} bytes, SHA-256 ${row.sha256}` : null;
    }).filter(Boolean).sort();
    if (!expected.length) continue;
    const charter = fs.readFileSync(confinedFile(authority.packageRoot, node.charter, `${node.id} charter`), "utf8");
    const actual = [...charter.matchAll(/^- `(live:[^`]+)` — (\d+) bytes, SHA-256 `([0-9a-f]{64})`$/gm)].map((match) => `${match[1]} — ${match[2]} bytes, SHA-256 ${match[3]}`).sort();
    if (JSON.stringify(actual) !== JSON.stringify(expected)) errors.push(`${node.id}: live authority input parity mismatch`);
  }
  return errors;
}

function validateCatalogs(authority) {
  const errors = [];
  const root = authority.packageRoot;
  const rows = (name, table) => readToml(confinedFile(path.join(root, "catalogs"), name, `catalog ${name}`))[table] || [];
  const reviewRows = rows("review-profiles.toml", "profile");
  const gateRows = rows("gate-profiles.toml", "profile");
  const resourceRows = rows("resource-profiles.toml", "profile");
  const domainRows = rows("conflict-domains.toml", "domain");
  const indexed = (items, label) => {
    const map = new Map();
    for (const item of items) {
      if (map.has(item.id)) errors.push(`duplicate ${label} ${item.id}`);
      map.set(item.id, item);
    }
    return map;
  };
  const reviews = indexed(reviewRows, "review profile");
  const gates = indexed(gateRows, "gate profile");
  const resources = indexed(resourceRows, "resource profile");
  const domains = indexed(domainRows, "conflict domain");
  for (const profile of reviewRows) {
    if (profile.id === "history") {
      if (profile.reviewers !== 0 || profile.lenses.length !== 1 || profile.risk_band !== "audit-only" || profile.confirmation_policy !== "not-required") errors.push("history review profile must remain closed legacy validation");
      continue;
    }
    const expectedCount = profile.risk_band === "high" ? 3 : profile.risk_band === "medium" ? profile.reviewers : 1;
    if (profile.independent !== true || profile.reviewers !== expectedCount || profile.lenses.length !== profile.reviewers || profile.lenses[0] !== "adversarial" || (profile.risk_band === "high" && profile.lenses[1] !== "conformance") || (profile.risk_band === "medium" && (profile.reviewers < 1 || profile.reviewers > 2)) || profile.dispatch !== "fresh-distinct-harness-task" || profile.provider_policy !== "provider-neutral" || !["low", "medium", "high"].includes(profile.minimum_effort) || !["not-required", "targeted", "independent-full"].includes(profile.confirmation_policy)) {
      errors.push(`${profile.id}: review profile violates the risk-scaled fresh provider-neutral harness policy`);
    }
  }
  for (const profile of gateRows) if (profile.id !== "legacy-receipt" && (!Array.isArray(profile.final) || profile.final.length === 0)) errors.push(`${profile.id}: final gate commands must be nonempty`);
  for (const profile of resourceRows) if (!Number.isSafeInteger(profile.capacity_hint) || profile.capacity_hint < 1) errors.push(`${profile.id}: invalid capacity_hint`);
  for (const domain of domainRows) {
    if (!Array.isArray(domain.path_roots) || !domain.path_roots.length || !Array.isArray(domain.symbols) || !domain.symbols.length) errors.push(`${domain.id}: conflict domain lacks concrete path/symbol ownership`);
    for (const relative of domain.path_roots || []) try { safeRelative(relative, `conflict domain ${domain.id}`); }
    catch (error) { errors.push(error.message); }
  }
  for (const node of authority.nodes) {
    if (!reviews.has(node.review_profile)) errors.push(`${node.id}: unknown review profile ${node.review_profile}`);
    if (!gates.has(node.gate_profile)) errors.push(`${node.id}: unknown gate profile ${node.gate_profile}`);
    if (!resources.has(node.resource_class)) errors.push(`${node.id}: unknown resource profile ${node.resource_class}`);
    for (const domain of node.conflict_domains) if (!domains.has(domain)) errors.push(`${node.id}: unknown conflict domain ${domain}`);
    if (node.review_profile !== "history") {
      const surfaces = charterMutationRoots(authority, node);
      if (!surfaces.length) errors.push(`${node.id}: charter has no structured production surfaces`);
      for (const surface of surfaces) {
        try { safeRelative(surface, `${node.id} production surface`); }
        catch (error) { errors.push(error.message); continue; }
        const covered = node.conflict_domains.some((id) => (domains.get(id)?.path_roots || []).some((root) => pathsIntersect(surface, root)));
        if (!covered) errors.push(`${node.id}: production surface is outside every acquired conflict domain: ${surface}`);
      }
    }
  }
  return errors;
}

function validateActivation(authority) {
  const errors = [];
  const { nodes, metadata, packageRoot } = authority;
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const activation = readToml(path.join(packageRoot, "authority/state/activation.toml"));
  const custody = readToml(path.join(packageRoot, "authority/state/external-custody-boundary.toml"));
  const requiredCustody = nodes.flatMap((node) => node.external_requirements.map((requirement) => `${node.id}:${requirement}`)).sort();
  if (JSON.stringify([...(custody.required_slots || [])].sort()) !== JSON.stringify(requiredCustody)) errors.push("external custody boundary does not enumerate every exact authorization slot");
  const externalManifest = readToml(path.join(packageRoot, "authority/state/external-authorizations.toml"));
  const ratifications = readToml(path.join(packageRoot, "authority/state/trusted-ratifications.toml"));
  const anchored = [...(custody.directive_anchored_slots || [])].sort();
  const unverified = [...(custody.authorization_required_slots || [])].sort();
  if (JSON.stringify([...anchored, ...unverified].sort()) !== JSON.stringify(requiredCustody) || anchored.some((slot) => unverified.includes(slot))) errors.push("external custody verified/unverified slot partition is incomplete or overlapping");
  const exactDirectiveSlot = externalManifest.authorization?.find((row) => `${row.node_id}:${row.authorization}` === "ORC0:maintainer_unified_v2_activation");
  try {
    const directive = confinedFile(packageRoot, custody.directive_path, "maintainer activation directive");
    if (sha256(fs.readFileSync(directive)) !== custody.directive_sha256 || custody.directive_sha256 !== "1865ad8cbb565906066f49675e1bbfa656476f3a746aebd3d497adf133e91e9b") errors.push("maintainer activation directive bytes/digest mismatch");
  } catch (error) { errors.push(`maintainer activation directive missing/unsafe: ${error.message}`); }
  if (!exactDirectiveSlot || exactDirectiveSlot.granted_by !== "maintainer-task-thread-2026-08-27" || exactDirectiveSlot.ratification_path !== custody.directive_path || exactDirectiveSlot.ratification_receipt_sha256 !== custody.directive_sha256 || exactDirectiveSlot.grant_mode !== "MAINTAINER_DIRECTIVE_FINALIZED_CANDIDATE" || exactDirectiveSlot.directive_scope !== "unified-v2-orc0-activation-only") errors.push("ORC0 directive-anchored authorization slot is not exact");
  const unauthorizedStaticSlots = (externalManifest.authorization || []).filter((row) => !anchored.includes(`${row.node_id}:${row.authorization}`));
  if (unauthorizedStaticSlots.length || (ratifications.slot || []).length) errors.push("fail-closed custody boundary contains an unanchored static authorization/ratification slot");
  if (fs.existsSync(path.join(packageRoot, "authority/state/.activation.lock"))) errors.push("activation transaction is incomplete; partial activation is refused");
  if (metadata.state !== activation.package_state) errors.push("activation/root package state mismatch");
  if (activation.c1_accepted_sha !== "267cfd0079022dd278b2414e209f459f27d6a721" || activation.c1_accepted_tree !== "c1bf69e65346fe3febfd8ed9eccd27f7e5bf18fa") errors.push("C1 accepted identity mismatch");
  if (activation.j1_state === "IN_FLIGHT" && activation.j1_receipt !== "") errors.push("J1 IN_FLIGHT must not claim a receipt");
  if (activation.j1_state === "LANDED_GRANDFATHERED" && !/^J1-LANDED-GRANDFATHERED:[0-9a-f]{64}$/.test(activation.j1_receipt)) errors.push("J1 LANDED_GRANDFATHERED must bind its exact landed receipt");
  if (activation.package_state === "DORMANT" && (activation.orc0_receipt || activation.activation_authorization || activation.active_authority_sha256 || activation.activation_transition)) errors.push("DORMANT activation contains ACTIVE-only receipt bindings");
  if (activation.package_state === "ACTIVE" && (activation.j1_state !== "LANDED_GRANDFATHERED" || !/^ORC0:[0-9a-f]{64}$/.test(activation.orc0_receipt) || !/^maintainer_unified_v2_activation:[0-9a-f]{64}$/.test(activation.activation_authorization) || !isDigest(activation.active_authority_sha256) || !/^ACT-[A-Z0-9-]+:[0-9a-f]{64}$/.test(activation.activation_transition))) errors.push("ACTIVE activation is missing exact J1 landed/ORC0/authorization/digest/transition bindings");
  const orc = byId.get("ORC0");
  if (!orc || JSON.stringify(orc.predecessors) !== JSON.stringify(["C1", "J1"]) || !orc.external_requirements.includes("maintainer_unified_v2_activation")) errors.push("ORC0 must require exact C1 acceptance, J1 grandfathered landing, and maintainer activation");
  if (byId.get("TCM0")?.dispatchable !== false || !byId.get("TCM0R") || ["TCM1", "TCM2", "TCM3", "TCM4"].some((id) => !byId.get(id).predecessors.includes("TCM0R") || byId.get(id).predecessors.includes("TCM0"))) errors.push("TCM0 rejection/replacement topology invalid");
  const br0 = byId.get("BR0");
  if (!br0 || br0.predecessors.length !== 0 || JSON.stringify(br0.external_requirements) !== JSON.stringify(["maintainer_rev11_repair_freeze_lift", "maintainer_successor_genesis"]) || byId.get("UAK0")?.predecessors[0] !== "BR0") errors.push("source-canonical BR0 successor genesis topology invalid");
  if (byId.has("BR0P") || nodes.some((node) => /^HFP[0-9]+$/.test(node.id))) errors.push("invented BR0P/HFP authority is forbidden");
  if (byId.get("OPT0")?.dispatchable !== false || byId.has("VCB0")) errors.push("OPT0 must remain non-dispatchable and unsupported VCB0 must be absent");
  for (const node of nodes) {
    if (node.review_profile !== "history" && node.id !== "ORC0" && node.activation_gate !== "ORC0") errors.push(`${node.id}: new v2 node lacks ORC0 activation gate`);
    if (node.initial_state === "ACCEPTED" && node.review_profile !== "history") errors.push(`${node.id}: non-history initial ACCEPTED is forbidden`);
  }
  return errors;
}

export function validateAuthority(authority, { strict = true, checkGenerated = true, checkAmendments = true, runtimeRoot = defaultRuntimeRoot(authority.packageRoot) } = {}) {
  const errors = [];
  const { metadata, nodes, moduleModels, packageRoot } = authority;
  if (metadata.schema !== 4 || metadata.revision !== 11) errors.push("root schema/revision mismatch");
  if (metadata.pinned_commit !== "5a8ca4a391ea6d748f2891f1e39de6aaeec7987e" || metadata.pinned_tree !== "7ab8e3317fb8c8a15b7d63c6114a76f79f79f46d") errors.push("root pinned basis mismatch");
  if (new Set(metadata.modules).size !== metadata.modules.length) errors.push("duplicate module path");
  for (const { model, relative } of moduleModels) {
    if (model.schema !== 4 || model.pinned_commit !== metadata.pinned_commit || model.pinned_tree !== metadata.pinned_tree) errors.push(`${relative}: module metadata mismatch`);
  }
  errors.push(...validateGraphModel(nodes, { packageRoot }));
  errors.push(...validateCatalogs(authority));
  errors.push(...validateSourceRefs(authority));
  errors.push(...validateSchemasAndTemplates(authority));
  errors.push(...validateSourceCoverage(packageRoot, nodes));
  errors.push(...validateCanonicalSuccessorLedger(authority));
  errors.push(...validateLiveSourceLock(authority));
  errors.push(...validateActivation(authority));
  if (checkAmendments) errors.push(...validateAmendments(authority, { runtimeRoot }));
  if (checkGenerated) {
    for (const [relative, expectedRaw] of generatedFiles(authority)) {
      const expected = expectedRaw.endsWith("\n") ? expectedRaw : `${expectedRaw}\n`;
      const file = path.join(packageRoot, relative);
      if (!fs.existsSync(file) || fs.readFileSync(file, "utf8") !== expected) errors.push(`generated projection is stale: ${relative}`);
    }
  }
  if (strict && errors.length) return errors;
  return errors;
}

function receiptInvalidationInventory(authority, runtimeRoot, context) {
  const impacted = new Set(context.impactClosure);
  const amendmentState = loadAmendmentRows(authority);
  if (amendmentState.errors.length) throw new Error(amendmentState.errors.join("; "));
  const currentRow = context.amendment;
  const priorInvalidations = new Set(
    amendmentState.rows
      .filter((row) => row.amendment_id !== currentRow.amendment_id)
      .filter((row) => {
        const errors = authorityAncestryErrors(row.after_authority_sha256, "__receipt_inventory__", context.beforeAuthoritySha256, amendmentState.rows, `amendment ${row.amendment_id}`);
        return errors.length === 0;
      })
      .flatMap((row) => row.invalidated_receipts),
  );
  let baselineDigest = context.beforeAuthoritySha256;
  const lockFile = path.join(authority.packageRoot, "authority/state/authority-lock.toml");
  if (fs.existsSync(lockFile)) {
    const lock = readToml(lockFile);
    if (isDigest(lock.baseline_authority_sha256)) baselineDigest = lock.baseline_authority_sha256;
  }
  const result = [];
  const consider = (file, schemaName, originDigest, expectedDirectory) => {
    const checked = validatePayloadArtifact(file, authority.packageRoot, schemaName, "accepted receipt inventory");
    if (checked.errors.length) throw new Error(checked.errors.join("; "));
    const row = checked.row;
    if (!row || row.verdict !== "ACCEPTED" || !impacted.has(row.node_id)) return;
    if (path.dirname(file) !== expectedDirectory || path.basename(file) !== `${row.node_id}.toml`) throw new Error(`accepted receipt inventory filename mismatch ${file}`);
    const receiptRef = reference(row.node_id, { digest: checked.digest });
    if (priorInvalidations.has(receiptRef)) return;
    if (authorityAncestryErrors(originDigest, row.node_id, context.beforeAuthoritySha256, amendmentState.rows, `accepted receipt ${receiptRef}`).length) return;
    result.push(receiptRef);
  };
  const legacy = staticLegacyFiles(authority);
  if (legacy.errors.length) throw new Error(legacy.errors.join("; "));
  const legacyDirectory = path.join(authority.packageRoot, "state/legacy-receipts");
  for (const file of legacy.files) consider(file, "legacy-receipt.schema.json", baselineDigest, legacyDirectory);
  const runtime = runtimeFiles(authority, runtimeRoot, "receipts");
  if (runtime.errors.length) throw new Error(runtime.errors.join("; "));
  const runtimeDirectoryPath = runtime.files[0] ? path.dirname(runtime.files[0]) : path.join(path.resolve(runtimeRoot), "receipts");
  for (const file of runtime.files) {
    const parsed = readToml(file);
    if (parsed.type !== "v2-acceptance" || !isDigest(parsed.authority_sha256)) throw new Error(`accepted receipt inventory contains a non-v2 artifact ${file}`);
    consider(file, "receipt.schema.json", parsed.authority_sha256, runtimeDirectoryPath);
  }
  return [...new Set(result)].sort();
}

function amendmentDependencies(authority, runtimeRoot) {
  return {
    computeAuthorityDigest,
    loadAuthority,
    validateSchemaObject,
    deriveInvalidatedReceipts: (context) => receiptInvalidationInventory(authority, runtimeRoot, context),
  };
}

export function validateAmendments(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot) } = {}) {
  return validateAmendmentChain(authority, amendmentDependencies(authority, runtimeRoot));
}

export function createAmendment(authority, options) {
  const runtimeRoot = options?.runtimeRoot || defaultRuntimeRoot(authority.packageRoot);
  return createAuthorityAmendment(authority, options, amendmentDependencies(authority, runtimeRoot));
}

const AUTHORITY_DIGEST_ROOTS = ["authority", "catalogs", "charters", "contracts", "provenance", "schemas", "sources", "state", "templates", "tools"];

export function computeAuthorityDigest(packageRoot = PACKAGE_ROOT) {
  const rootReal = fs.realpathSync(packageRoot);
  const files = [];
  const walk = (directory, relativeDirectory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const relative = path.posix.join(relativeDirectory, entry.name);
      if (relative === "authority/state/authority-lock.toml" || relative.startsWith("authority/state/amendments/")) continue;
      if (relative === "authority/state/.activation.lock" || relative.startsWith("authority/state/.activation.lock/")) continue;
      const absolute = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`authority digest refuses symlink ${relative}`);
      if (entry.isDirectory()) walk(absolute, relative);
      else if (entry.isFile()) files.push([relative, absolute]);
      else throw new Error(`authority digest refuses unsupported entry ${relative}`);
    }
  };
  for (const relative of AUTHORITY_DIGEST_ROOTS) {
    const absolute = path.join(rootReal, relative);
    if (fs.existsSync(absolute)) walk(absolute, relative);
  }
  const hash = crypto.createHash("sha256");
  for (const [relative, absolute] of files.sort(([left], [right]) => left.localeCompare(right))) {
    let bytes = fs.readFileSync(absolute);
    // Runtime lifecycle outputs are separately receipt-bound. Canonicalizing
    // only those mutable outputs keeps the authority digest stable through
    // activation while preserving immutable J1 expectations in its identity.
    if (relative === "authority/root.toml") {
      bytes = Buffer.from(bytes.toString("utf8").replace(/^state = "(?:DORMANT|ACTIVE)"$/m, 'state = "<lifecycle-state>"'));
    } else if (relative === "authority/state/activation.toml") {
      let text = bytes.toString("utf8");
      text = text.replace(/^package_state = "(?:DORMANT|ACTIVE)"$/m, 'package_state = "<lifecycle-state>"');
      for (const key of ["orc0_receipt", "activation_authorization", "active_authority_sha256", "activation_transition"]) text = text.replace(new RegExp(`^${key} = ".*"$`, "m"), `${key} = "<lifecycle-binding>"`);
      bytes = Buffer.from(text);
    }
    hash.update(relative);
    hash.update("\0");
    hash.update(String(bytes.length));
    hash.update("\0");
    hash.update(bytes);
    hash.update("\0");
  }
  return hash.digest("hex");
}

function payloadPrefix(text) {
  const markers = text.match(/^payload_sha256\s*=/gm) || [];
  if (markers.length !== 1) return null;
  const match = /^payload_sha256 = "([0-9a-f]{64})"\n?$/.exec(text.slice(text.search(/^payload_sha256\s*=/m)));
  if (!match) return null;
  const index = text.search(/^payload_sha256\s*=/m);
  return { prefix: text.slice(0, index), digest: match[1] };
}

function gitOutput(args, cwd) {
  return childProcess.execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: CHILD_PROCESS_TIMEOUT_MS, killSignal: "SIGKILL" }).trim();
}

function gitRoot(packageRoot) {
  try { return gitOutput(["rev-parse", "--show-toplevel"], packageRoot); }
  catch { return null; }
}

export function trustedLocalControlRoot(authority) {
  const repository = gitRoot(authority.packageRoot);
  if (!repository) throw new Error("trusted-local lifecycle requires a Git repository");
  const common = gitOutput(["rev-parse", "--git-common-dir"], repository);
  const commonRoot = path.resolve(repository, common);
  return path.join(commonRoot, "verter-unified-trusted-local");
}

function trustedLocalAcceptances(authority, runtimeRoot) {
  const acceptances = new Map(); const errors = [];
  const resolvedRuntime = path.resolve(runtimeRoot);
  let anchor;
  try {
    const controlRoot = trustedLocalControlRoot(authority);
    const anchorFile = path.join(controlRoot, "anchor.json");
    if (!fs.existsSync(anchorFile)) return { acceptances, errors };
    anchor = readLocalAnchor({ controlRoot });
  } catch (error) {
    errors.push(`trusted-local custody anchor is unreadable or malformed: ${error.message}`);
    return { acceptances, errors };
  }

  const readClaim = (file, label, claimErrors) => {
    if (!fs.existsSync(file)) { claimErrors.push(`${label} runtime artifact is missing: ${file}`); return null; }
    try { const bytes = fs.readFileSync(file); return { bytes, parsed: JSON.parse(bytes) }; }
    catch (error) { claimErrors.push(`${label} runtime artifact is malformed: ${error.message}`); return null; }
  };
  const acceptedByRound = new Map(); const acceptedByNode = new Map();
  for (const [roundId, round] of Object.entries(anchor.rounds).filter(([, row]) => row.status === "ACCEPTED")) {
    const lease = anchor.leases[round.lease_id];
    if (lease && lease.runtime_root !== resolvedRuntime) continue;
    const claimErrors = []; const label = `trusted-local accepted claim ${roundId}`;
    if (!lease) claimErrors.push(`${label} has no exact anchor lease`);
    else {
      if (lease.lease_id !== round.lease_id || lease.node_id !== round.node_id || lease.status !== "ACCEPTED") claimErrors.push(`${label} round/lease identity or status mismatches`);
      if (!lease.finalization?.candidate || !lease.finalization?.review_target_sha256) claimErrors.push(`${label} has no exact finalized candidate custody`);
    }
    const file = path.join(resolvedRuntime, "trusted-local", "acceptances", `${roundId}.json`);
    const artifact = readClaim(file, label, claimErrors);
    const expected = lease?.finalization?.candidate && {
      schema: 1, type: "trusted-local-acceptance", assurance: "operator-attested-local-consistency",
      round_id: roundId, lease_id: lease.lease_id, node_id: round.node_id, accepted_by: lease.holder,
      candidate_sha: lease.finalization.candidate.sha, candidate_tree: lease.finalization.candidate.tree,
      candidate_ref: lease.finalization.candidate.ref, review_target_sha256: lease.finalization.review_target_sha256,
    };
    if (artifact && (!expected || JSON.stringify(artifact.parsed) !== JSON.stringify(expected))) claimErrors.push(`${label} runtime artifact mismatches the anchor claim`);
    if (claimErrors.length) { errors.push(...claimErrors); continue; }
    const receipt = artifact.parsed; const digest = sha256(artifact.bytes);
    const trusted = { roundId, round, lease, receipt, landing: null, file, digest, reference: `${round.node_id}:${digest}` };
    acceptedByRound.set(roundId, trusted);
    const claims = acceptedByNode.get(round.node_id) || []; claims.push(trusted); acceptedByNode.set(round.node_id, claims);
  }

  for (const [roundId, landingClaim] of Object.entries(anchor.landings || {})) {
    const round = anchor.rounds[roundId]; const lease = round && anchor.leases[round.lease_id];
    if (lease && lease.runtime_root !== resolvedRuntime) continue;
    const claimErrors = []; const label = `trusted-local landing claim ${roundId}`;
    const trusted = acceptedByRound.get(roundId);
    if (!round || round.status !== "ACCEPTED") claimErrors.push(`${label} has no ACCEPTED round claim`);
    if (!lease || lease.status !== "ACCEPTED" || lease.runtime_root !== resolvedRuntime) claimErrors.push(`${label} has no exact accepted anchor lease in this runtime`);
    if (!trusted) claimErrors.push(`${label} has no valid accepted-round custody`);
    const file = path.join(resolvedRuntime, "trusted-local", "landings", `${roundId}.json`);
    const artifact = readClaim(file, label, claimErrors);
    if (artifact && JSON.stringify(artifact.parsed) !== JSON.stringify(landingClaim)) claimErrors.push(`${label} runtime artifact mismatches the anchor claim`);
    if (trusted && (landingClaim.type !== "trusted-local-candidate-landing" || landingClaim.round_id !== roundId || landingClaim.lease_id !== trusted.lease.lease_id || landingClaim.node_id !== trusted.receipt.node_id || landingClaim.landed_by !== trusted.lease.holder || landingClaim.candidate_sha !== trusted.receipt.candidate_sha || landingClaim.candidate_tree !== trusted.receipt.candidate_tree || landingClaim.candidate_ref !== trusted.receipt.candidate_ref || landingClaim.canonical_sha !== trusted.receipt.candidate_sha || landingClaim.canonical_tree !== trusted.receipt.candidate_tree)) claimErrors.push(`${label} identity mismatches its accepted candidate custody`);
    if (claimErrors.length) { errors.push(...claimErrors); continue; }
    const digest = sha256(artifact.bytes);
    trusted.landing = { ...artifact.parsed, file, digest, reference: `${trusted.receipt.node_id}-LANDING:${digest}` };
  }

  for (const [nodeId, claims] of acceptedByNode) {
    if (claims.length !== 1) errors.push(`trusted-local accepted claims for ${nodeId} are ambiguous: ${claims.map((row) => row.roundId).join(", ")}`);
    else acceptances.set(nodeId, claims[0]);
  }
  return { acceptances, errors };
}

function trustedSuccessorReceipt(authority, trusted) {
  if (!trusted?.landing) return { receipt: null, error: "" };
  const repository = gitRoot(authority.packageRoot);
  const canonicalRef = `refs/heads/${authority.metadata.canonical_integration_branch}`;
  const canonicalSha = repository && validGitRef(canonicalRef, repository);
  const candidateTree = repository && gitTree(trusted.receipt.candidate_sha, repository);
  const landingTree = repository && gitTree(trusted.landing.canonical_sha, repository);
  if (!repository || candidateTree !== trusted.receipt.candidate_tree || landingTree !== trusted.landing.canonical_tree
    || trusted.landing.canonical_ref !== canonicalRef || !canonicalSha || !gitIsAncestor(trusted.landing.canonical_sha, canonicalSha, repository)) {
    return { receipt: null, error: `trusted-local landing ${trusted.roundId} is not an exact candidate/landing Git identity retained by the current canonical history` };
  }
  return { receipt: {
    receipt_id: trusted.receipt.node_id,
    node_id: trusted.receipt.node_id,
    digest: trusted.digest,
    file: trusted.file,
    candidate_sha: trusted.receipt.candidate_sha,
    candidate_tree: trusted.receipt.candidate_tree,
    candidate_ref: trusted.receipt.candidate_ref,
    integration_sha: trusted.landing.canonical_sha,
    integration_tree: trusted.landing.canonical_tree,
    trusted_local_acceptance: trusted,
  }, error: "" };
}

function gitWorktrees(repository) {
  if (!repository) return [];
  try {
    return gitOutput(["worktree", "list", "--porcelain", "-z"], repository)
      .split("\0\0")
      .map((record) => Object.fromEntries(record.split("\0").filter(Boolean).map((field) => {
        const split = field.indexOf(" ");
        return split < 0 ? [field, ""] : [field.slice(0, split), field.slice(split + 1)];
      })))
      .filter((row) => row.worktree);
  } catch { return []; }
}

function gitTree(sha, cwd) {
  try {
    if (gitOutput(["cat-file", "-t", sha], cwd) !== "commit") return null;
    return gitOutput(["show", "-s", "--format=%T", sha], cwd);
  }
  catch { return null; }
}

function gitObjectAt(sha, relative, cwd) {
  try {
    const output = childProcess.execFileSync("git", ["ls-tree", "-z", sha, "--", relative], { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: CHILD_PROCESS_TIMEOUT_MS, killSignal: "SIGKILL" });
    const entry = output.split("\0").find((value) => value.endsWith(`\t${relative}`));
    return entry ? /^[0-7]+ [a-z]+ ([0-9a-f]{40})\t/.exec(entry)?.[1] || null : null;
  } catch { return null; }
}

function gitIsAncestor(ancestor, descendant, cwd) {
  try {
    childProcess.execFileSync("git", ["merge-base", "--is-ancestor", ancestor, descendant], { cwd, stdio: "ignore", timeout: CHILD_PROCESS_TIMEOUT_MS, killSignal: "SIGKILL" });
    return true;
  } catch { return false; }
}

function gitChangedPaths(base, integration, repository) {
  try {
    const output = childProcess.execFileSync("git", ["diff", "--name-only", "-z", base, integration, "--"], { cwd: repository, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], maxBuffer: 32 * 1024 * 1024, timeout: CHILD_PROCESS_TIMEOUT_MS, killSignal: "SIGKILL" });
    return output.split("\0").filter(Boolean).sort();
  } catch { return null; }
}

function gitFileAt(commit, file, repository) {
  try {
    const relative = path.relative(repository, file).split(path.sep).join("/");
    return gitPathAt(commit, relative, repository);
  } catch { return null; }
}

function gitPathAt(commit, relative, repository) {
  try {
    safeRelative(relative, "Git object path");
    return childProcess.execFileSync("git", ["show", `${commit}:${relative}`], { cwd: repository, stdio: ["ignore", "pipe", "ignore"], maxBuffer: 32 * 1024 * 1024, timeout: CHILD_PROCESS_TIMEOUT_MS, killSignal: "SIGKILL" });
  } catch { return null; }
}

function validatePayloadArtifact(file, packageRoot, schemaName, label) {
  const errors = [];
  let text;
  let row;
  try {
    const stat = fs.lstatSync(file);
    if (stat.isSymbolicLink() || !stat.isFile()) throw new Error("artifact must be a non-symlink regular file");
    text = fs.readFileSync(file, "utf8");
    row = parseToml(text);
  } catch (error) { return { errors: [`malformed ${label} ${file}: ${error.message}`], row: null, digest: null, text: null }; }
  const payload = payloadPrefix(text);
  if (!payload) errors.push(`${label} payload_sha256 must be the single final field ${file}`);
  else if (payload.digest !== row.payload_sha256 || sha256(payload.prefix) !== row.payload_sha256) errors.push(`${label} digest mismatch ${file}`);
  try { errors.push(...validateSchemaObject(row, loadSchema(packageRoot, schemaName), `${label}.${path.basename(file)}`)); }
  catch (error) { errors.push(error.message); }
  return { errors, row, digest: row?.payload_sha256 || null, text };
}

function runtimeDirectory(authority, runtimeRoot, subdirectory, { create = false } = {}) {
  const repository = gitRoot(authority.packageRoot);
  const resolved = path.resolve(runtimeRoot);
  const worktreeRoots = gitWorktrees(repository).map((row) => path.resolve(row.worktree));
  if (worktreeRoots.some((worktree) => isInside(worktree, resolved))) throw new Error(`runtime root must remain outside every Git worktree: ${resolved}`);
  if (!fs.existsSync(resolved)) {
    if (!create) return null;
    fs.mkdirSync(resolved, { recursive: true, mode: 0o700 });
  }
  if (fs.lstatSync(resolved).isSymbolicLink() || !fs.statSync(resolved).isDirectory()) throw new Error(`runtime root must be a non-symlink directory: ${resolved}`);
  const runtimeReal = fs.realpathSync(resolved);
  const worktreeReals = worktreeRoots.filter((worktree) => fs.existsSync(worktree)).map((worktree) => fs.realpathSync(worktree));
  if (worktreeReals.some((worktree) => isInside(worktree, runtimeReal))) throw new Error(`runtime root resolves inside a Git worktree: ${resolved}`);
  const child = path.join(resolved, subdirectory);
  if (!fs.existsSync(child)) {
    if (!create) return null;
    try {
      fs.mkdirSync(child, { mode: 0o700 });
    } catch (error) {
      // Concurrent lifecycle commands may both observe the directory as
      // absent.  EEXIST is safe only after the lstat/stat checks below prove
      // that the winner created the expected non-symlink directory.
      if (error?.code !== "EEXIST") throw error;
    }
  }
  if (fs.lstatSync(child).isSymbolicLink() || !fs.statSync(child).isDirectory()) throw new Error(`runtime ${subdirectory} must be a non-symlink directory`);
  const childReal = fs.realpathSync(child);
  if (!isInside(runtimeReal, childReal)) throw new Error(`runtime ${subdirectory} escapes its root`);
  return childReal;
}

function runtimeFiles(authority, runtimeRoot, subdirectory) {
  const errors = [];
  let directory;
  try { directory = runtimeDirectory(authority, runtimeRoot, subdirectory); }
  catch (error) { return { files: [], errors: [error.message] }; }
  if (!directory) return { files: [], errors };
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (!entry.isFile() || entry.isSymbolicLink() || !entry.name.endsWith(".toml")) errors.push(`runtime ${subdirectory} contains unsupported entry ${entry.name}`);
    else files.push(file);
  }
  return { files: files.sort(), errors };
}

function catalogMap(packageRoot, fileName, tableName) {
  const rows = readToml(confinedFile(path.join(packageRoot, "catalogs"), fileName, `catalog ${fileName}`))[tableName] || [];
  return new Map(rows.map((row) => [row.id, row]));
}

function evidenceProfiles(node) {
  // J1 is the single grandfathered IN_FLIGHT unit. It is not accepted legacy
  // history: closing it requires fresh canonical gates and three reviews.
  return node.id === "J1" && node.initial_state === "IN_FLIGHT"
    ? { gate: "canonical", review: "architecture-3" }
    : { gate: node.gate_profile, review: node.review_profile };
}

function candidateIdentity(row, packageRoot, label) {
  const errors = [];
  const repository = gitRoot(packageRoot);
  if (!repository) return [`${label}: Git repository unavailable`];
  const candidateTree = isFullSha(row.candidate_sha) ? gitTree(row.candidate_sha, repository) : null;
  if (!candidateTree || candidateTree !== row.candidate_tree) errors.push(`${label}: candidate Git identity/tree mismatch`);
  return errors;
}

function exactList(actual, expected, label) {
  const left = Array.isArray(actual) ? [...actual].sort() : [];
  const right = [...expected].sort();
  return JSON.stringify(left) === JSON.stringify(right) ? [] : [`${label}: exact set mismatch; expected [${right.join(", ")}], got [${left.join(", ")}]`];
}

function reference(id, row) {
  return `${id}:${row.digest || row.payload_sha256}`;
}

export function validateReceiptFile(file, node, packageRoot = PACKAGE_ROOT, context = null) {
  let parsed;
  try { parsed = parseToml(fs.readFileSync(file, "utf8")); }
  catch (error) { return { errors: [`malformed receipt ${file}: ${error.message}`], receipt: null, digest: null }; }
  const schemaName = parsed.type === "legacy-accepted" ? "legacy-receipt.schema.json" : "receipt.schema.json";
  const checked = validatePayloadArtifact(file, packageRoot, schemaName, "receipt");
  const receipt = checked.row;
  const errors = [...checked.errors];
  if (!node || receipt?.node_id !== node.id) errors.push(`receipt node mismatch ${file}`);
  if (!receipt) return { errors, receipt: null, digest: null };
  if (checked.errors.length) return { errors, receipt, digest: checked.digest };
  if (receipt.type === "legacy-accepted") {
    const manifest = readToml(confinedFile(packageRoot, "authority/state/legacy-receipts.toml", "legacy receipt manifest")).receipt || [];
    const manifestRow = manifest.find((row) => row.node_id === receipt.node_id);
    if (!manifestRow || sha256(fs.readFileSync(file)) !== manifestRow.file_sha256 || receipt.payload_sha256 !== manifestRow.payload_sha256) errors.push(`legacy receipt is not in the immutable manifest ${file}`);
    const expected = path.resolve(packageRoot, "state/legacy-receipts", `${receipt.node_id}.toml`);
    if (path.resolve(file) !== expected || node?.initial_state !== "ACCEPTED") errors.push(`invalid legacy receipt location/state ${file}`);
    errors.push(...exactList(receipt.predecessors, node?.predecessors || [], `legacy receipt ${receipt.node_id} predecessors`));
    const repository = gitRoot(packageRoot);
    const tree = repository && gitTree(receipt.accepted_sha, repository);
    // The closed v1 receipts predate the unified charter tree. Their digest is
    // bound to the original, ID-addressed Rev11 charter at the accepted commit.
    const charterAtAcceptance = repository && node && gitPathAt(receipt.accepted_sha, `docs/arch/refactor/rev11/charters/${node.id}.md`, repository);
    if (!charterAtAcceptance || sha256(charterAtAcceptance) !== receipt.charter_sha256) errors.push(`legacy receipt charter digest mismatch at accepted commit ${file}`);
    if (!tree || tree !== receipt.accepted_tree || receipt.accepted_sha !== receipt.candidate_sha || receipt.accepted_sha !== receipt.integration_sha || receipt.accepted_tree !== receipt.candidate_tree || receipt.accepted_tree !== receipt.integration_tree) errors.push(`invalid legacy receipt Git identity ${file}`);
  } else if (receipt.type === "v2-acceptance") {
    let charterFile;
    try { charterFile = confinedFile(packageRoot, node?.charter, `${node?.id || "unknown"} charter`); }
    catch (error) { errors.push(`receipt charter path invalid ${file}: ${error.message}`); }
    if (!charterFile || sha256(fs.readFileSync(charterFile)) !== receipt.charter_sha256) errors.push(`receipt charter digest mismatch ${file}`);
    const repository = gitRoot(packageRoot);
    if (!repository) errors.push(`receipt Git repository unavailable ${file}`);
    else {
      const baseTree = gitTree(receipt.base_sha, repository);
      const candidateTree = gitTree(receipt.candidate_sha, repository);
      const integrationTree = gitTree(receipt.integration_sha, repository);
      if (!baseTree) errors.push(`receipt base Git commit does not exist ${file}`);
      if (!candidateTree || candidateTree !== receipt.candidate_tree) errors.push(`receipt candidate Git identity/tree mismatch ${file}`);
      if (!integrationTree || integrationTree !== receipt.integration_tree) errors.push(`receipt integration Git identity/tree mismatch ${file}`);
      if (baseTree && candidateTree && !gitIsAncestor(receipt.base_sha, receipt.candidate_sha, repository)) errors.push(`receipt base is not an ancestor of candidate ${file}`);
      if (candidateTree && integrationTree && !gitIsAncestor(receipt.candidate_sha, receipt.integration_sha, repository)) errors.push(`receipt candidate is not an ancestor of integration ${file}`);
      const integrationHead = validGitRef(`refs/heads/${readToml(confinedFile(packageRoot, "authority/root.toml", "root authority")).canonical_integration_branch}`, repository);
      if (!integrationHead || !gitIsAncestor(receipt.integration_sha, integrationHead, repository)) errors.push(`receipt integration commit is not on the canonical integration branch ${file}`);
      const changed = baseTree && candidateTree ? gitChangedPaths(receipt.base_sha, receipt.candidate_sha, repository) : null;
      if (!changed) errors.push(`receipt changed-path Git diff is unavailable ${file}`);
      else {
        errors.push(...exactList(receipt.changed_paths, changed, `receipt ${receipt.node_id} candidate changed paths`));
        for (const relative of changed) if (gitObjectAt(receipt.candidate_sha, relative, repository) !== gitObjectAt(receipt.integration_sha, relative, repository)) errors.push(`receipt candidate touched blob is not preserved at integration: ${relative}`);
      }
    }
    if (!context) errors.push(`receipt validation context missing exact predecessor, authorization, lease, gate, and review evidence ${file}`);
    else errors.push(...validateReceiptContext(receipt, node, context, file));
  } else errors.push(`receipt type invalid ${file}`);
  return { errors, receipt, digest: checked.digest };
}

export function validateLandedReceiptFile(file, node, packageRoot = PACKAGE_ROOT) {
  const checked = validatePayloadArtifact(file, packageRoot, "landed-receipt.schema.json", "landed receipt");
  const row = checked.row;
  const errors = [...checked.errors];
  if (!row || checked.errors.length) return { errors, receipt: row, digest: checked.digest };
  if (!node || node.id !== "J1" || node.initial_state !== "IN_FLIGHT" || node.review_profile !== "history" || row.node_id !== node.id) errors.push("landed receipt is not the exact grandfathered J1 authority node");
  const repository = gitRoot(packageRoot);
  if (!repository) errors.push("landed receipt Git repository unavailable");
  else {
    if (gitTree(row.landed_sha, repository) !== row.landed_tree) errors.push("landed receipt Git identity/landed tree mismatch");
    let parent = null;
    try { parent = gitOutput(["rev-parse", `${row.landed_sha}^`], repository); } catch { /* retained as mismatch */ }
    if (parent !== row.landed_parent_sha) errors.push("landed receipt parent commit mismatch");
    const canonical = validGitRef(`refs/heads/${row.canonical_integration_branch}`, repository);
    if (row.canonical_integration_branch !== "program/architecture-lock" || !canonical || !gitIsAncestor(row.landed_sha, canonical, repository)) errors.push("landed receipt commit is not contained by the canonical integration branch");
    const charter = gitPathAt(row.landed_sha, row.live_charter_path, repository);
    if (!charter || sha256(charter) !== row.live_charter_sha256) errors.push("landed receipt live charter digest mismatch");
    if (gitObjectAt(row.landed_sha, row.landing_evidence_path, repository) !== row.landing_evidence_tree) errors.push("landed receipt landing-evidence tree mismatch");
    const contextPacket = gitPathAt(row.landed_sha, row.context_packet_path, repository);
    if (!contextPacket || sha256(contextPacket) !== row.context_packet_sha256) errors.push("landed receipt context-packet evidence mismatch");
  }
  try {
    const liveLock = readToml(confinedFile(packageRoot, "provenance/live-source-lock.toml", "J1 landed live-source lock"));
    const charterLock = (liveLock.source || []).find((candidate) => candidate.ref === `live:${row.live_charter_path}`);
    const contextLock = (liveLock.source || []).find((candidate) => candidate.ref === `live:${row.context_packet_path}`);
    if (!charterLock || charterLock.path !== row.live_charter_path || charterLock.commit !== row.landed_sha || charterLock.sha256 !== row.live_charter_sha256) errors.push("landed receipt does not match the exact live J1 charter lock");
    if (!contextLock || contextLock.path !== row.context_packet_path || contextLock.sha256 !== row.context_packet_sha256) errors.push("landed receipt does not match the exact J1 context-packet lock");
  } catch (error) { errors.push(`landed receipt live-source lock invalid: ${error.message}`); }
  return { errors, receipt: row, digest: checked.digest };
}

function artifactReference(value, rows, label) {
  if (typeof value !== "string") return { row: null, error: `${label}: malformed artifact reference` };
  const split = value.lastIndexOf(":");
  const id = value.slice(0, split);
  const digest = value.slice(split + 1);
  const row = rows.get(id);
  if (split < 1 || !isDigest(digest) || !row || row.digest !== digest) return { row: null, error: `${label}: missing or digest-mismatched artifact ${value}` };
  return { row, error: null };
}

function exactCandidate(row, receipt, label) {
  return row.candidate_sha === receipt.candidate_sha && row.candidate_tree === receipt.candidate_tree
    ? [] : [`${label}: candidate identity differs from acceptance receipt`];
}

function exactIntegration(row, receipt, label) {
  return row.integration_sha === receipt.integration_sha && row.integration_tree === receipt.integration_tree
    ? [] : [`${label}: integration identity differs from acceptance receipt`];
}

function targetedGateCommands(authority, node) {
  const charter = fs.readFileSync(confinedFile(authority.packageRoot, node.charter, `${node.id} charter`), "utf8");
  const section = charter.match(/^## Targeted verification\n([\s\S]*?)(?=^## |$(?![\s\S]))/m)?.[1] || "";
  const targeted = [...section.matchAll(/^\d+\.\s+`([^`\n]+)`(?:\s|$)/gm)].map((match) => match[1]);
  const profile = catalogMap(authority.packageRoot, "gate-profiles.toml", "profile").get(evidenceProfiles(node).gate);
  return [...new Set([...(profile?.final || []), ...targeted])];
}

function validateRuntimeAttachment(authority, runtimeRoot, relative, expectedDigest, label) {
  try {
    const file = confinedFile(path.resolve(runtimeRoot), relative, label);
    if (sha256(fs.readFileSync(file)) !== expectedDigest) return [`${label}: digest mismatch ${relative}`];
    return [];
  } catch (error) { return [`${label}: ${error.message}`]; }
}

function authorityAncestryErrors(digest, nodeId, currentDigest, amendments, label) {
  if (digest === currentDigest) return [];
  const byBefore = new Map(amendments.map((row) => [row.before_authority_sha256, row]));
  const visited = new Set(); let cursor = digest;
  while (cursor !== currentDigest) {
    if (visited.has(cursor)) return [`${label}: authority amendment ancestry is cyclic`];
    visited.add(cursor);
    const amendment = byBefore.get(cursor);
    if (!amendment) return [`${label}: authority digest is not current or a verified chain ancestor`];
    if (amendment.impact_closure.includes(nodeId)) return [`${label}: stale after impacting amendment ${amendment.amendment_id}`];
    cursor = amendment.after_authority_sha256;
  }
  return [];
}

function loadExternal(authority, runtimeRoot, now, amendments = [], finalizations = new Map()) {
  const authorizations = new Map();
  const errors = [];
  const manifest = readToml(confinedFile(authority.packageRoot, "authority/state/external-authorizations.toml", "external authorization manifest"));
  const allowed = new Map();
  for (const row of manifest.authorization || []) {
    const key = `${row.node_id}:${row.authorization}`;
    if (allowed.has(key)) errors.push(`duplicate immutable external authorization allowlist row ${key}`);
    else allowed.set(key, row);
  }
  const { files, errors: fileErrors } = runtimeFiles(authority, runtimeRoot, "external");
  errors.push(...fileErrors);
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const authorityDigest = computeAuthorityDigest(authority.packageRoot);
  for (const file of files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "external-authorization.schema.json", "external authorization schema");
    errors.push(...checked.errors);
    const row = checked.row;
    if (!row || checked.errors.length) continue;
    const key = `${row.node_id}:${row.authorization}`;
    const node = byId.get(row.node_id);
    const manifestRow = allowed.get(key);
    if (path.basename(file) !== `${row.node_id}--${row.authorization}.toml`) errors.push(`external authorization filename mismatch ${file}`);
    if (!node || !node.external_requirements.includes(row.authorization)) errors.push(`external authorization does not name a required node authorization ${key}`);
    if (!manifestRow || manifestRow.granted_by !== row.granted_by || manifestRow.ratification_path !== row.ratification_path || manifestRow.ratification_receipt_sha256 !== row.ratification_receipt_sha256 || manifestRow.expires_at !== row.expires_at || manifestRow.grant_mode !== row.grant_mode || manifestRow.directive_scope !== row.directive_scope) errors.push(`external authorization is not in the immutable trusted authorization slot ${key}`);
    else {
      try {
        const ratification = confinedFile(authority.packageRoot, manifestRow.ratification_path, `external authorization ratification ${key}`);
        if (sha256(fs.readFileSync(ratification)) !== manifestRow.ratification_receipt_sha256) errors.push(`external authorization ratification receipt digest mismatch ${key}`);
      } catch (error) { errors.push(`external authorization ratification receipt invalid ${key}: ${error.message}`); }
    }
    errors.push(...candidateIdentity(row, authority.packageRoot, `external authorization ${key}`));
    const finalization = [...finalizations.values()].find((candidate) => candidate.node_id === row.node_id && candidate.candidate_sha === row.candidate_sha && candidate.candidate_tree === row.candidate_tree);
    if (!finalization) errors.push(`external authorization ${key} is not bound to an exact validated candidate-finalize receipt`);
    errors.push(...authorityAncestryErrors(row.authority_sha256, row.node_id, authorityDigest, amendments, `external authorization ${key}`));
    const expiry = row.expires_at === "never" ? Number.POSITIVE_INFINITY : Date.parse(row.expires_at);
    if (row.expires_at !== "never" && (!Number.isFinite(expiry) || expiry <= now)) errors.push(`external authorization expired or malformed ${key}`);
    if (!errors.some((error) => error.includes(key) || error.includes(file))) authorizations.set(key, { ...row, digest: checked.digest, file, active: expiry > now });
  }
  for (const key of allowed.keys()) if (![...authorizations.keys()].includes(key) && files.some((file) => path.basename(file).startsWith(`${key.split(":")[0]}--${key.split(":")[1]}`))) {
    // The precise validation error above is retained; do not synthesize a row.
  }
  return { authorizations, errors };
}

function loadGateEvidence(authority, runtimeRoot, authorizations = new Map()) {
  const evidence = new Map();
  const errors = [];
  const profiles = catalogMap(authority.packageRoot, "gate-profiles.toml", "profile");
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const { files, errors: fileErrors } = runtimeFiles(authority, runtimeRoot, "gates");
  errors.push(...fileErrors);
  for (const file of files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "gate-evidence.schema.json", "gate evidence");
    errors.push(...checked.errors);
    const row = checked.row;
    if (!row || checked.errors.length) continue;
    const node = byId.get(row.node_id);
    const rowErrors = [];
    if (path.basename(file) !== `${row.evidence_id}.toml`) rowErrors.push(`gate evidence filename mismatch ${file}`);
    if (!node || evidenceProfiles(node).gate !== row.gate_profile || !profiles.has(row.gate_profile)) rowErrors.push(`gate evidence profile/node mismatch ${row.evidence_id}`);
    else {
      const profile = profiles.get(row.gate_profile);
      const commands = row.scope === "candidate" ? targetedGateCommands(authority, node) : profile.final;
      rowErrors.push(...exactList(row.commands, commands, `gate evidence ${row.evidence_id} ${row.scope} commands`));
    }
    rowErrors.push(...candidateIdentity(row, authority.packageRoot, `gate evidence ${row.evidence_id}`));
    if (node) for (const requirement of node.external_requirements) {
      const authorization = authorizations.get(`${node.id}:${requirement}`);
      if (!authorization || authorization.candidate_sha !== row.candidate_sha || authorization.candidate_tree !== row.candidate_tree) rowErrors.push(`gate evidence ${row.evidence_id} precedes or differs from finalized-candidate authorization ${requirement}`);
    }
    const repository = gitRoot(authority.packageRoot);
    if (!repository || gitTree(row.integration_sha, repository) !== row.integration_tree || !gitIsAncestor(row.candidate_sha, row.integration_sha, repository)) rowErrors.push(`gate evidence ${row.evidence_id}: integration Git identity/ancestry mismatch`);
    const started = Date.parse(row.started_at); const completed = Date.parse(row.completed_at);
    if (!Number.isFinite(started) || !Number.isFinite(completed) || completed < started) rowErrors.push(`gate evidence time order invalid ${row.evidence_id}`);
    rowErrors.push(...validateRuntimeAttachment(authority, runtimeRoot, row.result_path, row.result_sha256, `gate result ${row.evidence_id}`));
    try {
      const resultFile = confinedFile(path.resolve(runtimeRoot), row.result_path, `gate execution result ${row.evidence_id}`);
      const result = JSON.parse(fs.readFileSync(resultFile, "utf8"));
      rowErrors.push(...validateSchemaObject(result, loadSchema(authority.packageRoot, "gate-execution-result.schema.json"), `gate execution result ${row.evidence_id}`));
      const commandResultsValid = Array.isArray(result.results) && result.results.length === row.commands.length && result.results.every((entry, index) => entry.command === row.commands[index] && Array.isArray(entry.argv) && entry.argv.length > 0 && entry.status === 0 && entry.signal === "" && entry.timed_out === false && sha256(entry.stdout || "") === entry.stdout_sha256 && sha256(entry.stderr || "") === entry.stderr_sha256);
      if (result.schema !== 1 || result.type !== "gate-execution-result" || result.execution_custody !== "programctl-gate-run/v1" || result.node_id !== row.node_id || result.scope !== row.scope || result.candidate_sha !== row.candidate_sha || result.candidate_tree !== row.candidate_tree || result.integration_sha !== row.integration_sha || result.integration_tree !== row.integration_tree || result.executed_by !== row.executed_by || result.started_at !== row.started_at || result.completed_at !== row.completed_at || JSON.stringify(result.commands) !== JSON.stringify(row.commands) || !commandResultsValid || result.terminal_summary !== "PASS" || result.unexpected_skips !== 0) rowErrors.push(`gate evidence ${row.evidence_id} does not reconcile complete runner-owned command execution`);
      rowErrors.push(...exactList(row.executed_work, row.commands, `gate evidence ${row.evidence_id} executed work`));
      if (row.gate_profile === "docs-domain") {
        const docsExecution = result.results.find((entry) => entry.command === "node docs/arch/refactor/rev11/tools/run-docs-gate.mjs");
        if (!docsExecution) throw new Error("canonical docs command was not executed");
        const docsResult = JSON.parse(docsExecution.stdout);
      const scripts = fs.readdirSync(path.join(authority.packageRoot, "tools"), { withFileTypes: true })
        .filter((entry) => entry.isFile() && entry.name.endsWith(".mjs"))
        .map((entry) => path.posix.join("docs/arch/refactor/rev11/tools", entry.name)).sort();
      const tests = scripts.filter((name) => name.endsWith(".test.mjs"));
      const validators = [
        "build-program-dag.mjs --check", "build-source-clauses.mjs --check", "build-collapse-map.mjs --check",
        "build-conflict-ownership.mjs --check", "build-operational-charters.mjs --check", "validate-program-dag.mjs --strict",
        "validate-charters.mjs --strict", "validate-orchestration-state.mjs --strict", "validate-negative-controls.mjs", "self-test.mjs",
      ].map((entry) => `node docs/arch/refactor/rev11/tools/${entry}`);
      const plan = { schema: 1, type: "unified-docs-gate-plan", package_root: "docs/arch/refactor/rev11", syntax_inputs: scripts, test_inputs: tests, validators };
      const discoveryDigest = sha256(`${JSON.stringify(plan)}\n`);
        if (docsResult.type !== "unified-docs-gate-plan" || docsResult.discovery_sha256 !== discoveryDigest || docsResult.terminal_summary !== "PASS" || docsResult.unexpected_skips !== 0 || JSON.stringify(docsResult.syntax_inputs) !== JSON.stringify(scripts) || JSON.stringify(docsResult.test_inputs) !== JSON.stringify(tests) || JSON.stringify(docsResult.validators) !== JSON.stringify(validators) || !Array.isArray(docsResult.results) || docsResult.results.length === 0 || docsResult.results.some((entry) => entry.status !== 0 || entry.signal)) rowErrors.push(`gate evidence ${row.evidence_id} does not reconcile a complete canonical docs gate result`);
      }
    } catch (error) { rowErrors.push(`gate evidence ${row.evidence_id} runner-owned result is not valid structured JSON: ${error.message}`); }
    if (evidence.has(row.evidence_id)) rowErrors.push(`duplicate gate evidence ${row.evidence_id}`);
    errors.push(...rowErrors);
    if (!rowErrors.length) evidence.set(row.evidence_id, { ...row, digest: checked.digest, file });
  }
  return { evidence, errors };
}

export function validateReviewFindingDispositions(authority, row, { now = Date.now() } = {}) {
  const errors = [];
  const trusted = readToml(confinedFile(authority.packageRoot, "authority/state/trusted-ratifications.toml", "trusted ratification ledger")).slot || [];
  for (const finding of row.findings || []) {
    if (["P0", "P1"].includes(finding.severity)) errors.push(`review evidence ${row.evidence_id} contains blocking ${finding.severity} finding ${finding.fingerprint}`);
    if (finding.severity !== "P2") continue;
    if (finding.status !== "AUTHORIZED_DEFERRED") errors.push(`review evidence ${row.evidence_id} contains undispositioned P2 finding ${finding.fingerprint}`);
    const digest = /^finding-disposition:([0-9a-f]{64})$/.exec(finding.authorization_binding || "")?.[1];
    const slot = trusted.find((candidate) => candidate.purpose === "finding-disposition" && candidate.ratified_by === finding.owner && candidate.receipt_sha256 === digest);
    if (!slot) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} lacks a trusted disposition authorization`);
    else try {
      const receipt = confinedFile(authority.packageRoot, slot.receipt_path, `finding disposition ${finding.fingerprint}`);
      const bytes = fs.readFileSync(receipt);
      if (sha256(bytes) !== digest) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} disposition digest mismatch`);
      else {
        let disposition;
        try { disposition = JSON.parse(bytes.toString("utf8")); }
        catch { errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} disposition must be structured JSON`); }
        if (disposition) errors.push(...validateSchemaObject(disposition, loadSchema(authority.packageRoot, "finding-disposition.schema.json"), `finding disposition ${finding.fingerprint}`));
        const expected = disposition && disposition.node_id === row.node_id && disposition.candidate_sha === row.candidate_sha
          && disposition.candidate_tree === row.candidate_tree && disposition.review_profile === row.review_profile
          && disposition.lens === row.lens && disposition.severity === finding.severity && disposition.fingerprint === finding.fingerprint
          && disposition.owner === finding.owner && disposition.next_cycle_obligation === finding.next_cycle_obligation
          && disposition.next_cycle_receipt === finding.next_cycle_receipt
          && JSON.stringify(disposition.class_wide_sweep) === JSON.stringify(finding.class_wide_sweep);
        if (!expected) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} disposition artifact does not exactly reconcile candidate/profile/lens/severity and the one-time finding`);
        const expires = Date.parse(disposition?.expires_at);
        const reviewCompleted = Date.parse(row.completed_at);
        if (!Number.isFinite(expires) || !Number.isFinite(reviewCompleted) || expires <= reviewCompleted) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} disposition expiry is not bounded after review`);
        const nextMatch = /^((?:NEXT-)[A-Z0-9-]+):([0-9a-f]{64})$/.exec(finding.next_cycle_receipt || "");
        const nextSlot = nextMatch && trusted.find((candidate) => candidate.purpose === "next-cycle-closure" && candidate.ratified_by === finding.owner && candidate.receipt_sha256 === nextMatch[2]);
        if (!nextSlot) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} lacks an authenticated next-cycle closure receipt`);
        else try {
          const closureFile = confinedFile(authority.packageRoot, nextSlot.receipt_path, `next-cycle closure ${finding.fingerprint}`);
          const closureBytes = fs.readFileSync(closureFile);
          if (sha256(closureBytes) !== nextMatch[2]) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} next-cycle closure digest mismatch`);
          else {
            let closure;
            try { closure = JSON.parse(closureBytes.toString("utf8")); }
            catch { errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} next-cycle closure must be structured JSON`); }
            if (closure) errors.push(...validateSchemaObject(closure, loadSchema(authority.packageRoot, "next-cycle-closure.schema.json"), `next-cycle closure ${finding.fingerprint}`));
            const closedAt = Date.parse(closure?.closed_at);
            const exactClosure = closure && closure.receipt_id === nextMatch[1] && closure.node_id === row.node_id
              && closure.candidate_sha === row.candidate_sha && closure.candidate_tree === row.candidate_tree
              && closure.review_profile === row.review_profile && closure.lens === row.lens && closure.severity === finding.severity
              && closure.fingerprint === finding.fingerprint && closure.owner === finding.owner
              && closure.obligation === finding.next_cycle_obligation;
            if (!exactClosure) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} next-cycle closure does not exactly reconcile the obligation`);
            if (!Number.isFinite(closedAt) || closedAt < reviewCompleted || closedAt > now || closedAt >= expires) errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} next-cycle closure time is invalid or outside disposition expiry`);
          }
        } catch (error) { errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} next-cycle closure receipt invalid: ${error.message}`); }
      }
    } catch (error) { errors.push(`review evidence ${row.evidence_id} finding ${finding.fingerprint} disposition receipt invalid: ${error.message}`); }
  }
  return errors;
}

function reviewCapability(authority, row) {
  const errors = [];
  const digest = /^review-capability:([0-9a-f]{64})$/.exec(row.custody_binding || "")?.[1];
  const trusted = readToml(confinedFile(authority.packageRoot, "authority/state/trusted-ratifications.toml", "trusted review capability ledger")).slot || [];
  const slot = trusted.find((candidate) => candidate.purpose === "review-capability" && candidate.ratified_by === row.reviewer && candidate.receipt_sha256 === digest);
  if (!slot) return { errors: [`review evidence ${row.evidence_id} lacks a trusted immutable reviewer capability`], capability: null, script: null };
  let capability; let script = null;
  try {
    const receipt = confinedFile(authority.packageRoot, slot.receipt_path, `review capability ${row.reviewer}/${row.lens}`);
    const bytes = fs.readFileSync(receipt);
    if (sha256(bytes) !== digest) errors.push(`review capability ${row.reviewer}/${row.lens} receipt digest mismatch`);
    capability = JSON.parse(bytes.toString("utf8"));
    errors.push(...validateSchemaObject(capability, loadSchema(authority.packageRoot, "reviewer-capability.schema.json"), `review capability ${row.reviewer}/${row.lens}`));
    const keys = Object.keys(capability).sort();
    const expectedKeys = ["schema", "type", "reviewer", "lens", "model", "reasoning_effort", "runner", "executable_path", "executable_sha256"].sort();
    if (JSON.stringify(keys) !== JSON.stringify(expectedKeys) || capability.schema !== 1 || capability.type !== "reviewer-capability" || capability.reviewer !== row.reviewer || capability.lens !== row.lens || capability.model !== row.model || capability.reasoning_effort !== row.reasoning_effort || capability.runner !== "node" || !capability.executable_path?.startsWith("sources/reviewer-capabilities/") || capability.executable_sha256 !== row.reviewer_executable_sha256) errors.push(`review capability ${row.reviewer}/${row.lens} does not exactly bind evidence identity and executable`);
    script = confinedFile(authority.packageRoot, capability.executable_path, `reviewer executable ${row.reviewer}/${row.lens}`);
    if (sha256(fs.readFileSync(script)) !== capability.executable_sha256) errors.push(`reviewer executable ${row.reviewer}/${row.lens} digest mismatch`);
  } catch (error) { errors.push(`review capability ${row.reviewer}/${row.lens} invalid: ${error.message}`); }
  return { errors, capability, script };
}

export function validateReviewCapability(authority, row) {
  return reviewCapability(authority, row);
}

function loadReviewEvidence(authority, runtimeRoot, authorizations = new Map(), now = Date.now()) {
  const evidence = new Map();
  const errors = [];
  const profiles = catalogMap(authority.packageRoot, "review-profiles.toml", "profile");
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const { files, errors: fileErrors } = runtimeFiles(authority, runtimeRoot, "reviews");
  const dispositionUses = new Map();
  errors.push(...fileErrors);
  for (const file of files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "review-evidence.schema.json", "review evidence");
    errors.push(...checked.errors);
    const row = checked.row;
    if (!row || checked.errors.length) continue;
    const node = byId.get(row.node_id); const profile = profiles.get(row.review_profile);
    const rowErrors = [];
    if (path.basename(file) !== `${row.evidence_id}.toml`) rowErrors.push(`review evidence filename mismatch ${file}`);
    if (!node || evidenceProfiles(node).review !== row.review_profile || !profile || !profile.lenses.includes(row.lens)) rowErrors.push(`review evidence profile/lens mismatch ${row.evidence_id}`);
    else if (!row.model || row.reasoning_effort !== assessNodeEffort(node).review) rowErrors.push(`review evidence model/effort mismatch ${row.evidence_id}`);
    rowErrors.push(...validateReviewFindingDispositions(authority, row, { now }));
    rowErrors.push(...reviewCapability(authority, row).errors);
    rowErrors.push(...candidateIdentity(row, authority.packageRoot, `review evidence ${row.evidence_id}`));
    if (node) for (const requirement of node.external_requirements) {
      const authorization = authorizations.get(`${node.id}:${requirement}`);
      if (!authorization || authorization.candidate_sha !== row.candidate_sha || authorization.candidate_tree !== row.candidate_tree) rowErrors.push(`review evidence ${row.evidence_id} precedes or differs from finalized-candidate authorization ${requirement}`);
    }
    rowErrors.push(...validateRuntimeAttachment(authority, runtimeRoot, row.report_path, row.report_sha256, `review report ${row.evidence_id}`));
    try {
      const reportFile = confinedFile(path.resolve(runtimeRoot), row.report_path, `structured review report ${row.evidence_id}`);
      const report = JSON.parse(fs.readFileSync(reportFile, "utf8"));
      rowErrors.push(...validateSchemaObject(report, loadSchema(authority.packageRoot, "review-report.schema.json"), `review report ${row.evidence_id}`));
      const keys = Object.keys(report).sort();
      const expectedKeys = ["candidate_sha", "candidate_tree", "findings", "lens", "node_id", "reviewer", "schema", "verdict"].sort();
      if (JSON.stringify(keys) !== JSON.stringify(expectedKeys) || report.schema !== 1 || report.node_id !== row.node_id || report.candidate_sha !== row.candidate_sha || report.candidate_tree !== row.candidate_tree || report.reviewer !== row.reviewer || report.lens !== row.lens || report.verdict !== row.verdict || JSON.stringify(report.findings) !== JSON.stringify(row.findings)) rowErrors.push(`review report ${row.evidence_id} does not exactly reconcile structured evidence`);
    } catch (error) { rowErrors.push(`review report ${row.evidence_id} is not canonical structured JSON: ${error.message}`); }
    for (const finding of row.findings || []) if (finding.severity === "P2") {
      const prior = dispositionUses.get(finding.authorization_binding);
      const use = `${row.node_id}:${row.candidate_sha}:${row.review_profile}:${row.lens}:${finding.severity}:${finding.fingerprint}`;
      if (prior) rowErrors.push(`review evidence ${row.evidence_id} reuses one-time P2 disposition ${finding.authorization_binding}; first used by ${prior}`);
      else dispositionUses.set(finding.authorization_binding, use);
    }
    const started = Date.parse(row.started_at); const completed = Date.parse(row.completed_at);
    if (!Number.isFinite(started) || !Number.isFinite(completed) || completed < started) rowErrors.push(`review evidence time order invalid ${row.evidence_id}`);
    if (evidence.has(row.evidence_id)) rowErrors.push(`duplicate review evidence ${row.evidence_id}`);
    errors.push(...rowErrors);
    if (!rowErrors.length) evidence.set(row.evidence_id, { ...row, digest: checked.digest, file });
  }
  return { evidence, errors };
}

function loadDispatches(authority, runtimeRoot, leases) {
  const dispatches = new Map(); const errors = [];
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const inventory = runtimeFiles(authority, runtimeRoot, "dispatches"); errors.push(...inventory.errors);
  for (const file of inventory.files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "dispatch.schema.json", "dispatch receipt");
    const row = checked.row; const rowErrors = [...checked.errors];
    if (!row || checked.errors.length) { errors.push(...rowErrors); continue; }
    const node = byId.get(row.node_id);
    const lease = artifactReference(row.lease_receipt, leases, `dispatch ${row.dispatch_id} lease`);
    if (path.basename(file) !== `${row.dispatch_id}.toml`) rowErrors.push(`dispatch receipt filename mismatch ${file}`);
    if (!node) rowErrors.push(`dispatch receipt unknown node ${row.node_id}`);
    if (lease.error) rowErrors.push(lease.error);
    else {
      const expected = lease.row;
      if (expected.node_id !== row.node_id || expected.holder !== row.dispatched_by || expected.base_sha !== row.base_sha || expected.base_tree !== row.base_tree || expected.candidate_sha !== row.candidate_start_sha || expected.candidate_tree !== row.candidate_start_tree || expected.candidate_ref !== row.candidate_ref || expected.candidate_worktree !== row.candidate_worktree || expected.authority_sha256 !== row.authority_sha256) rowErrors.push(`dispatch ${row.dispatch_id} identity differs from admission lease`);
      rowErrors.push(...exactList(row.conflict_domains, expected.conflict_domains, `dispatch ${row.dispatch_id} conflict domains`));
      rowErrors.push(...exactList(row.scope_path_roots, expected.scope_path_roots, `dispatch ${row.dispatch_id} path scope`));
      rowErrors.push(...exactList(row.scope_symbols, expected.scope_symbols, `dispatch ${row.dispatch_id} symbol scope`));
      const dispatched = Date.parse(row.dispatched_at);
      if (!Number.isFinite(dispatched) || dispatched < Date.parse(expected.acquired_at) || dispatched > Date.parse(expected.expires_at)) rowErrors.push(`dispatch ${row.dispatch_id} falls outside lease epoch`);
    }
    if (node) {
      const charter = confinedFile(authority.packageRoot, node.charter, `${node.id} dispatch charter`);
      if (sha256(fs.readFileSync(charter)) !== row.charter_sha256) rowErrors.push(`dispatch ${row.dispatch_id} charter digest mismatch`);
    }
    rowErrors.push(...validateRuntimeAttachment(authority, runtimeRoot, row.packet_path, row.packet_sha256, `dispatch packet ${row.dispatch_id}`));
    if (dispatches.has(row.dispatch_id)) rowErrors.push(`duplicate dispatch receipt ${row.dispatch_id}`);
    errors.push(...rowErrors);
    if (!rowErrors.length) dispatches.set(row.dispatch_id, { ...row, digest: checked.digest, file });
  }
  return { dispatches, errors };
}

function loadFinalizations(authority, runtimeRoot, leases, dispatches) {
  const finalizations = new Map(); const errors = [];
  const inventory = runtimeFiles(authority, runtimeRoot, "finalizations"); errors.push(...inventory.errors);
  const repository = gitRoot(authority.packageRoot);
  for (const file of inventory.files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "candidate-finalization.schema.json", "candidate finalization");
    const row = checked.row; const rowErrors = [...checked.errors];
    if (!row || checked.errors.length) { errors.push(...rowErrors); continue; }
    const lease = artifactReference(row.lease_receipt, leases, `finalization ${row.finalization_id} lease`);
    const dispatch = artifactReference(row.dispatch_receipt, dispatches, `finalization ${row.finalization_id} dispatch`);
    if (path.basename(file) !== `${row.finalization_id}.toml`) rowErrors.push(`candidate finalization filename mismatch ${file}`);
    if (lease.error) rowErrors.push(lease.error);
    if (dispatch.error) rowErrors.push(dispatch.error);
    if (!lease.error) {
      const expected = lease.row;
      if (expected.node_id !== row.node_id || expected.holder !== row.finalized_by || expected.base_sha !== row.base_sha || expected.base_tree !== row.base_tree || expected.candidate_sha !== row.candidate_start_sha || expected.candidate_tree !== row.candidate_start_tree || expected.candidate_ref !== row.candidate_ref || expected.candidate_worktree !== row.candidate_worktree || expected.authority_sha256 !== row.authority_sha256) rowErrors.push(`finalization ${row.finalization_id} identity differs from admission lease`);
      const finalized = Date.parse(row.finalized_at);
      if (!Number.isFinite(finalized) || finalized < Date.parse(expected.acquired_at) || finalized > Date.parse(expected.expires_at)) rowErrors.push(`finalization ${row.finalization_id} falls outside lease epoch`);
      for (const changedPath of row.changed_paths) if (!expected.scope_path_roots.some((root) => changedPath === root || changedPath.startsWith(`${root}/`))) rowErrors.push(`finalization ${row.finalization_id} changed path is outside admission scope: ${changedPath}`);
    }
    if (!dispatch.error && dispatch.row.lease_receipt !== row.lease_receipt) rowErrors.push(`finalization ${row.finalization_id} dispatch differs from lease`);
    const candidateTree = repository && gitTree(row.candidate_sha, repository);
    if (!candidateTree || candidateTree !== row.candidate_tree || !gitIsAncestor(row.candidate_start_sha, row.candidate_sha, repository)) rowErrors.push(`finalization ${row.finalization_id} final candidate identity/ancestry mismatch`);
    const refSha = repository && validGitRef(row.candidate_ref, repository);
    if (refSha !== row.candidate_sha) rowErrors.push(`finalization ${row.finalization_id} candidate ref moved after freeze`);
    try {
      const worktree = candidateForRef(authority, row.candidate_ref);
      if (worktree.sha !== row.candidate_sha || worktree.tree !== row.candidate_tree || worktree.worktree !== row.candidate_worktree) rowErrors.push(`finalization ${row.finalization_id} worktree moved after freeze`);
    } catch (error) { rowErrors.push(`finalization ${row.finalization_id} frozen worktree invalid: ${error.message}`); }
    const changed = repository && gitChangedPaths(row.base_sha, row.candidate_sha, repository);
    if (!changed) rowErrors.push(`finalization ${row.finalization_id} base delta is unavailable`);
    else rowErrors.push(...exactList(row.changed_paths, changed, `finalization ${row.finalization_id} base delta`));
    if (finalizations.has(row.finalization_id)) rowErrors.push(`duplicate candidate finalization ${row.finalization_id}`);
    errors.push(...rowErrors);
    if (!rowErrors.length) finalizations.set(row.finalization_id, { ...row, digest: checked.digest, file });
  }
  return { finalizations, errors };
}

function validGitRef(ref, repository) {
  try {
    childProcess.execFileSync("git", ["check-ref-format", ref], { cwd: repository, stdio: "ignore", timeout: CHILD_PROCESS_TIMEOUT_MS, killSignal: "SIGKILL" });
    return gitOutput(["rev-parse", "--verify", `${ref}^{commit}`], repository);
  } catch { return null; }
}

function loadLeases(authority, runtimeRoot, now, amendments = []) {
  const all = new Map(); const releases = new Map(); const errors = [];
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  const resources = catalogMap(authority.packageRoot, "resource-profiles.toml", "profile");
  const domains = catalogMap(authority.packageRoot, "conflict-domains.toml", "domain");
  const reviewProfiles = catalogMap(authority.packageRoot, "review-profiles.toml", "profile");
  const authorityDigest = computeAuthorityDigest(authority.packageRoot);
  const repository = gitRoot(authority.packageRoot);
  const leaseFiles = runtimeFiles(authority, runtimeRoot, "leases");
  const releaseFiles = runtimeFiles(authority, runtimeRoot, "lease-releases");
  errors.push(...leaseFiles.errors, ...releaseFiles.errors);
  for (const file of leaseFiles.files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "lease.schema.json", "lease");
    errors.push(...checked.errors);
    const row = checked.row;
    if (!row || checked.errors.length) continue;
    const node = byId.get(row.node_id); const rowErrors = [];
    if (path.basename(file) !== `${row.lease_id}.toml`) rowErrors.push(`lease filename mismatch ${file}`);
    if (!node) rowErrors.push(`lease unknown node ${row.node_id}`);
    else {
      rowErrors.push(...exactList(row.conflict_domains, node.conflict_domains, `lease ${row.lease_id} conflict domains`));
      const expectedRoots = [...new Set(node.conflict_domains.flatMap((id) => domains.get(id)?.path_roots || []))].sort();
      const expectedSymbols = [...new Set(node.conflict_domains.flatMap((id) => domains.get(id)?.symbols || []))].sort();
      rowErrors.push(...exactList(row.scope_path_roots, expectedRoots, `lease ${row.lease_id} path scope`));
      rowErrors.push(...exactList(row.scope_symbols, expectedSymbols, `lease ${row.lease_id} symbol scope`));
      if (row.resource_class !== node.resource_class || !resources.has(row.resource_class)) rowErrors.push(`lease resource class mismatch ${row.lease_id}`);
      const reviewProfile = reviewProfiles.get(evidenceProfiles(node).review);
      const assignments = row.reviewer_assignments.map((value) => [value.slice(0, value.indexOf("=")), value.slice(value.indexOf("=") + 1)]);
      const reportPaths = row.review_report_paths.map((value) => [value.slice(0, value.indexOf("=")), value.slice(value.indexOf("=") + 1)]);
      rowErrors.push(...exactList(assignments.map(([lens]) => lens), reviewProfile?.lenses || [], `lease ${row.lease_id} reviewer lenses`));
      rowErrors.push(...exactList(reportPaths.map(([lens]) => lens), reviewProfile?.lenses || [], `lease ${row.lease_id} report lenses`));
      if (new Set(assignments.map(([, reviewer]) => reviewer)).size !== assignments.length || assignments.some(([, reviewer]) => reviewer === row.gate_runner)) rowErrors.push(`lease ${row.lease_id} reviewer identities are not independent`);
      if (!row.renewed_from) {
        if (row.gate_result_path !== `gate-results/${row.lease_id}.txt`) rowErrors.push(`lease ${row.lease_id} gate result destination mismatch`);
        if (row.integration_gate_result_path !== `gate-results/${row.lease_id}--integration.txt`) rowErrors.push(`lease ${row.lease_id} integration gate result destination mismatch`);
        for (const [lens, destination] of reportPaths) if (destination !== `review-reports/${row.lease_id}--${lens}.md`) rowErrors.push(`lease ${row.lease_id} review report destination mismatch ${lens}`);
      }
    }
    rowErrors.push(...authorityAncestryErrors(row.authority_sha256, row.node_id, authorityDigest, amendments, `lease ${row.lease_id}`));
    rowErrors.push(...candidateIdentity(row, authority.packageRoot, `lease ${row.lease_id}`));
    const refSha = repository && validGitRef(row.candidate_ref, repository);
    if (!refSha || !gitIsAncestor(row.candidate_sha, refSha, repository)) rowErrors.push(`lease candidate ref rewound or diverged from admission start ${row.lease_id}`);
    if (!gitTree(row.base_sha, repository) || gitTree(row.base_sha, repository) !== row.base_tree || !gitIsAncestor(row.base_sha, row.candidate_sha, repository)) rowErrors.push(`lease base identity/ancestry mismatch ${row.lease_id}`);
    try {
      const worktree = candidateForRef(authority, row.candidate_ref);
      if (worktree.worktree !== row.candidate_worktree) rowErrors.push(`lease candidate worktree mismatch ${row.lease_id}`);
    } catch (error) { rowErrors.push(`lease candidate worktree invalid ${row.lease_id}: ${error.message}`); }
    const acquired = Date.parse(row.acquired_at); const expires = Date.parse(row.expires_at);
    if (!Number.isFinite(acquired) || !Number.isFinite(expires) || acquired > now || expires <= acquired || expires - acquired > 24 * 60 * 60 * 1000) rowErrors.push(`lease interval invalid ${row.lease_id}`);
    if (all.has(row.lease_id)) rowErrors.push(`duplicate lease id ${row.lease_id}`);
    if (row.renewed_from) {
      const prior = artifactReference(row.renewed_from, all, `lease renewal ${row.lease_id}`);
      if (prior.error) rowErrors.push(prior.error);
      else if (prior.row.node_id !== row.node_id || prior.row.holder !== row.holder || prior.row.base_sha !== row.base_sha || prior.row.base_tree !== row.base_tree || prior.row.candidate_ref !== row.candidate_ref || prior.row.candidate_sha !== row.candidate_sha || prior.row.candidate_tree !== row.candidate_tree || prior.row.candidate_worktree !== row.candidate_worktree || prior.row.authority_sha256 !== row.authority_sha256 || JSON.stringify(prior.row.scope_path_roots) !== JSON.stringify(row.scope_path_roots) || JSON.stringify(prior.row.scope_symbols) !== JSON.stringify(row.scope_symbols) || prior.row.gate_runner !== row.gate_runner || prior.row.gate_result_path !== row.gate_result_path || prior.row.integration_gate_result_path !== row.integration_gate_result_path || JSON.stringify(prior.row.reviewer_assignments) !== JSON.stringify(row.reviewer_assignments) || JSON.stringify(prior.row.review_report_paths) !== JSON.stringify(row.review_report_paths) || acquired < Date.parse(prior.row.acquired_at)) rowErrors.push(`lease renewal identity mismatch ${row.lease_id}`);
    }
    errors.push(...rowErrors);
    if (!rowErrors.length) all.set(row.lease_id, { ...row, digest: checked.digest, file });
  }
  for (const file of releaseFiles.files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "lease-release.schema.json", "lease release");
    errors.push(...checked.errors);
    const row = checked.row; const rowErrors = [];
    if (!row || checked.errors.length) continue;
    if (path.basename(file) !== `${row.release_id}.toml`) rowErrors.push(`lease release filename mismatch ${file}`);
    const leaseRef = artifactReference(row.lease_receipt, all, `lease release ${row.release_id}`);
    if (leaseRef.error) rowErrors.push(leaseRef.error);
    else if (leaseRef.row.holder !== row.holder || Date.parse(row.released_at) < Date.parse(leaseRef.row.acquired_at) || Date.parse(row.released_at) > now) rowErrors.push(`lease release identity/time mismatch ${row.release_id}`);
    if (releases.has(row.lease_receipt)) rowErrors.push(`duplicate lease release ${row.lease_receipt}`);
    errors.push(...rowErrors);
    if (!rowErrors.length) releases.set(row.lease_receipt, { ...row, digest: checked.digest, file });
  }
  const renewed = new Set([...all.values()].map((row) => row.renewed_from).filter(Boolean));
  const active = [...all.values()].filter((row) => Date.parse(row.expires_at) > now && !releases.has(reference(row.lease_id, row)) && !renewed.has(reference(row.lease_id, row)));
  for (let i = 0; i < active.length; i += 1) for (let j = i + 1; j < active.length; j += 1) {
    const overlap = conflictReasons(active[i].conflict_domains, active[j].conflict_domains, domains);
    if (active[i].node_id === active[j].node_id) errors.push(`same-node lease conflict ${active[i].node_id}: ${active[i].lease_id}/${active[j].lease_id}`);
    else if (overlap.length) errors.push(`lease conflict ${active[i].node_id}/${active[j].node_id}: ${overlap.join(",")}`);
  }
  for (const [resourceId, profile] of resources) {
    const used = active.filter((row) => row.resource_class === resourceId).length;
    if (used > profile.capacity_hint) errors.push(`resource capacity exceeded ${resourceId}: ${used}/${profile.capacity_hint}`);
  }
  return { all, active, releases, errors };
}

function loadAmendmentRows(authority) {
  const rows = []; const errors = [];
  const directory = path.join(authority.packageRoot, "authority/state/amendments");
  if (!fs.existsSync(directory)) return { rows, errors };
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isSymbolicLink() || !entry.isFile() || !entry.name.endsWith(".toml")) { errors.push(`amendment directory contains unsupported entry ${entry.name}`); continue; }
    const checked = validatePayloadArtifact(file, authority.packageRoot, "amendment.schema.json", "amendment");
    errors.push(...checked.errors);
    if (!checked.errors.length) rows.push({ ...checked.row, digest: checked.digest, file });
  }
  return { rows, errors };
}

function loadActivationTransitions(authority, runtimeRoot, now) {
  const transitions = new Map(); const errors = [];
  const inventory = runtimeFiles(authority, runtimeRoot, "activations");
  errors.push(...inventory.errors);
  for (const file of inventory.files) {
    const checked = validatePayloadArtifact(file, authority.packageRoot, "activation-transition.schema.json", "activation transition");
    errors.push(...checked.errors);
    const row = checked.row; const rowErrors = [];
    if (!row || checked.errors.length) continue;
    if (path.basename(file) !== `${row.transition_id}.toml`) rowErrors.push(`activation transition filename mismatch ${file}`);
    if (row.authority_sha256 !== computeAuthorityDigest(authority.packageRoot)) rowErrors.push(`activation transition authority digest mismatch ${row.transition_id}`);
    if (Date.parse(row.activated_at) > now) rowErrors.push(`activation transition is future-dated ${row.transition_id}`);
    if (transitions.has(row.transition_id)) rowErrors.push(`duplicate activation transition ${row.transition_id}`);
    errors.push(...rowErrors);
    if (!rowErrors.length) transitions.set(row.transition_id, { ...row, digest: checked.digest, file });
  }
  return { transitions, errors };
}

function receiptAuthorityErrors(receipt, node, context, file) {
  if (context.invalidatedReceipts.has(reference(node.id, receipt))) return [`receipt ${node.id} is explicitly invalidated by the amendment chain`];
  return authorityAncestryErrors(receipt.authority_sha256, node.id, context.authorityDigest, context.amendments, `receipt ${node.id} ${file}`);
}

function validateReceiptContext(receipt, node, context, file) {
  const errors = [];
  errors.push(...receiptAuthorityErrors(receipt, node, context, file));
  const predecessors = node.predecessors.map((id) => context.predecessorReceipts.get(id)).filter(Boolean);
  if (predecessors.length !== node.predecessors.length) errors.push(`receipt missing validated predecessor context ${file}`);
  errors.push(...exactList(receipt.predecessor_receipts, node.predecessors.map((id) => {
    const row = context.predecessorReceipts.get(id); return row ? reference(row.receipt_id || id, row) : `${id}:MISSING`;
  }), `receipt ${node.id} predecessor receipts`));
  const repository = gitRoot(context.authority.packageRoot);
  for (const pred of predecessors) if (repository && !gitIsAncestor(pred.integration_sha || pred.landed_sha, receipt.base_sha, repository)) errors.push(`receipt predecessor ${pred.node_id} integration/landing is not an ancestor of base ${file}`);
  const conditionalIds = new Set(node.conditional_predecessors.map((value) => value.split(":")[0]));
  for (const id of receipt.opened_conditionals) if (!conditionalIds.has(id)) errors.push(`receipt opened undefined conditional ${id} ${file}`);
  const conditionalRows = receipt.opened_conditionals.map((id) => context.predecessorReceipts.get(id)).filter(Boolean);
  if (conditionalRows.length !== receipt.opened_conditionals.length) errors.push(`receipt missing validated conditional predecessor context ${file}`);
  errors.push(...exactList(receipt.conditional_predecessor_receipts, receipt.opened_conditionals.map((id) => {
    const row = context.predecessorReceipts.get(id); return row ? reference(row.receipt_id || id, row) : `${id}:MISSING`;
  }), `receipt ${node.id} conditional predecessor receipts`));
  for (const pred of conditionalRows) if (repository && !gitIsAncestor(pred.integration_sha || pred.landed_sha, receipt.base_sha, repository)) errors.push(`receipt conditional predecessor ${pred.node_id} is not an ancestor of base ${file}`);
  const expectedAuth = [];
  for (const requirement of node.external_requirements) {
    const auth = context.authorizations.get(`${node.id}:${requirement}`);
    if (!auth) errors.push(`receipt missing validated external authorization ${requirement} ${file}`);
    else expectedAuth.push(reference(requirement, auth));
  }
  errors.push(...exactList(receipt.external_authorizations, expectedAuth, `receipt ${node.id} external authorizations`));
  const activation = context.activation;
  let expectedActivation = "";
  if (node.id === "ORC0") {
    const j1 = context.landedReceipts.get("J1");
    if (!j1 || activation.j1_state !== "LANDED_GRANDFATHERED" || activation.j1_receipt !== reference(j1.receipt_id, j1)) errors.push(`ORC0 receipt bypasses exact grandfathered J1 landing binding ${file}`);
  } else if (node.activation_gate === "ORC0") {
    const orc = context.receipts.get("ORC0");
    const trustedOrc = context.trustedLocalOrc;
    expectedActivation = orc ? reference("ORC0", orc) : trustedOrc?.reference || "ORC0:MISSING";
    if (trustedOrc) {
      const canonicalRef = `refs/heads/${context.authority.metadata.canonical_integration_branch}`;
      const canonicalSha = repository && validGitRef(canonicalRef, repository);
      const transition = [...context.activationTransitions.values()].find((row) => row.orc0_receipt === trustedOrc.reference && row.orc0_landing === trustedOrc.landing?.reference);
      const landing = trustedOrc.landing;
      if (!transition || !landing
        || transition.candidate_sha !== trustedOrc.receipt.candidate_sha || transition.candidate_tree !== trustedOrc.receipt.candidate_tree || transition.candidate_ref !== trustedOrc.receipt.candidate_ref
        || landing.candidate_sha !== trustedOrc.receipt.candidate_sha || landing.candidate_tree !== trustedOrc.receipt.candidate_tree || landing.candidate_ref !== trustedOrc.receipt.candidate_ref
        || transition.canonical_ref !== canonicalRef || landing.canonical_ref !== canonicalRef || transition.canonical_sha !== landing.canonical_sha || transition.canonical_tree !== landing.canonical_tree
        || !canonicalSha || !gitIsAncestor(landing.canonical_sha, canonicalSha, repository)
        || Date.parse(receipt.accepted_at) < Date.parse(transition.activated_at)) errors.push(`receipt ${node.id} bypasses trusted-local ACTIVE/ORC0 lifecycle prerequisite`);
    } else {
    const transition = artifactReference(activation.activation_transition, context.activationTransitions, `receipt ${node.id} activation transition`);
    const authorization = context.authorizations.get(`ORC0:${activation.required_external_authorization}`);
    if (!orc || transition.error || !authorization || context.authority.metadata.state !== "ACTIVE" || activation.package_state !== "ACTIVE" || activation.orc0_receipt !== expectedActivation || activation.activation_authorization !== reference(activation.required_external_authorization, authorization) || activation.active_authority_sha256 !== context.authorityDigest || transition.row.j1_receipt !== activation.j1_receipt || transition.row.orc0_receipt !== expectedActivation || transition.row.activation_authorization !== activation.activation_authorization || transition.row.authority_sha256 !== context.authorityDigest || Date.parse(receipt.accepted_at) < Date.parse(transition.row.activated_at)) errors.push(`receipt ${node.id} bypasses ACTIVE/ORC0 lifecycle prerequisite`);
    }
  }
  if (receipt.activation_receipt !== expectedActivation) errors.push(`receipt ${node.id} activation receipt mismatch`);
  const lease = artifactReference(receipt.lease_receipt, context.leases, `receipt ${node.id} lease`);
  if (lease.error) errors.push(lease.error);
  else {
    if (lease.row.node_id !== node.id) errors.push(`receipt lease node mismatch ${file}`);
    if (lease.row.holder !== receipt.lease_holder || lease.row.candidate_ref !== receipt.candidate_ref) errors.push(`receipt lease holder/ref mismatch ${file}`);
    if (receipt.accepted_by !== lease.row.holder) errors.push(`receipt accepting identity differs from lease holder ${file}`);
    errors.push(...exactList(lease.row.conflict_domains, node.conflict_domains, `receipt ${node.id} lease domains`));
    const acquired = Date.parse(lease.row.acquired_at); const expires = Date.parse(lease.row.expires_at); const accepted = Date.parse(receipt.accepted_at);
    if (!Number.isFinite(accepted) || accepted < acquired || accepted > expires) errors.push(`receipt acceptance time is outside lease epoch ${file}`);
  }
  const dispatch = artifactReference(receipt.dispatch_receipt, context.dispatches, `receipt ${node.id} dispatch`);
  if (dispatch.error) errors.push(dispatch.error);
  else if (dispatch.row.node_id !== node.id || dispatch.row.lease_receipt !== receipt.lease_receipt) errors.push(`receipt ${node.id} dispatch/lease binding mismatch`);
  const finalization = artifactReference(receipt.finalization_receipt, context.finalizations, `receipt ${node.id} finalization`);
  if (finalization.error) errors.push(finalization.error);
  else {
    if (finalization.row.node_id !== node.id || finalization.row.lease_receipt !== receipt.lease_receipt || finalization.row.dispatch_receipt !== receipt.dispatch_receipt) errors.push(`receipt ${node.id} finalization custody mismatch`);
    if (finalization.row.base_sha !== receipt.base_sha || finalization.row.candidate_sha !== receipt.candidate_sha || finalization.row.candidate_tree !== receipt.candidate_tree) errors.push(`receipt ${node.id} does not bind exact finalized base/candidate`);
    errors.push(...exactList(receipt.changed_paths, finalization.row.changed_paths, `receipt ${node.id} finalized base delta`));
    if (receipt.accepted_by !== finalization.row.finalized_by || Date.parse(receipt.accepted_at) < Date.parse(finalization.row.finalized_at)) errors.push(`receipt ${node.id} acceptance precedes or changes finalization custody`);
  }
  if (!finalization.error) for (const requirement of node.external_requirements) {
    const auth = context.authorizations.get(`${node.id}:${requirement}`);
    if (auth && (auth.candidate_sha !== finalization.row.candidate_sha || auth.candidate_tree !== finalization.row.candidate_tree)) errors.push(`external authorization ${requirement} differs from finalized candidate`);
  }
  if (receipt.gate_receipts.length !== 2) errors.push(`receipt ${node.id} requires exact candidate and integration gate receipts`);
  const gateScopes = [];
  for (const value of receipt.gate_receipts) {
    const gate = artifactReference(value, context.gates, `receipt ${node.id} gate`);
    if (gate.error) errors.push(gate.error);
    else {
      if (gate.row.node_id !== node.id || gate.row.gate_profile !== evidenceProfiles(node).gate) errors.push(`receipt gate node/profile mismatch ${file}`);
      const expectedResult = !lease.error && (gate.row.scope === "integration" ? lease.row.integration_gate_result_path : lease.row.gate_result_path);
      if (!lease.error && (gate.row.executed_by !== lease.row.gate_runner || gate.row.result_path !== expectedResult)) errors.push(`receipt ${gate.row.scope} gate runner/destination differs from dispatch lease ${file}`);
      errors.push(...exactCandidate(gate.row, receipt, `receipt ${node.id} gate`));
      if (gate.row.scope === "integration") errors.push(...exactIntegration(gate.row, receipt, `receipt ${node.id} integration gate`));
      else if (gate.row.integration_sha !== gate.row.candidate_sha || gate.row.integration_tree !== gate.row.candidate_tree) errors.push(`receipt ${node.id} candidate gate is not bound to the exact finalized candidate identity`);
      if (!lease.error && (Date.parse(gate.row.started_at) < Date.parse(lease.row.acquired_at) || Date.parse(gate.row.completed_at) > Date.parse(lease.row.expires_at))) errors.push(`receipt gate evidence falls outside lease epoch ${file}`);
      gateScopes.push(gate.row.scope);
    }
  }
  if (exactList(gateScopes, ["candidate", "integration"], `receipt ${node.id} gate scopes`).length) errors.push(`receipt ${node.id} lacks distinct candidate and cross-block integration gates`);
  const profile = context.reviewProfiles.get(evidenceProfiles(node).review);
  if (!profile || receipt.review_receipts.length !== profile.reviewers) errors.push(`receipt ${node.id} review count does not match profile`);
  const reviewers = new Set(); const lenses = [];
  for (const value of receipt.review_receipts) {
    const review = artifactReference(value, context.reviews, `receipt ${node.id} review`);
    if (review.error) errors.push(review.error);
    else {
      if (review.row.node_id !== node.id || review.row.review_profile !== evidenceProfiles(node).review) errors.push(`receipt review node/profile mismatch ${file}`);
      if (!lease.error) {
        const reviewerAssignment = lease.row.reviewer_assignments.find((value) => value.startsWith(`${review.row.lens}=`));
        const reportAssignment = lease.row.review_report_paths.find((value) => value.startsWith(`${review.row.lens}=`));
        if (reviewerAssignment !== `${review.row.lens}=${review.row.reviewer}` || reportAssignment !== `${review.row.lens}=${review.row.report_path}`) errors.push(`receipt review identity/destination differs from dispatch lease ${file}`);
      }
      errors.push(...exactCandidate(review.row, receipt, `receipt ${node.id} review`));
      if (!lease.error && (Date.parse(review.row.started_at) < Date.parse(lease.row.acquired_at) || Date.parse(review.row.completed_at) > Date.parse(lease.row.expires_at))) errors.push(`receipt review evidence falls outside lease epoch ${file}`);
      reviewers.add(review.row.reviewer); lenses.push(review.row.lens);
    }
  }
  if (profile && (reviewers.size !== profile.reviewers || exactList(lenses, profile.lenses, `receipt ${node.id} review lenses`).length)) errors.push(`receipt ${node.id} reviews are not three independent exact lenses`);
  if (!lease.error) {
    const release = context.releases.get(receipt.lease_receipt);
    if (release) {
      const latest = Math.max(Date.parse(receipt.accepted_at), ...receipt.gate_receipts.map((value) => artifactReference(value, context.gates, "gate").row).filter(Boolean).map((row) => Date.parse(row.completed_at)), ...receipt.review_receipts.map((value) => artifactReference(value, context.reviews, "review").row).filter(Boolean).map((row) => Date.parse(row.completed_at)));
      if (Date.parse(release.released_at) < latest) errors.push(`receipt lease was released before completion ${file}`);
    }
  }
  for (const changedPath of receipt.changed_paths) {
    try { safeRelative(changedPath, `receipt ${node.id} changed path`); }
    catch (error) { errors.push(error.message); continue; }
    const covered = node.conflict_domains.some((id) => (context.domains.get(id)?.path_roots || []).some((root) => changedPath === root || changedPath.startsWith(`${root}/`)));
    if (!covered) errors.push(`receipt ${node.id} changed path is outside acquired conflict domains: ${changedPath}`);
    const owned = charterMutationRoots(context.authority, node).some((surface) => changedPath === surface || changedPath.startsWith(`${surface}/`));
    if (!owned) errors.push(`receipt ${node.id} changed path is outside its charter-owned mutation surfaces: ${changedPath}`);
  }
  if (node.release_gating === "product" && !context.receipts.has("BR0")) errors.push(`product release receipt ${node.id} requires accepted BR0 receipt`);
  return errors;
}

function staticLegacyFiles(authority) {
  const directory = path.join(authority.packageRoot, "state/legacy-receipts");
  const files = []; const errors = [];
  if (!fs.existsSync(directory)) return { files, errors: ["closed legacy receipt directory is missing"] };
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isFile() || entry.isSymbolicLink() || !entry.name.endsWith(".toml")) errors.push(`legacy receipt directory contains unsupported entry ${entry.name}`);
    else files.push(path.join(directory, entry.name));
  }
  return { files: files.sort(), errors };
}

function loadLandedReceipts(authority, runtimeRoot) {
  const receipts = new Map(); const errors = [];
  const inventory = runtimeFiles(authority, runtimeRoot, "landed-receipts");
  errors.push(...inventory.errors);
  const j1 = authority.nodes.find((node) => node.id === "J1");
  for (const file of inventory.files) {
    const checked = validateLandedReceiptFile(file, j1, authority.packageRoot);
    const row = checked.receipt; const rowErrors = [...checked.errors];
    if (row && path.basename(file) !== `${row.receipt_id}.toml`) rowErrors.push(`landed receipt filename mismatch ${file}`);
    if (row && receipts.has(row.node_id)) rowErrors.push(`duplicate landed receipt ${row.node_id}`);
    errors.push(...rowErrors);
    if (row && !rowErrors.length) receipts.set(row.node_id, { ...row, digest: checked.digest, file });
  }
  return { receipts, errors };
}

function loadReceipts(authority, runtimeRoot, context) {
  const { byId } = graphMaps(authority.nodes); const receipts = new Map(context.receipts); const errors = [];
  const manifest = readToml(confinedFile(authority.packageRoot, "authority/state/legacy-receipts.toml", "legacy receipt manifest"));
  const manifestRows = new Map();
  for (const row of manifest.receipt || []) {
    if (manifestRows.has(row.node_id)) errors.push(`duplicate closed legacy manifest row ${row.node_id}`);
    else manifestRows.set(row.node_id, row);
  }
  const legacy = staticLegacyFiles(authority); errors.push(...legacy.errors);
  errors.push(...exactList(legacy.files.map((file) => path.basename(file, ".toml")), manifestRows.keys(), "closed legacy manifest/file set"));
  for (const file of legacy.files) {
    const id = path.basename(file, ".toml"); const checked = validateReceiptFile(file, byId.get(id), authority.packageRoot);
    errors.push(...checked.errors);
    const value = { ...checked.receipt, digest: checked.digest, file };
    if (!checked.errors.length && context.invalidatedReceipts.has(reference(id, value))) errors.push(`legacy receipt ${id} is explicitly invalidated by the amendment chain`);
    else if (!checked.errors.length) { receipts.set(id, value); context.predecessorReceipts.set(id, value); }
  }
  const runtime = runtimeFiles(authority, runtimeRoot, "receipts"); errors.push(...runtime.errors);
  const runtimeById = new Map();
  for (const file of runtime.files) {
    let row;
    try { row = readToml(file); } catch (error) { errors.push(`malformed receipt ${file}: ${error.message}`); continue; }
    if (row.type !== "v2-acceptance" || path.basename(file) !== `${row.node_id}.toml` || !byId.has(row.node_id)) { errors.push(`runtime receipt filename/type/node mismatch ${file}`); continue; }
    if (runtimeById.has(row.node_id) || receipts.has(row.node_id)) errors.push(`duplicate accepted receipt ${row.node_id}`);
    else runtimeById.set(row.node_id, file);
  }
  context.receipts = receipts;
  for (const node of topological(authority.nodes).order) {
    const file = runtimeById.get(node.id); if (!file) continue;
    const checked = validateReceiptFile(file, node, authority.packageRoot, context);
    errors.push(...checked.errors);
    if (!checked.errors.length) {
      const value = { ...checked.receipt, digest: checked.digest, file };
      receipts.set(node.id, value); context.predecessorReceipts.set(node.id, value);
    }
  }
  return { receipts, errors };
}

export function deriveState(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), openedOptional = [], now = Date.now() } = {}) {
  const activation = readToml(confinedFile(authority.packageRoot, "authority/state/activation.toml", "activation state"));
  const amendmentErrors = validateAmendments(authority, { runtimeRoot });
  const amendmentState = loadAmendmentRows(authority);
  const transitionState = loadActivationTransitions(authority, runtimeRoot, now);
  const leaseState = loadLeases(authority, runtimeRoot, now, amendmentState.rows);
  const dispatchState = loadDispatches(authority, runtimeRoot, leaseState.all);
  const finalizationState = loadFinalizations(authority, runtimeRoot, leaseState.all, dispatchState.dispatches);
  const externalState = loadExternal(authority, runtimeRoot, now, amendmentState.rows, finalizationState.finalizations);
  const gateState = loadGateEvidence(authority, runtimeRoot, externalState.authorizations);
  const reviewState = loadReviewEvidence(authority, runtimeRoot, externalState.authorizations, now);
  const landedState = loadLandedReceipts(authority, runtimeRoot);
  const trustedCustody = trustedLocalAcceptances(authority, runtimeRoot);
  const trustedAcceptances = trustedCustody.acceptances;
  const trustedOrc = trustedAcceptances.get("ORC0") || null;
  const trustedReceipts = new Map(); const trustedReceiptErrors = [];
  for (const [nodeId, trusted] of trustedAcceptances) {
    const projected = trustedSuccessorReceipt(authority, trusted);
    if (projected.error) trustedReceiptErrors.push(projected.error);
    else if (projected.receipt) trustedReceipts.set(nodeId, projected.receipt);
  }
  const context = {
    authority,
    authorityDigest: computeAuthorityDigest(authority.packageRoot),
    receipts: new Map(trustedReceipts),
    predecessorReceipts: new Map([...landedState.receipts, ...trustedReceipts]),
    landedReceipts: landedState.receipts,
    authorizations: externalState.authorizations,
    leases: leaseState.all,
    gates: gateState.evidence,
    reviews: reviewState.evidence,
    dispatches: dispatchState.dispatches,
    finalizations: finalizationState.finalizations,
    releases: leaseState.releases,
    activation,
    activationTransitions: transitionState.transitions,
    domains: catalogMap(authority.packageRoot, "conflict-domains.toml", "domain"),
    reviewProfiles: catalogMap(authority.packageRoot, "review-profiles.toml", "profile"),
    amendments: amendmentState.rows,
    invalidatedReceipts: new Set(amendmentState.rows.flatMap((row) => row.invalidated_receipts)),
    trustedLocalOrc: trustedOrc,
    trustedLocalAcceptances: trustedAcceptances,
  };
  const receiptState = loadReceipts(authority, runtimeRoot, context);
  const errors = [...amendmentErrors, ...amendmentState.errors, ...externalState.errors, ...transitionState.errors, ...gateState.errors, ...reviewState.errors, ...leaseState.errors, ...dispatchState.errors, ...finalizationState.errors, ...landedState.errors, ...trustedCustody.errors, ...trustedReceiptErrors, ...receiptState.errors];
  const j1 = landedState.receipts.get("J1");
  const j1Reference = j1 ? reference(j1.receipt_id, j1) : "";
  const j1Bound = Boolean(j1) && activation.j1_state === "LANDED_GRANDFATHERED" && activation.j1_receipt === j1Reference;
  const directiveSlots = new Set((readToml(confinedFile(authority.packageRoot, "authority/state/external-authorizations.toml", "external directive slots")).authorization || []).map((row) => `${row.node_id}:${row.authorization}`));
  const orcAuthorization = externalState.authorizations.get(`ORC0:${activation.required_external_authorization}`);
  const orc = receiptState.receipts.get("ORC0");
  let phase = j1Bound && directiveSlots.has(`ORC0:${activation.required_external_authorization}`) ? "ORC0" : "DORMANT";
  if (j1 && !j1Bound) errors.push("external J1 landing receipt does not match the tracked expected-reference pin");
  if (activation.package_state === "ACTIVE" || authority.metadata.state === "ACTIVE") {
    const transition = artifactReference(activation.activation_transition, transitionState.transitions, "ACTIVE transition");
    const oldAcceptanceBound = j1 && activation.j1_state === "LANDED_GRANDFATHERED" && activation.j1_receipt === j1Reference
      && orc && activation.orc0_receipt === reference("ORC0", orc)
      && orcAuthorization && activation.activation_authorization === reference(activation.required_external_authorization, orcAuthorization);
    const trustedDirective = confinedFile(authority.packageRoot, "sources/maintainer-directive-2026-08-28-trusted-local-orc0.md", "trusted-local activation directive");
    const trustedAuthorization = `${activation.required_external_authorization}:${sha256(fs.readFileSync(trustedDirective))}`;
    const trustedAcceptanceBound = j1 && activation.j1_state === "LANDED_GRANDFATHERED" && activation.j1_receipt === j1Reference
      && trustedOrc && activation.orc0_receipt === trustedOrc.reference
      && activation.activation_authorization === trustedAuthorization;
    const activeBound = activation.package_state === "ACTIVE" && authority.metadata.state === "ACTIVE" && (oldAcceptanceBound || trustedAcceptanceBound)
      && activation.active_authority_sha256 === context.authorityDigest
      && !transition.error && transition.row.j1_receipt === j1Reference
      && transition.row.orc0_receipt === activation.orc0_receipt
      && transition.row.activation_authorization === activation.activation_authorization
      && transition.row.authority_sha256 === context.authorityDigest
      && (oldAcceptanceBound ? transition.row.activated_by === orcAuthorization.granted_by : transition.row.activated_by === trustedOrc.lease.holder);
    if (!activeBound) errors.push("premature activation: ACTIVE is not bound to exact J1/ORC0/authorization receipts and authority digest");
    else phase = "ACTIVE";
  } else if (activation.orc0_receipt || activation.activation_authorization || activation.active_authority_sha256 || activation.activation_transition) errors.push("premature activation: DORMANT authority contains tracked ACTIVE-only bindings");
  else if (transitionState.transitions.size) {
    const transitions = [...transitionState.transitions.values()]; const transition = transitions[0];
    const repository = gitRoot(authority.packageRoot); const canonicalRef = `refs/heads/${authority.metadata.canonical_integration_branch}`;
    const canonicalSha = repository && validGitRef(canonicalRef, repository);
    const directive = confinedFile(authority.packageRoot, "sources/maintainer-directive-2026-08-28-trusted-local-orc0.md", "trusted-local activation directive");
    const trustedAuthorization = `${activation.required_external_authorization}:${sha256(fs.readFileSync(directive))}`;
    const landing = trustedOrc?.landing;
    const activeBound = transitions.length === 1 && trustedOrc && landing
      && transition.j1_receipt === j1Reference
      && transition.orc0_receipt === trustedOrc.reference && transition.orc0_landing === landing.reference
      && transition.candidate_sha === trustedOrc.receipt.candidate_sha && transition.candidate_tree === trustedOrc.receipt.candidate_tree && transition.candidate_ref === trustedOrc.receipt.candidate_ref
      && landing.candidate_sha === trustedOrc.receipt.candidate_sha && landing.candidate_tree === trustedOrc.receipt.candidate_tree && landing.candidate_ref === trustedOrc.receipt.candidate_ref
      && transition.canonical_ref === canonicalRef && landing.canonical_ref === canonicalRef
      && transition.canonical_sha === landing.canonical_sha && transition.canonical_tree === landing.canonical_tree
      && landing.canonical_sha === trustedOrc.receipt.candidate_sha && landing.canonical_tree === trustedOrc.receipt.candidate_tree
      && canonicalSha && gitIsAncestor(landing.canonical_sha, canonicalSha, repository)
      && transition.activation_authorization === trustedAuthorization && transition.authority_sha256 === context.authorityDigest && transition.activated_by === trustedOrc.lease.holder;
    if (!activeBound) errors.push("premature activation: trusted-local transition is not bound to the exact accepted landing, canonical Git identity, authorization, and authority digest");
    else phase = "ACTIVE";
  }
  const active = phase === "ACTIVE";
  const states = new Map(); const opened = new Set(openedOptional); const { byId } = graphMaps(authority.nodes);
  for (const node of topological(authority.nodes).order) {
    const blockers = []; const receipt = receiptState.receipts.get(node.id); const ownLease = leaseState.active.find((lease) => lease.node_id === node.id);
    const landed = landedState.receipts.get(node.id);
    const trusted = trustedAcceptances.get(node.id);
    if (trusted) states.set(node.id, { status: "ACCEPTED", blockers, trusted_local_acceptance: trusted });
    else if (landed && j1Bound) states.set(node.id, { status: "LANDED_GRANDFATHERED", blockers, landed_receipt: landed });
    else if (receipt) states.set(node.id, { status: "ACCEPTED", blockers, receipt });
    else if (ownLease) states.set(node.id, { status: "IN_FLIGHT", blockers: [], lease: ownLease });
    else if (node.initial_state === "IN_FLIGHT") states.set(node.id, { status: "IN_FLIGHT", blockers: ["grandfathered v1 work has no accepted receipt"] });
    else if (node.initial_state === "RESCOPE_REQUIRED" || node.initial_state === "SUPERSEDED") states.set(node.id, { status: node.initial_state, blockers: [`static ${node.initial_state}`] });
    else if (!node.dispatchable) states.set(node.id, { status: "LOCKED", blockers: ["node is non-dispatchable"] });
    else {
        if (node.id === "ORC0" && phase === "DORMANT") blockers.push("package DORMANT: exact J1 receipt and activation authorization required before ORC0 admission");
        if (node.id !== "ORC0" && node.activation_gate === "ORC0" && !active) blockers.push(`package ${phase}: exact ORC0 activation is not ACTIVE`);
        for (const pred of node.predecessors) if (!context.predecessorReceipts.has(pred)) blockers.push(`missing satisfied predecessor receipt ${pred}`);
        if (node.release_gating === "product" && !receiptState.receipts.has("BR0")) blockers.push("product release requires accepted BR0 receipt");
        for (const conditional of node.conditional_predecessors) {
          const [id, condition] = conditional.split(":");
          if (condition === "when-opened" && opened.has(id) && !receiptState.receipts.has(id)) blockers.push(`conditional predecessor ${id} is opened but not accepted`);
        }
        for (const requirement of node.external_requirements) if (!directiveSlots.has(`${node.id}:${requirement}`)) blockers.push(`missing immutable static directive slot ${requirement}`);
        states.set(node.id, { status: blockers.length ? "BLOCKED" : "READY", blockers });
    }
  }
  for (const id of byId.keys()) if (!states.has(id)) states.set(id, { status: "BLOCKED", blockers: ["cyclic/unresolved graph"] });
  return {
    states, receipts: receiptState.receipts, landedReceipts: landedState.receipts, predecessorReceipts: context.predecessorReceipts, authorizations: externalState.authorizations,
    leases: leaseState.active, allLeases: leaseState.all, gates: gateState.evidence, reviews: reviewState.evidence,
    dispatches: dispatchState.dispatches, finalizations: finalizationState.finalizations,
    activationTransitions: transitionState.transitions,
    errors, active, phase, runtimeRoot: path.resolve(runtimeRoot), authorityDigest: context.authorityDigest, trustedLocalAcceptances: trustedAcceptances,
  };
}

function assertIdentity(value, label) {
  if (typeof value !== "string" || !value.trim() || value !== value.trim() || /[\0\r\n]/.test(value) || value.length > 200) throw new Error(`${label} must be a nonempty single-line identity`);
  return value;
}

function artifactBody(fields) {
  return `${Object.entries(fields).map(([key, value]) => `${key} = ${tomlValue(value)}`).join("\n")}\n`;
}

function artifactText(fields) {
  const body = artifactBody(fields);
  return `${body}payload_sha256 = "${digestPayload(body)}"\n`;
}

function atomicCreate(file, text) {
  const temp = path.join(path.dirname(file), `.${path.basename(file)}.${process.pid}.${crypto.randomBytes(8).toString("hex")}.tmp`);
  let descriptor;
  try {
    descriptor = fs.openSync(temp, "wx", 0o600);
    fs.writeFileSync(descriptor, text, "utf8");
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor); descriptor = undefined;
    // A POSIX rename may silently replace an existing destination. Linking the
    // fully fsynced same-directory inode is atomic and fails with EEXIST, so two
    // importers can never replace one another's immutable runtime artifact.
    try { fs.linkSync(temp, file); }
    catch (error) {
      if (error.code === "EEXIST") throw new Error(`immutable artifact already exists ${file}`);
      throw error;
    }
    const directory = fs.openSync(path.dirname(file), "r");
    try { fs.fsyncSync(directory); } finally { fs.closeSync(directory); }
    fs.unlinkSync(temp);
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    if (fs.existsSync(temp)) fs.unlinkSync(temp);
  }
}

function flatToml(model) {
  return `${Object.entries(model).map(([key, value]) => `${key} = ${tomlValue(value)}`).join("\n")}\n`;
}

function withActivationLock(authority, run) {
  const lock = path.join(authority.packageRoot, "authority/state/.activation.lock");
  try { fs.mkdirSync(lock, { mode: 0o700 }); }
  catch (error) {
    if (error.code === "EEXIST") throw new Error(`activation transaction is already in progress: ${lock}`);
    throw error;
  }
  try { return run(lock); }
  finally { if (fs.existsSync(lock)) fs.rmdirSync(lock); }
}

function stageReplacements(replacements, backupRoot = null) {
  const staged = [];
  try {
    for (const [file, content] of replacements) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      const nonce = `${process.pid}.${crypto.randomBytes(8).toString("hex")}`;
      const temporary = path.join(path.dirname(file), `.${path.basename(file)}.${nonce}.tmp`);
      const backup = backupRoot
        ? path.join(backupRoot, `${sha256(path.resolve(file)).slice(0, 24)}.${nonce}.bak`)
        : path.join(path.dirname(file), `.${path.basename(file)}.${nonce}.bak`);
      const descriptor = fs.openSync(temporary, "wx", 0o600);
      try { fs.writeFileSync(descriptor, content); fs.fsyncSync(descriptor); }
      finally { fs.closeSync(descriptor); }
      staged.push({ file, temporary, backup, hadOriginal: fs.existsSync(file), committed: false });
    }
    return staged;
  } catch (error) {
    for (const row of staged) if (fs.existsSync(row.temporary)) fs.unlinkSync(row.temporary);
    throw error;
  }
}

function commitReplacements(staged) {
  for (const row of staged) {
    if (row.hadOriginal) fs.renameSync(row.file, row.backup);
    try { fs.renameSync(row.temporary, row.file); }
    catch (error) {
      if (row.hadOriginal && fs.existsSync(row.backup)) fs.renameSync(row.backup, row.file);
      throw error;
    }
    row.committed = true;
    const directory = fs.openSync(path.dirname(row.file), "r");
    try { fs.fsyncSync(directory); } finally { fs.closeSync(directory); }
  }
}

function rollbackReplacements(staged) {
  for (const row of [...staged].reverse()) {
    if (row.committed && fs.existsSync(row.file)) fs.unlinkSync(row.file);
    if (row.hadOriginal && fs.existsSync(row.backup)) fs.renameSync(row.backup, row.file);
    if (fs.existsSync(row.temporary)) fs.unlinkSync(row.temporary);
  }
}

function finishReplacements(staged) {
  for (const row of staged) {
    if (fs.existsSync(row.backup)) fs.unlinkSync(row.backup);
    if (fs.existsSync(row.temporary)) fs.unlinkSync(row.temporary);
  }
}

const RUNTIME_IMPORTS = Object.freeze({
  authorization: { directory: "external", schema: "external-authorization.schema.json", id: (row) => `${row.node_id}--${row.authorization}`, map: "authorizations", key: (row) => `${row.node_id}:${row.authorization}` },
  gate: { directory: "gates", schema: "gate-evidence.schema.json", id: (row) => row.evidence_id, map: "gates", key: (row) => row.evidence_id },
  review: { directory: "reviews", schema: "review-evidence.schema.json", id: (row) => row.evidence_id, map: "reviews", key: (row) => row.evidence_id },
  receipt: { directory: "receipts", schema: "receipt.schema.json", id: (row) => row.node_id, map: "receipts", key: (row) => row.node_id },
  landed: { directory: "landed-receipts", schema: "landed-receipt.schema.json", id: (row) => row.receipt_id, map: "landedReceipts", key: (row) => row.node_id },
});

function sourceArtifact(file) {
  const absolute = path.resolve(file);
  if (!fs.existsSync(absolute) || fs.lstatSync(absolute).isSymbolicLink() || !fs.statSync(absolute).isFile()) throw new Error(`import source must be an existing non-symlink regular file: ${absolute}`);
  return absolute;
}

export function importRuntimeArtifact(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), kind, file, now = Date.now() } = {}) {
  mutationAuthority(authority, runtimeRoot);
  const contract = RUNTIME_IMPORTS[kind];
  if (!contract) throw new Error(`runtime artifact kind must be one of ${Object.keys(RUNTIME_IMPORTS).join(", ")}`);
  const source = sourceArtifact(file);
  const checked = validatePayloadArtifact(source, authority.packageRoot, contract.schema, `${kind} import`);
  if (checked.errors.length) throw new Error(`invalid ${kind} import: ${checked.errors.join("; ")}`);
  const row = checked.row;
  if (kind === "receipt" && row.type !== "v2-acceptance") throw new Error("receipt import only accepts v2-acceptance artifacts");
  if (kind === "gate") throw new Error("gate evidence import is forbidden; use gate-run so PASS is derived from real subprocess execution");
  if (kind === "review") throw new Error("review evidence import is forbidden; use review-run with an immutable trusted reviewer capability");
  if (kind === "landed") {
    const node = authority.nodes.find((candidate) => candidate.id === row.node_id);
    const landed = validateLandedReceiptFile(source, node, authority.packageRoot);
    if (landed.errors.length) throw new Error(`invalid landed import: ${landed.errors.join("; ")}`);
  }
  if (kind === "authorization" && row.grant_mode === "MAINTAINER_DIRECTIVE_FINALIZED_CANDIDATE") throw new Error("directive-mode authorization-import is forbidden; use authorization-create after candidate-finalize");
  const id = contract.id(row);
  if (!id || !/^[A-Za-z0-9._-]+$/.test(id)) throw new Error(`${kind} import has an unsafe artifact id`);
  const directory = runtimeDirectory(authority, runtimeRoot, contract.directory, { create: true });
  const destination = path.join(directory, `${id}.toml`);
  atomicCreate(destination, fs.readFileSync(source));
  try {
    const current = deriveState(authority, { runtimeRoot, now });
    if (current.errors.length || !current[contract.map]?.has(contract.key(row))) throw new Error(`import postcondition failed: ${current.errors.join("; ") || `${kind} was not accepted`}`);
    return { artifact: current[contract.map].get(contract.key(row)), state: current, destination };
  } catch (error) {
    if (fs.existsSync(destination)) fs.unlinkSync(destination);
    throw error;
  }
}

export function importAcceptanceReceipt(authority, options = {}) {
  return importRuntimeArtifact(authority, { ...options, kind: "receipt" });
}

export function createDirectiveAuthorization(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, holder, leaseId: requestedLeaseId, now = Date.now() } = {}) {
  mutationAuthority(authority, runtimeRoot); assertIdentity(holder, "authorization holder");
  if (id !== "ORC0") throw new Error("authorization-create is limited to the exact ORC0 maintainer directive slot");
  const state = deriveState(authority, { runtimeRoot, now });
  if (state.errors.length) throw new Error(`state invalid before authorization-create: ${state.errors.join("; ")}`);
  const lease = state.allLeases.get(requestedLeaseId);
  if (!lease || lease.node_id !== id || lease.holder !== holder) throw new Error("authorization-create requires the exact ORC0 lease id and holder");
  const finalization = [...state.finalizations.values()].find((row) => row.node_id === id && row.lease_receipt === reference(lease.lease_id, lease));
  if (!finalization) throw new Error("authorization-create requires the exact validated candidate-finalize receipt");
  const manifest = readToml(confinedFile(authority.packageRoot, "authority/state/external-authorizations.toml", "authorization directive manifest"));
  const slot = (manifest.authorization || []).find((row) => row.node_id === id && row.authorization === "maintainer_unified_v2_activation");
  if (!slot || slot.grant_mode !== "MAINTAINER_DIRECTIVE_FINALIZED_CANDIDATE" || slot.directive_scope !== "unified-v2-orc0-activation-only") throw new Error("authorization-create lacks the exact immutable ORC0 directive slot");
  const fields = {
    schema: 2, type: "external-authorization", authorization: slot.authorization, node_id: id,
    candidate_sha: finalization.candidate_sha, candidate_tree: finalization.candidate_tree,
    authority_sha256: computeAuthorityDigest(authority.packageRoot), granted_by: slot.granted_by,
    ratification_path: slot.ratification_path, ratification_receipt_sha256: slot.ratification_receipt_sha256,
    expires_at: slot.expires_at, grant_mode: slot.grant_mode, directive_scope: slot.directive_scope,
  };
  const directory = runtimeDirectory(authority, runtimeRoot, "external", { create: true });
  const destination = path.join(directory, `${id}--${slot.authorization}.toml`);
  atomicCreate(destination, artifactText(fields));
  try {
    const after = deriveState(authority, { runtimeRoot, now });
    const authorization = after.authorizations.get(`${id}:${slot.authorization}`);
    if (after.errors.length || !authorization) throw new Error(`authorization-create postcondition failed: ${after.errors.join("; ") || "authorization absent"}`);
    return { artifact: authorization, state: after, destination };
  } catch (error) {
    if (fs.existsSync(destination)) fs.unlinkSync(destination);
    throw error;
  }
}

export function activateProgram(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), orc0Receipt, authorization, activatedBy, now = Date.now() } = {}) {
  mutationAuthority(authority, runtimeRoot);
  assertIdentity(activatedBy, "activation identity");
  const before = deriveState(authority, { runtimeRoot, now });
  if (before.errors.length) throw new Error(`state invalid before activation: ${before.errors.join("; ")}`);
  const activationFile = path.join(authority.packageRoot, "authority/state/activation.toml");
  const rootFile = path.join(authority.packageRoot, "authority/root.toml");
  const activation = readToml(activationFile);
  if (authority.metadata.state !== "DORMANT" || activation.package_state !== "DORMANT" || before.phase !== "ORC0") throw new Error(`activation requires exact DORMANT/ORC0 pre-state, got root=${authority.metadata.state} activation=${activation.package_state} phase=${before.phase}`);
  const j1 = before.landedReceipts.get("J1"); const orc = before.receipts.get("ORC0");
  const auth = before.authorizations.get(`ORC0:${activation.required_external_authorization}`);
  const expectedOrc = orc && reference("ORC0", orc);
  const expectedAuth = auth && reference(activation.required_external_authorization, auth);
  if (!j1 || activation.j1_state !== "LANDED_GRANDFATHERED" || activation.j1_receipt !== reference(j1.receipt_id, j1)) throw new Error("activation requires an exact external grandfathered J1 landing matching the tracked expected-reference pin");
  if (!orc || orc0Receipt !== expectedOrc) throw new Error("activation ORC0 receipt does not match the exact validated receipt");
  if (!auth || authorization !== expectedAuth || activatedBy !== auth.granted_by) throw new Error("activation authorization/identity does not match the exact trusted candidate-bound grant");
  const authorityDigest = computeAuthorityDigest(authority.packageRoot);
  const transitionId = `ACT-${new Date(now).toISOString().replaceAll(/[-:.TZ]/g, "")}-${crypto.randomBytes(8).toString("hex").toUpperCase()}`;
  const transitionFields = {
    schema: 2, type: "activation-transition", transition_id: transitionId,
    from_state: "DORMANT", to_state: "ACTIVE", j1_receipt: reference(j1.receipt_id, j1),
    orc0_receipt: expectedOrc, activation_authorization: expectedAuth,
    authority_sha256: authorityDigest, activated_by: activatedBy, activated_at: new Date(now).toISOString(),
  };
  const transitionText = artifactText(transitionFields);
  const transitionDigest = digestPayload(artifactBody(transitionFields));
  const transitionDirectory = runtimeDirectory(authority, runtimeRoot, "activations", { create: true });
  const transitionFile = path.join(transitionDirectory, `${transitionId}.toml`);
  return withActivationLock(authority, (activationLock) => {
    const rootText = fs.readFileSync(rootFile, "utf8");
    const activeRootText = rootText.replace(/^state = "DORMANT"$/m, 'state = "ACTIVE"');
    if (activeRootText === rootText) throw new Error("root authority is not in the exact DORMANT state");
    const activeActivation = {
      ...activation,
      package_state: "ACTIVE",
      j1_state: "LANDED_GRANDFATHERED",
      j1_receipt: reference(j1.receipt_id, j1),
      orc0_receipt: expectedOrc,
      activation_authorization: expectedAuth,
      active_authority_sha256: authorityDigest,
      activation_transition: `${transitionId}:${transitionDigest}`,
    };
    const projectedAuthority = { ...authority, metadata: { ...authority.metadata, state: "ACTIVE" } };
    const replacements = [[rootFile, activeRootText], [activationFile, flatToml(activeActivation)]];
    for (const [relative, content] of generatedFiles(projectedAuthority)) replacements.push([path.join(authority.packageRoot, relative), content.endsWith("\n") ? content : `${content}\n`]);
    const staged = stageReplacements(replacements, activationLock);
    let transitionCreated = false;
    try {
      atomicCreate(transitionFile, transitionText); transitionCreated = true;
      commitReplacements(staged);
      const activeAuthority = loadAuthority(authority.packageRoot);
      const staticErrors = validateAuthority(activeAuthority, { strict: true, checkGenerated: true, runtimeRoot }).filter((error) => error !== "activation transaction is incomplete; partial activation is refused");
      const activeState = deriveState(activeAuthority, { runtimeRoot, now });
      if (staticErrors.length || activeState.errors.length || activeState.phase !== "ACTIVE" || !activeState.active) throw new Error(`activation postcondition failed: ${[...staticErrors, ...activeState.errors, `phase=${activeState.phase}`].join("; ")}`);
      finishReplacements(staged);
      return { transition: activeState.activationTransitions.get(transitionId), state: activeState, authority: activeAuthority };
    } catch (error) {
      rollbackReplacements(staged);
      if (transitionCreated && fs.existsSync(transitionFile)) fs.unlinkSync(transitionFile);
      throw error;
    }
  });
}

export function activateTrustedProgram(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), activatedBy, now = Date.now() } = {}) {
  mutationAuthority(authority, runtimeRoot); assertIdentity(activatedBy, "activation identity");
  const before = deriveState(authority, { runtimeRoot, now });
  if (before.errors.length) throw new Error(`state invalid before trusted-local activation: ${before.errors.join("; ")}`);
  const activation = readToml(path.join(authority.packageRoot, "authority/state/activation.toml")); const trustedOrc = before.trustedLocalAcceptances.get("ORC0"); const j1 = before.landedReceipts.get("J1");
  if (authority.metadata.state !== "DORMANT" || activation.package_state !== "DORMANT" || before.phase !== "ORC0") throw new Error("trusted-local activation requires exact DORMANT/ORC0 pre-state");
  if (!j1 || activation.j1_state !== "LANDED_GRANDFATHERED" || activation.j1_receipt !== reference(j1.receipt_id, j1)) throw new Error("trusted-local activation requires an exact external grandfathered J1 landing matching the tracked expected-reference pin");
  if (!trustedOrc || !trustedOrc.landing || trustedOrc.lease.holder !== activatedBy) throw new Error("trusted-local activation requires the current accepted ORC0 round, exact candidate landing, and its operator");
  assertTrustedFrozenCandidate(authority, { runtimeRoot, leaseId: trustedOrc.lease.lease_id });
  const repository = gitRoot(authority.packageRoot); const canonicalRef = `refs/heads/${authority.metadata.canonical_integration_branch}`;
  const canonicalSha = validGitRef(canonicalRef, repository); const canonicalTree = canonicalSha && gitTree(canonicalSha, repository);
  if (canonicalSha !== trustedOrc.receipt.candidate_sha || canonicalTree !== trustedOrc.receipt.candidate_tree || trustedOrc.landing.canonical_ref !== canonicalRef || trustedOrc.landing.canonical_sha !== canonicalSha || trustedOrc.landing.canonical_tree !== canonicalTree) throw new Error("trusted-local activation requires the canonical branch tip to remain exactly the reviewed and landed candidate identity");
  if (gitOutput(["status", "--porcelain=v1", "--untracked-files=all"], repository)) throw new Error("trusted-local activation requires a clean canonical repository worktree");
  const directive = fs.readFileSync(confinedFile(authority.packageRoot, "sources/maintainer-directive-2026-08-28-trusted-local-orc0.md", "trusted-local activation directive"));
  const authorization = `${activation.required_external_authorization}:${sha256(directive)}`;
  const authorityDigest = computeAuthorityDigest(authority.packageRoot); const transitionId = `ACT-${new Date(now).toISOString().replaceAll(/[-:.TZ]/g, "")}-${crypto.randomBytes(8).toString("hex").toUpperCase()}`;
  const transitionFields = { schema: 2, type: "activation-transition", transition_id: transitionId, from_state: "DORMANT", to_state: "ACTIVE", j1_receipt: reference(j1.receipt_id, j1), orc0_receipt: trustedOrc.reference, orc0_landing: trustedOrc.landing.reference, candidate_sha: trustedOrc.receipt.candidate_sha, candidate_tree: trustedOrc.receipt.candidate_tree, candidate_ref: trustedOrc.receipt.candidate_ref, canonical_ref: canonicalRef, canonical_sha: canonicalSha, canonical_tree: canonicalTree, activation_authorization: authorization, authority_sha256: authorityDigest, activated_by: activatedBy, activated_at: new Date(now).toISOString() };
  const transitionText = artifactText(transitionFields); const transitionDigest = digestPayload(artifactBody(transitionFields));
  const transitionModel = { ...transitionFields, payload_sha256: transitionDigest };
  const transitionErrors = validateSchemaObject(transitionModel, loadSchema(authority.packageRoot, "activation-transition.schema.json"), "trusted-local activation transition");
  if (transitionErrors.length) throw new Error(`trusted-local activation transition is invalid: ${transitionErrors.join("; ")}`);
  trustedController(authority).publishActivation({ runtimeRoot, roundId: trustedOrc.roundId, holder: activatedBy, transitionId, transitionBytes: Buffer.from(transitionText) });
  const activeState = deriveState(authority, { runtimeRoot, now });
  if (activeState.errors.length || activeState.phase !== "ACTIVE") throw new Error(`trusted-local activation postcondition failed: ${activeState.errors.join("; ")}`);
  return { transition: activeState.activationTransitions.get(transitionId), state: activeState, authority };
}

function withAdmissionLock(authority, runtimeRoot, run) {
  const leases = runtimeDirectory(authority, runtimeRoot, "leases", { create: true });
  const root = path.dirname(leases);
  const lock = path.join(root, ".admission.lock");
  let acquired = false;
  const waiter = new Int32Array(new SharedArrayBuffer(4));
  for (let attempt = 0; attempt < 5000 && !acquired; attempt += 1) {
    try { fs.mkdirSync(lock, { mode: 0o700 }); acquired = true; }
    catch (error) {
      if (error.code !== "EEXIST") throw error;
      Atomics.wait(waiter, 0, 0, 2);
    }
  }
  if (!acquired) throw new Error(`atomic admission lock remained held: ${lock}`);
  try { return run(root, leases); }
  finally { fs.rmdirSync(lock); }
}

function mutationAuthority(authority, runtimeRoot = defaultRuntimeRoot(authority.packageRoot)) {
  const errors = validateAuthority(authority, { strict: true, checkGenerated: true, runtimeRoot });
  if (errors.length) throw new Error(`static authority invalid: ${errors.join("; ")}`);
}

function trustedController(authority) {
  const history = JSON.parse(fs.readFileSync(confinedFile(authority.packageRoot, "authority/state/preactivation-orc0-history.json", "preactivation ORC0 history"), "utf8"));
  return createLocalLifecycle({ controlRoot: trustedLocalControlRoot(authority), preactivationHistory: history });
}

export function reinitializeTrustedLocal(authority, { operator, reason } = {}) {
  return reinitializeLocalLifecycle({ controlRoot: trustedLocalControlRoot(authority), operator, reason });
}

export function admitTrustedNode(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, holder, candidateRef, effortOverrides = {} } = {}) {
  mutationAuthority(authority, runtimeRoot);
  const node = authority.nodes.find((candidateNode) => candidateNode.id === id);
  if (!node) throw new Error(`unknown node ${id}`);
  const state = deriveState(authority, { runtimeRoot });
  if (state.errors.length) throw new Error(`state invalid: ${state.errors.join("; ")}`);
  if (state.states.get(id)?.status !== "READY") throw new Error(`${id} is not READY: ${state.states.get(id)?.blockers?.join("; ") || state.states.get(id)?.status}`);
  const candidate = admissionCandidate(authority, candidateRef);
  const review = extractProfile(path.join(authority.packageRoot, "catalogs/review-profiles.toml"), evidenceProfiles(node).review);
  const lease = trustedController(authority).admit({
    runtimeRoot,
    node: { ...node, risk: review?.risk_band || (node.class === "governance" ? "critical" : node.semantic_role === "convergence" ? "high" : "medium"), public_api: review?.id === "public-3", concurrency_sensitive: review?.id === "concurrency-3", semantic_authority: node.kind === "activation", review_lenses: review?.lenses, specialist_review_lens: review?.lenses?.[2] },
    candidate: { sha: candidate.sha, tree: candidate.tree, ref: candidateRef, worktree: candidate.worktree },
    holder,
    effortOverrides,
  });
  const packetFile = path.join(path.resolve(runtimeRoot), "trusted-local", "packets", `${lease.round_id}.json`);
  return { lease, packet: fs.readFileSync(packetFile, "utf8"), packetFile, briefPaths: lease.task_briefs };
}

export function finalizeTrustedCandidate(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), leaseId, holder } = {}) {
  mutationAuthority(authority, runtimeRoot);
  const anchor = readLocalAnchor({ controlRoot: trustedLocalControlRoot(authority) });
  const lease = anchor.leases[leaseId];
  if (!lease) throw new Error(`trusted-local lease is unknown: ${leaseId}`);
  const candidate = admissionCandidate(authority, lease.candidate.ref);
  if (!gitIsAncestor(lease.candidate.sha, candidate.sha, gitRoot(authority.packageRoot))) throw new Error("finalized candidate does not descend from its admitted start");
  return trustedController(authority).finalize({ runtimeRoot, leaseId, holder, candidate: { sha: candidate.sha, tree: candidate.tree, ref: lease.candidate.ref, worktree: candidate.worktree } });
}

export function dispatchTrustedNode(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, leaseId, holder } = {}) {
  mutationAuthority(authority, runtimeRoot);
  const anchor = readLocalAnchor({ controlRoot: trustedLocalControlRoot(authority) });
  const lease = anchor.leases[leaseId]; const round = lease && anchor.rounds[lease.round_id];
  if (!lease || lease.node_id !== id || lease.holder !== holder || lease.runtime_root !== path.resolve(runtimeRoot) || lease.status !== "ACTIVE" || round?.status !== "ACTIVE" || round.lease_id !== leaseId) throw new Error("dispatch requires the exact current trusted-local lease, node, runtime, and holder");
  const packetFile = path.join(path.resolve(runtimeRoot), "trusted-local", "packets", `${lease.round_id}.json`);
  return { lease, packet: fs.readFileSync(packetFile, "utf8"), packetFile, briefPaths: lease.task_briefs };
}

function assertTrustedFrozenCandidate(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), leaseId } = {}) {
  const anchor = readLocalAnchor({ controlRoot: trustedLocalControlRoot(authority) });
  const lease = anchor.leases[leaseId]; const frozen = lease?.finalization?.candidate;
  if (!frozen) throw new Error("operation requires an immutable finalized review target");
  const manifestBytes = Buffer.from(`${JSON.stringify(lease.finalization.review_target, null, 2)}\n`);
  const manifestFile = path.join(path.resolve(runtimeRoot), "trusted-local", "review-targets", `${lease.round_id}.json`);
  if (!fs.existsSync(manifestFile) || sha256(fs.readFileSync(manifestFile)) !== lease.finalization.review_target_sha256 || sha256(manifestBytes) !== lease.finalization.review_target_sha256) throw new Error("frozen review target manifest bytes changed; invalidate and restart the review round");
  const current = admissionCandidate(authority, frozen.ref);
  if (current.sha !== frozen.sha || current.tree !== frozen.tree || current.worktree !== frozen.worktree) throw new Error("frozen review target changed; invalidate and restart the review round");
  if (lease.runtime_root !== path.resolve(runtimeRoot)) throw new Error("frozen review target runtime mismatch");
  return lease;
}

export function recordTrustedReview(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  const lease = assertTrustedFrozenCandidate(authority, options);
  const mode = options.worktreeMode || "read-only";
  if (mode === "write-enabled") {
    const repository = gitRoot(authority.packageRoot); const frozenPath = fs.realpathSync(lease.finalization.candidate.worktree);
    if (!options.disposableWorktree || !fs.existsSync(options.disposableWorktree)) throw new Error("write-enabled review requires an existing disposable Git worktree");
    const disposablePath = path.resolve(options.disposableWorktree); const disposableReal = fs.realpathSync(disposablePath);
    if (disposableReal === frozenPath) throw new Error("write-enabled reviewer worktree must be distinct from the frozen train worktree");
    const listed = gitWorktrees(repository).find((row) => fs.realpathSync(row.worktree) === disposableReal);
    const head = listed && gitOutput(["rev-parse", "HEAD"], disposableReal); const tree = head && gitTree(head, disposableReal);
    if (!listed || head !== lease.finalization.candidate.sha || tree !== lease.finalization.candidate.tree || gitOutput(["status", "--porcelain=v1", "--untracked-files=all"], disposableReal)) throw new Error("writable review requires a clean listed disposable worktree at the exact frozen candidate SHA/tree");
    options = { ...options, worktreeMode: mode, disposableWorktree: disposablePath, disposableCandidateSha: head, disposableCandidateTree: tree };
  } else options = { ...options, worktreeMode: "read-only" };
  return trustedController(authority).recordReview(options);
}

export function recordTrustedReviewCleanup(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  const repository = gitRoot(authority.packageRoot);
  if (options.worktree && (fs.existsSync(options.worktree) || gitWorktrees(repository).some((row) => path.resolve(row.worktree) === path.resolve(options.worktree)))) throw new Error("disposable review worktree must be removed from Git and the filesystem before cleanup is recorded");
  return trustedController(authority).recordReviewCleanup(options);
}

export function recordTrustedRole(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  assertTrustedFrozenCandidate(authority, options);
  return trustedController(authority).recordRole(options);
}

export function recordTrustedArchitectDecision(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  return trustedController(authority).recordArchitectDecision(options);
}

export function closeTrustedRound(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  return trustedController(authority).close(options);
}

export function renewTrustedLease(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  return trustedController(authority).renew(options);
}

export function acceptTrustedRound(authority, options = {}) {
  mutationAuthority(authority, options.runtimeRoot);
  const anchor = readLocalAnchor({ controlRoot: trustedLocalControlRoot(authority) });
  const round = anchor.rounds[options.roundId];
  assertTrustedFrozenCandidate(authority, { ...options, leaseId: round?.lease_id });
  return trustedController(authority).accept(options);
}

export function recordTrustedLanding(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), roundId, holder } = {}) {
  mutationAuthority(authority, runtimeRoot);
  const current = deriveState(authority, { runtimeRoot });
  if (current.errors.length) throw new Error(`state invalid before trusted-local landing: ${current.errors.join("; ")}`);
  const trusted = [...current.trustedLocalAcceptances.values()].find((row) => row.roundId === roundId);
  if (!trusted || trusted.roundId !== roundId || trusted.lease.holder !== holder) throw new Error("landing requires the exact accepted trusted-local round and holder");
  const repository = gitRoot(authority.packageRoot); const canonicalRef = `refs/heads/${authority.metadata.canonical_integration_branch}`;
  const canonicalSha = validGitRef(canonicalRef, repository); const canonicalTree = canonicalSha && gitTree(canonicalSha, repository);
  if (!canonicalSha || !canonicalTree || canonicalSha !== trusted.receipt.candidate_sha || canonicalTree !== trusted.receipt.candidate_tree) throw new Error("canonical branch tip must exactly equal the reviewed candidate SHA and tree before landing is recorded");
  return trustedController(authority).recordLanding({ runtimeRoot, roundId, holder, canonicalRef, canonicalSha, canonicalTree });
}

function leaseId(nodeId, now) {
  return `${nodeId}-${new Date(now).toISOString().replaceAll(/[-:.TZ]/g, "")}-${crypto.randomBytes(8).toString("hex")}`;
}

function normalizeReviewers(profile, reviewers, gateRunner) {
  const parsed = (reviewers || []).map((value) => {
    if (typeof value !== "string" || !value.includes("=")) throw new Error(`reviewer assignment must be LENS=IDENTITY: ${value}`);
    const split = value.indexOf("="); const lens = value.slice(0, split); const identity = assertIdentity(value.slice(split + 1), `reviewer ${lens}`);
    if (!/^[a-z][a-z0-9-]*$/.test(lens)) throw new Error(`invalid reviewer lens ${lens}`);
    return [lens, identity];
  });
  const lensErrors = exactList(parsed.map(([lens]) => lens), profile?.lenses || [], "admission reviewer lenses");
  if (lensErrors.length) throw new Error(lensErrors[0]);
  if (new Set(parsed.map(([, identity]) => identity)).size !== parsed.length || parsed.some(([, identity]) => identity === gateRunner)) throw new Error("reviewer identities must be distinct from one another and the gate runner");
  return parsed;
}

function leaseFields(authority, node, options, candidate, now, id, renewedFrom = "") {
  const reviewProfile = catalogMap(authority.packageRoot, "review-profiles.toml", "profile").get(evidenceProfiles(node).review);
  const gateRunner = assertIdentity(options.gateRunner, "gate runner");
  const reviewers = normalizeReviewers(reviewProfile, options.reviewers, gateRunner);
  const domains = catalogMap(authority.packageRoot, "conflict-domains.toml", "domain");
  const scopePathRoots = [...new Set(node.conflict_domains.flatMap((domain) => domains.get(domain)?.path_roots || []))].sort();
  const scopeSymbols = [...new Set(node.conflict_domains.flatMap((domain) => domains.get(domain)?.symbols || []))].sort();
  if (!scopePathRoots.length || !scopeSymbols.length) throw new Error(`${node.id} admission scope is empty or unresolved`);
  return {
    schema: 2,
    type: "admission-lease",
    lease_id: id,
    node_id: node.id,
    holder: assertIdentity(options.holder, "lease holder"),
    base_sha: candidate.baseSha,
    base_tree: candidate.baseTree,
    candidate_ref: options.candidateRef,
    candidate_sha: candidate.sha,
    candidate_tree: candidate.tree,
    candidate_worktree: candidate.worktree,
    authority_sha256: computeAuthorityDigest(authority.packageRoot),
    conflict_domains: [...node.conflict_domains],
    scope_path_roots: scopePathRoots,
    scope_symbols: scopeSymbols,
    resource_class: node.resource_class,
    gate_runner: gateRunner,
    gate_result_path: options.gateResultPath || `gate-results/${id}.txt`,
    integration_gate_result_path: options.integrationGateResultPath || `gate-results/${id}--integration.txt`,
    reviewer_assignments: reviewers.map(([lens, identity]) => `${lens}=${identity}`),
    review_report_paths: options.reviewReportPaths || reviewers.map(([lens]) => `${lens}=review-reports/${id}--${lens}.md`),
    renewed_from: renewedFrom,
    acquired_at: new Date(now).toISOString(),
    expires_at: new Date(now + options.ttlSeconds * 1000).toISOString(),
  };
}

function candidateForRef(authority, candidateRef) {
  if (typeof candidateRef !== "string" || !candidateRef.startsWith("refs/")) throw new Error("candidate-ref must be a full refs/... Git ref");
  const repository = gitRoot(authority.packageRoot); const sha = repository && validGitRef(candidateRef, repository); const tree = sha && gitTree(sha, repository);
  if (!sha || !tree) throw new Error(`candidate-ref is not a resolvable commit ref: ${candidateRef}`);
  const worktrees = gitWorktrees(repository);
  const worktree = worktrees.find((entry) => entry.branch === candidateRef && entry.HEAD === sha)?.worktree;
  if (!worktree || !fs.existsSync(worktree) || fs.lstatSync(worktree).isSymbolicLink() || !fs.statSync(worktree).isDirectory()) throw new Error(`candidate-ref must be checked out in one exact non-symlink Git worktree: ${candidateRef}`);
  if (gitOutput(["status", "--porcelain=v1", "--untracked-files=all"], worktree)) throw new Error(`candidate-ref worktree must be clean: ${candidateRef}`);
  return { sha, tree, worktree: fs.realpathSync(worktree) };
}

function admissionCandidate(authority, candidateRef) {
  const candidate = candidateForRef(authority, candidateRef);
  const repository = gitRoot(authority.packageRoot);
  const canonical = validGitRef(`refs/heads/${authority.metadata.canonical_integration_branch}`, repository);
  const baseSha = canonical && gitOutput(["merge-base", candidate.sha, canonical], repository);
  const baseTree = baseSha && gitTree(baseSha, repository);
  if (!baseSha || !baseTree || !gitIsAncestor(baseSha, candidate.sha, repository)) throw new Error(`cannot bind admission base for ${candidateRef}`);
  return { ...candidate, baseSha, baseTree };
}

function assertTtl(ttlSeconds) {
  if (!Number.isSafeInteger(ttlSeconds) || ttlSeconds < 1 || ttlSeconds > 24 * 60 * 60) throw new Error("ttl-seconds must be an integer from 1 through 86400");
}

export function admitNode(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, holder, candidateRef, gateRunner, reviewers = [], ttlSeconds = 3600, now } = {}) {
  mutationAuthority(authority, runtimeRoot); assertTtl(ttlSeconds);
  const node = authority.nodes.find((candidateNode) => candidateNode.id === id);
  if (!node) throw new Error(`unknown node ${id}`);
  const candidate = admissionCandidate(authority, candidateRef);
  const admittedResult = withAdmissionLock(authority, runtimeRoot, (_root, leasesDirectory) => {
    // Capture production time only after acquiring the cross-process lock. A
    // waiter must not validate a winner's lease against a pre-lock timestamp.
    const effectiveNow = now ?? Date.now();
    const state = deriveState(authority, { runtimeRoot, now: effectiveNow });
    if (state.errors.length) throw new Error(`state invalid: ${state.errors.join("; ")}`);
    const row = state.states.get(id);
    const grandfatheredJ1Closeout = id === "J1" && node.initial_state === "IN_FLIGHT" && row?.status === "IN_FLIGHT" && !state.leases.some((lease) => lease.node_id === id);
    if (row?.status !== "READY" && !grandfatheredJ1Closeout) throw new Error(`${id} is not READY: ${row?.blockers?.join("; ") || row?.status || "missing"}`);
    const newId = leaseId(id, effectiveNow);
    const fields = leaseFields(authority, node, { holder, candidateRef, gateRunner, reviewers, ttlSeconds }, candidate, effectiveNow, newId);
    runtimeDirectory(authority, runtimeRoot, "gate-results", { create: true });
    runtimeDirectory(authority, runtimeRoot, "review-reports", { create: true });
    const file = path.join(leasesDirectory, `${newId}.toml`);
    atomicCreate(file, artifactText(fields));
    const admitted = deriveState(authority, { runtimeRoot, now: effectiveNow });
    if (admitted.errors.length || admitted.states.get(id)?.status !== "IN_FLIGHT") {
      fs.unlinkSync(file);
      throw new Error(`atomic admission postcondition failed: ${admitted.errors.join("; ") || admitted.states.get(id)?.status}`);
    }
    return { lease: admitted.allLeases.get(newId), state: admitted, file };
  });
  try { return { lease: admittedResult.lease, state: admittedResult.state, packet: packetFor(authority, admittedResult.state, id, { holder, leaseId: admittedResult.lease.lease_id }) }; }
  catch (error) {
    withAdmissionLock(authority, runtimeRoot, () => { if (fs.existsSync(admittedResult.file)) fs.unlinkSync(admittedResult.file); });
    throw new Error(`admission packet construction failed and lease was rolled back: ${error.message}`);
  }
}

export function renewLease(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), leaseId: requestedId, holder, ttlSeconds = 3600, now } = {}) {
  mutationAuthority(authority, runtimeRoot); assertTtl(ttlSeconds); assertIdentity(holder, "lease holder");
  return withAdmissionLock(authority, runtimeRoot, (_root, leasesDirectory) => {
    const effectiveNow = now ?? Date.now();
    const state = deriveState(authority, { runtimeRoot, now: effectiveNow });
    if (state.errors.length) throw new Error(`state invalid: ${state.errors.join("; ")}`);
    const prior = state.allLeases.get(requestedId);
    if (!prior || !state.leases.some((lease) => lease.lease_id === requestedId)) throw new Error(`lease is missing, expired, released, or superseded: ${requestedId}`);
    if (prior.holder !== holder) throw new Error(`lease holder mismatch ${requestedId}`);
    const node = authority.nodes.find((candidateNode) => candidateNode.id === prior.node_id);
    const newId = leaseId(prior.node_id, effectiveNow);
    const reviewerPairs = prior.reviewer_assignments;
    const fields = leaseFields(authority, node, {
      holder, candidateRef: prior.candidate_ref, gateRunner: prior.gate_runner, reviewers: reviewerPairs, ttlSeconds,
      gateResultPath: prior.gate_result_path, integrationGateResultPath: prior.integration_gate_result_path, reviewReportPaths: prior.review_report_paths,
    }, { sha: prior.candidate_sha, tree: prior.candidate_tree, worktree: prior.candidate_worktree, baseSha: prior.base_sha, baseTree: prior.base_tree }, effectiveNow, newId, reference(prior.lease_id, prior));
    const file = path.join(leasesDirectory, `${newId}.toml`); atomicCreate(file, artifactText(fields));
    const renewed = deriveState(authority, { runtimeRoot, now: effectiveNow });
    if (renewed.errors.length || !renewed.leases.some((lease) => lease.lease_id === newId) || renewed.leases.some((lease) => lease.lease_id === requestedId)) {
      fs.unlinkSync(file); throw new Error(`atomic lease renewal postcondition failed: ${renewed.errors.join("; ")}`);
    }
    return { lease: renewed.allLeases.get(newId), state: renewed };
  });
}

export function releaseLease(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), leaseId: requestedId, holder, now } = {}) {
  mutationAuthority(authority, runtimeRoot); assertIdentity(holder, "lease holder");
  return withAdmissionLock(authority, runtimeRoot, (root) => {
    const effectiveNow = now ?? Date.now();
    const state = deriveState(authority, { runtimeRoot, now: effectiveNow });
    if (state.errors.length) throw new Error(`state invalid: ${state.errors.join("; ")}`);
    const lease = state.allLeases.get(requestedId);
    if (!lease || !state.leases.some((row) => row.lease_id === requestedId)) throw new Error(`lease is missing, expired, released, or superseded: ${requestedId}`);
    if (lease.holder !== holder) throw new Error(`lease holder mismatch ${requestedId}`);
    const releases = runtimeDirectory(authority, runtimeRoot, "lease-releases", { create: true });
    const releaseId = `${requestedId}--release--${crypto.randomBytes(8).toString("hex")}`;
    const fields = { schema: 2, type: "lease-release", release_id: releaseId, lease_receipt: reference(lease.lease_id, lease), holder, released_at: new Date(effectiveNow).toISOString() };
    const file = path.join(releases, `${releaseId}.toml`); atomicCreate(file, artifactText(fields));
    const released = deriveState(authority, { runtimeRoot, now: effectiveNow });
    if (released.errors.length || released.leases.some((row) => row.lease_id === requestedId)) {
      fs.unlinkSync(file); throw new Error(`atomic lease release postcondition failed: ${released.errors.join("; ")}`);
    }
    return { release: { ...fields, digest: readToml(file).payload_sha256, file }, state: released, runtimeRoot: root };
  });
}

export function explainNode(authority, state, id) {
  const node = authority.nodes.find((candidate) => candidate.id === id);
  if (!node) throw new Error(`unknown node ${id}`);
  const row = state.states.get(id);
  return { id, name: node.name, status: row.status, blockers: row.blockers, predecessors: node.predecessors.map((pred) => ({ id: pred, status: state.states.get(pred)?.status })), conditional_predecessors: node.conditional_predecessors, external_requirements: node.external_requirements, conflict_domains: node.conflict_domains, resource_class: node.resource_class, gate_profile: node.gate_profile, review_profile: node.review_profile, activation_gate: node.activation_gate };
}

function extractProfile(file, id) {
  const model = readToml(file);
  return (model.profile || []).find((row) => row.id === id);
}

function markdownBlock(content, language = "text") {
  const longest = Math.max(2, ...[...content.matchAll(/`+/g)].map((match) => match[0].length));
  const fence = "`".repeat(longest + 1);
  return `${fence}${language}\n${content.endsWith("\n") ? content : `${content}\n`}${fence}`;
}

function programDagBlock(text, id) {
  const blocks = [...text.matchAll(/^\[\[block\]\]\n[\s\S]*?(?=^\[\[block\]\]|$(?![\s\S]))/gm)].map((match) => match[0].trimEnd());
  const indexed = blocks.map((block) => ({ block, id: /^id = "([A-Z][A-Z0-9-]*)"$/m.exec(block)?.[1] })).filter((row) => row.id);
  const exact = indexed.find((row) => row.id === id);
  if (exact) return exact;
  const parents = indexed.filter((row) => id.startsWith(row.id) && id.length > row.id.length).sort((left, right) => right.id.length - left.id.length || left.id.localeCompare(right.id));
  if (!parents.length || (parents[1] && parents[1].id.length === parents[0].id.length)) return null;
  return parents[0];
}

function relevantMarkdownExcerpt(content, node) {
  const matches = [...content.matchAll(/^#{1,2} .+$[\s\S]*?(?=^#{1,2} |$(?![\s\S]))/gm)].map((match) => match[0].trimEnd());
  if (!matches.length) return content.trimEnd();
  const risks = new Set([node.id, ...node.predecessors, ...node.conflict_domains]);
  const headingPattern = /(?:scope|contract|accept|invariant|verification|test|deletion|forbidden|review|risk|owner|dependency|predecessor|authority|ruling|decision|outcome|abort)/i;
  const selected = matches.filter((section, index) => index === 0 || headingPattern.test(section.split("\n", 1)[0]) || [...risks].some((risk) => new RegExp(`\\b${risk.replaceAll(/[-/\\^$*+?.()|[\]{}]/g, "\\$&")}\\b`).test(section)));
  return selected.join("\n\n");
}

export function packetSourceBindings(authority, node) {
  const sections = [];
  const sourceLock = readToml(confinedFile(authority.packageRoot, "provenance/source-lock.toml", "source lock"));
  const sourceRows = new Map((sourceLock.source || []).map((row) => [path.basename(row.path), row]));
  const liveLock = readToml(confinedFile(authority.packageRoot, "provenance/live-source-lock.toml", "live source lock"));
  const liveRows = new Map((liveLock.source || []).map((row) => [row.ref, row]));
  const repository = gitRoot(authority.packageRoot);
  for (const ref of node.source_refs) {
    if (ref.startsWith("source:")) {
      const match = /^source:([A-Za-z0-9._-]+):L(\d+)$/.exec(ref);
      const locked = match && sourceRows.get(match[1]);
      if (!match || !locked) throw new Error(`dispatch packet source lock missing ${ref}`);
      const file = confinedFile(authority.packageRoot, locked.path, `packet source ${ref}`);
      const line = fs.readFileSync(file, "utf8").split("\n")[Number(match[2]) - 1];
      if (line === undefined) throw new Error(`dispatch packet source line missing ${ref}`);
      sections.push(`### ${ref}\n\nLocked source: \`${locked.path}\`, ${locked.bytes} bytes, SHA-256 \`${locked.sha256}\`. Excerpt SHA-256 \`${sha256(`${line.trimEnd()}\n`)}\`.\n\n${markdownBlock(line, "markdown")}`);
    } else if (ref.startsWith("live:")) {
      const locked = liveRows.get(ref);
      if (!locked || !repository) throw new Error(`dispatch packet live lock missing ${ref}`);
      const contentBytes = locked.commit ? gitPathAt(locked.commit, locked.path, repository) : fs.readFileSync(confinedFile(repository, locked.path, `packet live source ${ref}`));
      if (!contentBytes) throw new Error(`dispatch packet live Git object missing ${locked.commit}:${locked.path}`);
      const content = contentBytes.toString("utf8");
      if (locked.path === "docs/arch/refactor/rev11/program-dag.toml" || locked.path === "docs/arch/architecture-lock/ledger/program-state.toml") {
        const selected = programDagBlock(content, node.id);
        if (!selected) throw new Error(`dispatch packet cannot identify live split-parent block for ${node.id}`);
        sections.push(`### ${ref} — block ${selected.id}\n\nLocked file: ${locked.bytes} bytes, SHA-256 \`${locked.sha256}\`. Exact block SHA-256 \`${sha256(`${selected.block}\n`)}\`.\n\n${markdownBlock(selected.block, "toml")}`);
      } else {
        const excerpt = relevantMarkdownExcerpt(content, node);
        sections.push(`### ${ref}\n\nLocked full file: ${locked.bytes} bytes, SHA-256 \`${locked.sha256}\`. Node/risk-relevant excerpt SHA-256 \`${sha256(`${excerpt}\n`)}\`.\n\n${markdownBlock(excerpt, path.extname(locked.path).slice(1) || "text")}`);
      }
    }
  }
  const authorizationManifest = readToml(confinedFile(authority.packageRoot, "authority/state/external-authorizations.toml", "packet external authorization manifest"));
  for (const row of (authorizationManifest.authorization || []).filter((candidate) => candidate.node_id === node.id && node.external_requirements.includes(candidate.authorization))) {
    const ratification = fs.readFileSync(confinedFile(authority.packageRoot, row.ratification_path, `${node.id} packet ratification`));
    if (sha256(ratification) !== row.ratification_receipt_sha256) throw new Error(`${node.id} packet ratification digest mismatch ${row.authorization}`);
    sections.push(`### External custody slot ${node.id}:${row.authorization}\n\nGrant mode \`${row.grant_mode}\`; grantor \`${row.granted_by}\`; scope \`${row.directive_scope}\`; exact receipt \`${row.ratification_path}\`, SHA-256 \`${row.ratification_receipt_sha256}\`.\n\n${markdownBlock(ratification.toString("utf8"), "markdown")}`);
  }
  const coverage = readToml(confinedFile(authority.packageRoot, "provenance/source-coverage.toml", "source coverage")).requirement || [];
  const atoms = coverage.filter((row) => row.applicable_nodes?.includes(node.id)).sort((left, right) => left.id.localeCompare(right.id));
  const atomBinding = sha256(`${JSON.stringify(atoms.map((row) => ({ id: row.id, kind: row.kind, source: row.source, from_line: row.from_line, to_line: row.to_line, target: row.target, applicable_nodes: row.applicable_nodes, text_sha256: row.text_sha256 })))}\n`);
  const attachmentRelative = `provenance/packet-source-clauses/${node.id}.md`;
  const attachment = fs.readFileSync(confinedFile(authority.packageRoot, attachmentRelative, `${node.id} source-clause attachment`));
  sections.push(`### Exact applicable operative clauses for ${node.id} — embedded node-scoped attachment\n\nThe complete ${atoms.length}-clause applicable set is embedded verbatim below from the authority-digest-bound node-only attachment \`${attachmentRelative}\` (${attachment.length} bytes, SHA-256 \`${sha256(attachment)}\`). Its canonical atom subset digest is \`${atomBinding}\`. The persisted dispatch packet therefore remains cold-executable without resolving the whole source coverage ledger or any proposal file.\n\n${markdownBlock(attachment.toString("utf8"), "markdown")}`);
  return sections.join("\n\n");
}

function packetContracts(authority, node) {
  const directory = path.join(authority.packageRoot, "contracts");
  const relevant = new Set(["acceptance.md", "dag.md", "integration.md", "leases.md", "orchestration.md", "receipts.md", "reviews.md", "sizing.md"]);
  if (node.class === "governance" || node.train.startsWith("governance.")) relevant.add("amendments.md");
  if (node.train.includes("compiler") || node.product.includes("compiler")) relevant.add("compiler-architecture.md");
  return fs.readdirSync(directory).filter((name) => relevant.has(name)).sort().map((name) => {
    const content = fs.readFileSync(confinedFile(directory, name, `dispatch contract ${name}`), "utf8");
    const operative = content.split(/^## Transferred source requirement atoms$/m)[0].trimEnd();
    return `### contracts/${name} — full-file SHA-256 \`${sha256(content)}\`\n\nDigest-bound operative excerpt (the exact node-targeted requirement atoms appear in the source bindings below):\n\n${markdownBlock(operative, "markdown")}`;
  }).join("\n\n");
}

function packetLease(state, id, options) {
  const lease = state.allLeases?.get(options?.leaseId);
  if (!lease || !state.leases.some((row) => row.lease_id === lease.lease_id) || lease.node_id !== id || lease.holder !== options?.holder) throw new Error(`${id} dispatch requires its exact active lease id and holder`);
  return lease;
}

export function packetFor(authority, state, id, options = {}) {
  const node = authority.nodes.find((candidate) => candidate.id === id);
  if (!node) throw new Error(`unknown node ${id}`);
  const row = state.states.get(id);
  const lease = packetLease(state, id, options);
  const charter = fs.readFileSync(confinedFile(authority.packageRoot, node.charter, `${id} charter`), "utf8");
  const operativeCharter = charter.split(/^## Transferred source requirement atoms$/m)[0].trimEnd();
  const predecessorSummaries = node.predecessors.map((pred) => {
    const receipt = state.receipts.get(pred);
    return receipt ? `- ${pred}: ACCEPTED integration=${receipt.integration_sha} tree=${receipt.integration_tree} receipt=${receipt.digest}` : `- ${pred}: ${state.states.get(pred)?.status || "MISSING"}`;
  }).join("\n") || "- none";
  const boundProfiles = evidenceProfiles(node);
  const gate = extractProfile(path.join(authority.packageRoot, "catalogs/gate-profiles.toml"), boundProfiles.gate);
  const review = extractProfile(path.join(authority.packageRoot, "catalogs/review-profiles.toml"), boundProfiles.review);
  const domains = catalogMap(authority.packageRoot, "conflict-domains.toml", "domain");
  const baseSha = lease.base_sha;
  const baseTree = lease.base_tree;
  const commandList = targetedGateCommands(authority, node);
  const integrationCommands = gate.final;
  const domainText = node.conflict_domains.map((domainId) => {
    const domain = domains.get(domainId);
    if (!domain) throw new Error(`dispatch packet missing conflict domain ${domainId}`);
    return `- \`${domainId}\`: paths=${JSON.stringify(domain.path_roots)} symbols=${JSON.stringify(domain.symbols)}`;
  }).join("\n");
  const reviewerText = lease.reviewer_assignments.map((assignment) => {
    const split = assignment.indexOf("="); const lens = assignment.slice(0, split); const reviewer = assignment.slice(split + 1);
    const destination = lease.review_report_paths.find((value) => value.startsWith(`${lens}=`)).slice(lens.length + 1);
    return `- lens=\`${lens}\` reviewer=\`${reviewer}\` provider/model=\`bound by the fresh harness task record\` minimum-effort=\`${review.minimum_effort}\` effective-effort=\`${assessNodeEffort(node).review}\` report=\`${path.join(state.runtimeRoot, destination)}\``;
  }).join("\n");
  const leaseText = fs.readFileSync(lease.file, "utf8");
  return `# Dispatch packet — ${id}

Derived status: **${row.status}**${row.blockers.length ? ` — ${row.blockers.join("; ")}` : ""}. Lifecycle phase: **${state.phase}**.

## Candidate and immutable admission

- Base: SHA \`${baseSha}\`, tree \`${baseTree}\`, canonical branch \`${authority.metadata.canonical_integration_branch}\`.
- Candidate start: ref \`${lease.candidate_ref}\`, SHA \`${lease.candidate_sha}\`, tree \`${lease.candidate_tree}\`. Later implementation commits require one immutable \`candidate-finalize\` receipt before evidence or acceptance.
- Worktree: \`${lease.candidate_worktree}\`.
- Authority/control digest: \`${state.authorityDigest}\`; charter digest: \`${sha256(charter)}\`.
- Scope roots: ${JSON.stringify(lease.scope_path_roots)}; symbols: ${JSON.stringify(lease.scope_symbols)}.
- Lease: \`${reference(lease.lease_id, lease)}\`; holder \`${lease.holder}\`; epoch ${lease.acquired_at} through ${lease.expires_at}.

${markdownBlock(leaseText, "toml")}

## Direct predecessor receipt summaries

${predecessorSummaries}

## Exact conflict-domain authority

${domainText}

Resource class \`${node.resource_class}\`; capacity is checked atomically at admission.

## Exact gate plan

- Candidate gate profile: \`${boundProfiles.gate}\`; runner: \`${lease.gate_runner}\`; result: \`${path.join(state.runtimeRoot, lease.gate_result_path)}\`.
${commandList.map((command, index) => `${index + 1}. \`${command}\``).join("\n")}

- Cross-block integration gate for the same conflict domains; runner: \`${lease.gate_runner}\`; result: \`${path.join(state.runtimeRoot, lease.integration_gate_result_path)}\`.
${integrationCommands.map((command, index) => `${index + 1}. \`${command}\``).join("\n")}

## Exact independent-review plan

- Profile: \`${boundProfiles.review}\`; independent=${review.independent}; required=${review.reviewers}.
${reviewerText}

## Required report-back schema

Report exact candidate/base SHA and tree, authority and charter digests, lease receipt, changed paths, production/deletion/migration counts, the preflight proof selection and terse rationale, TDD RED/GREEN commands and outputs for behavioral code changes, every selected existing/type/compiler/static/gate/inspection/benchmark result, every gate command/result/digest, every review report/digest, residual finding fingerprints, and every abort/rescope decision. Acceptance refuses changed paths outside the leased roots, evidence outside the lease epoch, unexpected skips, non-PASS gates/reviews, post-review tree changes, or renamed reviewer/runner/destination identities.

## Exact source and live authority inputs

${packetSourceBindings(authority, node)}

## Binding contract text

${packetContracts(authority, node)}

## Current charter — SHA-256 \`${sha256(charter)}\`

${markdownBlock(operativeCharter, "markdown")}
`;
}

export function assertDispatchable(authority, state, id, options = {}) {
  if (state.errors.length) throw new Error(`state invalid: ${state.errors.join("; ")}`);
  const row = state.states.get(id);
  if (!row) throw new Error(`unknown node ${id}`);
  const lease = packetLease(state, id, options);
  if (row.status !== "IN_FLIGHT" || row.lease?.lease_id !== lease.lease_id) throw new Error(`${id} is not admitted IN_FLIGHT under lease ${lease.lease_id}: ${row.blockers.join("; ") || row.status}`);
  return packetFor(authority, state, id, options);
}

export function dispatchNode(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, holder, leaseId: requestedLeaseId, now } = {}) {
  mutationAuthority(authority, runtimeRoot); assertIdentity(holder, "dispatch identity");
  return withAdmissionLock(authority, runtimeRoot, () => {
    const effectiveNow = now ?? Date.now();
    const current = deriveState(authority, { runtimeRoot, now: effectiveNow });
    const packet = assertDispatchable(authority, current, id, { holder, leaseId: requestedLeaseId });
    const lease = current.allLeases.get(requestedLeaseId);
    const dispatchId = `${requestedLeaseId}--dispatch`;
    const packets = runtimeDirectory(authority, runtimeRoot, "dispatch-packets", { create: true });
    const dispatches = runtimeDirectory(authority, runtimeRoot, "dispatches", { create: true });
    const packetRelative = `dispatch-packets/${dispatchId}.md`;
    const packetFile = path.join(packets, `${dispatchId}.md`);
    const receiptFile = path.join(dispatches, `${dispatchId}.toml`);
    const node = authority.nodes.find((row) => row.id === id);
    const fields = {
      schema: 2, type: "dispatch-receipt", dispatch_id: dispatchId, node_id: id,
      lease_receipt: reference(lease.lease_id, lease), base_sha: lease.base_sha, base_tree: lease.base_tree,
      candidate_start_sha: lease.candidate_sha, candidate_start_tree: lease.candidate_tree,
      candidate_ref: lease.candidate_ref, candidate_worktree: lease.candidate_worktree,
      authority_sha256: lease.authority_sha256,
      charter_sha256: sha256(fs.readFileSync(confinedFile(authority.packageRoot, node.charter, `${id} dispatch charter`))),
      conflict_domains: lease.conflict_domains, scope_path_roots: lease.scope_path_roots, scope_symbols: lease.scope_symbols,
      packet_path: packetRelative, packet_sha256: sha256(packet), dispatched_at: new Date(effectiveNow).toISOString(), dispatched_by: holder,
    };
    atomicCreate(packetFile, packet);
    try { atomicCreate(receiptFile, artifactText(fields)); }
    catch (error) { if (fs.existsSync(packetFile)) fs.unlinkSync(packetFile); throw error; }
    const after = deriveState(authority, { runtimeRoot, now: effectiveNow });
    const dispatch = after.dispatches.get(dispatchId);
    if (after.errors.length || !dispatch) {
      if (fs.existsSync(receiptFile)) fs.unlinkSync(receiptFile);
      if (fs.existsSync(packetFile)) fs.unlinkSync(packetFile);
      throw new Error(`dispatch postcondition failed: ${after.errors.join("; ") || dispatchId}`);
    }
    return { packet, dispatch, state: after };
  });
}

export function finalizeCandidate(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, holder, leaseId: requestedLeaseId, now } = {}) {
  mutationAuthority(authority, runtimeRoot); assertIdentity(holder, "finalization identity");
  return withAdmissionLock(authority, runtimeRoot, () => {
    const effectiveNow = now ?? Date.now();
    const current = deriveState(authority, { runtimeRoot, now: effectiveNow });
    const lease = packetLease(current, id, { holder, leaseId: requestedLeaseId });
    if (current.states.get(id)?.status !== "IN_FLIGHT") throw new Error(`${id} candidate finalization requires an active admitted lease`);
    const dispatch = [...current.dispatches.values()].find((row) => row.node_id === id && row.lease_receipt === reference(lease.lease_id, lease));
    if (!dispatch) throw new Error(`${id} candidate finalization requires its immutable dispatch receipt`);
    const candidate = candidateForRef(authority, lease.candidate_ref);
    const repository = gitRoot(authority.packageRoot);
    if (candidate.worktree !== lease.candidate_worktree || !gitIsAncestor(lease.candidate_sha, candidate.sha, repository)) throw new Error(`${id} final candidate does not descend from the admitted start in the exact worktree`);
    const changedPaths = gitChangedPaths(lease.base_sha, candidate.sha, repository);
    if (!changedPaths?.length) throw new Error(`${id} final candidate has an empty or unavailable base delta`);
    for (const changedPath of changedPaths) if (!lease.scope_path_roots.some((root) => changedPath === root || changedPath.startsWith(`${root}/`))) throw new Error(`${id} final candidate changed path is outside admission scope: ${changedPath}`);
    const finalizationId = `${requestedLeaseId}--final`;
    const fields = {
      schema: 2, type: "candidate-finalization", finalization_id: finalizationId, node_id: id,
      lease_receipt: reference(lease.lease_id, lease), dispatch_receipt: reference(dispatch.dispatch_id, dispatch),
      base_sha: lease.base_sha, base_tree: lease.base_tree,
      candidate_start_sha: lease.candidate_sha, candidate_start_tree: lease.candidate_tree,
      candidate_ref: lease.candidate_ref, candidate_sha: candidate.sha, candidate_tree: candidate.tree,
      candidate_worktree: lease.candidate_worktree, authority_sha256: lease.authority_sha256,
      changed_paths: changedPaths, finalized_at: new Date(effectiveNow).toISOString(), finalized_by: holder,
    };
    const directory = runtimeDirectory(authority, runtimeRoot, "finalizations", { create: true });
    const file = path.join(directory, `${finalizationId}.toml`); atomicCreate(file, artifactText(fields));
    const after = deriveState(authority, { runtimeRoot, now: effectiveNow });
    const finalization = after.finalizations.get(finalizationId);
    if (after.errors.length || !finalization) {
      if (fs.existsSync(file)) fs.unlinkSync(file);
      throw new Error(`candidate finalization postcondition failed: ${after.errors.join("; ") || finalizationId}`);
    }
    return { finalization, state: after };
  });
}

function commandArgv(command) {
  if (typeof command !== "string" || !command.trim() || /[\0\r\n;&|<>`$]/.test(command)) throw new Error(`gate command is not a safe direct-exec command: ${command}`);
  const argv = command.trim().split(/\s+/);
  if (!/^[A-Za-z0-9._/-]+$/.test(argv[0])) throw new Error(`gate command executable is unsafe: ${argv[0]}`);
  return argv;
}

export function executeCommandPlan(commands, { cwd, timeoutMs = 4 * 60 * 60 * 1000 } = {}) {
  if (!Array.isArray(commands) || !commands.length) throw new Error("gate command plan must be nonempty");
  const results = [];
  for (const command of commands) {
    const argv = commandArgv(command);
    const started = Date.now();
    const child = childProcess.spawnSync(argv[0], argv.slice(1), { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024, timeout: timeoutMs, killSignal: "SIGKILL" });
    const stdout = child.stdout || ""; const stderr = child.stderr || "";
    const timedOut = child.error?.code === "ETIMEDOUT";
    results.push({ command, argv, status: child.status ?? -1, signal: child.signal || "", timed_out: timedOut, elapsed_ms: Date.now() - started, stdout, stderr, stdout_sha256: sha256(stdout), stderr_sha256: sha256(stderr) });
    if (child.error || child.status !== 0 || child.signal) throw new Error(`gate command failed: ${command}; status=${child.status}; signal=${child.signal || ""}; error=${child.error?.message || ""}`);
  }
  return results;
}

export function runGateEvidence(authority, { runtimeRoot = defaultRuntimeRoot(authority.packageRoot), id, scope, holder, leaseId: requestedLeaseId, integrationSha, timeoutMs } = {}) {
  mutationAuthority(authority, runtimeRoot); assertIdentity(holder, "gate custody holder");
  if (!['candidate', 'integration'].includes(scope)) throw new Error("gate scope must be candidate or integration");
  const before = deriveState(authority, { runtimeRoot });
  if (before.errors.length) throw new Error(`state invalid before gate-run: ${before.errors.join("; ")}`);
  const lease = before.allLeases.get(requestedLeaseId);
  if (!lease || lease.node_id !== id || lease.holder !== holder) throw new Error("gate-run requires the exact lease id and holder");
  const finalization = [...before.finalizations.values()].find((row) => row.node_id === id && row.lease_receipt === reference(lease.lease_id, lease));
  if (!finalization) throw new Error("gate-run requires the exact validated candidate-finalize receipt");
  const node = authority.nodes.find((row) => row.id === id); const repository = gitRoot(authority.packageRoot);
  const integrationTree = repository && gitTree(integrationSha, repository);
  if (scope === "candidate") {
    if (integrationSha !== finalization.candidate_sha || integrationTree !== finalization.candidate_tree) throw new Error("candidate gate-run must bind its execution identity to the exact finalized candidate");
  } else {
    if (!integrationTree || !gitIsAncestor(finalization.candidate_sha, integrationSha, repository)) throw new Error("integration gate-run identity must contain the finalized candidate");
    const canonical = validGitRef(`refs/heads/${authority.metadata.canonical_integration_branch}`, repository);
    if (canonical !== integrationSha) throw new Error("integration gate-run requires the exact canonical integration branch head");
  }
  for (const requirement of node.external_requirements) {
    const authorization = before.authorizations.get(`${id}:${requirement}`);
    if (!authorization || authorization.candidate_sha !== finalization.candidate_sha || authorization.candidate_tree !== finalization.candidate_tree) throw new Error(`gate-run requires finalized-candidate authorization ${requirement}`);
  }
  const profile = catalogMap(authority.packageRoot, "gate-profiles.toml", "profile").get(evidenceProfiles(node).gate);
  const commands = scope === "candidate" ? targetedGateCommands(authority, node) : profile.final;
  const startedAt = new Date().toISOString();
  const results = executeCommandPlan(commands, { cwd: scope === "candidate" ? finalization.candidate_worktree : repository, timeoutMs });
  const completedAt = new Date().toISOString();
  const resultPath = scope === "candidate" ? lease.gate_result_path : lease.integration_gate_result_path;
  const result = {
    schema: 1, type: "gate-execution-result", execution_custody: "programctl-gate-run/v1", node_id: id, scope,
    candidate_sha: finalization.candidate_sha, candidate_tree: finalization.candidate_tree,
    integration_sha: integrationSha, integration_tree: integrationTree, executed_by: lease.gate_runner,
    commands, started_at: startedAt, completed_at: completedAt, terminal_summary: "PASS", unexpected_skips: 0, results,
  };
  const resultText = `${JSON.stringify(result, null, 2)}\n`;
  const evidenceId = `${finalization.finalization_id}--${scope}-gate`;
  const fields = {
    schema: 2, type: "gate-evidence", execution_custody: "programctl-gate-run/v1", evidence_id: evidenceId, node_id: id,
    gate_profile: evidenceProfiles(node).gate, scope, candidate_sha: finalization.candidate_sha, candidate_tree: finalization.candidate_tree,
    integration_sha: integrationSha, integration_tree: integrationTree, commands, executed_work: commands, unexpected_skips: 0,
    terminal_summary: "PASS", result_path: resultPath, result_sha256: sha256(resultText), started_at: startedAt, completed_at: completedAt,
    executed_by: lease.gate_runner,
  };
  const resultName = path.posix.basename(resultPath);
  if (resultPath !== `gate-results/${resultName}`) throw new Error("gate-run result destination is not the exact immutable lease-owned gate-results path");
  const resultFile = path.join(runtimeDirectory(authority, runtimeRoot, "gate-results", { create: true }), resultName);
  const evidenceDirectory = runtimeDirectory(authority, runtimeRoot, "gates", { create: true });
  const evidenceFile = path.join(evidenceDirectory, `${evidenceId}.toml`);
  atomicCreate(resultFile, resultText);
  try { atomicCreate(evidenceFile, artifactText(fields)); }
  catch (error) { if (fs.existsSync(resultFile)) fs.unlinkSync(resultFile); throw error; }
  try {
    const after = deriveState(authority, { runtimeRoot }); const evidence = after.gates.get(evidenceId);
    if (after.errors.length || !evidence) throw new Error(`gate-run postcondition failed: ${after.errors.join("; ") || "evidence absent"}`);
    return { evidence, state: after };
  } catch (error) {
    if (fs.existsSync(evidenceFile)) fs.unlinkSync(evidenceFile);
    if (fs.existsSync(resultFile)) fs.unlinkSync(resultFile);
    throw error;
  }
}

export function runReviewEvidence() {
  throw new Error("review-run is retired and audit-only under trusted-local ORC0; use fresh harness tasks plus harness-record");
}

export function metrics(authority) {
  return JSON.parse(generatedFiles(authority).get("generated/METRICS.json"));
}

export function digestPayload(textWithoutDigest) {
  return sha256(textWithoutDigest.endsWith("\n") ? textWithoutDigest : `${textWithoutDigest}\n`);
}
