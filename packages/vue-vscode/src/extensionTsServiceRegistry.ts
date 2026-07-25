/**
 * Project-bound routing for the extension-hosted TypeScript language service.
 *
 * The extension provider must serve every carrier from the TypeScript the
 * OWNING PROJECT installed — not from whichever install happens to sit at the
 * first workspace folder. A pnpm/npm monorepo with TypeScript under
 * `packages/app/node_modules` is a workspace with a perfectly good TypeScript;
 * anchoring resolution at `workspaceFolders[0]` reports it unavailable and, with
 * the fail-closed contract in force, disables the provider for the whole window.
 *
 * So resolution is PROJECT-BOUND, mirroring the repo's Project-Bound External-TS
 * Contract: the LSP already resolves each file's owning project and declares it
 * on `open`/`updateOpen` as `projectRootPath` (where its TypeScript is
 * installed) plus `projectConfigPath` (the config that defines it). This
 * registry binds `file → project` from that authoritative signal and routes
 * every later query for the file to that project's own service. Different
 * projects may therefore run different TypeScript versions, each with its own
 * config and its own compiler options.
 *
 * Service identity is the CONFIGURED PROJECT — its config file when the LSP
 * declares one, its root otherwise — resolved under the host's filesystem case
 * policy, one `ExtensionTsService` per project, created lazily on first use. The
 * config, not the directory, is the identity because one directory routinely
 * holds several configured projects (`tsconfig.app.json` + `tsconfig.node.json`
 * is the stock Vite layout) whose compiler options differ: keying by directory
 * would serve one of them with the other's rules. The per-file binding map is
 * keyed by the same path identity function, so a project and a file never
 * disagree about what counts as the same path.
 *
 * Fail-closed is likewise per project. A project whose TypeScript does not
 * resolve (or resolves library-less) throws its own cached error and fires its
 * own one-shot notification; sibling projects keep serving. A request whose
 * owning project cannot be determined at all fails closed rather than guessing —
 * the registry never infers a project from a bare path.
 */

import { statSync } from "fs";
import { basename, dirname, join, resolve } from "path";

import { ExtensionTsService, SESSION_SCOPED_RESPONSES } from "./extensionTsService.js";
import type { UnavailableNotifier } from "./extensionTsService.js";

/** Construction seam so specs can drive the registry with a scripted service. */
export interface TsServiceLike {
  handleQuery(command: string, args: Record<string, unknown>): unknown;
}

/**
 * The configured project that owns a file, exactly as the LSP declares it.
 *
 * `configPath` is absent only for the pre-snapshot last-resort binding, where no
 * configured owner is known yet; the service then discovers a config itself
 * rather than being handed a guessed one.
 */
export interface ProjectBinding {
  readonly root: string;
  readonly configPath?: string;
}

export interface ExtensionTsServiceRegistryOptions {
  /** Fired once per project that cannot serve; carries the actionable message. */
  readonly onUnavailable?: UnavailableNotifier;
  /** Service factory; defaults to the real in-process language service. */
  readonly createService?: (
    binding: ProjectBinding,
    onUnavailable?: UnavailableNotifier,
  ) => TsServiceLike;
  /**
   * Whether the filesystem holding a path folds its case; defaults to
   * [`fsFoldsCaseAt`], which probes the real volume. Injectable so a spec can
   * drive BOTH volume policies on one host — a case-sensitive volume is not
   * creatable from a test, and the policy decides whether two differently-cased
   * configs are one project or two.
   */
  readonly fsFoldsCase?: (path: string) => boolean;
}

/** `updateOpen` entry shapes, as the Rust extension provider sends them. */
interface OpenEntry {
  file: string;
  fileContent?: string;
  projectRootPath?: string;
  projectConfigPath?: string;
}
interface ChangedEntry {
  fileName: string;
}

/**
 * Whether the filesystem holding `path` folds case — PROBED from that
 * filesystem, not assumed from the platform.
 *
 * Case folding is a property of the mounted VOLUME, not of the OS: an APFS
 * volume formatted case-sensitive (and every Linux filesystem) keeps
 * `/repo/App/tsconfig.json` and `/repo/app/tsconfig.json` as two distinct
 * projects. Folding by platform alone lowercases both onto ONE service key
 * there, so two projects with two option sets are served as one — the merge
 * direction, which is never safe. Preserving case can only over-separate (one
 * project keyed twice, each serving its own files correctly), so an
 * undeterminable volume preserves case.
 *
 * The probe is the standard one TypeScript's own `sys` uses: ask the filesystem
 * whether a case-swapped spelling of a real directory names the SAME directory
 * (same device + inode). Windows short-circuits: NTFS/ReFS fold, and `ino` is
 * not a reliable identity there.
 *
 * Memoised per directory — a probe is one `statSync` pair per new directory, and
 * the answer is a volume property that cannot change under a live window.
 */
const caseFoldingByDirectory = new Map<string, boolean>();

function swapCase(segment: string): string {
  let swapped = "";
  for (const ch of segment) {
    const lower = ch.toLowerCase();
    swapped += ch === lower ? ch.toUpperCase() : lower;
  }
  return swapped;
}

function directoryFoldsCase(dir: string): boolean {
  const parent = dirname(dir);
  const name = basename(dir);
  // A segment with no case-bearing character cannot answer the question; climb.
  if (swapCase(name) === name) {
    return parent === dir ? false : directoryFoldsCase(parent);
  }
  try {
    const self = statSync(dir);
    const swapped = statSync(join(parent, swapCase(name)));
    return self.dev === swapped.dev && self.ino === swapped.ino;
  } catch {
    // The swapped spelling does not exist ⇒ the volume distinguishes case. An
    // unreadable/nonexistent directory answers the same way: preserve case.
    return false;
  }
}

export function fsFoldsCaseAt(path: string): boolean {
  if (process.platform === "win32") return true;
  // Probe the nearest EXISTING ancestor: the path itself is routinely a file
  // that has not been written yet (a generated companion), and a missing path
  // tells us nothing about its volume.
  let dir = resolve(path);
  for (;;) {
    const parent = dirname(dir);
    if (parent === dir) return false;
    dir = parent;
    const cached = caseFoldingByDirectory.get(dir);
    if (cached !== undefined) return cached;
    let exists = false;
    try {
      exists = statSync(dir).isDirectory();
    } catch {
      exists = false;
    }
    if (!exists) continue;
    const folds = directoryFoldsCase(dir);
    caseFoldingByDirectory.set(dir, folds);
    return folds;
  }
}

/**
 * The filesystem identity of a path, for BOTH the project-identity and the
 * per-file map.
 *
 * One function, not two: keying projects case-folded while keying files raw would
 * make `/ws/App.vue.tsx` and `/ws/app.vue.tsx` two bindings of one file on a
 * case-folding volume — the second one binding through a different code path
 * than the first, or not at all.
 */
function pathIdentity(path: string, foldsCase: (path: string) => boolean): string {
  const abs = resolve(path);
  return foldsCase(abs) ? abs.toLowerCase() : abs;
}

/** The file a command addresses, or `undefined` for a command that names none. */
function commandFile(command: string, args: Record<string, unknown>): string | undefined {
  if (command === "getCombinedCodeFix") {
    const scope = args.scope as { args?: { file?: string } } | undefined;
    return scope?.args?.file;
  }
  return typeof args.file === "string" ? args.file : undefined;
}

export class ExtensionTsServiceRegistry {
  private readonly services = new Map<string, TsServiceLike>();
  /** `file → project identity`, bound from the LSP's declared project. */
  private readonly fileProjects = new Map<string, string>();
  /** Declared bindings, keyed by identity — services need the real paths. */
  private readonly bindings = new Map<string, ProjectBinding>();
  private readonly options: ExtensionTsServiceRegistryOptions;

  constructor(options: ExtensionTsServiceRegistryOptions = {}) {
    this.options = options;
  }

  /** The filesystem identity of `path` under this host's volume case policy. */
  private pathKey(path: string): string {
    return pathIdentity(path, this.options.fsFoldsCase ?? fsFoldsCaseAt);
  }

  /**
   * The ROOT of every live project service, in creation order
   * (diagnostics/tests). Two entries may share a root: two configured projects
   * in one directory are two projects.
   */
  get projectRoots(): string[] {
    return [...this.services.keys()].map((key) => this.bindings.get(key)?.root ?? key);
  }

  handleQuery(command: string, args: Record<string, unknown>): unknown {
    // Constant-response commands carry no file and touch no project state; they
    // must not bind — or construct — a project.
    if (command in SESSION_SCOPED_RESPONSES) return SESSION_SCOPED_RESPONSES[command];

    if (command === "updateOpen") return this.handleUpdateOpen(args);

    const file = commandFile(command, args);
    const declared = declaredBinding(args.projectRootPath, args.projectConfigPath);
    const project = this.bindProject(command, file, declared);
    return this.serviceFor(project).handleQuery(command, args);
  }

  /**
   * `updateOpen` is the one command that can span projects: fan it out so each
   * project's service sees only its own files. The response is `true`, matching
   * the single-service shape the Rust provider expects.
   */
  private handleUpdateOpen(args: Record<string, unknown>): unknown {
    const openFiles = (args.openFiles ?? []) as OpenEntry[];
    const changedFiles = (args.changedFiles ?? []) as ChangedEntry[];
    const closedFiles = (args.closedFiles ?? []) as string[];

    const perRoot = new Map<
      string,
      { openFiles: OpenEntry[]; changedFiles: ChangedEntry[]; closedFiles: string[] }
    >();
    const bucket = (root: string) => {
      let entry = perRoot.get(root);
      if (!entry) {
        entry = { openFiles: [], changedFiles: [], closedFiles: [] };
        perRoot.set(root, entry);
      }
      return entry;
    };

    for (const entry of openFiles) {
      bucket(
        this.bindProject(
          "updateOpen",
          entry.file,
          declaredBinding(entry.projectRootPath, entry.projectConfigPath),
        ),
      ).openFiles.push(entry);
    }
    for (const entry of changedFiles) {
      bucket(this.bindProject("updateOpen", entry.fileName)).changedFiles.push(entry);
    }
    for (const file of closedFiles) {
      // A close for an unbound file is a no-op, not a failure: the project may
      // already have been torn down.
      const known = this.knownProject(file);
      if (known) bucket(known).closedFiles.push(file);
      this.fileProjects.delete(this.pathKey(file));
    }

    for (const [project, payload] of perRoot) {
      this.serviceFor(project).handleQuery(
        "updateOpen",
        payload as unknown as Record<string, unknown>,
      );
    }
    return true;
  }

  /**
   * Resolve — and remember — the owning project for `file`.
   *
   * Two sources, both authoritative, and nothing else: the project the LSP
   * DECLARED on this request, or the one it declared on an earlier request for
   * the same file. There is deliberately no folder-ownership guess: inferring a
   * project from a bare path is exactly what the Project-Bound External-TS
   * Contract forbids, and in the layout that matters — a nested package inside
   * one workspace folder — the guess is wrong in the same way the old producer
   * was wrong. An unbound file fails closed instead.
   *
   * The identity is the declared CONFIG when there is one, the root otherwise:
   * two configured projects rooted at one directory are two services with two
   * option sets, and a re-declaration that changes the config rebinds the file
   * to the other project rather than silently reusing the first one's rules.
   */
  private bindProject(
    command: string,
    file: string | undefined,
    declared?: ProjectBinding,
  ): string {
    if (file !== undefined && declared !== undefined) {
      const key = this.pathKey(declared.configPath ?? declared.root);
      this.fileProjects.set(this.pathKey(file), key);
      this.bindings.set(key, declared);
      return key;
    }
    const known = file === undefined ? undefined : this.knownProject(file);
    if (known) return known;

    throw new Error(
      `Verter: the extension TypeScript provider could not determine which project owns ` +
        `${file ?? `the "${command}" request`}. The provider serves each project from that ` +
        `project's own TypeScript and does not guess a root, so this request fails closed. ` +
        `Open the file inside a workspace folder, or choose a different verter.typeProvider.`,
    );
  }

  private knownProject(file: string): string | undefined {
    const key = this.fileProjects.get(this.pathKey(file));
    return key !== undefined && this.bindings.has(key) ? key : undefined;
  }

  private serviceFor(key: string): TsServiceLike {
    const existing = this.services.get(key);
    if (existing) return existing;
    const binding = this.bindings.get(key) ?? { root: key };
    const create =
      this.options.createService ??
      ((b: ProjectBinding, notify?: UnavailableNotifier) =>
        new ExtensionTsService(b.root, notify, b.configPath));
    const service = create(binding, this.options.onUnavailable);
    this.services.set(key, service);
    return service;
  }
}

/**
 * The project the LSP declared on this request, if it declared one.
 *
 * A config without a root is not a binding: the root is what the project's
 * TypeScript is resolved from, so a declaration missing it is incomplete and the
 * request falls through to the remembered binding (or fails closed).
 */
function declaredBinding(root: unknown, configPath: unknown): ProjectBinding | undefined {
  if (typeof root !== "string") return undefined;
  return { root, configPath: typeof configPath === "string" ? configPath : undefined };
}

/**
 * The production `$/verter/tsQuery` request handler.
 *
 * This is the extension's public LSP boundary for the extension type provider:
 * the LSP's JSON-RPC request lands here, and whatever this function throws is
 * what the Rust side receives as a failed request (a typed provider error). It
 * is a standalone factory so the boundary is exercisable headlessly — the
 * fail-closed contract (throw AND notify once) is a user-visible outcome and
 * must not be tested only through the service's internals.
 */
export function createTsQueryHandler(
  registry: ExtensionTsServiceRegistry,
): (params: { command: string; arguments: Record<string, unknown> }) => unknown {
  return (params) => registry.handleQuery(params.command, params.arguments ?? {});
}
