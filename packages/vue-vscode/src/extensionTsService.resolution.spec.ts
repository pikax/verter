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

import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { ExtensionTsService } from "./extensionTsService.js";
import {
  materializeLibLessWorkspaceTypeScript,
  materializeLibShapedNonFileWorkspaceTypeScript,
  materializeNativePreviewWorkspaceTypeScript,
  materializeWorkspaceTypeScript,
} from "./extensionTsService.testUtils.js";

const tmps: string[] = [];
afterEach(() => {
  for (const d of tmps.splice(0)) rmSync(d, { recursive: true, force: true });
});

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

// The setting's own copy is the only description most users ever read. It must
// not promise less than the service enforces: a refusal class the UI does not
// mention reads to the user as a bug in Verter.
describe("verter.typeProvider — the `extension` option's user-facing copy", () => {
  it("names every refusal class the service implements", () => {
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
    const description = setting.enumDescriptions[setting.enum.indexOf("extension")]!;

    expect(description).toMatch(/no bundled fallback/i);
    // (1) nothing resolves, (2) resolves but library-less, (3) an engine this
    // in-process service cannot drive.
    expect(description).toMatch(/no resolvable TypeScript/i);
    expect(description).toMatch(/lib\.\*\.d\.ts/);
    expect(description).toMatch(/native \(7\.x\/tsgo\)/i);
    // And that the refusal is PER PROJECT, not per window.
    expect(description).toMatch(/sibling projects keep working/i);
  });
});
