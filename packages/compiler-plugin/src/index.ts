import type tsModule from "typescript/lib/tsserverlibrary";
import path from "path";
import fs from "fs";

import { isRelativeVue, isVue } from "./helpers/vueExtensions.js";
import { getDtsSnapshot } from "./helpers/getDtsSnapshot.js";

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  const create = (info: tsModule.server.PluginCreateInfo) => {
    // Get a list of things to remove from the completion list from the config object.
    // If nothing was specified, we'll just remove 'caller'
    const whatToRemove: string[] = info.config.remove || ["caller"];
    // info.project.projectService.logger.msg('test', tsModule.server.Msg.Err)
    // info.project.projectService.logger.info("Vue 3 Plugin is activated");
    // Set up decorator object
    const languageServiceHost = {} as Partial<tsModule.LanguageServiceHost>;

    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();
    const compilerOptions = info.project.getCompilerOptions();

    const languageServiceHostProxy = new Proxy(info.languageServiceHost, {
      get(target, key: keyof tsModule.LanguageServiceHost) {
        if (key in target || key in languageServiceHost) {
          logger.info("[VERTER] Proxying: " + key);
        }
        return languageServiceHost[key]
          ? languageServiceHost[key]
          : target[key];
      },
    });

    // TypeScript plugins have a `cwd` of `/`, which causes issues with import resolution.
    process.chdir(directory);

    const languageService = ts.createLanguageService(languageServiceHostProxy);

    languageServiceHost.getScriptKind = (fileName) => {
      if (!info.languageServiceHost.getScriptKind) {
        logger.info("[Verter] getScriptKind UNKOWN - " + fileName);
        return ts.ScriptKind.Unknown;
      }
      if (isVue(fileName)) {
        logger.info("[Verter] getScriptKind vue - " + fileName);
        return ts.ScriptKind.TSX;
      }
      return info.languageServiceHost.getScriptKind(fileName);
    };

    languageServiceHost.getScriptVersion = (fileName) => {
      if (isVue(fileName)) {
        logger.info("[Verter] getScriptVersion vue - " + fileName);
        return "0";
      }
      return info.languageServiceHost.getScriptVersion(fileName);
    };

    languageServiceHost.getCustomTransformers = () => {
      logger.info("[Verter] getCustomTransformers");
      return info.languageServiceHost.getCustomTransformers?.() || {};
    };

    languageServiceHost.resolveTypeReferenceDirectiveReferences = (
      typeDirectiveNames,
      containingFile,
      redirectedReference,
      options,
      containingSourceFile,
      reusedNames
    ) => {
      logger.info("[Verter] resolveTypeReferenceDirectiveReferences");
      return info.languageServiceHost.resolveTypeReferenceDirectiveReferences(
        typeDirectiveNames,
        containingFile,
        redirectedReference,
        options,
        containingSourceFile,
        reusedNames
      );
    };
    languageServiceHost.getSourceFile = (fileName) => {
      const r = info.languageServiceHost.getSourceFile(fileName);
      if (r) {
        logger.info("[Verter] getSourceFile - " + fileName);
      }
      return r;
    };

    const _containsFile = info.project.containsFile.bind(info.project);
    info.project.containsFile = (fileName) => {
      const r = _containsFile(fileName);
      logger.info("[Verter] project containsFile - " + fileName + " | " + r);
      return r;
    };
    const _getExternalFiles = info.project.getExternalFiles.bind(info.project);
    info.project.getExternalFiles = () => {
      const r = _getExternalFiles();
      logger.info("[Verter] project getExternalFiles - " + r);
      return r;
    };

    const _getFileNames = info.project.getFileNames.bind(info.project);
    info.project.getFileNames = () => {
      const r = _getFileNames();
      logger.info("[Verter] project getFileNames - " + r);
      return r;
    };
    const _getExcludedFiles = info.project.getExcludedFiles.bind(info.project);
    info.project.getExcludedFiles = () => {
      const r = _getExcludedFiles();
      logger.info("[Verter] project getExcludedFiles - " + r);
      return r;
    };
    const _XgetScriptFileNames = info.project.getScriptFileNames.bind(
      info.project
    );
    info.project.getScriptFileNames = () => {
      const r = _XgetScriptFileNames();
      logger.info("[Verter] project getScriptFileNames - " + r);
      const vuef = Array.from(vueFiles.values());
      logger.info(
        "[Verter]  project getScriptFileNames - Appending: " +
          vuef.length +
          " files" +
          vuef
      );
      return r.concat(...vuef);
      return r;
    };

    // info.project.updateGraph()

    const _getCompilationSettings = info.project.getCompilationSettings.bind(
      info.project
    );
    info.project.getCompilationSettings = () => {
      // logger.info("[Verter] project getCompilationSettings");
      const settings = _getCompilationSettings();
      return {
        ...settings,
        jsx: ts.JsxEmit.Preserve,
      };
    };

    languageServiceHost.getScriptSnapshot = (fileName) => {
      if (isVue(fileName) && fs.existsSync(fileName)) {
        logger.info("[Verter] getScriptSnapshot vue - " + fileName);
        return getDtsSnapshot(
          ts,
          fileName,
          logger
          // compilerOptions,
          // directory
        );
      }
      return info.languageServiceHost.getScriptSnapshot(fileName);
    };

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
        // logger.info(
        //   "[[[" +
        //     moduleName +
        //     "| " +
        //     containingFile +
        //     "| " +
        //     isRelativeVue(moduleName) +
        //     "| " +
        //     isVue(moduleName) +
        //     "]]]"
        // );

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

        // Find the imported CSS module, if it exists.
        const vueModulePath = normalizedLocations.find((location) =>
          fs.existsSync(location)
        );

        logger.info(
          "[Verter] createModuleResolver vue - " +
            resolvedModule +
            " -ModulePath-  " +
            vueModulePath
        );
        if (vueModulePath) {
          logger.info("wwww -- Vue 3 Plugin found path" + vueModulePath);
          return {
            extension: ts.Extension.Tsx,
            isExternalLibraryImport: false,
            resolvedFileName: path.resolve(vueModulePath),
          };
        }

        logger.info("--- Vue 3 Plugin NOT found path" + vueModulePath);

        // const vueModulePath = failedLocations.find(
        //   (x) =>
        //     (baseUrl ? x.includes(baseUrl) : true) &&
        //     x.endsWith(match) &&
        //     fs.existsSync(x)
        // );

        // if (!vueModulePath) return;
        // return {
        //   extension: ts.Extension.Dts,
        //   isExternalLibraryImport: false,
        //   resolvedFileName: path.resolve(vueModulePath),
        // };
      };

    // if (!info.languageServiceHost.resolveModuleNameLiterals) {
    //   info.project.projectService.logger.info("Vue 3 Plugin is not supported");
    //   throw new Error("not supported");
    // }
    // const _resolveModuleNameLiterals =
    //   info.languageServiceHost.resolveModuleNameLiterals.bind(
    //     info.languageServiceHost
    //   );
    // languageServiceHost.resolveModuleNameLiterals = (
    //   moduleNames,
    //   containingFile,
    //   ...rest
    // ) => {
    //   const resolvedModules = _resolveModuleNameLiterals(
    //     moduleNames,
    //     containingFile,
    //     ...rest
    //   );

    //   const moduleResolver = createModuleResolver(containingFile);

    //   return moduleNames.map(({ text: moduleName }, index) => {
    //     try {
    //       const resolvedModule = moduleResolver(
    //         moduleName,
    //         () => resolvedModules[index]
    //       );
    //       if (resolvedModule) return { resolvedModule };
    //     } catch (e) {
    //       logger.info("err" + e);
    //       return resolvedModules[index];
    //     }
    //     return resolvedModules[index];
    //   });
    // };

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
              () => resolvedModules[index]
            );
            if (resolvedModule) return { resolvedModule };
          } catch (e) {
            logger.error(e);
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
              languageServiceHost.getResolvedModuleWithFailedLookupLocationsFromCache?.(
                moduleName,
                containingFile
              )
            );
            if (resolvedModule) return resolvedModule;
          } catch (e) {
            logger.error(e);
            return resolvedModules[index];
          }
          return resolvedModules[index];
        });
      };
    }

    // // Remove specified entries from completion list
    // languageService.getCompletionsAtPosition = (
    //   fileName,
    //   position,
    //   options
    // ) => {
    //   // This is just to let you hook into something to
    //   // see the debugger working
    //   logger.info("getCompletionsAtPosition" + fileName + position + options);

    //   const prior = info.languageService.getCompletionsAtPosition(
    //     fileName,
    //     position,
    //     options
    //   );
    //   if (!prior) return;

    //   const oldLength = prior.entries.length;
    //   prior.entries = prior.entries.filter(
    //     (e) => whatToRemove.indexOf(e.name) < 0
    //   );

    //   // Sample logging for diagnostic purposes
    //   if (oldLength !== prior.entries.length) {
    //     const entriesRemoved = oldLength - prior.entries.length;
    //     info.project.projectService.logger.info(
    //       `Removed ${entriesRemoved} entries from the completion list`
    //     );
    //   }

    //   return prior;
    // };
    const getScriptSnapshot = info.languageServiceHost.getScriptSnapshot.bind(
      info.languageServiceHost
    );
    languageServiceHost.getScriptSnapshot = (fileName) => {
      if (isVue(fileName)) {
        logger.info("[Verter] getScriptSnapshot VUE - " + fileName);
        const dts = getDtsSnapshot(ts, fileName, logger);
        return dts;
      }

      const snap = getScriptSnapshot(fileName);
      return snap;
    };

    logger.info(
      "[Verter] readDirectory - " + !!info.languageServiceHost.readDirectory
    );

    const _readDirectory = info.languageServiceHost.readDirectory
      ? info.languageServiceHost.readDirectory.bind(info.languageServiceHost)
      : undefined;
    languageServiceHost.readDirectory = _readDirectory
      ? (path, extensions, exclude, include, depth) => {
          logger.info("[Verter] readDirectory: " + path);
          return _readDirectory(
            path,
            extensions ? [...extensions, ".vue"] : undefined,
            exclude,
            include,
            depth
          );
        }
      : undefined;

    // const _hasInvalidatedResolutions = info.languageServiceHost.hasInvalidatedResolution;
    // if (_hasInvalidatedResolutions) {
    //   languageServiceHost.hasInvalidatedResolution = (fileName) => {
    //     if (isVue(fileName)) {
    //       logger.info("[Verter] hasInvalidatedResolution - " + fileName);
    //       return true;
    //     }
    //     return _hasInvalidatedResolutions(fileName);
    //   };
    // }

    const _getScriptFileNames =
      info.languageServiceHost.getScriptFileNames.bind(
        info.languageServiceHost
      );
    languageServiceHost.getScriptFileNames = () => {
      const files = _getScriptFileNames();
      logger.info(
        "[Verter] getScriptFileNames: " + files.length + " files" + files
      );

      const vuef = Array.from(vueFiles.values());
      logger.info(
        "[Verter] getScriptFileNames - Appending: " +
          vuef.length +
          " files" +
          vuef
      );
      return files.concat(...vuef);
    };

    // const _resolveModuleNames = languageServiceHost.resolveModuleNames;
    // if (_resolveModuleNames) {
    //   languageServiceHost.resolveModuleNames = (
    //     moduleNames,
    //     containingFile,
    //     ...rest
    //   ) => {
    //     logger.info("resolveModuleNames" + moduleNames + containingFile + rest);
    //     const resolved = _resolveModuleNames(
    //       moduleNames,
    //       containingFile,
    //       ...rest
    //     );
    //     if (containingFile.endsWith(".vue")) {
    //       debugger;
    //     }
    //     return resolved;
    //   };
    // }
    // const _resolveModuleNamesLiterals =
    //   languageServiceHost.resolveModuleNameLiterals;
    // if (_resolveModuleNamesLiterals) {
    //   languageServiceHost.resolveModuleNameLiterals = (
    //     moduleNames,
    //     containingFile,
    //     ...rest
    //   ) => {
    //     logger.info(
    //       "resolveModuleNameLiterals" + moduleNames + containingFile + rest
    //     );
    //     const resolved = _resolveModuleNamesLiterals(
    //       moduleNames,
    //       containingFile,
    //       ...rest
    //     );
    //     if (containingFile.endsWith(".vue")) {
    //       debugger;
    //     }

    //     return resolved.map((x, idx) => {
    //       const moduleName = moduleNames[idx].text;
    //       if (x.resolvedModule || !isVue(moduleName)) {
    //         return x;
    //       }

    //       debugger
    //       return {
    //         resolvedModule: {
    //           extension: ts.Extension.Tsx,
    //           isExternalLibraryImport: false,
    //           resolvedFileName: path.resolve(
    //             path.dirname(containingFile),
    //             moduleName
    //           ),
    //         },
    //       };
    //     });
    //     if (resolved.indexOf((x) => x)) return resolved;
    //   };
    // }

    return languageService;
  };

  const vueFiles = new Set<string>([
    // TODO remove
    "d:/dev/personal/vue-typescript/packages/example/Test.vue",
    "d:\\dev\\personal\\vue-typescript\\packages\\example\\Test.vue",
  ]);

  const getExternalFiles = (project: tsModule.server.ConfiguredProject) => {
    const files = project.getFileNames(true, true).filter(isVue);
    project.projectService.logger.info("Got files\n" + files.join("\n"));
    for (const file of files) {
      vueFiles.add(file);
    }
    return files;
  };

  return {
    create,
    getExternalFiles,
  };
};

export = init;
