/**
 * In-process TypeScript language service for the VS Code extension.
 *
 * Adapted from `@verter/ts-service` (Experiment D) — same LanguageServiceHost
 * and request dispatcher, but runs directly in the extension host process
 * instead of over TCP. The Rust LSP sends `$/verter/tsQuery` requests which
 * are dispatched to `handleQuery()`.
 *
 * TypeScript is resolved ONLY from the workspace — from the project's own
 * `node_modules` chain and nothing else — so the language service uses the
 * project's own TS version and its lib files. There is no bundled fallback: a
 * compiler packed into the extension resolves its default libs next to the
 * bundle, where no `lib.*.d.ts` ships, so it would answer from a lib-less
 * service — silently wrong diagnostics. There is no AMBIENT fallback either: the
 * global folders Node ends a bare specifier at (`NODE_PATH`,
 * `$HOME/.node_modules`, …) are a compiler the project did not install, with its
 * own version and its own libs, and are never consulted.
 *
 * One instance serves ONE configured project — its root (where its TypeScript is
 * installed) and its config file (which gives it its compiler options), both
 * declared by the LSP. `ExtensionTsServiceRegistry` owns the per-project routing
 * so a monorepo's packages each use their own TypeScript; this class never
 * chooses a project.
 *
 * Two failures fail closed, both through one cached, actionable error thrown by
 * every later query with the `onUnavailable` notifier firing exactly once:
 *  - no workspace TypeScript resolves at all; and
 *  - one resolves but carries no `lib.*.d.ts` default libraries — a library-less
 *    install answers with the same silently-wrong diagnostics a bundled compiler
 *    would, so it is refused rather than served (this mirrors the LSP's own
 *    tsserver discovery, which rejects library-less candidates:
 *    `crates/verter_type_runtime/src/discovery.rs` `validate_tsserver_candidate`).
 */

import { existsSync, readdirSync, statSync } from "fs";
import { createRequire } from "module";
import { basename, dirname, join, resolve } from "path";
import type * as ts from "typescript";

import { VERTER_TYPES_STUB } from "@verter/typescript-plugin/verter-types-stub";

/** The module every generated IDE carrier imports its type helpers from. */
const VERTER_TYPES_MODULE = "@verter/types";

/** Called once with the actionable message when no workspace TypeScript resolves. */
export type UnavailableNotifier = (message: string) => void;

/**
 * Commands that carry no file and touch no project state — their response is a
 * constant, so the per-project router answers them without binding a project.
 * The service returns the SAME values (single source of truth for the wire
 * shape) once it is initialised.
 */
export const SESSION_SCOPED_RESPONSES: Readonly<Record<string, unknown>> = Object.freeze({
  configure: {},
  compilerOptionsForInferredProjects: true,
  exit: {},
});

/**
 * Config diagnostics that do NOT invalidate the parsed compiler options.
 *
 * `TS18003` ("No inputs were found in config file …") reports on the config's
 * own file LIST. This service's program is the set of files the LSP opens, so an
 * empty input list is expected (a `files: []` solution config, a package whose
 * sources are all carriers) and says nothing about whether the OPTIONS parsed.
 * Every other error-category config diagnostic means the options TypeScript
 * returned are partially salvaged, which is exactly what must not be served.
 */
const NON_FATAL_CONFIG_ERROR_CODES: ReadonlySet<number> = new Set([18003]);

/**
 * A TypeScript install must carry its default libraries.
 *
 * The predicate is the LSP-side rule from
 * `crates/verter_type_runtime/src/discovery.rs` (`validate_tsserver_candidate`),
 * in both of its halves: the NAME shape (`lib.` prefix, `.d.ts` suffix,
 * non-empty middle segment) AND `entry.file_type().is_file()`. The second half
 * is load-bearing — a DIRECTORY named `lib.es2025.d.ts`, or a symlink dangling
 * at one, satisfies a name-only filter while carrying no library at all, and the
 * service would then be admitted library-less. `statSync` follows symlinks, so a
 * symlink TO a real lib file counts (it is a library) and a dangling one throws
 * and does not.
 */
function countDefaultLibs(libDir: string): number {
  return readdirSync(libDir).filter((name) => {
    if (!name.startsWith("lib.") || !name.endsWith(".d.ts")) return false;
    if (name.length <= "lib..d.ts".length) return false;
    try {
      return statSync(join(libDir, name)).isFile();
    } catch {
      // Unreadable or dangling: not a library we can type-check against.
      return false;
    }
  }).length;
}

/**
 * The `node_modules` directories that belong to `root` — its own, then each
 * ancestor's, in Node's lookup order and stopping at the filesystem root.
 *
 * This is Node's own list — `Module._nodeModulePaths`, the directories a bare
 * specifier is searched in — WITHOUT Node's final step. Node ends a bare
 * specifier at GLOBAL_FOLDERS — the `NODE_PATH` entries, `$HOME/.node_modules`,
 * `$HOME/.node_libraries` and `$PREFIX/lib/node` — so `require.resolve` can
 * answer from a TypeScript that has nothing to do with the project: a different
 * major version, a different set of `lib.*.d.ts`, installed by something else
 * entirely. Serving from it is exactly the silently-wrong-diagnostics outcome
 * this provider refuses a bundled compiler for, so the chain is walked
 * explicitly and the global folders are never consulted.
 *
 * A directory already named `node_modules` contributes NOTHING from its own
 * iteration, which is Node's rule for a project that lives inside a dependency
 * tree: no `node_modules/node_modules` is ever probed, and the directory itself
 * still gets searched because the PARENT's iteration already contributes that
 * identical path. Skipping it — rather than letting it contribute itself — is
 * what keeps the list Node's: contributing itself names the same directory
 * twice, which resolves the same but is no longer the list Node walks.
 */
export function workspaceNodeModulesChain(root: string): string[] {
  const chain: string[] = [];
  let dir = resolve(root);
  for (;;) {
    if (basename(dir) !== "node_modules") chain.push(join(dir, "node_modules"));
    const parent = dirname(dir);
    if (parent === dir) return chain;
    dir = parent;
  }
}

/**
 * The directory whose `node_modules` holds the workspace's own TypeScript, or
 * `undefined` when nothing in `searched` installs one.
 *
 * Takes the already-computed chain rather than deriving its own, so the
 * directories REPORTED when this returns `undefined` are by construction the
 * directories that were actually consulted — the two can never drift into a
 * diagnostic that names a search which did not happen.
 *
 * Presence is decided by the package MANIFEST, the same signal every package
 * manager writes and every resolver reads. `existsSync` follows symlinks, so a
 * pnpm/yarn store link (and the junction the fixtures materialize) counts as the
 * install it points at.
 *
 * The answer is the OWNING DIRECTORY rather than the entry path so that the
 * module itself is still loaded by Node from there — `main`, `exports` and every
 * other package-resolution rule stay Node's, and the first lookup path already
 * holds the package, so no later candidate (and no global folder) is reachable.
 */
function workspaceTypeScriptOwner(searched: readonly string[]): string | undefined {
  for (const nodeModules of searched) {
    if (existsSync(join(nodeModules, "typescript", "package.json"))) return dirname(nodeModules);
  }
  return undefined;
}

/**
 * The `MODULE_NOT_FOUND` that resolving through Node would have raised, for the
 * case Node is never asked about: nothing in the workspace's chain installs
 * TypeScript at all.
 *
 * This is the commonest failure there is, and it needs the same underlying
 * detail every other failure here carries as a `cause` — the actionable message
 * alone cannot separate "no TypeScript is installed" from "TypeScript is
 * installed somewhere this provider deliberately does not look", and those need
 * different fixes.
 *
 * It reproduces what Node attached: the `MODULE_NOT_FOUND` code, and the same
 * `Cannot find module 'typescript'` first line. What it does NOT reproduce is
 * Node's `Require stack:` tail — nothing was required, so there is no stack, and
 * that tail named only the anchor file anyway. The directories actually
 * consulted go there instead: strictly the more useful fact, and the one a user
 * with a hoisted or misplaced install needs to see.
 */
function typeScriptNotInstalled(searched: readonly string[]): Error {
  return Object.assign(
    new Error(
      `Cannot find module 'typescript'\n` +
        `Searched (the project's own node_modules chain — Node's lookup list ` +
        `without its global-folder step):\n- ${searched.join("\n- ")}`,
    ),
    { code: "MODULE_NOT_FOUND" },
  );
}

export class ExtensionTsService {
  private ts!: typeof ts;
  private service!: ts.LanguageService;
  private fileSnapshots = new Map<string, ts.IScriptSnapshot>();
  private fileVersions = new Map<string, number>();
  private openFiles = new Set<string>();
  private workspaceRoot: string;
  private configPath: string | undefined;
  private onUnavailable?: UnavailableNotifier;
  private initialized = false;
  private initializationError: Error | undefined;

  /**
   * @param workspaceRoot the owning project's ROOT — where its TypeScript is
   *   installed and what the language service treats as the current directory.
   * @param configPath the config file that DEFINES the owning project, as
   *   declared by the LSP. Optional because the pre-snapshot last-resort binding
   *   knows no configured owner; the service then discovers a config itself.
   */
  constructor(workspaceRoot: string, onUnavailable?: UnavailableNotifier, configPath?: string) {
    this.workspaceRoot = workspaceRoot;
    this.configPath = configPath;
    this.onUnavailable = onUnavailable;
  }

  private ensureInitialized(): void {
    if (this.initializationError) throw this.initializationError;
    if (this.initialized) return;

    // The workspace TypeScript is the ONLY source. `require.resolve` on its own
    // is NOT that guarantee: its last step is the global folders, so an ambient
    // `NODE_PATH` (every pnpm bin shim exports one) or a legacy
    // `$HOME/.node_modules` answers for a project that installed no TypeScript
    // at all. The owning directory is therefore found in the workspace's own
    // `node_modules` chain first, and Node resolves the module from THERE.
    // Nothing installed in the chain and a chain entry that fails to load are one
    // outcome to the user — no usable TypeScript of this project's own — so both
    // fail closed through one message.
    const unresolvable = (cause?: unknown): Error =>
      this.failClosed(
        `Verter: the extension TypeScript provider could not resolve a workspace ` +
          `TypeScript installation from ${this.workspaceRoot}. Install TypeScript in ` +
          `the workspace (npm install -D typescript) or choose a different ` +
          `verter.typeProvider. The provider stays disabled for this project rather ` +
          `than serve wrong diagnostics from a TypeScript the project does not use.`,
        cause,
      );

    let tsModule: typeof ts;
    let entryPath: string;
    const searched = workspaceNodeModulesChain(this.workspaceRoot);
    const owner = workspaceTypeScriptOwner(searched);
    if (owner === undefined) throw unresolvable(typeScriptNotInstalled(searched));
    try {
      const wsRequire = createRequire(join(owner, "package.json"));
      // Resolve the entry FIRST: its directory is the install's lib directory,
      // which must be validated before the compiler is allowed to answer.
      entryPath = wsRequire.resolve("typescript");
      tsModule = wsRequire("typescript") as typeof ts;
    } catch (cause) {
      throw unresolvable(cause);
    }

    // The native-preview TypeScript layout (the `typescript` 7.x package) is a
    // thin launcher: its entry exposes no `createLanguageService`, and its
    // libraries live in a separate platform package, NOT beside the entry. It is
    // a complete, correct install — so it must not be blamed for missing
    // libraries and told to reinstall. Classify it on the resolved API SHAPE
    // (what this service actually needs), never on a version string, and point
    // at the provider that speaks that engine.
    if (typeof (tsModule as Partial<typeof ts>).createLanguageService !== "function") {
      throw this.failClosed(
        `Verter: the extension TypeScript provider cannot drive the TypeScript resolved ` +
          `from ${this.workspaceRoot} (${entryPath}): it exposes no in-process language ` +
          `service. The native TypeScript (7.x / tsgo) family is served by a different ` +
          `engine — set verter.typeProvider to a TSGO-backed provider, or install a ` +
          `TypeScript 5.x in this project. The provider stays disabled for this project ` +
          `rather than serve wrong diagnostics.`,
      );
    }

    // A resolvable-but-library-less install (pruned/vendored/corrupted) is the
    // same defect class as a bundled compiler: it type-checks against no lib, so
    // every global (`string`, `Promise`, DOM) reads as an error. Refuse it.
    const libDir = dirname(entryPath);
    let defaultLibCount: number;
    try {
      defaultLibCount = countDefaultLibs(libDir);
    } catch (cause) {
      throw this.failClosed(
        `Verter: the extension TypeScript provider could not read the library ` +
          `directory of the workspace TypeScript at ${libDir}. Reinstall TypeScript ` +
          `in the workspace (npm install -D typescript) or choose a different ` +
          `verter.typeProvider.`,
        cause,
      );
    }
    if (defaultLibCount === 0) {
      throw this.failClosed(
        `Verter: the extension TypeScript provider refused the workspace TypeScript ` +
          `at ${libDir}: it carries no lib.*.d.ts default libraries, so it would answer ` +
          `from a library-less language service — silently wrong diagnostics. Reinstall ` +
          `TypeScript in the workspace (npm install -D typescript) or choose a different ` +
          `verter.typeProvider.`,
      );
    }

    this.ts = tsModule;

    const compilerOptions: ts.CompilerOptions = this.resolveCompilerOptions();

    // `@verter/types` is where every generated carrier gets its type helpers, so
    // a project that cannot resolve it types every template binding as `any`.
    //
    // The resolution host below is deliberately built on RAW `ts.sys`, not on the
    // virtual-aware `host`: the question "does this project install the package?"
    // must be answered by the disk alone. Answering it through a host that already
    // claims the virtual path exists would make a hoisted or aliased real install
    // lose to Verter's own declarations.
    const verterTypesVirtualPath = join(
      this.workspaceRoot,
      "node_modules",
      "@verter",
      "types",
      "index.d.ts",
    );
    const diskResolutionHost: ts.ModuleResolutionHost = {
      fileExists: this.ts.sys.fileExists,
      readFile: this.ts.sys.readFile,
      directoryExists: this.ts.sys.directoryExists,
      getDirectories: this.ts.sys.getDirectories,
      realpath: this.ts.sys.realpath,
      useCaseSensitiveFileNames: this.ts.sys.useCaseSensitiveFileNames,
    };
    const isVerterTypesVirtualPath = (fileName: string): boolean =>
      this.samePath(fileName, verterTypesVirtualPath);
    // A real file at the virtual path is served verbatim — the fallback only ever
    // fills a hole, it never overwrites what the project installed.
    const readVirtualAware = (fileName: string): string | undefined =>
      isVerterTypesVirtualPath(fileName)
        ? (this.ts.sys.readFile(fileName) ?? VERTER_TYPES_STUB)
        : this.ts.sys.readFile(fileName);

    const host: ts.LanguageServiceHost = {
      getScriptFileNames: () => [...this.openFiles],
      getScriptVersion: (fileName) => String(this.fileVersions.get(fileName) ?? 0),
      getScriptSnapshot: (fileName) => {
        if (this.fileSnapshots.has(fileName)) return this.fileSnapshots.get(fileName)!;
        try {
          const content = readVirtualAware(fileName);
          if (content !== undefined) {
            const snap = this.ts.ScriptSnapshot.fromString(content);
            this.fileSnapshots.set(fileName, snap);
            return snap;
          }
        } catch {
          // Ignore read errors
        }
        return undefined;
      },
      getCurrentDirectory: () => this.workspaceRoot,
      getCompilationSettings: () => compilerOptions,
      getDefaultLibFileName: this.ts.getDefaultLibFilePath,
      fileExists: (fileName) =>
        isVerterTypesVirtualPath(fileName) ? true : this.ts.sys.fileExists(fileName),
      readFile: readVirtualAware,
      readDirectory: this.ts.sys.readDirectory,
      directoryExists: this.ts.sys.directoryExists,
      getDirectories: this.ts.sys.getDirectories,

      // Ordinary resolution runs FIRST for every specifier, including
      // `@verter/types`. Only when the project resolves nothing at all does the
      // virtual declaration file stand in — so an installed copy always wins, and
      // wins as a WHOLE package (a name the install does not export stays
      // unresolved rather than being backfilled from Verter's declarations, which
      // would silently merge two versions of the same package).
      //
      // The resolution MODE is per IMPORT SITE, not per project: under
      // `node16`/`nodenext` an `import` in an ES module resolves a package's
      // `import` condition while the same specifier in a CommonJS file resolves
      // `require`, and a `resolution-mode` attribute overrides both. Dropping the
      // mode makes `resolveModuleName` default to the CommonJS conditions, so a
      // correctly-written ESM import silently picks up a dual-published package's
      // CJS types. `getModeForUsageLocation` is the compiler's own answer for
      // this literal in this file — never re-derived here.
      resolveModuleNameLiterals: (
        moduleLiterals,
        containingFile,
        redirectedReference,
        options,
        containingSourceFile,
      ) =>
        moduleLiterals.map((literal) => {
          const resolved = this.ts.resolveModuleName(
            literal.text,
            containingFile,
            options,
            diskResolutionHost,
            undefined,
            redirectedReference,
            this.ts.getModeForUsageLocation(containingSourceFile, literal, options),
          );
          if (literal.text !== VERTER_TYPES_MODULE || resolved.resolvedModule) {
            return resolved;
          }
          return {
            ...resolved,
            resolvedModule: {
              extension: this.ts.Extension.Dts,
              isExternalLibraryImport: true,
              resolvedFileName: verterTypesVirtualPath,
            },
          };
        }),
    };

    this.service = this.ts.createLanguageService(host, this.ts.createDocumentRegistry());
    this.initialized = true;
  }

  /**
   * Whether two paths name the same file.
   *
   * Separator-insensitive (TypeScript hands back forward slashes even on Windows,
   * while `path.join` produces backslashes there) and case-folded exactly when the
   * host filesystem is, so a case-sensitive filesystem never conflates two files.
   */
  private samePath(left: string, right: string): boolean {
    const normalize = (value: string): string => {
      const slashed = value.replace(/\\/g, "/");
      return this.ts.sys.useCaseSensitiveFileNames ? slashed : slashed.toLowerCase();
    };
    return normalize(left) === normalize(right);
  }

  /**
   * Cache the unavailability, notify once, and return the error to throw. Every
   * later query rethrows this same instance, so the provider stays closed for
   * this project until the window reloads.
   */
  private failClosed(message: string, cause?: unknown): Error {
    this.initializationError = new Error(message, cause === undefined ? undefined : { cause });
    this.onUnavailable?.(message);
    return this.initializationError;
  }

  /**
   * The owning project's compiler options.
   *
   * The DECLARED config wins: a configured project IS its config file, and the
   * LSP knows exactly which one owns this file. Only when nothing was declared
   * (the pre-snapshot last-resort binding) does the service discover one, and
   * then it must look for `jsconfig.json` too — a project configured by
   * `jsconfig.json`, or by any `tsconfig.*.json`, is no less configured, and
   * falling through to invented defaults answers with rules the user never
   * wrote (`checkJs`, `strict`, `jsx`, path aliases…).
   *
   * Whichever config is used, it is parsed against ITS OWN directory. Relative
   * `baseUrl` / `paths` / `rootDir` entries are written relative to the config
   * that declares them, so parsing an ancestor's config against the project root
   * points every alias at a directory that does not exist.
   *
   * A config that EXISTS but cannot be consumed fails closed. Falling through to
   * the inferred defaults there is the worst outcome available: the project has
   * rules, the service could not read them, and the user is answered under
   * unrelated invented ones (`strict`, `checkJs`, `jsx`, path aliases…) with no
   * signal that their configuration was discarded. The defaults are reachable
   * only when NO config exists at all — the inferred-project case, and only for
   * the pre-snapshot last-resort binding, since a configured owner always
   * declares its config.
   */
  private resolveCompilerOptions(): ts.CompilerOptions {
    const configPath =
      this.configPath ??
      this.ts.findConfigFile(this.workspaceRoot, this.ts.sys.fileExists, "tsconfig.json") ??
      this.ts.findConfigFile(this.workspaceRoot, this.ts.sys.fileExists, "jsconfig.json");

    if (configPath) {
      const configFile = this.ts.readConfigFile(configPath, this.ts.sys.readFile);
      if (configFile.error) {
        throw this.failClosed(
          `Verter: the extension TypeScript provider could not read the configuration ` +
            `file that defines this project (${configPath}): ` +
            `${this.ts.flattenDiagnosticMessageText(configFile.error.messageText, " ")} ` +
            `Fix the config, or choose a different verter.typeProvider. The provider stays ` +
            `disabled for this project rather than answer under compiler options the ` +
            `project never declared.`,
        );
      }
      const parsed = this.ts.parseJsonConfigFileContent(
        configFile.config,
        this.ts.sys,
        dirname(configPath),
        undefined,
        configPath,
      );
      // `parseJsonConfigFileContent` reports unknown/invalid options, bad
      // `extends` targets, and malformed values HERE rather than throwing — the
      // returned `options` are what TypeScript salvaged. Ignoring them is the
      // same silent-wrong-rules defect as ignoring a read error, so a genuine
      // config error fails closed too. The exception is the empty-input report:
      // this service's program is the set of files the LSP opens, never the
      // config's own file list, so "no inputs" says nothing about the options.
      const fatal = parsed.errors.filter(
        (diagnostic) =>
          diagnostic.category === this.ts.DiagnosticCategory.Error &&
          !NON_FATAL_CONFIG_ERROR_CODES.has(diagnostic.code),
      );
      if (fatal.length > 0) {
        throw this.failClosed(
          `Verter: the extension TypeScript provider could not parse the configuration ` +
            `file that defines this project (${configPath}): ` +
            `${fatal
              .map((diagnostic) =>
                this.ts.flattenDiagnosticMessageText(diagnostic.messageText, " "),
              )
              .join(" ")} ` +
            `Fix the config, or choose a different verter.typeProvider. The provider stays ` +
            `disabled for this project rather than answer under partially-salvaged ` +
            `compiler options.`,
        );
      }
      return parsed.options;
    }

    // Default options matching ts-service/server.ts
    return {
      target: this.ts.ScriptTarget.ESNext,
      module: this.ts.ModuleKind.ESNext,
      moduleResolution: this.ts.ModuleResolutionKind.Bundler,
      jsx: this.ts.JsxEmit.ReactJSX,
      jsxImportSource: "vue",
      strict: true,
      allowJs: true,
      checkJs: false,
      noEmit: true,
      allowNonTsExtensions: true,
    };
  }

  /**
   * Handle a tsserver-format query. Called by the `$/verter/tsQuery` handler.
   * Returns the response body (same shape as tsserver responses).
   */
  handleQuery(command: string, args: Record<string, unknown>): unknown {
    this.ensureInitialized();

    switch (command) {
      case "configure":
      case "compilerOptionsForInferredProjects":
        return SESSION_SCOPED_RESPONSES[command];

      case "open": {
        const file = args.file as string;
        const content = args.fileContent as string | undefined;
        this.openFiles.add(file);
        if (content !== undefined) {
          this.fileSnapshots.set(file, this.ts.ScriptSnapshot.fromString(content));
          this.fileVersions.set(file, (this.fileVersions.get(file) ?? 0) + 1);
        }
        return {};
      }

      case "updateOpen": {
        const openEntries = (args.openFiles ?? []) as Array<{
          file: string;
          fileContent?: string;
        }>;
        const changedEntries = (args.changedFiles ?? []) as Array<{
          fileName: string;
          textChanges?: Array<{
            start: { line: number; offset: number };
            end: { line: number; offset: number };
            newText: string;
          }>;
        }>;
        const closedEntries = (args.closedFiles ?? []) as string[];

        for (const entry of openEntries) {
          this.openFiles.add(entry.file);
          if (entry.fileContent !== undefined) {
            this.fileSnapshots.set(
              entry.file,
              this.ts.ScriptSnapshot.fromString(entry.fileContent),
            );
            this.fileVersions.set(entry.file, (this.fileVersions.get(entry.file) ?? 0) + 1);
          }
        }

        for (const entry of changedEntries) {
          const currentSnap = this.fileSnapshots.get(entry.fileName);
          if (currentSnap && entry.textChanges?.length) {
            let text = currentSnap.getText(0, currentSnap.getLength());
            const changes = [...entry.textChanges].sort(
              (a, b) => b.start.line - a.start.line || b.start.offset - a.start.offset,
            );
            for (const change of changes) {
              const startOffset = this.positionToOffset(
                text,
                change.start.line,
                change.start.offset,
              );
              const endOffset = this.positionToOffset(text, change.end.line, change.end.offset);
              text = text.slice(0, startOffset) + change.newText + text.slice(endOffset);
            }
            this.fileSnapshots.set(entry.fileName, this.ts.ScriptSnapshot.fromString(text));
            this.fileVersions.set(entry.fileName, (this.fileVersions.get(entry.fileName) ?? 0) + 1);
          }
        }

        for (const file of closedEntries) {
          this.openFiles.delete(file);
        }

        return true;
      }

      case "close": {
        const file = args.file as string;
        this.openFiles.delete(file);
        return {};
      }

      case "quickinfo": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const info = this.service.getQuickInfoAtPosition(file, offset);
        if (!info) return undefined;
        const display = this.ts.displayPartsToString(info.displayParts);
        const docs = this.ts.displayPartsToString(info.documentation);
        const start = this.offsetToPosition(text, info.textSpan.start);
        const end = this.offsetToPosition(text, info.textSpan.start + info.textSpan.length);
        return {
          kind: info.kind,
          kindModifiers: info.kindModifiers,
          start: { line: start.line, offset: start.offset },
          end: { line: end.line, offset: end.offset },
          displayString: display,
          documentation: docs,
          tags: info.tags?.map((t) => ({
            name: t.name,
            text: t.text ? t.text.map((p) => p.text).join("") : undefined,
          })),
        };
      }

      case "completionInfo": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const completions = this.service.getCompletionsAtPosition(file, offset, {
          includeCompletionsForModuleExports: true,
          includeCompletionsWithInsertText: true,
        });
        if (!completions) return undefined;
        return {
          isGlobalCompletion: completions.isGlobalCompletion,
          isMemberCompletion: completions.isMemberCompletion,
          entries: completions.entries.map((e) => ({
            name: e.name,
            kind: e.kind,
            kindModifiers: e.kindModifiers,
            sortText: e.sortText,
            insertText: e.insertText,
            replacementSpan: e.replacementSpan
              ? this.spanToRange(text, e.replacementSpan)
              : undefined,
            // Auto-import resolve key: a module-export entry carries `source`
            // (+ the optional opaque `data` blob), which `getCompletionEntryDetails`
            // keys the auto-import `codeActions` lookup on. Forwarding them lets
            // the provider re-issue `completionEntryDetails` for the selected
            // entry — without them the extension provider could never resolve an
            // auto-import. `hasAction` is NOT forwarded: it is purely an output
            // hint (not an input to the details lookup), and the auto-import
            // resolve contract is `source`/`data` only — an auto-import entry
            // always carries `source`. The other `hasAction:true` shapes
            // (class-member snippet completions, missing-comma insertion,
            // type-only-alias wrappers) are a DIFFERENT action class this
            // resolve path deliberately does not route as imports. See
            // `crates/verter_type_runtime/src/protocol.rs` (`is_actionable`) and
            // `docs/arch/provider-completion-resolve-design.md`.
            source: e.source,
            data: e.data,
            labelDetails: e.labelDetails,
            sourceDisplay: e.sourceDisplay
              ? this.ts.displayPartsToString(e.sourceDisplay)
              : undefined,
          })),
        };
      }

      case "completionEntryDetails": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        // The selected entries to resolve, each `{ name, source?, data? }` —
        // `source`/`data` route an external-module entry to the right symbol.
        const entryNames = (args.entryNames as unknown[]) ?? [];
        const details = entryNames.map((raw) => {
          const entry = raw as { name?: string; source?: string; data?: unknown };
          const name = entry.name ?? "";
          // `formatOptions` MUST be provided: when resolving an auto-import
          // (external-module) entry, TypeScript builds the import-insertion
          // `codeActions` through its formatter, which dereferences the format
          // settings. Passing `undefined` crashes the import code-action builder
          // (`Cannot read properties of undefined (reading 'options')`), so the
          // extension provider could never resolve an auto-import edit. Default
          // format settings are sufficient — the inserted import is normalized
          // by the shared tsserver-family resolve mapper downstream.
          const detail = this.service.getCompletionEntryDetails(
            file,
            offset,
            name,
            this.ts.getDefaultFormatCodeSettings("\n"),
            entry.source,
            undefined,
            entry.data as ts.CompletionEntryData | undefined,
          );
          if (!detail) return { name };
          return {
            name: detail.name,
            kind: detail.kind,
            kindModifiers: detail.kindModifiers,
            displayParts: detail.displayParts?.map((p) => ({ text: p.text, kind: p.kind })),
            documentation: detail.documentation?.map((p) => ({ text: p.text, kind: p.kind })),
            tags: detail.tags?.map((t) => ({
              name: t.name,
              text: t.text ? t.text.map((p) => p.text).join("") : undefined,
            })),
            // The auto-import edit set: each code action's `changes` are tsserver
            // `{ fileName, textChanges }` with 1-based line/offset positions, the
            // shape the shared tsserver-family resolve mapper consumes.
            codeActions: detail.codeActions?.map((action) => ({
              description: action.description,
              changes: action.changes.map((change) => ({
                fileName: change.fileName,
                textChanges: change.textChanges.map((tc) => {
                  const changeText = this.getFileText(change.fileName);
                  return {
                    start: this.offsetToPosition(changeText, tc.span.start),
                    end: this.offsetToPosition(changeText, tc.span.start + tc.span.length),
                    newText: tc.newText,
                  };
                }),
              })),
            })),
          };
        });
        return details;
      }

      case "definition":
      case "typeDefinition": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const fn_ =
          command === "definition"
            ? this.service.getDefinitionAtPosition
            : this.service.getTypeDefinitionAtPosition;
        const defs = fn_.call(this.service, file, offset);
        return (defs ?? []).map((d) => ({
          file: d.fileName,
          start: this.offsetToPosition(this.getFileText(d.fileName), d.textSpan.start),
          end: this.offsetToPosition(
            this.getFileText(d.fileName),
            d.textSpan.start + d.textSpan.length,
          ),
        }));
      }

      case "references": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const refs = this.service.getReferencesAtPosition(file, offset);
        return {
          refs: (refs ?? []).map((r) => ({
            file: r.fileName,
            start: this.offsetToPosition(this.getFileText(r.fileName), r.textSpan.start),
            end: this.offsetToPosition(
              this.getFileText(r.fileName),
              r.textSpan.start + r.textSpan.length,
            ),
            isDefinition: (r as unknown as Record<string, unknown>).isDefinition ?? false,
            isWriteAccess: r.isWriteAccess,
          })),
        };
      }

      case "rename": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const locations = this.service.findRenameLocations(file, offset, false, false);
        const locArray: ts.RenameLocation[] = locations ? [...locations] : [];
        return {
          info: {
            canRename: !!locations,
            localizedErrorMessage: locations ? "" : "Cannot rename",
          },
          locs: this.groupBy(locArray, (r) => r.fileName).map(([locFile, spans]) => ({
            file: locFile,
            locs: spans.map((s) => ({
              start: this.offsetToPosition(this.getFileText(locFile), s.textSpan.start),
              end: this.offsetToPosition(
                this.getFileText(locFile),
                s.textSpan.start + s.textSpan.length,
              ),
            })),
          })),
        };
      }

      case "signatureHelp": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const help = this.service.getSignatureHelpItems(file, offset, {});
        if (!help) return undefined;
        return {
          items: help.items.map((item) => ({
            isVariadic: item.isVariadic,
            prefixDisplayParts: item.prefixDisplayParts,
            suffixDisplayParts: item.suffixDisplayParts,
            separatorDisplayParts: item.separatorDisplayParts,
            parameters: item.parameters.map((p) => ({
              name: p.name,
              documentation: p.documentation,
              displayParts: p.displayParts,
              isOptional: p.isOptional,
            })),
            documentation: item.documentation,
          })),
          selectedItemIndex: help.selectedItemIndex,
          argumentIndex: help.argumentIndex,
          argumentCount: help.argumentCount,
        };
      }

      case "semanticDiagnosticsSync": {
        const file = args.file as string;
        const text = this.getFileText(file);
        return this.service.getSemanticDiagnostics(file).map((d) => this.mapDiagnostic(text, d));
      }

      // Parse-error diagnostics. The native TS experience merges these with the
      // semantic set; a semantic-only path drops them (tsserver-family parity).
      case "syntacticDiagnosticsSync": {
        const file = args.file as string;
        const text = this.getFileText(file);
        return this.service.getSyntacticDiagnostics(file).map((d) => this.mapDiagnostic(text, d));
      }

      // Suggestion diagnostics (unused-symbol / hint findings) — also part of the
      // native merged set, dropped by a semantic-only path (tsserver-family parity).
      case "suggestionDiagnosticsSync": {
        const file = args.file as string;
        const text = this.getFileText(file);
        return this.service.getSuggestionDiagnostics(file).map((d) => this.mapDiagnostic(text, d));
      }

      case "getCodeFixes": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const startPos = this.positionToOffset(
          text,
          args.startLine as number,
          args.startOffset as number,
        );
        const endPos = this.positionToOffset(
          text,
          args.endLine as number,
          args.endOffset as number,
        );
        const errorCodes = (args.errorCodes ?? []) as number[];

        const fixes = this.service.getCodeFixesAtPosition(
          file,
          startPos,
          endPos,
          errorCodes,
          {},
          {},
        );

        return fixes.map((fix) => ({
          description: fix.description,
          // Pass through the typed fix-all identity so the Rust extension provider
          // can follow each distinct `fixId` with a `getCombinedCodeFix` request and
          // surface the "Delete all unused declarations" companion. Both are
          // OPTIONAL on `ts.CodeFixAction` (a non-combinable fix carries neither);
          // forwarding `undefined` is harmless (the Rust side reads them as None).
          fixId: fix.fixId,
          fixAllDescription: fix.fixAllDescription,
          changes: fix.changes.map((change) => ({
            fileName: change.fileName,
            textChanges: change.textChanges.map((tc) => ({
              start: this.offsetToPosition(this.getFileText(change.fileName), tc.span.start),
              end: this.offsetToPosition(
                this.getFileText(change.fileName),
                tc.span.start + tc.span.length,
              ),
              newText: tc.newText,
            })),
          })),
        }));
      }

      // The "fix all" companion for a combinable `fixId` (e.g. "Delete all unused
      // declarations"). The Rust extension provider sends the shared
      // `combined_code_fix_args` scope shape:
      //   { scope: { type: "file", args: { file } }, fixId }
      // (see `verter_type_runtime::tsserver::ipc::combined_code_fix_args`). The
      // response mirrors the `getCodeFixes` `changes` shape exactly — 1-based
      // `{ line, offset }` positions — so the shared
      // `parse_tsserver_combined_code_fix` reads it without a second mapping.
      case "getCombinedCodeFix": {
        const scope = args.scope as { type: "file"; args: { file: string } };
        const file = scope.args.file;
        const fixId = args.fixId as {} | string;

        const combined = this.service.getCombinedCodeFix(
          { type: "file", fileName: file },
          fixId,
          {},
          {},
        );

        return {
          changes: combined.changes.map((change) => ({
            fileName: change.fileName,
            textChanges: change.textChanges.map((tc) => ({
              start: this.offsetToPosition(this.getFileText(change.fileName), tc.span.start),
              end: this.offsetToPosition(
                this.getFileText(change.fileName),
                tc.span.start + tc.span.length,
              ),
              newText: tc.newText,
            })),
          })),
        };
      }

      case "encodedSemanticClassifications-full": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const startArg = args.start as { line: number; offset: number } | undefined;
        const endArg = args.end as { line: number; offset: number } | undefined;

        const startPos = startArg ? this.positionToOffset(text, startArg.line, startArg.offset) : 0;
        const endPos = endArg
          ? this.positionToOffset(text, endArg.line, endArg.offset)
          : text.length;

        const result = this.service.getEncodedSemanticClassifications(
          file,
          { start: startPos, length: endPos - startPos },
          "2020" as ts.SemanticClassificationFormat,
        );

        return { spans: result.spans };
      }

      case "documentHighlights": {
        const file = args.file as string;
        const text = this.getFileText(file);
        const offset = this.positionToOffset(text, args.line as number, args.offset as number);
        const filesToSearch = (args.filesToSearch ?? [file]) as string[];
        const highlights = this.service.getDocumentHighlights(file, offset, filesToSearch);

        if (!highlights) return [];

        return highlights.map((group) => ({
          file: group.fileName,
          highlightSpans: group.highlightSpans.map((span) => ({
            start: this.offsetToPosition(this.getFileText(group.fileName), span.textSpan.start),
            end: this.offsetToPosition(
              this.getFileText(group.fileName),
              span.textSpan.start + span.textSpan.length,
            ),
            kind:
              span.kind === this.ts.HighlightSpanKind.writtenReference
                ? "writtenReference"
                : "reference",
          })),
        }));
      }

      case "provideInlayHints": {
        const file = args.file as string;
        const text = this.getFileText(file);

        const startPos = (args.start as number) ?? 0;
        const length = (args.length as number) ?? text.length;

        const hints = this.service.provideInlayHints(file, { start: startPos, length }, undefined);

        return hints.map((hint) => ({
          text: hint.text,
          position: this.offsetToPosition(text, hint.position),
          kind: hint.kind === this.ts.InlayHintKind.Type ? "Type" : "Parameter",
          whitespaceBefore: hint.whitespaceBefore,
          whitespaceAfter: hint.whitespaceAfter,
        }));
      }

      case "exit":
        return SESSION_SCOPED_RESPONSES[command];

      default:
        throw new Error(`Unknown command: ${command}`);
    }
  }

  // ── Helpers ─────────────────────────────────────────────────

  /**
   * Map a TS `Diagnostic` (from any of the semantic/syntactic/suggestion passes)
   * onto the wire diagnostic shape the Rust extension provider parses. All three
   * passes share one mapping so they merge into a uniform set on the Rust side.
   */
  private mapDiagnostic(text: string, d: ts.Diagnostic) {
    return {
      start: d.start !== undefined ? this.offsetToPosition(text, d.start) : undefined,
      end:
        d.start !== undefined && d.length !== undefined
          ? this.offsetToPosition(text, d.start + d.length)
          : undefined,
      text: this.ts.flattenDiagnosticMessageText(d.messageText, "\n"),
      code: d.code,
      category:
        d.category === this.ts.DiagnosticCategory.Error
          ? "error"
          : d.category === this.ts.DiagnosticCategory.Warning
            ? "warning"
            : "suggestion",
    };
  }

  private getFileText(file: string): string {
    const snap = this.fileSnapshots.get(file);
    if (snap) return snap.getText(0, snap.getLength());
    try {
      return this.ts.sys.readFile(file) ?? "";
    } catch {
      return "";
    }
  }

  /** 1-based line/offset to 0-based byte offset */
  private positionToOffset(text: string, line: number, offset: number): number {
    let currentLine = 1;
    let i = 0;
    while (currentLine < line && i < text.length) {
      if (text[i] === "\n") currentLine++;
      i++;
    }
    return i + offset - 1;
  }

  /** 0-based byte offset to 1-based line/offset */
  private offsetToPosition(text: string, offset: number): { line: number; offset: number } {
    let line = 1;
    let lastLineStart = 0;
    for (let i = 0; i < offset && i < text.length; i++) {
      if (text[i] === "\n") {
        line++;
        lastLineStart = i + 1;
      }
    }
    return { line, offset: offset - lastLineStart + 1 };
  }

  private spanToRange(text: string, span: ts.TextSpan) {
    return {
      start: this.offsetToPosition(text, span.start),
      end: this.offsetToPosition(text, span.start + span.length),
    };
  }

  private groupBy<T>(items: T[], key: (item: T) => string): [string, T[]][] {
    const map = new Map<string, T[]>();
    for (const item of items) {
      const k = key(item);
      const arr = map.get(k) ?? [];
      arr.push(item);
      map.set(k, arr);
    }
    return [...map.entries()];
  }
}
