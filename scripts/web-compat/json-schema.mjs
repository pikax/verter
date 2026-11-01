// A closed draft-07 subset validator for the two pinned compatibility schemas.
// An unrecognised keyword throws rather than passing silently, so a schema
// upgrade that introduces new validation semantics stops generation instead of
// weakening it.

const ANNOTATION_KEYWORDS = new Set([
  "$schema",
  "$comment",
  "$id",
  "title",
  "description",
  "definitions",
  "tsType",
  "errorMessage",
  "format",
]);

function typeMatches(type, value, nullable) {
  if (value === null) return type === "null" || nullable === true;
  switch (type) {
    case "object":
      return typeof value === "object" && !Array.isArray(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "boolean":
      return typeof value === "boolean";
    case "number":
      return typeof value === "number";
    case "integer":
      return Number.isInteger(value);
    case "null":
      return false;
    default:
      throw new Error(`json-schema: unsupported type ${JSON.stringify(type)}`);
  }
}

function deepEqual(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** A validator over one root schema. `validate` returns the first failure in traversal order, or null. */
export function schemaValidator(root) {
  const patterns = new Map();
  const regex = (source) => {
    let compiled = patterns.get(source);
    if (compiled === undefined) {
      compiled = new RegExp(source, "u");
      patterns.set(source, compiled);
    }
    return compiled;
  };

  const resolve = (ref) => {
    const prefix = "#/definitions/";
    if (!ref.startsWith(prefix)) throw new Error(`json-schema: unsupported $ref ${ref}`);
    const target = root.definitions?.[ref.slice(prefix.length)];
    if (target === undefined) throw new Error(`json-schema: dangling $ref ${ref}`);
    return target;
  };

  const fail = (instancePath, keyword) => ({ instance_path: instancePath, keyword });

  const check = (schema, value, path) => {
    if (schema === true) return null;
    if (schema === false) return fail(path, "false");
    for (const keyword of Object.keys(schema)) {
      if (ANNOTATION_KEYWORDS.has(keyword)) continue;
      const failure = checkKeyword(keyword, schema, value, path);
      if (failure !== null) return failure;
    }
    return null;
  };

  const checkKeyword = (keyword, schema, value, path) => {
    const argument = schema[keyword];
    switch (keyword) {
      case "$ref":
        return check(resolve(argument), value, path);
      case "type":
        return typeMatches(argument, value, schema.nullable) ? null : fail(path, "type");
      case "nullable":
        return null;
      case "const":
        return deepEqual(argument, value) ? null : fail(path, "const");
      case "enum":
        return argument.some((option) => deepEqual(option, value)) ? null : fail(path, "enum");
      case "pattern":
        return typeof value !== "string" || regex(argument).test(value)
          ? null
          : fail(path, "pattern");
      case "minItems":
        return !Array.isArray(value) || value.length >= argument ? null : fail(path, "minItems");
      case "items": {
        if (!Array.isArray(value)) return null;
        for (let index = 0; index < value.length; index += 1) {
          const failure = check(argument, value[index], `${path}/${index}`);
          if (failure !== null) return failure;
        }
        return null;
      }
      case "minProperties":
        return !isPlainObject(value) || Object.keys(value).length >= argument
          ? null
          : fail(path, "minProperties");
      case "maxProperties":
        return !isPlainObject(value) || Object.keys(value).length <= argument
          ? null
          : fail(path, "maxProperties");
      case "required": {
        if (!isPlainObject(value)) return null;
        for (const name of argument) {
          if (!Object.hasOwn(value, name)) return fail(`${path}/${name}`, "required");
        }
        return null;
      }
      case "properties": {
        if (!isPlainObject(value)) return null;
        for (const name of Object.keys(argument)) {
          if (!Object.hasOwn(value, name)) continue;
          const failure = check(argument[name], value[name], `${path}/${name}`);
          if (failure !== null) return failure;
        }
        return null;
      }
      case "patternProperties": {
        if (!isPlainObject(value)) return null;
        for (const pattern of Object.keys(argument)) {
          for (const name of Object.keys(value)) {
            if (!regex(pattern).test(name)) continue;
            const failure = check(argument[pattern], value[name], `${path}/${name}`);
            if (failure !== null) return failure;
          }
        }
        return null;
      }
      case "additionalProperties": {
        if (!isPlainObject(value)) return null;
        const declared = Object.keys(schema.properties ?? {});
        const patternsDeclared = Object.keys(schema.patternProperties ?? {});
        for (const name of Object.keys(value)) {
          if (declared.includes(name)) continue;
          if (patternsDeclared.some((pattern) => regex(pattern).test(name))) continue;
          const failure = check(argument, value[name], `${path}/${name}`);
          if (failure !== null) {
            return argument === false ? fail(`${path}/${name}`, "additionalProperties") : failure;
          }
        }
        return null;
      }
      case "propertyNames": {
        if (!isPlainObject(value)) return null;
        for (const name of Object.keys(value)) {
          if (check(argument, name, `${path}/${name}`) !== null) {
            return fail(`${path}/${name}`, "propertyNames");
          }
        }
        return null;
      }
      case "anyOf":
        return argument.some((option) => check(option, value, path) === null)
          ? null
          : fail(path, "anyOf");
      case "oneOf":
        return argument.filter((option) => check(option, value, path) === null).length === 1
          ? null
          : fail(path, "oneOf");
      case "allOf": {
        for (const option of argument) {
          const failure = check(option, value, path);
          if (failure !== null) return failure;
        }
        return null;
      }
      case "if":
        if (schema.else !== undefined) throw new Error("json-schema: unsupported else");
        return check(argument, value, path) === null && schema.then !== undefined
          ? check(schema.then, value, path)
          : null;
      case "then":
        return null;
      case "dependencies": {
        if (!isPlainObject(value)) return null;
        for (const name of Object.keys(argument)) {
          if (!Object.hasOwn(value, name)) continue;
          const failure = check(argument[name], value, path);
          if (failure !== null) return failure;
        }
        return null;
      }
      default:
        throw new Error(`json-schema: unsupported keyword ${keyword}`);
    }
  };

  return { validate: (schema, value, path = "") => check(schema, value, path) };
}
