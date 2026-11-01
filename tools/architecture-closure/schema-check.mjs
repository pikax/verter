// The JSON Schema subset the closure report schema is written in. The root
// package carries no schema library and this tool adds no dependency, so a
// schema using anything else is refused up front: it can never rely on a rule
// nothing enforces.

const ANNOTATIONS = new Set(["$schema", "$id", "$defs", "title", "description"]);

// A keyword applying to one instance type must sit beside that `type`, so it
// cannot go silently unenforced against a value of another type.
const KEYWORD_TYPE = {
  minLength: "string",
  minimum: "integer",
  items: "array",
  properties: "object",
  required: "object",
  additionalProperties: "object",
};

const KEYWORDS = new Set(["$ref", "enum", "type", ...Object.keys(KEYWORD_TYPE)]);

const TYPE_CHECKS = {
  object: (v) => v !== null && typeof v === "object" && !Array.isArray(v),
  array: Array.isArray,
  string: (v) => typeof v === "string",
  boolean: (v) => typeof v === "boolean",
  integer: Number.isInteger,
};

function resolveRef(root, ref) {
  const match = /^#\/\$defs\/([^/]+)$/.exec(ref);
  const target = match && root.$defs?.[match[1]];
  if (!target) throw new Error(`schema: unresolved $ref ${ref}`);
  return target;
}

function assertSupported(root, schema, path) {
  for (const keyword of Object.keys(schema)) {
    if (ANNOTATIONS.has(keyword)) continue;
    if (!KEYWORDS.has(keyword))
      throw new Error(`schema: unsupported keyword ${keyword} at ${path}`);
    const owner = KEYWORD_TYPE[keyword];
    if (owner && schema.type !== owner) {
      throw new Error(`schema: ${keyword} needs type ${owner} at ${path}`);
    }
  }
  if (schema.type && !TYPE_CHECKS[schema.type]) {
    throw new Error(`schema: unsupported type ${schema.type} at ${path}`);
  }
  if (schema.$ref) resolveRef(root, schema.$ref);
  for (const group of ["properties", "$defs"]) {
    for (const [name, child] of Object.entries(schema[group] ?? {})) {
      assertSupported(root, child, `${path}/${group}/${name}`);
    }
  }
  if (schema.items) assertSupported(root, schema.items, `${path}/items`);
  if (typeof schema.additionalProperties === "object") {
    assertSupported(root, schema.additionalProperties, `${path}/additionalProperties`);
  }
}

function checkObject(root, schema, value, path, out) {
  for (const key of schema.required ?? []) {
    if (!Object.hasOwn(value, key)) {
      out.push({ path, keyword: "required", message: `missing required property ${key}` });
    }
  }
  const properties = schema.properties ?? {};
  for (const [key, child] of Object.entries(value)) {
    const childPath = `${path}/${key}`;
    if (Object.hasOwn(properties, key)) {
      checkNode(root, properties[key], child, childPath, out);
    } else if (schema.additionalProperties === false) {
      out.push({ path: childPath, keyword: "additionalProperties", message: "unknown property" });
    } else if (schema.additionalProperties) {
      checkNode(root, schema.additionalProperties, child, childPath, out);
    }
  }
}

function checkNode(root, schema, value, path, out) {
  if (schema.$ref) {
    checkNode(root, resolveRef(root, schema.$ref), value, path, out);
    return;
  }
  if (schema.enum && !schema.enum.includes(value)) {
    out.push({ path, keyword: "enum", message: `not one of ${schema.enum.join(", ")}` });
    return;
  }
  if (schema.type && !TYPE_CHECKS[schema.type](value)) {
    out.push({ path, keyword: "type", message: `expected ${schema.type}` });
    return;
  }
  if (schema.minLength !== undefined && value.length < schema.minLength) {
    out.push({ path, keyword: "minLength", message: `shorter than ${schema.minLength}` });
  }
  if (schema.minimum !== undefined && value < schema.minimum) {
    out.push({ path, keyword: "minimum", message: `below ${schema.minimum}` });
  }
  if (schema.items) {
    value.forEach((item, index) => checkNode(root, schema.items, item, `${path}/${index}`, out));
  }
  if (schema.type === "object") checkObject(root, schema, value, path, out);
}

/** Every structural violation of `value` against `schema`, in document order. */
export function checkSchema(schema, value) {
  assertSupported(schema, schema, "");
  const out = [];
  checkNode(schema, schema, value, "", out);
  return out;
}
