import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const AUTHORITY_DIGEST_ROOTS = ["authority", "catalogs", "charters", "contracts", "provenance", "schemas", "sources", "state", "templates", "tools"];
const LOCK_RELATIVE = "authority/state/authority-lock.toml";
const AMENDMENTS_RELATIVE = "authority/state/amendments";
const WRITE_LOCK_NAME = ".amendment-write.lock";
const SHA256 = /^[0-9a-f]{64}$/;
const AMENDMENT_ID = /^AMD-[A-Z0-9-]+$/;

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function isInside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
}

function safeRelative(relative, label) {
  if (typeof relative !== "string" || relative.length === 0 || /[\0-\x1f\x7f\\]/u.test(relative)) throw new Error(`${label}: invalid relative path`);
  if (path.posix.isAbsolute(relative) || relative.split("/").some((part) => part === "" || part === "." || part === "..")) throw new Error(`${label}: path escapes its authority root`);
  return relative;
}

function safeEntry(packageRoot, relative, { type = "file", optional = false } = {}) {
  safeRelative(relative, relative);
  const root = fs.realpathSync(packageRoot);
  let current = root;
  for (const segment of relative.split("/")) {
    current = path.join(current, segment);
    if (!fs.existsSync(current)) {
      if (optional) return null;
      throw new Error(`${relative}: missing`);
    }
    const stat = fs.lstatSync(current);
    if (stat.isSymbolicLink()) throw new Error(`${relative}: symbolic links are forbidden`);
  }
  const resolved = fs.realpathSync(current);
  if (!isInside(root, resolved)) throw new Error(`${relative}: resolves outside the authority package`);
  const stat = fs.statSync(resolved);
  if (type === "file" && !stat.isFile()) throw new Error(`${relative}: expected a regular file`);
  if (type === "directory" && !stat.isDirectory()) throw new Error(`${relative}: expected a directory`);
  return resolved;
}

function ensureDirectory(packageRoot, relative) {
  const root = fs.realpathSync(packageRoot);
  let current = root;
  for (const segment of safeRelative(relative, relative).split("/")) {
    current = path.join(current, segment);
    if (!fs.existsSync(current)) fs.mkdirSync(current, { mode: 0o700 });
    const stat = fs.lstatSync(current);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`${relative}: directory chain must contain only real directories`);
    if (!isInside(root, fs.realpathSync(current))) throw new Error(`${relative}: directory resolves outside the authority package`);
  }
  return current;
}

function parseValue(raw, location) {
  const value = raw.trim();
  if (value.startsWith('"') || value.startsWith("[")) {
    try { return JSON.parse(value); }
    catch (error) { throw new Error(`${location}: malformed string or array: ${error.message}`); }
  }
  if (/^-?(?:0|[1-9][0-9]*)$/u.test(value)) {
    const number = Number(value);
    if (!Number.isSafeInteger(number)) throw new Error(`${location}: integer is outside the safe range`);
    return number;
  }
  if (value === "true") return true;
  if (value === "false") return false;
  throw new Error(`${location}: unsupported TOML value`);
}

function parseFlatToml(text, location) {
  if (typeof text !== "string" || text.includes("\0")) throw new Error(`${location}: invalid text`);
  const result = Object.create(null);
  const lines = text.endsWith("\n") ? text.slice(0, -1).split("\n") : text.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line.length === 0) throw new Error(`${location}:${index + 1}: blank lines are forbidden`);
    if (/^[#[]/u.test(line)) throw new Error(`${location}:${index + 1}: comments and tables are forbidden`);
    const match = /^([A-Za-z][A-Za-z0-9_]*) = (.+)$/u.exec(line);
    if (!match) throw new Error(`${location}:${index + 1}: malformed field`);
    const key = match[1];
    if (["__proto__", "prototype", "constructor"].includes(key)) throw new Error(`${location}:${index + 1}: unsafe key`);
    if (Object.hasOwn(result, key)) throw new Error(`${location}:${index + 1}: duplicate field ${key}`);
    result[key] = parseValue(match[2], `${location}.${key}`);
  }
  return result;
}

function parseTrustedRatifications(text, location) {
  if (typeof text !== "string" || text.includes("\0")) throw new Error(`${location}: invalid text`);
  const root = Object.create(null);
  const slots = [];
  let current = null;
  const lines = text.endsWith("\n") ? text.slice(0, -1).split("\n") : text.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (line === "") continue;
    if (line === "[[slot]]") {
      if (Object.hasOwn(root, "slot")) throw new Error(`${location}:${index + 1}: slot cannot use both inline and table forms`);
      current = Object.create(null);
      slots.push(current);
      continue;
    }
    if (/^[#[]/u.test(line)) throw new Error(`${location}:${index + 1}: comments and unknown tables are forbidden`);
    const match = /^([A-Za-z][A-Za-z0-9_]*) = (.+)$/u.exec(line);
    if (!match) throw new Error(`${location}:${index + 1}: malformed field`);
    const key = match[1];
    if (["__proto__", "prototype", "constructor"].includes(key)) throw new Error(`${location}:${index + 1}: unsafe key`);
    const target = current || root;
    if (Object.hasOwn(target, key)) throw new Error(`${location}:${index + 1}: duplicate field ${key}`);
    target[key] = parseValue(match[2], `${location}.${key}`);
  }
  if (slots.length) root.slot = slots;
  if (!Object.hasOwn(root, "slot")) root.slot = [];
  return root;
}

function valueType(value) {
  if (Array.isArray(value)) return "array";
  if (value === null) return "null";
  if (Number.isInteger(value)) return "integer";
  return typeof value;
}

function schemaErrors(value, schema, location) {
  const errors = [];
  if (schema.const !== undefined && value !== schema.const) errors.push(`${location}: expected constant ${JSON.stringify(schema.const)}`);
  if (schema.enum && !schema.enum.includes(value)) errors.push(`${location}: value is not in the permitted enum`);
  if (schema.type && valueType(value) !== schema.type && !(schema.type === "number" && typeof value === "number")) {
    errors.push(`${location}: expected ${schema.type}`);
    return errors;
  }
  if (schema.type === "object") {
    const properties = schema.properties || {};
    for (const key of schema.required || []) if (!Object.hasOwn(value, key)) errors.push(`${location}: missing required property ${key}`);
    if (schema.additionalProperties === false) {
      for (const key of Object.keys(value)) if (!Object.hasOwn(properties, key)) errors.push(`${location}: additional property ${key}`);
    }
    for (const [key, child] of Object.entries(properties)) if (Object.hasOwn(value, key)) errors.push(...schemaErrors(value[key], child, `${location}.${key}`));
  } else if (schema.type === "array") {
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${location}: requires at least ${schema.minItems} items`);
    if (schema.maxItems !== undefined && value.length > schema.maxItems) errors.push(`${location}: permits at most ${schema.maxItems} items`);
    if (schema.uniqueItems && new Set(value.map((item) => JSON.stringify(item))).size !== value.length) errors.push(`${location}: items must be unique`);
    if (schema.items) value.forEach((item, index) => errors.push(...schemaErrors(item, schema.items, `${location}[${index}]`)));
  } else if (schema.type === "string") {
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${location}: string is shorter than ${schema.minLength}`);
    if (schema.pattern && !new RegExp(schema.pattern, "u").test(value)) errors.push(`${location}: string does not match ${schema.pattern}`);
  }
  return errors;
}

function loadSchema(packageRoot, name) {
  const file = safeEntry(packageRoot, `schemas/${name}`);
  try { return JSON.parse(fs.readFileSync(file, "utf8")); }
  catch (error) { throw new Error(`schemas/${name}: ${error.message}`); }
}

function validateObject(value, schema, location, validateSchemaObject) {
  const errors = schemaErrors(value, schema, location);
  if (validateSchemaObject) {
    for (const error of validateSchemaObject(value, schema, location)) if (!errors.includes(error)) errors.push(error);
  }
  return errors;
}

function payload(text, location) {
  const markers = [...text.matchAll(/^payload_sha256\s*=/gmu)];
  if (markers.length !== 1) return { errors: [`${location}: payload_sha256 must be the single final field`] };
  const index = markers[0].index;
  const final = /^payload_sha256 = "([0-9a-f]{64})"\n?$/u.exec(text.slice(index));
  if (!final) return { errors: [`${location}: payload_sha256 must be the single final field`] };
  const prefix = text.slice(0, index);
  const expected = sha256(prefix);
  const errors = final[1] === expected ? [] : [`${location}: payload digest mismatch`];
  return { digest: final[1], prefix, errors };
}

function parseAmendmentText(text, packageRoot, location, dependencies) {
  const errors = [];
  const framed = payload(text, location);
  errors.push(...framed.errors);
  let row = null;
  try { row = parseFlatToml(text, location); }
  catch (error) { errors.push(error.message); }
  if (row) {
    try { errors.push(...validateObject(row, loadSchema(packageRoot, "amendment.schema.json"), location, dependencies.validateSchemaObject)); }
    catch (error) { errors.push(error.message); }
    if (framed.digest && row.payload_sha256 !== framed.digest) errors.push(`${location}: parsed payload digest differs from the final field`);
  }
  return { row, digest: framed.digest || null, errors };
}

function parseLockFile(packageRoot, dependencies) {
  const lockFile = safeEntry(packageRoot, LOCK_RELATIVE, { optional: true });
  if (!lockFile) return { lockFile: null, lock: null, errors: [] };
  const errors = [];
  let lock = null;
  try { lock = parseFlatToml(fs.readFileSync(lockFile, "utf8"), LOCK_RELATIVE); }
  catch (error) { errors.push(error.message); }
  if (lock) {
    try { errors.push(...validateObject(lock, loadSchema(packageRoot, "authority-lock.schema.json"), "authority-lock", dependencies.validateSchemaObject)); }
    catch (error) { errors.push(error.message); }
  }
  return { lockFile, lock, errors };
}

function trustedSlotKey(purpose, ratifiedBy, receiptSha256) {
  return `${purpose}\0${ratifiedBy}\0${receiptSha256}`;
}

function loadTrustedRatifications(packageRoot, dependencies) {
  const location = "authority/state/trusted-ratifications.toml";
  const errors = [];
  const byKey = new Map();
  let ledger = null;
  try {
    const file = safeEntry(packageRoot, location);
    ledger = parseTrustedRatifications(fs.readFileSync(file, "utf8"), location);
    errors.push(...validateObject(ledger, loadSchema(packageRoot, "trusted-ratifications.schema.json"), "trusted-ratifications", dependencies.validateSchemaObject));
  } catch (error) {
    errors.push(error.message);
    return { ledger, byKey, errors };
  }
  const usedPaths = new Set();
  for (const [index, row] of ledger.slot.entries()) {
    const rowLocation = `trusted-ratifications.slot[${index}]`;
    const key = trustedSlotKey(row.purpose, row.ratified_by, row.receipt_sha256);
    if (byKey.has(key)) errors.push(`${rowLocation}: duplicate trusted ratification slot`);
    if (typeof row.ratified_by !== "string" || row.ratified_by.length === 0 || row.ratified_by !== row.ratified_by.trim() || /[\0-\x1f\x7f]/u.test(row.ratified_by)) errors.push(`${rowLocation}: ratified_by must be a normalized identity`);
    if (!SHA256.test(row.receipt_sha256 || "") || /^0{64}$/u.test(row.receipt_sha256)) errors.push(`${rowLocation}: receipt_sha256 must be a non-placeholder SHA-256`);
    if (usedPaths.has(row.receipt_path)) errors.push(`${rowLocation}: receipt_path is used by more than one trusted slot`);
    usedPaths.add(row.receipt_path);
    try {
      if (typeof row.receipt_path !== "string" || !/^authority\/state\/ratification-receipts\/[A-Za-z0-9][A-Za-z0-9._-]*$/u.test(row.receipt_path)) throw new Error("receipt_path is outside the trusted receipt directory");
      const receiptFile = safeEntry(packageRoot, row.receipt_path);
      const actual = sha256(fs.readFileSync(receiptFile));
      if (actual !== row.receipt_sha256) errors.push(`${rowLocation}: trusted ratification receipt digest mismatch for ${row.receipt_path}`);
    } catch (error) { errors.push(`${rowLocation}: ${error.message}`); }
    byKey.set(key, row);
  }
  return { ledger, byKey, errors };
}

function amendmentFiles(packageRoot, { ignoreWriteLock = false } = {}) {
  const directory = safeEntry(packageRoot, AMENDMENTS_RELATIVE, { type: "directory", optional: true });
  if (!directory) return { directory: null, files: [], errors: [] };
  const files = [];
  const errors = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === WRITE_LOCK_NAME && ignoreWriteLock) continue;
    const file = path.join(directory, entry.name);
    if (entry.name === WRITE_LOCK_NAME) errors.push(`${AMENDMENTS_RELATIVE}: amendment write is incomplete or in progress`);
    else if (entry.isSymbolicLink() || !entry.isFile() || !entry.name.endsWith(".toml")) errors.push(`${AMENDMENTS_RELATIVE}: unsupported entry ${entry.name}`);
    else files.push(file);
  }
  return { directory, files: files.sort(), errors };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
  return JSON.stringify(value);
}

function graph(authority, label) {
  if (!authority || !Array.isArray(authority.nodes)) throw new Error(`${label}: authority.nodes must be an array`);
  const byId = new Map();
  const children = new Map();
  const moduleById = new Map();
  const charterById = new Map();
  for (const rawNode of authority.nodes) {
    if (!rawNode || typeof rawNode !== "object" || !/^[A-Z][A-Z0-9-]*$/u.test(rawNode.id || "")) throw new Error(`${label}: invalid node id`);
    if (byId.has(rawNode.id)) throw new Error(`${label}: duplicate node ${rawNode.id}`);
    if (!Array.isArray(rawNode.predecessors) || rawNode.predecessors.some((id) => typeof id !== "string")) throw new Error(`${label}: node ${rawNode.id} has invalid predecessors`);
    const node = Object.fromEntries(Object.entries(rawNode).filter(([key]) => !key.startsWith("_")));
    byId.set(node.id, node);
    children.set(node.id, []);
    if (typeof rawNode._module === "string") moduleById.set(node.id, rawNode._module);
    if (typeof node.charter === "string") charterById.set(node.id, node.charter);
  }
  for (const node of byId.values()) {
    for (const predecessor of node.predecessors) {
      if (!byId.has(predecessor)) throw new Error(`${label}: node ${node.id} has missing predecessor ${predecessor}`);
      children.get(predecessor).push(node.id);
    }
  }
  for (const values of children.values()) values.sort();
  return { byId, children, moduleById, charterById };
}

function changedNodes(beforeGraph, afterGraph) {
  const ids = new Set([...beforeGraph.byId.keys(), ...afterGraph.byId.keys()]);
  return [...ids].filter((id) => canonical(beforeGraph.byId.get(id)) !== canonical(afterGraph.byId.get(id))).sort();
}

const GLOBAL_CHANGE_ROOTS = new Set(["catalogs", "contracts", "provenance", "schemas", "sources", "state", "templates", "tools"]);

function moduleMetadata(authority) {
  if (!Array.isArray(authority.moduleModels)) return null;
  return new Map(authority.moduleModels.map(({ relative, model }) => [relative, Object.fromEntries(Object.entries(model).filter(([key]) => key !== "node"))]));
}

function impactSeeds(beforeAuthority, afterAuthority, beforeGraph, afterGraph, paths) {
  const semantic = changedNodes(beforeGraph, afterGraph);
  const seeds = new Set(semantic);
  const allNodes = [...new Set([...beforeGraph.byId.keys(), ...afterGraph.byId.keys()])].sort();
  const charterOwners = new Map();
  for (const model of [beforeGraph, afterGraph]) {
    for (const [id, charter] of model.charterById) {
      if (!charterOwners.has(charter)) charterOwners.set(charter, new Set());
      charterOwners.get(charter).add(id);
    }
  }
  const beforeMetadata = moduleMetadata(beforeAuthority);
  const afterMetadata = moduleMetadata(afterAuthority);
  let global = false;
  for (const relative of paths) {
    const charterNodes = charterOwners.get(relative);
    if (charterNodes) {
      for (const id of charterNodes) seeds.add(id);
      continue;
    }
    if (relative.startsWith("charters/")) {
      global = true;
      continue;
    }
    if (relative.startsWith("authority/dag/")) {
      const module = relative.slice("authority/".length);
      const idsInModule = new Set();
      for (const model of [beforeGraph, afterGraph]) for (const [id, owner] of model.moduleById) if (owner === module) idsInModule.add(id);
      const explained = semantic.some((id) => idsInModule.has(id));
      const metadataChanged = beforeMetadata && afterMetadata
        ? canonical(beforeMetadata.get(module)) !== canonical(afterMetadata.get(module))
        : false;
      if (!explained || metadataChanged) global = true;
      continue;
    }
    const root = relative.split("/", 1)[0];
    if (GLOBAL_CHANGE_ROOTS.has(root) || relative === "authority/root.toml" || relative.startsWith("authority/state/") || relative.startsWith("authority/")) global = true;
  }
  if (global) for (const id of allNodes) seeds.add(id);
  return [...seeds].sort();
}

function descendants(seeds, model) {
  const result = new Set(seeds);
  const queue = seeds.filter((id) => model.children.has(id));
  while (queue.length) {
    const id = queue.shift();
    for (const child of model.children.get(id) || []) {
      if (!result.has(child)) {
        result.add(child);
        queue.push(child);
      }
    }
  }
  return result;
}

function impactClosure(ids, beforeGraph, afterGraph) {
  return [...new Set([...descendants(ids, beforeGraph), ...descendants(ids, afterGraph)])].sort();
}

function currentImpactClosure(ids, currentGraph) {
  return [...descendants(ids, currentGraph)].sort();
}

function authorityManifest(packageRoot) {
  const root = fs.realpathSync(packageRoot);
  const manifest = new Map();
  const walk = (directory, relativeDirectory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const relative = path.posix.join(relativeDirectory, entry.name);
      if (relative === LOCK_RELATIVE || relative.startsWith(`${AMENDMENTS_RELATIVE}/`)) continue;
      const absolute = path.join(directory, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`authority snapshot refuses symlink ${relative}`);
      if (entry.isDirectory()) walk(absolute, relative);
      else if (entry.isFile()) manifest.set(relative, sha256(fs.readFileSync(absolute)));
      else throw new Error(`authority snapshot refuses unsupported entry ${relative}`);
    }
  };
  for (const relative of AUTHORITY_DIGEST_ROOTS) {
    const absolute = path.join(root, relative);
    if (!fs.existsSync(absolute)) continue;
    const stat = fs.lstatSync(absolute);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`authority snapshot root must be a real directory: ${relative}`);
    walk(absolute, relative);
  }
  return manifest;
}

function changedPaths(beforeManifest, afterManifest) {
  const paths = new Set([...beforeManifest.keys(), ...afterManifest.keys()]);
  return [...paths].filter((relative) => beforeManifest.get(relative) !== afterManifest.get(relative)).sort();
}

function sortedUniqueStrings(values) {
  return Array.isArray(values) && values.every((value) => typeof value === "string") && values.every((value, index) => index === 0 || values[index - 1] < value);
}

function normalizedDerivedReceipts(values, label, impactClosure) {
  if (!Array.isArray(values)) throw new Error(`${label}: deriveInvalidatedReceipts must return an array`);
  if (values.some((value) => typeof value !== "string" || !/^[A-Z][A-Z0-9-]*:[0-9a-f]{64}$/u.test(value))) throw new Error(`${label}: derived receipt references must use node-id:sha256`);
  if (new Set(values).size !== values.length) throw new Error(`${label}: derived receipt references must be unique`);
  const impacted = new Set(impactClosure);
  for (const value of values) {
    const nodeId = value.slice(0, value.indexOf(":"));
    if (!impacted.has(nodeId)) throw new Error(`${label}: derived receipt node ${nodeId} is outside the impact closure`);
  }
  return [...values].sort();
}

function invalidationContext(mode, authority, row, beforeAuthority = null) {
  return Object.freeze({
    mode,
    authority,
    beforeAuthority,
    amendment: row,
    impactClosure: Object.freeze([...(row.impact_closure || [])]),
    changedNodes: Object.freeze([...(row.changed_nodes || [])]),
    changedPaths: Object.freeze([...(row.changed_paths || [])]),
    beforeAuthoritySha256: row.before_authority_sha256,
    afterAuthoritySha256: row.after_authority_sha256,
  });
}

function validateChangedPath(relative) {
  try {
    safeRelative(relative, `changed path ${relative}`);
    if (!AUTHORITY_DIGEST_ROOTS.includes(relative.split("/")[0])) return `changed path is outside the authority digest roots: ${relative}`;
    if (relative === LOCK_RELATIVE || relative.startsWith(`${AMENDMENTS_RELATIVE}/`)) return `changed path names amendment-chain state: ${relative}`;
    return null;
  } catch (error) { return error.message; }
}

function validateAmendmentsInternal(authority, dependencies, { ignoreWriteLock = false } = {}) {
  const errors = [];
  if (typeof dependencies.computeAuthorityDigest !== "function") return ["amendments: computeAuthorityDigest dependency is required"];
  let trusted;
  try { trusted = loadTrustedRatifications(authority.packageRoot, dependencies); }
  catch (error) { trusted = { byKey: new Map(), errors: [error.message] }; }
  errors.push(...trusted.errors);
  let currentDigest;
  let currentGraph;
  try { currentDigest = dependencies.computeAuthorityDigest(authority.packageRoot); }
  catch (error) { errors.push(`authority digest failed: ${error.message}`); }
  try { currentGraph = graph(authority, "current authority"); }
  catch (error) { errors.push(error.message); }

  let lockResult;
  try { lockResult = parseLockFile(authority.packageRoot, dependencies); }
  catch (error) { lockResult = { lock: null, errors: [error.message] }; }
  errors.push(...lockResult.errors);
  let inventory;
  try { inventory = amendmentFiles(authority.packageRoot, { ignoreWriteLock }); }
  catch (error) { inventory = { files: [], errors: [error.message] }; }
  errors.push(...inventory.errors);
  if (!lockResult.lock) {
    errors.push("authority amendment lock is missing");
    if (inventory.files.length) errors.push("amendments exist without an authority lock");
    return errors;
  }
  const lock = lockResult.lock;
  if (currentDigest && lock.current_authority_sha256 !== currentDigest) errors.push(`authority lock current digest ${lock.current_authority_sha256} does not match ${currentDigest}`);

  const records = [];
  const byDigest = new Map();
  const ids = new Set();
  for (const file of inventory.files) {
    const location = `${AMENDMENTS_RELATIVE}/${path.basename(file)}`;
    let parsed;
    try { parsed = parseAmendmentText(fs.readFileSync(file, "utf8"), authority.packageRoot, location, dependencies); }
    catch (error) { parsed = { row: null, digest: null, errors: [error.message] }; }
    errors.push(...parsed.errors);
    if (!parsed.row || !parsed.digest) continue;
    const row = parsed.row;
    if (`${row.amendment_id}.toml` !== path.basename(file)) errors.push(`${location}: filename must equal amendment_id`);
    if (ids.has(row.amendment_id)) errors.push(`${location}: duplicate amendment id ${row.amendment_id}`);
    ids.add(row.amendment_id);
    if (byDigest.has(parsed.digest)) errors.push(`${location}: duplicate amendment payload digest`);
    byDigest.set(parsed.digest, { row, digest: parsed.digest, file });
    records.push({ row, digest: parsed.digest, file });
    for (const key of ["changed_paths", "changed_nodes", "impact_closure", "invalidated_receipts"]) {
      if (!sortedUniqueStrings(row[key])) errors.push(`${location}: ${key} must be strictly sorted and unique`);
    }
    for (const relative of row.changed_paths || []) {
      const error = validateChangedPath(relative);
      if (error) errors.push(`${location}: ${error}`);
    }
    if (typeof dependencies.deriveInvalidatedReceipts === "function") {
      try {
        const expected = normalizedDerivedReceipts(
          dependencies.deriveInvalidatedReceipts(invalidationContext("validate", authority, row)),
          `${location} invalidation validation`,
          row.impact_closure || [],
        );
        const actual = Array.isArray(row.invalidated_receipts) ? row.invalidated_receipts : [];
        const actualSet = new Set(actual);
        const expectedSet = new Set(expected);
        const missing = expected.filter((receipt) => !actualSet.has(receipt));
        const extra = actual.filter((receipt) => !expectedSet.has(receipt));
        if (missing.length || extra.length) errors.push(`${location}: invalidated_receipts must exactly equal mechanically derived receipts; missing=${JSON.stringify(missing)} extra=${JSON.stringify(extra)}`);
      } catch (error) { errors.push(`${location}: invalidation derivation failed: ${error.message}`); }
    }
    if (!row.ratified_by || row.ratified_by !== row.ratified_by.trim() || /[\0-\x1f\x7f]/u.test(row.ratified_by)) errors.push(`${location}: ratified_by must be a non-empty normalized identity`);
    if (!SHA256.test(row.ratification_receipt_sha256 || "") || /^0{64}$/u.test(row.ratification_receipt_sha256)) errors.push(`${location}: ratification receipt must be a non-placeholder SHA-256`);
    const trustedKey = trustedSlotKey("authority-amendment", row.ratified_by, row.ratification_receipt_sha256);
    if (!trusted.byKey.has(trustedKey)) errors.push(`${location}: amendment is not authorized by a trusted authority-amendment ratification slot`);
    if (row.before_authority_sha256 === row.after_authority_sha256) errors.push(`${location}: before and after authority digests must differ`);
    if (!Number.isSafeInteger(row.before_generation) || row.before_generation < 0) errors.push(`${location}: before_generation must be a non-negative safe integer`);
    if (!Number.isSafeInteger(row.after_generation) || row.after_generation !== row.before_generation + 1) errors.push(`${location}: generation must advance by exactly one`);
  }

  if (!Number.isSafeInteger(lock.generation) || lock.generation < 0) errors.push("authority lock generation must be a non-negative safe integer");

  if (records.length === 0) {
    if (lock.last_amendment_id !== "" || lock.last_amendment_sha256 !== "") errors.push("authority lock names an amendment but the chain is empty");
    if (lock.baseline_authority_sha256 !== lock.current_authority_sha256) errors.push("empty amendment chain requires identical baseline and current digests");
    if (lock.generation !== 0) errors.push("empty amendment chain requires generation 0");
    return errors;
  }
  if (!lock.last_amendment_id || !lock.last_amendment_sha256) errors.push("non-empty amendment chain requires a complete lock head");
  const reverse = [];
  const visited = new Set();
  let cursor = lock.last_amendment_sha256;
  while (cursor) {
    if (visited.has(cursor)) {
      errors.push("authority amendment chain contains a cycle");
      break;
    }
    visited.add(cursor);
    const record = byDigest.get(cursor);
    if (!record) {
      errors.push(`authority amendment chain is broken at ${cursor}`);
      break;
    }
    reverse.push(record);
    cursor = record.row.previous_amendment_sha256;
  }
  if (visited.size !== records.length) errors.push("authority amendment chain does not contain every amendment file");
  const chain = reverse.reverse();
  if (chain.length) {
    const first = chain[0];
    const last = chain[chain.length - 1];
    if (first.row.previous_amendment_sha256 !== "") errors.push("first amendment must have an empty previous amendment digest");
    if (first.row.before_authority_sha256 !== lock.baseline_authority_sha256) errors.push("first amendment does not start at the locked baseline digest");
    if (first.row.before_generation !== 0) errors.push("first amendment must start at generation 0");
    for (let index = 1; index < chain.length; index += 1) {
      if (chain[index].row.before_authority_sha256 !== chain[index - 1].row.after_authority_sha256) errors.push(`${chain[index].row.amendment_id}: before digest does not equal the prior after digest`);
      if (chain[index].row.previous_amendment_sha256 !== chain[index - 1].digest) errors.push(`${chain[index].row.amendment_id}: previous amendment digest mismatch`);
      if (chain[index].row.before_generation !== chain[index - 1].row.after_generation) errors.push(`${chain[index].row.amendment_id}: before generation does not equal the prior after generation`);
    }
    if (last.row.after_authority_sha256 !== lock.current_authority_sha256) errors.push("last amendment after digest does not equal the locked current digest");
    if (last.row.amendment_id !== lock.last_amendment_id || last.digest !== lock.last_amendment_sha256) errors.push("authority lock head does not name the last amendment");
    if (last.row.after_generation !== lock.generation) errors.push("authority lock generation does not equal the last amendment after generation");
    if (currentGraph && Array.isArray(last.row.changed_nodes)) {
      const expected = currentImpactClosure(last.row.changed_nodes, currentGraph);
      if (canonical(last.row.impact_closure) !== canonical(expected)) errors.push(`${last.row.amendment_id}: impact_closure must exactly equal the changed-node descendant closure`);
    }
  }
  return errors;
}

export function validateAmendments(authority, dependencies = {}) {
  try { return validateAmendmentsInternal(authority, dependencies); }
  catch (error) { return [`amendments: ${error.message}`]; }
}

/**
 * Derive the authority-bound portion of an ACTIVE -> ACTIVE reactivation.
 *
 * The lifecycle owner must first validate `previousActivationTransition` as
 * the current ACTIVE transition and supply that transition's bound authority
 * digest/generation. This helper then proves that exactly one ratified
 * amendment advances that ACTIVE identity to the current authority head. It
 * deliberately derives the authorizing identity and receipt from the
 * immutable amendment instead of accepting activation authorization inputs.
 */
export function buildActiveAmendmentReactivation(authority, options, dependencies = {}) {
  if (!authority?.packageRoot) throw new Error("buildActiveAmendmentReactivation requires an authority packageRoot");
  const allowed = new Set(["previousPackageState", "previousActivationTransition", "previousAuthoritySha256", "previousAuthorityGeneration"]);
  for (const key of Object.keys(options || {})) if (!allowed.has(key)) throw new Error(`ACTIVE amendment reactivation option ${key} is not caller-controlled`);
  if (options?.previousPackageState !== "ACTIVE") throw new Error("amendment reactivation requires a prior ACTIVE package state");
  const previousTransition = options?.previousActivationTransition;
  const transitionMatch = /^ACT-[A-Z0-9-]+:([0-9a-f]{64})$/u.exec(previousTransition || "");
  if (!transitionMatch || /^0{64}$/u.test(transitionMatch[1])) throw new Error("amendment reactivation requires a non-placeholder prior activation transition");
  if (!SHA256.test(options?.previousAuthoritySha256 || "") || /^0{64}$/u.test(options.previousAuthoritySha256)) throw new Error("amendment reactivation requires a non-placeholder prior ACTIVE authority digest");
  if (!Number.isSafeInteger(options?.previousAuthorityGeneration) || options.previousAuthorityGeneration < 0) throw new Error("amendment reactivation requires a non-negative prior ACTIVE authority generation");

  const chainErrors = validateAmendmentsInternal(authority, dependencies);
  if (chainErrors.length) throw new Error(`amendment reactivation requires a valid amendment chain: ${chainErrors.join("; ")}`);
  const { lock, errors: lockErrors } = parseLockFile(authority.packageRoot, dependencies);
  if (lockErrors.length || !lock?.last_amendment_id || !lock.last_amendment_sha256) throw new Error(`amendment reactivation requires a non-empty authority amendment head${lockErrors.length ? `: ${lockErrors.join("; ")}` : ""}`);
  const inventory = amendmentFiles(authority.packageRoot);
  if (inventory.errors.length) throw new Error(`amendment reactivation inventory is invalid: ${inventory.errors.join("; ")}`);
  const headFile = inventory.files.find((file) => path.basename(file) === `${lock.last_amendment_id}.toml`);
  if (!headFile) throw new Error("amendment reactivation cannot resolve the locked amendment head");
  const parsed = parseAmendmentText(fs.readFileSync(headFile, "utf8"), authority.packageRoot, `${AMENDMENTS_RELATIVE}/${path.basename(headFile)}`, dependencies);
  if (parsed.errors.length || !parsed.row || parsed.digest !== lock.last_amendment_sha256) throw new Error(`amendment reactivation head is invalid: ${parsed.errors.join("; ") || "locked digest mismatch"}`);
  const head = parsed.row;
  if (head.before_authority_sha256 !== options.previousAuthoritySha256) throw new Error("amendment does not start from the prior ACTIVE authority digest");
  if (head.before_generation !== options.previousAuthorityGeneration) throw new Error("amendment does not start from the prior ACTIVE authority generation");
  if (head.after_authority_sha256 !== lock.current_authority_sha256 || head.after_generation !== lock.generation) throw new Error("amendment does not end at the current locked authority identity");

  return Object.freeze({
    from_state: "ACTIVE",
    to_state: "ACTIVE",
    previous_activation_transition: previousTransition,
    authority_amendment: `${head.amendment_id}:${parsed.digest}`,
    before_authority_sha256: head.before_authority_sha256,
    after_authority_sha256: head.after_authority_sha256,
    before_generation: head.before_generation,
    after_generation: head.after_generation,
    reactivated_by: head.ratified_by,
    ratification_receipt_sha256: head.ratification_receipt_sha256,
  });
}

function quote(value) {
  return JSON.stringify(value);
}

function stringArray(values) {
  return `[${values.map(quote).join(", ")}]`;
}

function renderAmendment(row) {
  const prefix = [
    `schema = ${row.schema}`,
    `type = ${quote(row.type)}`,
    `amendment_id = ${quote(row.amendment_id)}`,
    `ratified_by = ${quote(row.ratified_by)}`,
    `ratification_receipt_sha256 = ${quote(row.ratification_receipt_sha256)}`,
    `previous_amendment_sha256 = ${quote(row.previous_amendment_sha256)}`,
    `before_authority_sha256 = ${quote(row.before_authority_sha256)}`,
    `after_authority_sha256 = ${quote(row.after_authority_sha256)}`,
    `before_generation = ${row.before_generation}`,
    `after_generation = ${row.after_generation}`,
    `changed_paths = ${stringArray(row.changed_paths)}`,
    `changed_nodes = ${stringArray(row.changed_nodes)}`,
    `impact_closure = ${stringArray(row.impact_closure)}`,
    `invalidated_receipts = ${stringArray(row.invalidated_receipts)}`,
    "",
  ].join("\n");
  const digest = sha256(prefix);
  return { text: `${prefix}payload_sha256 = "${digest}"\n`, digest };
}

function renderLock(lock) {
  return [
    `schema = ${lock.schema}`,
    `baseline_authority_sha256 = ${quote(lock.baseline_authority_sha256)}`,
    `current_authority_sha256 = ${quote(lock.current_authority_sha256)}`,
    `generation = ${lock.generation}`,
    `last_amendment_id = ${quote(lock.last_amendment_id)}`,
    `last_amendment_sha256 = ${quote(lock.last_amendment_sha256)}`,
    "",
  ].join("\n");
}

function historySnapshot(packageRoot) {
  const result = new Map();
  const lockFile = safeEntry(packageRoot, LOCK_RELATIVE, { optional: true });
  if (lockFile) result.set(LOCK_RELATIVE, fs.readFileSync(lockFile));
  const inventory = amendmentFiles(packageRoot, { ignoreWriteLock: true });
  if (inventory.errors.length) throw new Error(inventory.errors.join("; "));
  for (const file of inventory.files) result.set(`${AMENDMENTS_RELATIVE}/${path.basename(file)}`, fs.readFileSync(file));
  return result;
}

function equalHistory(left, right) {
  if (left.size !== right.size) return false;
  for (const [relative, bytes] of left) if (!right.has(relative) || !bytes.equals(right.get(relative))) return false;
  return true;
}

function writeExclusive(file, text) {
  const descriptor = fs.openSync(file, "wx", 0o600);
  try {
    fs.writeFileSync(descriptor, text, "utf8");
    fs.fsyncSync(descriptor);
  } finally { fs.closeSync(descriptor); }
}

function restoreLock(lockFile, previousBytes, amendmentsDirectory) {
  if (previousBytes === null) {
    if (fs.existsSync(lockFile)) fs.unlinkSync(lockFile);
    return;
  }
  const temporary = path.join(amendmentsDirectory, `.restore-lock-${crypto.randomBytes(8).toString("hex")}.tmp`);
  const descriptor = fs.openSync(temporary, "wx", 0o600);
  try {
    fs.writeFileSync(descriptor, previousBytes);
    fs.fsyncSync(descriptor);
  } finally { fs.closeSync(descriptor); }
  fs.renameSync(temporary, lockFile);
}

export function createAmendment(authority, options, dependencies = {}) {
  if (!authority?.packageRoot) throw new Error("createAmendment requires an authority packageRoot");
  if (typeof dependencies.computeAuthorityDigest !== "function") throw new Error("createAmendment requires computeAuthorityDigest");
  if (typeof dependencies.loadAuthority !== "function") throw new Error("createAmendment requires loadAuthority");
  if (typeof dependencies.deriveInvalidatedReceipts !== "function") throw new Error("createAmendment deriveInvalidatedReceipts dependency is required");
  if (options && Object.hasOwn(options, "invalidatedReceipts")) throw new Error("invalidatedReceipts is derived and cannot be supplied by the caller");
  const id = options?.id;
  const beforeRoot = options?.beforeRoot;
  const ratifiedBy = options?.ratifiedBy;
  const receipt = options?.ratificationReceiptSha256;
  if (!AMENDMENT_ID.test(id || "")) throw new Error("amendment id must match AMD-[A-Z0-9-]+");
  if (typeof beforeRoot !== "string" || !fs.existsSync(beforeRoot)) throw new Error("beforeRoot must name an existing authority package");
  if (typeof ratifiedBy !== "string" || ratifiedBy.length === 0 || ratifiedBy !== ratifiedBy.trim() || /[\0-\x1f\x7f]/u.test(ratifiedBy)) throw new Error("ratifiedBy must be a non-empty normalized identity");
  if (!SHA256.test(receipt || "") || /^0{64}$/u.test(receipt)) throw new Error("ratificationReceiptSha256 must be a non-placeholder SHA-256");

  const packageRoot = fs.realpathSync(authority.packageRoot);
  const amendmentsDirectory = ensureDirectory(packageRoot, AMENDMENTS_RELATIVE);
  const writeLock = path.join(amendmentsDirectory, WRITE_LOCK_NAME);
  try { fs.mkdirSync(writeLock, { mode: 0o700 }); }
  catch (error) {
    if (error.code === "EEXIST") throw new Error("authority amendment write is already in progress");
    throw error;
  }

  let amendmentFile = null;
  let amendmentInstalled = false;
  let lockInstalled = false;
  let previousLockBytes = null;
  let amendmentTemporary = null;
  let lockTemporary = null;
  try {
    const beforeAuthority = dependencies.loadAuthority(beforeRoot);
    const beforeHistory = historySnapshot(beforeRoot);
    const currentHistory = historySnapshot(packageRoot);
    if (!equalHistory(beforeHistory, currentHistory)) throw new Error("beforeRoot and current authority do not carry the same immutable amendment history");
    const beforeTrusted = loadTrustedRatifications(beforeRoot, dependencies);
    const afterTrusted = loadTrustedRatifications(packageRoot, dependencies);
    if (beforeTrusted.errors.length) throw new Error(`beforeRoot trusted ratifications are invalid: ${beforeTrusted.errors.join("; ")}`);
    if (afterTrusted.errors.length) throw new Error(`current trusted ratifications are invalid: ${afterTrusted.errors.join("; ")}`);
    const trustedKey = trustedSlotKey("authority-amendment", ratifiedBy, receipt);
    const beforeSlot = beforeTrusted.byKey.get(trustedKey);
    const afterSlot = afterTrusted.byKey.get(trustedKey);
    if (!beforeSlot || !afterSlot || canonical(beforeSlot) !== canonical(afterSlot)) throw new Error("a matching trusted authority-amendment ratification slot must exist unchanged in both before and after authority");
    const beforeLockResult = parseLockFile(beforeRoot, dependencies);
    if (beforeLockResult.errors.length) throw new Error(beforeLockResult.errors.join("; "));
    if (beforeLockResult.lock) {
      const beforeErrors = validateAmendmentsInternal(beforeAuthority, dependencies, { ignoreWriteLock: true });
      if (beforeErrors.length) throw new Error(`beforeRoot amendment chain is invalid: ${beforeErrors.join("; ")}`);
    } else if ([...beforeHistory.keys()].some((relative) => relative.startsWith(`${AMENDMENTS_RELATIVE}/`))) {
      throw new Error("beforeRoot contains amendments without an authority lock");
    }

    amendmentFile = path.join(amendmentsDirectory, `${id}.toml`);
    if (fs.existsSync(amendmentFile)) throw new Error(`amendment ${id} already exists`);
    const beforeDigest = dependencies.computeAuthorityDigest(beforeRoot);
    const afterDigest = dependencies.computeAuthorityDigest(packageRoot);
    if (!SHA256.test(beforeDigest) || !SHA256.test(afterDigest)) throw new Error("authority digest callback must return lowercase SHA-256 values");
    if (beforeLockResult.lock && beforeLockResult.lock.current_authority_sha256 !== beforeDigest) throw new Error("beforeRoot digest does not match its authority lock");
    const paths = changedPaths(authorityManifest(beforeRoot), authorityManifest(packageRoot));
    if (paths.length === 0 || beforeDigest === afterDigest) throw new Error("an authority amendment must describe a non-empty authority change");
    const beforeGraph = graph(beforeAuthority, "before authority");
    const afterGraph = graph(authority, "after authority");
    const nodes = impactSeeds(beforeAuthority, authority, beforeGraph, afterGraph, paths);
    const closure = impactClosure(nodes, beforeGraph, afterGraph);
    const previousDigest = beforeLockResult.lock?.last_amendment_sha256 || "";
    const beforeGeneration = beforeLockResult.lock?.generation ?? 0;
    const amendmentWithoutInvalidation = {
      schema: 2,
      type: "authority-amendment",
      amendment_id: id,
      ratified_by: ratifiedBy,
      ratification_receipt_sha256: receipt,
      previous_amendment_sha256: previousDigest,
      before_authority_sha256: beforeDigest,
      after_authority_sha256: afterDigest,
      before_generation: beforeGeneration,
      after_generation: beforeGeneration + 1,
      changed_paths: paths,
      changed_nodes: nodes,
      impact_closure: closure,
    };
    const invalidatedReceipts = normalizedDerivedReceipts(
      dependencies.deriveInvalidatedReceipts(invalidationContext("create", authority, amendmentWithoutInvalidation, beforeAuthority)),
      `amendment ${id}`,
      closure,
    );
    const amendment = { ...amendmentWithoutInvalidation, invalidated_receipts: invalidatedReceipts };
    const rendered = renderAmendment(amendment);
    amendment.payload_sha256 = rendered.digest;
    const parsedGenerated = parseAmendmentText(rendered.text, packageRoot, `${AMENDMENTS_RELATIVE}/${id}.toml`, dependencies);
    if (parsedGenerated.errors.length) throw new Error(`generated amendment is invalid: ${parsedGenerated.errors.join("; ")}`);
    const lock = {
      schema: 2,
      baseline_authority_sha256: beforeLockResult.lock?.baseline_authority_sha256 || beforeDigest,
      current_authority_sha256: afterDigest,
      generation: beforeGeneration + 1,
      last_amendment_id: id,
      last_amendment_sha256: rendered.digest,
    };
    const lockErrors = validateObject(lock, loadSchema(packageRoot, "authority-lock.schema.json"), "authority-lock", dependencies.validateSchemaObject);
    if (lockErrors.length) throw new Error(`generated authority lock is invalid: ${lockErrors.join("; ")}`);
    if (dependencies.computeAuthorityDigest(packageRoot) !== afterDigest) throw new Error("authority changed while the amendment was being prepared");

    const nonce = crypto.randomBytes(8).toString("hex");
    amendmentTemporary = path.join(amendmentsDirectory, `.${id}-${nonce}.tmp`);
    lockTemporary = path.join(amendmentsDirectory, `.authority-lock-${nonce}.tmp`);
    writeExclusive(amendmentTemporary, rendered.text);
    writeExclusive(lockTemporary, renderLock(lock));
    const lockFile = path.join(packageRoot, LOCK_RELATIVE);
    previousLockBytes = fs.existsSync(lockFile) ? fs.readFileSync(lockFile) : null;
    fs.linkSync(amendmentTemporary, amendmentFile);
    fs.unlinkSync(amendmentTemporary);
    amendmentTemporary = null;
    amendmentInstalled = true;
    fs.renameSync(lockTemporary, lockFile);
    lockTemporary = null;
    lockInstalled = true;

    const postErrors = validateAmendmentsInternal(authority, dependencies, { ignoreWriteLock: true });
    if (postErrors.length) throw new Error(`installed amendment chain is invalid: ${postErrors.join("; ")}`);
    return { amendment, amendmentFile, amendmentSha256: rendered.digest, lock, lockFile };
  } catch (error) {
    if (lockInstalled) {
      try { restoreLock(path.join(packageRoot, LOCK_RELATIVE), previousLockBytes, amendmentsDirectory); }
      catch (restoreError) { throw new AggregateError([error, restoreError], "amendment failed and the authority lock could not be restored"); }
    }
    if (amendmentInstalled && amendmentFile && fs.existsSync(amendmentFile)) fs.unlinkSync(amendmentFile);
    throw error;
  } finally {
    for (const temporary of [amendmentTemporary, lockTemporary]) if (temporary && fs.existsSync(temporary)) fs.unlinkSync(temporary);
    if (fs.existsSync(writeLock)) fs.rmdirSync(writeLock);
  }
}
