import type tsModule from "typescript/lib/tsserverlibrary";
import path from "node:path";
import { existsSync } from "node:fs";
import {
  CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY,
  cleanupCarrierVirtualImportPath,
  containingFileAwareExists,
  editorOwnsCarrierMembership,
  editorOwnsCarrierSourceFeatures,
  e2eProviderOnlyCompletions,
  isCarrierCompanionPath,
  isCarrierSourcePath,
  isModuleLevelDefinition,
  isRelativeVue,
  isRelativeVueTs,
  isVue,
  mapCarrierSourceOffsetToGenerated,
  mapCarrierSourceOffsetToGeneratedAll,
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
import { prepareVueJsxCarrier } from "./helpers/vueJsxAuthority";

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
  /**
   * Bound callbacks captured before this plugin decorates the LanguageService.
   * A visible carrier can be opened through an inferred-project plugin factory
   * while its companion belongs to a configured project. Calling the owning
   * factory's decorated service from that request would either re-enter the
   * plugin or reject the different service identity, so source features query
   * the exact owner's original TypeScript program through these callbacks.
   */
  readonly sourceFeatures: {
    readonly getReferencesAtPosition?: tsModule.LanguageService["getReferencesAtPosition"];
    readonly findReferences?: tsModule.LanguageService["findReferences"];
    readonly findRenameLocations?: tsModule.LanguageService["findRenameLocations"];
  };
  readonly remapContext: CarrierRemapContext;
  readonly responseRemapEnabled: () => boolean;
  readonly getStore: () => DiskCarrierStoreReader;
  readonly readCompanion: (fileName: string) => string | undefined;
  readonly readSource: (fileName: string) => string | undefined;
  readonly editorOwnsMembership: () => boolean;
  readonly ensureResponseSourceScriptInfo: (fileName: string) => boolean;
}
const processEditorProjectRuntimes = new Map<string, ProcessEditorProjectRuntime>();
let processCurrentConfig: Record<string, unknown> | undefined;

const SVELTE_JSX_RUNTIME_SUBPATHS = new Map<string, string>([
  ["@verter/svelte-jsx/jsx-runtime", "jsx-runtime.d.ts"],
  ["@verter/svelte-jsx/jsx-dev-runtime", "jsx-dev-runtime.d.ts"],
  ["@verter/svelte-jsx/svg/jsx-runtime", "svg/jsx-runtime.d.ts"],
  ["@verter/svelte-jsx/svg/jsx-dev-runtime", "svg/jsx-dev-runtime.d.ts"],
  ["@verter/svelte-jsx/mathml/jsx-runtime", "mathml/jsx-runtime.d.ts"],
  ["@verter/svelte-jsx/mathml/jsx-dev-runtime", "mathml/jsx-dev-runtime.d.ts"],
]);

/**
 * Locate the types-only Svelte JSX authority shipped as this plugin's runtime
 * dependency. pnpm development links place it beside the plugin workspace;
 * the VSIX staging contract places it in the plugin's own node_modules; npm is
 * also free to hoist it beside the scoped plugin package. Probe those concrete
 * production layouts once per project and fail closed when none is complete.
 */
function findBundledSvelteJsxPackage(): string | undefined {
  const pluginRoot = path.resolve(__dirname, "..");
  const candidates = [
    path.join(pluginRoot, "node_modules", "@verter", "svelte-jsx"),
    path.resolve(pluginRoot, "..", "svelte-jsx"),
  ];
  let ancestor = pluginRoot;
  for (;;) {
    candidates.push(path.join(ancestor, "node_modules", "@verter", "svelte-jsx"));
    const parent = path.dirname(ancestor);
    if (parent === ancestor) break;
    ancestor = parent;
  }
  for (const candidate of candidates) {
    if (
      existsSync(path.join(candidate, "package.json")) &&
      existsSync(path.join(candidate, "jsx-runtime.d.ts"))
    ) {
      return path.resolve(candidate);
    }
  }
  return undefined;
}

function isWithinDirectory(directory: string, candidate: string): boolean {
  const relative = path.relative(path.resolve(directory), path.resolve(candidate));
  return (
    relative === "" ||
    (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))
  );
}

function sameStorePath(reader: DiskCarrierStoreReader, left: string, right: string): boolean {
  return reader.canonicalPath(left) === reader.canonicalPath(right);
}

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

function asciiEqualsAt(source: string, offset: number, expectedLowercase: string): boolean {
  if (offset < 0 || offset + expectedLowercase.length > source.length) return false;
  for (let index = 0; index < expectedLowercase.length; index += 1) {
    let code = source.charCodeAt(offset + index);
    if (code >= 65 && code <= 90) code += 32;
    if (code !== expectedLowercase.charCodeAt(index)) return false;
  }
  return true;
}

function isHtmlTagBoundary(source: string, offset: number): boolean {
  if (offset >= source.length) return true;
  const code = source.charCodeAt(offset);
  return (
    code === 47 ||
    code === 62 ||
    code === 9 ||
    code === 10 ||
    code === 12 ||
    code === 13 ||
    code === 32
  );
}

function findHtmlTagEnd(source: string, start: number): number {
  let quote = 0;
  for (let offset = start; offset < source.length; offset += 1) {
    const code = source.charCodeAt(offset);
    if (quote !== 0) {
      if (code === quote) quote = 0;
      continue;
    }
    if (code === 34 || code === 39) {
      quote = code;
    } else if (code === 62) {
      return offset;
    }
  }
  return -1;
}

/**
 * Classify a raw SFC offset without interpreting the generated companion.
 *
 * HTML raw-text rules make the first matching `</script>` terminate a script
 * block even when those bytes appear in JavaScript text. The scan therefore
 * needs only quote-aware opening-tag handling plus comment skipping; it does
 * not guess from generated TSX or project configuration. This serves Vue and
 * Svelte carriers alike and does no source allocation on the completion path.
 */
function sfcScriptImportAnchor(source: string | undefined, position: number): number | null {
  if (source === undefined || position < 0 || position > source.length) return null;
  let offset = 0;
  while (offset < source.length) {
    const tagStart = source.indexOf("<", offset);
    if (tagStart < 0 || tagStart > position) return null;
    if (source.startsWith("<!--", tagStart)) {
      const commentEnd = source.indexOf("-->", tagStart + 4);
      if (commentEnd < 0) return null;
      offset = commentEnd + 3;
      continue;
    }
    if (
      !asciiEqualsAt(source, tagStart + 1, "script") ||
      !isHtmlTagBoundary(source, tagStart + 7)
    ) {
      offset = tagStart + 1;
      continue;
    }
    const openEnd = findHtmlTagEnd(source, tagStart + 7);
    if (openEnd < 0) return null;
    let closeStart = source.indexOf("<", openEnd + 1);
    while (
      closeStart >= 0 &&
      (!asciiEqualsAt(source, closeStart + 1, "/script") ||
        !isHtmlTagBoundary(source, closeStart + 8))
    ) {
      closeStart = source.indexOf("<", closeStart + 1);
    }
    let contentStart = openEnd + 1;
    if (source.charCodeAt(contentStart) === 13 && source.charCodeAt(contentStart + 1) === 10) {
      contentStart += 2;
    } else if (source.charCodeAt(contentStart) === 10 || source.charCodeAt(contentStart) === 13) {
      contentStart += 1;
    }
    if (closeStart < 0) return position > openEnd ? contentStart : null;
    if (position > openEnd && position <= closeStart) return contentStart;
    const closeEnd = findHtmlTagEnd(source, closeStart + 8);
    if (closeEnd < 0) return null;
    offset = closeEnd + 1;
  }
  return null;
}

function isInsideSfcScript(source: string | undefined, position: number): boolean {
  return sfcScriptImportAnchor(source, position) !== null;
}

function identifierPrefixAt(source: string | undefined, position: number): string | null {
  if (source === undefined || position <= 0 || position > source.length) return null;
  const lineStart = source.lastIndexOf("\n", position - 1) + 1;
  const match = source.slice(lineStart, position).match(/[$_\p{ID_Continue}]+$/u);
  if (match === null || !/^[$_\p{ID_Start}]/u.test(match[0])) return null;
  return match[0];
}

function identifierSpanAt(source: string | undefined, position: number): tsModule.TextSpan | null {
  if (source === undefined || position < 0 || position > source.length) return null;
  const lineStart = source.lastIndexOf("\n", Math.max(0, position - 1)) + 1;
  const nextLine = source.indexOf("\n", position);
  const lineEnd = nextLine < 0 ? source.length : nextLine;
  const left = source.slice(lineStart, position).match(/[$_\p{ID_Continue}]+$/u)?.[0] ?? "";
  const right = source.slice(position, lineEnd).match(/^[$_\p{ID_Continue}]*/u)?.[0] ?? "";
  const identifier = left + right;
  if (identifier.length === 0 || !/^[$_\p{ID_Start}]/u.test(identifier)) return null;
  return { start: position - left.length, length: identifier.length };
}

function isFrameworkAttributeNamePosition(source: string | undefined, position: number): boolean {
  if (source === undefined || position <= 0 || position > source.length) return false;
  const tagStart = source.lastIndexOf("<", position - 1);
  if (tagStart < 0 || source.lastIndexOf(">", position - 1) > tagStart) return false;
  const first = source.charCodeAt(tagStart + 1);
  if (first === 47 || first === 33 || first === 63) return false;

  let quote = 0;
  let braceDepth = 0;
  for (let offset = tagStart + 1; offset < position; offset++) {
    const code = source.charCodeAt(offset);
    if (quote !== 0) {
      if (code === 92) offset += 1;
      else if (code === quote) quote = 0;
      continue;
    }
    if (code === 34 || code === 39) quote = code;
    else if (code === 123) braceDepth += 1;
    else if (code === 125 && braceDepth > 0) braceDepth -= 1;
  }
  return quote === 0 && braceDepth === 0;
}

function camelToKebab(value: string): string {
  let result = "";
  for (let index = 0; index < value.length; index++) {
    const character = value[index];
    if (character >= "A" && character <= "Z") {
      if (index > 0) result += "-";
      result += character.toLowerCase();
    } else {
      result += character;
    }
  }
  return result;
}

function frameworkAttributeCompletionName(label: string): string {
  const base = label.endsWith("?") ? label.slice(0, -1) : label;
  if (base.startsWith("on") && base.length > 2 && /[A-Z]/.test(base[2])) {
    return `@${camelToKebab(base.slice(2))}`;
  }
  return /[A-Z]/.test(base) ? camelToKebab(base) : base;
}

function isPureImportInsertion(ts: typeof tsModule, text: string): boolean {
  const parsed = ts.createSourceFile(
    "__verter_completion_import.ts",
    text,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  ) as tsModule.SourceFile & { parseDiagnostics?: readonly tsModule.Diagnostic[] };
  return (
    (parsed.parseDiagnostics?.length ?? 0) === 0 &&
    parsed.statements.length > 0 &&
    parsed.statements.every(
      (statement) => ts.isImportDeclaration(statement) || ts.isImportEqualsDeclaration(statement),
    )
  );
}

/** Stable identity for diagnostics produced by more than one LS pass. */
function diagnosticIdentity(
  diagnostic: tsModule.Diagnostic,
  flattenMessage: (
    messageText: string | tsModule.DiagnosticMessageChain,
    newLine: string,
  ) => string,
): string {
  return JSON.stringify([
    diagnostic.file === undefined ? undefined : normalizePath(diagnostic.file.fileName),
    diagnostic.start,
    diagnostic.length,
    diagnostic.category,
    diagnostic.code,
    flattenMessage(diagnostic.messageText, "\n"),
  ]);
}

const init: tsModule.server.PluginModuleFactory = ({ typescript: ts }) => {
  const priorNonEditorExternalsByProject = new WeakMap<tsModule.server.Project, Set<string>>();

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
    let editorOwnsSourceFeatures = editorOwnsCarrierSourceFeatures(effectiveConfig);
    let providerOnlyCompletions = e2eProviderOnlyCompletions(effectiveConfig);
    let carrierStoreRefreshToken = effectiveConfig[CARRIER_STORE_REFRESH_TOKEN_CONFIG_KEY];
    const projectKey = info.project.getProjectName();
    if (!priorNonEditorExternalsByProject.has(info.project)) {
      priorNonEditorExternalsByProject.set(info.project, new Set());
    }
    processBoundProjects.add(projectKey);
    processCurrentConfig = effectiveConfig;
    writeEditorTsserverAttestation(processCurrentConfig, processBoundProjects);
    processStoreDirByProject.set(projectKey, storeDir);
    processEditorOwnsCarrierMembershipByProject.set(projectKey, editorOwnsMembership);
    // PROJECT-SCOPED reader: `create(info)` is per configured project, so every
    // carrier the host hooks serve comes ONLY from this project's manifest entry
    // — a sibling tsconfig's carrier (compiled under different `paths`/`types`/
    // `lib`) is never served here.
    let store = new DiskCarrierStoreReader(
      storeDir,
      projectKey,
      info.serverHost.useCaseSensitiveFileNames,
    );
    const vueJsxContentCache = new Map<
      string,
      { readonly original: string; readonly prepared: string }
    >();
    // Assigned after the host hooks have captured their original implementations.
    // The configuration updater closes over this mutable context so every response
    // provider sees the same project-scoped reader as the live host hooks.
    let remapContext: CarrierRemapContext;
    let invalidatedReadyVersions = store.readyFileVersions();
    const pendingScriptInfoReloads = new Set<string>();
    let pendingResolutionCacheClear = false;
    let refreshScheduled = false;
    processUpdateProjectConfig.set(projectKey, (config) => {
      const nextStoreDir = resolveCarrierStoreDir(config);
      const nextResponseRemap = resolveResponseRemap(config);
      const nextEditorOwnsMembership = editorOwnsCarrierMembership(config);
      const nextEditorOwnsSourceFeatures = editorOwnsCarrierSourceFeatures(config);
      const nextProviderOnlyCompletions = e2eProviderOnlyCompletions(config);
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
        nextEditorOwnsSourceFeatures !== editorOwnsSourceFeatures ||
        nextProviderOnlyCompletions !== providerOnlyCompletions ||
        publicationAdvanced;
      storeDir = nextStoreDir;
      responseRemap = nextResponseRemap;
      editorOwnsMembership = nextEditorOwnsMembership;
      editorOwnsSourceFeatures = nextEditorOwnsSourceFeatures;
      providerOnlyCompletions = nextProviderOnlyCompletions;
      carrierStoreRefreshToken = nextCarrierStoreRefreshToken;
      processStoreDirByProject.set(projectKey, storeDir);
      processEditorOwnsCarrierMembershipByProject.set(projectKey, editorOwnsMembership);
      if (storeChanged) {
        store = new DiskCarrierStoreReader(
          storeDir,
          projectKey,
          info.serverHost.useCaseSensitiveFileNames,
        );
        if (remapContext !== undefined) {
          remapContext.reader = store;
        }
        logger.info(
          store.isAvailable()
            ? `[Verter] carrier store reconfigured: ${storeDir}`
            : "[Verter] carrier store removed; carrier serving disabled",
        );
      }
      if (publicationAdvanced || storeChanged) {
        vueJsxContentCache.clear();
      }
      const nextReadyVersions = store.readyFileVersions();
      for (const [providerPath, version] of nextReadyVersions) {
        if (invalidatedReadyVersions.get(providerPath) !== version) {
          pendingScriptInfoReloads.add(providerPath);
        }
      }
      invalidatedReadyVersions = nextReadyVersions;
      if (!servingStateChanged) return;
      if (publicationAdvanced || storeChanged) {
        pendingResolutionCacheClear = true;
      }
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
          const projectService = info.project.projectService as unknown as {
            reloadFileNamesOfConfiguredProject?: (project: typeof configuredProject) => boolean;
            clearSemanticCache?: (project: typeof configuredProject) => void;
            getScriptInfo?: (fileName: string) =>
              | {
                  reloadFromFile?: () => boolean;
                }
              | undefined;
          };
          // A configured project may have cached Svelte's ambient `*.svelte`
          // fallback before the authored carrier was published. Clearing only
          // the target project's semantic/resolution cache is therefore the
          // first refresh step; a dirty graph alone retains the stale module
          // resolution. This private runtime method is present across supported
          // TS 5.x/6.x and is deliberately invoked through this narrow cast.
          if (pendingResolutionCacheClear) {
            pendingResolutionCacheClear = false;
            projectService.clearSemanticCache?.(configuredProject);
          }
          // External carrier roots are virtual: their ScriptInfos have no disk
          // watcher that can observe the content-addressed blob swap. Refresh
          // only companions whose manifest identity advanced, using the patched
          // serverHost.readFile as the sole content authority, before rebuilding
          // the configured graph. This covers both IDE roots and imported API
          // carriers without reloading every carrier on each keystroke.
          for (const providerPath of pendingScriptInfoReloads) {
            pendingScriptInfoReloads.delete(providerPath);
            try {
              projectService.getScriptInfo?.(providerPath)?.reloadFromFile?.();
            } catch (error) {
              logger.info(
                `[Verter] failed to reload carrier ScriptInfo ${providerPath}: ${String(error)}`,
              );
            }
          }
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
    const bundledSvelteJsxPackage = findBundledSvelteJsxPackage();
    if (bundledSvelteJsxPackage === undefined) {
      logger.info(
        "[Verter] bundled @verter/svelte-jsx runtime is unavailable; Svelte JSX resolution fails closed",
      );
    }
    const configuredProjectRoot =
      projectKey.endsWith(".json") && !projectKey.startsWith("/dev/null/")
        ? path.dirname(projectKey)
        : directory;
    const ownerResolutionAnchor = path.join(configuredProjectRoot, "__verter_svelte_owner__.tsx");
    const ownerModuleResolutionHost: tsModule.ModuleResolutionHost = {
      fileExists: _fileExists,
      readFile: _readFile,
      directoryExists: info.serverHost.directoryExists?.bind(info.serverHost),
      getDirectories: info.serverHost.getDirectories?.bind(info.serverHost),
      realpath: info.serverHost.realpath?.bind(info.serverHost),
      useCaseSensitiveFileNames: info.serverHost.useCaseSensitiveFileNames,
    };

    const specializeEditorTsserverCarrier = (
      providerPath: string,
      content: string | undefined,
    ): string | undefined => {
      if (content === undefined) return undefined;
      const owned = store.ownedSourceFor(providerPath);
      if (owned?.role !== "CarrierIde" || !sameStorePath(store, owned.provider_uri, providerPath)) {
        return content;
      }
      const cacheKey = store.canonicalPath(providerPath);
      const cached = vueJsxContentCache.get(cacheKey);
      if (cached?.original === content) return cached.prepared;

      const prepared = prepareVueJsxCarrier(providerPath, content)?.content ?? content;
      vueJsxContentCache.set(cacheKey, { original: content, prepared });
      return prepared;
    };

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
          return specializeEditorTsserverCarrier(
            companion,
            store.readBlobSync(sourceReady.blob_rel, companion),
          );
        }
        const sourceCompanion = store.companionForSource(fileName);
        if (sourceCompanion && store.ownedSourceFor(sourceCompanion)) {
          const result = coldResolveCompanion(store, sourceCompanion);
          if (result.kind === "ready") {
            return specializeEditorTsserverCarrier(
              sourceCompanion,
              store.readBlobSync(result.readyFile.blob_rel, sourceCompanion),
            );
          }
          if (result.kind === "lastGood") {
            return specializeEditorTsserverCarrier(sourceCompanion, result.content);
          }
        }
      }

      // A direct companion provider path (`Comp.vue.tsx`) → serve it directly.
      const ready = store.readyFile(fileName);
      if (ready) {
        return specializeEditorTsserverCarrier(
          fileName,
          store.readBlobSync(ready.blob_rel, fileName),
        );
      }
      if (store.ownedSourceFor(fileName)) {
        const result = coldResolveCompanion(store, fileName);
        if (result.kind === "ready") {
          return specializeEditorTsserverCarrier(
            fileName,
            store.readBlobSync(result.readyFile.blob_rel, fileName),
          );
        }
        if (result.kind === "lastGood") {
          return specializeEditorTsserverCarrier(fileName, result.content);
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
        if (editorOwnsMembership && editorOwnsSourceFeatures) {
          // TypeScript's cross-project references/rename coordinator refuses to
          // invoke a project's language-service provider until `containsFile`
          // admits the request identity. A visible SFC is intentionally absent
          // from the companion Program, so assign that *virtual request*
          // membership to exactly the configured runtime that owns its ready
          // companion. Inferred projects must return false for the same source:
          // dual claims make TypeScript aggregate a remapped source span through
          // a Program that has no raw SFC SourceFile.
          const owner = editorOwnerForSource(fileName);
          if (owner !== undefined) return owner === editorRuntime;
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
      if (owned && sameStorePath(store, owned.provider_uri, fileName)) {
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

        const svelteJsxRuntime = SVELTE_JSX_RUNTIME_SUBPATHS.get(moduleName);
        if (svelteJsxRuntime !== undefined) {
          // The projection and JSX namespace are one versioned Verter unit.
          // Never accept a consumer-installed shim that may not match this
          // compiler/plugin version; use the plugin-owned runtime or fail closed.
          if (bundledSvelteJsxPackage === undefined) return null;
          const resolvedFileName = path.join(bundledSvelteJsxPackage, svelteJsxRuntime);
          return existsSync(resolvedFileName)
            ? {
                extension: ts.Extension.Dts,
                isExternalLibraryImport: true,
                resolvedFileName,
              }
            : null;
        }

        if (
          bundledSvelteJsxPackage !== undefined &&
          isWithinDirectory(bundledSvelteJsxPackage, containingFile) &&
          (moduleName === "svelte" || moduleName.startsWith("svelte/"))
        ) {
          // The shim lives with the editor plugin, outside the user's project,
          // so a normal node_modules ancestor walk cannot see that project's
          // Svelte install. Resolve its transitive imports from the exact owning
          // configured-project root. A missing owner dependency is a hard miss;
          // do not accidentally bind to a development/extension copy of Svelte.
          return (
            ts.resolveModuleName(
              moduleName,
              ownerResolutionAnchor,
              info.project.getCompilerOptions(),
              ownerModuleResolutionHost,
            ).resolvedModule ?? null
          );
        }

        // An already-IDE-carrier-shaped relative specifier resolves to itself.
        if (isRelativeVueTs(moduleName)) {
          const resolved = path.resolve(path.dirname(containingFile), moduleName);
          return {
            extension: providerExtension(resolved),
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
            if (resolvedModule === null) {
              return { resolvedModule: undefined };
            }
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

    // IDE carriers contain JSX, so preserve it for TypeScript. A ready
    // JavaScript carrier is a distinct `.jsx` external root; TypeScript drops
    // such a root entirely unless `allowJs` is enabled. Turn that admission bit
    // on only while this exact project owns a ready JSX carrier. `checkJs`
    // remains untouched: the generated carrier opts into checking file-locally
    // with `// @ts-check`, so unrelated project JavaScript is not reclassified.
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
      const hasReadyJsxCarrier = store
        .readyIdeCompanions()
        .some((provider) => store.readyFile(provider)?.script_kind === "JSX");
      if (hasReadyJsxCarrier && settings.allowJs !== true) {
        settings.allowJs = true;
      }
      return settings;
    };

    const languageService = info.languageService;
    const sourceFeatures: ProcessEditorProjectRuntime["sourceFeatures"] = {
      getReferencesAtPosition: languageService.getReferencesAtPosition?.bind(languageService),
      findReferences: languageService.findReferences?.bind(languageService),
      findRenameLocations: languageService.findRenameLocations?.bind(languageService),
    };

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
      sourceFeatures,
      remapContext,
      responseRemapEnabled: () => responseRemap,
      getStore: () => store,
      readCompanion: carrierContent,
      readSource: readOriginalSource,
      editorOwnsMembership: () => editorOwnsMembership,
      ensureResponseSourceScriptInfo: (fileName) => {
        const companion = store.companionForSource(fileName);
        if (companion === undefined) return true;
        if (info.project.projectService.getScriptInfo(fileName) !== undefined) return true;

        const ready = store.readyFile(companion);
        if (ready === undefined) return false;
        const create = info.project.projectService.getOrCreateScriptInfoForNormalizedPath;
        if (typeof create !== "function") return false;

        // Protocol commands such as `references` serialize LanguageService
        // spans through ProjectService and require a ScriptInfo for every target.
        // An imported, unopened SFC is intentionally absent from the TypeScript
        // Program even though its generated companion is present. Materialize a
        // closed mixed-content ScriptInfo from the owning server host so the
        // source-facing response remains serializable without pretending the raw
        // SFC is an open/program member or leaking the companion path instead.
        return (
          create.call(
            info.project.projectService,
            ts.server.toNormalizedPath(fileName),
            false,
            undefined,
            manifestScriptKind(ts, ready.script_kind),
            true,
            info.serverHost,
          ) !== undefined
        );
      },
    };
    processEditorProjectRuntimes.set(projectKey, editorRuntime);

    function editorOwnerForSource(fileName: string): ProcessEditorProjectRuntime | undefined {
      // Most calls arrive on the configured project that owns the companion.
      // Prefer that exact runtime before consulting process-wide candidates;
      // stale inferred-project factories can coexist for the lifetime of
      // tsserver and must never make an otherwise exact local owner ambiguous.
      if (editorRuntime.editorOwnsMembership()) {
        const localStore = editorRuntime.getStore();
        const localCompanion = localStore.companionForSource(fileName);
        if (localCompanion !== undefined && localStore.readyFile(localCompanion) !== undefined) {
          return editorRuntime;
        }
      }
      const runtimes = [...processEditorProjectRuntimes.values()];
      const candidates = runtimes.filter((runtime) => {
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

    function protocolSafeMappedSpans<T extends tsModule.DocumentSpan>(
      runtime: ProcessEditorProjectRuntime,
      spans: readonly T[],
    ): T[] {
      return spans.filter((span) => runtime.ensureResponseSourceScriptInfo(span.fileName));
    }

    function protocolSafeReferencedSymbols(
      runtime: ProcessEditorProjectRuntime,
      symbols: readonly tsModule.ReferencedSymbol[],
    ): tsModule.ReferencedSymbol[] {
      const safe: tsModule.ReferencedSymbol[] = [];
      for (const symbol of symbols) {
        if (!runtime.ensureResponseSourceScriptInfo(symbol.definition.fileName)) continue;
        safe.push({
          ...symbol,
          references: protocolSafeMappedSpans(runtime, symbol.references),
        });
      }
      return safe;
    }

    /** Resolve a generated companion revisit to the configured project that published it. */
    function editorOwnerForCompanion(fileName: string): ProcessEditorProjectRuntime | undefined {
      if (editorRuntime.editorOwnsMembership() && editorRuntime.getStore().readyFile(fileName)) {
        return editorRuntime;
      }
      const candidates = [...processEditorProjectRuntimes.values()].filter(
        (runtime) =>
          runtime.editorOwnsMembership() && runtime.getStore().readyFile(fileName) !== undefined,
      );
      if (candidates.length <= 1) return candidates[0];

      const sources = candidates.map(
        (runtime) => runtime.getStore().ownedSourceFor(fileName)?.source_uri,
      );
      const source = sources[0];
      if (
        source === undefined ||
        sources.some(
          (candidate) =>
            candidate === undefined || !sameStorePath(candidates[0].getStore(), source, candidate),
        )
      ) {
        return undefined;
      }
      return editorOwnerForSource(source);
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
      if (!editorOwnsSourceFeatures || !isVue(fileName)) return null;
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

    /**
     * Discover generated projections linked by an exact source-map origin.
     * Vue's ref-unwrapped alias maps to the authored setup declaration, so a
     * raw TypeScript reference/rename set containing either declaration yields
     * both generated positions. This consumes already-returned spans and never
     * performs a nested language-service query from inside an active provider
     * callback (tsserver does not guarantee that reentrant path).
     */
    function linkedEditorCarrierPositionsFromSpans(
      routed: EditorCarrierPosition,
      spans: readonly { fileName: string; textSpan: tsModule.TextSpan }[],
    ): number[] {
      const positions = new Set<number>();
      for (const span of spans) {
        if (!sameStorePath(routed.runtime.getStore(), span.fileName, routed.companion)) continue;
        const mapped = remapCarrierSpan(
          routed.runtime.getStore(),
          routed.companion,
          span.textSpan,
          routed.runtime.readCompanion,
          routed.runtime.readSource,
        );
        if (mapped === null) continue;
        for (const position of mapCarrierSourceOffsetToGeneratedAll(
          routed.runtime.getStore(),
          routed.companion,
          mapped.fileName,
          mapped.textSpan.start,
          routed.runtime.readCompanion,
          routed.runtime.readSource,
        )) {
          positions.add(position);
        }
      }
      return [...positions];
    }

    function documentSpanKey(span: { fileName: string; textSpan: tsModule.TextSpan }): string {
      return `${normalizePath(span.fileName)}\0${span.textSpan.start}\0${span.textSpan.length}`;
    }

    function dedupeDocumentSpans<T extends { fileName: string; textSpan: tsModule.TextSpan }>(
      spans: readonly T[],
    ): T[] {
      const seen = new Set<string>();
      return spans.filter((span) => {
        const key = documentSpanKey(span);
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
      });
    }

    /**
     * Collapse duplicate rename targets while preferring an exact identifier
     * replacement over TypeScript's shorthand-preservation decoration. Linked
     * carrier projections can map both a generated object shorthand
     * (`foo` -> `foo: next`) and the authored declaration (`foo` -> `next`) to
     * the same source span. The plain location is the faithful source edit.
     * A lone decorated location is retained: real authored shorthand syntax
     * still needs TypeScript's prefix/suffix semantics.
     */
    function dedupeRenameLocations(
      locations: readonly tsModule.RenameLocation[],
    ): tsModule.RenameLocation[] {
      const out: tsModule.RenameLocation[] = [];
      const indexBySpan = new Map<string, number>();
      const hasDecoration = (location: tsModule.RenameLocation): boolean =>
        Boolean(location.prefixText) || Boolean(location.suffixText);
      for (const location of locations) {
        const key = documentSpanKey(location);
        const existingIndex = indexBySpan.get(key);
        if (existingIndex === undefined) {
          indexBySpan.set(key, out.length);
          out.push(location);
        } else if (hasDecoration(out[existingIndex]) && !hasDecoration(location)) {
          out[existingIndex] = location;
        }
      }
      return out;
    }

    function mergeReferencedSymbolsByDefinition(
      symbols: readonly tsModule.ReferencedSymbol[],
    ): tsModule.ReferencedSymbol[] {
      const merged = new Map<string, tsModule.ReferencedSymbol>();
      for (const symbol of symbols) {
        const key = documentSpanKey(symbol.definition);
        const existing = merged.get(key);
        if (existing === undefined) {
          merged.set(key, {
            ...symbol,
            references: dedupeDocumentSpans(symbol.references),
          });
        } else {
          existing.references = dedupeDocumentSpans([...existing.references, ...symbol.references]);
        }
      }
      return [...merged.values()];
    }

    /**
     * A selected non-editor provider still needs this plugin's project-membership
     * and module-resolution hooks, but it is the sole owner of semantic features
     * on the visible framework source. Returning the raw TypeScript service's
     * answer would register a second provider against `.vue`; VS Code merges that
     * answer with Verter's managed/shared provider and leaks globals, generated
     * helpers, and non-actionable completion items. Source-feature wrappers must
     * therefore fail closed while membership remains enabled.
     */
    const editorYieldsCarrierSourceFeatures = (fileName: string): boolean => {
      if (!editorOwnsMembership || editorOwnsSourceFeatures) return false;
      if (isVue(fileName)) return true;
      return isCarrierCompanionPath(store, fileName);
    };

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
        !sameStorePath(start.runtime.getStore(), start.companion, end.companion) ||
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

    /** Map one companion response span back to the exact visible editor source. */
    function editorSourceSpan(
      routed: EditorCarrierPosition,
      sourceFileName: string,
      span: tsModule.TextSpan,
    ): tsModule.TextSpan | null {
      const mapped = remapCarrierSpan(
        routed.runtime.getStore(),
        routed.companion,
        span,
        routed.runtime.readCompanion,
        routed.runtime.readSource,
      );
      return mapped !== null &&
        sameStorePath(routed.runtime.getStore(), mapped.fileName, sourceFileName)
        ? mapped.textSpan
        : null;
    }

    function remapEditorCompletionChanges(
      runtime: ProcessEditorProjectRuntime,
      sourceFileName: string,
      sourcePosition: number,
      changes: readonly tsModule.FileTextChanges[],
    ): tsModule.FileTextChanges[] {
      const runtimeContext: CarrierRemapContext = {
        reader: runtime.getStore(),
        readCompanion: runtime.readCompanion,
        readSource: runtime.readSource,
        fileExists: _fileExists,
      };
      const sourceText = runtime.readSource(sourceFileName);
      const importAnchor = sfcScriptImportAnchor(sourceText, sourcePosition);
      const out: tsModule.FileTextChanges[] = [];
      for (const change of changes) {
        const mapped = remapAllFileTextChanges(runtimeContext, [change]);
        if (mapped.length > 0) {
          out.push(...mapped);
          continue;
        }

        // TypeScript inserts a new import at generated offset zero, which is
        // correctly unmapped when the companion starts with synthetic JSX and
        // helper preambles. Recover only a syntactically pure import insertion,
        // and only into the exact source script block that requested the
        // completion. Every other unmappable edit remains dropped atomically.
        const owned = runtime.getStore().ownedSourceFor(change.fileName);
        if (
          importAnchor === null ||
          owned === undefined ||
          !sameStorePath(runtime.getStore(), owned.provider_uri, change.fileName) ||
          !sameStorePath(runtime.getStore(), owned.source_uri, sourceFileName) ||
          change.textChanges.length === 0 ||
          change.textChanges.some(
            (edit) => edit.span.length !== 0 || !isPureImportInsertion(ts, edit.newText),
          )
        ) {
          continue;
        }
        const existsRelToContaining = containingFileAwareExists(_fileExists, sourceFileName);
        out.push({
          ...change,
          fileName: sourceFileName,
          textChanges: change.textChanges.map((edit) => ({
            ...edit,
            span: { start: importAnchor, length: 0 },
            newText: cleanupCarrierVirtualImportPath(edit.newText, existsRelToContaining),
          })),
        });
      }
      return out;
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
      nonEditorSupplement?: (target: string) => readonly T[],
    ): T[] {
      if (editorYieldsCarrierSourceFeatures(fileName)) return [];
      if (!isCarrierSourcePath(fileName)) {
        return [...query(fileName)];
      }
      // On the out-of-process verter_lsp backend the configured Program owns
      // the carrier SOURCE identity (`Comp.vue` / `Comp.svelte`), while the host serves the IDE
      // companion bytes for that SourceFile. Query that identity directly and
      // leave the generated coordinates untouched for Rust's authoritative
      // source-map merge. The editor-owned topology below is different: VS Code
      // keeps the raw SFC ScriptInfo, so its request must route to the distinct
      // companion and map the response back here.
      if (!editorOwnsMembership) {
        const primary = [...query(fileName)];
        const companion = store.companionForSource(fileName);
        if (
          nonEditorSupplement === undefined ||
          companion === undefined ||
          store.readyFile(companion) === undefined
        ) {
          return primary;
        }

        // tsserver can serialize semantic diagnostics for the configured
        // Program's carrier-source identity, but its separate suggestion command
        // requires a client-owned ScriptInfo that this internal backend must not
        // manufacture. Fold the LanguageService suggestion pass into the semantic
        // result while we are already inside the owning configured Program. Rust
        // still issues all three protocol passes and performs the public union;
        // the standalone suggestion request may fail without losing TS6133.
        const out = [...primary];
        const seen = new Set(
          primary.map((diagnostic) =>
            diagnosticIdentity(diagnostic, ts.flattenDiagnosticMessageText),
          ),
        );
        for (const diagnostic of nonEditorSupplement(fileName)) {
          const key = diagnosticIdentity(diagnostic, ts.flattenDiagnosticMessageText);
          if (seen.has(key)) continue;
          seen.add(key);
          out.push(diagnostic);
        }
        return out;
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
    const _getSemanticDiagnostics = languageService.getSemanticDiagnostics.bind(languageService);
    const _getSuggestionDiagnostics =
      languageService.getSuggestionDiagnostics.bind(languageService);

    languageService.getSyntacticDiagnostics = (fileName) =>
      editorSourceDiagnostics(fileName, "getSyntacticDiagnostics", _getSyntacticDiagnostics);

    languageService.getSemanticDiagnostics = (fileName) =>
      editorSourceDiagnostics(
        fileName,
        "getSemanticDiagnostics",
        _getSemanticDiagnostics,
        _getSuggestionDiagnostics,
      );

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
        if (editorYieldsCarrierSourceFeatures(fileName)) {
          return { spans: [], endOfLineState: 0 as tsModule.EndOfLineState };
        }
        if (!isVue(fileName)) {
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
          if (
            mapped !== null &&
            sameStorePath(routed.runtime.getStore(), mapped.fileName, fileName)
          ) {
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

    const _getDocumentHighlights = languageService.getDocumentHighlights?.bind(languageService);
    if (_getDocumentHighlights) {
      languageService.getDocumentHighlights = (fileName, position, filesToSearch) => {
        if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
        if (!isVue(fileName)) {
          return _getDocumentHighlights(fileName, position, filesToSearch);
        }
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) return undefined;
        const targetStore = routed.runtime.getStore();
        const mappedSearchFiles = [
          ...new Set(
            filesToSearch.map((candidate) => {
              const companion = targetStore.companionForSource(candidate);
              return companion !== undefined && targetStore.readyFile(companion) !== undefined
                ? companion
                : candidate;
            }),
          ),
        ];
        const result = routed.runtime.languageService.getDocumentHighlights(
          routed.companion,
          routed.position,
          mappedSearchFiles,
        );
        if (result === undefined) return undefined;

        const out: tsModule.DocumentHighlights[] = [];
        for (const highlightedFile of result) {
          if (!isCarrierCompanionPath(targetStore, highlightedFile.fileName)) {
            out.push({
              ...highlightedFile,
              highlightSpans: [...highlightedFile.highlightSpans],
            });
            continue;
          }
          const bySource = new Map<string, tsModule.HighlightSpan[]>();
          for (const highlight of highlightedFile.highlightSpans) {
            const mapped = remapCarrierSpan(
              targetStore,
              highlightedFile.fileName,
              highlight.textSpan,
              routed.runtime.readCompanion,
              routed.runtime.readSource,
            );
            if (mapped === null) continue;
            const key = normalizePath(mapped.fileName);
            const spans = bySource.get(key) ?? [];
            spans.push({ ...highlight, textSpan: mapped.textSpan });
            bySource.set(key, spans);
          }
          for (const [sourceFileName, highlightSpans] of bySource) {
            out.push({ fileName: sourceFileName, highlightSpans });
          }
        }
        return out.length > 0 ? out : undefined;
      };
    }

    function remapDefinitionLike<
      T extends {
        fileName: string;
        textSpan: tsModule.TextSpan;
        contextSpan?: tsModule.TextSpan;
        originalTextSpan?: tsModule.TextSpan;
        kind?: string;
        verterAliasTarget?: true;
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
          (isModuleLevelDefinition(definition) || definition.verterAliasTarget === true)
        ) {
          // Fail-closed-safe: if the source path can't be resolved, DROP the
          // definition (never surface the companion path).
          return remapModuleLevelCompanionToSource(store, definition);
        }
        // FAIL CLOSED. A specific-token definition in a known companion whose
        // span cannot map is not user-navigable. Drop it rather than leaking a
        // private virtual path or pairing a source URI with a generated offset.
        if (isCarrierCompanionPath(store, definition.fileName)) return undefined;

        // A non-companion path is a real file/display token; cosmetic suffix
        // cleanup remains safe because no carrier span is being reinterpreted.
        definition.fileName = fixCarrierImportPath(definition.fileName);
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
      if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
      if (isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) return undefined;
        const result = routed.runtime.languageService.getDefinitionAndBoundSpan(
          routed.companion,
          routed.position,
        );
        if (result === undefined) return undefined;
        const textSpan = editorSourceSpan(routed, fileName, result.textSpan);
        return textSpan === null ? undefined : { ...result, textSpan };
      }
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
      if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
      if (isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        return routed === null
          ? undefined
          : routed.runtime.languageService.getDefinitionAtPosition(
              routed.companion,
              routed.position,
            );
      }
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
      if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
      if (isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        return routed === null
          ? undefined
          : routed.runtime.languageService.getTypeDefinitionAtPosition(
              routed.companion,
              routed.position,
            );
      }
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
      if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
      if (isVue(fileName)) {
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
        if (
          mapped === null ||
          !sameStorePath(routed.runtime.getStore(), mapped.fileName, fileName)
        ) {
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
      if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
      if (isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) return undefined;
        const result = routed.runtime.languageService.getCompletionEntryDetails(
          routed.companion,
          routed.position,
          entryName,
          formatOptions,
          source,
          preferences,
          data,
        );
        if (result?.codeActions && responseRemap) {
          for (const action of result.codeActions) {
            action.changes = remapEditorCompletionChanges(
              routed.runtime,
              fileName,
              position,
              action.changes,
            );
          }
        }
        return result;
      }
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
          if (responseRemap && !(editorOwnsMembership && isCarrierCompanionPath(store, fileName))) {
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
    const isMemberCompletionEntry = (entry: tsModule.CompletionEntry): boolean => {
      switch (entry.kind) {
        case ts.ScriptElementKind.memberVariableElement:
        case ts.ScriptElementKind.memberFunctionElement:
        case ts.ScriptElementKind.memberGetAccessorElement:
        case ts.ScriptElementKind.memberSetAccessorElement:
        case ts.ScriptElementKind.enumMemberElement:
          return true;
        default:
          return false;
      }
    };
    const isLocalTemplateScopeEntry = (entry: tsModule.CompletionEntry): boolean => {
      if (
        entry.source !== undefined ||
        entry.kindModifiers?.split(",").includes(ts.ScriptElementKindModifier.ambientModifier) ===
          true ||
        entry.name.startsWith("___VERTER___") ||
        entry.name.startsWith("$V_") ||
        [
          "__props",
          "__emit",
          "__slots",
          "__expose",
          "__returned",
          "arguments",
          "globalThis",
          "undefined",
        ].includes(entry.name)
      ) {
        return false;
      }
      switch (entry.kind) {
        case ts.ScriptElementKind.moduleElement:
        case ts.ScriptElementKind.classElement:
        case ts.ScriptElementKind.localClassElement:
        case ts.ScriptElementKind.enumElement:
        case ts.ScriptElementKind.variableElement:
        case ts.ScriptElementKind.localVariableElement:
        case ts.ScriptElementKind.variableUsingElement:
        case ts.ScriptElementKind.variableAwaitUsingElement:
        case ts.ScriptElementKind.functionElement:
        case ts.ScriptElementKind.localFunctionElement:
        case ts.ScriptElementKind.parameterElement:
        case ts.ScriptElementKind.alias:
        case ts.ScriptElementKind.constElement:
        case ts.ScriptElementKind.letElement:
          return true;
        default:
          return false;
      }
    };
    languageService.getCompletionsAtPosition = (
      fileName,
      position,
      options,
      formattingSettings,
    ) => {
      let result: tsModule.CompletionInfo | undefined;
      if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
      if (isVue(fileName)) {
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) return undefined;
        const scriptOwned = isInsideSfcScript(routed.runtime.readSource(fileName), position);
        let companionResult = routed.runtime.languageService.getCompletionsAtPosition(
          routed.companion,
          routed.position,
          options,
          formattingSettings,
        );
        if (companionResult === undefined) return undefined;
        // Script blocks are wholly TypeScript-owned: locals, globals, module
        // exports, and actionable auto-imports must behave like a regular
        // `.ts`/`.js` file. In template expressions, only a true member list has
        // enough expression context to be safe on the direct editor route;
        // Verter owns bare render-proxy scope, attributes, and recovery there.
        if (
          !scriptOwned &&
          (!companionResult.isMemberCompletion ||
            companionResult.entries.some((entry) => !isMemberCompletionEntry(entry)))
        ) {
          const prefix =
            identifierPrefixAt(routed.runtime.readSource(fileName), position) ??
            (providerOnlyCompletions ? "" : null);
          if (prefix === null) return undefined;
          const lexicalEntries = companionResult.entries.filter(
            (entry) => entry.name.startsWith(prefix) && isLocalTemplateScopeEntry(entry),
          );
          if (lexicalEntries.length === 0) return undefined;
          companionResult = {
            ...companionResult,
            entries: lexicalEntries,
            isGlobalCompletion: false,
            isMemberCompletion: false,
          };
        }
        if (
          !scriptOwned &&
          isFrameworkAttributeNamePosition(routed.runtime.readSource(fileName), position)
        ) {
          companionResult = {
            ...companionResult,
            entries: companionResult.entries.map((entry) => {
              const name = frameworkAttributeCompletionName(entry.name);
              if (name === entry.name) return entry;
              return {
                ...entry,
                name,
                kindModifiers: (entry.kindModifiers ?? "")
                  .split(",")
                  .filter((modifier) => modifier !== "optional")
                  .join(","),
                insertText:
                  entry.insertText === undefined || entry.insertText === entry.name
                    ? name
                    : frameworkAttributeCompletionName(entry.insertText),
              };
            }),
          };
        }
        const mapSpan = (span: tsModule.TextSpan): tsModule.TextSpan | null => {
          const mapped = remapCarrierSpan(
            routed.runtime.getStore(),
            routed.companion,
            span,
            routed.runtime.readCompanion,
            routed.runtime.readSource,
          );
          return mapped !== null &&
            sameStorePath(routed.runtime.getStore(), mapped.fileName, fileName)
            ? mapped.textSpan
            : null;
        };
        const mapCompletionReplacementSpan = (
          span: tsModule.TextSpan,
        ): tsModule.TextSpan | null => {
          const mapped = mapSpan(span);
          if (mapped !== null) return mapped;

          // JSX projections commonly map the authored identifier but leave the
          // immediately-following generated delimiter synthetic. TypeScript's
          // optional replacement span ends at that delimiter, so the generic
          // two-endpoint mapper correctly refuses it. A completion request has
          // an additional safe witness: the visible identifier containing the
          // caret. Recover only when the generated replacement contains the
          // routed caret and its mapped start is exactly that authored token's
          // start. This never guesses across a source boundary or generated-only
          // region, and keeps the generic span mapper strict for every other
          // semantic feature.
          const spanEnd = span.start + span.length;
          if (routed.position < span.start || routed.position > spanEnd) return null;
          const sourceIdentifier = identifierSpanAt(routed.runtime.readSource(fileName), position);
          if (sourceIdentifier === null) return null;
          const mappedStart = mapSpan({ start: span.start, length: 0 });
          return mappedStart?.start === sourceIdentifier.start ? sourceIdentifier : null;
        };
        const optionalReplacementSpan = companionResult.optionalReplacementSpan
          ? mapCompletionReplacementSpan(companionResult.optionalReplacementSpan)
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
          const replacementSpan = mapCompletionReplacementSpan(entry.replacementSpan);
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
        if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
        if (isVue(fileName)) {
          const routed = editorCarrierPosition(fileName, position);
          if (routed === null) return undefined;
          const queryReferences = routed.runtime.sourceFeatures.getReferencesAtPosition;
          if (queryReferences === undefined) return undefined;
          const pending = [routed.position];
          const queried = new Set<number>();
          const rawReferences: tsModule.ReferenceEntry[] = [];
          while (pending.length > 0) {
            const linkedPosition = pending.shift()!;
            if (queried.has(linkedPosition)) continue;
            queried.add(linkedPosition);
            const batch = queryReferences(routed.companion, linkedPosition) ?? [];
            rawReferences.push(...batch);
            for (const linked of linkedEditorCarrierPositionsFromSpans(routed, batch)) {
              if (!queried.has(linked)) pending.push(linked);
            }
          }
          const deduped = dedupeDocumentSpans(rawReferences);
          const mapped = routed.runtime.responseRemapEnabled()
            ? remapDocumentSpans(routed.runtime.remapContext, deduped)
            : deduped;
          return dedupeDocumentSpans(protocolSafeMappedSpans(routed.runtime, mapped));
        }
        const companionOwner = editorOwnsSourceFeatures
          ? editorOwnerForCompanion(fileName)
          : undefined;
        const result = companionOwner?.sourceFeatures.getReferencesAtPosition
          ? companionOwner.sourceFeatures.getReferencesAtPosition(fileName, position)
          : _getReferencesAtPosition(fileName, position);
        const shouldRemap = companionOwner?.responseRemapEnabled() ?? responseRemap;
        if (!result || !shouldRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `ReferenceEntry` whose `fileName` is a carrier companion is
        // mapped back to source (path + span); an unmappable companion
        // reference (a generated-only region) is DROPPED. References in the
        // user's real `.ts` pass through unchanged.
        const responseRuntime = companionOwner ?? editorRuntime;
        return dedupeDocumentSpans(
          protocolSafeMappedSpans(
            responseRuntime,
            remapDocumentSpans(responseRuntime.remapContext, result),
          ),
        );
      };
    }

    const _findReferences = languageService.findReferences?.bind(languageService);
    if (_findReferences) {
      languageService.findReferences = (fileName, position) => {
        if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
        if (isVue(fileName)) {
          const routed = editorCarrierPosition(fileName, position);
          if (routed === null) return undefined;
          const queryReferences = routed.runtime.sourceFeatures.findReferences;
          if (queryReferences === undefined) return undefined;
          const pending = [routed.position];
          const queried = new Set<number>();
          const rawMerged = new Map<string, tsModule.ReferencedSymbol>();
          while (pending.length > 0) {
            const linkedPosition = pending.shift()!;
            if (queried.has(linkedPosition)) continue;
            queried.add(linkedPosition);
            const batch = queryReferences(routed.companion, linkedPosition) ?? [];
            const spans = batch.flatMap((symbol) => [symbol.definition, ...symbol.references]);
            for (const linked of linkedEditorCarrierPositionsFromSpans(routed, spans)) {
              if (!queried.has(linked)) pending.push(linked);
            }
            for (const symbol of batch) {
              const key = documentSpanKey(symbol.definition);
              const existing = rawMerged.get(key);
              if (existing === undefined) {
                rawMerged.set(key, {
                  ...symbol,
                  references: dedupeDocumentSpans(symbol.references),
                });
              } else {
                existing.references = dedupeDocumentSpans([
                  ...existing.references,
                  ...symbol.references,
                ]);
              }
            }
          }
          if (!routed.runtime.responseRemapEnabled()) return [...rawMerged.values()];
          const mapped: tsModule.ReferencedSymbol[] = [];
          for (const symbol of rawMerged.values()) {
            const remapped = remapReferencedSymbol(routed.runtime.remapContext, symbol);
            if (remapped !== undefined) mapped.push(remapped);
          }
          const merged = mergeReferencedSymbolsByDefinition(
            protocolSafeReferencedSymbols(routed.runtime, mapped),
          );
          return merged;
        }
        const companionOwner = editorOwnsSourceFeatures
          ? editorOwnerForCompanion(fileName)
          : undefined;
        const result = companionOwner?.sourceFeatures.findReferences
          ? companionOwner.sourceFeatures.findReferences(fileName, position)
          : _findReferences(fileName, position);
        const shouldRemap = companionOwner?.responseRemapEnabled() ?? responseRemap;
        if (!result || !shouldRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `ReferencedSymbol` is remapped: its definition + every reference
        // entry. A symbol whose definition is an unmappable companion is
        // dropped entirely (fail closed).
        const out: tsModule.ReferencedSymbol[] = [];
        const responseContext = companionOwner?.remapContext ?? remapContext;
        for (const symbol of result) {
          const mapped = remapReferencedSymbol(responseContext, symbol);
          if (mapped !== undefined) {
            out.push(mapped);
          }
        }
        return mergeReferencedSymbolsByDefinition(
          protocolSafeReferencedSymbols(companionOwner ?? editorRuntime, out),
        );
      };
    }

    const _getImplementationAtPosition =
      languageService.getImplementationAtPosition?.bind(languageService);
    if (_getImplementationAtPosition) {
      languageService.getImplementationAtPosition = (fileName, position) => {
        if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
        if (isVue(fileName)) {
          const routed = editorCarrierPosition(fileName, position);
          return routed === null
            ? undefined
            : routed.runtime.languageService.getImplementationAtPosition(
                routed.companion,
                routed.position,
              );
        }
        const result = _getImplementationAtPosition(fileName, position);
        if (!result || !responseRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `ImplementationLocation` (a `DocumentSpan`) whose `fileName` is a
        // carrier companion maps back to source; an unmappable companion
        // location is dropped (fail closed). `ImplementationLocation` also
        // carries `displayParts`/`kind`, preserved by the in-place remap.
        return protocolSafeMappedSpans(editorRuntime, remapDocumentSpans(remapContext, result));
      };
    }

    // ── rename: companion locations → source (drop unmappable) ─────────────

    const _getRenameInfo = languageService.getRenameInfo?.bind(languageService);
    if (_getRenameInfo) {
      languageService.getRenameInfo = (fileName, position, preferences) => {
        if (editorYieldsCarrierSourceFeatures(fileName)) {
          return {
            canRename: false,
            localizedErrorMessage: "The selected type provider owns this framework source.",
          };
        }
        if (!isVue(fileName)) {
          return _getRenameInfo(fileName, position, preferences);
        }
        const routed = editorCarrierPosition(fileName, position);
        if (routed === null) {
          return { canRename: false, localizedErrorMessage: "The source position is not ready." };
        }
        const result = routed.runtime.languageService.getRenameInfo(
          routed.companion,
          routed.position,
          preferences,
        );
        if (!result.canRename) return result;
        const triggerSpan = editorSourceSpan(routed, fileName, result.triggerSpan);
        if (triggerSpan === null) {
          return {
            canRename: false,
            localizedErrorMessage: "The source position cannot be renamed.",
          };
        }
        const fileToRename = result.fileToRename
          ? (routed.runtime.getStore().ownedSourceFor(result.fileToRename)?.source_uri ??
            result.fileToRename)
          : undefined;
        return {
          ...result,
          triggerSpan,
          ...(fileToRename === undefined ? {} : { fileToRename }),
        };
      };
    }

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
        if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
        if (isVue(fileName)) {
          const routed = editorCarrierPosition(fileName, position);
          if (routed === null) return undefined;
          const queryRenameLocations = routed.runtime.sourceFeatures.findRenameLocations;
          if (queryRenameLocations === undefined) return undefined;
          const pending = [routed.position];
          const queried = new Set<number>();
          const rawLocations: tsModule.RenameLocation[] = [];
          while (pending.length > 0) {
            const linkedPosition = pending.shift()!;
            if (queried.has(linkedPosition)) continue;
            queried.add(linkedPosition);
            const batch =
              queryRenameLocations(
                routed.companion,
                linkedPosition,
                findInStrings,
                findInComments,
                preferences as tsModule.UserPreferences,
              ) ?? [];
            rawLocations.push(...batch);
            for (const linked of linkedEditorCarrierPositionsFromSpans(routed, batch)) {
              if (!queried.has(linked)) pending.push(linked);
            }
          }
          const deduped = dedupeRenameLocations(rawLocations);
          const mapped = routed.runtime.responseRemapEnabled()
            ? remapDocumentSpans(routed.runtime.remapContext, deduped)
            : deduped;
          return dedupeRenameLocations(protocolSafeMappedSpans(routed.runtime, mapped));
        }
        const companionOwner = editorOwnsSourceFeatures
          ? editorOwnerForCompanion(fileName)
          : undefined;
        const result = companionOwner?.sourceFeatures.findRenameLocations
          ? companionOwner.sourceFeatures.findRenameLocations(
              fileName,
              position,
              findInStrings,
              findInComments,
              preferences as tsModule.UserPreferences,
            )
          : _findRenameLocations(
              fileName,
              position,
              findInStrings,
              findInComments,
              preferences as tsModule.UserPreferences,
            );
        const shouldRemap = companionOwner?.responseRemapEnabled() ?? responseRemap;
        if (!result || !shouldRemap) {
          // RAW passthrough on the verter_lsp-internal backend (Rust maps).
          return result;
        }
        // Each `RenameLocation` whose `fileName` is a carrier companion is
        // mapped back to source (path + span); an unmappable companion location
        // (an edit that would land in a generated-only region) is DROPPED so a
        // rename never produces a mis-located edit. Real `.ts` locations pass
        // through.
        const responseRuntime = companionOwner ?? editorRuntime;
        return dedupeRenameLocations(
          protocolSafeMappedSpans(
            responseRuntime,
            remapDocumentSpans(responseRuntime.remapContext, result),
          ),
        );
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
        if (editorYieldsCarrierSourceFeatures(fileName)) return [];
        if (isVue(fileName)) {
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
        if (editorYieldsCarrierSourceFeatures(fileName)) return [];
        if (isVue(fileName)) {
          const routed = editorCarrierSelection(fileName, { pos: start, end });
          if (routed === null || typeof routed.selection === "number") return [];
          return routed.runtime.languageService.getCodeFixesAtPosition(
            routed.companion,
            routed.selection.pos,
            routed.selection.end,
            errorCodes,
            formatOptions,
            preferences,
          );
        }
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
        if (editorYieldsCarrierSourceFeatures(fileName)) return undefined;
        if (isVue(fileName)) {
          const routed = editorCarrierSelection(fileName, positionOrRange);
          if (routed === null) return undefined;
          return routed.runtime.languageService.getEditsForRefactor(
            routed.companion,
            formatOptions,
            routed.selection,
            refactorName,
            actionName,
            preferences,
            interactiveRefactorArguments,
          );
        }
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
      const store = new DiskCarrierStoreReader(
        storeDir,
        projectKey,
        ts.sys.useCaseSensitiveFileNames,
      );
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
    const store = new DiskCarrierStoreReader(
      storeDir,
      projectKey,
      ts.sys.useCaseSensitiveFileNames,
    );
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
    // TypeScript includes plugin externals in `getRootFiles()` after the first
    // graph build. Subtract exactly the prior external set to recover the true
    // configured-root baseline; filtering the live roots directly would make an
    // external retract itself on the next call and oscillate the graph.
    const priorExternals = priorNonEditorExternalsByProject.get(project) ?? new Set<string>();
    const configuredRoots = new Set(
      project
        .getRootFiles()
        .map(canonical)
        .filter((root) => !priorExternals.has(root)),
    );
    // Advertise BOTH identities of every ready carrier:
    // - the SOURCE identity, so host hooks substitute its generated content
    //   without a second document identity;
    // - the COMPANION identity (`{carrier}.tsx`/`.jsx`), so the configured
    //   project KEEPS the companion the Rust host opened. Without a companion
    //   root, the open companion's membership is transient: any project
    //   structure reload (reloadProjects, tsconfig recompute) drops it into a
    //   per-file inferred project, and every projectFileName-targeted query
    //   then fails closed ("Could not find source file"). Companions are
    //   distinct paths from their sources, so the double-identity hazard above
    //   does not apply to them.
    const sources = store
      .readyIdeSources()
      .filter((source) => !configuredRoots.has(canonical(source)));
    const companions = store
      .readyIdeCompanions()
      .filter((companion) => !configuredRoots.has(canonical(companion)));
    const out = [...new Set([...sources, ...companions])];
    priorNonEditorExternalsByProject.set(project, new Set(out.map(canonical)));
    project.projectService.logger.info(
      `[Verter] getExternalFiles(${projectKey}): ${sources.length} ready carrier source(s), ${companions.length} companion(s)`,
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
