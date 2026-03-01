import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import fs from "node:fs";
import { isRelativeVue, isVue } from "./helpers/utils";
import { parseFile } from "./helpers/getDtsSnapshot";
import { VERTER_TYPES_STUB } from "./helpers/verterTypesStub";

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  const create = (info: tsModule.server.PluginCreateInfo) => {
    const languageServiceHost = {} as Partial<tsModule.LanguageServiceHost>;
    const languageServiceHostProxy = new Proxy(info.languageServiceHost, {
      get(target, key: keyof tsModule.LanguageServiceHost) {
        return languageServiceHost[key] ? languageServiceHost[key] : target[key];
      },
    });

    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();

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

    const languageService = ts.createLanguageService(languageServiceHostProxy);

    if (info.languageServiceHost.resolveModuleNameLiterals) {
      const _resolveModuleNameLiterals = info.languageServiceHost.resolveModuleNameLiterals.bind(
        info.languageServiceHost,
      );

      languageServiceHost.resolveModuleNameLiterals = (moduleNames, containingFile, ...rest) => {
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

        if (isRelativeVue(moduleName)) {
          logger.info(
            "[Verter] createModuleResolver relative vue - " +
              moduleName +
              " -- " +
              path.resolve(path.dirname(containingFile), moduleName),
          );
          return {
            extension: ts.Extension.Tsx,
            isExternalLibraryImport: false,
            resolvedFileName: path.resolve(path.dirname(containingFile), moduleName),
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

    const _readFile = info.serverHost.readFile.bind(info.serverHost);
    info.serverHost.readFile = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return VERTER_TYPES_STUB;
      }
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
      return _fileExists(fileName);
    };

    return languageService;
  };

  const getExternalFiles = (project: tsModule.server.ConfiguredProject) => {
    const files = project.getFileNames(true, true).filter(isVue);
    project.projectService.logger.info("[Verter] Got files\n" + files.join("\n"));
    return files;
  };

  return {
    create,
    getExternalFiles,
  };
};

export = init;
