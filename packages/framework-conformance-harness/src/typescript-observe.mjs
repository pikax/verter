// TypeScript-observation validator (BF2 owned scope: "diagnostics,
// source-map, and TypeScript-observable product validation" — BF2.md).
//
// This module is the reusable VALIDATOR MECHANISM: it programmatically
// determines what the real TypeScript compiler observes about a set of
// produced artifacts — the exports it sees, the exact types it assigns
// them, and every diagnostic it raises, with full spans — and serializes
// that as a deterministic observation record two artifact sets can be
// compared on. A type that silently changes (a prop type drifting from
// `string` to `number` with no diagnostic anywhere) changes the record and
// is caught.
//
// It drives the REAL pinned `typescript` compiler API over an in-memory
// program — never a mock, never a re-implementation, and never production
// compiler behavior: this package supplies the mechanism only; the blocks
// that produce real candidate TypeScript-visible products own using it for
// actual product conformance results.

import { createHash } from "node:crypto";
import path from "node:path";
import ts from "typescript";

const COMPILER_OPTIONS = {
  strict: true,
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  skipLibCheck: true,
  noEmit: true,
};

/**
 * The NORMALIZED compiler-option record entering the observation: enum
 * values spelled as their stable names (never ordinal numbers, which can
 * renumber across TypeScript releases), every option explicit. A candidate
 * observed under different options is a DIFFERENT observation, not a
 * comparable one.
 */
const NORMALIZED_COMPILER_OPTIONS = Object.freeze({
  strict: true,
  target: ts.ScriptTarget[COMPILER_OPTIONS.target],
  module: ts.ModuleKind[COMPILER_OPTIONS.module],
  moduleResolution: ts.ModuleResolutionKind[COMPILER_OPTIONS.moduleResolution],
  skipLibCheck: true,
  noEmit: true,
});

function sha256(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(value[k])}`).join(",")}}`;
}

function inMemoryHost(fileMap) {
  const base = ts.createCompilerHost(COMPILER_OPTIONS, true);
  return {
    ...base,
    getSourceFile(fileName, languageVersion, ...rest) {
      const code = fileMap.get(fileName);
      if (code !== undefined) return ts.createSourceFile(fileName, code, languageVersion, true);
      return base.getSourceFile(fileName, languageVersion, ...rest);
    },
    readFile(fileName) {
      return fileMap.get(fileName) ?? base.readFile(fileName);
    },
    fileExists(fileName) {
      return fileMap.has(fileName) || base.fileExists(fileName);
    },
    writeFile() {
      throw new Error("TypeScript observation is read-only: nothing is ever emitted");
    },
  };
}

function positionAt(sourceFile, offset) {
  if (sourceFile === undefined || offset === undefined) return null;
  const { line, character } = sourceFile.getLineAndCharacterOfPosition(offset);
  return { line: line + 1, column: character + 1 };
}

function messageChain(messageText) {
  if (typeof messageText === "string") return [messageText];
  const chain = [];
  const stack = [messageText];
  while (stack.length > 0) {
    const link = stack.shift();
    if (!link) continue;
    chain.push(link.messageText);
    for (const next of link.next ?? []) stack.push(next);
  }
  return chain;
}

function canonicalTsDiagnostic(diagnostic) {
  const file = diagnostic.file;
  return {
    kind: ts.DiagnosticCategory[diagnostic.category].toLowerCase(),
    code: diagnostic.code,
    message: messageChain(diagnostic.messageText),
    source: file?.fileName ?? null,
    start: file ? positionAt(file, diagnostic.start) : null,
    end:
      file && diagnostic.start !== undefined && diagnostic.length !== undefined
        ? positionAt(file, diagnostic.start + diagnostic.length)
        : null,
    related: (diagnostic.relatedInformation ?? []).map((info) => ({
      message: messageChain(info.messageText),
      source: info.file?.fileName ?? null,
      start: info.file ? positionAt(info.file, info.start) : null,
      end:
        info.file && info.start !== undefined && info.length !== undefined
          ? positionAt(info.file, info.start + info.length)
          : null,
    })),
  };
}

const TYPE_FORMAT = ts.TypeFormatFlags.NoTruncation | ts.TypeFormatFlags.InTypeAlias;

/** Structural expansion depth for observed export types. A NAMED type's
 * display string alone would hide a member whose type silently changed
 * (`ButtonProps` prints as `ButtonProps` either way), so object-like types
 * are expanded member-by-member to this bounded depth. A NAMED type sitting
 * exactly at the boundary gets one additional expansion hop before the
 * display-string fallback (see `expandType`) — a stated bound, not a hidden
 * sample: an all-named chain is structurally observed to depth
 * EXPANSION_DEPTH + 1. */
const EXPANSION_DEPTH = 3;

/**
 * A type whose display string is just its NAME (a symbol-named
 * interface/class/enum or an alias reference), hiding its structure —
 * as opposed to an anonymous object/union/intersection whose NoTruncation
 * display string already spells the full structure out.
 */
function isNamedType(type) {
  if (type.aliasSymbol !== undefined) return true;
  const symbol = type.getSymbol();
  if (symbol === undefined) return false;
  const name = symbol.getName();
  return name !== "__type" && name !== "__object" && name !== "__function" && name !== "__class";
}

/**
 * Modifier observation for a member symbol. `readonly` is read from the
 * member's declarations' combined modifier flags — the `typeToString` /
 * symbol-flag surface does NOT carry readonly-ness, so without this a
 * readonly-modifier-only change was invisible to the comparison.
 */
function memberModifiers(property) {
  return {
    optional: (property.flags & ts.SymbolFlags.Optional) !== 0,
    readonly: (property.declarations ?? []).some(
      (declaration) => (ts.getCombinedModifierFlags(declaration) & ts.ModifierFlags.Readonly) !== 0,
    ),
  };
}

/**
 * Structural observation of one call or construct signature: every
 * parameter's name/expanded type/optionality plus the expanded return type
 * — recorded through the same `expandType` machinery and depth budget as an
 * ordinary property member, so a signature-only drift (a callable member's
 * parameter or return type changing) alters the observation record exactly
 * like a property drift does.
 */
function expandSignature(program, checker, signature, depth, seen) {
  return {
    parameters: signature.getParameters().map((parameter) => {
      const declaration = parameter.valueDeclaration ?? parameter.declarations?.[0];
      const observed =
        declaration !== undefined
          ? expandType(
              program,
              checker,
              checker.getTypeOfSymbolAtLocation(parameter, declaration),
              depth,
              seen,
            )
          : { display: "<no declaration>" };
      return {
        name: parameter.getName(),
        ...observed,
        optional: (parameter.flags & ts.SymbolFlags.Optional) !== 0,
      };
    }),
    returnType: expandType(program, checker, signature.getReturnType(), depth, seen),
  };
}

/** True when a signature/index declaration belongs to the default lib (that
 * surface is TypeScript's own, not the artifact's — same exclusion as
 * default-lib property members). */
function isDefaultLibDeclaration(program, declaration) {
  return (
    declaration !== undefined && program.isSourceFileDefaultLibrary(declaration.getSourceFile())
  );
}

function expandType(program, checker, type, depth, seen) {
  const display = checker.typeToString(type, undefined, TYPE_FORMAT);
  if (type === undefined || seen.has(type)) return { display };
  // Depth budget. An ANONYMOUS type at the boundary loses nothing to the
  // fallback — its NoTruncation display string spells the whole structure —
  // but a NAMED type's display is only its name, so the named fallback
  // would hide every member beneath it. A named type at depth 0 therefore
  // expands ONE additional hop (its children observe at depth -1, where
  // everything falls back); see EXPANSION_DEPTH's doc for the stated bound.
  if (depth < 0) return { display };
  if (depth === 0 && !isNamedType(type)) return { display };
  const objectLike =
    type.flags & (ts.TypeFlags.Object | ts.TypeFlags.Intersection | ts.TypeFlags.Union);
  if (!objectLike) return { display };
  seen.add(type);
  const members = {};
  for (const property of [...checker.getPropertiesOfType(type)].sort((a, b) =>
    a.getName().localeCompare(b.getName()),
  )) {
    const declaration = property.valueDeclaration ?? property.declarations?.[0];
    if (declaration === undefined) continue;
    // Members declared by the default lib (Array.prototype and friends on
    // tuple/array types) are TypeScript's own surface, not the artifact's —
    // and their internal symbol-name spellings carry per-process ids, so
    // they are excluded from the deterministic observation record.
    if (program.isSourceFileDefaultLibrary(declaration.getSourceFile())) continue;
    const propertyType = checker.getTypeOfSymbolAtLocation(property, declaration);
    members[property.getName()] = {
      ...expandType(program, checker, propertyType, depth - 1, seen),
      ...memberModifiers(property),
    };
  }
  // Callable/construct/index surfaces are members too: a callable
  // interface's return type, a construct signature's produced instance, or
  // an index signature's value type drifting is exactly as observable to
  // TypeScript consumers as a property drift, so each is recorded
  // structurally (declaration order preserved — overload order is
  // semantic).
  const callSignatures = type
    .getCallSignatures()
    .filter((signature) => !isDefaultLibDeclaration(program, signature.getDeclaration()))
    .map((signature) => expandSignature(program, checker, signature, depth - 1, seen));
  const constructSignatures = type
    .getConstructSignatures()
    .filter((signature) => !isDefaultLibDeclaration(program, signature.getDeclaration()))
    .map((signature) => expandSignature(program, checker, signature, depth - 1, seen));
  const indexSignatures = checker
    .getIndexInfosOfType(type)
    .filter((info) => !isDefaultLibDeclaration(program, info.declaration))
    .map((info) => ({
      keyType: expandType(program, checker, info.keyType, depth - 1, seen),
      valueType: expandType(program, checker, info.type, depth - 1, seen),
      readonly: info.isReadonly,
    }));
  seen.delete(type);
  const out = { display };
  if (Object.keys(members).length > 0) out.members = members;
  if (callSignatures.length > 0) out.callSignatures = callSignatures;
  if (constructSignatures.length > 0) out.constructSignatures = constructSignatures;
  if (indexSignatures.length > 0) out.indexSignatures = indexSignatures;
  return out;
}

/**
 * Observes a set of artifacts exactly as TypeScript would. Use ROOTED
 * virtual file names ("/component.ts") so relative imports between
 * artifacts resolve unambiguously.
 *
 * The record captures the FULL observation identity, not only its results:
 * the exact compiler/API version, the normalized compiler options, the
 * referenced default-lib files, the virtual-file inputs (name + content
 * digest), and a queryIdentity digest over all of those — two observation
 * records are comparable results of the SAME query only when those match,
 * and the deep comparison reports any of them drifting exactly like a
 * result drift.
 *
 * @param {Array<{ fileName: string, code: string }>} artifacts the produced
 *   files (.ts / .d.ts / .js as named); every artifact is a root file
 * @returns {{
 *   typescript: { version: string },
 *   compilerOptions: object,
 *   libs: string[],
 *   inputs: Array<{ fileName: string, sha256: string }>,
 *   queryIdentity: string,
 *   diagnostics: Array<object>,
 *   modules: Record<string, { exports: Record<string, { flags: string[], type: object }> }>,
 * }} deterministic observation record
 */
export function observeTypeScript(artifacts) {
  const fileMap = new Map(artifacts.map(({ fileName, code }) => [fileName, code]));
  const host = inMemoryHost(fileMap);
  const program = ts.createProgram([...fileMap.keys()], COMPILER_OPTIONS, host);
  const checker = program.getTypeChecker();

  const libs = program
    .getSourceFiles()
    .filter((sourceFile) => program.isSourceFileDefaultLibrary(sourceFile))
    .map((sourceFile) => path.basename(sourceFile.fileName))
    .sort();
  const inputs = [...fileMap.entries()]
    .map(([fileName, code]) => ({ fileName, sha256: sha256(code) }))
    .sort((a, b) => a.fileName.localeCompare(b.fileName));
  const queryIdentity = sha256(
    stableStringify({
      typescript: { version: ts.version },
      compilerOptions: NORMALIZED_COMPILER_OPTIONS,
      libs,
      inputs,
    }),
  );

  const diagnostics = ts
    .getPreEmitDiagnostics(program)
    .filter((d) => d.file === undefined || fileMap.has(d.file.fileName))
    .map(canonicalTsDiagnostic)
    .sort((a, b) =>
      `${a.source}:${a.start?.line}:${a.start?.column}:${a.code}`.localeCompare(
        `${b.source}:${b.start?.line}:${b.start?.column}:${b.code}`,
      ),
    );

  const modules = {};
  for (const fileName of fileMap.keys()) {
    const sourceFile = program.getSourceFile(fileName);
    const moduleSymbol = sourceFile ? checker.getSymbolAtLocation(sourceFile) : undefined;
    const exports = {};
    if (moduleSymbol !== undefined) {
      for (const symbol of checker.getExportsOfModule(moduleSymbol)) {
        const resolved =
          symbol.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(symbol) : symbol;
        const declaration = resolved.valueDeclaration ?? resolved.declarations?.[0];
        const type =
          declaration !== undefined
            ? expandType(
                program,
                checker,
                resolved.flags & ts.SymbolFlags.Type && !(resolved.flags & ts.SymbolFlags.Value)
                  ? checker.getDeclaredTypeOfSymbol(resolved)
                  : checker.getTypeOfSymbolAtLocation(resolved, declaration),
                EXPANSION_DEPTH,
                new Set(),
              )
            : { display: "<no declaration>" };
        exports[symbol.getName()] = {
          flags: symbolFlagNames(resolved.flags),
          type,
        };
      }
    }
    modules[fileName] = { exports };
  }

  return {
    typescript: { version: ts.version },
    compilerOptions: { ...NORMALIZED_COMPILER_OPTIONS },
    libs,
    inputs,
    queryIdentity,
    diagnostics,
    modules,
  };
}

const OBSERVED_FLAGS = [
  "Variable",
  "Function",
  "Class",
  "Interface",
  "TypeAlias",
  "Enum",
  "Property",
  "Method",
];

function symbolFlagNames(flags) {
  const names = [];
  for (const name of OBSERVED_FLAGS) {
    const bits = name === "Variable" ? ts.SymbolFlags.Variable : ts.SymbolFlags[name];
    if (flags & bits) names.push(name);
  }
  return names;
}

/**
 * Deterministic deep comparison of two observation records. Every
 * difference is reported with a path — an export whose observed TYPE
 * changed with zero diagnostics anywhere still fails.
 *
 * @returns {{ equal: boolean, differences: string[] }}
 */
export function compareObservations(golden, candidate) {
  const differences = [];
  walk(golden, candidate, "$");
  function walk(a, b, path) {
    if (a === b) return;
    if (
      a === null ||
      b === null ||
      typeof a !== typeof b ||
      Array.isArray(a) !== Array.isArray(b)
    ) {
      differences.push(`${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`);
      return;
    }
    if (typeof a !== "object") {
      differences.push(`${path}: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`);
      return;
    }
    if (Array.isArray(a)) {
      if (a.length !== b.length) {
        differences.push(`${path}: length ${a.length} vs ${b.length}`);
        return;
      }
      a.forEach((item, i) => walk(item, b[i], `${path}[${i}]`));
      return;
    }
    for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
      walk(a[key], b[key], `${path}.${key}`);
    }
  }
  return { equal: differences.length === 0, differences };
}
