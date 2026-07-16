import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import {
  CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY,
  cleanupCarrierVirtualImportPath,
  containingFileAwareExists,
  editorOwnsCarrierMembership,
  isCarrierCompanionPath,
  isModuleLevelDefinition,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  mapCarrierSourceOffsetToGenerated,
  normalizePath,
  remapAllFileTextChanges,
  remapCarrierSpan,
  remapDocumentSpans,
  remapModuleLevelCompanionToSource,
  remapReferencedSymbol,
  toIdeCarrierFileName,
  type CarrierRemapContext,
  type ManifestScriptKind,
} from "@verter/language-shared";
import {
  DiskCarrierStoreReader,
  resolveCarrierStoreDir,
  resolveResponseRemap,
} from "./helpers/carrierStore";
import { coldResolveCompanion } from "./helpers/coldRead";
import {
  getAliasedNavigationResult,
  getAliasedQuickInfo,
  getModuleSpecifierNavigationResult,
  retargetAliasedDefinitionInfos,
} from "./helpers/barrelNavigation";
import { VERTER_TYPES_STUB } from "./helpers/verterTypesStub";
import { writeEditorTsserverAttestation } from "./helpers/editorAttestation";

// tsserver may invoke the plugin module factory separately for different projects.
// Attestation is process-scoped, so every factory must publish the same union rather
// than allowing one project's receipt to overwrite another's evidence.
const processBoundProjects = new Set<string>();
const processStoreDirByProject = new Map<string, string | undefined>();
const processEditorOwnsCarrierMembershipByProject = new Map<string, boolean>();
const processUpdateProjectConfig = new Map<string, (config: Record<string, unknown>) => void>();
interface ProcessEditorProjectRuntime {
  readonly projectKey: string;
  readonly projectService: tsModule.server.ProjectService;
  readonly languageService: tsModule.LanguageService;
  readonly getStore: () => DiskCarrierStoreReader;
  readonly readCompanion: (fileName: string) => string | undefined;
  readonly readSource: (fileName: string) => string | undefined;
  readonly editorOwnsMembership: () => boolean;
}
const processEditorProjectRuntimes = new Map<string, ProcessEditorProjectRuntime>();
let processCurrentConfig: Record<string, unknown> | undefined;

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
 * `getExternalFiles`) reads from the store via `DiskCarrierStoreReader`. A path the
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
  const create = (info: tsModule.server.PluginCreateInfo) => {
    const logger = info.project.projectService.logger;
    const directory = info.project.getCurrentDirectory();

    // The Rust LSP passes the RESOLVED per-workspace store dir in the plugin
    // config; `VERTER_CARRIER_STORE_DIR` is the environment fallback. When
    // neither is set the store is unavailable and the reader is a no-op.
    const effectiveConfig = { ...(processCurrentConfig ?? {}), ...(info.config ?? {}) };
    let storeDir = resolveCarrierStoreDir(effectiveConfig);
    // Whether to map carrier-companion provider RESPONSES (definition/references/
    // rename/code-action edits/completion-detail edits) back to `.vue`/`.svelte`
    // source. ENABLED by default for the VS Code DIRECT surface (the plugin is the
    // sole mapper); DISABLED by the verter_lsp-internal backend (the Rust merge
    // layer is the sole mapper there, so the plugin returns RAW companion responses
    // and there is no double-mapping). Carrier serving remains enabled on both
    // surfaces; the editor-owned surface preserves the raw source ScriptInfo and
    // admits a distinct generated companion, while non-editor hosts continue to
    // serve generated content under the source identity.
    let responseRemap = resolveResponseRemap(effectiveConfig);
    let editorOwnsMembership = editorOwnsCarrierMembership(effectiveConfig);
    let carrierStoreRefreshToken = effectiveConfig[CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY];
    const projectKey = info.project.getProjectName();
    processBoundProjects.add(projectKey);
    processCurrentConfig = effectiveConfig;
    writeEditorTsserverAttestation(processCurrentConfig, processBoundProjects);
    processStoreDirByProject.set(projectKey, storeDir);
    processEditorOwnsCarrierMembershipByProject.set(projectKey, editorOwnsMembership);
    // PROJECT-SCOPED reader: `create(info)` is per configured project, so every
    // carrier the host hooks serve comes ONLY from this project's manifest entry
    // — a sibling tsconfig's carrier (compiled under different `paths`/`types`/
    // `lib`) is never served here.
    let store = new DiskCarrierStoreReader(storeDir, projectKey);
    // Assigned after the host hooks have captured their original implementations.
    // The configuration updater closes over this mutable context so every response
    // provider sees the same project-scoped reader as the live host hooks.
    let remapContext: CarrierRemapContext;
    let refreshScheduled = false;
    processUpdateProjectConfig.set(projectKey, (config) => {
      const nextStoreDir = resolveCarrierStoreDir(config);
      const nextResponseRemap = resolveResponseRemap(config);
      const nextEditorOwnsMembership = editorOwnsCarrierMembership(config);
      const nextCarrierStoreRefreshToken = config[CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY];
      const storeChanged = nextStoreDir !== storeDir;
      const publicationAdvanced = !Object.is(
        nextCarrierStoreRefreshToken,
        carrierStoreRefreshToken,
      );
      const servingStateChanged =
        storeChanged ||
        nextResponseRemap !== responseRemap ||
        nextEditorOwnsMembership !== editorOwnsMembership ||
        publicationAdvanced;
      storeDir = nextStoreDir;
      responseRemap = nextResponseRemap;
      editorOwnsMembership = nextEditorOwnsMembership;
      carrierStoreRefreshToken = nextCarrierStoreRefreshToken;
      processStoreDirByProject.set(projectKey, storeDir);
      processEditorOwnsCarrierMembershipByProject.set(projectKey, editorOwnsMembership);
      if (storeChanged) {
        store = new DiskCarrierStoreReader(storeDir, projectKey);
        if (remapContext !== undefined) {
          remapContext.reader = store;
        }
        logger.info(
          store.isAvailable()
            ? `[Verter] carrier store reconfigured: ${storeDir}`
            : "[Verter] carrier store removed; carrier serving disabled",
        );
      }
      if (!servingStateChanged) return;
      // The configure request can arrive while tsserver is updating this project's
      // graph. Mutating that graph re-entrantly corrupts ConfiguredProject state.
      // Coalesce refreshes onto the next event-loop turn: the store swap is already
      // visible synchronously, while graph invalidation runs after the current
      // tsserver command has unwound.
      if (!refreshScheduled) {
        refreshScheduled = true;
        setImmediate(() => {
          refreshScheduled = false;
          // TypeScript incorporates plugin `getExternalFiles` into configured
          // project roots only through its targeted file-name reload path. A
          // normal dirty graph pass reads the new list after building the old
          // roots, which is too late. Both TS 5.9 and TS 6 expose this stable
          // runtime method (although it is omitted from the public declaration).
          // Use it only for the concrete configured project; never clear every
          // editor project or mutate the graph inside `configurePlugin`.
          const configuredProject = info.project as typeof info.project & {
            getConfigFilePath?: () => tsModule.server.NormalizedPath;
          };
          const projectService = info.project
            .projectService as typeof info.project.projectService & {
            reloadFileNamesOfConfiguredProject?: (project: typeof configuredProject) => boolean;
          };
          if (
            typeof configuredProject.getConfigFilePath === "function" &&
            typeof projectService.reloadFileNamesOfConfiguredProject === "function"
          ) {
            projectService.reloadFileNamesOfConfiguredProject(configuredProject);
            info.project.refreshDiagnostics();
            return;
          }
          // `markAsDirty` is a stable tsserver runtime method but intentionally
          // omitted from the public plugin declaration. Keep the cast narrow;
          // `refreshDiagnostics` is the typed public notification surface.
          (
            info.project as typeof info.project & {
              markAsDirty?: () => void;
            }
          ).markAsDirty?.();
          info.project.refreshDiagnostics();
        });
      }
    });
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
      // Non-editor hosts use the carrier source as the generated document
      // identity. VS Code already owns that source identity (including its open
      // raw text), so its plugin surface leaves the source snapshot untouched
      // and serves only the distinct companion identity below.
      if (!editorOwnsMembership) {
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
      if (!editorOwnsMembership) {
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

    // A configured editor project can answer for its visible framework source
    // through the companion request router even though that raw mixed-content
    // file is deliberately absent from the TypeScript Program. Expose that
    // virtual membership to tsserver's diagnostic scheduler; otherwise `geterr`
    // drops the file before the wrapped diagnostic methods are invoked.
    const _projectContainsFile = info.project.containsFile?.bind(info.project);
    if (_projectContainsFile) {
      info.project.containsFile = (fileName, requireOpen) => {
        if (editorOwnsMembership) {
          const companion = store.companionForSource(fileName);
          if (companion !== undefined && store.readyFile(companion) !== undefined) {
            return true;
          }
          // Open framework sources live in an inferred editor project while
          // their companions live in the exact configured project. The inferred
          // project can still answer through that process-level route, so keep
          // `geterr` from discarding the source before the diagnostic wrapper.
          if (editorOwnerForSource(fileName) !== undefined) {
            return true;
          }
        }
        return _projectContainsFile(fileName, requireOpen);
      };
    }

    // ── host-proxy matrix: read carrier companions from the store ──────────

    const _getScriptSnapshot = info.languageServiceHost.getScriptSnapshot?.bind(
      info.languageServiceHost,
    );
    const readOriginalSource = (fileName: string): string | undefined => {
      const snapshot = _getScriptSnapshot?.(fileName);
      return snapshot === undefined
        ? _readFile(fileName)
        : snapshot.getText(0, snapshot.getLength());
    };
    info.languageServiceHost.getScriptSnapshot = (fileName) => {
      // Always enter the original Project host first. In real tsserver this is
      // not a redundant read: it creates and attaches the ScriptInfo required
      // by the document registry for every virtual root/import. Returning a
      // store snapshot before this call leaves `setDocument` with no ScriptInfo.
      const hostSnapshot = _getScriptSnapshot?.(fileName);
      const content = carrierContent(fileName);
      if (content !== undefined) {
        return ts.ScriptSnapshot.fromString(content);
      }
      return hostSnapshot;
    };

    const _getScriptVersion = info.languageServiceHost.getScriptVersion?.bind(
      info.languageServiceHost,
    );
    info.languageServiceHost.getScriptVersion = (fileName) => {
      // Project.getScriptVersion also participates in ScriptInfo creation.
      // Preserve that lifecycle even though the manifest version remains the
      // carrier's authoritative invalidation token.
      const hostVersion = _getScriptVersion?.(fileName);
      // A carrier SOURCE path is versioned by its IDE companion's ready entry.
      const sourceReady = editorOwnsMembership ? undefined : store.readyFileForSource(fileName);
      if (sourceReady) {
        return `${sourceReady.version}:${sourceReady.content_hash}`;
      }
      const ready = store.readyFile(fileName);
      if (ready) {
        // `version` is monotonic only within one LSP lifetime. Include the
        // content identity so reopening a persistent store cannot make a fresh
        // carrier collide with a same-number snapshot from a prior process.
        return `${ready.version}:${ready.content_hash}`;
      }
      return hostVersion ?? "0";
    };

    const _getScriptKind = info.languageServiceHost.getScriptKind?.bind(info.languageServiceHost);
    info.languageServiceHost.getScriptKind = (fileName) => {
      // A carrier SOURCE path (`Comp.vue`) is parsed AS its IDE companion's kind
      // (TSX/JSX): tsserver makes the source a member via `getExternalFiles`, and
      // it must read the generated TSX, not the raw `.vue` text.
      const sourceReady = editorOwnsMembership ? undefined : store.readyFileForSource(fileName);
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

    // ── module resolution: in-project `.vue`/`.svelte` → public carrier ────

    const ideCarrierForSource = (sourcePath: string): string | undefined =>
      store.companionForSource(sourcePath) ?? toIdeCarrierFileName(sourcePath) ?? undefined;
    const importedCarrierForSource = (sourcePath: string): string | undefined =>
      store.apiCompanionForSource(sourcePath) ?? ideCarrierForSource(sourcePath);

    const createModuleResolver =
      (containingFile: string) =>
      (
        moduleName: string,
        resolveModule: () =>
          | (tsModule.ResolvedModuleWithFailedLookupLocations & {
              failedLookupLocations: readonly string[];
            })
          | undefined,
      ): tsModule.ResolvedModuleFull | null | undefined => {
        const providerExtension = (provider: string): tsModule.Extension => {
          const normalized = normalizePath(provider).toLowerCase();
          if (normalized.endsWith(".jsx")) return ts.Extension.Jsx;
          if (normalized.endsWith(".tsx")) return ts.Extension.Tsx;
          if (normalized.endsWith(".js")) return ts.Extension.Js;
          return ts.Extension.Ts;
        };
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

        // A relative bare `./Comp.vue` / `./Comp.svelte` import prefers the ready
        // public API carrier. That is the consumer-facing TypeScript surface for
        // direct and barrel imports; the JSX/TSX carrier remains editor-only. If
        // public publication has not completed, retain the existing IDE fallback
        // so an already-open project never resolves to an unreadable virtual file.
        if (isRelativeVue(moduleName)) {
          const resolved = path.resolve(path.dirname(containingFile), moduleName);
          const importedCarrier = importedCarrierForSource(resolved);
          if (importedCarrier) {
            return {
              extension: providerExtension(importedCarrier),
              isExternalLibraryImport: false,
              resolvedFileName: importedCarrier,
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
        const importedCarrier = importedCarrierForSource(
          normalizePath(path.resolve(carrierSource)),
        );
        if (!importedCarrier) {
          return;
        }
        return {
          extension: providerExtension(importedCarrier),
          isExternalLibraryImport: false,
          resolvedFileName: importedCarrier,
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
      // Keep TypeScript's exact compiler-options object. TypeScript 6 attaches
      // configured-project metadata to this identity, and replacing it (even with
      // every current property descriptor copied) can detach later internal state.
      if (settings.jsx !== ts.JsxEmit.Preserve) {
        settings.jsx = ts.JsxEmit.Preserve;
      }
      return settings;
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
    remapContext = {
      reader: store,
      readCompanion: (provider) => carrierContent(provider),
      readSource: readOriginalSource,
      fileExists: _fileExists,
    };

    const editorRuntime: ProcessEditorProjectRuntime = {
      projectKey,
      projectService: info.project.projectService,
      languageService,
      getStore: () => store,
      readCompanion: carrierContent,
      readSource: readOriginalSource,
      editorOwnsMembership: () => editorOwnsMembership,
    };
    processEditorProjectRuntimes.set(projectKey, editorRuntime);

    function editorOwnerForSource(fileName: string): ProcessEditorProjectRuntime | undefined {
      const candidates = [...processEditorProjectRuntimes.values()].filter((runtime) => {
        if (!runtime.editorOwnsMembership()) return false;
        const runtimeStore = runtime.getStore();
        const companion = runtimeStore.companionForSource(fileName);
        return companion !== undefined && runtimeStore.readyFile(companion) !== undefined;
      });
      if (candidates.length === 0) return undefined;

      // A source can participate in more than one tsconfig. Ask the owning
      // ProjectService for the exact config it already resolved for the open
      // ScriptInfo; this is the same decision tsserver logs and uses when it
      // assigns an editor document. The method is stable at runtime in TS 5.9
      // and TS 6 but omitted from the public declaration, so retain a strict
      // one-candidate fallback and otherwise fail closed.
      const projectService = info.project.projectService as typeof info.project.projectService & {
        getConfigFileNameForFile?: (
          scriptInfo: tsModule.server.ScriptInfo,
          findFromCacheOnly: boolean,
        ) => tsModule.server.NormalizedPath | undefined;
      };
      const getScriptInfo = (
        projectService as unknown as {
          getScriptInfo?: (fileName: string) => tsModule.server.ScriptInfo | undefined;
        }
      ).getScriptInfo;
      const scriptInfo = getScriptInfo?.call(projectService, fileName);
      const configFile =
        scriptInfo === undefined
          ? undefined
          : projectService.getConfigFileNameForFile?.(scriptInfo, false);
      if (configFile !== undefined) {
        const normalizedConfig = normalizePath(configFile).toLowerCase();
        const exact = candidates.find(
          (runtime) => normalizePath(runtime.projectKey).toLowerCase() === normalizedConfig,
        );
        if (exact !== undefined) return exact;
      }
      return candidates.length === 1 ? candidates[0] : undefined;
    }

    interface EditorCarrierPosition {
      runtime: ProcessEditorProjectRuntime;
      companion: string;
      position: number;
    }

    /** Resolve one visible source position into its exact configured companion. */
    function editorCarrierPosition(
      fileName: string,
      position: number,
    ): EditorCarrierPosition | null {
      if (!editorOwnsMembership || !isVue(fileName)) return null;
      const runtime = editorOwnerForSource(fileName);
      if (runtime === undefined) return null;
      const runtimeStore = runtime.getStore();
      const companion = runtimeStore.companionForSource(fileName);
      if (companion === undefined || runtimeStore.readyFile(companion) === undefined) return null;
      const generatedPosition = mapCarrierSourceOffsetToGenerated(
        runtimeStore,
        companion,
        fileName,
        position,
        runtime.readCompanion,
        runtime.readSource,
      );
      return generatedPosition === null
        ? null
        : { runtime, companion, position: generatedPosition };
    }

    function editorCarrierSelection(
      fileName: string,
      selection: number | tsModule.TextRange,
    ): {
      runtime: ProcessEditorProjectRuntime;
      companion: string;
      selection: number | tsModule.TextRange;
    } | null {
      if (typeof selection === "number") {
        const mapped = editorCarrierPosition(fileName, selection);
        return mapped === null
          ? null
          : {
              runtime: mapped.runtime,
              companion: mapped.companion,
              selection: mapped.position,
            };
      }
      const start = editorCarrierPosition(fileName, selection.pos);
      const end = editorCarrierPosition(fileName, selection.end);
      if (
        start === null ||
        end === null ||
        start.runtime !== end.runtime ||
        normalizePath(start.companion) !== normalizePath(end.companion) ||
        end.position < start.position
      ) {
        return null;
      }
      return {
        runtime: start.runtime,
        companion: start.companion,
        selection: { pos: start.position, end: end.position },
      };
    }

    /**
     * VS Code opens the real framework source and owns that ScriptInfo, while
     * this plugin owns a distinct generated companion root. Route diagnostics
     * to the companion, then translate every located result back through the
     * published source map. A carrier that is not ready, or a generated-only
     * span that cannot be mapped faithfully, produces no TypeScript diagnostic
     * rather than raw SFC parse noise or a companion-path leak.
     */
    function editorSourceDiagnostics<T extends tsModule.Diagnostic>(
      fileName: string,
      method: "getSyntacticDiagnostics" | "getSemanticDiagnostics" | "getSuggestionDiagnostics",
      query: (target: string) => readonly T[],
    ): T[] {
      if (!editorOwnsMembership || !isVue(fileName)) {
        return [...query(fileName)];
      }
      const targetRuntime = editorOwnerForSource(fileName);
      if (targetRuntime === undefined) {
        return [];
      }
      const targetStore = targetRuntime.getStore();
      const companion = targetStore.companionForSource(fileName);
      if (companion === undefined || targetStore.readyFile(companion) === undefined) return [];
      const program = languageService.getProgram?.();
      if (program === undefined) {
        return [];
      }
      const diagnostics =
        targetRuntime === editorRuntime
          ? query(companion)
          : (
              targetRuntime.languageService[method] as unknown as (target: string) => readonly T[]
            ).call(targetRuntime.languageService, companion);
      const out: T[] = [];
      for (const diagnostic of diagnostics) {
        const mapped = mapEditorDiagnostic(diagnostic, program, targetRuntime);
        if (mapped !== undefined) out.push(mapped);
      }
      return out;
    }

    function mapEditorDiagnostic<T extends tsModule.Diagnostic>(
      diagnostic: T,
      program: tsModule.Program,
      targetRuntime: ProcessEditorProjectRuntime,
    ): T | undefined {
      const targetStore = targetRuntime.getStore();
      const diagnosticFile = diagnostic.file;
      if (
        diagnosticFile === undefined ||
        !isCarrierCompanionPath(targetStore, diagnosticFile.fileName)
      ) {
        return diagnostic;
      }
      if (diagnostic.start === undefined || diagnostic.length === undefined) {
        return undefined;
      }
      const mapped = remapCarrierSpan(
        targetStore,
        diagnosticFile.fileName,
        { start: diagnostic.start, length: diagnostic.length },
        targetRuntime.readCompanion,
        targetRuntime.readSource,
      );
      if (mapped === null) {
        return undefined;
      }
      const sourceText = targetRuntime.readSource(mapped.fileName);
      const sourceFile =
        program.getSourceFile(mapped.fileName) ??
        (sourceText === undefined
          ? undefined
          : ts.createSourceFile(
              mapped.fileName,
              sourceText,
              ts.ScriptTarget.Latest,
              true,
              ts.ScriptKind.TSX,
            ));
      if (sourceFile === undefined) {
        return undefined;
      }
      const relatedInformation = diagnostic.relatedInformation
        ?.map((related) => mapEditorDiagnostic(related, program, targetRuntime))
        .filter(
          (related): related is tsModule.DiagnosticRelatedInformation => related !== undefined,
        );
      return {
        ...diagnostic,
        file: sourceFile,
        start: mapped.textSpan.start,
        length: mapped.textSpan.length,
        ...(relatedInformation === undefined ? {} : { relatedInformation }),
      } as T;
    }

    const _getSyntacticDiagnostics = languageService.getSyntacticDiagnostics.bind(languageService);
    languageService.getSyntacticDiagnostics = (fileName) =>
      editorSourceDiagnostics(fileName, "getSyntacticDiagnostics", _getSyntacticDiagnostics);

    const _getSemanticDiagnostics = languageService.getSemanticDiagnostics.bind(languageService);
    languageService.getSemanticDiagnostics = (fileName) =>
      editorSourceDiagnostics(fileName, "getSemanticDiagnostics", _getSemanticDiagnostics);

    const _getSuggestionDiagnostics =
      languageService.getSuggestionDiagnostics.bind(languageService);
    languageService.getSuggestionDiagnostics = (fileName) =>
      editorSourceDiagnostics(
        fileName,
        "getSuggestionDiagnostics",
        _getSuggestionDiagnostics,
      ) as tsModule.DiagnosticWithLocation[];

    const _getEncodedSemanticClassifications =
      languageService.getEncodedSemanticClassifications?.bind(languageService);
    if (_getEncodedSemanticClassifications) {
      languageService.getEncodedSemanticClassifications = (fileName, span, format) => {
        if (!editorOwnsMembership || !isVue(fileName)) {
          return _getEncodedSemanticClassifications(fileName, span, format);
        }
        const routed = editorCarrierSelection(fileName, {
          pos: span.start,
          end: span.start + span.length,
        });
        if (routed === null || typeof routed.selection === "number") {
          return { spans: [], endOfLineState: 0 as tsModule.EndOfLineState };
        }
        const result = routed.runtime.languageService.getEncodedSemanticClassifications(
          routed.companion,
          {
            start: routed.selection.pos,
            length: routed.selection.end - routed.selection.pos,
          },
          format,
        );
        const mappedSpans: number[] = [];
        for (let index = 0; index + 2 < result.spans.length; index += 3) {
          const mapped = remapCarrierSpan(
            routed.runtime.getStore(),
            routed.companion,
            { start: result.spans[index], length: result.spans[index + 1] },
            routed.runtime.readCompanion,
            routed.runtime.readSource,
          );
          if (mapped !== null && normalizePath(mapped.fileName) === normalizePath(fileName)) {
            mappedSpans.push(
              mapped.textSpan.start,
              mapped.textSpan.length,
              result.spans[index + 2],
            );
          }
        }
        return { spans: mappedSpans, endOfLineState: result.endOfLineState };
      };
    }

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
        return importedCarrierForSource(resolved);
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
      if (editorOwnsMembership && isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) return undefined;
        const quickInfo = routed.runtime.languageService.getQuickInfoAtPosition(
          routed.companion,
          routed.position,
        );
        if (quickInfo === undefined) return undefined;
        const mapped = remapCarrierSpan(
          routed.runtime.getStore(),
          routed.companion,
          quickInfo.textSpan,
          routed.runtime.readCompanion,
          routed.runtime.readSource,
        );
        if (mapped === null || normalizePath(mapped.fileName) !== normalizePath(fileName)) {
          return undefined;
        }
        return { ...quickInfo, textSpan: mapped.textSpan };
      }
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
      let result: tsModule.CompletionInfo | undefined;
      if (editorOwnsMembership && isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) return undefined;
        const companionResult = routed.runtime.languageService.getCompletionsAtPosition(
          routed.companion,
          routed.position,
          options,
          formattingSettings,
        );
        if (companionResult === undefined) return undefined;
        const mapSpan = (span: tsModule.TextSpan): tsModule.TextSpan | null => {
          const mapped = remapCarrierSpan(
            routed.runtime.getStore(),
            routed.companion,
            span,
            routed.runtime.readCompanion,
            routed.runtime.readSource,
          );
          return mapped !== null && normalizePath(mapped.fileName) === normalizePath(fileName)
            ? mapped.textSpan
            : null;
        };
        const optionalReplacementSpan = companionResult.optionalReplacementSpan
          ? mapSpan(companionResult.optionalReplacementSpan)
          : undefined;
        if (companionResult.optionalReplacementSpan && optionalReplacementSpan === null) {
          return undefined;
        }
        const mappedOptionalReplacementSpan = optionalReplacementSpan ?? undefined;
        const entries: tsModule.CompletionEntry[] = [];
        for (const entry of companionResult.entries) {
          if (entry.replacementSpan === undefined) {
            entries.push(entry);
            continue;
          }
          const replacementSpan = mapSpan(entry.replacementSpan);
          if (replacementSpan !== null) entries.push({ ...entry, replacementSpan });
        }
        result = {
          ...companionResult,
          entries,
          ...(mappedOptionalReplacementSpan === undefined
            ? {}
            : { optionalReplacementSpan: mappedOptionalReplacementSpan }),
        };
      } else {
        result = _getCompletionsAtPosition(fileName, position, options, formattingSettings);
      }
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

    const _getApplicableRefactors = languageService.getApplicableRefactors?.bind(languageService);
    if (_getApplicableRefactors) {
      languageService.getApplicableRefactors = (
        fileName,
        positionOrRange,
        preferences,
        triggerReason,
        kind,
        includeInteractiveActions,
      ) => {
        if (editorOwnsMembership && isVue(fileName)) {
          const routed = editorCarrierSelection(fileName, positionOrRange);
          if (routed === null) return [];
          return routed.runtime.languageService.getApplicableRefactors(
            routed.companion,
            routed.selection,
            preferences,
            triggerReason,
            kind,
            includeInteractiveActions,
          );
        }
        return _getApplicableRefactors(
          fileName,
          positionOrRange,
          preferences,
          triggerReason,
          kind,
          includeInteractiveActions,
        );
      };
    }

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

  // `getExternalFiles` follows the owning host's document model. Non-editor
  // hosts advertise READY framework SOURCE identities whose host hooks serve
  // generated content. VS Code already owns each open raw source ScriptInfo, so
  // its editor surface advertises only the distinct generated companion roots.
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
    const editorOwnsMembership =
      processEditorOwnsCarrierMembershipByProject.get(projectKey) ??
      editorOwnsCarrierMembership(processCurrentConfig);
    if (editorOwnsMembership) {
      const storeDir =
        processStoreDirByProject.get(projectKey) ?? resolveCarrierStoreDir(undefined);
      const store = new DiskCarrierStoreReader(storeDir, projectKey);
      const out = store.isAvailable() ? store.readyIdeCompanions() : [];
      project.projectService.logger.info(
        `[Verter] getExternalFiles(${projectKey}): editor owns sources; ${out.length} companion root(s)`,
      );
      return out;
    }
    const recorded = processStoreDirByProject.get(projectKey);
    const storeDir = recorded ?? resolveCarrierStoreDir(undefined);
    // The reader is scoped to this project, so only its ready source identities
    // are admitted — never a sibling tsconfig's carriers.
    const store = new DiskCarrierStoreReader(storeDir, projectKey);
    if (!store.isAvailable()) {
      return [];
    }
    // VS Code registers the contributed framework languages as extra file
    // extensions, so matching carriers are already configured-project roots.
    // Returning one of those same paths again as a plugin external file makes
    // TypeScript 6 acquire two documents for one identity and can trip
    // ProjectService.setDocument while an open file is reassigned. External
    // files are only the ready carriers the project does not already own.
    const canonical = (fileName: string) => {
      const normalized = normalizePath(fileName);
      return ts.sys.useCaseSensitiveFileNames ? normalized : normalized.toLowerCase();
    };
    const projectFiles = new Set(
      [...project.getRootFiles(), ...project.getFileNames()].map(canonical),
    );
    const out = store.readyIdeSources().filter((source) => !projectFiles.has(canonical(source)));
    project.projectService.logger.info(
      `[Verter] getExternalFiles(${projectKey}): ${out.length} ready carrier source(s)`,
    );
    return out;
  };

  return {
    create,
    getExternalFiles,
    onConfigurationChanged(config: Record<string, unknown>) {
      processCurrentConfig = config;
      writeEditorTsserverAttestation(processCurrentConfig, processBoundProjects);
      for (const update of processUpdateProjectConfig.values()) update(config);
    },
  };
};

export = init;
