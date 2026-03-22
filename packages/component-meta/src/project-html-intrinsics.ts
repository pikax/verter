import { createRequire } from "node:module";
import { resolve } from "node:path";
import type * as tsTypes from "typescript";
import type { NativeMetaProject } from "./runtime/project-engine.js";

export interface ProjectHtmlIntrinsicMember {
  name: string;
  kind: "attr" | "listener";
  rawType: string;
}

export interface ProjectHtmlIntrinsicTag {
  tag: string;
  members: ProjectHtmlIntrinsicMember[];
}

export interface ProjectHtmlIntrinsicsCatalog {
  fallback?: ProjectHtmlIntrinsicMember[];
  tags: ProjectHtmlIntrinsicTag[];
}

const VIRTUAL_FILE_NAME = "__verter_html_intrinsics__.ts";
const EXCLUDED_ATTR_NAMES = new Set([
  "innerHTML",
  "innerText",
  "key",
  "ref",
  "ref_for",
  "ref_key",
  "textContent",
]);

type TypeScriptModule = typeof import("typescript");

function loadTypeScript(projectRoot: string): TypeScriptModule | null {
  const _require = typeof require === "function" ? require : createRequire(import.meta.url);
  try {
    const entry = _require.resolve("typescript", { paths: [projectRoot] });
    return _require(entry) as TypeScriptModule;
  } catch {
    return null;
  }
}

function cleanTypeText(typeText: string): string {
  return typeText
    .replace(/\s+/g, " ")
    .replace(/\s*\|\s*undefined\b/g, "")
    .trim();
}

function onPropToEventName(name: string): string | null {
  return /^on[A-Z]/.test(name) ? name[2].toLowerCase() + name.slice(3) : null;
}

function compareStrings(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function buildCompilerOptions(
  ts: TypeScriptModule,
  projectRoot: string,
  config?: Record<string, unknown>,
): tsTypes.CompilerOptions {
  const compilerOptionsJson =
    config && typeof config.compilerOptions === "object" && config.compilerOptions
      ? (config.compilerOptions as Record<string, unknown>)
      : {};
  const converted = ts.convertCompilerOptionsFromJson(compilerOptionsJson, projectRoot);
  const options = converted.options;

  if (options.jsx == null) {
    options.jsx = ts.JsxEmit.Preserve;
  }
  if (options.target == null) {
    options.target = ts.ScriptTarget.ESNext;
  }
  if (options.module == null) {
    options.module = ts.ModuleKind.ESNext;
  }
  if (options.moduleResolution == null) {
    options.moduleResolution =
      options.module === ts.ModuleKind.Node16 || options.module === ts.ModuleKind.NodeNext
        ? ts.ModuleResolutionKind.NodeNext
        : ts.ModuleResolutionKind.Bundler;
  }
  options.noEmit = true;
  options.skipLibCheck = true;
  options.allowJs = true;
  options.allowSyntheticDefaultImports ??= true;

  return options;
}

function createProgram(
  ts: TypeScriptModule,
  projectRoot: string,
  compilerOptions: tsTypes.CompilerOptions,
): { program: tsTypes.Program; virtualFilePath: string } {
  const virtualFilePath = resolve(projectRoot, VIRTUAL_FILE_NAME);
  const virtualSource = [
    `import type { JSX } from "vue/jsx";`,
    `import type * as Vue from "vue";`,
    `type __VerterIntrinsicElements = JSX.IntrinsicElements;`,
    `type __VerterHtmlAttributes = Vue.HTMLAttributes;`,
  ].join("\n");
  const defaultHost = ts.createCompilerHost(compilerOptions, true);
  const normalize = (filePath: string) => filePath.replace(/\\/g, "/").toLowerCase();
  const virtualKey = normalize(virtualFilePath);

  const readFile = (filePath: string): string | undefined => {
    if (normalize(filePath) === virtualKey) {
      return virtualSource;
    }
    return defaultHost.readFile(filePath);
  };

  const host: tsTypes.CompilerHost = {
    ...defaultHost,
    getCurrentDirectory() {
      return projectRoot;
    },
    fileExists(filePath) {
      if (normalize(filePath) === virtualKey) {
        return true;
      }
      return defaultHost.fileExists(filePath);
    },
    readFile,
    getSourceFile(fileName, languageVersion, onError, shouldCreateNewSourceFile) {
      if (normalize(fileName) === virtualKey) {
        return ts.createSourceFile(
          fileName,
          virtualSource,
          languageVersion,
          true,
          ts.ScriptKind.TS,
        );
      }
      return defaultHost.getSourceFile(
        fileName,
        languageVersion,
        onError,
        shouldCreateNewSourceFile,
      );
    },
    resolveModuleNames(moduleNames, containingFile) {
      return moduleNames.map((moduleName) => {
        const resolved = ts.resolveModuleName(moduleName, containingFile, compilerOptions, {
          fileExists: (filePath) =>
            normalize(filePath) === virtualKey || defaultHost.fileExists(filePath),
          readFile,
          directoryExists: defaultHost.directoryExists?.bind(defaultHost),
          getCurrentDirectory: () => projectRoot,
          getDirectories: defaultHost.getDirectories?.bind(defaultHost),
          realpath: defaultHost.realpath?.bind(defaultHost),
          useCaseSensitiveFileNames: () => defaultHost.useCaseSensitiveFileNames(),
        });
        return resolved.resolvedModule;
      });
    },
  };

  return {
    program: ts.createProgram([virtualFilePath], compilerOptions, host),
    virtualFilePath,
  };
}

function findTypeAlias(
  ts: TypeScriptModule,
  checker: tsTypes.TypeChecker,
  sourceFile: tsTypes.SourceFile,
  aliasName: string,
): tsTypes.Type | null {
  for (const statement of sourceFile.statements) {
    if (ts.isTypeAliasDeclaration(statement) && statement.name.text === aliasName) {
      return checker.getTypeFromTypeNode(statement.type);
    }
  }
  return null;
}

function extractMembers(
  ts: TypeScriptModule,
  checker: tsTypes.TypeChecker,
  sourceFile: tsTypes.SourceFile,
  type: tsTypes.Type,
): ProjectHtmlIntrinsicMember[] {
  const members = new Map<string, ProjectHtmlIntrinsicMember>();
  const apparentType = checker.getApparentType(type);

  for (const symbol of checker.getPropertiesOfType(apparentType)) {
    const name = symbol.getName();
    if (!name || name.startsWith("__@")) {
      continue;
    }

    const rawType = cleanTypeText(
      checker.typeToString(
        checker.getTypeOfSymbolAtLocation(symbol, sourceFile),
        sourceFile,
        ts.TypeFormatFlags.NoTruncation |
          ts.TypeFormatFlags.UseAliasDefinedOutsideCurrentScope |
          ts.TypeFormatFlags.InTypeAlias,
      ),
    );
    const eventName = onPropToEventName(name);

    if (eventName) {
      members.set(`listener:${eventName}`, {
        name: eventName,
        kind: "listener",
        rawType,
      });
      continue;
    }

    if (EXCLUDED_ATTR_NAMES.has(name)) {
      continue;
    }

    members.set(`attr:${name}`, {
      name,
      kind: "attr",
      rawType,
    });
  }

  return Array.from(members.values()).sort((left, right) => {
    const kindOrder = compareStrings(left.kind, right.kind);
    return kindOrder !== 0 ? kindOrder : compareStrings(left.name, right.name);
  });
}

export async function loadProjectHtmlIntrinsicsCatalog(
  projectRoot: string,
  config?: Record<string, unknown>,
): Promise<ProjectHtmlIntrinsicsCatalog | null> {
  const ts = loadTypeScript(projectRoot);
  if (!ts) {
    return null;
  }

  try {
    const compilerOptions = buildCompilerOptions(ts, projectRoot, config);
    const { program, virtualFilePath } = createProgram(ts, projectRoot, compilerOptions);
    const sourceFile = program.getSourceFile(virtualFilePath);
    if (!sourceFile) {
      return null;
    }

    const checker = program.getTypeChecker();
    const intrinsicElementsType = findTypeAlias(
      ts,
      checker,
      sourceFile,
      "__VerterIntrinsicElements",
    );
    if (!intrinsicElementsType) {
      return null;
    }

    const tags: ProjectHtmlIntrinsicTag[] = checker
      .getPropertiesOfType(checker.getApparentType(intrinsicElementsType))
      .map((tagSymbol) => ({
        tag: tagSymbol.getName(),
        members: extractMembers(
          ts,
          checker,
          sourceFile,
          checker.getTypeOfSymbolAtLocation(tagSymbol, sourceFile),
        ),
      }))
      .filter((tag) => tag.tag.length > 0 && tag.members.length > 0)
      .sort((left, right) => compareStrings(left.tag, right.tag));

    if (tags.length === 0) {
      return null;
    }

    const htmlAttributesType = findTypeAlias(ts, checker, sourceFile, "__VerterHtmlAttributes");
    const fallback =
      htmlAttributesType == null
        ? undefined
        : extractMembers(ts, checker, sourceFile, htmlAttributesType);

    return fallback && fallback.length > 0 ? { fallback, tags } : { tags };
  } catch {
    return null;
  }
}

export async function configureProjectHtmlIntrinsics(
  nativeProject: NativeMetaProject,
  options: { root: string; config?: Record<string, unknown> },
  loader: typeof loadProjectHtmlIntrinsicsCatalog = loadProjectHtmlIntrinsicsCatalog,
): Promise<void> {
  const catalog = await loader(options.root, options.config);
  if (!catalog) {
    return;
  }
  nativeProject.setHtmlIntrinsicsCatalog(JSON.stringify(catalog));
}
