import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
// File access uses info.serverHost (TS server's filesystem) in sync contexts
// and workspace (Rust VFS) in async contexts.
import {
  cleanupCarrierVirtualImportPath,
  getVueVirtualFileInfo,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  normalizePath,
  resolveVuePublicApiMode,
  stripVueVirtualSuffix,
  stripVueVirtualSuffixBackingAware,
  toVueVirtualFileName,
} from "./helpers/utils";
import { parseFile, FALLBACK_STUB, remapVirtualSpan } from "./helpers/getDtsSnapshot";
import type { MacroTypeDependencyAccess } from "./helpers/macroTypeHydration";
import {
  getAliasedNavigationResult,
  getAliasedQuickInfo,
  getModuleSpecifierNavigationResult,
  retargetAliasedDefinitionInfos,
} from "./helpers/barrelNavigation";
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
    const verterTypesInstalled = info.serverHost.fileExists(
      path.join(directory, "node_modules", "@verter", "types", "index.d.ts"),
    );
    if (!verterTypesInstalled) {
      logger.info("[Verter] @verter/types not installed, will serve virtual stub");
    }

    const _fileExists = info.serverHost.fileExists.bind(info.serverHost);
    const _readFile = info.serverHost.readFile.bind(info.serverHost);

    // Backing-file-aware carrier virtual-suffix strip bound to the UNDERLYING
    // host `fileExists`. Unlike the pure-shape `normalizeSourcePath`, this only
    // strips an AMBIGUOUS `X.svelte.ts` virtual suffix when the backing
    // `X.svelte` carrier exists, so a real standalone rune module is never
    // collapsed into a phantom component path on the resolve / hydration path.
    const stripBackingAware = (fileName: string): string =>
      stripVueVirtualSuffixBackingAware(fileName, _fileExists);

    const resolvePublicApiMode = (containingFile: string) =>
      resolveVuePublicApiMode(exposeBindingsTesting, containingFile, (sourceFileName) =>
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
            "[Verter] createModuleResolver relative vue.ts - " + moduleName + " -- " + resolved,
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
            "[Verter] createModuleResolver relative vue - " + moduleName + " -- " + resolved,
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
            info.serverHost.fileExists(candidate),
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
        const resolvedModules = _resolveModuleNameLiterals(moduleNames, containingFile, ...rest);
        const moduleResolver = createModuleResolver(containingFile);

        return moduleNames.map(({ text: moduleName }, index) => {
          try {
            const resolvedModule = moduleResolver(moduleName, () => resolvedModules[index] as any);
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
          stripBackingAware(containingFile),
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
          ? stripBackingAware(resolved.resolvedModule.resolvedFileName)
          : undefined;
      },
      readSource(fileName) {
        const normalized = stripBackingAware(fileName);
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
      // The UNDERLYING host existence predicate (not the carrier-virtual-aware
      // override installed below). Macro-type hydration uses this to prove a
      // backing carrier source exists before normalising an AMBIGUOUS
      // `X.svelte.ts` virtual suffix, so a real rune module is never corrupted
      // into a phantom `X.svelte` component path.
      fileExists(fileName) {
        return _fileExists(fileName);
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
            directory,
            macroTypeAccess,
            virtualInfo.mode,
          );
        }
        // `{carrier}.ts` is AMBIGUOUS for Svelte — `store.svelte.ts` is a REAL
        // first-class rune module (a non-component carrier with its own rune
        // surface) when no backing `store.svelte` source exists, NOT a component
        // API virtual. Fall through to the real file rather than returning a
        // synthesized carrier API for the phantom component.
        return _readFile(fileName);
      }

      const file = _readFile(fileName);
      if (isVue(fileName) && file) {
        logger.info("[Verter] readFile - " + fileName + " -- " + file.length);
        return parseFile(fileName, file, logger, directory, macroTypeAccess);
      }
      return file;
    };

    info.serverHost.fileExists = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return true;
      }

      const virtualInfo = getVueVirtualFileInfo(fileName);
      if (virtualInfo) {
        // The carrier virtual file exists iff its backing carrier source does;
        // otherwise (a real `.svelte.ts` first-class rune module — a non-component
        // carrier with its own rune surface) check the real file.
        return _fileExists(virtualInfo.sourceFileName) || _fileExists(fileName);
      }

      return _fileExists(fileName);
    };

    const languageService = info.languageService;

    function fixVuePath(fileName: string): string {
      // Backing-file-aware: a navigation / completion target that is a real
      // `store.svelte.ts` rune module (no backing `store.svelte`) keeps its own
      // path, so go-to-definition does not jump to a phantom `store.svelte`.
      return stripBackingAware(fileName);
    }

    function remapDefinitionLike<
      T extends {
        fileName: string;
        textSpan: tsModule.TextSpan;
        contextSpan?: tsModule.TextSpan;
        originalTextSpan?: tsModule.TextSpan;
      },
    >(definition: T): T {
      if (isVue(definition.fileName)) {
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

    function getProgramSourceContext(fileName: string): {
      checker: tsModule.TypeChecker;
      sourceFile: tsModule.SourceFile;
    } | null {
      const program = languageService.getProgram?.();
      if (!program) {
        return null;
      }

      const sourceFile = program.getSourceFile(fileName);
      if (!sourceFile) {
        return null;
      }

      return {
        checker: program.getTypeChecker(),
        sourceFile,
      };
    }

    function getIdentifierTextAtPosition(
      sourceFile: tsModule.SourceFile,
      position: number,
    ): string | undefined {
      const runtimeTs = ts as typeof tsModule & {
        getTouchingPropertyName?: (
          sourceFile: tsModule.SourceFile,
          position: number,
        ) => tsModule.Node | undefined;
        getTokenAtPosition?: (
          sourceFile: tsModule.SourceFile,
          position: number,
        ) => tsModule.Node | undefined;
      };

      const token =
        runtimeTs.getTouchingPropertyName?.(sourceFile, position) ??
        runtimeTs.getTokenAtPosition?.(sourceFile, position);
      if (!token) {
        return undefined;
      }

      const text = token.getText(sourceFile);
      return /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(text) ? text : undefined;
    }

    function resolveModuleFileName(containingFile: string, moduleName: string): string | undefined {
      const publicApiMode = resolvePublicApiMode(containingFile);

      if (moduleName === "@verter/types" && !verterTypesInstalled) {
        return verterTypesVirtualPath;
      }

      if (isRelativeVueTs(moduleName)) {
        return path.resolve(path.dirname(containingFile), moduleName);
      }

      if (isRelativeVue(moduleName)) {
        const resolved = path.resolve(path.dirname(containingFile), moduleName);
        return toVueVirtualFileName(resolved, publicApiMode);
      }

      const result = ts.resolveModuleName(
        moduleName,
        stripBackingAware(containingFile),
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

      return result.resolvedModule?.resolvedFileName;
    }

    function retargetAliasedDefinitions(
      definitions: readonly tsModule.DefinitionInfo[] | undefined,
      preferredName?: string,
    ): tsModule.DefinitionInfo[] | undefined {
      if (!definitions?.length) {
        return undefined;
      }

      const program = languageService.getProgram?.();
      if (!program) {
        return [...definitions];
      }

      return (
        retargetAliasedDefinitionInfos(
          ts,
          program.getTypeChecker(),
          (candidateFileName) => program.getSourceFile(candidateFileName),
          definitions,
          preferredName,
        ) ?? [...definitions]
      );
    }

    function getPreferredRetargetName(
      fileName: string,
      position: number,
      context?: { sourceFile: tsModule.SourceFile } | null,
    ): string | undefined {
      const sourceFile = context?.sourceFile ?? getProgramSourceContext(fileName)?.sourceFile;
      return sourceFile ? getIdentifierTextAtPosition(sourceFile, position) : undefined;
    }

    const _getDefinitionAndBoundSpan =
      languageService.getDefinitionAndBoundSpan.bind(languageService);
    languageService.getDefinitionAndBoundSpan = (fileName, position) => {
      const context = getProgramSourceContext(fileName);
      if (context) {
        const aliased = getAliasedNavigationResult(
          ts,
          context.checker,
          context.sourceFile,
          position,
        );
        if (aliased?.definitions.length) {
          for (const def of aliased.definitions) {
            remapDefinitionLike(def);
          }
          return {
            textSpan: aliased.textSpan,
            definitions: aliased.definitions,
          };
        }

        const moduleNavigation = getModuleSpecifierNavigationResult(
          ts,
          context.sourceFile,
          position,
          (moduleName) => resolveModuleFileName(fileName, moduleName),
        );
        if (moduleNavigation?.definitions.length) {
          for (const def of moduleNavigation.definitions) {
            remapDefinitionLike(def);
          }
          return {
            textSpan: moduleNavigation.textSpan,
            definitions: moduleNavigation.definitions,
          };
        }
      }

      const result = _getDefinitionAndBoundSpan(fileName, position);
      if (result?.definitions) {
        const preferredName = getPreferredRetargetName(fileName, position, context);
        const definitions = retargetAliasedDefinitions(result.definitions, preferredName) ?? [
          ...result.definitions,
        ];
        for (const def of definitions) {
          remapDefinitionLike(def);
        }
        result.definitions = definitions;
      }
      return result;
    };

    const _getDefinitionAtPosition = languageService.getDefinitionAtPosition.bind(languageService);
    languageService.getDefinitionAtPosition = (fileName, position) => {
      const context = getProgramSourceContext(fileName);
      if (context) {
        const aliased = getAliasedNavigationResult(
          ts,
          context.checker,
          context.sourceFile,
          position,
        );
        if (aliased?.definitions.length) {
          for (const def of aliased.definitions) {
            remapDefinitionLike(def);
          }
          return aliased.definitions;
        }

        const moduleNavigation = getModuleSpecifierNavigationResult(
          ts,
          context.sourceFile,
          position,
          (moduleName) => resolveModuleFileName(fileName, moduleName),
        );
        if (moduleNavigation?.definitions.length) {
          for (const def of moduleNavigation.definitions) {
            remapDefinitionLike(def);
          }
          return moduleNavigation.definitions;
        }
      }

      const result = _getDefinitionAtPosition(fileName, position);
      if (result) {
        const preferredName = getPreferredRetargetName(fileName, position, context);
        const definitions = retargetAliasedDefinitions(result, preferredName) ?? [...result];
        for (const def of definitions) {
          remapDefinitionLike(def);
        }
        return definitions;
      }
      return result;
    };

    const _getTypeDefinitionAtPosition =
      languageService.getTypeDefinitionAtPosition.bind(languageService);
    languageService.getTypeDefinitionAtPosition = (fileName, position) => {
      const context = getProgramSourceContext(fileName);
      if (context) {
        const aliased = getAliasedNavigationResult(
          ts,
          context.checker,
          context.sourceFile,
          position,
        );
        if (aliased?.definitions.length) {
          for (const def of aliased.definitions) {
            remapDefinitionLike(def);
          }
          return aliased.definitions;
        }
      }

      const result = _getTypeDefinitionAtPosition(fileName, position);
      if (result) {
        const preferredName = getPreferredRetargetName(fileName, position, context);
        const definitions = retargetAliasedDefinitions(result, preferredName) ?? [...result];
        for (const def of definitions) {
          remapDefinitionLike(def);
        }
        return definitions;
      }
      return result;
    };

    const _getQuickInfoAtPosition = languageService.getQuickInfoAtPosition.bind(languageService);
    languageService.getQuickInfoAtPosition = (fileName, position) => {
      const originalQuickInfo = _getQuickInfoAtPosition(fileName, position);
      const context = getProgramSourceContext(fileName);
      if (context) {
        const quickInfo = getAliasedQuickInfo(
          ts,
          { getQuickInfoAtPosition: _getQuickInfoAtPosition },
          context.checker,
          context.sourceFile,
          position,
        );
        if (quickInfo) {
          return quickInfo;
        }
      }

      const originalDefinitions = _getDefinitionAtPosition(fileName, position);
      const preferredName = getPreferredRetargetName(fileName, position, context);
      const retargeted = retargetAliasedDefinitions(originalDefinitions, preferredName);
      if (originalDefinitions?.length && retargeted?.length) {
        const original = originalDefinitions[0];
        const target = retargeted[0];
        if (
          target.fileName !== original.fileName ||
          target.textSpan.start !== original.textSpan.start ||
          target.textSpan.length !== original.textSpan.length
        ) {
          const targetQuickInfo = _getQuickInfoAtPosition(target.fileName, target.textSpan.start);
          if (targetQuickInfo) {
            return {
              ...targetQuickInfo,
              textSpan: originalQuickInfo?.textSpan ?? targetQuickInfo.textSpan,
            };
          }
        }
      }

      return originalQuickInfo;
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
          action.description = cleanupCarrierVirtualImportPath(action.description, _fileExists);
          for (const change of action.changes) {
            for (const edit of change.textChanges) {
              edit.newText = cleanupCarrierVirtualImportPath(edit.newText, _fileExists);
            }
          }
        }
      }
      if (result?.sourceDisplay) {
        result.sourceDisplay = result.sourceDisplay.map((part) => ({
          ...part,
          text: cleanupCarrierVirtualImportPath(part.text, _fileExists),
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
      const result = _getCompletionsAtPosition(fileName, position, options, formattingSettings);
      if (result?.entries) {
        for (const entry of result.entries) {
          if (entry.sourceDisplay) {
            entry.sourceDisplay = entry.sourceDisplay.map((part) => ({
              ...part,
              text: cleanupCarrierVirtualImportPath(part.text, _fileExists),
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
