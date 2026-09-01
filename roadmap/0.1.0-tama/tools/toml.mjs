import fs from "node:fs";

// Minimal, safe TOML subset shared by every roadmap tool. Supported:
// top-level `key = value`, `[table]`, `[[array-table]]` sections, bare and
// quoted keys, string/integer/boolean/array values, and single-line inline
// tables (`{ key = value, ... }`). Comments respect quoted strings. Duplicate
// keys and prototype-bearing keys are rejected.

const FORBIDDEN_KEYS = new Set(["__proto__", "prototype", "constructor"]);

function assertSafeKey(key, lineNumber) {
  if (FORBIDDEN_KEYS.has(key))
    throw new Error(`TOML line ${lineNumber}: unsafe prototype-bearing key ${key}`);
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

/** Split `text` on top-level occurrences of a single-character separator. */
function splitTopLevel(text, separator) {
  const parts = [];
  let depthBrace = 0;
  let depthBracket = 0;
  let quoted = false;
  let escaped = false;
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (char === "\\" && quoted) {
      escaped = true;
      continue;
    }
    if (char === '"') quoted = !quoted;
    if (quoted) continue;
    if (char === "{") depthBrace += 1;
    else if (char === "}") depthBrace -= 1;
    else if (char === "[") depthBracket += 1;
    else if (char === "]") depthBracket -= 1;
    else if (char === separator && depthBrace === 0 && depthBracket === 0) {
      parts.push(text.slice(start, index));
      start = index + 1;
    }
    if (depthBrace < 0 || depthBracket < 0) throw new Error("unbalanced delimiter");
  }
  parts.push(text.slice(start));
  return parts;
}

const BARE_KEY = /^[A-Za-z0-9_-]+$/u;
const QUOTED_KEY = /^"(?:[^"\\]|\\.)*"$/u;

function parseKey(raw, lineNumber) {
  const key = raw.trim();
  if (BARE_KEY.test(key)) {
    assertSafeKey(key, lineNumber);
    return key;
  }
  if (QUOTED_KEY.test(key)) {
    let parsed;
    try {
      parsed = JSON.parse(key);
    } catch (error) {
      throw new Error(`TOML line ${lineNumber}: invalid quoted key: ${error.message}`);
    }
    if (typeof parsed !== "string" || parsed.length === 0)
      throw new Error(`TOML line ${lineNumber}: empty quoted key`);
    assertSafeKey(parsed, lineNumber);
    return parsed;
  }
  throw new Error(`TOML line ${lineNumber}: malformed key ${raw}`);
}

function parseInlineTable(raw, lineNumber) {
  const inner = raw.trim().slice(1, -1).trim();
  const table = {};
  if (!inner) return table;
  let entries;
  try {
    entries = splitTopLevel(inner, ",");
  } catch {
    throw new Error(`TOML line ${lineNumber}: malformed inline table`);
  }
  for (const entry of entries) {
    const trimmed = entry.trim();
    if (!trimmed) throw new Error(`TOML line ${lineNumber}: empty inline-table entry`);
    let keyRaw;
    let valueRaw;
    try {
      const eqParts = splitTopLevel(trimmed, "=");
      if (eqParts.length < 2) throw new Error("missing =");
      keyRaw = eqParts[0];
      valueRaw = eqParts.slice(1).join("=");
    } catch {
      throw new Error(`TOML line ${lineNumber}: malformed inline-table entry ${trimmed}`);
    }
    const key = parseKey(keyRaw, lineNumber);
    if (Object.hasOwn(table, key))
      throw new Error(`TOML line ${lineNumber}: duplicate inline-table key ${key}`);
    table[key] = parseValue(valueRaw, lineNumber);
  }
  return table;
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
  if (value.startsWith("{")) {
    if (!value.endsWith("}")) throw new Error(`TOML line ${lineNumber}: unterminated inline table`);
    return parseInlineTable(value, lineNumber);
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
    const assignment = line.match(/^("(?:[^"\\]|\\.)*"|[A-Za-z0-9_-]+)\s*=\s*(.+)$/u);
    if (!assignment) throw new Error(`TOML line ${lineNumber}: malformed statement`);
    const [, keyRaw, raw] = assignment;
    const key = parseKey(keyRaw, lineNumber);
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

/** Serialize one value the way parseToml reads it back. */
export function serializeTomlValue(value) {
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value)) throw new Error(`unsupported number ${value}`);
    return String(value);
  }
  if (Array.isArray(value)) return JSON.stringify(value);
  if (value !== null && typeof value === "object") return serializeInlineTable(value);
  throw new Error(`unsupported TOML value ${String(value)}`);
}

/** Serialize an inline table with the object's own insertion order. */
export function serializeInlineTable(table) {
  const entries = Object.entries(table).map(([key, value]) => {
    const keyText = BARE_KEY.test(key) ? key : JSON.stringify(key);
    return `${keyText} = ${serializeTomlValue(value)}`;
  });
  return entries.length === 0 ? "{}" : `{ ${entries.join(", ")} }`;
}

/** Serialize a key for an assignment line (quoted unless bare-safe). */
export function serializeTomlKey(key) {
  return BARE_KEY.test(key) ? key : JSON.stringify(key);
}
