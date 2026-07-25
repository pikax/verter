// Public-boundary regression guard for the extension type provider's
// PROJECT-BOUND TypeScript resolution and its fail-closed contract.
//
// Two user-visible outcomes are locked here:
//
//  1. A project is served from ITS OWN TypeScript. The LSP resolves each file's
//     owning project and sends that root as `projectRootPath`; a monorepo whose
//     TypeScript lives under `packages/app/node_modules`, or a multi-root
//     workspace whose TypeScript lives in a folder other than the first, must be
//     served — not told no workspace TypeScript exists. (Anchoring resolution at
//     `workspaceFolders[0]` reported those workspaces unavailable, and under the
//     fail-closed contract that turns into a hard error for the whole window.)
//
//  2. Fail-closed is PER PROJECT. A project that cannot serve throws its own
//     cached, actionable error and fires its own one-shot notification, while
//     its siblings keep answering.
//
// The tests drive `createTsQueryHandler` — the actual function registered on
// `$/verter/tsQuery` — so the assertions are on the LSP request boundary: what
// the handler returns, what it throws (the Rust side's typed provider error),
// and what reaches the error-notification sink (`window.showErrorMessage` in
// production).
//
// SCOPE — this file is the CONSUMER half. It proves the registry serves a nested
// package from that package's own TypeScript GIVEN the owning project root on
// `open`. That the LSP actually SENDS the owning project root (rather than the
// workspace folder, which is what made a nested package look TypeScript-less) is
// the PRODUCER half, proved against the real `ExtensionTypeProvider` in
// `crates/verter_lsp/src/extension_provider_tests.rs`
// (`open_stamps_the_owning_package_root_not_the_workspace_folder`). Neither half
// is evidence on its own: a registry test that supplies the root itself cannot
// discriminate a producer that supplies the wrong one.

import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { materializeWorkspaceTypeScript } from "./extensionTsService.testUtils.js";
import {
  ExtensionTsServiceRegistry,
  createTsQueryHandler,
  fsFoldsCaseAt,
} from "./extensionTsServiceRegistry.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

/** A disposable workspace/package root with a tsconfig, optionally with TypeScript. */
function makeProject(dir: string, withTypeScript: boolean): string {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "package.json"), JSON.stringify({ name: "fixture", private: true }));
  writeFileSync(
    join(dir, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: { module: "esnext", target: "esnext", moduleResolution: "bundler" },
      include: ["*.ts"],
    }),
  );
  if (withTypeScript) materializeWorkspaceTypeScript(dir);
  return dir;
}

function makeTempRoot(name: string): string {
  const root = mkdtempSync(join(tmpdir(), name));
  tmps.push(root);
  return root;
}

const SOURCE = "export const answer: number = 42;\n";

/** Write the fixture module and return its path. `answer` is at 1-based offset 14. */
function writeEntry(projectDir: string): string {
  const filePath = join(projectDir, "entry.ts");
  writeFileSync(filePath, SOURCE);
  return filePath;
}

interface QuickInfo {
  kind: string;
  displayString: string;
}

describe("extension TS provider — project-bound resolution", () => {
  it("serves a nested package from the TypeScript installed in that package", () => {
    // A pnpm-style monorepo: the workspace root has NO TypeScript; the package
    // that owns the file does. This is the layout that was falsely reported
    // unavailable when resolution anchored at the first workspace folder.
    const wsRoot = makeTempRoot("ext-ts-monorepo-");
    makeProject(wsRoot, false);
    const pkgRoot = makeProject(join(wsRoot, "packages", "app"), true);
    const filePath = writeEntry(pkgRoot);

    const unavailable: string[] = [];
    const handler = createTsQueryHandler(
      new ExtensionTsServiceRegistry({
        onUnavailable: (message) => unavailable.push(message),
      }),
    );

    // The LSP names the owning project on `open` — that binding, not the first
    // workspace folder, decides which TypeScript answers.
    handler({
      command: "open",
      arguments: {
        file: filePath,
        fileContent: SOURCE,
        scriptKindName: "TS",
        projectRootPath: pkgRoot,
      },
    });

    // Later queries carry no `projectRootPath`; the binding must persist.
    const info = handler({
      command: "quickinfo",
      arguments: { file: filePath, line: 1, offset: 14 },
    }) as QuickInfo;
    expect(info.kind).toBe("const");
    expect(info.displayString).toContain("answer: number");
    expect(unavailable).toEqual([]);
  });

  it("serves a multi-root workspace whose TypeScript lives outside the first folder", () => {
    // Two sibling workspace folders. The FIRST has no TypeScript; the second
    // does, and owns the file.
    const parent = makeTempRoot("ext-ts-multiroot-");
    makeProject(join(parent, "docs"), false);
    const secondFolder = makeProject(join(parent, "app"), true);
    const filePath = writeEntry(secondFolder);

    const unavailable: string[] = [];
    const handler = createTsQueryHandler(
      new ExtensionTsServiceRegistry({
        onUnavailable: (message) => unavailable.push(message),
      }),
    );

    handler({
      command: "open",
      arguments: {
        file: filePath,
        fileContent: SOURCE,
        scriptKindName: "TS",
        projectRootPath: secondFolder,
      },
    });

    const info = handler({
      command: "quickinfo",
      arguments: { file: filePath, line: 1, offset: 14 },
    }) as QuickInfo;
    expect(info.displayString).toContain("answer: number");
    expect(unavailable).toEqual([]);
  });

  it("keeps one project serving when a sibling project has no TypeScript", () => {
    const parent = makeTempRoot("ext-ts-mixed-");
    const good = makeProject(join(parent, "with-ts"), true);
    const bad = makeProject(join(parent, "without-ts"), false);
    const goodFile = writeEntry(good);
    const badFile = writeEntry(bad);

    const unavailable: string[] = [];
    const handler = createTsQueryHandler(
      new ExtensionTsServiceRegistry({
        onUnavailable: (message) => unavailable.push(message),
      }),
    );

    handler({
      command: "open",
      arguments: {
        file: goodFile,
        fileContent: SOURCE,
        scriptKindName: "TS",
        projectRootPath: good,
      },
    });

    // The failing project fails ALONE, through the real request boundary.
    let thrown: unknown;
    try {
      handler({
        command: "open",
        arguments: {
          file: badFile,
          fileContent: SOURCE,
          scriptKindName: "TS",
          projectRootPath: bad,
        },
      });
    } catch (error) {
      thrown = error;
    }
    expect(thrown, "a project without TypeScript must fail closed").toBeInstanceOf(Error);
    const message = (thrown as Error).message;
    expect(message).toMatch(/could not resolve a workspace TypeScript/);
    // The message names the OWNING project, not the first workspace folder.
    expect(message).toContain(bad);
    expect(message).not.toContain(good);
    // Exactly one user-facing notification, for that project only.
    expect(unavailable).toEqual([message]);

    // The healthy sibling is untouched.
    const info = handler({
      command: "quickinfo",
      arguments: { file: goodFile, line: 1, offset: 14 },
    }) as QuickInfo;
    expect(info.displayString).toContain("answer: number");

    // The failure is sticky per project and does not re-notify.
    let second: unknown;
    try {
      handler({ command: "semanticDiagnosticsSync", arguments: { file: badFile } });
    } catch (error) {
      second = error;
    }
    expect(second).toBe(thrown);
    expect(unavailable).toEqual([message]);
  });

  it("routes a cross-project updateOpen to each file's own project", () => {
    const parent = makeTempRoot("ext-ts-fanout-");
    const a = makeProject(join(parent, "a"), true);
    const b = makeProject(join(parent, "b"), true);
    const fileA = writeEntry(a);
    const fileB = writeEntry(b);

    const registry = new ExtensionTsServiceRegistry({});
    const handler = createTsQueryHandler(registry);

    expect(
      handler({
        command: "updateOpen",
        arguments: {
          openFiles: [
            { file: fileA, fileContent: SOURCE, projectRootPath: a },
            { file: fileB, fileContent: SOURCE, projectRootPath: b },
          ],
        },
      }),
    ).toBe(true);

    // Two distinct project services exist, and each answers for its own file.
    expect(registry.projectRoots).toHaveLength(2);
    for (const file of [fileA, fileB]) {
      const info = handler({
        command: "quickinfo",
        arguments: { file, line: 1, offset: 14 },
      }) as QuickInfo;
      expect(info.displayString).toContain("answer: number");
    }
  });

  it("answers project-independent commands without binding or building a project", () => {
    const registry = new ExtensionTsServiceRegistry({});
    const handler = createTsQueryHandler(registry);

    expect(handler({ command: "configure", arguments: {} })).toEqual({});
    expect(
      handler({ command: "compilerOptionsForInferredProjects", arguments: { options: {} } }),
    ).toBe(true);
    expect(registry.projectRoots).toEqual([]);
  });

  it("does not fall back to folder ownership for a file the LSP never bound", () => {
    // The registry has NO owner-inference seam: a project is either declared by
    // the LSP or previously bound for that file. A folder-ownership guess would
    // name the workspace folder for a nested package — the exact wrong root the
    // declared binding exists to replace — so an unbound file fails closed even
    // when a perfectly good project sits above it on disk.
    const parent = makeTempRoot("ext-ts-nofallback-");
    const pkgRoot = makeProject(join(parent, "packages", "app"), true);
    const filePath = writeEntry(pkgRoot);

    const registry = new ExtensionTsServiceRegistry({});
    const handler = createTsQueryHandler(registry);

    expect(() =>
      handler({ command: "quickinfo", arguments: { file: filePath, line: 1, offset: 14 } }),
    ).toThrow(/could not determine which project owns/);
    // Nothing was constructed on the way to failing.
    expect(registry.projectRoots).toEqual([]);
  });

  it("keeps two differently-cased configs as two projects on a case-sensitive volume", () => {
    // Case folding is a VOLUME property, not an OS one: APFS formatted
    // case-sensitive (and every Linux filesystem) keeps `/repo/App/tsconfig.json`
    // and `/repo/app/tsconfig.json` as two distinct configured projects with two
    // option sets. Folding by `process.platform` lowercases both onto ONE
    // service key there and serves one project with the other's rules — the
    // MERGE direction, which is never recoverable.
    //
    // A case-sensitive volume cannot be created from a test on a macOS host, so
    // the volume policy is injected; the default probe is covered below against
    // the real filesystem.
    const created: Array<string | undefined> = [];
    const handler = createTsQueryHandler(
      new ExtensionTsServiceRegistry({
        fsFoldsCase: () => false,
        createService: (binding) => {
          created.push(binding.configPath);
          return { handleQuery: () => ({ config: binding.configPath }) };
        },
      }),
    );

    const upper = join(tmpdir(), "case-volume", "App");
    const lower = join(tmpdir(), "case-volume", "app");
    handler({
      command: "open",
      arguments: {
        file: join(upper, "entry.ts"),
        fileContent: SOURCE,
        projectRootPath: upper,
        projectConfigPath: join(upper, "tsconfig.json"),
      },
    });
    handler({
      command: "open",
      arguments: {
        file: join(lower, "entry.ts"),
        fileContent: SOURCE,
        projectRootPath: lower,
        projectConfigPath: join(lower, "tsconfig.json"),
      },
    });

    expect(created).toEqual([join(upper, "tsconfig.json"), join(lower, "tsconfig.json")]);
    // …and each file keeps answering from ITS OWN project.
    expect(handler({ command: "quickinfo", arguments: { file: join(upper, "entry.ts") } })).toEqual(
      { config: join(upper, "tsconfig.json") },
    );
    expect(handler({ command: "quickinfo", arguments: { file: join(lower, "entry.ts") } })).toEqual(
      { config: join(lower, "tsconfig.json") },
    );
  });

  it("folds two spellings of one config onto one project on a case-folding volume", () => {
    // The mirror of the case above: where the volume genuinely folds, two
    // spellings are ONE project and must not be served twice.
    const created: Array<string | undefined> = [];
    const handler = createTsQueryHandler(
      new ExtensionTsServiceRegistry({
        fsFoldsCase: () => true,
        createService: (binding) => {
          created.push(binding.configPath);
          return { handleQuery: () => ({ config: binding.configPath }) };
        },
      }),
    );

    const root = join(tmpdir(), "folding-volume", "App");
    handler({
      command: "open",
      arguments: {
        file: join(root, "entry.ts"),
        fileContent: SOURCE,
        projectRootPath: root,
        projectConfigPath: join(root, "tsconfig.json"),
      },
    });
    handler({
      command: "open",
      arguments: {
        file: join(tmpdir(), "folding-volume", "app", "entry.ts"),
        fileContent: SOURCE,
        projectRootPath: join(tmpdir(), "folding-volume", "app"),
        projectConfigPath: join(tmpdir(), "folding-volume", "app", "tsconfig.json"),
      },
    });

    expect(created).toEqual([join(root, "tsconfig.json")]);
  });

  it("derives the case policy from the filesystem, not from the platform name", () => {
    // Ground truth, measured on the volume the test itself runs on: a directory
    // created as `CaseProbe` is reachable as `caseprobe` exactly when the volume
    // folds. The default probe must agree with that measurement — a policy read
    // off `process.platform` agrees only by coincidence, and stops agreeing on a
    // case-sensitive APFS volume, where it is wrong in the merge direction.
    const root = makeTempRoot("ext-ts-fold-probe-");
    mkdirSync(join(root, "CaseProbe"));
    const volumeFolds = existsSync(join(root, "caseprobe"));

    expect(fsFoldsCaseAt(join(root, "CaseProbe", "not-yet-written.vue.tsx"))).toBe(volumeFolds);
  });

  it("propagates the refusal of the project a file was just rebound to", () => {
    // The registry commits `file → project` from the declaration on THIS
    // request, so a re-declaration (an ownership authority landing after the
    // file was first opened) rebinds the file. If the newly declared project
    // cannot serve, that refusal must reach the caller — the file must NOT keep
    // answering from the project it used to be bound to. A follow-up
    // `completionEntryDetails` that silently returned the old project's answer
    // is a cross-project stale result, which is the outcome the whole
    // project-bound contract exists to prevent.
    const parent = makeTempRoot("ext-ts-rebind-refusal-");
    const serving = makeProject(join(parent, "serving"), true);
    const refusing = makeProject(join(parent, "refusing"), false);
    const filePath = writeEntry(serving);

    const unavailable: string[] = [];
    const handler = createTsQueryHandler(
      new ExtensionTsServiceRegistry({
        onUnavailable: (message) => unavailable.push(message),
      }),
    );

    handler({
      command: "open",
      arguments: {
        file: filePath,
        fileContent: SOURCE,
        scriptKindName: "TS",
        projectRootPath: serving,
        projectConfigPath: join(serving, "tsconfig.json"),
      },
    });
    const before = handler({
      command: "quickinfo",
      arguments: { file: filePath, line: 1, offset: 14 },
    }) as QuickInfo;
    expect(before.displayString).toContain("answer: number");

    // The rebind: same file, newly declared owner, no TypeScript of its own.
    expect(() =>
      handler({
        command: "open",
        arguments: {
          file: filePath,
          fileContent: SOURCE,
          scriptKindName: "TS",
          projectRootPath: refusing,
          projectConfigPath: join(refusing, "tsconfig.json"),
        },
      }),
    ).toThrow(/could not resolve a workspace TypeScript installation/);

    // Every later query for the file routes to the refused project and throws —
    // it never falls back to the project that used to own it.
    expect(() =>
      handler({
        command: "completionEntryDetails",
        arguments: { file: filePath, line: 1, offset: 14, entryNames: ["answer"] },
      }),
    ).toThrow(/could not resolve a workspace TypeScript installation/);
    expect(() =>
      handler({ command: "quickinfo", arguments: { file: filePath, line: 1, offset: 14 } }),
    ).toThrow(/could not resolve a workspace TypeScript installation/);
    expect(unavailable).toHaveLength(1);
  });

  it("keys the file binding and the service identity by the same path identity", () => {
    // The service identity folds case on a case-folding filesystem. The per-file
    // binding map must fold identically, or a follow-up query that spells the
    // path with different case misses its binding and fails closed against a
    // project that is open and serving.
    const parent = makeTempRoot("ext-ts-case-");
    const projectRoot = makeProject(join(parent, "app"), true);
    const filePath = writeEntry(projectRoot);

    const registry = new ExtensionTsServiceRegistry({});
    const handler = createTsQueryHandler(registry);
    handler({
      command: "open",
      arguments: {
        file: filePath,
        fileContent: SOURCE,
        scriptKindName: "TS",
        projectRootPath: projectRoot,
      },
    });

    // The volume's own policy — the same probe the registry keys by.
    const foldsCase = fsFoldsCaseAt(filePath);
    const restated = filePath.replace(/entry\.ts$/, "ENTRY.ts");
    if (foldsCase) {
      const info = handler({
        command: "quickinfo",
        arguments: { file: restated, line: 1, offset: 14 },
      }) as QuickInfo;
      expect(info.displayString).toContain("answer: number");
    } else {
      // On a case-sensitive filesystem those are genuinely different files, and
      // an unbound one must still fail closed rather than borrow the binding.
      expect(() =>
        handler({ command: "quickinfo", arguments: { file: restated, line: 1, offset: 14 } }),
      ).toThrow(/could not determine which project owns/);
    }
    // Either way, exactly one project service exists.
    expect(registry.projectRoots).toHaveLength(1);
  });

  it("fails closed rather than guess a project for an unowned file", () => {
    const registry = new ExtensionTsServiceRegistry({});
    const handler = createTsQueryHandler(registry);

    let thrown: unknown;
    try {
      handler({
        command: "quickinfo",
        arguments: { file: "/nowhere/entry.ts", line: 1, offset: 1 },
      });
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Error);
    expect((thrown as Error).message).toMatch(/could not determine which project owns/);
    expect((thrown as Error).message).toContain("/nowhere/entry.ts");
    expect(registry.projectRoots).toEqual([]);
  });
});
