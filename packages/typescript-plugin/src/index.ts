import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import fs from "node:fs";
import { isRelativeVue, isVue } from "./helpers/utils";
import { parseFile } from "./helpers/getDtsSnapshot";

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  const create = (info: tsModule.server.PluginCreateInfo) => {
    const languageServiceHost = {} as Partial<tsModule.LanguageServiceHost>;
    const languageServiceHostProxy = new Proxy(info.languageServiceHost, {
      get(target, key: keyof tsModule.LanguageServiceHost) {
        // if (key in target || key in languageServiceHost) {
        //   logger.info("[VERTER] Proxying: " + key);
        // }
        return languageServiceHost[key]
          ? languageServiceHost[key]
          : target[key];
      },
    });

    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();
    const compilerOptions = info.project.getCompilerOptions();

    // TypeScript plugins have a `cwd` of `/`, which causes issues with import resolution.
    process.chdir(directory);

    const languageService = ts.createLanguageService(languageServiceHostProxy);

    // TypeScript 5.x
    if (info.languageServiceHost.resolveModuleNameLiterals) {
      const _resolveModuleNameLiterals =
        info.languageServiceHost.resolveModuleNameLiterals.bind(
          info.languageServiceHost
        );

      languageServiceHost.resolveModuleNameLiterals = (
        moduleNames,
        containingFile,
        ...rest
      ) => {
        const resolvedModules = _resolveModuleNameLiterals(
          moduleNames,
          containingFile,
          ...rest
        );

        const moduleResolver = createModuleResolver(containingFile);

        return moduleNames.map(({ text: moduleName }, index) => {
          try {
            const resolvedModule = moduleResolver(
              moduleName,
              () => resolvedModules[index] as any
            );
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
    // TypeScript 4.x
    else if (info.languageServiceHost.resolveModuleNames) {
      const _resolveModuleNames =
        info.languageServiceHost.resolveModuleNames.bind(
          info.languageServiceHost
        );

      languageServiceHost.resolveModuleNames = (
        moduleNames,
        containingFile,
        ...rest
      ) => {
        const resolvedModules = _resolveModuleNames(
          moduleNames,
          containingFile,
          ...rest
        );

        const moduleResolver = createModuleResolver(containingFile);

        return moduleNames.map((moduleName, index) => {
          try {
            const resolvedModule = moduleResolver(moduleName, () =>
              // @ts-expect-error
              languageServiceHost.getResolvedModuleWithFailedLookupLocationsFromCache?.(
                moduleName,
                containingFile
              )
            );
            if (resolvedModule) return resolvedModule;
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
          | undefined
      ): tsModule.ResolvedModuleFull | undefined => {
        if (isRelativeVue(moduleName)) {
          logger.info(
            "[Verter] createModuleResolver relative vue - " +
              moduleName +
              " -- " +
              path.resolve(path.dirname(containingFile), moduleName)
          );
          return {
            extension: ts.Extension.Tsx,
            isExternalLibraryImport: false,
            resolvedFileName: path.resolve(
              path.dirname(containingFile),
              moduleName
            ),
          };
        }
        if (!isVue(moduleName)) {
          return;
        }

        const resolvedModule = resolveModule();

        logger.info(
          "[Verter] createModuleResolver vue - " +
            resolvedModule +
            " -- " +
            resolvedModule
        );
        if (!resolvedModule) return;

        const baseUrl = info.project.getCompilerOptions().baseUrl;
        const match = "/index.ts";

        const failedLocations = resolvedModule.failedLookupLocations;
        // Filter to only one extension type, and remove that extension. This leaves us with the actual file name.
        // Example: "usr/person/project/src/dir/File.module.css/index.d.ts" > "usr/person/project/src/dir/File.module.css"
        const normalizedLocations = failedLocations.reduce<string[]>(
          (locations, location) => {
            if (
              (baseUrl ? location.includes(baseUrl) : true) &&
              location.endsWith(match)
            ) {
              return [...locations, location.replace(match, "")];
            }
            return locations;
          },
          []
        );

        // // Find the imported CSS module, if it exists.
        // const vueModulePath = normalizedLocations.find((location) =>
        //   fs.existsSync(location)
        // );

        // logger.info(
        //   "[Verter] createModuleResolver vue - " +
        //     resolvedModule +
        //     " -ModulePath-  " +
        //     vueModulePath
        // );
        // if (vueModulePath) {
        //   logger.info("wwww -- Vue 3 Plugin found path" + vueModulePath);
        //   return {
        //     extension: ts.Extension.Tsx,
        //     isExternalLibraryImport: false,
        //     resolvedFileName: path.resolve(vueModulePath),
        //   };
        // }

        // logger.info("--- Vue 3 Plugin NOT found path" + vueModulePath);

        const vueModulePath = failedLocations.find(
          (x) =>
            (baseUrl ? x.includes(baseUrl) : true) &&
            x.endsWith(match) &&
            fs.existsSync(x)
        );

        if (!vueModulePath) return;
        return {
          extension: ts.Extension.Dts,
          isExternalLibraryImport: false,
          resolvedFileName: path.resolve(vueModulePath),
        };
      };

    // patching
    const _readFile = info.serverHost.readFile.bind(info.serverHost);
    info.serverHost.readFile = (fileName: string) => {
      const file = _readFile(fileName);
      if (isVue(fileName) && file) {
        logger.info("[Verter] readFile - " + fileName + " -- " + file!.length);
        return parseFile(fileName, file, logger);
      }
      return file;
    };

    // languageServiceHost.getScriptSnapshot = (fileName: string) => {
    //   if (isVue(fileName) && fs.existsSync(fileName)) {
    //     logger.info("[Verter] getScriptSnapshot vue - " + fileName);
    //     // return getDtsSnapshot(
    //     //   ts,
    //     //   fileName,
    //     //   logger
    //     //   // compilerOptions,
    //     //   // directory
    //     // );
    //     return info.languageServiceHost.getScriptSnapshot(fileName);
    //   }
    //   return info.languageServiceHost.getScriptSnapshot(fileName);
    // };

    // /patching
    return languageService;
  };

  const getExternalFiles = (project: tsModule.server.ConfiguredProject) => {
    const files = project.getFileNames(true, true).filter(isVue);
    project.projectService.logger.info(
      "[Verter] Got files\n" + files.join("\n")
    );
    return files;
  };

  return {
    create,
    getExternalFiles,
  };
};

export = init;
