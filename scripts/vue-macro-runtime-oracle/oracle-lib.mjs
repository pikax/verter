import { createHash } from "node:crypto";
import { createRequire } from "node:module";

import { VUE_MACRO_RUNTIME_FIXTURES } from "./fixtures.mjs";

const require = createRequire(import.meta.url);
const { compileScript, parse } = require("@vue/compiler-sfc");
const compilerPackage = require("@vue/compiler-sfc/package.json");
const compilerRequire = createRequire(require.resolve("@vue/compiler-sfc/package.json"));
const babelParser = compilerRequire("@babel/parser");

export const VUE_MACRO_ORACLE_SCHEMA_VERSION = 1;
export const VUE_MACRO_ORACLE_VERSION = "3.5.34";

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function propertyName(node) {
  if (node.type === "Identifier") return node.name;
  if (node.type === "StringLiteral" || node.type === "NumericLiteral") {
    return String(node.value);
  }
  throw new Error(`unsupported computed runtime property name: ${node.type}`);
}

function objectProperty(object, name) {
  return object.properties.find(
    (property) => property.type === "ObjectProperty" && propertyName(property.key) === name,
  );
}

function boolValue(expression, field) {
  if (expression.type === "BooleanLiteral") return expression.value;
  throw new Error(`${field} must be a boolean literal, got ${expression.type}`);
}

function constructorNames(expression) {
  if (expression.type === "NullLiteral") return [];
  const values = expression.type === "ArrayExpression" ? expression.elements : [expression];
  return values.map((value) => {
    if (value?.type !== "Identifier") {
      throw new Error(`runtime constructor must be an identifier, got ${value?.type}`);
    }
    return value.name;
  });
}

function printExpression(expression, source) {
  return source.slice(expression.start, expression.end).replace(/\s+/g, " ").trim();
}

function componentOptions(program) {
  for (const statement of program.body) {
    if (statement.type !== "ExportDefaultDeclaration") continue;
    const expression = statement.declaration;
    if (expression.type !== "CallExpression" || expression.arguments.length === 0) continue;
    const options = expression.arguments[0];
    if (options.type === "ObjectExpression") return options;
  }
  throw new Error("compiled output has no exported component options object");
}

function extractProps(options, source) {
  const property = objectProperty(options, "props");
  if (!property) return [];
  if (property.value.type !== "ObjectExpression") {
    throw new Error("typed runtime props must compile to an object literal");
  }

  return property.value.properties.map((row) => {
    if (row.type !== "ObjectProperty" || row.value.type !== "ObjectExpression") {
      throw new Error(`unsupported runtime prop row: ${row.type}`);
    }
    const type = objectProperty(row.value, "type");
    const required = objectProperty(row.value, "required");
    const skipCheck = objectProperty(row.value, "skipCheck");
    const defaultValue = objectProperty(row.value, "default");
    return {
      name: propertyName(row.key),
      constructors: type ? constructorNames(type.value) : [],
      required: required ? boolValue(required.value, "required") : null,
      skipCheck: skipCheck ? boolValue(skipCheck.value, "skipCheck") : false,
      default: defaultValue ? printExpression(defaultValue.value, source) : null,
    };
  });
}

function extractEmits(options) {
  const property = objectProperty(options, "emits");
  if (!property) return [];
  if (property.value.type !== "ArrayExpression") {
    throw new Error("typed runtime emits must compile to an array literal");
  }
  return property.value.elements.map((event) => {
    if (event?.type !== "StringLiteral") {
      throw new Error(`runtime emit must be a string literal, got ${event?.type}`);
    }
    return event.value;
  });
}

export function extractRuntimeShape(compiledSource) {
  const ast = babelParser.parse(compiledSource, {
    sourceType: "module",
    plugins: ["typescript"],
  });
  const options = componentOptions(ast.program);
  return {
    props: extractProps(options, compiledSource),
    emits: extractEmits(options),
  };
}

function compileFixture(fixture) {
  const filename = fixture.filename ?? `/fixtures/${fixture.id}.vue`;
  const parsed = parse(fixture.source, { filename });
  if (parsed.errors.length !== 0) {
    throw new Error(`${fixture.id}: parse failed: ${parsed.errors.join("\n")}`);
  }
  const files = fixture.supportFiles ?? {};
  const fs = {
    fileExists(file) {
      return Object.hasOwn(files, file);
    },
    readFile(file) {
      return files[file];
    },
  };
  const compiled = compileScript(parsed.descriptor, {
    id: `oracle-${fixture.id}`,
    fs,
  });
  return {
    id: fixture.id,
    axes: fixture.axes,
    sourceSha256: sha256(fixture.source),
    runtime: extractRuntimeShape(compiled.content),
  };
}

function fixtureFingerprint() {
  return sha256(
    JSON.stringify(
      VUE_MACRO_RUNTIME_FIXTURES.map((fixture) => ({
        id: fixture.id,
        axes: fixture.axes,
        filename: fixture.filename ?? null,
        source: fixture.source,
        supportFiles: fixture.supportFiles ?? {},
      })),
    ),
  );
}

export function generateOracle() {
  if (compilerPackage.version !== VUE_MACRO_ORACLE_VERSION) {
    throw new Error(
      `Vue macro oracle requires @vue/compiler-sfc@${VUE_MACRO_ORACLE_VERSION}, ` +
        `loaded ${compilerPackage.version}`,
    );
  }
  return {
    schemaVersion: VUE_MACRO_ORACLE_SCHEMA_VERSION,
    provenance: {
      generatedBy: "scripts/gen-vue-macro-runtime-oracle.mjs",
      compiler: "@vue/compiler-sfc",
      version: VUE_MACRO_ORACLE_VERSION,
      fixtureSha256: fixtureFingerprint(),
      compileOptions: {
        isProd: false,
        inlineTemplate: false,
      },
    },
    cases: VUE_MACRO_RUNTIME_FIXTURES.map(compileFixture),
  };
}

export function stableOracleJson(oracle) {
  return `${JSON.stringify(oracle, null, 2)}\n`;
}

export function oracleDiff(expected, actual) {
  const expectedText = stableOracleJson(expected);
  const actualText = stableOracleJson(actual);
  if (expectedText === actualText) return null;
  const expectedLines = expectedText.split("\n");
  const actualLines = actualText.split("\n");
  const count = Math.max(expectedLines.length, actualLines.length);
  for (let index = 0; index < count; index += 1) {
    if (expectedLines[index] !== actualLines[index]) {
      return (
        `line ${index + 1}: expected ${JSON.stringify(expectedLines[index])}, ` +
        `actual ${JSON.stringify(actualLines[index])}`
      );
    }
  }
  return "oracle differs";
}
