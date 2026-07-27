// Headless regression guard for the EXTENSION type provider's TypeScript
// resolution contract.
//
// The extension provider must serve from the WORKSPACE's own TypeScript — the
// version the project actually uses, with the lib files that install carries.
// There is deliberately NO bundled fallback: a TypeScript bundled into the VSIX
// resolves its default libs next to the packed extension bundle, where no
// `lib.*.d.ts` ships, so it would answer from a lib-less language service —
// silently wrong diagnostics. When no workspace TypeScript resolves, the service
// therefore FAILS CLOSED: every query throws one cached, actionable error and the
// `onUnavailable` notifier fires exactly once (the extension surfaces it as a VS
// Code error notification), instead of degrading to wrong answers.
//
// These tests are discriminating: against the pre-change service (which silently
// fell back to a bundled `require("typescript")`) the fail-closed test gets real
// answers instead of the thrown error, and the bundle-composition guard finds the
// TypeScript compiler among the bundled inputs.

import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import Module, { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService, workspaceNodeModulesChain } from "./extensionTsService.js";
import {
  materializeLibLessWorkspaceTypeScript,
  materializeLibShapedNonFileWorkspaceTypeScript,
  materializeNativePreviewWorkspaceTypeScript,
  materializeWorkspaceTypeScript,
  realTypeScriptPackageDir,
} from "./extensionTsService.testUtils.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

/**
 * Recompute Node's GLOBAL_FOLDERS from the CURRENT `NODE_PATH`.
 *
 * Node reads `NODE_PATH` once, at process start, so setting the variable from
 * inside a test changes nothing on its own. `_initPaths` is the internal that
 * performs that read; calling it is the only way to make an ambient install
 * reachable — and unreachable again — while the process is running.
 */
function reinitializeNodeGlobalFolders(): void {
  (Module as unknown as { _initPaths(): void })._initPaths();
}

/**
 * Node's OWN `node_modules` lookup list for `from` — the directories a bare
 * `require.resolve` consults before it falls through to the global folders.
 *
 * The chain under test claims to be exactly this list, so it is pinned against
 * the real implementation rather than a hand-written expectation: a
 * hand-written list only ever proves the author and the code agree, while this
 * fails the moment the two walks disagree — in either direction.
 */
function nodeModulePathsOf(from: string): string[] {
  return (Module as unknown as { _nodeModulePaths(from: string): string[] })._nodeModulePaths(from);
}

function makeWorkspace(name: string): { root: string } {
  const root = mkdtempSync(join(tmpdir(), name));
  tmps.push(root);
  writeFileSync(join(root, "package.json"), JSON.stringify({ name: "fixture", private: true }));
  writeFileSync(
    join(root, "tsconfig.json"),
    JSON.stringify({
      compilerOptions: { module: "esnext", target: "esnext", moduleResolution: "bundler" },
      include: ["*.ts"],
    }),
  );
  return { root };
}

describe("ExtensionTsService — workspace TypeScript resolution", () => {
  it("initialises and answers when the workspace TypeScript resolves", () => {
    const { root } = makeWorkspace("ext-ts-present-");
    materializeWorkspaceTypeScript(root);

    const source = "export const answer: number = 42;\n";
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, source);

    const unavailable: string[] = [];
    const svc = new ExtensionTsService(root, (message) => unavailable.push(message));
    svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });

    // `answer` starts at 1-based offset 14 on line 1.
    const info = svc.handleQuery("quickinfo", { file: filePath, line: 1, offset: 14 }) as {
      kind: string;
      displayString: string;
    };
    expect(info.kind).toBe("const");
    expect(info.displayString).toContain("answer: number");
    // The workspace path resolved — the unavailability notifier must never fire.
    expect(unavailable).toEqual([]);
  });

  it("fails closed with one actionable notification when no workspace TypeScript resolves", () => {
    const { root } = makeWorkspace("ext-ts-absent-");
    // NO materializeWorkspaceTypeScript: the fixture has no resolvable TypeScript,
    // like a user workspace without a typescript install.

    const source = 'export const broken: number = "not a number";\n';
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, source);

    const unavailable: string[] = [];
    const svc = new ExtensionTsService(root, (message) => unavailable.push(message));

    let first: unknown;
    try {
      svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
    } catch (error) {
      first = error;
    }
    expect(first, "a workspace without TypeScript must fail closed, not answer").toBeInstanceOf(
      Error,
    );
    const message = (first as Error).message;
    // The failure must name the remediation, not just the symptom.
    expect(message).toMatch(/could not resolve a workspace TypeScript/);
    expect(message).toMatch(/npm install -D typescript/);
    expect(message).toMatch(/verter\.typeProvider/);

    // The user-facing surface fires exactly once with that same actionable message.
    expect(unavailable).toEqual([message]);

    // Fail-closed is sticky: a later diagnostics query throws the SAME cached
    // error — the service never silently produces diagnostics without a
    // workspace TypeScript — and the notifier does not fire again.
    let second: unknown;
    try {
      svc.handleQuery("semanticDiagnosticsSync", { file: filePath });
    } catch (error) {
      second = error;
    }
    expect(second).toBe(first);
    expect(unavailable).toEqual([message]);
  });

  // The message above is a SUMMARY. Behind it there must still be the underlying
  // detail — which module was not found, and where it was looked for — because
  // "TypeScript is not installed here" and "TypeScript is installed somewhere
  // this provider does not look" produce the SAME summary and need different
  // fixes; the searched list is what separates them.
  //
  // Resolving through Node used to supply that detail for free: the raised
  // `MODULE_NOT_FOUND` was attached as the thrown error's `cause`. Walking the
  // chain ourselves means Node never raises it, so it has to be carried
  // deliberately — otherwise the failure users hit MOST often is the one that
  // reports least, while the rarer load failure right below keeps its cause.
  it("carries the module-not-found detail, and the directories it searched, as the cause", () => {
    const { root } = makeWorkspace("ext-ts-cause-");
    // NO materializeWorkspaceTypeScript: nothing is installed anywhere in the
    // fixture's chain, which is the commonest real failure.

    const svc = new ExtensionTsService(root, () => {});

    let thrown: unknown;
    try {
      svc.handleQuery("open", {
        file: join(root, "entry.ts"),
        fileContent: "",
        scriptKindName: "TS",
      });
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Error);

    const cause = (thrown as { cause?: unknown }).cause;
    expect(
      cause,
      "the commonest failure must not be the one that discards its diagnostic",
    ).toBeInstanceOf(Error);
    // The machine-readable half the Node error carried.
    expect((cause as { code?: unknown }).code).toBe("MODULE_NOT_FOUND");
    // The human-readable half: WHICH module.
    expect((cause as Error).message).toContain("typescript");

    // …and WHERE it was looked for. Every directory the resolver actually
    // consulted is named — not just the first, or the user cannot tell an
    // uninstalled project from one whose install sits above the search.
    const causeMessage = (cause as Error).message;
    for (const searched of workspaceNodeModulesChain(root)) {
      expect(causeMessage, `the cause must name ${searched}`).toContain(searched);
    }
    // Discriminates a first-entry-only report from a whole-chain one: an
    // ancestor entry is present, and it is not the workspace's own.
    expect(causeMessage).toContain(join(dirname(root), "node_modules"));
  });

  // Node does not end a bare `require.resolve` at the project: its last step is
  // the GLOBAL FOLDERS — every `NODE_PATH` entry, `$HOME/.node_modules`,
  // `$HOME/.node_libraries`, `$PREFIX/lib/node`. A TypeScript found there is not
  // the project's — another version, another set of `lib.*.d.ts`, installed by
  // something else entirely — so serving from it is the same silently-wrong
  // diagnostics the bundled compiler was refused for, delivered to a user whose
  // project installs no TypeScript at all. The provider therefore walks the
  // workspace's OWN `node_modules` chain and stops there.
  //
  // The ambient reachability is MADE here, not assumed: the fixture points
  // `NODE_PATH` at a real TypeScript and asks Node to re-read its global
  // folders, then restores both. And the premise is ASSERTED before the refusal
  // is checked — an ambient setup that silently failed to apply would otherwise
  // leave this test passing while proving nothing.
  it("refuses a TypeScript reachable only through Node's global folders", () => {
    const { root } = makeWorkspace("ext-ts-ambient-");
    // NO materializeWorkspaceTypeScript: nothing is installed in the workspace.
    const ambient = mkdtempSync(join(tmpdir(), "ext-ts-ambient-global-"));
    tmps.push(ambient);
    symlinkSync(realTypeScriptPackageDir(), join(ambient, "typescript"), "junction");

    const previousNodePath = process.env.NODE_PATH;
    process.env.NODE_PATH = ambient;
    reinitializeNodeGlobalFolders();
    try {
      // The premise: a bare resolve from the fixture DOES find the ambient
      // TypeScript. Whatever the service does below, it is deciding against a
      // reachable compiler, not against an empty machine.
      expect(() => createRequire(join(root, "package.json")).resolve("typescript")).not.toThrow();

      const source = "export const answer: number = 42;\n";
      const filePath = join(root, "entry.ts");
      writeFileSync(filePath, source);

      const unavailable: string[] = [];
      const svc = new ExtensionTsService(root, (message) => unavailable.push(message));

      let thrown: unknown;
      try {
        svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
      } catch (error) {
        thrown = error;
      }
      expect(
        thrown,
        "an ambient TypeScript the project never installed must not be served",
      ).toBeInstanceOf(Error);
      const message = (thrown as Error).message;
      expect(message).toMatch(/could not resolve a workspace TypeScript/);
      expect(message).toContain(root);
      expect(unavailable).toEqual([message]);
    } finally {
      if (previousNodePath === undefined) delete process.env.NODE_PATH;
      else process.env.NODE_PATH = previousNodePath;
      reinitializeNodeGlobalFolders();
    }
  });

  // A workspace TypeScript that RESOLVES but carries no default libraries is the
  // same defect the bundled fallback had: the language service type-checks
  // against no lib, so `string`, `Promise` and every DOM global report as
  // errors. The provider must refuse it, exactly as the LSP's own tsserver
  // discovery refuses a library-less candidate — not serve it silently.
  it("fails closed when the workspace TypeScript resolves but carries no default libs", () => {
    const { root } = makeWorkspace("ext-ts-libless-");
    const libDir = materializeLibLessWorkspaceTypeScript(root);

    const source = "export const answer: number = 42;\n";
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, source);

    const unavailable: string[] = [];
    const svc = new ExtensionTsService(root, (message) => unavailable.push(message));

    let first: unknown;
    try {
      svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
    } catch (error) {
      first = error;
    }
    expect(first, "a library-less workspace TypeScript must be refused, not served").toBeInstanceOf(
      Error,
    );
    const message = (first as Error).message;
    expect(message).toMatch(/no lib\.\*\.d\.ts default libraries/);
    // The message must name the offending install so the user can fix it.
    expect(message).toContain(libDir);
    expect(message).toMatch(/npm install -D typescript/);
    expect(unavailable).toEqual([message]);

    // Sticky and non-answering, like the unresolvable case.
    let second: unknown;
    try {
      svc.handleQuery("quickinfo", { file: filePath, line: 1, offset: 14 });
    } catch (error) {
      second = error;
    }
    expect(second).toBe(first);
    expect(unavailable).toEqual([message]);
  });

  // A default-library check that only looks at NAMES admits an install whose lib
  // directory contains a DIRECTORY called `lib.es2025.d.ts`, or a symlink
  // dangling at `lib.dom.d.ts`. Neither is a library; a service built on them
  // type-checks against nothing, which is the exact defect the check exists to
  // stop. The LSP-side rule (`validate_tsserver_candidate`) requires a regular
  // file, and so must this one.
  it("does not count a directory or a dangling symlink as a default library", () => {
    const { root } = makeWorkspace("ext-ts-libshaped-");
    const libDir = materializeLibShapedNonFileWorkspaceTypeScript(root);

    const source = "export const answer: number = 42;\n";
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, source);

    const unavailable: string[] = [];
    const svc = new ExtensionTsService(root, (message) => unavailable.push(message));

    let thrown: unknown;
    try {
      svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
    } catch (error) {
      thrown = error;
    }
    expect(
      thrown,
      "lib-NAMED entries that are not regular files must not satisfy the default-lib check",
    ).toBeInstanceOf(Error);
    const message = (thrown as Error).message;
    expect(message).toMatch(/no lib\.\*\.d\.ts default libraries/);
    expect(message).toContain(libDir);
    expect(unavailable).toEqual([message]);
  });

  // The native-preview (7.x / tsgo) layout is a COMPLETE install whose entry is
  // a launcher with no in-process language service and whose libraries live in a
  // separate platform package. It must be refused — this service cannot drive it
  // — but refused for the RIGHT reason: telling the user to reinstall TypeScript
  // because it "carries no lib.*.d.ts" is a wrong diagnosis of a healthy install.
  it("refuses a native-preview TypeScript by engine, not by blaming missing libraries", () => {
    const { root } = makeWorkspace("ext-ts-native-");
    materializeNativePreviewWorkspaceTypeScript(root);

    const source = "export const answer: number = 42;\n";
    const filePath = join(root, "entry.ts");
    writeFileSync(filePath, source);

    const unavailable: string[] = [];
    const svc = new ExtensionTsService(root, (message) => unavailable.push(message));

    let thrown: unknown;
    try {
      svc.handleQuery("open", { file: filePath, fileContent: source, scriptKindName: "TS" });
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Error);
    const message = (thrown as Error).message;
    expect(message).toMatch(/no in-process language service/);
    expect(message).toMatch(/TSGO/);
    // NOT the library-less diagnosis, and NOT a reinstall instruction.
    expect(message).not.toMatch(/lib\.\*\.d\.ts/);
    expect(message).not.toMatch(/npm install -D typescript/);
    expect(unavailable).toEqual([message]);
  });
});

// The chain the provider searches is Node's own `node_modules` lookup list with
// its FINAL step — the global folders — removed, and nothing else changed. That
// is the whole claim: the provider narrows Node's search, it does not invent a
// different one. Pinning it against `Module._nodeModulePaths` is what keeps the
// claim honest; a hand-written expected list would only prove the author and the
// code agree with each other.
describe("workspaceNodeModulesChain — Node's lookup list minus the global folders", () => {
  it("matches Node's own list exactly for an ordinary project root", () => {
    const { root } = makeWorkspace("ext-ts-chain-");
    expect(workspaceNodeModulesChain(root)).toEqual(nodeModulePathsOf(root));
  });

  // A project that lives INSIDE a dependency tree. Node does not probe a
  // `node_modules/node_modules`: it skips that directory's own iteration
  // entirely, because the PARENT's iteration already contributes the identical
  // path. A walk that instead makes the directory contribute itself emits that
  // path twice — same resolution, but no longer Node's list.
  it("matches Node's own list for a root that is itself a node_modules directory", () => {
    const { root } = makeWorkspace("ext-ts-chain-nested-");
    const inside = join(root, "node_modules");
    const chain = workspaceNodeModulesChain(inside);
    expect(chain).toEqual(nodeModulePathsOf(inside));
    // The discriminating half, stated directly: no entry repeats.
    expect(chain).toEqual([...new Set(chain)]);
  });

  it("matches Node's own list for a package nested under a dependency tree", () => {
    const { root } = makeWorkspace("ext-ts-chain-pkg-");
    const inside = join(root, "node_modules", "some-pkg");
    expect(workspaceNodeModulesChain(inside)).toEqual(nodeModulePathsOf(inside));
  });
});

// The setting's own copy is the only description most users ever read. It must
// not promise less than the service enforces: a refusal class the UI does not
// mention reads to the user as a bug in Verter.
describe("verter.typeProvider — the `extension` option's user-facing copy", () => {
  /** The `extension` option's description, as VS Code will show it. */
  function extensionOptionDescription(): string {
    const manifest = JSON.parse(
      readFileSync(join(dirname(import.meta.dirname), "package.json"), "utf8"),
    ) as {
      contributes: {
        configuration: {
          properties: Record<string, { enum: string[]; enumDescriptions: string[] }>;
        };
      };
    };
    const setting = manifest.contributes.configuration.properties["verter.typeProvider"]!;
    return setting.enumDescriptions[setting.enum.indexOf("extension")]!;
  }

  it("names every refusal class the service implements", () => {
    const description = extensionOptionDescription();

    expect(description).toMatch(/no bundled fallback/i);
    // (1) nothing resolves, (2) resolves but library-less, (3) an engine this
    // in-process service cannot drive.
    expect(description).toMatch(/no resolvable TypeScript/i);
    expect(description).toMatch(/lib\.\*\.d\.ts/);
    expect(description).toMatch(/native \(7\.x\/tsgo\)/i);
    // And that the refusal is PER PROJECT, not per window.
    expect(description).toMatch(/sibling projects keep working/i);
  });

  // The fourth refusal, and the one a reader is most likely to guess wrong:
  // this mode has no global tier, so a TypeScript reachable only through
  // `NODE_PATH` or a legacy global folder is refused even though `tsserver` —
  // sitting right beside it in the same picker — would use it. The docs table
  // says so; the setting the user actually reads must not be the surface that
  // stays silent, or the two disagree and the refusal reads as a Verter bug.
  it("says it has no global tier, matching the docs row", () => {
    const description = extensionOptionDescription();

    expect(description).toMatch(/no global tier/i);
    expect(description).toContain("NODE_PATH");
    // The claim is scoped to THIS provider, so it names the one it differs from.
    expect(description).toMatch(/tsserver/);
  });
});
