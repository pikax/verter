import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import {
  cleanupCarrierVirtualImportPath,
  containingFileAwareExists,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  normalizePath,
  toIdeCarrierFileName,
} from "./helpers/utils";
import {
  CarrierStoreReader,
  resolveCarrierStoreDir,
  resolveResponseRemap,
  type ManifestScriptKind,
} from "./helpers/carrierStore";
import { coldResolveCompanion } from "./helpers/coldRead";
import {
  isCarrierCompanionPath,
  isModuleLevelDefinition,
  remapAllFileTextChanges,
  remapCarrierSpan,
  remapDocumentSpans,
  remapModuleLevelCompanionToSource,
  remapReferencedSymbol,
  type CarrierRemapContext,
} from "./helpers/carrierRemap";
import {
  getAliasedNavigationResult,
  getAliasedQuickInfo,
  getModuleSpecifierNavigationResult,
  retargetAliasedDefinitionInfos,
} from "./helpers/barrelNavigation";
import { VERTER_TYPES_STUB } from "./helpers/verterTypesStub";

/**
 * The `@verter/typescript-plugin` is a THIN SYNCHRONOUS READER over the Rust
 * `verter_lsp`-published on-disk carrier-snapshot store. The Rust LSP is the
 * sole carrier authority: it compiles every framework carrier (`.vue`/
 * `.svelte`) and publishes the result to a content-addressed store + atomic
 * manifest. This plugin runs inside the user's tsserver (a separate process
 * with no shared memory) and serves carrier companions to tsserver by reading
 * that store synchronously — it never compiles a carrier itself.
 *
 * Every carrier-content host hook (`getScriptSnapshot`/`getScriptVersion`/
 * `getScriptKind`/`readFile`/`fileExists`/`resolveModuleNameLiterals`/
 * `getExternalFiles`) reads from the store via `CarrierStoreReader`. A path the
 * store knows nothing about falls through to real disk; a known-but-not-yet-ready
 * companion bounded-blocks (the C10 sticky-`TS2307` defense). When the store dir
 * is unavailable the plugin serves nothing for carriers and falls through for
 * everything else (fail closed).
 */

/** Map a manifest script-kind string to the TS `ScriptKind` enum. */
function manifestScriptKind(ts: typeof tsModule, kind: ManifestScriptKind): tsModule.ScriptKind {
  switch (kind) {
    case "TSX":
      return ts.ScriptKind.TSX;
    case "TS":
      return ts.ScriptKind.TS;
    case "JSX":
      return ts.ScriptKind.JSX;
    case "JS":
      return ts.ScriptKind.JS;
  }
}

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  // The resolved store dir per configured project, recorded by `create` (which
  // has the plugin config) so `getExternalFiles` (which does NOT receive the
  // config) can reuse the same store. Keyed by the project name (tsconfig path).
  const storeDirByProject = new Map<string, string | undefined>();

  const create = (info: tsModule.server.PluginCreateInfo) => {
    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();

    // The Rust LSP passes the RESOLVED per-workspace store dir in the plugin
    // config; `VERTER_CARRIER_STORE_DIR` is the environment fallback. When
    // neither is set the store is unavailable and the reader is a no-op.
    const storeDir = resolveCarrierStoreDir(info.config);
    // Whether to map carrier-companion provider RESPONSES (definition/references/
    // rename/code-action edits/completion-detail edits) back to `.vue`/`.svelte`
    // source. ENABLED by default for the VS Code DIRECT surface (the plugin is the
    // sole mapper); DISABLED by the verter_lsp-internal backend (the Rust merge
    // layer is the sole mapper there, so the plugin returns RAW companion responses
    // and there is no double-mapping). Content/membership/resolution hooks are
    // ALWAYS on regardless — only response-mapping is gated.
    const responseRemap = resolveResponseRemap(info.config);
    const projectKey = info.project.getProjectName();
    storeDirByProject.set(projectKey, storeDir);
    // PROJECT-SCOPED reader: `create(info)` is per configured project, so every
    // carrier the host hooks serve comes ONLY from this project's manifest entry
    // — a sibling tsconfig's carrier (compiled under different `paths`/`types`/
    // `lib`) is never served here.
    const store = new CarrierStoreReader(storeDir, projectKey);
    if (store.isAvailable()) {
      logger.info(`[Verter] carrier store: ${storeDir}`);
    } else {
      logger.info(
        "[Verter] no carrier store dir (carrierStoreDir / VERTER_CARRIER_STORE_DIR unset) — " +
          "carriers serve nothing, falling through to disk",
      );
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

    // ── carrier-store reads (the SOLE carrier-content authority) ───────────

    /**
     * The carrier blob the store serves for `fileName`, which may be EITHER a
     * companion provider path (`Comp.vue.tsx` / `Comp.vue.verter.ts`) OR a carrier
     * SOURCE path (`Comp.vue`). `getExternalFiles` advertises the SOURCE path to
     * tsserver (so the carrier is a configured-project member under
     * `extraFileExtensions`); tsserver then asks the host for the SOURCE path's
     * snapshot — answered here with the IDE companion's carrier content. A direct
     * companion query (in-project import resolution) is served directly.
     *
     * Resolves the IDE companion FIRST for a source path, then falls back to a
     * direct companion read; a known-but-not-yet-ready companion cold-reads
     * (bounded-block / last-good); else `undefined` (unknown / negative).
     */
    const carrierContent = (fileName: string): string | undefined => {
      // A carrier SOURCE path (`Comp.vue`) → serve its IDE companion's content.
      const sourceReady = store.readyFileForSource(fileName);
      if (sourceReady) {
        const companion = store.companionForSource(fileName) ?? fileName;
        return store.readBlobSync(sourceReady.blob_rel, companion);
      }
      const sourceCompanion = store.companionForSource(fileName);
      if (sourceCompanion && store.ownedSourceFor(sourceCompanion)) {
        const result = coldResolveCompanion(store, sourceCompanion);
        if (result.kind === "ready") {
          return store.readBlobSync(result.readyFile.blob_rel, sourceCompanion);
        }
        if (result.kind === "lastGood") {
          return result.content;
        }
      }

      // A direct companion provider path (`Comp.vue.tsx`) → serve it directly.
      const ready = store.readyFile(fileName);
      if (ready) {
        return store.readBlobSync(ready.blob_rel, fileName);
      }
      if (store.ownedSourceFor(fileName)) {
        const result = coldResolveCompanion(store, fileName);
        if (result.kind === "ready") {
          return store.readBlobSync(result.readyFile.blob_rel, fileName);
        }
        if (result.kind === "lastGood") {
          return result.content;
        }
      }
      return undefined;
    };

    /** Whether `fileName` is a carrier (source OR companion) the store can serve. */
    const carrierExists = (fileName: string): boolean => {
      if (store.readyFileForSource(fileName)) {
        return true;
      }
      const sourceCompanion = store.companionForSource(fileName);
      if (sourceCompanion && store.ownedSourceFor(sourceCompanion)) {
        const result = coldResolveCompanion(store, sourceCompanion);
        if (result.kind === "ready" || result.kind === "lastGood") {
          return true;
        }
      }
      if (store.readyFile(fileName)) {
        return true;
      }
      if (store.ownedSourceFor(fileName)) {
        const result = coldResolveCompanion(store, fileName);
        return result.kind === "ready" || result.kind === "lastGood";
      }
      return false;
    };

    // ── host-proxy matrix: read carrier companions from the store ──────────

    const _getScriptSnapshot = info.languageServiceHost.getScriptSnapshot?.bind(
      info.languageServiceHost,
    );
    info.languageServiceHost.getScriptSnapshot = (fileName) => {
      const content = carrierContent(fileName);
      if (content !== undefined) {
        return ts.ScriptSnapshot.fromString(content);
      }
      return _getScriptSnapshot?.(fileName);
    };

    const _getScriptVersion = info.languageServiceHost.getScriptVersion?.bind(
      info.languageServiceHost,
    );
    info.languageServiceHost.getScriptVersion = (fileName) => {
      // A carrier SOURCE path is versioned by its IDE companion's ready entry.
      const sourceReady = store.readyFileForSource(fileName);
      if (sourceReady) {
        return String(sourceReady.version);
      }
      const ready = store.readyFile(fileName);
      if (ready) {
        return String(ready.version);
      }
      return _getScriptVersion?.(fileName) ?? "0";
    };

    const _getScriptKind = info.languageServiceHost.getScriptKind?.bind(info.languageServiceHost);
    info.languageServiceHost.getScriptKind = (fileName) => {
      // A carrier SOURCE path (`Comp.vue`) is parsed AS its IDE companion's kind
      // (TSX/JSX): tsserver makes the source a member via `getExternalFiles`, and
      // it must read the generated TSX, not the raw `.vue` text.
      const sourceReady = store.readyFileForSource(fileName);
      if (sourceReady) {
        return manifestScriptKind(ts, sourceReady.script_kind);
      }
      const ready = store.readyFile(fileName);
      if (ready) {
        return manifestScriptKind(ts, ready.script_kind);
      }
      const owned = store.ownedSourceFor(fileName);
      if (owned && normalizePath(owned.provider_uri) === normalizePath(fileName)) {
        return manifestScriptKind(ts, owned.script_kind);
      }
      return _getScriptKind?.(fileName) ?? ts.ScriptKind.Unknown;
    };

    info.serverHost.readFile = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return VERTER_TYPES_STUB;
      }
      const content = carrierContent(fileName);
      if (content !== undefined) {
        return content;
      }
      return _readFile(fileName);
    };

    info.serverHost.fileExists = (fileName: string) => {
      if (!verterTypesInstalled && normalizePath(fileName) === verterTypesVirtualPath) {
        return true;
      }
      if (carrierExists(fileName)) {
        return true;
      }
      return _fileExists(fileName);
    };

    // Directory/realpath/canonical proxies — wired so a partial proxy never
    // constructs a wrong Program graph. `getExternalFiles` (below) is the
    // membership mechanism for tsserver, so these delegate to the real host;
    // the store companions are virtual paths under real directories, so the
    // real directory enumeration is correct.
    const _resolveTypeReferenceDirectiveReferences =
      info.languageServiceHost.resolveTypeReferenceDirectiveReferences?.bind(
        info.languageServiceHost,
      );
    if (_resolveTypeReferenceDirectiveReferences) {
      info.languageServiceHost.resolveTypeReferenceDirectiveReferences = (
        typeDirectiveReferences,
        containingFile,
        redirectedReference,
        options,
        containingSourceFile,
        ...rest
      ) =>
        // No type-reference directive targets a carrier companion — delegate.
        // Wired so the matrix is complete and the hook is never silently absent.
        _resolveTypeReferenceDirectiveReferences(
          typeDirectiveReferences,
          containingFile,
          redirectedReference,
          options,
          containingSourceFile,
          ...rest,
        );
    }

    // ── module resolution: in-project `.vue`/`.svelte` → IDE carrier ───────

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
        if (moduleName === "@verter/types" && !verterTypesInstalled) {
          return {
            extension: ts.Extension.Dts,
            isExternalLibraryImport: true,
            resolvedFileName: verterTypesVirtualPath,
          };
        }

        // An already-IDE-carrier-shaped relative specifier resolves to itself.
        if (isRelativeVueTs(moduleName)) {
          const resolved = path.resolve(path.dirname(containingFile), moduleName);
          return {
            extension: ts.Extension.Ts,
            isExternalLibraryImport: false,
            resolvedFileName: resolved,
          };
        }

        // A relative bare `./Comp.vue` / `./Comp.svelte` import redirects to the
        // COMPONENT IDE carrier (`Comp.vue.tsx` / `Comp.svelte.tsx`) — the source
        // carrier identity both engines resolve to, derived from the `ide`
        // column. NOT the `.verter.ts` API carrier (the cross-package target).
        if (isRelativeVue(moduleName)) {
          const resolved = path.resolve(path.dirname(containingFile), moduleName);
          const ideCarrier = toIdeCarrierFileName(resolved);
          if (ideCarrier) {
            return {
              extension: ts.Extension.Tsx,
              isExternalLibraryImport: false,
              resolvedFileName: ideCarrier,
            };
          }
          return;
        }

        // A non-relative carrier specifier: let TS resolve to the carrier source,
        // then redirect to its IDE carrier. TS's own resolver runs FIRST; we only
        // act on a `.vue`/`.svelte` specifier TS could not resolve by extension.
        if (!isVue(moduleName)) {
          return;
        }
        const resolvedModule = resolveModule();
        if (!resolvedModule) {
          return;
        }
        const baseUrl = info.project.getCompilerOptions().baseUrl;
        const failedLocations = resolvedModule.failedLookupLocations;
        const carrierSource = failedLocations.find(
          (candidate) =>
            (baseUrl ? candidate.includes(baseUrl) : true) &&
            isVue(candidate) &&
            _fileExists(candidate),
        );
        if (!carrierSource) {
          return;
        }
        const ideCarrier = toIdeCarrierFileName(normalizePath(path.resolve(carrierSource)));
        if (!ideCarrier) {
          return;
        }
        return {
          extension: ts.Extension.Tsx,
          isExternalLibraryImport: false,
          resolvedFileName: ideCarrier,
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

    // The IDE carrier is `.tsx`, so JSX must be enabled for the companion's
    // generated JSX to type-check.
    const _getCompilationSettings = info.languageServiceHost.getCompilationSettings.bind(
      info.languageServiceHost,
    );
    info.languageServiceHost.getCompilationSettings = () => {
      const settings = _getCompilationSettings();
      return { ...settings, jsx: ts.JsxEmit.Preserve };
    };

    const languageService = info.languageService;

    // ── navigation-span remapping (store-backed source maps) ───────────────

    function fixCarrierImportPath(fileName: string, containingFile?: string): string {
      // Strip a carrier companion suffix (`.vue.tsx` / `.vue.verter.ts` / …)
      // back to the bare carrier path in a navigation/display token.
      const existsRel =
        containingFile === undefined
          ? _fileExists
          : containingFileAwareExists(_fileExists, containingFile);
      return cleanupCarrierVirtualImportPath(fileName, existsRel);
    }

    // The shared companion→source RESPONSE-remap context: every provider
    // response that can carry a carrier companion path + span (definition,
    // references, rename, code-action / refactor edits, completion-detail
    // edits) is mapped back to the `.vue`/`.svelte` source through this ONE
    // context, so the response surface uses the exact same offset translator
    // (`remapCarrierSpan`) and companion-ownership classification the host hooks
    // serve from. `readCompanion` reads the generated companion blob the
    // response offsets index; `readSource` reads the original carrier source;
    // `fileExists` disambiguates an ambiguous Svelte rune specifier from a
    // companion in the inserted-import rewrite.
    const remapContext: CarrierRemapContext = {
      reader: store,
      readCompanion: (provider) => carrierContent(provider),
      readSource: (source) => _readFile(source),
      fileExists: _fileExists,
    };

    function remapDefinitionLike<
      T extends {
        fileName: string;
        textSpan: tsModule.TextSpan;
        contextSpan?: tsModule.TextSpan;
        originalTextSpan?: tsModule.TextSpan;
        kind?: string;
      },
    >(definition: T): T | undefined {
      if (isVue(definition.fileName)) {
        return definition;
      }

      const remapped = remapCarrierSpan(
        store,
        definition.fileName,
        definition.textSpan,
        (provider) => carrierContent(provider),
        (source) => _readFile(source),
      );

      if (!remapped) {
        // A MODULE-LEVEL companion target — a definition whose target is the
        // carrier AS A FILE / MODULE (go-to-def on a `./Comp.vue` import
        // specifier, or a default-export component reference), stamped
        // `kind: "module"` / `"script"`. Its module-start region legitimately
        // has no specific source mapping, so the CORRECT navigation target is
        // the `.vue`/`.svelte` SOURCE FILE itself: rewrite `fileName` → source
        // and the span → the source file start. Leaving the companion (a
        // `.vue.tsx` not on disk) would break go-to-def into the imported `.vue`.
        if (
          isCarrierCompanionPath(store, definition.fileName) &&
          isModuleLevelDefinition(definition)
        ) {
          // Fail-closed-safe: if the source path can't be resolved, DROP the
          // definition (never surface the companion path).
          return remapModuleLevelCompanionToSource(store, definition);
        }
        // FAIL CLOSED. A definition whose `fileName` IS a carrier companion
        // (`Comp.vue.tsx`) but whose SPECIFIC-token span could not be mapped
        // must NOT have its path rewritten to the bare `Comp.vue` while keeping
        // the GENERATED-TSX span — that pairs a source path with a generated
        // offset (navigation lands on the wrong token). Leave the companion path
        // AND span intact (self-consistent) so nothing is mis-mapped. Only a
        // NON-companion path (a plain `.ts`, a free-form display token) gets the
        // display-only carrier-suffix cleanup, where there was no carrier span
        // to map.
        if (!store.readyFile(definition.fileName) && !store.ownedSourceFor(definition.fileName)) {
          definition.fileName = fixCarrierImportPath(definition.fileName);
        }
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

    /**
     * Remap a definition array in place, DROPPING any entry `remapDefinitionLike`
     * fails closed on (a module-level companion whose source path could not be
     * resolved). A definition response is a plain array, so the dropped-entry
     * case rebuilds it; the common case (every entry remapped) preserves order.
     */
    function remapDefinitions<
      T extends {
        fileName: string;
        textSpan: tsModule.TextSpan;
        contextSpan?: tsModule.TextSpan;
        originalTextSpan?: tsModule.TextSpan;
        kind?: string;
      },
    >(definitions: readonly T[]): T[] {
      // RAW passthrough on the verter_lsp-internal backend: the Rust merge layer
      // is the sole companion→source mapper there, so return the companion
      // responses untouched (mapping here would double-map).
      if (!responseRemap) {
        return [...definitions];
      }
      const out: T[] = [];
      for (const def of definitions) {
        const mapped = remapDefinitionLike(def);
        if (mapped !== undefined) {
          out.push(mapped);
        }
      }
      return out;
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
      return { checker: program.getTypeChecker(), sourceFile };
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
      if (moduleName === "@verter/types" && !verterTypesInstalled) {
        return verterTypesVirtualPath;
      }
      if (isRelativeVueTs(moduleName)) {
        return path.resolve(path.dirname(containingFile), moduleName);
      }
      if (isRelativeVue(moduleName)) {
        const resolved = path.resolve(path.dirname(containingFile), moduleName);
        return toIdeCarrierFileName(resolved) ?? undefined;
      }
      const result = ts.resolveModuleName(
        moduleName,
        containingFile,
        info.project.getCompilerOptions(),
        {
          fileExists: info.serverHost.fileExists.bind(info.serverHost),
          readFile: info.serverHost.readFile.bind(info.serverHost),
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
          return { textSpan: aliased.textSpan, definitions: remapDefinitions(aliased.definitions) };
        }

        const moduleNavigation = getModuleSpecifierNavigationResult(
          ts,
          context.sourceFile,
          position,
          (moduleName) => resolveModuleFileName(fileName, moduleName),
        );
        if (moduleNavigation?.definitions.length) {
          return {
            textSpan: moduleNavigation.textSpan,
            definitions: remapDefinitions(moduleNavigation.definitions),
          };
        }
      }

      const result = _getDefinitionAndBoundSpan(fileName, position);
      if (result?.definitions) {
        const preferredName = getPreferredRetargetName(fileName, position, context);
        const definitions = retargetAliasedDefinitions(result.definitions, preferredName) ?? [
          ...result.definitions,
        ];
        result.definitions = remapDefinitions(definitions);
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
          return remapDefinitions(aliased.definitions);
        }

        const moduleNavigation = getModuleSpecifierNavigationResult(
          ts,
          context.sourceFile,
          position,
          (moduleName) => resolveModuleFileName(fileName, moduleName),
        );
        if (moduleNavigation?.definitions.length) {
          return remapDefinitions(moduleNavigation.definitions);
        }
      }

      const result = _getDefinitionAtPosition(fileName, position);
      if (result) {
        const preferredName = getPreferredRetargetName(fileName, position, context);
        const definitions = retargetAliasedDefinitions(result, preferredName) ?? [...result];
        return remapDefinitions(definitions);
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
          return remapDefinitions(aliased.definitions);
        }
      }

      const result = _getTypeDefinitionAtPosition(fileName, position);
      if (result) {
        const preferredName = getPreferredRetargetName(fileName, position, context);
        const definitions = retargetAliasedDefinitions(result, preferredName) ?? [...result];
        return remapDefinitions(definitions);
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
      // An engine-produced completion-detail code-action whose edit targets a
      // carrier companion FILE (a `.vue.tsx` path + a generated-region span) is
      // mapped back to the `.vue`/`.svelte` source (companion path → source
      // path, span → source span; an unmappable change is dropped — fail
      // closed); whose specifier targets a companion (`./Comp.vue.tsx`, …) is
      // rewritten to the bare `./Comp.vue`/`.svelte` specifier (the auto-import
      // `completion_resolve` case). Both run through the shared response
      // remapper. The action DESCRIPTION (free-form display text) keeps the
      // display-only carrier-suffix cleanup. An ambiguous Svelte suffix is
      // stripped only when its backing carrier exists.
      const existsRelToContaining = containingFileAwareExists(_fileExists, fileName);
      if (result?.codeActions) {
        for (const action of result.codeActions) {
          // The action DESCRIPTION is free-form DISPLAY text — its carrier-suffix
          // cleanup is cosmetic (not a path/span remap) and stays on BOTH surfaces.
          action.description = cleanupCarrierVirtualImportPath(
            action.description,
            existsRelToContaining,
          );
          // The actual completion-detail code-action EDITS (companion→source span
          // remap + inserted-import specifier rewrite) are a RESPONSE remap: gated
          // OFF on the verter_lsp-internal backend, where the Rust completion-resolve
          // / merge layer owns them. Raw companion edits pass through there.
          if (responseRemap) {
            action.changes = remapAllFileTextChanges(remapContext, action.changes);
          }
        }
      }
      if (result?.sourceDisplay) {
        result.sourceDisplay = result.sourceDisplay.map((part) => ({
          ...part,
          text: cleanupCarrierVirtualImportPath(part.text, existsRelToContaining),
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
        const existsRelToContaining = containingFileAwareExists(_fileExists, fileName);
        for (const entry of result.entries) {
          if (entry.sourceDisplay) {
            entry.sourceDisplay = entry.sourceDisplay.map((part) => ({
              ...part,
              text: cleanupCarrierVirtualImportPath(part.text, existsRelToContaining),
            }));
          }
          if (entry.source) {
            entry.source = fixCarrierImportPath(entry.source, fileName);
          }
        }
      }
      return result;
    };

    // ── references: companion entries → source (drop unmappable) ───────────
    //
    // Each new override guards on the underlying method being present (the same
    // `?.bind` posture the host hooks use): a partial `LanguageService` is never
    // crashed by a missing method, and the override is wired only when there is
    // something to wrap.

    const _getReferencesAtPosition = languageService.getReferencesAtPosition?.bind(languageService);
    if (_getReferencesAtPosition) {
      languageService.getReferencesAtPosition = (fileName, position) => {
        const result = _getReferencesAtPosition(fileName, position);
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `ReferenceEntry` whose `fileName` is a carrier companion is
        // mapped back to source (path + span); an unmappable companion
        // reference (a generated-only region) is DROPPED. References in the
        // user's real `.ts` pass through unchanged.
        return remapDocumentSpans(remapContext, result);
      };
    }

    const _findReferences = languageService.findReferences?.bind(languageService);
    if (_findReferences) {
      languageService.findReferences = (fileName, position) => {
        const result = _findReferences(fileName, position);
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `ReferencedSymbol` is remapped: its definition + every reference
        // entry. A symbol whose definition is an unmappable companion is
        // dropped entirely (fail closed).
        const out: tsModule.ReferencedSymbol[] = [];
        for (const symbol of result) {
          const mapped = remapReferencedSymbol(remapContext, symbol);
          if (mapped !== undefined) {
            out.push(mapped);
          }
        }
        return out;
      };
    }

    const _getImplementationAtPosition =
      languageService.getImplementationAtPosition?.bind(languageService);
    if (_getImplementationAtPosition) {
      languageService.getImplementationAtPosition = (fileName, position) => {
        const result = _getImplementationAtPosition(fileName, position);
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `ImplementationLocation` (a `DocumentSpan`) whose `fileName` is a
        // carrier companion maps back to source; an unmappable companion
        // location is dropped (fail closed). `ImplementationLocation` also
        // carries `displayParts`/`kind`, preserved by the in-place remap.
        return remapDocumentSpans(remapContext, result);
      };
    }

    // ── rename: companion locations → source (drop unmappable) ─────────────

    const _findRenameLocations = languageService.findRenameLocations?.bind(languageService) as
      | tsModule.LanguageService["findRenameLocations"]
      | undefined;
    if (_findRenameLocations) {
      languageService.findRenameLocations = (
        fileName: string,
        position: number,
        findInStrings: boolean,
        findInComments: boolean,
        preferences?: tsModule.UserPreferences | boolean,
      ) => {
        const result = _findRenameLocations(
          fileName,
          position,
          findInStrings,
          findInComments,
          preferences as tsModule.UserPreferences,
        );
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `RenameLocation` whose `fileName` is a carrier companion is
        // mapped back to source (path + span); an unmappable companion location
        // (an edit that would land in a generated-only region) is DROPPED so a
        // rename never produces a mis-located edit. Real `.ts` locations pass
        // through.
        return remapDocumentSpans(remapContext, result);
      };
    }

    // ── code fixes / refactors / file-rename: companion edits → source ─────

    const _getCodeFixesAtPosition = languageService.getCodeFixesAtPosition?.bind(languageService);
    if (_getCodeFixesAtPosition) {
      languageService.getCodeFixesAtPosition = (
        fileName,
        start,
        end,
        errorCodes,
        formatOptions,
        preferences,
      ) => {
        const result = _getCodeFixesAtPosition(
          fileName,
          start,
          end,
          errorCodes,
          formatOptions,
          preferences,
        );
        if (!responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend: the Rust
          // `merge_code_actions` maps companion edits → source AND owns the
          // inserted-import specifier rewrite there.
          return result;
        }
        // Each fix's `changes` (FileTextChanges[]) targeting a carrier companion
        // is mapped back to source (path + each change span); an unmappable
        // change is dropped, and a fix whose companion edit is wholly unmappable
        // drops that file edit. A companion import-specifier in `newText` is
        // rewritten to the bare carrier. Real-file edits pass through (specifier
        // rewrite only).
        return result.map((fix) => ({
          ...fix,
          changes: remapAllFileTextChanges(remapContext, fix.changes),
        }));
      };
    }

    const _getCombinedCodeFix = languageService.getCombinedCodeFix?.bind(languageService);
    if (_getCombinedCodeFix) {
      languageService.getCombinedCodeFix = (scope, fixId, formatOptions, preferences) => {
        const result = _getCombinedCodeFix(scope, fixId, formatOptions, preferences);
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        return { ...result, changes: remapAllFileTextChanges(remapContext, result.changes) };
      };
    }

    const _getEditsForRefactor = languageService.getEditsForRefactor?.bind(languageService);
    if (_getEditsForRefactor) {
      languageService.getEditsForRefactor = (
        fileName,
        formatOptions,
        positionOrRange,
        refactorName,
        actionName,
        preferences,
        interactiveRefactorArguments,
      ) => {
        const result = _getEditsForRefactor(
          fileName,
          formatOptions,
          positionOrRange,
          refactorName,
          actionName,
          preferences,
          interactiveRefactorArguments,
        );
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        return { ...result, edits: remapAllFileTextChanges(remapContext, result.edits) };
      };
    }

    const _getEditsForFileRename = languageService.getEditsForFileRename?.bind(languageService);
    if (_getEditsForFileRename) {
      languageService.getEditsForFileRename = (
        oldFilePath,
        newFilePath,
        formatOptions,
        preferences,
      ) => {
        const result = _getEditsForFileRename(oldFilePath, newFilePath, formatOptions, preferences);
        if (!responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        return remapAllFileTextChanges(remapContext, result);
      };
    }

    return languageService;
  };

  // `getExternalFiles` is the tsserver membership mechanism: it returns the
  // COMPANION provider paths of READY carriers (the `.vue.tsx` / `.svelte.tsx`
  // IDE companions whose blob exists). The companion path is the identity that
  // becomes a CONFIGURED-project member here — it is exactly what the host hooks
  // (`getScriptSnapshot`/`getScriptKind`/`getScriptVersion`) serve and what the
  // LSP queries diagnostics/navigation for, so the advertised identity matches
  // the served identity (the `extraFileExtensions` `configure` lets the source
  // extension participate; the published companion carries the generated TSX). A
  // companion is NEVER advertised before its blob exists (the primary C10
  // defense).
  //
  // PROJECT-SCOPED. `getExternalFiles(project)` is called once per configured
  // project; it must advertise ONLY the carriers the manifest attributes to THIS
  // `project`, keyed by `project.getProjectName()` (the tsconfig path = the
  // manifest `projects` key). Iterating every manifest project would LEAK a
  // sibling tsconfig's carriers into this project's Program — they would be
  // type-checked under the WRONG `paths`/`types`/`lib`. The store dir is the one
  // `create` resolved for this project (the same plugin config), with the
  // environment fallback for a project `create` has not yet seen.
  const getExternalFiles = (project: tsModule.server.ConfiguredProject) => {
    const projectKey = project.getProjectName();
    const recorded = storeDirByProject.get(projectKey);
    const storeDir = recorded ?? resolveCarrierStoreDir(undefined);
    // The reader is scoped to THIS project's identity, so `readyIdeCompanions`
    // returns only this project's ready IDE companions (intersected with its
    // owned set) — never a sibling tsconfig's.
    const store = new CarrierStoreReader(storeDir, projectKey);
    if (!store.isAvailable()) {
      return [];
    }
    const out = store.readyIdeCompanions();
    project.projectService.logger.info(
      `[Verter] getExternalFiles(${projectKey}): ${out.length} ready carrier companion(s)`,
    );
    return out;
  };

  return {
    create,
    getExternalFiles,
  };
};

export = init;
