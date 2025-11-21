import { createLanguageServiceEnvironment } from "@volar/language-server/lib/project/simpleProject";
import { createConnection, createServer } from "@volar/language-server/node";
import {
  createLanguage,
  createParsedCommandLine,
  createParsedCommandLineByJson,
  createVueLanguagePlugin,
} from "@vue/language-core";
import {
  createLanguageService,
  createUriMap,
  createVueLanguageServicePlugins,
} from "@vue/language-service";
import { URI } from "vscode-uri";

if (process.argv.includes("--version")) {
  console.log(require("./package.json").version);
} else {
  let ts;
  for (const arg of process.argv) {
    if (arg.startsWith("--tsdk=")) {
      const tsdk = arg.substring("--tsdk=".length);
      const tsPath = require.resolve("./typescript.js", { paths: [tsdk] });
      ts = require(tsPath);
      break;
    }
  }
  ts ??= require("typescript");
  console.log("Using TypeScript version:", ts.version, startServer);
  startServer(ts);
}

export function startServer(ts) {
  const connection = createConnection();
  const server = createServer(connection);
  const tsserverRequestHandlers = new Map();
  let tsserverRequestId = 0;
  connection.listen();
  connection.onNotification("tsserver/response", ([id, res]) => {
    var _a;
    (_a = tsserverRequestHandlers.get(id)) === null || _a === void 0
      ? void 0
      : _a(res);
    tsserverRequestHandlers.delete(id);
  });
  connection.onInitialize((params) => {
    const tsconfigProjects = createUriMap();
    const file2ProjectInfo = new Map();
    server.fileWatcher.onDidChangeWatchedFiles(({ changes }) => {
      for (const change of changes) {
        const changeUri = URI.parse(change.uri);
        if (tsconfigProjects.has(changeUri)) {
          tsconfigProjects.get(changeUri).dispose();
          tsconfigProjects.delete(changeUri);
          file2ProjectInfo.clear();
        }
      }
    });
    let simpleLanguageService;
    return server.initialize(
      params,
      {
        setup() {},
        async getLanguageService(uri) {
          if (uri.scheme === "file") {
            const fileName = uri.fsPath.replace(/\\/g, "/");
            let projectInfoPromise = file2ProjectInfo.get(fileName);
            if (!projectInfoPromise) {
              projectInfoPromise = sendTsServerRequest(
                "_vue:" + ts.server.protocol.CommandTypes.ProjectInfo,
                {
                  file: fileName,
                  needFileNameList: false,
                }
              );
              file2ProjectInfo.set(fileName, projectInfoPromise);
            }
            const projectInfo = await projectInfoPromise;
            if (projectInfo) {
              const { configFileName } = projectInfo;
              let languageService = tsconfigProjects.get(
                URI.file(configFileName)
              );
              if (!languageService) {
                languageService = createProjectLanguageService(
                  server,
                  configFileName
                );
                tsconfigProjects.set(URI.file(configFileName), languageService);
              }
              return languageService;
            }
          }
          return simpleLanguageService !== null &&
            simpleLanguageService !== void 0
            ? simpleLanguageService
            : (simpleLanguageService = createProjectLanguageService(
                server,
                undefined
              ));
        },
        getExistingLanguageServices() {
          return Promise.all(
            [...tsconfigProjects.values(), simpleLanguageService].filter(
              (promise) => !!promise
            )
          );
        },
        reload() {
          for (const languageService of tsconfigProjects.values()) {
            languageService.dispose();
          }
          tsconfigProjects.clear();
          if (simpleLanguageService) {
            simpleLanguageService.dispose();
            simpleLanguageService = undefined;
          }
        },
      },
      createVueLanguageServicePlugins(ts, {
        collectExtractProps(...args) {
          return sendTsServerRequest("_vue:collectExtractProps", args);
        },
        getComponentDirectives(...args) {
          return sendTsServerRequest("_vue:getComponentDirectives", args);
        },
        getComponentEvents(...args) {
          return sendTsServerRequest("_vue:getComponentEvents", args);
        },
        getComponentNames(...args) {
          return sendTsServerRequest("_vue:getComponentNames", args);
        },
        getComponentProps(...args) {
          return sendTsServerRequest("_vue:getComponentProps", args);
        },
        getComponentSlots(...args) {
          return sendTsServerRequest("_vue:getComponentSlots", args);
        },
        getElementAttrs(...args) {
          return sendTsServerRequest("_vue:getElementAttrs", args);
        },
        getElementNames(...args) {
          return sendTsServerRequest("_vue:getElementNames", args);
        },
        getImportPathForFile(...args) {
          return sendTsServerRequest("_vue:getImportPathForFile", args);
        },
        isRefAtPosition(...args) {
          return sendTsServerRequest("_vue:isRefAtPosition", args);
        },
        resolveModuleName(...args) {
          return sendTsServerRequest("_vue:resolveModuleName", args);
        },
        getDocumentHighlights(fileName, position) {
          return sendTsServerRequest(
            "_vue:documentHighlights-full",
            Object.assign(Object.assign({ file: fileName }, { position }), {
              filesToSearch: [fileName],
            })
          );
        },
        getEncodedSemanticClassifications(fileName, span) {
          return sendTsServerRequest(
            "_vue:encodedSemanticClassifications-full",
            Object.assign(Object.assign({ file: fileName }, span), {
              format: ts.SemanticClassificationFormat.TwentyTwenty,
            })
          );
        },
        async getQuickInfoAtPosition(fileName, { line, character }) {
          const result = await sendTsServerRequest(
            "_vue:" + ts.server.protocol.CommandTypes.Quickinfo,
            {
              file: fileName,
              line: line + 1,
              offset: character + 1,
            }
          );
          return result === null || result === void 0
            ? void 0
            : result.displayString;
        },
      })
    );
    async function sendTsServerRequest(command, args) {
      return await new Promise((resolve) => {
        const requestId = ++tsserverRequestId;
        tsserverRequestHandlers.set(requestId, resolve);
        connection.sendNotification("tsserver/request", [
          requestId,
          command,
          args,
        ]);
      });
    }
    function createProjectLanguageService(server, tsconfig) {
      const commonLine =
        tsconfig && !ts.server.isInferredProjectName(tsconfig)
          ? createParsedCommandLine(ts, ts.sys, tsconfig)
          : createParsedCommandLineByJson(
              ts,
              ts.sys,
              ts.sys.getCurrentDirectory(),
              {}
            );
      const language = createLanguage(
        [
          {
            getLanguageId: (uri) => {
              var _a;
              return (_a = server.documents.get(uri)) === null || _a === void 0
                ? void 0
                : _a.languageId;
            },
          },
          createVueLanguagePlugin(
            ts,
            commonLine.options,
            commonLine.vueOptions,
            (uri) => uri.fsPath.replace(/\\/g, "/")
          ),
        ],
        createUriMap(),
        (uri) => {
          const document = server.documents.get(uri);
          if (document) {
            language.scripts.set(
              uri,
              document.getSnapshot(),
              document.languageId
            );
          } else {
            language.scripts.delete(uri);
          }
        }
      );
      return createLanguageService(
        language,
        server.languageServicePlugins,
        createLanguageServiceEnvironment(server, [
          ...server.workspaceFolders.all,
        ]),
        {}
      );
    }
  });
  connection.onInitialized(server.initialized);
  connection.onShutdown(server.shutdown);
}
