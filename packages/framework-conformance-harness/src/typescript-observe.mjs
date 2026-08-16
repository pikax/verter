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

/**
 * Thrown when an observed artifact names a module the host cannot resolve.
 *
 * This is a HARD failure, never a degradation. An unresolvable `import("vue")`
 * or `import("svelte")` does not error inside a `.d.ts` under `skipLibCheck` —
 * TypeScript silently types the reference `any`, and two artifacts that differ
 * only in their framework types then observe IDENTICALLY. An observation taken
 * in that state decides nothing, so it is refused instead of returned.
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
 * The OBSERVATION DOMAIN: the realized, pinned framework closure an artifact's
 * module references are given meaning by.
 *
 * `null` is the domain-less domain — for artifacts that reference no external
 * module. Naming a framework roots every virtual artifact INSIDE that
 * framework's isolated oracle install, so TypeScript's OWN node module
 * resolution walks up to that install's `node_modules` and finds the pinned
 * package's real declarations. TypeScript stays the observer; the install
 * supplies the meaning of the imports.
 */
/**
 * The WORKSPACE domain: this repository's own `@verter/*` declaration packages.
 *
 * The IDE/TSX products a carrier publishes are JSX modules whose meaning lives
 * behind `@jsxImportSource @verter/svelte-jsx` and `@verter/types`. Those are
 * workspace packages, not oracle-install packages, and pnpm links them
 * per-consumer — no single directory has both — so the domain is provisioned by
 * mapping each package name to its own on-disk directory through TypeScript's
 * own `paths`. The declarations resolved are the packages' REAL emitted
 * `.d.ts`; nothing is hand-written.
 *
 * Every mapped target is asserted present, so a missing build REFUSES the
 * observation rather than silently degrading a reference to `any`.
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
    // Bare specifier and subpaths both map to the package's own directory, so
    // TypeScript reads the package's `types` / `exports` entries itself.
    paths[name] = [directory];
    paths[`${name}/*`] = [path.join(directory, "*")];
  }
  if (missing.length > 0) {
    throw new WorkspaceDomainError(missing);
  }
  return paths;
}

/**
 * Thrown when a workspace declaration package the observation domain needs is
 * not present on disk. A hard failure for the same reason
 * [`ModuleResolutionError`] is: an absent package types every reference to it
 * `any`, and the observation would decide nothing.
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
    // Rooted inside the repository so a relative import between artifacts and
    // any real workspace file resolves the way a consumer's would.
    return {
      framework: "workspace",
      installDir: REPO_ROOT,
      root: path.join(REPO_ROOT, "__verter_observed__"),
      packageVersion: null,
      paths: workspacePackagePaths(),
      // The IDE products ARE JSX modules: without this the `@jsxImportSource`
      // pragma is inert and the projection's element types are never checked.
      jsx: true,
    };
  }
  const { installDir } = ensureOracleDomain(framework);
  // The pinned package's own manifest version — recorded in the observation's
  // identity so two observations taken against different closures are never
  // reported as comparable results of the same query.
  const manifestPath = path.join(installDir, "node_modules", framework, "package.json");
  const manifest = JSON.parse(ts.sys.readFile(manifestPath) ?? "{}");
  return {
    framework,
    installDir,
    // A directory INSIDE the install, so node resolution from an artifact
    // reaches `<installDir>/node_modules`.
    root: path.join(installDir, "__verter_observed__"),
    packageVersion: manifest.version ?? null,
    paths: undefined,
    jsx: false,
  };
}

/**
 * The compiler options ONE observation runs under.
 *
 * `checkDeclarationFiles` turns `skipLibCheck` OFF. That matters for the
 * declaration-only claim: a runtime value statement inside a `.d.ts` is
 * `TS1036`, but `skipLibCheck` suppresses every error raised inside a
 * declaration file — so under the default options a surface carrying real
 * runtime code observes exactly like an ambient-clean one. Diagnostics stay
 * filtered to the observed artifacts, so the pinned framework closure's own
 * declarations never contribute noise.
 */
function compilerOptionsFor(checkDeclarationFiles, domain) {
  const options = { ...COMPILER_OPTIONS, skipLibCheck: !checkDeclarationFiles };
  // `paths` values are ABSOLUTE, so no `baseUrl` is needed (and `baseUrl` is
  // deprecated in this TypeScript).
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
      // A virtual artifact's own directory need not exist on disk; every other
      // directory question is the real filesystem's.
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
 * Normalize a relocated relative path into the portable form the observation
 * identity carries.
 *
 * `path.relative` yields BACKSLASHES on Windows, and an identity carrying
 * platform-specific separators gives the SAME observation a different
 * `queryIdentity` per platform — two machines observing one tree would compare
 * as divergent for no semantic reason.
 *
 * Splitting on `path.sep` cannot be exercised on a POSIX machine, where `sep`
 * is already `/` and the normalization is a no-op. Splitting on BOTH separators
 * makes the rule platform-independent AND drivable everywhere: a
 * Windows-shaped path normalizes identically on Linux and macOS, so the
 * behaviour is testable rather than merely asserted.
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
 * Every module reference an artifact makes, enumerated by TYPESCRIPT ITSELF.
 *
 * `ts.preProcessFile(text, readImportFiles, detectJavaScriptImports)` is the
 * compiler's own scanner for "what does this file reference". Using it — rather
 * than a hand-written AST walk — is what makes the enumeration COMPLETE by
 * construction instead of by assertion: a reference form a walk forgot is a
 * bypass, and a hand-written walk cannot prove it forgot nothing.
 * `preProcessFile` reports, in `importedFiles`, every one of: import
 * declarations, `export … from`, `export *`, `import("x")` type nodes,
 * `typeof import("x")`, `declare module "x"` augmentations,
 * `import x = require("x")`, dynamic `import("x")` expressions, and
 * `require("x")` calls; and, in three SEPARATE channels,
 * `/// <reference types="x" />` (`typeReferenceDirectives`),
 * `/// <reference path="x" />` (`referencedFiles`) and
 * `/// <reference lib="x" />` (`libReferenceDirectives`). ALL FOUR channels are
 * gated below: a channel left ungated is a hole of exactly the kind this
 * function exists to close — an unresolvable `path` reference surfaces only as
 * a `TS6053` diagnostic and the observation proceeds against a file that was
 * never read.
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
 * The lib names TypeScript itself accepts, taken from the compiler's own
 * `libMap` when it is exposed and derived from `libs` otherwise — never a
 * hand-written list.
 */
const KNOWN_LIB_NAMES = new Set((ts.libs ?? []).map((name) => String(name).toLowerCase()));

/**
 * FAIL-CLOSED module-resolution gate. Runs before any type is read, so an
 * observation is either taken in a fully-resolved domain or refused outright.
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
    // A `path` reference is a plain relative file reference. Its declaration
    // content affects checker output, so accepting a disk-only target would let
    // an input absent from the observation identity change the result.
    for (const reference of pathReferences) {
      const targetIdentity = toIdentityPath(path.resolve(path.dirname(fileName), reference));
      if (!fileIdentities.has(targetIdentity)) unresolved.push({ fileName, specifier: reference });
    }
    // A `lib` reference names a built-in lib file; TypeScript maps the name to
    // `lib.<name>.d.ts` and its miss is likewise diagnostic-only.
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

/**
 * TypeScript spells a unique-symbol-keyed member `__@<symbol>@<id>`, where
 * `<id>` is a PER-CHECKER symbol id — `__@brand@63` in one program and
 * `__@brand@144` in the next, for the same member of the same pinned type.
 * Two observations of identical artifacts would then never compare equal.
 *
 * The symbol NAME is the semantic part and is kept; only the per-process id is
 * dropped. This is the same class the default-lib member exclusion above
 * already guards against, surfacing again now that framework declarations
 * participate in the observation.
 */
function stableMemberName(name) {
  const branded = /^__@(.+)@\d+$/.exec(name);
  return branded === null ? name : `__@${branded[1]}`;
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
    members[stableMemberName(property.getName())] = {
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
export function observeTypeScript(artifacts, options = {}) {
  const domain = resolveDomain(options.frameworkDomain ?? null);
  // Rooting: a domain-less observation keeps the caller's own rooted names; a
  // framework observation relocates every artifact INSIDE the pinned install so
  // TypeScript's own node resolution reaches its `node_modules`. The relocation
  // is a pure prefix, so relative imports between artifacts are unaffected.
  const relocate = (fileName) =>
    domain.root === "/" ? fileName : path.join(domain.root, fileName.replace(/^\/+/, ""));
  const fileMap = new Map(artifacts.map(({ fileName, code }) => [relocate(fileName), code]));
  const compilerOptions = compilerOptionsFor(options.checkDeclarationFiles === true, domain);
  const host = inMemoryHost(fileMap, compilerOptions);
  const program = ts.createProgram([...fileMap.keys()], compilerOptions, host);
  // FAIL-CLOSED, before a single type is read: an artifact naming a module the
  // domain cannot resolve refuses the observation.
  assertModulesResolve(fileMap, host, compilerOptions);
  const checker = program.getTypeChecker();

  const libs = program
    .getSourceFiles()
    .filter((sourceFile) => program.isSourceFileDefaultLibrary(sourceFile))
    .map((sourceFile) => path.basename(sourceFile.fileName))
    .sort();
  // Reported under the CALLER's spelling: the relocation is an environment
  // detail, and an install path baked into the record would make two runs on
  // different machines incomparable for no semantic reason.
  const unrelocate = (fileName) =>
    domain.root === "/" ? fileName : toIdentityPath(path.relative(domain.root, fileName));
  const inputs = [...fileMap.entries()]
    .map(([fileName, code]) => ({ fileName: unrelocate(fileName), sha256: sha256(code) }))
    .sort((a, b) => a.fileName.localeCompare(b.fileName));
  // The domain is part of the observation IDENTITY: two observations taken
  // against different framework closures are not comparable results of the
  // same query, and `compareObservations` reports the drift like any other.
  const observationDomain = {
    framework: domain.framework,
    packageVersion: domain.packageVersion,
  };
  const normalizedOptions = {
    ...NORMALIZED_COMPILER_OPTIONS,
    skipLibCheck: compilerOptions.skipLibCheck,
    jsx: compilerOptions.jsx === undefined ? null : ts.JsxEmit[compilerOptions.jsx],
    // The mapped package NAMES enter the identity; their absolute directories
    // deliberately do not, so two machines observing the same tree compare.
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
