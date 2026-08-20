// TypeScript-observation validator: what the real TypeScript compiler
// observes about a set of produced artifacts — exports, assigned types,
// and every diagnostic with full spans — serialized as a deterministic
// record two artifact sets can be compared on. A type that silently
// changes (`string` → `number` with no diagnostic) changes the record.
//
// Drives the pinned `typescript` compiler API over an in-memory program —
// never a mock or re-implementation. This package supplies the mechanism;
// callers that produce TypeScript-visible products own using it for
// product conformance.

import { createHash } from "node:crypto";
import path from "node:path";
import ts from "typescript";

import { ensureOracleDomain } from "./oracle-install.mjs";
import { REPO_ROOT } from "./paths.mjs";

const COMPILER_OPTIONS = {
  strict: true,
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ESNext,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  skipLibCheck: true,
  noEmit: true,
};

/**
 * Normalized compiler-option record: enum values as stable names (never
 * ordinals, which can renumber across TypeScript releases), every option
 * explicit. Different options are a different observation, not a comparable
 * one.
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

/**
 * Thrown when an observed artifact names a module the host cannot resolve.
 *
 * Hard failure, never a degradation. An unresolvable `import("vue")` /
 * `import("svelte")` does not error inside a `.d.ts` under `skipLibCheck` —
 * TypeScript types the reference `any`, and two artifacts that differ only
 * in framework types then observe identically. Refused, not returned.
 */
export class ModuleResolutionError extends Error {
  constructor(unresolved) {
    super(
      `TypeScript observation refused: ${unresolved.length} module reference(s) do not resolve ` +
        "in the observation domain, so their types would silently degrade to `any`:\n" +
        unresolved
          .map(({ fileName, specifier }) => `  ${fileName}: cannot resolve "${specifier}"`)
          .join("\n"),
    );
    this.name = "ModuleResolutionError";
    this.unresolved = unresolved;
  }
}

/**
 * Workspace observation domain: this repo's `@verter/*` declaration packages.
 *
 * Observation domain is the realized, pinned closure that gives module
 * references meaning. `null` is domain-less (no external modules). Naming
 * a framework roots every virtual artifact inside that framework's isolated
 * oracle install so TypeScript's own node resolution finds the pinned
 * declarations. TypeScript stays the observer; the install supplies import
 * meaning.
 *
 * IDE/TSX products are JSX modules whose meaning lives behind
 * `@jsxImportSource @verter/svelte-jsx` and `@verter/types`. Those are
 * workspace packages, not oracle-install packages, and pnpm links them
 * per-consumer — no single directory has both — so the domain maps each
 * package name to its on-disk directory through TypeScript `paths`. Resolved
 * declarations are the packages' real emitted `.d.ts`. A missing build
 * refuses the observation rather than degrading a reference to `any`.
 */
function workspacePackagePaths() {
  const packages = {
    "@verter/svelte-jsx": path.join(REPO_ROOT, "packages", "svelte-jsx"),
    "@verter/types": path.join(REPO_ROOT, "packages", "types"),
  };
  const missing = [];
  const paths = {};
  for (const [name, directory] of Object.entries(packages)) {
    const manifest = path.join(directory, "package.json");
    if (ts.sys.fileExists(manifest) !== true) {
      missing.push({ name, expected: manifest });
      continue;
    }
    // Bare specifier and subpaths both map to the package directory so
    // TypeScript reads `types` / `exports` itself.
    paths[name] = [directory];
    paths[`${name}/*`] = [path.join(directory, "*")];
  }
  if (missing.length > 0) {
    throw new WorkspaceDomainError(missing);
  }
  return paths;
}

/**
 * Thrown when a workspace declaration package is missing on disk. Same
 * reason as `ModuleResolutionError`: an absent package types every
 * reference `any`, so the observation would decide nothing.
 */
export class WorkspaceDomainError extends Error {
  constructor(missing) {
    super(
      "TypeScript observation refused: the workspace declaration domain is incomplete:\n" +
        missing.map(({ name, expected }) => `  ${name}: no manifest at ${expected}`).join("\n"),
    );
    this.name = "WorkspaceDomainError";
    this.missing = missing;
  }
}

function resolveDomain(framework) {
  if (framework === null || framework === undefined) {
    return {
      framework: null,
      installDir: null,
      root: "/",
      packageVersion: null,
      paths: undefined,
      jsx: false,
    };
  }
  if (framework === "workspace") {
    // Rooted inside the repo so relative imports between artifacts and
    // workspace files resolve as a consumer's would.
    return {
      framework: "workspace",
      installDir: REPO_ROOT,
      root: path.join(REPO_ROOT, "__verter_observed__"),
      packageVersion: null,
      paths: workspacePackagePaths(),
      // IDE products are JSX: without this `@jsxImportSource` is inert and
      // element types are never checked.
      jsx: true,
    };
  }
  const { installDir } = ensureOracleDomain(framework);
  // Pinned package manifest version — part of observation identity so
  // different closures are never compared as the same query.
  const manifestPath = path.join(installDir, "node_modules", framework, "package.json");
  const manifest = JSON.parse(ts.sys.readFile(manifestPath) ?? "{}");
  return {
    framework,
    installDir,
    // Inside the install so node resolution from an artifact reaches
    // `<installDir>/node_modules`.
    root: path.join(installDir, "__verter_observed__"),
    packageVersion: manifest.version ?? null,
    paths: undefined,
    jsx: false,
  };
}

/**
 * Compiler options one observation runs under.
 *
 * `checkDeclarationFiles` turns `skipLibCheck` off. A runtime value
 * statement inside a `.d.ts` is `TS1036`, but `skipLibCheck` suppresses
 * every error in a declaration file — so a surface carrying real runtime
 * code would observe like an ambient-clean one. Diagnostics stay filtered
 * to the observed artifacts; pinned framework declarations add no noise.
 */
function compilerOptionsFor(checkDeclarationFiles, domain) {
  const options = { ...COMPILER_OPTIONS, skipLibCheck: !checkDeclarationFiles };
  // `paths` values are absolute — no `baseUrl` (deprecated in this TypeScript).
  if (domain.paths !== undefined) options.paths = domain.paths;
  if (domain.jsx) options.jsx = ts.JsxEmit.ReactJSX;
  return options;
}

function inMemoryHost(fileMap, compilerOptions) {
  const base = ts.createCompilerHost(compilerOptions, true);
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
    directoryExists(directoryName) {
      // A virtual artifact's directory need not exist on disk; other
      // directory questions are the real filesystem's.
      for (const fileName of fileMap.keys()) {
        if (path.dirname(fileName) === directoryName) return true;
      }
      return base.directoryExists ? base.directoryExists(directoryName) : false;
    },
    writeFile() {
      throw new Error("TypeScript observation is read-only: nothing is ever emitted");
    },
  };
}

/**
 * Normalize a relocated relative path into the portable observation-identity
 * form.
 *
 * `path.relative` yields backslashes on Windows; platform-specific
 * separators would give the same observation a different `queryIdentity`
 * per platform. Splitting on `path.sep` is a no-op on POSIX. Splitting on
 * both separators makes the rule platform-independent and testable: a
 * Windows-shaped path normalizes identically on Linux and macOS.
 *
 * @param {string} relativePath a path relative to the domain root
 * @returns {string} a leading-slash, forward-slash-only identity path
 */
export function toIdentityPath(relativePath) {
  return `/${relativePath
    .split(/[\\/]+/)
    .filter(Boolean)
    .join("/")}`;
}

/**
 * Every module reference an artifact makes, enumerated by TypeScript itself.
 *
 * `ts.preProcessFile` is the compiler's own scanner. A hand-written AST walk
 * cannot prove it forgot nothing. `importedFiles` covers import declarations,
 * `export … from`, `export *`, `import("x")` type nodes, `typeof import("x")`,
 * `declare module "x"`, `import x = require("x")`, dynamic `import("x")`,
 * and `require("x")`. Three more channels: `typeReferenceDirectives`,
 * `referencedFiles`, `libReferenceDirectives`. All four are gated: an
 * unresolvable `path` reference otherwise surfaces only as `TS6053` and the
 * observation proceeds against a file that was never read.
 */
function moduleReferencesOf(code) {
  const preprocessed = ts.preProcessFile(code, true, true);
  return {
    modules: (preprocessed.importedFiles ?? []).map((reference) => reference.fileName),
    typeDirectives: (preprocessed.typeReferenceDirectives ?? []).map(
      (reference) => reference.fileName,
    ),
    pathReferences: (preprocessed.referencedFiles ?? []).map((reference) => reference.fileName),
    libReferences: (preprocessed.libReferenceDirectives ?? []).map(
      (reference) => reference.fileName,
    ),
  };
}

/**
 * Lib names TypeScript itself accepts (`libMap` when exposed, else `libs`)
 * — never a hand-written list.
 */
const KNOWN_LIB_NAMES = new Set((ts.libs ?? []).map((name) => String(name).toLowerCase()));

/**
 * Fail-closed module-resolution gate. Runs before any type is read: fully
 * resolved or refused.
 */
function assertModulesResolve(fileMap, host, compilerOptions) {
  const unresolved = [];
  const fileIdentities = new Set([...fileMap.keys()].map(toIdentityPath));
  for (const [fileName, code] of fileMap.entries()) {
    const { modules, typeDirectives, pathReferences, libReferences } = moduleReferencesOf(code);
    for (const specifier of modules) {
      const resolved = ts.resolveModuleName(specifier, fileName, compilerOptions, host);
      if (resolved.resolvedModule === undefined) unresolved.push({ fileName, specifier });
    }
    for (const directive of typeDirectives) {
      const resolved = ts.resolveTypeReferenceDirective(directive, fileName, compilerOptions, host);
      if (resolved.resolvedTypeReferenceDirective === undefined) {
        unresolved.push({ fileName, specifier: directive });
      }
    }
    // `path` references are relative files whose content affects checker
    // output; a disk-only target would let an input absent from observation
    // identity change the result.
    for (const reference of pathReferences) {
      const targetIdentity = toIdentityPath(path.resolve(path.dirname(fileName), reference));
      if (!fileIdentities.has(targetIdentity)) unresolved.push({ fileName, specifier: reference });
    }
    // `lib` references name built-in `lib.<name>.d.ts`; a miss is
    // diagnostic-only unless gated here.
    for (const reference of libReferences) {
      if (!KNOWN_LIB_NAMES.has(reference.toLowerCase())) {
        unresolved.push({ fileName, specifier: reference });
      }
    }
  }
  if (unresolved.length > 0) throw new ModuleResolutionError(unresolved);
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

/**
 * Structural expansion depth for observed export types. A named type's
 * display string hides member drift (`ButtonProps` prints as `ButtonProps`
 * either way), so object-like types expand member-by-member to this bound.
 * A named type at the boundary gets one extra hop before the display-string
 * fallback (see `expandType`): an all-named chain is observed to
 * EXPANSION_DEPTH + 1.
 */
const EXPANSION_DEPTH = 3;

/**
 * Display string is just the type name (symbol-named interface/class/enum
 * or alias), hiding structure — unlike an anonymous object/union/intersection
 * whose NoTruncation display already spells the structure.
 */
function isNamedType(type) {
  if (type.aliasSymbol !== undefined) return true;
  const symbol = type.getSymbol();
  if (symbol === undefined) return false;
  const name = symbol.getName();
  return name !== "__type" && name !== "__object" && name !== "__function" && name !== "__class";
}

/**
 * Member modifiers. `readonly` comes from combined declaration modifier
 * flags — `typeToString` / symbol flags do not carry it, so a
 * readonly-only change would otherwise be invisible.
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
 * Structural observation of one call or construct signature: each
 * parameter's name/expanded type/optionality plus expanded return type,
 * through the same `expandType` depth budget as a property, so
 * signature-only drift alters the record like property drift.
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

/**
 * True when a signature/index declaration belongs to the default lib
 * (TypeScript's surface, not the artifact's — same exclusion as default-lib
 * property members).
 */
function isDefaultLibDeclaration(program, declaration) {
  return (
    declaration !== undefined && program.isSourceFileDefaultLibrary(declaration.getSourceFile())
  );
}

/**
 * Unique-symbol-keyed members are spelled `__@<symbol>@<id>`, where `<id>`
 * is a per-checker symbol id (`__@brand@63` vs `__@brand@144` for the same
 * member). Keep the symbol name; drop the per-process id so identical
 * artifacts compare equal.
 */
function stableMemberName(name) {
  const branded = /^__@(.+)@\d+$/.exec(name);
  return branded === null ? name : `__@${branded[1]}`;
}

function expandType(program, checker, type, depth, seen) {
  const display = checker.typeToString(type, undefined, TYPE_FORMAT);
  if (type === undefined || seen.has(type)) return { display };
  // Depth budget. An anonymous type at the boundary loses nothing (NoTruncation
  // already spells the structure); a named type's display is only its name.
  // A named type at depth 0 expands one extra hop (children at depth -1).
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
    // Default-lib members (Array.prototype on tuple/array types) are
    // TypeScript's surface and carry per-process symbol ids.
    if (program.isSourceFileDefaultLibrary(declaration.getSourceFile())) continue;
    const propertyType = checker.getTypeOfSymbolAtLocation(property, declaration);
    members[stableMemberName(property.getName())] = {
      ...expandType(program, checker, propertyType, depth - 1, seen),
      ...memberModifiers(property),
    };
  }
  // Callable/construct/index surfaces are members too; record them
  // structurally (declaration order preserved — overload order is semantic).
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
 * Observe a set of artifacts exactly as TypeScript would. Use rooted virtual
 * file names (`/component.ts`) so relative imports resolve unambiguously.
 *
 * The record captures full observation identity, not only results:
 * compiler/API version, normalized options, default-lib files, virtual-file
 * inputs (name + content digest), and a `queryIdentity` digest over those.
 * Two records are the same query only when those match; identity drift is
 * reported like a result drift.
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
export function observeTypeScript(artifacts, options = {}) {
  const domain = resolveDomain(options.frameworkDomain ?? null);
  // Domain-less keeps the caller's rooted names; a framework observation
  // relocates artifacts inside the pinned install so TypeScript node
  // resolution reaches its `node_modules`. Pure prefix — relative imports
  // between artifacts are unaffected.
  const relocate = (fileName) =>
    domain.root === "/" ? fileName : path.join(domain.root, fileName.replace(/^\/+/, ""));
  const fileMap = new Map(artifacts.map(({ fileName, code }) => [relocate(fileName), code]));
  const compilerOptions = compilerOptionsFor(options.checkDeclarationFiles === true, domain);
  const host = inMemoryHost(fileMap, compilerOptions);
  const program = ts.createProgram([...fileMap.keys()], compilerOptions, host);
  // Fail-closed before any type is read: an unresolvable module refuses.
  assertModulesResolve(fileMap, host, compilerOptions);
  const checker = program.getTypeChecker();

  const libs = program
    .getSourceFiles()
    .filter((sourceFile) => program.isSourceFileDefaultLibrary(sourceFile))
    .map((sourceFile) => path.basename(sourceFile.fileName))
    .sort();
  // Report under the caller's spelling: an install path in the record would
  // make two machines incomparable.
  const unrelocate = (fileName) =>
    domain.root === "/" ? fileName : toIdentityPath(path.relative(domain.root, fileName));
  const inputs = [...fileMap.entries()]
    .map(([fileName, code]) => ({ fileName: unrelocate(fileName), sha256: sha256(code) }))
    .sort((a, b) => a.fileName.localeCompare(b.fileName));
  // Domain is part of observation identity: different closures are not the
  // same query.
  const observationDomain = {
    framework: domain.framework,
    packageVersion: domain.packageVersion,
  };
  const normalizedOptions = {
    ...NORMALIZED_COMPILER_OPTIONS,
    skipLibCheck: compilerOptions.skipLibCheck,
    jsx: compilerOptions.jsx === undefined ? null : ts.JsxEmit[compilerOptions.jsx],
    // Mapped package names enter the identity; absolute directories do not.
    pathMappings:
      compilerOptions.paths === undefined ? null : Object.keys(compilerOptions.paths).sort(),
  };
  const queryIdentity = sha256(
    stableStringify({
      typescript: { version: ts.version },
      compilerOptions: normalizedOptions,
      observationDomain,
      libs,
      inputs,
    }),
  );

  const diagnostics = ts
    .getPreEmitDiagnostics(program)
    .filter((d) => d.file === undefined || fileMap.has(d.file.fileName))
    .map(canonicalTsDiagnostic)
    .map((diagnostic) => ({
      ...diagnostic,
      source: diagnostic.source === null ? null : unrelocate(diagnostic.source),
    }))
    .sort((a, b) =>
      `${a.source}:${a.start?.line}:${a.start?.column}:${a.code}`.localeCompare(
        `${b.source}:${b.start?.line}:${b.start?.column}:${b.code}`,
      ),
    );

  const modules = {};
  for (const fileName of fileMap.keys()) {
    const observedName = unrelocate(fileName);
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
    modules[observedName] = { exports };
  }

  return {
    typescript: { version: ts.version },
    compilerOptions: { ...normalizedOptions },
    observationDomain,
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
 * difference is reported with a path — a type-only export change with
 * zero diagnostics still fails.
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
