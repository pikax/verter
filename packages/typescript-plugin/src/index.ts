import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import fs from "node:fs";
import {
  getVueVirtualFileInfo,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  normalizePath,
  resolveVuePublicApiMode,
  stripVueVirtualSuffix,
  toVueVirtualFileName,
} from "./helpers/utils";
import {
  parseFile,
  FALLBACK_STUB,
  remapVirtualSpan,
} from "./helpers/getDtsSnapshot";
import type { MacroTypeDependencyAccess } from "./helpers/macroTypeHydration";
import { isTestFileWithContext } from "./helpers/testFileDetection";
import { VERTER_TYPES_STUB } from "./helpers/verterTypesStub";

function normalizeSourcePath(fileName: string): string {
  return stripVueVirtualSuffix(fileName);
}

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  const create = (info: tsModule.server.PluginCreateInfo) => {
    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();
    const exposeBindingsTesting = info.config?.exposeBindingsTesting === true;

    try {
      const native: typeof import("@verter/native") = require("@verter/native");
      const testHost = new native.VerterHost();
      logger.info("[Verter] NAPI binary loaded successfully (VerterHost created)");
      void testHost;
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      logger.info(`[Verter] NAPI binary load FAILED: ${msg}`);
      logger.info("[Verter] .vue files will use fallback types (DefineComponent<{}, {}>)");
    }

    process.chdir(directory);

    const verterTypesVirtualPath = normalizePath(
      path.join(directory, "node_modules", "@verter", "types", "index.d.ts"),
    );
    const verterTypesInstalled = fs.existsSync(
      path.join(directory, "node_modules", "@verter", "types", "index.d.ts"),
    );
    if (!verterTypesInstalled) {
      logger.info("[Verter] @verter/types not installed, will serve virtual stub");
    }

    const _fileExists = info.serverHost.fileExists.bind(info.serverHost);
    const _readFile = info.serverHost.readFile.bind(info.serverHost);

    const resolvePublicApiMode = (containingFile: string) =>
      resolveVuePublicApiMode(
        exposeBindingsTesting,
        containingFile,
        (sourceFileName) =>
          isTestFileWithContext(normalizeSourcePath(sourceFileName), {
            fileExists: _fileExists,
            readFile: _readFile,
          }),
      );

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
        const publicApiMode = resolvePublicApiMode(containingFile);

        if (moduleName === "@verter/types" && !verterTypesInstalled) {
          return {
            extension: ts.Extension.Dts,
            isExternalLibraryImport: true,
            resolvedFileName: verterTypesVirtualPath,
          };
        }

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
            resolvedFileName: toVueVirtualFileName(resolved, publicApiMode),
          };
        }

        if (!isVue(moduleName)) {
          return;
        }

        const resolvedModule = resolveModule();
        logger.info(
          "[Verter] createModuleResolver vue - " + resolvedModule + " -- " + resolvedModule,
        );
        if (!resolvedModule) {
          return;
        }

        const baseUrl = info.project.getCompilerOptions().baseUrl;
        const failedLocations = resolvedModule.failedLookupLocations;
        const vueModulePath = failedLocations.find(
          (candidate) =>
            (baseUrl ? candidate.includes(baseUrl) : true) &&
            candidate.endsWith("/index.ts") &&
            fs.existsSync(candidate),
        );

        if (!vueModulePath) {
          return;
        }

        const resolvedVueModulePath = normalizePath(path.resolve(vueModulePath));
        return {
          extension: ts.Extension.Dts,
          isExternalLibraryImport: false,
          resolvedFileName: isVue(resolvedVueModulePath)
            ? toVueVirtualFileName(resolvedVueModulePath, publicApiMode)
            : resolvedVueModulePath,
        };
      };

    if (info.languageServiceHost.resolveModuleNameLiterals) {
      const _resolveModuleNameLiterals = info.languageServiceHost.resolveModuleNameLiterals.bind(
        info.languageServiceHost,
      );

      info.languageServiceHost.resolveModuleNameLiterals = (
        moduleNames,
        containingFile,
        ...rest
      ) => {
        const resolvedModules = _resolveModuleNameLiterals(
          moduleNames,
          containingFile,
          ...rest,
        );
        const moduleResolver = createModuleResolver(containingFile);

        return moduleNames.map(({ text: moduleName }, index) => {
          try {
            const resolvedModule = moduleResolver(
              moduleName,
              () => resolvedModules[index] as any,
            );
            if (resolvedModule) {
              return { resolvedModule };
            }
          } catch (e) {
            logger.info(`[Verter] module resolution override failed: ${String(e)}`);
            return resolvedModules[index];
          }
          return resolvedModules[index];
        });
      };
    }

    const _getCompilationSettings = info.languageServiceHost.getCompilationSettings.bind(
      info.languageServiceHost,
    );
    info.languageServiceHost.getCompilationSettings = () => {
      const settings = _getCompilationSettings();
      return { ...settings, jsx: ts.JsxEmit.Preserve };
    };

    const macroTypeAccess: MacroTypeDependencyAccess = {
      resolveModule(containingFile, specifier) {
        const resolved = ts.resolveModuleName(
          specifier,
          normalizeSourcePath(containingFile),
          info.project.getCompilerOptions(),
          {
            fileExists: _fileExists,
            readFile: _readFile,
            directoryExists: info.serverHost.directoryExists?.bind(info.serverHost),
            getCurrentDirectory: () => directory,
            getDirectories: info.serverHost.getDirectories?.bind(info.serverHost),
            realpath: info.serverHost.realpath?.bind(info.serverHost),
            useCaseSensitiveFileNames: () => info.serverHost.useCaseSensitiveFileNames,
          },
        );
        return resolved.resolvedModule
          ? normalizeSourcePath(resolved.resolvedModule.resolvedFileName)
          : undefined;
      },
      readSource(fileName) {
        const normalized = normalizeSourcePath(fileName);
        const snapshot =
          info.languageServiceHost.getScriptSnapshot?.(normalized) ??
          (normalized !== fileName
            ? info.languageServiceHost.getScriptSnapshot?.(fileName)
            : undefined);
        if (snapshot) {
          return snapshot.getText(0, snapshot.getLength());
        }
        return _readFile(normalized) ?? (normalized !== fileName ? _readFile(fileName) : undefined);
      },
    };

    info.serverHost.readFile = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return VERTER_TYPES_STUB;
      }

      const virtualInfo = getVueVirtualFileInfo(fileName);
      if (virtualInfo) {
        const file = _readFile(virtualInfo.sourceFileName);
        if (file) {
          return parseFile(
            virtualInfo.sourceFileName,
            file,
            logger,
            macroTypeAccess,
            virtualInfo.mode,
          );
        }
        return FALLBACK_STUB;
      }

      const file = _readFile(fileName);
      if (isVue(fileName) && file) {
        logger.info("[Verter] readFile - " + fileName + " -- " + file.length);
        return parseFile(fileName, file, logger, macroTypeAccess);
      }
      return file;
    };

    info.serverHost.fileExists = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return true;
      }

      const virtualInfo = getVueVirtualFileInfo(fileName);
      if (virtualInfo) {
        return _fileExists(virtualInfo.sourceFileName);
      }

      return _fileExists(fileName);
    };

    const languageService = info.languageService;

    function fixVuePath(fileName: string): string {
      return stripVueVirtualSuffix(fileName);
    }

    function cleanupVueVirtualImportPath(text: string): string {
      return text
        .replace(/\.vue\.__verter_test\.ts/g, ".vue")
        .replace(/\.vue\.(d\.)?ts/g, ".vue");
    }

    function remapDefinitionLike<
      T extends {
        fileName: string;
        textSpan: tsModule.TextSpan;
        contextSpan?: tsModule.TextSpan;
        originalTextSpan?: tsModule.TextSpan;
      },
    >(definition: T): T {
      if (definition.fileName.endsWith(".vue")) {
        return definition;
      }

      const remapped = remapVirtualSpan(definition.fileName, definition.textSpan, (target) =>
        _readFile(target),
      );

      if (!remapped) {
        definition.fileName = fixVuePath(definition.fileName);
        return definition;
      }

      definition.fileName = remapped.fileName;
      definition.textSpan = remapped.textSpan;
      if (definition.contextSpan) {
        definition.contextSpan = remapped.textSpan;
      }
      if (definition.originalTextSpan) {
        definition.originalTextSpan = remapped.textSpan;
      }
      return definition;
    }

    const _getDefinitionAndBoundSpan =
      languageService.getDefinitionAndBoundSpan.bind(languageService);
    languageService.getDefinitionAndBoundSpan = (fileName, position) => {
      const result = _getDefinitionAndBoundSpan(fileName, position);
      if (result?.definitions) {
        for (const def of result.definitions) {
          remapDefinitionLike(def);
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
          remapDefinitionLike(def);
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
          remapDefinitionLike(def);
        }
      }
      return result;
    };

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
          action.description = cleanupVueVirtualImportPath(action.description);
          for (const change of action.changes) {
            for (const edit of change.textChanges) {
              edit.newText = cleanupVueVirtualImportPath(edit.newText);
            }
          }
        }
      }
      if (result?.sourceDisplay) {
        result.sourceDisplay = result.sourceDisplay.map((part) => ({
          ...part,
          text: cleanupVueVirtualImportPath(part.text),
        }));
      }
      return result;
    };

    const _getCompletionsAtPosition =
      languageService.getCompletionsAtPosition.bind(languageService);
    languageService.getCompletionsAtPosition = (
      fileName,
      position,
      options,
      formattingSettings,
    ) => {
      const result = _getCompletionsAtPosition(
        fileName,
        position,
        options,
        formattingSettings,
      );
      if (result?.entries) {
        for (const entry of result.entries) {
          if (entry.sourceDisplay) {
            entry.sourceDisplay = entry.sourceDisplay.map((part) => ({
              ...part,
              text: cleanupVueVirtualImportPath(part.text),
            }));
          }
          if (entry.source) {
            entry.source = fixVuePath(entry.source);
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
      `[Verter] getExternalFiles: ${files.length} .vue file(s) -> ${files.length} .vue.ts virtual file(s)`,
    );
    if (files.length > 0) {
      project.projectService.logger.info("[Verter] Got files\n" + files.join("\n"));
    }
    return files.map((fileName) => fileName + ".ts");
  };

  return {
    create,
    getExternalFiles,
  };
};

export = init;
