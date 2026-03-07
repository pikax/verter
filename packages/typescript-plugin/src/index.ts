import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import fs from "node:fs";
import { isRelativeVue, isVue, isRelativeVueTs } from "./helpers/utils";
import { parseFile, FALLBACK_STUB } from "./helpers/getDtsSnapshot";
import { VERTER_TYPES_STUB } from "./helpers/verterTypesStub";

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  const create = (info: tsModule.server.PluginCreateInfo) => {
    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();

    // Eagerly test NAPI loading at plugin creation time
    try {
      const native: typeof import("@verter/native") = require("@verter/native");
      const testHost = new native.VerterHost();
      logger.info(`[Verter] NAPI binary loaded successfully (VerterHost created)`);
      // Let it be GC'd — getDtsSnapshot creates its own host
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      logger.info(`[Verter] NAPI binary load FAILED: ${msg}`);
      logger.info(`[Verter] .vue files will use fallback types (DefineComponent<{}, {}>)`);
    }

    // TypeScript plugins have a `cwd` of `/`, which causes issues with import resolution.
    process.chdir(directory);

    // Virtual @verter/types resolution: serve a stub when not installed in user's project.
    // Generated TSX imports from "@verter/types" — without this, TS shows "Cannot find module" errors.
    const verterTypesVirtualPath = normalizePath(
      path.join(directory, "node_modules", "@verter", "types", "index.d.ts"),
    );
    const verterTypesInstalled = fs.existsSync(
      path.join(directory, "node_modules", "@verter", "types", "index.d.ts"),
    );
    if (!verterTypesInstalled) {
      logger.info("[Verter] @verter/types not installed, will serve virtual stub");
    }

    // Patch module resolution on the existing host (no Proxy, no new language service).
    // This is the standard TypeScript plugin pattern that works across TS 5.x/5.8/5.9.
    if (info.languageServiceHost.resolveModuleNameLiterals) {
      const _resolveModuleNameLiterals = info.languageServiceHost.resolveModuleNameLiterals.bind(
        info.languageServiceHost,
      );

      info.languageServiceHost.resolveModuleNameLiterals = (moduleNames, containingFile, ...rest) => {
        const resolvedModules = _resolveModuleNameLiterals(moduleNames, containingFile, ...rest);

        const moduleResolver = createModuleResolver(containingFile);

        return moduleNames.map(({ text: moduleName }, index) => {
          try {
            const resolvedModule = moduleResolver(moduleName, () => resolvedModules[index] as any);
            if (resolvedModule) return { resolvedModule };
          } catch (e) {
            // @ts-expect-error
            logger.msg(e.toString(), "Err");
            return resolvedModules[index];
          }
          return resolvedModules[index];
        });
      };
    }

    const createModuleResolver =
      (containingFile: string) =>
      (
        moduleName: string,
        resolveModule: () =>
          | (tsModule.ResolvedModuleWithFailedLookupLocations & {
              failedLookupLocations: readonly string[];
            })
          | undefined,
      ): tsModule.ResolvedModuleFull | undefined => {
        // Resolve @verter/types to virtual stub when not installed
        if (moduleName === "@verter/types" && !verterTypesInstalled) {
          return {
            extension: ts.Extension.Dts,
            isExternalLibraryImport: true,
            resolvedFileName: verterTypesVirtualPath,
          };
        }

        // Handle .vue.ts imports (IDE codegen rewrites .vue → .vue.ts so the
        // type provider resolves to the public API output, not the IDE .vue.tsx)
        if (isRelativeVueTs(moduleName)) {
          const resolved = path.resolve(path.dirname(containingFile), moduleName);
          logger.info(
            "[Verter] createModuleResolver relative vue.ts - " +
              moduleName +
              " -- " +
              resolved,
          );
          return {
            extension: ts.Extension.Ts,
            isExternalLibraryImport: false,
            resolvedFileName: resolved,
          };
        }

        if (isRelativeVue(moduleName)) {
          const resolved = path.resolve(path.dirname(containingFile), moduleName);
          logger.info(
            "[Verter] createModuleResolver relative vue - " +
              moduleName +
              " -- " +
              resolved,
          );
          return {
            extension: ts.Extension.Ts,
            isExternalLibraryImport: false,
            resolvedFileName: resolved + ".ts",
          };
        }
        if (!isVue(moduleName)) {
          return;
        }

        const resolvedModule = resolveModule();

        logger.info(
          "[Verter] createModuleResolver vue - " + resolvedModule + " -- " + resolvedModule,
        );
        if (!resolvedModule) return;

        const baseUrl = info.project.getCompilerOptions().baseUrl;
        const match = "/index.ts";

        const failedLocations = resolvedModule.failedLookupLocations;
        const vueModulePath = failedLocations.find(
          (x) => (baseUrl ? x.includes(baseUrl) : true) && x.endsWith(match) && fs.existsSync(x),
        );

        if (!vueModulePath) return;
        return {
          extension: ts.Extension.Dts,
          isExternalLibraryImport: false,
          resolvedFileName: path.resolve(vueModulePath),
        };
      };

    // Override jsx to "preserve" — with react-jsx, TypeScript maps JSX children
    // to a 'children' prop that Vue's HTMLAttributes doesn't include (TS2322).
    // "preserve" mode avoids this while still loading JSX types via jsxImportSource.
    const _getCompilationSettings = info.languageServiceHost.getCompilationSettings.bind(
      info.languageServiceHost,
    );
    info.languageServiceHost.getCompilationSettings = () => {
      const settings = _getCompilationSettings();
      return { ...settings, jsx: ts.JsxEmit.Preserve };
    };

    // Patch file system on the shared server host for virtual file support.
    const _readFile = info.serverHost.readFile.bind(info.serverHost);
    info.serverHost.readFile = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return VERTER_TYPES_STUB;
      }
      // .vue.d.ts virtual file → read underlying .vue, return getPublicApi()
      if (fileName.endsWith(".vue.d.ts")) {
        const vuePath = fileName.slice(0, -5); // strip ".d.ts"
        const file = _readFile(vuePath);
        if (file) return parseFile(vuePath, file, logger);
        return FALLBACK_STUB;
      }
      // .vue.ts virtual file → same as .vue.d.ts (IDE codegen uses .vue.ts suffix)
      if (fileName.endsWith(".vue.ts")) {
        const vuePath = fileName.slice(0, -3); // strip ".ts"
        const file = _readFile(vuePath);
        if (file) return parseFile(vuePath, file, logger);
        return FALLBACK_STUB;
      }
      // Direct .vue reads (for other TS operations)
      const file = _readFile(fileName);
      if (isVue(fileName) && file) {
        logger.info("[Verter] readFile - " + fileName + " -- " + file!.length);
        return parseFile(fileName, file, logger);
      }
      return file;
    };

    const _fileExists = info.serverHost.fileExists.bind(info.serverHost);
    info.serverHost.fileExists = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return true;
      }
      if (fileName.endsWith(".vue.d.ts")) {
        return _fileExists(fileName.slice(0, -5)); // check if .vue exists
      }
      if (fileName.endsWith(".vue.ts")) {
        return _fileExists(fileName.slice(0, -3)); // check if .vue exists
      }
      return _fileExists(fileName);
    };

    // Wrap the existing language service — standard plugin pattern.
    // Do NOT create a new ts.createLanguageService() — that breaks TS 5.8+
    // because the standalone LS's internal project lacks a resolutionCache.
    const languageService = info.languageService;

    /** Strip virtual suffixes (.vue.d.ts → .vue, .vue.ts → .vue) */
    function fixVuePath(p: string): string {
      if (p.endsWith(".vue.d.ts")) return p.slice(0, -5);
      if (p.endsWith(".vue.ts")) return p.slice(0, -3);
      return p;
    }

    // Fix go-to-definition: .vue.d.ts/.vue.ts → .vue so Ctrl+Click opens the real .vue file
    const _getDefinitionAndBoundSpan =
      languageService.getDefinitionAndBoundSpan.bind(languageService);
    languageService.getDefinitionAndBoundSpan = (fileName, position) => {
      const result = _getDefinitionAndBoundSpan(fileName, position);
      if (result?.definitions) {
        for (const def of result.definitions) {
          def.fileName = fixVuePath(def.fileName);
        }
      }
      return result;
    };

    const _getDefinitionAtPosition =
      languageService.getDefinitionAtPosition.bind(languageService);
    languageService.getDefinitionAtPosition = (fileName, position) => {
      const result = _getDefinitionAtPosition(fileName, position);
      if (result) {
        for (const def of result) {
          def.fileName = fixVuePath(def.fileName);
        }
      }
      return result;
    };

    const _getTypeDefinitionAtPosition =
      languageService.getTypeDefinitionAtPosition.bind(languageService);
    languageService.getTypeDefinitionAtPosition = (fileName, position) => {
      const result = _getTypeDefinitionAtPosition(fileName, position);
      if (result) {
        for (const def of result) {
          def.fileName = fixVuePath(def.fileName);
        }
      }
      return result;
    };

    // Fix auto-import paths: .vue.d.ts → .vue in all completion-related surfaces
    const _getCompletionEntryDetails =
      languageService.getCompletionEntryDetails.bind(languageService);
    languageService.getCompletionEntryDetails = (
      fileName,
      position,
      entryName,
      formatOptions,
      source,
      preferences,
      data,
    ) => {
      const result = _getCompletionEntryDetails(
        fileName,
        position,
        entryName,
        formatOptions,
        source,
        preferences,
        data,
      );
      if (result?.codeActions) {
        for (const action of result.codeActions) {
          // Fix display text (what user sees in tooltip)
          action.description = action.description.replace(/\.vue\.(d\.)?ts/g, ".vue");
          for (const change of action.changes) {
            for (const edit of change.textChanges) {
              // Fix actual import statement text
              edit.newText = edit.newText.replace(/\.vue\.(d\.)?ts(['"])/g, ".vue$2");
            }
          }
        }
      }
      // Fix sourceDisplay if present (shown next to completion item)
      if (result?.sourceDisplay) {
        result.sourceDisplay = result.sourceDisplay.map((part) => ({
          ...part,
          text: part.text.replace(/\.vue\.(d\.)?ts/g, ".vue"),
        }));
      }
      return result;
    };

    // Fix completion list source paths
    const _getCompletionsAtPosition =
      languageService.getCompletionsAtPosition.bind(languageService);
    languageService.getCompletionsAtPosition = (fileName, position, options, formattingSettings) => {
      const result = _getCompletionsAtPosition(fileName, position, options, formattingSettings);
      if (result?.entries) {
        for (const entry of result.entries) {
          // Fix source display text in completion list
          if (entry.sourceDisplay) {
            entry.sourceDisplay = entry.sourceDisplay.map((part) => ({
              ...part,
              text: part.text.replace(/\.vue\.(d\.)?ts/g, ".vue"),
            }));
          }
          // Fix source property (used for grouping)
          if (entry.source?.endsWith(".vue.d.ts")) {
            entry.source = entry.source.slice(0, -5); // strip .d.ts
          } else if (entry.source?.endsWith(".vue.ts")) {
            entry.source = entry.source.slice(0, -3); // strip .ts
          }
        }
      }
      return result;
    };

    return languageService;
  };

  const getExternalFiles = (project: tsModule.server.ConfiguredProject) => {
    const files = project.getFileNames(true, true).filter(isVue);
    project.projectService.logger.info(
      `[Verter] getExternalFiles: ${files.length} .vue file(s) → ${files.length} .vue.ts virtual file(s)`,
    );
    if (files.length > 0) {
      project.projectService.logger.info("[Verter] Got files\n" + files.join("\n"));
    }
    return files.map((f) => f + ".ts");
  };

  return {
    create,
    getExternalFiles,
  };
};

export = init;
